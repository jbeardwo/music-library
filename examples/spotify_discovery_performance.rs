//! Disposable indexed discovery-cache measurement; never contacts a provider.
use music_library::Library;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("DISPOSABLE_DATABASE")?;
    let library = Library::open(&path)?;
    let mut db = rusqlite::Connection::open(&path)?;
    let tracks: i64 = db.query_row("SELECT count(*) FROM track", [], |r| r.get(0))?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
    let tx = db.transaction()?;
    tx.execute("WITH RECURSIVE n(i) AS(SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<200000) INSERT OR REPLACE INTO spotify_discovery SELECT 'measurement:'||i,'{\"albums\":{\"items\":[],\"next\":null}}',?1 FROM n",[now])?;
    tx.commit()?;
    let mut timings = Vec::new();
    for n in 0..1000 {
        let key = format!("measurement:{}", 1 + (n * 199) % 200000);
        let start = Instant::now();
        assert!(library.spotify_discovery_response(&key, now)?.is_some());
        timings.push(start.elapsed());
    }
    timings.sort();
    println!(
        "Tracks={tracks}; cached queries=200000; lookup p50={:?} p95={:?}; provider requests=0",
        timings[500], timings[950]
    );
    println!("query plan: {}",db.query_row("EXPLAIN QUERY PLAN SELECT response_json FROM spotify_discovery WHERE fingerprint=?1 AND completed_at>?2",rusqlite::params!["measurement:1",now-2592000],|r|r.get::<_,String>(3))?);
    Ok(())
}
