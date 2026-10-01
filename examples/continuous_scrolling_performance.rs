//! Bounded bidirectional keyset scrolling over the deterministic 200k fixture.
use music_library::{
    Library,
    browse::{Request, Sort},
};
use std::{collections::HashSet, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/performance/library-200k.sqlite".into());
    let l = Library::open(&path)?;
    let db = rusqlite::Connection::open(&path)?;
    let expected: i64 =
        db.query_row("SELECT count(*) FROM library_membership", [], |r| r.get(0))?;
    for sort in [Sort::Title, Sort::Descending, Sort::Album] {
        let mut request = Request {
            sort,
            album_sort: Sort::Title,
            limit: 201,
            ..Default::default()
        };
        let mut window = vec![];
        let mut seen = HashSet::new(); // Diagnostic only: proves no duplicate or missing identity.
        let mut timings = vec![];
        let total = Instant::now();
        loop {
            let start = Instant::now();
            let mut rows = l.browse(&request)?;
            timings.push(start.elapsed().as_secs_f64() * 1000.0);
            let more = rows.len() > 200;
            rows.truncate(200);
            for row in &rows {
                assert!(seen.insert(row.id.clone()), "duplicate keyset row");
            }
            request.after = rows.last().map(|r| r.cursor.clone());
            window.append(&mut rows);
            if window.len() > 600 {
                window.drain(..window.len() - 600);
            }
            assert!(window.len() <= 600);
            if !more {
                break;
            }
        }
        assert_eq!(seen.len(), expected as usize);
        timings.sort_by(f64::total_cmp);
        println!(
            "{sort:?}: {} rows, {} fetches, p50={:.3}ms p95={:.3}ms max={:.3}ms total={:?}, materialized<=600",
            seen.len(),
            timings.len(),
            timings[timings.len() / 2],
            timings[timings.len() * 95 / 100],
            timings.last().unwrap(),
            total.elapsed()
        );
        let start = Instant::now();
        let back = l.browse_before(&Request {
            after: window.first().map(|r| r.cursor.clone()),
            ..request
        })?;
        assert_eq!(back.len(), 201);
        println!("deep reverse {sort:?}: {:?}", start.elapsed());
    }
    for (label, sql) in [
        (
            "forward titles",
            "SELECT track_id FROM effective_track_metadata INDEXED BY track_browse_title WHERE (lower(title),track_id) > ('Track 190000','track-190000') ORDER BY lower(title),track_id LIMIT 201",
        ),
        (
            "reverse titles",
            "SELECT track_id FROM effective_track_metadata INDEXED BY track_browse_title WHERE (lower(title),track_id) < ('Track 190000','track-190000') ORDER BY lower(title) DESC,track_id DESC LIMIT 201",
        ),
        (
            "reverse playlist entries",
            "SELECT id FROM playlist_entry WHERE playlist_id='probe' AND (position,id)<(190000,'entry') ORDER BY position DESC,id DESC LIMIT 201",
        ),
    ] {
        let mut q = db.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
        let plans = q
            .query_map([], |r| r.get::<_, String>(3))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        println!("{label}: {plans:?}");
        assert!(!plans.iter().any(|p| p.contains("TEMP B-TREE")));
    }
    Ok(())
}
