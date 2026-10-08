//! Measurements on an explicitly supplied disposable copy of the deterministic fixture.
use music_library::{
    Library,
    metadata::{Change, Target},
};
use rusqlite::Connection;
use std::{hint::black_box, time::Instant};
fn measure(name: &str, n: usize, mut operation: impl FnMut()) {
    let mut times = Vec::new();
    for _ in 0..n {
        let start = Instant::now();
        operation();
        times.push(start.elapsed().as_secs_f64() * 1000.);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "{name}: median {:.3} ms, p95 {:.3} ms",
        times[n / 2],
        times[(n * 95 / 100).min(n - 1)]
    );
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .expect("pass a disposable database copy");
    let mut l = Library::open(&path)?;
    let db = Connection::open(&path)?;
    let count: i64 = db.query_row("SELECT count(*) FROM track", [], |r| r.get(0))?;
    let (track,album):(String,String)=db.query_row("SELECT t.id,r.album_id FROM track t JOIN release r ON r.id=t.release_id ORDER BY t.id LIMIT 1",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
    println!(
        "{count} Tracks; target Album {} Tracks",
        l.inspect_metadata(&Target::Album(album.clone()))?
            .track_count
    );
    let t = Target::Track(track);
    let a = Target::Album(album);
    measure("open Track", 100, || {
        black_box(l.inspect_metadata(&t).unwrap());
    });
    measure("open Album", 100, || {
        black_box(l.inspect_metadata(&a).unwrap());
    });
    let before: i64 = db.query_row("SELECT count(*) FROM track_search", [], |r| r.get(0))?;
    let mut n = 0;
    measure("save Track title + targeted FTS", 30, || {
        n += 1;
        black_box(
            l.save_metadata(
                &t,
                &[Change {
                    field: "title".into(),
                    value: Some(format!("Metadata measurement {n}")),
                }],
                &[],
            )
            .unwrap(),
        );
    });
    measure("save Album title + affected FTS", 30, || {
        n += 1;
        black_box(
            l.save_metadata(
                &a,
                &[Change {
                    field: "title".into(),
                    value: Some(format!("Album measurement {n}")),
                }],
                &[],
            )
            .unwrap(),
        );
    });
    measure("canonical Artist assignment + targeted refresh", 30, || {
        n += 1;
        l.save_metadata(
            &a,
            &[Change {
                field: "artist_assignment".into(),
                value: Some(format!("Artist assignment {n}")),
            }],
            &[],
        )
        .unwrap();
    });
    measure("Artist lookup", 100, || {
        black_box(l.metadata_artist_choices("Artist assignment").unwrap());
    });
    assert_eq!(
        before,
        db.query_row("SELECT count(*) FROM track_search", [], |r| r
            .get::<_, i64>(0))?
    );
    for sql in [
        "EXPLAIN QUERY PLAN SELECT t.id FROM release r JOIN track t ON t.release_id=r.id WHERE r.album_id='target'",
        "EXPLAIN QUERY PLAN SELECT ts.source_id FROM track_source ts WHERE ts.track_id='target'",
        "EXPLAIN QUERY PLAN SELECT * FROM track_metadata_override WHERE track_id='target'",
    ] {
        for row in db.prepare(sql)?.query_map([], |r| r.get::<_, String>(3))? {
            println!("{}", row?);
        }
    }
    Ok(())
}
