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
    pub title: String,
    pub artist: String,
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
fn load_input(db: &rusqlite::Connection, track: &TrackId) -> Result<Input> {
    let (mut input, release) = db.query_row(
            "SELECT e.title,e.artist_names,e.release_title,t.release_id,e.duration_ms,t.disc_number,t.track_number FROM track t JOIN effective_track_metadata e ON e.track_id=t.id WHERE t.id=?1",
            [track.as_ref()], |r| Ok((Input { track_id: track.clone(), title:r.get(0)?, artist:r.get(1)?, album:r.get(2)?, duration_ms:r.get::<_,Option<i64>>(4)?.and_then(|v| u64::try_from(v).ok()), disc:r.get(5)?, number:r.get(6)? },r.get::<_,String>(3)?)))?;
    if input.artist.trim().is_empty() {
        let mut statement = db.prepare("SELECT COALESCE(c.credited_name,a.name),COALESCE(c.join_phrase,'') FROM release_artist_credit c JOIN artist a ON a.id=c.artist_id WHERE c.release_id=?1 ORDER BY c.position")?;
        let credits = statement.query_map([release], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for credit in credits {
            let (name, join) = credit?;
            input.artist.push_str(&name);
            input.artist.push_str(&join);
        }
    }
    Ok(input)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Assessment {
    Unique(usize),
    NeedsSelection,
    NoMatch,
}

/// Deliberately stricter than the known-Album matcher: complete bounded page,
/// exact conservative Artist/title/Album strings, and no supplied duration or
/// position contradiction. No ranking, fuzzy edits, or version-word stripping.
pub fn assess(input: &Input, page: &Page<Candidate>) -> Assessment {
    fn normalized(s: &str) -> String {
        s.split_whitespace()
            .map(str::to_lowercase)
            .collect::<Vec<_>>()
            .join(" ")
    }
    let exact =
        |left: &str, right: &str| !left.trim().is_empty() && normalized(left) == normalized(right);
    let eligible: Vec<_> = page
        .items
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            exact(&input.title, &c.title)
                && exact(&input.artist, &c.artist)
                && exact(&input.album, &c.album)
                && !input.duration_ms.is_some_and(|d| {
                    d > 0 && c.duration_ms > 0 && d.abs_diff(c.duration_ms) > 3_000
                })
                && !input
                    .disc
                    .is_some_and(|d| d > 0 && c.disc > 0 && d != c.disc)
                && !input
                    .number
                    .is_some_and(|n| n > 0 && c.number > 0 && n != c.number)
        })
        .map(|(index, _)| index)
        .collect();
    if let [index] = eligible.as_slice()
        && page.next_offset.is_none()
    {
        return Assessment::Unique(*index);
    }
    if !page
        .items
        .iter()
        .any(|c| exact(&input.title, &c.title) && exact(&input.artist, &c.artist))
    {
        Assessment::NoMatch
    } else {
        Assessment::NeedsSelection
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
