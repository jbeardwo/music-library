//! Disposable catalog-import diagnosis using the production candidate rules.
use music_library::{
    Library, album_candidates,
    album_matching::{self, MatchReply, Preparation},
    album_program,
    catalog::{CatalogProvider, CatalogSession},
    domain::*,
    edition::LocalTrackEvidence,
};
use std::time::Instant;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let artist = args.next().ok_or("ARTIST ALBUM")?;
    let title = args.next().ok_or("ALBUM")?;
    let worker = args.next().as_deref() == Some("--worker");
    let path = std::path::PathBuf::from(
        std::env::var_os("MUSIC_LIBRARY_DIAGNOSTIC_DATABASE").ok_or("Set new /tmp database")?,
    );
    if !path.starts_with("/tmp") || path.exists() {
        return Err("Requires a fresh disposable /tmp database".into());
    }
    let mut mb = CatalogSession::new(music_library_musicbrainz::MusicBrainz::new());
    let page = mb.search_albums(
        &format!("releasegroup:\"{title}\" AND artist:\"{artist}\""),
        0,
    )?;
    println!("IMPORT CANDIDATES {page:?}");
    let candidates: Vec<_> = page
        .items
        .iter()
        .filter(|a| {
            a.title.eq_ignore_ascii_case(&title)
                && a.artist.eq_ignore_ascii_case(&artist)
                && a.primary_type == "Album"
                && a.secondary_types.is_empty()
        })
        .collect();
    let [candidate] = candidates.as_slice() else {
        return Err("No unique catalog Album".into());
    };
    let release = mb.add_album(candidate)?;
    let mut library = Library::open(&path)?;
    let imported = library.add_catalog_release(&release)?;
    let album = library.album_for_release(&imported.release_id)?;
    let scope = music_library_spotify::matching_scope();
    println!(
        "APPLICATION artist={artist:?} album={:?} Tracks={}",
        album.title,
        imported.track_ids.len()
    );
    let preparation = library.prepare_album_match_for(&album.album_id, &scope)?;
    println!("Production preparation: {preparation:?}");
    let mut evidence = vec![];
    let db = rusqlite::Connection::open(&path)?;
    for track in &imported.track_ids {
        let input = library.song_resolution_input(track)?;
        println!("APPLICATION TRACK {input:?}");
        let recording_id = db.query_row(
            "SELECT recording_id FROM track WHERE id=?1",
            [track.as_ref()],
            |r| r.get::<_, String>(0),
        )?;
        evidence.push(LocalTrackEvidence {
            track_id: track.clone(),
            recording_id: RecordingId(recording_id),
            evidence: music_library::edition::TrackEvidence {
                title: Some(input.title),
                disc: input.disc,
                number: input.number,
                duration_ms: input.duration_ms,
                artists: vec![music_library::edition::ArtistEvidence {
                    name: input.artist,
                    ..Default::default()
                }],
                ..Default::default()
            },
        });
    }
    drop(db);
    let actual = library.local_album_tracks(&album.album_id)?;
    println!("Production program input contains {} Tracks", actual.len());
    if !actual.is_empty() {
        evidence = actual;
    }
    if worker {
        run_worker(&mut library, &imported)?;
    } else {
        let mut spotify = music_library_spotify::Spotify::from_env()?;
        let artists = spotify.search_artists(&artist)?;
        println!("ARTISTS {artists:?}");
        let identity = album_matching::resolve_artist(&artist, &artists)
            .map_err(|e| format!("Artist: {e:?}"))?;
        let page = spotify.artist_albums(&identity, &title)?;
        println!("ALBUMS {page:?}");
        let mut cache = vec![];
        let started = Instant::now();
        let outcome = album_candidates::resolve(
            &mut spotify,
            &title,
            &identity,
            &page,
            false,
            &evidence,
            &mut cache,
        )?;
        println!(
            "Candidate result {outcome:?}; elapsed={:?}",
            started.elapsed()
        );
        if let album_matching::MatchOutcome::Matched(id)
        | album_matching::MatchOutcome::MatchedClose(id) = &outcome
            && !cache.iter().any(|p| &p.album == id)
        {
            cache.push(spotify.album_programs(id)?);
        }
        for p in &cache {
            println!(
                "PROGRAM {} fit={:?}",
                p.album.external_id,
                album_candidates::fit(&evidence, p)
            );
            for program in &p.programs {
                println!("complete={}", program.complete);
                for t in &program.tracks {
                    println!(
                        "TRACK {:?}/{:?} {:?} {:?}ms {:?}",
                        t.disc, t.number, t.title, t.duration_ms, t.identities
                    );
                }
            }
            for (local, result) in evidence
                .iter()
                .zip(album_program::compare_album(&evidence, p))
            {
                println!("COMPARISON {:?}: {result:?}", local.evidence.title);
                for program in &p.programs {
                    for (i, t) in program.tracks.iter().enumerate().filter(|(_, t)| {
                        t.number == local.evidence.number && t.disc == local.evidence.disc
                    }) {
                        println!(
                            "POSITION CHECK {:?}: {:?}; provider Artists {:?}",
                            local.evidence.title,
                            album_program::inspect_candidate(local, t, i),
                            t.artists
                        );
                    }
                }
            }
        }
        println!(
            "Spotify token/catalog HTTP counts={:?}",
            spotify.request_counts()
        );
        if let Preparation::Ready(input) = preparation {
            let started = Instant::now();
            let outcome = library.complete_album_match_for(
                MatchReply {
                    input,
                    artist: Some(identity),
                    outcome,
                    matched_album: None,
                },
                &scope,
            )?;
            println!(
                "Persistence outcome={outcome:?}; elapsed={:?}",
                started.elapsed()
            );
            if let album_matching::MatchOutcome::Matched(id)
            | album_matching::MatchOutcome::MatchedClose(id) = outcome
                && let Some(input) = library.prepare_album_program(&album.album_id, &id)?
            {
                let programs = cache.into_iter().find(|p| p.album == id).unwrap();
                let started = Instant::now();
                println!(
                    "Program persistence {:?}",
                    library.complete_album_program(album_program::Reply {
                        input,
                        result: Ok(programs)
                    })?
                );
                println!("Program persistence elapsed={:?}", started.elapsed());
            }
        }
    }
    let before = imported
        .track_ids
        .iter()
        .map(|t| library.track_provider_occurrences(t, "spotify"))
        .collect::<music_library::Result<Vec<_>>>()?;
    println!(
        "Album identities {:?}",
        library.list_album_external_identities(&album.album_id)?
    );
    drop(library);
    let library = Library::open(&path)?;
    let mut count = 0;
    for (track, expected) in imported.track_ids.iter().zip(before) {
        let actual = library.track_provider_occurrences(track, "spotify")?;
        assert_eq!(actual, expected);
        count += usize::from(!actual.is_empty());
        println!(
            "REOPEN {} {:?} {:?}",
            track.as_ref(),
            library.song_resolution_input(track)?.title,
            actual
        );
        let capability = music_library::playback_resolver::RemoteCapability {
            provider: "spotify",
            unavailable: None,
            catalog_available: true,
            accepts: |id| {
                music_library_spotify::playback::Song::from_associations(std::slice::from_ref(id))
                    .is_ok()
            },
        };
        println!(
            "REOPEN ROUTE {:?}",
            library.playback_route(track, &capability)?
        );
    }
    println!(
        "Persisted {count}/{}; reopen and routing catalog HTTP=0",
        imported.track_ids.len()
    );
    Ok(())
}

struct Counted(music_library_spotify::Spotify);
impl Drop for Counted {
    fn drop(&mut self) {
        println!(
            "WORKER Spotify token/catalog HTTP counts={:?}",
            self.0.request_counts()
        );
    }
}
impl CatalogProvider for Counted {
    fn album_candidate_programs(&self) -> bool {
        self.0.album_candidate_programs()
    }
    fn album_candidate_track_count(&self, id: &ExternalIdentity) -> Option<u32> {
        self.0.album_candidate_track_count(id)
    }
    fn album_program_namespaces(&self) -> Vec<(String, String)> {
        self.0.album_program_namespaces()
    }
    fn album_programs(
        &mut self,
        id: &ExternalIdentity,
    ) -> Result<album_program::Programs, music_library::catalog::CatalogError> {
        self.0.album_programs(id)
    }
    fn search_artists(
        &mut self,
        name: &str,
    ) -> Result<
        music_library::catalog::Page<music_library::catalog::ArtistCandidate>,
        music_library::catalog::CatalogError,
    > {
        self.0.search_artists(name)
    }
    fn artist_albums(
        &mut self,
        id: &ExternalIdentity,
        title: &str,
    ) -> Result<
        music_library::catalog::Page<music_library::catalog::ArtistAlbumCandidate>,
        music_library::catalog::CatalogError,
    > {
        self.0.artist_albums(id, title)
    }
    fn albums_for_artists(
        &mut self,
        ids: &[ExternalIdentity],
        title: &str,
    ) -> Result<
        music_library::catalog::Page<music_library::catalog::ArtistAlbumCandidate>,
        music_library::catalog::CatalogError,
    > {
        self.0.albums_for_artists(ids, title)
    }
    fn search_albums(
        &mut self,
        q: &str,
        o: u32,
    ) -> Result<
        music_library::catalog::Page<music_library::catalog::AlbumCandidate>,
        music_library::catalog::CatalogError,
    > {
        self.0.search_albums(q, o)
    }
    fn releases(
        &mut self,
        id: &ExternalIdentity,
        o: u32,
    ) -> Result<
        music_library::catalog::Page<music_library::catalog::ReleaseCandidate>,
        music_library::catalog::CatalogError,
    > {
        self.0.releases(id, o)
    }
    fn release(
        &mut self,
        id: &ExternalIdentity,
    ) -> Result<music_library::catalog::Release, music_library::catalog::CatalogError> {
        self.0.release(id)
    }
}
fn run_worker(
    library: &mut Library,
    imported: &ImportedRelease,
) -> Result<(), Box<dyn std::error::Error>> {
    enum Event {
        Album(Box<MatchReply>),
        Programs(Box<album_program::Reply>),
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let program_tx = tx.clone();
    let mut matcher = album_matching::AlbumMatcher::for_provider(
        Counted(music_library_spotify::Spotify::from_env()?),
        music_library_spotify::matching_scope(),
        move |r| {
            let _ = tx.send(Event::Album(Box::new(r)));
        },
        move |r| {
            let _ = program_tx.send(Event::Programs(Box::new(r)));
        },
        |_| {},
    )?;
    matcher.after_import(
        library,
        std::slice::from_ref(imported),
        album_matching::AutoMatchPolicy::default(),
    )?;
    while matcher.pending_count() > 0 {
        let event = rx.recv_timeout(std::time::Duration::from_secs(120))?;
        let start = Instant::now();
        match event {
            Event::Album(r) => {
                println!("WORKER Album result {:?}", r.outcome);
                if matches!(
                    r.outcome,
                    album_matching::MatchOutcome::Deferred(_)
                        | album_matching::MatchOutcome::Error(_)
                ) {
                    return Err("Provider unavailable; diagnostic stops without retries".into());
                }
                println!("WORKER completion {:?}", matcher.complete(library, *r));
            }
            Event::Programs(r) => {
                if r.result.is_err() {
                    return Err("Program unavailable; diagnostic stops without retries".into());
                }
                println!(
                    "WORKER program completion {:?}",
                    matcher.complete_programs(library, *r)
                );
            }
        }
        println!("WORKER local completion elapsed={:?}", start.elapsed());
    }
    Ok(())
}
