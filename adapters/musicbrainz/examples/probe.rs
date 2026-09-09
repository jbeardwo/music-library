//! Explicit opt-in read-only live probe; never part of automated tests.
use music_library::catalog::CatalogProvider;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut client = music_library_musicbrainz::MusicBrainz::new();
    if matches!(
        args.first().map(String::as_str),
        Some("--album" | "--editions")
    ) {
        let query = args.get(1).ok_or("provide Album query")?;
        let mut session = music_library::catalog::CatalogSession::new(client);
        let albums = session.search_albums(query, 0)?;
        // This read-only probe chooses the first Album-type result explicitly;
        // the UI always lets the user select the candidate, including EPs/singles.
        let album = albums
            .items
            .iter()
            .find(|a| a.primary_type == "Album")
            .ok_or("No Album-type search match")?;
        if args[0] == "--editions" {
            let editions = session.editions(album, 0)?;
            println!(
                "{} candidate editions; more page: {:?}",
                editions.items.len(),
                editions.next_offset
            );
            for r in &editions.items {
                println!(
                    "{} | {} | {} | {} | {} discs / {} tracks | {} | {}",
                    r.identity.external_id,
                    r.status,
                    r.title,
                    r.date,
                    r.disc_count,
                    r.track_count,
                    r.formats.join(", "),
                    r.comment
                );
            }
            return Ok(());
        }
        let release = session.add_album(album)?;
        println!("Album: {} | {} | {}", album.title, album.artist, album.date);
        println!(
            "Representative: {} | {} | {} media | {} Tracks (read-only probe)",
            release.identity.external_id,
            release.date,
            release.media.len(),
            release.media.iter().map(|m| m.tracks.len()).sum::<usize>()
        );
    } else if args.first().map(String::as_str) == Some("--release") {
        let id = args.get(1).ok_or("provide Release MBID")?;
        let release = client.release(&music_library::domain::ExternalIdentity {
            provider: "musicbrainz".into(),
            kind: "release".into(),
            external_id: id.clone(),
        })?;
        println!(
            "{}: {} media, {} Tracks",
            release.title,
            release.media.len(),
            release.media.iter().map(|m| m.tracks.len()).sum::<usize>()
        );
    } else {
        let query = args.first().ok_or("provide query or --release MBID")?;
        let groups = client.search_albums(query, 0)?;
        for g in &groups.items {
            println!("{} | {} | {}", g.identity.external_id, g.title, g.artist);
        }
    }
    Ok(())
}
