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
        self.playlists_direction(after, limit, false)
    }
    /// Fetch preceding playlist identities nearest-first for a scrolling window.
    pub fn playlists_before(&self, before: &Cursor, limit: u32) -> Result<Vec<Row>> {
        self.playlists_direction(Some(before), limit, true)
    }
    fn playlists_direction(
        &self,
        after: Option<&Cursor>,
        limit: u32,
        reverse: bool,
    ) -> Result<Vec<Row>> {
        let sql = "SELECT id,name FROM playlist WHERE name COLLATE NOCASE >= ?1 COLLATE NOCASE AND (name COLLATE NOCASE,id)>(?1 COLLATE NOCASE,?2) ORDER BY name COLLATE NOCASE,id LIMIT ?3";
        let sql = if reverse {
            sql.replace(" >= ", " <= ").replace(")>", ")<").replace(
                "ORDER BY name COLLATE NOCASE,id",
                "ORDER BY name COLLATE NOCASE DESC,id DESC",
            )
        } else {
            sql.to_string()
        };
        let mut q = self.store.connection.prepare(&sql)?;
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
    read_selected_entries(
        connection,
        &[playlist.to_string()],
        after
            .map(|position| Cursor {
                release: playlist.into(),
                position,
                id: "\u{10ffff}".into(),
                ..Default::default()
            })
            .as_ref(),
        limit,
        &[],
    )
}

pub(crate) fn read_selected_entries(
    connection: &Connection,
    playlists: &[String],
    after: Option<&Cursor>,
    limit: Option<u32>,
    entries: &[String],
) -> Result<Vec<Row>> {
    read_selected_entries_direction(connection, playlists, after, limit, entries, false)
}
fn read_selected_entries_direction(
    connection: &Connection,
    playlists: &[String],
    after: Option<&Cursor>,
    limit: Option<u32>,
    entries: &[String],
    reverse: bool,
) -> Result<Vec<Row>> {
    let sql = "SELECT p.id,p.position,t.id,t.release_id,e.title,a.title,e.artist_names,e.year,EXISTS(SELECT 1 FROM track_source s JOIN local_file_observation l ON l.source_id=s.source_id WHERE s.track_id=t.id AND l.available=1),p.playlist_id,pl.name FROM playlist_entry p CROSS JOIN track t ON t.id=p.track_id JOIN effective_track_metadata e ON e.track_id=t.id JOIN release r ON r.id=t.release_id JOIN album_application_metadata a ON a.album_id=r.album_id JOIN playlist pl ON pl.id=p.playlist_id WHERE p.playlist_id IN (SELECT value FROM json_each(?1)) AND (p.playlist_id,p.position,p.id)>(?2,?3,?4) AND (?6 IS NULL OR p.id IN (SELECT value FROM json_each(?6))) ORDER BY p.playlist_id,p.position,p.id LIMIT ?5";
    let sql = if reverse {
        sql.replace(")>", ")<").replace(
            "ORDER BY p.playlist_id,p.position,p.id",
            "ORDER BY p.playlist_id DESC,p.position DESC,p.id DESC",
        )
    } else {
        sql.to_string()
    };
    // For one selected playlist, expose the position bound directly to the
    // order index. The complete tuple still resolves entry identity and ties.
    let sql = if playlists.len() == 1 && after.is_some_and(|c| c.release == playlists[0]) {
        sql.replace(
            "ORDER BY",
            if reverse {
                "AND p.position <= ?3 ORDER BY"
            } else {
                "AND p.position >= ?3 ORDER BY"
            },
        )
    } else {
        sql
    };
    let mut q = connection.prepare(&sql)?;
    Ok(q.query_map(
        params![
            serde_json::to_string(playlists).expect("IDs"),
            after.map(|c| c.release.as_str()).unwrap_or(""),
            after.map(|c| c.position).unwrap_or(-1),
            after.map(|c| c.id.as_str()).unwrap_or(""),
            limit.map(i64::from).unwrap_or(-1),
            (!entries.is_empty()).then(|| serde_json::to_string(entries).expect("IDs"))
        ],
        |r| {
            let id: String = r.get(0)?;
            let position: i64 = r.get(1)?;
            let playlist: String = r.get(9)?;
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
                subtitle: track.artist_names.clone(),
                year: track.year,
                group: playlist.clone(),
                group_label: r.get(10)?,
                cursor: Cursor {
                    release: playlist,
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

impl Library {
    /// Fetch preceding entries nearest-first, preserving duplicate entry identities.
    pub fn selected_playlist_entries_before(
        &self,
        playlists: &[String],
        before: &Cursor,
        limit: u32,
    ) -> Result<Vec<Row>> {
        read_selected_entries_direction(
            &self.store.connection,
            playlists,
            Some(before),
            Some(limit.clamp(1, 201)),
            &[],
            true,
        )
    }
    pub fn selected_playlist_entries(
        &self,
        playlists: &[String],
        after: Option<&Cursor>,
        limit: u32,
    ) -> Result<Vec<Row>> {
        read_selected_entries(
            &self.store.connection,
            playlists,
            after,
            Some(limit.clamp(1, 201)),
            &[],
        )
    }
}
impl crate::browse::QueueReader {
    pub fn playlist_range(
        &self,
        playlists: &[String],
        anchor: &str,
        target: &str,
    ) -> Result<Vec<String>> {
        let mut q=self.0.prepare("SELECT playlist_id,position FROM playlist_entry WHERE playlist_id IN (SELECT value FROM json_each(?1)) AND id IN (?2,?3) ORDER BY playlist_id,position")?;
        let endpoints = q
            .query_map(
                params![
                    serde_json::to_string(playlists).expect("IDs"),
                    anchor,
                    target
                ],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if endpoints.len() != if anchor == target { 1 } else { 2 } {
            return Err(Error::Invalid("Selection endpoint vanished".into()));
        }
        let first = &endpoints[0];
        let last = endpoints.last().unwrap();
        let mut q=self.0.prepare("SELECT id FROM playlist_entry WHERE playlist_id IN (SELECT value FROM json_each(?1)) AND (playlist_id,position)>=(?2,?3) AND (playlist_id,position)<=(?4,?5) ORDER BY playlist_id,position")?;
        Ok(q.query_map(
            params![
                serde_json::to_string(playlists).expect("IDs"),
                first.0,
                first.1,
                last.0,
                last.1
            ],
            |r| r.get(0),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

/// A prepared operation owns its input snapshot; QML receives only aggregate counts.
#[derive(Clone, Debug)]
pub struct AppendPlan {
    pub playlist_id: String,
    pub name: String,
    pub tracks: Vec<TrackId>,
    pub duplicate_entries: usize,
}
impl Library {
    pub fn prepare_playlist_append(
        &self,
        playlist: &str,
        tracks: Vec<TrackId>,
    ) -> Result<AppendPlan> {
        prepare_append(&self.store.connection, playlist, tracks)
    }
    /// Atomically append the entire batch, or skip identities present in the destination.
    /// Duplicates internal to the source snapshot remain intentional occurrences.
    pub fn apply_playlist_append(
        &mut self,
        plan: &AppendPlan,
        include_duplicates: bool,
    ) -> Result<usize> {
        apply_append(&mut self.store.connection, plan, include_duplicates)
    }
}

fn prepare_append(
    connection: &Connection,
    playlist: &str,
    tracks: Vec<TrackId>,
) -> Result<AppendPlan> {
    let name = connection.query_row("SELECT name FROM playlist WHERE id=?1", [playlist], |r| {
        r.get(0)
    })?;
    let duplicate_entries = duplicate_tracks(connection, playlist, &tracks)?.len();
    Ok(AppendPlan {
        playlist_id: playlist.into(),
        name,
        tracks,
        duplicate_entries,
    })
}
fn apply_append(
    connection: &mut Connection,
    plan: &AppendPlan,
    include_duplicates: bool,
) -> Result<usize> {
    let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let duplicates = if include_duplicates {
        std::collections::HashSet::new()
    } else {
        duplicate_tracks(&tx, &plan.playlist_id, &plan.tracks)?
            .into_iter()
            .collect()
    };
    let mut position: i64 = tx.query_row(
        "SELECT COALESCE(MAX(position)+1,0) FROM playlist_entry WHERE playlist_id=?1",
        [&plan.playlist_id],
        |r| r.get(0),
    )?;
    let mut count = 0;
    {
        let mut insert = tx.prepare(
            "INSERT INTO playlist_entry(id,playlist_id,track_id,position) VALUES(?1,?2,?3,?4)",
        )?;
        for track in &plan.tracks {
            if !duplicates.contains(track.as_ref()) {
                insert.execute(params![
                    uuid::Uuid::new_v4().to_string(),
                    plan.playlist_id,
                    track.as_ref(),
                    position
                ])?;
                count += 1;
                position += 1;
            }
        }
    }
    if count > 0 {
        tx.execute(
            "UPDATE playlist SET updated_at=unixepoch() WHERE id=?1",
            [&plan.playlist_id],
        )?;
    }
    tx.commit()?;
    Ok(count)
}

fn duplicate_tracks(
    connection: &Connection,
    playlist: &str,
    tracks: &[TrackId],
) -> Result<Vec<String>> {
    let ids = tracks.iter().map(AsRef::as_ref).collect::<Vec<_>>();
    let mut q=connection.prepare("SELECT j.value FROM json_each(?2) j WHERE EXISTS(SELECT 1 FROM playlist_entry p WHERE p.track_id=j.value AND p.playlist_id=?1)")?;
    Ok(q.query_map(
        params![playlist, serde_json::to_string(&ids).expect("IDs")],
        |r| r.get(0),
    )?
    .collect::<rusqlite::Result<Vec<_>>>()?)
}

impl crate::browse::QueueReader {
    pub fn playlists_range(&self, anchor: &str, target: &str) -> Result<Vec<String>> {
        let key = |id: &str| {
            self.0.query_row(
                "SELECT lower(name),id FROM playlist WHERE id=?1",
                [id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
        };
        let a = key(anchor)?;
        let b = key(target)?;
        let (first, last) = if a <= b { (a, b) } else { (b, a) };
        let mut q=self.0.prepare("SELECT id FROM playlist WHERE (name COLLATE NOCASE,id)>=(?1 COLLATE NOCASE,?2) AND (name COLLATE NOCASE,id)<=(?3 COLLATE NOCASE,?4) ORDER BY name COLLATE NOCASE,id")?;
        Ok(
            q.query_map(params![first.0, first.1, last.0, last.1], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?,
        )
    }
    pub fn visible_playlist_entry_ids(
        &self,
        playlists: &[String],
        entries: &[String],
    ) -> Result<Vec<String>> {
        if entries.is_empty() {
            return Ok(vec![]);
        }
        let mut q=self.0.prepare("SELECT id FROM playlist_entry WHERE id IN (SELECT value FROM json_each(?1)) AND playlist_id IN (SELECT value FROM json_each(?2))")?;
        Ok(q.query_map(
            params![
                serde_json::to_string(entries).expect("IDs"),
                serde_json::to_string(playlists).expect("IDs")
            ],
            |r| r.get(0),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

impl crate::browse::QueueReader {
    pub fn read_playlists(
        self,
        playlists: &[String],
        start: Option<&str>,
    ) -> Result<(Vec<TrackSearchResult>, usize)> {
        let rows = read_selected_entries(&self.0, playlists, None, None, &[])?;
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

impl Library {
    pub fn delete_playlists(&mut self, ids: &[String]) -> Result<()> {
        self.store.connection.execute(
            "DELETE FROM playlist WHERE id IN (SELECT value FROM json_each(?1))",
            [serde_json::to_string(ids).expect("IDs")],
        )?;
        Ok(())
    }
    pub fn remove_playlist_entries(&mut self, ids: &[String]) -> Result<()> {
        let tx = self.store.connection.transaction()?;
        let json = serde_json::to_string(ids).expect("IDs");
        tx.execute("UPDATE playlist SET updated_at=unixepoch() WHERE id IN (SELECT playlist_id FROM playlist_entry WHERE id IN (SELECT value FROM json_each(?1)))",[&json])?;
        tx.execute(
            "DELETE FROM playlist_entry WHERE id IN (SELECT value FROM json_each(?1))",
            [&json],
        )?;
        tx.commit()?;
        Ok(())
    }
}

/// Background application operations use a dedicated connection, never a UI write transaction.
pub struct AppendWorker(Connection);
impl AppendWorker {
    pub fn prepare(&self, playlist: &str, tracks: Vec<TrackId>) -> Result<AppendPlan> {
        prepare_append(&self.0, playlist, tracks)
    }
    pub fn apply(&mut self, plan: &AppendPlan, include_duplicates: bool) -> Result<usize> {
        apply_append(&mut self.0, plan, include_duplicates)
    }
}
impl Library {
    pub fn playlist_append_worker(&self) -> Result<AppendWorker> {
        let path = self
            .store
            .connection
            .path()
            .filter(|p| !p.is_empty())
            .ok_or_else(|| {
                Error::Invalid(
                    "Background playlist operations require a file-backed library".into(),
                )
            })?;
        let connection = Connection::open(path)?;
        connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")?;
        Ok(AppendWorker(connection))
    }
}
