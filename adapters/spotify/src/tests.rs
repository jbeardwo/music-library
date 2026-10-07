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
    let item = json!({"id":"1234567890123456789012","name":"Song & Title","disc_number":1,"track_number":2,"duration_ms":123456,"artists":[{"id":"artist1","name":"Artist"}],"album":{"id":"album1","name":"Album Deluxe","artists":[{"id":"artist1","name":"Artist"}],"release_date":"2020","release_date_precision":"year","album_type":"album","total_tracks":15},"external_ids":{"isrc":"example"}});
    let page = (
        200,
        json!({"tracks":{"items":[item.clone(),item],"total":50,"offset":0,"next":"untrusted-next"}}),
        None,
    );
    let mut mock = Mock::new(vec![token(), page.clone(), page]);
    assert_eq!(mock.client.request_counts(), (0, 0));
    let input = music_library::song_resolution::Input {
        evidence: Default::default(),
        association_providers: vec![],
        spotify_excluded: false,
        album_date: None,
        album_artists: vec![],
        album_required_tracks: 0,
        track_id: music_library::domain::TrackId("application-track".into()),
        primary_artist: None,
        artists: vec![],
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
    assert_eq!(found.items[0].album_total_tracks, Some(15));
    assert_eq!(found.items[0].album_type, "album");
    assert_eq!(
        found.items[0].album_artists[0].identities,
        vec![id("artist", "artist1")]
    );
    let counts = mock.client.request_counts();
    let mut selection =
        music_library::song_resolution::Selection::new(input.clone(), found.items.clone());
    assert!(selection.visible_indices().is_empty());
    selection.show_all();
    assert_eq!(selection.visible_indices(), vec![0]);
    assert_eq!(mock.client.request_counts(), counts);
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
    assert!(requests[1].contains("q=artist%3A%22Artist%22"));
    assert!(requests[1].contains("type=artist"));
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
    let rejected = mock.client.rejected_album_candidates();
    assert_eq!(rejected.len(), 1);
    assert_eq!(rejected[0].identity, id("album", "wrong"));
    assert_eq!(rejected[0].artist_ids, vec![id("artist", "other")]);
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

#[test]
fn featured_display_uses_structured_primary_for_search_and_keeps_provider_artists() {
    let item = json!({"id":"1234567890123456789012","name":"Feel Good Inc.","disc_number":1,"track_number":6,"duration_ms":222640,"artists":[{"id":"primary","name":"Gorillaz"},{"id":"guest","name":"De La Soul"}],"album":{"id":"album","name":"Demon Days","artists":[{"id":"primary","name":"Gorillaz"}],"release_date":"2005"}});
    let mut mock = Mock::new(vec![
        token(),
        (200, json!({"id":"primary","name":"Gorillaz"}), None),
        (
            200,
            json!({"tracks":{"items":[item],"total":1,"offset":0,"next":null}}),
            None,
        ),
    ]);
    let primary = ArtistEvidence {
        name: "Gorillaz".into(),
        identities: vec![id("artist", "primary")],
        join_phrase: " feat. ".into(),
    };
    let input = music_library::song_resolution::Input {
        evidence: Default::default(),
        association_providers: vec![],
        spotify_excluded: false,
        album_date: None,
        album_artists: vec![],
        album_required_tracks: 0,
        track_id: music_library::domain::TrackId("application".into()),
        title: "Feel Good Inc.".into(),
        artist: "Gorillaz feat. De La Soul".into(),
        primary_artist: Some(primary.clone()),
        artists: vec![
            primary,
            ArtistEvidence {
                name: "De La Soul".into(),
                ..Default::default()
            },
        ],
        album: "Demon Days".into(),
        duration_ms: None,
        disc: Some(1),
        number: Some(6),
    };
    let before = input.clone();
    let found = mock.client.search_songs(&input).unwrap();
    assert_eq!(input, before);
    assert_eq!(found.items[0].artists.len(), 2);
    assert_eq!(
        found.items[0].artists[1].identities,
        vec![id("artist", "guest")]
    );
    assert_eq!(
        music_library::song_resolution::assess(&input, &found),
        music_library::song_resolution::Assessment::Unique(0)
    );
    assert_eq!(mock.client.request_counts(), (1, 2));
    let requests = mock.finish();
    let request = requests[2]
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap();
    let url = Url::parse(&format!("http://mock{request}")).unwrap();
    let query = url
        .query_pairs()
        .find(|(k, _)| k == "q")
        .unwrap()
        .1
        .into_owned();
    assert_eq!(query, "track:\"Feel Good Inc.\" artist:\"Gorillaz\"");
}

#[test]
#[ignore = "read-only live Artist-query diagnosis"]
fn live_artist_query_forms() {
    let mut client = Spotify::from_env().unwrap();
    for query in ["artist:\"toe\"", "\"toe\"", "toe"] {
        let page: ArtistSearch = client
            .get(
                "search",
                &[
                    ("q", query.into()),
                    ("type", "artist".into()),
                    ("limit", "10".into()),
                    ("offset", "0".into()),
                ],
            )
            .unwrap();
        println!(
            "QUERY {query:?} total={} more={} artists={:?}",
            page.artists.total,
            page.artists.more(),
            page.artists
                .items
                .iter()
                .map(|a| (&a.name, &a.id))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn artist_name_only_retry_is_bounded_and_preserves_exact_acceptance() {
    let unrelated = (
        200,
        json!({"artists":{"items":[{"id":"wrong","name":"Artist Joe Smith"}],"total":1,"next":null}}),
        None,
    );
    let toe = (
        200,
        json!({"artists":{"items":[{"id":"toe1","name":"toe"},{"id":"other","name":"Lil Toe"}],"total":2,"next":null}}),
        None,
    );
    let mut mock = Mock::new(vec![token(), unrelated.clone(), toe]);
    let page = mock.client.search_artists("toe").unwrap();
    assert_eq!(
        music_library::album_matching::resolve_artist("toe", &page).unwrap(),
        id("artist", "toe1")
    );
    let requests = mock.finish();
    assert_eq!(requests.len(), 3);
    assert!(requests[1].contains("q=artist%3A%22toe%22"));
    assert!(requests[2].contains("q=%22toe%22"));
    assert!(requests[2].contains("limit=10"));
    let mut mock = Mock::new(vec![token(), unrelated.clone(), unrelated]);
    let page = mock.client.search_artists("toe").unwrap();
    assert!(music_library::album_matching::resolve_artist("toe", &page).is_err());
    assert_eq!(mock.finish().len(), 3);
}

#[test]
fn interactive_catalog_search_browse_detail_and_selective_import_are_bounded() {
    let a = json!({"id":"album1","name":"Album","artists":[{"id":"artist1","name":"Artist"}],"release_date":"2020","album_type":"album"});
    let mut hit = song(1);
    hit["album"] = a.clone();
    let mut mock = Mock::new(vec![
        token(),
        (
            200,
            json!({"tracks":{"items":[hit],"total":300,"offset":0,"next":"untrusted"}}),
            None,
        ),
        (
            200,
            json!({"items":[a],"total":100,"offset":10,"next":"untrusted"}),
            None,
        ),
        (
            200,
            json!({"items":[song(1),song(2)],"total":2,"offset":0}),
            None,
        ),
        (200, a.clone(), None),
    ]);
    let songs = mock.client.catalog_songs("Song").unwrap();
    assert_eq!(songs.items.len(), 1);
    assert_eq!(songs.items[0].album.title, "Album");
    let albums = mock
        .client
        .browse_artist(&id("artist", "artist1"), 10)
        .unwrap();
    assert_eq!(albums.next_offset, Some(20));
    let detail = mock.client.catalog_album(&albums.items[0]).unwrap();
    assert_eq!(detail.media[0].tracks.len(), 2);
    assert_eq!(
        detail.media[0].tracks[0].duration,
        Some(music_library::catalog::Duration {
            milliseconds: 200000,
            approximate: false
        })
    );
    let temp = tempfile::tempdir().unwrap();
    let mut library = music_library::Library::open(temp.path().join("catalog.sqlite")).unwrap();
    let imported = library.add_catalog_selection(&detail, &[(1, 1)]).unwrap();
    assert_eq!(
        library.catalog_saved_positions(&detail).unwrap(),
        vec![(1, 1)]
    );
    assert_eq!(library.add_catalog_release(&detail).unwrap(), imported);
    assert_eq!(library.catalog_saved_positions(&detail).unwrap().len(), 2);
    mock.client.catalog_album(&albums.items[0]).unwrap(); // Cached program, no Track lookups.
    let reopened = mock.client.release(&detail.identity).unwrap();
    assert_eq!(reopened.media[0].tracks.len(), 2);
    let requests = mock.finish();
    assert_eq!(
        requests.len(),
        5,
        "one token, one search, one Artist page, one Album page, one reference metadata lookup"
    );
    assert!(requests[1].contains("type=track"));
    assert!(requests[2].contains("artists/artist1/albums"));
    assert!(requests[2].contains("offset=10"));
    assert!(requests[3].contains("albums/album1/tracks"));
}

#[test]
fn artwork_uses_established_album_and_nearest_sufficient_image() {
    let mut mock = Mock::new(vec![
        token(),
        (
            200,
            json!({"images":[
                {"url":"https://cdn/small","width":64},
                {"url":"https://cdn/large","width":1000},
                {"url":"https://cdn/display","width":640}
            ]}),
            None,
        ),
    ]);
    assert_eq!(
        mock.client.artwork_url(&id("album", "album1")).unwrap(),
        Some("https://cdn/display".into())
    );
    let requests = mock.finish();
    assert!(requests[1].starts_with("GET /albums/album1?"));
    assert!(
        !requests
            .iter()
            .any(|r| r.contains("/search") || r.contains("me/player"))
    );
}

#[test]
fn case_equivalent_artist_search_names_share_query_without_merging_identities() {
    let artists = json!({"artists":{"items":[
        {"id":"first","name":"Bygones"},
        {"id":"middle","name":"Other"},
        {"id":"second","name":"BYGONES"}
    ],"total":3}});
    let albums = json!({"albums":{"items":[
        {"id":"album","name":"Spiritual Bankruptcy","artists":[{"id":"second","name":"BYGONES"}],"album_type":"single","total_tracks":5},
        {"id":"wrong","name":"Spiritual Bankruptcy","artists":[{"id":"third","name":"Bygones"}],"album_type":"single","total_tracks":5}
    ],"total":2}});
    let mut mock = Mock::new(vec![token(), (200, artists, None), (200, albums, None)]);
    mock.client.search_artists("Bygones").unwrap();
    let page = mock
        .client
        .scoped_albums(
            &[id("artist", "first"), id("artist", "second")],
            "Spiritual Bankruptcy",
        )
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].artist_ids, vec![id("artist", "second")]);
    assert_eq!(
        mock.client.rejected_album_candidates()[0].identity,
        id("album", "wrong")
    );
    let requests = mock.finish();
    let url = Url::parse(&format!(
        "http://host{}",
        requests[2].split_whitespace().nth(1).unwrap()
    ))
    .unwrap();
    let query = url
        .query_pairs()
        .find(|(k, _)| k == "q")
        .unwrap()
        .1
        .into_owned();
    assert!(
        query.contains(" artist:"),
        "case variants retain Artist-scoped discovery: {query}"
    );
}

#[test]
fn typographic_album_search_uses_general_normalization_without_extra_requests() {
    let mut mock = Mock::new(vec![
        token(),
        (200, json!({"id":"artist1","name":"Artist"}), None),
        (
            200,
            json!({"albums":{"items":[{"id":"album1","name":"X'ed Out","artists":[{"id":"artist1","name":"Artist"}],"release_date":"2013","release_date_precision":"year","album_type":"album","total_tracks":12}],"total":1,"next":null}}),
            None,
        ),
    ]);
    mock.client
        .artist_albums(&id("artist", "artist1"), "X’ed Out")
        .unwrap();
    let requests = mock.finish();
    let url = Url::parse(&format!(
        "http://test{}",
        requests[2].split_whitespace().nth(1).unwrap()
    ))
    .unwrap();
    let query = url
        .query_pairs()
        .find(|(k, _)| k == "q")
        .unwrap()
        .1
        .into_owned();
    assert_eq!(query, "album:\"X'ed Out\" artist:\"Artist\"");
    assert_eq!(requests.len(), 3);
}

#[test]
fn trusted_artist_identity_supplies_current_search_name_without_rewriting_credit() {
    let item = json!({"id":"1234567890123456789012","name":"Song","disc_number":1,"track_number":1,"duration_ms":200000,"artists":[{"id":"renamed","name":"Current band name"}],"album":{"id":"album","name":"Album","artists":[{"id":"renamed","name":"Current band name"}],"release_date":"2013","total_tracks":1}});
    let mut mock = Mock::new(vec![
        token(),
        (
            200,
            json!({"id":"renamed","name":"Current band name"}),
            None,
        ),
        (
            200,
            json!({"tracks":{"items":[item],"total":1,"next":null}}),
            None,
        ),
    ]);
    let primary = ArtistEvidence {
        name: "Historical band name".into(),
        identities: vec![id("artist", "renamed")],
        join_phrase: String::new(),
    };
    let input = music_library::song_resolution::Input {
        track_id: music_library::domain::TrackId("canonical".into()),
        evidence: Default::default(),
        association_providers: vec![],
        spotify_excluded: false,
        album_date: None,
        album_artists: vec![],
        album_required_tracks: 1,
        primary_artist: Some(primary.clone()),
        title: "Song".into(),
        artist: primary.name.clone(),
        artists: vec![primary],
        album: "Album".into(),
        duration_ms: None,
        disc: None,
        number: None,
    };
    let before = input.clone();
    let page = mock.client.search_songs(&input).unwrap();
    assert_eq!(
        music_library::song_resolution::assess(&input, &page),
        music_library::song_resolution::Assessment::Unique(0)
    );
    assert_eq!(input, before);
    let requests = mock.finish();
    let url = Url::parse(&format!(
        "http://test{}",
        requests[2].split_whitespace().nth(1).unwrap()
    ))
    .unwrap();
    assert_eq!(
        url.query_pairs().find(|(k, _)| k == "q").unwrap().1,
        "track:\"Song\" artist:\"Current band name\""
    );
    assert_eq!(requests.len(), 3);
}

#[test]
fn manually_excluded_song_lookup_performs_no_provider_request() {
    use music_library::song_resolution::SongSearch;
    let temp = tempfile::tempdir().unwrap();
    let mut library = music_library::Library::open(temp.path().join("excluded.sqlite")).unwrap();
    let imported = library
        .create_catalog_release(&music_library::domain::CatalogReleaseInput {
            title: "Album".into(),
            year: None,
            artists: vec![],
            tracks: vec![music_library::domain::CatalogTrackInput {
                title: "Song".into(),
                disc_number: None,
                track_number: None,
                artists: vec![],
            }],
        })
        .unwrap();
    library.mark_not_on_spotify(&imported.track_ids[0]).unwrap();
    let input = library
        .song_resolution_input(&imported.track_ids[0])
        .unwrap();
    let mut mock = Mock::new(vec![]);
    let result = mock.client.search_songs(&input).unwrap();
    assert!(result.items.is_empty());
    assert_eq!(mock.client.request_counts(), (0, 0));
}

#[test]
fn persisted_absent_album_discovery_has_zero_repeated_requests_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let mut mock = Mock::new(vec![
        token(),
        artists(),
        (
            200,
            json!({"albums":{"items":[],"total":0,"offset":0,"next":null}}),
            None,
        ),
    ]);
    mock.client = mock.client.with_library(path.to_str()).unwrap();
    mock.client.search_artists("Artist").unwrap();
    let first = mock
        .client
        .artist_albums(&id("artist", "artist1"), "Absent")
        .unwrap();
    assert!(first.items.is_empty());
    let before = mock.client.request_counts();
    // Large absent Album: 150 Tracks share the same discovery context on every pass.
    for _ in 0..3 {
        for _ in 0..150 {
            assert!(
                mock.client
                    .artist_albums(&id("artist", "artist1"), "Absent")
                    .unwrap()
                    .items
                    .is_empty()
            );
        }
    }
    assert_eq!(mock.client.request_counts(), before);
    assert_eq!(mock.client.request_categories()["album search"], 1);
    let mut restarted = Spotify::at(
        Config::new(
            "private-client".into(),
            "private-secret".into(),
            "US".into(),
        )
        .unwrap(),
        mock.client.base.clone(),
        mock.client.token_url.clone(),
    )
    .with_library(path.to_str())
    .unwrap();
    restarted.search_artists("Artist").unwrap();
    assert!(
        restarted
            .artist_albums(&id("artist", "artist1"), "Absent")
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(restarted.request_counts(), (0, 0));
    assert_eq!(mock.finish().len(), 3);
}

#[test]
fn quota_cooldown_stops_remaining_requests_but_keeps_successful_cache() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let mut mock = Mock::new(vec![
        token(),
        artists(),
        (
            429,
            json!({"error":{"status":429,"message":"QUOTA_EXCEEDED"}}),
            Some("5893"),
        ),
    ]);
    mock.client = mock.client.with_library(path.to_str()).unwrap();
    mock.client.search_artists("Artist").unwrap();
    mock.client.refresh_discovery(true);
    assert!(matches!(
        mock.client.search_artists("Artist"),
        Err(CatalogError::RateLimited { .. })
    ));
    let before = mock.client.request_counts();
    for n in 0..150 {
        assert!(matches!(
            mock.client.search_artists(&format!("Other {n}")),
            Err(CatalogError::RateLimited { .. })
        ));
    }
    assert_eq!(mock.client.request_counts(), before);
    let db = music_library::Library::open(&path).unwrap();
    assert!((5892..=5893).contains(&db.spotify_cooldown_remaining(Spotify::now()).unwrap()));
    mock.client.refresh_discovery(false);
    assert_eq!(mock.client.search_artists("Artist").unwrap().items.len(), 1);
    assert_eq!(mock.client.request_counts(), before);
    let mut restarted = Spotify::at(
        Config::new(
            "private-client".into(),
            "private-secret".into(),
            "US".into(),
        )
        .unwrap(),
        mock.client.base.clone(),
        mock.client.token_url.clone(),
    )
    .with_library(path.to_str())
    .unwrap();
    assert!(matches!(
        restarted.search_artists("New"),
        Err(CatalogError::RateLimited { .. })
    ));
    assert_eq!(restarted.request_counts(), (0, 0));
    assert_eq!(mock.finish().len(), 3);
}

#[test]
fn failed_refreshes_preserve_successful_candidates() {
    for status in [401, 403, 500, 429] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let failure = (
            status,
            json!({"error":{"status":status,"message":"failure"}}),
            None,
        );
        let responses = if status == 401 {
            vec![token(), artists(), failure.clone(), token(), failure]
        } else {
            vec![token(), artists(), failure]
        };
        let mut mock = Mock::new(responses);
        mock.client = mock.client.with_library(path.to_str()).unwrap();
        mock.client.search_artists("Artist").unwrap();
        mock.client.refresh_discovery(true);
        assert!(mock.client.search_artists("Artist").is_err());
        let before = mock.client.request_counts();
        mock.client.refresh_discovery(false);
        assert_eq!(mock.client.search_artists("Artist").unwrap().items.len(), 1);
        assert_eq!(mock.client.request_counts(), before);
        mock.finish();
    }
}

#[test]
fn large_absent_album_reconciles_repeatedly_without_network_amplification() {
    for cached in [false, true] {
        use music_library::{
            Library,
            album_matching::{AlbumMatcher, MatchOutcome},
            domain::*,
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let mut library = Library::open(&path).unwrap();
        let credit = ArtistCreditInput {
            name: "Artist".into(),
            role: None,
        };
        let imported = library
            .create_catalog_release(&CatalogReleaseInput {
                title: "Absent".into(),
                year: None,
                artists: vec![credit.clone()],
                tracks: (1..=150)
                    .map(|n| CatalogTrackInput {
                        title: format!("Song {n}"),
                        disc_number: Some(1),
                        track_number: Some(n),
                        artists: vec![credit.clone()],
                    })
                    .collect(),
            })
            .unwrap();
        for track in &imported.track_ids {
            library.add_to_library(track).unwrap();
        }
        let album = library.album_id_for_track(&imported.track_ids[0]).unwrap();
        let empty = (
            200,
            json!({"albums":{"items":[],"total":0,"next":null,"offset":0}}),
            None,
        );
        let mut responses = vec![token(), artists()];
        responses.extend(std::iter::repeat_n(empty, if cached { 1 } else { 4 }));
        let mock = Mock::new(responses);
        let requests = mock.requests.clone();
        let (send, receive) = std::sync::mpsc::channel();
        let mut matcher = AlbumMatcher::for_provider(
            mock.client
                .with_library(if cached { path.to_str() } else { None })
                .unwrap(),
            matching_scope(),
            move |r| {
                send.send(r).unwrap();
            },
            |_| {},
            |_| {},
        )
        .unwrap();
        for pass in 0..4 {
            matcher.match_album(&library, &album).unwrap();
            let reply = receive.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(
                matches!(reply.outcome, MatchOutcome::NoConfidentMatch),
                "{:?}",
                reply.outcome
            );
            matcher.complete(&mut library, reply);
            assert_eq!(
                requests.lock().unwrap().len(),
                if cached { 3 } else { 3 + pass },
                "pass {pass}, cached={cached}"
            );
        }
        for track in &imported.track_ids {
            library.mark_not_on_spotify(track).unwrap();
        }
        assert!(library.spotify_retry_albums(None, 20).unwrap().is_empty());
        matcher.match_album(&library, &album).unwrap();
        assert!(receive.try_recv().is_err());
        assert_eq!(requests.lock().unwrap().len(), if cached { 3 } else { 6 });
        drop(matcher);
        mock.thread.join().unwrap();
    }
}

#[test]
fn track_discovery_reuses_candidates_after_evaluation_only_input_changes() {
    use music_library::{Library, domain::*};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let mut library = Library::open(&path).unwrap();
    let credit = ArtistCreditInput {
        name: "Artist".into(),
        role: None,
    };
    let imported = library
        .create_catalog_release(&CatalogReleaseInput {
            title: "Album".into(),
            year: None,
            artists: vec![credit.clone()],
            tracks: vec![CatalogTrackInput {
                title: "Song".into(),
                disc_number: Some(1),
                track_number: Some(1),
                artists: vec![credit],
            }],
        })
        .unwrap();
    let mut input = library
        .song_resolution_input(&imported.track_ids[0])
        .unwrap();
    let empty = (
        200,
        json!({"tracks":{"items":[],"total":0,"offset":0,"next":null}}),
        None,
    );
    let mut mock = Mock::new(vec![token(), empty.clone(), empty.clone(), empty]);
    mock.client = mock.client.with_library(path.to_str()).unwrap();
    mock.client.search_songs(&input).unwrap();
    let first = mock.client.request_counts();
    input.duration_ms = Some(170000);
    input.album = "New Album evaluation context".into();
    input.album_required_tracks = 150;
    input.disc = Some(2);
    mock.client.search_songs(&input).unwrap();
    assert_eq!(mock.client.request_counts(), first);
    library
        .save_metadata(
            &music_library::metadata::Target::Track(imported.track_ids[0].as_ref().into()),
            &[music_library::metadata::Change {
                field: "genre".into(),
                value: Some("Jazz".into()),
            }],
            &[],
        )
        .unwrap();
    mock.client
        .search_songs(
            &library
                .song_resolution_input(&imported.track_ids[0])
                .unwrap(),
        )
        .unwrap();
    assert_eq!(mock.client.request_counts(), first);
    input.title = "Changed".into();
    mock.client.search_songs(&input).unwrap();
    assert_eq!(mock.client.request_counts().1, first.1 + 1);
    input.primary_artist.as_mut().unwrap().name = "New Artist".into();
    mock.client.search_songs(&input).unwrap();
    assert_eq!(mock.client.request_counts().1, first.1 + 2);
    mock.finish();
}

#[test]
#[ignore = "explicit disposable database, local mock provider only"]
fn disposable_existing_album_cache_audit() {
    use music_library::{
        Library,
        album_matching::{AlbumMatcher, MatchOutcome, Preparation},
        domain::AlbumId,
    };
    let path = std::env::var("SPOTIFY_REPLAY_DATABASE").expect("disposable database");
    let album = AlbumId(std::env::var("SPOTIFY_REPLAY_ALBUM_ID").expect("Album ID"));
    let mut library = Library::open(&path).unwrap();
    let Preparation::Ready(input) = library
        .prepare_album_match_for(&album, &matching_scope())
        .unwrap()
    else {
        panic!("eligible unresolved Album required")
    };
    let artist = input
        .known_artist
        .as_ref()
        .map(|a| a.external_id.as_str())
        .unwrap_or("fixtureArtist");
    let artist_json = json!({"id":artist,"name":input.artist});
    let artist_response = if input.known_artist.is_some() {
        artist_json
    } else {
        json!({"artists":{"items":[artist_json],"total":1,"next":null,"offset":0}})
    };
    let empty = (
        200,
        json!({"albums":{"items":[],"total":0,"next":null,"offset":0}}),
        None,
    );
    let mut responses = vec![token(), (200, artist_response, None), empty.clone()];
    if input.title.split_whitespace().count() > 1 {
        responses.push(empty);
    }
    let mock = Mock::new(responses);
    let requests = mock.requests.clone();
    let (send, receive) = std::sync::mpsc::channel();
    let mut matcher = AlbumMatcher::for_provider(
        mock.client.with_library(Some(&path)).unwrap(),
        matching_scope(),
        move |r| {
            send.send(r).unwrap();
        },
        |_| {},
        |_| {},
    )
    .unwrap();
    let mut first = 0;
    for pass in 0..4 {
        matcher.match_album(&library, &album).unwrap();
        let reply = receive.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            matches!(reply.outcome, MatchOutcome::NoConfidentMatch),
            "{:?}",
            reply.outcome
        );
        matcher.complete(&mut library, reply);
        let count = requests.lock().unwrap().len();
        if pass == 0 {
            first = count;
        } else {
            assert_eq!(count, first);
        }
        println!(
            "disposable Album pass {pass}: cumulative mock HTTP={count}; additional={}",
            if pass == 0 { count } else { 0 }
        );
    }
    println!(
        "Track count={}; live Spotify requests=0",
        library.local_album_tracks(&album).unwrap().len()
    );
    drop(matcher);
    mock.thread.join().unwrap();
}

#[test]
fn network_and_malformed_refresh_preserve_the_old_successful_result() {
    for malformed in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let mut responses = vec![token(), artists()];
        if malformed {
            responses.push((200, json!({"malformed":true}), None));
        }
        let mut mock = Mock::new(responses);
        mock.client = mock.client.with_library(path.to_str()).unwrap();
        mock.client.search_artists("Artist").unwrap();
        mock.client.refresh_discovery(true);
        assert!(mock.client.search_artists("Artist").is_err());
        let before = mock.client.request_counts();
        mock.client.refresh_discovery(false);
        assert_eq!(mock.client.search_artists("Artist").unwrap().items.len(), 1);
        assert_eq!(mock.client.request_counts(), before);
        mock.finish();
    }
}

#[test]
fn expired_negative_search_is_eligible_and_album_title_changes_rekey_discovery() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let empty = (
        200,
        json!({"albums":{"items":[],"total":0,"offset":0,"next":null}}),
        None,
    );
    let mut mock = Mock::new(vec![
        token(),
        artists(),
        empty.clone(),
        empty.clone(),
        empty,
    ]);
    mock.client = mock.client.with_library(path.to_str()).unwrap();
    mock.client.search_artists("Artist").unwrap();
    mock.client
        .artist_albums(&id("artist", "artist1"), "Absent")
        .unwrap();
    let query_path = mock
        .requests
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_owned();
    let url = mock.client.base.join(&query_path).unwrap();
    let key = format!(
        "spotify:{}:{}",
        music_library::spotify_lifecycle::SEARCH_STRATEGY_VERSION,
        url
    );
    let db = music_library::Library::open(&path).unwrap();
    db.persist_spotify_discovery(
        &key,
        &json!({"albums":{"items":[],"total":0,"offset":0,"next":null}}).to_string(),
        Spotify::now() - music_library::spotify_lifecycle::DISCOVERY_TTL_SECONDS - 1,
    )
    .unwrap();
    mock.client
        .artist_albums(&id("artist", "artist1"), "Absent")
        .unwrap();
    assert_eq!(mock.client.request_categories()["album search"], 2);
    assert_eq!(
        mock.client.request_audit().back().unwrap().reason,
        DiscoveryRequestReason::Expired
    );
    mock.client
        .artist_albums(&id("artist", "artist1"), "Renamed")
        .unwrap();
    assert_eq!(mock.client.request_categories()["album search"], 3);
    assert_eq!(
        mock.client.request_audit().back().unwrap().reason,
        DiscoveryRequestReason::NoCompletedResult
    );
    mock.client
        .artist_albums(&id("artist", "artist1"), "Renamed")
        .unwrap();
    assert_eq!(mock.client.request_categories()["album search"], 3);
    mock.finish();
}

#[test]
fn three_cached_album_candidates_can_be_accepted_after_historical_rejection() {
    use music_library::{
        Library,
        album_matching::{AlbumMatcher, MatchOutcome},
        domain::*,
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let mut library = Library::open(&path).unwrap();
    let credit = ArtistCreditInput {
        name: "Artist".into(),
        role: None,
    };
    let imported = library
        .create_catalog_release(&CatalogReleaseInput {
            title: "Album".into(),
            year: None,
            artists: vec![credit.clone()],
            tracks: (1..=3)
                .map(|n| CatalogTrackInput {
                    title: format!("Song {n}"),
                    disc_number: Some(1),
                    track_number: Some(n),
                    artists: vec![credit.clone()],
                })
                .collect(),
        })
        .unwrap();
    for track in &imported.track_ids {
        library.add_to_library(track).unwrap();
    }
    let album = library.album_id_for_track(&imported.track_ids[0]).unwrap();
    let candidates=[("album1","Album"),("album2","Album (Live)"),("album3","Album (Remix)")].into_iter().map(|(id,name)|json!({"id":id,"name":name,"artists":[{"id":"artist1","name":"Artist"}],"album_type":"album","total_tracks":3})).collect::<Vec<_>>();
    let tracks = (1..=3)
        .map(|n| {
            let mut s = song(n);
            s["id"] = json!(format!("{n:022}"));
            s
        })
        .collect::<Vec<_>>();
    let mock = Mock::new(vec![
        token(),
        artists(),
        (
            200,
            json!({"albums":{"items":candidates,"total":3,"next":null,"offset":0}}),
            None,
        ),
        (
            200,
            json!({"items":tracks,"total":3,"offset":0,"next":null}),
            None,
        ),
    ]);
    let requests = mock.requests.clone();
    let (send, receive) = std::sync::mpsc::channel();
    let mut matcher = AlbumMatcher::for_provider(
        mock.client.with_library(path.to_str()).unwrap(),
        matching_scope(),
        move |r| {
            send.send(r).unwrap();
        },
        |_| {},
        |_| {},
    )
    .unwrap();
    matcher.match_album(&library, &album).unwrap();
    let mut historical = receive.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        matches!(
            historical.outcome,
            MatchOutcome::Matched(_) | MatchOutcome::MatchedClose(_)
        ),
        "{:?}",
        historical.outcome
    );
    // Simulate an older rejecting evaluator without changing successful discovery.
    historical.outcome = MatchOutcome::NoConfidentMatch;
    historical.matched_album = None;
    historical.diagnostic = None;
    matcher.complete(&mut library, historical);
    assert!(
        library
            .track_provider_occurrences(&imported.track_ids[0], "spotify")
            .unwrap()
            .is_empty()
    );
    let before = requests.lock().unwrap().len();
    matcher.match_album(&library, &album).unwrap();
    let current = receive.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        matches!(
            current.outcome,
            MatchOutcome::Matched(_) | MatchOutcome::MatchedClose(_)
        ),
        "{:?}",
        current.outcome
    );
    matcher.complete(&mut library, current);
    assert_eq!(requests.lock().unwrap().len(), before);
    assert_eq!(
        library
            .list_album_external_identities(&album)
            .unwrap()
            .iter()
            .filter(|id| id.provider == "spotify")
            .count(),
        1
    );
    drop(matcher);
    mock.thread.join().unwrap();
}

#[test]
fn catalog_timeout_during_refresh_preserves_successful_discovery() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let server = thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        thread::sleep(Duration::from_millis(50));
        drop(socket);
    });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let mut client = Spotify::at(
        Config::new(
            "private-client".into(),
            "private-secret".into(),
            "US".into(),
        )
        .unwrap(),
        base.clone(),
        base.join("token").unwrap(),
    )
    .with_library(path.to_str())
    .unwrap();
    client.agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_millis(10)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into();
    let mut url = base.join("search").unwrap();
    url.query_pairs_mut()
        .append_pair("market", "US")
        .append_pair("q", "artist:\"Artist\"")
        .append_pair("type", "artist")
        .append_pair("limit", "10")
        .append_pair("offset", "0");
    let key = format!(
        "spotify:{}:{}",
        music_library::spotify_lifecycle::SEARCH_STRATEGY_VERSION,
        url
    );
    let db = music_library::Library::open(&path).unwrap();
    db.persist_spotify_discovery(&key, &artists().1.to_string(), Spotify::now())
        .unwrap();
    client.refresh_discovery(true);
    assert!(matches!(
        client.search_artists("Artist"),
        Err(CatalogError::Timeout(_))
    ));
    let before = client.request_counts();
    client.refresh_discovery(false);
    assert_eq!(client.search_artists("Artist").unwrap().items.len(), 1);
    assert_eq!(client.request_counts(), before);
    server.join().unwrap();
}

#[test]
fn diagnostic_cache_only_lookup_never_authenticates_or_queries_on_misses() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let mut mock = Mock::new(vec![
        token(),
        (
            200,
            json!({"tracks":{"items":[],"total":0,"offset":0,"next":null}}),
            None,
        ),
    ]);
    mock.client = mock.client.with_library(path.to_str()).unwrap();
    let mut input = music_library::song_resolution::Input {
        track_id: music_library::domain::TrackId("diagnostic".into()),
        spotify_excluded: false,
        evidence: Default::default(),
        association_providers: vec![],
        album_date: None,
        album_artists: vec![],
        album_required_tracks: 0,
        primary_artist: None,
        title: "Song".into(),
        artist: "Artist".into(),
        artists: vec![],
        album: "Album".into(),
        duration_ms: None,
        disc: None,
        number: None,
    };
    mock.client.search_songs(&input).unwrap();
    let mut offline = Spotify::at(
        Config::new("offline".into(), "offline".into(), "US".into()).unwrap(),
        mock.client.base.clone(),
        mock.client.token_url.clone(),
    )
    .with_library(path.to_str())
    .unwrap();
    assert!(offline.try_cached_songs(&input).unwrap().is_some());
    input.title = "Changed".into();
    assert!(offline.try_cached_songs(&input).unwrap().is_none());
    input.title = "Song".into();
    offline.config.market = "GB".into();
    assert!(offline.try_cached_songs(&input).unwrap().is_none());
    assert_eq!(offline.request_counts(), (0, 0));
    assert_eq!(mock.finish().len(), 2);
}
