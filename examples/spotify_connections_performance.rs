use music_library::{
    Library,
    browse::{Request, SongColumn},
};
use std::time::Instant;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or("target/performance/library-200k.sqlite".into());
    let l = Library::open(path)?;
    let now = Instant::now();
    let count = l.unresolved_spotify_count()?;
    println!("unresolved count={count}: {:?}", now.elapsed());
    for column in [
        SongColumn::Song,
        SongColumn::Artist,
        SongColumn::Album,
        SongColumn::Reason,
    ] {
        let mut request = Request {
            unresolved_spotify: true,
            song_column: Some(column),
            limit: 201,
            ..Default::default()
        };
        let mut timings = vec![];
        let mut total = 0;
        for page in 0..20 {
            let now = Instant::now();
            let mut rows = l.browse(&request)?;
            let elapsed = now.elapsed();
            if page < 2 {
                println!("{column:?} page {page}: {} rows {:?}", rows.len(), elapsed);
            }
            timings.push(elapsed);
            rows.truncate(200);
            total += rows.len();
            request.after = rows.last().map(|r| r.cursor.clone());
            if rows.len() < 200 {
                break;
            }
        }
        timings.sort();
        println!(
            "{column:?}: {total} rows in bounded pages, p95 {:?}",
            timings[timings.len() * 95 / 100]
        );
        let rows = l.browse(&Request {
            after: None,
            limit: 1,
            ..request.clone()
        })?;
        if let Some(row) = rows.first() {
            let now = Instant::now();
            let filtered = l.browse(&Request {
                after: None,
                album: Some(l.album_id_for_track(&music_library::domain::TrackId(row.id.clone()))?),
                ..request
            })?;
            println!(
                "{column:?} Album filter: {} rows {:?}",
                filtered.len(),
                now.elapsed()
            );
        }
    }
    Ok(())
}
