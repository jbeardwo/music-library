use music_library::{
    Library,
    domain::{ArtistId, ExternalIdentity},
    storage::Error,
};
use rusqlite::{Connection, params};
fn mb(value: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: "musicbrainz".into(),
        kind: "artist".into(),
        external_id: value.into(),
    }
}
fn setup() -> (tempfile::TempDir, Library, Connection) {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("library");
    let lib = Library::open(&path).unwrap();
    let db = Connection::open(path).unwrap();
    db.execute_batch("PRAGMA foreign_keys=ON;
      INSERT INTO artist(id,name) VALUES ('source','Original name'),('canonical','Different canonical name'),('other','Other');
      INSERT INTO album(id) VALUES ('album'); INSERT INTO release(id,album_id) VALUES ('release','album');
      INSERT INTO track(id,release_id) VALUES ('track','release'); INSERT INTO library_membership(track_id) VALUES ('track');").unwrap();
    for (table, column, entity) in [
        ("album_artist_credit", "album_id", "album"),
        ("release_artist_credit", "release_id", "release"),
        ("track_artist_credit", "track_id", "track"),
    ] {
        db.execute(&format!("INSERT INTO {table}({column},position,artist_id,role,join_phrase,credited_name) VALUES (?1,0,'source','main',' feat. ','Printed alias'),(?1,1,'other','guest',' & ',NULL),(?1,2,'source','main','',NULL)"),[entity]).unwrap();
    }
    (tmp, lib, db)
}
type CreditSnapshot = (
    String,
    i64,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);
fn snapshot(db: &Connection) -> Vec<CreditSnapshot> {
    let mut all = vec![];
    for table in [
        "album_artist_credit",
        "release_artist_credit",
        "track_artist_credit",
    ] {
        all.extend(db.prepare(&format!("SELECT '{table}',position,artist_id,role,join_phrase,credited_name FROM {table} ORDER BY position")).unwrap().query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap());
    }
    all
}
#[test]
fn atomic_reassignment_preserves_every_credit_position_and_presentation() {
    let (_tmp, mut lib, db) = setup();
    let source = ArtistId("source".into());
    let canonical = ArtistId("canonical".into());
    for artist in [&source, &canonical] {
        lib.attach_artist_external_identity(artist, &mb("shared"))
            .unwrap();
    }
    let opaque = ExternalIdentity {
        provider: "other".into(),
        kind: "arbitrary".into(),
        external_id: "kept".into(),
    };
    lib.attach_artist_external_identity(&source, &opaque)
        .unwrap();
    assert!(lib.merge_artist(&source, &canonical).unwrap());
    for row in snapshot(&db) {
        assert_eq!(row.2, if row.1 == 1 { "other" } else { "canonical" });
        assert_eq!(
            row.4.as_deref(),
            Some(match row.1 {
                0 => " feat. ",
                1 => " & ",
                _ => "",
            })
        );
        if row.1 == 0 {
            assert_eq!(row.5.as_deref(), Some("Printed alias"));
            assert_eq!(row.3.as_deref(), Some("main"));
        }
        if row.1 == 2 {
            assert_eq!(row.5.as_deref(), Some("Original name"));
        }
    }
    assert_eq!(
        lib.list_artist_external_identities(&canonical)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        lib.resolve_artists_external_identity(&mb("shared"))
            .unwrap(),
        vec![canonical.clone()]
    );
    assert!(!lib.merge_artist(&source, &canonical).unwrap());
    assert!(!lib.merge_artist(&canonical, &canonical).unwrap());
    for table in ["album", "release", "track", "library_membership"] {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    assert_eq!(
        db.query_row("SELECT count(*) FROM artist WHERE id='source'", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
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
fn conflict_and_mid_reassignment_failure_roll_back_every_reference_and_identity() {
    for conflict in [true, false] {
        let (_tmp, mut lib, db) = setup();
        let source = ArtistId("source".into());
        let canonical = ArtistId("canonical".into());
        lib.attach_artist_external_identity(&source, &mb("one"))
            .unwrap();
        lib.attach_artist_external_identity(&canonical, &mb(if conflict { "two" } else { "one" }))
            .unwrap();
        if !conflict {
            db.execute_batch("CREATE TRIGGER reject_track BEFORE UPDATE ON track_artist_credit BEGIN SELECT RAISE(ABORT,'induced failure'); END;").unwrap();
        }
        let before = snapshot(&db);
        let error = lib.merge_artist(&source, &canonical).unwrap_err();
        if conflict {
            assert!(matches!(error, Error::ArtistIdentityConflict));
        }
        assert_eq!(snapshot(&db), before);
        assert_eq!(
            lib.list_artist_external_identities(&source).unwrap(),
            vec![mb("one")]
        );
        assert_eq!(
            lib.list_artist_external_identities(&canonical).unwrap(),
            vec![mb(if conflict { "two" } else { "one" })]
        );
    }
}
#[test]
fn credit_reassignment_and_external_lookup_are_indexed() {
    let (_tmp, _lib, db) = setup();
    for table in [
        "album_artist_credit",
        "release_artist_credit",
        "track_artist_credit",
    ] {
        let plan=db.prepare(&format!("EXPLAIN QUERY PLAN UPDATE {table} SET artist_id='canonical' WHERE artist_id='source'")).unwrap().query_map([],|r|r.get::<_,String>(3)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap().join("\n");
        assert!(
            plan.contains(&format!("SEARCH {table}"))
                && plan.contains(&format!("INDEX {table}_artist")),
            "{plan}"
        );
    }
    let plan:String=db.query_row("EXPLAIN QUERY PLAN SELECT artist_id FROM artist_external_identity WHERE provider=?1 AND kind=?2 AND external_id=?3 ORDER BY artist_id",params!["musicbrainz","artist","shared"],|r|r.get(3)).unwrap();
    assert!(plan.contains("artist_external_identity_lookup"), "{plan}");
}
#[test]
fn v6_backfills_presentation_and_failed_v7_upgrade_is_atomic() {
    for fail in [false, true] {
        let (tmp, lib, db) = setup();
        drop(lib);
        for table in [
            "album_artist_credit",
            "release_artist_credit",
            "track_artist_credit",
        ] {
            db.execute_batch(&format!("ALTER TABLE {table} DROP COLUMN credited_name"))
                .unwrap();
        }
        db.execute_batch("PRAGMA user_version=6").unwrap();
        if fail {
            db.execute_batch("CREATE TRIGGER fail_backfill BEFORE UPDATE ON track_artist_credit BEGIN SELECT RAISE(ABORT,'induced'); END").unwrap();
        }
        drop(db);
        assert_eq!(Library::open(tmp.path().join("library")).is_err(), fail);
        let db = Connection::open(tmp.path().join("library")).unwrap();
        assert_eq!(
            db.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
                .unwrap(),
            if fail { 6 } else { 7 }
        );
        for table in [
            "album_artist_credit",
            "release_artist_credit",
            "track_artist_credit",
        ] {
            if fail {
                assert_eq!(db.query_row(&format!("SELECT count(*) FROM pragma_table_info('{table}') WHERE name='credited_name'"),[],|r|r.get::<_,i64>(0)).unwrap(),0);
            } else {
                assert_eq!(
                    db.query_row(
                        &format!("SELECT credited_name FROM {table} WHERE position=0"),
                        [],
                        |r| r.get::<_, String>(0)
                    )
                    .unwrap(),
                    "Original name"
                );
            }
        }
    }
}
