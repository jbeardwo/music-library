use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use thiserror::Error;

use crate::domain::{
    ArtistCreditInput, ArtistId, DiscoveryCandidate, ImportReleaseRequest, ImportedRelease,
    ObservedMetadata, ReleaseId, RootId, SearchRequest, SourceId, TrackId, TrackSearchResult,
};

const INITIAL_MIGRATION: &str = include_str!("../migrations/0001_initial.sql");
const MAX_PAGE_SIZE: u32 = 200;

#[derive(Debug, Error)]
pub enum Error {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("filesystem error at {path}: {source}")]
    Filesystem {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("metadata could not be read from {path}: {message}")]
    Metadata { path: PathBuf, message: String },
    #[error("invalid operation: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Store {
    connection: Connection,
}

#[derive(Clone, Debug)]
pub(crate) struct KnownLocalSource {
    pub source_id: SourceId,
    pub size_bytes: u64,
    pub modified_ns: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct ScannedLocalSource {
    pub source_id: Option<SourceId>,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub modified_ns: i64,
    pub metadata: Option<ObservedMetadata>,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> Result<Self> {
        connection.execute_batch(
            "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;",
        )?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version == 0 {
            connection.execute_batch(INITIAL_MIGRATION)?;
            connection.pragma_update(None, "user_version", 1)?;
        } else if version != 1 {
            return Err(Error::Invalid(format!(
                "database schema version {version} is newer than this application supports"
            )));
        }
        Ok(Self { connection })
    }

    pub fn register_local_root(&mut self, path: impl AsRef<Path>) -> Result<RootId> {
        let location = path_to_bytes(path.as_ref());
        if let Some(id) = self
            .connection
            .query_row(
                "SELECT id FROM discovery_root WHERE kind = 'local_filesystem' AND location = ?1",
                [&location],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            return Ok(RootId(id));
        }
        let id = RootId::new();
        self.connection.execute(
            "INSERT INTO discovery_root(id, kind, location) VALUES (?1, 'local_filesystem', ?2)",
            params![id.as_ref(), location],
        )?;
        Ok(id)
    }

    pub(crate) fn root_path(&self, root_id: &RootId) -> Result<PathBuf> {
        self.connection
            .query_row(
                "SELECT location FROM discovery_root WHERE id = ?1",
                [root_id.as_ref()],
                |row| row.get::<_, Vec<u8>>(0).map(bytes_to_path),
            )
            .optional()?
            .ok_or_else(|| Error::Invalid(format!("unknown discovery root {}", root_id.0)))
    }

    pub(crate) fn begin_scan(&mut self, root_id: &RootId) -> Result<i64> {
        self.connection.execute(
            "INSERT INTO scan_run(root_id, status) VALUES (?1, 'running')",
            [root_id.as_ref()],
        )?;
        Ok(self.connection.last_insert_rowid())
    }

    pub(crate) fn fail_scan(&mut self, scan_id: i64) -> Result<()> {
        self.connection.execute(
            "UPDATE scan_run SET status = 'failed' WHERE id = ?1 AND status = 'running'",
            [scan_id],
        )?;
        Ok(())
    }

    pub(crate) fn known_local_source(
        &self,
        root_id: &RootId,
        path: &Path,
    ) -> Result<Option<KnownLocalSource>> {
        let path = path_to_bytes(path);
        self.connection
            .query_row(
                "SELECT source_id, size_bytes, modified_ns
                 FROM local_file_observation WHERE root_id = ?1 AND path = ?2",
                params![root_id.as_ref(), path],
                |row| {
                    Ok(KnownLocalSource {
                        source_id: SourceId(row.get(0)?),
                        size_bytes: row.get::<_, i64>(1)? as u64,
                        modified_ns: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub(crate) fn apply_scan_batch(
        &mut self,
        root_id: &RootId,
        scan_id: i64,
        items: &[ScannedLocalSource],
    ) -> Result<()> {
        let tx = self.connection.transaction()?;
        for item in items {
            let source_id = item.source_id.clone().unwrap_or_else(SourceId::new);
            tx.execute(
                "INSERT OR IGNORE INTO playable_source(id, kind) VALUES (?1, 'local_file')",
                [source_id.as_ref()],
            )?;
            tx.execute(
                "INSERT INTO local_file_observation(
                    source_id, root_id, path, size_bytes, modified_ns, available, last_seen_scan_id
                 ) VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)
                 ON CONFLICT(root_id, path) DO UPDATE SET
                    size_bytes = excluded.size_bytes,
                    modified_ns = excluded.modified_ns,
                    available = 1,
                    last_seen_scan_id = excluded.last_seen_scan_id,
                    last_observed_at = unixepoch()",
                params![
                    source_id.as_ref(),
                    root_id.as_ref(),
                    path_to_bytes(&item.path),
                    item.size_bytes as i64,
                    item.modified_ns,
                    scan_id
                ],
            )?;
            if let Some(metadata) = &item.metadata {
                write_file_metadata(&tx, &source_id, metadata)?;
                refresh_associated_effective_track(&tx, &source_id)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn complete_scan(&mut self, root_id: &RootId, scan_id: i64) -> Result<u64> {
        let tx = self.connection.transaction()?;
        let changed = tx.execute(
            "UPDATE local_file_observation
             SET available = 0, last_observed_at = unixepoch()
             WHERE root_id = ?1 AND available = 1
               AND (last_seen_scan_id IS NULL OR last_seen_scan_id <> ?2)",
            params![root_id.as_ref(), scan_id],
        )?;
        tx.execute(
            "UPDATE scan_run SET status = 'completed', completed_at = unixepoch()
             WHERE id = ?1 AND root_id = ?2 AND status = 'running'",
            params![scan_id, root_id.as_ref()],
        )?;
        tx.commit()?;
        Ok(changed as u64)
    }

    pub fn list_discovery_candidates(
        &self,
        after_source_id: Option<&SourceId>,
        limit: u32,
    ) -> Result<Vec<DiscoveryCandidate>> {
        let limit = bounded_limit(limit);
        let after = after_source_id.map(AsRef::as_ref).unwrap_or("");
        let mut statement = self.connection.prepare(
            "SELECT ps.id, l.path, l.available,
                    m.track_title, m.release_title, m.disc_number, m.track_number,
                    m.year, m.duration_ms, m.format,
                    COALESCE((
                        SELECT group_concat(name, char(31)) FROM (
                            SELECT name FROM file_artist_observation
                            WHERE source_id = ps.id AND scope = 'track' ORDER BY position
                        )
                    ), ''),
                    COALESCE((
                        SELECT group_concat(name, char(31)) FROM (
                            SELECT name FROM file_artist_observation
                            WHERE source_id = ps.id AND scope = 'release' ORDER BY position
                        )
                    ), '')
             FROM playable_source ps
             JOIN local_file_observation l ON l.source_id = ps.id
             LEFT JOIN file_metadata_observation m ON m.source_id = ps.id
             LEFT JOIN track_source ts ON ts.source_id = ps.id
             WHERE ts.source_id IS NULL AND ps.id > ?1
             ORDER BY ps.id LIMIT ?2",
        )?;
        let rows = statement.query_map(params![after, limit], |row| {
            Ok((
                SourceId(row.get(0)?),
                bytes_to_path(row.get::<_, Vec<u8>>(1)?),
                row.get::<_, bool>(2)?,
                ObservedMetadata {
                    track_title: row.get(3)?,
                    release_title: row.get(4)?,
                    disc_number: row.get::<_, Option<u32>>(5)?,
                    track_number: row.get::<_, Option<u32>>(6)?,
                    year: row.get(7)?,
                    duration_ms: row.get::<_, Option<i64>>(8)?.map(|value| value as u64),
                    format: row.get(9)?,
                    track_artists: split_artist_names(row.get(10)?),
                    release_artists: split_artist_names(row.get(11)?),
                },
            ))
        })?;
        let mut candidates = Vec::new();
        for row in rows {
            let (source_id, path, available, metadata) = row?;
            candidates.push(DiscoveryCandidate {
                source_id,
                path,
                available,
                metadata,
            });
        }
        Ok(candidates)
    }

    pub fn import_release(&mut self, request: &ImportReleaseRequest) -> Result<ImportedRelease> {
        if request.tracks.is_empty() {
            return Err(Error::Invalid(
                "a Release import needs at least one Track".into(),
            ));
        }
        let tx = self.connection.transaction()?;
        let release_id = ReleaseId::new();
        tx.execute("INSERT INTO release(id) VALUES (?1)", [release_id.as_ref()])?;
        tx.execute(
            "INSERT INTO release_application_metadata(release_id, title) VALUES (?1, ?2)",
            params![release_id.as_ref(), request.release_title],
        )?;
        insert_credits(
            &tx,
            "release_artist_credit",
            release_id.as_ref(),
            &request.release_artists,
        )?;

        let mut track_ids = Vec::with_capacity(request.tracks.len());
        for input in &request.tracks {
            let eligible: bool = tx
                .query_row(
                    "SELECT l.available = 1 AND ts.source_id IS NULL
                     FROM playable_source ps
                     JOIN local_file_observation l ON l.source_id = ps.id
                     LEFT JOIN track_source ts ON ts.source_id = ps.id
                     WHERE ps.id = ?1",
                    [input.source_id.as_ref()],
                    |row| row.get(0),
                )
                .optional()?
                .unwrap_or(false);
            if !eligible {
                return Err(Error::Invalid(format!(
                    "source {} is unavailable, unknown, or already associated",
                    input.source_id.0
                )));
            }
            let track_id = TrackId::new();
            tx.execute(
                "INSERT INTO track(id, release_id, disc_number, track_number)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    track_id.as_ref(),
                    release_id.as_ref(),
                    input.disc_number,
                    input.track_number
                ],
            )?;
            if let Some(title) = &input.title_fallback {
                tx.execute(
                    "INSERT INTO track_application_metadata(track_id, title) VALUES (?1, ?2)",
                    params![track_id.as_ref(), title],
                )?;
            }
            tx.execute(
                "INSERT INTO track_source(track_id, source_id) VALUES (?1, ?2)",
                params![track_id.as_ref(), input.source_id.as_ref()],
            )?;
            tx.execute(
                "INSERT INTO library_membership(track_id) VALUES (?1)",
                [track_id.as_ref()],
            )?;
            insert_credits(
                &tx,
                "track_artist_credit",
                track_id.as_ref(),
                &input.artists,
            )?;
            refresh_effective_track_tx(&tx, &track_id)?;
            track_ids.push(track_id);
        }
        tx.commit()?;
        Ok(ImportedRelease {
            release_id,
            track_ids,
        })
    }

    pub fn remove_from_library(&mut self, track_id: &TrackId) -> Result<bool> {
        Ok(self.connection.execute(
            "DELETE FROM library_membership WHERE track_id = ?1",
            [track_id.as_ref()],
        )? > 0)
    }

    pub fn set_track_title_override(&mut self, track_id: &TrackId, value: &str) -> Result<()> {
        let tx = self.connection.transaction()?;
        tx.execute(
            "INSERT INTO track_title_override(track_id, value) VALUES (?1, ?2)
             ON CONFLICT(track_id) DO UPDATE SET value = excluded.value, updated_at = unixepoch()",
            params![track_id.as_ref(), value],
        )?;
        refresh_effective_track_tx(&tx, track_id)?;
        tx.commit()?;
        Ok(())
    }

    pub fn clear_track_title_override(&mut self, track_id: &TrackId) -> Result<bool> {
        let tx = self.connection.transaction()?;
        let changed = tx.execute(
            "DELETE FROM track_title_override WHERE track_id = ?1",
            [track_id.as_ref()],
        )? > 0;
        refresh_effective_track_tx(&tx, track_id)?;
        tx.commit()?;
        Ok(changed)
    }

    pub fn search(&self, request: &SearchRequest) -> Result<Vec<TrackSearchResult>> {
        let limit = bounded_limit(request.limit);
        let cursor_title = request
            .after
            .as_ref()
            .map(|c| c.title.as_str())
            .unwrap_or("");
        let cursor_id = request
            .after
            .as_ref()
            .map(|c| c.track_id.as_ref())
            .unwrap_or("");
        let release_id = request.release_id.as_ref().map(AsRef::as_ref);
        let artist_id = request.artist_id.as_ref().map(AsRef::as_ref);
        let fts_query = fts_prefix_query(&request.text);
        let mut statement = self.connection.prepare(
            "SELECT e.track_id, t.release_id, e.title, e.release_title, e.artist_names,
                    EXISTS(
                        SELECT 1 FROM track_source ts
                        JOIN local_file_observation l ON l.source_id = ts.source_id
                        WHERE ts.track_id = t.id AND l.available = 1
                    ) AS available
             FROM effective_track_metadata e
             JOIN track t ON t.id = e.track_id
             JOIN library_membership lm ON lm.track_id = t.id
             WHERE (?1 = '' OR e.rowid IN (
                       SELECT rowid FROM track_search WHERE track_search MATCH ?1
                   ))
               AND (?2 IS NULL OR t.release_id = ?2)
               AND (?3 IS NULL OR EXISTS (
                       SELECT 1 FROM track_artist_credit tac
                       WHERE tac.track_id = t.id AND tac.artist_id = ?3
                   ))
               AND (?4 IS NULL OR EXISTS(
                       SELECT 1 FROM track_source ts
                       JOIN local_file_observation l ON l.source_id = ts.source_id
                       WHERE ts.track_id = t.id AND l.available = 1
                   ) = ?4)
               AND (?5 = '' OR e.title > ?5 OR (e.title = ?5 AND e.track_id > ?6))
             ORDER BY e.title, e.track_id
             LIMIT ?7",
        )?;
        let rows = statement.query_map(
            params![
                fts_query,
                release_id,
                artist_id,
                request.availability,
                cursor_title,
                cursor_id,
                limit
            ],
            |row| {
                Ok(TrackSearchResult {
                    track_id: TrackId(row.get(0)?),
                    release_id: ReleaseId(row.get(1)?),
                    title: row.get(2)?,
                    release_title: row.get(3)?,
                    artist_names: row.get(4)?,
                    available: row.get(5)?,
                })
            },
        )?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
}

fn bounded_limit(limit: u32) -> u32 {
    limit.clamp(1, MAX_PAGE_SIZE)
}

fn fts_prefix_query(input: &str) -> String {
    input
        .split_whitespace()
        .filter(|part| !part.is_empty())
        .map(|part| format!("\"{}\"*", part.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn split_artist_names(names: String) -> Vec<String> {
    if names.is_empty() {
        Vec::new()
    } else {
        names.split('\u{1f}').map(str::to_owned).collect()
    }
}

fn write_file_metadata(
    tx: &Transaction<'_>,
    source_id: &SourceId,
    metadata: &ObservedMetadata,
) -> Result<()> {
    tx.execute(
        "INSERT INTO file_metadata_observation(
            source_id, track_title, release_title, disc_number, track_number,
            year, duration_ms, format
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(source_id) DO UPDATE SET
            track_title = excluded.track_title,
            release_title = excluded.release_title,
            disc_number = excluded.disc_number,
            track_number = excluded.track_number,
            year = excluded.year,
            duration_ms = excluded.duration_ms,
            format = excluded.format,
            observed_at = unixepoch()",
        params![
            source_id.as_ref(),
            metadata.track_title,
            metadata.release_title,
            metadata.disc_number,
            metadata.track_number,
            metadata.year,
            metadata.duration_ms.map(|value| value as i64),
            metadata.format
        ],
    )?;
    tx.execute(
        "DELETE FROM file_artist_observation WHERE source_id = ?1",
        [source_id.as_ref()],
    )?;
    for (scope, artists) in [
        ("track", &metadata.track_artists),
        ("release", &metadata.release_artists),
    ] {
        for (position, name) in artists.iter().enumerate() {
            tx.execute(
                "INSERT INTO file_artist_observation(source_id, scope, position, name)
                 VALUES (?1, ?2, ?3, ?4)",
                params![source_id.as_ref(), scope, position as i64, name],
            )?;
        }
    }
    Ok(())
}

fn insert_credits(
    tx: &Transaction<'_>,
    table: &str,
    entity_id: &str,
    credits: &[ArtistCreditInput],
) -> Result<()> {
    let entity_column = match table {
        "track_artist_credit" => "track_id",
        "release_artist_credit" => "release_id",
        _ => return Err(Error::Invalid("unsupported credit table".into())),
    };
    let sql = format!(
        "INSERT INTO {table}({entity_column}, position, artist_id, role) VALUES (?1, ?2, ?3, ?4)"
    );
    for (position, credit) in credits.iter().enumerate() {
        let artist_id = ArtistId::new();
        tx.execute(
            "INSERT INTO artist(id, name) VALUES (?1, ?2)",
            params![artist_id.as_ref(), credit.name],
        )?;
        tx.execute(
            &sql,
            params![entity_id, position as i64, artist_id.as_ref(), credit.role],
        )?;
    }
    Ok(())
}

fn refresh_associated_effective_track(tx: &Transaction<'_>, source_id: &SourceId) -> Result<()> {
    let track_id = tx
        .query_row(
            "SELECT track_id FROM track_source WHERE source_id = ?1",
            [source_id.as_ref()],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(track_id) = track_id {
        refresh_effective_track_tx(tx, &TrackId(track_id))?;
    }
    Ok(())
}

fn refresh_effective_track_tx(tx: &Transaction<'_>, track_id: &TrackId) -> Result<()> {
    refresh_effective_track_impl(tx, track_id)
}

#[cfg(unix)]
fn path_to_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
fn bytes_to_path(bytes: Vec<u8>) -> PathBuf {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(OsString::from_vec(bytes))
}

#[cfg(windows)]
fn path_to_bytes(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(windows)]
fn bytes_to_path(bytes: Vec<u8>) -> PathBuf {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    let wide = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    PathBuf::from(OsString::from_wide(&wide))
}

fn refresh_effective_track_impl(connection: &Connection, track_id: &TrackId) -> Result<()> {
    let changed = connection.execute(
        "INSERT INTO effective_track_metadata(
            track_id, title, release_title, artist_names, year, duration_ms, format
         )
         SELECT t.id,
                COALESCE(o.value, f.track_title, app.title, ''),
                r.title,
                COALESCE((
                    SELECT group_concat(name, ', ') FROM (
                        SELECT a.name AS name
                        FROM track_artist_credit c
                        JOIN artist a ON a.id = c.artist_id
                        WHERE c.track_id = t.id ORDER BY c.position
                    )
                ), ''),
                COALESCE(f.year, r.year), f.duration_ms, f.format
         FROM track t
         JOIN release_application_metadata r ON r.release_id = t.release_id
         LEFT JOIN track_application_metadata app ON app.track_id = t.id
         LEFT JOIN track_title_override o ON o.track_id = t.id
         LEFT JOIN track_source ts ON ts.track_id = t.id
            AND NOT EXISTS (
                SELECT 1 FROM track_source other
                WHERE other.track_id = t.id AND other.source_id <> ts.source_id
            )
         LEFT JOIN file_metadata_observation f ON f.source_id = ts.source_id
         WHERE t.id = ?1
         ON CONFLICT(track_id) DO UPDATE SET
            title = excluded.title,
            release_title = excluded.release_title,
            artist_names = excluded.artist_names,
            year = excluded.year,
            duration_ms = excluded.duration_ms,
            format = excluded.format",
        [track_id.as_ref()],
    )?;
    if changed == 0 {
        return Err(Error::Invalid(format!("unknown Track {}", track_id.0)));
    }
    connection.execute(
        "DELETE FROM track_search
         WHERE rowid = (SELECT rowid FROM effective_track_metadata WHERE track_id = ?1)",
        [track_id.as_ref()],
    )?;
    connection.execute(
        "INSERT INTO track_search(rowid, track_id, title, artist_names, release_title)
         SELECT rowid, track_id, title, artist_names, release_title
         FROM effective_track_metadata WHERE track_id = ?1",
        [track_id.as_ref()],
    )?;
    Ok(())
}
