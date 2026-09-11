use std::path::Path;

use crate::domain::{
    CatalogReleaseInput, DiscoveryCandidate, ExternalIdentity, ImportReleaseRequest,
    ImportedRelease, PlayableSource, ReleaseId, RootId, ScanReport, SearchRequest, SourceId,
    TrackId, TrackSearchResult,
};
use crate::filesystem::{MetadataExtractor, scan};
use crate::storage::{Result, Store};

/// Frontend-independent entry point for backend product operations.
pub struct Library {
    store: Store,
}

impl Library {
    pub fn create_recording(&mut self) -> Result<crate::domain::Recording> {
        self.store.create_recording()
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

    pub fn search(&self, request: &SearchRequest) -> Result<Vec<TrackSearchResult>> {
        self.store.search(request)
    }
}
