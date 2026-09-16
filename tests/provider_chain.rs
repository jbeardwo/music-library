use music_library::{
    Library,
    album_matching::{AlbumMatcher, AutoMatchPolicy, MatchOutcome, MatchReply},
    album_program::{self, Program, Programs, TrackOutcome},
    catalog::*,
    domain::*,
    edition::TrackEvidence,
    filesystem::MetadataExtractor,
    provider_chain::{ProviderChain, ProviderSlot},
};
use std::sync::{Arc, Mutex, mpsc};
fn id(p: &str, k: &str, v: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: p.into(),
        kind: k.into(),
        external_id: v.into(),
    }
}
fn scope(p: &str) -> MatchingScope {
    MatchingScope {
        provider: p.into(),
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
#[derive(Clone, Copy)]
enum Mode {
    Found,
    NoAlbum,
    ArtistAmbiguous,
    AlbumAmbiguous,
    Unavailable,
    Configuration,
    ProgramOutage,
    Recording,
}
struct Fake {
    name: String,
    mode: Mode,
    calls: Arc<Mutex<Vec<String>>>,
}
impl Fake {
    fn call(&self, what: &str) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{}:{what}", self.name));
    }
}
impl CatalogProvider for Fake {
    fn album_program_namespaces(&self) -> Vec<(String, String)> {
        vec![(self.name.clone(), "album".into())]
    }
    fn search_artists(&mut self, _: &str) -> Result<Page<ArtistCandidate>, CatalogError> {
        self.call("artist");
        if matches!(self.mode, Mode::Unavailable) {
            self.mode = Mode::Found;
            return Err(CatalogError::ServiceUnavailable {
                message: "busy".into(),
                retry_after: None,
            });
        }
        if matches!(self.mode, Mode::Configuration) {
            return Err(CatalogError::Configuration {
                status: 403,
                message: "access denied".into(),
            });
        }
        let candidate = ArtistCandidate {
            identity: id(&self.name, "artist", "artist"),
            name: "Artist".into(),
            aliases: vec![],
            comment: String::new(),
            country: String::new(),
            artist_type: String::new(),
            score: None,
        };
        let mut items = vec![candidate.clone()];
        if matches!(self.mode, Mode::ArtistAmbiguous) {
            items.push(ArtistCandidate {
                identity: id(&self.name, "artist", "other"),
                ..candidate
            })
        }
        Ok(page(items))
    }
    fn artist_albums(
        &mut self,
        a: &ExternalIdentity,
        title: &str,
    ) -> Result<Page<ArtistAlbumCandidate>, CatalogError> {
        self.call("album");
        if matches!(self.mode, Mode::NoAlbum) {
            return Ok(page(vec![]));
        }
        let c = ArtistAlbumCandidate {
            identity: id(&self.name, "album", title),
            title: title.into(),
            artist: "Artist".into(),
            artist_ids: vec![a.clone()],
            primary_type: String::new(),
            date: String::new(),
            comment: String::new(),
        };
        let mut items = vec![c.clone()];
        if matches!(self.mode, Mode::AlbumAmbiguous) {
            items.push(ArtistAlbumCandidate {
                identity: id(&self.name, "album", "alternative"),
                ..c
            })
        }
        Ok(page(items))
    }
    fn album_programs(&mut self, a: &ExternalIdentity) -> Result<Programs, CatalogError> {
        self.call("program");
        if matches!(self.mode, Mode::ProgramOutage) {
            self.mode = Mode::Found;
            return Err(CatalogError::ServiceUnavailable {
                message: "busy".into(),
                retry_after: None,
            });
        }
        Ok(Programs {
            album: a.clone(),
            programs: vec![Program {
                identity: None,
                complete: true,
                tracks: (1..=10)
                    .map(|n| TrackEvidence {
                        title: Some(format!("Song {n}")),
                        disc: Some(1),
                        number: Some(n),
                        identities: vec![id(&self.name, "song", &n.to_string())],
                        recording: music_library::edition::RecordingEvidence {
                            identities: if matches!(self.mode, Mode::Recording) {
                                vec![id(&self.name, "performance", &n.to_string())]
                            } else {
                                vec![]
                            },
                            isrcs: vec![],
                        },
                        ..Default::default()
                    })
                    .collect(),
            }],
            note: String::new(),
        })
    }
    fn search_albums(&mut self, _: &str, _: u32) -> Result<Page<AlbumCandidate>, CatalogError> {
        panic!("no global search")
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
            release_title: Some(
                p.parent()
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .into(),
            ),
            release_artists: vec!["Artist".into()],
            track_title: Some(format!("Song {n}")),
            track_number: Some(n),
            disc_number: Some(1),
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
        release_title: folder.file_name().unwrap().to_str().unwrap().into(),
        release_artists: vec![],
        tracks,
    })
    .unwrap()
}
enum Event {
    Album(Box<MatchReply>),
    Program(Box<album_program::Reply>),
}
fn chain(
    modes: &[(&str, Mode)],
    calls: Arc<Mutex<Vec<String>>>,
) -> (ProviderChain, mpsc::Receiver<Event>) {
    let (tx, rx) = mpsc::channel();
    let slots = modes
        .iter()
        .map(|(name, mode)| {
            let a = tx.clone();
            let p = tx.clone();
            ProviderSlot {
                scope: scope(name),
                matcher: AlbumMatcher::for_provider(
                    Fake {
                        name: (*name).into(),
                        mode: *mode,
                        calls: calls.clone(),
                    },
                    scope(name),
                    move |r| {
                        a.send(Event::Album(Box::new(r))).unwrap();
                    },
                    move |r| {
                        p.send(Event::Program(Box::new(r))).unwrap();
                    },
                    |_| {},
                )
                .map_err(|e| e.to_string()),
            }
        })
        .collect();
    (ProviderChain::new(slots).unwrap(), rx)
}
fn drain(c: &mut ProviderChain, rx: &mpsc::Receiver<Event>, lib: &mut Library) {
    while c.is_processing() {
        match rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap() {
            Event::Album(r) => {
                c.complete(lib, *r);
            }
            Event::Program(r) => {
                c.complete_programs(lib, *r);
            }
        }
    }
}
#[test]
fn bounded_failures_fall_back_but_success_never_queries_second_provider() {
    for mode in [
        Mode::Found,
        Mode::NoAlbum,
        Mode::ArtistAmbiguous,
        Mode::AlbumAmbiguous,
        Mode::Unavailable,
        Mode::Configuration,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut lib = Library::open(temp.path().join("db")).unwrap();
        let imported = import(&mut lib, &temp.path().join("Album"));
        let album = lib
            .album_for_release(&imported.release_id)
            .unwrap()
            .album_id;
        let calls = Arc::new(Mutex::new(vec![]));
        let (mut c, rx) = chain(&[("a", mode), ("b", Mode::Found)], calls.clone());
        c.after_import(
            &lib,
            std::slice::from_ref(&imported),
            AutoMatchPolicy::default(),
        )
        .unwrap();
        drain(&mut c, &rx, &mut lib);
        let winner = if matches!(mode, Mode::Found) {
            "a"
        } else {
            "b"
        };
        assert_eq!(c.provider(&album), winner);
        assert!(matches!(c.outcome(&album), Some(MatchOutcome::Matched(_))));
        let log = calls.lock().unwrap().clone();
        if winner == "a" {
            assert!(log.iter().all(|s| s.starts_with("a:")))
        } else {
            assert!(log[0].starts_with("a:"));
            assert!(log.iter().any(|s| s == "b:program"));
            assert!(!log.iter().any(|s| s == "a:program"));
            assert_eq!(c.attempts(&album).len(), 2);
        }
        assert_eq!(lib.list_album_external_identities(&album).unwrap().len(), 1);
        if matches!(mode, Mode::NoAlbum | Mode::AlbumAmbiguous) {
            assert!(
                !lib.resolve_artists_external_identity(&id("a", "artist", "artist"))
                    .unwrap()
                    .is_empty(),
                "independently resolved Artist survives Album failure"
            );
        }
        assert!(
            lib.list_release_external_identities(&imported.release_id)
                .unwrap()
                .is_empty()
        );
        assert_eq!(lib.local_album_tracks(&album).unwrap().len(), 3);
        assert_eq!(
            lib.provider_track_associations(&album, winner)
                .unwrap()
                .len(),
            3
        );
        assert!(
            matches!(c.program_outcome(&album),Some(album_program::Outcome::Complete(rows))if rows.iter().all(|(_,r)|matches!(r,TrackOutcome::Matched(m)if m.recording.identities.is_empty())))
        );
        let before = calls.lock().unwrap().len();
        c.retry_catalog_matching(&lib).unwrap();
        assert_eq!(
            calls.lock().unwrap().len(),
            before,
            "recovery must not enrich a successful fallback Album again"
        );
    }
}
#[test]
fn order_known_identity_restart_and_album_job_locality() {
    for order in [["a", "b"], ["b", "a"]] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("db");
        let mut lib = Library::open(&path).unwrap();
        let a = import(&mut lib, &temp.path().join("AlbumA"));
        let b = import(&mut lib, &temp.path().join("AlbumB"));
        let album = lib.album_for_release(&a.release_id).unwrap().album_id;
        let calls = Arc::new(Mutex::new(vec![]));
        let (mut c, rx) = chain(
            &[(order[0], Mode::Found), (order[1], Mode::Found)],
            calls.clone(),
        );
        c.after_import(&lib, &[a.clone(), b], AutoMatchPolicy::default())
            .unwrap();
        drain(&mut c, &rx, &mut lib);
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                format!("{}:artist", order[0]),
                format!("{}:album", order[0]),
                format!("{}:program", order[0]),
                format!("{}:album", order[0]),
                format!("{}:program", order[0])
            ]
        );
        drop(c);
        drop(lib);
        let mut lib = Library::open(&path).unwrap();
        assert_eq!(
            lib.provider_track_associations(&album, order[0])
                .unwrap()
                .len(),
            3
        );
        calls.lock().unwrap().clear();
        let (mut c, rx) = chain(
            &[(order[1], Mode::Found), (order[0], Mode::Found)],
            calls.clone(),
        );
        c.after_import(&lib, &[a], AutoMatchPolicy::default())
            .unwrap();
        drain(&mut c, &rx, &mut lib);
        assert_eq!(
            *calls.lock().unwrap(),
            vec![format!("{}:program", order[0])],
            "accepted identity beats rediscovery even after reversing configuration"
        );
    }
}
#[test]
fn unresolved_tracks_do_not_trigger_another_provider() {
    let temp = tempfile::tempdir().unwrap();
    let mut lib = Library::open(temp.path().join("db")).unwrap();
    let imported = import(&mut lib, &temp.path().join("Album"));
    let album = lib
        .album_for_release(&imported.release_id)
        .unwrap()
        .album_id;
    let track = lib.local_album_tracks(&album).unwrap().remove(0);
    lib.set_track_title_override(&track.track_id, "Unknown local song")
        .unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let (mut c, rx) = chain(&[("a", Mode::Found), ("b", Mode::Found)], calls.clone());
    c.after_import(&lib, &[imported], AutoMatchPolicy::default())
        .unwrap();
    drain(&mut c, &rx, &mut lib);
    assert_eq!(c.pending_count(), 0);
    assert!(calls.lock().unwrap().iter().all(|s| s.starts_with("a:")));
    assert!(
        matches!(c.program_outcome(&album),Some(album_program::Outcome::Complete(rows))if rows.iter().any(|(_,r)|matches!(r,TrackOutcome::NoConfidentMatch)))
    );
}

#[test]
fn manual_provider_pin_coexisting_ids_and_recording_confirmation() {
    let temp = tempfile::tempdir().unwrap();
    let mut lib = Library::open(temp.path().join("db")).unwrap();
    let imported = import(&mut lib, &temp.path().join("Album"));
    let album = lib
        .album_for_release(&imported.release_id)
        .unwrap()
        .album_id;
    for p in ["a", "b"] {
        lib.attach_album_external_identity(&album, &id(p, "album", "album"))
            .unwrap();
    }
    let track = lib.local_album_tracks(&album).unwrap().remove(0);
    let mut provider = Fake {
        name: "b".into(),
        mode: Mode::Recording,
        calls: Arc::new(Mutex::new(vec![])),
    };
    let programs = provider.album_programs(&id("b", "album", "album")).unwrap();
    let selection = lib
        .prepare_manual_track(&album, &track.track_id, &programs)
        .unwrap();
    lib.confirm_manual_track(&selection, 0).unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let (mut c, rx) = chain(&[("a", Mode::Found), ("b", Mode::Recording)], calls.clone());
    c.after_import(&lib, &[imported], AutoMatchPolicy::default())
        .unwrap();
    drain(&mut c, &rx, &mut lib);
    assert_eq!(*calls.lock().unwrap(), vec!["b:program"]);
    assert_eq!(lib.list_album_external_identities(&album).unwrap().len(), 2);
    assert!(
        matches!(c.program_outcome(&album),Some(album_program::Outcome::Complete(rows))if rows.iter().any(|(_,r)|matches!(r,TrackOutcome::ManuallyMatched(_))))
    );
    assert!(
        lib.local_album_tracks(&album).unwrap().iter().all(|t| !t
            .evidence
            .recording
            .identities
            .is_empty())
    );
}

#[test]
fn configuration_failure_without_worker_still_allows_configured_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let mut lib = Library::open(temp.path().join("db")).unwrap();
    let imported = import(&mut lib, &temp.path().join("Album"));
    let album = lib
        .album_for_release(&imported.release_id)
        .unwrap()
        .album_id;
    let (tx, rx) = mpsc::channel();
    let p = tx.clone();
    let calls = Arc::new(Mutex::new(vec![]));
    let matcher = AlbumMatcher::for_provider(
        Fake {
            name: "b".into(),
            mode: Mode::Found,
            calls: calls.clone(),
        },
        scope("b"),
        move |r| {
            tx.send(Event::Album(Box::new(r))).unwrap();
        },
        move |r| {
            p.send(Event::Program(Box::new(r))).unwrap();
        },
        |_| {},
    )
    .unwrap();
    let mut c = ProviderChain::new(vec![
        ProviderSlot {
            scope: scope("a"),
            matcher: Err("Missing credentials".into()),
        },
        ProviderSlot {
            scope: scope("b"),
            matcher: Ok(matcher),
        },
    ])
    .unwrap();
    c.after_import(&lib, &[imported], AutoMatchPolicy::default())
        .unwrap();
    drain(&mut c, &rx, &mut lib);
    assert!(matches!(
        c.attempts(&album)[0].outcome,
        MatchOutcome::ConfigurationError(_)
    ));
    assert_eq!(c.provider(&album), "b");
    assert!(c.provider_messages()[0].contains("Missing credentials"));
}

#[test]
fn automatic_cooldown_is_serial_and_wakes_all_preserved_jobs_after_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let mut lib = Library::open(temp.path().join("db")).unwrap();
    let a = import(&mut lib, &temp.path().join("AlbumA"));
    let b = import(&mut lib, &temp.path().join("AlbumB"));
    let calls = Arc::new(Mutex::new(vec![]));
    let (mut c, rx) = chain(
        &[("a", Mode::Unavailable), ("b", Mode::NoAlbum)],
        calls.clone(),
    );
    c.after_import(&lib, &[a, b], AutoMatchPolicy::default())
        .unwrap();
    drain(&mut c, &rx, &mut lib);
    assert_eq!(c.pending_count(), 2);
    assert_eq!(
        calls
            .lock()
            .unwrap()
            .iter()
            .filter(|s| *s == "a:artist")
            .count(),
        1
    );
    let token = c.retry_schedule_for(0).unwrap().token;
    c.cooldown_provider(&lib, 0, token).unwrap();
    c.cooldown_provider(&lib, 0, token).unwrap();
    drain(&mut c, &rx, &mut lib);
    assert_eq!(c.pending_count(), 0);
    assert_eq!(
        calls
            .lock()
            .unwrap()
            .iter()
            .filter(|s| *s == "a:program")
            .count(),
        2
    );
    let before = calls.lock().unwrap().len();
    c.cooldown_provider(&lib, 0, token).unwrap();
    assert_eq!(calls.lock().unwrap().len(), before);
}

#[test]
fn all_misses_finish_and_outages_preserve_work_until_explicit_probe() {
    for modes in [
        [Mode::NoAlbum, Mode::NoAlbum],
        [Mode::NoAlbum, Mode::Unavailable],
        [Mode::ProgramOutage, Mode::Found],
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut lib = Library::open(temp.path().join("db")).unwrap();
        let imported = import(&mut lib, &temp.path().join("Album"));
        let album = lib
            .album_for_release(&imported.release_id)
            .unwrap()
            .album_id;
        let calls = Arc::new(Mutex::new(vec![]));
        let (mut c, rx) = chain(&[("a", modes[0]), ("b", modes[1])], calls.clone());
        c.after_import(&lib, &[imported], AutoMatchPolicy::default())
            .unwrap();
        drain(&mut c, &rx, &mut lib);
        if matches!(modes[1], Mode::NoAlbum) {
            assert_eq!(c.pending_count(), 0);
            assert_eq!(c.outcome(&album), Some(&MatchOutcome::NoConfidentMatch));
        } else {
            assert_eq!(c.pending_count(), 1);
            c.retry_catalog_matching(&lib).unwrap();
            drain(&mut c, &rx, &mut lib);
            assert_eq!(c.pending_count(), 0);
            assert!(matches!(
                c.outcome(&album),
                Some(MatchOutcome::Matched(_) | MatchOutcome::AlreadyMatched)
            ));
        }
        if matches!(modes[0], Mode::ProgramOutage) {
            assert!(
                calls.lock().unwrap().iter().all(|s| s.starts_with("a:")),
                "Track outage must not switch a successful Album to another provider"
            );
        }
    }
}
