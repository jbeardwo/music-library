//! Opt-in, read-only Artist-first discovery. Never part of normal tests.
use music_library::{
    album_matching::{accepted_album, resolve_artist},
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
        match resolve_artist(artist, &page) {
            Ok(identity) => identity,
            Err(state) => {
                println!("Decision: {state:?}");
                return Ok(());
            }
        }
    };
    println!("Artist: {}", id.external_id);
    let page = client.artist_albums(&id, title)?;
    println!("Album candidates (more={}):", page.next_offset.is_some());
    for candidate in &page.items {
        println!("  {} | {}", candidate.title, candidate.identity.external_id);
    }
    let decision = accepted_album(title, &id, &page);
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
