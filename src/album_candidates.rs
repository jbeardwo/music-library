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

/// Shared lower-bound test: partial imports never imply an exact total.
pub fn can_accommodate(required: usize, total: Option<u32>) -> bool {
    total.is_none_or(|n| n as usize >= required)
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
    context: (&str, Option<crate::catalog_date::Date>),
    artist: &ExternalIdentity,
    page: &Page<ArtistAlbumCandidate>,
    manual: bool,
    local: &[LocalTrackEvidence],
    cache: &mut Vec<Programs>,
) -> Result<MatchOutcome, CatalogError> {
    let (title, date) = context;
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
        let possible=can_accommodate(required,count);
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
    // Dates compare only fully supported human programs. Unknown structural
    // competitors cannot be dismissed by date; no nearest-year tie breaker.
    if examined.len() > 1 && examined.iter().all(|(_, _, f)| *f == Fit::Supported) {
        let agreements: Vec<_> = examined
            .iter()
            .map(|(c, _, _)| {
                crate::catalog_date::agreement(date, crate::catalog_date::Date::parse(&c.date))
            })
            .collect();
        let best = agreements.iter().max().copied().unwrap();
        if best > crate::catalog_date::Agreement::UnknownOrDifferent
            && agreements.iter().filter(|a| **a == best).count() == 1
        {
            let index = agreements.iter().position(|a| *a == best).unwrap();
            let preferred = examined.remove(index);
            if preferred
                .1
                .programs
                .iter()
                .any(|p| album_program::program_fit(local, p).exact_positions >= 3)
            {
                return Ok(accepted_album_confirmed(
                    title,
                    artist,
                    &Page {
                        items: vec![preferred.0],
                        next_offset: None,
                    },
                    manual,
                ));
            }
            examined.insert(index, preferred);
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
    if !examined.is_empty() {
        // Compare each representation separately. Cross-program template ranking
        // must not discard a plausible Album merely to manufacture ID agreement.
        let mappings: Vec<_> = examined
            .iter()
            .map(|(_, p, _)| {
                local
                    .iter()
                    .map(|t| album_program::compare(t, p))
                    .collect::<Vec<_>>()
            })
            .collect();
        let tracks: Vec<_> = local.iter().enumerate().map(|(i,t)| {
            let mut outcome = mappings[0][i].clone();
            if let TrackOutcome::Matched(m) | TrackOutcome::AlreadyMatched(m) = &mut outcome {
                m.occurrences.retain(|id| mappings.iter().all(|rows| {
                    matches!(&rows[i], TrackOutcome::Matched(other) | TrackOutcome::AlreadyMatched(other) if other.occurrences.contains(id))
                }));
                if !mappings.iter().all(|rows|matches!(&rows[i],TrackOutcome::Matched(other)|TrackOutcome::AlreadyMatched(other) if other.duration==m.duration)) {m.duration=None;}
                // No Recording claim is made while provider objects are unresolved.
                m.recording.identities.clear();
                if m.recording_status == album_program::RecordingStatus::Identified {
                    m.recording_status = album_program::RecordingStatus::Ambiguous;
                }
                m.explanation = format!("Agreement across {} plausible provider Album objects; no Album ID selected. {}", examined.len(), m.explanation);
            }
            (t.clone(), outcome)
        }).collect();
        let all_supported = examined.len() > 1
            && examined.iter().all(|(_, _, fit)| *fit == Fit::Supported)
            && mappings.iter().all(|rows| {
                rows.iter().all(|o| {
                    matches!(
                        o,
                        TrackOutcome::Matched(_) | TrackOutcome::AlreadyMatched(_)
                    )
                })
            });
        let any_agreed = tracks.iter().any(|(_,o)| matches!(o, TrackOutcome::Matched(m) | TrackOutcome::AlreadyMatched(m) if !m.occurrences.is_empty()));
        if all_supported || any_agreed {
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
