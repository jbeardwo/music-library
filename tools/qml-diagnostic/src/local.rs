//! Startup-only local import into a disposable database; never modifies source files.
use music_library::{
    Library,
    domain::{ImportReleaseRequest, ImportTrackInput, ImportedRelease},
    filesystem::LoftyMetadataExtractor,
    matching,
};
use std::{collections::BTreeMap, path::Path};
use tempfile::TempDir;

pub fn load(
    folder: &Path,
) -> Result<(TempDir, Library, Vec<ImportedRelease>), Box<dyn std::error::Error>> {
    let folder = folder.canonicalize()?;
    if !folder.is_dir() {
        return Err("local audio input must be a folder".into());
    }
    let temp = TempDir::new()?;
    let mut library = Library::open(temp.path().join("local-audio.sqlite"))?;
    let root = library.register_local_root(&folder)?;
    library.scan_local_root(&root, &mut LoftyMetadataExtractor)?;
    let mut after = None;
    let mut groups = BTreeMap::new();
    loop {
        let candidates = library.list_discovery_candidates(after.as_ref(), 100)?;
        if candidates.is_empty() {
            break;
        }
        for candidate in candidates {
            after = Some(candidate.source_id.clone());
            // Group tagged files within one directory. Never infer cross-directory edition identity.
            let title = candidate
                .metadata
                .release_title
                .clone()
                .unwrap_or_else(|| "Unknown Album".into());
            let artists = if candidate.metadata.release_artists.is_empty() {
                &candidate.metadata.track_artists
            } else {
                &candidate.metadata.release_artists
            };
            let key = (
                candidate.path.parent().unwrap_or(&folder).to_path_buf(),
                matching::normalize(&title),
                matching::credit(artists).unwrap_or_default(),
            );
            let group = groups.entry(key).or_insert_with(|| ImportReleaseRequest {
                release_title: title,
                release_artists: vec![],
                tracks: vec![],
            });
            group.tracks.push(ImportTrackInput {
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
            });
        }
    }
    let mut imported = Vec::new();
    for request in groups.into_values() {
        imported.push(library.import_release(&request)?);
    }
    Ok((temp, library, imported))
}
