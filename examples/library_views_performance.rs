//! Bounded view checks on a disposable copy of the deterministic 200k fixture.
//! Optional --seed-genres adds deterministic local genre observations to that copy.
use music_library::{
    Library,
    browse::{Pane, Request, Sort},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .expect("pass a disposable 200k SQLite copy");
    let library = Library::open(&path)?;
    if std::env::args().any(|a| a == "--seed-genres") {
        let db = rusqlite::Connection::open(&path)?;
        db.execute_batch("PRAGMA foreign_keys=ON; BEGIN;
            INSERT OR IGNORE INTO playable_source(id,kind) SELECT 'genre-' || id,'local_file' FROM track;
            INSERT OR IGNORE INTO track_source(track_id,source_id) SELECT id,'genre-' || id FROM track;
            INSERT OR IGNORE INTO file_genre_observation(source_id,genre) SELECT 'genre-' || id,printf('Genre %02d',CAST(substr(id,7) AS INTEGER)%50) FROM track;
            COMMIT; ANALYZE;")?;
    }
    for (name, request) in [
        (
            "Genres",
            Request {
                pane: Pane::Genres,
                ..Default::default()
            },
        ),
        (
            "Albums Year",
            Request {
                pane: Pane::Albums,
                sort: Sort::Year,
                ..Default::default()
            },
        ),
        (
            "Songs A-Z",
            Request {
                sort: Sort::Title,
                ..Default::default()
            },
        ),
        (
            "Songs Z-A",
            Request {
                sort: Sort::Descending,
                ..Default::default()
            },
        ),
        (
            "Genre Albums",
            Request {
                pane: Pane::Albums,
                genre: Some("Genre 01".into()),
                sort: Sort::Year,
                ..Default::default()
            },
        ),
        (
            "Genre Songs",
            Request {
                genre: Some("Genre 01".into()),
                sort: Sort::Descending,
                ..Default::default()
            },
        ),
        (
            "Genre Album order",
            Request {
                genre: Some("Genre 01".into()),
                sort: Sort::Album,
                ..Default::default()
            },
        ),
    ] {
        for run in 0..3 {
            let request = Request {
                limit: 201,
                ..request.clone()
            };
            let start = std::time::Instant::now();
            let first = library.browse(&request)?;
            let next = library.browse(&Request {
                after: first.last().map(|r| r.cursor.clone()),
                ..request.clone()
            })?;
            assert!(first.len() <= 201 && next.len() <= 201);
            if let Some(target) = next.last().or(first.last()) {
                assert!(
                    library
                        .browse_around(&request, &target.id)?
                        .iter()
                        .any(|r| r.id == target.id)
                );
            }
            println!(
                "{name} run {run}: first={} next={} first+next+seek={:.2}ms",
                first.len(),
                next.len(),
                start.elapsed().as_secs_f64() * 1000.
            );
        }
    }
    Ok(())
}
