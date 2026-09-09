use music_library::{
    Library,
    domain::{CatalogReleaseInput, CatalogTrackInput, ExternalIdentity},
};
use rusqlite::{Connection, params};

fn identity() -> ExternalIdentity {
    ExternalIdentity {
        provider: "musicbrainz".into(),
        kind: "release_group".into(),
        external_id: "shared".into(),
    }
}
fn create(library: &mut Library) -> music_library::domain::ImportedRelease {
    library
        .create_catalog_release(&CatalogReleaseInput {
            title: "Friendly".into(),
            year: Some(2005),
            artists: vec![],
            tracks: vec![CatalogTrackInput {
                title: "Song".into(),
                artists: vec![],
                disc_number: Some(1),
                track_number: Some(1),
            }],
        })
        .unwrap()
}
#[test]
fn album_identity_sharing_indexes_and_lifecycle() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("library.db");
    let mut library = Library::open(&path).unwrap();
    let first = create(&mut library);
    let second = create(&mut library);
    let a = library.album_for_release(&first.release_id).unwrap();
    let b = library.album_for_release(&second.release_id).unwrap();
    assert_ne!(a.album_id, b.album_id);
    assert_eq!(a.title, "Friendly");
    assert_eq!(a.year, Some(2005));
    assert!(
        library
            .resolve_albums_external_identity(&identity())
            .unwrap()
            .is_empty()
    );
    assert!(
        library
            .attach_album_external_identity(&a.album_id, &identity())
            .unwrap()
    );
    assert!(
        !library
            .attach_album_external_identity(&a.album_id, &identity())
            .unwrap()
    );
    assert!(
        library
            .attach_album_external_identity(&b.album_id, &identity())
            .unwrap()
    );
    assert_eq!(
        library
            .resolve_albums_external_identity(&identity())
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        library.list_album_external_identities(&a.album_id).unwrap(),
        vec![identity()]
    );
    drop(library);
    let library = Library::open(&path).unwrap();
    assert_eq!(
        library
            .resolve_albums_external_identity(&identity())
            .unwrap()
            .len(),
        2
    );
    let db = Connection::open(path).unwrap();
    db.execute_batch("PRAGMA foreign_keys=ON").unwrap();
    for sql in [
        "EXPLAIN QUERY PLAN SELECT album_id FROM album_external_identity WHERE provider='musicbrainz' AND kind='release_group' AND external_id='shared'",
        "EXPLAIN QUERY PLAN SELECT provider,kind,external_id FROM album_external_identity WHERE album_id='id' ORDER BY provider,kind,external_id",
    ] {
        let plan: String = db.query_row(sql, [], |r| r.get(3)).unwrap();
        assert!(plan.contains("SEARCH") && plan.contains("INDEX"), "{plan}");
    }
    assert!(
        db.execute("INSERT INTO release(id) VALUES ('invalid')", [])
            .is_err()
    );
    assert!(
        db.execute("DELETE FROM album WHERE id=?1", [a.album_id.as_ref()])
            .is_err()
    );
    db.execute(
        "DELETE FROM release WHERE id=?1",
        [first.release_id.as_ref()],
    )
    .unwrap();
    db.execute("DELETE FROM album WHERE id=?1", [a.album_id.as_ref()])
        .unwrap();
    assert_eq!(
        library
            .resolve_albums_external_identity(&identity())
            .unwrap(),
        vec![b.album_id]
    );
    assert_eq!(
        library
            .list_album_external_identities(&a.album_id)
            .unwrap()
            .len(),
        0
    );
}
#[test]
fn v3_backfill_preserves_release_track_source_membership_metadata_and_moves_group_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("legacy.db");
    let db = Connection::open(&path).unwrap();
    db.execute_batch(include_str!("../migrations/0001_initial.sql"))
        .unwrap();
    db.execute_batch(include_str!("../migrations/0002_external_identities.sql"))
        .unwrap();
    db.execute_batch(include_str!(
        "../migrations/0003_artist_credit_join_phrases.sql"
    ))
    .unwrap();
    db.execute_batch("INSERT INTO release(id,created_at) VALUES ('r1',123),('r2',456);
        INSERT INTO release_application_metadata VALUES ('r1','Same friendly title',2005),('r2','Same friendly title',2005);
        INSERT INTO artist(id,name) VALUES ('artist','Credited Artist');
        INSERT INTO release_artist_credit(release_id,position,artist_id,join_phrase) VALUES ('r1',0,'artist','');
        INSERT INTO track(id,release_id,disc_number,track_number) VALUES ('t','r1',2,3);
        INSERT INTO track_application_metadata VALUES ('t','Original title');
        INSERT INTO track_title_override(track_id,value) VALUES ('t','User title');
        INSERT INTO library_membership(track_id) VALUES ('t');
        INSERT INTO playable_source(id,kind) VALUES ('source','local_file');
        INSERT INTO track_source(track_id,source_id) VALUES ('t','source');
        INSERT INTO discovery_root(id,kind,location) VALUES ('root','local_filesystem',X'2F');
        INSERT INTO local_file_observation(source_id,root_id,path,size_bytes,modified_ns,available) VALUES ('source','root',X'2F61',12,34,0);
        INSERT INTO release_external_identity VALUES ('r1','musicbrainz','release_group','shared'),('r2','musicbrainz','release_group','shared'),('r1','musicbrainz','release','edition');").unwrap();
    drop(db);
    let library = Library::open(&path).unwrap();
    let a = library
        .album_for_release(&music_library::domain::ReleaseId("r1".into()))
        .unwrap();
    let b = library
        .album_for_release(&music_library::domain::ReleaseId("r2".into()))
        .unwrap();
    assert_ne!(a.album_id, b.album_id);
    assert_ne!(a.album_id.as_ref(), "r1");
    assert_eq!(a.title, "Same friendly title");
    assert_eq!(a.year, Some(2005));
    assert_eq!(a.artist_names, "Credited Artist");
    assert_eq!(
        library
            .resolve_albums_external_identity(&identity())
            .unwrap()
            .len(),
        2
    );
    assert!(
        library
            .resolve_releases_external_identity(&identity())
            .unwrap()
            .is_empty()
    );
    let db = Connection::open(path).unwrap();
    assert_eq!(
        db.query_row("SELECT created_at FROM release WHERE id='r1'", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        123
    );
    let preserved:(String,i32,i32,String,i64,i64,i64)=db.query_row("SELECT t.release_id,t.disc_number,t.track_number,o.value,l.size_bytes,l.modified_ns,l.available FROM track t JOIN track_title_override o ON o.track_id=t.id JOIN library_membership m ON m.track_id=t.id JOIN track_source s ON s.track_id=t.id JOIN local_file_observation l ON l.source_id=s.source_id WHERE t.id='t'",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).unwrap();
    assert_eq!(
        preserved,
        ("r1".into(), 2, 3, "User title".into(), 12, 34, 0)
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, u32>(
            0
        ))
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM release_external_identity WHERE release_id=?1 AND kind='release'",
            params!["r1"],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn album_migration_checks_integrity_and_rolls_back_failed_rebuild() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("invalid.db");
    let db = Connection::open(&path).unwrap();
    db.execute_batch(include_str!("../migrations/0001_initial.sql"))
        .unwrap();
    db.execute_batch(include_str!("../migrations/0002_external_identities.sql"))
        .unwrap();
    db.execute_batch(include_str!(
        "../migrations/0003_artist_credit_join_phrases.sql"
    ))
    .unwrap();
    db.execute_batch("PRAGMA foreign_keys=OFF; INSERT INTO release(id) VALUES ('preserved'); INSERT INTO track(id,release_id) VALUES ('invalid','missing');").unwrap();
    drop(db);
    assert!(Library::open(&path).is_err());
    let db = Connection::open(path).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='album'",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT id FROM release", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "preserved"
    );
}
