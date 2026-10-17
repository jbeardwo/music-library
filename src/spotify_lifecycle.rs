//! Durable reconciliation lifecycle. This module performs no provider requests.
use crate::{
    Library,
    catalog::Page,
    domain::{AlbumId, TrackId},
    song_resolution::{Candidate, Input},
    storage::{Error, Result},
};
use rusqlite::params;

/// Increment when acceptance/evidence semantics change. Successful identities are never invalidated.
pub const EVALUATION_VERSION: u32 = 15;
pub const LOCAL_BATCH: u32 = 32;
/// Memory/query chunk size, never the maximum lifetime of a reconciliation run.
pub const RECONCILIATION_ALBUM_BATCH_SIZE: u32 = 20;
/// Compatibility alias for individual bounded Album operations.
pub const RETRY_ALBUMS: u32 = RECONCILIATION_ALBUM_BATCH_SIZE;
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct LocalBatch {
    pub artist_revisions: Vec<crate::domain::ArtistId>,
    pub examined: usize,
    pub updated: Vec<TrackId>,
    pub accepted: Vec<TrackId>,
    pub needs_retry: usize,
    pub provider_searches: usize,
    pub still_unresolved: usize,
    pub errors: Vec<String>,
}
fn json<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|e| Error::Invalid(e.to_string()))
}
pub(crate) fn excluded(db: &rusqlite::Connection, track: &TrackId, provider: &str) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM track_provider_exclusion WHERE track_id=?1 AND provider=?2)",
        params![track.as_ref(), provider],
        |r| r.get(0),
    )?)
}
pub(crate) fn album_unexcluded(db: &rusqlite::Connection, album: &AlbumId) -> Result<bool> {
    Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM track t JOIN release r ON r.id=t.release_id WHERE r.album_id=?1 AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=t.id AND x.provider='spotify'))",[album.as_ref()],|r|r.get(0))?)
}
impl Library {
    pub fn defer_spotify_album_provider(&mut self, album: &AlbumId) -> Result<()> {
        self.store.connection.execute("UPDATE spotify_connection_review SET reason_code='provider_unavailable',reason='Provider work is unavailable; cached/local work completed',stale=0,evaluation_version=?2 WHERE state='needs_retry' AND track_id IN(SELECT t.id FROM release r JOIN track t ON t.release_id=r.id WHERE r.album_id=?1) AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=spotify_connection_review.track_id)",params![album.as_ref(),EVALUATION_VERSION])?;
        Ok(())
    }
    pub fn clear_spotify_bulk_window(&mut self, album: &AlbumId) -> Result<()> {
        let key = self.spotify_bulk_window_key(album)?;
        self.store
            .connection
            .execute("DELETE FROM spotify_discovery WHERE fingerprint=?1", [key])?;
        Ok(())
    }
    fn spotify_bulk_window_key(&self, album: &AlbumId) -> Result<String> {
        let tracks = self.local_album_tracks(album)?;
        let semantic: Vec<_> = tracks
            .iter()
            .map(|t| {
                (
                    &t.track_id,
                    &t.evidence.title,
                    t.evidence.disc,
                    t.evidence.number,
                    t.evidence.duration_ms,
                    &t.evidence.artists,
                    &t.evidence.recording,
                )
            })
            .collect();
        let context = match self.prepare_album_match_for(
            album,
            &crate::catalog::MatchingScope {
                provider: "spotify".into(),
                artist_kind: "artist".into(),
                album_kind: "album".into(),
            },
        )? {
            crate::album_matching::Preparation::Ready(input) => Some((
                input.raw_title,
                input.artist_id,
                input.known_artist,
                input.equivalent_artists,
                input.evidence,
            )),
            crate::album_matching::Preparation::Done(_) => None,
        };
        let metadata: (String, String, Option<i64>) = self.store.connection.query_row(
            "SELECT title,COALESCE(artist_credit,match_artist_credit),year FROM effective_album_metadata WHERE album_id=?1",
            [album.as_ref()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let bytes = json(&(EVALUATION_VERSION, metadata, context, semantic))?;
        // Provider cache fingerprint only; never a canonical entity identifier.
        Ok(format!(
            "spotify-bulk-window:{}:{}",
            album.as_ref(),
            uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, bytes.as_bytes())
        ))
    }
    pub fn spotify_bulk_window_current(&self, album: &AlbumId) -> Result<bool> {
        let key = self.spotify_bulk_window_key(album)?;
        Ok(self.store.connection.query_row("SELECT EXISTS(SELECT 1 FROM spotify_discovery WHERE fingerprint=?1 AND completed_at>unixepoch()-?2)",params![key,DISCOVERY_TTL_SECONDS],|r|r.get(0))?)
    }
    pub fn record_spotify_bulk_window(&mut self, album: &AlbumId) -> Result<()> {
        let key = self.spotify_bulk_window_key(album)?;
        self.store.connection.execute("INSERT INTO spotify_discovery(fingerprint,response_json,completed_at) VALUES(?1,'{\"bulk_search_window_exhausted\":true,\"provider_search_performed_for_every_track\":false}',unixepoch()) ON CONFLICT(fingerprint) DO NOTHING",[key])?;
        self.store.connection.execute("UPDATE spotify_connection_review SET state='unresolved',stale=0,evaluation_version=?2,reason_code='provider_search_deferred',reason='Provider search deferred: the reconciliation run used its bounded standalone Track search allowance; use individual Track search' WHERE state='needs_retry' AND track_id IN(SELECT t.id FROM release r JOIN track t ON t.release_id=r.id WHERE r.album_id=?1) AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=spotify_connection_review.track_id)", params![album.as_ref(),EVALUATION_VERSION])?;
        Ok(())
    }
    pub fn trusted_artist_credit_names(
        &self,
        artist: &crate::domain::ArtistId,
    ) -> Result<Vec<String>> {
        let mut q=self.store.connection.prepare("SELECT DISTINCT n.name FROM artist_external_identity i JOIN trusted_artist_name n ON n.provider=i.provider AND n.kind=i.kind AND n.external_id=i.external_id WHERE i.artist_id=?1 ORDER BY n.role DESC,n.name_key LIMIT 32")?;
        Ok(q.query_map([artist.as_ref()], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    /// Resolution by a complete trusted catalog Artist page, independent of an
    /// already associated Album. Names/aliases must support the current credit.
    pub fn ingest_resolved_catalog_artist(
        &mut self,
        artist: &crate::domain::ArtistId,
        observation: &crate::catalog::ArtistCandidate,
    ) -> Result<()> {
        if observation.identity.provider != "musicbrainz" || observation.identity.kind != "artist" {
            return Err(crate::Error::Invalid(
                "Expected catalog Artist identity".into(),
            ));
        }
        let name: String = self.store.connection.query_row(
            "SELECT name FROM artist WHERE id=?1",
            [artist.as_ref()],
            |r| r.get(0),
        )?;
        if std::iter::once(&observation.name)
            .chain(&observation.aliases)
            .all(|n| crate::matching::normalize(n) != crate::matching::normalize(&name))
        {
            return Err(crate::Error::Invalid(
                "Catalog Artist names do not support the canonical Artist".into(),
            ));
        }
        let ids = self.list_artist_external_identities(artist)?;
        if ids.iter().any(|i| {
            i.provider == "musicbrainz" && i.kind == "artist" && i != &observation.identity
        }) {
            return Err(crate::Error::Invalid(
                "Trusted catalog Artist identity conflict".into(),
            ));
        }
        self.attach_artist_external_identity(artist, &observation.identity)?;
        self.ingest_trusted_artist_names(observation)?;
        Ok(())
    }
    /// Exact provider-name discovery is usable only with retained catalog provenance.
    pub fn ingest_catalog_artist_provider_name(
        &mut self,
        artist: &crate::domain::ArtistId,
        candidate: &crate::catalog::ArtistCandidate,
    ) -> Result<bool> {
        if candidate.identity.provider != "spotify"
            || candidate.identity.kind != "artist"
            || candidate.identity.external_id.is_empty()
        {
            return Err(crate::Error::Invalid(
                "Invalid provider Artist identity".into(),
            ));
        }
        let tx = self
            .store
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let changed=tx.execute("INSERT INTO catalog_artist_provider_name(catalog_artist_id,provider,kind,external_id,name_key) SELECT i.external_id,?2,'artist',?3,n.name_key FROM artist_external_identity i JOIN trusted_artist_name n ON n.provider=i.provider AND n.kind=i.kind AND n.external_id=i.external_id WHERE i.artist_id=?1 AND i.provider='musicbrainz' AND n.name_key=?4 ON CONFLICT DO NOTHING",params![artist.as_ref(),candidate.identity.provider,candidate.identity.external_id,crate::matching::normalize(&candidate.name)])?;
        if changed > 0 {
            tx.execute("UPDATE spotify_connection_review SET stale=1 WHERE track_id IN(SELECT t.id FROM album_artist_credit ac JOIN release r ON r.album_id=ac.album_id JOIN track t ON t.release_id=r.id WHERE ac.artist_id=?1) AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=spotify_connection_review.track_id)",[artist.as_ref()])?;
        }
        tx.commit()?;
        Ok(changed > 0)
    }
    pub fn unobserved_catalog_artist(
        &self,
        artist: &crate::domain::ArtistId,
    ) -> Result<Option<crate::domain::ExternalIdentity>> {
        use rusqlite::OptionalExtension;
        Ok(self.store.connection.query_row("SELECT i.external_id FROM artist_external_identity i WHERE i.artist_id=?1 AND i.provider='musicbrainz' AND i.kind='artist' AND NOT EXISTS(SELECT 1 FROM trusted_artist_name n WHERE n.provider=i.provider AND n.kind=i.kind AND n.external_id=i.external_id) ORDER BY i.external_id LIMIT 1",[artist.as_ref()],|r|Ok(crate::domain::ExternalIdentity{provider:"musicbrainz".into(),kind:"artist".into(),external_id:r.get(0)?})).optional()?)
    }
    pub fn ingest_trusted_artist_names(
        &mut self,
        observation: &crate::catalog::ArtistCandidate,
    ) -> Result<usize> {
        if observation.identity.provider != "musicbrainz" || observation.identity.kind != "artist" {
            return Err(crate::Error::Invalid(
                "Artist names require trusted catalog provenance".into(),
            ));
        }
        let tx = self
            .store
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM artist_external_identity WHERE provider=?1 AND kind=?2 AND external_id=?3)",params![observation.identity.provider,observation.identity.kind,observation.identity.external_id],|r|r.get(0))?;
        if !exists {
            return Err(crate::Error::Invalid(
                "Artist catalog identity is not established".into(),
            ));
        }
        let mut changed = 0;
        for (name, role) in std::iter::once((&observation.name, "preferred"))
            .chain(observation.aliases.iter().map(|n| (n, "alias")))
        {
            let key = crate::matching::normalize(name);
            if !key.is_empty() {
                changed+=tx.execute("INSERT INTO trusted_artist_name(provider,kind,external_id,name,name_key,role) VALUES(?1,'artist',?2,?3,?4,?5) ON CONFLICT DO NOTHING",params![observation.identity.provider,observation.identity.external_id,name,key,role])?;
            }
        }
        if changed > 0 {
            tx.execute("UPDATE spotify_connection_review SET stale=1 WHERE track_id IN(SELECT t.id FROM artist_external_identity i JOIN album_artist_credit ac ON ac.artist_id=i.artist_id JOIN release r ON r.album_id=ac.album_id JOIN track t ON t.release_id=r.id WHERE i.provider=?1 AND i.kind=?2 AND i.external_id=?3) AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=spotify_connection_review.track_id)",params![observation.identity.provider,observation.identity.kind,observation.identity.external_id])?;
        }
        tx.commit()?;
        Ok(changed)
    }
    /// Names scoped to an established catalog identity, never provider-observed similarity.
    pub fn trusted_artist_search_names(&self, name: &str) -> Result<Vec<String>> {
        let mut q = self.store.connection.prepare("SELECT DISTINCT n.name FROM artist a JOIN artist_external_identity i ON i.artist_id=a.id JOIN trusted_artist_name n ON n.provider=i.provider AND n.kind=i.kind AND n.external_id=i.external_id WHERE (lower(a.name)=lower(?1) OR EXISTS(SELECT 1 FROM trusted_artist_name seed WHERE seed.provider=i.provider AND seed.kind=i.kind AND seed.external_id=i.external_id AND seed.name_key=?2)) AND n.name_key<>?2 ORDER BY n.role DESC, EXISTS(SELECT 1 FROM spotify_discovery d WHERE d.fingerprint>='spotify-name:' AND d.fingerprint<'spotify-name;' AND lower(json_extract(d.response_json,'$'))=n.name_key) DESC,n.name_key LIMIT 2")?;
        Ok(
            q.query_map(params![name, crate::matching::normalize(name)], |r| {
                r.get(0)
            })?
            .collect::<rusqlite::Result<_>>()?,
        )
    }
    /// Bounded offline cache provenance. It never chooses a market for requests.
    pub fn retained_spotify_market(&self) -> Result<Option<String>> {
        let keys=self.store.connection.prepare("SELECT fingerprint FROM spotify_discovery WHERE fingerprint>='spotify:' AND fingerprint<'spotify;' ORDER BY fingerprint LIMIT 64")?.query_map([],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let markets = keys
            .iter()
            .filter_map(|key| key.split("market=").nth(1).and_then(|v| v.get(..2)))
            .collect::<std::collections::BTreeSet<_>>();
        Ok(if markets.len() == 1 {
            markets.first().map(|v| v.to_string())
        } else {
            None
        })
    }
    /// Separate write connection for bounded local work away from the UI thread.
    pub fn spotify_review_worker(&self) -> Result<Option<Self>> {
        self.store
            .connection
            .path()
            .filter(|p| !p.is_empty())
            .map(Self::open)
            .transpose()
    }
    pub fn spotify_manually_excluded(&self, track: &TrackId) -> Result<bool> {
        excluded(&self.store.connection, track, "spotify")
    }
    pub fn mark_not_on_spotify(&mut self, track: &TrackId) -> Result<()> {
        self.store.connection.execute("INSERT INTO track_provider_exclusion(track_id,provider) VALUES(?1,'spotify') ON CONFLICT DO NOTHING",[track.as_ref()])?;
        Ok(())
    }
    /// One atomic user action. Connections established since selection are retained.
    pub fn mark_tracks_not_on_spotify(&mut self, tracks: &[TrackId]) -> Result<usize> {
        let tx = self
            .store
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "INSERT INTO track_provider_exclusion(track_id,provider) SELECT t.id,'spotify' FROM track t WHERE t.id IN(SELECT value FROM json_each(?1)) AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=t.id) ON CONFLICT DO NOTHING",
            [json(&tracks)?],
        )?;
        tx.commit()?;
        Ok(changed)
    }
    /// Snapshot user action on current saved unresolved Tracks, never an inherited rule.
    pub fn mark_album_unresolved_not_on_spotify(&mut self, album: &AlbumId) -> Result<usize> {
        let tx = self
            .store
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "INSERT INTO track_provider_exclusion(track_id,provider) SELECT t.id,'spotify' FROM release r JOIN track t ON t.release_id=r.id JOIN library_membership m ON m.track_id=t.id WHERE r.album_id=?1 AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=t.id) ON CONFLICT DO NOTHING",
            [album.as_ref()],
        )?;
        tx.commit()?;
        Ok(changed)
    }
    /// Re-enable eligibility; clearing negative knowledge never creates an identity.
    pub fn check_spotify_again(&mut self, track: &TrackId) -> Result<()> {
        self.check_tracks_spotify_again(std::slice::from_ref(track))?;
        Ok(())
    }
    /// Atomically re-enable the selected Tracks without touching associations.
    pub fn check_tracks_spotify_again(&mut self, tracks: &[TrackId]) -> Result<usize> {
        let tx = self
            .store
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "DELETE FROM track_provider_exclusion WHERE track_id IN(SELECT value FROM json_each(?1)) AND provider='spotify'",
            [json(&tracks)?],
        )?;
        tx.execute(
            "UPDATE spotify_connection_review SET stale=1 WHERE track_id IN(SELECT value FROM json_each(?1))",
            [json(&tracks)?],
        )?;
        tx.commit()?;
        Ok(changed)
    }
    pub fn marked_spotify_count(&self) -> Result<u64> {
        Ok(self.store.connection.query_row("SELECT count(*) FROM track_provider_exclusion x INDEXED BY provider_exclusion_list JOIN library_membership m ON m.track_id=x.track_id WHERE x.provider='spotify'",[],|r|r.get::<_,i64>(0))? as u64)
    }
    /// Bound the explicit retry plan and deduplicate by Album. Display filters do not change eligibility.
    pub fn spotify_retry_albums(
        &self,
        after: Option<&AlbumId>,
        limit: u32,
    ) -> Result<Vec<AlbumId>> {
        self.spotify_reconciliation_albums(after, limit, false)
    }
    /// Keyset chunk for a run. Automatic runs ignore stable completed failures.
    pub fn spotify_reconciliation_albums(
        &self,
        after: Option<&AlbumId>,
        limit: u32,
        automatic: bool,
    ) -> Result<Vec<AlbumId>> {
        let mut q=self.store.connection.prepare("SELECT b.album_id FROM album_browse_order b INDEXED BY album_order_title WHERE b.title_key>=COALESCE((SELECT title_key FROM album_browse_order WHERE album_id=?1),'') AND (?1 IS NULL OR (b.title_key,b.album_id)>(SELECT title_key,album_id FROM album_browse_order WHERE album_id=?1)) AND EXISTS(SELECT 1 FROM release r JOIN track t ON t.release_id=r.id JOIN library_membership m ON m.track_id=t.id JOIN spotify_connection_review v ON v.track_id=t.id WHERE r.album_id=b.album_id AND (?3=0 OR v.state='needs_retry' OR v.stale=1 OR v.evaluation_version<?4 OR v.reason_code='not_evaluated') AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=t.id AND x.provider='spotify') AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=t.id)) ORDER BY b.title_key,b.album_id LIMIT ?2")?;
        Ok(q.query_map(
            params![
                after.map(AsRef::as_ref),
                limit.clamp(1, RECONCILIATION_ALBUM_BATCH_SIZE),
                automatic,
                EVALUATION_VERSION
            ],
            |r| Ok(AlbumId(r.get(0)?)),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// True Needs-retry contexts take priority over the ordinary browsing cursor.
    /// New work before that cursor must never be silently omitted from a retry.
    pub fn spotify_retry_needed_albums(&self, limit: u32) -> Result<Vec<AlbumId>> {
        let mut q=self.store.connection.prepare("SELECT b.album_id FROM album_browse_order b INDEXED BY album_order_title WHERE EXISTS(SELECT 1 FROM effective_album_metadata a WHERE a.album_id=b.album_id AND a.match_title!='' AND a.match_artist_credit!='') AND (SELECT count(*) FROM album_artist_credit ac WHERE ac.album_id=b.album_id)=1 AND EXISTS(SELECT 1 FROM release r JOIN track t ON t.release_id=r.id JOIN library_membership m ON m.track_id=t.id JOIN spotify_connection_review v ON v.track_id=t.id WHERE r.album_id=b.album_id AND v.state='needs_retry' AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=t.id AND x.provider='spotify') AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=t.id)) ORDER BY b.title_key,b.album_id LIMIT ?1")?;
        Ok(
            q.query_map([limit.clamp(1, RETRY_ALBUMS)], |r| Ok(AlbumId(r.get(0)?)))?
                .collect::<rusqlite::Result<_>>()?,
        )
    }
    /// Cursor-bounded contexts affected when an established Artist gains provider
    /// identity. Uses the Artist index, never a library-wide Track reevaluation.
    pub fn spotify_artist_retry_albums(
        &self,
        artist: &crate::domain::ArtistId,
        after: Option<&AlbumId>,
        limit: u32,
    ) -> Result<Vec<AlbumId>> {
        let mut q = self.store.connection.prepare(
            "SELECT ac.album_id FROM album_artist_credit ac INDEXED BY album_artist_credit_artist WHERE ac.artist_id=?1 AND (?2 IS NULL OR ac.album_id>?2) AND EXISTS(SELECT 1 FROM release r JOIN track t ON t.release_id=r.id JOIN library_membership m ON m.track_id=t.id WHERE r.album_id=ac.album_id AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=t.id AND x.provider='spotify') AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=t.id)) GROUP BY ac.album_id ORDER BY ac.album_id LIMIT ?3")?;
        Ok(q.query_map(
            params![
                artist.as_ref(),
                after.map(AsRef::as_ref),
                limit.clamp(1, RETRY_ALBUMS)
            ],
            |r| Ok(AlbumId(r.get(0)?)),
        )?
        .collect::<rusqlite::Result<_>>()?)
    }
    /// Fresh discovery is needed when local evaluation could not replay this context.
    /// Used by both the status display and explicit batch retry policy.
    pub fn spotify_album_needs_retry(&self, album: &AlbumId) -> Result<bool> {
        Ok(self.store.connection.query_row("SELECT EXISTS(SELECT 1 FROM release r JOIN track t ON t.release_id=r.id JOIN library_membership m ON m.track_id=t.id JOIN spotify_connection_review v ON v.track_id=t.id WHERE r.album_id=?1 AND v.state='needs_retry' AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=t.id AND x.provider='spotify') AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=t.id))", [album.as_ref()], |r| r.get(0))?)
    }
    /// An old review summary without inspectable Album evidence is retryable,
    /// not a review decision. Preserve current successful negative discovery.
    pub fn spotify_album_review_evidence_unavailable(&mut self, album: &AlbumId) -> Result<()> {
        self.store.connection.execute(
            "UPDATE spotify_connection_review SET state='needs_retry',evaluation_version=?2,stale=0,reason_code='cache_refresh_required',reason='Album discovery/program cache is absent or no longer reusable; provider refresh required' WHERE track_id IN(SELECT t.id FROM release r JOIN track t ON t.release_id=r.id JOIN library_membership m ON m.track_id=t.id WHERE r.album_id=?1) AND reason_code NOT IN ('no_candidates','not_evaluated','artist_unresolved') AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=spotify_connection_review.track_id) AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=spotify_connection_review.track_id AND x.provider='spotify')",
            params![album.as_ref(), EVALUATION_VERSION],
        )?;
        Ok(())
    }
    /// Bounded Track fallback after a batch's Album work; no provider activity here.
    pub fn spotify_retry_tracks(&self, albums: &[AlbumId], limit: u32) -> Result<Vec<TrackId>> {
        self.spotify_retry_tracks_after(albums, None, limit)
    }
    /// Cursor through the complete fallback set; the page bound is not a total-work cap.
    pub fn spotify_retry_tracks_after(
        &self,
        albums: &[AlbumId],
        after: Option<&TrackId>,
        limit: u32,
    ) -> Result<Vec<TrackId>> {
        let mut q=self.store.connection.prepare("SELECT t.id FROM release r JOIN track t ON t.release_id=r.id JOIN library_membership m ON m.track_id=t.id WHERE r.album_id IN(SELECT value FROM json_each(?1)) AND (?3 IS NULL OR (r.album_id,COALESCE(t.disc_number,1),COALESCE(t.track_number,2147483647),t.id)>(SELECT ar.album_id,COALESCE(at.disc_number,1),COALESCE(at.track_number,2147483647),at.id FROM track at JOIN release ar ON ar.id=at.release_id WHERE at.id=?3)) AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=t.id AND x.provider='spotify') AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=t.id) ORDER BY r.album_id,COALESCE(t.disc_number,1),COALESCE(t.track_number,2147483647),t.id LIMIT ?2")?;
        Ok(q.query_map(
            params![json(&albums)?, limit.clamp(1, 20), after.map(AsRef::as_ref)],
            |r| Ok(TrackId(r.get(0)?)),
        )?
        .collect::<rusqlite::Result<_>>()?)
    }
    pub fn spotify_album_has_eligible(&self, album: &AlbumId) -> Result<bool> {
        Ok(self.store.connection.query_row("SELECT EXISTS(SELECT 1 FROM release r JOIN track t ON t.release_id=r.id JOIN library_membership m ON m.track_id=t.id WHERE r.album_id=?1 AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=t.id AND x.provider='spotify') AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=t.id))",[album.as_ref()],|r|r.get(0))?)
    }
    /// Inspection may expose persisted evidence even after discovery TTL expires.
    /// The snapshot is retained so inspection cannot masquerade as a fresh search.
    pub fn cached_spotify_track_attempt(
        &self,
        track: &TrackId,
    ) -> Result<Option<(Input, Page<Candidate>)>> {
        use rusqlite::OptionalExtension;
        let row: Option<(String, String)> = self
            .store
            .connection
            .query_row(
                "SELECT input_json,page_json FROM spotify_reconciliation_cache WHERE track_id=?1",
                [track.as_ref()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        row.map(|(input, page)| {
            Ok((
                serde_json::from_str(&input).map_err(|e| Error::Invalid(e.to_string()))?,
                serde_json::from_str(&page).map_err(|e| Error::Invalid(e.to_string()))?,
            ))
        })
        .transpose()
    }
    /// A complete bounded page retains competition evidence, not merely its chosen candidate.
    pub(crate) fn cache_spotify_attempt_tx(
        tx: &rusqlite::Transaction<'_>,
        input: &Input,
        page: &Page<Candidate>,
    ) -> Result<()> {
        if input.spotify_excluded || page.items.iter().any(|c| c.identity.provider != "spotify") {
            return Ok(());
        }
        let current = crate::song_resolution::load_input(tx, &input.track_id)?;
        if current.spotify_excluded {
            return Ok(());
        }
        if page.items.len() <= 20 {
            tx.execute("INSERT INTO spotify_reconciliation_cache(track_id,input_json,page_json) VALUES(?1,?2,?3) ON CONFLICT(track_id) DO UPDATE SET input_json=excluded.input_json,page_json=excluded.page_json",params![input.track_id.as_ref(),json(input)?,json(page)?])?;
        } else {
            tx.execute(
                "DELETE FROM spotify_reconciliation_cache WHERE track_id=?1",
                [input.track_id.as_ref()],
            )?;
        }
        tx.execute("UPDATE spotify_connection_review SET evaluation_version=?2,stale=?3,state='unresolved' WHERE track_id=?1",params![input.track_id.as_ref(),EVALUATION_VERSION,input!=&current])?;
        Ok(())
    }
    /// Indexed, cursor-bounded stale scan. Exclusions/trusted identities are filtered in SQL.
    pub fn stale_spotify_tracks(&self, limit: u32) -> Result<Vec<TrackId>> {
        let eligible = " AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=s.track_id AND x.provider='spotify') AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track t WHERE t.track_id=s.track_id)";
        let sql = format!(
            "SELECT track_id FROM (SELECT s.track_id FROM spotify_connection_review s INDEXED BY spotify_review_stale JOIN library_membership m ON m.track_id=s.track_id WHERE s.stale=1 {eligible} LIMIT ?2) UNION SELECT track_id FROM (SELECT s.track_id FROM spotify_connection_review s INDEXED BY spotify_review_version JOIN library_membership m ON m.track_id=s.track_id WHERE s.evaluation_version<?1 {eligible} LIMIT ?2) LIMIT ?2"
        );
        let mut q = self.store.connection.prepare(&sql)?;
        Ok(q.query_map(
            params![EVALUATION_VERSION, limit.clamp(1, LOCAL_BATCH)],
            |r| Ok(TrackId(r.get(0)?)),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// Network-free work unit. Call on a worker connection, delivering targeted updates between batches.
    pub fn reevaluate_stale_spotify(&mut self, limit: u32) -> Result<LocalBatch> {
        let ids = self.stale_spotify_tracks(limit)?;
        let mut batch = LocalBatch::default();
        let ids_json = json(&ids)?;
        // Identify replayable rows in one batch; legacy reasons alone contain no candidate data.
        let replayable:std::collections::HashSet<String>=self.store.connection.prepare("SELECT c.track_id FROM spotify_reconciliation_cache c WHERE c.track_id IN(SELECT value FROM json_each(?1)) UNION SELECT t.id FROM spotify_program_cache c JOIN release r ON r.album_id=c.album_id JOIN track t ON t.release_id=r.id WHERE t.id IN(SELECT value FROM json_each(?1))")?.query_map([&ids_json],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
        let cached:std::collections::HashMap<TrackId,(String,String)>=self.store.connection.prepare("SELECT track_id,input_json,page_json FROM spotify_reconciliation_cache WHERE track_id IN(SELECT value FROM json_each(?1))")?.query_map([&ids_json],|r|Ok((TrackId(r.get(0)?),(r.get(1)?,r.get(2)?))))?.collect::<rusqlite::Result<_>>()?;
        let programs:std::collections::HashMap<AlbumId,String>=self.store.connection.prepare("SELECT album_id,programs_json FROM spotify_program_cache WHERE album_id IN(SELECT DISTINCT r.album_id FROM track t JOIN release r ON r.id=t.release_id WHERE t.id IN(SELECT value FROM json_each(?1)))")?.query_map([&ids_json],|r|Ok((AlbumId(r.get(0)?),r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        let missing: Vec<_> = ids
            .iter()
            .filter(|t| !replayable.contains(t.as_ref()))
            .collect();
        if !missing.is_empty() {
            let changed=self.store.connection.execute("UPDATE spotify_connection_review SET evaluation_version=?2,stale=0,state='needs_retry',reason_code='cache_refresh_required',reason='No retained Track page or established Album program; provider discovery required' WHERE track_id IN(SELECT value FROM json_each(?1)) AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=spotify_connection_review.track_id AND x.provider='spotify') AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=spotify_connection_review.track_id)",params![json(&missing)?,EVALUATION_VERSION])?;
            batch.updated.extend(missing.into_iter().cloned());
            batch.examined += changed;
            batch.needs_retry += changed;
        }
        for track in ids.into_iter().filter(|t| replayable.contains(t.as_ref())) {
            let pending:bool=self.store.connection.query_row("SELECT (stale=1 OR evaluation_version<?2) AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=?1 AND x.provider='spotify') AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=?1) FROM spotify_connection_review WHERE track_id=?1",params![track.as_ref(),EVALUATION_VERSION],|r|r.get(0))?;
            if !pending {
                continue;
            }
            batch.updated.push(track.clone());
            batch.examined += 1;
            if let Err(error) =
                self.reevaluate_one(&track, cached.get(&track), &programs, &mut batch)
            {
                batch.errors.push(format!("{}: {error}", track.as_ref()));
                // Do not spin on an unavailable database/cache; explicit retry remains possible.
                self.store.connection.execute("UPDATE spotify_connection_review SET evaluation_version=?2,stale=0,state='needs_retry',reason_code='local_reevaluation_failed',reason=?3 WHERE track_id=?1",params![track.as_ref(),EVALUATION_VERSION,format!("Local reevaluation failed: {error}; retry local evaluation")])?;
            }
        }
        Ok(batch)
    }
    /// Complete affected Album contexts to a bounded local fixed point. No search,
    /// no global stale scan and no invalidation of trusted/excluded Tracks.
    pub fn reevaluate_spotify_albums_locally(&mut self, albums: &[AlbumId]) -> Result<LocalBatch> {
        if albums.len() > RETRY_ALBUMS as usize {
            return Err(Error::Invalid("Local retry Album bound exceeded".into()));
        }
        let mut batch = LocalBatch::default();
        loop {
            let ids = self.spotify_retry_tracks_complete(albums)?;
            if ids.is_empty() {
                break;
            }
            let ids_json = json(&ids)?;
            let cached:std::collections::HashMap<TrackId,(String,String)>=self.store.connection.prepare("SELECT track_id,input_json,page_json FROM spotify_reconciliation_cache WHERE track_id IN(SELECT value FROM json_each(?1))")?.query_map([&ids_json],|r|Ok((TrackId(r.get(0)?),(r.get(1)?,r.get(2)?))))?.collect::<rusqlite::Result<_>>()?;
            let programs:std::collections::HashMap<AlbumId,String>=self.store.connection.prepare("SELECT album_id,programs_json FROM spotify_program_cache WHERE album_id IN(SELECT value FROM json_each(?1))")?.query_map([json(&albums)?],|r|Ok((AlbumId(r.get(0)?),r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
            let before = batch.accepted.len();
            for track in ids {
                if !self
                    .track_provider_occurrences(&track, "spotify")?
                    .is_empty()
                {
                    continue;
                }
                batch.examined += 1;
                batch.updated.push(track.clone());
                if let Err(error) =
                    self.reevaluate_one(&track, cached.get(&track), &programs, &mut batch)
                {
                    batch.errors.push(format!("{}: {error}", track.as_ref()));
                    self.store.connection.execute("UPDATE spotify_connection_review SET evaluation_version=?2,stale=0,state='needs_retry',reason_code='local_reevaluation_failed',reason=?3 WHERE track_id=?1",params![track.as_ref(),EVALUATION_VERSION,format!("Local reevaluation failed: {error}; retry local evaluation")])?;
                }
            }
            if batch.accepted.len() == before || !batch.errors.is_empty() {
                break;
            }
        }
        batch.accepted.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
        batch.accepted.dedup();
        batch.updated.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
        batch.updated.dedup();
        batch.still_unresolved = self.spotify_retry_tracks_complete(albums)?.len();
        Ok(batch)
    }
    fn spotify_retry_tracks_complete(&self, albums: &[AlbumId]) -> Result<Vec<TrackId>> {
        let mut result = vec![];
        let mut after = None;
        loop {
            let page = self.spotify_retry_tracks_after(albums, after.as_ref(), 20)?;
            if page.is_empty() {
                break;
            }
            after = page.last().cloned();
            result.extend(page);
        }
        Ok(result)
    }
    fn reevaluate_one(
        &mut self,
        track: &TrackId,
        cached: Option<&(String, String)>,
        programs: &std::collections::HashMap<AlbumId, String>,
        batch: &mut LocalBatch,
    ) -> Result<()> {
        let mut song_evaluated = false;
        if let Some((snapshot, page)) = cached {
            let input = self.song_resolution_input(track)?;
            if input.spotify_excluded {
                return Ok(());
            }
            let snapshot: Input =
                serde_json::from_str(snapshot).map_err(|e| Error::Invalid(e.to_string()))?;
            let page: Page<Candidate> =
                serde_json::from_str(page).map_err(|e| Error::Invalid(e.to_string()))?;
            // A new discovery scope needs a fresh complete page. Evidence within the same
            // scope (date, type, positions, trusted IDs/equivalence) may be reconsidered locally.
            if crate::matching::normalize_album_title(&snapshot.title)
                == crate::matching::normalize_album_title(&input.title)
                && crate::matching::normalize_album_title(&snapshot.album)
                    == crate::matching::normalize_album_title(&input.album)
                && snapshot.primary_artist.as_ref().map(|a| &a.name)
                    == input.primary_artist.as_ref().map(|a| &a.name)
            {
                let accepted = self.apply_song_evaluation(&input, &page)?;
                self.persist_spotify_song_review(&input, &page)?;
                if accepted.is_some() {
                    batch.accepted.push(track.clone());
                    return Ok(());
                }
                // A failed Track search must not suppress stronger accumulated
                // Album-program evidence already persisted for this context.
                song_evaluated = true;
            }
        }
        // A persisted program can be replayed only under its still-established Album identity.
        let album = self.album_id_for_track(track)?;
        if let Some(program) = programs.get(&album) {
            let program: crate::album_program::Programs =
                serde_json::from_str(program).map_err(|e| Error::Invalid(e.to_string()))?;
            if let Some(current) = self.prepare_album_program(&album, &program.album)? {
                let eligible:Vec<TrackId>=self.store.connection.prepare("SELECT t.id FROM track t JOIN release r ON r.id=t.release_id JOIN library_membership m ON m.track_id=t.id WHERE r.album_id=?1 AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=t.id AND x.provider='spotify') AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=t.id)")?.query_map([album.as_ref()],|r|Ok(TrackId(r.get(0)?)))?.collect::<rusqlite::Result<_>>()?;
                let outcome = self.complete_album_program(crate::album_program::Reply {
                    input: current,
                    result: Ok(program),
                })?;
                match outcome {
                    crate::album_program::Outcome::Error(e) => return Err(Error::Invalid(e)),
                    crate::album_program::Outcome::Deferred(e) => {
                        return Err(Error::Invalid(e.to_string()));
                    }
                    _ => {}
                }
                let accepted:Vec<TrackId>=self.store.connection.prepare("SELECT DISTINCT s.track_id FROM trusted_spotify_track s WHERE s.track_id IN(SELECT value FROM json_each(?1))")?.query_map([json(&eligible)?],|r|Ok(TrackId(r.get(0)?)))?.collect::<rusqlite::Result<_>>()?;
                batch.examined += eligible.len().saturating_sub(1);
                batch.still_unresolved += eligible.len().saturating_sub(accepted.len());
                batch.updated.extend(eligible);
                batch.accepted.extend(accepted);
                return Ok(());
            }
        }
        if song_evaluated {
            batch.still_unresolved += 1;
            return Ok(());
        }
        let current_negative:bool=self.store.connection.query_row("SELECT reason_code='no_candidates' AND stale=0 AND evaluation_version=?2 FROM spotify_connection_review WHERE track_id=?1",params![track.as_ref(),EVALUATION_VERSION],|r|r.get(0))?;
        if current_negative {
            batch.still_unresolved += 1;
            return Ok(());
        }
        self.store.connection.execute("UPDATE spotify_connection_review SET state='needs_retry',evaluation_version=?2,stale=0,reason_code='cache_refresh_required',reason='No reusable discovery for current Track evidence; provider discovery required' WHERE track_id=?1",params![track.as_ref(),EVALUATION_VERSION])?;
        batch.needs_retry += 1;
        Ok(())
    }
}

/// Successful discovery expires lazily; browsing never refreshes it.
pub const DISCOVERY_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;
/// Query construction/breadth only. Never increment for evaluator changes.
pub const SEARCH_STRATEGY_VERSION: u32 = 6;
impl Library {
    pub fn database_path(&self) -> Option<String> {
        self.store
            .connection
            .path()
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
    }
    pub fn spotify_discovery_response(
        &self,
        fingerprint: &str,
        now: i64,
    ) -> Result<Option<String>> {
        use rusqlite::OptionalExtension;
        Ok(self.store.connection.query_row(
            "SELECT response_json FROM spotify_discovery WHERE fingerprint=?1 AND completed_at>?2",
            params![fingerprint, now - DISCOVERY_TTL_SECONDS], |r| r.get(0)).optional()?)
    }
    /// Retained observation for explicit offline inspection, independent of TTL.
    pub fn spotify_discovery_observation(&self, fingerprint: &str) -> Result<Option<String>> {
        use rusqlite::OptionalExtension;
        Ok(self
            .store
            .connection
            .query_row(
                "SELECT response_json FROM spotify_discovery WHERE fingerprint=?1",
                [fingerprint],
                |r| r.get(0),
            )
            .optional()?)
    }
    pub fn persist_spotify_discovery(
        &self,
        fingerprint: &str,
        response: &str,
        now: i64,
    ) -> Result<()> {
        // Incremental retention; never purge on failed refresh, and bound cleanup work.
        self.store.connection.execute("DELETE FROM spotify_discovery WHERE fingerprint IN(SELECT fingerprint FROM spotify_discovery WHERE completed_at<?1 AND fingerprint<>?2 ORDER BY completed_at LIMIT 32)",params![now-DISCOVERY_TTL_SECONDS,fingerprint])?;
        self.store.connection.execute("INSERT INTO spotify_discovery VALUES(?1,?2,?3) ON CONFLICT(fingerprint) DO UPDATE SET response_json=excluded.response_json,completed_at=excluded.completed_at", params![fingerprint,response,now])?;
        Ok(())
    }
    pub fn spotify_cooldown_remaining(&self, now: i64) -> Result<i64> {
        use rusqlite::OptionalExtension;
        Ok(self
            .store
            .connection
            .query_row(
                "SELECT until_epoch FROM provider_cooldown WHERE provider='spotify'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0)
            .saturating_sub(now)
            .max(0))
    }
    pub fn retain_spotify_cooldown(&self, until: i64) -> Result<()> {
        self.store.connection.execute("INSERT INTO provider_cooldown VALUES('spotify',?1) ON CONFLICT(provider) DO UPDATE SET until_epoch=max(until_epoch,excluded.until_epoch)",[until])?;
        Ok(())
    }
}

/// Both delta-seconds and HTTP-date Retry-After forms; injected time for tests.
pub fn spotify_retry_delay(header: Option<&str>, now: i64) -> i64 {
    header
        .and_then(|s| {
            s.trim().parse::<i64>().ok().or_else(|| {
                httpdate::parse_http_date(s.trim())
                    .ok()?
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|d| (d.as_secs() as i64).saturating_sub(now))
            })
        })
        .unwrap_or(30)
        .max(1)
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryFreshness {
    NeverSearched,
    Fresh,
    Expired,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct DiscoveryStatus {
    pub freshness: DiscoveryFreshness,
    pub completed_at: Option<i64>,
    pub cached_candidate_count: Option<usize>,
    pub has_unfetched_pages: bool,
    pub provider_retry_in_seconds: i64,
}
impl Library {
    /// Diagnostic lookup for an exact query fingerprint; no provider activity.
    pub fn spotify_discovery_status(&self, fingerprint: &str, now: i64) -> Result<DiscoveryStatus> {
        use rusqlite::OptionalExtension;
        let row: Option<(i64, String)> = self
            .store
            .connection
            .query_row(
                "SELECT completed_at,response_json FROM spotify_discovery WHERE fingerprint=?1",
                [fingerprint],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let mut status = DiscoveryStatus {
            freshness: DiscoveryFreshness::NeverSearched,
            completed_at: None,
            cached_candidate_count: None,
            has_unfetched_pages: false,
            provider_retry_in_seconds: self.spotify_cooldown_remaining(now)?,
        };
        if let Some((completed, response)) = row {
            let value: serde_json::Value =
                serde_json::from_str(&response).map_err(|e| Error::Invalid(e.to_string()))?;
            let page = value
                .get("albums")
                .or_else(|| value.get("tracks"))
                .or_else(|| value.get("artists"))
                .unwrap_or(&value);
            status.cached_candidate_count =
                page.get("items").and_then(|v| v.as_array()).map(Vec::len);
            status.has_unfetched_pages = page.get("next").is_some_and(|v| !v.is_null())
                || page
                    .get("total")
                    .and_then(|v| v.as_u64())
                    .is_some_and(|total| {
                        page.get("offset").and_then(|v| v.as_u64()).unwrap_or(0)
                            + (status.cached_candidate_count.unwrap_or(0) as u64)
                            < total
                    });
            status.completed_at = Some(completed);
            status.freshness = if completed > now - DISCOVERY_TTL_SECONDS {
                DiscoveryFreshness::Fresh
            } else {
                DiscoveryFreshness::Expired
            };
        }
        Ok(status)
    }
}

impl Library {
    /// A discovery marker, not identity: local replays never repeatedly spend
    /// the Track budget on the same unsuccessful fallback scope.
    pub fn spotify_playable_discovery_fresh(&self, input: &Input) -> Result<bool> {
        let key = playable_discovery_key(input)?;
        Ok(self.store.connection.query_row("SELECT EXISTS(SELECT 1 FROM spotify_discovery WHERE fingerprint=?1 AND completed_at>unixepoch()-?2)",params![key,DISCOVERY_TTL_SECONDS],|r|r.get(0))?)
    }
    pub fn record_spotify_playable_discovery(&mut self, input: &Input) -> Result<()> {
        if self.song_resolution_input(&input.track_id)? != *input {
            return Err(Error::ReconciliationEvidenceChanged);
        }
        let key = playable_discovery_key(input)?;
        self.store.connection.execute("INSERT INTO spotify_discovery(fingerprint,response_json,completed_at) VALUES(?1,'{}',unixepoch()) ON CONFLICT(fingerprint) DO UPDATE SET completed_at=excluded.completed_at",[key])?;
        Ok(())
    }
}
fn playable_discovery_key(input: &Input) -> Result<String> {
    Ok(format!(
        "playable-track-search:1:{}:{}",
        input.track_id.as_ref(),
        json(&(
            crate::matching::normalize_album_title(&input.title),
            crate::song_resolution::established_playable_artist(input)
        ))?
    ))
}
