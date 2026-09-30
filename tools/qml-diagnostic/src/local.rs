//! Startup-only local import into a disposable database; never modifies source files.
use music_library::{Library, domain::ImportedRelease};
use std::path::Path;
use tempfile::TempDir;

pub fn load(
    folder: &Path,
) -> Result<(TempDir, Library, Vec<ImportedRelease>), Box<dyn std::error::Error>> {
    let folder = folder.canonicalize()?;
    if !folder.is_dir() {
        return Err("local audio input must be a folder".into());
    }
    let temp = TempDir::new()?;
    let database = std::env::var_os("MUSIC_LIBRARY_DIAGNOSTIC_DATABASE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| temp.path().join("local-audio.sqlite"));
    let mut library = Library::open(database)?;
    library.register_local_root(&folder)?;
    // The same background configured-location ingestion used by From file runs after QML opens.
    Ok((temp, library, vec![]))
}
