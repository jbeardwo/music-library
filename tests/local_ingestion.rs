#[path = "support/metadata_schema.rs"]
mod metadata_schema;
use music_library::{
    Error, Library, Result,
    browse::{Pane, Request as Browse},
    domain::*,
    filesystem::{LoftyMetadataExtractor, MetadataExtractor},
    library_removal::Target,
    library_search::Kind,
    local_ingestion::{Report, Request},
};
use rusqlite::Connection;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Default)]
struct Tags {
    reads: usize,
}
impl MetadataExtractor for Tags {
    fn supports(&self, path: &Path) -> bool {
        path.extension().is_some_and(|e| e == "flac")
    }
    fn read(&mut self, path: &Path) -> Result<ObservedMetadata> {
        self.reads += 1;
        if path.file_stem().unwrap() == "broken" {
            return Err(Error::Metadata {
                path: path.into(),
                message: "test malformed audio".into(),
            });
        }
        Ok(ObservedMetadata {
            track_title: Some(path.file_stem().unwrap().to_string_lossy().into()),
            release_title: Some("Album".into()),
            track_artists: vec!["Artist".into()],
            release_artists: vec!["Artist".into()],
            track_number: Some(1),
            disc_number: Some(1),
            ..Default::default()
        })
    }
}
fn run(l: &mut Library, request: Request, tags: &mut dyn MetadataExtractor) -> Report {
    l.ingest_local(&request, tags, &mut |_| {}).unwrap()
}
fn songs(l: &Library) -> Vec<music_library::browse::Row> {
    l.browse(&Browse {
        pane: Pane::Songs,
        limit: 200,
        ..Default::default()
    })
    .unwrap()
}
fn folder(base: &Path, name: &str, files: &[&str]) -> PathBuf {
    let folder = base.join(name);
    std::fs::create_dir_all(&folder).unwrap();
    for file in files {
        std::fs::write(folder.join(file), b"disposable").unwrap();
    }
    folder
}
#[test]
fn single_multi_file_import_no_root_unchanged_dedup_and_later_scan_adoption() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let music = folder(temp.path(), "downloads", &["first.flac", "second.flac"]);
    let mut l = Library::open(&path).unwrap();
    let mut tags = Tags::default();
    let first = music.join("first.flac");
    let second = music.join("second.flac");
    assert_eq!(
        run(&mut l, Request::Files(vec![first.clone()]), &mut tags).imported,
        1
    );
    assert!(l.local_locations().unwrap().is_empty());
    let track = songs(&l)[0].track.as_ref().unwrap().track_id.clone();
    let source = l
        .available_playback_source(&track)
        .unwrap()
        .unwrap()
        .source_id;
    let report = run(
        &mut l,
        Request::Files(vec![first.clone(), second.clone(), first.clone()]),
        &mut tags,
    );
    assert_eq!(
        (report.imported, report.parsed, report.unchanged),
        (1, 1, 1)
    );
    assert_eq!(tags.reads, 2);
    assert!(l.local_locations().unwrap().is_empty());
    let root = l.register_local_root(&music).unwrap();
    let scan = l.scan_local_root(&root, &mut tags).unwrap();
    assert_eq!(scan.parsed, 0);
    assert_eq!(scan.unchanged, 2);
    assert_eq!(
        l.available_playback_source(&track)
            .unwrap()
            .unwrap()
            .source_id,
        source
    );
    assert_eq!(run(&mut l, Request::Rescan(root), &mut tags).imported, 0);
    assert_eq!(songs(&l).len(), 2);
    assert_eq!(tags.reads, 2);
    drop(l);
    let l = Library::open(path).unwrap();
    assert_eq!(songs(&l).len(), 2);
    for p in [first, second] {
        assert_eq!(std::fs::read(p).unwrap(), b"disposable");
    }
}
#[test]
fn roots_persist_repeat_independently_and_removal_does_not_remove_locations() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let a = folder(temp.path(), "one", &["one.flac"]);
    let b = folder(temp.path(), "two", &["two.flac"]);
    let mut l = Library::open(&path).unwrap();
    let mut tags = Tags::default();
    assert_eq!(
        run(&mut l, Request::Folder(a.clone()), &mut tags).imported,
        1
    );
    assert_eq!(
        run(&mut l, Request::Folder(b.clone()), &mut tags).imported,
        1
    );
    let roots = l.local_locations().unwrap();
    assert_eq!(roots.len(), 2);
    assert_eq!(
        l.register_local_root(a.join(".")).unwrap(),
        roots.iter().find(|r| r.path == a).unwrap().id
    );
    assert_eq!(
        run(&mut l, Request::Folder(a.clone()), &mut tags).imported,
        0
    );
    let removed = songs(&l)
        .into_iter()
        .find(|r| r.title == "one")
        .unwrap()
        .track
        .unwrap()
        .track_id;
    l.remove_library_object(&Target::Track(removed), true)
        .unwrap();
    assert_eq!(l.local_locations().unwrap().len(), 2);
    drop(l);
    let mut l = Library::open(&path).unwrap();
    assert_eq!(l.local_locations().unwrap().len(), 2);
    let report = run(&mut l, Request::ConfiguredLocations, &mut tags);
    assert_eq!(report.imported, 0);
    assert_eq!(songs(&l).len(), 1);
    assert_eq!(tags.reads, 2);
    assert_eq!(run(&mut l, Request::Folder(a), &mut tags).imported, 1);
    assert_eq!(songs(&l).len(), 2);
    // Future location removal keeps sources/membership/exclusions, unlike Track removal.
    let source = l
        .available_playback_source(&songs(&l)[0].track.as_ref().unwrap().track_id)
        .unwrap();
    assert!(l.remove_local_location(&roots[0].id).unwrap());
    assert_eq!(l.local_locations().unwrap().len(), 1);
    assert_eq!(songs(&l).len(), 2);
    assert!(source.is_some());
    assert_eq!(
        Connection::open(path)
            .unwrap()
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        0
    );
}
#[test]
fn suppression_exact_file_and_selected_folder_override_only_successfully_admitted_files() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let a = folder(temp.path(), "a", &["one.flac", "two.flac"]);
    let b = folder(temp.path(), "b", &["other.flac"]);
    let mut l = Library::open(&path).unwrap();
    let mut tags = Tags::default();
    run(&mut l, Request::Folder(a.clone()), &mut tags);
    run(&mut l, Request::Folder(b.clone()), &mut tags);
    let tracks: HashMap<_, _> = songs(&l)
        .into_iter()
        .map(|r| (r.title, r.track.unwrap().track_id))
        .collect();
    for track in tracks.values() {
        l.remove_library_object(&Target::Track(track.clone()), true)
            .unwrap();
    }
    assert_eq!(
        run(&mut l, Request::ConfiguredLocations, &mut tags).imported,
        0
    );
    assert!(songs(&l).is_empty());
    assert_eq!(tags.reads, 3);
    assert_eq!(
        run(&mut l, Request::Files(vec![a.join("one.flac")]), &mut tags).imported,
        1
    );
    assert_eq!(songs(&l)[0].track.as_ref().unwrap().track_id, tracks["one"]);
    assert_eq!(
        run(&mut l, Request::Folder(a.clone()), &mut tags).imported,
        1
    );
    assert_eq!(songs(&l).len(), 2);
    assert!(!songs(&l).iter().any(|s| s.title == "other"));
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM local_source_suppression", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        1
    );
    // A suppressed file that changes into unreadable audio stays suppressed.
    let other = l
        .available_playback_source(&tracks["other"])
        .unwrap()
        .unwrap()
        .source_id;
    std::fs::write(b.join("other.flac"), b"changed bytes").unwrap();
    struct Fails;
    impl MetadataExtractor for Fails {
        fn supports(&self, _: &Path) -> bool {
            true
        }
        fn read(&mut self, p: &Path) -> Result<ObservedMetadata> {
            Err(Error::Metadata {
                path: p.into(),
                message: "unreadable".into(),
            })
        }
    }
    let report = run(&mut l, Request::Folder(b), &mut Fails);
    assert_eq!(report.imported, 0);
    assert_eq!(report.unreadable, 1);
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM local_source_suppression WHERE source_id=?1",
            [other.as_ref()],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        run(&mut l, Request::ConfiguredLocations, &mut tags).imported,
        0
    );
    assert_eq!(l.local_locations().unwrap().len(), 2);
}
#[test]
fn mixed_files_and_folder_errors_are_compact_nonfatal_and_do_not_hide_encountered_music() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let music = folder(
        temp.path(),
        "audio",
        &["valid.flac", "broken.flac", "notes.txt"],
    );
    let mut l = Library::open(&path).unwrap();
    let mut tags = Tags::default();
    let report = run(
        &mut l,
        Request::Files(vec![
            music.join("valid.flac"),
            music.join("broken.flac"),
            music.join("notes.txt"),
            music.join("missing.flac"),
        ]),
        &mut tags,
    );
    assert_eq!(
        (report.imported, report.unsupported, report.unreadable),
        (1, 1, 2)
    );
    assert_eq!(run(&mut l, Request::Folder(music), &mut tags).unreadable, 1);
    assert_eq!(songs(&l).len(), 1);
    let empty = folder(temp.path(), "empty", &[]);
    assert_eq!(run(&mut l, Request::Folder(empty), &mut tags).imported, 0);
    assert!(
        l.ingest_local(
            &Request::Folder(temp.path().join("missing")),
            &mut tags,
            &mut |_| {}
        )
        .is_err()
    );
    assert_eq!(l.local_search("valid", Kind::Song).unwrap().len(), 1);
}
fn real_file(folder: &Path) -> PathBuf {
    use lofty::{
        config::WriteOptions,
        picture::{Picture, PictureType},
        prelude::{Accessor, TagExt},
        tag::{ItemKey, Tag, TagType},
    };
    let path = folder.join("real.flac");
    std::fs::copy("tests/fixtures/provenance/silence.flac", &path).unwrap();
    let mut tag = Tag::new(TagType::VorbisComments);
    tag.set_title("Local Song".into());
    tag.set_album("Local Album".into());
    tag.set_artist("Local Artist".into());
    tag.set_track(1);
    tag.set_disk(1);
    tag.insert_text(ItemKey::AlbumArtist, "Local Artist".into());
    tag.insert_text(
        ItemKey::MusicBrainzRecordingId,
        "preserved-observed-recording".into(),
    );
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(120, 60)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let mut pic = Picture::from_reader(&mut std::io::Cursor::new(bytes.into_inner())).unwrap();
    pic.set_pic_type(PictureType::CoverFront);
    tag.push_picture(pic);
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    path
}
#[test]
fn real_metadata_provenance_artwork_and_persistence_match_automatic_scanning() {
    let temp = tempfile::tempdir().unwrap();
    let music = folder(temp.path(), "music", &[]);
    let file = real_file(&music);
    let before = std::fs::read(&file).unwrap();
    let mut a = Library::open(temp.path().join("auto")).unwrap();
    let mut b = Library::open(temp.path().join("explicit")).unwrap();
    let root = a.register_local_root(&music).unwrap();
    let automatic = run(&mut a, Request::Rescan(root), &mut LoftyMetadataExtractor);
    let explicit = run(
        &mut b,
        Request::Files(vec![file.clone()]),
        &mut LoftyMetadataExtractor,
    );
    assert_eq!((automatic.imported, explicit.imported), (1, 1));
    assert!(b.local_locations().unwrap().is_empty());
    for column in ["track_title", "release_title", "provenance_json", "format"] {
        let query = format!("SELECT {column} FROM file_metadata_observation");
        let left: String = Connection::open(temp.path().join("auto"))
            .unwrap()
            .query_row(&query, [], |r| r.get(0))
            .unwrap();
        let right: String = Connection::open(temp.path().join("explicit"))
            .unwrap()
            .query_row(&query, [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, right);
        if column == "provenance_json" {
            assert!(left.contains("preserved-observed-recording"));
        }
    }
    let left = songs(&a)[0].track.clone().unwrap();
    let right = songs(&b)[0].track.clone().unwrap();
    assert_eq!(
        (left.title, left.release_title, left.artist_names),
        (right.title, right.release_title, right.artist_names)
    );
    struct Offline;
    impl music_library::artwork::Provider for Offline {
        fn fetch(&mut self, _: &ExternalIdentity) -> Option<(String, Vec<u8>)> {
            panic!("local artwork must resolve before providers")
        }
    }
    for (library, report, cache) in [
        (&a, automatic, temp.path().join("art-a")),
        (&b, explicit, temp.path().join("art-b")),
    ] {
        let album = library
            .album_for_release(&report.releases[0].release_id)
            .unwrap()
            .album_id;
        let mut resolver = library.artwork_resolver(cache).unwrap();
        let art = resolver.resolve(&[album.0], &mut Offline).unwrap()[0]
            .1
            .clone()
            .unwrap();
        assert_eq!(image::image_dimensions(art).unwrap(), (120, 60));
        assert_eq!(
            library
                .local_search("Local Song", Kind::Song)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(report.releases[0].track_ids.len(), 1);
    }
    std::fs::write(music.join("bad.flac"), b"malformed").unwrap();
    let report = run(
        &mut b,
        Request::Files(vec![music.join("bad.flac"), file.clone()]),
        &mut LoftyMetadataExtractor,
    );
    assert_eq!(report.unreadable, 1);
    assert_eq!(report.unchanged, 1);
    assert_eq!(std::fs::read(file).unwrap(), before);
    drop(b);
    let b = Library::open(temp.path().join("explicit")).unwrap();
    assert_eq!(songs(&b).len(), 1);
}
#[test]
fn shared_post_import_matching_is_best_effort_during_provider_outage() {
    use music_library::{
        album_matching::{AlbumMatcher, AutoMatchPolicy, MatchOutcome},
        catalog::*,
    };
    struct Offline;
    impl CatalogProvider for Offline {
        fn search_artists(
            &mut self,
            _: &str,
        ) -> std::result::Result<Page<ArtistCandidate>, CatalogError> {
            Err(CatalogError::TransportUnavailable("offline test".into()))
        }
        fn search_albums(
            &mut self,
            _: &str,
            _: u32,
        ) -> std::result::Result<Page<AlbumCandidate>, CatalogError> {
            unreachable!()
        }
        fn releases(
            &mut self,
            _: &ExternalIdentity,
            _: u32,
        ) -> std::result::Result<Page<ReleaseCandidate>, CatalogError> {
            unreachable!()
        }
        fn release(&mut self, _: &ExternalIdentity) -> std::result::Result<Release, CatalogError> {
            unreachable!()
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let music = folder(temp.path(), "files", &["song.flac"]);
    for request in [
        Request::Files(vec![music.join("song.flac")]),
        Request::Folder(music),
    ] {
        let mut l = Library::open_in_memory().unwrap();
        let report = run(&mut l, request, &mut Tags::default());
        assert_eq!(report.imported, 1);
        let (tx, rx) = std::sync::mpsc::channel();
        let mut matcher = AlbumMatcher::new(
            Offline,
            move |r| {
                tx.send(r).unwrap();
            },
            |_| {},
        )
        .unwrap();
        let queued = matcher
            .after_import(&l, &report.releases, AutoMatchPolicy::default())
            .unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].1, MatchOutcome::Pending);
        let reply = rx.recv_timeout(std::time::Duration::from_secs(3)).unwrap();
        matcher.complete(&mut l, reply);
        assert_eq!(songs(&l).len(), 1);
        assert_eq!(l.local_search("song", Kind::Song).unwrap().len(), 1);
    }
}
#[test]
fn indexed_batch_path_suppression_lookup_and_multiple_locations_are_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let mut l = Library::open(&path).unwrap();
    let mut tags = Tags::default();
    let a = folder(temp.path(), "a", &["song.flac"]);
    let b = folder(temp.path(), "b", &["other.flac"]);
    run(&mut l, Request::Folder(a.clone()), &mut tags);
    run(&mut l, Request::Folder(b), &mut tags);
    let root = l
        .local_locations()
        .unwrap()
        .into_iter()
        .find(|r| r.path == a)
        .unwrap()
        .id;
    let report = run(&mut l, Request::Rescan(root), &mut tags);
    assert_eq!(report.scanned, 1);
    assert_eq!(tags.reads, 2);
    let db = Connection::open(path).unwrap();
    let plan=db.prepare("EXPLAIN QUERY PLAN SELECT l.source_id,x.source_id FROM json_each(?1) p CROSS JOIN local_file_observation l INDEXED BY local_file_path ON l.path=unhex(p.value) LEFT JOIN local_source_suppression x ON x.source_id=l.source_id").unwrap().query_map(["[]"],|r|r.get::<_,String>(3)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap().join("\n");
    assert!(
        plan.contains("local_file_path") && plan.contains("SEARCH x") && !plan.contains("SCAN l"),
        "{plan}"
    );
}

#[test]
fn nested_folder_selection_reuses_source_exclusion_and_independent_root_observations() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let album = folder(temp.path(), "music/Artist/Album", &["song.flac"]);
    let parent = temp.path().join("music");
    let mut l = Library::open(&path).unwrap();
    let mut tags = Tags::default();
    let outer = l.register_local_root(&parent).unwrap();
    run(&mut l, Request::Rescan(outer.clone()), &mut tags);
    let track = songs(&l)[0].track.as_ref().unwrap().track_id.clone();
    let source = l
        .available_playback_source(&track)
        .unwrap()
        .unwrap()
        .source_id;
    l.remove_library_object(&Target::Track(track.clone()), true)
        .unwrap();
    assert_eq!(
        run(&mut l, Request::Rescan(outer.clone()), &mut tags).imported,
        0
    );
    assert_eq!(
        run(&mut l, Request::Folder(album.clone()), &mut tags).imported,
        1
    );
    assert_eq!(songs(&l)[0].track.as_ref().unwrap().track_id, track);
    assert_eq!(
        l.available_playback_source(&track)
            .unwrap()
            .unwrap()
            .source_id,
        source
    );
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM local_file_observation", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM local_source_suppression", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM local_root_source", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(l.local_locations().unwrap().len(), 2);
    assert_eq!(
        run(&mut l, Request::ConfiguredLocations, &mut tags).imported,
        0
    );
    assert_eq!(tags.reads, 1);
    assert!(l.remove_local_location(&outer).unwrap());
    assert_eq!(
        run(&mut l, Request::ConfiguredLocations, &mut tags).unchanged,
        1
    );
    assert_eq!(
        l.available_playback_source(&track)
            .unwrap()
            .unwrap()
            .source_id,
        source
    );
    assert_eq!(songs(&l).len(), 1);
    assert_eq!(l.local_locations().unwrap()[0].path, album);
}

#[test]
fn migration_from_18_preserves_exclusion_identity_and_seeds_root_links() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let music = folder(temp.path(), "music", &["song.flac"]);
    let mut l = Library::open(&path).unwrap();
    run(&mut l, Request::Folder(music.clone()), &mut Tags::default());
    let track = songs(&l)[0].track.as_ref().unwrap().track_id.clone();
    let source = l
        .available_playback_source(&track)
        .unwrap()
        .unwrap()
        .source_id;
    l.remove_library_object(&Target::Track(track.clone()), true)
        .unwrap();
    drop(l);
    let db = Connection::open(&path).unwrap();
    metadata_schema::downgrade(&db);
    db.execute_batch(include_str!("support/drop_song_details.sql"))
        .unwrap();
    db.execute_batch(
        "DROP TABLE local_root_source; DROP INDEX local_file_path; DROP TABLE file_genre_observation; ALTER TABLE file_metadata_observation DROP COLUMN genres_observed; DROP TABLE IF EXISTS playlist_source; DROP TABLE IF EXISTS playlist_entry; DROP TABLE IF EXISTS playlist; PRAGMA user_version=18;",
    )
    .unwrap();
    drop(db);
    let mut l = Library::open(&path).unwrap();
    assert!(songs(&l).is_empty());
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM local_source_suppression WHERE source_id=?1",
            [source.as_ref()],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM local_root_source WHERE source_id=?1",
            [source.as_ref()],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        30
    );
    assert_eq!(
        run(&mut l, Request::ConfiguredLocations, &mut Tags::default()).imported,
        0
    );
    assert_eq!(
        run(
            &mut l,
            Request::Files(vec![music.join("song.flac")]),
            &mut Tags::default()
        )
        .imported,
        1
    );
    assert_eq!(songs(&l)[0].track.as_ref().unwrap().track_id, track);
}
#[test]
fn failed_location_migration_rolls_back_sources_and_exclusions() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let music = folder(temp.path(), "music", &["song.flac"]);
    let mut l = Library::open(&path).unwrap();
    run(&mut l, Request::Folder(music), &mut Tags::default());
    let track = songs(&l)[0].track.as_ref().unwrap().track_id.clone();
    l.remove_library_object(&Target::Track(track), true)
        .unwrap();
    drop(l);
    let db = Connection::open(&path).unwrap();
    metadata_schema::downgrade(&db);
    db.execute_batch(include_str!("support/drop_song_details.sql"))
        .unwrap();
    db.execute_batch("DROP TABLE local_root_source; CREATE TABLE local_root_source(sentinel TEXT); DROP INDEX local_file_path; DROP TABLE file_genre_observation; ALTER TABLE file_metadata_observation DROP COLUMN genres_observed; DROP TABLE IF EXISTS playlist_source; DROP TABLE IF EXISTS playlist_entry; DROP TABLE IF EXISTS playlist; PRAGMA user_version=18;").unwrap();
    assert!(Library::open(&path).is_err());
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        18
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM local_file_observation", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM local_source_suppression", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
}
#[test]
fn sidecar_artwork_uses_the_same_resolver_for_detached_and_scanned_files() {
    struct Offline;
    impl music_library::artwork::Provider for Offline {
        fn fetch(&mut self, _: &ExternalIdentity) -> Option<(String, Vec<u8>)> {
            panic!("sidecar must precede provider artwork")
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let music = folder(temp.path(), "music", &[]);
    let file = music.join("song.flac");
    std::fs::copy("tests/fixtures/provenance/silence.flac", &file).unwrap();
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(60, 120)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    std::fs::write(music.join("cover.png"), png.into_inner()).unwrap();
    for (name, request) in [
        ("scanned", Request::Folder(music)),
        ("selected", Request::Files(vec![file.clone()])),
    ] {
        let mut l = Library::open(temp.path().join(name)).unwrap();
        let report = run(&mut l, request, &mut LoftyMetadataExtractor);
        let album = l
            .album_for_release(&report.releases[0].release_id)
            .unwrap()
            .album_id;
        let mut resolver = l
            .artwork_resolver(temp.path().join(format!("cache-{name}")))
            .unwrap();
        let art = resolver.resolve(&[album.0], &mut Offline).unwrap()[0]
            .1
            .clone()
            .unwrap();
        assert_eq!(image::image_dimensions(art).unwrap(), (60, 120));
    }
}

#[test]
#[ignore = "opt-in targeted import with 200k unrelated persisted sources"]
fn targeted_import_with_200k_unrelated_sources() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let mut l = Library::open(&path).unwrap();
    let mut db = Connection::open(&path).unwrap();
    {
        let tx = db.transaction().unwrap();
        let mut source = tx
            .prepare("INSERT INTO playable_source(id,kind) VALUES (?1,'local_file')")
            .unwrap();
        let mut observation=tx.prepare("INSERT INTO local_file_observation(source_id,path,size_bytes,modified_ns,available) VALUES (?1,?2,1,1,0)").unwrap();
        let mut suppress = tx
            .prepare("INSERT INTO local_source_suppression(source_id) VALUES (?1)")
            .unwrap();
        for i in 0..200000 {
            let id = format!("fixture-source-{i:08}");
            let file = temp.path().join("unselected").join(format!("{i}.flac"));
            #[cfg(unix)]
            let bytes = {
                use std::os::unix::ffi::OsStrExt;
                file.as_os_str().as_bytes().to_vec()
            };
            #[cfg(windows)]
            let bytes = {
                use std::os::windows::ffi::OsStrExt;
                file.as_os_str()
                    .encode_wide()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>()
            };
            source.execute([&id]).unwrap();
            observation.execute(rusqlite::params![&id, bytes]).unwrap();
            if i % 2 == 0 {
                suppress.execute([&id]).unwrap();
            }
        }
        drop(source);
        drop(observation);
        drop(suppress);
        tx.commit().unwrap();
    }
    let mut tags = Tags::default();
    let mut locations = Vec::new();
    for name in ["first", "second"] {
        let music = folder(temp.path(), name, &["song.flac"]);
        let start = std::time::Instant::now();
        let report = run(&mut l, Request::Folder(music.clone()), &mut tags);
        eprintln!(
            "200k-source context, selected {name} folder: {:?}",
            start.elapsed()
        );
        assert_eq!((report.scanned, report.parsed, report.imported), (1, 1, 1));
        locations.push(music);
    }
    assert_eq!(tags.reads, 2);
    assert_eq!(l.local_locations().unwrap().len(), 2);
    let root = l
        .local_locations()
        .unwrap()
        .into_iter()
        .find(|r| r.path == locations[0])
        .unwrap()
        .id;
    let start = std::time::Instant::now();
    let report = run(&mut l, Request::Rescan(root), &mut tags);
    eprintln!(
        "200k-source context, one configured root rescan: {:?}",
        start.elapsed()
    );
    assert_eq!(
        (
            report.scanned,
            report.parsed,
            report.unchanged,
            report.imported
        ),
        (1, 0, 1, 0)
    );
    assert_eq!(tags.reads, 2);
    let detached = folder(temp.path(), "downloads", &["selected.flac"]);
    let start = std::time::Instant::now();
    let report = run(
        &mut l,
        Request::Files(vec![detached.join("selected.flac")]),
        &mut tags,
    );
    eprintln!(
        "200k-source context, explicit standalone file: {:?}",
        start.elapsed()
    );
    assert_eq!((report.scanned, report.parsed, report.imported), (1, 1, 1));
    assert_eq!(l.local_locations().unwrap().len(), 2);
    assert_eq!(
        db.query_row("SELECT count(*) FROM local_source_suppression", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        100000
    );
}

struct ArtistFolders;
impl MetadataExtractor for ArtistFolders {
    fn supports(&self, p: &Path) -> bool {
        p.extension().is_some_and(|e| e == "flac")
    }
    fn read(&mut self, p: &Path) -> Result<ObservedMetadata> {
        let album = p
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let artist = p
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let artist = if album.ends_with('2') {
            artist.to_lowercase()
        } else {
            artist
        };
        Ok(ObservedMetadata {
            track_title: Some("Song".into()),
            release_title: Some(album),
            release_artists: vec![artist.clone()],
            track_artists: vec![artist],
            ..Default::default()
        })
    }
}

#[test]
fn artist_folder_root_matches_parent_root_artist_identities() {
    let temp = tempfile::tempdir().unwrap();
    let music = temp.path().join("Music");
    for artist in ["Hella", "Bygones", "Floral"] {
        for n in 1..=6 {
            folder(&music, &format!("{artist}/Album {n}"), &["song.flac"]);
        }
    }
    let mut automatic = Library::open(temp.path().join("automatic.db")).unwrap();
    let root = automatic.register_local_root(&music).unwrap();
    run(&mut automatic, Request::Rescan(root), &mut ArtistFolders);
    let mut explicit = Library::open(temp.path().join("explicit.db")).unwrap();
    for artist in ["Hella", "Bygones", "Floral"] {
        run(
            &mut explicit,
            Request::Folder(music.join(artist)),
            &mut ArtistFolders,
        );
    }
    for library in [&automatic, &explicit] {
        let artists = library
            .browse(&Browse {
                pane: Pane::Artists,
                limit: 200,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            artists.len(),
            3,
            "one canonical identity per Artist directory"
        );
        for artist in artists {
            assert_eq!(
                library
                    .browse(&Browse {
                        pane: Pane::Albums,
                        artist: Some(ArtistId(artist.id)),
                        limit: 200,
                        ..Default::default()
                    })
                    .unwrap()
                    .len(),
                6
            );
        }
        assert_eq!(songs(library).len(), 18);
    }
    // IDs are opaque and separately generated; compare the actual credit partition,
    // including singleton Album/Release/Track relationships, not QML name grouping.
    fn partition(path: &Path) -> Vec<(String, i64, i64, i64)> {
        Connection::open(path).unwrap().prepare("SELECT lower(a.name),count(DISTINCT ac.album_id),count(DISTINCT rc.release_id),count(DISTINCT tc.track_id) FROM artist a JOIN album_artist_credit ac ON ac.artist_id=a.id JOIN release_artist_credit rc ON rc.artist_id=a.id JOIN track_artist_credit tc ON tc.artist_id=a.id GROUP BY a.id ORDER BY lower(a.name)").unwrap().query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap().map(std::result::Result::unwrap).collect()
    }
    assert_eq!(
        partition(&temp.path().join("automatic.db")),
        partition(&temp.path().join("explicit.db"))
    );
}

#[test]
fn cached_artist_folder_reimport_repairs_prior_split_with_provider_identity_preserved() {
    let temp = tempfile::tempdir().unwrap();
    let music = temp.path().join("Music");
    let hella = music.join("Hella");
    for n in 1..=6 {
        folder(&music, &format!("Hella/Album {n}"), &["song.flac"]);
    }
    let path = temp.path().join("db");
    let mut library = Library::open(&path).unwrap();
    run(
        &mut library,
        Request::Folder(hella.clone()),
        &mut ArtistFolders,
    );
    // Reproduce pre-fix identities at the storage layer, not a grouped UI view.
    let db = Connection::open(&path).unwrap();
    let albums: Vec<String> = db
        .prepare("SELECT id FROM album ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(std::result::Result::unwrap)
        .collect();
    db.execute("DELETE FROM local_artist_context", []).unwrap();
    let mut preserved = None;
    for (index, album) in albums.iter().enumerate() {
        let id = ArtistId(format!("prior-artist-{index}"));
        db.execute(
            "INSERT INTO artist(id,name) VALUES (?1,'Hella')",
            [id.as_ref()],
        )
        .unwrap();
        db.execute(
            "UPDATE album_artist_credit SET artist_id=?1 WHERE album_id=?2",
            rusqlite::params![id.as_ref(), album],
        )
        .unwrap();
        db.execute("UPDATE release_artist_credit SET artist_id=?1 WHERE release_id IN (SELECT id FROM release WHERE album_id=?2)", rusqlite::params![id.as_ref(),album]).unwrap();
        db.execute("UPDATE track_artist_credit SET artist_id=?1 WHERE track_id IN (SELECT t.id FROM track t JOIN release r ON r.id=t.release_id WHERE r.album_id=?2)", rusqlite::params![id.as_ref(),album]).unwrap();
        if index == 0 {
            library
                .attach_artist_external_identity(
                    &id,
                    &ExternalIdentity {
                        provider: "musicbrainz".into(),
                        kind: "artist".into(),
                        external_id: "trusted-hella".into(),
                    },
                )
                .unwrap();
            preserved = Some(id);
        }
    }
    assert_eq!(
        library
            .browse(&Browse {
                pane: Pane::Artists,
                limit: 200,
                ..Default::default()
            })
            .unwrap()
            .len(),
        6
    );
    let before: Vec<_> = songs(&library).into_iter().map(|r| r.id).collect();
    let report = run(&mut library, Request::Folder(hella), &mut ArtistFolders);
    assert_eq!(
        (report.parsed, report.unchanged, report.imported),
        (0, 6, 0)
    );
    let artists = library
        .browse(&Browse {
            pane: Pane::Artists,
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(artists.len(), 1);
    assert_eq!(artists[0].id, preserved.unwrap().as_ref());
    assert_eq!(
        songs(&library)
            .into_iter()
            .map(|r| r.id)
            .collect::<Vec<_>>(),
        before
    );
    // A new Album under the selected Artist root and earlier Albums under a
    // broader root share the same physical Artist-directory context.
    library.register_local_root(&music).unwrap();
    folder(&music, "Hella/Album 7", &["song.flac"]);
    run(
        &mut library,
        Request::ConfiguredLocations,
        &mut ArtistFolders,
    );
    assert_eq!(
        library
            .browse(&Browse {
                pane: Pane::Artists,
                limit: 200,
                ..Default::default()
            })
            .unwrap()
            .len(),
        1
    );
    assert_eq!(songs(&library).len(), 7);
}

#[test]
#[ignore = "requires disposable copied Hella/Bygones/Floral audio directories"]
fn real_artist_folder_import_matches_configured_root() {
    let music = PathBuf::from(
        std::env::var_os("MUSIC_LIBRARY_ARTIST_IMPORT_AUDIT").expect("copied Music directory"),
    );
    let temp = tempfile::tempdir().unwrap();
    let mut scanned = Library::open(temp.path().join("scanned.db")).unwrap();
    let root = scanned.register_local_root(&music).unwrap();
    let automatic = run(
        &mut scanned,
        Request::Rescan(root),
        &mut LoftyMetadataExtractor,
    );
    assert!(automatic.imported > 0);
    let mut selected = Library::open(temp.path().join("selected.db")).unwrap();
    for artist in ["Hella", "Bygones", "Floral"] {
        assert!(
            run(
                &mut selected,
                Request::Folder(music.join(artist)),
                &mut LoftyMetadataExtractor
            )
            .imported
                > 0
        );
    }
    fn state(library: &Library) -> Vec<(String, Vec<String>, usize)> {
        let mut state = vec![];
        for artist in library
            .browse(&Browse {
                pane: Pane::Artists,
                limit: 200,
                ..Default::default()
            })
            .unwrap()
        {
            let id = ArtistId(artist.id);
            let mut albums: Vec<_> = library
                .browse(&Browse {
                    pane: Pane::Albums,
                    artist: Some(id.clone()),
                    limit: 200,
                    ..Default::default()
                })
                .unwrap()
                .into_iter()
                .map(|r| r.title)
                .collect();
            albums.sort();
            let tracks = library
                .library_queue(&Browse {
                    artist: Some(id),
                    ..Default::default()
                })
                .unwrap()
                .len();
            state.push((artist.title.to_lowercase(), albums, tracks));
        }
        state.sort();
        state
    }
    let before = state(&scanned);
    assert_eq!(
        before.len(),
        3,
        "real copied fixture has one identity per Artist"
    );
    assert_eq!(state(&selected), before);
    assert_eq!(songs(&scanned).len(), songs(&selected).len());
    let track = songs(&selected)[0].track.as_ref().unwrap().track_id.clone();
    selected
        .remove_library_object(&Target::Track(track), true)
        .unwrap();
    run(
        &mut selected,
        Request::ConfiguredLocations,
        &mut LoftyMetadataExtractor,
    );
    assert_eq!(songs(&selected).len() + 1, songs(&scanned).len());
    for artist in ["Hella", "Bygones", "Floral"] {
        run(
            &mut selected,
            Request::Folder(music.join(artist)),
            &mut LoftyMetadataExtractor,
        );
    }
    assert_eq!(state(&selected), before);
}

#[test]
fn local_genres_survive_migration_missing_sources_and_membership_removal() {
    use lofty::{
        config::WriteOptions,
        prelude::{Accessor, TagExt},
        tag::{ItemKey, Tag, TagType},
    };
    let temp = tempfile::tempdir().unwrap();
    let music = temp.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let file = music.join("song.flac");
    std::fs::copy("tests/fixtures/provenance/silence.flac", &file).unwrap();
    let mut tag = Tag::new(TagType::VorbisComments);
    tag.set_title("Tagged song".into());
    tag.set_album("Tagged Album".into());
    tag.set_artist("Tagged Artist".into());
    tag.insert_text(ItemKey::AlbumArtist, "Tagged Artist".into());
    tag.insert_text(ItemKey::Genre, "Rock".into());
    tag.save_to_path(&file, WriteOptions::default()).unwrap();
    let path = temp.path().join("db");
    let mut l = Library::open(&path).unwrap();
    let root = l.register_local_root(&music).unwrap();
    l.scan_local_root(&root, &mut LoftyMetadataExtractor)
        .unwrap();
    let candidates = l.list_discovery_candidates(None, 20).unwrap();
    assert_eq!(candidates[0].metadata.genres, ["Rock"]);
    assert!(
        l.browse(&Browse {
            pane: Pane::Genres,
            limit: 200,
            ..Default::default()
        })
        .unwrap()
        .is_empty(),
        "discovery does not save membership"
    );
    run(
        &mut l,
        Request::Folder(music.clone()),
        &mut LoftyMetadataExtractor,
    );
    let track = songs(&l)[0].track.as_ref().unwrap().track_id.clone();
    l.set_track_title_override(&track, "My title").unwrap();
    let source = l
        .available_playback_source(&track)
        .unwrap()
        .unwrap()
        .source_id;
    drop(l);
    let db = Connection::open(&path).unwrap();
    metadata_schema::downgrade(&db);
    db.execute_batch(include_str!("support/drop_song_details.sql"))
        .unwrap();
    db.execute_batch("DROP TABLE file_genre_observation; ALTER TABLE file_metadata_observation DROP COLUMN genres_observed; DROP TABLE IF EXISTS playlist_source; DROP TABLE IF EXISTS playlist_entry; DROP TABLE IF EXISTS playlist; PRAGMA user_version=19;").unwrap();
    drop(db);
    let mut l = Library::open(&path).unwrap();
    let scan = l
        .scan_local_root(&root, &mut LoftyMetadataExtractor)
        .unwrap();
    assert_eq!(scan.parsed, 1, "older observations acquire genres once");
    assert_eq!(
        l.scan_local_root(&root, &mut LoftyMetadataExtractor)
            .unwrap()
            .unchanged,
        1
    );
    let genres = Browse {
        pane: Pane::Genres,
        limit: 200,
        ..Default::default()
    };
    assert_eq!(l.browse(&genres).unwrap()[0].title, "Rock");
    assert_eq!(songs(&l)[0].title, "My title");
    assert_eq!(
        l.available_playback_source(&track)
            .unwrap()
            .unwrap()
            .source_id,
        source
    );
    std::fs::rename(&file, music.join("temporarily-hidden.txt")).unwrap();
    l.scan_local_root(&root, &mut LoftyMetadataExtractor)
        .unwrap();
    assert_eq!(
        l.browse(&genres).unwrap()[0].title,
        "Rock",
        "missing source preserves metadata and membership"
    );
    l.remove_from_library(&track).unwrap();
    std::fs::rename(music.join("temporarily-hidden.txt"), &file).unwrap();
    l.scan_local_root(&root, &mut LoftyMetadataExtractor)
        .unwrap();
    assert!(
        l.browse(&genres).unwrap().is_empty(),
        "rescan must not restore removed membership"
    );
    l.add_to_library(&track).unwrap();
    tag.insert_text(ItemKey::Genre, "Jazz".into());
    tag.save_to_path(&file, WriteOptions::default()).unwrap();
    l.scan_local_root(&root, &mut LoftyMetadataExtractor)
        .unwrap();
    assert_eq!(
        l.browse(&genres).unwrap()[0].title,
        "Jazz",
        "refresh changes only source observation"
    );
    assert_eq!(songs(&l)[0].title, "My title");
}

#[test]
fn genre_migration_failure_rolls_back_marker_and_schema_version() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    drop(Library::open(&path).unwrap());
    let db = Connection::open(&path).unwrap();
    metadata_schema::downgrade(&db);
    db.execute_batch(include_str!("support/drop_song_details.sql"))
        .unwrap();
    db.execute_batch("DROP TABLE file_genre_observation; ALTER TABLE file_metadata_observation DROP COLUMN genres_observed; DROP TABLE IF EXISTS playlist_source; DROP TABLE IF EXISTS playlist_entry; DROP TABLE IF EXISTS playlist; PRAGMA user_version=19; CREATE TABLE file_genre_observation(sentinel TEXT);").unwrap();
    assert!(Library::open(&path).is_err());
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        19
    );
    assert_eq!(db.query_row("SELECT count(*) FROM pragma_table_info('file_metadata_observation') WHERE name='genres_observed'",[],|r|r.get::<_,u32>(0)).unwrap(),0);
    assert_eq!(db.query_row("SELECT count(*) FROM pragma_table_info('file_genre_observation') WHERE name='sentinel'",[],|r|r.get::<_,u32>(0)).unwrap(),1);
}
