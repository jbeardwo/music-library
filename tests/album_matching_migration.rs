use music_library::Library;
use rusqlite::Connection;

fn v4(path: &std::path::Path) -> Connection {
    let db = Connection::open(path).unwrap();
    for migration in [
        include_str!("../migrations/0001_initial.sql"),
        include_str!("../migrations/0002_external_identities.sql"),
        include_str!("../migrations/0003_artist_credit_join_phrases.sql"),
        include_str!("../migrations/0004_albums.sql"),
    ] {
        db.execute_batch(migration).unwrap();
    }
    db
}

#[test]
fn matching_upgrade_backfills_unicode_credit_display_without_changing_entities() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("v4.db");
    let db = v4(&path);
    db.execute_batch("INSERT INTO album(id) VALUES ('a');
        INSERT INTO album_application_metadata VALUES ('a','  ÉCHO   (Live) ',2005);
        INSERT INTO artist(id,name) VALUES ('x','Ärtist A'),('y','Artist B');
        INSERT INTO album_artist_credit(album_id,position,artist_id,join_phrase) VALUES ('a',0,'x',' feat. '),('a',1,'y','');
        INSERT INTO album_external_identity VALUES ('a','provider','album','external');
        INSERT INTO release(id,album_id) VALUES ('r','a');
        INSERT INTO track(id,release_id) VALUES ('t','r');
        INSERT INTO library_membership(track_id) VALUES ('t');").unwrap();
    drop(db);
    drop(Library::open(&path).unwrap());
    let db = Connection::open(&path).unwrap();
    let values: (String,String,String,i32) = db.query_row("SELECT title,match_title,match_artist_credit,year FROM album_application_metadata WHERE album_id='a'", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
    assert_eq!(
        values,
        (
            "  ÉCHO   (Live) ".into(),
            "écho (live)".into(),
            "ärtist a feat. artist b".into(),
            2005
        )
    );
    assert_eq!(db.query_row("SELECT t.id FROM track t JOIN release r ON r.id=t.release_id JOIN album_external_identity i ON i.album_id=r.album_id JOIN library_membership m ON m.track_id=t.id", [], |r|r.get::<_,String>(0)).unwrap(),"t");
    assert_eq!(
        db.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, u32>(
            0
        ))
        .unwrap(),
        0
    );
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        6
    );
    let plan: String = db.query_row("EXPLAIN QUERY PLAN SELECT album_id FROM album_application_metadata WHERE match_title=?1 AND match_artist_credit=?2 LIMIT 65", ["écho (live)","ärtist a feat. artist b"], |r|r.get(3)).unwrap();
    assert!(
        plan.contains("SEARCH") && plan.contains("COVERING INDEX album_matching_lookup"),
        "{plan}"
    );
    drop(db);
    // Opening an already migrated library must not run the backfill again.
    drop(Library::open(&path).unwrap());
}

#[test]
fn failed_matching_migration_rolls_back_columns_and_version() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("v4.db");
    let db = v4(&path);
    db.execute_batch("CREATE INDEX album_matching_lookup ON album(created_at)")
        .unwrap();
    drop(db);
    assert!(Library::open(&path).is_err());
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        4
    );
    assert_eq!(db.query_row("SELECT count(*) FROM pragma_table_info('album_application_metadata') WHERE name LIKE 'match_%'",[],|r|r.get::<_,u32>(0)).unwrap(),0);
}

#[test]
fn candidate_track_verification_uses_album_release_and_track_indexes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.db");
    drop(Library::open(&path).unwrap());
    let db = Connection::open(path).unwrap();
    let plan = db.prepare("EXPLAIN QUERY PLAN SELECT r.id,t.disc_number,t.track_number,e.title FROM release r JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=?1").unwrap().query_map(["a"],|r|r.get::<_,String>(3)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap().join("\n");
    assert!(
        plan.contains("release_album")
            && plan.contains("track_release_order")
            && plan.contains("sqlite_autoindex_effective_track_metadata_1"),
        "{plan}"
    );
    assert!(!plan.contains("SCAN"), "{plan}");
}

#[test]
fn external_matching_eligibility_uses_only_entity_scoped_indexed_reads() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    drop(Library::open(&path).unwrap());
    let db = Connection::open(path).unwrap();
    for sql in [
        "SELECT match_title,match_artist_credit FROM album_application_metadata WHERE album_id=?1",
        "SELECT 1 FROM album_external_identity WHERE album_id=?1 AND provider='musicbrainz' AND kind='release_group'",
        "SELECT m.track_title FROM release r JOIN track t ON t.release_id=r.id JOIN track_source ts ON ts.track_id=t.id JOIN file_metadata_observation m ON m.source_id=ts.source_id WHERE r.album_id=?1",
    ] {
        let plan = db
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap()
            .query_map(["album"], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
            .join("\n");
        assert!(!plan.contains("SCAN"), "{plan}");
        assert!(plan.contains("SEARCH"), "{plan}");
    }
}
