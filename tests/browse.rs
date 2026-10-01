use music_library::{
    Library,
    browse::{Pane, Request},
    domain::*,
};

fn album(
    l: &mut Library,
    title: &str,
    artist: &str,
    tracks: &[(&str, u32, u32)],
) -> ImportedRelease {
    l.create_catalog_release(&CatalogReleaseInput {
        title: title.into(),
        year: None,
        artists: vec![ArtistCreditInput {
            name: artist.into(),
            role: None,
        }],
        tracks: tracks
            .iter()
            .map(|(title, disc, pos)| CatalogTrackInput {
                title: (*title).into(),
                disc_number: Some(*disc),
                track_number: Some(*pos),
                artists: vec![ArtistCreditInput {
                    name: artist.into(),
                    role: None,
                }],
            })
            .collect(),
    })
    .unwrap()
}

#[test]
fn membership_filters_partial_albums_and_program_order() {
    let tmp = tempfile::tempdir().unwrap();
    let mut l = Library::open(tmp.path().join("library.sqlite")).unwrap();
    let a = album(
        &mut l,
        "Zebra",
        "toe",
        &[("z song", 1, 1), ("A song", 1, 3), ("b song", 2, 1)],
    );
    let b = album(&mut l, "alpha", "tricot", &[("c song", 1, 1)]);
    let _unsaved = album(&mut l, "Hidden", "Unsaved artist", &[("Hidden song", 1, 1)]);
    for id in [&a.track_ids[0], &a.track_ids[2], &b.track_ids[0]] {
        l.add_to_library(id).unwrap();
    }
    // This fixture explicitly represents shared Artists; creation deliberately
    // does not infer identity from equal credit names.
    let db = rusqlite::Connection::open(tmp.path().join("library.sqlite")).unwrap();
    db.execute_batch("UPDATE track_artist_credit SET artist_id=(SELECT min(a.id) FROM artist a WHERE a.name=(SELECT name FROM artist WHERE id=track_artist_credit.artist_id));
        UPDATE release_artist_credit SET artist_id=(SELECT min(a.id) FROM artist a WHERE a.name=(SELECT name FROM artist WHERE id=release_artist_credit.artist_id));
        UPDATE album_artist_credit SET artist_id=(SELECT min(a.id) FROM artist a WHERE a.name=(SELECT name FROM artist WHERE id=album_artist_credit.artist_id));").unwrap();
    drop(db);
    let artists = l
        .browse(&Request {
            pane: Pane::Artists,
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        artists.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
        ["toe", "tricot"]
    );
    let albums = l
        .browse(&Request {
            pane: Pane::Albums,
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        albums.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
        ["alpha", "Zebra"]
    );
    let artist = ArtistId(artists[0].id.clone());
    let artist_albums = l
        .browse(&Request {
            pane: Pane::Albums,
            artist: Some(artist.clone()),
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(artist_albums.len(), 1);
    let songs = l
        .browse(&Request {
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        songs.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
        ["b song", "c song", "z song"]
    );
    let artist_songs = l
        .browse(&Request {
            artist: Some(artist.clone()),
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        artist_songs
            .iter()
            .map(|r| r.title.as_str())
            .collect::<Vec<_>>(),
        ["b song", "z song"]
    );
    let album_id = AlbumId(artist_albums[0].id.clone());
    let ordered = l
        .browse(&Request {
            album: Some(album_id),
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        ordered.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
        ["z song", "b song"]
    );
    let queue = l
        .browse(&Request {
            artist: Some(artist),
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        queue.iter().map(|r| &r.id).collect::<Vec<_>>(),
        artist_songs.iter().map(|r| &r.id).collect::<Vec<_>>()
    );
    // No source is needed for visibility. Removing membership affects every pane.
    l.remove_from_library(&b.track_ids[0]).unwrap();
    assert_eq!(
        l.browse(&Request {
            pane: Pane::Artists,
            limit: 200,
            ..Default::default()
        })
        .unwrap()
        .len(),
        1
    );
}

#[test]
fn cursor_is_complete_for_duplicate_titles_positions_and_empty_titles() {
    let tmp = tempfile::tempdir().unwrap();
    let mut l = Library::open(tmp.path().join("library.sqlite")).unwrap();
    let a = album(
        &mut l,
        "Album",
        "Artist",
        &[("", 1, 1), ("same", 1, 2), ("Same", 1, 2), ("same", 2, 1)],
    );
    for id in &a.track_ids {
        l.add_to_library(id).unwrap();
    }
    let album = AlbumId(
        l.browse(&Request {
            pane: Pane::Albums,
            limit: 1,
            ..Default::default()
        })
        .unwrap()[0]
            .id
            .clone(),
    );
    for scope in [None, Some(album)] {
        let mut request = Request {
            limit: 1,
            album: scope,
            ..Default::default()
        };
        let mut ids = std::collections::HashSet::new();
        loop {
            let rows = l.browse(&request).unwrap();
            if rows.is_empty() {
                break;
            }
            assert_eq!(rows.len(), 1);
            assert!(ids.insert(rows[0].id.clone()));
            request.after = Some(rows[0].cursor.clone());
        }
        assert_eq!(ids.len(), 4);
    }
}

#[test]
fn queue_is_complete_beyond_a_page_and_track_display_falls_back_to_album_credit() {
    let tmp = tempfile::tempdir().unwrap();
    let mut library = Library::open(tmp.path().join("library.sqlite")).unwrap();
    let imported = library
        .create_catalog_release(&CatalogReleaseInput {
            title: "Program".into(),
            year: None,
            artists: vec![ArtistCreditInput {
                name: "Album artist".into(),
                role: None,
            }],
            tracks: (0..450)
                .map(|i| CatalogTrackInput {
                    title: format!("Song {i:03}"),
                    artists: vec![],
                    disc_number: Some(i / 225 + 1),
                    track_number: Some(i % 225 + 1),
                })
                .collect(),
        })
        .unwrap();
    for track in &imported.track_ids {
        library.add_to_library(track).unwrap();
    }
    library
        .remove_from_library(&imported.track_ids[10])
        .unwrap();
    let album = library.album_for_release(&imported.release_id).unwrap();
    let request = Request {
        album: Some(album.album_id),
        limit: 200,
        ..Default::default()
    };
    assert_eq!(library.browse(&request).unwrap().len(), 200);
    let queue = library.library_queue(&request).unwrap();
    assert_eq!(queue.len(), 449);
    assert_eq!(queue[0].artist_names, "Album artist");
    assert_eq!(queue[224].title, "Song 225");
    assert_eq!(queue.last().unwrap().title, "Song 449");
    assert!(
        !queue
            .iter()
            .any(|row| row.track_id == imported.track_ids[10])
    );
    // The presentation fallback does not write Track credits or effective metadata.
    assert_eq!(
        library
            .search(&SearchRequest {
                limit: 1,
                ..Default::default()
            })
            .unwrap()[0]
            .artist_names,
        ""
    );
}

#[test]
fn complete_program_matches_paged_songs_in_every_scope_with_duplicate_titles() {
    let temp = tempfile::tempdir().unwrap();
    let mut library = Library::open(temp.path().join("program.sqlite")).unwrap();
    let input: Vec<_> = (0..1005)
        .map(|i| {
            (
                if i % 2 == 0 { "same" } else { "A song" },
                i / 225 + 1,
                1005 - i,
            )
        })
        .collect();
    let imported = album(&mut library, "Program", "Artist", &input);
    for id in &imported.track_ids {
        library.add_to_library(id).unwrap();
    }
    let album = library
        .album_for_release(&imported.release_id)
        .unwrap()
        .album_id;
    let db = rusqlite::Connection::open(temp.path().join("program.sqlite")).unwrap();
    let artist = ArtistId(
        db.query_row(
            "SELECT artist_id FROM album_artist_credit WHERE album_id=?1",
            [album.as_ref()],
            |r| r.get(0),
        )
        .unwrap(),
    );
    db.execute(
        "UPDATE track_artist_credit SET artist_id=?1",
        [artist.as_ref()],
    )
    .unwrap();
    drop(db);
    for request in [
        Request::default(),
        Request {
            artist: Some(artist.clone()),
            ..Default::default()
        },
        Request {
            artist: Some(artist.clone()),
            sort: music_library::browse::Sort::Album,
            album_sort: music_library::browse::Sort::Title,
            ..Default::default()
        },
        Request {
            artist: Some(artist),
            sort: music_library::browse::Sort::Album,
            album_sort: music_library::browse::Sort::Year,
            ..Default::default()
        },
        Request {
            album: Some(album),
            ..Default::default()
        },
    ] {
        let mut page = Request {
            limit: 200,
            ..request.clone()
        };
        let mut visible = Vec::new();
        loop {
            let rows = library.browse(&page).unwrap();
            if rows.is_empty() {
                break;
            }
            page.after = Some(rows.last().unwrap().cursor.clone());
            visible.extend(rows.into_iter().map(|r| TrackId(r.id)));
        }
        let queue = library
            .library_queue_reader()
            .unwrap()
            .read(&request)
            .unwrap();
        assert_eq!(queue.len(), 1005);
        assert_eq!(
            queue.iter().map(|r| r.track_id.clone()).collect::<Vec<_>>(),
            visible
        );
        // Later-page duplicate title resolves by ID, not by first equal title.
        let selected = &visible[403];
        assert_eq!(
            queue.iter().position(|r| &r.track_id == selected),
            Some(403)
        );
    }
}

#[test]
fn alternate_sorts_page_seek_and_snapshot_agree() {
    use music_library::browse::Sort;
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("sort.sqlite");
    let mut l = Library::open(&path).unwrap();
    for (name, year, artist) in [
        ("Zulu", Some(2001), "Same"),
        ("Alpha", Some(2020), "Same"),
        ("Unknown", None, "Other"),
    ] {
        let input: Vec<_> = (0..225)
            .map(|i| {
                (
                    if i % 2 == 0 {
                        "Duplicate"
                    } else {
                        "Alpha song"
                    },
                    i / 100 + 1,
                    225 - i,
                )
            })
            .collect();
        let release = album(&mut l, name, artist, &input);
        for id in &release.track_ids {
            l.add_to_library(id).unwrap();
        }
        let a = l.album_for_release(&release.release_id).unwrap().album_id;
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute(
                "UPDATE album_application_metadata SET year=?1 WHERE album_id=?2",
                rusqlite::params![year, a.as_ref()],
            )
            .unwrap();
    }
    let albums = l
        .browse(&Request {
            pane: Pane::Albums,
            sort: Sort::Year,
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        albums.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
        ["Alpha", "Zulu", "Unknown"]
    );
    let artists = l
        .browse(&Request {
            pane: Pane::Artists,
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    let reverse = l
        .browse(&Request {
            pane: Pane::Artists,
            sort: Sort::Descending,
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    assert!(
        reverse
            .windows(2)
            .all(|r| (r[0].title.to_lowercase(), &r[0].id) > (r[1].title.to_lowercase(), &r[1].id))
    );
    let grouped = l
        .browse(&Request {
            pane: Pane::Albums,
            sort: Sort::Artist,
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    assert_ne!(
        grouped[1].group, grouped[2].group,
        "equal artist labels retain separate identities"
    );
    for pane in [Pane::Artists, Pane::Albums, Pane::Songs] {
        for sort in [
            Sort::Title,
            Sort::Descending,
            Sort::Year,
            Sort::Artist,
            Sort::Album,
        ] {
            let scopes = if pane == Pane::Songs {
                vec![
                    (None, None),
                    (None, Some(AlbumId(albums[0].id.clone()))),
                    (Some(ArtistId(artists[0].id.clone())), None),
                ]
            } else {
                vec![(None, None)]
            };
            for (artist, album) in scopes {
                let request = Request {
                    pane,
                    sort,
                    artist,
                    album,
                    limit: 37,
                    ..Default::default()
                };
                let mut page = request.clone();
                let mut ids = Vec::new();
                loop {
                    let rows = l.browse(&page).unwrap();
                    if rows.is_empty() {
                        break;
                    }
                    page.after = rows.last().map(|r| r.cursor.clone());
                    ids.extend(rows.into_iter().map(|r| r.id));
                }
                assert_eq!(
                    ids.len(),
                    ids.iter().collect::<std::collections::HashSet<_>>().len()
                );
                for id in ids.iter().step_by(97) {
                    let from = l.browse_from(&request, id).unwrap();
                    let position = ids.iter().position(|v| v == id).unwrap();
                    assert_eq!(
                        from.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
                        ids[position..ids.len().min(position + 37)]
                    );
                    assert!(
                        l.browse_around(&request, id)
                            .unwrap()
                            .iter()
                            .any(|r| &r.id == id)
                    );
                }
                if pane == Pane::Songs {
                    let queue = l.library_queue_reader().unwrap().read(&request).unwrap();
                    assert_eq!(
                        ids,
                        queue
                            .iter()
                            .map(|r| r.track_id.0.clone())
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
    }
}

#[test]
fn album_year_fallback_and_song_groups_follow_album_sort() {
    use music_library::browse::Sort;
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("years");
    let mut l = Library::open(&path).unwrap();
    let old = album(&mut l, "Alpha", "Artist", &[("Z", 1, 1), ("A", 1, 2)]);
    let new = album(&mut l, "Zulu", "Artist", &[("B", 1, 1)]);
    for id in old.track_ids.iter().chain(&new.track_ids) {
        l.add_to_library(id).unwrap();
    }
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "UPDATE effective_track_metadata SET year=2000 WHERE track_id=?1",
        [old.track_ids[0].as_ref()],
    )
    .unwrap();
    db.execute(
        "UPDATE effective_track_metadata SET year=2020 WHERE track_id=?1",
        [new.track_ids[0].as_ref()],
    )
    .unwrap();
    let request = Request {
        pane: Pane::Albums,
        sort: Sort::Year,
        limit: 200,
        ..Default::default()
    };
    let rows = l.browse(&request).unwrap();
    assert_eq!(
        rows.iter().map(|r| (&*r.title, r.year)).collect::<Vec<_>>(),
        [("Zulu", Some(2020)), ("Alpha", Some(2000))]
    );
    let queue = l
        .library_queue(&Request {
            sort: Sort::Album,
            album_sort: Sort::Year,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        queue.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
        ["B", "Z", "A"]
    );
    let queue = l
        .library_queue(&Request {
            sort: Sort::Album,
            album_sort: Sort::Title,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        queue.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
        ["Z", "A", "B"]
    );
    // Application metadata has precedence and updates the materialized index.
    db.execute(
        "UPDATE album_application_metadata SET year=2025 WHERE title='Alpha'",
        [],
    )
    .unwrap();
    assert_eq!(l.browse(&request).unwrap()[0].title, "Alpha");
    db.execute(
        "UPDATE album_application_metadata SET year=NULL WHERE title='Alpha'",
        [],
    )
    .unwrap();
    db.execute(
        "UPDATE effective_track_metadata SET year=2030 WHERE track_id=?1",
        [old.track_ids[0].as_ref()],
    )
    .unwrap();
    assert_eq!(l.browse(&request).unwrap()[0].year, Some(2030));
}

#[test]
fn genres_intersect_saved_membership_and_album_scope_with_bounded_reverse_pages() {
    use music_library::browse::Sort;
    for count in [230, 2230] {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("library.sqlite");
        let mut l = Library::open(&path).unwrap();
        let titles: Vec<_> = (0..count).map(|i| format!("Song {i:04}")).collect();
        let tracks: Vec<_> = titles
            .iter()
            .enumerate()
            .map(|(i, t)| (t.as_str(), 1 + (i / 115) as u32, 1 + (i % 115) as u32))
            .collect();
        let a = album(&mut l, "Mixed", "Artist", &tracks);
        let b = album(&mut l, "Other", "Other Artist", &[("Zulu", 1, 1)]);
        let hidden = album(&mut l, "Hidden", "Hidden", &[("Hidden", 1, 1)]);
        for id in a.track_ids.iter().chain(&b.track_ids) {
            l.add_to_library(id).unwrap();
        }
        let db = rusqlite::Connection::open(&path).unwrap();
        for (i, id) in a
            .track_ids
            .iter()
            .chain(&b.track_ids)
            .chain(&hidden.track_ids)
            .enumerate()
        {
            let source = format!("source-{i}");
            db.execute(
                "INSERT INTO playable_source(id,kind) VALUES (?1,'local_file')",
                [&source],
            )
            .unwrap();
            db.execute(
                "INSERT INTO track_source(track_id,source_id) VALUES (?1,?2)",
                rusqlite::params![id.as_ref(), source],
            )
            .unwrap();
            let genre = if i == count + 1 {
                "Unsaved only"
            } else if i % 2 == 0 {
                "Rock"
            } else {
                "Jazz"
            };
            db.execute(
                "INSERT INTO file_genre_observation(source_id,genre) VALUES (?1,?2)",
                rusqlite::params![source, genre],
            )
            .unwrap();
        }
        let genres = l
            .browse(&Request {
                pane: Pane::Genres,
                limit: 200,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            genres.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
            ["Jazz", "Rock"]
        );
        let rock = Request {
            genre: Some("Rock".into()),
            limit: 201,
            ..Default::default()
        };
        let albums = l
            .browse(&Request {
                pane: Pane::Albums,
                ..rock.clone()
            })
            .unwrap();
        assert_eq!(
            albums.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
            ["Mixed", "Other"]
        );
        let mixed = AlbumId(albums[0].id.clone());
        let songs = l
            .browse(&Request {
                album: Some(mixed.clone()),
                sort: Sort::Album,
                ..rock.clone()
            })
            .unwrap();
        assert_eq!(songs.len(), (count / 2).min(201));
        assert!(
            songs
                .iter()
                .all(|r| r.multi_disc && r.track_number.is_some())
        );
        assert_eq!(songs[0].disc_number, Some(1));
        assert!(songs.last().unwrap().disc_number.unwrap() > 1);
        let program = l
            .library_queue(&Request {
                album: Some(mixed),
                sort: Sort::Descending,
                ..rock.clone()
            })
            .unwrap();
        assert_eq!(program.len(), count / 2);
        assert_eq!(program[0].title, format!("Song {:04}", count - 2));
        let asc = l
            .library_queue(&Request {
                sort: Sort::Title,
                ..Default::default()
            })
            .unwrap();
        let descending = Request {
            sort: Sort::Descending,
            limit: 17,
            ..Default::default()
        };
        let mut page = descending.clone();
        let mut ids = vec![];
        loop {
            let rows = l.browse(&page).unwrap();
            assert!(rows.len() <= 17);
            if rows.is_empty() {
                break;
            }
            page.after = rows.last().map(|r| r.cursor.clone());
            ids.extend(rows.into_iter().map(|r| r.id));
        }
        assert_eq!(
            ids,
            asc.iter()
                .rev()
                .map(|r| r.track_id.as_ref().to_string())
                .collect::<Vec<_>>()
        );
        for id in [ids.first().unwrap(), &ids[210], ids.last().unwrap()] {
            assert_eq!(&l.browse_from(&descending, id).unwrap()[0].id, id);
            assert!(
                l.browse_around(&descending, id)
                    .unwrap()
                    .iter()
                    .any(|r| &r.id == id)
            );
        }
        l.remove_from_library(&b.track_ids[0]).unwrap();
        assert_eq!(
            l.browse(&Request {
                pane: Pane::Albums,
                ..rock
            })
            .unwrap()
            .len(),
            1
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM playlist", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}

#[test]
fn reverse_keysets_recover_exact_preceding_chunks_for_every_order() {
    use music_library::browse::Sort;
    let tmp = tempfile::tempdir().unwrap();
    let mut l = Library::open(tmp.path().join("reverse.sqlite")).unwrap();
    for n in 0..220 {
        let r = album(
            &mut l,
            &format!("Album {:03}", n / 2),
            &format!("Artist {:03}", n / 2),
            &[("Same song", 1, 1), ("Same song", 2, 1)],
        );
        for t in r.track_ids {
            l.add_to_library(&t).unwrap();
        }
    }
    for (pane, sorts) in [
        (Pane::Artists, vec![Sort::Title, Sort::Descending]),
        (Pane::Albums, vec![Sort::Title, Sort::Year, Sort::Artist]),
        (
            Pane::Songs,
            vec![Sort::Title, Sort::Descending, Sort::Album],
        ),
    ] {
        for sort in sorts {
            for album_sort in [Sort::Title, Sort::Year] {
                let request = Request {
                    pane,
                    sort,
                    album_sort,
                    limit: 200,
                    ..Default::default()
                };
                let first = l.browse(&request).unwrap();
                let second = l
                    .browse(&Request {
                        after: Some(first.last().unwrap().cursor.clone()),
                        ..request.clone()
                    })
                    .unwrap();
                assert!(!second.is_empty());
                let back = l
                    .browse_before(&Request {
                        after: Some(second[0].cursor.clone()),
                        ..request
                    })
                    .unwrap();
                assert_eq!(
                    back.iter().rev().map(|r| &r.id).collect::<Vec<_>>(),
                    first.iter().map(|r| &r.id).collect::<Vec<_>>(),
                    "{pane:?} {sort:?} {album_sort:?}"
                );
            }
        }
    }
}
