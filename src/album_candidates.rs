//! Provider-neutral, bounded representation disambiguation inside a resolved Artist.
use crate::{
    album_matching::{MatchOutcome, accepted_album_confirmed},
    album_program::{self, Programs, TrackOutcome},
    catalog::{ArtistAlbumCandidate, CatalogError, CatalogProvider, Page, Timing},
    domain::ExternalIdentity,
    edition::LocalTrackEvidence,
};
pub const MAX_PROGRAM_CANDIDATES: usize = 3;

/// Lower bound only. Duplicate placements in multiple local editions do not add
/// to the bound. Missing totals/completeness are irrelevant; extra provider songs
/// are never penalized. Per-disc track numbers cannot exceed the entire program.
pub fn required_tracks(local: &[LocalTrackEvidence]) -> usize {
    let keys: std::collections::HashSet<_> = local
        .iter()
        .filter_map(|t| {
            if let Some(n) = t.evidence.number.filter(|n| *n > 0) {
                Some(format!("{}:{n}", t.evidence.disc.unwrap_or(1)))
            } else {
                t.evidence
                    .title
                    .as_ref()
                    .map(|s| album_program::comparison_title(s))
                    .filter(|s| !s.is_empty())
                    .map(|s| format!("title:{s}"))
            }
        })
        .collect();
    keys.len().max(
        local
            .iter()
            .filter_map(|t| t.evidence.number)
            .max()
            .unwrap_or(0) as usize,
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fit {
    Supported,
    Rejected(&'static str),
    Uncertain,
}
pub fn fit(local: &[LocalTrackEvidence], p: &Programs) -> Fit {
    if p.programs.is_empty() || p.programs.iter().any(|p| !p.complete) {
        return Fit::Uncertain;
    }
    let required = required_tracks(local);
    let mut rejection = None;
    let mut uncertain = false;
    for program in &p.programs {
        if program.tracks.len() < required {
            rejection = Some("program cannot contain present positions/Tracks");
            continue;
        }
        let fit = album_program::program_fit(local, program);
        if fit.identity_conflicts > 0 {
            rejection = Some("conflicting trusted Recording identity");
            continue;
        }
        if fit.unmatched >= 2 {
            rejection = Some("at least two present Tracks cannot be explained");
            continue;
        }
        if fit.duration_mismatches >= 2 {
            rejection = Some("multiple incompatible durations");
            continue;
        }
        if !local.is_empty() && fit.unmatched == 0 {
            return Fit::Supported;
        }
        // One weak disagreement must not eliminate the representation.
        uncertain = true;
    }
    if uncertain {
        Fit::Uncertain
    } else {
        Fit::Rejected(rejection.unwrap_or("no complete program"))
    }
}

/// Fetched programs are returned to the worker for reuse. No storage, provider
/// identifiers as primary IDs, exact-edition claims or per-Track HTTP calls.
pub fn resolve(
    provider: &mut impl CatalogProvider,
    title: &str,
    artist: &ExternalIdentity,
    page: &Page<ArtistAlbumCandidate>,
    manual: bool,
    local: &[LocalTrackEvidence],
    cache: &mut Vec<Programs>,
) -> Result<MatchOutcome, CatalogError> {
    let initial = accepted_album_confirmed(title, artist, page, manual);
    if !provider.album_candidate_programs() || local.is_empty() {
        return Ok(initial);
    }
    let candidates = match &initial {
        MatchOutcome::Matched(id) | MatchOutcome::MatchedClose(id) => page
            .items
            .iter()
            .filter(|c| &c.identity == id)
            .cloned()
            .collect::<Vec<_>>(),
        MatchOutcome::AlbumAmbiguous(c) => c.clone(),
        _ => return Ok(initial),
    };
    let required = required_tracks(local);
    let surviving:Vec<_>=candidates.into_iter().filter(|c| {
        let count=provider.album_candidate_track_count(&c.identity);
        let possible=count.is_none_or(|n|n as usize>=required);
        Timing::event(format_args!("album candidate={} total={count:?} local_required={required} survives_cheap={possible}",c.identity.external_id));
        possible
    }).collect();
    let filtered = Page {
        items: surviving.clone(),
        next_offset: page.next_offset,
    };
    let outcome = accepted_album_confirmed(title, artist, &filtered, manual);
    if !matches!(outcome, MatchOutcome::AlbumAmbiguous(_))
        || page.next_offset.is_some()
        || surviving.len() > MAX_PROGRAM_CANDIDATES
    {
        return Ok(outcome);
    }
    let mut examined = vec![];
    for candidate in &surviving {
        let programs = if let Some(p) = cache.iter().find(|p| p.album == candidate.identity) {
            p.clone()
        } else {
            let p = provider.album_programs(&candidate.identity)?;
            if p.album != candidate.identity {
                return Err(CatalogError::Other(
                    "Candidate programs belong to another Album".into(),
                ));
            }
            if cache.len() == 4 {
                cache.remove(0);
            }
            cache.push(p.clone());
            p
        };
        let assessment = fit(local, &programs);
        Timing::event(format_args!(
            "album candidate={} program_fit={assessment:?}",
            candidate.identity.external_id
        ));
        if !matches!(assessment, Fit::Rejected(_)) {
            examined.push((candidate.clone(), programs, assessment));
        }
    }
    if let [(candidate, programs, Fit::Supported)] = examined.as_slice() {
        // At least three corroborating distinct positions for a program-based
        // choice. A one-Track subset cannot settle catalog representation identity.
        if programs
            .programs
            .iter()
            .any(|p| album_program::program_fit(local, p).exact_positions >= 3)
        {
            return Ok(accepted_album_confirmed(
                title,
                artist,
                &Page {
                    items: vec![candidate.clone()],
                    next_offset: None,
                },
                manual,
            ));
        }
    }
    if examined.len() > 1 && examined.iter().all(|(_, _, f)| *f == Fit::Supported) {
        let mappings: Vec<_> = examined
            .iter()
            .map(|(_, p, _)| album_program::compare_album(local, p))
            .collect();
        let matched_title = |o: &TrackOutcome| match o {
            TrackOutcome::Matched(m) | TrackOutcome::AlreadyMatched(m) => {
                Some(album_program::comparison_title(&m.title))
            }
            _ => None,
        };
        if (0..local.len()).all(|i| {
            matched_title(&mappings[0][i]).is_some()
                && mappings
                    .iter()
                    .all(|m| matched_title(&m[i]) == matched_title(&mappings[0][i]))
        }) {
            let combined=Programs{album:examined[0].1.album.clone(),programs:examined.iter().flat_map(|(_,p,_)|p.programs.clone()).collect(),note:"Equivalent provider representations; no Album ID selected; associations are diagnostic until identity resolves".into()};
            let tracks = local
                .iter()
                .cloned()
                .zip(album_program::compare_album(local, &combined))
                .collect();
            return Ok(MatchOutcome::AlbumEquivalent {
                candidates: examined.into_iter().map(|(c, _, _)| c).collect(),
                tracks,
            });
        }
    }
    Ok(if examined.is_empty() {
        MatchOutcome::NoConfidentMatch
    } else {
        MatchOutcome::AlbumAmbiguous(examined.into_iter().map(|(c, _, _)| c).collect())
    })
}
