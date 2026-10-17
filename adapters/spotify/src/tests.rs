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
        recording: Default::default(),
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
        Self::with_hook(responses, None)
    }
    fn with_hook(
        responses: Vec<(u16, Value, Option<&str>)>,
        hook: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
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
                if let Some(hook) = &hook {
                    hook();
                }
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
    let mut mock = Mock::new(vec![
        token(),
        artists(),
        empty.clone(),
        found,
        empty.clone(),
        empty,
    ]);
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
    assert_eq!(requests.len(), 6);
    assert!(requests[3].contains("Bitches+Aint+Shit+But+Good+People+Artist"));
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

fn retained_album_context(
    local_titles: &[&str],
    provider_titles: &[&str],
) -> (
    tempfile::TempDir,
    music_library::Library,
    music_library::domain::ImportedRelease,
    music_library::domain::AlbumId,
) {
    use music_library::domain::*;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("context.db");
    let mut library = music_library::Library::open(&path).unwrap();
    let credit = ArtistCreditInput {
        name: "Artist".into(),
        role: None,
    };
    let release = library
        .create_catalog_release(&CatalogReleaseInput {
            title: "Album".into(),
            year: None,
            artists: vec![credit.clone()],
            tracks: local_titles
                .iter()
                .enumerate()
                .map(|(i, title)| CatalogTrackInput {
                    title: (*title).into(),
                    disc_number: Some(1),
                    track_number: Some(i as u32 + 1),
                    artists: vec![credit.clone()],
                })
                .collect(),
        })
        .unwrap();
    for t in &release.track_ids {
        library.add_to_library(t).unwrap();
    }
    let album = library.album_id_for_track(&release.track_ids[0]).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    let artist = ArtistId(
        db.query_row(
            "SELECT artist_id FROM album_artist_credit WHERE album_id=?1",
            [album.as_ref()],
            |r| r.get(0),
        )
        .unwrap(),
    );
    library
        .attach_artist_external_identity(&artist, &id("artist", "artist1"))
        .unwrap();
    library
        .persist_spotify_discovery("spotify-name:1:US:artist1", "\"Artist\"", Spotify::now())
        .unwrap();
    let mut query = Url::parse("https://api.spotify.com/v1/search").unwrap();
    let music_library::album_matching::Preparation::Ready(input) =
        library.spotify_album_review_input(&album).unwrap()
    else {
        panic!("expected searchable Album")
    };
    let album_query = format!("album:{} artist:{}", quoted(&input.title), quoted("Artist"));
    query
        .query_pairs_mut()
        .append_pair("market", "US")
        .append_pair("q", &album_query)
        .append_pair("type", "album")
        .append_pair("limit", "10")
        .append_pair("offset", "0");
    library.persist_spotify_discovery(&format!("spotify:{}:{query}",music_library::spotify_lifecycle::SEARCH_STRATEGY_VERSION),&json!({"albums":{"items":[{"id":"album1","name":"Album","artists":[{"id":"artist1","name":"Artist"}],"album_type":"album","total_tracks":provider_titles.len()}],"total":1,"offset":0,"next":null}}).to_string(),Spotify::now()).unwrap();
    let tracks = provider_titles
        .iter()
        .enumerate()
        .map(|(i, title)| {
            let mut t = song(i as u32 + 1);
            t["name"] = json!(title);
            t
        })
        .collect::<Vec<_>>();
    library
        .persist_spotify_discovery(
            "spotify:1:https://api.spotify.com/v1/albums/album1/tracks?market=US&limit=50&offset=0",
            &json!({"items":tracks,"total":provider_titles.len(),"offset":0,"next":null})
                .to_string(),
            Spotify::now(),
        )
        .unwrap();
    (dir, library, release, album)
}

#[test]
fn sole_deluxe_cached_program_connects_only_corresponding_canonical_tracks() {
    let (dir, mut library, release, album) = retained_album_context(
        &["A", "B", "C", "D", "Local Only"],
        &["A", "B", "C", "D", "Provider Bonus"],
    );
    let db = rusqlite::Connection::open(dir.path().join("context.db")).unwrap();
    let (key, response): (String, String) = db.query_row("SELECT fingerprint,response_json FROM spotify_discovery WHERE fingerprint LIKE '%type=album%'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    let mut response: serde_json::Value = serde_json::from_str(&response).unwrap();
    response["albums"]["items"][0]["name"] = json!("Album (Deluxe Edition)");
    library
        .persist_spotify_discovery(&key, &response.to_string(), Spotify::now())
        .unwrap();
    let batch = album_context::reconcile(&mut library, std::slice::from_ref(&album), "US").unwrap();
    assert_eq!(batch.accepted.len(), 4);
    assert_eq!(library.unresolved_spotify_count().unwrap(), 1);
    for t in &release.track_ids[..4] {
        assert_eq!(
            library
                .track_provider_occurrences(t, "spotify")
                .unwrap()
                .len(),
            1
        );
    }
    assert!(
        library
            .track_provider_occurrences(&release.track_ids[4], "spotify")
            .unwrap()
            .is_empty()
    );
    assert_eq!(library.library_queue(&Default::default()).unwrap().len(), 5);
    assert_eq!(
        db.query_row("SELECT count(*) FROM track", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        5
    );
    assert_eq!(
        db.query_row(
            "SELECT match_title FROM effective_album_metadata WHERE album_id=?1",
            [album.as_ref()],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "album"
    );
    assert!(
        album_context::reconcile(&mut library, &[album], "US")
            .unwrap()
            .accepted
            .is_empty()
    );
}
#[test]
fn retained_album_occurrences_propagate_without_track_search_and_complete_one_local_retry() {
    let (_dir, mut library, release, album) =
        retained_album_context(&["A", "B", "C"], &["A", "B", "C"]);
    let before = library.unresolved_spotify_count().unwrap();
    assert_eq!(before, 3);
    let (selection, more) = album_context::selection(&library, &release.track_ids[0], "US")
        .unwrap()
        .unwrap();
    assert!(!more);
    assert_eq!(selection.candidates().len(), 1);
    assert_eq!(
        selection.candidate_provenance(0),
        "Found through Album candidate"
    );
    assert_eq!(
        library.unresolved_spotify_count().unwrap(),
        3,
        "inspection is read-only"
    );
    let first = album_context::reconcile(&mut library, std::slice::from_ref(&album), "US").unwrap();
    assert_eq!(first.accepted.len(), 3);
    assert_eq!(library.unresolved_spotify_count().unwrap(), 0);
    let second = album_context::reconcile(&mut library, &[album], "US").unwrap();
    assert!(second.accepted.is_empty());
    assert_eq!(second.examined, 0);
}

#[test]
fn album_context_exposes_spelling_and_wording_differences_without_fuzzy_acceptance() {
    let (_dir, mut library, r, album) = retained_album_context(
        &[
            "A",
            "B",
            "C",
            "D",
            "E",
            "F",
            "G",
            "H",
            "Quetzal",
            "If I Sit Still Maybe I'll Make It Out of Here",
        ],
        &[
            "A",
            "B",
            "C",
            "D",
            "E",
            "F",
            "G",
            "H",
            "Quetzel",
            "If I Sit Still Maybe I'll Get Out of Here",
        ],
    );
    let (selection, _) = album_context::selection(&library, &r.track_ids[8], "US")
        .unwrap()
        .unwrap();
    assert!(selection.candidates().iter().any(|c| c.title == "Quetzel"));
    assert_eq!(
        selection.candidates()[selection.visible_indices()[0]].title,
        "Quetzel",
        "lexical evidence ranks the plausible spelling first without accepting it"
    );
    assert_eq!(
        selection.candidate_provenance(0),
        "Found through Album candidate"
    );
    let page = Page {
        items: selection.candidates().to_vec(),
        next_offset: None,
    };
    assert!(
        library
            .apply_song_evaluation(selection.input(), &page)
            .unwrap()
            .is_none()
    );
    let (selection, _) = album_context::selection(&library, &r.track_ids[9], "US")
        .unwrap()
        .unwrap();
    assert!(
        selection
            .candidates()
            .iter()
            .any(|c| c.title.contains("Get Out"))
    );
    library.mark_not_on_spotify(&r.track_ids[9]).unwrap();
    album_context::reconcile(&mut library, &[album], "US").unwrap();
    assert!(
        library
            .track_provider_occurrences(&r.track_ids[9], "spotify")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn single_series_title_has_one_bounded_artist_boilerplate_fallback_and_cache_reuse() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("series.db");
    let mut mock = Mock::new(vec![
        token(),
        artists(),
        (
            200,
            json!({"albums":{"items":[],"total":0,"offset":0,"next":null}}),
            None,
        ),
        (
            200,
            json!({"albums":{"items":[{"id":"album1","name":"Artist on Audiotree Live","artists":[{"id":"artist1","name":"Artist"}],"album_type":"album","total_tracks":3}],"total":1,"offset":0,"next":null}}),
            None,
        ),
    ]);
    mock.client = mock.client.with_library(path.to_str()).unwrap();
    mock.client.search_artists("Artist").unwrap();
    let page = mock
        .client
        .artist_albums(&id("artist", "artist1"), "Audiotree")
        .unwrap();
    assert_eq!(page.items[0].title, "Artist on Audiotree Live");
    let before = mock.client.request_counts();
    mock.client
        .artist_albums(&id("artist", "artist1"), "Audiotree")
        .unwrap();
    assert_eq!(mock.client.request_counts(), before);
    let requests = mock.finish();
    assert_eq!(requests.len(), 4);
    assert!(requests[3].contains("%22Artist%22+Audiotree"));
    assert!(!requests[3].contains("album%3A") && !requests[3].contains("artist%3A"));
    assert!(
        requests
            .iter()
            .all(|r| !r.contains("/artists/artist1/albums"))
    );
}
#[test]
fn series_live_fallback_never_adds_a_third_search() {
    let empty = || {
        (
            200,
            json!({"albums":{"items":[],"total":0,"offset":0,"next":null}}),
            None,
        )
    };
    let mut mock = Mock::new(vec![token(), artists(), empty(), empty()]);
    mock.client.search_artists("Artist").unwrap();
    assert!(
        mock.client
            .artist_albums(&id("artist", "artist1"), "Audiotree Live")
            .unwrap()
            .items
            .is_empty()
    );
    let requests = mock.finish();
    assert_eq!(requests.len(), 4);
    assert!(requests[3].contains("Artist+on+Audiotree+Live"));
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
        (200, json!({"albums":{"items":[],"total":0}}), None),
    ]);
    mock.client
        .artist_albums(&id("artist", "artist1"), "Album")
        .unwrap();
    let requests = mock.finish();
    assert_eq!(requests.len(), 4);
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
        recording: Default::default(),
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
        recording: Default::default(),
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
    assert_eq!(mock.client.request_categories()["album search"], 2);
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
    assert_eq!(mock.finish().len(), 4);
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
            mock.client
                .search_artists(&format!("Unrelated provider title number {n}")),
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
        responses.extend(std::iter::repeat_n(empty, if cached { 2 } else { 8 }));
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
                if cached { 4 } else { 4 + pass * 2 },
                "pass {pass}, cached={cached}"
            );
        }
        for track in &imported.track_ids {
            library.mark_not_on_spotify(track).unwrap();
        }
        assert!(library.spotify_retry_albums(None, 20).unwrap().is_empty());
        matcher.match_album(&library, &album).unwrap();
        assert!(receive.try_recv().is_err());
        assert_eq!(requests.lock().unwrap().len(), if cached { 4 } else { 10 });
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
    assert_eq!(mock.client.request_categories()["album search"], 3);
    assert_eq!(
        mock.client.request_audit().back().unwrap().reason,
        DiscoveryRequestReason::Expired
    );
    mock.client
        .artist_albums(&id("artist", "artist1"), "Renamed")
        .unwrap();
    assert_eq!(mock.client.request_categories()["album search"], 5);
    assert_eq!(
        mock.client.request_audit().back().unwrap().reason,
        DiscoveryRequestReason::NoCompletedResult
    );
    mock.client
        .artist_albums(&id("artist", "artist1"), "Renamed")
        .unwrap();
    assert_eq!(mock.client.request_categories()["album search"], 5);
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
    let key = format!("spotify:{}:{}", 1, url);
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
        recording: Default::default(),
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

#[test]
fn album_inspection_exposes_every_cached_candidate_without_requests_or_ttl_refresh() {
    use music_library::{Library, album_matching::Preparation, domain::*};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("review.db");
    let mut library = Library::open(&path).unwrap();
    let credit = ArtistCreditInput {
        name: "Artist".into(),
        role: None,
    };
    let r = library
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
    for t in &r.track_ids {
        library.add_to_library(t).unwrap();
    }
    let album = library.album_id_for_track(&r.track_ids[0]).unwrap();
    let candidates=[("album1","Album"),("album2","Album (Live)"),("album3","Album (Remix)")].into_iter().map(|(id,name)|json!({"id":id,"name":name,"artists":[{"id":"artist1","name":"Artist"}],"album_type":"album","total_tracks":3})).collect::<Vec<_>>();
    let program = (
        200,
        json!({"items":(1..=3).map(song).collect::<Vec<_>>(),"total":3,"offset":0,"next":null}),
        None,
    );
    let mut mock = Mock::new(vec![
        token(),
        artists(),
        (
            200,
            json!({"albums":{"items":candidates,"total":3,"next":null,"offset":0}}),
            None,
        ),
        program.clone(),
        program.clone(),
        program,
    ]);
    let base = mock.client.base.clone();
    let token_url = mock.client.token_url.clone();
    let market = mock.client.config.market.clone();
    let config = || Config::new("offline".into(), "offline".into(), market.clone()).unwrap();
    mock.client = mock.client.with_library(path.to_str()).unwrap();
    let Preparation::Ready(initial) = library.spotify_album_review_input(&album).unwrap() else {
        panic!("initial input");
    };
    let found = mock.client.search_artists(&initial.artist).unwrap();
    library
        .attach_artist_external_identity(&initial.artist_id, &found.items[0].identity)
        .unwrap();
    let page = mock
        .client
        .artist_albums(&found.items[0].identity, &initial.title)
        .unwrap();
    let mut all = page.items;
    all.extend(mock.client.rejected_album_candidates());
    for c in all {
        mock.client.album_programs(&c.identity).unwrap();
    }
    let before = mock.requests.lock().unwrap().len();
    let Preparation::Ready(input) = library.spotify_album_review_input(&album).unwrap() else {
        panic!("review input");
    };
    let db = rusqlite::Connection::open(&path).unwrap();
    let observations: Vec<(String, i64)> = db
        .prepare("SELECT fingerprint,completed_at FROM spotify_discovery ORDER BY fingerprint")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    for _ in 0..3 {
        let offline = Spotify::at(config(), base.clone(), token_url.clone())
            .with_library(path.to_str())
            .unwrap();
        let (report, programs) = Spotify::cached_album_review_with_client(offline, &input)
            .unwrap()
            .unwrap();
        assert_eq!(report.candidates.len(), 3);
        assert_eq!(programs.len(), 3);
        assert!(report.candidates.iter().all(|c| !c.programs.is_empty()));
        assert_eq!(mock.requests.lock().unwrap().len(), before);
    }
    let after: Vec<(String, i64)> = db
        .prepare("SELECT fingerprint,completed_at FROM spotify_discovery ORDER BY fingerprint")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(observations, after);
    db.execute("UPDATE spotify_discovery SET completed_at=0", [])
        .unwrap();
    let offline = Spotify::at(config(), base, token_url)
        .with_library(path.to_str())
        .unwrap();
    assert_eq!(
        Spotify::cached_album_review_with_client(offline, &input)
            .unwrap()
            .unwrap()
            .0
            .candidates
            .len(),
        3,
        "retained evidence remains inspectable after TTL expires"
    );
    assert_eq!(mock.requests.lock().unwrap().len(), before);
    mock.thread.join().unwrap();
}

#[test]
fn pasted_album_and_track_fetch_exact_objects_and_reuse_cache() {
    let album_id = "1234567890123456789012";
    let track_id = "2234567890123456789012";
    let album = json!({"id":album_id,"name":"Provider title unrelated to local Japanese title","artists":[{"id":"artist1","name":"A"}],"album_type":"album","total_tracks":1,"release_date":"2020","release_date_precision":"year"});
    let track = json!({"id":track_id,"name":"Provider Track","disc_number":1,"track_number":1,"duration_ms":123456,"artists":[{"id":"artist1","name":"A"}],"album":album.clone()});
    let mut mock = Mock::new(vec![
        token(),
        (200, album, None),
        (
            200,
            json!({"items":[track.clone()],"offset":0,"total":1,"next":null}),
            None,
        ),
        (200, track, None),
    ]);
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("library.db");
    mock.client = mock.client.with_library(path.to_str()).unwrap();
    let first = mock
        .client
        .explicit_album(&format!("spotify:album:{album_id}"))
        .unwrap();
    assert_eq!(first.1.programs[0].tracks.len(), 1);
    assert_eq!(
        mock.client
            .explicit_album(&format!("https://open.spotify.com/album/{album_id}?si=x"))
            .unwrap()
            .0
            .identity,
        first.0.identity
    );
    let first = mock.client.explicit_track(track_id).unwrap();
    assert_eq!(
        mock.client
            .explicit_track(&format!("spotify:track:{track_id}"))
            .unwrap(),
        first
    );
    let requests = mock.finish();
    assert_eq!(requests.len(), 4);
    assert!(requests.iter().all(|r| !r.contains("search?")));
}

#[test]
fn bounded_playable_fallback_discovers_only_two_unresolved_tracks_and_reuses_cache() {
    struct Search {
        calls: usize,
    }
    impl music_library::song_resolution::SongSearch for Search {
        fn search_songs(
            &mut self,
            input: &music_library::song_resolution::Input,
        ) -> Result<
            music_library::catalog::Page<music_library::song_resolution::Candidate>,
            music_library::catalog::CatalogError,
        > {
            self.calls += 1;
            Ok(music_library::catalog::Page {
                items: vec![music_library::song_resolution::Candidate {
                    recording: Default::default(),
                    identity: music_library::domain::ExternalIdentity {
                        provider: "spotify".into(),
                        kind: "track".into(),
                        external_id: format!("single-{}", input.track_id.as_ref()),
                    },
                    album_identity: None,
                    title: input.title.clone(),
                    artist: input.artist.clone(),
                    artists: input.album_artists.clone(),
                    album: "Separate provider single".into(),
                    date: String::new(),
                    album_artists: input.album_artists.clone(),
                    album_type: "single".into(),
                    album_total_tracks: Some(1),
                    duration_ms: 200000,
                    disc: 1,
                    number: 1,
                }],
                next_offset: None,
            })
        }
    }
    let (_dir, mut library, imported, album) = retained_album_context(
        &["A", "B", "C", "D"],
        &["Unrelated 1", "Unrelated 2", "Unrelated 3", "Unrelated 4"],
    );
    let mut search = Search { calls: 0 };
    let result = crate::album_context::reconcile_best_available(
        &mut library,
        std::slice::from_ref(&album),
        "US",
        &mut search,
    )
    .unwrap();
    assert_eq!(
        search.calls,
        2,
        "batch {result:?}, input {:?}",
        library
            .song_resolution_input(&imported.track_ids[0])
            .unwrap()
    );
    assert_eq!(result.accepted.len(), 2);
    assert_eq!(
        library.album_id_for_track(&imported.track_ids[0]).unwrap(),
        album
    );
    let before = search.calls;
    let _ = crate::album_context::reconcile_best_available(
        &mut library,
        std::slice::from_ref(&album),
        "US",
        &mut search,
    )
    .unwrap();
    assert_eq!(search.calls - before, 0); // unchanged bulk window is not reset by another click
    let before = search.calls;
    let result = crate::album_context::reconcile_best_available(
        &mut library,
        std::slice::from_ref(&album),
        "US",
        &mut search,
    )
    .unwrap();
    assert_eq!(search.calls, before);
    assert!(result.accepted.is_empty());
}

#[test]
fn failed_playable_discovery_is_cached_and_provider_errors_abort_the_batch() {
    struct Search {
        calls: usize,
        error: bool,
    }
    impl music_library::song_resolution::SongSearch for Search {
        fn search_songs(
            &mut self,
            _: &music_library::song_resolution::Input,
        ) -> Result<
            music_library::catalog::Page<music_library::song_resolution::Candidate>,
            music_library::catalog::CatalogError,
        > {
            self.calls += 1;
            if self.error {
                Err(music_library::catalog::CatalogError::Other(
                    "provider unavailable".into(),
                ))
            } else {
                Ok(music_library::catalog::Page {
                    items: vec![],
                    next_offset: None,
                })
            }
        }
    }
    let (_dir, mut library, _, album) =
        retained_album_context(&["A", "B"], &["Unrelated 1", "Unrelated 2"]);
    let mut search = Search {
        calls: 0,
        error: false,
    };
    crate::album_context::reconcile_best_available(
        &mut library,
        std::slice::from_ref(&album),
        "US",
        &mut search,
    )
    .unwrap();
    assert_eq!(search.calls, 2);
    assert!(
        !library.spotify_album_needs_retry(&album).unwrap(),
        "completed evidence is a stable exception"
    );
    let music_library::album_matching::Preparation::Ready(input) =
        library.spotify_album_review_input(&album).unwrap()
    else {
        panic!("review context");
    };
    let (report, _) = Spotify::cached_album_review(&library.database_path().unwrap(), "US", &input)
        .unwrap()
        .unwrap();
    assert!(
        !report.candidates.is_empty(),
        "stable review always has inspectable evidence"
    );
    crate::album_context::reconcile_best_available(
        &mut library,
        std::slice::from_ref(&album),
        "US",
        &mut search,
    )
    .unwrap();
    assert_eq!(search.calls, 2);
    let (_dir, mut library, _, album) =
        retained_album_context(&["A", "B"], &["Unrelated 1", "Unrelated 2"]);
    let mut search = Search {
        calls: 0,
        error: true,
    };
    let batch = crate::album_context::reconcile_best_available(
        &mut library,
        std::slice::from_ref(&album),
        "US",
        &mut search,
    )
    .unwrap();
    assert_eq!(search.calls, 1);
    assert_eq!(batch.errors.len(), 1);
}

#[test]
fn fresh_apostrophe_discovery_uses_bounded_plain_query_and_persistent_cache() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut mock = Mock::new(vec![
        token(),
        artists(),
        (
            200,
            json!({"albums":{"items":[],"total":0,"offset":0,"next":null}}),
            None,
        ),
        (
            200,
            json!({"albums":{"items":[{"id":"album1","name":"X’ed Out","artists":[{"id":"artist1","name":"Artist"}],"album_type":"album","total_tracks":12}],"total":1,"offset":0,"next":null}}),
            None,
        ),
    ]);
    mock.client = mock.client.with_library(path.to_str()).unwrap();
    mock.client.search_artists("Artist").unwrap();
    for title in ["X'ed Out", "X’ed Out", "X‘ed Out", "Xʼed Out", "X＇ed Out"] {
        let page = mock
            .client
            .artist_albums(&id("artist", "artist1"), title)
            .unwrap();
        assert_eq!(page.items[0].title, "X’ed Out");
        assert!(matches!(
            music_library::album_matching::accepted_album(title, &id("artist", "artist1"), &page),
            music_library::album_matching::MatchOutcome::Matched(_)
        ));
    }
    assert_eq!(mock.client.request_counts(), (1, 3));
    let requests = mock.finish();
    assert!(requests[2].contains("X%27ed+Out"));
    assert!(requests[3].contains("Xed+Out+Artist"));
    assert!(!requests[3].contains("album%3A"));
}

#[test]
fn provider_requests_wait_without_holding_sqlite_writer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    music_library::Library::open(&path).unwrap();
    let writer_path = path.clone();
    let hook = Arc::new(move || {
        let db = rusqlite::Connection::open(&writer_path).unwrap();
        db.busy_timeout(std::time::Duration::from_millis(100))
            .unwrap();
        db.execute_batch("BEGIN IMMEDIATE; INSERT INTO spotify_discovery VALUES('proof','{}',1) ON CONFLICT DO NOTHING; COMMIT;").unwrap();
    });
    let mut mock = Mock::with_hook(vec![token(), artists()], Some(hook));
    mock.client = mock.client.with_library(path.to_str()).unwrap();
    assert_eq!(mock.client.search_artists("Artist").unwrap().items.len(), 1);
    assert_eq!(mock.finish().len(), 2);
}

#[test]
fn different_equivalent_artist_names_get_one_bounded_scoped_discovery_fallback() {
    let empty = json!({"albums":{"items":[],"total":0,"next":null}});
    let found = json!({"albums":{"items":[{"id":"first-contact","name":"First Contact","artists":[{"id":"alias","name":"tsosis"}],"album_type":"album","total_tracks":10}],"total":1,"next":null}});
    let mut mock = Mock::new(vec![token(), (200, empty, None), (200, found, None)]);
    mock.client
        .artist_names
        .insert("alias".into(), "tsosis".into());
    mock.client
        .artist_names
        .insert("canonical".into(), "ZZ Canonical Artist".into());
    let page = mock
        .client
        .scoped_albums(
            &[id("artist", "canonical"), id("artist", "alias")],
            "First Contact",
        )
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].identity.external_id, "first-contact");
    assert_eq!(mock.client.request_counts(), (1, 2));
    let requests = mock.finish();
    let url = Url::parse(&format!(
        "http://host{}",
        requests[2].split_whitespace().nth(1).unwrap()
    ))
    .unwrap();
    let query = url
        .query_pairs()
        .find(|(key, _)| key == "q")
        .unwrap()
        .1
        .into_owned();
    assert!(query.contains("artist:\"ZZ Canonical Artist\""), "{query}");
}

#[test]
fn playable_search_allowance_tracks_actual_calls_across_internal_batches() {
    struct Search(usize);
    impl music_library::song_resolution::SongSearch for Search {
        fn search_songs(
            &mut self,
            _: &music_library::song_resolution::Input,
        ) -> Result<Page<music_library::song_resolution::Candidate>, CatalogError> {
            self.0 += 1;
            Ok(Page {
                items: vec![],
                next_offset: None,
            })
        }
    }
    let (_dir, mut library, imported, album) =
        retained_album_context(&["A", "B"], &["Unrelated 1", "Unrelated 2"]);
    let mut search = Search(0);
    let empty = crate::album_context::reconcile_best_available_with_budget(
        &mut library,
        &[],
        "US",
        &mut search,
        2,
    )
    .unwrap();
    assert_eq!(
        empty.provider_searches, 0,
        "an empty batch must not consume the run allowance"
    );
    let first = crate::album_context::reconcile_best_available_with_budget(
        &mut library,
        std::slice::from_ref(&album),
        "US",
        &mut search,
        1,
    )
    .unwrap();
    assert_eq!(first.provider_searches, 1);
    library
        .set_track_title_override(&imported.track_ids[1], "Changed B")
        .unwrap();
    let second = crate::album_context::reconcile_best_available_with_budget(
        &mut library,
        std::slice::from_ref(&album),
        "US",
        &mut search,
        1,
    )
    .unwrap();
    assert_eq!(second.provider_searches, 1);
    let final_batch = crate::album_context::reconcile_best_available_with_budget(
        &mut library,
        std::slice::from_ref(&album),
        "US",
        &mut search,
        0,
    )
    .unwrap();
    assert_eq!(final_batch.provider_searches, 0);
    assert_eq!(search.0, 2);
}

#[test]
fn trusted_catalog_alias_discovers_provider_artist_without_user_equivalence() {
    use music_library::{Library, domain::*};
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db.sqlite");
    let mut library = Library::open(&path).unwrap();
    let imported = library
        .create_catalog_release(&CatalogReleaseInput {
            title: "Album".into(),
            year: None,
            artists: vec![ArtistCreditInput {
                name: "Artist A".into(),
                role: None,
            }],
            tracks: vec![CatalogTrackInput {
                title: "Song".into(),
                disc_number: Some(1),
                track_number: Some(1),
                artists: vec![],
            }],
        })
        .unwrap();
    library.add_to_library(&imported.track_ids[0]).unwrap();
    let album = library.album_id_for_track(&imported.track_ids[0]).unwrap();
    let music_library::album_matching::Preparation::Ready(input) =
        library.spotify_album_review_input(&album).unwrap()
    else {
        panic!()
    };
    let observation = ArtistCandidate {
        identity: ExternalIdentity {
            provider: "musicbrainz".into(),
            kind: "artist".into(),
            external_id: "catalog-artist".into(),
        },
        name: "Artist B".into(),
        aliases: vec!["Artist A".into()],
        comment: String::new(),
        country: String::new(),
        artist_type: String::new(),
        score: None,
    };
    library
        .ingest_resolved_catalog_artist(&input.artist_id, &observation)
        .unwrap();
    let empty = (200, json!({"artists":{"items":[],"total":0}}), None);
    let provider = (
        200,
        json!({"artists":{"items":[{"id":"abcdefghijklmnopqrstuv","name":"Artist B"}],"total":1}}),
        None,
    );
    let albums = (
        200,
        json!({"albums":{"items":[{"id":"abcdefghijklmnopqrstuw","name":"Album","album_type":"album","artists":[{"id":"abcdefghijklmnopqrstuv","name":"Artist B"}],"total_tracks":1}],"total":1}}),
        None,
    );
    let mut mock = Mock::new(vec![token(), empty.clone(), empty, provider, albums]);
    mock.client = mock
        .client
        .with_library(Some(path.to_str().unwrap()))
        .unwrap();
    let page = mock.client.search_artists("Artist A").unwrap();
    let identity = music_library::album_matching::resolve_artist("Artist A", &page).unwrap();
    assert_eq!(identity, id("artist", "abcdefghijklmnopqrstuv"));
    assert!(music_library::album_matching::resolve_artist("Artist AA", &page).is_err());
    assert_eq!(
        mock.client.artist_albums(&identity, "Album").unwrap().items[0].identity,
        id("album", "abcdefghijklmnopqrstuw")
    );
    assert!(
        library
            .ingest_catalog_artist_provider_name(&input.artist_id, &page.items[0])
            .unwrap()
    );
    let mut unrelated = page.items[0].clone();
    unrelated.name = "Artist BB".into();
    unrelated.identity = id("artist", "unrelated");
    assert!(
        !library
            .ingest_catalog_artist_provider_name(&input.artist_id, &unrelated)
            .unwrap()
    );
    let music_library::album_matching::Preparation::Ready(current) =
        library.spotify_album_review_input(&album).unwrap()
    else {
        panic!()
    };
    assert!(current.equivalent_artists.contains(&identity));
    assert_eq!(current.artist, "artist a");
    assert_eq!(mock.finish().len(), 5);
}

#[test]
fn catalog_attested_credit_can_establish_identity_from_a_strong_cached_program() {
    let (dir, mut library, _, album) = retained_album_context(&["A", "B", "C"], &["A", "B", "C"]);
    let music_library::album_matching::Preparation::Ready(input) =
        library.spotify_album_review_input(&album).unwrap()
    else {
        panic!()
    };
    library
        .ingest_resolved_catalog_artist(
            &input.artist_id,
            &ArtistCandidate {
                identity: ExternalIdentity {
                    provider: "musicbrainz".into(),
                    kind: "artist".into(),
                    external_id: "catalog-artist".into(),
                },
                name: "Artist".into(),
                aliases: vec!["Former Name".into()],
                comment: String::new(),
                country: String::new(),
                artist_type: String::new(),
                score: None,
            },
        )
        .unwrap();
    let db = rusqlite::Connection::open(dir.path().join("context.db")).unwrap();
    let rows = db
        .prepare("SELECT fingerprint,response_json FROM spotify_discovery")
        .unwrap()
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    fn rename(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(fields) => {
                if fields.get("id").and_then(|v| v.as_str()) == Some("artist1") {
                    fields.insert("id".into(), json!("abcdefghijklmnopqrstuv"));
                    fields.insert("name".into(), json!("Former Name"));
                }
                for value in fields.values_mut() {
                    rename(value);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    rename(value);
                }
            }
            _ => {}
        }
    }
    for (key, body) in rows {
        let mut value: serde_json::Value = serde_json::from_str(&body).unwrap();
        rename(&mut value);
        library
            .persist_spotify_discovery(&key, &value.to_string(), Spotify::now())
            .unwrap();
    }
    assert!(
        crate::album_context::reconcile_catalog_credit_names(&mut library, &album, "US").unwrap()
    );
    assert!(
        !crate::album_context::reconcile_catalog_credit_names(&mut library, &album, "US").unwrap()
    );
    let batch = crate::album_context::reconcile(&mut library, &[album], "US").unwrap();
    assert_eq!(batch.accepted.len(), 3);
}

#[test]
fn fallback_cursor_skips_a_full_cached_page_without_ending_the_search_window() {
    let titles: Vec<String> = (1..=25).map(|n| format!("Local {n}")).collect();
    let other: Vec<String> = (1..=25)
        .map(|n| format!("Unrelated provider title number {n}"))
        .collect();
    let (_dir, mut library, imported, album) = retained_album_context(
        &titles.iter().map(String::as_str).collect::<Vec<_>>(),
        &other.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    // Completed negative Album discovery, not a truncated competing candidate
    // set: the work left here is specifically bounded standalone Track search.
    rusqlite::Connection::open(_dir.path().join("context.db")).unwrap()
        .execute("UPDATE spotify_discovery SET response_json='{\"albums\":{\"items\":[],\"total\":0,\"next\":null}}' WHERE json_type(response_json,'$.albums')='object'", []).unwrap();
    for track in &imported.track_ids[..20] {
        let input = library.song_resolution_input(track).unwrap();
        library.record_spotify_playable_discovery(&input).unwrap();
    }
    struct Search(Vec<String>);
    impl music_library::song_resolution::SongSearch for Search {
        fn search_songs(
            &mut self,
            input: &music_library::song_resolution::Input,
        ) -> Result<
            music_library::catalog::Page<music_library::song_resolution::Candidate>,
            music_library::catalog::CatalogError,
        > {
            self.0.push(input.title.clone());
            Ok(music_library::catalog::Page {
                items: vec![],
                next_offset: None,
            })
        }
    }
    let mut search = Search(vec![]);
    let batch = crate::album_context::reconcile_best_available(
        &mut library,
        std::slice::from_ref(&album),
        "US",
        &mut search,
    )
    .unwrap();
    assert_eq!(batch.provider_searches, 2, "{batch:?}");
    assert_eq!(search.0, vec!["Local 21", "Local 22"]);
    let second =
        crate::album_context::reconcile_best_available(&mut library, &[album], "US", &mut search)
            .unwrap();
    assert_eq!(second.provider_searches, 0);
}

#[test]
fn ordinary_and_self_titled_discovery_retains_exact_artist_scope() {
    for title in [
        "1234 EP",
        "Clown Core",
        "Toilet",
        "Van",
        "Get Disowned",
        "Painted Shut",
        "Bark Your Head Off, Dog",
    ] {
        let mut mock = Mock::new(vec![
            token(),
            (
                200,
                json!({"albums":{"items":[
            {"id":"unrelated","name":title,"artists":[{"id":"school","name":"Clown School"}],"album_type":if title.ends_with(" EP") {"single"} else {"album"},"total_tracks":4},
            {"id":"intended","name":title,"artists":[{"id":"core","name":"Clown Core"}],"album_type":if title.ends_with(" EP") {"single"} else {"album"},"total_tracks":4}
        ],"total":2,"next":null}}),
                None,
            ),
        ]);
        mock.client
            .artist_names
            .insert("core".into(), "Clown Core".into());
        let page = mock
            .client
            .artist_albums(&id("artist", "core"), title)
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].identity.external_id, "intended");
        assert_eq!(
            mock.client.rejected_album_candidates()[0].artist,
            "Clown School"
        );
        let query = &mock.client.album_discovery_queries()[0];
        assert!(query.contains("artist:\"Clown Core\""), "{query}");
        assert!(query.contains("album:"), "{query}");
        assert_eq!(
            mock.finish().len(),
            2,
            "no redundant fallback for a relevant result"
        );
    }
}
#[test]
fn ordinary_empty_discovery_uses_one_artist_constrained_alternate() {
    let empty = json!({"albums":{"items":[],"total":0,"next":null}});
    let found = json!({"albums":{"items":[{"id":"intended","name":"Clown Core","artists":[{"id":"core","name":"Clown Core"}],"album_type":"album","total_tracks":4}],"total":1,"next":null}});
    let mut mock = Mock::new(vec![token(), (200, empty, None), (200, found, None)]);
    mock.client
        .artist_names
        .insert("core".into(), "Clown Core".into());
    assert_eq!(
        mock.client
            .artist_albums(&id("artist", "core"), "Clown Core")
            .unwrap()
            .items
            .len(),
        1
    );
    assert_eq!(
        mock.client.album_discovery_queries(),
        vec![
            "album:\"Clown Core\" artist:\"Clown Core\"",
            "\"Clown Core\" Clown Core"
        ]
    );
    assert_eq!(mock.finish().len(), 3);
}
#[test]
fn literal_wao_component_search_is_bounded_and_keeps_two_exclamation_marks() {
    let empty = json!({"albums":{"items":[],"total":0,"next":null}});
    let found = json!({"albums":{"items":[{"id":"wao","name":"WAO!!","artists":[{"id":"band","name":"Band"}],"album_type":"album","total_tracks":4}],"total":1,"next":null}});
    let mut mock = Mock::new(vec![token(), (200, empty, None), (200, found, None)]);
    mock.client
        .artist_names
        .insert("band".into(), "Band".into());
    assert_eq!(
        mock.client
            .artist_albums(&id("artist", "band"), "日本語 / WAO!!")
            .unwrap()
            .items
            .len(),
        1
    );
    assert_eq!(mock.client.album_discovery_queries()[1], "\"Band\" WAO!!");
    assert_eq!(mock.finish().len(), 3);
}
#[test]
fn review_discovery_exposes_query_and_exact_artist_rejection() {
    let mut mock = Mock::new(vec![
        token(),
        (
            200,
            json!({"albums":{"items":[{"id":"wrong","name":"Clown Core","artists":[{"id":"school","name":"Clown School"}],"album_type":"album","total_tracks":4}],"total":1,"next":null}}),
            None,
        ),
        (
            200,
            json!({"albums":{"items":[],"total":0,"next":null}}),
            None,
        ),
    ]);
    mock.client
        .artist_names
        .insert("core".into(), "Clown Core".into());
    let page = mock
        .client
        .artist_albums(&id("artist", "core"), "Clown Core")
        .unwrap();
    let report = music_library::album_candidates::initial_report(
        &mock.client,
        ("Clown Core", None),
        Some(&id("artist", "core")),
        &page,
        &[],
    );
    let text = report.explain_track("");
    assert!(
        text.contains("Search query: album:\"Clown Core\" artist:\"Clown Core\""),
        "{text}"
    );
    assert!(
        text.contains("Clown School") && text.contains("Candidate Artist IDs differ"),
        "{text}"
    );
    assert_eq!(mock.finish().len(), 3);
}

#[test]
fn cached_remaster_program_attaches_playback_preserving_identity_and_warnings() {
    for title in [
        "Album (Remastered)",
        "Album (2015 Remaster)",
        "Album - 2020 Remastered",
        "Album (Deluxe Remaster)",
        "Album (Reissue)",
    ] {
        let (dir, mut library, release, album) =
            retained_album_context(&["A", "B", "C", "D"], &["A", "B", "C", "D"]);
        let db = rusqlite::Connection::open(dir.path().join("context.db")).unwrap();
        db.execute(
            "UPDATE album_application_metadata SET year=2006 WHERE album_id=?1",
            [album.as_ref()],
        )
        .unwrap();
        // Explicit canonical year evidence, independent of the provider observation.
        db.execute(
            "INSERT INTO album_metadata_override(album_id,year,year_set) VALUES(?1,2006,1)",
            [album.as_ref()],
        )
        .unwrap();
        let before: (String, Option<i64>) = db
            .query_row(
                "SELECT title,year FROM effective_album_metadata WHERE album_id=?1",
                [album.as_ref()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let identities = library.list_album_external_identities(&album).unwrap();
        let (key, raw): (String,String) = db.query_row("SELECT fingerprint,response_json FROM spotify_discovery WHERE fingerprint LIKE '%type=album%'", [], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
        let mut raw: serde_json::Value = serde_json::from_str(&raw).unwrap();
        raw["albums"]["items"][0]["name"] = json!(title);
        raw["albums"]["items"][0]["release_date"] = json!("2012");
        library
            .persist_spotify_discovery(&key, &raw.to_string(), Spotify::now())
            .unwrap();
        let music_library::album_matching::Preparation::Ready(input) =
            library.spotify_album_review_input(&album).unwrap()
        else {
            panic!()
        };
        let (report, _) = Spotify::cached_album_review(
            dir.path().join("context.db").to_str().unwrap(),
            "US",
            &input,
        )
        .unwrap()
        .unwrap();
        assert_eq!(report.decision, "Accepted", "{title}");
        assert_eq!(report.candidate_count, 1);
        assert!(
            report.candidates[0]
                .warnings
                .iter()
                .any(|w| w.contains("year differs"))
        );
        let explanation = report.explain_track(release.track_ids[0].as_ref());
        assert!(explanation.contains("Automatic association accepted"));
        assert!(explanation.contains("Warning: Release year differs: 2006 ↔ 2012"));
        assert!(!explanation.contains("Primary blocker:"));
        let batch =
            album_context::reconcile(&mut library, std::slice::from_ref(&album), "US").unwrap();
        assert_eq!(batch.accepted.len(), 4, "{title}");
        assert_eq!(batch.provider_searches, 0);
        assert_eq!(batch.needs_retry, 0);
        assert_eq!(
            library.list_album_external_identities(&album).unwrap(),
            identities
        );
        let after: (String, Option<i64>) = db
            .query_row(
                "SELECT title,year FROM effective_album_metadata WHERE album_id=?1",
                [album.as_ref()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(before, after);
        for track in &release.track_ids {
            assert_eq!(library.album_id_for_track(track).unwrap(), album);
            let (_, page) = library
                .cached_spotify_track_attempt(track)
                .unwrap()
                .unwrap();
            assert_eq!(page.items[0].album, title);
            assert_eq!(page.items[0].date, "2012");
            let (_, reason) = library.spotify_review_summary(track).unwrap();
            assert!(reason.contains("remaster/reissue"), "{reason}");
            assert!(reason.contains("2006 ↔ 2012"), "{reason}");
        }
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM spotify_discovery WHERE fingerprint LIKE '%type=track%'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }
}
