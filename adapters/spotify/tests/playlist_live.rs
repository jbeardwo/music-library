//! Explicit opt-in only: user-authorized remote reads and a disposable local snapshot.
use music_library::{
    Library,
    browse::{Pane, Request},
    playback_resolver::{RemoteCapability, Route},
    playlist_import::Outcome,
};
use music_library_spotify::{
    playback::Playback,
    playlists::{Error, parse_playlist},
};
#[test]
#[ignore = "requires connected Spotify user authorization with playlist-read scopes"]
fn live_spotify_playlist_snapshot() {
    let mut spotify =
        Playback::from_env().expect("Configure SPOTIFY_CLIENT_ID and existing user authorization");
    if spotify.require_playlist_scopes().is_err()
        && std::env::var("MUSIC_LIBRARY_SPOTIFY_TEST_AUTHORIZE").as_deref() == Ok("1")
    {
        let authorization = spotify
            .begin_authorization()
            .expect("Start existing PKCE flow");
        println!(
            "Authorize playlist read access: {}",
            spotify.snapshot.authorization_url.as_ref().unwrap()
        );
        loop {
            if spotify
                .finish_authorization(&authorization)
                .expect("Complete PKCE authorization")
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
    spotify.require_playlist_scopes().expect("Reconnect Spotify in Import from Spotify to grant playlist-read-private and playlist-read-collaborative");
    let input = std::env::var("MUSIC_LIBRARY_SPOTIFY_TEST_PLAYLIST").ok();
    let plan = if let Some(input) = input {
        parse_playlist(&input).unwrap();
        spotify.fetch_playlist(&input, &mut |_| {}).unwrap()
    } else {
        let playlists = spotify.account_playlists().unwrap();
        let mut selected = None;
        for playlist in playlists {
            match spotify.fetch_playlist(&playlist.id, &mut |_| {}) {
                Ok(plan) => {
                    selected = Some(plan);
                    break;
                }
                Err(Error::AccessDenied | Error::NotFound) => continue,
                Err(error) => panic!("{error}"),
            }
        }
        selected.expect("No account playlist is accessible; set MUSIC_LIBRARY_SPOTIFY_TEST_PLAYLIST to a playlist accessible with the connected account")
    };
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("snapshot.sqlite");
    let mut library = Library::open(&path).unwrap();
    let Outcome::Imported {
        playlist_id,
        entries,
    } = library.import_playlist_snapshot(&plan).unwrap()
    else {
        panic!()
    };
    assert_eq!(entries, plan.items.len());
    for pane in [Pane::Artists, Pane::Genres, Pane::Albums, Pane::Songs] {
        assert!(
            library
                .browse(&Request {
                    pane,
                    limit: 200,
                    ..Default::default()
                })
                .unwrap()
                .is_empty()
        );
    }
    let queue = library
        .library_queue_reader()
        .unwrap()
        .read_playlist(&playlist_id, None)
        .unwrap()
        .0;
    assert_eq!(queue.len(), entries);
    for (track, item) in queue.iter().zip(&plan.items) {
        assert_eq!(track.title, item.title);
        assert_eq!(
            library
                .track_provider_occurrences(&track.track_id, "spotify")
                .unwrap(),
            vec![item.identity.clone()]
        );
        assert_eq!(
            library
                .playback_route(
                    &track.track_id,
                    &RemoteCapability {
                        provider: "spotify",
                        unavailable: None,
                        catalog_available: false,
                        accepts: |i| i.kind == "track"
                    }
                )
                .unwrap(),
            Route::Remote(item.identity.clone())
        );
    }
    let details = library.playlist_details(&playlist_id).unwrap().unwrap();
    drop(spotify);
    drop(library);
    // This phase performs no remote requests at all, equivalent to offline catalog access.
    let mut library = Library::open(&path).unwrap();
    assert_eq!(
        library.playlist_details(&playlist_id).unwrap(),
        Some(details.clone())
    );
    let restored = library
        .library_queue_reader()
        .unwrap()
        .read_playlist(&playlist_id, None)
        .unwrap()
        .0;
    assert_eq!(
        restored.iter().map(|r| &r.track_id).collect::<Vec<_>>(),
        queue.iter().map(|r| &r.track_id).collect::<Vec<_>>()
    );
    assert!(matches!(
        library.import_playlist_snapshot(&plan).unwrap(),
        Outcome::AlreadyImported { .. }
    ));
    println!(
        "Validated {:?}: {} entries, {} unsupported and {} unavailable skipped; duration {} ms ({} unknown); restart/offline metadata and zero Library memberships",
        plan.name,
        entries,
        plan.unsupported,
        plan.unavailable,
        details.known_duration_ms,
        details.unknown_duration_count
    );
}
