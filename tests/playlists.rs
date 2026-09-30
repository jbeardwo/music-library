use music_library::{Library, browse::Request, domain::*};
#[test]
fn persisted_entries_membership_and_snapshot_are_independent() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("library.sqlite");
    let mut l = Library::open(&path).unwrap();
    let tracks = l
        .create_catalog_release(&CatalogReleaseInput {
            title: "Release".into(),
            year: None,
            artists: vec![],
            tracks: ["Zulu", "Alpha"]
                .into_iter()
                .map(|title| CatalogTrackInput {
                    title: title.into(),
                    disc_number: None,
                    track_number: None,
                    artists: vec![],
                })
                .collect(),
        })
        .unwrap()
        .track_ids;
    let p = l.create_playlist("Favorites").unwrap();
    assert!(l.create_playlist(" ").is_err());
    l.rename_playlist(&p, "Listen later").unwrap();
    assert_eq!(l.playlists(None, 200).unwrap()[0].title, "Listen later");
    let a = l.append_playlist_track(&p, &tracks[0]).unwrap();
    let b = l.append_playlist_track(&p, &tracks[1]).unwrap();
    let c = l.append_playlist_track(&p, &tracks[0]).unwrap();
    assert_ne!(a, c);
    assert!(
        l.browse(&Request {
            limit: 200,
            ..Default::default()
        })
        .unwrap()
        .is_empty()
    );
    l.add_to_library(&tracks[0]).unwrap();
    l.remove_from_library(&tracks[0]).unwrap();
    assert_eq!(l.playlist_entries(&p, None, 200).unwrap().len(), 3);
    l.move_playlist_entry(&p, &c, false).unwrap();
    let rows = l.playlist_entries(&p, None, 200).unwrap();
    assert_eq!(
        rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        [a.as_str(), c.as_str(), b.as_str()]
    );
    let page = l.playlist_entries(&p, None, 1).unwrap();
    assert_eq!(
        l.playlist_entries(&p, Some(page[0].cursor.position), 1)
            .unwrap()[0]
            .id,
        c
    );
    let reader = l.library_queue_reader().unwrap();
    let (snapshot, start) = reader.read_playlist(&p, Some(&c)).unwrap();
    assert_eq!(start, 1);
    assert_eq!(
        snapshot
            .iter()
            .map(|r| r.track_id.clone())
            .collect::<Vec<_>>(),
        [tracks[0].clone(), tracks[0].clone(), tracks[1].clone()]
    );
    l.move_playlist_entry(&p, &c, true).unwrap();
    assert_eq!(l.playlist_entries(&p, None, 200).unwrap()[2].id, c);
    l.move_playlist_entry(&p, &c, false).unwrap();
    l.add_to_library(&tracks[1]).unwrap();
    l.remove_playlist_entry(&p, &b).unwrap();
    assert_eq!(
        l.browse(&Request {
            limit: 200,
            ..Default::default()
        })
        .unwrap()[0]
            .id,
        tracks[1].as_ref()
    );
    l.remove_playlist_entry(&p, &a).unwrap();
    assert_eq!(snapshot.len(), 3);
    drop(l);
    let mut l = Library::open(&path).unwrap();
    assert_eq!(l.playlist_entries(&p, None, 200).unwrap()[0].id, c);
    l.delete_playlist(&p).unwrap();
    assert!(l.playlists(None, 200).unwrap().is_empty());
    assert!(l.playlist_entries(&p, None, 200).unwrap().is_empty());
    l.add_to_library(&tracks[1]).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    let plan:String=db.query_row("EXPLAIN QUERY PLAN SELECT track_id FROM playlist_entry WHERE playlist_id='p' AND position>100 ORDER BY position LIMIT 201",[],|r|r.get(3)).unwrap();
    assert!(plan.contains("playlist_entry_order"), "{plan}");
}

#[test]
fn failed_playlist_migration_is_atomic() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.sqlite");
    drop(Library::open(&path).unwrap());
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("DROP TABLE playlist_entry; DROP TABLE playlist; PRAGMA user_version=20; CREATE TABLE playlist_entry(sentinel TEXT);").unwrap();
    assert!(Library::open(&path).is_err());
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        20
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='playlist'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}
