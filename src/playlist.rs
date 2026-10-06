//! Application-owned ordered Track references, independent of saved membership.
use crate::{
    Library, Result,
    browse::{Cursor, Row},
    domain::*,
    storage::Error,
};
use rusqlite::{Connection, OptionalExtension, params};

/// Playlist occurrences contribute individually, independently of saved membership.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Details {
    pub id: String,
    pub name: String,
    pub entry_count: u64,
    pub known_duration_ms: u64,
    pub unknown_duration_count: u64,
    pub approximate_duration_count: u64,
}

const DETAILS_SQL: &str = "SELECT pl.id,pl.name,COUNT(p.id),COALESCE(SUM(CASE WHEN e.duration_ms>=0 THEN e.duration_ms ELSE 0 END),0),COUNT(p.id)-COUNT(CASE WHEN e.duration_ms>=0 THEN 1 END),COUNT(CASE WHEN e.duration_ms>=0 AND e.duration_approximate=1 THEN 1 END) FROM playlist pl LEFT JOIN playlist_entry p INDEXED BY playlist_entry_order ON p.playlist_id=pl.id LEFT JOIN effective_track_metadata e ON e.track_id=p.track_id WHERE pl.id=?1 GROUP BY pl.id";

fn read_details(connection: &Connection, playlist: &str) -> Result<Option<Details>> {
    use rusqlite::OptionalExtension;
    Ok(connection
        .query_row(DETAILS_SQL, [playlist], |r| {
            Ok(Details {
                id: r.get(0)?,
                name: r.get(1)?,
                entry_count: r.get::<_, i64>(2)? as u64,
                known_duration_ms: r.get::<_, i64>(3)? as u64,
                unknown_duration_count: r.get::<_, i64>(4)? as u64,
                approximate_duration_count: r.get::<_, i64>(5)? as u64,
            })
        })
        .optional()?)
}

impl crate::browse::QueueReader {
    /// Read on the existing read-only application connection away from the UI thread.
    pub fn playlist_details(&self, playlist: &str) -> Result<Option<Details>> {
        read_details(&self.0, playlist)
    }
}

impl Library {
    pub fn playlist_details(&self, playlist: &str) -> Result<Option<Details>> {
        read_details(&self.store.connection, playlist)
    }
    /// Cheap content invalidation by stable local ID; titles and metadata do not change it.
    pub fn playlist_content_revisions(&self, playlists: &[String]) -> Result<Vec<(String, i64)>> {
        let mut query=self.store.connection.prepare("SELECT id,content_revision FROM playlist WHERE id IN (SELECT value FROM json_each(?1)) ORDER BY id")?;
        Ok(query
            .query_map([serde_json::to_string(playlists).expect("IDs")], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

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
                    connection_reason: String::new(),
                    connection_reason_code: String::new(),
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
                    playlist_position: None,
                    duration_ms: None,
                    duration_approximate: false,
                    genres: String::new(),
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
    // Page contents and its prefix rank must observe the same persisted order.
    let read_tx = connection.unchecked_transaction()?;
    let connection = &*read_tx;
    let sql = "SELECT p.id,p.position,t.id,t.release_id,e.title,a.title,e.artist_names,e.year,EXISTS(SELECT 1 FROM track_source s JOIN local_file_observation l ON l.source_id=s.source_id WHERE s.track_id=t.id AND l.available=1),p.playlist_id,pl.name,t.track_number,t.disc_number,e.duration_ms,e.duration_approximate,e.genre_names FROM playlist_entry p CROSS JOIN track t ON t.id=p.track_id JOIN effective_track_metadata e ON e.track_id=t.id JOIN release r ON r.id=t.release_id JOIN album_application_metadata a ON a.album_id=r.album_id JOIN playlist pl ON pl.id=p.playlist_id WHERE p.playlist_id IN (SELECT value FROM json_each(?1)) AND (p.playlist_id,p.position,p.id)>(?2,?3,?4) AND (?6 IS NULL OR p.id IN (SELECT value FROM json_each(?6))) ORDER BY p.playlist_id,p.position,p.id LIMIT ?5";
    let sql = sql.replace("e.artist_names", &crate::browse::song_artist_credit_sql());
    // Full queue snapshots need Track metadata and entry order, not display positions.
    let sql = if limit.is_none() {
        sql.replace(
            "pl.name,t.track_number,t.disc_number,e.duration_ms,e.duration_approximate,e.genre_names",
            "pl.name",
        )
    } else {
        sql.to_string()
    };
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
    let mut rows = q
        .query_map(
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
                    connection_reason: String::new(),
                    connection_reason_code: String::new(),
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
                    playlist_position: None,
                    duration_ms: if limit.is_some() {
                        r.get::<_, Option<i64>>(13)?
                            .filter(|v| *v >= 0)
                            .map(|v| v as u64)
                    } else {
                        None
                    },
                    duration_approximate: if limit.is_some() { r.get(14)? } else { false },
                    genres: if limit.is_some() {
                        r.get(15)?
                    } else {
                        String::new()
                    },
                    track_number: if limit.is_some() { r.get(11)? } else { None },
                    disc_number: if limit.is_some() { r.get(12)? } else { None },
                    multi_disc: false,
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(q);
    if limit.is_some() && entries.is_empty() {
        // One indexed prefix count per playlist in a bounded chunk, not per row.
        // Stored positions may have gaps; the cursor retains them for keyset paging.
        let mut count = connection
            .prepare("SELECT COUNT(*) FROM playlist_entry WHERE playlist_id=?1 AND position<=?2")?;
        let mut playlist = String::new();
        let mut ordinal = 0_u64;
        for row in &mut rows {
            if row.group != playlist {
                playlist.clone_from(&row.group);
                ordinal = count.query_row(params![playlist, row.cursor.position], |r| {
                    r.get::<_, i64>(0)
                })? as u64;
            }
            row.playlist_position = Some(ordinal);
            if reverse {
                ordinal -= 1;
            } else {
                ordinal += 1;
            }
        }
    }
    read_tx.commit()?;
    Ok(rows)
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
    /// Resolve selected release positions to canonical IDs, then use the shared
    /// aggregate duplicate policy. No saved membership is created.
    pub fn prepare_catalog_playlist_append(
        &mut self,
        playlist: &str,
        release: &crate::catalog::Release,
        positions: &[(u32, u32)],
    ) -> Result<AppendPlan> {
        if positions.is_empty() {
            return Err(Error::Invalid("Select a Song".into()));
        }
        // Validate the destination before creating canonical entities.
        self.store.connection.query_row(
            "SELECT id FROM playlist WHERE id=?1",
            [playlist],
            |r| r.get::<_, String>(0),
        )?;
        let imported = self.ensure_catalog_release(release)?;
        let mut q = self
            .store
            .connection
            .prepare("SELECT id,disc_number,track_number FROM track WHERE release_id=?1")?;
        let rows = q
            .query_map([imported.release_id.as_ref()], |r| {
                Ok((
                    TrackId(r.get(0)?),
                    r.get::<_, Option<u32>>(1)?,
                    r.get::<_, Option<u32>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut by_position: std::collections::HashMap<(u32, u32), Vec<TrackId>> =
            Default::default();
        for (id, disc, number) in rows {
            if let (Some(disc), Some(number)) = (disc, number) {
                by_position.entry((disc, number)).or_default().push(id);
            }
        }
        let tracks = positions
            .iter()
            .map(|position| {
                let matches = by_position
                    .get(position)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                if matches.len() != 1 {
                    return Err(Error::Invalid(
                        "Song position is ambiguous or missing".into(),
                    ));
                }
                Ok(matches[0].clone())
            })
            .collect::<Result<Vec<_>>>()?;
        self.prepare_playlist_append(playlist, tracks)
    }

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

/// Session-only display ordering. It never changes persisted playlist positions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Column {
    #[default]
    Position,
    Title,
    Artist,
    Album,
    Length,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewSort {
    pub column: Column,
    pub descending: bool,
}
impl ViewSort {
    pub fn allows_reordering(self) -> bool {
        self == Self::default()
    }
    fn key(self) -> String {
        let key = match self.column {
            Column::Position => "p.position",
            Column::Title => "e.title COLLATE NOCASE",
            Column::Artist => {
                return format!("{} COLLATE NOCASE", crate::browse::song_artist_credit_sql());
            }
            Column::Album => "a.title COLLATE NOCASE",
            Column::Length => "COALESCE(CASE WHEN e.duration_ms>=0 THEN e.duration_ms END,-1)",
        };
        key.into()
    }
}
// Reconstructible connection-local projection. Only the selected playlists are
// indexed; durable metadata remains on canonical Tracks/Releases. Both external
// commits and same-connection writes invalidate it before the next page fetch.
fn ensure_view_cache(connection: &Connection, playlists: &[String], sort: ViewSort) -> Result<()> {
    use rusqlite::OptionalExtension;
    connection.execute_batch("CREATE TEMP TABLE IF NOT EXISTS playlist_view_cache_state(request TEXT,version INTEGER,changes INTEGER)")?;
    let version: i64 = connection.query_row("PRAGMA main.data_version", [], |r| r.get(0))?;
    let request = format!(
        "{:?}:{}",
        sort.column,
        serde_json::to_string(playlists).expect("IDs")
    );
    let cached = connection
        .query_row(
            "SELECT request,version,changes FROM playlist_view_cache_state",
            [],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;
    if cached.is_some_and(|(r, v, c)| {
        r == request && v == version && c as u64 == connection.total_changes()
    }) {
        return Ok(());
    }
    connection.execute_batch("DROP TABLE IF EXISTS temp.playlist_view_cache; CREATE TEMP TABLE playlist_view_cache(id TEXT PRIMARY KEY,playlist_id TEXT,position INTEGER,ordinal INTEGER,sort_key COLLATE NOCASE); DELETE FROM playlist_view_cache_state;")?;
    let album_join = if matches!(sort.column, Column::Album | Column::Artist) {
        "JOIN track t ON t.id=p.track_id JOIN release r ON r.id=t.release_id JOIN album_application_metadata a ON a.album_id=r.album_id"
    } else {
        ""
    };
    let sql = format!(
        "INSERT INTO playlist_view_cache SELECT p.id,p.playlist_id,p.position,ROW_NUMBER() OVER(PARTITION BY p.playlist_id ORDER BY p.position),{} FROM playlist_entry p INDEXED BY playlist_entry_order JOIN effective_track_metadata e ON e.track_id=p.track_id {album_join} WHERE p.playlist_id IN (SELECT value FROM json_each(?1))",
        sort.key()
    );
    connection.execute(&sql, [serde_json::to_string(playlists).expect("IDs")])?;
    connection.execute_batch("CREATE INDEX temp.playlist_view_cache_order ON playlist_view_cache(sort_key COLLATE NOCASE,playlist_id,position,id)")?;
    connection.execute(
        "INSERT INTO playlist_view_cache_state VALUES(?1,?2,?3)",
        params![request, version, (connection.total_changes() + 1) as i64],
    )?;
    Ok(())
}
fn view_sql(sort: ViewSort, reverse: bool) -> String {
    let descending = sort.descending ^ reverse;
    let direction = if descending { "DESC" } else { "ASC" };
    let comparison = if descending { "<" } else { ">" };
    let bound = if sort.column == Column::Length {
        "?2"
    } else {
        "?2 COLLATE NOCASE"
    };
    format!(
        "SELECT id,playlist_id,position,ordinal,sort_key FROM playlist_view_cache WHERE (sort_key,playlist_id,position,id){comparison}({bound},?3,?4,?5) ORDER BY sort_key COLLATE NOCASE {direction},playlist_id {direction},position {direction},id {direction} LIMIT ?6"
    )
}
fn read_view(
    connection: &Connection,
    playlists: &[String],
    sort: ViewSort,
    after: Option<&Cursor>,
    limit: u32,
    reverse: bool,
) -> Result<Vec<Row>> {
    if sort.column == Column::Position {
        let descending = sort.descending ^ reverse;
        let end = Cursor {
            release: "\u{10ffff}".into(),
            position: i64::MAX,
            id: "\u{10ffff}".into(),
            ..Default::default()
        };
        return read_selected_entries_direction(
            connection,
            playlists,
            after.or(descending.then_some(&end)),
            Some(limit.clamp(1, 201)),
            &[],
            descending,
        );
    }
    let tx = connection.unchecked_transaction()?;
    ensure_view_cache(&tx, playlists, sort)?;
    let sql = if after.is_some() {
        view_sql(sort, reverse)
    } else {
        let direction = if sort.descending ^ reverse {
            "DESC"
        } else {
            "ASC"
        };
        format!(
            "SELECT id,playlist_id,position,ordinal,sort_key FROM playlist_view_cache ORDER BY sort_key COLLATE NOCASE {direction},playlist_id {direction},position {direction},id {direction} LIMIT ?6"
        )
    };
    let mut query = tx.prepare(&sql)?;
    let key = after
        .map(|c| {
            if sort.column == Column::Length {
                rusqlite::types::Value::Integer(c.disc)
            } else {
                rusqlite::types::Value::Text(c.title.clone())
            }
        })
        .unwrap_or(rusqlite::types::Value::Null);
    if after.is_some() {
        query.raw_bind_parameter(2, key)?;
        query.raw_bind_parameter(3, after.map(|c| c.release.as_str()).unwrap_or(""))?;
        query.raw_bind_parameter(4, after.map(|c| c.position).unwrap_or(-1))?;
        query.raw_bind_parameter(5, after.map(|c| c.id.as_str()).unwrap_or(""))?;
    }
    query.raw_bind_parameter(6, limit.clamp(1, 201))?;
    let keys = query
        .raw_query()
        .mapped(|r| {
            let mut cursor = Cursor {
                id: r.get(0)?,
                release: r.get(1)?,
                position: r.get(2)?,
                ..Default::default()
            };
            if sort.column == Column::Length {
                cursor.disc = r.get(4)?;
            } else {
                cursor.title = r.get(4)?;
            }
            Ok((cursor, r.get::<_, i64>(3)? as u64))
        })
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(query);
    // A single bounded metadata query, rather than one query per entry.
    let ids = keys.iter().map(|(c, _)| c.id.clone()).collect::<Vec<_>>();
    if ids.is_empty() {
        tx.commit()?;
        return Ok(Vec::new());
    }
    // Fetch canonical metadata in the same read snapshot.
    let sql = "SELECT p.id,t.id,t.release_id,e.title,a.title,e.artist_names,e.year,EXISTS(SELECT 1 FROM track_source s JOIN local_file_observation l ON l.source_id=s.source_id WHERE s.track_id=t.id AND l.available=1),pl.name,t.track_number,t.disc_number,e.duration_ms,e.duration_approximate,e.genre_names FROM playlist_entry p JOIN track t ON t.id=p.track_id JOIN effective_track_metadata e ON e.track_id=t.id JOIN release r ON r.id=t.release_id JOIN album_application_metadata a ON a.album_id=r.album_id JOIN playlist pl ON pl.id=p.playlist_id WHERE p.id IN (SELECT value FROM json_each(?1))";
    let sql = sql.replace("e.artist_names", &crate::browse::song_artist_credit_sql());
    let mut query = tx.prepare(&sql)?;
    let mut metadata = query
        .query_map([serde_json::to_string(&ids).expect("IDs")], |r| {
            let track = TrackSearchResult {
                track_id: TrackId(r.get(1)?),
                release_id: ReleaseId(r.get(2)?),
                title: r.get(3)?,
                release_title: r.get(4)?,
                artist_names: r.get(5)?,
                year: r.get(6)?,
                available: r.get(7)?,
            };
            Ok(Row {
                connection_reason: String::new(),
                connection_reason_code: String::new(),
                id: r.get(0)?,
                title: track.title.clone(),
                subtitle: track.artist_names.clone(),
                year: track.year,
                group: String::new(),
                group_label: r.get(8)?,
                cursor: Cursor::default(),
                track: Some(track),
                playlist_position: None,
                duration_ms: r
                    .get::<_, Option<i64>>(11)?
                    .filter(|v| *v >= 0)
                    .map(|v| v as u64),
                duration_approximate: r.get(12)?,
                genres: r.get(13)?,
                track_number: r.get(9)?,
                disc_number: r.get(10)?,
                multi_disc: false,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .map(|r| (r.id.clone(), r))
        .collect::<std::collections::HashMap<_, _>>();
    drop(query);
    let rows = keys
        .into_iter()
        .filter_map(|(cursor, ordinal)| {
            metadata.remove(&cursor.id).map(|mut r| {
                r.group.clone_from(&cursor.release);
                r.cursor = cursor;
                r.playlist_position = Some(ordinal);
                r
            })
        })
        .collect();
    tx.commit()?;
    Ok(rows)
}
impl Library {
    pub fn playlist_view(
        &self,
        playlists: &[String],
        sort: ViewSort,
        after: Option<&Cursor>,
        limit: u32,
        reverse: bool,
    ) -> Result<Vec<Row>> {
        read_view(
            &self.store.connection,
            playlists,
            sort,
            after,
            limit,
            reverse,
        )
    }
}
impl crate::browse::QueueReader {
    pub fn playlist_view_range(
        &self,
        playlists: &[String],
        sort: ViewSort,
        anchor: &str,
        target: &str,
    ) -> Result<Vec<String>> {
        if sort.column == Column::Position {
            let mut range = self.playlist_range(playlists, anchor, target)?;
            if sort.descending {
                range.reverse();
            }
            return Ok(range);
        }
        let tx = self.0.unchecked_transaction()?;
        ensure_view_cache(&tx, playlists, sort)?;
        let direction = if sort.descending { "DESC" } else { "ASC" };
        let sql = format!(
            "WITH numbered AS MATERIALIZED (SELECT id,ROW_NUMBER() OVER(ORDER BY sort_key COLLATE NOCASE {direction},playlist_id {direction},position {direction},id {direction}) AS n FROM playlist_view_cache) SELECT id FROM numbered WHERE n BETWEEN (SELECT MIN(n) FROM numbered WHERE id IN (?1,?2)) AND (SELECT MAX(n) FROM numbered WHERE id IN (?1,?2)) ORDER BY n"
        );
        let mut query = tx.prepare(&sql)?;
        let rows = query
            .query_map(params![anchor, target], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(query);
        tx.commit()?;
        Ok(rows)
    }
}

impl crate::browse::QueueReader {
    /// Resolve a stable entry to its current presentation cursor after enrichment.
    pub fn playlist_view_cursor(
        &self,
        playlists: &[String],
        sort: ViewSort,
        id: &str,
    ) -> Result<Option<Cursor>> {
        if sort.column == Column::Position {
            return Ok(self.0.query_row("SELECT playlist_id,position FROM playlist_entry WHERE id=?1 AND playlist_id IN (SELECT value FROM json_each(?2))",params![id,serde_json::to_string(playlists).expect("IDs")], |r| Ok(Cursor {id:id.into(),release:r.get(0)?,position:r.get(1)?,..Default::default()})).optional()?);
        }
        let tx = self.0.unchecked_transaction()?;
        ensure_view_cache(&tx, playlists, sort)?;
        let cursor = tx
            .query_row(
                "SELECT playlist_id,position,sort_key FROM playlist_view_cache WHERE id=?1",
                [id],
                |r| {
                    let mut cursor = Cursor {
                        id: id.into(),
                        release: r.get(0)?,
                        position: r.get(1)?,
                        ..Default::default()
                    };
                    if sort.column == Column::Length {
                        cursor.disc = r.get(2)?;
                    } else {
                        cursor.title = r.get(2)?;
                    }
                    Ok(cursor)
                },
            )
            .optional()?;
        tx.commit()?;
        Ok(cursor)
    }
    pub fn playlist_view(
        &self,
        playlists: &[String],
        sort: ViewSort,
        after: Option<&Cursor>,
        limit: u32,
        reverse: bool,
    ) -> Result<Vec<Row>> {
        read_view(&self.0, playlists, sort, after, limit, reverse)
    }
}

#[cfg(test)]
mod view_plan_tests {
    use super::*;
    #[test]
    fn alternate_playlist_pages_seek_the_temporary_order_index() {
        let mut library = Library::open_in_memory().unwrap();
        let release = library
            .create_catalog_release(&CatalogReleaseInput {
                title: "Release".into(),
                year: None,
                artists: vec![],
                tracks: vec![CatalogTrackInput {
                    title: "Title".into(),
                    disc_number: None,
                    track_number: None,
                    artists: vec![],
                }],
            })
            .unwrap();
        let playlist = library.create_playlist("View").unwrap();
        library
            .append_playlist_track(&playlist, &release.track_ids[0])
            .unwrap();
        for column in [Column::Title, Column::Artist, Column::Album, Column::Length] {
            let sort = ViewSort {
                column,
                descending: false,
            };
            library
                .playlist_view(std::slice::from_ref(&playlist), sort, None, 200, false)
                .unwrap();
            for reverse in [false, true] {
                let mut query = library
                    .store
                    .connection
                    .prepare(&("EXPLAIN QUERY PLAN ".to_string() + &view_sql(sort, reverse)))
                    .unwrap();
                let plan = query
                    .query_map(params![rusqlite::types::Null, "", "", 0, "", 201], |r| {
                        r.get::<_, String>(3)
                    })
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap();
                assert!(
                    plan.iter().any(|p| p.contains(
                        "SEARCH playlist_view_cache USING INDEX playlist_view_cache_order"
                    )),
                    "{plan:?}"
                );
                assert!(!plan.iter().any(|p| p.contains("TEMP B-TREE")), "{plan:?}");
            }
        }
    }
}
