//! Local-only measurements on a disposable copy of the 200k fixture.
use music_library::{
    Library,
    browse::{Request, SongColumn},
    spotify_lifecycle::LOCAL_BATCH,
};
use std::time::Instant;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("DISPOSABLE_DATABASE")?;
    let opening = Instant::now();
    let mut l = Library::open(&path)?;
    println!("open/migration: {:?}", opening.elapsed());
    let db = rusqlite::Connection::open(&path)?;
    db.execute("INSERT OR IGNORE INTO track_provider_exclusion(track_id,provider) SELECT track_id,'spotify' FROM library_membership WHERE NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=library_membership.track_id) ORDER BY track_id LIMIT 1000",[])?;
    for marked in [false, true] {
        let mut request = Request {
            unresolved_spotify: true,
            marked_spotify: marked,
            song_column: Some(SongColumn::Song),
            limit: 201,
            ..Default::default()
        };
        for n in 0..3 {
            let t = Instant::now();
            let mut rows = l.browse(&request)?;
            println!(
                "marked={marked} page={n} rows={} elapsed={:?}",
                rows.len(),
                t.elapsed()
            );
            rows.truncate(200);
            request.after = rows.last().map(|r| r.cursor.clone());
        }
    }
    let mut scans = vec![];
    let mut batches = vec![];
    let mut processed = 0;
    for _ in 0..100 {
        let t = Instant::now();
        l.stale_spotify_tracks(LOCAL_BATCH)?;
        scans.push(t.elapsed());
        let t = Instant::now();
        let b = l.reevaluate_stale_spotify(LOCAL_BATCH)?;
        batches.push(t.elapsed());
        processed += b.examined;
    }
    scans.sort();
    batches.sort();
    println!(
        "stale scan p50 {:?} p95 {:?}; local batch (32) p50 {:?} p95 {:?}; processed={processed}; provider requests=0",
        scans[50], scans[95], batches[50], batches[95]
    );
    // Compare the replayable path separately from cheap missing-cache bookkeeping.
    let cached = l
        .stale_spotify_tracks(LOCAL_BATCH)?
        .into_iter()
        .take(8)
        .collect::<Vec<_>>();
    for track in &cached {
        let input = l.song_resolution_input(track)?;
        let candidate = music_library::song_resolution::Candidate {
            identity: music_library::domain::ExternalIdentity {
                provider: "spotify".into(),
                kind: "track".into(),
                external_id: format!("synthetic-lifecycle-{}", track.as_ref()),
            },
            album_identity: None,
            title: input.title.clone(),
            artist: input.artist.clone(),
            artists: input.artists.clone(),
            album: input.album.clone(),
            album_artists: input.album_artists.clone(),
            album_type: String::new(),
            album_total_tracks: Some(input.album_required_tracks as u32),
            date: String::new(),
            duration_ms: input.duration_ms.unwrap_or(200000),
            disc: input.disc.unwrap_or(1),
            number: input.number.unwrap_or(1),
        };
        l.persist_spotify_song_review(
            &input,
            &music_library::catalog::Page {
                items: vec![candidate],
                next_offset: None,
            },
        )?;
    }
    db.execute("UPDATE spotify_connection_review SET evaluation_version=1,stale=1 WHERE track_id IN(SELECT value FROM json_each(?1))",[serde_json::to_string(&cached)?])?;
    let t = Instant::now();
    let b = l.reevaluate_stale_spotify(LOCAL_BATCH)?;
    println!(
        "cached replay batch: {} checked, {} accepted, {:?}, provider requests=0",
        b.examined,
        b.accepted.len(),
        t.elapsed()
    );
    Ok(())
}
