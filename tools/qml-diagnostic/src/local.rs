//! Startup-only local folder import into a disposable database; never modifies source files.
use music_library::{
    Library,
    domain::{ImportReleaseRequest, ImportTrackInput},
    filesystem::LoftyMetadataExtractor,
};
use std::path::Path;
use tempfile::TempDir;

pub fn load(folder: &Path) -> Result<(TempDir, Library), Box<dyn std::error::Error>> {
    let folder = folder.canonicalize()?;
    if !folder.is_dir() {
        return Err("local audio input must be a folder".into());
    }
    let temp = TempDir::new()?;
    let mut library = Library::open(temp.path().join("local-audio.sqlite"))?;
    let root = library.register_local_root(&folder)?;
    library.scan_local_root(&root, &mut LoftyMetadataExtractor)?;
    let mut after = None;
    loop {
        let candidates = library.list_discovery_candidates(after.as_ref(), 100)?;
        if candidates.is_empty() {
            break;
        }
        for candidate in candidates {
            after = Some(candidate.source_id.clone());
            // Deliberately conservative disposable import: one Release per source, no album matching.
            let release = library.import_release(&ImportReleaseRequest {
                release_title: candidate
                    .metadata
                    .release_title
                    .unwrap_or_else(|| "Local audio diagnostic".into()),
                release_artists: vec![],
                tracks: vec![ImportTrackInput {
                    source_id: candidate.source_id,
                    title_fallback: Some(
                        candidate
                            .path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                    ),
                    artists: vec![],
                    disc_number: candidate.metadata.disc_number,
                    track_number: candidate.metadata.track_number,
                }],
            })?;
            for id in release.track_ids {
                library.add_to_library(&id)?;
            }
        }
    }
    Ok((temp, library))
}
