use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use thiserror::Error;

use crate::domain::{
    ArtistCreditInput, ArtistId, CatalogReleaseInput, DiscoveryCandidate, ExternalIdentity,
    ImportReleaseRequest, ImportedRelease, ObservedMetadata, PlayableSource, ReleaseId, RootId,
    SearchRequest, SourceId, SourceLocation, TrackId, TrackSearchResult,
};

const INITIAL_MIGRATION: &str = include_str!("../migrations/0001_initial.sql");
const EXTERNAL_IDENTITIES_MIGRATION: &str =
    include_str!("../migrations/0002_external_identities.sql");
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
    pub fn prepare_album_match(
        &self,
        id: &crate::domain::AlbumId,
    ) -> Result<crate::album_matching::Preparation> {
        prepare_album_match(&self.connection, id)
    }
    pub fn complete_album_match(
        &mut self,
        reply: crate::album_matching::MatchReply,
    ) -> Result<crate::album_matching::MatchOutcome> {
        use crate::album_matching::{MatchOutcome, Preparation};
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        // Artist resolution stands independently of Album success. Persist it even
        // when the following Album request failed, without changing the local name.
        if let Some(identity) = &reply.artist {
            if identity.provider != "musicbrainz"
                || identity.kind != "artist"
                || identity.external_id.is_empty()
            {
                return Err(Error::Invalid("invalid Artist matching identity".into()));
            }
            let name: Option<String> = tx
                .query_row(
                    "SELECT name FROM artist WHERE id=?1",
                    [reply.input.artist_id.as_ref()],
                    |r| r.get(0),
                )
                .optional()?;
            if name.as_deref().map(crate::matching::normalize).as_deref()
                != Some(&reply.input.artist)
            {
                return Ok(MatchOutcome::Skipped);
            }
            let existing = artist_matching_identities(&tx, &reply.input.artist_id)?;
            if existing.iter().any(|id| id != identity) {
                return Ok(MatchOutcome::ArtistAmbiguous(vec![]));
            }
            tx.execute("INSERT INTO artist_external_identity(artist_id,provider,kind,external_id) VALUES (?1,?2,?3,?4) ON CONFLICT DO NOTHING",params![reply.input.artist_id.as_ref(),identity.provider,identity.kind,identity.external_id])?;
        }
        let outcome = match prepare_album_match(&tx, &reply.input.album_id)? {
            Preparation::Done(outcome) => outcome,
            Preparation::Ready(current)
                if current.title != reply.input.title
                    || current.artist != reply.input.artist
                    || current.artist_id != reply.input.artist_id =>
            {
                MatchOutcome::Skipped
            }
            Preparation::Ready(_) => match &reply.outcome {
                MatchOutcome::Matched(identity) | MatchOutcome::MatchedClose(identity) => {
                    if reply.artist.is_none()
                        || identity.provider != "musicbrainz"
                        || identity.kind != "release_group"
                        || identity.external_id.is_empty()
                    {
                        return Err(Error::Invalid("invalid Album matching identity".into()));
                    }
                    tx.execute("INSERT INTO album_external_identity(album_id,provider,kind,external_id) VALUES (?1,?2,?3,?4) ON CONFLICT DO NOTHING", params![reply.input.album_id.as_ref(), identity.provider, identity.kind, identity.external_id])?;
                    reply.outcome
                }
                _ => reply.outcome,
            },
        };
        tx.commit()?;
        Ok(outcome)
    }
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(mut connection: Connection) -> Result<Self> {
        connection.execute_batch(
            "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;",
        )?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version == 0 {
            connection.execute_batch(INITIAL_MIGRATION)?;
            connection.pragma_update(None, "user_version", 1)?;
        } else if version > 6 {
            return Err(Error::Invalid(format!(
                "database schema version {version} is newer than this application supports"
            )));
        }
        if version < 2 {
            connection.execute_batch(EXTERNAL_IDENTITIES_MIGRATION)?;
        }
        if version < 3 {
            connection.execute_batch(include_str!(
                "../migrations/0003_artist_credit_join_phrases.sql"
            ))?;
        }
        if version < 4 {
            connection.execute_batch(include_str!("../migrations/0004_albums.sql"))?;
        }
        if version < 5 {
            let tx =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute_batch(include_str!("../migrations/0005_album_matching.sql"))?;
            // One upgrade-only pass; close the reader before updating its table.
            let ids = tx
                .prepare("SELECT album_id FROM album_application_metadata")?
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for id in ids {
                refresh_album_match_key(&tx, &id)?;
            }
            tx.pragma_update(None, "user_version", 5)?;
            tx.commit()?;
        }
        if version < 6 {
            connection.execute_batch(include_str!(
                "../migrations/0006_artist_external_identities.sql"
            ))?;
        }
        Ok(Self { connection })
    }

    /// Returns true for a new association, false for an identical existing association.
    pub fn attach_artist_external_identity(
        &mut self,
        id: &ArtistId,
        identity: &ExternalIdentity,
    ) -> Result<bool> {
        Ok(self.connection.execute(
            "INSERT INTO artist_external_identity(artist_id, provider, kind, external_id) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(artist_id, provider, kind, external_id) DO NOTHING",
            params![id.as_ref(), identity.provider, identity.kind, identity.external_id],
        )? != 0)
    }

    /// Lists in binary provider/kind/ID order; an unknown entity returns an empty list.
    pub fn list_artist_external_identities(&self, id: &ArtistId) -> Result<Vec<ExternalIdentity>> {
        let mut statement = self.connection.prepare(
            "SELECT provider, kind, external_id FROM artist_external_identity WHERE artist_id = ?1 ORDER BY provider, kind, external_id",
        )?;
        Ok(statement
            .query_map([id.as_ref()], |row| {
                Ok(ExternalIdentity {
                    provider: row.get(0)?,
                    kind: row.get(1)?,
                    external_id: row.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Returns every associated entity; result order is unspecified.
    pub fn resolve_artists_external_identity(
        &self,
        identity: &ExternalIdentity,
    ) -> Result<Vec<ArtistId>> {
        let mut statement = self.connection.prepare(
            "SELECT artist_id FROM artist_external_identity WHERE provider = ?1 AND kind = ?2 AND external_id = ?3",
        )?;
        Ok(statement
            .query_map(
                params![identity.provider, identity.kind, identity.external_id],
                |row| row.get::<_, String>(0).map(ArtistId),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Returns true for a new association, false for an identical existing association.
    pub fn attach_track_external_identity(
        &mut self,
        id: &TrackId,
        identity: &ExternalIdentity,
    ) -> Result<bool> {
        Ok(self.connection.execute(
            "INSERT INTO track_external_identity(track_id, provider, kind, external_id) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(track_id, provider, kind, external_id) DO NOTHING",
            params![id.as_ref(), identity.provider, identity.kind, identity.external_id],
        )? != 0)
    }

    /// Lists in binary provider/kind/ID order; an unknown entity returns an empty list.
    pub fn list_track_external_identities(&self, id: &TrackId) -> Result<Vec<ExternalIdentity>> {
        let mut statement = self.connection.prepare(
            "SELECT provider, kind, external_id FROM track_external_identity WHERE track_id = ?1 ORDER BY provider, kind, external_id",
        )?;
        Ok(statement
            .query_map([id.as_ref()], |row| {
                Ok(ExternalIdentity {
                    provider: row.get(0)?,
                    kind: row.get(1)?,
                    external_id: row.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Returns every associated entity; result order is unspecified.
    pub fn resolve_tracks_external_identity(
        &self,
        identity: &ExternalIdentity,
    ) -> Result<Vec<TrackId>> {
        let mut statement = self.connection.prepare(
            "SELECT track_id FROM track_external_identity WHERE provider = ?1 AND kind = ?2 AND external_id = ?3",
        )?;
        Ok(statement
            .query_map(
                params![identity.provider, identity.kind, identity.external_id],
                |row| row.get::<_, String>(0).map(TrackId),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Returns true for a new association, false for an identical existing association.
    pub fn attach_release_external_identity(
        &mut self,
        id: &ReleaseId,
        identity: &ExternalIdentity,
    ) -> Result<bool> {
        Ok(self.connection.execute(
            "INSERT INTO release_external_identity(release_id, provider, kind, external_id) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(release_id, provider, kind, external_id) DO NOTHING",
            params![id.as_ref(), identity.provider, identity.kind, identity.external_id],
        )? != 0)
    }

    /// Lists in binary provider/kind/ID order; an unknown entity returns an empty list.
    pub fn list_release_external_identities(
        &self,
        id: &ReleaseId,
    ) -> Result<Vec<ExternalIdentity>> {
        let mut statement = self.connection.prepare(
            "SELECT provider, kind, external_id FROM release_external_identity WHERE release_id = ?1 ORDER BY provider, kind, external_id",
        )?;
        Ok(statement
            .query_map([id.as_ref()], |row| {
                Ok(ExternalIdentity {
                    provider: row.get(0)?,
                    kind: row.get(1)?,
                    external_id: row.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Returns every associated entity; result order is unspecified.
    pub fn resolve_releases_external_identity(
        &self,
        identity: &ExternalIdentity,
    ) -> Result<Vec<ReleaseId>> {
        let mut statement = self.connection.prepare(
            "SELECT release_id FROM release_external_identity WHERE provider = ?1 AND kind = ?2 AND external_id = ?3",
        )?;
        Ok(statement
            .query_map(
                params![identity.provider, identity.kind, identity.external_id],
                |row| row.get::<_, String>(0).map(ReleaseId),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn attach_album_external_identity(
        &mut self,
        id: &crate::domain::AlbumId,
        identity: &ExternalIdentity,
    ) -> Result<bool> {
        Ok(self.connection.execute(
            "INSERT INTO album_external_identity(album_id, provider, kind, external_id) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(album_id, provider, kind, external_id) DO NOTHING",
            params![id.as_ref(), identity.provider, identity.kind, identity.external_id],
        )? != 0)
    }

    /// Lists in binary provider/kind/ID order; an unknown entity returns an empty list.
    pub fn list_album_external_identities(
        &self,
        id: &crate::domain::AlbumId,
    ) -> Result<Vec<ExternalIdentity>> {
        let mut statement = self.connection.prepare(
            "SELECT provider, kind, external_id FROM album_external_identity WHERE album_id = ?1 ORDER BY provider, kind, external_id",
        )?;
        Ok(statement
            .query_map([id.as_ref()], |row| {
                Ok(ExternalIdentity {
                    provider: row.get(0)?,
                    kind: row.get(1)?,
                    external_id: row.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Returns every associated entity; result order is unspecified.
    pub fn resolve_albums_external_identity(
        &self,
        identity: &ExternalIdentity,
    ) -> Result<Vec<crate::domain::AlbumId>> {
        let mut statement = self.connection.prepare(
            "SELECT album_id FROM album_external_identity WHERE provider = ?1 AND kind = ?2 AND external_id = ?3",
        )?;
        Ok(statement
            .query_map(
                params![identity.provider, identity.kind, identity.external_id],
                |row| row.get::<_, String>(0).map(crate::domain::AlbumId),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn album_for_release(&self, release_id: &ReleaseId) -> Result<crate::domain::Album> {
        self.connection.query_row(
            "SELECT a.album_id, a.title, a.year, COALESCE((SELECT group_concat(name, '') FROM (
                SELECT ar.name || COALESCE(c.join_phrase, CASE WHEN EXISTS (
                    SELECT 1 FROM album_artist_credit n WHERE n.album_id=c.album_id AND n.position>c.position
                ) THEN ', ' ELSE '' END) AS name
                FROM album_artist_credit c JOIN artist ar ON ar.id=c.artist_id
                WHERE c.album_id=a.album_id ORDER BY c.position)), '')
             FROM release r JOIN album_application_metadata a ON a.album_id=r.album_id WHERE r.id=?1",
            [release_id.as_ref()], |r| Ok(crate::domain::Album {
                album_id: crate::domain::AlbumId(r.get(0)?), title:r.get(1)?, year:r.get(2)?, artist_names:r.get(3)?
            })).map_err(Into::into)
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
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        // Validate and read each source once, before deciding which entities to create.
        let observations = import_observations(&tx, request)?;
        let credits = local_album_credit(request, &observations);
        let album_id = if let Some(ref credits) = credits {
            let artist_key = crate::matching::credit(
                &credits.iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
            )
            .expect("validated credit");
            let candidates = tx.prepare(
                "SELECT album_id FROM album_application_metadata WHERE match_title=?1 AND match_artist_credit=?2 LIMIT 65"
            )?.query_map(params![crate::matching::normalize(&request.release_title), artist_key], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            // Bound pathological same-name groups; never accept from a truncated set.
            let mut supported = Vec::new();
            if candidates.len() <= 64 {
                for id in candidates {
                    if album_has_track_support(&tx, &id, request, &observations)? {
                        supported.push(id);
                        if supported.len() == 2 {
                            break;
                        }
                    }
                }
            }
            if let [id] = supported.as_slice() {
                crate::domain::AlbumId(id.clone())
            } else {
                create_album_tx(&tx, &request.release_title, None, credits)?
            }
        } else {
            create_album_tx(&tx, &request.release_title, None, &request.release_artists)?
        };
        // Album agreement is never evidence of edition identity, even with one stored Release.
        let release_id = ReleaseId::new();
        tx.execute(
            "INSERT INTO release(id, album_id) VALUES (?1, ?2)",
            params![release_id.as_ref(), album_id.as_ref()],
        )?;
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

    pub fn create_catalog_release(
        &mut self,
        input: &CatalogReleaseInput,
    ) -> Result<ImportedRelease> {
        if input.tracks.is_empty() {
            return Err(Error::Invalid(
                "a catalog Release needs at least one Track".into(),
            ));
        }

        let tx = self.connection.transaction()?;
        let result = create_catalog_release_tx(&tx, input, None)?;
        tx.commit()?;
        Ok(result)
    }

    /// Atomic Album + selected edition import. Ambiguous existing primary identity
    /// is an error, never an arbitrary choice among multiple application editions.
    pub fn add_catalog_release(
        &mut self,
        release: &crate::catalog::Release,
    ) -> Result<ImportedRelease> {
        let _total = crate::catalog::Timing::new("persistence.total");
        crate::catalog::Timing::event(format_args!(
            "import_begin tracks={} media={} identities={}",
            release.media.iter().map(|m| m.tracks.len()).sum::<usize>(),
            release.media.len(),
            2 + release.identities.len()
                + release
                    .media
                    .iter()
                    .flat_map(|m| &m.tracks)
                    .map(|t| t.identities.len())
                    .sum::<usize>()
        ));
        let begin = crate::catalog::Timing::detail("persistence.transaction_begin");
        let mut inserted_identities = 0;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        drop(begin);
        let resolve = crate::catalog::Timing::detail("persistence.release_resolve");
        let identity = &release.identity;
        let existing = {
            let mut q = tx.prepare("SELECT release_id FROM release_external_identity WHERE provider=?1 AND kind=?2 AND external_id=?3")?;
            q.query_map(
                params![identity.provider, identity.kind, identity.external_id],
                |r| r.get::<_, String>(0),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?
        };
        if existing.len() > 1 {
            return Err(Error::Invalid("catalog Release identity has multiple existing associations; resolve ambiguity before adding".into()));
        }
        drop(resolve);
        let resolve = crate::catalog::Timing::detail("persistence.album_resolve");
        let album = &release.album;
        let album_ids = {
            let mut q = tx.prepare("SELECT album_id FROM album_external_identity WHERE provider=?1 AND kind=?2 AND external_id=?3")?;
            q.query_map(
                params![
                    album.identity.provider,
                    album.identity.kind,
                    album.identity.external_id
                ],
                |r| r.get::<_, String>(0),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?
        };
        if album_ids.len() > 1 {
            return Err(Error::Invalid(
                "catalog Album identity has multiple associations; resolve ambiguity before adding"
                    .into(),
            ));
        }
        drop(resolve);
        let album_id = if let Some(id) = album_ids.first() {
            crate::domain::AlbumId(id.clone())
        } else if let Some(release_id) = existing.first() {
            // An exact edition identity already anchors an Album; do not manufacture another.
            let id = crate::domain::AlbumId(tx.query_row(
                "SELECT album_id FROM release WHERE id=?1",
                [release_id],
                |r| r.get(0),
            )?);
            {
                let _identity = crate::catalog::Timing::detail("persistence.album_identities");
                inserted_identities += tx.execute("INSERT INTO album_external_identity(album_id,provider,kind,external_id) VALUES (?1,?2,?3,?4)", params![id.as_ref(),album.identity.provider,album.identity.kind,album.identity.external_id])?;
            }
            id
        } else {
            let credits = album
                .credits
                .iter()
                .map(|c| ArtistCreditInput {
                    name: c.name.clone(),
                    role: None,
                })
                .collect::<Vec<_>>();
            let id = create_album_tx(
                &tx,
                &album.title,
                album.date.get(..4).and_then(|v| v.parse().ok()),
                &credits,
            )?;
            {
                let _identity = crate::catalog::Timing::detail("persistence.album_identities");
                inserted_identities += tx.execute("INSERT INTO album_external_identity(album_id,provider,kind,external_id) VALUES (?1,?2,?3,?4)", params![id.as_ref(),album.identity.provider,album.identity.kind,album.identity.external_id])?;
            }
            for (position, credit) in album.credits.iter().enumerate() {
                tx.execute("UPDATE album_artist_credit SET join_phrase=?1 WHERE album_id=?2 AND position=?3", params![credit.join_phrase,id.as_ref(),position as i64])?;
            }
            refresh_album_match_key(&tx, id.as_ref())?;
            id
        };
        let imported = if let Some(id) = existing.first() {
            let owner: String =
                tx.query_row("SELECT album_id FROM release WHERE id=?1", [id], |r| {
                    r.get(0)
                })?;
            if owner != album_id.as_ref() {
                return Err(Error::Invalid(
                    "existing Release belongs to another Album; reconcile explicitly".into(),
                ));
            }
            let release_id = ReleaseId(id.clone());
            let track_ids = {
                let mut q = tx.prepare("SELECT id FROM track WHERE release_id=?1 ORDER BY disc_number, track_number, id")?;
                q.query_map([id], |r| r.get::<_, String>(0).map(TrackId))?
                    .collect::<std::result::Result<Vec<_>, _>>()?
            };
            ImportedRelease {
                release_id,
                track_ids,
            }
        } else {
            let tracks: Vec<_> = release
                .media
                .iter()
                .flat_map(|m| m.tracks.iter().map(move |t| (m.position, t)))
                .collect();
            if tracks.is_empty() {
                return Err(Error::Invalid("catalog Release has no Tracks".into()));
            }
            let credits = |values: &[crate::catalog::Credit]| {
                values
                    .iter()
                    .map(|c| ArtistCreditInput {
                        name: c.name.clone(),
                        role: None,
                    })
                    .collect()
            };
            let input = crate::domain::CatalogReleaseInput {
                title: release.title.clone(),
                year: release.date.get(..4).and_then(|v| v.parse().ok()),
                artists: credits(&release.credits),
                tracks: tracks
                    .iter()
                    .map(|(disc, t)| crate::domain::CatalogTrackInput {
                        title: t.title.clone(),
                        artists: credits(&t.credits),
                        disc_number: Some(*disc),
                        track_number: Some(t.position),
                    })
                    .collect(),
            };
            let imported = create_catalog_release_tx(&tx, &input, Some(&album_id))?;
            for id in std::iter::once(&release.identity).chain(&release.identities) {
                {
                    let _identity =
                        crate::catalog::Timing::detail("persistence.release_identities");
                    inserted_identities += tx.execute("INSERT INTO release_external_identity(release_id,provider,kind,external_id) VALUES (?1,?2,?3,?4) ON CONFLICT DO NOTHING",
                    params![imported.release_id.as_ref(),id.provider,id.kind,id.external_id])?;
                }
            }
            let credits_timer =
                crate::catalog::Timing::detail("persistence.release_credit_phrases");
            for (position, credit) in release.credits.iter().enumerate() {
                tx.execute("UPDATE release_artist_credit SET join_phrase=?1 WHERE release_id=?2 AND position=?3",
                    params![credit.join_phrase,imported.release_id.as_ref(),position as i64])?;
            }
            drop(credits_timer);
            for (track_id, (_, track)) in imported.track_ids.iter().zip(tracks) {
                for id in &track.identities {
                    {
                        let _identity =
                            crate::catalog::Timing::detail("persistence.track_identities");
                        inserted_identities += tx.execute("INSERT INTO track_external_identity(track_id,provider,kind,external_id) VALUES (?1,?2,?3,?4) ON CONFLICT DO NOTHING",
                        params![track_id.as_ref(),id.provider,id.kind,id.external_id])?;
                    }
                }
                let credits_timer =
                    crate::catalog::Timing::detail("persistence.track_credit_phrases");
                for (position, credit) in track.credits.iter().enumerate() {
                    tx.execute("UPDATE track_artist_credit SET join_phrase=?1 WHERE track_id=?2 AND position=?3",
                        params![credit.join_phrase,track_id.as_ref(),position as i64])?;
                }
                drop(credits_timer);
                refresh_effective_track_tx(&tx, track_id)?;
            }
            imported
        };
        let membership = crate::catalog::Timing::detail("persistence.membership");
        for id in &imported.track_ids {
            tx.execute(
                "INSERT INTO library_membership(track_id) VALUES (?1) ON CONFLICT DO NOTHING",
                [id.as_ref()],
            )?;
        }
        drop(membership);
        let commit = crate::catalog::Timing::detail("persistence.commit");
        tx.commit()?;
        drop(commit);
        crate::catalog::Timing::event(format_args!("identities_inserted={inserted_identities}"));
        Ok(imported)
    }

    pub fn add_to_library(&mut self, track_id: &TrackId) -> Result<bool> {
        Ok(self.connection.execute(
            "INSERT OR IGNORE INTO library_membership(track_id) VALUES (?1)",
            [track_id.as_ref()],
        )? > 0)
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

    /// Resolve one available source using existing association indexes, independently of membership.
    pub fn available_playback_source(&self, track_id: &TrackId) -> Result<Option<PlayableSource>> {
        Ok(self
            .connection
            .query_row(
                "SELECT ps.id, l.path
             FROM track_source ts
             CROSS JOIN playable_source ps ON ps.id = ts.source_id
             CROSS JOIN local_file_observation l ON l.source_id = ps.id
             WHERE ts.track_id = ?1 AND ps.kind = 'local_file' AND l.available = 1
             ORDER BY ts.source_id COLLATE BINARY
             LIMIT 1",
                [track_id.as_ref()],
                |row| {
                    Ok(PlayableSource {
                        source_id: SourceId(row.get(0)?),
                        location: SourceLocation::LocalFile(bytes_to_path(row.get(1)?)),
                    })
                },
            )
            .optional()?)
    }

    pub fn search(&self, request: &SearchRequest) -> Result<Vec<TrackSearchResult>> {
        // SQLite deliberately preserves CROSS JOIN order in every availability EXISTS
        // below: probe this Track's associations before its local observations. An
        // ordinary JOIN chose a global available-source scan per Track in measurements.
        // See https://www.sqlite.org/optoverview.html#manual_control_of_query_plans_using_cross_join
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
        if let Some(release_id) = release_id {
            let mut statement = self.connection.prepare(
                "SELECT e.track_id, t.release_id, e.title, e.release_title, e.artist_names, e.year,
                        EXISTS(
                            SELECT 1 FROM track_source ts
                            CROSS JOIN local_file_observation l
                            WHERE ts.track_id = t.id AND l.source_id = ts.source_id AND l.available = 1
                        ) AS available
                 FROM track t
                 JOIN effective_track_metadata e ON e.track_id = t.id
                 JOIN library_membership lm ON lm.track_id = t.id
                 WHERE t.release_id = ?1
                   AND (?2 = '' OR e.rowid IN (
                           SELECT rowid FROM track_search WHERE track_search MATCH ?2
                       ))
                   AND (?3 IS NULL OR EXISTS (
                           SELECT 1 FROM track_artist_credit tac
                           WHERE tac.track_id = t.id AND tac.artist_id = ?3
                       ))
                   AND (?4 IS NULL OR EXISTS(
                           SELECT 1 FROM track_source ts
                           CROSS JOIN local_file_observation l
                           WHERE ts.track_id = t.id AND l.source_id = ts.source_id AND l.available = 1
                       ) = ?4)
                   AND (?5 = '' OR e.title > ?5 OR (e.title = ?5 AND e.track_id > ?6))
                 ORDER BY e.title, e.track_id
                 LIMIT ?7",
            )?;
            let rows = statement.query_map(
                params![
                    release_id,
                    fts_query,
                    artist_id,
                    request.availability,
                    cursor_title,
                    cursor_id,
                    limit
                ],
                map_search_result,
            )?;
            return rows
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Into::into);
        }
        let mut statement = self.connection.prepare(
            "SELECT e.track_id, t.release_id, e.title, e.release_title, e.artist_names, e.year,
                    EXISTS(
                        SELECT 1 FROM track_source ts
                        CROSS JOIN local_file_observation l
                        WHERE ts.track_id = t.id AND l.source_id = ts.source_id AND l.available = 1
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
                       CROSS JOIN local_file_observation l
                       WHERE ts.track_id = t.id AND l.source_id = ts.source_id AND l.available = 1
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
            map_search_result,
        )?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
}

struct ImportObservation {
    album: Option<String>,
    title: Option<String>,
    album_artists: Vec<String>,
    track_artists: Vec<String>,
}

fn import_observations(
    tx: &Transaction<'_>,
    request: &ImportReleaseRequest,
) -> Result<Vec<ImportObservation>> {
    let mut query = tx.prepare(
        "SELECT l.available = 1 AND ts.source_id IS NULL, m.release_title, m.track_title,
          COALESCE((SELECT group_concat(name, char(31)) FROM (SELECT name FROM file_artist_observation WHERE source_id=l.source_id AND scope='release' ORDER BY position)), ''),
          COALESCE((SELECT group_concat(name, char(31)) FROM (SELECT name FROM file_artist_observation WHERE source_id=l.source_id AND scope='track' ORDER BY position)), '')
         FROM local_file_observation l LEFT JOIN track_source ts ON ts.source_id=l.source_id
         LEFT JOIN file_metadata_observation m ON m.source_id=l.source_id WHERE l.source_id=?1"
    )?;
    request
        .tracks
        .iter()
        .map(|track| {
            let result = query
                .query_row([track.source_id.as_ref()], |r| {
                    Ok((
                        r.get::<_, bool>(0)?,
                        ImportObservation {
                            album: r.get(1)?,
                            title: r.get(2)?,
                            album_artists: split_artist_names(r.get(3)?),
                            track_artists: split_artist_names(r.get(4)?),
                        },
                    ))
                })
                .optional()?;
            match result {
                Some((true, observation)) => Ok(observation),
                _ => Err(Error::Invalid(format!(
                    "source {} is unavailable, unknown, or already associated",
                    track.source_id.0
                ))),
            }
        })
        .collect()
}

// Compare within one stored edition, never assemble evidence from incompatible editions.
fn album_has_track_support(
    tx: &Transaction<'_>,
    album_id: &str,
    request: &ImportReleaseRequest,
    observations: &[ImportObservation],
) -> Result<bool> {
    let local = request
        .tracks
        .iter()
        .zip(observations)
        .filter_map(|(input, o)| {
            let title = o
                .title
                .as_deref()
                .filter(|title| crate::matching::usable(title))?;
            Some(crate::matching::TrackEvidence {
                disc: input.disc_number.filter(|n| *n > 0)?,
                position: input.track_number.filter(|n| *n > 0)?,
                title: crate::matching::normalize(title),
            })
        })
        .collect::<Vec<_>>();
    if local.is_empty() {
        return Ok(false);
    }
    let mut query = tx.prepare(
        "SELECT r.id,t.disc_number,t.track_number,e.title FROM release r
         JOIN track t ON t.release_id=r.id
         JOIN effective_track_metadata e ON e.track_id=t.id
         WHERE r.album_id=?1",
    )?;
    let mut editions =
        std::collections::BTreeMap::<String, Vec<crate::matching::TrackEvidence>>::new();
    let rows = query.query_map([album_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<u32>>(1)?,
            r.get::<_, Option<u32>>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (release, disc, position, title) = row?;
        if let (Some(disc), Some(position)) = (disc, position) {
            editions
                .entry(release)
                .or_default()
                .push(crate::matching::TrackEvidence {
                    disc,
                    position,
                    title: crate::matching::normalize(&title),
                });
        }
    }
    Ok(editions
        .values()
        .any(|tracks| crate::matching::tracks_support_album(&local, tracks)))
}

fn local_album_credit(
    request: &ImportReleaseRequest,
    observations: &[ImportObservation],
) -> Option<Vec<ArtistCreditInput>> {
    use crate::matching::{credit, normalize, usable};
    if !usable(&request.release_title)
        || !observations
            .iter()
            .any(|o| o.title.as_deref().is_some_and(usable))
        || observations.iter().any(|o| {
            o.album
                .as_deref()
                .is_some_and(|a| !usable(a) || normalize(a) != normalize(&request.release_title))
        })
    {
        return None;
    }
    let explicit = request
        .release_artists
        .iter()
        .map(|c| c.name.clone())
        .collect::<Vec<_>>();
    let tagged = observations
        .iter()
        .filter(|o| !o.album_artists.is_empty())
        .map(|o| o.album_artists.clone())
        .collect::<Vec<_>>();
    let evidence = if !explicit.is_empty() {
        let key = credit(&explicit)?;
        if tagged
            .iter()
            .any(|names| credit(names).as_ref() != Some(&key))
        {
            return None;
        }
        return Some(request.release_artists.clone());
    } else if !tagged.is_empty() {
        tagged
    } else {
        request
            .tracks
            .iter()
            .zip(observations)
            .map(|(input, o)| {
                if input.artists.is_empty() {
                    o.track_artists.clone()
                } else {
                    input.artists.iter().map(|c| c.name.clone()).collect()
                }
            })
            .collect()
    };
    let first = evidence.first()?;
    let key = credit(first)?;
    if evidence
        .iter()
        .any(|names| credit(names).as_ref() != Some(&key))
    {
        return None;
    }
    Some(
        first
            .iter()
            .map(|name| ArtistCreditInput {
                name: name.clone(),
                role: None,
            })
            .collect(),
    )
}

fn refresh_album_match_key(tx: &Transaction<'_>, album_id: &str) -> Result<()> {
    let title: String = tx.query_row(
        "SELECT title FROM album_application_metadata WHERE album_id=?1",
        [album_id],
        |r| r.get(0),
    )?;
    let mut query = tx.prepare("SELECT a.name, c.join_phrase FROM album_artist_credit c JOIN artist a ON a.id=c.artist_id WHERE c.album_id=?1 ORDER BY c.position")?;
    let credits = query
        .query_map([album_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut display = String::new();
    for (index, (name, phrase)) in credits.iter().enumerate() {
        display.push_str(name);
        display.push_str(phrase.as_deref().unwrap_or(if index + 1 < credits.len() {
            ", "
        } else {
            ""
        }));
    }
    tx.execute("UPDATE album_application_metadata SET match_title=?1, match_artist_credit=?2 WHERE album_id=?3", params![crate::matching::normalize(&title), crate::matching::normalize(&display), album_id])?;
    Ok(())
}

fn create_album_tx(
    tx: &Transaction<'_>,
    title: &str,
    year: Option<i32>,
    credits: &[ArtistCreditInput],
) -> Result<crate::domain::AlbumId> {
    let _create = crate::catalog::Timing::detail("persistence.album_create_metadata_credits");
    let id = crate::domain::AlbumId::new();
    tx.execute("INSERT INTO album(id) VALUES (?1)", [id.as_ref()])?;
    tx.execute(
        "INSERT INTO album_application_metadata(album_id,title,year) VALUES (?1,?2,?3)",
        params![id.as_ref(), title, year],
    )?;
    insert_credits(tx, "album_artist_credit", id.as_ref(), credits)?;
    refresh_album_match_key(tx, id.as_ref())?;
    Ok(id)
}

fn create_catalog_release_tx(
    tx: &Transaction<'_>,
    input: &CatalogReleaseInput,
    album_id: Option<&crate::domain::AlbumId>,
) -> Result<ImportedRelease> {
    let create = crate::catalog::Timing::detail("persistence.release_create");
    let release_id = ReleaseId::new();
    let album_id = match album_id {
        Some(id) => id.clone(),
        None => create_album_tx(tx, &input.title, input.year, &input.artists)?,
    };
    tx.execute(
        "INSERT INTO release(id, album_id) VALUES (?1, ?2)",
        params![release_id.as_ref(), album_id.as_ref()],
    )?;
    drop(create);
    let metadata = crate::catalog::Timing::detail("persistence.release_metadata_credits");
    tx.execute(
        "INSERT INTO release_application_metadata(release_id, title, year)
             VALUES (?1, ?2, ?3)",
        params![release_id.as_ref(), input.title, input.year],
    )?;
    insert_credits(
        tx,
        "release_artist_credit",
        release_id.as_ref(),
        &input.artists,
    )?;

    drop(metadata);
    let mut track_ids = Vec::with_capacity(input.tracks.len());
    for track in &input.tracks {
        let create = crate::catalog::Timing::detail("persistence.track_create");
        let track_id = TrackId::new();
        tx.execute(
            "INSERT INTO track(id, release_id, disc_number, track_number)
                 VALUES (?1, ?2, ?3, ?4)",
            params![
                track_id.as_ref(),
                release_id.as_ref(),
                track.disc_number,
                track.track_number
            ],
        )?;
        drop(create);
        let metadata = crate::catalog::Timing::detail("persistence.track_metadata_credits");
        tx.execute(
            "INSERT INTO track_application_metadata(track_id, title) VALUES (?1, ?2)",
            params![track_id.as_ref(), track.title],
        )?;
        insert_credits(tx, "track_artist_credit", track_id.as_ref(), &track.artists)?;
        drop(metadata);
        refresh_effective_track_tx(tx, &track_id)?;
        track_ids.push(track_id);
    }
    Ok(ImportedRelease {
        release_id,
        track_ids,
    })
}

fn map_search_result(row: &rusqlite::Row<'_>) -> rusqlite::Result<TrackSearchResult> {
    Ok(TrackSearchResult {
        track_id: TrackId(row.get(0)?),
        release_id: ReleaseId(row.get(1)?),
        title: row.get(2)?,
        release_title: row.get(3)?,
        artist_names: row.get(4)?,
        year: row.get(5)?,
        available: row.get(6)?,
    })
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
        "album_artist_credit" => "album_id",
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
    let _refresh = crate::catalog::Timing::detail("persistence.effective_fts");
    let changed = connection.execute(
        "INSERT INTO effective_track_metadata(
            track_id, title, release_title, artist_names, year, duration_ms, format
         )
         SELECT t.id,
                COALESCE(o.value, f.track_title, app.title, ''),
                r.title,
                COALESCE((
                    SELECT group_concat(name, '') FROM (
                        SELECT a.name || COALESCE(c.join_phrase,
                            CASE WHEN EXISTS (SELECT 1 FROM track_artist_credit next
                                WHERE next.track_id=c.track_id AND next.position>c.position)
                            THEN ', ' ELSE '' END) AS name
                        FROM track_artist_credit c
                        JOIN artist a ON a.id = c.artist_id
                        WHERE c.track_id = t.id ORDER BY c.position
                    )
                ), ''),
                COALESCE(f.year, r.year), f.duration_ms, f.format
         FROM track t
         JOIN release edition ON edition.id = t.release_id
         JOIN album_application_metadata r ON r.album_id = edition.album_id
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

fn prepare_album_match(
    db: &Connection,
    id: &crate::domain::AlbumId,
) -> Result<crate::album_matching::Preparation> {
    use crate::album_matching::{MatchInput, MatchOutcome, Preparation};
    let (title, artist): (String, String) = db.query_row(
        "SELECT match_title,match_artist_credit FROM album_application_metadata WHERE album_id=?1",
        [id.as_ref()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if db.query_row("SELECT EXISTS(SELECT 1 FROM album_external_identity WHERE album_id=?1 AND provider='musicbrainz' AND kind='release_group')", [id.as_ref()], |r|r.get::<_,bool>(0))? {
        return Ok(Preparation::Done(MatchOutcome::AlreadyMatched));
    }
    if !crate::album_matching::eligible_text(&title, &artist) {
        return Ok(Preparation::Done(MatchOutcome::Skipped));
    }
    let credits = db.prepare("SELECT c.artist_id,a.name,c.join_phrase FROM album_artist_credit c JOIN artist a ON a.id=c.artist_id WHERE c.album_id=?1 ORDER BY c.position")?.query_map([id.as_ref()], |r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let [(artist_id, name, phrase)] = credits.as_slice() else {
        return Ok(Preparation::Done(MatchOutcome::ArtistAmbiguous(vec![])));
    };
    if phrase.as_deref().is_some_and(|s| !s.trim().is_empty())
        || crate::matching::normalize(name) != artist
    {
        return Ok(Preparation::Done(MatchOutcome::ArtistAmbiguous(vec![])));
    }
    let artist_id = ArtistId(artist_id.clone());
    let mut known = artist_matching_identities(db, &artist_id)?;
    if known.len() > 1 {
        return Ok(Preparation::Done(MatchOutcome::ArtistAmbiguous(
            known
                .into_iter()
                .map(|identity| crate::catalog::ArtistCandidate {
                    identity,
                    name: name.clone(),
                    comment: "Multiple stored identities".into(),
                    country: String::new(),
                    artist_type: String::new(),
                    score: None,
                })
                .collect(),
        )));
    }
    let known_artist = known.pop();
    let mut query = db.prepare("SELECT m.track_title FROM release r JOIN track t ON t.release_id=r.id JOIN track_source ts ON ts.track_id=t.id JOIN file_metadata_observation m ON m.source_id=ts.source_id WHERE r.album_id=?1")?;
    let mut titles = query.query_map([id.as_ref()], |r| r.get::<_, Option<String>>(0))?;
    let mut usable = false;
    for title in &mut titles {
        if title?.as_deref().is_some_and(crate::matching::usable) {
            usable = true;
            break;
        }
    }
    Ok(if usable {
        Preparation::Ready(MatchInput {
            album_id: id.clone(),
            title,
            artist,
            artist_id,
            known_artist,
        })
    } else {
        Preparation::Done(MatchOutcome::Skipped)
    })
}

fn artist_matching_identities(db: &Connection, id: &ArtistId) -> Result<Vec<ExternalIdentity>> {
    Ok(db.prepare("SELECT external_id FROM artist_external_identity WHERE artist_id=?1 AND provider='musicbrainz' AND kind='artist'")?.query_map([id.as_ref()], |r|Ok(ExternalIdentity { provider:"musicbrainz".into(),kind:"artist".into(),external_id:r.get(0)? }))?.collect::<rusqlite::Result<Vec<_>>>()?)
}
