//! Reuse the deterministic database_performance fixture to measure library panes.
use music_library::{
    Library,
    browse::{Pane, Request, Sort},
    domain::{AlbumId, ArtistId},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/performance/library-200k.sqlite".into());
    let library = Library::open(path)?;
    let first_album = library
        .browse(&Request {
            pane: Pane::Albums,
            limit: 1,
            ..Default::default()
        })?
        .remove(0)
        .id;
    for (name, request) in [
        (
            "Artists",
            Request {
                pane: Pane::Artists,
                ..Default::default()
            },
        ),
        (
            "Albums",
            Request {
                pane: Pane::Albums,
                ..Default::default()
            },
        ),
        ("Songs", Request::default()),
        (
            "Artist Albums",
            Request {
                pane: Pane::Albums,
                artist: Some(ArtistId("artist-0001".into())),
                ..Default::default()
            },
        ),
        (
            "Artist Songs",
            Request {
                artist: Some(ArtistId("artist-0001".into())),
                ..Default::default()
            },
        ),
        (
            "Album Songs",
            Request {
                album: Some(AlbumId(first_album.clone())),
                ..Default::default()
            },
        ),
    ] {
        let request = Request {
            limit: 201,
            ..request
        };
        let start = std::time::Instant::now();
        let rows = library.browse(&request)?;
        println!(
            "{name}: {} rows in {:.2}ms",
            rows.len(),
            start.elapsed().as_secs_f64() * 1000.
        );
        let request = Request {
            after: rows.last().map(|r| r.cursor.clone()),
            ..request
        };
        let start = std::time::Instant::now();
        let rows = library.browse(&request)?;
        println!(
            "{name} next: {} rows in {:.2}ms",
            rows.len(),
            start.elapsed().as_secs_f64() * 1000.
        );
    }
    for (pane, sort, album_sort, scoped) in [
        (Pane::Artists, Sort::Descending, Sort::Default, false),
        (Pane::Albums, Sort::Year, Sort::Default, false),
        (Pane::Albums, Sort::Artist, Sort::Default, false),
        (Pane::Albums, Sort::Title, Sort::Default, true),
        (Pane::Albums, Sort::Year, Sort::Default, true),
        (Pane::Songs, Sort::Title, Sort::Default, true),
        (Pane::Songs, Sort::Album, Sort::Year, true),
        (Pane::Songs, Sort::Album, Sort::Title, true),
    ] {
        let request = Request {
            pane,
            sort,
            album_sort,
            artist: scoped.then(|| ArtistId("artist-common".into())),
            limit: 201,
            ..Default::default()
        };
        for run in 0..3 {
            let start = std::time::Instant::now();
            let rows = library.browse(&request)?;
            let first_ms = start.elapsed().as_secs_f64() * 1000.;
            let second = library.browse(&Request {
                after: rows.last().map(|r| r.cursor.clone()),
                ..request.clone()
            })?;
            let next_ms = start.elapsed().as_secs_f64() * 1000. - first_ms;
            let target = second.last().or(rows.last()).unwrap();
            library.browse_around(&request, &target.id)?;
            println!(
                "{pane:?}/{sort:?} album-order={album_sort:?} scoped={scoped} first + next + seek run {run}: {:.2}ms (first {first_ms:.2}, next {next_ms:.2})",
                start.elapsed().as_secs_f64() * 1000.
            );
        }
    }
    let start = std::time::Instant::now();
    let rows = library.library_queue(&Request {
        artist: Some(ArtistId("artist-common".into())),
        ..Default::default()
    })?;
    println!(
        "Large Artist queue: {} Tracks in {:.2}ms",
        rows.len(),
        start.elapsed().as_secs_f64() * 1000.
    );
    let mut request = Request {
        limit: 201,
        ..Default::default()
    };
    for _ in 0..100 {
        let rows = library.browse(&request)?;
        request.after = rows.last().map(|r| r.cursor.clone());
    }
    let start = std::time::Instant::now();
    let rows = library.browse(&request)?;
    println!(
        "Songs after 20k: {} rows in {:.2}ms",
        rows.len(),
        start.elapsed().as_secs_f64() * 1000.
    );
    Ok(())
}
