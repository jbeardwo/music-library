//! Offline audit; run only against a disposable database copy.
use music_library::Library;
use std::time::Instant;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("disposable database path required")?;
    let l = Library::open(path)?;
    println!("roots={:?}", l.local_locations()?);
    let started = Instant::now();
    let pending = l.pending_musicbrainz_policy(256)?;
    assert!(pending.len() <= 256);
    println!(
        "MusicBrainz policy startup batch={} elapsed={:?}",
        pending.len(),
        started.elapsed()
    );
    for provider in music_library::connections::PROVIDERS {
        let now = Instant::now();
        let coverage = l.connection_coverage(provider.key)?;
        println!(
            "{} coverage={} elapsed={:?}",
            provider.key,
            serde_json::to_string(&coverage)?,
            now.elapsed()
        );
        for marked in [false, true] {
            let mut after = None;
            let mut timings = vec![];
            let mut groups = 0;
            for _ in 0..20 {
                let now = Instant::now();
                let rows = l.connection_groups(provider.key, marked, after.as_ref(), 100)?;
                timings.push(now.elapsed());
                groups += rows.len();
                if let Some(first) = rows.first() {
                    let now = Instant::now();
                    let tracks = l.connection_tracks(provider.key, marked, &first.id, None, 200)?;
                    if after.is_none() {
                        println!(
                            "{} marked={} sample={} {} matched={}/{} relevant={} tracks={} expansion={:?}",
                            provider.key,
                            marked,
                            first.artist,
                            first.title,
                            first.matched,
                            first.total,
                            first.relevant,
                            tracks.len(),
                            now.elapsed()
                        );
                    }
                }
                after = rows.last().map(|g| g.id.clone());
                if rows.len() < 100 {
                    break;
                }
            }
            timings.sort();
            println!(
                "{} marked={} groups={} page_p95={:?}",
                provider.key,
                marked,
                groups,
                timings[timings.len() * 95 / 100]
            );
        }
    }
    let c = l.connection_coverage("spotify")?;
    let unresolved = l.unresolved_spotify_count()? as i64;
    assert_eq!(c.missing - c.marked, unresolved);
    println!(
        "spotify authoritative_unresolved={unresolved} generic_unresolved={} marked={}",
        c.missing - c.marked,
        c.marked
    );
    Ok(())
}
