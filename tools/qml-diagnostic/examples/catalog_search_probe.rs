use music_library::{
    catalog::*,
    catalog_search::{Hit, rank},
};
fn main() {
    let mut clients: Vec<(&str, Box<dyn CatalogProvider>)> = vec![(
        "musicbrainz",
        Box::new(music_library_musicbrainz::MusicBrainz::new()),
    )];
    if let Ok(client) = music_library_spotify::Spotify::from_env() {
        clients.push(("spotify", Box::new(client)));
    }
    let queries: Vec<String> = std::env::args().skip(1).collect();
    let queries = if queries.is_empty() {
        vec![
            "tricot",
            "T H E",
            "Hatsumimi",
            "toe",
            "For Long Tomorrow",
            "Gorillaz",
            "Demon Days",
            "Feel Good Inc",
            "Hop Along",
            "Painted Shut",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    } else {
        queries
    };
    for (name, mut client) in clients {
        for query in &queries {
            let mut hits = vec![];
            match client.search_artists(query) {
                Ok(p) => hits.extend(p.items.into_iter().map(Hit::Artist)),
                Err(e) => eprintln!("{name} {query} artists: {e}"),
            }
            match client.catalog_albums(query) {
                Ok(p) => hits.extend(p.items.into_iter().map(Hit::Album)),
                Err(e) => eprintln!("{name} {query} albums: {e}"),
            }
            match client.catalog_songs(query) {
                Ok(p) => hits.extend(p.items.into_iter().map(|s| Hit::Song(Box::new(s)))),
                Err(e) => eprintln!("{name} {query} songs: {e}"),
            }
            rank(&mut hits, query);
            println!("{name} query={query:?} count={}", hits.len());
            for h in &hits {
                match h {
                    Hit::Artist(a) => println!("  Artist {}", a.name),
                    Hit::Album(a) => println!("  Album {} — {}", a.title, a.artist),
                    Hit::Song(s) => {
                        println!("  Song {} — {} — {}", s.title, s.artist, s.album.title)
                    }
                }
            }
        }
    }
}
