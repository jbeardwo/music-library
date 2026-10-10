//! Regression replay using retained exact-object responses on a disposable DB.
//! This simulates a complete bounded discovery response; it makes no HTTP calls.
use music_library::{
    Library,
    catalog::Page,
    domain::{ExternalIdentity, TrackId},
    song_resolution::{Candidate, evaluate},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("disposable DB path required")?;
    let ids = std::env::args().skip(2).map(TrackId).collect::<Vec<_>>();
    let db = rusqlite::Connection::open(&path)?;
    let mut objects = vec![];
    for track in &ids {
        let spotify:String=db.query_row("SELECT external_id FROM track_external_identity WHERE track_id=?1 AND provider='spotify' AND kind='track'",[track.as_ref()],|r|r.get(0))?;
        let raw:String=db.query_row("SELECT response_json FROM spotify_discovery WHERE fingerprint LIKE ?1 ORDER BY completed_at DESC LIMIT 1",[format!("%/tracks/{spotify}?%")],|r|r.get(0))?;
        objects.push((
            track.clone(),
            serde_json::from_str::<serde_json::Value>(&raw)?,
        ));
        db.execute(
            "DELETE FROM manual_track_association WHERE track_id=?1 AND album_provider='spotify'",
            [track.as_ref()],
        )?;
        db.execute(
            "DELETE FROM provider_track_association WHERE track_id=?1 AND album_provider='spotify'",
            [track.as_ref()],
        )?;
        db.execute(
            "DELETE FROM track_external_identity WHERE track_id=?1 AND provider='spotify'",
            [track.as_ref()],
        )?;
    }
    drop(db);
    let mut library = Library::open(&path)?;
    let identity = |kind: &str, key: &str| ExternalIdentity {
        provider: "spotify".into(),
        kind: kind.into(),
        external_id: key.into(),
    };
    for (track, object) in objects {
        let string = |v: &serde_json::Value, k: &str| v[k].as_str().unwrap_or_default().to_owned();
        let credits = |v: &serde_json::Value| {
            v.as_array()
                .unwrap()
                .iter()
                .map(|a| music_library::edition::ArtistEvidence {
                    name: string(a, "name"),
                    identities: vec![identity("artist", &string(a, "id"))],
                    join_phrase: String::new(),
                })
                .collect::<Vec<_>>()
        };
        let album = &object["album"];
        let candidate = Candidate {
            recording: music_library::edition::RecordingEvidence {
                identities: vec![],
                isrcs: object["external_ids"]["isrc"]
                    .as_str()
                    .map(str::to_owned)
                    .into_iter()
                    .collect(),
            },
            identity: identity("track", &string(&object, "id")),
            album_identity: Some(identity("album", &string(album, "id"))),
            title: string(&object, "name"),
            artist: string(&object["artists"][0], "name"),
            artists: credits(&object["artists"]),
            album: string(album, "name"),
            date: string(album, "release_date"),
            album_artists: credits(&album["artists"]),
            album_type: string(album, "album_type"),
            album_total_tracks: album["total_tracks"].as_u64().map(|n| n as u32),
            duration_ms: object["duration_ms"].as_u64().unwrap_or(0),
            disc: object["disc_number"].as_u64().unwrap_or(0) as u32,
            number: object["track_number"].as_u64().unwrap_or(0) as u32,
        };
        let before = library.album_id_for_track(&track)?;
        let input = library.song_resolution_input(&track)?;
        let page = Page {
            items: vec![candidate.clone()],
            next_offset: None,
        };
        println!(
            "{} [{}] canonical Album {} {} -> Spotify {} [{}]",
            input.title,
            track.as_ref(),
            before.as_ref(),
            input.album,
            candidate.album,
            candidate.album_identity.unwrap().external_id
        );
        println!("decision {:?}", evaluate(&input, &page, 0, &[]).unwrap());
        library.persist_spotify_song_review(&input, &page)?;
        println!(
            "accepted {:?}",
            library.apply_song_evaluation(&input, &page)?
        );
        assert_eq!(library.album_id_for_track(&track)?, before);
    }
    println!(
        "Provider requests: 0; retained exact-object evidence replay, not a live search-uniqueness audit."
    );
    Ok(())
}
