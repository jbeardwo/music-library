use std::path::PathBuf;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, PartialEq)]
        pub struct $name(pub String);

        impl $name {
            pub(crate) fn new() -> Self {
                Self(uuid::Uuid::new_v4().to_string())
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

id_type!(AlbumId);
id_type!(ReleaseId);
id_type!(TrackId);
id_type!(ArtistId);
id_type!(SourceId);
id_type!(RootId);

/// Provider-neutral identity. Strings are persisted exactly, without normalization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalIdentity {
    pub provider: String,
    pub kind: String,
    pub external_id: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ObservedMetadata {
    pub track_title: Option<String>,
    pub release_title: Option<String>,
    pub track_artists: Vec<String>,
    pub release_artists: Vec<String>,
    pub disc_number: Option<u32>,
    pub track_number: Option<u32>,
    pub year: Option<i32>,
    pub duration_ms: Option<u64>,
    pub format: Option<String>,
}

/// A resolved source snapshot, separate from durable Track identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayableSource {
    pub source_id: SourceId,
    pub location: SourceLocation,
}

/// Source adapters supply engine input here. Only the existing local adapter is implemented.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SourceLocation {
    LocalFile(PathBuf),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveryCandidate {
    pub source_id: SourceId,
    pub path: PathBuf,
    pub available: bool,
    pub metadata: ObservedMetadata,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtistCreditInput {
    pub name: String,
    pub role: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ImportReleaseRequest {
    pub release_title: String,
    pub release_artists: Vec<ArtistCreditInput>,
    pub tracks: Vec<ImportTrackInput>,
}

#[derive(Clone, Debug)]
pub struct ImportTrackInput {
    pub source_id: SourceId,
    pub title_fallback: Option<String>,
    pub artists: Vec<ArtistCreditInput>,
    pub disc_number: Option<u32>,
    pub track_number: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct CatalogReleaseInput {
    pub title: String,
    pub year: Option<i32>,
    pub artists: Vec<ArtistCreditInput>,
    pub tracks: Vec<CatalogTrackInput>,
}

#[derive(Clone, Debug)]
pub struct CatalogTrackInput {
    pub title: String,
    pub artists: Vec<ArtistCreditInput>,
    pub disc_number: Option<u32>,
    pub track_number: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedRelease {
    pub release_id: ReleaseId,
    pub track_ids: Vec<TrackId>,
}

#[derive(Clone, Debug, Default)]
pub struct SearchRequest {
    pub text: String,
    pub availability: Option<bool>,
    pub release_id: Option<ReleaseId>,
    pub artist_id: Option<ArtistId>,
    pub after: Option<SearchCursor>,
    pub limit: u32,
}

#[derive(Clone, Debug)]
pub struct SearchCursor {
    pub title: String,
    pub track_id: TrackId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrackSearchResult {
    pub track_id: TrackId,
    pub release_id: ReleaseId,
    pub title: String,
    pub release_title: String,
    pub artist_names: String,
    pub year: Option<i32>,
    pub available: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScanReport {
    pub discovered: u64,
    pub parsed: u64,
    pub unchanged: u64,
    pub unavailable: u64,
}

/// Friendly application-owned grouping; Releases retain edition-specific identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Album {
    pub album_id: AlbumId,
    pub title: String,
    pub artist_names: String,
    pub year: Option<i32>,
}
