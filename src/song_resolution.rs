//! Bounded song lookup and conservative explicit-Play acceptance. Never Album,
//! Recording, or edition acceptance.
use crate::{
    catalog::{CatalogError, Page},
    domain::*,
    storage::{Error, Result, Store},
};
use rusqlite::params;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    pub track_id: TrackId,
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// Adapter-declared song/occurrence identity, never Recording identity.
    pub identity: ExternalIdentity,
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
        self.primary_artist
            .as_ref()
            .filter(|a| !a.name.trim().is_empty())
            .map_or(&self.artist, |a| &a.name)
    }
}
fn load_input(db: &rusqlite::Connection, track: &TrackId) -> Result<Input> {
    let (mut input, album) = db.query_row(
            "SELECT e.title,e.artist_names,e.release_title,r.album_id,e.duration_ms,t.disc_number,t.track_number FROM track t JOIN release r ON r.id=t.release_id JOIN effective_track_metadata e ON e.track_id=t.id WHERE t.id=?1",
            [track.as_ref()], |r| Ok((Input { track_id: track.clone(), album_date:None, album_artists:vec![], album_required_tracks:0, primary_artist:None, artists:vec![], title:r.get(0)?, artist:r.get(1)?, album:r.get(2)?, duration_ms:r.get::<_,Option<i64>>(4)?.and_then(|v| u64::try_from(v).ok()), disc:r.get(5)?, number:r.get(6)? },r.get::<_,String>(3)?)))?;
    let year: Option<i32> = db.query_row(
        "SELECT year FROM album_application_metadata WHERE album_id=?1",
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
    !left.trim().is_empty() && crate::matching::normalize(left) == crate::matching::normalize(right)
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
}
/// Presentation feasibility is weaker than automatic acceptance. No persistence,
/// provider calls, duration-only rejection or invented contributor equivalence.
pub fn feasibility(input: &Input, c: &Candidate) -> Feasibility {
    let mut reasons = vec![];
    if !primary_compatible(input, c) {
        reasons.push("different primary Artist");
    }
    if crate::artist_credit::compare(&input.artists, &c.artists)
        == crate::artist_credit::Compatibility::Contradictory
    {
        reasons.push("contradictory Artist credits");
    }
    if !same_title(input, c) {
        reasons.push("different Track title or semantic version");
    }
    if !input.album.trim().is_empty() && !exact(&input.album, &c.album) {
        reasons.push(match c.album_type.as_str() {
            "compilation" | "Compilation" => "compilation outside established Album context",
            "single" | "Single" | "EP" => "Single/EP outside established Album context",
            _ => "different established Album title",
        });
    }
    if let (Some(a), Some(b)) = (input.album_artists.first(), c.album_artists.first())
        && !crate::artist_credit::same_artist(a, b)
    {
        reasons.push("different Album Artist");
    }
    if !crate::album_candidates::can_accommodate(input.album_required_tracks, c.album_total_tracks)
    {
        reasons.push("Album program too short for established Tracks");
    }
    if input
        .disc
        .is_some_and(|n| n > 0 && c.disc > 0 && n != c.disc)
        || input
            .number
            .is_some_and(|n| n > 0 && c.number > 0 && n != c.number)
    {
        reasons.push("different position in established Album");
    }
    if !reasons.is_empty()
        && input
            .duration_ms
            .is_some_and(|n| n > 0 && c.duration_ms > 0 && n.abs_diff(c.duration_ms) > 30_000)
    {
        reasons.push("dramatically different duration corroborates contradictions");
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
    Feasibility { class, reasons }
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
fn context_matches(input: &Input, c: &Candidate) -> bool {
    feasibility(input, c).class != FeasibilityClass::Infeasible
        && !input.album.trim().is_empty()
        // Preserve the stricter existing explicit-Play acceptance gate.
        && !input.duration_ms.is_some_and(|d| d > 0 && c.duration_ms > 0 && d.abs_diff(c.duration_ms) > 3_000)
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

impl Store {
    pub fn song_resolution_input(&self, track: &TrackId) -> Result<Input> {
        load_input(&self.connection, track)
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
        tx.execute("INSERT INTO track_external_identity(track_id,provider,kind,external_id) VALUES(?1,?2,?3,?4)",params![selection.input.track_id.as_ref(),candidate.identity.provider,candidate.identity.kind,candidate.identity.external_id])?;
        crate::storage::observe_duration(
            &tx,
            &selection.input.track_id,
            &candidate.identity.provider,
            &candidate.identity.external_id,
            crate::catalog::Duration {
                milliseconds: candidate.duration_ms,
                approximate: false,
            },
            0,
        )?;
        tx.commit()?;
        Ok(candidate.identity.clone())
    }
}
