//! Read-only bounded live diagnosis; no acceptance or persistence changes.
use music_library::{
    album_matching::{accepted_album, resolve_artist},
    catalog::CatalogProvider,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = music_library_spotify::Spotify::from_env()?;
    if std::env::args().any(|a| a == "--controls") {
        for query in [
            "album:\"Wretches\" artist:\"Hop Along, Queen Ansleis\"",
            "album:\"Good People\" artist:\"Hella\"",
            "album:\"Bitches\" artist:\"Hella\"",
        ] {
            println!(
                "DIAGNOSTIC ONLY {query}: {:?}",
                client.search_albums(query, 0)?
            );
        }
        return Ok(());
    }
    for (artist, title) in [
        ("Tera Melos", "Trash Generator"),
        ("Hop Along", "Wretches"),
        ("Hella", "Bitches Ain't Shit But Good People"),
    ] {
        println!("DIAGNOSIS {artist} / {title}");
        let artists = client.search_artists(artist)?;
        let resolved = resolve_artist(artist, &artists);
        println!("Artist resolution: {resolved:?}");
        let Ok(identity) = resolved else { continue };
        let page = client.artist_albums(&identity, title)?;
        println!(
            "Before programs: {:?}",
            accepted_album(title, &identity, &page)
        );
        for candidate in page.items.iter().take(3) {
            println!(
                "PROGRAM {} {:?}",
                candidate.identity.external_id, candidate.title
            );
            let programs = client.album_programs(&candidate.identity)?;
            for p in programs.programs {
                println!("complete={} count={}", p.complete, p.tracks.len());
                for t in p.tracks {
                    println!(
                        "{:?}/{:?} {:?} {:?}ms {:?}",
                        t.disc, t.number, t.title, t.duration_ms, t.identities
                    );
                }
            }
        }
        if page.items.is_empty() {
            let broad = client.search_albums(&format!("album:\"{title}\""), 0)?;
            println!("DIAGNOSTIC title-only control (not used for acceptance): {broad:?}");
        }
    }
    Ok(())
}
