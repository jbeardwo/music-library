use music_library::{
    Library, Result,
    browse::{Pane, Request},
    domain::*,
    filesystem::MetadataExtractor,
    library_removal::Target,
    library_search::Kind,
};
use rusqlite::Connection;
use std::path::Path;

fn rows(l: &Library, pane: Pane) -> Vec<music_library::browse::Row> {
    l.browse(&Request {
        pane,
        limit: 200,
        ..Default::default()
    })
    .unwrap()
}
fn catalog(l: &mut Library, name: &str, count: usize) -> ImportedRelease {
    let r = l
        .create_catalog_release(&CatalogReleaseInput {
            title: name.into(),
            year: None,
            artists: vec![ArtistCreditInput {
                name: "Same name".into(),
                role: None,
            }],
            tracks: (0..count)
                .map(|i| CatalogTrackInput {
                    title: format!("Song {i}"),
                    disc_number: Some(1),
                    track_number: Some(i as u32 + 1),
                    artists: vec![],
                })
                .collect(),
        })
        .unwrap();
    for t in &r.track_ids {
        l.add_to_library(t).unwrap();
    }
    r
}
struct Tags;
impl MetadataExtractor for Tags {
    fn supports(&self, p: &Path) -> bool {
        p.extension().is_some_and(|e| e == "flac")
    }
    fn read(&mut self, p: &Path) -> Result<ObservedMetadata> {
        Ok(ObservedMetadata {
            track_title: Some(p.file_stem().unwrap().to_string_lossy().into()),
            release_title: Some("Same title".into()),
            ..Default::default()
        })
    }
}
#[test]
fn local_suppression_restart_siblings_scan_and_explicit_readd() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("audio");
    std::fs::create_dir(&folder).unwrap();
    for name in ["first", "second"] {
        std::fs::write(folder.join(format!("{name}.flac")), b"disposable").unwrap();
    }
    let path = temp.path().join("library.sqlite");
    let mut l = Library::open(&path).unwrap();
    let root = l.register_local_root(&folder).unwrap();
    l.scan_local_root(&root, &mut Tags).unwrap();
    let sources = l.list_discovery_candidates(None, 20).unwrap();
    let r = l
        .import_release(&ImportReleaseRequest {
            release_title: "Same title".into(),
            release_artists: vec![],
            tracks: sources
                .iter()
                .map(|s| ImportTrackInput {
                    source_id: s.source_id.clone(),
                    title_fallback: None,
                    artists: vec![],
                    disc_number: None,
                    track_number: None,
                })
                .collect(),
        })
        .unwrap();
    let first = Target::Track(r.track_ids[0].clone());
    assert!(l.removal_preview(&first).unwrap().has_local_sources);
    let snapshot = l
        .library_queue(&Request {
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER reject_local_removal BEFORE DELETE ON library_membership BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    assert!(l.remove_library_object(&first, true).is_err());
    assert_eq!(
        db.query_row("SELECT count(*) FROM local_source_suppression", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
    assert_eq!(rows(&l, Pane::Songs).len(), 2);
    db.execute_batch("DROP TRIGGER reject_local_removal;")
        .unwrap();
    drop(db);
    assert_eq!(l.remove_library_object(&first, true).unwrap(), 1);
    assert_eq!(snapshot.len(), 2);
    assert_eq!(rows(&l, Pane::Songs).len(), 1);
    assert!(
        l.available_playback_source(&r.track_ids[0])
            .unwrap()
            .is_some()
    );
    drop(l);
    let mut l = Library::open(&path).unwrap();
    assert_eq!(rows(&l, Pane::Songs).len(), 1);
    assert_eq!(l.scan_local_root(&root, &mut Tags).unwrap().parsed, 0);
    assert_eq!(l.restore_unsuppressed_local_memberships(&root).unwrap(), 0);
    assert_eq!(rows(&l, Pane::Songs).len(), 1);
    assert_eq!(
        l.readd_local_source(&sources[0].source_id).unwrap(),
        r.track_ids[0]
    );
    assert_eq!(l.remove_library_object(&first, false).unwrap(), 1);
    l.scan_local_root(&root, &mut Tags).unwrap();
    assert_eq!(l.restore_unsuppressed_local_memberships(&root).unwrap(), 1);
    l.remove_library_object(&first, true).unwrap();
    assert!(l.add_to_library(&r.track_ids[0]).unwrap());
    l.remove_library_object(&first, false).unwrap();
    assert_eq!(l.restore_unsuppressed_local_memberships(&root).unwrap(), 1);
    l.remove_library_object(&first, true).unwrap();
    assert!(l.clear_local_suppression(&sources[0].source_id).unwrap());
    assert_eq!(l.restore_unsuppressed_local_memberships(&root).unwrap(), 1);
    for s in &sources {
        assert_eq!(std::fs::read(&s.path).unwrap(), b"disposable");
    }
    let album = Target::Album(AlbumId(rows(&l, Pane::Albums)[0].id.clone()));
    l.remove_library_object(&first, true).unwrap();
    assert_eq!(l.removal_preview(&album).unwrap().saved_tracks, 1);
    assert_eq!(l.remove_library_object(&album, true).unwrap(), 1);
    assert!(rows(&l, Pane::Albums).is_empty());
    assert_eq!(l.restore_unsuppressed_local_memberships(&root).unwrap(), 0);
    let db = Connection::open(&path).unwrap();
    let plan: String = db
        .query_row(
            "EXPLAIN QUERY PLAN SELECT 1 FROM local_source_suppression WHERE source_id=?1",
            [sources[0].source_id.as_ref()],
            |r| r.get(3),
        )
        .unwrap();
    assert!(plan.contains("INDEX"), "{plan}");
    let mut statement = db.prepare("EXPLAIN QUERY PLAN SELECT ts.track_id FROM local_root_source s CROSS JOIN local_file_observation l ON l.source_id=s.source_id JOIN track_source ts ON ts.source_id=l.source_id WHERE s.root_id=?1 AND l.available=1 AND NOT EXISTS (SELECT 1 FROM local_source_suppression x WHERE x.source_id=l.source_id)").unwrap();
    let plan = statement
        .query_map([root.as_ref()], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
        .join("\n");
    assert!(
        plan.contains("SEARCH s") && plan.contains("INDEX") && !plan.contains("SCAN x"),
        "{plan}"
    );
}
#[test]
fn sourceless_partial_full_album_provider_identity_and_search() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.sqlite");
    let mut l = Library::open(&path).unwrap();
    let a = catalog(&mut l, "Album", 3);
    let identity = ExternalIdentity {
        provider: "test".into(),
        kind: "track".into(),
        external_id: "durable".into(),
    };
    l.attach_track_external_identity(&a.track_ids[0], &identity)
        .unwrap();
    let t = Target::Track(a.track_ids[0].clone());
    assert!(!l.removal_preview(&t).unwrap().has_local_sources);
    assert_eq!(l.remove_library_object(&t, true).unwrap(), 1);
    assert!(l.local_search("Song 0", Kind::Song).unwrap().is_empty());
    assert_eq!(
        l.list_track_external_identities(&a.track_ids[0]).unwrap(),
        vec![identity]
    );
    let album = Target::Album(AlbumId(rows(&l, Pane::Albums)[0].id.clone()));
    assert_eq!(l.removal_preview(&album).unwrap().saved_tracks, 2);
    assert_eq!(l.remove_library_object(&album, false).unwrap(), 2);
    assert!(rows(&l, Pane::Albums).is_empty());
    assert!(rows(&l, Pane::Artists).is_empty());
    let b = catalog(&mut l, "Other album", 2);
    let album = Target::Album(AlbumId(rows(&l, Pane::Albums)[0].id.clone()));
    assert_eq!(
        l.remove_library_object(&album, false).unwrap(),
        b.track_ids.len() as u64
    );
    drop(l);
    let l = Library::open(path).unwrap();
    assert!(rows(&l, Pane::Songs).is_empty());
}
#[test]
fn canonical_artist_multi_album_shared_credits_and_same_name_are_safe() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.sqlite");
    let mut l = Library::open(&path).unwrap();
    let a = catalog(&mut l, "A", 2);
    let b = catalog(&mut l, "B", 2);
    let c = catalog(&mut l, "C", 1);
    let db = Connection::open(&path).unwrap();
    let artist: String = db
        .query_row(
            "SELECT artist_id FROM release_artist_credit WHERE release_id=?1",
            [a.release_id.as_ref()],
            |r| r.get(0),
        )
        .unwrap();
    // Canonical shared credit, deliberately independent of the duplicate name in C.
    db.execute(
        "INSERT INTO track_artist_credit(track_id,position,artist_id) VALUES (?1,0,?2)",
        rusqlite::params![b.track_ids[0].as_ref(), artist],
    )
    .unwrap();
    let target = Target::Artist(ArtistId(artist));
    assert_eq!(l.removal_preview(&target).unwrap().saved_tracks, 3);
    assert_eq!(l.remove_library_object(&target, true).unwrap(), 3);
    let remaining = rows(&l, Pane::Songs);
    assert_eq!(remaining.len(), 2);
    assert!(remaining.iter().any(|r| r.id == c.track_ids[0].as_ref()));
    assert!(!rows(&l, Pane::Artists).iter().any(|r| r.id
        == match &target {
            Target::Artist(a) => a.as_ref(),
            _ => unreachable!(),
        }));
}
#[test]
fn large_removal_rolls_back_membership_and_suppression_together() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.sqlite");
    let mut l = Library::open(&path).unwrap();
    let a = catalog(&mut l, "Large", 1500);
    let target = Target::Album(AlbumId(rows(&l, Pane::Albums)[0].id.clone()));
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER reject_removal BEFORE DELETE ON library_membership BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    assert!(l.remove_library_object(&target, true).is_err());
    assert_eq!(l.removal_preview(&target).unwrap().saved_tracks, 1500);
    db.execute_batch("DROP TRIGGER reject_removal;").unwrap();
    assert_eq!(
        l.removal_worker().unwrap().remove(&target, true).unwrap(),
        a.track_ids.len() as u64
    );
    assert_eq!(l.removal_preview(&target).unwrap().saved_tracks, 0);
}

#[test]
#[ignore = "opt-in deterministic 200k Track removal measurement"]
fn removal_200k_tracks() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.sqlite");
    let mut l = Library::open(&path).unwrap();
    let release = catalog(&mut l, "Stress", 1);
    let db = Connection::open(&path).unwrap();
    let recording: String = db
        .query_row(
            "SELECT recording_id FROM track WHERE id=?1",
            [release.track_ids[0].as_ref()],
            |r| r.get(0),
        )
        .unwrap();
    // Deterministic fixture-only IDs, independent of filenames and metadata.
    db.execute(
        "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<199999)
        INSERT INTO track(id,release_id,recording_id) SELECT printf('stress-%08d',i),?1,?2 FROM n",
        rusqlite::params![release.release_id.as_ref(), recording],
    )
    .unwrap();
    db.execute(
        "INSERT OR IGNORE INTO library_membership(track_id) SELECT id FROM track",
        [],
    )
    .unwrap();
    let target = Target::Album(AlbumId(rows(&l, Pane::Albums)[0].id.clone()));
    let start = std::time::Instant::now();
    assert_eq!(l.removal_preview(&target).unwrap().saved_tracks, 200000);
    eprintln!("200k preview: {:?}", start.elapsed());
    let start = std::time::Instant::now();
    assert_eq!(l.remove_library_object(&target, true).unwrap(), 200000);
    eprintln!("200k removal: {:?}", start.elapsed());
    assert_eq!(l.removal_preview(&target).unwrap().saved_tracks, 0);
}

#[test]
fn explicit_import_can_override_excluded_unassociated_source() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("audio");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("song.flac"), b"disposable").unwrap();
    let path = temp.path().join("library.sqlite");
    let mut l = Library::open(&path).unwrap();
    let root = l.register_local_root(&folder).unwrap();
    l.scan_local_root(&root, &mut Tags).unwrap();
    let source = l.list_discovery_candidates(None, 1).unwrap()[0]
        .source_id
        .clone();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "INSERT INTO local_source_suppression(source_id) VALUES (?1)",
        [source.as_ref()],
    )
    .unwrap();
    assert!(l.list_discovery_candidates(None, 1).unwrap().is_empty());
    let imported = l
        .import_release(&ImportReleaseRequest {
            release_title: "Explicitly chosen".into(),
            release_artists: vec![],
            tracks: vec![ImportTrackInput {
                source_id: source,
                title_fallback: None,
                artists: vec![],
                disc_number: None,
                track_number: None,
            }],
        })
        .unwrap();
    assert_eq!(rows(&l, Pane::Songs)[0].id, imported.track_ids[0].as_ref());
    assert_eq!(
        db.query_row("SELECT count(*) FROM local_source_suppression", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
}
