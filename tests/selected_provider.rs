//! A simpler provider ontology exercises the real application worker and SQLite.
use music_library::{
    Library,
    album_matching::{AlbumMatcher, AutoMatchPolicy, CircuitState, MatchOutcome, MatchReply},
    album_program::{self, Program, Programs, RecordingStatus, TrackOutcome},
    catalog::*,
    domain::*,
    edition::TrackEvidence,
    filesystem::MetadataExtractor,
};
use std::sync::{Arc, Mutex, mpsc};
fn id(provider: &str, kind: &str, value: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: provider.into(),
        kind: kind.into(),
        external_id: value.into(),
    }
}
fn scope() -> MatchingScope {
    MatchingScope {
        provider: "song-catalog".into(),
        artist_kind: "artist".into(),
        album_kind: "album".into(),
    }
}
fn page<T>(items: Vec<T>) -> Page<T> {
    Page {
        items,
        next_offset: None,
    }
}
struct Provider {
    calls: Arc<Mutex<Vec<&'static str>>>,
    fail: bool,
    alternatives: bool,
    equivalent: bool,
}
fn programs(album: ExternalIdentity) -> Programs {
    Programs {
        album,
        programs: vec![Program {
            identity: None,
            complete: true,
            tracks: (1..=15)
                .map(|n| TrackEvidence {
                    title: Some(format!("Song {n}")),
                    disc: Some(1),
                    number: Some(n),
                    duration_ms: Some(200000),
                    identities: vec![id("song-catalog", "song", &n.to_string())],
                    ..Default::default()
                })
                .collect(),
        }],
        note: "No edition or Recording ontology".into(),
    }
}
impl CatalogProvider for Provider {
    fn album_candidate_programs(&self) -> bool {
        self.alternatives
    }
    fn album_candidate_track_count(&self, _: &ExternalIdentity) -> Option<u32> {
        Some(15)
    }
    fn album_program_namespaces(&self) -> Vec<(String, String)> {
        vec![("song-catalog".into(), "album".into())]
    }
    fn search_artists(&mut self, _: &str) -> Result<Page<ArtistCandidate>, CatalogError> {
        self.calls.lock().unwrap().push("artist");
        Ok(page(vec![ArtistCandidate {
            identity: id("song-catalog", "artist", "artist"),
            name: "Artist".into(),
            aliases: vec![],
            comment: String::new(),
            country: String::new(),
            artist_type: String::new(),
            score: None,
        }]))
    }
    fn artist_albums(
        &mut self,
        artist: &ExternalIdentity,
        _: &str,
    ) -> Result<Page<ArtistAlbumCandidate>, CatalogError> {
        self.calls.lock().unwrap().push("album");
        let candidate = ArtistAlbumCandidate {
            identity: id("song-catalog", "album", "album"),
            title: "Album".into(),
            artist: "Artist".into(),
            artist_ids: vec![artist.clone()],
            date: String::new(),
            primary_type: String::new(),
            comment: String::new(),
        };
        let mut candidates = vec![candidate.clone()];
        if self.alternatives {
            candidates.push(ArtistAlbumCandidate {
                identity: id("song-catalog", "album", "alternative"),
                ..candidate
            });
        }
        Ok(page(candidates))
    }
    fn album_programs(&mut self, album: &ExternalIdentity) -> Result<Programs, CatalogError> {
        self.calls.lock().unwrap().push("program");
        if std::mem::take(&mut self.fail) {
            Err(CatalogError::RateLimited {
                status: 429,
                message: "quota".into(),
                retry_after: Some("30".into()),
            })
        } else {
            let mut p = programs(album.clone());
            if album.external_id == "alternative" && !self.equivalent {
                for n in [1, 4] {
                    p.programs[0].tracks[n].title = Some("Entirely different song".into());
                }
            }
            Ok(p)
        }
    }
    fn search_albums(&mut self, _: &str, _: u32) -> Result<Page<AlbumCandidate>, CatalogError> {
        panic!("no broad search")
    }
    fn releases(
        &mut self,
        _: &ExternalIdentity,
        _: u32,
    ) -> Result<Page<ReleaseCandidate>, CatalogError> {
        panic!("no edition")
    }
    fn release(&mut self, _: &ExternalIdentity) -> Result<Release, CatalogError> {
        panic!("no edition")
    }
}
struct Tags;
impl MetadataExtractor for Tags {
    fn supports(&self, _: &std::path::Path) -> bool {
        true
    }
    fn read(&mut self, p: &std::path::Path) -> music_library::Result<ObservedMetadata> {
        let n = p.file_name().unwrap().to_str().unwrap().parse().unwrap();
        Ok(ObservedMetadata {
            release_title: Some("Album".into()),
            release_artists: vec!["Artist".into()],
            track_title: Some(format!("Song {n}")),
            track_number: Some(n),
            disc_number: Some(1),
            duration_ms: Some(200000),
            ..Default::default()
        })
    }
}
fn import(lib: &mut Library, folder: &std::path::Path) -> ImportedRelease {
    std::fs::create_dir(folder).unwrap();
    for n in [2, 5, 10] {
        std::fs::write(folder.join(n.to_string()), b"audio").unwrap();
    }
    let root = lib.register_local_root(folder).unwrap();
    lib.scan_local_root(&root, &mut Tags).unwrap();
    let tracks = lib
        .list_discovery_candidates(None, 100)
        .unwrap()
        .into_iter()
        .map(|s| ImportTrackInput {
            source_id: s.source_id,
            title_fallback: None,
            artists: vec![],
            disc_number: s.metadata.disc_number,
            track_number: s.metadata.track_number,
        })
        .collect();
    lib.import_release(&ImportReleaseRequest {
        release_title: "Album".into(),
        release_artists: vec![],
        tracks,
    })
    .unwrap()
}
enum Reply {
    Album(Box<MatchReply>),
    Programs(Box<album_program::Reply>),
}
fn worker(
    fail: bool,
) -> (
    AlbumMatcher,
    mpsc::Receiver<Reply>,
    Arc<Mutex<Vec<&'static str>>>,
) {
    worker_with_alternatives(fail, false)
}
fn worker_with_alternatives(
    fail: bool,
    alternatives: bool,
) -> (
    AlbumMatcher,
    mpsc::Receiver<Reply>,
    Arc<Mutex<Vec<&'static str>>>,
) {
    worker_with_options(fail, alternatives, false)
}
fn worker_with_options(
    fail: bool,
    alternatives: bool,
    equivalent: bool,
) -> (
    AlbumMatcher,
    mpsc::Receiver<Reply>,
    Arc<Mutex<Vec<&'static str>>>,
) {
    let calls = Arc::new(Mutex::new(vec![]));
    let (tx, rx) = mpsc::channel();
    let albums = tx.clone();
    let matcher = AlbumMatcher::for_provider(
        Provider {
            calls: calls.clone(),
            fail,
            alternatives,
            equivalent,
        },
        scope(),
        move |r| {
            albums.send(Reply::Album(Box::new(r))).unwrap();
        },
        move |r| {
            tx.send(Reply::Programs(Box::new(r))).unwrap();
        },
        |_| {},
    )
    .unwrap();
    (matcher, rx, calls)
}
fn drain(matcher: &mut AlbumMatcher, rx: &mpsc::Receiver<Reply>, lib: &mut Library) {
    while matcher.pending_count() > 0 {
        match rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap() {
            Reply::Album(r) => {
                matcher.complete(lib, *r);
            }
            Reply::Programs(r) => {
                let outcome = matcher.complete_programs(lib, *r);
                if matches!(outcome, album_program::Outcome::Deferred(_)) {
                    assert!(matches!(
                        matcher.circuit_state(),
                        CircuitState::Unavailable(_)
                    ));
                    assert!(rx.try_recv().is_err());
                    matcher
                        .cooldown_elapsed(lib, matcher.retry_schedule().unwrap().token)
                        .unwrap();
                }
            }
        }
    }
}

#[test]
fn selected_provider_matches_partial_album_persists_songs_and_coexists_with_richer_identity() {
    for fail in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("db");
        let mut lib = Library::open(&path).unwrap();
        let imported = import(&mut lib, &temp.path().join("music"));
        let album = lib
            .album_for_release(&imported.release_id)
            .unwrap()
            .album_id;
        let previous = id("musicbrainz", "release_group", "previous-group");
        lib.attach_album_external_identity(&album, &previous)
            .unwrap();
        let tracks = lib.local_album_tracks(&album).unwrap();
        let recording = id("musicbrainz", "recording", "previous-recording");
        lib.attach_recording_external_identity(&tracks[0].recording_id, &recording)
            .unwrap();
        let (mut matcher, rx, calls) = worker(fail);
        matcher
            .after_import(
                &lib,
                std::slice::from_ref(&imported),
                AutoMatchPolicy::default(),
            )
            .unwrap();
        drain(&mut matcher, &rx, &mut lib);
        assert!(matches!(
            matcher.outcome(&album),
            Some(MatchOutcome::Matched(_))
        ));
        assert!(
            matches!(matcher.program_outcome(&album),Some(album_program::Outcome::Complete(rows)) if rows.len()==3 && rows.iter().all(|(_,o)|matches!(o,TrackOutcome::Matched(m)|TrackOutcome::AlreadyMatched(m) if m.recording_status==RecordingStatus::NotProvided && m.recording.identities.is_empty())))
        );
        assert_eq!(
            *calls.lock().unwrap(),
            if fail {
                vec!["artist", "album", "program", "program"]
            } else {
                vec!["artist", "album", "program"]
            }
        );
        assert_eq!(lib.list_album_external_identities(&album).unwrap().len(), 2);
        assert!(
            lib.list_release_external_identities(&imported.release_id)
                .unwrap()
                .is_empty()
        );
        assert!(
            lib.list_recording_external_identities(&tracks[0].recording_id)
                .unwrap()
                .contains(&recording)
        );
        assert_eq!(lib.local_album_tracks(&album).unwrap().len(), 3);
        assert_eq!(
            lib.track_provider_occurrences(&tracks[0].track_id, "song-catalog")
                .unwrap(),
            vec![id("song-catalog", "song", "2")]
        );
        assert!(
            lib.track_provider_occurrences(&tracks[0].track_id, "unconfigured")
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            lib.provider_track_associations(&album, "song-catalog")
                .unwrap()
                .len(),
            3
        );
        // Manual chooser uses the same in-memory bounded Album programs: zero requests.
        let cached = matcher.cached_programs(&album);
        assert!(cached.is_some());
        let selection = lib
            .prepare_manual_track(&album, &tracks[1].track_id, cached.unwrap())
            .unwrap();
        lib.confirm_manual_track(&selection, 0).unwrap();
        assert_eq!(
            lib.track_provider_occurrences(&tracks[1].track_id, "song-catalog")
                .unwrap(),
            selection.candidates()[0].evidence.identities
        );
        assert!(
            lib.list_recording_external_identities(&tracks[1].recording_id)
                .unwrap()
                .is_empty()
        );
        drop(matcher);
        drop(lib);
        let mut lib = Library::open(&path).unwrap();
        assert_eq!(
            lib.provider_track_associations(&album, "song-catalog")
                .unwrap()
                .len(),
            3
        );
        assert_eq!(lib.manual_track_associations(&album).unwrap().len(), 1);
        assert_eq!(
            lib.track_provider_occurrences(&tracks[1].track_id, "song-catalog")
                .unwrap(),
            selection.candidates()[0].evidence.identities
        );
        let (mut matcher, rx, calls) = worker(false);
        matcher
            .after_import(&lib, &[imported], AutoMatchPolicy::default())
            .unwrap();
        drain(&mut matcher, &rx, &mut lib);
        assert_eq!(*calls.lock().unwrap(), vec!["program"]);
        assert!(
            matches!(matcher.program_outcome(&album),Some(album_program::Outcome::Complete(rows)) if rows.iter().any(|(_,o)|matches!(o,TrackOutcome::ManuallyMatched(_))))
        );
        lib.clear_manual_track_for(&album, &tracks[1].track_id, "song-catalog")
            .unwrap();
        assert!(lib.manual_track_associations(&album).unwrap().is_empty());
        let db = rusqlite::Connection::open(path).unwrap();
        let plan:Vec<String>=db.prepare("EXPLAIN QUERY PLAN SELECT a.track_id FROM release r CROSS JOIN track t ON t.release_id=r.id JOIN provider_track_association a ON a.track_id=t.id AND a.album_provider=?2 WHERE r.album_id=?1").unwrap().query_map(rusqlite::params![album.as_ref(),"song-catalog"],|r|r.get(3)).unwrap().map(Result::unwrap).collect();
        assert!(plan.iter().any(|s| s.contains("release_album")));
        assert!(plan.iter().any(|s| s.contains("track_release_order")));
        assert!(plan.iter().any(|s| s.contains("SEARCH a")));
    }
}

#[test]
fn candidate_disambiguation_reuses_program_for_enrichment_and_manual_selection() {
    let temp = tempfile::tempdir().unwrap();
    let mut lib = Library::open(temp.path().join("db")).unwrap();
    let imported = import(&mut lib, &temp.path().join("music"));
    let album = lib
        .album_for_release(&imported.release_id)
        .unwrap()
        .album_id;
    let (mut matcher, rx, calls) = worker_with_alternatives(false, true);
    matcher
        .after_import(
            &lib,
            std::slice::from_ref(&imported),
            AutoMatchPolicy::default(),
        )
        .unwrap();
    drain(&mut matcher, &rx, &mut lib);
    assert_eq!(
        *calls.lock().unwrap(),
        vec!["artist", "album", "program", "program"]
    );
    assert_eq!(
        matcher.outcome(&album),
        Some(&MatchOutcome::Matched(id("song-catalog", "album", "album")))
    );
    assert_eq!(
        lib.provider_track_associations(&album, "song-catalog")
            .unwrap()
            .len(),
        3
    );
    assert_eq!(lib.local_album_tracks(&album).unwrap().len(), 3);
    assert!(
        lib.list_release_external_identities(&imported.release_id)
            .unwrap()
            .is_empty()
    );
    let track = lib.local_album_tracks(&album).unwrap().remove(0);
    let selection = lib
        .prepare_manual_track(
            &album,
            &track.track_id,
            matcher.cached_programs(&album).unwrap(),
        )
        .unwrap();
    lib.confirm_manual_track(&selection, 0).unwrap();
    assert_eq!(calls.lock().unwrap().len(), 4);
}

#[test]
fn equivalent_catalog_objects_finish_without_arbitrary_album_identity() {
    let temp = tempfile::tempdir().unwrap();
    let mut lib = Library::open(temp.path().join("db")).unwrap();
    let imported = import(&mut lib, &temp.path().join("music"));
    let album = lib
        .album_for_release(&imported.release_id)
        .unwrap()
        .album_id;
    let (mut matcher, rx, calls) = worker_with_options(false, true, true);
    matcher
        .after_import(&lib, &[imported], AutoMatchPolicy::default())
        .unwrap();
    drain(&mut matcher, &rx, &mut lib);
    assert!(matches!(
        matcher.outcome(&album),
        Some(MatchOutcome::AlbumEquivalent { .. })
    ));
    assert!(
        matches!(matcher.program_outcome(&album), Some(album_program::Outcome::Complete(rows)) if rows.len()==3 && rows.iter().all(|(_,o)|matches!(o,TrackOutcome::Matched(_))))
    );
    assert!(
        lib.list_album_external_identities(&album)
            .unwrap()
            .is_empty()
    );
    assert!(
        lib.provider_track_associations(&album, "song-catalog")
            .unwrap()
            .is_empty()
    );
    assert_eq!(matcher.pending_count(), 0);
    assert_eq!(calls.lock().unwrap().len(), 4);
}

#[test]
fn changed_track_evidence_prevents_stale_program_based_album_attachment() {
    let temp = tempfile::tempdir().unwrap();
    let mut lib = Library::open(temp.path().join("db")).unwrap();
    let imported = import(&mut lib, &temp.path().join("music"));
    let album = lib
        .album_for_release(&imported.release_id)
        .unwrap()
        .album_id;
    let (mut matcher, rx, _) = worker_with_alternatives(false, true);
    matcher
        .after_import(&lib, &[imported], AutoMatchPolicy::default())
        .unwrap();
    let Reply::Album(reply) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap() else {
        panic!()
    };
    let track = lib.local_album_tracks(&album).unwrap().remove(0);
    lib.set_track_title_override(&track.track_id, "Changed while request was active")
        .unwrap();
    assert_eq!(
        matcher.complete(&mut lib, *reply),
        MatchOutcome::NoConfidentMatch
    );
    assert!(
        lib.list_album_external_identities(&album)
            .unwrap()
            .is_empty()
    );
    assert_eq!(matcher.pending_count(), 0);
}

#[test]
fn independent_provider_workers_do_not_share_circuits_and_manual_choices_coexist() {
    let temp = tempfile::tempdir().unwrap();
    let mut lib = Library::open(temp.path().join("db")).unwrap();
    let imported = import(&mut lib, &temp.path().join("music"));
    let album = lib
        .album_for_release(&imported.release_id)
        .unwrap()
        .album_id;
    let known = id("song-catalog", "album", "album");
    lib.attach_album_external_identity(&album, &known).unwrap();
    let other = id("other", "album", "different");
    lib.attach_album_external_identity(&album, &other).unwrap();
    let tracks = lib.local_album_tracks(&album).unwrap();
    for album_identity in [known, other] {
        let selection = lib
            .prepare_manual_track(&album, &tracks[0].track_id, &programs(album_identity))
            .unwrap();
        lib.confirm_manual_track(&selection, 0).unwrap();
    }
    assert_eq!(lib.manual_track_associations(&album).unwrap().len(), 2);
    lib.clear_manual_track_for(&album, &tracks[0].track_id, "song-catalog")
        .unwrap();
    assert_eq!(
        lib.manual_track_associations(&album).unwrap()[0]
            .album
            .provider,
        "other"
    );
    let (mut failing, rx, _) = worker(true);
    let (healthy, _, _) = worker(false);
    failing
        .after_import(&lib, &[imported], AutoMatchPolicy::default())
        .unwrap();
    let Reply::Programs(reply) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap() else {
        panic!()
    };
    failing.complete_programs(&mut lib, *reply);
    assert!(matches!(
        failing.circuit_state(),
        CircuitState::Unavailable(_)
    ));
    assert!(matches!(healthy.circuit_state(), CircuitState::Available));
}

#[test]
fn migration_preserves_existing_manual_recording_ownership_and_clear() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let mut lib = Library::open(&path).unwrap();
    let imported = import(&mut lib, &temp.path().join("music"));
    let album = lib
        .album_for_release(&imported.release_id)
        .unwrap()
        .album_id;
    let known = id("song-catalog", "album", "album");
    lib.attach_album_external_identity(&album, &known).unwrap();
    let track = lib.local_album_tracks(&album).unwrap().remove(0);
    drop(lib);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(include_str!("support/drop_manual_schema.sql"))
        .unwrap();
    db.execute_batch(include_str!(
        "../migrations/0011_manual_track_associations.sql"
    ))
    .unwrap();
    let mut candidate = music_library::manual_track::candidates(&programs(known.clone())).remove(0);
    candidate.evidence.recording.identities =
        vec![id("recording-provider", "performance", "known")];
    db.execute("INSERT INTO recording_external_identity VALUES(?1,'recording-provider','performance','known')",[track.recording_id.as_ref()]).unwrap();
    db.execute("INSERT INTO recording_manual_identity VALUES(?1,'recording-provider','performance','known')",[track.recording_id.as_ref()]).unwrap();
    db.execute(
        "INSERT INTO manual_track_association VALUES(?1,?2,?3,?4,?5)",
        rusqlite::params![
            track.track_id.as_ref(),
            known.provider,
            known.kind,
            known.external_id,
            serde_json::to_string(&candidate).unwrap()
        ],
    )
    .unwrap();
    db.execute("INSERT INTO manual_track_recording_claim VALUES(?1,?2,'recording-provider','performance','known')",rusqlite::params![track.track_id.as_ref(),track.recording_id.as_ref()]).unwrap();
    drop(db);
    let mut lib = Library::open(&path).unwrap();
    assert_eq!(
        lib.manual_track_associations(&album).unwrap()[0].candidate,
        candidate
    );
    assert_eq!(
        lib.list_recording_external_identities(&track.recording_id)
            .unwrap()
            .len(),
        1
    );
    lib.clear_manual_track_for(&album, &track.track_id, "song-catalog")
        .unwrap();
    assert!(
        lib.list_recording_external_identities(&track.recording_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
#[ignore = "local selected-provider comparison and persistence timings; no network"]
fn occurrence_comparison_and_persistence_timing() {
    for count in [5, 15, 30] {
        let mut p = programs(id("song-catalog", "album", "album"));
        p.programs[0].tracks = (1..=count)
            .map(|n| TrackEvidence {
                title: Some(format!("Song {n}")),
                number: Some(n),
                disc: Some(1),
                duration_ms: Some(200000),
                identities: vec![id("song-catalog", "song", &n.to_string())],
                ..Default::default()
            })
            .collect();
        let local: Vec<_> = p.programs[0]
            .tracks
            .iter()
            .enumerate()
            .map(|(n, e)| music_library::edition::LocalTrackEvidence {
                track_id: TrackId(format!("t{n}")),
                recording_id: RecordingId(format!("r{n}")),
                evidence: TrackEvidence {
                    identities: vec![],
                    ..e.clone()
                },
            })
            .collect();
        let start = std::time::Instant::now();
        for _ in 0..1000 {
            std::hint::black_box(album_program::compare_album(&local, &p));
        }
        println!(
            "{count} Tracks / one occurrence-only program: {:?} per comparison",
            start.elapsed() / 1000
        );
    }
    let temp = tempfile::tempdir().unwrap();
    let mut lib = Library::open(temp.path().join("db")).unwrap();
    let imported = import(&mut lib, &temp.path().join("music"));
    let album = lib
        .album_for_release(&imported.release_id)
        .unwrap()
        .album_id;
    let known = id("song-catalog", "album", "album");
    lib.attach_album_external_identity(&album, &known).unwrap();
    let start = std::time::Instant::now();
    for _ in 0..100 {
        let input = lib.prepare_album_program(&album, &known).unwrap().unwrap();
        lib.complete_album_program(album_program::Reply {
            input,
            result: Ok(programs(known.clone())),
        })
        .unwrap();
    }
    println!(
        "3 local Tracks: batch load + compare + transactional association writes {:?}",
        start.elapsed() / 100
    );
    let start = std::time::Instant::now();
    for _ in 0..1000 {
        std::hint::black_box(
            lib.provider_track_associations(&album, "song-catalog")
                .unwrap(),
        );
    }
    println!(
        "3 persisted song associations: restart reconstruction {:?}",
        start.elapsed() / 1000
    );
}
