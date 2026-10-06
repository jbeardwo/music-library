//! Explicit bounded searches for selected canonical Tracks; read-only unless --apply or --cache-only is supplied.
use music_library::{
    Library,
    domain::TrackId,
    song_resolution::{SongSearch, evaluate},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let db = args.next().ok_or("DATABASE TRACK_ID...")?;
    let mut l = Library::open(db)?;
    let mut ids: Vec<_> = args.collect();
    let cache_only = ids.first().is_some_and(|v| v == "--cache-only");
    if cache_only {
        ids.remove(0);
    }
    let apply = ids.first().is_some_and(|v| v == "--apply");
    if apply {
        ids.remove(0);
    }
    let mut provider = music_library_spotify::Spotify::from_env()?;
    for id in ids.into_iter().take(30) {
        let input = l.song_resolution_input(&TrackId(id))?;
        let page = provider.search_songs(&input)?;
        if cache_only {
            l.persist_spotify_song_review(&input, &page)?;
        }
        let trusted = l.list_track_external_identities(&input.track_id)?;
        let application = if apply {
            Some(
                l.apply_song_evaluation(&input, &page)
                    .map(|id| id.map(|id| id.external_id))
                    .map_err(|e| e.to_string()),
            )
        } else {
            None
        };
        println!(
            "{}",
            serde_json::json!({"track_id":input.track_id.as_ref(),"local_track":input.title,"local_artist":input.artist,"local_album":input.album,"evidence":input.evidence,"application":application,"candidate_count":page.items.len(),"more_candidates":page.next_offset.is_some(),"candidates":page.items.iter().enumerate().map(|(i,c)|serde_json::json!({"track":c.title,"artist":c.artist,"album":c.album,"track_id":c.identity.external_id,"artist_ids":c.artists.iter().flat_map(|a|a.identities.iter()).collect::<Vec<_>>(),"evaluation":evaluate(&input,&page,i,&trusted)})).collect::<Vec<_>>() })
        );
    }
    eprintln!("Bounded request counts: {:?}", provider.request_counts());
    Ok(())
}
