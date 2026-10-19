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
    TrackTitleMismatch,
    AlbumVersionMismatch,
    ReleaseTypeMismatch,
    TrackCountMismatch,
    PositionMismatch,
    InsufficientProgramAnchors,
    TrustedIdentityConflict,
    IncompleteProgram,
    DurationConflict,
    CompetingCandidates,
    WeakerArtistCredit,
    UnmatchedTrack,
    IncompleteCandidatePage,
    CandidateLimit,
    AcceptedExactTitle,
    AcceptedNormalizedTitle,
    AcceptedArtistBoilerplate,
    AcceptedReleaseTypeSuffix,
    AcceptedMinorTypo,
    AcceptedPlayableEdition,
    EquivalentCandidatesCollapsed,
    EquivalentOccurrences,
    AlreadyAssociated,
    NotEvaluated,
    ManualReviewRequired,
    InsufficientEvidence,
    EvidenceChanged,
    CompletionFailed,
}
impl Reason {
    pub fn review_label(self) -> &'static str {
        match self {
            Self::NoCandidates => "No Spotify candidate",
            Self::ArtistUnresolved => "Artist identity needs review",
            Self::ArtistMismatch | Self::TrustedIdentityConflict => {
                "Artist / trusted identity conflict"
            }
            Self::AlbumTitleMismatch => "Album title mismatch",
            Self::TrackTitleMismatch => "Track title / version mismatch",
            Self::AlbumVersionMismatch | Self::ReleaseTypeMismatch => "Version qualifier conflict",
            Self::TrackCountMismatch => "Album program mismatch",
            Self::PositionMismatch => "No corresponding Track position",
            Self::InsufficientProgramAnchors => "Insufficient positional evidence",
            Self::CompetingCandidates => "Competing candidates",
            Self::WeakerArtistCredit => "Comparable program with additional Artist credits",
            Self::UnmatchedTrack => "Track absent from the preferred Spotify program",
            Self::NotEvaluated => "No reconciliation attempted yet",
            Self::ManualReviewRequired => "Manual review required",
            Self::InsufficientEvidence => "Insufficient evidence",
            Self::IncompleteProgram | Self::IncompleteCandidatePage | Self::CandidateLimit => {
                "Insufficient evidence"
            }
            Self::DurationConflict => "Duration conflict",
            Self::EvidenceChanged => "Evidence changed; re-evaluate",
            Self::CompletionFailed => "Association persistence failed",
            Self::AcceptedPlayableEdition => {
                "Connected from corresponding provider edition; canonical Release retained"
            }
            _ => "Album needs review",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::NoCandidates => "No candidates returned by the bounded search",
            Self::ArtistUnresolved => "Artist identity is not established",
            Self::ArtistMismatch => "Candidate Artist IDs differ from the established Artist",
            Self::AlbumTitleMismatch => "Album titles do not agree under conservative comparison",
            Self::TrackTitleMismatch => "Track titles or semantic versions disagree",
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
            Self::WeakerArtistCredit => {
                "Equally strong program with additional provider Artist credits; cleaner candidate preferred"
            }
            Self::UnmatchedTrack => "No corresponding occurrence in the preferred Spotify program",
            Self::IncompleteCandidatePage => {
                "Candidate page has more results; uniqueness is not established"
            }
            Self::CandidateLimit => "Program comparison bound exceeded",
            Self::AcceptedExactTitle => {
                "Established Artist, exact Album title and compatible positioned program"
            }
            Self::AcceptedArtistBoilerplate => {
                "Established Artist boilerplate normalized; strong positioned program corroborates"
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
            Self::AcceptedPlayableEdition => {
                "Sole compatible provider edition supplies the strongly aligned songs; canonical edition metadata retained"
            }
            Self::EquivalentCandidatesCollapsed => {
                "Equivalent provider objects share a fully corroborated ordered program; deterministic representation selected"
            }
            Self::EquivalentOccurrences => {
                "Only agreeing occurrences are usable; no preferred Album representation established"
            }
            Self::AlreadyAssociated => "Existing trusted association retained",
            Self::EvidenceChanged => {
                "Persisted metadata or trusted associations changed before completion; evaluation withheld"
            }
            Self::ManualReviewRequired => "Compatible candidate awaits explicit manual review",
            Self::InsufficientEvidence => "No candidate has sufficient compatible evidence",
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
    EstablishedArtistBoilerplate,
    MinorTypo,
    PlayableEdition,
    SharedTitleSpan,
    Conflict,
    VersionConflict,
    TypeConflict,
}
impl TitleComparison {
    pub fn label(self) -> &'static str {
        match self {
            Self::Exact => "exact title",
            Self::Typography => "typography normalized",
            Self::EstablishedArtistBoilerplate => {
                "established Artist boilerplate normalized; requires strong program evidence"
            }
            Self::ReleaseTypeSuffix => "compatible release-type suffix normalized",
            Self::MinorTypo => "minor typo; requires strong program evidence",
            Self::PlayableEdition => {
                "alternate playable edition; requires unique strong program evidence"
            }
            Self::SharedTitleSpan => {
                "shared Album title component; requires strong program evidence"
            }
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
/// Release-title correspondence only; never sufficient without program evidence.
pub fn playable_edition_pair(local: &str, provider: &str) -> bool {
    fn annotation(text: &str) -> bool {
        let words: Vec<_> = text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .collect();
        words.iter().any(|w| {
            matches!(
                *w,
                "remaster"
                    | "remastered"
                    | "reissue"
                    | "deluxe"
                    | "expanded"
                    | "extended"
                    | "anniversary"
            )
        }) && words.iter().all(|w| {
            matches!(
                *w,
                "remaster"
                    | "remastered"
                    | "reissue"
                    | "deluxe"
                    | "expanded"
                    | "extended"
                    | "anniversary"
                    | "edition"
                    | "version"
                    | "bonus"
                    | "tracks"
                    | "digital"
                    | "digitally"
            ) || w.parse::<u32>().is_ok()
                || ["th", "st", "nd", "rd"].iter().any(|suffix| {
                    w.strip_suffix(suffix)
                        .is_some_and(|n| n.parse::<u32>().is_ok())
                })
        })
    }
    fn base(title: &str) -> (String, bool) {
        let normalized = normalize(title);
        let mut text = normalized.as_str();
        let mut stripped = false;
        loop {
            let candidate = text
                .strip_suffix(')')
                .and_then(|t| t.rsplit_once(" ("))
                .or_else(|| text.rsplit_once(" - "));
            if let Some((prefix, suffix)) = candidate
                && annotation(suffix)
                && !prefix.trim().is_empty()
            {
                text = prefix.trim();
                stripped = true;
                continue;
            }
            for suffix in [
                " deluxe edition",
                " expanded edition",
                " extended edition",
                " anniversary edition",
                " remastered",
                " remaster",
                " reissue",
                " deluxe",
            ] {
                if let Some(prefix) = text.strip_suffix(suffix)
                    && !prefix.trim().is_empty()
                {
                    text = prefix.trim();
                    stripped = true;
                    break;
                }
            }
            break;
        }
        (album_representation_title(text), stripped)
    }
    let (a, changed_a) = base(local);
    let (b, changed_b) = base(provider);
    (changed_a || changed_b) && !a.is_empty() && a == b
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
    pub warnings: Vec<String>,
    #[serde(skip)]
    pub provider_rank: Option<usize>,
    pub title: TitleEvidence,
    pub local_date: Option<String>,
    pub provider_date: String,
    pub date_agreement: String,
    pub local_tracks: usize,
    pub required_tracks: usize,
    pub provider_tracks: Option<u32>,
    pub programs: Vec<album_program::PositionedProgram>,
    pub equivalent_to: Option<ExternalIdentity>,
    pub playable_family: Option<ExternalIdentity>,
    pub decision: String,
    pub reasons: Vec<Reason>,
}
impl CandidateEvidence {
    fn equivalent_title_typography(&self) -> bool {
        normalize(&crate::matching::title_typography(&self.title.local_raw))
            == normalize(&crate::matching::title_typography(&self.title.provider_raw))
    }
    /// The exact anchor threshold consumed by conservative title relaxation.
    pub fn minimum_program_anchors(&self) -> usize {
        if self.title.comparison == TitleComparison::PlayableEdition {
            return 3;
        }
        if self.title.comparison == TitleComparison::Exact || self.equivalent_title_typography() {
            0
        } else {
            3.max(self.required_tracks.saturating_sub(1))
        }
    }
    pub fn acceptance_requirements(&self) -> String {
        let title_requirement = if self.minimum_program_anchors() == 0 {
            "Exact or typographically equivalent title: the standard compatible positioned program is still required".into()
        } else {
            format!(
                "Title relaxation requires at least {} independent title/position anchors",
                self.minimum_program_anchors()
            )
        };
        format!(
            "Established provider Artist identity (or explicitly confirmed equivalence); valid Album identity namespace; compatible title/type/version; complete program; no duplicate, missing, shifted or conflicting positions. {title_requirement}. Competing programs require a unique date winner and at least 3 anchors each; a typo cannot win by date. Missing program data means an earlier gate or bound stopped evaluation."
        )
    }
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Report {
    pub discovery_queries: Vec<String>,
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
    initial_report_with_equivalents(provider, context, artist, page, local, &[])
}
pub fn initial_report_with_equivalents(
    provider: &impl CatalogProvider,
    context: (&str, Option<crate::catalog_date::Date>),
    artist: Option<&ExternalIdentity>,
    page: &Page<ArtistAlbumCandidate>,
    local: &[LocalTrackEvidence],
    equivalents: &[ExternalIdentity],
) -> Report {
    let mut candidates = page.items.clone();
    candidates.extend(provider.rejected_album_candidates());
    candidates.sort_by(|a, b| identity_key(&a.identity).cmp(&identity_key(&b.identity)));
    candidates.dedup_by(|a, b| a.identity == b.identity);
    let count = candidates.len();
    Report {
        discovery_queries: provider.album_discovery_queries(),
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
                let mut title = title_evidence(context.0, &c);
                let accepted = artist.is_some_and(|a| {
                    c.artist_ids
                        .first()
                        .is_some_and(|id| id == a || equivalents.contains(id))
                        && a.kind == "artist"
                        && !a.external_id.is_empty()
                        && c.identity.provider == a.provider
                        && c.identity.kind == "album"
                        && !c.identity.external_id.is_empty()
                });
                let primary_established = artist.is_some_and(|id| {
                    c.artist_ids
                        .first()
                        .is_some_and(|candidate| candidate == id || equivalents.contains(candidate))
                });
                if primary_established
                    && title.comparison == TitleComparison::Conflict
                    && title.provider_normalized.strip_prefix("this is ")
                        == Some(title.local_normalized.as_str())
                {
                    title.comparison = TitleComparison::EstablishedArtistBoilerplate;
                }
                let established_names = std::iter::once(c.artist.as_str()).chain(
                    local
                        .iter()
                        .filter_map(|t| t.evidence.artists.first())
                        .filter(|a| {
                            artist.is_some_and(|id| {
                                a.identities.contains(id)
                                    || a.identities.iter().any(|i| equivalents.contains(i))
                            })
                        })
                        .map(|a| a.name.as_str()),
                );
                if primary_established
                    && matches!(
                        title.comparison,
                        TitleComparison::Conflict | TitleComparison::VersionConflict
                    )
                {
                    for name in established_names {
                        if let Some(base) = crate::matching::artist_album_title(&c.title, name)
                            && (crate::matching::normalize_album_title(context.0)
                                == crate::matching::normalize_album_title(base)
                                || crate::matching::normalize_album_title(base)
                                    == format!(
                                        "{} live",
                                        crate::matching::normalize_album_title(context.0)
                                    ))
                        {
                            title.provider_normalized =
                                crate::matching::normalize_album_title(base);
                            title.comparison = TitleComparison::EstablishedArtistBoilerplate;
                            break;
                        }
                    }
                }
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
                ) && !provider
                    .album_candidate_track_count(&c.identity)
                    .is_some_and(|n| local.len() >= 3 && n as usize * 5 >= local.len() * 4)
                {
                    reasons.push(Reason::TrackCountMismatch);
                }
                CandidateEvidence {
                    identity: c.identity.clone(),
                    artist: c.artist,
                    artist_ids: sorted_ids(&c.artist_ids),
                    artist_accepted: accepted,
                    warnings: if accepted && c.artist_ids.len() > 1 {
                        vec![
                            "Additional provider Artist credits retained; no alias inferred".into(),
                        ]
                    } else {
                        vec![]
                    },
                    provider_rank: page
                        .items
                        .iter()
                        .position(|item| item.identity == c.identity)
                        .map(|i| i + 1),
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
                    playable_family: None,
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
    if p.tracks
        .iter()
        .any(|t| t.decision == P::TrustedIdentityConflict)
    {
        return Some(Reason::TrustedIdentityConflict);
    }
    if p.provider_count < required && !p.alignment.strong {
        return Some(Reason::TrackCountMismatch);
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
    if p.tracks.iter().any(|t| {
        matches!(
            t.decision,
            P::InsufficientAnchors | P::AmbiguousCorrespondence
        )
    }) {
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
        && (same_program(ap, bp)
            || (!a.date.is_empty() && a.date == b.date && same_supported_program(ap, bp)))
}

// Different Spotify occurrence IDs do not necessarily mean different musical
// programs. This is representation selection, never an Artist/Recording merge.
fn same_supported_program(a: &Programs, b: &Programs) -> bool {
    let ([a], [b]) = (a.programs.as_slice(), b.programs.as_slice()) else {
        return false;
    };
    if !a.complete || !b.complete || a.tracks.len() < 3 || a.tracks.len() != b.tracks.len() {
        return false;
    }
    let mut titles = std::collections::HashSet::new();
    let mut positions = std::collections::HashSet::new();
    a.tracks.iter().zip(&b.tracks).all(|(x, y)| {
        let title = album_program::comparison_title(x.title.as_deref().unwrap_or(""));
        let credits = |t: &crate::edition::TrackEvidence| {
            let ids: Vec<_> = t
                .artists
                .iter()
                .flat_map(|a| a.identities.clone())
                .collect();
            (!t.artists.is_empty()
                && t.artists.iter().all(|a| {
                    a.identities
                        .iter()
                        .any(|i| i.provider == "spotify" && i.kind == "artist")
                }))
            .then(|| sorted_ids(&ids))
        };
        let conflicting_recording = [(x, y), (y, x)].iter().any(|(x, y)| {
            x.recording.identities.iter().any(|id| {
                y.recording
                    .identities
                    .iter()
                    .any(|other| other.provider == id.provider && other.kind == id.kind)
                    && !y.recording.identities.contains(id)
            })
        }) || (!x.recording.isrcs.is_empty()
            && !y.recording.isrcs.is_empty()
            && !x
                .recording
                .isrcs
                .iter()
                .any(|id| y.recording.isrcs.contains(id)));
        !title.is_empty()
            && [x, y].iter().all(|t| {
                t.identities
                    .iter()
                    .filter(|id| {
                        id.provider == "spotify" && id.kind == "track" && !id.external_id.is_empty()
                    })
                    .count()
                    == 1
            })
            && titles.insert(title.clone())
            && title == album_program::comparison_title(y.title.as_deref().unwrap_or(""))
            && x.disc == y.disc
            && x.number == y.number
            && x.number.is_some_and(|n| n > 0)
            && positions.insert((x.disc, x.number))
            && credits(x).is_some()
            && credits(x) == credits(y)
            && !x.duration_approximate
            && !y.duration_approximate
            && x.duration_ms
                .zip(y.duration_ms)
                .is_some_and(|(a, b)| a > 0 && b > 0 && a.abs_diff(b) <= 100)
            && !conflicting_recording
    })
}
#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_spotify(
    provider: &mut impl CatalogProvider,
    context: (&str, Option<crate::catalog_date::Date>),
    artist: &ExternalIdentity,
    page: &Page<ArtistAlbumCandidate>,
    local: &[LocalTrackEvidence],
    cache: &mut Vec<Programs>,
    equivalents: &[ExternalIdentity],
    known: &crate::canonical_evidence::AlbumEvidence,
) -> Result<Resolution, CatalogError> {
    let mut report =
        initial_report_with_equivalents(provider, context, Some(artist), page, local, equivalents);
    // Credit order is presentation when another established representation has
    // the identical identity set AND complete ordered occurrence identities.
    // Membership of the established Artist alone never relaxes a collaboration.
    let order_only: Vec<_> = report
        .candidates
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            (page.next_offset.is_none()
                && page.items.len() <= MAX_PROGRAM_CANDIDATES
                && row.reasons == vec![Reason::ArtistMismatch])
            .then(|| {
                let candidate = page.items.iter().find(|c| c.identity == row.identity)?;
                let other = page.items.iter().find(|other| {
                    report.candidates.iter().any(|r| {
                        r.identity == other.identity && r.artist_accepted && r.reasons.is_empty()
                    }) && sorted_ids(&candidate.artist_ids) == sorted_ids(&other.artist_ids)
                })?;
                Some((index, candidate.clone(), other.clone()))
            })
            .flatten()
        })
        .take(MAX_PROGRAM_CANDIDATES)
        .collect();
    for (index, candidate, other) in order_only {
        let mut programs = vec![];
        for identity in [&candidate.identity, &other.identity] {
            let p = if let Some(p) = cache.iter().find(|p| &p.album == identity) {
                p.clone()
            } else {
                let p = provider.album_programs(identity)?;
                if p.album != *identity {
                    return Err(CatalogError::Other(
                        "Candidate program belongs to another Album".into(),
                    ));
                }
                if cache.len() == MAX_PROGRAM_CANDIDATES {
                    cache.remove(0);
                }
                cache.push(p.clone());
                p
            };
            programs.push(p);
        }
        if equivalent(&candidate, &programs[0], &other, &programs[1]) {
            let row = &mut report.candidates[index];
            row.artist_accepted = true;
            row.reasons.clear();
            row.decision = "Withheld".into();
            row.warnings.push("Provider Artist credit order differs; identical Artist identity set and complete ordered Track identities".into());
        }
    }
    // An established provider observation can corroborate an incomplete display
    // title. Semantic version conflicts remain a veto, never discarded.
    for row in &mut report.candidates {
        if row.title.comparison == TitleComparison::Conflict
            && qualifiers(&row.title.local_raw).is_empty()
            && qualifiers(&row.title.provider_raw).is_empty()
            && row.title.local_type_hint.is_none()
            && suffix_base(&album_representation_title(&row.title.provider_raw))
                .1
                .is_none()
            && !playable_edition_pair(&row.title.local_raw, &row.title.provider_raw)
            && crate::matching::shared_title_span(&row.title.local_raw, &row.title.provider_raw)
                .is_some()
        {
            row.title.comparison = TitleComparison::SharedTitleSpan;
            row.reasons.retain(|r| *r != Reason::AlbumTitleMismatch);
            row.warnings.push(
                "Shared Album title component; raw metadata retained; strong program required"
                    .into(),
            );
        }
        if row.title.comparison == TitleComparison::Conflict {
            for observation in &known.titles {
                if let Some(candidate) = page.items.iter().find(|c| c.identity == row.identity) {
                    let title = title_evidence(&observation.value, candidate);
                    if matches!(
                        title.comparison,
                        TitleComparison::Exact
                            | TitleComparison::Typography
                            | TitleComparison::ReleaseTypeSuffix
                    ) {
                        row.title = title;
                        row.reasons.retain(|r| *r != Reason::AlbumTitleMismatch);
                        break;
                    }
                }
            }
        }
        if known.release_types.iter().any(|o| {
            !crate::canonical_evidence::release_type_compatible(
                &o.value,
                &row.title.provider_type,
                &row.identity.provider,
            )
        }) {
            row.reasons.push(Reason::ReleaseTypeMismatch);
        }
    }
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
        report.rank_candidates();
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
        if !can_accommodate(row.required_tracks, row.provider_tracks)
            && !row
                .provider_tracks
                .is_some_and(|n| local.len() >= 3 && n as usize * 5 >= local.len() * 4)
        {
            if !row.reasons.contains(&Reason::TrackCountMismatch) {
                row.reasons.push(Reason::TrackCountMismatch);
            }
            row.decision = "Rejected".into();
        }
    }
    // A Spotify locator is a playable representation, not an assertion that the
    // canonical Release has become this provider's remaster/deluxe edition.
    // Prefer normal candidates; only a sole compatible edition can be a fallback.
    if page.next_offset.is_none() && !report.candidates.iter().any(|c| c.reasons.is_empty()) {
        let alternatives: Vec<_> = report
            .candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                c.artist_accepted
                    && !c.reasons.is_empty()
                    && c.reasons.iter().all(|r| {
                        matches!(r, Reason::AlbumVersionMismatch | Reason::AlbumTitleMismatch)
                    })
                    && playable_edition_pair(&c.title.local_raw, &c.title.provider_raw)
            })
            .map(|(i, _)| i)
            .collect();
        if let [index] = alternatives.as_slice() {
            let row = &mut report.candidates[*index];
            row.title.comparison = TitleComparison::PlayableEdition;
            row.reasons.clear();
            row.decision = "Withheld".into();
            row.warnings.push("Spotify edition differs; using corresponding songs as playable sources without changing the canonical Release".into());
            if let Some(local_date) = context.1
                && let Some(provider_date) = crate::catalog_date::Date::parse(&row.provider_date)
                && local_date.year != provider_date.year
            {
                row.warnings.push(format!("Release year differs: {} ↔ {}; playback correspondence does not establish canonical Release identity", local_date.year, provider_date.year));
            }
        }
    }
    let viable: Vec<_> = report
        .candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| c.reasons.is_empty())
        .map(|(i, _)| i)
        .collect();
    if viable.len() > MAX_PROGRAM_CANDIDATES
        || page.next_offset.is_some()
            && viable
                .iter()
                .all(|i| report.candidates[*i].title.comparison == TitleComparison::PlayableEdition)
    {
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
            .cloned()
            .or_else(|| {
                provider
                    .rejected_album_candidates()
                    .into_iter()
                    .find(|c| c.identity == row.identity)
            })
            .ok_or_else(|| {
                CatalogError::Other(
                    "Evaluated candidate is missing from the bounded provider snapshot".into(),
                )
            })?;
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
            .map(|p| album_program::inspect_album_program(local, p, context.0))
            .collect();
        row.warnings
            .extend(row.programs.iter().flat_map(|p| p.warnings.clone()));
        if row.title.comparison == TitleComparison::EstablishedArtistBoilerplate
            && qualifiers(&row.title.local_raw) != qualifiers(&row.title.provider_raw)
        {
            let annotation = format!(
                "{} live",
                crate::matching::album_representation_title(context.0)
            );
            let systematic = p.programs.iter().all(|program| {
                program
                    .tracks
                    .iter()
                    .filter(|t| {
                        t.title
                            .as_deref()
                            .and_then(|title| title.rsplit_once(" - "))
                            .is_some_and(|(_, suffix)| {
                                let suffix = crate::matching::album_representation_title(suffix);
                                suffix.strip_suffix(" version").unwrap_or(&suffix) == annotation
                            })
                    })
                    .count()
                    * 5
                    >= program.tracks.len() * 4
                    && program.tracks.len() >= 3
            });
            if !systematic {
                row.reasons.push(Reason::AlbumVersionMismatch);
                row.decision = "Rejected".into();
                continue;
            }
        }
        let reasons: Vec<_> = row
            .programs
            .iter()
            .filter_map(|p| program_reason(p, row.required_tracks))
            .collect();
        let compatible = !row.programs.is_empty() && reasons.is_empty();
        let relaxed = row.minimum_program_anchors() != 0;
        let strong = row
            .programs
            .iter()
            .all(|p| p.anchors >= row.minimum_program_anchors());
        // Typo/suffix/typography cannot be an identity shortcut. No duration
        // contradictions or comparable competitors may be hidden by a typo.
        let duration_conflict = matches!(
            row.title.comparison,
            TitleComparison::MinorTypo | TitleComparison::PlayableEdition
        ) && row.programs.iter().any(|p| {
            p.tracks.iter().any(|t| {
                t.provider.as_ref().is_some_and(|c| {
                    t.local
                        .duration_ms
                        .zip(c.duration_ms)
                        .is_some_and(|(a, b)| {
                            a.abs_diff(b)
                                > if row.title.comparison == TitleComparison::PlayableEdition {
                                    10_000
                                } else {
                                    3000
                                }
                        })
                })
            })
        });
        if row.title.comparison == TitleComparison::PlayableEdition
            && row.programs.iter().any(|p| {
                let matched = p
                    .alignment
                    .rows
                    .iter()
                    .filter(|r| r.local_index.is_some() && r.provider_index.is_some())
                    .count();
                // Provider extras are expected for expanded editions. Coverage
                // of the canonical program, not consumption of bonus material,
                // is the relevant measure; normal positional/identity guards
                // below still apply when two-sided alignment is not strong.
                matched < 3 || matched * 5 < local.len() * 4
            })
        {
            row.reasons = vec![Reason::InsufficientProgramAnchors];
            row.decision = "Withheld".into();
        } else if !compatible {
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
    // Playback families pool useful occurrences without accepting multiple
    // canonical editions. They are candidate-set scoped and pairwise guarded.
    let family_compatible = |i: usize, j: usize| {
        let a = &report.candidates[i];
        let b = &report.candidates[j];
        let ap = cache.iter().find(|p| p.album == a.identity);
        let bp = cache.iter().find(|p| p.album == b.identity);
        a.artist_accepted && b.artist_accepted && a.artist_ids == b.artist_ids
            && album_representation_title(&a.title.provider_raw) == album_representation_title(&b.title.provider_raw)
            && a.title.provider_type == b.title.provider_type
            && matches!(a.title.comparison, TitleComparison::Exact | TitleComparison::Typography)
            && matches!(b.title.comparison, TitleComparison::Exact | TitleComparison::Typography)
            && ap.zip(bp).is_some_and(|(a,b)| {
                matches!((a.programs.as_slice(), b.programs.as_slice()), ([a],[b])
                    if album_program::compatible_provider_programs(a,b) && album_program::compatible_provider_programs(b,a))
            })
    };
    let mut families: Vec<Vec<usize>> = vec![];
    let mut visited = std::collections::HashSet::new();
    for i in 0..report.candidates.len() {
        if !visited.insert(i) {
            continue;
        }
        let mut group = vec![i];
        let mut cursor = 0;
        while cursor < group.len() {
            for j in 0..report.candidates.len() {
                if !visited.contains(&j) && family_compatible(group[cursor], j) {
                    visited.insert(j);
                    group.push(j);
                }
            }
            cursor += 1;
        }
        // Compatibility must hold for every pair, not merely transitively.
        if group.len() > 1
            && group
                .iter()
                .enumerate()
                .all(|(k, i)| group[k + 1..].iter().all(|j| family_compatible(*i, *j)))
        {
            families.push(group);
        }
    }
    for group in families {
        let representative = group
            .iter()
            .map(|i| report.candidates[*i].identity.clone())
            .min_by(|a, b| identity_key(a).cmp(&identity_key(b)))
            .unwrap();
        for index in group {
            report.candidates[index].playable_family = Some(representative.clone());
            report.candidates[index].warnings.push("Compatible provider release family for playback; canonical editions remain distinct".into());
        }
    }
    // A complete, uniquely identifying program can eliminate a similarly
    // named release with a demonstrably different program. Missing titles or
    // unfinished pagination inside a program remain uncertainty, not rejection.
    let complete_winner = examined.iter().any(|(i, _, _)| {
        let row = &report.candidates[*i];
        row.reasons.is_empty()
            && row.programs.iter().all(|p| {
                p.complete && p.alignment.strong && p.anchors >= 3 && p.anchors == local.len()
            })
    });
    if complete_winner
        && local.iter().all(|t| {
            t.evidence
                .title
                .as_deref()
                .is_some_and(crate::matching::usable)
        })
    {
        examined.retain(|(i, _, programs)| {
            let row = &mut report.candidates[*i];
            let wrong_program = !row.reasons.is_empty()
                && row.programs.iter().all(|p| p.complete && p.anchors * 2 < local.len())
                && programs.programs.iter().all(|p| p.complete && p.tracks.iter().all(|t|
                    t.title.as_deref().is_some_and(crate::matching::usable)));
            if wrong_program {
                row.reasons = vec![Reason::TrackTitleMismatch];
                row.decision = "Rejected".into();
                row.warnings.push("Complete Track program does not represent the canonical program; a fully matching candidate dominates".into());
            }
            !wrong_program
        });
    }
    // Complete equivalent programs with exact established credits dominate an
    // otherwise equally supported object carrying additional provider credits.
    let exact: Vec<_> = examined
        .iter()
        .filter(|(i, c, _)| c.artist_ids.len() == 1 && report.candidates[*i].reasons.is_empty())
        .map(|(_, _, p)| p.clone())
        .collect();
    examined.retain(|(_, c, p)| {
        c.artist_ids.len() == 1 || !exact.iter().any(|other| same_program(other, p))
    });
    // Track-level additional credits also matter when the Album credit is the
    // same. Comparable ordered programs cannot force review solely because one
    // occurrence adds a guest to every Track.
    let additional = |p: &Programs| {
        p.programs
            .iter()
            .flat_map(|p| &p.tracks)
            .map(|t| t.artists.len().saturating_sub(1))
            .sum::<usize>()
    };
    let comparable = |a: &Programs, b: &Programs| {
        let ([a], [b]) = (a.programs.as_slice(), b.programs.as_slice()) else {
            return false;
        };
        a.complete
            && b.complete
            && a.tracks.len() == b.tracks.len()
            && a.tracks.len() >= 3
            && a.tracks.iter().zip(&b.tracks).all(|(a, b)| {
                a.disc == b.disc
                    && a.number == b.number
                    && normalize(a.title.as_deref().unwrap_or(""))
                        == normalize(b.title.as_deref().unwrap_or(""))
                    && a.duration_ms
                        .zip(b.duration_ms)
                        .is_none_or(|(a, b)| a.abs_diff(b) <= 3000)
                    && a.artists
                        .first()
                        .zip(b.artists.first())
                        .is_some_and(|(a, b)| {
                            a.identities.iter().any(|id| b.identities.contains(id))
                        })
                    && !a.recording.identities.iter().any(|id| {
                        b.recording.identities.iter().any(|other| {
                            other.provider == id.provider && other.kind == id.kind && other != id
                        })
                    })
            })
    };
    let weaker: Vec<usize> = examined
        .iter()
        .filter(|(i, c, p)| {
            report.candidates[*i].reasons.is_empty()
                && examined.iter().any(|(j, other, op)| {
                    i != j
                        && c.artist_ids == other.artist_ids
                        && report.candidates[*j].reasons.is_empty()
                        && additional(op) < additional(p)
                        && comparable(op, p)
                        && report.candidates[*j]
                            .programs
                            .iter()
                            .map(|p| p.anchors)
                            .max()
                            >= report.candidates[*i]
                                .programs
                                .iter()
                                .map(|p| p.anchors)
                                .max()
                })
        })
        .map(|(i, _, _)| *i)
        .collect();
    for index in &weaker {
        report.candidates[*index].decision = "Rejected".into();
        report.candidates[*index].reasons = vec![Reason::WeakerArtistCredit];
    }
    examined.retain(|(i, _, _)| !weaker.contains(i));
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
    // A truncated broad name search need not veto a uniquely complete exact
    // program. Partial/typo/version evidence still requires complete discovery.
    if page.next_offset.is_some()
        && !winner.is_some_and(|g| {
            let row = &report.candidates[examined[groups[g][0]].0];
            matches!(
                row.title.comparison,
                TitleComparison::Exact
                    | TitleComparison::Typography
                    | TitleComparison::ReleaseTypeSuffix
            ) && row.programs.iter().all(|p| {
                p.complete && p.alignment.strong && p.anchors >= 3 && p.anchors == local.len()
            })
        })
    {
        return Ok(finish(
            MatchOutcome::AlbumAmbiguous(page.items.clone()),
            report,
            Reason::IncompleteCandidatePage,
        ));
    }
    if let Some(group_index) = winner {
        let group = &groups[group_index];
        let (index, c, _) = &examined[group[0]];
        let reason = if group.len() > 1 {
            Reason::EquivalentCandidatesCollapsed
        } else {
            match report.candidates[*index].title.comparison {
                TitleComparison::Exact => Reason::AcceptedExactTitle,
                TitleComparison::Typography => Reason::AcceptedNormalizedTitle,
                TitleComparison::EstablishedArtistBoilerplate => Reason::AcceptedArtistBoilerplate,
                TitleComparison::ReleaseTypeSuffix => Reason::AcceptedReleaseTypeSuffix,
                TitleComparison::PlayableEdition => Reason::AcceptedPlayableEdition,
                TitleComparison::SharedTitleSpan => Reason::AcceptedNormalizedTitle,
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
    /// Explainable evidence order, independent of Spotify response ordering.
    pub fn rank_candidates(&mut self) {
        let score = |c: &CandidateEvidence| {
            let class = match c.decision.as_str() {
                "Accepted" => 2,
                "Rejected" => 0,
                _ => 1,
            };
            let title = match c.title.comparison {
                TitleComparison::Exact => 5,
                TitleComparison::Typography => 4,
                TitleComparison::ReleaseTypeSuffix
                | TitleComparison::EstablishedArtistBoilerplate => 3,
                TitleComparison::MinorTypo => 2,
                _ => 0,
            };
            (
                class,
                c.artist_accepted,
                title,
                c.programs.iter().map(|p| p.anchors).max().unwrap_or(0),
                std::cmp::Reverse(c.warnings.len()),
                std::cmp::Reverse(c.reasons.len()),
            )
        };
        self.candidates.sort_by(|a, b| {
            score(b)
                .cmp(&score(a))
                .then_with(|| identity_key(&a.identity).cmp(&identity_key(&b.identity)))
        });
    }
    /// Concise review reason from structured evidence, never title-specific logic.
    pub fn review_reason(&self, track: &str) -> Reason {
        use album_program::PositionDecision;
        let accepted = self.candidates.iter().any(|c| c.decision == "Accepted");
        let decisions: Vec<_> = self
            .candidates
            .iter()
            .filter(|c| !accepted || c.decision == "Accepted")
            .flat_map(|c| &c.programs)
            .flat_map(|p| &p.tracks)
            .filter(|t| t.track_id == track)
            .map(|t| t.decision)
            .collect();
        if accepted && decisions.iter().all(|d| *d == PositionDecision::LocalOnly) {
            return Reason::UnmatchedTrack;
        }
        for (decision, reason) in [
            (
                PositionDecision::VersionConflict,
                Reason::TrackTitleMismatch,
            ),
            (
                PositionDecision::TrustedIdentityConflict,
                Reason::TrustedIdentityConflict,
            ),
            (PositionDecision::ShiftedProgram, Reason::TrackCountMismatch),
            (PositionDecision::PositionMismatch, Reason::PositionMismatch),
            (
                PositionDecision::InsufficientAnchors,
                Reason::InsufficientProgramAnchors,
            ),
        ] {
            if !decisions.is_empty() && decisions.iter().all(|d| *d == decision) {
                return reason;
            }
        }
        if !self.candidates.is_empty()
            && self
                .candidates
                .iter()
                .any(|c| c.title.comparison == TitleComparison::VersionConflict)
        {
            return Reason::AlbumVersionMismatch;
        }
        for reason in [
            Reason::ArtistMismatch,
            Reason::AlbumVersionMismatch,
            Reason::TrackCountMismatch,
            Reason::PositionMismatch,
            Reason::InsufficientProgramAnchors,
            Reason::AlbumTitleMismatch,
        ] {
            if !self.candidates.is_empty()
                && self.candidates.iter().all(|c| c.reasons.contains(&reason))
            {
                return reason;
            }
        }
        self.reasons
            .first()
            .copied()
            .unwrap_or(Reason::NotEvaluated)
    }

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
        text.insert_str(
            0,
            &format!(
                "{}: {}\n",
                if self.decision == "Accepted" {
                    "Automatic association accepted"
                } else {
                    "Primary blocker"
                },
                self.reasons
                    .first()
                    .copied()
                    .unwrap_or(Reason::NotEvaluated)
                    .label()
            ),
        );
        for query in &self.discovery_queries {
            text.push_str(&format!("Search query: {query}\n"));
        }
        for c in &self.candidates {
            text.push_str(&format!("\nCandidate: {} [{}]\nArtist: {} — {} [{}]\nAlbum title: {:?} → {:?}\nNormalized: {:?} → {:?} ({})\nRelease type: {}; local title hint: {}\nDate: local {}; provider {}; comparison {}\nTrack count: {} local, {} required; provider {}\nDecision: {}\nReason: {}\n",c.title.provider_raw,identity_text(&c.identity),c.artist,if c.artist_accepted{"accepted"}else{"not accepted"},c.artist_ids.iter().map(identity_text).collect::<Vec<_>>().join(", "),c.title.local_raw,c.title.provider_raw,c.title.local_normalized,c.title.provider_normalized,c.title.comparison.label(),if c.title.provider_type.is_empty(){"unknown"}else{&c.title.provider_type},c.title.local_type_hint.as_deref().unwrap_or("none"),c.local_date.as_deref().unwrap_or("unknown"),if c.provider_date.is_empty(){"unknown"}else{&c.provider_date},c.date_agreement,c.local_tracks,c.required_tracks,c.provider_tracks.map(|n|n.to_string()).unwrap_or_else(||"unknown".into()),c.decision,c.reasons.iter().map(|r|r.label()).collect::<Vec<_>>().join("; ")));
            if let Some(id) = &c.equivalent_to {
                text.push_str(&format!(
                    "Equivalent provider representation: {}\n",
                    identity_text(id)
                ));
            }
            for warning in &c.warnings {
                text.push_str(&format!("Warning: {warning}\n"));
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
