use super::*;
use music_library::song_resolution::SongSearch as _;
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    thread,
};

struct Mock {
    client: Spotify,
    requests: Arc<Mutex<Vec<String>>>,
    thread: thread::JoinHandle<()>,
}
fn token() -> (u16, Value, Option<&'static str>) {
    (
        200,
        json!({"access_token":"private-token","token_type":"Bearer","expires_in":3600}),
        None,
    )
}
fn artists() -> (u16, Value, Option<&'static str>) {
    (
        200,
        json!({"artists":{"items":[{"id":"artist1","name":"Artist"}],"total":1,"offset":0,"next":null}}),
        None,
    )
}
#[test]
fn explicit_song_search_is_bounded_uses_catalog_auth_and_yields_playback_occurrences() {
    let item = json!({"id":"1234567890123456789012","name":"Song & Title","disc_number":1,"track_number":2,"duration_ms":123456,"artists":[{"id":"artist1","name":"Artist"}],"album":{"id":"album1","name":"Album Deluxe","artists":[],"release_date":"2020"},"external_ids":{"isrc":"example"}});
    let page = (
        200,
        json!({"tracks":{"items":[item.clone(),item],"total":50,"offset":0,"next":"untrusted-next"}}),
        None,
    );
    let mut mock = Mock::new(vec![token(), page.clone(), page]);
    assert_eq!(mock.client.request_counts(), (0, 0));
    let input = music_library::song_resolution::Input {
        track_id: music_library::domain::TrackId("application-track".into()),
        title: "Song \"Title\"".into(),
        artist: "Artist".into(),
        album: "Album".into(),
        duration_ms: None,
        disc: None,
        number: None,
    };
    let found = mock.client.search_songs(&input).unwrap();
    assert_eq!(found.items.len(), 1);
    assert_eq!(found.next_offset, Some(10));
    assert_eq!(
        found.items[0].identity,
        id("track", "1234567890123456789012")
    );
    assert_eq!(found.items[0].album, "Album Deluxe");
    assert_eq!(
        playback::Song::from_associations(&[found.items[0].identity.clone()])
            .unwrap()
            .uri(),
        "spotify:track:1234567890123456789012"
    );
    mock.client.search_songs(&input).unwrap();
    assert_eq!(mock.client.request_counts(), (1, 2));
    let requests = mock.finish();
    assert!(requests[0].contains("grant_type=client_credentials"));
    let line = requests[1].lines().next().unwrap();
    let url = Url::parse(&format!(
        "http://mock{}",
        line.split_whitespace().nth(1).unwrap()
    ))
    .unwrap();
    let params: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(params["type"], "track");
    assert_eq!(params["limit"], "10");
    assert_eq!(params["offset"], "0");
    assert_eq!(params["market"], "US");
    assert_eq!(
        params["q"],
        "track:\"Song \\\"Title\\\"\" artist:\"Artist\""
    );
    assert!(!requests.iter().any(|r| r.contains("me/player")));
}
impl Mock {
    fn new(responses: Vec<(u16, Value, Option<&str>)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let requests = Arc::new(Mutex::new(vec![]));
        let log = requests.clone();
        let responses: Vec<_> = responses
            .into_iter()
            .map(|(s, b, r)| (s, b, r.map(str::to_owned)))
            .collect();
        let thread = thread::spawn(move || {
            for (status, body, retry) in responses {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut socket = loop {
                    match listener.accept() {
                        Ok((s, _)) => break s,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                Instant::now() < deadline,
                                "Expected mock request did not arrive"
                            );
                            thread::sleep(Duration::from_millis(1));
                        }
                        Err(e) => panic!("{e}"),
                    }
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = vec![];
                loop {
                    let mut buf = [0; 4096];
                    let count = socket.read(&mut buf).unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buf[..count]);
                    let text = String::from_utf8_lossy(&bytes);
                    if let Some(end) = text.find("\r\n\r\n") {
                        let length = text[..end]
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                log.lock().unwrap().push(String::from_utf8(bytes).unwrap());
                let body = body.to_string();
                write!(socket,"HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n{body}",body.len(),retry.map(|r|format!("Retry-After: {r}\r\n")).unwrap_or_default()).unwrap();
            }
        });
        Self {
            client: Spotify::at(
                Config::new(
                    "private-client".into(),
                    "private-secret".into(),
                    "US".into(),
                )
                .unwrap(),
                url.clone(),
                url.join("token").unwrap(),
            ),
            requests,
            thread,
        }
    }
    fn finish(self) -> Vec<String> {
        self.thread.join().unwrap();
        Arc::try_unwrap(self.requests)
            .unwrap()
            .into_inner()
            .unwrap()
    }
}

#[test]
fn empty_full_title_uses_one_bounded_artist_scoped_token_without_changing_acceptance() {
    use music_library::album_matching::{MatchOutcome, accepted_album};
    let empty = (
        200,
        json!({"albums":{"items":[],"total":0,"offset":0,"next":null}}),
        None,
    );
    let found = (
        200,
        json!({"albums":{"items":[{"id":"album1","name":"Bitches Ain't Shit But Good People","artists":[{"id":"artist1","name":"Artist"}],"album_type":"single","total_tracks":4,"release_date":"2003-03-31"}],"total":1,"offset":0,"next":null}}),
        None,
    );
    let mut mock = Mock::new(vec![token(), artists(), empty.clone(), found, empty]);
    mock.client.search_artists("Artist").unwrap();
    let artist = id("artist", "artist1");
    let page = mock
        .client
        .artist_albums(&artist, "Bitches Ain't Shit But Good People")
        .unwrap();
    assert_eq!(page.items[0].primary_type, "Single");
    assert_eq!(
        mock.client
            .album_candidate_track_count(&id("album", "album1")),
        Some(4)
    );
    assert!(matches!(
        accepted_album("Bitches Ain't Shit But Good People", &artist, &page),
        MatchOutcome::Matched(_)
    ));
    assert!(matches!(
        accepted_album("Unrelated title", &artist, &page),
        MatchOutcome::NoConfidentMatch
    ));
    assert!(
        mock.client
            .artist_albums(&artist, "Wretches")
            .unwrap()
            .items
            .is_empty()
    );
    let requests = mock.finish();
    assert_eq!(requests.len(), 5);
    assert!(requests[3].contains("album%3A%22Bitches%22+artist%3A%22Artist%22"));
}

#[test]
fn client_credentials_cache_refresh_and_redaction() {
    let mut mock = Mock::new(vec![token(), artists(), artists(), token(), artists()]);
    assert_eq!(
        mock.client.search_artists("Artist").unwrap().items[0].identity,
        id("artist", "artist1")
    );
    mock.client.search_artists("Artist").unwrap();
    mock.client.token.as_mut().unwrap().expires = Instant::now();
    mock.client.search_artists("Artist").unwrap();
    let debug = format!("{:?}", mock.client);
    for secret in ["private-client", "private-secret", "private-token"] {
        assert!(!debug.contains(secret));
    }
    let requests = mock.finish();
    assert_eq!(requests.len(), 5);
    assert!(requests[0].starts_with("POST /token "));
    assert!(requests[0].contains("grant_type=client_credentials"));
    assert!(
        requests[0].to_ascii_lowercase().contains(
            &format!(
                "authorization: basic {}",
                STANDARD.encode("private-client:private-secret")
            )
            .to_ascii_lowercase()
        )
    );
    assert!(requests[1].contains("market=US"));
    assert!(requests[1].contains("limit=10"));
}

#[test]
fn authentication_refresh_is_once_and_configuration_does_not_loop() {
    let unauthorized = (401, json!({"error":{"message":"expired"}}), None);
    let mut mock = Mock::new(vec![
        token(),
        unauthorized.clone(),
        token(),
        artists(),
        unauthorized.clone(),
        token(),
        unauthorized,
    ]);
    mock.client.search_artists("Artist").unwrap();
    let error = mock.client.search_artists("Artist").unwrap_err();
    assert!(matches!(
        error,
        CatalogError::Configuration { status: 401, .. }
    ));
    assert!(!error.is_provider_unavailable());
    assert_eq!(mock.client.search_artists("Artist").unwrap_err(), error);
    assert_eq!(mock.finish().len(), 7);
    let mut mock = Mock::new(vec![(
        400,
        json!({"error":"invalid_client","error_description":"private-secret"}),
        None,
    )]);
    let error = mock.client.search_artists("Artist").unwrap_err();
    assert!(matches!(
        error,
        CatalogError::Configuration { status: 400, .. }
    ));
    assert!(!format!("{error:?} {error}").contains("private-secret"));
    assert_eq!(mock.client.search_artists("Artist").unwrap_err(), error);
    assert_eq!(mock.finish().len(), 1);
}

#[test]
fn errors_classify_quota_service_configuration_and_request_failures() {
    for (status, unavailable) in [
        (429, true),
        (503, true),
        (500, true),
        (403, false),
        (400, false),
    ] {
        let mut mock = Mock::new(vec![
            token(),
            (
                status,
                json!({"error":{"message":"private-secret private-token","reason":"QUOTA_EXCEEDED"}}),
                Some("30"),
            ),
        ]);
        let error = mock.client.search_artists("Artist").unwrap_err();
        assert_eq!(error.is_provider_unavailable(), unavailable);
        assert!(error.to_string().contains(&status.to_string()));
        assert!(!format!("{error:?}").contains("private-secret"));
        assert!(!format!("{error:?}").contains("private-token"));
        if status == 429 {
            assert!(
                matches!(&error,CatalogError::RateLimited{retry_after:Some(r),message,..} if r=="30" && message.contains("QUOTA_EXCEEDED"))
            );
        }
        mock.finish();
    }
}

fn song(n: u32) -> Value {
    json!({"id":format!("song{n}"),"name":format!("Song {n}"),"disc_number":1,"track_number":n,"duration_ms":200000,"artists":[{"id":"artist1","name":"Artist"}],"external_ids":{"isrc":"TESTISRC"}})
}
#[test]
fn album_search_identity_filter_and_paginated_occurrence_program_are_cached() {
    let albums = json!({"albums":{"items":[{"id":"album1","name":"Album","artists":[{"id":"artist1","name":"Artist"}],"album_type":"album"},{"id":"wrong","name":"Album","artists":[{"id":"other","name":"Artist"}]}],"total":2}});
    let first = json!({"items":[song(1),song(2)],"offset":0,"total":3,"next":"https://untrusted.invalid/never-follow"});
    let last = json!({"items":[song(3)],"offset":2,"total":3,"next":null});
    let mut mock = Mock::new(vec![
        token(),
        artists(),
        (200, albums, None),
        (200, first, None),
        (200, last, None),
    ]);
    mock.client.search_artists("Artist").unwrap();
    let page = mock
        .client
        .artist_albums(&id("artist", "artist1"), "Album")
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].identity, id("album", "album1"));
    let p = mock.client.album_programs(&page.items[0].identity).unwrap();
    assert_eq!(p.programs.len(), 1);
    assert!(p.programs[0].identity.is_none());
    assert!(p.programs[0].complete);
    assert_eq!(p.programs[0].tracks.len(), 3);
    for track in &p.programs[0].tracks {
        assert!(track.recording.identities.is_empty());
        assert_eq!(track.identities[0].kind, "track");
        assert_eq!(track.recording.isrcs, vec!["TESTISRC"]);
    }
    assert_eq!(mock.client.album_programs(&p.album).unwrap(), p);
    let requests = mock.finish();
    assert_eq!(requests.len(), 5);
    assert!(requests[3].starts_with("GET /albums/album1/tracks?"));
    assert!(requests[4].contains("offset=2"));
    let query = Url::parse(&format!(
        "http://host{}",
        requests[2].split_whitespace().nth(1).unwrap()
    ))
    .unwrap();
    assert_eq!(
        query.query_pairs().find(|(k, _)| k == "q").unwrap().1,
        "album:\"Album\" artist:\"Artist\""
    );
}

#[test]
fn known_artist_cache_miss_uses_single_lookup_not_discography() {
    let mut mock = Mock::new(vec![
        token(),
        (200, json!({"id":"artist1","name":"Artist"}), None),
        (200, json!({"albums":{"items":[],"total":0}}), None),
    ]);
    mock.client
        .artist_albums(&id("artist", "artist1"), "Album")
        .unwrap();
    let requests = mock.finish();
    assert_eq!(requests.len(), 3);
    assert!(requests[1].starts_with("GET /artists/artist1?"));
}

#[test]
fn explicit_market_and_transport_failures() {
    assert!(Config::new("id".into(), "secret".into(), "".into()).is_err());
    assert!(Config::new("".into(), "secret".into(), "US".into()).is_err());
    assert_eq!(
        Config::new("id".into(), "secret".into(), "us".into())
            .unwrap()
            .market,
        "US"
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap();
    drop(listener);
    let url = Url::parse(&format!("http://{port}/")).unwrap();
    let mut client = Spotify::at(
        Config::new("id".into(), "secret".into(), "US".into()).unwrap(),
        url.clone(),
        url,
    );
    assert!(matches!(
        client.search_artists("Artist"),
        Err(CatalogError::TransportUnavailable(_))
    ));
    assert!(matches!(
        transport(ureq::Error::Timeout(ureq::Timeout::Global)),
        CatalogError::Timeout(_)
    ));
}

#[test]
fn incomplete_pages_remain_bounded_and_missing_isrc_is_normal() {
    let mut responses = vec![token()];
    for page in 0..20 {
        let items: Vec<_> = (1..=50)
            .map(|n| {
                let mut value = song(page * 50 + n);
                value.as_object_mut().unwrap().remove("external_ids");
                value
            })
            .collect();
        responses.push((
            200,
            json!({"items":items,"offset":page*50,"total":1001,"next":"ignored"}),
            None,
        ));
    }
    let mut mock = Mock::new(responses);
    let programs = mock.client.album_programs(&id("album", "album1")).unwrap();
    assert!(!programs.programs[0].complete);
    assert_eq!(programs.programs[0].tracks.len(), 1000);
    assert!(
        programs.programs[0]
            .tracks
            .iter()
            .all(|t| t.recording.isrcs.is_empty() && t.recording.identities.is_empty())
    );
    assert!(
        mock.client.programs.is_empty(),
        "incomplete results are not cached as complete"
    );
    assert_eq!(
        mock.finish().len(),
        21,
        "one token plus bounded Album pages, no Track lookups"
    );
}

#[test]
fn malformed_and_changing_pages_are_errors_not_outages() {
    let mut mock = Mock::new(vec![
        token(),
        (200, json!({"items":[song(1)],"offset":5,"total":1}), None),
    ]);
    let error = mock
        .client
        .album_programs(&id("album", "album1"))
        .unwrap_err();
    assert!(!error.is_provider_unavailable());
    mock.finish();
    let mut mock = Mock::new(vec![token(), (200, json!({"unexpected":"shape"}), None)]);
    assert!(matches!(
        mock.client.search_artists("Artist"),
        Err(CatalogError::Other(_))
    ));
    mock.finish();
}
