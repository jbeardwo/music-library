//! Explicit, bounded discovery/evidence audit on a disposable database.
use music_library::{
    Library,
    album_matching::{Preparation, resolve_artist},
    catalog::CatalogProvider,
};
use music_library_spotify::Spotify;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("disposable database required")?;
    let live = std::env::args().any(|s| s == "--discover");
    let mut library = Library::open(&path)?;
    let market = library.retained_spotify_market()?.unwrap_or("US".into());
    let db = rusqlite::Connection::open(&path)?;
    let mut albums=db.prepare("SELECT album_id,title,(SELECT a.name FROM album_artist_credit ac JOIN artist a ON a.id=ac.artist_id WHERE ac.album_id=e.album_id ORDER BY ac.position LIMIT 1) FROM effective_album_metadata e WHERE title IN ('X’ed Out','X''ed Out','Homeboy','100 One Says','Angle','Balance','Underwater','Work','Freshman Year','First Contact','Blue Version','Floral EP','Floral LP') ORDER BY title")?.query_map([],|r|Ok((music_library::domain::AlbumId(r.get(0)?),r.get::<_,String>(1)?,r.get::<_,String>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    if std::env::args().any(|s| s == "--inbox-only") {
        albums.retain(|(_, title, _)| {
            matches!(
                title.as_str(),
                "Work" | "Freshman Year" | "First Contact" | "Blue Version"
            )
        });
    }
    if std::env::args().any(|s| s == "--stress") {
        use std::sync::{
            Arc, Barrier,
            atomic::{AtomicBool, Ordering},
        };
        let ready = Arc::new(Barrier::new(2));
        let done = Arc::new(AtomicBool::new(false));
        let targets: Vec<_> = albums.iter().map(|(id, _, _)| id.clone()).collect();
        let worker_targets = targets.clone();
        let worker_ready = ready.clone();
        let worker_done = done.clone();
        let worker_path = path.clone();
        let worker_market = market.clone();
        let worker = std::thread::spawn(move || {
            let result = (|| -> Result<_, String> {
                let mut library = Library::open(worker_path).map_err(|e| e.to_string())?;
                worker_ready.wait();
                let mut connected = 0;
                for album in worker_targets {
                    let batch = music_library_spotify::album_context::reconcile(
                        &mut library,
                        std::slice::from_ref(&album),
                        &worker_market,
                    )
                    .map_err(|e| e.to_string())?;
                    if !batch.errors.is_empty() {
                        return Err(batch.errors.join("; "));
                    }
                    connected += batch.accepted.len();
                }
                Ok(connected)
            })();
            worker_done.store(true, Ordering::SeqCst);
            result
        });
        ready.wait();
        let mut opens = 0;
        while !done.load(Ordering::SeqCst) || opens < 50 {
            let album = &targets[opens % targets.len()];
            if let Preparation::Ready(input) = library.spotify_album_review_input(album)? {
                let _ = Spotify::cached_album_review(&path, &market, &input)?;
            }
            library.spotify_review_groups(None, 200)?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs() as i64;
            library.persist_spotify_discovery(&format!("stress:{opens}"), "{}", now)?;
            opens += 1;
        }
        let connected = worker.join().map_err(|_| "stress worker panicked")??;
        println!(
            "STRESS review_opens={opens} concurrent_cache_writes={opens} targets={} newly_connected={connected} sqlite_lock_errors=0 provider_requests=0",
            targets.len()
        );
        return Ok(());
    }
    let mut client = if live {
        Some(Spotify::from_env()?.with_library(Some(&path))?)
    } else {
        None
    };
    for (album, title, artist) in albums {
        if std::env::args().any(|s| s == "--catalog-aliases") {
            let mut provider = music_library_musicbrainz::MusicBrainz::new();
            if let Some(observation) = provider.enrich_artist_names(&mut library, &album)? {
                println!(
                    "CATALOG_NAMES {} {} {} {:?}",
                    artist, observation.identity.external_id, observation.name, observation.aliases
                );
            }
            if live && let Preparation::Ready(input) = library.spotify_album_review_input(&album)? {
                let names = library.trusted_artist_search_names(&input.artist)?;
                if let Some(name) = names.first() {
                    let client = client.as_mut().unwrap();
                    let page = client.search_artists(name)?;
                    if let Ok(identity) = resolve_artist(name, &page) {
                        let candidate = page.items.iter().find(|c| c.identity == identity).unwrap();
                        library.ingest_catalog_artist_provider_name(&input.artist_id, candidate)?;
                        println!(
                            "TRUSTED_PROVIDER_NAME {} {} {}",
                            artist, name, identity.external_id
                        );
                    }
                }
            }
        }

        if std::env::args().any(|s| s == "--catalog-aliases") {
            println!(
                "CATALOG_CREDIT {} changed={}",
                title,
                music_library_spotify::album_context::reconcile_catalog_credit_names(
                    &mut library,
                    &album,
                    &market
                )?
            );
        }
        let Preparation::Ready(input) = library.spotify_album_review_input(&album)? else {
            continue;
        };
        if live
            && (title.contains("Out")
                || artist == "The Speed of Sound in Seawater"
                || artist == "Elephant Gym"
                || title == "Freshman Year")
        {
            let client = client.as_mut().unwrap();
            let identity = if let Some(id) = &input.known_artist {
                id.clone()
            } else {
                resolve_artist(&input.artist, &client.search_artists(&input.artist)?)
                    .map_err(|e| format!("Artist discovery: {e:?}"))?
            };
            let ids = if input.equivalent_artists.is_empty() {
                vec![identity.clone()]
            } else {
                input.equivalent_artists.clone()
            };
            let page = client.albums_for_artists(&ids, &input.title)?;
            for candidate in page
                .items
                .iter()
                .filter(|c| {
                    c.artist_ids.iter().any(|id| ids.contains(id))
                        && (music_library::matching::normalize_album_title(&c.title)
                            == music_library::matching::normalize_album_title(&input.raw_title)
                            || music_library::matching::shared_title_span(
                                &c.title,
                                &input.raw_title,
                            )
                            .is_some())
                })
                .take(2)
            {
                client.album_programs(&candidate.identity)?;
            }
            let local = library.local_album_tracks(&album)?;
            let resolution = music_library::album_candidates::resolve_canonical(
                client,
                (&input.raw_title, input.date),
                &identity,
                &page,
                false,
                &local,
                &mut vec![],
                &input.equivalent_artists,
                &input.evidence,
            )?;
            library.persist_spotify_review(&album, &resolution.report)?;
            println!(
                "DISCOVERY {} {}",
                title,
                serde_json::to_string(&resolution.report)?
            );
        }
        if let Some((report, programs)) = Spotify::cached_album_review(&path, &market, &input)? {
            println!("EVIDENCE {} {}", title, serde_json::to_string(&report)?);
            println!("PROGRAMS {} {}", title, serde_json::to_string(&programs)?);
            for p in &programs {
                for program in &p.programs {
                    let local = library.local_album_tracks(&album)?;
                    println!(
                        "DIRECT {} {}",
                        title,
                        serde_json::to_string(
                            &music_library::album_program::inspect_album_program(
                                &local,
                                program,
                                &input.raw_title
                            )
                        )?
                    );
                }
            }
            let before = library
                .local_album_tracks(&album)?
                .iter()
                .filter(|t| {
                    library
                        .track_provider_occurrences(&t.track_id, "spotify")
                        .unwrap()
                        .is_empty()
                })
                .count();
            let batch = music_library_spotify::album_context::reconcile(
                &mut library,
                std::slice::from_ref(&album),
                &market,
            )?;
            println!(
                "RESULT {} initial={} resolved={} remaining={} errors={:?}",
                title,
                before,
                batch.accepted.len(),
                batch.still_unresolved,
                batch.errors
            );
            for t in library.local_album_tracks(&album)? {
                println!(
                    "MAPPING {} {} {}",
                    title,
                    t.evidence.title.as_deref().unwrap_or(""),
                    serde_json::to_string(
                        &library.track_provider_occurrences(&t.track_id, "spotify")?
                    )?
                );
            }
        } else {
            println!("EVIDENCE {} no cached discovery", title);
        }
    }
    if let Some(client) = client {
        println!("PROVIDER_REQUESTS {:?}", client.request_counts());
    } else {
        println!("PROVIDER_REQUESTS (0, 0)");
    }
    Ok(())
}
