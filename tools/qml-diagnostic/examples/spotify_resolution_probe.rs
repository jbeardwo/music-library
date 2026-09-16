//! Opt-in catalog-source fixture and explicit song search. Never auto-confirms.
use music_library::{
    Library,
    catalog::CatalogSession,
    domain::TrackId,
    song_resolution::{Selection, SongSearch},
};
use std::io::{self, BufRead};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mode = args
        .next()
        .ok_or("--add-musicbrainz ARTIST ALBUM or --track APPLICATION_TRACK_ID")?;
    let database = std::env::var_os("MUSIC_LIBRARY_DIAGNOSTIC_DATABASE")
        .ok_or("Set disposable diagnostic database")?;
    let mut library = Library::open(database)?;
    if mode == "--add-musicbrainz" {
        let artist = args.next().ok_or("ARTIST required")?;
        let title = args.next().ok_or("ALBUM required")?;
        let mut provider = CatalogSession::new(music_library_musicbrainz::MusicBrainz::new());
        let albums = provider.search_albums(&title, 0)?;
        let matches: Vec<_> = albums
            .items
            .into_iter()
            .filter(|a| {
                a.title.eq_ignore_ascii_case(&title) && a.artist.eq_ignore_ascii_case(&artist)
            })
            .collect();
        if matches.len() != 1 {
            return Err("No unique exact catalog Album in bounded page".into());
        }
        let release = provider.add_album(&matches[0])?;
        let imported = library.add_catalog_release(&release)?;
        for track in imported.track_ids {
            let input = library.song_resolution_input(&track)?;
            println!(
                "Catalog Track: {} — {} | {} | Spotify association={:?}",
                input.artist,
                input.title,
                track.as_ref(),
                library.track_provider_occurrences(&track, "spotify")?
            );
        }
        return Ok(());
    }
    if mode != "--track" && mode != "--inspect" {
        return Err("Unknown mode".into());
    }
    let track = TrackId(args.next().ok_or("Track ID required")?);
    let input = library.song_resolution_input(&track)?;
    let existing = library.track_provider_occurrences(&track, "spotify")?;
    println!(
        "Application: {} — {} | {} | existing={existing:?}",
        input.artist, input.title, input.album
    );
    if mode == "--inspect" {
        println!("Read-only inspection; zero catalog requests");
        return Ok(());
    }
    if !existing.is_empty() {
        println!("Already associated; zero catalog requests");
        return Ok(());
    }
    let mut client = music_library_spotify::Spotify::from_env()?;
    let started = std::time::Instant::now();
    let page = client.search_songs(&input)?;
    println!(
        "Catalog token/API counts={:?}; elapsed={:?}; more={}",
        client.request_counts(),
        started.elapsed(),
        page.next_offset.is_some()
    );
    for (index, c) in page.items.iter().enumerate() {
        println!(
            "{index}: {} — {} | {} ({}) | {} ms | {}:{} | {}",
            c.artist,
            c.title,
            c.album,
            c.date,
            c.duration_ms,
            c.disc,
            c.number,
            c.identity.external_id
        );
    }
    let selection = Selection::new(input, page.items);
    println!("Explicitly confirm INDEX or cancel (no automatic selection):");
    if let Some(Ok(line)) = io::stdin().lock().lines().next()
        && let Ok(index) = line.parse::<usize>()
    {
        println!(
            "Accepted occurrence: {:?}",
            library.confirm_song_resolution(&selection, index)?
        );
    } else {
        println!("Canceled; no identity written");
    }
    Ok(())
}
