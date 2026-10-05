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
    db.execute_batch("DROP TABLE IF EXISTS playlist_source; DROP TABLE playlist_entry; DROP TABLE playlist; PRAGMA user_version=20; CREATE TABLE playlist_entry(sentinel TEXT);").unwrap();
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

#[test]
fn reverse_playlist_windows_preserve_duplicate_entries_and_name_ties() {
    let tmp = tempfile::tempdir().unwrap();
    let mut l = Library::open(tmp.path().join("scroll.sqlite")).unwrap();
    let track = l
        .create_catalog_release(&CatalogReleaseInput {
            title: "Release".into(),
            year: None,
            artists: vec![],
            tracks: vec![CatalogTrackInput {
                title: "Song".into(),
                disc_number: None,
                track_number: None,
                artists: vec![],
            }],
        })
        .unwrap()
        .track_ids
        .remove(0);
    let mut playlists = vec![];
    for _ in 0..205 {
        playlists.push(l.create_playlist("Same name").unwrap());
    }
    let first = l.playlists(None, 200).unwrap();
    let second = l.playlists(Some(&first[199].cursor), 200).unwrap();
    let back = l.playlists_before(&second[0].cursor, 200).unwrap();
    assert_eq!(
        back.iter().rev().map(|r| &r.id).collect::<Vec<_>>(),
        first.iter().map(|r| &r.id).collect::<Vec<_>>()
    );
    for p in &playlists[..2] {
        for _ in 0..205 {
            l.append_playlist_track(p, &track).unwrap();
        }
    }
    let first = l
        .selected_playlist_entries(&playlists[..2], None, 200)
        .unwrap();
    let second = l
        .selected_playlist_entries(&playlists[..2], Some(&first[199].cursor), 200)
        .unwrap();
    let back = l
        .selected_playlist_entries_before(&playlists[..2], &second[0].cursor, 200)
        .unwrap();
    assert_eq!(
        back.iter().rev().map(|r| &r.id).collect::<Vec<_>>(),
        first.iter().map(|r| &r.id).collect::<Vec<_>>()
    );
    let third = l
        .selected_playlist_entries(&playlists[..2], Some(&second[199].cursor), 200)
        .unwrap();
    let ordered = first
        .iter()
        .chain(&second)
        .chain(&third)
        .collect::<Vec<_>>();
    assert!(
        ordered
            .windows(2)
            .all(|w| (&w[0].cursor.release, w[0].cursor.position, &w[0].id)
                < (&w[1].cursor.release, w[1].cursor.position, &w[1].id))
    );
    let ids = first
        .iter()
        .chain(&second)
        .chain(&third)
        .map(|r| &r.id)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(ids.len(), 410);
    assert!(
        first
            .iter()
            .chain(&second)
            .chain(&third)
            .all(|r| r.track.as_ref().unwrap().track_id == track)
    );
}

#[test]
fn details_count_occurrences_duration_and_gap_safe_positions() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("details.sqlite");
    let mut library = Library::open(&path).unwrap();
    let tracks = library
        .create_catalog_release(&CatalogReleaseInput {
            title: "Catalog release".into(),
            year: None,
            artists: vec![],
            tracks: [9, 31]
                .into_iter()
                .map(|number| CatalogTrackInput {
                    title: format!("Track {number}"),
                    artists: vec![],
                    disc_number: Some(2),
                    track_number: Some(number),
                })
                .collect(),
        })
        .unwrap()
        .track_ids;
    let playlist = library.create_playlist("Listen later").unwrap();
    let empty = library.playlist_details(&playlist).unwrap().unwrap();
    assert_eq!(
        (
            empty.entry_count,
            empty.known_duration_ms,
            empty.unknown_duration_count
        ),
        (0, 0, 0)
    );
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "UPDATE effective_track_metadata SET duration_ms=120000 WHERE track_id=?1",
        [tracks[0].as_ref()],
    )
    .unwrap();
    db.execute(
        "UPDATE effective_track_metadata SET duration_ms=180000 WHERE track_id=?1",
        [tracks[1].as_ref()],
    )
    .unwrap();
    let a = library
        .append_playlist_track(&playlist, &tracks[0])
        .unwrap();
    let b = library
        .append_playlist_track(&playlist, &tracks[1])
        .unwrap();
    let c = library
        .append_playlist_track(&playlist, &tracks[0])
        .unwrap();
    let details = library.playlist_details(&playlist).unwrap().unwrap();
    assert_eq!(details.name, "Listen later");
    assert_eq!(
        (
            details.entry_count,
            details.known_duration_ms,
            details.unknown_duration_count
        ),
        (3, 420000, 0)
    );
    assert!(
        library
            .search(&SearchRequest::default())
            .unwrap()
            .is_empty()
    );
    let rows = library.playlist_entries(&playlist, None, 200).unwrap();
    assert_eq!(
        rows.iter().map(|r| r.playlist_position).collect::<Vec<_>>(),
        [Some(1), Some(2), Some(3)]
    );
    assert_eq!(rows[0].track_number, Some(9));
    assert_ne!(rows[0].id, rows[2].id);
    library.move_playlist_entry(&playlist, &c, false).unwrap();
    assert_eq!(
        library.playlist_details(&playlist).unwrap().unwrap(),
        details
    );
    let rows = library.playlist_entries(&playlist, None, 200).unwrap();
    assert_eq!(
        rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        [&a, &c, &b]
    );
    assert_eq!(
        rows.iter().map(|r| r.playlist_position).collect::<Vec<_>>(),
        [Some(1), Some(2), Some(3)]
    );
    db.execute(
        "UPDATE effective_track_metadata SET duration_ms=NULL WHERE track_id=?1",
        [tracks[1].as_ref()],
    )
    .unwrap();
    let partial = library.playlist_details(&playlist).unwrap().unwrap();
    assert_eq!(
        (
            partial.entry_count,
            partial.known_duration_ms,
            partial.unknown_duration_count
        ),
        (3, 240000, 1)
    );
    library.rename_playlist(&playlist, "Renamed").unwrap();
    assert_eq!(
        library.playlist_details(&playlist).unwrap().unwrap().name,
        "Renamed"
    );
    library.remove_playlist_entry(&playlist, &c).unwrap();
    let rows = library.playlist_entries(&playlist, None, 200).unwrap();
    assert_eq!(
        rows.iter().map(|r| r.playlist_position).collect::<Vec<_>>(),
        [Some(1), Some(2)]
    );
    assert_eq!(
        rows[1].cursor.position, 2,
        "persisted gaps remain cursor data"
    );
    let d = library
        .append_playlist_track(&playlist, &tracks[0])
        .unwrap();
    assert_eq!(
        library.playlist_entries(&playlist, None, 200).unwrap()[2].playlist_position,
        Some(3)
    );
    library.remove_playlist_entry(&playlist, &a).unwrap();
    drop(library);
    let mut library = Library::open(&path).unwrap();
    let rows = library.playlist_entries(&playlist, None, 200).unwrap();
    assert_eq!(
        rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        [&b, &d]
    );
    assert_eq!(
        rows.iter().map(|r| r.playlist_position).collect::<Vec<_>>(),
        [Some(1), Some(2)]
    );
    let details = library
        .library_queue_reader()
        .unwrap()
        .playlist_details(&playlist)
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            details.entry_count,
            details.known_duration_ms,
            details.unknown_duration_count
        ),
        (2, 120000, 1)
    );
    // Library album ordering still exposes canonical album track numbers.
    for track in &tracks {
        library.add_to_library(track).unwrap();
    }
    let album_rows = library
        .browse(&Request {
            sort: music_library::browse::Sort::Album,
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        album_rows
            .iter()
            .map(|r| r.track_number)
            .collect::<Vec<_>>(),
        [Some(9), Some(31)]
    );
    assert!(album_rows.iter().all(|r| r.playlist_position.is_none()));
    library.delete_playlist(&playlist).unwrap();
    assert!(library.playlist_details(&playlist).unwrap().is_none());
}

#[test]
fn absolute_playlist_positions_cross_forward_reverse_chunks_and_reloads() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("chunks.sqlite");
    let mut library = Library::open(&path).unwrap();
    let track = library
        .create_catalog_release(&CatalogReleaseInput {
            title: "One release".into(),
            year: None,
            artists: vec![],
            tracks: vec![CatalogTrackInput {
                title: "Repeated Song".into(),
                artists: vec![],
                disc_number: Some(2),
                track_number: Some(9),
            }],
        })
        .unwrap()
        .track_ids
        .remove(0);
    let playlist = library.create_playlist("1000 duplicates").unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute("INSERT INTO playlist_entry(id,playlist_id,track_id,position) WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM n WHERE i<999) SELECT 'entry-'||i,?1,?2,i*3 FROM n", rusqlite::params![playlist, track.as_ref()]).unwrap();
    let first = library.playlist_entries(&playlist, None, 200).unwrap();
    let second = library
        .playlist_entries(&playlist, Some(first.last().unwrap().cursor.position), 200)
        .unwrap();
    assert_eq!(second[0].playlist_position, Some(201));
    assert_eq!(second[199].playlist_position, Some(400));
    let deep = library
        .playlist_entries(&playlist, Some(2697), 200)
        .unwrap();
    assert_eq!(deep[0].playlist_position, Some(901));
    assert_eq!(deep.last().unwrap().playlist_position, Some(1000));
    let reverse = library
        .selected_playlist_entries_before(std::slice::from_ref(&playlist), &deep[0].cursor, 200)
        .unwrap();
    assert_eq!(reverse[0].playlist_position, Some(900));
    assert_eq!(reverse.last().unwrap().playlist_position, Some(701));
    library
        .remove_playlist_entry(&playlist, "entry-100")
        .unwrap();
    let deep = library
        .playlist_entries(&playlist, Some(2697), 200)
        .unwrap();
    assert_eq!(deep[0].playlist_position, Some(900));
    let next = library.append_playlist_track(&playlist, &track).unwrap();
    drop(library);
    let library = Library::open(&path).unwrap();
    let deep = library
        .playlist_entries(&playlist, Some(2697), 200)
        .unwrap();
    assert_eq!(deep.last().unwrap().id, next);
    assert_eq!(deep.last().unwrap().playlist_position, Some(1000));
    let details = library.playlist_details(&playlist).unwrap().unwrap();
    assert_eq!(
        (
            details.entry_count,
            details.unknown_duration_count,
            details.known_duration_ms
        ),
        (1000, 1000, 0)
    );
    // Both prefix counts and aggregation stay scoped to the playlist order index.
    let plans = db.prepare("EXPLAIN QUERY PLAN SELECT COUNT(*) FROM playlist_entry WHERE playlist_id=?1 AND position<=?2").unwrap().query_map(rusqlite::params![playlist, 2700], |r| r.get::<_, String>(3)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
    assert!(
        plans.iter().any(|p| p.contains("playlist_entry_order")),
        "{plans:?}"
    );
}

#[test]
fn playlist_table_sorts_occurrences_globally_without_mutating_order() {
    use music_library::playlist::{Column, ViewSort};
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("table.sqlite");
    let mut l = Library::open(&path).unwrap();
    let tracks = l
        .create_catalog_release(&CatalogReleaseInput {
            title: "Catalog edition".into(),
            year: None,
            artists: vec![],
            tracks: ["Song C", "song A", "Song B"]
                .into_iter()
                .enumerate()
                .map(|(i, title)| CatalogTrackInput {
                    title: title.into(),
                    disc_number: Some(2),
                    track_number: Some(20 - i as u32),
                    artists: vec![],
                })
                .collect(),
        })
        .unwrap()
        .track_ids;
    let p = l.create_playlist("Table").unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    for (i, track) in tracks.iter().enumerate() {
        db.execute("UPDATE effective_track_metadata SET artist_names=?2,release_title=?3,duration_ms=?4 WHERE track_id=?1",rusqlite::params![track.as_ref(),["Zulu","Alpha","Beta"][i],["R2","R3","R1"][i],[Some(100_000_i64),Some(9_000),None][i]]).unwrap();
    }
    let mut expected = Vec::new();
    for i in 0..1003 {
        expected.push(l.append_playlist_track(&p, &tracks[i % 3]).unwrap());
    }
    // Legacy gaps must not leak into the displayed # column.
    db.execute(
        "UPDATE playlist_entry SET position=-position-1 WHERE playlist_id=?1",
        [&p],
    )
    .unwrap();
    db.execute(
        "UPDATE playlist_entry SET position=(-position-1)*3 WHERE playlist_id=?1",
        [&p],
    )
    .unwrap();
    let stats = l.playlist_details(&p).unwrap();
    for column in [
        Column::Position,
        Column::Title,
        Column::Artist,
        Column::Album,
        Column::Length,
    ] {
        for descending in [false, true] {
            let sort = ViewSort { column, descending };
            let mut all = Vec::new();
            let mut cursor = None;
            loop {
                let page = l
                    .playlist_view(std::slice::from_ref(&p), sort, cursor.as_ref(), 200, false)
                    .unwrap();
                if page.is_empty() {
                    break;
                }
                assert!(page.len() <= 200);
                if !all.is_empty() {
                    let back = l
                        .playlist_view(
                            std::slice::from_ref(&p),
                            sort,
                            Some(&page[0].cursor),
                            200,
                            true,
                        )
                        .unwrap();
                    assert_eq!(
                        back.iter().rev().map(|r| &r.id).collect::<Vec<_>>(),
                        all[all.len() - 200..]
                            .iter()
                            .map(|r: &music_library::browse::Row| &r.id)
                            .collect::<Vec<_>>()
                    );
                }
                cursor = Some(page.last().unwrap().cursor.clone());
                all.extend(page);
            }
            assert_eq!(all.len(), 1003);
            let key = |r: &music_library::browse::Row| match column {
                Column::Position => format!("{:08}", r.playlist_position.unwrap()),
                Column::Title => r.title.to_ascii_lowercase(),
                Column::Artist => r.subtitle.to_ascii_lowercase(),
                Column::Album => "catalog edition".to_string(),
                Column::Length => {
                    format!("{:09}", r.duration_ms.map(|v| v as i64).unwrap_or(-1) + 1)
                }
            };
            assert!(all.windows(2).all(|w| if descending {
                key(&w[0]) >= key(&w[1])
            } else {
                key(&w[0]) <= key(&w[1])
            }));
            for row in &all {
                assert_eq!(
                    expected[row.playlist_position.unwrap() as usize - 1],
                    row.id
                );
                assert_eq!(
                    row.track_number,
                    Some(20 - (row.playlist_position.unwrap() as u32 - 1) % 3)
                );
            }
            assert_eq!(
                all.iter()
                    .map(|r| &r.id)
                    .collect::<std::collections::HashSet<_>>()
                    .len(),
                1003
            );
            let range = l
                .library_queue_reader()
                .unwrap()
                .playlist_view_range(std::slice::from_ref(&p), sort, &all[190].id, &all[410].id)
                .unwrap();
            assert_eq!(
                range,
                all[190..=410]
                    .iter()
                    .map(|r| r.id.clone())
                    .collect::<Vec<_>>()
            );
            assert_eq!(l.playlist_details(&p).unwrap(), stats);
            let canonical = l.playlist_entries(&p, None, 200).unwrap();
            assert_eq!(
                canonical.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
                expected[..200]
            );
        }
    }
    assert!(ViewSort::default().allows_reordering());
    assert!(
        !ViewSort {
            column: Column::Position,
            descending: true
        }
        .allows_reordering()
    );
    let (queue, start) = l
        .library_queue_reader()
        .unwrap()
        .read_playlist(&p, Some(&expected[423]))
        .unwrap();
    assert_eq!(start, 423);
    assert_eq!(queue.len(), 1003);
    assert!(
        l.browse(&Request {
            limit: 200,
            ..Default::default()
        })
        .unwrap()
        .is_empty()
    );
    drop(l);
    let l = Library::open(&path).unwrap();
    assert_eq!(
        l.playlist_view(
            std::slice::from_ref(&p),
            ViewSort::default(),
            None,
            200,
            false
        )
        .unwrap()[0]
            .id,
        expected[0]
    );
}

#[test]
fn playlist_display_cache_uses_canonical_metadata_and_invalidates_after_edits() {
    use music_library::playlist::{Column, ViewSort};
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("cache.sqlite");
    let mut l = Library::open(&path).unwrap();
    let p = l.create_playlist("Cache").unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    let mut tracks = Vec::new();
    let mut ids = Vec::new();
    for (i, album) in ["Zulu", "Alpha", "Middle"].into_iter().enumerate() {
        let release = l
            .create_catalog_release(&CatalogReleaseInput {
                title: album.into(),
                year: None,
                artists: vec![],
                tracks: vec![CatalogTrackInput {
                    title: ["z", "a", "A"][i].into(),
                    disc_number: None,
                    track_number: None,
                    artists: vec![],
                }],
            })
            .unwrap();
        let t = release.track_ids[0].clone();
        db.execute(
            "UPDATE effective_track_metadata SET artist_names=?2,duration_ms=?3 WHERE track_id=?1",
            rusqlite::params![
                t.as_ref(),
                ["Ada", "Zulu", ""][i],
                [Some(100_000_i64), Some(9_000), None][i]
            ],
        )
        .unwrap();
        ids.push(l.append_playlist_track(&p, &t).unwrap());
        tracks.push(t);
    }
    let read = |l: &Library, column| {
        l.playlist_view(
            std::slice::from_ref(&p),
            ViewSort {
                column,
                descending: false,
            },
            None,
            200,
            false,
        )
        .unwrap()
    };
    assert_eq!(
        read(&l, Column::Album)
            .iter()
            .map(|r| &r.id)
            .collect::<Vec<_>>(),
        [&ids[1], &ids[2], &ids[0]]
    );
    assert_eq!(
        read(&l, Column::Artist)
            .iter()
            .map(|r| &r.id)
            .collect::<Vec<_>>(),
        [&ids[2], &ids[0], &ids[1]]
    );
    assert_eq!(
        read(&l, Column::Length)
            .iter()
            .map(|r| &r.id)
            .collect::<Vec<_>>(),
        [&ids[2], &ids[1], &ids[0]]
    );
    assert_eq!(
        read(&l, Column::Title)
            .iter()
            .map(|r| &r.id)
            .collect::<Vec<_>>(),
        [&ids[1], &ids[2], &ids[0]]
    );
    db.execute(
        "UPDATE effective_track_metadata SET title='',duration_ms=-1 WHERE track_id=?1",
        [tracks[0].as_ref()],
    )
    .unwrap();
    let rows = read(&l, Column::Title);
    assert_eq!(rows[0].id, ids[0]);
    assert!(rows[0].title.is_empty());
    assert_eq!(rows[0].duration_ms, None);
    l.move_playlist_entry(&p, &ids[0], true).unwrap();
    assert_eq!(read(&l, Column::Title)[0].playlist_position, Some(2));
    l.remove_playlist_entry(&p, &ids[1]).unwrap();
    let appended = l.append_playlist_track(&p, &tracks[0]).unwrap();
    let rows = read(&l, Column::Title);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].id, ids[0]);
    assert_eq!(rows[0].playlist_position, Some(1));
    assert_eq!(rows[1].id, appended);
    assert_eq!(rows[1].playlist_position, Some(3));
    l.add_to_library(&tracks[0]).unwrap();
    assert_eq!(read(&l, Column::Title).len(), 3);
    l.remove_from_library(&tracks[0]).unwrap();
    assert_eq!(read(&l, Column::Title).len(), 3);
}

#[test]
fn playlist_content_revisions_cover_all_entry_writers_and_ignore_titles_metadata_and_other_playlists()
 {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.sqlite");
    let mut library = Library::open(&path).unwrap();
    let tracks = library
        .create_catalog_release(&CatalogReleaseInput {
            title: "Edition".into(),
            year: None,
            artists: vec![],
            tracks: ["A", "B"]
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
    let a = library.create_playlist("A").unwrap();
    let b = library.create_playlist("B").unwrap();
    let revision = |library: &Library, id: &str| {
        library.playlist_content_revisions(&[id.into()]).unwrap()[0].1
    };
    assert_eq!(revision(&library, &a), 0);
    let first = library.append_playlist_track(&a, &tracks[0]).unwrap();
    let initial = revision(&library, &a);
    assert!(initial > 0);
    assert_eq!(revision(&library, &b), 0);
    library.rename_playlist(&a, "Renamed").unwrap();
    library
        .set_track_title_override(&tracks[0], "Title override")
        .unwrap();
    assert_eq!(revision(&library, &a), initial);
    let mut worker = library.playlist_append_worker().unwrap();
    let plan = worker.prepare(&a, vec![tracks[1].clone()]).unwrap();
    worker.apply(&plan, true).unwrap();
    let appended = revision(&library, &a);
    assert!(appended > initial);
    library.move_playlist_entry(&a, &first, true).unwrap();
    let moved = revision(&library, &a);
    assert!(moved > appended);
    library.append_playlist_track(&b, &tracks[0]).unwrap();
    assert_eq!(revision(&library, &a), moved);
    library
        .remove_playlist_entries(std::slice::from_ref(&first))
        .unwrap();
    let removed = revision(&library, &a);
    assert!(removed > moved);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("BEGIN; DELETE FROM playlist_entry; ROLLBACK;")
        .unwrap();
    assert_eq!(revision(&library, &a), removed);
    assert_eq!(
        Library::open(&path)
            .unwrap()
            .playlist_content_revisions(&[a])
            .unwrap()[0]
            .1,
        removed
    );
}

#[test]
fn playlist_repoint_reads_canonical_song_projection_with_album_credit_fallback() {
    use music_library::{
        browse::Pane,
        playlist::{Column, ViewSort},
    };
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("library.sqlite");
    let mut l = Library::open(&path).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    let create = |l: &mut Library, title: &str, album: &str, artists: Vec<ArtistCreditInput>| {
        l.create_catalog_release(&CatalogReleaseInput {
            title: album.into(),
            year: None,
            artists,
            tracks: vec![CatalogTrackInput {
                title: title.into(),
                artists: vec![],
                disc_number: Some(1),
                track_number: Some(1),
            }],
        })
        .unwrap()
    };
    let old = create(
        &mut l,
        "Spotify title",
        "Spotify Album",
        vec![ArtistCreditInput {
            name: "Spotify singer".into(),
            role: None,
        }],
    );
    let canonical = create(
        &mut l,
        "Canonical title",
        "Canonical Album",
        vec![
            ArtistCreditInput {
                name: "Canonical Alpha".into(),
                role: None,
            },
            ArtistCreditInput {
                name: "Canonical Beta".into(),
                role: None,
            },
        ],
    );
    let unknown = create(&mut l, "No artist", "Uncredited", vec![]);
    let track = &canonical.track_ids[0];
    l.add_to_library(track).unwrap();
    let album = l.album_for_release(&canonical.release_id).unwrap();
    db.execute("UPDATE album_artist_credit SET credited_name='Singer credit',join_phrase=' feat. ' WHERE album_id=?1 AND position=0",[album.album_id.as_ref()]).unwrap();
    db.execute("UPDATE effective_track_metadata SET artist_names='',genre_names='Jazz / Rock',duration_ms=123456 WHERE track_id=?1",[track.as_ref()]).unwrap();
    let playlist = l.create_playlist("Metadata projection").unwrap();
    let entry = l
        .append_playlist_track(&playlist, &old.track_ids[0])
        .unwrap();
    let sort = ViewSort {
        column: Column::Artist,
        descending: false,
    };
    let reader = l.library_queue_reader().unwrap();
    assert_eq!(
        reader
            .playlist_view(std::slice::from_ref(&playlist), sort, None, 200, false)
            .unwrap()[0]
            .subtitle,
        "Spotify singer"
    );
    let before = l
        .playlist_content_revisions(std::slice::from_ref(&playlist))
        .unwrap();
    db.execute(
        "UPDATE playlist_entry SET track_id=?2 WHERE id=?1",
        rusqlite::params![entry, track.as_ref()],
    )
    .unwrap();
    assert_ne!(
        before,
        l.playlist_content_revisions(std::slice::from_ref(&playlist))
            .unwrap()
    );
    let row = reader
        .playlist_view(std::slice::from_ref(&playlist), sort, None, 200, false)
        .unwrap()
        .remove(0);
    let song = l
        .browse(&Request {
            pane: Pane::Songs,
            tracks: vec![track.as_ref().to_owned()],
            limit: 200,
            ..Default::default()
        })
        .unwrap()
        .remove(0);
    assert_eq!(row.id, entry);
    assert_eq!(row.title, "Canonical title");
    assert_eq!(row.subtitle, "Singer credit feat. Canonical Beta");
    assert_eq!(row.subtitle, song.subtitle);
    assert_eq!(
        row.track.as_ref().unwrap().release_title,
        song.track.as_ref().unwrap().release_title
    );
    assert_eq!(row.genres, "Jazz / Rock");
    assert_eq!(row.genres, song.genres);
    assert_eq!(row.duration_ms, Some(123456));
    let canonical_row = l.playlist_entries(&playlist, None, 200).unwrap().remove(0);
    assert_eq!(canonical_row.subtitle, row.subtitle);
    assert_eq!(canonical_row.genres, row.genres);
    // Explicit effective Track display credit must win over Album identity names.
    db.execute("UPDATE effective_track_metadata SET artist_names='Track singer with guest' WHERE track_id=?1",[track.as_ref()]).unwrap();
    assert_eq!(
        reader
            .playlist_view(std::slice::from_ref(&playlist), sort, None, 200, false)
            .unwrap()[0]
            .subtitle,
        "Track singer with guest"
    );
    l.append_playlist_track(&playlist, &unknown.track_ids[0])
        .unwrap();
    assert!(
        l.playlist_entries(&playlist, None, 200).unwrap()[1]
            .subtitle
            .is_empty()
    );
    drop(reader);
    drop(l);
    let l = Library::open(&path).unwrap();
    assert_eq!(
        l.playlist_entries(&playlist, None, 200).unwrap()[0].subtitle,
        "Track singer with guest"
    );
}
