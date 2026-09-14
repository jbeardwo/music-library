use music_library::{
    Library,
    album_matching::{AlbumMatcher, AutoMatchPolicy, MatchOutcome, MatchReply},
    album_program::{self, Program, Programs},
    catalog::*,
    domain::*,
    edition::{RecordingEvidence, TrackEvidence},
    filesystem::MetadataExtractor,
};
use std::sync::{Arc, Mutex, mpsc};

fn identity(kind: &str, value: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: "musicbrainz".into(),
        kind: kind.into(),
        external_id: value.into(),
    }
}
struct Tags(String);
impl MetadataExtractor for Tags {
    fn supports(&self, _: &std::path::Path) -> bool {
        true
    }
    fn read(&mut self, _: &std::path::Path) -> music_library::Result<ObservedMetadata> {
        Ok(ObservedMetadata {
            release_title: Some(self.0.clone()),
            release_artists: vec!["Artist".into()],
            track_title: Some("Song".into()),
            track_number: Some(1),
            ..Default::default()
        })
    }
}
fn import(lib: &mut Library, folder: &std::path::Path, title: &str) -> ImportedRelease {
    std::fs::create_dir(folder).unwrap();
    std::fs::write(folder.join("song"), b"fixture").unwrap();
    let root = lib.register_local_root(folder).unwrap();
    lib.scan_local_root(&root, &mut Tags(title.into())).unwrap();
    let tracks = lib
        .list_discovery_candidates(None, 10)
        .unwrap()
        .into_iter()
        .filter(|source| source.metadata.release_title.as_deref() == Some(title))
        .map(|s| ImportTrackInput {
            source_id: s.source_id,
            title_fallback: None,
            artists: vec![],
            disc_number: None,
            track_number: Some(1),
        })
        .collect();
    lib.import_release(&ImportReleaseRequest {
        release_title: title.into(),
        release_artists: vec![],
        tracks,
    })
    .unwrap()
}
#[derive(Clone, Copy, PartialEq)]
enum Scenario {
    Matched,
    UnresolvedTrack,
    ArtistAmbiguous,
    AlbumAmbiguous,
    NoAlbum,
    Outage,
}
struct Provider {
    log: Arc<Mutex<Vec<String>>>,
    scenario: Scenario,
    failed: bool,
}
fn page<T>(items: Vec<T>) -> Page<T> {
    Page {
        items,
        next_offset: None,
    }
}
impl CatalogProvider for Provider {
    fn album_program_namespaces(&self) -> Vec<(String, String)> {
        vec![("musicbrainz".into(), "release_group".into())]
    }
    fn search_artists(&mut self, name: &str) -> Result<Page<ArtistCandidate>, CatalogError> {
        self.log.lock().unwrap().push(format!("artist:{name}"));
        let candidate = |id| ArtistCandidate {
            identity: identity("artist", id),
            name: name.into(),
            aliases: vec![],
            comment: String::new(),
            country: String::new(),
            artist_type: String::new(),
            score: None,
        };
        if self.scenario == Scenario::ArtistAmbiguous && !self.failed {
            self.failed = true;
            Ok(page(vec![candidate("a"), candidate("b")]))
        } else {
            Ok(page(vec![candidate("a")]))
        }
    }
    fn artist_albums(
        &mut self,
        artist: &ExternalIdentity,
        title: &str,
    ) -> Result<Page<ArtistAlbumCandidate>, CatalogError> {
        self.log.lock().unwrap().push(format!("album:{title}"));
        let candidate = |id: &str| ArtistAlbumCandidate {
            identity: identity("release_group", id),
            title: title.into(),
            artist: "Artist".into(),
            artist_ids: vec![artist.clone()],
            date: String::new(),
            primary_type: "Album".into(),
            comment: String::new(),
        };
        Ok(page(
            if title == "a" && self.scenario == Scenario::NoAlbum {
                vec![]
            } else if title == "a" && self.scenario == Scenario::AlbumAmbiguous {
                vec![candidate("A"), candidate("A-other")]
            } else {
                vec![candidate(title)]
            },
        ))
    }
    fn album_programs(&mut self, album: &ExternalIdentity) -> Result<Programs, CatalogError> {
        self.log
            .lock()
            .unwrap()
            .push(format!("program:{}", album.external_id));
        if self.scenario == Scenario::Outage && !self.failed {
            self.failed = true;
            return Err(CatalogError::Timeout("provider unavailable".into()));
        }
        Ok(Programs {
            album: album.clone(),
            programs: vec![Program {
                identity: None,
                complete: true,
                tracks: vec![TrackEvidence {
                    number: Some(1),
                    title: Some(
                        if self.scenario == Scenario::UnresolvedTrack && album.external_id == "a" {
                            "Unrelated"
                        } else {
                            "Song"
                        }
                        .into(),
                    ),
                    recording: RecordingEvidence {
                        identities: vec![identity("recording", &album.external_id)],
                        isrcs: vec![],
                    },
                    ..Default::default()
                }],
            }],
            note: String::new(),
        })
    }
    fn search_albums(&mut self, _: &str, _: u32) -> Result<Page<AlbumCandidate>, CatalogError> {
        panic!("no broad search")
    }
    fn releases(
        &mut self,
        _: &ExternalIdentity,
        _: u32,
    ) -> Result<Page<ReleaseCandidate>, CatalogError> {
        panic!("no per-Track requests")
    }
    fn release(&mut self, _: &ExternalIdentity) -> Result<Release, CatalogError> {
        panic!("no per-Track requests")
    }
}
enum Reply {
    Album(Box<MatchReply>),
    Programs(Box<album_program::Reply>),
}

#[test]
fn album_jobs_finish_enrichment_before_advancing_and_manual_outcomes_do_not_block() {
    for scenario in [
        Scenario::Matched,
        Scenario::UnresolvedTrack,
        Scenario::ArtistAmbiguous,
        Scenario::AlbumAmbiguous,
        Scenario::NoAlbum,
        Scenario::Outage,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut lib = Library::open(temp.path().join("db")).unwrap();
        let a = import(&mut lib, &temp.path().join("A"), "A");
        let b = import(&mut lib, &temp.path().join("B"), "B");
        let album_a = lib.album_for_release(&a.release_id).unwrap().album_id;
        let album_b = lib.album_for_release(&b.release_id).unwrap().album_id;
        assert_ne!(album_a, album_b);
        let log = Arc::new(Mutex::new(vec![]));
        let (tx, rx) = mpsc::channel();
        let albums = tx.clone();
        let mut matcher = AlbumMatcher::new_with_programs(
            Provider {
                log: log.clone(),
                scenario,
                failed: false,
            },
            move |r| albums.send(Reply::Album(Box::new(r))).unwrap(),
            move |r| tx.send(Reply::Programs(Box::new(r))).unwrap(),
            |_| {},
        )
        .unwrap();
        matcher
            .after_import(&lib, &[a, b], AutoMatchPolicy::default())
            .unwrap();
        while matcher.pending_count() > 0 {
            match rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap() {
                Reply::Album(r) => {
                    matcher.complete(&mut lib, *r);
                }
                Reply::Programs(r) => {
                    let id = r.input.album_id.clone();
                    let outcome = matcher.complete_programs(&mut lib, *r);
                    if matches!(outcome, album_program::Outcome::Deferred(_)) {
                        assert!(!log.lock().unwrap().iter().any(|s| s == "album:b"));
                        assert!(rx.try_recv().is_err());
                        let token = matcher.retry_schedule().unwrap().token;
                        matcher.cooldown_elapsed(&lib, token).unwrap();
                    } else if id == album_a {
                        assert!(matches!(outcome, album_program::Outcome::Complete(_)));
                    }
                }
            }
        }
        assert!(matches!(
            matcher.program_outcome(&album_b),
            Some(album_program::Outcome::Complete(_))
        ));
        if scenario == Scenario::UnresolvedTrack {
            assert!(
                matches!(matcher.program_outcome(&album_a),Some(album_program::Outcome::Complete(rows)) if rows.iter().all(|(_,outcome)|matches!(outcome,album_program::TrackOutcome::NoConfidentMatch)))
            );
        }
        for track in lib.local_album_tracks(&album_b).unwrap() {
            assert!(
                lib.list_recording_external_identities(&track.recording_id)
                    .unwrap()
                    .contains(&identity("recording", "b"))
            );
        }
        let calls = log.lock().unwrap();
        let b_start = calls
            .iter()
            .position(|s| s == "album:b")
            .unwrap_or_else(|| panic!("Missing B search: {calls:?}"));
        if matches!(
            scenario,
            Scenario::Matched | Scenario::UnresolvedTrack | Scenario::Outage
        ) {
            assert!(
                calls.iter().rposition(|s| s == "program:a").unwrap() < b_start,
                "{calls:?}"
            );
        } else {
            assert!(!calls.iter().any(|s| s == "program:a"));
            assert!(matches!(
                matcher.outcome(&album_a),
                Some(
                    MatchOutcome::ArtistAmbiguous(_)
                        | MatchOutcome::AlbumAmbiguous(_)
                        | MatchOutcome::NoConfidentMatch
                )
            ));
        }
        assert_eq!(
            calls.iter().filter(|s| s.starts_with("program:")).count(),
            match scenario {
                Scenario::Matched | Scenario::UnresolvedTrack => 2,
                Scenario::Outage => 3,
                _ => 1,
            }
        );
        assert!(matcher.cached_programs(&album_b).is_some());
    }
}
