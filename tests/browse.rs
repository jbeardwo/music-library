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
            program: true,
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        queue.iter().map(|r| &r.id).collect::<Vec<_>>(),
        ordered.iter().map(|r| &r.id).collect::<Vec<_>>()
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
    for program in [false, true] {
        let mut request = Request {
            limit: 1,
            program,
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
