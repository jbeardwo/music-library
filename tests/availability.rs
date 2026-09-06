use music_library::Library;
use music_library::domain::{
    ArtistCreditInput, ArtistId, CatalogReleaseInput, CatalogTrackInput, SearchCursor,
    SearchRequest,
};
use rusqlite::{Connection, params};
use tempfile::TempDir;

#[test]
fn availability_sources_membership_filters_and_pages() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("availability.sqlite");
    let mut library = Library::open(&path).unwrap();
    let release = library
        .create_catalog_release(&CatalogReleaseInput {
            title: "Edition".into(),
            year: None,
            artists: vec![],
            tracks: (0..6)
                .map(|_| CatalogTrackInput {
                    title: "Same Match".into(),
                    artists: vec![ArtistCreditInput {
                        name: "Performer".into(),
                        role: None,
                    }],
                    disc_number: None,
                    track_number: None,
                })
                .collect(),
        })
        .unwrap();
    for id in &release.track_ids[..5] {
        library.add_to_library(id).unwrap();
    }
    let db = Connection::open(path).unwrap();
    db.execute_batch("PRAGMA foreign_keys = ON; INSERT INTO discovery_root(id,kind,location) VALUES ('root','local_filesystem',X'2F');").unwrap();
    // No sources; unavailable; available; mixed; two available; available nonmember.
    for (source, track, available) in [
        ("a", 1, false),
        ("b", 2, true),
        ("c", 3, false),
        ("d", 3, true),
        ("e", 4, true),
        ("f", 4, true),
        ("g", 5, true),
    ] {
        db.execute(
            "INSERT INTO playable_source(id,kind) VALUES (?1,'local_file')",
            [source],
        )
        .unwrap();
        db.execute(
            "INSERT INTO track_source(track_id,source_id) VALUES (?1,?2)",
            params![release.track_ids[track].as_ref(), source],
        )
        .unwrap();
        db.execute("INSERT INTO local_file_observation(source_id,root_id,path,size_bytes,modified_ns,available) VALUES (?1,'root',CAST(?1 AS BLOB),1,1,?2)", params![source,available]).unwrap();
    }
    let artist = ArtistId(
        db.query_row("SELECT id FROM artist WHERE name='Performer'", [], |r| {
            r.get(0)
        })
        .unwrap(),
    );
    // Catalog import conservatively creates distinct Artist identities per credit.
    // Associate this fixture's credits with the selected shared Artist explicitly.
    db.execute(
        "UPDATE track_artist_credit SET artist_id=?1",
        [artist.as_ref()],
    )
    .unwrap();
    for changed in [false, true] {
        if changed {
            db.execute(
                "UPDATE local_file_observation SET available=0 WHERE source_id IN ('b','d','e')",
                [],
            )
            .unwrap();
        }
        let available_indices = if changed { vec![4] } else { vec![2, 3, 4] };
        for with_release in [false, true] {
            for with_artist in [false, true] {
                for with_fts in [false, true] {
                    for availability in [None, Some(false), Some(true)] {
                        let request = SearchRequest {
                            text: if with_fts {
                                "Match".into()
                            } else {
                                String::new()
                            },
                            release_id: with_release.then(|| release.release_id.clone()),
                            artist_id: with_artist.then(|| artist.clone()),
                            availability,
                            limit: 20,
                            after: None,
                        };
                        let mut expected: Vec<_> = (0..5)
                            .filter(|i| {
                                availability.is_none_or(|a| a == available_indices.contains(i))
                            })
                            .map(|i| release.track_ids[i].clone())
                            .collect();
                        expected.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
                        let rows = library.search(&request).unwrap();
                        assert_eq!(
                            rows.iter().map(|r| r.track_id.clone()).collect::<Vec<_>>(),
                            expected
                        );
                        for row in &rows {
                            assert_eq!(
                                row.available,
                                available_indices
                                    .iter()
                                    .any(|i| release.track_ids[*i] == row.track_id)
                            );
                        }
                        let mut paged = vec![];
                        let mut page_request = SearchRequest {
                            limit: 1,
                            ..request
                        };
                        for _ in 0..6 {
                            let page = library.search(&page_request).unwrap();
                            let Some(last) = page.last() else {
                                break;
                            };
                            page_request.after = Some(SearchCursor {
                                title: last.title.clone(),
                                track_id: last.track_id.clone(),
                            });
                            paged.extend(page);
                        }
                        assert_eq!(paged, rows);
                    }
                }
            }
        }
    }
    assert_eq!(
        db.query_row("SELECT count(*) FROM track_source", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        7
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM library_membership", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        5
    );
}
