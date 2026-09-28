use music_library::{
    Library,
    browse::{Pane, Request},
    domain::*,
    library_search::{Kind, SECTION_LIMIT},
};
use tempfile::TempDir;
struct Fixture {
    _temp: TempDir,
    library: Library,
    artist: ArtistId,
    album: AlbumId,
    tracks: Vec<TrackId>,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("search.sqlite");
        let mut library = Library::open(&path).unwrap();
        let mut added = Vec::new();
        for (artist, album, titles) in [
            (
                "Hop Along",
                "Painted Shut",
                vec!["Waitress", "The Knock", "Hidden unsaved"],
            ),
            (
                "Hop Along",
                "Bark Your Head Off, Dog",
                vec!["How Simple", "Waitress"],
            ),
            ("toe", "For Long Tomorrow", vec!["Goodbye", "Hop song"]),
            ("Unrelated", "Nothing Else", vec!["Other"]),
        ] {
            let r = library
                .create_catalog_release(&CatalogReleaseInput {
                    title: album.into(),
                    year: None,
                    artists: vec![ArtistCreditInput {
                        name: artist.into(),
                        role: None,
                    }],
                    tracks: titles
                        .iter()
                        .enumerate()
                        .map(|(i, t)| CatalogTrackInput {
                            title: (*t).into(),
                            artists: vec![],
                            disc_number: Some(1),
                            track_number: Some(i as u32 + 1),
                        })
                        .collect(),
                })
                .unwrap();
            for id in &r.track_ids {
                library.add_to_library(id).unwrap();
            }
            added.push(r);
        }
        library.remove_from_library(&added[0].track_ids[2]).unwrap();
        let album = library
            .album_for_release(&added[0].release_id)
            .unwrap()
            .album_id;
        let other = library
            .album_for_release(&added[1].release_id)
            .unwrap()
            .album_id;
        let db = rusqlite::Connection::open(path).unwrap();
        let aid = |a: &AlbumId| {
            ArtistId(
                db.query_row(
                    "SELECT artist_id FROM album_artist_credit WHERE album_id=?1",
                    [a.as_ref()],
                    |r| r.get(0),
                )
                .unwrap(),
            )
        };
        let artist = aid(&album);
        let duplicate = aid(&other);
        let identities = db
            .prepare("SELECT id FROM artist WHERE name='Hop Along'")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        drop(db);
        for id in identities {
            library.merge_artist(&ArtistId(id), &artist).unwrap();
        }
        // Fixture explicitly establishes this shared Artist; search never does it.
        library.merge_artist(&duplicate, &artist).unwrap();
        Self {
            _temp: temp,
            library,
            artist,
            album,
            tracks: added[0].track_ids.clone(),
        }
    }
}
#[test]
fn exact_prefix_case_tokens_context_and_direct_ranking() {
    let f = Fixture::new();
    for q in ["hop", "Hop Along", "HOP ALONG", "hop-along"] {
        let h = f.library.local_search(q, Kind::All).unwrap();
        assert!(
            h.iter()
                .any(|r| r.kind == Kind::Artist && r.id == f.artist.as_ref())
        );
        assert!(
            h.iter()
                .any(|r| r.kind == Kind::Album && r.title == "Painted Shut")
        );
        assert!(
            h.iter()
                .any(|r| r.kind == Kind::Album && r.title == "Bark Your Head Off, Dog")
        );
        assert!(
            h.iter()
                .any(|r| r.kind == Kind::Song && r.title == "Waitress")
        );
        assert!(!h.iter().any(|r| r.title == "Other"
            || r.title == "Nothing Else"
            || r.title == "Hidden unsaved"));
    }
    let hop = f.library.local_search("hop", Kind::Song).unwrap();
    assert_eq!(hop[0].title, "Hop song"); // direct beats contextual Hop Along songs
    assert!(hop[1..].iter().all(|h| h.score == 4));
    for q in ["painted", "Painted Shut", "painted shut"] {
        let h = f.library.local_search(q, Kind::All).unwrap();
        assert!(
            h.iter()
                .any(|h| h.kind == Kind::Album && h.id == f.album.as_ref())
        );
        assert_eq!(h.iter().filter(|h| h.kind == Kind::Song).count(), 2);
    }
    for q in ["waitress", "WAITRESS", "waitr"] {
        let h = f.library.local_search(q, Kind::Song).unwrap();
        assert_eq!(h.len(), 2);
        assert_ne!(h[0].id, h[1].id);
        assert_ne!(h[0].album_id, h[1].album_id);
        assert!(h.iter().all(|h| h.artist == "Hop Along"));
    }
    assert_eq!(
        f.library.local_search("long tom", Kind::Album).unwrap()[0].title,
        "For Long Tomorrow"
    );
    assert_eq!(
        f.library.local_search("goodbye", Kind::Song).unwrap()[0].title,
        "Goodbye"
    );
    assert!(
        f.library
            .local_search(" \" OR *", Kind::All)
            .unwrap()
            .is_empty()
    );
}
#[test]
fn filters_membership_stable_context_and_direct_browse_seek() {
    let mut f = Fixture::new();
    for kind in [Kind::Artist, Kind::Album, Kind::Song] {
        let hits = f.library.local_search("hop", kind).unwrap();
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| h.kind == kind));
    }
    assert!(
        f.library
            .local_search("hop", Kind::Playlist)
            .unwrap()
            .is_empty()
    );
    // Source-less saved Tracks remain visible, with their established Album and Artist.
    let hit = f
        .library
        .local_search("waitress", Kind::Song)
        .unwrap()
        .into_iter()
        .find(|h| h.id == f.tracks[0].as_ref())
        .unwrap();
    assert_eq!(hit.artist_id, Some(f.artist.clone()));
    assert_eq!(hit.album_id, Some(f.album.clone()));
    let rows = f
        .library
        .browse_from(
            &Request {
                album: Some(f.album),
                limit: 200,
                ..Default::default()
            },
            &hit.id,
        )
        .unwrap();
    assert_eq!(rows[0].id, hit.id);
    f.library.remove_from_library(&f.tracks[0]).unwrap();
    assert!(
        !f.library
            .local_search("waitress", Kind::Song)
            .unwrap()
            .iter()
            .any(|h| h.id == f.tracks[0].as_ref())
    );
}
#[test]
fn late_objects_and_duplicate_titles_seek_without_page_walks_and_results_are_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let mut library = Library::open(temp.path().join("late.sqlite")).unwrap();
    let mut last = None;
    for i in 0..211 {
        let r = library
            .create_catalog_release(&CatalogReleaseInput {
                title: format!("Album {i:03}"),
                year: None,
                artists: vec![ArtistCreditInput {
                    name: format!("Artist {i:03}"),
                    role: None,
                }],
                tracks: (0..if i == 210 { 211 } else { 1 })
                    .map(|n| CatalogTrackInput {
                        title: "Duplicate".into(),
                        artists: vec![],
                        disc_number: Some(1),
                        track_number: Some(n + 1),
                    })
                    .collect(),
            })
            .unwrap();
        for t in &r.track_ids {
            library.add_to_library(t).unwrap();
        }
        last = Some(r);
    }
    let last = last.unwrap();
    let artist = library
        .local_search("Artist 210", Kind::Artist)
        .unwrap()
        .remove(0);
    let album = library
        .local_search("Album 210", Kind::Album)
        .unwrap()
        .remove(0);
    assert_eq!(
        library
            .browse_from(
                &Request {
                    pane: Pane::Artists,
                    limit: 200,
                    ..Default::default()
                },
                &artist.id
            )
            .unwrap()[0]
            .id,
        artist.id
    );
    assert_eq!(
        library
            .browse_from(
                &Request {
                    pane: Pane::Albums,
                    limit: 200,
                    ..Default::default()
                },
                &album.id
            )
            .unwrap()[0]
            .id,
        album.id
    );
    let target = last.track_ids[210].as_ref();
    let rows = library
        .browse_from(
            &Request {
                album: album.album_id,
                limit: 200,
                ..Default::default()
            },
            target,
        )
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, target);
    let around = library
        .browse_around(
            &Request {
                album: Some(AlbumId(album.id.clone())),
                limit: 200,
                ..Default::default()
            },
            target,
        )
        .unwrap();
    assert_eq!(around.len(), 101);
    assert_eq!(around.last().unwrap().id, target);
    for q in ["a", "d", "artist", "album"] {
        let hits = library.local_search(q, Kind::All).unwrap();
        assert!(hits.len() <= SECTION_LIMIT * 3);
        for k in [Kind::Artist, Kind::Album, Kind::Song] {
            assert!(hits.iter().filter(|h| h.kind == k).count() <= SECTION_LIMIT);
        }
    }
}
#[test]
fn indexes_follow_metadata_changes_and_deletes() {
    let f = Fixture::new();
    let path = f._temp.path().join("search.sqlite");
    let db = rusqlite::Connection::open(path).unwrap();
    db.execute(
        "UPDATE artist SET name='Renamed Artist' WHERE id=?1",
        [f.artist.as_ref()],
    )
    .unwrap();
    db.execute(
        "UPDATE album_application_metadata SET title='Renamed Album' WHERE album_id=?1",
        [f.album.as_ref()],
    )
    .unwrap();
    assert_eq!(
        f.library
            .local_search("Renamed Artist", Kind::Artist)
            .unwrap()[0]
            .id,
        f.artist.as_ref()
    );
    assert!(
        f.library
            .local_search("Hop Along", Kind::Artist)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        f.library
            .local_search("Renamed Album", Kind::Album)
            .unwrap()[0]
            .id,
        f.album.as_ref()
    );
    assert!(
        f.library
            .local_search("Painted Shut", Kind::Album)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn navigation_window_keeps_earlier_album_tracks_and_rejects_removed_target() {
    let mut f = Fixture::new();
    let request = Request {
        album: Some(f.album),
        limit: 200,
        ..Default::default()
    };
    let rows = f
        .library
        .browse_around(&request, f.tracks[1].as_ref())
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, f.tracks[0].as_ref());
    assert_eq!(rows[1].id, f.tracks[1].as_ref());
    f.library.remove_from_library(&f.tracks[1]).unwrap();
    assert!(
        f.library
            .browse_around(&request, f.tracks[1].as_ref())
            .is_err()
    );
}

#[test]
fn unavailable_and_provider_associated_tracks_are_still_local_search_results() {
    let mut f = Fixture::new();
    let baseline = f
        .library
        .local_search("waitress", Kind::Song)
        .unwrap()
        .into_iter()
        .map(|h| h.id)
        .collect::<Vec<_>>();
    f.library
        .attach_track_external_identity(
            &f.tracks[0],
            &ExternalIdentity {
                provider: "spotify".into(),
                kind: "track".into(),
                external_id: "existing-association".into(),
            },
        )
        .unwrap();
    let db = rusqlite::Connection::open(f._temp.path().join("search.sqlite")).unwrap();
    db.execute_batch("INSERT INTO discovery_root(id,kind,location) VALUES ('missing-root','local_filesystem',X'2F'); INSERT INTO playable_source(id,kind) VALUES ('missing-source','local_file'); INSERT INTO local_file_observation(source_id,root_id,path,size_bytes,modified_ns,available) VALUES ('missing-source','missing-root',X'2F6D697373696E67',1,1,0);").unwrap();
    db.execute(
        "INSERT INTO track_source(track_id,source_id) VALUES (?1,'missing-source')",
        [f.tracks[0].as_ref()],
    )
    .unwrap();
    assert_eq!(
        f.library
            .local_search("waitress", Kind::Song)
            .unwrap()
            .into_iter()
            .map(|h| h.id)
            .collect::<Vec<_>>(),
        baseline
    );
}

#[test]
fn search_tracks_effective_metadata_and_never_exposes_unsaved_entities() {
    let mut f = Fixture::new();
    f.library
        .set_track_title_override(&f.tracks[0], "Remembered title")
        .unwrap();
    assert_eq!(
        f.library.local_search("remembered", Kind::Song).unwrap()[0].id,
        f.tracks[0].as_ref()
    );
    assert!(
        !f.library
            .local_search("waitress", Kind::Song)
            .unwrap()
            .iter()
            .any(|h| h.id == f.tracks[0].as_ref())
    );
    f.library.clear_track_title_override(&f.tracks[0]).unwrap();
    assert!(
        f.library
            .local_search("waitress", Kind::Song)
            .unwrap()
            .iter()
            .any(|h| h.id == f.tracks[0].as_ref())
    );
    f.library
        .create_catalog_release(&CatalogReleaseInput {
            title: "Unsaved Album".into(),
            year: None,
            artists: vec![ArtistCreditInput {
                name: "Unsaved Artist".into(),
                role: None,
            }],
            tracks: vec![CatalogTrackInput {
                title: "Unsaved Song".into(),
                artists: vec![],
                disc_number: None,
                track_number: None,
            }],
        })
        .unwrap();
    assert!(
        f.library
            .local_search("unsaved", Kind::All)
            .unwrap()
            .is_empty()
    );
    for id in &f.tracks {
        f.library.remove_from_library(id).unwrap();
    }
    assert!(
        f.library
            .local_search("painted", Kind::All)
            .unwrap()
            .is_empty()
    );
}
