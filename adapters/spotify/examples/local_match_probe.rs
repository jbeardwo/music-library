//! Opt-in live validation using the unchanged diagnostic importer and matcher.
#[path = "../../../tools/qml-diagnostic/src/local.rs"]
mod local;
use music_library::{
    album_matching::{AlbumMatcher, AutoMatchPolicy, MatchReply},
    album_program,
};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
enum Reply {
    Album(Box<MatchReply>),
    Program(Box<album_program::Reply>),
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let folder = std::env::args_os().nth(1).ok_or(
        "usage: local_match_probe FOLDER (set MUSIC_LIBRARY_DIAGNOSTIC_DATABASE for restart check)",
    )?;
    let (_temp, mut library, imports) = local::load(std::path::Path::new(&folder))?;
    println!(
        "Local import complete: {} Releases; no provider request yet",
        imports.len()
    );
    for imported in &imports {
        let album = library.album_for_release(&imported.release_id)?;
        println!(
            "Before network: {:?}: {} stored Spotify song associations",
            album.title,
            library
                .provider_track_associations(&album.album_id, "spotify")?
                .len()
        );
    }
    let (tx, rx) = mpsc::channel();
    let album_tx = tx.clone();
    let mut matcher = AlbumMatcher::for_provider(
        music_library_spotify::Spotify::from_env()?,
        music_library_spotify::matching_scope(),
        move |r| {
            let _ = album_tx.send(Reply::Album(Box::new(r)));
        },
        move |r| {
            let _ = tx.send(Reply::Program(Box::new(r)));
        },
        |_| {},
    )?;
    let start = Instant::now();
    matcher.after_import(&library, &imports, AutoMatchPolicy::default())?;
    while matcher.pending_count() > 0 {
        match rx.recv_timeout(Duration::from_secs(120))? {
            Reply::Album(reply) => {
                println!(
                    "Album result: {:?}; provider metadata: {:?}",
                    reply.outcome, reply.matched_album
                );
                if matches!(
                    reply.outcome,
                    music_library::album_matching::MatchOutcome::Error(_)
                        | music_library::album_matching::MatchOutcome::Deferred(_)
                ) {
                    println!("Stopping live validation on provider error; no further requests");
                    return Ok(());
                }
                matcher.complete(&mut library, *reply);
            }
            Reply::Program(reply) => {
                if let Err(error) = &reply.result {
                    println!("Provider error: {error}; stopping without retries");
                    return Ok(());
                }
                let outcome = matcher.complete_programs(&mut library, *reply);
                if let album_program::Outcome::Complete(rows) = outcome {
                    for (local, result) in rows {
                        println!("Local {:?} -> {:?}", local.evidence.title, result);
                    }
                } else {
                    println!("Program result: {outcome:?}");
                }
            }
        }
    }
    println!("Provider workflow elapsed: {:?}", start.elapsed());
    for imported in &imports {
        let album = library.album_for_release(&imported.release_id)?;
        if let Some(programs) = matcher.cached_programs(&album.album_id) {
            for program in &programs.programs {
                println!(
                    "Cached provider program: {} Tracks; complete={}",
                    program.tracks.len(),
                    program.complete
                );
                for t in &program.tracks {
                    println!(
                        "Provider {:?}/{:?} {:?} {:?}ms {:?}",
                        t.disc, t.number, t.title, t.duration_ms, t.identities
                    );
                }
            }
        }
    }
    let albums: Vec<_> = imports
        .iter()
        .map(|r| library.album_for_release(&r.release_id).map(|a| a.album_id))
        .collect::<Result<_, _>>()?;
    let before: Vec<_> = albums
        .iter()
        .map(|a| library.provider_track_associations(a, "spotify"))
        .collect::<Result<_, _>>()?;
    drop(matcher);
    drop(library);
    if let Some(path) = std::env::var_os("MUSIC_LIBRARY_DIAGNOSTIC_DATABASE") {
        let library = music_library::Library::open(path)?;
        let after: Vec<_> = albums
            .iter()
            .map(|a| library.provider_track_associations(a, "spotify"))
            .collect::<Result<_, _>>()?;
        assert_eq!(before, after);
        println!(
            "Restart check: {} stored song associations identical; no file reads or provider requests",
            after.iter().map(Vec::len).sum::<usize>()
        );
    }
    Ok(())
}
