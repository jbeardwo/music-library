//! Evidence and decisions for bounded Spotify representation matching.
use super::*;
use crate::{
    album_matching::{edit_close, qualifiers},
    matching::{album_representation_title, normalize},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    NoCandidates,
    ArtistUnresolved,
    ArtistMismatch,
    AlbumTitleMismatch,
    AlbumVersionMismatch,
    ReleaseTypeMismatch,
    TrackCountMismatch,
    PositionMismatch,
    InsufficientProgramAnchors,
    TrustedIdentityConflict,
    IncompleteProgram,
    DurationConflict,
    CompetingCandidates,
    IncompleteCandidatePage,
    CandidateLimit,
    AcceptedExactTitle,
    AcceptedNormalizedTitle,
    AcceptedReleaseTypeSuffix,
    AcceptedMinorTypo,
    EquivalentCandidatesCollapsed,
    EquivalentOccurrences,
    AlreadyAssociated,
    NotEvaluated,
    EvidenceChanged,
    CompletionFailed,
}
impl Reason {
    pub fn label(self) -> &'static str {
        match self {
            Self::NoCandidates => "No candidates returned by the bounded search",
            Self::ArtistUnresolved => "Artist identity is not established",
            Self::ArtistMismatch => "Candidate Artist IDs differ from the established Artist",
            Self::AlbumTitleMismatch => "Album titles do not agree under conservative comparison",
            Self::AlbumVersionMismatch => "Meaningful Album/version qualifier differs",
            Self::ReleaseTypeMismatch => "Release type conflicts with the title's format suffix",
            Self::TrackCountMismatch => {
                "Provider program cannot accommodate the local Tracks/positions"
            }
            Self::PositionMismatch => "Missing, duplicate or shifted disc/Track positions",
            Self::InsufficientProgramAnchors => "Insufficient independent title/position anchors",
            Self::TrustedIdentityConflict => {
                "Existing trusted occurrence/Recording/Artist identity conflicts"
            }
            Self::IncompleteProgram => "Provider program is incomplete; evidence is insufficient",
            Self::DurationConflict => "Multiple incompatible durations",
            Self::CompetingCandidates => "Competing supported provider representations remain",
            Self::IncompleteCandidatePage => {
                "Candidate page has more results; uniqueness is not established"
            }
            Self::CandidateLimit => "Program comparison bound exceeded",
            Self::AcceptedExactTitle => {
                "Established Artist, exact Album title and compatible positioned program"
            }
            Self::AcceptedNormalizedTitle => {
                "Harmless typography normalized; strong positioned program corroborates"
            }
            Self::AcceptedReleaseTypeSuffix => {
                "Compatible release-type suffix normalized; strong positioned program corroborates"
            }
            Self::AcceptedMinorTypo => {
                "One-edit Album typo supported by strong Artist/position/program evidence"
            }
            Self::EquivalentCandidatesCollapsed => {
                "Equivalent provider objects share the complete ordered Track identities; deterministic representation selected"
            }
            Self::EquivalentOccurrences => {
                "Only agreeing occurrences are usable; no preferred Album representation established"
            }
            Self::AlreadyAssociated => "Existing trusted association retained",
            Self::EvidenceChanged => {
                "Persisted metadata or trusted associations changed before completion; evaluation withheld"
            }
            Self::CompletionFailed => {
                "Association could not be persisted; inspect the completion error"
            }
            Self::NotEvaluated => {
                "No evaluation retained in this session; re-evaluate this Album explicitly"
            }
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TitleComparison {
    Exact,
    Typography,
    ReleaseTypeSuffix,
    MinorTypo,
    Conflict,
    VersionConflict,
    TypeConflict,
}
impl TitleComparison {
    pub fn label(self) -> &'static str {
        match self {
            Self::Exact => "exact title",
            Self::Typography => "typography normalized",
            Self::ReleaseTypeSuffix => "compatible release-type suffix normalized",
            Self::MinorTypo => "minor typo; requires strong program evidence",
            Self::Conflict => "title mismatch",
            Self::VersionConflict => "meaningful version qualifier differs",
            Self::TypeConflict => "release type conflicts",
        }
    }
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct TitleEvidence {
    pub local_raw: String,
    pub provider_raw: String,
    pub local_normalized: String,
    pub provider_normalized: String,
    pub comparison: TitleComparison,
    pub local_type_hint: Option<String>,
    pub provider_type: String,
}
fn suffix_base(title: &str) -> (&str, Option<&str>) {
    for suffix in ["ep", "lp"] {
        if let Some(base) = title.strip_suffix(&format!(" {suffix}")) {
            return (base, Some(suffix));
        }
    }
    (title, None)
}
pub fn title_evidence(local: &str, candidate: &ArtistAlbumCandidate) -> TitleEvidence {
    let l = album_representation_title(local);
    let r = album_representation_title(&candidate.title);
    let (lb, lh) = suffix_base(&l);
    let (rb, rh) = suffix_base(&r);
    let kind = normalize(&candidate.primary_type);
    // Spotify's Single bucket is compatible with an EP hint, not proof of EP identity.
    // Program corroboration remains mandatory before this comparison can win.
    let compatible = |hint: &str| match hint {
        "ep" => kind == "ep" || candidate.identity.provider == "spotify" && kind == "single",
        "lp" => kind == "album",
        _ => false,
    };
    let conflicting_type = lh
        .into_iter()
        .chain(rh)
        .any(|hint| !kind.is_empty() && !compatible(hint));
    let version = qualifiers(&l) != qualifiers(&r);
    let can_strip = lh.into_iter().chain(rh).all(compatible) && (lh.is_some() || rh.is_some());
    let (ln, rn) = if can_strip {
        (lb, rb)
    } else {
        (l.as_str(), r.as_str())
    };
    let comparison = if version {
        TitleComparison::VersionConflict
    } else if conflicting_type {
        TitleComparison::TypeConflict
    } else if normalize(local) == normalize(&candidate.title) {
        TitleComparison::Exact
    } else if can_strip && ln == rn {
        TitleComparison::ReleaseTypeSuffix
    } else if l == r {
        TitleComparison::Typography
    } else if edit_close(ln, rn, 1, 5) {
        TitleComparison::MinorTypo
    } else {
        TitleComparison::Conflict
    };
    TitleEvidence {
        local_raw: local.into(),
        provider_raw: candidate.title.clone(),
        local_normalized: ln.into(),
        provider_normalized: rn.into(),
        comparison,
        local_type_hint: lh.map(str::to_owned),
        provider_type: candidate.primary_type.clone(),
    }
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct CandidateEvidence {
    pub identity: ExternalIdentity,
    pub artist: String,
    pub artist_ids: Vec<ExternalIdentity>,
    pub artist_accepted: bool,
    pub title: TitleEvidence,
    pub local_date: Option<String>,
    pub provider_date: String,
    pub date_agreement: String,
    pub local_tracks: usize,
    pub required_tracks: usize,
    pub provider_tracks: Option<u32>,
    pub programs: Vec<album_program::PositionedProgram>,
    pub equivalent_to: Option<ExternalIdentity>,
    pub decision: String,
    pub reasons: Vec<Reason>,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Report {
    pub local_title: String,
    pub established_artist: Option<ExternalIdentity>,
    pub candidate_count: usize,
    pub more_candidates: bool,
    pub candidates: Vec<CandidateEvidence>,
    pub decision: String,
    pub reasons: Vec<Reason>,
}
#[derive(Debug)]
pub struct Resolution {
    pub outcome: MatchOutcome,
    pub report: Report,
}
fn identity_key(id: &ExternalIdentity) -> (&str, &str, &str) {
    (&id.provider, &id.kind, &id.external_id)
}
fn sorted_ids(ids: &[ExternalIdentity]) -> Vec<ExternalIdentity> {
    let mut ids = ids.to_vec();
    ids.sort_by(|a, b| identity_key(a).cmp(&identity_key(b)));
    ids.dedup();
    ids
}
pub fn initial_report(
    provider: &impl CatalogProvider,
    context: (&str, Option<crate::catalog_date::Date>),
    artist: Option<&ExternalIdentity>,
    page: &Page<ArtistAlbumCandidate>,
    local: &[LocalTrackEvidence],
) -> Report {
    let mut candidates = page.items.clone();
    candidates.extend(provider.rejected_album_candidates());
    candidates.sort_by(|a, b| identity_key(&a.identity).cmp(&identity_key(&b.identity)));
    candidates.dedup_by(|a, b| a.identity == b.identity);
    let count = candidates.len();
    Report {
        local_title: context.0.into(),
        established_artist: artist.cloned(),
        candidate_count: count,
        more_candidates: page.next_offset.is_some(),
        decision: "Withheld".into(),
        reasons: vec![],
        candidates: candidates
            .into_iter()
            .take(10)
            .map(|c| {
                let title = title_evidence(context.0, &c);
                let accepted = artist.is_some_and(|a| {
                    c.artist_ids.as_slice() == std::slice::from_ref(a)
                        && a.kind == "artist"
                        && !a.external_id.is_empty()
                        && c.identity.provider == a.provider
                        && c.identity.kind == "album"
                        && !c.identity.external_id.is_empty()
                });
                let mut reasons = vec![];
                if !accepted {
                    reasons.push(if artist.is_some() {
                        Reason::ArtistMismatch
                    } else {
                        Reason::ArtistUnresolved
                    });
                }
                match title.comparison {
                    TitleComparison::Conflict => reasons.push(Reason::AlbumTitleMismatch),
                    TitleComparison::VersionConflict => reasons.push(Reason::AlbumVersionMismatch),
                    TitleComparison::TypeConflict => reasons.push(Reason::ReleaseTypeMismatch),
                    _ => {}
                }
                if !can_accommodate(
                    required_tracks(local),
                    provider.album_candidate_track_count(&c.identity),
                ) {
                    reasons.push(Reason::TrackCountMismatch);
                }
                CandidateEvidence {
                    identity: c.identity.clone(),
                    artist: c.artist,
                    artist_ids: sorted_ids(&c.artist_ids),
                    artist_accepted: accepted,
                    title,
                    local_date: context.1.map(|d| {
                        format!(
                            "{}{}{}",
                            d.year,
                            d.month.map(|m| format!("-{m:02}")).unwrap_or_default(),
                            d.day.map(|n| format!("-{n:02}")).unwrap_or_default()
                        )
                    }),
                    provider_date: c.date.clone(),
                    date_agreement: format!(
                        "{:?}",
                        crate::catalog_date::agreement(
                            context.1,
                            crate::catalog_date::Date::parse(&c.date)
                        )
                    ),
                    local_tracks: local.len(),
                    required_tracks: required_tracks(local),
                    provider_tracks: provider.album_candidate_track_count(&c.identity),
                    programs: vec![],
                    equivalent_to: None,
                    decision: if !reasons.is_empty() {
                        "Rejected"
                    } else {
                        "Withheld"
                    }
                    .into(),
                    reasons,
                }
            })
            .collect(),
    }
}
fn program_reason(p: &album_program::PositionedProgram, required: usize) -> Option<Reason> {
    use album_program::PositionDecision as P;
    if !p.complete {
        return Some(Reason::IncompleteProgram);
    }
    if p.provider_count < required {
        return Some(Reason::TrackCountMismatch);
    }
    if p.tracks
        .iter()
        .any(|t| t.decision == P::TrustedIdentityConflict)
    {
        return Some(Reason::TrustedIdentityConflict);
    }
    if p.duplicate_positions > 0
        || p.tracks
            .iter()
            .any(|t| matches!(t.decision, P::PositionMismatch | P::ShiftedProgram))
    {
        return Some(Reason::PositionMismatch);
    }
    if p.tracks.iter().any(|t| t.decision == P::VersionConflict) {
        return Some(Reason::AlbumVersionMismatch);
    }
    if p.tracks
        .iter()
        .any(|t| t.decision == P::InsufficientAnchors)
    {
        return Some(Reason::InsufficientProgramAnchors);
    }
    None
}
fn same_program(a: &Programs, b: &Programs) -> bool {
    let ([a], [b]) = (a.programs.as_slice(), b.programs.as_slice()) else {
        return false;
    };
    a.complete
        && b.complete
        && a.tracks.len() == b.tracks.len()
        && a.tracks.iter().zip(&b.tracks).all(|(x, y)| {
            let xid: Vec<_> = x
                .identities
                .iter()
                .filter(|i| i.provider == "spotify" && i.kind == "track")
                .collect();
            let yid: Vec<_> = y
                .identities
                .iter()
                .filter(|i| i.provider == "spotify" && i.kind == "track")
                .collect();
            xid.len() == 1
                && xid == yid
                && x.disc == y.disc
                && x.number == y.number
                && x.number.is_some_and(|n| n > 0)
                && album_program::comparison_title(x.title.as_deref().unwrap_or(""))
                    == album_program::comparison_title(y.title.as_deref().unwrap_or(""))
                && sorted_ids(
                    &x.artists
                        .iter()
                        .flat_map(|a| a.identities.clone())
                        .collect::<Vec<_>>(),
                ) == sorted_ids(
                    &y.artists
                        .iter()
                        .flat_map(|a| a.identities.clone())
                        .collect::<Vec<_>>(),
                )
        })
}
fn equivalent(
    a: &ArtistAlbumCandidate,
    ap: &Programs,
    b: &ArtistAlbumCandidate,
    bp: &Programs,
) -> bool {
    let dates = match (
        crate::catalog_date::Date::parse(&a.date),
        crate::catalog_date::Date::parse(&b.date),
    ) {
        (Some(a), Some(b)) => a.year == b.year && a.month.zip(b.month).is_none_or(|(a, b)| a == b),
        (None, None) => a.date == b.date,
        _ => false,
    };
    dates
        && normalize(&a.primary_type) == normalize(&b.primary_type)
        && sorted_ids(&a.artist_ids) == sorted_ids(&b.artist_ids)
        && album_representation_title(&a.title) == album_representation_title(&b.title)
        && same_program(ap, bp)
}
pub(super) fn resolve_spotify(
    provider: &mut impl CatalogProvider,
    context: (&str, Option<crate::catalog_date::Date>),
    artist: &ExternalIdentity,
    page: &Page<ArtistAlbumCandidate>,
    local: &[LocalTrackEvidence],
    cache: &mut Vec<Programs>,
) -> Result<Resolution, CatalogError> {
    let mut report = initial_report(provider, context, Some(artist), page, local);
    let finish = |outcome, mut report: Report, reason| {
        report.reasons = vec![reason];
        report.decision = if matches!(
            outcome,
            MatchOutcome::Matched(_) | MatchOutcome::MatchedClose(_)
        ) {
            "Accepted"
        } else {
            "Withheld"
        }
        .into();
        Resolution { outcome, report }
    };
    if local.is_empty() {
        return Ok(finish(
            MatchOutcome::NoConfidentMatch,
            report,
            Reason::InsufficientProgramAnchors,
        ));
    }
    for row in &mut report.candidates {
        if !can_accommodate(row.required_tracks, row.provider_tracks) {
            if !row.reasons.contains(&Reason::TrackCountMismatch) {
                row.reasons.push(Reason::TrackCountMismatch);
            }
            row.decision = "Rejected".into();
        }
    }
    let viable: Vec<_> = report
        .candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| c.reasons.is_empty())
        .map(|(i, _)| i)
        .collect();
    if page.next_offset.is_some() || viable.len() > MAX_PROGRAM_CANDIDATES {
        return Ok(finish(
            MatchOutcome::AlbumAmbiguous(page.items.clone()),
            report,
            if page.next_offset.is_some() {
                Reason::IncompleteCandidatePage
            } else {
                Reason::CandidateLimit
            },
        ));
    }
    let mut examined = vec![];
    for index in viable {
        let row = &mut report.candidates[index];
        let c = page
            .items
            .iter()
            .find(|c| c.identity == row.identity)
            .unwrap()
            .clone();
        let p = if let Some(p) = cache.iter().find(|p| p.album == c.identity) {
            p.clone()
        } else {
            let p = provider.album_programs(&c.identity)?;
            if p.album != c.identity {
                return Err(CatalogError::Other(
                    "Candidate program belongs to another Album".into(),
                ));
            }
            if cache.len() == 4 {
                cache.remove(0);
            }
            cache.push(p.clone());
            p
        };
        row.programs = p
            .programs
            .iter()
            .map(|p| album_program::inspect_positioned_program(local, p))
            .collect();
        let reasons: Vec<_> = row
            .programs
            .iter()
            .filter_map(|p| program_reason(p, row.required_tracks))
            .collect();
        let compatible = !row.programs.is_empty() && reasons.is_empty();
        let relaxed = row.title.comparison != TitleComparison::Exact;
        let strong = row
            .programs
            .iter()
            .all(|p| p.anchors >= 3 && p.anchors + 1 >= required_tracks(local));
        // Typo/suffix/typography cannot be an identity shortcut. No duration
        // contradictions or comparable competitors may be hidden by a typo.
        let duration_conflict = row.title.comparison == TitleComparison::MinorTypo
            && row.programs.iter().any(|p| {
                p.tracks.iter().any(|t| {
                    t.provider.as_ref().is_some_and(|c| {
                        t.local
                            .duration_ms
                            .zip(c.duration_ms)
                            .is_some_and(|(a, b)| a.abs_diff(b) > 3000)
                    })
                })
            });
        if !compatible {
            row.reasons = if reasons.is_empty() {
                vec![Reason::IncompleteProgram]
            } else {
                reasons
            };
            let uncertain = row.reasons.iter().all(|r| {
                *r == Reason::IncompleteProgram || *r == Reason::InsufficientProgramAnchors
            });
            row.decision = if uncertain { "Withheld" } else { "Rejected" }.into();
            if uncertain {
                examined.push((index, c, p));
            }
        } else if relaxed && !strong {
            row.reasons = vec![Reason::InsufficientProgramAnchors];
            row.decision = "Rejected".into();
        } else if duration_conflict {
            row.reasons = vec![Reason::DurationConflict];
            row.decision = "Rejected".into();
        } else {
            examined.push((index, c, p));
        }
    }
    let mut groups: Vec<Vec<usize>> = vec![];
    for (i, (_, c, p)) in examined.iter().enumerate() {
        if let Some(g) = groups.iter_mut().find(|g| {
            g.iter().all(|member| {
                let (_, other, op) = &examined[*member];
                equivalent(c, p, other, op)
            })
        }) {
            g.push(i)
        } else {
            groups.push(vec![i])
        }
    }
    // Same response order never influences a preferred locator. First compare
    // existing date evidence, then retain a stable provider ID ordering.
    for group in &mut groups {
        group.sort_by(|a, b| {
            let a = &examined[*a].1;
            let b = &examined[*b].1;
            crate::catalog_date::agreement(context.1, crate::catalog_date::Date::parse(&b.date))
                .cmp(&crate::catalog_date::agreement(
                    context.1,
                    crate::catalog_date::Date::parse(&a.date),
                ))
                .then_with(|| identity_key(&a.identity).cmp(&identity_key(&b.identity)))
        });
    }
    let all_supported = examined
        .iter()
        .all(|(i, _, _)| report.candidates[*i].reasons.is_empty());
    let winner = if !all_supported {
        None
    } else if groups.len() == 1 {
        Some(0)
    } else if groups.len() > 1 {
        // Preserve the existing precision-aware date preference, after program
        // support. A typo must not win by dismissing a similarly strong competitor.
        let agreements: Vec<_> = groups
            .iter()
            .map(|g| {
                crate::catalog_date::agreement(
                    context.1,
                    crate::catalog_date::Date::parse(&examined[g[0]].1.date),
                )
            })
            .collect();
        let best = agreements.iter().max().copied().unwrap();
        let typo = examined
            .iter()
            .any(|(i, _, _)| report.candidates[*i].title.comparison == TitleComparison::MinorTypo);
        if !typo
            && best > crate::catalog_date::Agreement::UnknownOrDifferent
            && agreements.iter().filter(|a| **a == best).count() == 1
            && examined.iter().all(|(i, _, _)| {
                report.candidates[*i]
                    .programs
                    .iter()
                    .all(|p| p.anchors >= 3)
            })
        {
            agreements.iter().position(|a| *a == best)
        } else {
            None
        }
    } else {
        None
    };
    if let Some(group_index) = winner {
        let group = &groups[group_index];
        let (index, c, _) = &examined[group[0]];
        let reason = if group.len() > 1 {
            Reason::EquivalentCandidatesCollapsed
        } else {
            match report.candidates[*index].title.comparison {
                TitleComparison::Exact => Reason::AcceptedExactTitle,
                TitleComparison::Typography => Reason::AcceptedNormalizedTitle,
                TitleComparison::ReleaseTypeSuffix => Reason::AcceptedReleaseTypeSuffix,
                _ => Reason::AcceptedMinorTypo,
            }
        };
        for (i, _, _) in &examined {
            report.candidates[*i].decision = "Withheld".into();
            report.candidates[*i].reasons = vec![Reason::CompetingCandidates];
        }
        for member in group {
            let row = &mut report.candidates[examined[*member].0];
            row.reasons = vec![reason];
            row.decision = "Accepted".into();
            if *member != group[0] {
                row.equivalent_to = Some(c.identity.clone());
            }
        }
        let outcome = if report.candidates[*index].title.comparison == TitleComparison::Exact {
            MatchOutcome::Matched(c.identity.clone())
        } else {
            MatchOutcome::MatchedClose(c.identity.clone())
        };
        return Ok(finish(outcome, report, reason));
    }
    if all_supported
        && examined.len() > 1
        && examined
            .iter()
            .all(|(i, _, _)| report.candidates[*i].title.comparison == TitleComparison::Exact)
    {
        let filtered = Page {
            items: examined.iter().map(|(_, c, _)| c.clone()).collect(),
            next_offset: None,
        };
        let outcome =
            super::resolve_legacy(provider, context, artist, &filtered, false, local, cache)?;
        if matches!(outcome, MatchOutcome::AlbumEquivalent { .. }) {
            for (i, _, _) in &examined {
                report.candidates[*i].reasons = vec![Reason::EquivalentOccurrences];
            }
            return Ok(finish(outcome, report, Reason::EquivalentOccurrences));
        }
    }
    let reason = if examined.len() == 1 && !all_supported {
        report.candidates[examined[0].0].reasons[0]
    } else if examined.is_empty() {
        report
            .candidates
            .iter()
            .find(|c| c.artist_accepted)
            .or(report.candidates.first())
            .and_then(|c| c.reasons.first())
            .copied()
            .unwrap_or(Reason::NoCandidates)
    } else {
        Reason::CompetingCandidates
    };
    for (i, _, _) in &examined {
        if report.candidates[*i].reasons.is_empty() {
            report.candidates[*i].reasons = vec![Reason::CompetingCandidates];
        }
    }
    Ok(finish(
        if examined.is_empty() || examined.len() == 1 && !all_supported {
            MatchOutcome::NoConfidentMatch
        } else {
            MatchOutcome::AlbumAmbiguous(examined.into_iter().map(|(_, c, _)| c).collect())
        },
        report,
        reason,
    ))
}

impl Report {
    /// Presentation of the same structured decision used by the matcher, limited
    /// to the selected Track. The full ordered program is never dumped into QML.
    pub fn explain_track(&self, track: &str) -> String {
        let mut text = format!(
            "Local Album: {}\nCandidate page: {} objects{}\nEstablished Artist: {}\nDecision: {}\nReason: {}\n",
            self.local_title,
            self.candidate_count,
            if self.more_candidates {
                " (more results; bounded page)"
            } else {
                ""
            },
            self.established_artist
                .as_ref()
                .map(identity_text)
                .unwrap_or_else(|| "not established".into()),
            self.decision,
            self.reasons
                .iter()
                .map(|r| r.label())
                .collect::<Vec<_>>()
                .join("; ")
        );
        for c in &self.candidates {
            text.push_str(&format!("\nCandidate: {} [{}]\nArtist: {} — {} [{}]\nAlbum title: {:?} → {:?}\nNormalized: {:?} → {:?} ({})\nRelease type: {}; local title hint: {}\nDate: local {}; provider {}; comparison {}\nTrack count: {} local, {} required; provider {}\nDecision: {}\nReason: {}\n",c.title.provider_raw,identity_text(&c.identity),c.artist,if c.artist_accepted{"accepted"}else{"not accepted"},c.artist_ids.iter().map(identity_text).collect::<Vec<_>>().join(", "),c.title.local_raw,c.title.provider_raw,c.title.local_normalized,c.title.provider_normalized,c.title.comparison.label(),if c.title.provider_type.is_empty(){"unknown"}else{&c.title.provider_type},c.title.local_type_hint.as_deref().unwrap_or("none"),c.local_date.as_deref().unwrap_or("unknown"),if c.provider_date.is_empty(){"unknown"}else{&c.provider_date},c.date_agreement,c.local_tracks,c.required_tracks,c.provider_tracks.map(|n|n.to_string()).unwrap_or_else(||"unknown".into()),c.decision,c.reasons.iter().map(|r|r.label()).collect::<Vec<_>>().join("; ")));
            if let Some(id) = &c.equivalent_to {
                text.push_str(&format!(
                    "Equivalent provider representation: {}\n",
                    identity_text(id)
                ));
            }
            if c.programs.is_empty() {
                text.push_str(
                    "Program: not fetched; earlier evidence/bounds withheld evaluation\n",
                );
            }
            for p in &c.programs {
                text.push_str(&explain_positioned_program(p, track));
            }
        }
        text
    }
}
fn identity_text(id: &ExternalIdentity) -> String {
    format!("{}:{}:{}", id.provider, id.kind, id.external_id)
}
fn identities_text(ids: &[ExternalIdentity]) -> String {
    let ids = sorted_ids(ids);
    if ids.is_empty() {
        "none".into()
    } else {
        ids.iter().map(identity_text).collect::<Vec<_>>().join(", ")
    }
}
pub fn explain_positioned_program(p: &album_program::PositionedProgram, track: &str) -> String {
    let mut text = format!(
        "Program: {} / {} local positions corroborated; {} provider Tracks; {}; {} duplicate positions\n",
        p.anchors,
        p.tracks.len(),
        p.provider_count,
        if p.complete { "complete" } else { "incomplete" },
        p.duplicate_positions
    );
    if let Some(t) = p.tracks.iter().find(|t| t.track_id == track) {
        let provider = t.provider.as_ref();
        text.push_str(&format!("Selected Track: {:?} → {:?}\nPosition: local disc {:?}, Track {:?}; compared as disc {}; provider disc {:?}, Track {:?}\nPosition/program decision: {}\nKnown local occurrences: {}\nKnown local Recording identities: {}\nKnown local Artist identities: {}\nProvider Track: {}\nProvider Artist: {}\nProvider Recording identities: {}\nDurations: local {:?} ms; provider {:?} ms (not Recording identity)\n",t.local.title,provider.and_then(|p|p.title.as_ref()),t.local.disc,t.local.number,t.normalized_disc,provider.and_then(|p|p.disc),provider.and_then(|p|p.number),t.decision.label(),identities_text(&t.local.identities),identities_text(&t.local.recording.identities),identities_text(&t.local.artists.iter().flat_map(|a|a.identities.clone()).collect::<Vec<_>>()),provider.map(|p|identities_text(&p.identities)).unwrap_or_else(||"none at this position".into()),provider.map(|p|p.artists.iter().map(|a|format!("{} [{}]",a.name,identities_text(&a.identities))).collect::<Vec<_>>().join(", ")).unwrap_or_else(||"unknown".into()),provider.map(|p|identities_text(&p.recording.identities)).unwrap_or_else(||"none".into()),t.local.duration_ms,provider.and_then(|p|p.duration_ms)));
    }
    text
}
