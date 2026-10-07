//! Bounded canonical song evaluation and acceptance for diagnostics and Play. Never Album,
//! Recording, or edition acceptance.
use crate::{
    catalog::{CatalogError, Page},
    domain::*,
    storage::{Error, Result, Store},
};
use rusqlite::params;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Input {
    pub track_id: TrackId,
    #[serde(default)]
    pub spotify_excluded: bool,
    pub evidence: crate::canonical_evidence::TrackEvidence,
    pub association_providers: Vec<String>,
    pub album_date: Option<crate::catalog_date::Date>,
    pub album_artists: Vec<crate::edition::ArtistEvidence>,
    /// Lower bound from all established Tracks, independent of membership.
    pub album_required_tracks: usize,
    /// Canonical first Track Artist, or the single Album Artist when no Track
    /// credit is available. No punctuation parsing or display rewrite.
    pub primary_artist: Option<crate::edition::ArtistEvidence>,
    pub title: String,
    pub artist: String,
    /// Ordered structured credits, separate from the unchanged display string.
    pub artists: Vec<crate::edition::ArtistEvidence>,
    pub album: String,
    pub duration_ms: Option<u64>,
    pub disc: Option<u32>,
    pub number: Option<u32>,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Candidate {
    /// Adapter-declared song/occurrence identity, never Recording identity.
    pub identity: ExternalIdentity,
    pub album_identity: Option<ExternalIdentity>,
    pub title: String,
    pub artist: String,
    /// Ordered structured credits, separate from the unchanged display string.
    pub artists: Vec<crate::edition::ArtistEvidence>,
    pub album: String,
    pub date: String,
    pub album_artists: Vec<crate::edition::ArtistEvidence>,
    pub album_type: String,
    pub album_total_tracks: Option<u32>,
    pub duration_ms: u64,
    pub disc: u32,
    pub number: u32,
}
pub trait SongSearch: Send {
    /// One bounded explicit request; no automatic acceptance or follow-up pages.
    fn search_songs(&mut self, input: &Input)
    -> std::result::Result<Page<Candidate>, CatalogError>;
}
#[derive(Clone, Debug)]
pub struct Selection {
    input: Input,
    candidates: Vec<Candidate>,
    show_all: bool,
    assessments: Vec<Feasibility>,
}
impl Selection {
    pub fn new(input: Input, candidates: Vec<Candidate>) -> Self {
        let assessments = classify(&input, &candidates);
        Self {
            input,
            candidates,
            show_all: false,
            assessments,
        }
    }
    pub fn show_all(&mut self) {
        self.show_all = true;
    }
    pub fn visible_indices(&self) -> Vec<usize> {
        let mut rows: Vec<_> = self
            .assessments
            .iter()
            .enumerate()
            .map(|(i, a)| (i, a.class))
            .filter(|(_, class)| self.show_all || *class != FeasibilityClass::Infeasible)
            .collect();
        rows.sort_by_key(|(_, class)| *class);
        rows.into_iter().map(|(i, _)| i).collect()
    }
    pub fn assessments(&self) -> &[Feasibility] {
        &self.assessments
    }
    pub fn is_showing_all(&self) -> bool {
        self.show_all
    }
    pub fn candidates(&self) -> &[Candidate] {
        &self.candidates
    }
    pub fn input(&self) -> &Input {
        &self.input
    }
}
impl Input {
    pub fn search_artist(&self) -> &str {
        if self
            .evidence
            .artist_credits
            .iter()
            .any(|o| matches!(o.origin, crate::canonical_evidence::Origin::UserOverride))
        {
            return &self.artist;
        }
        self.primary_artist
            .as_ref()
            .filter(|a| !a.name.trim().is_empty())
            .map_or(&self.artist, |a| &a.name)
    }
}
pub(crate) fn load_input(db: &rusqlite::Connection, track: &TrackId) -> Result<Input> {
    let (mut input, album) = db.query_row(
            "SELECT e.title,e.artist_names,e.release_title,r.album_id,e.duration_ms,e.disc_number,e.track_number FROM track t JOIN release r ON r.id=t.release_id JOIN effective_track_metadata e ON e.track_id=t.id WHERE t.id=?1",
            [track.as_ref()], |r| Ok((Input { track_id: track.clone(), spotify_excluded:false, evidence:Default::default(), association_providers:vec![], album_date:None, album_artists:vec![], album_required_tracks:0, primary_artist:None, artists:vec![], title:r.get(0)?, artist:r.get(1)?, album:r.get(2)?, duration_ms:r.get::<_,Option<i64>>(4)?.and_then(|v| u64::try_from(v).ok()), disc:r.get(5)?, number:r.get(6)? },r.get::<_,String>(3)?)))?;
    let year: Option<i32> = db.query_row(
        "SELECT year FROM effective_album_metadata WHERE album_id=?1",
        [&album],
        |r| r.get(0),
    )?;
    input.album_date = year.and_then(|y| crate::catalog_date::Date::parse(&y.to_string()));
    input.album_artists = crate::artist_credit::load(db, "album", &album)?
        .into_iter()
        .map(|a| {
            let mut e = a.evidence;
            e.name = a.canonical_name;
            e
        })
        .collect();
    // Read Album-scoped positions only; membership and source availability do not
    // define the established program. This remains a lower bound for partial imports.
    let mut positions = db.prepare("SELECT DISTINCT COALESCE(t.disc_number,1),t.track_number FROM release r JOIN track t ON t.release_id=r.id WHERE r.album_id=?1 AND t.track_number>0")?;
    let positions = positions
        .query_map([&album], |r| Ok((r.get::<_, u32>(0)?, r.get::<_, u32>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    input.album_required_tracks = positions.len().max(
        positions
            .iter()
            .map(|(_, n)| *n as usize)
            .max()
            .unwrap_or(0),
    );
    let mut credits = crate::artist_credit::load(db, "track", track.as_ref())?;
    let has_track_credits = !credits.is_empty();
    if credits.is_empty() {
        let album_credits = crate::artist_credit::load(db, "album", &album)?;
        if let [primary] = album_credits.as_slice() {
            let mut evidence = primary.evidence.clone();
            evidence.name = primary.canonical_name.clone();
            input.primary_artist = Some(evidence);
        }
        let display = album_credits
            .iter()
            .map(|a| format!("{}{}", a.evidence.name, a.evidence.join_phrase))
            .collect::<String>();
        if input.artist.trim().is_empty() {
            input.artist = display;
            credits = album_credits;
        } else if crate::matching::normalize(&input.artist) == crate::matching::normalize(&display)
        {
            credits = album_credits;
        }
    }
    if let Some(primary) = credits.first()
        && (has_track_credits || credits.len() == 1)
    {
        let mut evidence = primary.evidence.clone();
        evidence.name = primary.canonical_name.clone();
        input.primary_artist = Some(evidence);
    }
    input.artists = credits
        .into_iter()
        .map(|mut a| {
            a.evidence.name = a.canonical_name;
            a.evidence
        })
        .collect();
    input.evidence = crate::canonical_evidence::load(db, track, &album)?;
    let consensus = |values: &[crate::canonical_evidence::Observation<String>]| {
        let usable: Vec<_> = values
            .iter()
            .filter(|o| crate::matching::usable(&o.value))
            .collect();
        usable
            .first()
            .filter(|first| {
                usable.iter().all(|o| {
                    crate::matching::normalize_album_title(&o.value)
                        == crate::matching::normalize_album_title(&first.value)
                })
            })
            .map(|o| o.value.clone())
    };
    if !crate::matching::usable(&input.title)
        && let Some(title) = consensus(&input.evidence.titles)
    {
        input.title = title;
    }
    if !crate::matching::usable(&input.album)
        && let Some(title) = consensus(&input.evidence.album_titles)
    {
        input.album = title;
    }
    input.album_date = crate::canonical_evidence::agreed_date(&input.evidence.dates);
    if let Some(value) = crate::metadata::override_value(
        db,
        &crate::metadata::Target::Track(track.0.clone()),
        "year",
    )?
    .or(crate::metadata::override_value(
        db,
        &crate::metadata::Target::Album(album.clone()),
        "year",
    )?) {
        input.album_date = crate::catalog_date::Date::parse(&value);
    }
    let mut q = db.prepare("SELECT provider FROM track_external_identity WHERE track_id=?1 UNION SELECT album_provider FROM manual_track_association WHERE track_id=?1 UNION SELECT album_provider FROM provider_track_association WHERE track_id=?1 ORDER BY 1")?;
    input.association_providers = q
        .query_map([track.as_ref()], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    input.spotify_excluded = db.query_row("SELECT EXISTS(SELECT 1 FROM track_provider_exclusion WHERE track_id=?1 AND provider='spotify')",[track.as_ref()],|r|r.get(0))?;
    Ok(input)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Assessment {
    Unique(usize),
    NeedsSelection,
    NoMatch,
}

fn primary_compatible(input: &Input, c: &Candidate) -> bool {
    match (&input.primary_artist, c.artists.first()) {
        (Some(a), Some(b)) => crate::artist_credit::same_artist(a, b),
        _ => crate::matching::normalize(&input.artist) == crate::matching::normalize(&c.artist),
    }
}
fn credits_compatible(input: &Input, c: &Candidate) -> bool {
    if !primary_compatible(input, c) {
        return false;
    }
    if input.artists.is_empty() || c.artists.is_empty() {
        return !input.artist.trim().is_empty()
            && crate::matching::normalize(&input.artist) == crate::matching::normalize(&c.artist);
    }
    crate::artist_credit::compare(&input.artists, &c.artists)
        == crate::artist_credit::Compatibility::Equivalent
}
fn exact(left: &str, right: &str) -> bool {
    !left.trim().is_empty()
        && crate::matching::normalize_album_title(left)
            == crate::matching::normalize_album_title(right)
}
fn album_compatible(input: &Input, c: &Candidate) -> bool {
    let titles = std::iter::once(input.album.as_str())
        .chain(input.evidence.album_titles.iter().map(|o| o.value.as_str()));
    titles.into_iter().any(|title| exact(title, &c.album))
}
fn same_title(input: &Input, c: &Candidate) -> bool {
    let left = crate::edition::TrackEvidence {
        title: Some(input.title.clone()),
        artists: input.artists.clone(),
        ..Default::default()
    };
    let right = crate::edition::TrackEvidence {
        title: Some(c.title.clone()),
        artists: c.artists.clone(),
        ..Default::default()
    };
    if crate::album_program::title_relation(&left, &right)
        == crate::album_program::TitleRelation::Contradictory
    {
        return false;
    }
    if input.evidence.titles.iter().any(|o| {
        let evidence = crate::edition::TrackEvidence {
            title: Some(o.value.clone()),
            artists: input.artists.clone(),
            ..Default::default()
        };
        crate::album_program::title_relation(&evidence, &right)
            == crate::album_program::TitleRelation::Contradictory
    }) {
        return false;
    }
    std::iter::once(input.title.as_str())
        .chain(input.evidence.titles.iter().map(|o| o.value.as_str()))
        .any(|title| title_agrees(title, &input.artists, c))
}
fn title_agrees(title: &str, artists: &[crate::edition::ArtistEvidence], c: &Candidate) -> bool {
    let left = crate::edition::TrackEvidence {
        title: Some(title.into()),
        artists: artists.to_vec(),
        ..Default::default()
    };
    let right = crate::edition::TrackEvidence {
        title: Some(c.title.clone()),
        artists: c.artists.clone(),
        ..Default::default()
    };
    crate::album_program::title_relation(&left, &right)
        == crate::album_program::TitleRelation::Agrees
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FeasibilityClass {
    Preferred,
    Alternate,
    Infeasible,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Feasibility {
    pub class: FeasibilityClass,
    pub reasons: Vec<&'static str>,
    pub reason_codes: Vec<crate::album_candidates::Reason>,
}
/// Presentation feasibility is weaker than automatic acceptance. No persistence,
/// provider calls, duration-only rejection or invented contributor equivalence.
pub fn feasibility(input: &Input, c: &Candidate) -> Feasibility {
    use crate::album_candidates::Reason;
    let mut reason_codes = vec![];
    let mut reasons = vec![];
    if !primary_compatible(input, c) {
        reasons.push("different primary Artist");
        reason_codes.push(Reason::ArtistMismatch);
    }
    if crate::artist_credit::compare(&input.artists, &c.artists)
        == crate::artist_credit::Compatibility::Contradictory
    {
        reasons.push("contradictory Artist credits");
        reason_codes.push(Reason::ArtistMismatch);
    }
    if !same_title(input, c) {
        reasons.push("different Track title or semantic version");
        reason_codes.push(Reason::TrackTitleMismatch);
    }
    if !input.album.trim().is_empty() && !album_compatible(input, c) {
        reason_codes.push(Reason::AlbumTitleMismatch);
        reasons.push(match c.album_type.as_str() {
            "compilation" | "Compilation" => "compilation outside established Album context",
            "single" | "Single" | "EP" => "Single/EP outside established Album context",
            _ => "different established Album title",
        });
    }
    if input.evidence.release_types.iter().any(|o| {
        !crate::canonical_evidence::release_type_compatible(
            &o.value,
            &c.album_type,
            &c.identity.provider,
        )
    }) {
        reasons.push("known release type conflicts");
        reason_codes.push(Reason::ReleaseTypeMismatch);
    }
    if let (Some(a), Some(b)) = (input.album_artists.first(), c.album_artists.first())
        && !crate::artist_credit::same_artist(a, b)
    {
        reasons.push("different Album Artist");
        reason_codes.push(Reason::ArtistMismatch);
    }
    if !crate::album_candidates::can_accommodate(input.album_required_tracks, c.album_total_tracks)
    {
        reasons.push("Album program too short for established Tracks");
        reason_codes.push(Reason::TrackCountMismatch);
    }
    if input
        .evidence
        .discs
        .iter()
        .any(|o| c.disc > 0 && o.value != c.disc)
        || input
            .evidence
            .positions
            .iter()
            .any(|o| c.number > 0 && o.value != c.number)
        || input
            .disc
            .is_some_and(|n| n > 0 && c.disc > 0 && n != c.disc)
        || input
            .number
            .is_some_and(|n| n > 0 && c.number > 0 && n != c.number)
    {
        reasons.push("different position in established Album");
        reason_codes.push(Reason::PositionMismatch);
    }
    if !reasons.is_empty()
        && input
            .duration_ms
            .is_some_and(|n| n > 0 && c.duration_ms > 0 && n.abs_diff(c.duration_ms) > 30_000)
    {
        reasons.push("dramatically different duration corroborates contradictions");
        reason_codes.push(Reason::DurationConflict);
    }
    let class = if !reasons.is_empty() {
        FeasibilityClass::Infeasible
    } else if input.album_date.is_some()
        && crate::catalog_date::agreement(
            input.album_date,
            crate::catalog_date::Date::parse(&c.date),
        ) == crate::catalog_date::Agreement::UnknownOrDifferent
    {
        FeasibilityClass::Alternate
    } else {
        FeasibilityClass::Preferred
    };
    Feasibility {
        class,
        reasons,
        reason_codes,
    }
}
/// Compare date precision among feasible results only. Equal/unknown dates do
/// not establish a unique winner, and every compatible alternate stays visible.
pub fn classify(input: &Input, candidates: &[Candidate]) -> Vec<Feasibility> {
    let mut assessments: Vec<_> = candidates.iter().map(|c| feasibility(input, c)).collect();
    let agreements: Vec<_> = candidates
        .iter()
        .map(|c| {
            crate::catalog_date::agreement(
                input.album_date,
                crate::catalog_date::Date::parse(&c.date),
            )
        })
        .collect();
    let best = assessments
        .iter()
        .zip(&agreements)
        .filter(|(a, _)| a.class != FeasibilityClass::Infeasible)
        .map(|(_, d)| *d)
        .max();
    for (a, d) in assessments.iter_mut().zip(agreements) {
        if a.class != FeasibilityClass::Infeasible && best.is_some_and(|best| d < best) {
            a.class = FeasibilityClass::Alternate;
        }
    }
    assessments
}
fn duration_compatible(input: &Input, c: &Candidate) -> bool {
    let tolerance = duration_tolerance(c);
    !input
        .duration_ms
        .is_some_and(|d| d > 0 && c.duration_ms > 0 && d.abs_diff(c.duration_ms) > tolerance)
        && !input.evidence.durations.iter().any(|o| {
            o.value > 0 && c.duration_ms > 0 && o.value.abs_diff(c.duration_ms) > tolerance
        })
}
fn duration_tolerance(c: &Candidate) -> u64 {
    if c.identity.provider == "spotify" {
        10_000
    } else {
        3_000
    }
}
fn context_matches(input: &Input, c: &Candidate) -> bool {
    feasibility(input, c).class != FeasibilityClass::Infeasible
        && !input.album.trim().is_empty()
        // Preserve the stricter existing explicit-Play acceptance gate.
        && duration_compatible(input,c)
}
/// A complete bounded page, exact musical title/Album context, equivalent
/// structured credits and no supplied duration/position contradiction. Unknown
/// contributor relationships in another plausible candidate block auto-acceptance.
/// No ranking, fuzzy Artist names or unverified title suffix stripping.
pub fn assess(input: &Input, page: &Page<Candidate>) -> Assessment {
    let plausible: Vec<_> = page
        .items
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            context_matches(input, c)
                && crate::artist_credit::compare(&input.artists, &c.artists)
                    != crate::artist_credit::Compatibility::Contradictory
        })
        .collect();
    if let [(index, candidate)] = plausible.as_slice()
        && page.next_offset.is_none()
        && credits_compatible(input, candidate)
    {
        return Assessment::Unique(*index);
    }
    if page
        .items
        .iter()
        .any(|c| same_title(input, c) && primary_compatible(input, c))
    {
        Assessment::NeedsSelection
    } else {
        Assessment::NoMatch
    }
}

/// A shared, ordered comparison rendered by every diagnostic frontend. All gates
/// below call the same predicates used by `assess`; this is not another matcher.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ComparisonField {
    pub label: &'static str,
    pub local: String,
    pub candidate: String,
    pub status: &'static str,
    pub evidence: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalDecision {
    Accept,
    Reject,
    NeedsReview,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct CandidateEvaluation {
    pub fields: Vec<ComparisonField>,
    pub final_decision: FinalDecision,
    pub provenance: String,
    pub association_persisted: bool,
    pub primary_blocker: Option<String>,
    pub primary_code: Option<&'static str>,
    pub warnings: Vec<String>,
    pub decision: &'static str,
    pub requirements: &'static str,
    pub artist_equivalence_available: bool,
}
fn shown(v: &str) -> String {
    if v.trim().is_empty() {
        "—".into()
    } else {
        v.into()
    }
}
fn length(v: u64) -> String {
    if v == 0 {
        "—".into()
    } else {
        format!("{}:{:02}", v / 60000, (v / 1000) % 60)
    }
}
/// Song occurrence acceptance and Album reconciliation are distinct decisions.
/// A qualifying song result never implies equivalent Release/Recording identity.
pub fn evaluate(
    input: &Input,
    page: &Page<Candidate>,
    index: usize,
    trusted: &[ExternalIdentity],
) -> Option<CandidateEvaluation> {
    let c = page.items.get(index)?;
    let f = feasibility(input, c);
    let accepted = assess(input, page) == Assessment::Unique(index);
    let plausible = page
        .items
        .iter()
        .filter(|c| {
            context_matches(input, c)
                && crate::artist_credit::compare(&input.artists, &c.artists)
                    != crate::artist_credit::Compatibility::Contradictory
        })
        .count();
    let known_duration = input
        .duration_ms
        .or_else(|| input.evidence.durations.first().map(|o| o.value));
    let duration_ok = duration_compatible(input, c);
    let conflict = trusted.iter().any(|id| {
        id.provider == c.identity.provider && id.kind == c.identity.kind && id != &c.identity
    });
    let existing = input.association_providers.contains(&c.identity.provider);
    let invalid_identity = c.identity.provider.is_empty()
        || c.identity.kind.is_empty()
        || c.identity.external_id.is_empty()
        || c.identity.kind == "recording"
        || c.identity.provider == "spotify" && c.identity.kind != "track";
    let blocker = if input.spotify_excluded && c.identity.provider == "spotify" {
        Some(("manually_excluded", "Manually marked not on Spotify; choose Check Spotify again to re-enable reconciliation.".into()))
    } else if invalid_identity {
        Some((
            "invalid_candidate_identity",
            "Candidate does not supply a valid provider Track occurrence identity.".into(),
        ))
    } else if conflict {
        Some((
            "trusted_identity_conflict",
            "Existing trusted provider identity points to a different Track.".into(),
        ))
    } else if existing && !trusted.contains(&c.identity) {
        Some(("existing_provider_association", "An existing provider association must be reviewed before replacement; it may belong to an earlier Album context.".into()))
    } else if let Some(reason) = f.reasons.first() {
        Some((
            match f.reason_codes.first() {
                Some(crate::album_candidates::Reason::ArtistMismatch) => "artist_mismatch",
                Some(crate::album_candidates::Reason::PositionMismatch) => "position_mismatch",
                Some(crate::album_candidates::Reason::TrackCountMismatch) => "track_count_mismatch",
                Some(crate::album_candidates::Reason::TrackTitleMismatch) => "track_title_mismatch",
                Some(crate::album_candidates::Reason::AlbumTitleMismatch) => "album_title_mismatch",
                Some(crate::album_candidates::Reason::ReleaseTypeMismatch) => {
                    "release_type_mismatch"
                }
                _ => "structural_conflict",
            },
            if f.reason_codes.first() == Some(&crate::album_candidates::Reason::ArtistMismatch) {
                "Artist identities are not established as equivalent.".into()
            } else {
                format!("Candidate conflicts with established context: {reason}.")
            },
        ))
    } else if input.album.trim().is_empty() {
        Some((
            "album_context_missing",
            "Automatic matching requires an established Album title.".into(),
        ))
    } else if !duration_ok {
        Some((
            "duration_threshold",
            "Duration difference exceeds the 3-second automatic acceptance tolerance.".into(),
        ))
    } else if page.next_offset.is_some() {
        Some((
            "incomplete_candidate_page",
            "More provider results exist; uniqueness is not established on this bounded page."
                .into(),
        ))
    } else if plausible > 1 {
        Some(("competing_candidates","Another candidate has comparable compatible evidence; no unique candidate is established.".into()))
    } else if !credits_compatible(input, c) {
        Some((
            "artist_credit_incomplete",
            "The complete ordered Artist credits are not established as equivalent.".into(),
        ))
    } else {
        None
    };
    let mut warnings: Vec<String> = f.reasons.iter().skip(1).map(|r| (*r).into()).collect();
    if input
        .evidence
        .titles
        .iter()
        .any(|o| !title_agrees(&o.value, &input.artists, c))
    {
        warnings.push("Persisted title observations disagree; source values are shown under Evidence sources.".into());
    }
    if input
        .evidence
        .dates
        .iter()
        .filter_map(|o| crate::catalog_date::Date::parse(&o.value))
        .any(|d| {
            crate::catalog_date::Date::parse(&c.date).is_some_and(|other| d.year != other.year)
        })
    {
        warnings.push("Known release-date observations conflict with the candidate.".into());
    }
    if let Some(d) = known_duration.filter(|d| *d > 0 && c.duration_ms > 0 && *d != c.duration_ms) {
        warnings.push(format!(
            "Duration differs by {:.1} seconds (automatic tolerance: 3 seconds)",
            d.abs_diff(c.duration_ms) as f64 / 1000.0
        ));
    }
    if input.album_date.is_some()
        && crate::catalog_date::Date::parse(&c.date).is_some()
        && crate::catalog_date::agreement(
            input.album_date,
            crate::catalog_date::Date::parse(&c.date),
        ) == crate::catalog_date::Agreement::UnknownOrDifferent
    {
        warnings.push(
            "Known release dates differ; date alone does not reject a song candidate.".into(),
        );
    }
    let mut fields = vec![];
    let mut field =
        |label, local: String, candidate: String, ok: bool, normalized: bool, evidence: String| {
            fields.push(ComparisonField {
                label,
                local: shown(&local),
                candidate: shown(&candidate),
                status: if !ok {
                    "conflict"
                } else if normalized {
                    "normalized"
                } else {
                    "equivalent"
                },
                evidence,
            });
        };
    field(
        "Song",
        input.title.clone(),
        c.title.clone(),
        same_title(input, c),
        input.title != c.title,
        "Musical title and semantic version comparison".into(),
    );
    field(
        "Artist credit",
        input.artist.clone(),
        c.artist.clone(),
        credits_compatible(input, c),
        input.artist != c.artist
            && crate::matching::normalize_album_title(&input.artist)
                == crate::matching::normalize_album_title(&c.artist),
        if primary_compatible(input, c) {
            "Primary Artist compatible; complete ordered credits checked"
        } else {
            "Artist identities are not established as equivalent"
        }
        .into(),
    );
    field(
        "Album",
        input.album.clone(),
        c.album.clone(),
        album_compatible(input, c),
        input.album != c.album,
        "Established Album title comparison".into(),
    );
    fields.push(ComparisonField {
        label: "Release type",
        local: "—".into(),
        candidate: shown(&c.album_type),
        status: "unknown",
        evidence: "No known release type; missing evidence is neutral".into(),
    });
    if let Some(field) = fields.last_mut() {
        let types: Vec<_> = input
            .evidence
            .release_types
            .iter()
            .map(|o| o.value.as_str())
            .collect();
        if !types.is_empty() {
            field.local = types.join(" / ");
            field.status = if c.album_type.is_empty() {
                "unknown"
            } else if types
                .iter()
                .all(|t| crate::matching::normalize(t) == crate::matching::normalize(&c.album_type))
            {
                "equivalent"
            } else if types.iter().all(|t| {
                crate::canonical_evidence::release_type_compatible(
                    t,
                    &c.album_type,
                    &c.identity.provider,
                )
            }) {
                "normalized"
            } else {
                "conflict"
            };
            field.evidence = "Persisted provider release types; Album program controls representational compatibility".into();
        }
    }
    for (label, a, b) in [
        (
            "Disc",
            input
                .disc
                .or_else(|| input.evidence.discs.first().map(|o| o.value)),
            c.disc,
        ),
        (
            "Track",
            input
                .number
                .or_else(|| input.evidence.positions.first().map(|o| o.value)),
            c.number,
        ),
    ] {
        fields.push(ComparisonField{label,local:a.filter(|n|*n>0).map(|n|n.to_string()).unwrap_or("—".into()),candidate:if b==0{"—".into()}else{b.to_string()},status:if a.is_some_and(|n|n>0 && b>0 && n!=b){"conflict"}else if a.is_none()||a==Some(0)||b==0{"unknown"}else{"equivalent"},evidence:if label=="Disc"{"Missing/zero disc is unknown in song lookup; Album program normalizes missing disc to 1"}else{"Supplied positions must not contradict the established Track position"}.into()});
    }
    fields.push(ComparisonField {
        label: "Track count",
        local: format!("at least {}", input.album_required_tracks),
        candidate: c
            .album_total_tracks
            .map(|n| n.to_string())
            .unwrap_or("—".into()),
        status: if crate::album_candidates::can_accommodate(
            input.album_required_tracks,
            c.album_total_tracks,
        ) {
            "equivalent"
        } else {
            "conflict"
        },
        evidence: "Provider program must accommodate established positions (not just saved Tracks)"
            .into(),
    });
    fields.push(ComparisonField {
        label: "Duration",
        local: length(known_duration.unwrap_or(0)),
        candidate: length(c.duration_ms),
        status: if !duration_ok {
            "conflict"
        } else if known_duration.is_none() || c.duration_ms == 0 {
            "unknown"
        } else {
            "equivalent"
        },
        evidence: format!(
            "Automatic song acceptance tolerance: {} seconds; unknown duration is not a conflict",
            duration_tolerance(c) / 1000
        ),
    });
    fields.push(ComparisonField {
        label: "Release date",
        local: input
            .album_date
            .map(|d| {
                format!(
                    "{:04}{}{}",
                    d.year,
                    d.month.map(|m| format!("-{m:02}")).unwrap_or_default(),
                    d.day.map(|day| format!("-{day:02}")).unwrap_or_default()
                )
            })
            .unwrap_or_else(|| {
                if input.evidence.dates.is_empty() {
                    "—".into()
                } else {
                    input
                        .evidence
                        .dates
                        .iter()
                        .map(|o| o.value.clone())
                        .collect::<std::collections::BTreeSet<_>>()
                        .into_iter()
                        .collect::<Vec<_>>()
                        .join(" / ")
                }
            }),
        candidate: shown(&c.date),
        status: if input.album_date.is_none()
            && input
                .evidence
                .dates
                .iter()
                .filter_map(|o| crate::catalog_date::Date::parse(&o.value))
                .count()
                > 1
        {
            "warning"
        } else if input.album_date.is_none() || crate::catalog_date::Date::parse(&c.date).is_none()
        {
            "unknown"
        } else if crate::catalog_date::agreement(
            input.album_date,
            crate::catalog_date::Date::parse(&c.date),
        ) == crate::catalog_date::Agreement::UnknownOrDifferent
        {
            "warning"
        } else {
            "equivalent"
        },
        evidence: "Date precision ranks presentation only; it does not establish identity".into(),
    });
    fields.push(ComparisonField {
        label: "Trusted external IDs",
        local: shown(
            &trusted
                .iter()
                .map(|i| format!("{} {}: {}", i.provider, i.kind, i.external_id))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        candidate: format!(
            "{} {}: {}",
            c.identity.provider, c.identity.kind, c.identity.external_id
        ) + &c
            .album_identity
            .as_ref()
            .map(|i| format!("\n{} {}: {}", i.provider, i.kind, i.external_id))
            .unwrap_or_default(),
        status: if conflict { "conflict" } else { "warning" },
        evidence: "Existing trusted identities are preserved; replacement requires separate review"
            .into(),
    });
    fields.push(ComparisonField {
        label: "Artist IDs",
        local: shown(
            &input
                .artists
                .iter()
                .flat_map(|a| a.identities.iter())
                .map(|i| format!("{}: {}", i.provider, i.external_id))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        candidate: shown(
            &c.artists
                .iter()
                .flat_map(|a| a.identities.iter())
                .map(|i| format!("{}: {}", i.provider, i.external_id))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        status: if primary_compatible(input, c) {
            "equivalent"
        } else {
            "conflict"
        },
        evidence: "Includes identities reachable through explicitly confirmed Artist equivalence"
            .into(),
    });
    // Show the observation that actually corroborated the candidate, while
    // keeping the unchanged display value and every source available below.
    if !title_agrees(&input.title, &input.artists, c)
        && let Some(o) = input
            .evidence
            .titles
            .iter()
            .find(|o| title_agrees(&o.value, &input.artists, c))
        && let Some(field) = fields.iter_mut().find(|f| f.label == "Song")
    {
        field.local = format!("{}\nDisplay: {}", o.value, input.title);
        field.evidence="Candidate agrees with a persisted title observation; differing display/local values are retained under Evidence sources".into();
        field.status = "equivalent";
    }
    if !exact(&input.album, &c.album)
        && let Some(o) = input
            .evidence
            .album_titles
            .iter()
            .find(|o| exact(&o.value, &c.album))
        && let Some(field) = fields.iter_mut().find(|f| f.label == "Album")
    {
        field.local = format!("{}\nDisplay: {}", o.value, input.album);
        field.evidence="Candidate agrees with a persisted Album observation; differing display/local values are retained under Evidence sources".into();
        field.status = "equivalent";
    }
    let final_decision = if blocker.is_none() && accepted {
        FinalDecision::Accept
    } else if invalid_identity || conflict || f.class == FeasibilityClass::Infeasible {
        FinalDecision::Reject
    } else {
        FinalDecision::NeedsReview
    };
    Some(CandidateEvaluation {
        fields,
        final_decision,
        provenance: input.evidence.annotations(),
        association_persisted: trusted.contains(&c.identity),
        artist_equivalence_available: !primary_compatible(input, c)
            && input.primary_artist.is_some()
            && c.artists.first().is_some_and(|a| a.identities.len() == 1),
        primary_code: blocker.as_ref().map(|b| b.0),
        primary_blocker: blocker.map(|b| b.1),
        warnings,
        decision: match final_decision {
            FinalDecision::Accept if trusted.contains(&c.identity) => {
                "Connected: trusted association persisted"
            }
            FinalDecision::Accept => "Accept: unique compatible candidate",
            FinalDecision::Reject => "Rejected: automatic association withheld",
            FinalDecision::NeedsReview => "Needs review: automatic association withheld",
        },
        requirements: "Automatic song acceptance requires a complete bounded page, one compatible candidate, established Album title, equivalent ordered Artist credits, and no supplied position/duration conflict. Album reconciliation additionally checks program structure and anchors.",
    })
}

/// Current evidence may explain fields, but a candidate page discovered for a
/// different snapshot cannot establish uniqueness for the new context.
pub fn evaluate_attempt(
    snapshot: &Input,
    current: &Input,
    page: &Page<Candidate>,
    index: usize,
    trusted: &[ExternalIdentity],
) -> Option<CandidateEvaluation> {
    let mut evaluation = evaluate(current, page, index, trusted)?;
    if snapshot != current
        && !evaluation.association_persisted
        && evaluation.final_decision == FinalDecision::Accept
    {
        evaluation.final_decision = FinalDecision::NeedsReview;
        evaluation.primary_code = Some("evidence_changed");
        evaluation.primary_blocker = Some(
            "Known evidence changed since this search; re-evaluate with current evidence.".into(),
        );
        evaluation.decision = "Needs review: candidate search snapshot is stale";
    }
    Some(evaluation)
}
fn persist_occurrence(db: &rusqlite::Connection, track: &TrackId, c: &Candidate) -> Result<()> {
    db.execute("INSERT INTO track_external_identity(track_id,provider,kind,external_id) VALUES(?1,?2,?3,?4)",params![track.as_ref(),c.identity.provider,c.identity.kind,c.identity.external_id])?;
    crate::storage::observe_duration(
        db,
        track,
        &c.identity.provider,
        &c.identity.external_id,
        crate::catalog::Duration {
            milliseconds: c.duration_ms,
            approximate: false,
        },
        0,
    )?;
    let persisted: bool=db.query_row("SELECT EXISTS(SELECT 1 FROM track_external_identity WHERE track_id=?1 AND provider=?2 AND kind=?3 AND external_id=?4)",params![track.as_ref(),c.identity.provider,c.identity.kind,c.identity.external_id],|r|r.get(0))?;
    if c.identity.provider == "spotify" {
        db.execute("UPDATE spotify_connection_review SET reason_code='already_associated',reason='Connected to Spotify' WHERE track_id=?1",[track.as_ref()])?;
    }
    if !persisted {
        return Err(Error::Invalid(
            "Database did not retain the accepted association".into(),
        ));
    }
    Ok(())
}
impl Store {
    pub fn song_resolution_input(&self, track: &TrackId) -> Result<Input> {
        load_input(&self.connection, track)
    }
    pub fn apply_song_evaluation(
        &mut self,
        input: &Input,
        page: &Page<Candidate>,
    ) -> Result<Option<ExternalIdentity>> {
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let current = load_input(&tx, &input.track_id)?;
        if current != *input {
            return Err(Error::ReconciliationEvidenceChanged);
        }
        let trusted = {
            let mut q = tx.prepare(
                "SELECT provider,kind,external_id FROM track_external_identity WHERE track_id=?1",
            )?;
            q.query_map([current.track_id.as_ref()], |r| {
                Ok(ExternalIdentity {
                    provider: r.get(0)?,
                    kind: r.get(1)?,
                    external_id: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let candidate = page
            .items
            .iter()
            .enumerate()
            .find(|(index, _)| {
                evaluate_attempt(input, &current, page, *index, &trusted)
                    .is_some_and(|e| e.final_decision == FinalDecision::Accept)
            })
            .map(|(_, c)| c);
        if let Some(c) = candidate {
            if !trusted.contains(&c.identity) {
                persist_occurrence(&tx, &current.track_id, c)
                    .map_err(|e| Error::AssociationPersistenceFailed(Box::new(e)))?;
            }
            tx.commit()
                .map_err(|e| Error::AssociationPersistenceFailed(Box::new(Error::Database(e))))?;
            return Ok(Some(c.identity.clone()));
        }
        tx.commit()?;
        Ok(None)
    }
    pub fn confirm_song_resolution(
        &mut self,
        selection: &Selection,
        index: usize,
    ) -> Result<ExternalIdentity> {
        let candidate = selection
            .candidates
            .get(index)
            .ok_or_else(|| Error::Invalid("Explicitly select a catalog song first".into()))?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if load_input(&tx, &selection.input.track_id)? != selection.input {
            return Err(Error::Invalid(
                "Track metadata changed; search again before confirming".into(),
            ));
        }
        // This first path adds only an absent association. Replacing independent
        // identities needs explicit correction semantics and is deliberately deferred.
        let existing: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM manual_track_association WHERE track_id=?1 AND album_provider=?2) OR EXISTS(SELECT 1 FROM provider_track_association WHERE track_id=?1 AND album_provider=?2) OR EXISTS(SELECT 1 FROM track_external_identity WHERE track_id=?1 AND provider=?2)",params![selection.input.track_id.as_ref(),candidate.identity.provider],|r|r.get(0))?;
        if existing {
            return Err(Error::Invalid(
                "This Track already has a provider association; replacement is not supported here"
                    .into(),
            ));
        }
        if candidate.identity.kind == "recording"
            || candidate.identity.provider == "spotify" && candidate.identity.kind != "track"
        {
            return Err(Error::Invalid(
                "Select a provider Track occurrence, not a Recording identity".into(),
            ));
        }
        persist_occurrence(&tx, &selection.input.track_id, candidate)?;
        tx.commit()?;
        Ok(candidate.identity.clone())
    }
}
