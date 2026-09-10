use music_library::{
    Library,
    domain::{CatalogReleaseInput, CatalogTrackInput, ExternalIdentity, ReleaseId, TrackId},
    storage::Error,
};
use rusqlite::{Connection, params};
use tempfile::TempDir;

fn identity(provider: &str, kind: &str, external_id: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: provider.into(),
        kind: kind.into(),
        external_id: external_id.into(),
    }
}

fn catalog(library: &mut Library) -> music_library::domain::ImportedRelease {
    library
        .create_catalog_release(&CatalogReleaseInput {
            title: "Edition".into(),
            year: None,
            artists: vec![],
            tracks: vec![CatalogTrackInput {
                title: "Track".into(),
                artists: vec![],
                disc_number: None,
                track_number: None,
            }],
        })
        .unwrap()
}

#[test]
fn attach_list_resolve_idempotency_sharing_and_opaque_strings() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("library.sqlite");
    let mut library = Library::open(&path).unwrap();
    let first = catalog(&mut library);
    let second = catalog(&mut library);
    let track = &first.track_ids[0];
    let ids = [
        identity("musicbrainz", "track", "track-id"),
        identity("musicbrainz", "recording", "recording-id"),
        identity("isrc", "recording", "ISRC-one"),
        identity("isrc", "recording", "ISRC-two"),
        identity("future provider", "custom kind", " 'Case/雪? "),
        identity("future provider", "custom kind", " 'case/雪? "),
        identity("another provider", "custom kind", " 'Case/雪? "),
        identity("future provider", "another kind", " 'Case/雪? "),
    ];
    for id in &ids {
        assert!(library.attach_track_external_identity(track, id).unwrap());
        assert!(!library.attach_track_external_identity(track, id).unwrap());
        assert_eq!(
            library.resolve_tracks_external_identity(id).unwrap(),
            vec![track.clone()]
        );
    }
    let mut expected = ids.to_vec();
    expected.sort_by(|a, b| {
        (&a.provider, &a.kind, &a.external_id).cmp(&(&b.provider, &b.kind, &b.external_id))
    });
    assert_eq!(
        library.list_track_external_identities(track).unwrap(),
        expected
    );
    // Sharing is valid for every opaque kind, not just provider-specific exceptions.
    for id in &ids {
        assert!(
            library
                .attach_track_external_identity(&second.track_ids[0], id)
                .unwrap()
        );
        assert!(
            !library
                .attach_track_external_identity(&second.track_ids[0], id)
                .unwrap()
        );
        let matches = library.resolve_tracks_external_identity(id).unwrap();
        assert_eq!(matches.len(), 2);
        assert!(matches.contains(track) && matches.contains(&second.track_ids[0]));
    }

    // The two tables have independent identity namespaces. Multiple same-kind IDs
    // are permitted on Releases too; strings are not constrained to these examples.
    let release_ids = [
        ids[0].clone(),
        identity("musicbrainz", "release", "release-a"),
        identity("musicbrainz", "release", "release-b"),
        identity("musicbrainz", "release_group", "group"),
    ];
    for id in &release_ids {
        assert!(
            library
                .attach_release_external_identity(&first.release_id, id)
                .unwrap()
        );
        assert!(
            !library
                .attach_release_external_identity(&first.release_id, id)
                .unwrap()
        );
        assert_eq!(
            library.resolve_releases_external_identity(id).unwrap(),
            vec![first.release_id.clone()]
        );
    }
    assert_eq!(
        library
            .list_release_external_identities(&first.release_id)
            .unwrap()
            .len(),
        release_ids.len()
    );
    for id in &release_ids {
        assert!(
            library
                .attach_release_external_identity(&second.release_id, id)
                .unwrap()
        );
        assert!(
            !library
                .attach_release_external_identity(&second.release_id, id)
                .unwrap()
        );
        let matches = library.resolve_releases_external_identity(id).unwrap();
        assert_eq!(matches.len(), 2);
        assert!(matches.contains(&first.release_id) && matches.contains(&second.release_id));
    }
    let missing = identity("absent", "kind", "id");
    assert!(
        library
            .resolve_tracks_external_identity(&missing)
            .unwrap()
            .is_empty()
    );
    assert!(
        library
            .resolve_releases_external_identity(&missing)
            .unwrap()
            .is_empty()
    );
    drop(library);
    let library = Library::open(&path).unwrap();
    assert_eq!(
        library.list_track_external_identities(track).unwrap(),
        expected
    );
    assert_eq!(
        library
            .list_release_external_identities(&first.release_id)
            .unwrap()
            .len(),
        release_ids.len()
    );
    // Attaching identity did not create membership or sources, or change metadata.
    let db = Connection::open(path).unwrap();
    for table in ["library_membership", "playable_source", "track_source"] {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    assert_eq!(
        db.query_row(
            "SELECT title FROM track_application_metadata WHERE track_id = ?1",
            [track.as_ref()],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "Track"
    );
}

#[test]
fn schema_enforces_foreign_keys_uniqueness_and_entity_cascades() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("library.sqlite");
    let mut library = Library::open(&path).unwrap();
    let first = catalog(&mut library);
    let other = catalog(&mut library);
    let id = identity("provider", "kind", "identifier");
    let missing = identity("provider", "kind", "missing");
    for result in [
        library.attach_track_external_identity(&TrackId("missing".into()), &missing),
        library.attach_release_external_identity(&ReleaseId("missing".into()), &missing),
    ] {
        assert!(
            matches!(result, Err(Error::Database(rusqlite::Error::SqliteFailure(error, _)))
            if error.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY)
        );
    }
    library
        .attach_track_external_identity(&first.track_ids[0], &id)
        .unwrap();
    library
        .attach_release_external_identity(&first.release_id, &id)
        .unwrap();
    let db = Connection::open(path).unwrap();
    db.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    for (table, column, owner, other_owner) in [
        (
            "track_external_identity",
            "track_id",
            first.track_ids[0].as_ref(),
            other.track_ids[0].as_ref(),
        ),
        (
            "release_external_identity",
            "release_id",
            first.release_id.as_ref(),
            other.release_id.as_ref(),
        ),
    ] {
        let sql = format!(
            "INSERT INTO {table}({column}, provider, kind, external_id) VALUES (?1, 'provider', 'kind', 'identifier')"
        );
        assert!(db.execute(&sql, [owner]).is_err());
        assert_eq!(db.execute(&sql, [other_owner]).unwrap(), 1);
    }
    library.add_to_library(&first.track_ids[0]).unwrap();
    library.remove_from_library(&first.track_ids[0]).unwrap();
    assert_eq!(
        library.resolve_tracks_external_identity(&id).unwrap().len(),
        2
    );
    // Deleting one Track preserves the other Track's shared association and both Releases.
    db.execute(
        "DELETE FROM track WHERE id = ?1",
        [first.track_ids[0].as_ref()],
    )
    .unwrap();
    assert_eq!(
        library.resolve_tracks_external_identity(&id).unwrap(),
        vec![other.track_ids[0].clone()]
    );
    assert_eq!(
        library
            .resolve_releases_external_identity(&id)
            .unwrap()
            .len(),
        2
    );
    // Release deletion cascades to its Track, but leaves the other Release's identity.
    db.execute(
        "DELETE FROM release WHERE id = ?1",
        [other.release_id.as_ref()],
    )
    .unwrap();
    assert!(
        library
            .resolve_tracks_external_identity(&id)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        library.resolve_releases_external_identity(&id).unwrap(),
        vec![first.release_id.clone()]
    );

    assert!(
        !db.prepare("PRAGMA foreign_key_check")
            .unwrap()
            .exists([])
            .unwrap()
    );
}

#[test]
fn migration_upgrades_v1_preserves_entities_and_is_atomic_on_failure() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("v1.sqlite");
    let db = Connection::open(&path).unwrap();
    db.execute_batch(include_str!("../migrations/0001_initial.sql"))
        .unwrap();
    db.execute_batch("PRAGMA user_version = 1; INSERT INTO release(id) VALUES ('legacy-release'); INSERT INTO track(id, release_id) VALUES ('legacy-track', 'legacy-release');").unwrap();
    let mut library = Library::open(&path).unwrap();
    let id = identity("provider", "kind", "id");
    library
        .attach_track_external_identity(&TrackId("legacy-track".into()), &id)
        .unwrap();
    library
        .attach_release_external_identity(&ReleaseId("legacy-release".into()), &id)
        .unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        7
    );
    let additional = catalog(&mut library);
    library
        .attach_track_external_identity(&additional.track_ids[0], &id)
        .unwrap();
    library
        .attach_release_external_identity(&additional.release_id, &id)
        .unwrap();
    assert_eq!(
        library.resolve_tracks_external_identity(&id).unwrap().len(),
        2
    );
    assert_eq!(
        library
            .resolve_releases_external_identity(&id)
            .unwrap()
            .len(),
        2
    );
    drop(library);
    assert!(Library::open(&path).is_ok());

    let broken = temp.path().join("broken.sqlite");
    let db = Connection::open(&broken).unwrap();
    db.execute_batch(include_str!("../migrations/0001_initial.sql"))
        .unwrap();
    db.execute_batch(
        "PRAGMA user_version = 1; CREATE TABLE release_external_identity(sentinel TEXT);",
    )
    .unwrap();
    assert!(Library::open(&broken).is_err());
    assert_eq!(
        db.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name = 'track_external_identity'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    db.execute_batch("PRAGMA user_version = 8;").unwrap();
    assert!(matches!(Library::open(&broken), Err(Error::Invalid(_))));
}

#[test]
fn external_resolution_and_entity_listing_use_indexes() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("plans.sqlite");
    let mut library = Library::open(&path).unwrap();
    let release = catalog(&mut library);
    let id = identity("provider", "kind", "id");
    library
        .attach_track_external_identity(&release.track_ids[0], &id)
        .unwrap();
    library
        .attach_release_external_identity(&release.release_id, &id)
        .unwrap();
    let db = Connection::open(path).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        7
    );
    for (table, column, owner) in [
        (
            "track_external_identity",
            "track_id",
            release.track_ids[0].as_ref(),
        ),
        (
            "release_external_identity",
            "release_id",
            release.release_id.as_ref(),
        ),
    ] {
        let unique: bool = db
            .query_row(
                r#"SELECT "unique" FROM pragma_index_list(?1) WHERE name = ?2"#,
                params![table, format!("{table}_lookup")],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!unique);
        let resolution: String = db.query_row(
            &format!("EXPLAIN QUERY PLAN SELECT {column} FROM {table} WHERE provider = ?1 AND kind = ?2 AND external_id = ?3"),
            params![id.provider, id.kind, id.external_id], |row| row.get(3),
        ).unwrap();
        assert!(
            resolution.contains(&format!("{table}_lookup")),
            "{resolution}"
        );
        assert!(
            resolution.contains("SEARCH") && resolution.contains("USING INDEX"),
            "{resolution}"
        );
        assert!(
            resolution.contains("provider=? AND kind=? AND external_id=?"),
            "{resolution}"
        );
        let listing: String = db.query_row(
            &format!("EXPLAIN QUERY PLAN SELECT provider, kind, external_id FROM {table} WHERE {column} = ?1 ORDER BY provider, kind, external_id"),
            [owner], |row| row.get(3),
        ).unwrap();
        assert!(
            listing.contains("SEARCH") && listing.contains("COVERING INDEX"),
            "{listing}"
        );
        println!("{resolution}\n{listing}");
    }
}
