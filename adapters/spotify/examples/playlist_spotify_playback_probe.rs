//! Opt-in actual Connect verification for a persisted Spotify-only playlist Track.
use music_library::domain::{ExternalIdentity, TrackId};
use music_library_spotify::playback::{Playback, Song};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let library = music_library::Library::open(args.get(1).ok_or("database required")?)?;
    let track = TrackId(args.get(2).ok_or("Track ID required")?.clone());
    let identities: Vec<ExternalIdentity> =
        library.track_provider_occurrences(&track, "spotify")?;
    let capability = music_library::playback_resolver::RemoteCapability {
        provider: "spotify",
        unavailable: None,
        catalog_available: false,
        accepts: |id| id.kind == "track",
    };
    if !matches!(
        library.playback_route(&track, &capability)?,
        music_library::playback_resolver::Route::Remote(_)
    ) {
        return Err("Track does not resolve to persisted Spotify".into());
    }
    let song = Song::from_associations(&identities)?;
    let mut playback = Playback::from_env()?;
    playback.devices()?;
    let device = playback
        .snapshot
        .devices
        .iter()
        .find(|d| d.is_active && !d.is_restricted)
        .ok_or("No active Spotify Connect device; open Spotify and try again")?;
    let id = device.id.clone().ok_or("No device ID")?;
    println!("Device: {}", device.name);
    playback.select_device(&id)?;
    playback.play_from_start(&song)?;
    std::thread::sleep(std::time::Duration::from_secs(2));
    playback.poll()?;
    println!(
        "Playing: {}; Track: {:?}",
        playback.snapshot.state.playing, playback.snapshot.state.track_id
    );
    let played = playback.snapshot.state.playing
        && identities
            .iter()
            .any(|i| Some(&i.external_id) == playback.snapshot.state.track_id.as_ref());
    playback.pause()?;
    if !played {
        return Err("Spotify state did not confirm the requested Track".into());
    }
    Ok(())
}
