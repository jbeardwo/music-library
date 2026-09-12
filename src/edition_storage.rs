//! Targeted read-only snapshot construction; no provider filtering or network work.
use crate::{
    domain::*,
    edition::*,
    storage::{Result, Store},
};
use rusqlite::{Connection, OpenFlags};

fn identities(db: &Connection, entity: &str, id: &str) -> Result<Vec<ExternalIdentity>> {
    // Entity names are internal constants, never caller input.
    Ok(db.prepare(&format!("SELECT provider,kind,external_id FROM {entity}_external_identity WHERE {entity}_id=?1 ORDER BY provider,kind,external_id"))?
        .query_map([id], |r| Ok(ExternalIdentity { provider:r.get(0)?,kind:r.get(1)?,external_id:r.get(2)? }))?.collect::<rusqlite::Result<_>>()?)
}
fn artists(db: &Connection, entity: &str, id: &str) -> Result<Vec<ArtistEvidence>> {
    let rows = db.prepare(&format!("SELECT c.artist_id,COALESCE(c.credited_name,a.name),COALESCE(c.join_phrase,'') FROM {entity}_artist_credit c JOIN artist a ON a.id=c.artist_id WHERE c.{entity}_id=?1 ORDER BY c.position"))?
        .query_map([id], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(id, name, join_phrase)| {
            Ok(ArtistEvidence {
                identities: identities(db, "artist", &id)?,
                name,
                join_phrase,
            })
        })
        .collect()
}
fn snapshot(db: &Connection, release: &ReleaseId) -> Result<LocalEditionEvidence> {
    let (album,title,album_title,year): (String,String,String,Option<i32>) = db.query_row("SELECT r.album_id,m.title,a.title,m.year FROM release r JOIN release_application_metadata m ON m.release_id=r.id JOIN album_application_metadata a ON a.album_id=r.album_id WHERE r.id=?1",[release.as_ref()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
    let mut tracks = db.prepare("SELECT t.id,t.recording_id,t.disc_number,t.track_number,e.title,e.duration_ms FROM track t JOIN effective_track_metadata e ON e.track_id=t.id WHERE t.release_id=?1 ORDER BY t.disc_number,t.track_number,t.id")?
        .query_map([release.as_ref()], |r| Ok(LocalTrackEvidence {track_id:TrackId(r.get(0)?),recording_id:RecordingId(r.get(1)?),evidence:TrackEvidence {disc:r.get(2)?,number:r.get(3)?,title:r.get(4)?,duration_ms:r.get::<_,Option<i64>>(5)?.and_then(|v|u64::try_from(v).ok()),..Default::default()} }))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for t in &mut tracks {
        t.evidence.identities = identities(db, "track", t.track_id.as_ref())?;
        t.evidence.artists = artists(db, "track", t.track_id.as_ref())?;
        for id in identities(db, "recording", t.recording_id.as_ref())? {
            if id.provider == "isrc" && id.kind == "recording" {
                t.evidence.recording.isrcs.push(id.external_id);
            } else {
                t.evidence.recording.identities.push(id);
            }
        }
    }
    Ok(LocalEditionEvidence {
        grouping_identities: identities(db, "album", &album)?,
        album_id: AlbumId(album),
        release_id: release.clone(),
        album_title,
        identities: identities(db, "release", release.as_ref())?,
        exact_identities: vec![],
        metadata: EditionMetadata {
            title: Some(title),
            date: year.map(|y| y.to_string()),
            artists: artists(db, "release", release.as_ref())?,
            ..Default::default()
        },
        tracks,
        completeness: Completeness::Unknown,
    })
}
impl Store {
    pub fn edition_evidence(&self, release: &ReleaseId) -> Result<LocalEditionEvidence> {
        let tx = self.connection.unchecked_transaction()?;
        let evidence = snapshot(&tx, release)?;
        tx.commit()?;
        Ok(evidence)
    }
}
/// Audit CLI entry point: never runs migrations or writes to the supplied database.
pub fn read_only(
    path: impl AsRef<std::path::Path>,
    release: &ReleaseId,
) -> Result<LocalEditionEvidence> {
    let mut db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let tx = db.transaction()?;
    let evidence = snapshot(&tx, release)?;
    tx.commit()?;
    Ok(evidence)
}
