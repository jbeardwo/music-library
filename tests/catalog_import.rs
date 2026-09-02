use music_library::Library;
use music_library::domain::{
    ArtistCreditInput, CatalogReleaseInput, CatalogTrackInput, SearchRequest,
};
use rusqlite::Connection;
use tempfile::TempDir;

fn credit(name: &str, role: &str) -> ArtistCreditInput {
    ArtistCreditInput {
        name: name.into(),
        role: Some(role.into()),
    }
}

#[test]
fn catalog_release_is_searchable_and_membership_is_independent_of_sources() {
    let temp = TempDir::new().unwrap();
    let database_path = temp.path().join("library.sqlite");
    let mut library = Library::open(&database_path).unwrap();

    let created = library
        .create_catalog_release(&CatalogReleaseInput {
            title: "The Catalog Edition".into(),
            year: Some(1998),
            artists: vec![credit("Release Ensemble", "primary")],
            tracks: vec![
                CatalogTrackInput {
                    title: "Source-Free Prelude".into(),
                    artists: vec![
                        credit("Catalog Artist", "primary"),
                        credit("Catalog Guest", "featured"),
                    ],
                    disc_number: Some(1),
                    track_number: Some(1),
                },
                CatalogTrackInput {
                    title: "Source-Free Finale".into(),
                    artists: vec![credit("Catalog Artist", "primary")],
                    disc_number: Some(2),
                    track_number: Some(4),
                },
            ],
        })
        .unwrap();
    assert_eq!(created.track_ids.len(), 2);

    let database = Connection::open(&database_path).unwrap();
    let source_count: i64 = database
        .query_row("SELECT count(*) FROM playable_source", [], |row| row.get(0))
        .unwrap();
    let association_count: i64 = database
        .query_row("SELECT count(*) FROM track_source", [], |row| row.get(0))
        .unwrap();
    assert_eq!(source_count, 0);
    assert_eq!(association_count, 0);

    assert!(
        library
            .search(&SearchRequest {
                text: "Catalog".into(),
                limit: 20,
                ..SearchRequest::default()
            })
            .unwrap()
            .is_empty()
    );

    for track_id in &created.track_ids {
        assert!(library.add_to_library(track_id).unwrap());
        assert!(!library.add_to_library(track_id).unwrap());
    }

    let results = library
        .search(&SearchRequest {
            text: "Catalog Artist Edition".into(),
            availability: Some(false),
            limit: 20,
            ..SearchRequest::default()
        })
        .unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].release_title, "The Catalog Edition");
    assert_eq!(results[0].year, Some(1998));
    assert!(!results[0].available);
    assert_eq!(results[1].year, Some(1998));
    assert!(!results[1].available);
    let prelude = results
        .iter()
        .find(|result| result.title == "Source-Free Prelude")
        .unwrap();
    assert_eq!(prelude.artist_names, "Catalog Artist, Catalog Guest");

    let first_track = &created.track_ids[0];
    assert!(library.remove_from_library(first_track).unwrap());
    assert_eq!(
        library
            .search(&SearchRequest {
                text: "Source-Free Prelude".into(),
                limit: 20,
                ..SearchRequest::default()
            })
            .unwrap()
            .len(),
        0
    );
    assert!(library.add_to_library(first_track).unwrap());
    assert_eq!(
        library
            .search(&SearchRequest {
                text: "Source-Free Prelude".into(),
                limit: 20,
                ..SearchRequest::default()
            })
            .unwrap()
            .len(),
        1
    );
}
