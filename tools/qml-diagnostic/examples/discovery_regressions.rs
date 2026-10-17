//! Focused reconciliation evidence on a disposable copy; no canonical metadata edits.
use music_library::{Library, album_matching::Preparation, catalog::CatalogProvider};
use music_library_spotify::Spotify;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("disposable database required")?;
    assert!(path.starts_with("/tmp/"));
    let live = std::env::args().any(|s| s == "--discover");
    let mut library = Library::open(&path)?;
    let db = rusqlite::Connection::open(&path)?;
    let albums=db.prepare("SELECT DISTINCT e.album_id,e.title,a.name FROM effective_album_metadata e JOIN album_artist_credit ac ON ac.album_id=e.album_id JOIN artist a ON a.id=ac.artist_id WHERE (a.name='Clown Core' OR a.name='Hop Along' AND e.title IN ('Get Disowned','Painted Shut','Bark Your Head Off, Dog') OR a.name='Hella' AND e.title='Homeboy' OR a.name='Elephant Gym' AND e.title='Dreams' OR e.title='WAO!!') OR e.album_id IN (SELECT r.album_id FROM effective_track_metadata t JOIN track tr ON tr.id=t.track_id JOIN release r ON r.id=tr.release_id WHERE lower(t.title) IN ('guy vs. d.c. sniper','fifzteen','soundtrack to insecurity','dodr','mini rockers')) ORDER BY a.name,e.title")?.query_map([],|r|Ok((music_library::domain::AlbumId(r.get(0)?),r.get::<_,String>(1)?,r.get::<_,String>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut provider = if live {
        Some(Spotify::from_env()?.with_library(Some(&path))?)
    } else {
        None
    };
    for (album, title, artist) in albums {
        let Preparation::Ready(input) = library.spotify_album_review_input(&album)? else {
            continue;
        };
        println!(
            "INPUT {} {} {:?} aliases={:?}",
            artist,
            title,
            input,
            library.trusted_artist_search_names(&input.artist)?
        );
        if let Some((report, programs)) = Spotify::cached_album_review(&path, "US", &input)? {
            println!("BEFORE {} {}", title, serde_json::to_string(&report)?);
            println!("PROGRAMS {} {}", title, serde_json::to_string(&programs)?);
        }
        if let Some(client) = provider.as_mut() {
            println!(
                "ARTIST_ACQUISITION {} changed={}",
                title,
                music_library_spotify::album_context::discover_catalog_credit_names(
                    &mut library,
                    &album,
                    client
                )?
            );
            let Preparation::Ready(current) = library.spotify_album_review_input(&album)? else {
                continue;
            };
            let mut ids = current.equivalent_artists.clone();
            if let Some(id) = &current.known_artist
                && !ids.contains(id)
            {
                ids.insert(0, id.clone());
            }
            if ids.is_empty() {
                continue;
            }
            let page = client.albums_for_artists(&ids, &current.title)?;
            println!(
                "DISCOVERED {} {:?} rejected={:?}",
                title,
                page,
                client.rejected_album_candidates()
            );
            let local = library.local_album_tracks(&album)?;
            let result = music_library::album_candidates::resolve_canonical(
                client,
                (&current.raw_title, current.date),
                current.known_artist.as_ref().unwrap_or(&ids[0]),
                &page,
                false,
                &local,
                &mut vec![],
                &current.equivalent_artists,
                &current.evidence,
            )?;
            library.persist_spotify_review(&album, &result.report)?;
            println!(
                "LIVE_REPORT {} {}",
                title,
                serde_json::to_string(&result.report)?
            );
        }
        let batch = music_library_spotify::album_context::reconcile(
            &mut library,
            std::slice::from_ref(&album),
            "US",
        )?;
        println!("RESULT {} {}", title, serde_json::to_string(&batch)?);
        if let Some((report, programs)) = Spotify::cached_album_review(
            &path,
            "US",
            &library
                .spotify_album_review_input(&album)
                .and_then(|p| match p {
                    Preparation::Ready(i) => Ok(i),
                    _ => Err(music_library::Error::Invalid("not ready".into())),
                })?,
        )? {
            println!("AFTER {} {}", title, serde_json::to_string(&report)?);
            println!(
                "AFTER_PROGRAMS {} {}",
                title,
                serde_json::to_string(&programs)?
            );
            for p in &programs {
                for program in &p.programs {
                    println!(
                        "PLAYBACK {} {}",
                        title,
                        serde_json::to_string(
                            &music_library::album_program::inspect_playable_program(
                                &library.local_album_tracks(&album)?,
                                program,
                                &title
                            )
                        )?
                    );
                }
            }
        }
    }
    println!(
        "PROVIDER_REQUESTS {:?}",
        provider.as_ref().map(|p| p.request_counts())
    );
    Ok(())
}
