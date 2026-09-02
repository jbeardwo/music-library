use std::fs::Metadata;
use std::path::Path;
use std::time::UNIX_EPOCH;

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::prelude::Accessor;
use lofty::probe::Probe;
use walkdir::WalkDir;

use crate::domain::{ObservedMetadata, RootId, ScanReport};
use crate::storage::{Error, Result, ScannedLocalSource, Store};

const SCAN_BATCH_SIZE: usize = 256;

pub trait MetadataExtractor {
    fn supports(&self, path: &Path) -> bool;
    fn read(&mut self, path: &Path) -> Result<ObservedMetadata>;
}

#[derive(Default)]
pub struct LoftyMetadataExtractor;

impl MetadataExtractor for LoftyMetadataExtractor {
    fn supports(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "aac"
                        | "aiff"
                        | "ape"
                        | "flac"
                        | "m4a"
                        | "mp3"
                        | "mp4"
                        | "ogg"
                        | "opus"
                        | "wav"
                        | "wv"
                )
            })
    }

    fn read(&mut self, path: &Path) -> Result<ObservedMetadata> {
        let tagged = Probe::open(path)
            .and_then(Probe::read)
            .map_err(|error| Error::Metadata {
                path: path.to_path_buf(),
                message: error.to_string(),
            })?;
        let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
        let properties = tagged.properties();
        Ok(ObservedMetadata {
            track_title: tag.and_then(|tag| tag.title().map(|value| value.into_owned())),
            release_title: tag.and_then(|tag| tag.album().map(|value| value.into_owned())),
            track_artists: tag
                .and_then(|tag| tag.artist().map(|value| vec![value.into_owned()]))
                .unwrap_or_default(),
            release_artists: Vec::new(),
            disc_number: tag.and_then(|tag| tag.disk()),
            track_number: tag.and_then(|tag| tag.track()),
            year: tag
                .and_then(|tag| tag.date())
                .map(|value| value.year as i32),
            duration_ms: Some(properties.duration().as_millis() as u64),
            format: path
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| value.to_ascii_lowercase()),
        })
    }
}

pub(crate) fn scan(
    store: &mut Store,
    root_id: &RootId,
    extractor: &mut dyn MetadataExtractor,
) -> Result<ScanReport> {
    let root = store.root_path(root_id)?;
    let scan_id = store.begin_scan(root_id)?;
    let result = scan_started(store, root_id, scan_id, &root, extractor);
    if result.is_err() {
        store.fail_scan(scan_id)?;
    }
    result
}

fn scan_started(
    store: &mut Store,
    root_id: &RootId,
    scan_id: i64,
    root: &Path,
    extractor: &mut dyn MetadataExtractor,
) -> Result<ScanReport> {
    let mut report = ScanReport::default();
    let mut batch = Vec::with_capacity(SCAN_BATCH_SIZE);
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|error| Error::Filesystem {
            path: error.path().unwrap_or(root).to_path_buf(),
            source: error
                .io_error()
                .map(|inner| std::io::Error::new(inner.kind(), inner.to_string()))
                .unwrap_or_else(|| std::io::Error::other(error.to_string())),
        })?;
        if !entry.file_type().is_file() || !extractor.supports(entry.path()) {
            continue;
        }
        let attributes = entry.metadata().map_err(|error| Error::Filesystem {
            path: entry.path().to_path_buf(),
            source: std::io::Error::other(error.to_string()),
        })?;
        let size_bytes = attributes.len();
        let modified_ns = modified_ns(entry.path(), &attributes)?;
        let known = store.known_local_source(root_id, entry.path())?;
        let unchanged = known.as_ref().is_some_and(|known| {
            known.size_bytes == size_bytes && known.modified_ns == modified_ns
        });
        let metadata = if unchanged {
            report.unchanged += 1;
            None
        } else {
            report.parsed += 1;
            Some(extractor.read(entry.path())?)
        };
        batch.push(ScannedLocalSource {
            source_id: known.map(|known| known.source_id),
            path: entry.path().to_path_buf(),
            size_bytes,
            modified_ns,
            metadata,
        });
        report.discovered += 1;
        if batch.len() == SCAN_BATCH_SIZE {
            store.apply_scan_batch(root_id, scan_id, &batch)?;
            batch.clear();
        }
    }
    if !batch.is_empty() {
        store.apply_scan_batch(root_id, scan_id, &batch)?;
    }
    report.unavailable = store.complete_scan(root_id, scan_id)?;
    Ok(report)
}

fn modified_ns(path: &Path, metadata: &Metadata) -> Result<i64> {
    let modified = metadata.modified().map_err(|source| Error::Filesystem {
        path: path.to_path_buf(),
        source,
    })?;
    let nanos = match modified.duration_since(UNIX_EPOCH) {
        Ok(duration) => i128::try_from(duration.as_nanos()).unwrap_or(i128::MAX),
        Err(error) => -i128::try_from(error.duration().as_nanos()).unwrap_or(i128::MAX),
    };
    i64::try_from(nanos).map_err(|_| {
        Error::Invalid(format!(
            "modification time for {} is out of range",
            path.display()
        ))
    })
}
