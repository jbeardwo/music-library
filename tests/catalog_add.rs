use music_library::{
    Library,
    catalog::{Credit, Medium, Release, Track},
    domain::{ExternalIdentity, SearchRequest},
};
use rusqlite::Connection;

fn id(provider: &str, kind: &str, value: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: provider.into(),
        kind: kind.into(),
        external_id: value.into(),
    }
}
fn release(key: &str) -> Release {
    let credits = vec![
        Credit {
            name: "Artist A".into(),
            join_phrase: " feat. ".into(),
        },
        Credit {
            name: "Artist B".into(),
            join_phrase: "".into(),
        },
    ];
    Release {
        album: music_library::catalog::Album {
            identity: id("musicbrainz", "release_group", "group"),
            title: "Edition (Live)".into(),
            date: "2001-02-03".into(),
            credits: credits.clone(),
        },
        identity: id("musicbrainz", "release", key),
        identities: vec![],
        title: "Edition (Live)".into(),
        date: "2001-02-03".into(),
        credits: credits.clone(),
        media: vec![
            Medium {
                position: 1,
                tracks: vec![Track {
                    position: 1,
                    title: "Song (Live)".into(),
                    credits: credits.clone(),
                    identities: vec![
                        id("musicbrainz", "track", &format!("{key}-1")),
                        id("musicbrainz", "recording", "recording"),
                        id("isrc", "recording", "one"),
                        id("isrc", "recording", "two"),
                    ],
                }],
            },
            Medium {
                position: 2,
                tracks: vec![Track {
                    position: 1,
                    title: "Song - Remix".into(),
                    credits,
                    identities: vec![
                        id("musicbrainz", "track", &format!("{key}-2")),
                        id("musicbrainz", "recording", "recording"),
                        id("isrc", "recording", "one"),
                    ],
                }],
            },
        ],
    }
}
#[test]
fn atomic_source_less_add_preserves_credits_identities_membership_and_reimport() {
    let temp = tempfile::TempDir::new().unwrap();
    let path = temp.path().join("library.sqlite");
    let mut library = Library::open(&path).unwrap();
    let input = release("edition-one");
    let imported = library.add_catalog_release(&input).unwrap();
    assert_eq!(imported.track_ids.len(), 2);
    let album = library.album_for_release(&imported.release_id).unwrap();
    assert_eq!(album.title, input.album.title);
    assert_eq!(
        library
            .resolve_albums_external_identity(&input.album.identity)
            .unwrap(),
        vec![album.album_id.clone()]
    );
    assert!(
        library
            .resolve_releases_external_identity(&input.album.identity)
            .unwrap()
            .is_empty()
    );
    let rows = library
        .search(&SearchRequest {
            release_id: Some(imported.release_id.clone()),
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert!(!row.available);
        assert_eq!(row.artist_names, "Artist A feat. Artist B");
        assert_eq!(row.year, Some(2001));
    }
    for (i, track) in imported.track_ids.iter().enumerate() {
        assert!(library.available_playback_source(track).unwrap().is_none());
        assert_eq!(
            library.list_track_external_identities(track).unwrap().len(),
            if i == 0 { 4 } else { 3 }
        );
    }
    assert_eq!(
        library
            .resolve_releases_external_identity(&input.identity)
            .unwrap(),
        std::slice::from_ref(&imported.release_id)
    );
    assert_eq!(
        library
            .list_release_external_identities(&imported.release_id)
            .unwrap()
            .len(),
        1
    );
    library.remove_from_library(&imported.track_ids[0]).unwrap();
    library
        .set_track_title_override(&imported.track_ids[0], "User title")
        .unwrap();
    let mut changed = input.clone();
    changed.title = "Do not refresh".into();
    changed.media.clear();
    assert_eq!(library.add_catalog_release(&changed).unwrap(), imported);
    assert_eq!(
        library
            .search(&SearchRequest {
                release_id: Some(imported.release_id.clone()),
                limit: 10,
                ..Default::default()
            })
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        library
            .search(&SearchRequest {
                text: "User title".into(),
                limit: 10,
                ..Default::default()
            })
            .unwrap()[0]
            .artist_names,
        "Artist A feat. Artist B"
    );
    let mut second_input = release("edition-two");
    second_input.title = "Edition Deluxe".into();
    second_input.date = "2025".into();
    second_input.album.title = "Do not refresh Album".into();
    let second = library.add_catalog_release(&second_input).unwrap();
    assert_eq!(
        library.album_for_release(&second.release_id).unwrap(),
        album
    );
    let second_rows = library
        .search(&SearchRequest {
            release_id: Some(second.release_id.clone()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(second_rows[0].release_title, "Edition (Live)");
    assert_eq!(second_rows[0].year, Some(2001));
    assert_ne!(second.release_id, imported.release_id);
    assert_eq!(
        library
            .album_for_release(&second.release_id)
            .unwrap()
            .album_id,
        album.album_id
    );
    assert_eq!(
        library
            .resolve_tracks_external_identity(&id("musicbrainz", "recording", "recording"))
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        library
            .resolve_tracks_external_identity(&id("isrc", "recording", "one"))
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        library
            .resolve_albums_external_identity(&id("musicbrainz", "release_group", "group"))
            .unwrap()
            .len(),
        1
    );
    let db = Connection::open(&path).unwrap();
    for table in ["playable_source", "track_source"] {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, u32>(0))
                .unwrap(),
            0
        );
    }
    let positions:Vec<(u32,u32)>=db.prepare("SELECT disc_number,track_number FROM track WHERE release_id=?1 ORDER BY disc_number,track_number").unwrap().query_map([imported.release_id.as_ref()],|r|Ok((r.get(0)?,r.get(1)?))).unwrap().map(Result::unwrap).collect();
    assert_eq!(positions, [(1, 1), (2, 1)]);
    // Shared generic identities are legal, but Add must not select an ambiguous owner.
    library
        .attach_release_external_identity(&second.release_id, &input.identity)
        .unwrap();
    assert!(library.add_catalog_release(&input).is_err());
    drop(library);
    assert_eq!(
        Library::open(path)
            .unwrap()
            .resolve_tracks_external_identity(&id("isrc", "recording", "one"))
            .unwrap()
            .len(),
        4
    );
}
#[test]
fn failed_import_rolls_back_every_entity_credit_identity_membership_and_search_row() {
    let temp = tempfile::TempDir::new().unwrap();
    let path = temp.path().join("library.sqlite");
    let mut library = Library::open(&path).unwrap();
    let db = Connection::open(path).unwrap();
    db.execute_batch("CREATE TRIGGER fail_catalog BEFORE INSERT ON track_external_identity WHEN NEW.external_id='broken-2' BEGIN SELECT RAISE(ABORT,'induced catalog failure'); END;").unwrap();
    assert!(library.add_catalog_release(&release("broken")).is_err());
    for table in [
        "album",
        "album_application_metadata",
        "album_artist_credit",
        "album_external_identity",
        "release",
        "track",
        "artist",
        "track_artist_credit",
        "release_artist_credit",
        "release_external_identity",
        "track_external_identity",
        "library_membership",
        "track_application_metadata",
        "release_application_metadata",
        "effective_track_metadata",
        "track_search",
    ] {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, u32>(0))
                .unwrap(),
            0,
            "{table}"
        );
    }
}
#[test]
fn credit_migration_upgrades_v2_and_retains_legacy_display() {
    let temp = tempfile::TempDir::new().unwrap();
    let path = temp.path().join("v2.sqlite");
    let db = Connection::open(&path).unwrap();
    db.execute_batch(include_str!("../migrations/0001_initial.sql"))
        .unwrap();
    db.execute_batch(include_str!("../migrations/0002_external_identities.sql"))
        .unwrap();
    db.execute_batch(
        "INSERT INTO release(id) VALUES ('r'); INSERT INTO track(id,release_id) VALUES ('t','r');
        INSERT INTO release_application_metadata(release_id,title) VALUES ('r','Legacy');
        INSERT INTO track_application_metadata(track_id,title) VALUES ('t','Legacy');
        INSERT INTO artist(id,name) VALUES ('a','One'),('b','Two');
        INSERT INTO track_artist_credit(track_id,position,artist_id) VALUES ('t',0,'a'),('t',1,'b');
        INSERT INTO library_membership(track_id) VALUES ('t');",
    )
    .unwrap();
    let mut library = Library::open(path).unwrap();
    library
        .set_track_title_override(&music_library::domain::TrackId("t".into()), "Legacy")
        .unwrap();
    assert_eq!(
        library
            .search(&SearchRequest {
                limit: 10,
                ..Default::default()
            })
            .unwrap()[0]
            .artist_names,
        "One, Two"
    );
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        4
    );
}

#[test]
fn exact_existing_edition_anchors_album_and_conflicting_owners_roll_back() {
    let mut library = Library::open_in_memory().unwrap();
    let input = release("edition");
    let old = library
        .create_catalog_release(&music_library::domain::CatalogReleaseInput {
            title: "Existing friendly title".into(),
            year: Some(1999),
            artists: vec![],
            tracks: vec![music_library::domain::CatalogTrackInput {
                title: "Existing track".into(),
                artists: vec![],
                disc_number: Some(1),
                track_number: Some(1),
            }],
        })
        .unwrap();
    let album = library.album_for_release(&old.release_id).unwrap();
    library
        .attach_release_external_identity(&old.release_id, &input.identity)
        .unwrap();
    assert_eq!(library.add_catalog_release(&input).unwrap(), old);
    assert_eq!(library.album_for_release(&old.release_id).unwrap(), album); // no metadata refresh
    assert_eq!(
        library
            .resolve_albums_external_identity(&input.album.identity)
            .unwrap(),
        vec![album.album_id.clone()]
    );
    let mut other = release("other-edition");
    other.album.identity.external_id = "other-album".into();
    let imported = library.add_catalog_release(&other).unwrap();
    let other_album = library.album_for_release(&imported.release_id).unwrap();
    other.identity = input.identity.clone();
    assert!(library.add_catalog_release(&other).is_err());
    assert_eq!(library.album_for_release(&old.release_id).unwrap(), album);
    assert_eq!(
        library.album_for_release(&imported.release_id).unwrap(),
        other_album
    );
    library
        .attach_album_external_identity(&other_album.album_id, &input.album.identity)
        .unwrap();
    assert!(library.add_catalog_release(&input).is_err()); // generic sharing is valid, implicit selection is not
}
