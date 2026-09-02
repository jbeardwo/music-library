use std::path::Path;

use crate::domain::{
    DiscoveryCandidate, ImportReleaseRequest, ImportedRelease, RootId, ScanReport, SearchRequest,
    SourceId, TrackId, TrackSearchResult,
};
use crate::filesystem::{MetadataExtractor, scan};
use crate::storage::{Result, Store};

/// Frontend-independent entry point for product operations in the first slice.
pub struct Library {
    store: Store,
}

impl Library {
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

    pub fn remove_from_library(&mut self, track_id: &TrackId) -> Result<bool> {
        self.store.remove_from_library(track_id)
    }

    pub fn set_track_title_override(&mut self, track_id: &TrackId, value: &str) -> Result<()> {
        self.store.set_track_title_override(track_id, value)
    }

    pub fn clear_track_title_override(&mut self, track_id: &TrackId) -> Result<bool> {
        self.store.clear_track_title_override(track_id)
    }

    pub fn search(&self, request: &SearchRequest) -> Result<Vec<TrackSearchResult>> {
        self.store.search(request)
    }
}
