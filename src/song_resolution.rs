//! Bounded Track occurrence evaluation for diagnostics and Play. Release-aware
//! matching and best-available playback are separate; neither infers Recording
//! identity or canonical Release equivalence.
use crate::{
    catalog::{CatalogError, Page},
    domain::*,
    storage::{Error, Result, Store},
};
use rusqlite::params;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Input {
    #[serde(default)]
    pub recording: crate::edition::RecordingEvidence,
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
    #[serde(default)]
    pub recording: crate::edition::RecordingEvidence,
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
    album_candidates: Vec<ExternalIdentity>,
    manual_attachment: bool,
    trusted: Vec<ExternalIdentity>,
}
impl Selection {
    pub fn new(input: Input, candidates: Vec<Candidate>) -> Self {
        let assessments = classify(&input, &candidates);
        Self {
            input,
            candidates,
            show_all: false,
            assessments,
            album_candidates: vec![],
            manual_attachment: false,
            trusted: vec![],
        }
    }
    pub fn show_all(&mut self) {
        self.show_all = true;
    }
    pub fn set_trusted_identities(&mut self, trusted: Vec<ExternalIdentity>) {
        self.trusted = trusted;
    }
    pub fn inherit_album_candidates(&mut self, candidates: Vec<Candidate>) {
        for candidate in candidates {
            if !self.album_candidates.contains(&candidate.identity) {
                self.album_candidates.push(candidate.identity.clone());
            }
            if !self
                .candidates
                .iter()
                .any(|existing| existing.identity == candidate.identity)
            {
                self.candidates.push(candidate);
            }
        }
        self.assessments = classify(&self.input, &self.candidates);
    }
    pub fn mark_manual_attachment(&mut self) {
        self.manual_attachment = true;
    }
    pub fn candidate_provenance(&self, index: usize) -> &'static str {
        if self.manual_attachment {
            "Manual attachment"
        } else if self
            .candidates
            .get(index)
            .is_some_and(|c| self.album_candidates.contains(&c.identity))
        {
            "Found through Album candidate"
        } else {
            "Found through Track search"
        }
    }
    pub fn visible_indices(&self) -> Vec<usize> {
        let mut rows: Vec<_> = self
            .assessments
            .iter()
            .enumerate()
            .map(|(i, a)| (i, a.class))
            .filter(|(i, class)| {
                self.show_all
                    || *class != FeasibilityClass::Infeasible
                    || playable_eligible(&self.input, &self.candidates[*i])
            })
            .collect();
        let page = Page {
            items: self.candidates.clone(),
            next_offset: None,
        };
        let score = |i: usize| {
            let c = &self.candidates[i];
            let decision = evaluate_attempt(&self.input, &self.input, &page, i, &self.trusted)
                .map(|e| e.final_decision);
            let class = match decision {
                Some(FinalDecision::Accept) => 2,
                Some(FinalDecision::NeedsReview) => 1,
                _ => 0,
            };
            (
                class,
                std::cmp::Reverse(self.assessments[i].class),
                title_agrees(&self.input.title, &self.input.artists, c),
                credits_compatible(&self.input, c),
                crate::matching::normalize(&self.input.album)
                    == crate::matching::normalize(&c.album),
                ranking_title_overlap(&self.input.title, &c.title),
                duration_compatible(&self.input, c),
                self.input.number == Some(c.number),
            )
        };
        rows.sort_by(|(a, _), (b, _)| {
            score(*b).cmp(&score(*a)).then_with(|| {
                self.candidates[*a]
                    .identity
                    .external_id
                    .cmp(&self.candidates[*b].identity.external_id)
            })
        });
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
// Ranking only: lexical overlap can order rejected/review candidates, never
// establish identity or relax automatic acceptance.
fn ranking_title_overlap(a: &str, b: &str) -> usize {
    let grams = |s: &str| {
        let chars = crate::album_program::comparison_title(s)
            .chars()
            .take(256)
            .collect::<Vec<_>>();
        chars
            .windows(2)
            .map(|p| (p[0], p[1]))
            .collect::<std::collections::BTreeSet<_>>()
    };
    let (a, b) = (grams(a), grams(b));
    a.intersection(&b).count() * 100 / a.union(&b).count().max(1)
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
            [track.as_ref()], |r| Ok((Input { recording:Default::default(), track_id: track.clone(), spotify_excluded:false, evidence:Default::default(), association_providers:vec![], album_date:None, album_artists:vec![], album_required_tracks:0, primary_artist:None, artists:vec![], title:r.get(0)?, artist:r.get(1)?, album:r.get(2)?, duration_ms:r.get::<_,Option<i64>>(4)?.and_then(|v| u64::try_from(v).ok()), disc:r.get(5)?, number:r.get(6)? },r.get::<_,String>(3)?)))?;
    input.recording.identities = db.prepare("SELECT i.provider,i.kind,i.external_id FROM recording_external_identity i JOIN track t ON t.recording_id=i.recording_id WHERE t.id=?1")?
        .query_map([track.as_ref()], |r| Ok(ExternalIdentity { provider:r.get(0)?, kind:r.get(1)?, external_id:r.get(2)? }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    input.recording.isrcs = input
        .recording
        .identities
        .iter()
        .filter(|id| id.provider == "isrc")
        .map(|id| id.external_id.clone())
        .collect();
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
/// Evidence dominance is a partial order, never a provider relevance score.
/// Comparable but distinct occurrences remain ambiguous. Stronger Artist credits
/// may win only when every supplied structural observation is at least as strong.
fn dominates(input: &Input, a: &Candidate, b: &Candidate) -> bool {
    let evidence = |c: &Candidate| {
        [
            credits_compatible(input, c),
            crate::artist_credit::compare(&input.album_artists, &c.album_artists)
                == crate::artist_credit::Compatibility::Equivalent,
            input.title.trim() == c.title.trim(),
            input.album.trim() == c.album.trim(),
            input.disc.is_some_and(|d| d == c.disc),
            input.number.is_some_and(|n| n == c.number),
        ]
    };
    // Replacement contributors are unresolved identity conflicts, unlike an
    // extra/omitted credit. They cannot be dismissed by exact primary credit.
    if crate::artist_credit::compare(&input.artists, &b.artists)
        == crate::artist_credit::Compatibility::DifferentAdditional
    {
        return false;
    }
    let left = evidence(a);
    let right = evidence(b);
    left.iter().zip(right).all(|(a, b)| *a || !b)
        && left.iter().zip(right).any(|(a, b)| *a && !b)
        && crate::catalog_date::agreement(
            input.album_date,
            crate::catalog_date::Date::parse(&a.date),
        ) >= crate::catalog_date::agreement(
            input.album_date,
            crate::catalog_date::Date::parse(&b.date),
        )
}

fn playable_title_base(title: &str) -> String {
    let title = title.trim();
    let annotation = title
        .strip_suffix(')')
        .and_then(|t| t.rsplit_once(" ("))
        .or_else(|| title.rsplit_once(" - "));
    if let Some((base, suffix)) = annotation {
        let normalized = crate::matching::normalize(suffix);
        let words: Vec<_> = normalized.split_whitespace().collect();
        let version = words.iter().any(|w| {
            matches!(
                *w,
                "live" | "acoustic" | "demo" | "remaster" | "remastered" | "remix" | "edit"
            )
        });
        if version
            && words.iter().all(|w| {
                matches!(
                    *w,
                    "live"
                        | "acoustic"
                        | "demo"
                        | "remaster"
                        | "remastered"
                        | "remix"
                        | "radio"
                        | "edit"
                        | "version"
                ) || (w.len() == 4 && w.chars().all(|c| c.is_ascii_digit()))
            })
        {
            return crate::matching::normalize_album_title(base);
        }
    }
    crate::matching::normalize_album_title(title)
}
fn playable_title_matches(input: &Input, c: &Candidate) -> bool {
    same_title(input, c)
        || (!input.title.trim().is_empty()
            && playable_title_base(&input.title) == playable_title_base(&c.title)
            && input
                .evidence
                .titles
                .iter()
                .all(|o| playable_title_base(&o.value) == playable_title_base(&c.title)))
}
fn equivalent_playable(a: &Candidate, b: &Candidate) -> bool {
    let recording = a
        .recording
        .identities
        .iter()
        .any(|id| b.recording.identities.contains(id))
        || a.recording
            .isrcs
            .iter()
            .any(|id| b.recording.isrcs.contains(id));
    recording
        && exact(&a.title, &b.title)
        && a.duration_ms > 0
        && b.duration_ms > 0
        && a.duration_ms.abs_diff(b.duration_ms) <= 100
        && crate::artist_credit::compare(&a.artists, &b.artists)
            == crate::artist_credit::Compatibility::Equivalent
}
fn recording_conflicts(input: &Input, c: &Candidate) -> bool {
    input.recording.identities.iter().any(|known| {
        c.recording.identities.iter().any(|other| {
            known.provider == other.provider && known.kind == other.kind && known != other
        })
    }) || (!input.recording.isrcs.is_empty()
        && !c.recording.isrcs.is_empty()
        && !input
            .recording
            .isrcs
            .iter()
            .any(|id| c.recording.isrcs.contains(id)))
}
pub fn established_playable_artist(input: &Input) -> Option<&crate::edition::ArtistEvidence> {
    let primary = input.primary_artist.as_ref()?;
    if !primary.identities.is_empty() {
        return Some(primary);
    }
    match input.album_artists.as_slice() {
        [artist]
            if !artist.identities.is_empty()
                && crate::artist_credit::same_artist(primary, artist) =>
        {
            Some(artist)
        }
        _ => None,
    }
}
/// Playback-only eligibility across releases. Different release titles/types/
/// positions do not veto it. Trusted Recording/Artist contradictions stay guarded.
pub(crate) fn playable_eligible(input: &Input, c: &Candidate) -> bool {
    c.identity.provider == "spotify"
        && c.identity.kind == "track"
        && !c.identity.external_id.is_empty()
        && established_playable_artist(input).is_some_and(|artist| {
            c.artists.iter().any(|credit| {
                artist
                    .identities
                    .iter()
                    .any(|id| credit.identities.contains(id))
                    && crate::artist_credit::same_artist(artist, credit)
            })
        })
        && (!matches!(
            crate::artist_credit::compare(&input.artists, &c.artists),
            crate::artist_credit::Compatibility::Contradictory
                | crate::artist_credit::Compatibility::DifferentAdditional
        ) || (!input.artists.is_empty()
            && input.artists.iter().all(|known| {
                c.artists
                    .iter()
                    .any(|credit| crate::artist_credit::same_artist(known, credit))
            })))
        && !recording_conflicts(input, c)
        && playable_title_matches(input, c)
        && duration_compatible(input, c)
}

/// Select a best available playable occurrence without asserting Album identity.
/// Distinct occurrences remain ambiguous unless one has strictly stronger title
/// and credit evidence. Provider ordering never breaks a confidence tie.
pub fn assess_playable(input: &Input, page: &Page<Candidate>) -> Assessment {
    if input.spotify_excluded || page.next_offset.is_some() {
        return Assessment::NeedsSelection;
    }
    let viable: Vec<_> = page
        .items
        .iter()
        .enumerate()
        .filter(|(_, c)| playable_eligible(input, c))
        .collect();
    let score = |c: &Candidate| (exact(&input.title, &c.title), credits_compatible(input, c));
    let best = viable.iter().map(|(_, c)| score(c)).max();
    let mut strongest: Vec<_> = viable
        .into_iter()
        .filter(|(_, c)| Some(score(c)) == best)
        .collect();
    strongest.sort_by(|(_, a), (_, b)| a.identity.external_id.cmp(&b.identity.external_id));
    let mut distinct: Vec<(usize, &Candidate)> = vec![];
    for item in strongest {
        if !distinct.iter().any(|(_, other)| {
            item.1.identity == other.identity || equivalent_playable(item.1, other)
        }) {
            distinct.push(item);
        }
    }
    let strongest = distinct;
    match strongest.as_slice() {
        [(index, _)] => Assessment::Unique(*index),
        [] => Assessment::NoMatch,
        _ => Assessment::NeedsSelection,
    }
}

/// Evaluate the complete bounded set before choosing its sole undominated,
/// fully compatible occurrence. Provider ordering has no confidence semantics.
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
    let strongest: Vec<_> = plausible
        .iter()
        .filter(|(_, c)| {
            !plausible
                .iter()
                .any(|(_, other)| dominates(input, other, c))
        })
        .collect();
    if let [(index, candidate)] = strongest.as_slice()
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
    let strict_accepted = assess(input, page) == Assessment::Unique(index);
    let playable = assess_playable(input, page);
    let fallback = !matches!(assess(input, page), Assessment::Unique(_))
        && playable == Assessment::Unique(index);
    let accepted = strict_accepted || fallback;
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
    let trusted_here = trusted.contains(&c.identity) && !invalid_identity && !conflict;
    let blocker = if trusted_here {
        None
    } else if input.spotify_excluded && c.identity.provider == "spotify" {
        Some(("manually_excluded", "Manually marked not on Spotify; choose Check Spotify again to re-enable reconciliation.".into()))
    } else if invalid_identity {
        Some((
            "invalid_candidate_identity",
            "Candidate does not supply a valid provider Track occurrence identity.".into(),
        ))
    } else if recording_conflicts(input, c) {
        Some((
            "recording_identity_conflict",
            "Trusted Recording identities contradict this provider occurrence.".into(),
        ))
    } else if conflict {
        Some((
            "trusted_identity_conflict",
            "Existing trusted provider identity points to a different Track.".into(),
        ))
    } else if existing && !trusted.contains(&c.identity) {
        Some(("existing_provider_association", "An existing provider association must be reviewed before replacement; it may belong to an earlier Album context.".into()))
    } else if accepted {
        None
    } else if playable_eligible(input, c) && page.next_offset.is_none() {
        Some((
            "competing_candidates",
            "Multiple playable occurrences have comparable evidence; review the alternatives."
                .into(),
        ))
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
    } else if let Assessment::Unique(other) = assess(input, page)
        && other != index
        && dominates(input, &page.items[other], c)
    {
        Some(("stronger_candidate","Another candidate has stronger established Artist/title evidence with no weaker Album, position or date evidence.".into()))
    } else if !accepted && plausible > 1 {
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
    if fallback {
        if crate::artist_credit::compare(&input.artists, &c.artists)
            != crate::artist_credit::Compatibility::Equivalent
        {
            warnings.push("Established Artist is present; additional, omitted or differently ordered provider credits are retained.".into());
        }
        if !same_title(input, c) {
            warnings.push("Provider Track version differs; accepted for playback only, not Recording identity.".into());
        }
        warnings.push(format!("Accepted as best available Spotify Track; canonical Release unchanged. Local album: {}; Spotify album: {}.", input.album, c.album));
        warnings.extend(
            f.reasons
                .iter()
                .map(|reason| format!("Playable-source contextual difference: {reason}")),
        );
    }
    if trusted_here {
        warnings.extend(
            f.reasons
                .iter()
                .take(1)
                .map(|r| format!("Existing trusted association retained: {r}")),
        );
    }
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
            "Duration differs by {:.1} seconds (automatic tolerance: {} seconds)",
            d.abs_diff(c.duration_ms) as f64 / 1000.0,
            duration_tolerance(c) / 1000
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
        fields.push(ComparisonField{label,local:a.filter(|n|*n>0).map(|n|n.to_string()).unwrap_or("—".into()),candidate:if b==0{"—".into()}else{b.to_string()},status:if a.is_some_and(|n|n>0 && b>0 && n!=b){"conflict"}else if a.is_none()||a==Some(0)||b==0{"unknown"}else{"equivalent"},evidence:if label=="Disc"{"Missing/zero disc is unknown in song lookup; Album program normalizes missing disc to 1"}else{"Canonical position is comparison evidence; a different provider edition may supply playback"}.into()});
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
        evidence: "Provider count is edition evidence; Track playback does not identify the canonical Release"
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
    if fallback {
        for field in &mut fields {
            if matches!(
                field.label,
                "Album" | "Release type" | "Disc" | "Track" | "Track count" | "Release date"
            ) && field.status == "conflict"
            {
                field.status = "warning";
                field.evidence = "Different provider release context; accepted for playback without changing the canonical Release".into();
            }
            if matches!(field.label, "Artist credit" | "Artist IDs") && field.status == "conflict" {
                field.status = "warning";
                field.evidence = "Established Artist identity is present; provider credit order and additional credits are retained".into();
            }
            if field.label == "Song" && !same_title(input, c) {
                field.status = "warning";
                field.evidence = "Same musical title with an explicit provider version difference; playback-only acceptance".into();
            }
        }
    }
    let final_decision = if blocker.is_none() && (accepted || trusted_here) {
        FinalDecision::Accept
    } else if invalid_identity
        || conflict
        || (f.class == FeasibilityClass::Infeasible && !playable_eligible(input, c))
    {
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
            FinalDecision::Accept if fallback => "Accepted as best available Spotify Track",
            FinalDecision::Accept => "Accept: unique compatible candidate",
            FinalDecision::Reject => "Rejected: automatic association withheld",
            FinalDecision::NeedsReview => "Needs review: automatic association withheld",
        },
        requirements: "Release reconciliation checks Album identity and program structure. Playback-only fallback requires a complete bounded candidate set, established Artist identity, exact musical title or an explicit version qualifier, one strongest occurrence, compatible duration and no trusted Recording conflict. Different provider releases do not change the canonical Release.",
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
pub(crate) fn persist_occurrence(
    db: &rusqlite::Connection,
    track: &TrackId,
    c: &Candidate,
) -> Result<()> {
    db.execute("INSERT INTO track_external_identity(track_id,provider,kind,external_id) VALUES(?1,?2,?3,?4)",params![track.as_ref(),c.identity.provider,c.identity.kind,c.identity.external_id])?;
    if c.duration_ms > 0 {
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
    }
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
fn clear_spotify_occurrences(db: &rusqlite::Connection, track: &TrackId) -> Result<()> {
    for table in ["manual_track_association", "provider_track_association"] {
        db.execute(
            &format!("DELETE FROM {table} WHERE track_id=?1 AND album_provider='spotify'"),
            [track.as_ref()],
        )?;
    }
    db.execute(
        "DELETE FROM track_external_identity WHERE track_id=?1 AND provider='spotify'",
        [track.as_ref()],
    )?;
    db.execute("UPDATE spotify_connection_review SET state='needs_retry',reason_code='not_evaluated',reason='Spotify connection cleared; review or retry',stale=0 WHERE track_id=?1",[track.as_ref()])?;
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
    /// Explicit correction changes only Spotify playable associations. The
    /// snapshot guard prevents a late dialog from replacing newer user intent.
    pub fn replace_spotify_song_resolution(
        &mut self,
        selection: &Selection,
        index: usize,
    ) -> Result<ExternalIdentity> {
        let candidate = selection
            .candidates
            .get(index)
            .ok_or_else(|| Error::Invalid("Select a Spotify Track".into()))?;
        if candidate.identity.provider != "spotify"
            || candidate.identity.kind != "track"
            || candidate.identity.external_id.is_empty()
        {
            return Err(Error::Invalid("Select a Spotify Track occurrence".into()));
        }
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if load_input(&tx, &selection.input.track_id)? != selection.input {
            return Err(Error::ReconciliationEvidenceChanged);
        }
        clear_spotify_occurrences(&tx, &selection.input.track_id)?;
        persist_occurrence(&tx, &selection.input.track_id, candidate)?;
        tx.execute(
            "DELETE FROM track_provider_exclusion WHERE track_id=?1 AND provider='spotify'",
            [selection.input.track_id.as_ref()],
        )?;
        tx.commit()?;
        Ok(candidate.identity.clone())
    }
    pub fn clear_spotify_song_resolution(&mut self, track: &TrackId) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        load_input(&tx, track)?;
        clear_spotify_occurrences(&tx, track)?;
        tx.commit()?;
        Ok(())
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
        if candidate.identity.provider == "spotify" {
            tx.execute(
                "DELETE FROM track_provider_exclusion WHERE track_id=?1 AND provider='spotify'",
                [selection.input.track_id.as_ref()],
            )?;
        }
        tx.commit()?;
        Ok(candidate.identity.clone())
    }
}
