//! Opt-in explicit reconciliation. Back up the SQLite library before persisting repairs.
use music_library::{
    album_matching::{AlbumMatcher, AutoMatchPolicy, MatchReply},
    album_program,
};
use std::{sync::mpsc, time::Duration};
enum Reply {
    Album(Box<MatchReply>),
    Program(Box<album_program::Reply>),
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let mut library = music_library::Library::open(args.get(1).ok_or("database required")?)?;
    let playlist = args.get(2).ok_or("playlist ID required")?;
    let candidates = library.playlist_local_match_candidates(playlist)?;
    println!("{} local Release candidates", candidates.len());
    let (tx, rx) = mpsc::channel();
    let albums = tx.clone();
    let mut matcher = AlbumMatcher::for_provider(
        music_library_spotify::Spotify::from_env()?,
        music_library_spotify::matching_scope(),
        move |r| {
            let _ = albums.send(Reply::Album(Box::new(r)));
        },
        move |r| {
            let _ = tx.send(Reply::Program(Box::new(r)));
        },
        |_| {},
    )?;
    matcher.after_import(&library, &candidates, AutoMatchPolicy::default())?;
    while matcher.pending_count() > 0 {
        match rx.recv_timeout(Duration::from_secs(120))? {
            Reply::Album(r) => {
                println!("Album: {:?}", r.outcome);
                matcher.complete(&mut library, *r);
            }
            Reply::Program(r) => {
                let outcome = matcher.complete_programs(&mut library, *r);
                match outcome {
                    album_program::Outcome::Complete(rows) => {
                        println!("Program completed: {} Tracks", rows.len())
                    }
                    other => println!("Program: {other:?}"),
                }
            }
        }
    }
    let mut provider = music_library_spotify::Spotify::from_env()?;
    for (release, identity) in library.playlist_reconciliation_programs(playlist)? {
        let programs =
            music_library::catalog::CatalogProvider::album_programs(&mut provider, &identity)?;
        println!(
            "Incoming program: {} -> {} entries",
            identity.external_id,
            library.reconcile_playlist_album_program(playlist, &release, &programs)?
        );
    }
    println!(
        "Repointed {} entries",
        library.reconcile_playlist_tracks(playlist)?
    );
    let remote = music_library::playback_resolver::RemoteCapability {
        provider: "spotify",
        unavailable: None,
        catalog_available: false,
        accepts: |i| i.kind == "track",
    };
    for row in library.playlist_entries(playlist, None, 200)? {
        let track = row.track.unwrap().track_id;
        println!(
            "{} {} {} {:?}",
            row.id,
            row.title,
            track.as_ref(),
            library.playback_route(&track, &remote)?
        );
    }
    Ok(())
}
