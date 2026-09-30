use music_library::{
    Library,
    browse::{Pane, Request, Sort},
    domain::*,
    selection::Selection,
    track_container::Target,
};
fn fixture() -> (tempfile::TempDir, Library, Vec<ImportedRelease>) {
    let temp = tempfile::tempdir().unwrap();
    let mut l = Library::open(temp.path().join("db")).unwrap();
    let mut releases = vec![];
    for artist in ["Artist A", "Artist B", "Artist C"] {
        let release = l
            .create_catalog_release(&CatalogReleaseInput {
                title: format!("Album {artist}"),
                year: None,
                artists: vec![ArtistCreditInput {
                    name: artist.into(),
                    role: None,
                }],
                tracks: ["Same title", "Second"]
                    .into_iter()
                    .enumerate()
                    .map(|(i, title)| CatalogTrackInput {
                        title: title.into(),
                        disc_number: Some(1),
                        track_number: Some(i as u32 + 1),
                        artists: vec![ArtistCreditInput {
                            name: artist.into(),
                            role: None,
                        }],
                    })
                    .collect(),
            })
            .unwrap();
        for t in &release.track_ids {
            l.add_to_library(t).unwrap();
        }
        releases.push(release);
    }
    let db = rusqlite::Connection::open(temp.path().join("db")).unwrap();
    for (i, r) in releases.iter().enumerate() {
        for track in &r.track_ids {
            db.execute(
                "INSERT INTO playable_source(id,kind) VALUES(?1,'local_file')",
                [track.as_ref()],
            )
            .unwrap();
            db.execute(
                "INSERT INTO track_source(track_id,source_id) VALUES(?1,?1)",
                [track.as_ref()],
            )
            .unwrap();
            db.execute(
                "INSERT INTO file_genre_observation(source_id,genre) VALUES(?1,?2)",
                rusqlite::params![track.as_ref(), if i == 0 { "Rock" } else { "Jazz" }],
            )
            .unwrap();
        }
    }
    (temp, l, releases)
}
#[test]
fn stable_selection_toggle_ranges_context_and_pane_independence() {
    let mut panes: [Selection; 3] = Default::default();
    panes[0].single("a");
    panes[0].toggle("b");
    assert_eq!(panes[0].ids.len(), 2);
    panes[1].single("album");
    panes[2].single("song");
    assert!(!panes[0].context("a"));
    assert_eq!(panes[0].ids.len(), 2);
    assert!(panes[1].context("other"));
    assert_eq!(panes[1].ids.iter().cloned().collect::<Vec<_>>(), ["other"]);
    assert_eq!(panes[0].ids.len(), 2);
    assert!(panes[2].ids.contains("song"));
    panes[0].toggle("b");
    assert!(!panes[0].ids.contains("b"));
    panes[0].range(vec!["a".into(), "b".into(), "c".into()], "c");
    assert_eq!(panes[0].ids.len(), 3);
    panes[0].retain(&["c".into()].into_iter().collect());
    assert_eq!(panes[0].anchor.as_deref(), Some("c"));
}
#[test]
fn multi_filters_union_intersection_pruning_and_all_container_expansions() {
    let (_temp, l, releases) = fixture();
    let artists = l
        .browse(&Request {
            pane: Pane::Artists,
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    // Catalog creation does not infer Artist identity from equal strings; include all canonical credits for A/B.
    let selected = artists
        .iter()
        .filter(|r| r.title != "Artist C")
        .map(|r| r.id.clone())
        .collect::<Vec<_>>();
    let request = Request {
        artists: selected.clone(),
        sort: Sort::Title,
        limit: 201,
        ..Default::default()
    };
    let albums = l
        .browse(&Request {
            pane: Pane::Albums,
            ..request.clone()
        })
        .unwrap();
    assert_eq!(albums.len(), 2);
    assert_eq!(l.browse(&request).unwrap().len(), 4);
    let album = albums[0].id.clone();
    let narrowed = Request {
        albums: vec![album.clone()],
        ..request.clone()
    };
    assert_eq!(l.browse(&narrowed).unwrap().len(), 2);
    let reader = l.library_queue_reader().unwrap();
    let song_ids = releases
        .iter()
        .flat_map(|r| r.track_ids.iter().map(|t| t.0.clone()))
        .collect::<Vec<_>>();
    assert_eq!(reader.browse_ids(&narrowed, &song_ids).unwrap().len(), 2);
    assert_eq!(
        reader
            .resolve(&Target::Artists(selected), &request)
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        reader
            .resolve(
                &Target::Genres(vec!["Rock".into(), "Jazz".into()]),
                &request
            )
            .unwrap()
            .len(),
        6
    );
    assert_eq!(
        reader
            .resolve(
                &Target::Albums(albums.iter().map(|a| a.id.clone()).collect()),
                &request
            )
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        reader
            .resolve(
                &Target::Songs(vec![song_ids[0].clone(), song_ids[4].clone()]),
                &request
            )
            .unwrap()
            .len(),
        2
    );
    assert!(
        reader
            .resolve(&Target::Songs(vec![]), &request)
            .unwrap()
            .is_empty()
    );
    let genres = Request {
        genres: vec!["Rock".into(), "Jazz".into()],
        limit: 201,
        ..Default::default()
    };
    assert_eq!(
        l.browse(&Request {
            pane: Pane::Albums,
            ..genres.clone()
        })
        .unwrap()
        .len(),
        3
    );
    assert_eq!(
        l.browse(&Request {
            albums: vec![album],
            ..genres
        })
        .unwrap()
        .len(),
        2
    );
}
#[test]
fn ranges_cross_pages_in_both_display_orders() {
    let (temp, mut l, _) = fixture();
    let r = l
        .create_catalog_release(&CatalogReleaseInput {
            title: "Large".into(),
            year: None,
            artists: vec![],
            tracks: (0..420)
                .map(|i| CatalogTrackInput {
                    title: format!("Range {i:04}"),
                    disc_number: None,
                    track_number: Some(i + 1),
                    artists: vec![],
                })
                .collect(),
        })
        .unwrap();
    for t in &r.track_ids {
        l.add_to_library(t).unwrap();
    }
    let db = rusqlite::Connection::open(temp.path().join("db")).unwrap();
    let album: String = db
        .query_row(
            "SELECT album_id FROM release WHERE id=?1",
            [r.release_id.as_ref()],
            |r| r.get(0),
        )
        .unwrap();
    let reader = l.library_queue_reader().unwrap();
    for sort in [Sort::Title, Sort::Descending, Sort::Album] {
        let request = Request {
            albums: vec![album.clone()],
            sort,
            album_sort: Sort::Title,
            ..Default::default()
        };
        let ids = reader
            .range(
                &request,
                r.track_ids[20].as_ref(),
                r.track_ids[405].as_ref(),
            )
            .unwrap();
        assert_eq!(ids.len(), 386);
        assert_eq!(
            ids[0],
            r.track_ids[if sort == Sort::Descending { 405 } else { 20 }].0
        );
    }
}
#[test]
fn playlist_sources_duplicates_confirmation_and_canonical_identity() {
    let (_temp, mut l, r) = fixture();
    let source = l.create_playlist("Source").unwrap();
    let destination = l.create_playlist("Destination").unwrap();
    l.remove_from_library(&r[0].track_ids[0]).unwrap();
    let a = l
        .append_playlist_track(&source, &r[0].track_ids[0])
        .unwrap();
    let b = l
        .append_playlist_track(&source, &r[1].track_ids[0])
        .unwrap();
    let c = l
        .append_playlist_track(&source, &r[0].track_ids[0])
        .unwrap();
    let target = Target::Playlists(vec![source.clone()]);
    let rows = l
        .library_queue_reader()
        .unwrap()
        .resolve(&target, &Request::default())
        .unwrap();
    assert_eq!(
        rows.iter().map(|r| r.track_id.clone()).collect::<Vec<_>>(),
        [
            r[0].track_ids[0].clone(),
            r[1].track_ids[0].clone(),
            r[0].track_ids[0].clone()
        ]
    );
    assert_eq!(
        l.library_queue_reader()
            .unwrap()
            .playlist_range(std::slice::from_ref(&source), &a, &c)
            .unwrap(),
        [a.clone(), b.clone(), c.clone()]
    );
    let entries = l
        .library_queue_reader()
        .unwrap()
        .resolve(
            &Target::PlaylistEntries {
                playlists: vec![source],
                entries: vec![a, c],
            },
            &Request::default(),
        )
        .unwrap();
    assert_eq!(entries.len(), 2);
    l.append_playlist_track(&destination, &r[0].track_ids[0])
        .unwrap();
    let tracks = rows.into_iter().map(|r| r.track_id).collect();
    let plan = l.prepare_playlist_append(&destination, tracks).unwrap();
    assert_eq!(plan.duplicate_entries, 2); // equal titles on other canonical identities do not count.
    assert_eq!(l.apply_playlist_append(&plan, false).unwrap(), 1);
    assert_eq!(
        l.playlist_entries(&destination, None, 200).unwrap().len(),
        2
    );
    assert_eq!(l.apply_playlist_append(&plan, false).unwrap(), 0);
    assert_eq!(l.apply_playlist_append(&plan, true).unwrap(), 3);
    assert_eq!(
        l.playlist_entries(&destination, None, 200).unwrap().len(),
        5
    );
    let single = l
        .prepare_playlist_append(&destination, vec![r[0].track_ids[0].clone()])
        .unwrap();
    assert_eq!(single.duplicate_entries, 1);
    assert_eq!(l.apply_playlist_append(&single, true).unwrap(), 1);
    assert!(
        !l.browse(&Request {
            limit: 200,
            ..Default::default()
        })
        .unwrap()
        .iter()
        .any(|row| row.id == r[0].track_ids[0].0)
    );
}
