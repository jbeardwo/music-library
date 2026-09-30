use music_library::{
    Library,
    browse::{Request, Sort},
    domain::*,
};
#[test]
fn canonical_album_groups_survive_duplicate_titles_and_page_continuations() {
    let temp = tempfile::tempdir().unwrap();
    let mut l = Library::open(temp.path().join("db")).unwrap();
    for _ in 0..2 {
        let release = l
            .create_catalog_release(&CatalogReleaseInput {
                title: "Same album name".into(),
                year: None,
                artists: vec![],
                tracks: (0..201)
                    .map(|n| CatalogTrackInput {
                        title: format!("Song {n:03}"),
                        disc_number: Some(if n < 100 { 1 } else { 2 }),
                        track_number: Some(if n < 100 { n + 1 } else { n - 99 }),
                        artists: vec![],
                    })
                    .collect(),
            })
            .unwrap();
        for t in release.track_ids {
            l.add_to_library(&t).unwrap();
        }
    }
    let request = Request {
        sort: Sort::Album,
        album_sort: Sort::Title,
        limit: 200,
        ..Default::default()
    };
    let first = l.browse(&request).unwrap();
    assert_eq!(first.len(), 200);
    assert!(!first[0].cursor.album_key.is_empty());
    assert!(
        first
            .iter()
            .all(|r| r.cursor.album_key == first[0].cursor.album_key
                && r.track.as_ref().unwrap().release_title == "Same album name")
    );
    assert!(first[0].multi_disc);
    assert_eq!(first[100].disc_number, Some(2));
    assert_eq!(first[100].track_number, Some(1));
    let second = l
        .browse(&Request {
            after: first.last().map(|r| r.cursor.clone()),
            ..request.clone()
        })
        .unwrap();
    assert_eq!(second.len(), 200);
    assert_eq!(second[0].cursor.album_key, first[0].cursor.album_key);
    assert_ne!(second[0].cursor.album_key, second[1].cursor.album_key);
    assert_eq!(
        second[0].track.as_ref().unwrap().release_title,
        second[1].track.as_ref().unwrap().release_title
    );
    let third = l
        .browse(&Request {
            after: second.last().map(|r| r.cursor.clone()),
            ..request
        })
        .unwrap();
    assert_eq!(third.len(), 2);
    assert_eq!(third[0].cursor.album_key, second[1].cursor.album_key);
    for sort in [Sort::Title, Sort::Descending] {
        assert!(
            l.browse(&Request {
                sort,
                limit: 200,
                ..Default::default()
            })
            .unwrap()
            .iter()
            .all(|r| r.cursor.album_key.is_empty())
        );
    }
}
