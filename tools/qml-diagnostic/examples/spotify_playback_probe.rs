//! Explicit opt-in live control; never selects a device or starts audio by itself.
use music_library::{Library, domain::SearchRequest};
use music_library_spotify::playback::{AuthorizationState, Playback, Song};
use std::{
    io::{self, BufRead, Write},
    time::{Duration, Instant},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::var_os("MUSIC_LIBRARY_DIAGNOSTIC_DATABASE")
        .ok_or("Set an isolated diagnostic database")?;
    let library = Library::open(path)?;
    let rows = library.search(&SearchRequest {
        limit: 100,
        ..Default::default()
    })?;
    let mut songs = vec![];
    for row in rows {
        let started = Instant::now();
        let ids = library.track_provider_occurrences(&row.track_id, "spotify")?;
        if let Ok(song) = Song::from_associations(&ids) {
            println!(
                "Track {}: {} — {}; application={}; URI={}; lookup+construction={:?}",
                songs.len(),
                row.artist_names,
                row.title,
                row.track_id.as_ref(),
                song.uri(),
                started.elapsed()
            );
            songs.push(song);
        }
    }
    let mut p = Playback::from_env()?;
    if p.snapshot.authorization != AuthorizationState::Connected {
        let auth = p.begin_authorization()?;
        println!(
            "Open this authorization URL (no tokens):\n{}",
            p.snapshot.authorization_url.as_ref().unwrap()
        );
        io::stdout().flush()?;
        while !p.finish_authorization(&auth)? {
            std::thread::sleep(Duration::from_millis(100));
        }
        println!(
            "Connected; authorization stored separately from library. Token HTTP={}ms status={:?}",
            p.snapshot.token_http_ms, p.snapshot.last_http_status
        );
    } else {
        println!("Reloaded playback authorization; no device restored.");
    }
    p.devices()?;
    for d in &p.snapshot.devices {
        println!(
            "Device: id={:?} name={:?} type={} active={} restricted={} selectable={}",
            d.id,
            d.name,
            d.kind,
            d.is_active,
            d.is_restricted,
            d.id.is_some() && !d.is_restricted
        );
    }
    println!(
        "Commands: device ID | play TRACK_INDEX | pause | seek MS | state | devices | quit. Explicit device required."
    );
    for line in io::stdin().lock().lines() {
        let line = line?;
        let (command, value) = line.split_once(' ').unwrap_or((&line, ""));
        let started = Instant::now();
        let result = match command {
            "device" => p.select_device(value),
            "play" => p.play(songs.get(value.parse::<usize>()?).ok_or("Unknown Track")?),
            "pause" => p.pause(),
            "seek" => p.seek(value.parse()?),
            "state" => p.poll(),
            "devices" => p.devices(),
            "quit" => break,
            _ => {
                println!("Unknown command");
                continue;
            }
        };
        println!(
            "{command}: {result:?}; elapsed={:?}; token_requests={}; playback_requests={}; observed={:?}",
            started.elapsed(),
            p.snapshot.token_requests,
            p.snapshot.api_requests,
            p.snapshot.state
        );
        io::stdout().flush()?;
    }
    Ok(())
}
