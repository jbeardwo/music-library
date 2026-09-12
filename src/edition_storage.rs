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
    load_track_evidence(db, release, &mut tracks)?;
    let mut evidence = LocalEditionEvidence {
        provenance: Default::default(),
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
    };
    load_provenance(db, &mut evidence)?;
    Ok(evidence)
}

/// Batch the existing accepted identities/credits as well as the new observations:
/// reconstruction must not add queries as the local Track count grows.
fn load_track_evidence(
    db: &Connection,
    release: &ReleaseId,
    tracks: &mut [LocalTrackEvidence],
) -> Result<()> {
    let indexes: std::collections::HashMap<_, _> = tracks
        .iter()
        .enumerate()
        .map(|(index, t)| (t.track_id.0.clone(), index))
        .collect();
    for recording in [false, true] {
        let join = if recording {
            "recording_external_identity i ON i.recording_id=t.recording_id"
        } else {
            "track_external_identity i ON i.track_id=t.id"
        };
        let mut statement = db.prepare(&format!("SELECT t.id,i.provider,i.kind,i.external_id FROM track t JOIN {join} WHERE t.release_id=?1 ORDER BY t.id,i.provider,i.kind,i.external_id"))?;
        for row in statement.query_map([release.as_ref()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                ExternalIdentity {
                    provider: r.get(1)?,
                    kind: r.get(2)?,
                    external_id: r.get(3)?,
                },
            ))
        })? {
            let (track, identity) = row?;
            let evidence = &mut tracks[indexes[&track]].evidence;
            if !recording {
                evidence.identities.push(identity);
            }
            // Decode the established storage convention; no new observation is promoted.
            else if identity.provider == "isrc" && identity.kind == "recording" {
                evidence.recording.isrcs.push(identity.external_id);
            } else {
                evidence.recording.identities.push(identity);
            }
        }
    }
    let mut statement = db.prepare("SELECT t.id,c.position,COALESCE(c.credited_name,a.name),COALESCE(c.join_phrase,''),i.provider,i.kind,i.external_id
        FROM track t JOIN track_artist_credit c ON c.track_id=t.id JOIN artist a ON a.id=c.artist_id
        LEFT JOIN artist_external_identity i ON i.artist_id=a.id
        WHERE t.release_id=?1 ORDER BY t.id,c.position,i.provider,i.kind,i.external_id")?;
    let mut last = None;
    for row in statement.query_map([release.as_ref()], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, Option<String>>(6)?,
        ))
    })? {
        let (track, position, name, join_phrase, provider, kind, external_id) = row?;
        let index = indexes[&track];
        let credits = &mut tracks[index].evidence.artists;
        if last != Some((index, position)) {
            credits.push(ArtistEvidence {
                name,
                join_phrase,
                identities: vec![],
            });
            last = Some((index, position));
        }
        if let (Some(provider), Some(kind), Some(external_id)) = (provider, kind, external_id) {
            credits
                .last_mut()
                .expect("credit inserted above")
                .identities
                .push(ExternalIdentity {
                    provider,
                    kind,
                    external_id,
                });
        }
    }
    Ok(())
}
fn load_provenance(db: &Connection, evidence: &mut LocalEditionEvidence) -> Result<()> {
    // One indexed association query, no file reads or per-Track DB lookups.
    let mut sources = std::collections::HashMap::<TrackId, Vec<_>>::new();
    let mut statement = db.prepare(PROVENANCE_QUERY)?;
    for row in statement.query_map([evidence.release_id.as_ref()], |r| {
        Ok((
            TrackId(r.get(0)?),
            SourceId(r.get(1)?),
            r.get::<_, Option<String>>(2)?,
        ))
    })? {
        let (track, source, json) = row?;
        sources
            .entry(track)
            .or_default()
            .push(crate::provenance_storage::decode(&source, json.as_deref())?);
    }
    drop(statement);
    evidence.provenance = crate::provenance::EditionProvenance::new(
        evidence
            .tracks
            .iter()
            .map(|t| sources.remove(&t.track_id).unwrap_or_default())
            .collect(),
    );
    evidence.completeness = evidence.provenance.completeness;
    // Tags prove only their own ordered program. Do not assert completeness
    // for application positions that were supplied differently at import.
    if evidence
        .tracks
        .iter()
        .zip(&evidence.provenance.tracks)
        .any(|(t, sources)| {
            sources
                .iter()
                .any(|s| !s.positions.agrees_with(t.evidence.disc, t.evidence.number))
        })
    {
        evidence.completeness = Completeness::Unknown;
    }
    Ok(())
}

// Start from one Release's indexed associations; unavailable sources retain their
// observations durably but do not contribute to current evidence/completeness.
// CROSS JOIN intentionally prevents SQLite from starting at the global available
// source index (observed with ordinary JOIN, even for a single Release).
// https://www.sqlite.org/optoverview.html#manual_control_of_query_plans_using_cross_join
const PROVENANCE_QUERY: &str = "SELECT t.id,ts.source_id,m.provenance_json
    FROM track t CROSS JOIN track_source ts ON ts.track_id=t.id
    CROSS JOIN local_file_observation l ON l.source_id=ts.source_id AND l.available=1
    LEFT JOIN file_metadata_observation m ON m.source_id=ts.source_id
    WHERE t.release_id=?1 ORDER BY t.id,ts.source_id";

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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provenance_reconstruction_uses_existing_entity_and_source_indexes() {
        let store = Store::open_in_memory().unwrap();
        let plan = store
            .connection
            .prepare(&format!("EXPLAIN QUERY PLAN {PROVENANCE_QUERY}"))
            .unwrap()
            .query_map(["release"], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
            .join("\n");
        for expected in [
            "SEARCH t USING COVERING INDEX track_release_order",
            "SEARCH ts USING COVERING INDEX",
            "SEARCH l USING COVERING INDEX local_file_available (available=? AND source_id=?)",
            "SEARCH m USING INDEX",
        ] {
            assert!(plan.contains(expected), "{plan}");
        }
        assert!(!plan.contains("SCAN "), "{plan}");
        println!("{plan}");
    }
}
