//! Opt-in, one-Track live resolver audit. Requires an explicit device ID for remote audio.
use music_library::{
    Library,
    domain::TrackId,
    playback_resolver::{RemoteCapability, Route},
    song_resolution::{Assessment, Selection, SongSearch, assess},
};
use music_library_spotify::{
    Spotify,
    playback::{AuthorizationState, Playback, Song},
};
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let track = TrackId(
        args.next()
            .ok_or("APPLICATION_TRACK_ID [EXPLICIT_DEVICE_ID]")?,
    );
    let device = args.next();
    let db =
        std::env::var_os("MUSIC_LIBRARY_DIAGNOSTIC_DATABASE").ok_or("Set disposable database")?;
    let mut library = Library::open(db)?;
    let input = library.song_resolution_input(&track)?;
    println!(
        "Track: {} — {} [{}]; application={}",
        input.artist,
        input.title,
        input.album,
        track.as_ref()
    );
    let mut capability = RemoteCapability {
        provider: "spotify",
        unavailable: Some("remote not initialized".into()),
        catalog_available: true,
        accepts: |id| Song::from_associations(std::slice::from_ref(id)).is_ok(),
    };
    let started = Instant::now();
    let local = library.playback_route(&track, &capability)?;
    println!(
        "Local source lookup + filesystem verification: {:?}",
        started.elapsed()
    );
    if let Route::Local(source) = local {
        play_local(source)?;
        println!(
            "Catalog token/API=0/0; playback token/API=0/0 (remote clients never constructed)"
        );
        return Ok(());
    }
    let device =
        device.ok_or("No usable local source; pass an explicitly selected Spotify device ID")?;
    let mut playback = Playback::from_env()?;
    if playback.snapshot.authorization != AuthorizationState::Connected {
        return Err("Connect playback through the UI first".into());
    }
    playback.devices()?;
    for d in &playback.snapshot.devices {
        println!(
            "Device: {:?} name={} type={} active={} restricted={}",
            d.id, d.name, d.kind, d.is_active, d.is_restricted
        );
    }
    playback.select_device(&device)?;
    capability.unavailable = None;
    let started = Instant::now();
    let route = library.playback_route(&track, &capability)?;
    println!(
        "Route decision including persisted association lookup: {:?}",
        started.elapsed()
    );
    let mut catalog_counts = (0, 0);
    let identity = match route {
        Route::Remote(id) => id,
        Route::NeedsEnrichment => {
            let mut catalog = Spotify::from_env()?;
            let started = Instant::now();
            let page = catalog.search_songs(&input)?;
            catalog_counts = catalog.request_counts();
            println!(
                "Explicit Play catalog lookup: {:?}; token/API={catalog_counts:?}; candidates={} incomplete={}",
                started.elapsed(),
                page.items.len(),
                page.next_offset.is_some()
            );
            let assessment = assess(&input, &page);
            println!("Assessment: {assessment:?}");
            for c in &page.items {
                println!(
                    "Candidate: {} — {} [{}] disc={} track={} duration={} id={}",
                    c.artist,
                    c.title,
                    c.album,
                    c.disc,
                    c.number,
                    c.duration_ms,
                    c.identity.external_id
                );
            }
            let Assessment::Unique(index) = assessment else {
                println!(
                    "No automatic write/play; use the existing chooser if selection is required"
                );
                return Ok(());
            };
            let started = Instant::now();
            let id = library.confirm_song_resolution(&Selection::new(input, page.items), index)?;
            println!("Persist accepted song only: {:?}", started.elapsed());
            id
        }
        other => return Err(format!("Route changed: {other:?}").into()),
    };
    let song = Song::from_associations(&[identity])?;
    let started = Instant::now();
    playback.play(&song)?;
    println!(
        "Play {}: {:?}; HTTP={:?}",
        song.uri(),
        started.elapsed(),
        playback.snapshot.last_http_status
    );
    for _ in 0..5 {
        std::thread::sleep(std::time::Duration::from_secs(1));
        playback.poll()?;
        if playback.snapshot.state.playing
            && playback
                .snapshot
                .state
                .track_id
                .as_deref()
                .is_some_and(|id| song.uri() == format!("spotify:track:{id}"))
        {
            break;
        }
    }
    println!(
        "Observed after {:?}: {:?}",
        started.elapsed(),
        playback.snapshot.state
    );
    playback.pause()?;
    playback.poll()?;
    println!(
        "Paused: {:?}; catalog token/API={catalog_counts:?}; playback token/API={}/{}",
        playback.snapshot.state, playback.snapshot.token_requests, playback.snapshot.api_requests
    );
    Ok(())
}

#[cfg(feature = "gstreamer")]
fn play_local(
    source: music_library::domain::PlayableSource,
) -> Result<(), Box<dyn std::error::Error>> {
    use music_library::playback::{EngineEventKind, PlaybackEngine, PlaybackStatus};
    let (send, receive) = std::sync::mpsc::channel();
    let mut engine = music_library_gstreamer::GStreamerEngine::new(move |e| {
        let _ = send.send(e);
    })?;
    engine.start(&source)?;
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let event = receive.recv_timeout(deadline.saturating_duration_since(Instant::now()))?;
        match event.kind {
            EngineEventKind::State(PlaybackStatus::Playing) => break,
            EngineEventKind::Error(e) => return Err(e.into()),
            _ => {}
        }
    }
    println!("GStreamer confirmed Playing");
    std::thread::sleep(std::time::Duration::from_secs(2));
    engine.stop()?;
    Ok(())
}
#[cfg(not(feature = "gstreamer"))]
fn play_local(_: music_library::domain::PlayableSource) -> Result<(), Box<dyn std::error::Error>> {
    Err("Local route selected; rebuild probe with --features gstreamer".into())
}
