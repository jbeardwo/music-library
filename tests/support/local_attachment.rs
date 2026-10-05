// Offline catalog evidence shared by the core and real GStreamer validation.
pub fn get_disowned() -> music_library::catalog::Release {
    use music_library::{catalog::*, domain::ExternalIdentity};
    let v: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/get-disowned-spotify.json")).unwrap();
    let id = |kind: &str, value: &str| ExternalIdentity {
        provider: "spotify".into(),
        kind: kind.into(),
        external_id: value.into(),
    };
    let credits = vec![Credit {
        identity: Some(id("artist", v["artist_id"].as_str().unwrap())),
        name: v["artist"].as_str().unwrap().into(),
        join_phrase: String::new(),
    }];
    Release {
        album: Album {
            identity: id("album", v["album_id"].as_str().unwrap()),
            title: v["title"].as_str().unwrap().into(),
            date: "2012".into(),
            credits: credits.clone(),
        },
        identity: id("album", v["album_id"].as_str().unwrap()),
        identities: vec![],
        title: v["title"].as_str().unwrap().into(),
        date: "2012".into(),
        credits: credits.clone(),
        media: vec![Medium {
            position: 1,
            tracks: v["tracks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| Track {
                    position: t["number"].as_u64().unwrap() as u32,
                    title: t["title"].as_str().unwrap().into(),
                    credits: credits.clone(),
                    identities: vec![id("track", t["spotify_id"].as_str().unwrap())],
                    duration: t["duration_ms"].as_u64().map(|milliseconds| Duration {
                        milliseconds,
                        approximate: false,
                    }),
                })
                .collect(),
        }],
    }
}
