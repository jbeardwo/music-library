//! Application-owned ordered Track references, independent of saved membership.
use crate::{
    Library, Result,
    browse::{Cursor, Row},
    domain::*,
    storage::Error,
};
use rusqlite::{Connection, params};

impl Library {
    pub fn create_playlist(&mut self, name: &str) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        self.store.connection.execute(
            "INSERT INTO playlist(id,name) VALUES(?1,?2)",
            params![id, name.trim()],
        )?;
        Ok(id)
    }
    pub fn rename_playlist(&mut self, id: &str, name: &str) -> Result<()> {
        self.store.connection.execute(
            "UPDATE playlist SET name=?2,updated_at=unixepoch() WHERE id=?1",
            params![id, name.trim()],
        )?;
        Ok(())
    }
    pub fn delete_playlist(&mut self, id: &str) -> Result<()> {
        self.store
            .connection
            .execute("DELETE FROM playlist WHERE id=?1", [id])?;
        Ok(())
    }
    pub fn append_playlist_track(&mut self, playlist: &str, track: &TrackId) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let tx = self.store.connection.transaction()?;
        tx.execute("INSERT INTO playlist_entry(id,playlist_id,track_id,position) SELECT ?1,?2,?3,COALESCE(MAX(position)+1,0) FROM playlist_entry WHERE playlist_id=?2",params![id,playlist,track.as_ref()])?;
        tx.execute(
            "UPDATE playlist SET updated_at=unixepoch() WHERE id=?1",
            [playlist],
        )?;
        tx.commit()?;
        Ok(id)
    }
    pub fn remove_playlist_entry(&mut self, playlist: &str, entry: &str) -> Result<()> {
        let tx = self.store.connection.transaction()?;
        tx.execute(
            "DELETE FROM playlist_entry WHERE playlist_id=?1 AND id=?2",
            params![playlist, entry],
        )?;
        tx.execute(
            "UPDATE playlist SET updated_at=unixepoch() WHERE id=?1",
            [playlist],
        )?;
        tx.commit()?;
        Ok(())
    }
    /// Move an entry one place; gaps left by removal do not affect ordering.
    pub fn move_playlist_entry(&mut self, playlist: &str, entry: &str, down: bool) -> Result<()> {
        let tx = self.store.connection.transaction()?;
        let position: i64 = tx.query_row(
            "SELECT position FROM playlist_entry WHERE playlist_id=?1 AND id=?2",
            params![playlist, entry],
            |r| r.get(0),
        )?;
        let sql = if down {
            "SELECT id,position FROM playlist_entry WHERE playlist_id=?1 AND position>?2 ORDER BY position LIMIT 1"
        } else {
            "SELECT id,position FROM playlist_entry WHERE playlist_id=?1 AND position<?2 ORDER BY position DESC LIMIT 1"
        };
        use rusqlite::OptionalExtension;
        if let Some((other, next)) = tx
            .query_row(sql, params![playlist, position], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })
            .optional()?
        {
            tx.execute("UPDATE playlist_entry SET position=-1 WHERE id=?1", [entry])?;
            tx.execute(
                "UPDATE playlist_entry SET position=?2 WHERE id=?1",
                params![other, position],
            )?;
            tx.execute(
                "UPDATE playlist_entry SET position=?2 WHERE id=?1",
                params![entry, next],
            )?;
            tx.execute(
                "UPDATE playlist SET updated_at=unixepoch() WHERE id=?1",
                [playlist],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn playlists(&self, after: Option<&Cursor>, limit: u32) -> Result<Vec<Row>> {
        let mut q=self.store.connection.prepare("SELECT id,name FROM playlist WHERE name COLLATE NOCASE >= ?1 COLLATE NOCASE AND (name COLLATE NOCASE,id)>(?1 COLLATE NOCASE,?2) ORDER BY name COLLATE NOCASE,id LIMIT ?3")?;
        Ok(q.query_map(
            params![
                after.map(|c| c.title.as_str()).unwrap_or(""),
                after.map(|c| c.id.as_str()).unwrap_or(""),
                limit.clamp(1, 201)
            ],
            |r| {
                let id: String = r.get(0)?;
                let title: String = r.get(1)?;
                Ok(Row {
                    id: id.clone(),
                    title: title.clone(),
                    subtitle: String::new(),
                    year: None,
                    group: String::new(),
                    group_label: String::new(),
                    cursor: Cursor {
                        title,
                        id,
                        ..Default::default()
                    },
                    track: None,
                    track_number: None,
                    disc_number: None,
                    multi_disc: false,
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn playlist_entries(
        &self,
        playlist: &str,
        after: Option<i64>,
        limit: u32,
    ) -> Result<Vec<Row>> {
        read_entries(
            &self.store.connection,
            playlist,
            after,
            Some(limit.clamp(1, 201)),
        )
    }
}
impl crate::browse::QueueReader {
    /// Capture the complete persisted order, including duplicate entry identities.
    pub fn read_playlist(
        self,
        playlist: &str,
        start: Option<&str>,
    ) -> Result<(Vec<TrackSearchResult>, usize)> {
        let rows = read_entries(&self.0, playlist, None, None)?;
        let position = match start {
            Some(id) => rows
                .iter()
                .position(|r| r.id == id)
                .ok_or_else(|| Error::Invalid("Playlist entry no longer exists".into()))?,
            None => 0,
        };
        Ok((rows.into_iter().filter_map(|r| r.track).collect(), position))
    }
}
fn read_entries(
    connection: &Connection,
    playlist: &str,
    after: Option<i64>,
    limit: Option<u32>,
) -> Result<Vec<Row>> {
    let mut q=connection.prepare("SELECT p.id,p.position,t.id,t.release_id,e.title,a.title,e.artist_names,e.year,EXISTS(SELECT 1 FROM track_source s JOIN local_file_observation l ON l.source_id=s.source_id WHERE s.track_id=t.id AND l.available=1) FROM playlist_entry p CROSS JOIN track t ON t.id=p.track_id JOIN effective_track_metadata e ON e.track_id=t.id JOIN release r ON r.id=t.release_id JOIN album_application_metadata a ON a.album_id=r.album_id WHERE p.playlist_id=?1 AND p.position>?2 ORDER BY p.position LIMIT ?3")?;
    Ok(q.query_map(
        params![
            playlist,
            after.unwrap_or(-1),
            limit.map(i64::from).unwrap_or(-1)
        ],
        |r| {
            let id: String = r.get(0)?;
            let position: i64 = r.get(1)?;
            let track = TrackSearchResult {
                track_id: TrackId(r.get(2)?),
                release_id: ReleaseId(r.get(3)?),
                title: r.get(4)?,
                release_title: r.get(5)?,
                artist_names: r.get(6)?,
                year: r.get(7)?,
                available: r.get(8)?,
            };
            Ok(Row {
                id: id.clone(),
                title: track.title.clone(),
                subtitle: format!("{} — {}", track.artist_names, track.release_title),
                year: track.year,
                group: String::new(),
                group_label: String::new(),
                cursor: Cursor {
                    position,
                    id,
                    ..Default::default()
                },
                track: Some(track),
                track_number: None,
                disc_number: None,
                multi_disc: false,
            })
        },
    )?
    .collect::<rusqlite::Result<Vec<_>>>()?)
}
