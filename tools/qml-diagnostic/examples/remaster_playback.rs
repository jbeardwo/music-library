//! Cache-only remaster reconciliation evidence on a disposable copy.
use music_library::{Library, album_matching::Preparation};
use music_library_spotify::Spotify;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("disposable database required")?;
    assert!(path.starts_with("/tmp/"));
    let mut library = Library::open(&path)?;
    let db = rusqlite::Connection::open(&path)?;
    let albums=db.prepare("SELECT e.album_id,e.title,a.name FROM effective_album_metadata e JOIN album_artist_credit ac ON ac.album_id=e.album_id JOIN artist a ON a.id=ac.artist_id WHERE e.title='Critical Meat'")?.query_map([],|r|Ok((music_library::domain::AlbumId(r.get(0)?),r.get::<_,String>(1)?,r.get::<_,String>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
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
    println!("PROVIDER_REQUESTS 0 (cache-only API; no live client constructed)");
    Ok(())
}
