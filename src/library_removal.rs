//! Membership removal preserves entities, provenance, sources, and queue snapshots.
use crate::{Library, Result, domain::*};
use rusqlite::{Connection, params};

#[derive(Clone, Debug)]
pub enum Target {
    Artist(ArtistId),
    Album(AlbumId),
    Track(TrackId),
    Tracks(Vec<String>),
}
#[derive(Clone, Debug)]
pub struct Preview {
    pub title: String,
    pub saved_tracks: u64,
    pub has_local_sources: bool,
}
impl Target {
    fn parameter(&self) -> String {
        match self {
            Self::Artist(id) => id.as_ref().into(),
            Self::Album(id) => id.as_ref().into(),
            Self::Track(id) => id.as_ref().into(),
            Self::Tracks(ids) => serde_json::to_string(ids).expect("IDs"),
        }
    }
    fn tracks(&self) -> &'static str {
        match self {
            Self::Tracks(_) => "SELECT track_id FROM library_membership WHERE track_id IN (SELECT value FROM json_each(?1))",
            Self::Track(_) => "SELECT track_id FROM library_membership WHERE track_id=?1",
            Self::Album(_) => "SELECT t.id AS track_id FROM release r JOIN track t ON t.release_id=r.id JOIN library_membership lm ON lm.track_id=t.id WHERE r.album_id=?1",
            Self::Artist(_) => "SELECT track_id FROM library_membership WHERE track_id IN (
                SELECT track_id FROM track_artist_credit WHERE artist_id=?1
                UNION SELECT t.id FROM release_artist_credit c JOIN track t ON t.release_id=c.release_id WHERE c.artist_id=?1
                UNION SELECT t.id FROM album_artist_credit c JOIN release r ON r.album_id=c.album_id JOIN track t ON t.release_id=r.id WHERE c.artist_id=?1)",
        }
    }
}
fn preview(connection: &Connection, target: &Target) -> Result<Preview> {
    let title_sql = match target {
        Target::Artist(_) => "SELECT name FROM artist WHERE id=?1",
        Target::Album(_) => "SELECT title FROM album_application_metadata WHERE album_id=?1",
        Target::Track(_) => "SELECT title FROM effective_track_metadata WHERE track_id=?1",
        Target::Tracks(_) => "SELECT 'Selected items'",
    };
    let title = if matches!(target, Target::Tracks(_)) {
        "Selected items".to_string()
    } else {
        connection.query_row(title_sql, [target.parameter()], |r| r.get(0))?
    };
    let sql = format!(
        "WITH affected AS ({}) SELECT count(*), EXISTS(SELECT 1 FROM affected a JOIN track_source ts ON ts.track_id=a.track_id JOIN local_file_observation l ON l.source_id=ts.source_id) FROM affected",
        target.tracks()
    );
    let (saved_tracks, has_local_sources) =
        connection.query_row(&sql, [target.parameter()], |r| {
            Ok((r.get::<_, i64>(0)? as u64, r.get(1)?))
        })?;
    Ok(Preview {
        title,
        saved_tracks,
        has_local_sources,
    })
}
/// A separate connection for background confirmation queries and membership removal.
pub struct Worker(Connection);
impl Worker {
    pub fn preview(self, target: &Target) -> Result<Preview> {
        preview(&self.0, target)
    }
    pub fn remove(mut self, target: &Target, suppress_local: bool) -> Result<u64> {
        remove(&mut self.0, target, suppress_local)
    }
}
fn remove(connection: &mut Connection, target: &Target, suppress_local: bool) -> Result<u64> {
    let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    tx.execute_batch("CREATE TEMP TABLE IF NOT EXISTS removal_tracks(track_id TEXT PRIMARY KEY); DELETE FROM removal_tracks;")?;
    tx.execute(
        &format!("INSERT INTO removal_tracks {}", target.tracks()),
        [target.parameter()],
    )?;
    // Unchecked explicitly clears any older suppression for the affected sources.
    let sources = "SELECT ts.source_id FROM removal_tracks a JOIN track_source ts ON ts.track_id=a.track_id JOIN local_file_observation l ON l.source_id=ts.source_id";
    if suppress_local {
        tx.execute(
            &format!("INSERT OR IGNORE INTO local_source_suppression(source_id) {sources}"),
            [],
        )?;
    } else {
        tx.execute(
            &format!("DELETE FROM local_source_suppression WHERE source_id IN ({sources})"),
            [],
        )?;
    }
    let count = tx.execute(
        "DELETE FROM library_membership WHERE track_id IN (SELECT track_id FROM removal_tracks)",
        [],
    )?;
    tx.execute("DELETE FROM removal_tracks", [])?;
    tx.commit()?;
    Ok(count as u64)
}
impl Library {
    pub fn removal_preview(&self, target: &Target) -> Result<Preview> {
        preview(&self.store.connection, target)
    }
    pub fn remove_library_object(&mut self, target: &Target, suppress_local: bool) -> Result<u64> {
        remove(&mut self.store.connection, target, suppress_local)
    }
    pub fn removal_worker(&self) -> Result<Worker> {
        let path = self
            .store
            .connection
            .path()
            .filter(|p| !p.is_empty())
            .ok_or_else(|| {
                crate::Error::Invalid("Background removal requires a file-backed library".into())
            })?;
        let connection = Connection::open(path)?;
        connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")?;
        Ok(Worker(connection))
    }
    /// Explicit Add/import hook for an already associated local source.
    /// Restore the existing Track rather than creating a new occurrence.
    pub fn readd_local_source(&mut self, source: &SourceId) -> Result<TrackId> {
        let tx = self.store.connection.transaction()?;
        let track: String = tx.query_row("SELECT ts.track_id FROM track_source ts JOIN local_file_observation l ON l.source_id=ts.source_id WHERE ts.source_id=?1", [source.as_ref()], |r| r.get(0))?;
        tx.execute(
            "DELETE FROM local_source_suppression WHERE source_id=?1",
            [source.as_ref()],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO library_membership(track_id) VALUES (?1)",
            params![track],
        )?;
        tx.commit()?;
        Ok(TrackId(track))
    }
    /// Clear exclusion without adding membership. A later automatic import may restore it.
    pub fn clear_local_suppression(&mut self, source: &SourceId) -> Result<bool> {
        Ok(self.store.connection.execute(
            "DELETE FROM local_source_suppression WHERE source_id=?1",
            [source.as_ref()],
        )? > 0)
    }
    /// Automatic import phase after a successful scan; discovery itself never adds Tracks.
    pub fn restore_unsuppressed_local_memberships(&mut self, root: &RootId) -> Result<u64> {
        Ok(self.store.connection.execute("INSERT OR IGNORE INTO library_membership(track_id)
            SELECT ts.track_id FROM local_root_source s CROSS JOIN local_file_observation l ON l.source_id=s.source_id JOIN track_source ts ON ts.source_id=l.source_id
            WHERE s.root_id=?1 AND l.available=1 AND NOT EXISTS
            (SELECT 1 FROM local_source_suppression x WHERE x.source_id=l.source_id)", [root.as_ref()])? as u64)
    }
}
