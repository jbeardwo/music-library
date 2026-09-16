//! Opt-in, isolated live fallback validation. No credentials or tokens are printed.
#[path = "../src/local.rs"]
mod local;
use music_library::{
    album_matching::{AlbumMatcher, AutoMatchPolicy, MatchReply},
    album_program,
    provider_chain::{ProviderChain, ProviderSlot},
};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
enum Event {
    Album(Box<MatchReply>),
    Program(Box<album_program::Reply>),
    Retry(usize, u64),
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let folder = args
        .next()
        .ok_or("usage: provider_chain_probe FOLDER spotify,musicbrainz")?;
    let names = args
        .next()
        .ok_or("Specify provider order")?
        .into_string()
        .map_err(|_| "Invalid provider order")?;
    let discovery_control = match args.next() {
        None => false,
        Some(a) if a == "--discovery-control" => true,
        _ => return Err("Unknown diagnostic option".into()),
    };
    let path = std::env::var_os("MUSIC_LIBRARY_DIAGNOSTIC_DATABASE")
        .ok_or("Set a purpose-specific MUSIC_LIBRARY_DIAGNOSTIC_DATABASE")?;
    if discovery_control
        && (std::path::Path::new(&path).exists()
            || !std::path::Path::new(&path).starts_with("/tmp"))
    {
        return Err("Discovery control requires a new disposable database under /tmp".into());
    }
    let (_temp, mut library, imports) = local::load(std::path::Path::new(&folder))?;
    let albums = imports
        .iter()
        .map(|r| library.album_for_release(&r.release_id).map(|a| a.album_id))
        .collect::<Result<Vec<_>, _>>()?;
    if discovery_control {
        let mut db = rusqlite::Connection::open(&path)?;
        db.pragma_update(None, "foreign_keys", true)?;
        let tx = db.transaction()?;
        for album in &albums {
            tx.execute(
                "DELETE FROM album_external_identity WHERE album_id=?1",
                [album.as_ref()],
            )?;
        }
        tx.commit()?;
        println!(
            "DIAGNOSTIC CONTROL: suppressed accepted Album mappings in this new disposable DB; local files and durable raw observations unchanged"
        );
    }
    for a in &albums {
        println!(
            "Before network: Album {:?}; {} local Tracks; identities {:?}",
            a,
            library.local_album_tracks(a)?.len(),
            library.list_album_external_identities(a)?
        );
    }
    let (tx, rx) = mpsc::channel();
    let mut slots = vec![];
    for (i, name) in names.split(',').enumerate() {
        let a = tx.clone();
        let p = tx.clone();
        let timer = tx.clone();
        let emit = move |r| {
            let _ = a.send(Event::Album(Box::new(r)));
        };
        let programs = move |r| {
            let _ = p.send(Event::Program(Box::new(r)));
        };
        let retry = move |token| {
            let _ = timer.send(Event::Retry(i, token));
        };
        let (scope, matcher) = match name {
            "spotify" => {
                let scope = music_library_spotify::matching_scope();
                let matcher = music_library_spotify::Spotify::from_env()
                    .map_err(|e| e.to_string())
                    .and_then(|client| {
                        AlbumMatcher::for_provider(client, scope.clone(), emit, programs, retry)
                            .map_err(|e| e.to_string())
                    });
                (scope, matcher)
            }
            "musicbrainz" => {
                let scope = music_library::catalog::MatchingScope::musicbrainz();
                let matcher = AlbumMatcher::for_provider(
                    music_library_musicbrainz::MusicBrainz::new(),
                    scope.clone(),
                    emit,
                    programs,
                    retry,
                )
                .map_err(|e| e.to_string());
                (scope, matcher)
            }
            _ => return Err("Unknown configured provider".into()),
        };
        slots.push(ProviderSlot { scope, matcher });
    }
    let mut chain = ProviderChain::new(slots)?;
    let start = Instant::now();
    let mut persistence = Duration::ZERO;
    chain.after_import(&library, &imports, AutoMatchPolicy::default())?;
    while chain.is_processing() {
        match rx.recv_timeout(Duration::from_secs(180))? {
            Event::Album(r) => {
                let album = r.input.album_id.clone();
                println!("Attempt {}: {:?}", chain.provider(&album), r.outcome);
                chain.complete(&mut library, *r);
            }
            Event::Program(r) => {
                let album = r.input.album_id.clone();
                let t = Instant::now();
                let outcome = chain.complete_programs(&mut library, *r);
                persistence += t.elapsed();
                println!("Programs {}: {:?}", chain.provider(&album), outcome);
            }
            Event::Retry(i, t) => {
                chain.cooldown_provider(&library, i, t)?;
            }
        }
    }
    println!(
        "Workflow elapsed {:?}; local program completion {:?}; preserved jobs {}",
        start.elapsed(),
        persistence,
        chain.pending_count()
    );
    for a in &albums {
        println!(
            "Final provider {}: {:?}; history {:?}",
            chain.provider(a),
            chain.outcome(a),
            chain.attempts(a)
        );
    }
    let before = albums
        .iter()
        .map(|a| {
            Ok((
                library.list_album_external_identities(a)?,
                library.local_album_tracks(a)?,
                library.provider_track_associations(a, "spotify")?,
            ))
        })
        .collect::<music_library::Result<Vec<_>>>()?;
    drop(chain);
    drop(library);
    let library = music_library::Library::open(path)?;
    let after = albums
        .iter()
        .map(|a| {
            Ok((
                library.list_album_external_identities(a)?,
                library.local_album_tracks(a)?,
                library.provider_track_associations(a, "spotify")?,
            ))
        })
        .collect::<music_library::Result<Vec<_>>>()?;
    assert_eq!(before, after);
    println!("Restart reconstruction identical; no file reads or network during reconstruction");
    Ok(())
}
