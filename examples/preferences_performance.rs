//! Run only against a disposable copy of the deterministic 200k fixture.
use music_library::{
    Library,
    browse::{Pane, Request},
    domain::ArtistId,
    preferences::IgnoreTarget,
};
use std::time::Instant;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).expect("disposable fixture path");
    let mut l = Library::open(&path)?;
    let db = rusqlite::Connection::open(&path)?;
    let count: i64 = db.query_row("SELECT count(*) FROM track", [], |r| r.get(0))?;
    assert!(count >= 200000);
    let artist:String=db.query_row("SELECT artist_id FROM track_artist_credit GROUP BY artist_id ORDER BY count(*) DESC LIMIT 1",[],|r|r.get(0))?;
    l.set_artist_hidden(&ArtistId(artist.clone()), true)?;
    let start = Instant::now();
    l.set_tracks_ignored(&IgnoreTarget::Artist(artist.clone()), true)?;
    println!(
        "bulk Artist ignore {:?}: {:.2} ms",
        l.ignore_counts(&IgnoreTarget::Artist(artist.clone()))?,
        start.elapsed().as_secs_f64() * 1000.
    );
    for (name, request) in [
        (
            "Artists hidden filter",
            Request {
                pane: Pane::Artists,
                omit_hidden_artists: true,
                ..Default::default()
            },
        ),
        (
            "Albums derived",
            Request {
                pane: Pane::Albums,
                ..Default::default()
            },
        ),
        (
            "Artists derived",
            Request {
                pane: Pane::Artists,
                ..Default::default()
            },
        ),
        (
            "Ignored Songs",
            Request {
                ignored_tracks_only: true,
                ..Default::default()
            },
        ),
        (
            "Hidden Artists",
            Request {
                pane: Pane::Artists,
                hidden_artists_only: true,
                ..Default::default()
            },
        ),
    ] {
        let start = Instant::now();
        let rows = l.browse(&Request {
            limit: 200,
            ..request.clone()
        })?;
        if name.ends_with("derived") {
            let targets = rows
                .iter()
                .map(|r| {
                    if request.pane == Pane::Albums {
                        IgnoreTarget::Album(r.id.clone())
                    } else {
                        IgnoreTarget::Artist(r.id.clone())
                    }
                })
                .collect::<Vec<_>>();
            l.ignored_in_targets(&targets)?;
        }
        println!(
            "{name}: {} rows {:.2} ms",
            rows.len(),
            start.elapsed().as_secs_f64() * 1000.
        );
    }
    let reader = l.library_queue_reader()?;
    let start = Instant::now();
    let all = reader.resolve(
        &music_library::track_container::Target::Artists(vec![artist.clone()]),
        &Request::default(),
    )?;
    let rows = reader.filter_generated_tracks(all, &[])?;
    println!(
        "Artist generated queue: {} rows {:.2} ms",
        rows.len(),
        start.elapsed().as_secs_f64() * 1000.
    );
    let start = Instant::now();
    let all = l.library_queue(&Request::default())?;
    let rows = reader.filter_generated_tracks(all, &[])?;
    println!(
        "Songs generated queue: {} rows {:.2} ms",
        rows.len(),
        start.elapsed().as_secs_f64() * 1000.
    );
    l.set_artist_hidden(&ArtistId(artist.clone()), false)?;
    l.set_tracks_ignored(&IgnoreTarget::Artist(artist), false)?;
    Ok(())
}
