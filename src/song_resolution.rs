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
}
impl Selection {
    pub fn new(input: Input, candidates: Vec<Candidate>) -> Self {
        Self { input, candidates }
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
            [track.as_ref()], |r| Ok((Input { track_id: track.clone(), primary_artist:None, artists:vec![], title:r.get(0)?, artist:r.get(1)?, album:r.get(2)?, duration_ms:r.get::<_,Option<i64>>(4)?.and_then(|v| u64::try_from(v).ok()), disc:r.get(5)?, number:r.get(6)? },r.get::<_,String>(3)?)))?;
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
    exact(
        crate::artist_credit::musical_title(&input.title, &input.artists),
        crate::artist_credit::musical_title(&c.title, &c.artists),
    )
}
fn context_matches(input: &Input, c: &Candidate) -> bool {
    same_title(input, c)
        && primary_compatible(input, c)
        && exact(&input.album, &c.album)
        && !input
            .duration_ms
            .is_some_and(|d| d > 0 && c.duration_ms > 0 && d.abs_diff(c.duration_ms) > 3_000)
        && !input
            .disc
            .is_some_and(|d| d > 0 && c.disc > 0 && d != c.disc)
        && !input
            .number
            .is_some_and(|n| n > 0 && c.number > 0 && n != c.number)
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
        tx.commit()?;
        Ok(candidate.identity.clone())
    }
}
