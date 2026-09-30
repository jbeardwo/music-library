use music_library::{
    Library,
    browse::{Pane, Request},
    domain::*,
    filesystem::MetadataExtractor,
};
use std::path::Path;
struct Tags;
impl MetadataExtractor for Tags {
    fn supports(&self, _: &Path) -> bool {
        true
    }
    fn read(&mut self, path: &Path) -> music_library::Result<ObservedMetadata> {
        let album = path
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap();
        let artist = path
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap();
        Ok(ObservedMetadata {
            track_title: Some("Song".into()),
            release_title: Some(album.into()),
            release_artists: vec![artist.into()],
            ..Default::default()
        })
    }
}
fn import(l: &mut Library, root: &Path, album: &str, artist: &str) -> ArtistId {
    let directory = root.join("Bygones").join(album);
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join("song.flac");
    std::fs::write(&file, b"fixture").unwrap();
    let root = l.register_local_root(root).unwrap();
    l.scan_local_root(&root, &mut Tags).unwrap();
    let source = l
        .list_discovery_candidates(None, 100)
        .unwrap()
        .into_iter()
        .find(|c| c.path == file)
        .unwrap();
    let release = l
        .import_release(&ImportReleaseRequest {
            release_title: album.into(),
            release_artists: vec![ArtistCreditInput {
                name: artist.into(),
                role: None,
            }],
            tracks: vec![ImportTrackInput {
                source_id: source.source_id,
                title_fallback: None,
                artists: vec![],
                disc_number: Some(1),
                track_number: Some(1),
            }],
        })
        .unwrap();
    let album = l.album_for_release(&release.release_id).unwrap();
    // Public panes must reflect the canonical storage identity, without UI grouping.
    let artists = l
        .browse(&Request {
            pane: Pane::Artists,
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    artists
        .into_iter()
        .map(|r| ArtistId(r.id))
        .find(|id| {
            l.browse(&Request {
                pane: Pane::Albums,
                artist: Some(id.clone()),
                limit: 200,
                ..Default::default()
            })
            .unwrap()
            .iter()
            .any(|r| r.id == album.album_id.as_ref())
        })
        .unwrap()
}
#[test]
fn same_artist_directory_consolidates_case_variants_but_other_roots_do_not() {
    let temp = tempfile::tempdir().unwrap();
    let mut l = Library::open(temp.path().join("library.db")).unwrap();
    let root = temp.path().join("music");
    std::fs::create_dir(&root).unwrap();
    let a = import(&mut l, &root, "First", "Bygones");
    l.attach_artist_external_identity(
        &a,
        &ExternalIdentity {
            provider: "spotify".into(),
            kind: "artist".into(),
            external_id: "trusted".into(),
        },
    )
    .unwrap();
    let b = import(&mut l, &root, "Second", "bygones");
    assert_eq!(a, b);
    assert_eq!(
        l.browse(&Request {
            pane: Pane::Artists,
            limit: 200,
            ..Default::default()
        })
        .unwrap()
        .len(),
        1
    );
    assert_eq!(
        l.library_queue(&Request {
            artist: Some(a.clone()),
            ..Default::default()
        })
        .unwrap()
        .len(),
        2
    );
    let other = temp.path().join("other music");
    std::fs::create_dir(&other).unwrap();
    let c = import(&mut l, &other, "Other artist", "Bygones");
    assert_ne!(a, c);
    let db = rusqlite::Connection::open(temp.path().join("library.db")).unwrap();
    let names: Vec<String> = db
        .prepare("SELECT credited_name FROM album_artist_credit ORDER BY credited_name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(names, ["Bygones", "Bygones", "bygones"]);
}

#[test]
fn migration_preserves_conflicting_artist_ids_and_unrelated_equal_names() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.db");
    let mut l = Library::open(&path).unwrap();
    let root = temp.path().join("music");
    std::fs::create_dir(&root).unwrap();
    let a = import(&mut l, &root, "One", "Bygones");
    let other = temp.path().join("other");
    std::fs::create_dir(&other).unwrap();
    let b = import(&mut l, &other, "Two", "Bygones");
    for (id, value) in [(&a, "artist-one"), (&b, "artist-two")] {
        l.attach_artist_external_identity(
            id,
            &ExternalIdentity {
                provider: "spotify".into(),
                kind: "artist".into(),
                external_id: value.into(),
            },
        )
        .unwrap();
    }
    assert!(l.merge_artist(&a, &b).is_err());
    drop(l);
    let db = rusqlite::Connection::open(&path).unwrap();
    // Simulate old conflicting IDs in one Artist directory before migration.
    let root_id: String = db
        .query_row(
            "SELECT id FROM discovery_root WHERE location=?1",
            [root.as_os_str().as_encoded_bytes()],
            |r| r.get(0),
        )
        .unwrap();
    db.execute(
        "UPDATE local_file_observation SET root_id=?1,path=?2 WHERE root_id<>?1",
        rusqlite::params![
            root_id,
            root.join("Bygones/Two/song.flac")
                .as_os_str()
                .as_encoded_bytes()
        ],
    )
    .unwrap();
    db.execute_batch(
        "DROP TRIGGER IF EXISTS artist_lookup_insert;
DROP TRIGGER IF EXISTS artist_lookup_update;
DROP TRIGGER IF EXISTS artist_lookup_delete;
DROP TRIGGER IF EXISTS album_lookup_insert;
DROP TRIGGER IF EXISTS album_lookup_update;
DROP TRIGGER IF EXISTS album_lookup_delete;
DROP TABLE IF EXISTS artist_lookup;
DROP TABLE IF EXISTS album_lookup;
DROP TABLE local_root_source; DROP TABLE local_source_suppression; DROP TABLE output_calibration; DROP TABLE local_artist_context; DROP TABLE file_genre_observation; ALTER TABLE file_metadata_observation DROP COLUMN genres_observed; PRAGMA user_version=13;",
    )
    .unwrap();
    drop(db);
    let l = Library::open(&path).unwrap();
    assert_eq!(
        l.browse(&Request {
            pane: Pane::Artists,
            limit: 200,
            ..Default::default()
        })
        .unwrap()
        .len(),
        2
    );
}
