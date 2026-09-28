//! Read-only diagnosis of programs for an explicitly supplied provider Artist.
//! This does not accept the supplied Artist or Album in the application database.
use music_library::{Library, album_matching, album_program, catalog::CatalogProvider, domain::*};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [artist_id, title, output] = args.as_slice() else {
        return Err("SPOTIFY_ARTIST_ID ALBUM OUTPUT_JSON".into());
    };
    let path = std::path::PathBuf::from(
        std::env::var_os("MUSIC_LIBRARY_DIAGNOSTIC_DATABASE").ok_or("database")?,
    );
    if !path.starts_with("/tmp") {
        return Err("disposable /tmp database required".into());
    }
    let lib = Library::open(path)?;
    let rows = lib.search(&SearchRequest {
        limit: 100,
        ..Default::default()
    })?;
    let db =
        rusqlite::Connection::open(std::env::var_os("MUSIC_LIBRARY_DIAGNOSTIC_DATABASE").unwrap())?;
    let album = AlbumId(db.query_row("SELECT album_id FROM release LIMIT 1", [], |r| r.get(0))?);
    let local = lib.local_album_tracks(&album)?;
    let mut spotify = music_library_spotify::Spotify::from_env()?;
    let artist = ExternalIdentity {
        provider: "spotify".into(),
        kind: "artist".into(),
        external_id: artist_id.clone(),
    };
    let page = spotify.artist_albums(&artist, title)?;
    println!("CANDIDATES {page:?}");
    println!(
        "ALBUM DECISION {:?}",
        album_matching::accepted_album(title, &artist, &page)
    );
    let mut saved = vec![];
    for candidate in &page.items {
        let programs = spotify.album_programs(&candidate.identity)?;
        println!(
            "PROGRAM {} fit={:?}",
            candidate.identity.external_id,
            music_library::album_candidates::fit(&local, &programs)
        );
        for program in &programs.programs {
            for (i, track) in program.tracks.iter().enumerate() {
                println!("PROVIDER {track:?}");
                if let Some(t) = local
                    .iter()
                    .find(|t| t.evidence.number == track.number && t.evidence.disc == track.disc)
                {
                    println!(
                        "LOCAL {:?} CHECK {:?}",
                        t.evidence,
                        album_program::inspect_candidate(t, track, i)
                    );
                }
            }
            saved.push(serde_json::json!({"album":programs.album,"date":candidate.date,"tracks":program.tracks,"complete":program.complete}));
        }
    }
    let input = lib.song_resolution_input(&rows[0].track_id)?;
    std::fs::write(
        output,
        serde_json::to_string_pretty(
            &serde_json::json!({"title":input.album,"artist":input.album_artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(" & "),"year":input.album_date.map(|d|d.year),"tracks":local.iter().map(|t| &t.evidence).collect::<Vec<_>>(),"programs":saved}),
        )?,
    )?;
    println!("HTTP token/catalog {:?}", spotify.request_counts());
    Ok(())
}
