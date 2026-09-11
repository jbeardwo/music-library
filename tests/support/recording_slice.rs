use super::*;
use music_library::{
    album_matching::{AlbumMatcher, AutoMatchPolicy, CircuitState},
    catalog::{CatalogError, CatalogProvider, Page},
    recording::*,
};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

fn candidate(n: u32) -> Candidate {
    Candidate {
        identity: identity("recording", &format!("recording-{n}")),
        title: format!("Song {n}"),
        artist: "Artist".into(),
        artist_ids: vec![],
        duration_ms: None,
        isrcs: vec![format!("ISRC-{n}-A"), format!("ISRC-{n}-B")],
    }
}
fn page(count: u32) -> Page<Candidate> {
    Page {
        items: (1..=count).map(candidate).collect(),
        next_offset: None,
    }
}
fn local(f: &mut Fixture, group: &str, numbers: &[u32]) -> (ImportedRelease, AlbumId) {
    let request = f.files("Album", "Artist", numbers);
    let imported = f.library.import_release(&request).unwrap();
    let album = f
        .library
        .album_for_release(&imported.release_id)
        .unwrap()
        .album_id;
    f.library
        .attach_album_external_identity(&album, &identity("release_group", group))
        .unwrap();
    (imported, album)
}
#[test]
fn exact_sparse_artist_duration_ambiguity_and_versions() {
    let t = LocalTrack {
        track_id: TrackId("t".into()),
        recording_id: RecordingId("r".into()),
        title: "Song 1".into(),
        artist: "Artist".into(),
        artist_ids: vec![],
        duration_ms: Some(100_000),
        known: false,
    };
    let mut c = candidate(1);
    c.duration_ms = Some(103_000);
    let mut p = Page {
        items: vec![c.clone()],
        next_offset: None,
    };
    assert!(matches!(compare(&t, &p), TrackOutcome::Matched(_)));
    p.items[0].duration_ms = Some(103_001);
    assert_eq!(compare(&t, &p), TrackOutcome::NoConfidentMatch);
    p.items[0].duration_ms = None;
    p.items[0].artist.clear();
    assert!(matches!(compare(&t, &p), TrackOutcome::Matched(_)));
    p.items[0].artist = "Other Artist".into();
    assert_eq!(compare(&t, &p), TrackOutcome::NoConfidentMatch);
    p.items[0] = c.clone();
    c.identity.external_id = "other".into();
    p.items.push(c);
    assert_eq!(compare(&t, &p), TrackOutcome::Ambiguous);
    p.items.pop();
    p.next_offset = Some(100);
    assert_eq!(compare(&t, &p), TrackOutcome::Incomplete);
    p.next_offset = None;
    for suffix in [
        "Live",
        "Remix",
        "Edit",
        "Acoustic",
        "Demo",
        "Remastered",
        "Mix",
    ] {
        p.items[0].title = format!("Song 1 ({suffix})");
        assert_eq!(compare(&t, &p), TrackOutcome::NoConfidentMatch);
    }
    p.items[0].title = t.title.clone();
    p.items[0].artist = "Canonical".into();
    p.items[0].artist_ids = vec![identity("artist", "a")];
    let mut with_id = t.clone();
    with_id.artist_ids = vec![identity("artist", "a")];
    assert!(matches!(compare(&with_id, &p), TrackOutcome::Matched(_)));
    with_id.artist_ids = vec![identity("artist", "b")];
    assert_eq!(compare(&with_id, &p), TrackOutcome::NoConfidentMatch);
}
#[test]
fn partials_reuse_catalog_recordings_without_claiming_editions_or_tracks() {
    for numbers in [vec![1], vec![2, 5, 10], (1..=15).collect()] {
        let mut f = Fixture::new();
        let catalog = f.catalog("Album", "Artist", "group", "edition", 15);
        let (imported, album) = local(&mut f, "group", &numbers);
        let before = f.library.search(&SearchRequest::default()).unwrap();
        let input = f.library.prepare_recording_match(&album).unwrap().unwrap();
        assert_eq!(input.tracks.len(), numbers.len());
        let mut candidates = page(15);
        candidates.items[0].isrcs.clear();
        let outcome = f
            .library
            .complete_recording_match(Reply {
                input,
                result: Ok(candidates),
            })
            .unwrap();
        assert!(
            matches!(outcome,Outcome::Complete(rows) if rows.iter().all(|(_,r)|matches!(r,TrackOutcome::Matched(_))))
        );
        for track in &imported.track_ids {
            let n: u32 = f
                .db()
                .query_row(
                    "SELECT track_number FROM track WHERE id=?1",
                    [track.as_ref()],
                    |r| r.get(0),
                )
                .unwrap();
            let r = f.library.recording_for_track(track).unwrap();
            assert_eq!(
                r,
                f.library
                    .recording_for_track(&catalog.track_ids[(n - 1) as usize])
                    .unwrap()
            );
            let ids = f
                .library
                .list_recording_external_identities(&r.recording_id)
                .unwrap();
            assert!(ids.contains(&identity("recording", &format!("recording-{n}"))));
            assert_eq!(ids.len(), if n == 1 { 1 } else { 3 });
            assert!(
                f.library
                    .list_track_external_identities(track)
                    .unwrap()
                    .is_empty()
            );
            assert!(
                f.library
                    .available_playback_source(track)
                    .unwrap()
                    .is_some()
            );
        }
        assert!(
            f.library
                .list_release_external_identities(&imported.release_id)
                .unwrap()
                .is_empty()
        );
        assert_eq!(f.library.search(&SearchRequest::default()).unwrap(), before);
        assert!(f.library.prepare_recording_match(&album).unwrap().is_none());
    }
}
#[test]
fn identities_are_many_to_many_indexed_and_merge_is_atomic() {
    let mut f = Fixture::new();
    let (imported, _) = local(&mut f, "group", &[1, 2]);
    let a = f
        .library
        .recording_for_track(&imported.track_ids[0])
        .unwrap()
        .recording_id;
    let b = f
        .library
        .recording_for_track(&imported.track_ids[1])
        .unwrap()
        .recording_id;
    let shared = identity("recording", "same");
    for id in [&a, &b] {
        assert!(
            f.library
                .attach_recording_external_identity(id, &shared)
                .unwrap()
        );
        assert!(
            !f.library
                .attach_recording_external_identity(id, &shared)
                .unwrap()
        );
    }
    assert_eq!(
        f.library
            .resolve_recordings_external_identity(&shared)
            .unwrap()
            .len(),
        2
    );
    for sql in [
        "SELECT recording_id FROM recording_external_identity WHERE provider='musicbrainz' AND kind='recording' AND external_id='same'",
        "SELECT provider,kind,external_id FROM recording_external_identity WHERE recording_id='r'",
        "SELECT id FROM track WHERE recording_id='r'",
    ] {
        let plan: String = f
            .db()
            .query_row(&format!("EXPLAIN QUERY PLAN {sql}"), [], |r| r.get(3))
            .unwrap();
        assert!(plan.contains("SEARCH") && plan.contains("INDEX"), "{plan}");
    }
    f.library
        .attach_recording_external_identity(&b, &identity("recording", "contradictory"))
        .unwrap();
    assert!(f.library.merge_recording(&b, &a).is_err());
    assert_eq!(
        f.library
            .recording_for_track(&imported.track_ids[1])
            .unwrap()
            .recording_id,
        b
    );
    f.db()
        .execute(
            "DELETE FROM recording_external_identity WHERE external_id='contradictory'",
            [],
        )
        .unwrap();
    f.db().execute_batch("CREATE TRIGGER fail_recording_merge BEFORE UPDATE OF recording_id ON track BEGIN SELECT RAISE(ABORT,'induced'); END").unwrap();
    assert!(f.library.merge_recording(&b, &a).is_err());
    assert_eq!(
        f.library
            .resolve_recordings_external_identity(&shared)
            .unwrap()
            .len(),
        2
    );
    f.db()
        .execute_batch("DROP TRIGGER fail_recording_merge")
        .unwrap();
    assert!(f.library.merge_recording(&b, &a).unwrap());
    assert!(!f.library.merge_recording(&b, &a).unwrap());
    assert_eq!(
        f.library
            .recording_for_track(&imported.track_ids[1])
            .unwrap()
            .recording_id,
        a
    );
    assert!(
        f.library
            .list_recording_external_identities(&b)
            .unwrap()
            .is_empty()
    );
}
#[test]
fn recording_enrichment_rolls_back_all_tracks_on_failure_and_truncation_attaches_nothing() {
    let mut f = Fixture::new();
    let (imported, album) = local(&mut f, "group", &[1, 2, 3]);
    let input = f.library.prepare_recording_match(&album).unwrap().unwrap();
    let mut incomplete = page(3);
    incomplete.next_offset = Some(100);
    assert!(
        matches!(f.library.complete_recording_match(Reply{input:input.clone(),result:Ok(incomplete)}).unwrap(),Outcome::Complete(rows) if rows.iter().all(|(_,r)|*r==TrackOutcome::Incomplete))
    );
    f.db().execute_batch("CREATE TRIGGER fail_enrichment BEFORE INSERT ON recording_external_identity WHEN NEW.external_id='recording-2' BEGIN SELECT RAISE(ABORT,'induced'); END").unwrap();
    assert!(
        f.library
            .complete_recording_match(Reply {
                input: input.clone(),
                result: Ok(page(3))
            })
            .is_err()
    );
    for t in &imported.track_ids {
        let id = f.library.recording_for_track(t).unwrap().recording_id;
        assert!(
            f.library
                .list_recording_external_identities(&id)
                .unwrap()
                .is_empty()
        );
        assert!(f.library.available_playback_source(t).unwrap().is_some());
    }
    assert_eq!(
        f.library.prepare_recording_match(&album).unwrap(),
        Some(input)
    );
}

struct Provider {
    calls: Arc<Mutex<Vec<String>>>,
    fail: bool,
}
impl CatalogProvider for Provider {
    fn recordings(
        &mut self,
        group: &ExternalIdentity,
    ) -> std::result::Result<Page<Candidate>, CatalogError> {
        self.calls.lock().unwrap().push(group.external_id.clone());
        if self.fail {
            self.fail = false;
            return Err(CatalogError::ServiceUnavailable {
                message: "503".into(),
                retry_after: None,
            });
        }
        Ok(page(15))
    }
    fn search_albums(
        &mut self,
        _: &str,
        _: u32,
    ) -> std::result::Result<Page<catalog::AlbumCandidate>, CatalogError> {
        panic!("no Album search")
    }
    fn releases(
        &mut self,
        _: &ExternalIdentity,
        _: u32,
    ) -> std::result::Result<Page<catalog::ReleaseCandidate>, CatalogError> {
        panic!("no Release lookup")
    }
    fn release(
        &mut self,
        _: &ExternalIdentity,
    ) -> std::result::Result<catalog::Release, CatalogError> {
        panic!("no Release lookup")
    }
}
#[test]
fn one_serial_recording_request_per_album_and_outage_recovery_preserves_queue() {
    for fail in [false, true] {
        let mut f = Fixture::new();
        let (first, a) = local(&mut f, "a", &[1, 2, 3]);
        let request = f.files("Second Album", "Artist", &[1]);
        let second = f.library.import_release(&request).unwrap();
        let b = f
            .library
            .album_for_release(&second.release_id)
            .unwrap()
            .album_id;
        f.library
            .attach_album_external_identity(&b, &identity("release_group", "b"))
            .unwrap();
        let calls = Arc::new(Mutex::new(vec![]));
        let (tx, rx) = mpsc::channel();
        let mut matcher = AlbumMatcher::new_with_recordings(
            Provider {
                calls: calls.clone(),
                fail,
            },
            |_| panic!("no Artist/Album work"),
            move |r| tx.send(r).unwrap(),
            |_| {},
        )
        .unwrap();
        matcher
            .after_import(
                &f.library,
                &[first.clone(), second],
                AutoMatchPolicy::default(),
            )
            .unwrap();
        let reply = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        if fail {
            assert!(matches!(
                matcher.complete_recordings(&mut f.library, reply),
                Outcome::Deferred(_)
            ));
            assert!(matches!(
                matcher.circuit_state(),
                CircuitState::Unavailable(_)
            ));
            assert_eq!(*calls.lock().unwrap(), vec!["a"]);
            assert_eq!(matcher.pending_count(), 2);
            assert!(
                f.library
                    .available_playback_source(&first.track_ids[0])
                    .unwrap()
                    .is_some()
            );
            let token = matcher.retry_schedule().unwrap().token;
            matcher.cooldown_elapsed(&f.library, token).unwrap();
            matcher.cooldown_elapsed(&f.library, token).unwrap();
            matcher.complete_recordings(
                &mut f.library,
                rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            );
        } else {
            matcher.complete_recordings(&mut f.library, reply);
        }
        matcher.complete_recordings(
            &mut f.library,
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        );
        assert_eq!(matcher.circuit_state(), &CircuitState::Available);
        assert_eq!(
            *calls.lock().unwrap(),
            if fail {
                vec!["a", "a", "b"]
            } else {
                vec!["a", "b"]
            }
        );
        assert!(matches!(
            matcher.recording_outcome(&a),
            Some(Outcome::Complete(_))
        ));
        assert!(matches!(
            matcher.recording_outcome(&b),
            Some(Outcome::Complete(_))
        ));
        matcher.match_album(&f.library, &a).unwrap();
        assert_eq!(matcher.pending_count(), 0);
    }
}
#[test]
fn backfill_preserves_tracks_and_historical_ids_without_inferred_sharing() {
    let mut f = Fixture::new();
    let (imported, _) = local(&mut f, "group", &[1, 2, 3]);
    let shared = identity("recording", "historical");
    for t in &imported.track_ids {
        f.library
            .attach_track_external_identity(t, &shared)
            .unwrap();
    }
    let path = f.temp.path().join("db");
    drop(f.library);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("DROP INDEX track_recording; ALTER TABLE track DROP COLUMN recording_id; DROP TABLE recording_external_identity; DROP TABLE recording; PRAGMA user_version=7;").unwrap();
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
    drop(db);
    let twin_path = f.temp.path().join("twin");
    fs::copy(&path, &twin_path).unwrap();
    let broken_path = f.temp.path().join("broken");
    fs::copy(&path, &broken_path).unwrap();
    let broken = Connection::open(&broken_path).unwrap();
    broken
        .execute_batch(
            "PRAGMA foreign_keys=OFF; INSERT INTO track(id,release_id) VALUES ('broken','missing')",
        )
        .unwrap();
    drop(broken);
    assert!(Library::open(&broken_path).is_err());
    let broken = Connection::open(&broken_path).unwrap();
    assert_eq!(
        broken
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        7
    );
    assert_eq!(
        broken
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='recording'",
                [],
                |r| r.get::<_, u32>(0)
            )
            .unwrap(),
        0
    );
    let twin = Library::open(twin_path).unwrap();
    let library = Library::open(&path).unwrap();
    let mut recordings = std::collections::HashSet::new();
    for t in &imported.track_ids {
        let r = library.recording_for_track(t).unwrap();
        assert_eq!(r, twin.recording_for_track(t).unwrap());
        recordings.insert(r.recording_id.clone());
        assert_eq!(
            library.list_track_external_identities(t).unwrap(),
            vec![shared.clone()]
        );
        assert_eq!(
            library
                .list_recording_external_identities(&r.recording_id)
                .unwrap(),
            vec![shared.clone()]
        );
        assert!(library.available_playback_source(t).unwrap().is_some());
    }
    assert_eq!(recordings.len(), 3);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("PRAGMA foreign_keys=ON").unwrap();
    assert!(
        db.execute("UPDATE track SET recording_id=NULL", [])
            .is_err()
    );
    assert!(
        db.execute("UPDATE track SET recording_id='absent'", [])
            .is_err()
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, u32>(
            0
        ))
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM library_membership", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        3
    );
}
#[test]
#[ignore = "opt-in deterministic local timing, no network"]
fn recording_local_timing() {
    for count in [1, 3, 15, 30] {
        let mut measurements = vec![];
        for _ in 0..20 {
            let mut f = Fixture::new();
            let (_, album) = local(&mut f, "group", &(1..=count).collect::<Vec<_>>());
            let start = Instant::now();
            let input = f.library.prepare_recording_match(&album).unwrap().unwrap();
            let prepared = start.elapsed();
            let start = Instant::now();
            f.library
                .complete_recording_match(Reply {
                    input,
                    result: Ok(page(count)),
                })
                .unwrap();
            measurements.push((prepared, start.elapsed()));
        }
        measurements.sort_by_key(|m| m.1);
        let matching_median = measurements[10].1;
        measurements.sort_by_key(|m| m.0);
        println!(
            "{count} Tracks: prepare median {:?}, matching+transaction median {:?}",
            measurements[10].0, matching_median
        );
    }
}
