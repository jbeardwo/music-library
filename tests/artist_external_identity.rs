use music_library::{
    Library,
    domain::{ArtistId, ExternalIdentity},
};
use rusqlite::Connection;
fn id() -> ExternalIdentity {
    ExternalIdentity {
        provider: "musicbrainz".into(),
        kind: "artist".into(),
        external_id: "shared".into(),
    }
}
#[test]
fn artist_identity_sharing_idempotency_indexes_and_lifecycle() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("db");
    let mut lib = Library::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute_batch(
        "PRAGMA foreign_keys=ON; INSERT INTO artist(id,name) VALUES ('a','Same'),('b','Same');",
    )
    .unwrap();
    let a = ArtistId("a".into());
    let b = ArtistId("b".into());
    assert!(
        lib.resolve_artists_external_identity(&id())
            .unwrap()
            .is_empty()
    );
    assert!(lib.attach_artist_external_identity(&a, &id()).unwrap());
    assert!(!lib.attach_artist_external_identity(&a, &id()).unwrap());
    assert!(lib.attach_artist_external_identity(&b, &id()).unwrap());
    let mut other = id();
    other.external_id = "another".into();
    lib.attach_artist_external_identity(&a, &other).unwrap();
    assert_eq!(lib.list_artist_external_identities(&a).unwrap().len(), 2);
    assert_eq!(
        lib.resolve_artists_external_identity(&id()).unwrap().len(),
        2
    );
    assert!(
        lib.attach_artist_external_identity(&ArtistId("missing".into()), &id())
            .is_err()
    );
    for sql in [
        "SELECT artist_id FROM artist_external_identity WHERE provider='musicbrainz' AND kind='artist' AND external_id='shared'",
        "SELECT provider,kind,external_id FROM artist_external_identity WHERE artist_id='a' ORDER BY provider,kind,external_id",
    ] {
        let plan: String = db
            .query_row(&format!("EXPLAIN QUERY PLAN {sql}"), [], |r| r.get(3))
            .unwrap();
        assert!(plan.contains("SEARCH") && plan.contains("INDEX"), "{plan}");
    }
    db.execute("DELETE FROM artist WHERE id='a'", []).unwrap();
    assert_eq!(
        lib.resolve_artists_external_identity(&id()).unwrap(),
        vec![b]
    );
    assert!(lib.list_artist_external_identities(&a).unwrap().is_empty());
    drop(lib);
    let lib = Library::open(&path).unwrap();
    assert_eq!(
        lib.resolve_artists_external_identity(&id()).unwrap().len(),
        1
    );
}
#[test]
fn v5_upgrade_and_failed_migration_are_atomic() {
    for fail in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db");
        drop(Library::open(&path).unwrap());
        let db = Connection::open(&path).unwrap();
        db.execute_batch("DROP TABLE artist_external_identity; PRAGMA user_version=5; INSERT INTO artist(id,name) VALUES ('legacy','Preserved');").unwrap();
        if fail {
            db.execute_batch("CREATE TABLE artist_external_identity(sentinel TEXT)")
                .unwrap();
        }
        drop(db);
        assert_eq!(Library::open(&path).is_err(), fail);
        let db = Connection::open(&path).unwrap();
        assert_eq!(
            db.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
                .unwrap(),
            if fail { 5 } else { 6 }
        );
        assert_eq!(
            db.query_row("SELECT name FROM artist WHERE id='legacy'", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "Preserved"
        );
    }
}
