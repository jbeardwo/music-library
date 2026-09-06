//! Synthetic observations belong only to this disposable tool, never to backend APIs.
use music_library::{
    Library,
    domain::{ArtistCreditInput, CatalogReleaseInput, CatalogTrackInput},
};
use rusqlite::{Connection, params};
use tempfile::TempDir;

pub fn create() -> Result<(TempDir, Library), Box<dyn std::error::Error>> {
    let temp = TempDir::new()?;
    let path = temp.path().join("diagnostic.sqlite");
    let mut library = Library::open(&path)?;
    let release = library.create_catalog_release(&CatalogReleaseInput {
        title: "Diagnostic edition".into(),
        year: None,
        artists: vec![],
        tracks: (0..45)
            .map(|i| CatalogTrackInput {
                title: match i {
                    0 => "00 Sourceless".into(),
                    1 => "01 Unavailable".into(),
                    2 => "02 Available".into(),
                    3 => "03 Multiple available sources".into(),
                    // Repeated titles exercise the ID tiebreaker across page boundaries.
                    _ => "Sample duplicate title".into(),
                },
                artists: vec![ArtistCreditInput {
                    name: "Diagnostic artist".into(),
                    role: None,
                }],
                disc_number: None,
                track_number: Some(i + 1),
            })
            .collect(),
    })?;
    for id in &release.track_ids {
        library.add_to_library(id)?;
    }
    drop(library);
    // One live SQLite connection at a time, including during fixture creation.
    let db = Connection::open(&path)?;
    db.execute_batch("PRAGMA foreign_keys=ON; INSERT INTO discovery_root(id,kind,location) VALUES ('sample','local_filesystem',X'2F');")?;
    for (i, track) in release.track_ids.iter().enumerate().skip(1) {
        for n in 0..if i == 3 { 2 } else { 1 } {
            let source = format!("diagnostic-source-{i:02}-{n}");
            let location = temp.path().join(&source);
            #[cfg(unix)]
            let bytes = {
                use std::os::unix::ffi::OsStrExt;
                location.as_os_str().as_bytes().to_vec()
            };
            #[cfg(windows)]
            let bytes: Vec<u8> = {
                use std::os::windows::ffi::OsStrExt;
                location
                    .as_os_str()
                    .encode_wide()
                    .flat_map(u16::to_le_bytes)
                    .collect()
            };
            db.execute(
                "INSERT INTO playable_source(id,kind) VALUES (?1,'local_file')",
                [&source],
            )?;
            db.execute(
                "INSERT INTO track_source(track_id,source_id) VALUES (?1,?2)",
                params![track.as_ref(), source],
            )?;
            db.execute("INSERT INTO local_file_observation(source_id,root_id,path,size_bytes,modified_ns,available) VALUES (?1,'sample',?2,1,1,?3)", params![source,bytes,i != 1])?;
        }
    }
    drop(db);
    let library = Library::open(path)?;
    Ok((temp, library))
}
