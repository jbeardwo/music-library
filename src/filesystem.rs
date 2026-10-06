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
#[path = "filesystem/provenance.rs"]
pub(crate) mod provenance;

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
        let mut provenance = provenance::extract(tagged.tags());
        provenance.file_type = Some(format!("{:?}", tagged.file_type()));
        Ok(ObservedMetadata {
            genres: tag
                .map(|tag| {
                    tag.get_strings(lofty::tag::ItemKey::Genre)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            provenance,
            track_title: tag.and_then(|tag| tag.title().map(|value| value.into_owned())),
            release_title: tag.and_then(|tag| tag.album().map(|value| value.into_owned())),
            track_artists: tag
                .and_then(|tag| tag.artist().map(|value| vec![value.into_owned()]))
                .unwrap_or_default(),
            release_artists: tag
                .map(|tag| {
                    tag.get_strings(lofty::tag::ItemKey::AlbumArtist)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
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
    // A configured root must still be an inspectable directory, not a replacement file.
    std::fs::read_dir(root).map_err(|source| Error::Filesystem {
        path: root.to_path_buf(),
        source,
    })?;
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
        let known = store.known_local_source(root_id, entry.path())?;
        let item = match observe(entry.path(), known.as_ref(), extractor) {
            Ok(item) => item,
            Err(error) => {
                // This route failed; unseen siblings are not evidence of absence.
                if let Some(known) = known {
                    store.connection.execute(
                        "UPDATE local_file_observation SET available=0,last_observed_at=unixepoch() WHERE source_id=?1 AND available=1",
                        [known.source_id.as_ref()],
                    )?;
                }
                return Err(error);
            }
        };
        if item.metadata.is_some() {
            report.parsed += 1;
        } else {
            report.unchanged += 1;
        }
        batch.push(item);
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

/// Shared metadata observation for traversed and explicitly selected local files.
pub(crate) fn observe(
    path: &Path,
    known: Option<&crate::storage::KnownLocalSource>,
    extractor: &mut dyn MetadataExtractor,
) -> Result<ScannedLocalSource> {
    let file = std::fs::File::open(path).map_err(|source| Error::Filesystem {
        path: path.to_path_buf(),
        source,
    })?;
    let attributes = file.metadata().map_err(|source| Error::Filesystem {
        path: path.to_path_buf(),
        source,
    })?;
    if !attributes.is_file() {
        return Err(Error::Invalid("expected an audio file".into()));
    }
    let size_bytes = attributes.len();
    let modified_ns = modified_ns(path, &attributes)?;
    let unchanged = known.is_some_and(|k| {
        k.size_bytes == size_bytes && k.modified_ns == modified_ns && k.genres_observed
    });
    Ok(ScannedLocalSource {
        source_id: known.map(|k| k.source_id.clone()),
        path: path.to_path_buf(),
        size_bytes,
        modified_ns,
        metadata: if unchanged {
            None
        } else {
            Some(extractor.read(path)?)
        },
    })
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
