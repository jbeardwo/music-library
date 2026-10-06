use std::path::Path;

use crate::domain::{
    AlbumId, CatalogReleaseInput, DiscoveryCandidate, ExternalIdentity, ImportReleaseRequest,
    ImportedRelease, PlayableSource, ReleaseId, RootId, ScanReport, SearchRequest, SourceId,
    TrackId, TrackSearchResult,
};
use crate::filesystem::{MetadataExtractor, scan};
use crate::storage::{Result, Store};

/// Frontend-independent entry point for backend product operations.
pub struct Library {
    pub(crate) store: Store,
}

impl Library {
    pub fn spotify_review_summary(&self, track: &TrackId) -> Result<(String, String)> {
        Ok(self.store.connection.query_row(
            "SELECT reason_code,reason FROM spotify_connection_review WHERE track_id=?1",
            [track.as_ref()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }
    /// Targeted identity context for one diagnostic; no provider work or sources.
    pub fn diagnostic_track_identities(&self, track: &TrackId) -> Result<Vec<ExternalIdentity>> {
        let mut identities=self.store.connection.prepare("SELECT provider,kind,external_id FROM track_external_identity WHERE track_id=?1 UNION SELECT i.provider,i.kind,i.external_id FROM track t JOIN recording_external_identity i ON i.recording_id=t.recording_id WHERE t.id=?1 UNION SELECT i.provider,i.kind,i.external_id FROM track t JOIN release_external_identity i ON i.release_id=t.release_id WHERE t.id=?1 UNION SELECT i.provider,i.kind,i.external_id FROM track t JOIN release r ON r.id=t.release_id JOIN album_external_identity i ON i.album_id=r.album_id WHERE t.id=?1 ORDER BY provider,kind,external_id")?.query_map([track.as_ref()],|r|Ok(ExternalIdentity{provider:r.get(0)?,kind:r.get(1)?,external_id:r.get(2)?}))?.collect::<rusqlite::Result<Vec<_>>>()?;
        identities.extend(self.track_provider_occurrences(track, "spotify")?);
        identities.sort_by(|a, b| {
            (&a.provider, &a.kind, &a.external_id).cmp(&(&b.provider, &b.kind, &b.external_id))
        });
        identities.dedup();
        Ok(identities)
    }
    /// Record an explicit bounded song attempt; no association or membership changes.
    pub fn persist_spotify_song_review(
        &mut self,
        input: &crate::song_resolution::Input,
        page: &crate::catalog::Page<crate::song_resolution::Candidate>,
    ) -> Result<()> {
        use crate::album_candidates::Reason;
        let trusted = self.diagnostic_track_identities(&input.track_id)?;
        let evaluations = (0..page.items.len())
            .filter_map(|i| crate::song_resolution::evaluate(input, page, i, &trusted))
            .collect::<Vec<_>>();
        let code = evaluations
            .first()
            .and_then(|e| e.primary_code)
            .filter(|c| evaluations.iter().all(|e| e.primary_code == Some(*c)));
        let reason = if page.items.is_empty() {
            Reason::NoCandidates
        } else if let Some(code) = code {
            match code {
                "artist_mismatch" | "artist_credit_incomplete" => Reason::ArtistMismatch,
                "trusted_identity_conflict" | "existing_provider_association" => {
                    Reason::TrustedIdentityConflict
                }
                "release_type_mismatch" => Reason::ReleaseTypeMismatch,
                "track_count_mismatch" => Reason::TrackCountMismatch,
                "track_title_mismatch" => Reason::TrackTitleMismatch,
                "album_title_mismatch" => Reason::AlbumTitleMismatch,
                "position_mismatch" => Reason::PositionMismatch,
                "duration_threshold" => Reason::DurationConflict,
                "competing_candidates" => Reason::CompetingCandidates,
                "incomplete_candidate_page" => Reason::IncompleteCandidatePage,
                _ => Reason::InsufficientEvidence,
            }
        } else if matches!(
            crate::song_resolution::assess(input, page),
            crate::song_resolution::Assessment::Unique(_)
        ) {
            Reason::ManualReviewRequired
        } else if page.next_offset.is_some() {
            Reason::IncompleteCandidatePage
        } else {
            Reason::InsufficientEvidence
        };
        let code = serde_json::to_value(reason)
            .map_err(|e| crate::storage::Error::Invalid(e.to_string()))?;
        self.store.connection.execute(
            "UPDATE spotify_connection_review SET reason_code=?2,reason=?3 WHERE track_id=?1",
            rusqlite::params![
                input.track_id.as_ref(),
                code.as_str().unwrap_or("not_evaluated"),
                reason.review_label()
            ],
        )?;
        self.cache_spotify_attempt(input, page)?;
        Ok(())
    }
    /// Persist per-Track summaries of the existing Album decision, without provider work.
    pub fn persist_spotify_review(
        &mut self,
        album: &AlbumId,
        report: &crate::album_candidates::Report,
    ) -> Result<()> {
        let tx = self.store.connection.transaction()?;
        let tracks = tx
            .prepare(
                "SELECT t.id FROM track t JOIN release r ON r.id=t.release_id WHERE r.album_id=?1",
            )?
            .query_map([album.as_ref()], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for track in tracks {
            let reason = report.review_reason(&track);
            let code = serde_json::to_value(reason)
                .map_err(|e| crate::storage::Error::Invalid(e.to_string()))?;
            tx.execute(
                "UPDATE spotify_connection_review SET reason_code=?2,reason=?3 WHERE track_id=?1",
                rusqlite::params![
                    track,
                    code.as_str().unwrap_or("not_evaluated"),
                    reason.review_label()
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn local_releases_for_root(&self, root: &RootId) -> Result<Vec<ImportedRelease>> {
        self.store.local_releases_for_root(root)
    }
    pub fn local_album_tracks(
        &self,
        album: &crate::domain::AlbumId,
    ) -> Result<Vec<crate::edition::LocalTrackEvidence>> {
        self.store.local_album_tracks(album)
    }
    pub fn prepare_album_program(
        &self,
        album: &crate::domain::AlbumId,
        identity: &ExternalIdentity,
    ) -> Result<Option<crate::album_program::Input>> {
        self.store.prepare_album_program(album, identity)
    }
    pub fn complete_album_program(
        &mut self,
        reply: crate::album_program::Reply,
    ) -> Result<crate::album_program::Outcome> {
        self.store.complete_album_program(reply)
    }
    /// Composition boundary for embedded identifier capabilities. This replaces
    /// the default file-adapter validator; it does not reinterpret stored claims.
    pub fn set_provenance_validator(&mut self, validator: crate::provenance_acceptance::Validator) {
        self.store.provenance_validator = validator;
    }

    pub fn reconcile_local_provenance(
        &mut self,
        album: &crate::domain::AlbumId,
    ) -> Result<Vec<crate::provenance_acceptance::EntityReport>> {
        self.store.reconcile_local_provenance(album)
    }
    pub fn edition_evidence(
        &self,
        release: &ReleaseId,
    ) -> Result<crate::edition::LocalEditionEvidence> {
        self.store.edition_evidence(release)
    }
    pub fn create_recording(&mut self) -> Result<crate::domain::Recording> {
        self.store.create_recording()
    }
    pub fn manual_track_associations(
        &self,
        album: &crate::domain::AlbumId,
    ) -> Result<Vec<crate::manual_track::Association>> {
        self.store.manual_track_associations(album)
    }
    pub fn provider_track_associations(
        &self,
        album: &AlbumId,
        provider: &str,
    ) -> Result<Vec<(TrackId, crate::album_program::Match)>> {
        self.store.provider_track_associations(album, provider)
    }
    /// Accepted song/occurrence evidence for one Track. Manual decisions take
    /// precedence; availability and playback authorization are separate concerns.
    pub fn track_provider_occurrences(
        &self,
        track: &TrackId,
        provider: &str,
    ) -> Result<Vec<crate::domain::ExternalIdentity>> {
        self.store.track_provider_occurrences(track, provider)
    }
    pub fn song_resolution_input(&self, track: &TrackId) -> Result<crate::song_resolution::Input> {
        self.store.song_resolution_input(track)
    }
    pub fn apply_song_evaluation(
        &mut self,
        input: &crate::song_resolution::Input,
        page: &crate::catalog::Page<crate::song_resolution::Candidate>,
    ) -> Result<Option<ExternalIdentity>> {
        // An empty generic page has no provider namespace. Explicit Spotify
        // review recording handles empty Spotify pages independently.
        if !page.items.is_empty() && page.items.iter().all(|c| c.identity.provider == "spotify") {
            self.cache_spotify_attempt(input, page)?;
        }
        let result = self.store.apply_song_evaluation(input, page);
        let reason = match &result {
            Err(crate::storage::Error::ReconciliationEvidenceChanged) => {
                Some(crate::album_candidates::Reason::EvidenceChanged)
            }
            Err(crate::storage::Error::AssociationPersistenceFailed(_)) => {
                Some(crate::album_candidates::Reason::CompletionFailed)
            }
            _ => None,
        };
        if let Some(reason) = reason {
            let code = serde_json::to_value(reason)
                .map_err(|e| crate::storage::Error::Invalid(e.to_string()))?;
            // Preserve the original operation error if even recording its summary fails.
            let _ = self.store.connection.execute(
                "UPDATE spotify_connection_review SET reason_code=?2,reason=?3 WHERE track_id=?1",
                rusqlite::params![
                    input.track_id.as_ref(),
                    code.as_str().unwrap_or("completion_failed"),
                    reason.review_label()
                ],
            );
        }
        result
    }
    pub fn confirm_song_resolution(
        &mut self,
        selection: &crate::song_resolution::Selection,
        index: usize,
    ) -> Result<crate::domain::ExternalIdentity> {
        self.store.confirm_song_resolution(selection, index)
    }
    pub fn clear_manual_track_for(
        &mut self,
        album: &AlbumId,
        track: &TrackId,
        provider: &str,
    ) -> Result<bool> {
        self.store.clear_manual_track_for(album, track, provider)
    }
    pub fn prepare_manual_track(
        &self,
        album: &crate::domain::AlbumId,
        track: &TrackId,
        programs: &crate::album_program::Programs,
    ) -> Result<crate::manual_track::Selection> {
        self.store.prepare_manual_track(album, track, programs)
    }
    pub fn confirm_manual_track(
        &mut self,
        selection: &crate::manual_track::Selection,
        index: usize,
    ) -> Result<crate::manual_track::Association> {
        self.store.confirm_manual_track(selection, index)
    }
    pub fn clear_manual_track(
        &mut self,
        album: &crate::domain::AlbumId,
        track: &TrackId,
    ) -> Result<bool> {
        self.store.clear_manual_track(album, track)
    }
    pub fn recording_for_track(&self, track: &TrackId) -> Result<crate::domain::Recording> {
        self.store.recording_for_track(track)
    }
    pub fn attach_recording_external_identity(
        &mut self,
        id: &crate::domain::RecordingId,
        identity: &ExternalIdentity,
    ) -> Result<bool> {
        self.store.attach_recording_external_identity(id, identity)
    }
    pub fn list_recording_external_identities(
        &self,
        id: &crate::domain::RecordingId,
    ) -> Result<Vec<ExternalIdentity>> {
        self.store.list_recording_external_identities(id)
    }
    pub fn resolve_recordings_external_identity(
        &self,
        identity: &ExternalIdentity,
    ) -> Result<Vec<crate::domain::RecordingId>> {
        self.store.resolve_recordings_external_identity(identity)
    }
    pub fn merge_recording(
        &mut self,
        source: &crate::domain::RecordingId,
        canonical: &crate::domain::RecordingId,
    ) -> Result<bool> {
        self.store.merge_recording(source, canonical)
    }
    pub fn prepare_recording_match(
        &self,
        album: &crate::domain::AlbumId,
    ) -> Result<Option<crate::recording::Input>> {
        self.store.prepare_recording_match(album)
    }
    pub fn complete_recording_match(
        &mut self,
        reply: crate::recording::Reply,
    ) -> Result<crate::recording::Outcome> {
        self.store.complete_recording_match(reply)
    }
    pub fn prepare_album_match(
        &self,
        id: &crate::domain::AlbumId,
    ) -> Result<crate::album_matching::Preparation> {
        self.store.prepare_album_match(id)
    }
    pub fn prepare_album_match_for(
        &self,
        id: &crate::domain::AlbumId,
        scope: &crate::catalog::MatchingScope,
    ) -> Result<crate::album_matching::Preparation> {
        self.store.prepare_album_match_for(id, scope)
    }
    pub fn complete_album_match_for(
        &mut self,
        reply: crate::album_matching::MatchReply,
        scope: &crate::catalog::MatchingScope,
    ) -> Result<crate::album_matching::MatchOutcome> {
        self.store.complete_album_match_for(reply, scope)
    }
    pub fn complete_album_match(
        &mut self,
        reply: crate::album_matching::MatchReply,
    ) -> Result<crate::album_matching::MatchOutcome> {
        self.store.complete_album_match(reply)
    }
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            store: Store::open(path)?,
        })
    }

    pub fn open_in_memory() -> Result<Self> {
        Ok(Self {
            store: Store::open_in_memory()?,
        })
    }

    /// Attach without changing internal identity, metadata, sources, or membership.
    /// Returns false if this exact association already exists.
    pub fn merge_artist(
        &mut self,
        source: &crate::domain::ArtistId,
        canonical: &crate::domain::ArtistId,
    ) -> Result<bool> {
        self.store.merge_artist(source, canonical)
    }

    pub fn attach_artist_external_identity(
        &mut self,
        id: &crate::domain::ArtistId,
        identity: &ExternalIdentity,
    ) -> Result<bool> {
        self.store.attach_artist_external_identity(id, identity)
    }

    pub fn list_artist_external_identities(
        &self,
        id: &crate::domain::ArtistId,
    ) -> Result<Vec<ExternalIdentity>> {
        self.store.list_artist_external_identities(id)
    }

    pub fn resolve_artists_external_identity(
        &self,
        identity: &ExternalIdentity,
    ) -> Result<Vec<crate::domain::ArtistId>> {
        self.store.resolve_artists_external_identity(identity)
    }

    /// Attach without changing internal identity, metadata, sources, or membership.
    /// Returns false if this exact association already exists.
    pub fn attach_track_external_identity(
        &mut self,
        id: &TrackId,
        identity: &ExternalIdentity,
    ) -> Result<bool> {
        self.store.attach_track_external_identity(id, identity)
    }

    pub fn list_track_external_identities(&self, id: &TrackId) -> Result<Vec<ExternalIdentity>> {
        self.store.list_track_external_identities(id)
    }

    pub fn resolve_tracks_external_identity(
        &self,
        identity: &ExternalIdentity,
    ) -> Result<Vec<TrackId>> {
        self.store.resolve_tracks_external_identity(identity)
    }

    /// Attach without changing internal identity, metadata, sources, or membership.
    /// Returns false if this exact association already exists.
    pub fn attach_release_external_identity(
        &mut self,
        id: &ReleaseId,
        identity: &ExternalIdentity,
    ) -> Result<bool> {
        self.store.attach_release_external_identity(id, identity)
    }

    pub fn list_release_external_identities(
        &self,
        id: &ReleaseId,
    ) -> Result<Vec<ExternalIdentity>> {
        self.store.list_release_external_identities(id)
    }

    pub fn resolve_releases_external_identity(
        &self,
        identity: &ExternalIdentity,
    ) -> Result<Vec<ReleaseId>> {
        self.store.resolve_releases_external_identity(identity)
    }

    pub fn attach_album_external_identity(
        &mut self,
        id: &crate::domain::AlbumId,
        identity: &ExternalIdentity,
    ) -> Result<bool> {
        self.store.attach_album_external_identity(id, identity)
    }

    pub fn list_album_external_identities(
        &self,
        id: &crate::domain::AlbumId,
    ) -> Result<Vec<ExternalIdentity>> {
        self.store.list_album_external_identities(id)
    }

    pub fn resolve_albums_external_identity(
        &self,
        identity: &ExternalIdentity,
    ) -> Result<Vec<crate::domain::AlbumId>> {
        self.store.resolve_albums_external_identity(identity)
    }

    pub fn album_id_for_track(&self, track: &TrackId) -> Result<AlbumId> {
        self.store.album_id_for_track(track)
    }

    pub fn album_for_release(&self, release_id: &ReleaseId) -> Result<crate::domain::Album> {
        self.store.album_for_release(release_id)
    }

    pub fn register_local_root(&mut self, path: impl AsRef<Path>) -> Result<RootId> {
        self.store.register_local_root(path)
    }

    pub fn scan_local_root(
        &mut self,
        root_id: &RootId,
        extractor: &mut dyn MetadataExtractor,
    ) -> Result<ScanReport> {
        scan(&mut self.store, root_id, extractor)
    }

    pub fn list_discovery_candidates(
        &self,
        after_source_id: Option<&SourceId>,
        limit: u32,
    ) -> Result<Vec<DiscoveryCandidate>> {
        self.store.list_discovery_candidates(after_source_id, limit)
    }

    pub fn import_release(&mut self, request: &ImportReleaseRequest) -> Result<ImportedRelease> {
        self.store.import_release(request)
    }

    /// Creates a known Release and Tracks from structured metadata without
    /// inventing any playable source or library membership.
    pub fn create_catalog_release(
        &mut self,
        input: &CatalogReleaseInput,
    ) -> Result<ImportedRelease> {
        self.store.create_catalog_release(input)
    }

    pub fn add_catalog_release(
        &mut self,
        release: &crate::catalog::Release,
    ) -> Result<ImportedRelease> {
        self.store.add_catalog_release(release)
    }

    /// Reuse catalog identity without changing Library membership.
    pub fn ensure_catalog_release(
        &mut self,
        release: &crate::catalog::Release,
    ) -> Result<ImportedRelease> {
        self.store.ensure_catalog_release(release)
    }

    pub fn add_catalog_selection(
        &mut self,
        release: &crate::catalog::Release,
        positions: &[(u32, u32)],
    ) -> Result<ImportedRelease> {
        self.store.add_catalog_selection(release, Some(positions))
    }

    pub fn add_to_library(&mut self, track_id: &TrackId) -> Result<bool> {
        self.store.add_to_library(track_id)
    }

    pub fn remove_from_library(&mut self, track_id: &TrackId) -> Result<bool> {
        self.store.remove_from_library(track_id)
    }

    pub fn set_track_title_override(&mut self, track_id: &TrackId, value: &str) -> Result<()> {
        self.store.set_track_title_override(track_id, value)
    }

    pub fn clear_track_title_override(&mut self, track_id: &TrackId) -> Result<bool> {
        self.store.clear_track_title_override(track_id)
    }

    /// Select the available source with the smallest opaque ID in SQLite binary order.
    /// Availability is an observation; opening the source may still fail in the engine.
    pub fn available_playback_source(&self, track_id: &TrackId) -> Result<Option<PlayableSource>> {
        self.store.available_playback_source(track_id)
    }

    pub fn playback_route(
        &self,
        track: &TrackId,
        remote: &crate::playback_resolver::RemoteCapability<'_>,
    ) -> Result<crate::playback_resolver::Route> {
        self.store.playback_route(track, remote)
    }

    pub fn search(&self, request: &SearchRequest) -> Result<Vec<TrackSearchResult>> {
        self.store.search(request)
    }
}
