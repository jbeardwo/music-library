//! Local-only lifecycle audit. Use a disposable Library copy for mutation validation.
use music_library::{Library, domain::TrackId, spotify_lifecycle::LOCAL_BATCH};
use std::time::Instant;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("DATABASE [MARK_TRACK_ID]")?;
    let mut library = Library::open(&path)?;
    let before = library.unresolved_spotify_count()?;
    let marked = library.marked_spotify_count()?;
    let start = Instant::now();
    let mut examined = 0;
    let mut accepted = std::collections::HashSet::new();
    let mut retry = 0;
    let mut unresolved = 0;
    loop {
        let b = library.reevaluate_stale_spotify(LOCAL_BATCH)?;
        examined += b.examined;
        accepted.extend(b.accepted);
        retry += b.needs_retry;
        unresolved += b.still_unresolved;
        if !b.errors.is_empty() {
            return Err(b.errors.join("\n").into());
        }
        if b.examined == 0 {
            break;
        }
    }
    println!(
        "{}",
        serde_json::json!({"starting_unresolved":before,"starting_marked":marked,"locally_examined":examined,"newly_accepted":accepted.len(),"still_unresolved_evaluated":unresolved,"needs_retry_processed":retry,"ending_unresolved":library.unresolved_spotify_count()?,"ending_marked":library.marked_spotify_count()?,"automatic_provider_requests":0,"elapsed_ms":start.elapsed().as_secs_f64()*1000.0})
    );
    if let Some(track) = args.next() {
        let track = TrackId(track);
        let identities = library.diagnostic_track_identities(&track)?;
        library.mark_not_on_spotify(&track)?;
        println!(
            "Marked: unresolved={} marked={}",
            library.unresolved_spotify_count()?,
            library.marked_spotify_count()?
        );
        drop(library);
        library = Library::open(&path)?;
        assert!(library.spotify_manually_excluded(&track)?);
        assert!(!library.stale_spotify_tracks(LOCAL_BATCH)?.contains(&track));
        assert_eq!(identities, library.diagnostic_track_identities(&track)?);
        assert!(library.song_resolution_input(&track)?.spotify_excluded);
        library.check_spotify_again(&track)?;
        assert!(!library.spotify_manually_excluded(&track)?);
        assert_eq!(identities, library.diagnostic_track_identities(&track)?);
        println!(
            "Check again: unresolved={} marked={} eligible={}",
            library.unresolved_spotify_count()?,
            library.marked_spotify_count()?,
            library.stale_spotify_tracks(LOCAL_BATCH)?.contains(&track)
        );
    }
    Ok(())
}
