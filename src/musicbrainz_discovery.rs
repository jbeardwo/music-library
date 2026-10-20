//! MusicBrainz topology discovery. Shared correspondence remains the semantic authority.
use crate::{
    album_candidates::{self, Reason as R, TitleComparison as T},
    album_matching::{MatchInput, MatchOutcome as M},
    album_program::Programs,
    catalog::*,
    domain::*,
    edition::LocalTrackEvidence,
};
use serde::{Deserialize, Serialize};
/// One title query, three ten-Group catalog pages, at most six Group ensembles.
/// Each ensemble uses up to two bounded browse requests (official/fallback)
/// and up to three Release details. Transport's existing retries stay separate.
pub const MAX_PAGES: usize = 3;
pub const MAX_GROUP_PROGRAMS: usize = 6;
pub const MAX_PROGRAM_REQUESTS: usize = 5;
pub const MAX_REQUESTS: usize = 1 + MAX_PAGES + MAX_GROUP_PROGRAMS * MAX_PROGRAM_REQUESTS;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryVariantKind {
    PunctuationLayout,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryVariant {
    pub kind: QueryVariantKind,
    pub title: String,
}
/// At most one punctuation-layout retrieval variant; text/scripts are preserved.
pub fn title_query_variants(title: &str) -> Vec<QueryVariant> {
    if !title.contains('/') {
        return vec![];
    }
    let spaced = title
        .split('/')
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" / ");
    if spaced == title {
        vec![]
    } else {
        vec![QueryVariant {
            kind: QueryVariantKind::PunctuationLayout,
            title: spaced,
        }]
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PageObservation {
    pub offset: u32,
    pub page: Page<AlbumCandidate>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub title_variants: Vec<QueryVariant>,
    #[serde(default)]
    pub local_fingerprint: String,
    #[serde(default)]
    pub observed_at: i64,
    pub artist: Option<ExternalIdentity>,
    pub title: String,
    pub title_page: Option<Page<AlbumCandidate>>,
    pub pages: Vec<PageObservation>,
    pub programs: Vec<Programs>,
    pub evaluations: Vec<Evaluation>,
    pub budget_exhausted: bool,
    pub provider_error: Option<String>,
    pub requests_upper_bound: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Evaluation {
    pub group: ExternalIdentity,
    pub reasons: Vec<String>,
    pub mappings: Vec<usize>,
    pub strong: bool,
    #[serde(default)]
    pub plausible: bool,
}
pub fn evidence_fingerprint(local: &[LocalTrackEvidence]) -> String {
    let values = local
        .iter()
        .map(|t| (&t.track_id, &t.recording_id, &t.evidence))
        .collect::<Vec<_>>();
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        &serde_json::to_vec(&values).expect("typed program evidence serializes"),
    )
    .to_string()
}
fn scoped(c: &AlbumCandidate, a: &ExternalIdentity) -> bool {
    c.credits.first().and_then(|c| c.identity.as_ref()) == Some(a)
}
pub fn artist_candidate(c: &AlbumCandidate) -> ArtistAlbumCandidate {
    ArtistAlbumCandidate {
        identity: c.identity.clone(),
        title: c.title.clone(),
        artist: c.artist.clone(),
        artist_ids: c
            .credits
            .iter()
            .filter_map(|c| c.identity.clone())
            .collect(),
        date: c.date.clone(),
        primary_type: c.primary_type.clone(),
        comment: c.comment.clone(),
    }
}
/// Parenthetical provider extension is a retrieval hint only. It never makes
/// title_evidence equivalent and cannot bypass program or version gates.
pub fn extended_title(local: &str, provider: &str) -> bool {
    let local = crate::matching::normalize_album_title(local);
    let provider = crate::matching::normalize_album_title(provider);
    provider
        .strip_prefix(&local)
        .is_some_and(|s| s.starts_with(" (") && s.ends_with(')'))
}
fn rank(
    c: &AlbumCandidate,
    input: &MatchInput,
    a: &ExternalIdentity,
) -> (bool, bool, bool, bool, bool) {
    let t = album_candidates::title_evidence(&input.raw_title, &artist_candidate(c));
    let related = !matches!(
        t.comparison,
        T::Conflict | T::VersionConflict | T::TypeConflict
    ) || album_candidates::playable_edition_pair(&input.raw_title, &c.title)
        || extended_title(&input.raw_title, &c.title);
    let year = input
        .date
        .zip(crate::catalog_date::Date::parse(&c.date))
        .is_some_and(|(a, b)| a.year.abs_diff(b.year) <= 1);
    (
        scoped(c, a),
        related,
        year,
        !c.secondary_types
            .iter()
            .any(|t| matches!(t.as_str(), "Compilation" | "Live" | "Remix" | "Soundtrack")),
        !input.evidence.release_types.iter().any(|o| {
            let known = crate::matching::normalize(&o.value);
            let candidate = crate::matching::normalize(&c.primary_type);
            matches!(known.as_str(), "album" | "ep")
                && matches!(candidate.as_str(), "album" | "ep")
                && known != candidate
        }),
    )
}
/// Strong Album coverage is separate from automatic Recording certainty. Incomplete
/// alternative samples neither establish identity nor poison a complete sample.
pub fn evaluate(
    input: &MatchInput,
    c: &AlbumCandidate,
    a: &ExternalIdentity,
    local: &[LocalTrackEvidence],
    p: &Programs,
) -> Evaluation {
    let mut e = Evaluation {
        group: c.identity.clone(),
        reasons: vec![],
        mappings: vec![],
        strong: false,
        plausible: false,
    };
    if !scoped(c, a) {
        e.reasons.push("artist_identity_conflict".into());
    }
    let title = album_candidates::title_evidence(&input.raw_title, &artist_candidate(c));
    if matches!(title.comparison, T::VersionConflict | T::PlayableEdition)
        || crate::album_matching::qualifiers(&input.raw_title)
            != crate::album_matching::qualifiers(&c.title)
    {
        e.reasons.push("version_conflict".into());
    }
    if input.evidence.release_types.iter().any(|o| {
        let known = crate::matching::normalize(&o.value);
        let candidate = crate::matching::normalize(&c.primary_type);
        matches!(known.as_str(), "album" | "ep")
            && matches!(candidate.as_str(), "album" | "ep")
            && known != candidate
    }) {
        e.reasons.push("type_conflict".into());
    }
    let edition_years = |title: &str| {
        title
            .split(|c: char| !c.is_ascii_digit())
            .filter(|s| s.len() == 4)
            .filter_map(|s| s.parse::<u16>().ok())
            .filter(|n| (1900..=2099).contains(n))
            .collect::<Vec<_>>()
    };
    if !crate::album_matching::qualifiers(&input.raw_title).is_empty() {
        let a = edition_years(&input.raw_title);
        let b = edition_years(&c.title);
        if !a.is_empty() && !b.is_empty() && a != b {
            e.reasons.push("version_conflict".into());
        }
    }
    if title.comparison == T::TypeConflict {
        e.reasons.push("type_conflict".into());
    }
    for t in &input.evidence.titles {
        if let crate::canonical_evidence::Origin::Provider { identity } = &t.origin {
            if identity.provider == "musicbrainz"
                && identity.kind == "release_group"
                && identity != &c.identity
            {
                e.reasons.push("existing_trusted_group_conflict".into());
            }
        }
    }
    let mut support = false;
    let no_contradiction = e.reasons.is_empty();
    for program in &p.programs {
        let correspondence =
            crate::program_correspondence::correspond(local, program, &input.raw_title);
        let count = correspondence
            .program
            .tracks
            .iter()
            .filter(|t| t.decision.is_correspondence())
            .count();
        e.mappings.push(count);
        e.plausible |= no_contradiction
            && program.complete
            && count >= 3
            && count * 5 >= local.len() * 3
            && count * 5 >= program.tracks.len() * 3
            && correspondence.program.anchors >= 3;

        let fit = crate::album_program::program_fit(local, program);
        // Multiple independent anchors, coherent alignment, substantial two-sided
        // coverage, compatible counts, and no durable Recording contradiction.
        support |= program.complete
            && local.len() >= 3
            && count >= 3
            && count * 5 >= local.len() * 4
            && count * 5 >= program.tracks.len() * 4
            && correspondence.program.alignment.strong
            && fit.identity_conflicts == 0
            && fit.duration_mismatches < 2;
    }
    if !support {
        e.reasons.push(
            if p.programs.iter().any(|p| p.complete) {
                "insufficient_program_anchors"
            } else {
                "provider_evidence_incomplete"
            }
            .into(),
        );
    }
    e.strong = e.reasons.is_empty();
    e
}
fn candidates(s: &Snapshot) -> Vec<AlbumCandidate> {
    let mut all = s
        .title_page
        .iter()
        .flat_map(|p| p.items.clone())
        .chain(s.pages.iter().flat_map(|p| p.page.items.clone()))
        .collect::<Vec<_>>();
    all.sort_by(|a, b| a.identity.external_id.cmp(&b.identity.external_id));
    all.dedup_by(|a, b| a.identity == b.identity);
    all
}
/// Entire bounded continuation runs on the worker. Pages are not UI boundaries.
pub fn discover(
    provider: &mut impl CatalogProvider,
    input: &MatchInput,
    artist: &ExternalIdentity,
    local: &[LocalTrackEvidence],
    retained: Option<Snapshot>,
) -> album_candidates::Resolution {
    let mut s = retained
        .filter(|s| s.artist.as_ref() == Some(artist) && s.title == input.raw_title)
        .unwrap_or_default();
    if s.observed_at == 0 {
        s.observed_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
    }
    s.local_fingerprint = evidence_fingerprint(local);
    s.artist = Some(artist.clone());
    s.title = input.raw_title.clone();
    s.title_variants = title_query_variants(&input.title);
    s.provider_error = None;
    s.budget_exhausted = false;
    s.requests_upper_bound = 0;
    if s.title_page.is_none() {
        s.requests_upper_bound += 1;
        match provider.artist_albums(artist, &input.title) {
            Ok(p) => {
                s.title_page = Some(Page {
                    items: p
                        .items
                        .iter()
                        .take(10)
                        .map(|c| {
                            provider.album_candidate_observation(&c.identity).unwrap_or(
                                AlbumCandidate {
                                    identity: c.identity.clone(),
                                    title: c.title.clone(),
                                    artist: c.artist.clone(),
                                    credits: c
                                        .artist_ids
                                        .iter()
                                        .map(|id| Credit {
                                            identity: Some(id.clone()),
                                            name: c.artist.clone(),
                                            join_phrase: String::new(),
                                        })
                                        .collect(),
                                    date: c.date.clone(),
                                    primary_type: c.primary_type.clone(),
                                    secondary_types: vec![],
                                    comment: c.comment.clone(),
                                    score: None,
                                },
                            )
                        })
                        .collect(),
                    next_offset: p.next_offset,
                })
            }
            Err(e) => return finish(provider, input, artist, local, s, Some(e)),
        }
    }
    loop {
        let mut all = candidates(&s);
        all.sort_by_key(|c| std::cmp::Reverse(rank(c, input, artist)));
        // Title evidence first. Catalog candidates with a known different year
        // remain retained; fetch unknown/native titles after all bounded pages.
        let catalog_done = s.pages.last().is_some_and(|p| p.page.next_offset.is_none())
            || s.pages.len() == MAX_PAGES;
        for c in all.iter().filter(|c| scoped(c, artist)) {
            let r = rank(c, input, artist);
            if !r.3 && !r.1 {
                continue;
            }
            if !r.1 && !r.2 && !catalog_done {
                continue;
            }
            if !r.1 && !r.2 && input.date.is_some() && !c.date.is_empty() {
                continue;
            }
            if !s.programs.iter().any(|p| p.album == c.identity) {
                if s.programs.len() >= MAX_GROUP_PROGRAMS {
                    s.budget_exhausted = true;
                    break;
                }
                s.requests_upper_bound += MAX_PROGRAM_REQUESTS;
                match provider.album_programs(&c.identity) {
                    Ok(p) if p.album == c.identity => {
                        if let Some(at) = provider.album_program_observed_at(&c.identity) {
                            s.observed_at = s.observed_at.min(at);
                        }
                        s.programs.push(p)
                    }
                    Ok(_) => {
                        return finish(
                            provider,
                            input,
                            artist,
                            local,
                            s,
                            Some(CatalogError::Other("Program Group mismatch".into())),
                        );
                    }
                    Err(e) => return finish(provider, input, artist, local, s, Some(e)),
                }
            }
        }
        s.evaluations = all
            .iter()
            .filter_map(|c| {
                s.programs
                    .iter()
                    .find(|p| p.album == c.identity)
                    .map(|p| evaluate(input, c, artist, local, p))
            })
            .collect();
        let strong = s.evaluations.iter().filter(|e| e.strong).count();
        // Do not use provider order to dismiss an uninspected plausible peer.
        let uninspected = all.iter().any(|c| {
            let r = rank(c, input, artist);
            r.0 && r.3 && (r.1 || r.2) && !s.programs.iter().any(|p| p.album == c.identity)
        });
        if strong > 0 && !uninspected {
            break;
        }
        if catalog_done || s.budget_exhausted {
            break;
        }
        let offset = s.pages.last().and_then(|p| p.page.next_offset).unwrap_or(0);
        s.requests_upper_bound += 1;
        match provider.artist_release_groups(artist, offset) {
            Ok(mut page) => {
                page.items.truncate(10);
                s.pages.push(PageObservation { offset, page });
            }
            Err(e) => return finish(provider, input, artist, local, s, Some(e)),
        }
    }
    if s.pages.len() == MAX_PAGES
        && s.pages.last().is_some_and(|p| p.page.next_offset.is_some())
        && !s.evaluations.iter().any(|e| e.strong)
    {
        s.budget_exhausted = true;
    }
    finish(provider, input, artist, local, s, None)
}
fn finish(
    provider: &impl CatalogProvider,
    input: &MatchInput,
    artist: &ExternalIdentity,
    local: &[LocalTrackEvidence],
    s: Snapshot,
    error: Option<CatalogError>,
) -> album_candidates::Resolution {
    let mut s = s;
    debug_assert!(s.requests_upper_bound <= MAX_REQUESTS);
    let mut all = candidates(&s);
    s.evaluations = all
        .iter()
        .filter_map(|c| {
            s.programs
                .iter()
                .find(|p| p.album == c.identity)
                .map(|p| evaluate(input, c, artist, local, p))
        })
        .collect();
    all.sort_by_key(|c| {
        std::cmp::Reverse((
            s.evaluations
                .iter()
                .find(|e| e.group == c.identity)
                .is_some_and(|e| e.strong),
            s.evaluations
                .iter()
                .find(|e| e.group == c.identity)
                .and_then(|e| e.mappings.iter().max())
                .copied()
                .unwrap_or(0),
            rank(c, input, artist),
        ))
    });
    let page = Page {
        items: all.iter().map(artist_candidate).collect(),
        next_offset: None,
    };
    let mut report = album_candidates::initial_report(
        provider,
        (&input.raw_title, input.date),
        Some(artist),
        &page,
        local,
    );
    let strong = s
        .evaluations
        .iter()
        .filter(|e| e.strong)
        .collect::<Vec<_>>();
    let outcome = if let Some(e) = error {
        s.provider_error = Some(e.to_string());
        report.reasons = vec![R::IncompleteProgram];
        if e.is_provider_unavailable() {
            M::Deferred(e)
        } else {
            M::Error(e.to_string())
        }
    } else if strong.len() == 1
        && !s.budget_exhausted
        && !s.evaluations.iter().any(|e| e.plausible && !e.strong)
    {
        report.decision = "Accepted".into();
        report.reasons = vec![R::AcceptedProgramCorroboration];
        M::MatchedClose(strong[0].group.clone())
    } else if strong.len() > 1
        || (strong.len() == 1 && s.evaluations.iter().any(|e| e.plausible && !e.strong))
    {
        report.reasons = vec![R::CompetingCandidates];
        M::AlbumAmbiguous(
            all.iter()
                .filter(|c| {
                    s.evaluations
                        .iter()
                        .any(|e| e.plausible && e.group == c.identity)
                })
                .map(artist_candidate)
                .collect(),
        )
    } else {
        report.reasons = vec![if s.budget_exhausted {
            R::CandidateLimit
        } else if all.is_empty() {
            R::NoCandidates
        } else {
            R::InsufficientProgramAnchors
        }];
        M::NoConfidentMatch
    };
    for row in &mut report.candidates {
        row.provider_rank = s
            .title_page
            .as_ref()
            .and_then(|p| {
                p.items
                    .iter()
                    .position(|c| c.identity == row.identity)
                    .map(|i| i + 1)
            })
            .or_else(|| {
                s.pages.iter().find_map(|p| {
                    p.page
                        .items
                        .iter()
                        .position(|c| c.identity == row.identity)
                        .map(|i| p.offset as usize + i + 1)
                })
            });

        row.observation = all.iter().find(|c| c.identity == row.identity).cloned();
        if let Some(p) = s.programs.iter().find(|p| p.album == row.identity) {
            row.programs = p
                .programs
                .iter()
                .map(|p| {
                    crate::program_correspondence::correspond(local, p, &input.raw_title).program
                })
                .collect();
        }
        if let Some(e) = s.evaluations.iter().find(|e| e.group == row.identity) {
            row.warnings.extend(e.reasons.clone());
            row.decision =
                if matches!(&outcome,M::Matched(id)|M::MatchedClose(id) if id==&row.identity) {
                    "Accepted"
                } else {
                    "Withheld"
                }
                .into();

            row.reasons = if e.strong {
                report.reasons.clone()
            } else if e
                .reasons
                .iter()
                .any(|r| r == "existing_trusted_group_conflict")
            {
                vec![R::TrustedIdentityConflict]
            } else if e.reasons.iter().any(|r| r == "version_conflict") {
                vec![R::AlbumVersionMismatch]
            } else if e.reasons.iter().any(|r| r == "type_conflict") {
                vec![R::ReleaseTypeMismatch]
            } else if e
                .reasons
                .iter()
                .any(|r| r == "provider_evidence_incomplete")
            {
                vec![R::IncompleteProgram]
            } else {
                vec![R::InsufficientProgramAnchors]
            };
        }
    }
    report.candidates.sort_by_key(|row| {
        all.iter()
            .position(|c| c.identity == row.identity)
            .unwrap_or(usize::MAX)
    });
    report.musicbrainz_discovery = Some(s);
    album_candidates::Resolution { outcome, report }
}
