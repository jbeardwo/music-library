//! Opt-in, read-only Artist-first discovery. Never part of normal tests.
use music_library::{
    album_matching::{
        MatchOutcome, accepted_album, accepted_album_confirmed, corroborate_artist_album,
        resolve_artist,
    },
    catalog::CatalogProvider,
};
use music_library_musicbrainz::MusicBrainz;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if !(2..=3).contains(&args.len()) {
        return Err("usage: artist_match_probe ARTIST ALBUM [EXPLICIT_ARTIST_MBID]".into());
    }
    let (artist, title) = (&args[0], &args[1]);
    let mut client = MusicBrainz::new();
    let id = if let Some(id) = args.get(2) {
        music_library::domain::ExternalIdentity {
            provider: "musicbrainz".into(),
            kind: "artist".into(),
            external_id: id.clone(),
        }
    } else {
        let page = client.search_artists(artist)?;
        println!(
            "Artist search: {} results, more={}",
            page.items.len(),
            page.next_offset.is_some()
        );
        for candidate in &page.items {
            println!(
                "  {} | {} | aliases={:?}",
                candidate.name, candidate.identity.external_id, candidate.aliases
            );
        }
        match resolve_artist(artist, &page) {
            Ok(identity) => identity,
            Err(MatchOutcome::ArtistAmbiguous(candidates)) if !candidates.is_empty() => {
                let ids = candidates
                    .iter()
                    .map(|c| c.identity.clone())
                    .collect::<Vec<_>>();
                let albums = client.albums_for_artists(&ids, title)?;
                println!(
                    "Pair decision: {:?}",
                    corroborate_artist_album(artist, title, &candidates, &albums)
                );
                return Ok(());
            }
            Err(state) => {
                println!("Decision: {state:?}");
                return Ok(());
            }
        }
    };
    println!("Artist: {}", id.external_id);
    let page = if args.len() == 3 {
        client.artist_albums_confirmed(&id, title)?
    } else {
        client.artist_albums(&id, title)?
    };
    println!("Album candidates (more={}):", page.next_offset.is_some());
    for candidate in &page.items {
        println!("  {} | {}", candidate.title, candidate.identity.external_id);
    }
    let decision = accepted_album_confirmed(title, &id, &page, args.len() == 3);
    println!("Decision: {decision:?}");
    if let music_library::album_matching::MatchOutcome::MatchedClose(identity) = decision {
        let exact = page
            .items
            .iter()
            .find(|c| c.identity == identity)
            .unwrap()
            .title
            .clone();
        let control = client.artist_albums(&id, &exact)?;
        println!(
            "Exact-title control ({exact}): {:?}",
            accepted_album(&exact, &id, &control)
        );
    }
    Ok(())
}
