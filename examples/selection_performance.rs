//! Aggregate filters/actions on a disposable deterministic 200k database copy.
use music_library::{
    Library,
    browse::{Pane, Request, Sort},
    track_container::Target,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .expect("disposable 200k SQLite copy");
    let mut l = Library::open(&path)?;
    let db = rusqlite::Connection::open(&path)?;
    let artists=db.prepare("SELECT artist_id FROM track_artist_credit GROUP BY artist_id ORDER BY count(*) DESC,artist_id LIMIT 2")?.query_map([],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let genres = l
        .browse(&Request {
            pane: Pane::Genres,
            limit: 2,
            ..Default::default()
        })?
        .into_iter()
        .map(|r| r.id)
        .collect::<Vec<_>>();
    let albums = l
        .browse(&Request {
            pane: Pane::Albums,
            limit: 2,
            ..Default::default()
        })?
        .into_iter()
        .map(|r| r.id)
        .collect::<Vec<_>>();
    for (label, request) in [
        (
            "Artist union Songs",
            Request {
                artists: artists.clone(),
                sort: Sort::Title,
                limit: 201,
                ..Default::default()
            },
        ),
        (
            "Artist union Albums",
            Request {
                pane: Pane::Albums,
                artists: artists.clone(),
                sort: Sort::Year,
                limit: 201,
                ..Default::default()
            },
        ),
        (
            "Genre union Songs",
            Request {
                genres: genres.clone(),
                sort: Sort::Title,
                limit: 201,
                ..Default::default()
            },
        ),
        (
            "Genre Album intersection",
            Request {
                genres: genres.clone(),
                albums: albums.clone(),
                sort: Sort::Album,
                limit: 201,
                ..Default::default()
            },
        ),
    ] {
        for _ in 0..3 {
            let start = std::time::Instant::now();
            let first = l.browse(&request)?;
            let next = l.browse(&Request {
                after: first.last().map(|r| r.cursor.clone()),
                ..request.clone()
            })?;
            println!(
                "{label}: {}+{} rows {:?}",
                first.len(),
                next.len(),
                start.elapsed()
            );
        }
    }
    let reader = l.library_queue_reader()?;
    let start = std::time::Instant::now();
    let tracks = reader.resolve(
        &Target::Artists(artists.clone()),
        &Request {
            sort: Sort::Album,
            album_sort: Sort::Title,
            ..Default::default()
        },
    )?;
    println!(
        "{} Track aggregate snapshot {:?}",
        tracks.len(),
        start.elapsed()
    );
    let ids = tracks
        .iter()
        .map(|r| r.track_id.0.clone())
        .collect::<Vec<_>>();
    let start = std::time::Instant::now();
    let kept = reader.browse_ids(
        &Request {
            artists: artists.clone(),
            ..Default::default()
        },
        &ids,
    )?;
    println!("{} selected IDs pruned {:?}", kept.len(), start.elapsed());
    let p = l.create_playlist("Aggregate diagnostic")?;
    let start = std::time::Instant::now();
    let mut worker = l.playlist_append_worker()?;
    let plan = worker.prepare(&p, tracks.into_iter().map(|r| r.track_id).collect())?;
    worker.apply(&plan, true)?;
    println!(
        "{} entry batch append {:?}",
        plan.tracks.len(),
        start.elapsed()
    );
    let start = std::time::Instant::now();
    let duplicates = worker.prepare(&p, plan.tracks.clone())?;
    assert_eq!(duplicates.duplicate_entries, plan.tracks.len());
    assert_eq!(worker.apply(&duplicates, false)?, 0);
    println!("duplicate preview + skip-all {:?}", start.elapsed());
    for (label, sql, ids) in [
        (
            "Artists",
            "SELECT t.id FROM track t WHERE t.id IN (SELECT track_id FROM track_artist_credit WHERE artist_id IN (SELECT value FROM json_each(?1)))",
            artists,
        ),
        (
            "Genres",
            "SELECT ts.track_id FROM file_genre_observation g JOIN track_source ts ON ts.source_id=g.source_id WHERE g.genre IN (SELECT value FROM json_each(?1))",
            genres,
        ),
    ] {
        let mut q = db.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
        for row in q.query_map([serde_json::to_string(&ids)?], |r| r.get::<_, String>(3))? {
            println!("{label}: {}", row?);
        }
    }
    l.delete_playlist(&p)?;
    Ok(())
}
