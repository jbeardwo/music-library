//! Provider-neutral Recording identity and conservative, Album-scoped enrichment.
use crate::{
    catalog::{CatalogError, Page},
    domain::{AlbumId, ExternalIdentity, Recording, RecordingId, TrackId},
    matching::{normalize, usable},
    storage::{Error, Result, Store},
};
use rusqlite::{Connection, params};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub identity: ExternalIdentity,
    pub title: String,
    pub artist: String,
    pub artist_ids: Vec<ExternalIdentity>,
    pub duration_ms: Option<u64>,
    pub isrcs: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalTrack {
    pub track_id: TrackId,
    pub recording_id: RecordingId,
    pub title: String,
    pub artist: String,
    pub artist_ids: Vec<ExternalIdentity>,
    pub duration_ms: Option<u64>,
    pub known: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    pub album_id: AlbumId,
    pub group: ExternalIdentity,
    pub tracks: Vec<LocalTrack>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrackOutcome {
    Matched(Candidate),
    AlreadyMatched,
    Ambiguous,
    NoConfidentMatch,
    Incomplete,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Pending,
    Complete(Vec<(LocalTrack, TrackOutcome)>),
    Deferred(CatalogError),
    Error(String),
}
#[derive(Debug)]
pub struct Reply {
    pub input: Input,
    pub result: std::result::Result<Page<Candidate>, CatalogError>,
}

/// Exact titles only. Three seconds allows minor encoder/silence boundaries;
/// any larger known duration disagreement vetoes the candidate.
pub const DURATION_TOLERANCE_MS: u64 = 3_000;
pub fn compare(track: &LocalTrack, page: &Page<Candidate>) -> TrackOutcome {
    if track.known {
        return TrackOutcome::AlreadyMatched;
    }
    if page.next_offset.is_some() {
        return TrackOutcome::Incomplete;
    }
    let mut candidates = vec![];
    for c in &page.items {
        if c.identity.provider != "musicbrainz"
            || c.identity.kind != "recording"
            || c.identity.external_id.is_empty()
            || normalize(&track.title) != normalize(&c.title)
        {
            continue;
        }
        if let (Some(local), Some(provider)) = (track.duration_ms, c.duration_ms)
            && local.abs_diff(provider) > DURATION_TOLERANCE_MS
        {
            continue;
        }
        if !track.artist_ids.is_empty() && !c.artist_ids.is_empty() {
            if track.artist_ids != c.artist_ids {
                continue;
            }
        } else if usable(&track.artist)
            && usable(&c.artist)
            && normalize(&track.artist) != normalize(&c.artist)
        {
            continue;
        }
        if !candidates
            .iter()
            .any(|v: &&Candidate| v.identity == c.identity)
        {
            candidates.push(c);
        }
    }
    match candidates.as_slice() {
        [one] => TrackOutcome::Matched((*one).clone()),
        [] => TrackOutcome::NoConfidentMatch,
        _ => TrackOutcome::Ambiguous,
    }
}

pub(crate) fn create_recording_tx(db: &Connection) -> Result<RecordingId> {
    let id = RecordingId::new();
    db.execute("INSERT INTO recording(id) VALUES (?1)", [id.as_ref()])?;
    Ok(id)
}
fn identities(db: &Connection, id: &RecordingId) -> Result<Vec<ExternalIdentity>> {
    Ok(db.prepare("SELECT provider,kind,external_id FROM recording_external_identity WHERE recording_id=?1 ORDER BY provider,kind,external_id")?
        .query_map([id.as_ref()], |r| Ok(ExternalIdentity{provider:r.get(0)?,kind:r.get(1)?,external_id:r.get(2)?}))?.collect::<rusqlite::Result<_>>()?)
}
fn attach(db: &Connection, id: &RecordingId, identity: &ExternalIdentity) -> Result<bool> {
    Ok(db.execute("INSERT INTO recording_external_identity(recording_id,provider,kind,external_id) VALUES (?1,?2,?3,?4) ON CONFLICT(recording_id,provider,kind,external_id) DO NOTHING",params![id.as_ref(),identity.provider,identity.kind,identity.external_id])? != 0)
}
fn resolve(db: &Connection, identity: &ExternalIdentity) -> Result<Vec<RecordingId>> {
    Ok(db.prepare("SELECT recording_id FROM recording_external_identity WHERE provider=?1 AND kind=?2 AND external_id=?3 ORDER BY recording_id")?
        .query_map(params![identity.provider,identity.kind,identity.external_id], |r|r.get(0).map(RecordingId))?.collect::<rusqlite::Result<_>>()?)
}
fn check_mb_compatibility(
    db: &Connection,
    ids: &[&RecordingId],
    incoming: Option<&str>,
) -> Result<()> {
    let mut mbids = std::collections::HashSet::new();
    if let Some(value) = incoming {
        mbids.insert(value.to_owned());
    }
    for id in ids {
        for i in identities(db, id)? {
            if i.provider == "musicbrainz" && i.kind == "recording" {
                mbids.insert(i.external_id);
            }
        }
    }
    if mbids.len() > 1 {
        return Err(Error::Invalid(
            "Recording consolidation conflicts with different MusicBrainz Recording identities"
                .into(),
        ));
    }
    Ok(())
}
fn merge_tx(db: &Connection, source: &RecordingId, canonical: &RecordingId) -> Result<bool> {
    if !db.query_row(
        "SELECT EXISTS(SELECT 1 FROM recording WHERE id=?1)",
        [canonical.as_ref()],
        |r| r.get::<_, bool>(0),
    )? {
        return Err(Error::Invalid("canonical Recording does not exist".into()));
    }
    if source == canonical {
        return Ok(false);
    }
    check_mb_compatibility(db, &[source, canonical], None)?;
    db.execute("INSERT INTO recording_external_identity SELECT ?1,provider,kind,external_id FROM recording_external_identity WHERE recording_id=?2 ON CONFLICT DO NOTHING",params![canonical.as_ref(),source.as_ref()])?;
    db.execute(
        "UPDATE track SET recording_id=?1 WHERE recording_id=?2",
        params![canonical.as_ref(), source.as_ref()],
    )?;
    Ok(db.execute("DELETE FROM recording WHERE id=?1", [source.as_ref()])? != 0)
}
/// MusicBrainz-aware reuse trigger; generic identity attachment remains many-to-many.
pub(crate) fn bind_musicbrainz_tx(
    db: &Connection,
    source: &RecordingId,
    identity: &ExternalIdentity,
    isrcs: &[String],
) -> Result<RecordingId> {
    if identity.provider != "musicbrainz"
        || identity.kind != "recording"
        || identity.external_id.is_empty()
    {
        return Err(Error::Invalid(
            "invalid MusicBrainz Recording identity".into(),
        ));
    }
    let existing = resolve(db, identity)?;
    let canonical = existing.first().unwrap_or(source).clone();
    let all = std::iter::once(source)
        .chain(existing.iter())
        .collect::<Vec<_>>();
    check_mb_compatibility(db, &all, Some(&identity.external_id))?;
    attach(db, source, identity)?;
    for other in std::iter::once(source).chain(existing.iter()) {
        merge_tx(db, other, &canonical)?;
    }
    for isrc in isrcs {
        attach(
            db,
            &canonical,
            &ExternalIdentity {
                provider: "isrc".into(),
                kind: "recording".into(),
                external_id: isrc.clone(),
            },
        )?;
    }
    Ok(canonical)
}

fn prepare(db: &Connection, album: &AlbumId) -> Result<Option<Input>> {
    let groups = db.prepare("SELECT external_id FROM album_external_identity WHERE album_id=?1 AND provider='musicbrainz' AND kind='release_group' ORDER BY external_id")?
        .query_map([album.as_ref()], |r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let [group] = groups.as_slice() else {
        return Ok(None);
    };
    let mut tracks = db.prepare("SELECT t.id,t.recording_id,e.title,e.artist_names,e.duration_ms,EXISTS(SELECT 1 FROM recording_external_identity i WHERE i.recording_id=t.recording_id AND i.provider='musicbrainz' AND i.kind='recording') FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=?1 AND EXISTS(SELECT 1 FROM track_source ts JOIN local_file_observation l ON l.source_id=ts.source_id WHERE ts.track_id=t.id) ORDER BY r.id,t.disc_number,t.track_number,t.id")?
        .query_map([album.as_ref()], |r|Ok(LocalTrack{track_id:TrackId(r.get(0)?),recording_id:RecordingId(r.get(1)?),title:r.get(2)?,artist:r.get(3)?,duration_ms:r.get::<_,Option<i64>>(4)?.and_then(|v|u64::try_from(v).ok()),known:r.get(5)?,artist_ids:vec![]}))?.collect::<rusqlite::Result<Vec<_>>>()?;
    tracks.retain(|t| usable(&t.title));
    // Targeted credits only; IDs are usable only when every position has one MBID.
    let mut q = db.prepare("SELECT c.position,i.external_id FROM track_artist_credit c LEFT JOIN artist_external_identity i ON i.artist_id=c.artist_id AND i.provider='musicbrainz' AND i.kind='artist' WHERE c.track_id=?1 ORDER BY c.position,i.external_id")?;
    for t in &mut tracks {
        let credits = q
            .query_map([t.track_id.as_ref()], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if credits.iter().all(|c| c.1.is_some()) && !credits.windows(2).any(|c| c[0].0 == c[1].0) {
            t.artist_ids = credits
                .into_iter()
                .filter_map(|c| c.1)
                .map(|id| ExternalIdentity {
                    provider: "musicbrainz".into(),
                    kind: "artist".into(),
                    external_id: id,
                })
                .collect();
        }
    }
    if tracks.iter().all(|t| t.known) {
        return Ok(None);
    }
    Ok(Some(Input {
        album_id: album.clone(),
        group: ExternalIdentity {
            provider: "musicbrainz".into(),
            kind: "release_group".into(),
            external_id: group.clone(),
        },
        tracks,
    }))
}

impl Store {
    pub fn create_recording(&mut self) -> Result<Recording> {
        Ok(Recording {
            recording_id: create_recording_tx(&self.connection)?,
        })
    }
    pub fn recording_for_track(&self, track: &TrackId) -> Result<Recording> {
        Ok(Recording {
            recording_id: self.connection.query_row(
                "SELECT recording_id FROM track WHERE id=?1",
                [track.as_ref()],
                |r| r.get(0).map(RecordingId),
            )?,
        })
    }
    pub fn attach_recording_external_identity(
        &mut self,
        id: &RecordingId,
        identity: &ExternalIdentity,
    ) -> Result<bool> {
        attach(&self.connection, id, identity)
    }
    pub fn list_recording_external_identities(
        &self,
        id: &RecordingId,
    ) -> Result<Vec<ExternalIdentity>> {
        identities(&self.connection, id)
    }
    pub fn resolve_recordings_external_identity(
        &self,
        identity: &ExternalIdentity,
    ) -> Result<Vec<RecordingId>> {
        resolve(&self.connection, identity)
    }
    pub fn merge_recording(
        &mut self,
        source: &RecordingId,
        canonical: &RecordingId,
    ) -> Result<bool> {
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let changed = merge_tx(&tx, source, canonical)?;
        tx.commit()?;
        Ok(changed)
    }
    pub fn prepare_recording_match(&self, album: &AlbumId) -> Result<Option<Input>> {
        prepare(&self.connection, album)
    }
    pub fn complete_recording_match(&mut self, reply: Reply) -> Result<Outcome> {
        let page = match reply.result {
            Ok(page) => page,
            Err(e) if e.is_provider_unavailable() => return Ok(Outcome::Deferred(e)),
            Err(e) => return Ok(Outcome::Error(e.to_string())),
        };
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let Some(current) = prepare(&tx, &reply.input.album_id)? else {
            return Ok(Outcome::Complete(vec![]));
        };
        if current != reply.input {
            return Err(Error::Invalid(
                "Recording matching input changed; retry enrichment".into(),
            ));
        }
        let mut results = vec![];
        for track in &current.tracks {
            let outcome = compare(track, &page);
            if let TrackOutcome::Matched(candidate) = &outcome {
                // Prior Tracks in this transaction may already have consolidated this Recording.
                let source = tx.query_row(
                    "SELECT recording_id FROM track WHERE id=?1",
                    [track.track_id.as_ref()],
                    |r| r.get(0).map(RecordingId),
                )?;
                bind_musicbrainz_tx(&tx, &source, &candidate.identity, &candidate.isrcs)?;
            }
            results.push((track.clone(), outcome));
        }
        tx.commit()?;
        Ok(Outcome::Complete(results))
    }
}
