//! Read-only edition evidence audit. No assessment authorizes persistence.
use crate::{
    catalog::CatalogError,
    domain::{AlbumId, ExternalIdentity, RecordingId, ReleaseId, TrackId},
    matching::normalize,
};

#[derive(Clone, Debug, Default)]
pub struct ArtistEvidence {
    pub identities: Vec<ExternalIdentity>,
    pub name: String,
    pub join_phrase: String,
}
#[derive(Clone, Debug, Default)]
pub struct RecordingEvidence {
    /// Trusted identifiers of a particular recording/version, not occurrence IDs.
    pub identities: Vec<ExternalIdentity>,
    pub isrcs: Vec<String>,
}
#[derive(Clone, Debug, Default)]
pub struct TrackEvidence {
    pub identities: Vec<ExternalIdentity>,
    pub disc: Option<u32>,
    pub number: Option<u32>,
    pub title: Option<String>,
    pub artists: Vec<ArtistEvidence>,
    pub duration_ms: Option<u64>,
    pub recording: RecordingEvidence,
}
#[derive(Clone, Debug, Default)]
pub struct EditionMetadata {
    pub title: Option<String>,
    pub artists: Vec<ArtistEvidence>,
    /// ISO partial date; retain precision rather than inventing month/day values.
    pub date: Option<String>,
    pub barcodes: Vec<String>,
    pub labels: Vec<(String, Option<String>)>,
    pub country: Option<String>,
    pub media: Vec<String>,
    pub edition_text: Option<String>,
}
#[derive(Clone, Debug)]
pub struct EditionCandidate {
    /// Catalog locator; not automatically proof of exact edition semantics.
    pub identity: ExternalIdentity,
    /// Explicitly verified exact-edition identifiers; never barcode alone.
    pub exact_identities: Vec<ExternalIdentity>,
    pub grouping: Option<ExternalIdentity>,
    pub metadata: EditionMetadata,
    /// Musical order, flattened across media by the adapter; not an unordered set.
    pub tracks: Vec<TrackEvidence>,
    pub tracklist_complete: bool,
}
#[derive(Clone, Debug)]
pub struct LocalTrackEvidence {
    pub track_id: TrackId,
    pub recording_id: RecordingId,
    pub evidence: TrackEvidence,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Completeness {
    #[default]
    Unknown,
    /// Caller independently established the entire program and its musical order.
    /// Contiguous numbers, membership or equal provider counts are not proof.
    TrustedComplete,
}
#[derive(Clone, Debug)]
pub struct LocalEditionEvidence {
    pub album_id: AlbumId,
    pub release_id: ReleaseId,
    pub album_title: String,
    pub grouping_identities: Vec<ExternalIdentity>,
    pub identities: Vec<ExternalIdentity>,
    /// Independently trusted embedded/verified edition IDs; generic mappings aren't promoted.
    pub exact_identities: Vec<ExternalIdentity>,
    pub metadata: EditionMetadata,
    /// Musical order when complete; partial lists retain their original positions.
    pub tracks: Vec<LocalTrackEvidence>,
    pub completeness: Completeness,
}
/// Additive blocking detail capability. Interactive callers must use a worker.
/// Provider discovery and request budgeting remain separate from comparison.
pub trait EditionProvider: Send {
    fn edition(&mut self, identity: &ExternalIdentity) -> Result<EditionCandidate, CatalogError>;
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Finding {
    TitleSupport,
    ArtistSupport,
    GroupingSupport,
    ExactIdentitySupport,
    EditionIdentityConflict,
    BarcodeSupport,
    PackagingDifference,
    TrackCountConflict,
    PositionUnknown,
    OrderConflict,
    TitleDifference,
    ArtistDifference,
    RecordingSupport,
    RecordingIdentityConflict,
    IsrcSupport,
    IsrcDifference,
    DurationSupport,
    DurationConflict,
    IncompleteTracklist,
}
#[derive(Clone, Debug)]
pub struct TrackReport {
    pub track_id: TrackId,
    /// Index in the candidate's flattened list, absent when alignment is unknown.
    pub candidate_index: Option<usize>,
    pub findings: Vec<Finding>,
    pub supported: bool,
    pub contradiction: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Assessment {
    ExactEdition,
    ContentEquivalent,
    AlbumOnly,
    InsufficientEvidence,
    Contradictory,
    Ambiguous,
}
#[derive(Clone, Debug)]
pub struct CandidateReport {
    pub identity: ExternalIdentity,
    pub assessment: Assessment,
    pub findings: Vec<Finding>,
    pub tracks: Vec<TrackReport>,
    pub supported_tracks: usize,
}
#[derive(Clone, Debug)]
pub struct Report {
    pub assessment: Assessment,
    /// Discovery completeness is separate from per-candidate identity/content evidence.
    pub candidates_complete: bool,
    pub candidates: Vec<CandidateReport>,
}
fn overlap<T: PartialEq>(a: &[T], b: &[T]) -> bool {
    a.iter().any(|v| b.contains(v))
}
fn valid_identity(id: &ExternalIdentity) -> bool {
    !id.provider.is_empty() && !id.kind.is_empty() && !id.external_id.is_empty()
}
fn shared_identity(a: &[ExternalIdentity], b: &[ExternalIdentity]) -> bool {
    a.iter().any(|id| valid_identity(id) && b.contains(id))
}
fn conflict(a: &[ExternalIdentity], b: &[ExternalIdentity]) -> bool {
    a.iter().any(|a| {
        b.iter().any(|b| {
            valid_identity(a)
                && valid_identity(b)
                && a.provider == b.provider
                && a.kind == b.kind
                && a.external_id != b.external_id
        })
    })
}
fn names(credits: &[ArtistEvidence]) -> String {
    normalize(
        &credits
            .iter()
            .map(|c| format!("{}{}", c.name, c.join_phrase))
            .collect::<String>(),
    )
}
fn title(a: &Option<String>, b: &Option<String>) -> Option<bool> {
    match (
        a.as_ref().map(|v| normalize(v)),
        b.as_ref().map(|v| normalize(v)),
    ) {
        (Some(a), Some(b)) if !a.is_empty() && !b.is_empty() => Some(a == b),
        _ => None,
    }
}
fn track_report(
    local: &LocalTrackEvidence,
    candidate: Option<(usize, &TrackEvidence)>,
) -> TrackReport {
    let mut result = TrackReport {
        track_id: local.track_id.clone(),
        candidate_index: candidate.map(|(i, _)| i),
        findings: vec![],
        supported: false,
        contradiction: false,
    };
    let Some((_, c)) = candidate else {
        result.findings.push(Finding::PositionUnknown);
        return result;
    };
    let t = &local.evidence;
    let same_recording = shared_identity(&t.recording.identities, &c.recording.identities);
    if same_recording {
        result.findings.push(Finding::RecordingSupport);
    }
    if conflict(&t.recording.identities, &c.recording.identities)
        || conflict(&t.recording.identities, &t.recording.identities)
        || conflict(&c.recording.identities, &c.recording.identities)
    {
        result.findings.push(Finding::RecordingIdentityConflict);
        result.contradiction = true;
    }
    let same_title = title(&t.title, &c.title);
    if same_title == Some(false) {
        result.findings.push(Finding::TitleDifference);
    }
    let artist_difference = !names(&t.artists).is_empty()
        && !names(&c.artists).is_empty()
        && names(&t.artists) != names(&c.artists)
        && !(t.artists.len() == c.artists.len()
            && t.artists
                .iter()
                .zip(&c.artists)
                .all(|(a, b)| overlap(&a.identities, &b.identities)));
    if artist_difference {
        result.findings.push(Finding::ArtistDifference);
    }
    let isrc_difference = !t.recording.isrcs.is_empty()
        && !c.recording.isrcs.is_empty()
        && !overlap(&t.recording.isrcs, &c.recording.isrcs);
    if isrc_difference {
        result.findings.push(Finding::IsrcDifference);
    } else if overlap(&t.recording.isrcs, &c.recording.isrcs) {
        result.findings.push(Finding::IsrcSupport);
    }
    if let (Some(a), Some(b)) = (t.duration_ms, c.duration_ms) {
        if a.abs_diff(b) <= 3_000 {
            result.findings.push(Finding::DurationSupport);
        } else {
            result.findings.push(Finding::DurationConflict);
            // Report differing measurements without overruling trusted Recording identity.
            result.contradiction |= !same_recording;
        }
    }
    // ISRC alone is corroboration, not a substitute for recording identity or title.
    result.supported = !result.contradiction
        && (same_recording || (same_title == Some(true) && !artist_difference && !isrc_difference));
    result
}
fn candidate_report(local: &LocalEditionEvidence, c: &EditionCandidate) -> CandidateReport {
    let mut findings = vec![];
    let exact = shared_identity(&local.exact_identities, &c.exact_identities);
    if exact {
        findings.push(Finding::ExactIdentitySupport);
    }
    let mut contradictory = conflict(&local.exact_identities, &c.exact_identities)
        || conflict(&local.exact_identities, &local.exact_identities)
        || conflict(&c.exact_identities, &c.exact_identities);
    if contradictory {
        findings.push(Finding::EditionIdentityConflict);
    }
    let title_support = title(&Some(local.album_title.clone()), &c.metadata.title) == Some(true);
    let artist_support = !names(&local.metadata.artists).is_empty()
        && names(&local.metadata.artists) == names(&c.metadata.artists);
    let grouping_support = c
        .grouping
        .as_ref()
        .is_some_and(|id| local.grouping_identities.contains(id));
    if title_support {
        findings.push(Finding::TitleSupport);
    }
    if artist_support {
        findings.push(Finding::ArtistSupport);
    }
    if grouping_support {
        findings.push(Finding::GroupingSupport);
    }
    if overlap(&local.metadata.barcodes, &c.metadata.barcodes) {
        findings.push(Finding::BarcodeSupport);
    }
    if local.metadata.country != c.metadata.country
        || local.metadata.barcodes != c.metadata.barcodes
        || local.metadata.labels != c.metadata.labels
        || local.metadata.date != c.metadata.date
        || local.metadata.media != c.metadata.media
    {
        findings.push(Finding::PackagingDifference);
    }
    let complete = local.completeness == Completeness::TrustedComplete && c.tracklist_complete;
    if !complete {
        findings.push(Finding::IncompleteTracklist);
    }
    if complete && local.tracks.len() != c.tracks.len() {
        findings.push(Finding::TrackCountConflict);
        contradictory = true;
    }
    let tracks: Vec<_> = local
        .tracks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let aligned = if complete {
                c.tracks.get(i).map(|c| (i, c))
            } else {
                match (t.evidence.disc, t.evidence.number) {
                    (Some(disc), Some(number)) => {
                        let mut matched = c
                            .tracks
                            .iter()
                            .enumerate()
                            .filter(|(_, c)| c.disc == Some(disc) && c.number == Some(number));
                        let first = matched.next();
                        if matched.next().is_none() {
                            first
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            };
            let mut r = track_report(t, aligned);
            // A known sequence permutation is stronger than a spelling difference.
            if complete
                && !r.supported
                && !r.contradiction
                && c.tracks.iter().enumerate().any(|(j, other)| {
                    j != i && title(&t.evidence.title, &other.title) == Some(true)
                })
            {
                r.findings.push(Finding::OrderConflict);
                r.contradiction = true;
            }
            r
        })
        .collect();
    contradictory |= tracks.iter().any(|t| t.contradiction);
    let supported_tracks = tracks.iter().filter(|t| t.supported).count();
    let assessment = if contradictory {
        Assessment::Contradictory
    } else if exact {
        Assessment::ExactEdition
    } else if complete && !tracks.is_empty() && supported_tracks == local.tracks.len() {
        Assessment::ContentEquivalent
    } else if grouping_support || (title_support && artist_support) {
        Assessment::AlbumOnly
    } else {
        Assessment::InsufficientEvidence
    };
    CandidateReport {
        identity: c.identity.clone(),
        assessment,
        findings,
        tracks,
        supported_tracks,
    }
}
/// Compare a bounded candidate set. No score or result ordering resolves identity.
pub fn compare(
    local: &LocalEditionEvidence,
    candidates: &[EditionCandidate],
    complete: bool,
) -> Report {
    let reports: Vec<_> = candidates
        .iter()
        .map(|c| candidate_report(local, c))
        .collect();
    let exact = reports
        .iter()
        .filter(|r| r.assessment == Assessment::ExactEdition)
        .count();
    let content = reports
        .iter()
        .filter(|r| r.assessment == Assessment::ContentEquivalent)
        .count();
    // A contradictory positively identified candidate must not lose to a weaker match.
    let blocked = conflict(&local.exact_identities, &local.exact_identities)
        || reports.iter().any(|r| {
            r.assessment == Assessment::Contradictory
                && r.findings.contains(&Finding::ExactIdentitySupport)
        });
    let assessment = if blocked {
        Assessment::Contradictory
    } else if exact > 1 {
        Assessment::Ambiguous
    } else if exact == 1 {
        Assessment::ExactEdition
    } else if content > 1 {
        Assessment::Ambiguous
    } else if content == 1
        && complete
        && reports
            .iter()
            .filter(|r| r.assessment != Assessment::Contradictory)
            .count()
            == 1
    {
        Assessment::ContentEquivalent
    } else if content == 1 {
        Assessment::Ambiguous
    } else if reports
        .iter()
        .any(|r| r.assessment == Assessment::AlbumOnly)
    {
        Assessment::AlbumOnly
    } else if !reports.is_empty()
        && reports
            .iter()
            .all(|r| r.assessment == Assessment::Contradictory)
    {
        Assessment::Contradictory
    } else {
        Assessment::InsufficientEvidence
    };
    Report {
        assessment,
        candidates_complete: complete,
        candidates: reports,
    }
}
