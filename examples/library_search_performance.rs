//! Warm local-search timings using the existing deterministic 200k fixture.
use music_library::{Library, library_search::Kind};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/performance/library-200k.sqlite".into());
    let library = Library::open(path)?;
    let mut queries = std::env::args().skip(2).collect::<Vec<_>>();
    if queries.is_empty() {
        queries = [
            "Artist 0001",
            "Artist 00",
            "Release 00001",
            "Song 000001 Track",
            "000001 Track",
            "Artist 000",
            "a",
        ]
        .map(str::to_owned)
        .to_vec();
    }
    for query in queries {
        library.local_search(&query, Kind::All)?;
        let mut timings = Vec::new();
        let mut count = 0;
        for _ in 0..3 {
            let start = std::time::Instant::now();
            let hits = library.local_search(&query, Kind::All)?;
            timings.push(start.elapsed());
            count = hits.len();
        }
        timings.sort();
        println!(
            "{query:?}: {:.1} ms median, {count} rows",
            timings[1].as_secs_f64() * 1000.
        );
    }
    if let Some(hit) = library.local_search("Song 199999", Kind::Song)?.first() {
        use music_library::browse::{Pane, Request};
        for (pane, id) in [
            (Pane::Artists, hit.artist_id.as_ref().map(|id| id.as_ref())),
            (Pane::Albums, hit.album_id.as_ref().map(|id| id.as_ref())),
            (Pane::Songs, Some(hit.id.as_str())),
        ] {
            if let Some(id) = id {
                let request = Request {
                    pane,
                    artist: if pane == Pane::Albums {
                        hit.artist_id.clone()
                    } else {
                        None
                    },
                    album: if pane == Pane::Songs {
                        hit.album_id.clone()
                    } else {
                        None
                    },
                    limit: 200,
                    ..Default::default()
                };
                let start = std::time::Instant::now();
                let rows = library.browse_around(&request, id)?;
                println!(
                    "Navigate {pane:?}: {:.1} ms, {} rows",
                    start.elapsed().as_secs_f64() * 1000.,
                    rows.len()
                );
            }
        }
    }
    Ok(())
}
