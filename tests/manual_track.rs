use music_library::{
    Library,
    album_program::{Outcome, Program, Programs, Reply, TrackOutcome},
    domain::*,
    edition::*,
    filesystem::MetadataExtractor,
    manual_track,
};
use rusqlite::Connection;
fn id(kind: &str, value: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: "synthetic".into(),
        kind: kind.into(),
        external_id: value.into(),
    }
}
fn programs(recordings: bool) -> Programs {
    let tracks = (1..=15)
        .map(|n| TrackEvidence {
            identities: vec![id("song", &n.to_string())],
            disc: Some(1),
            number: Some(n),
            title: Some(format!("Provider Song {n}")),
            duration_ms: Some(280_000),
            recording: RecordingEvidence {
                identities: if recordings {
                    vec![id("performance", &n.to_string())]
                } else {
                    vec![]
                },
                isrcs: vec!["SUPPORTING".into()],
            },
            ..Default::default()
        })
        .collect();
    let p = Program {
        identity: Some(id("catalog-edition", "diagnostic-only")),
        tracks,
        complete: true,
    };
    Programs {
        album: id("album", "known"),
        programs: vec![p.clone(), p],
        note: String::new(),
    }
}
struct Tags(bool);
impl MetadataExtractor for Tags {
    fn supports(&self, _: &std::path::Path) -> bool {
        true
    }
    fn read(&mut self, p: &std::path::Path) -> music_library::Result<ObservedMetadata> {
        let n = p.file_name().unwrap().to_str().unwrap().parse().unwrap();
        Ok(ObservedMetadata {
            track_title: Some(if self.0 {
                "Retagged local title".into()
            } else {
                format!("Local Song {n}")
            }),
            release_title: Some("Known Album".into()),
            release_artists: vec!["Artist".into()],
            track_artists: vec!["Artist".into()],
            track_number: Some(n),
            disc_number: Some(1),
            duration_ms: Some(276_898),
            ..Default::default()
        })
    }
}
struct Fixture {
    temp: tempfile::TempDir,
    lib: Library,
    album: AlbumId,
    release: ImportedRelease,
    root: RootId,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("music");
        std::fs::create_dir(&folder).unwrap();
        for n in [2, 5, 10] {
            std::fs::write(folder.join(n.to_string()), b"audio").unwrap();
        }
        let mut lib = Library::open(temp.path().join("db")).unwrap();
        let root = lib.register_local_root(&folder).unwrap();
        lib.scan_local_root(&root, &mut Tags(false)).unwrap();
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
        let release = lib
            .import_release(&ImportReleaseRequest {
                release_title: "Known Album".into(),
                release_artists: vec![],
                tracks,
            })
            .unwrap();
        let album = lib.album_for_release(&release.release_id).unwrap().album_id;
        lib.attach_album_external_identity(&album, &id("album", "known"))
            .unwrap();
        Self {
            temp,
            lib,
            album,
            release,
            root,
        }
    }
    fn track(&self) -> TrackId {
        self.lib.local_album_tracks(&self.album).unwrap()[0]
            .track_id
            .clone()
    }
    fn db(&self) -> Connection {
        Connection::open(self.temp.path().join("db")).unwrap()
    }
    fn choose(&mut self, recordings: bool) -> manual_track::Association {
        let s = self
            .lib
            .prepare_manual_track(&self.album, &self.track(), &programs(recordings))
            .unwrap();
        self.lib.confirm_manual_track(&s, 1).unwrap()
    }
    fn count(&self, table: &str) -> i64 {
        self.db()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
}

#[test]
fn candidates_deduplicate_recordings_or_occurrences_without_title_merges() {
    for strong in [true, false] {
        let mut p = programs(strong);
        let c = manual_track::candidates(&p);
        assert_eq!(c.len(), 15);
        assert!(c.iter().all(|c| c.supporting_programs == 2));
        if strong {
            p.programs[1].tracks[0].recording.identities = vec![id("performance", "different")];
        } else {
            p.programs[1].tracks[0].identities = vec![id("song", "different")];
        }
        assert_eq!(manual_track::candidates(&p).len(), 16);
    }
}

#[test]
#[ignore = "opt-in local candidate preparation timing"]
fn manual_candidate_preparation_timing() {
    for count in [5, 15, 30] {
        let mut p = programs(true);
        let template = p.programs[0].tracks[0].clone();
        let tracks = (1..=count)
            .map(|n| {
                let mut t = template.clone();
                t.number = Some(n);
                t.title = Some(format!("Song {n}"));
                t.recording.identities = vec![id("performance", &n.to_string())];
                t
            })
            .collect();
        p.programs = vec![
            Program {
                identity: None,
                tracks,
                complete: true
            };
            3
        ];
        let start = std::time::Instant::now();
        for _ in 0..1000 {
            assert_eq!(
                std::hint::black_box(manual_track::candidates(&p)).len(),
                count as usize
            );
        }
        eprintln!(
            "{count} Tracks / 3 programs: {:?} per candidate preparation",
            start.elapsed() / 1000
        );
    }
}

#[test]
fn chooser_uses_owned_album_worker_then_cache_without_global_or_track_requests() {
    use music_library::{album_matching::AlbumMatcher, catalog::*};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };
    struct Provider(Arc<AtomicUsize>);
    impl CatalogProvider for Provider {
        fn album_program_namespaces(&self) -> Vec<(String, String)> {
            vec![("synthetic".into(), "album".into())]
        }
        fn album_programs(&mut self, album: &ExternalIdentity) -> Result<Programs, CatalogError> {
            assert_eq!(album, &id("album", "known"));
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(programs(true))
        }
        fn search_albums(&mut self, _: &str, _: u32) -> Result<Page<AlbumCandidate>, CatalogError> {
            panic!("no global search")
        }
        fn releases(
            &mut self,
            _: &ExternalIdentity,
            _: u32,
        ) -> Result<Page<ReleaseCandidate>, CatalogError> {
            panic!("no edition selection")
        }
        fn release(&mut self, _: &ExternalIdentity) -> Result<Release, CatalogError> {
            panic!("no per-Track lookup")
        }
    }
    let mut f = Fixture::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = mpsc::channel();
    let mut matcher = AlbumMatcher::new_with_programs(
        Provider(calls.clone()),
        |_| panic!("no Artist/Album discovery"),
        move |r| tx.send(r).unwrap(),
        |_| {},
    )
    .unwrap();
    matcher.request_album_programs(&f.lib, &f.album).unwrap();
    matcher.request_album_programs(&f.lib, &f.album).unwrap();
    let reply = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    matcher.complete_programs(&mut f.lib, reply);
    let selection = f
        .lib
        .prepare_manual_track(
            &f.album,
            &f.track(),
            matcher.cached_programs(&f.album).unwrap(),
        )
        .unwrap();
    f.lib.confirm_manual_track(&selection, 1).unwrap();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "opening/confirming uses the cached Album programs"
    );
    assert!(rx.try_recv().is_err());
    // Reproduce Clear's real orchestration, not just its storage deletion:
    // a completed Album must be enqueued again and leave Pending on reply.
    let track = f.track();
    for local in f
        .lib
        .local_album_tracks(&f.album)
        .unwrap()
        .into_iter()
        .skip(1)
    {
        f.lib
            .attach_recording_external_identity(
                &local.recording_id,
                &id("performance", &local.evidence.number.unwrap().to_string()),
            )
            .unwrap();
    }
    f.lib.clear_manual_track(&f.album, &track).unwrap();
    matcher.clear_program_outcome(&f.album);
    matcher.request_album_programs(&f.lib, &f.album).unwrap();
    assert_eq!(matcher.program_outcome(&f.album), Some(&Outcome::Pending));
    let mut reply = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    // Exact titles with Rich Kid's duration discrepancy, all other local Tracks
    // already independently identified. No special-case title logic is involved.
    if let Ok(p) = &mut reply.result {
        for program in &mut p.programs {
            for t in &mut program.tracks {
                t.title = Some(format!("Local Song {}", t.number.unwrap()));
            }
        }
    }
    matcher.complete_programs(&mut f.lib, reply);
    assert!(
        matches!(matcher.program_outcome(&f.album),Some(Outcome::Complete(rows)) if rows.iter().all(|(_,o)|matches!(o,TrackOutcome::Matched(m)|TrackOutcome::AlreadyMatched(m) if m.recording_status==music_library::album_program::RecordingStatus::Identified)))
    );
    assert_eq!(matcher.pending_count(), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    // Keep the outage/preservation assertions below about a manual association.
    let selection = f
        .lib
        .prepare_manual_track(&f.album, &track, matcher.cached_programs(&f.album).unwrap())
        .unwrap();
    f.lib.confirm_manual_track(&selection, 1).unwrap();
    let input = f
        .lib
        .prepare_album_program(&f.album, &id("album", "known"))
        .unwrap()
        .unwrap();
    matcher.complete_programs(
        &mut f.lib,
        Reply {
            input,
            result: Err(CatalogError::Timeout("offline".into())),
        },
    );
    matcher.request_album_programs(&f.lib, &f.album).unwrap();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "enqueue cannot bypass the paused circuit"
    );
    assert_eq!(f.lib.manual_track_associations(&f.album).unwrap().len(), 1);
}
#[test]
fn manual_recording_choice_is_durable_independent_of_tags_and_edition() {
    let mut f = Fixture::new();
    let before = f.lib.edition_evidence(&f.release.release_id).unwrap();
    let chosen = f.choose(true);
    let r = f.lib.recording_for_track(&f.track()).unwrap().recording_id;
    assert_eq!(
        f.lib.list_recording_external_identities(&r).unwrap(),
        vec![id("performance", "2")]
    );
    assert_eq!(f.count("recording_manual_identity"), 1);
    assert_eq!(f.count("recording_provenance_identity"), 0);
    let s = f
        .lib
        .prepare_manual_track(&f.album, &f.track(), &programs(true))
        .unwrap();
    assert_eq!(f.lib.confirm_manual_track(&s, 1).unwrap(), chosen);
    drop(f.lib);
    f.lib = Library::open(f.temp.path().join("db")).unwrap();
    assert_eq!(
        f.lib.manual_track_associations(&f.album).unwrap(),
        vec![chosen]
    );
    assert_eq!(f.count("track"), 3);
    assert_eq!(f.count("library_membership"), 3);
    assert_eq!(f.count("track_source"), 3);
    assert_eq!(f.count("release_external_identity"), 0);
    assert_eq!(f.count("track_external_identity"), 0);
    let after = f.lib.edition_evidence(&f.release.release_id).unwrap();
    assert_eq!(before.completeness, Completeness::Unknown);
    assert_eq!(after.completeness, Completeness::Unknown);
    for (a, b) in before.tracks.iter().zip(after.tracks) {
        assert_eq!(a.evidence.title, b.evidence.title);
    }
}
#[test]
fn occurrence_only_choice_survives_without_recording_or_exact_edition() {
    let mut f = Fixture::new();
    let chosen = f.choose(false);
    assert_eq!(chosen.candidate.evidence.identities, vec![id("song", "2")]);
    assert_eq!(f.count("recording_external_identity"), 0);
    assert_eq!(f.count("track_external_identity"), 0);
    assert_eq!(f.count("release_external_identity"), 0);
    drop(f.lib);
    f.lib = Library::open(f.temp.path().join("db")).unwrap();
    assert_eq!(
        f.lib.manual_track_associations(&f.album).unwrap(),
        vec![chosen]
    );
}
#[test]
fn automatic_reply_and_sampling_changes_cannot_undo_manual_choice() {
    let mut f = Fixture::new();
    f.choose(true);
    let input = f
        .lib
        .prepare_album_program(&f.album, &id("album", "known"))
        .unwrap()
        .unwrap();
    let mut p = programs(true);
    for p in &mut p.programs {
        p.tracks.clear();
    }
    let Outcome::Complete(rows) = f
        .lib
        .complete_album_program(Reply {
            input,
            result: Ok(p),
        })
        .unwrap()
    else {
        panic!()
    };
    assert!(matches!(&rows[0].1,TrackOutcome::ManuallyMatched(m) if m.title=="Provider Song 2"));
    assert_eq!(
        f.count("recording_manual_identity"),
        1,
        "same-row automatic refresh is not an independent confirmation"
    );
}
#[test]
fn clear_removes_sole_claim_and_allows_automatic_reassessment() {
    let mut f = Fixture::new();
    f.choose(true);
    let track = f.track();
    assert!(f.lib.clear_manual_track(&f.album, &track).unwrap());
    assert!(!f.lib.clear_manual_track(&f.album, &track).unwrap());
    assert_eq!(f.count("manual_track_association"), 0);
    assert_eq!(f.count("recording_external_identity"), 0);
    let input = f
        .lib
        .prepare_album_program(&f.album, &id("album", "known"))
        .unwrap()
        .unwrap();
    let Outcome::Complete(rows) = f
        .lib
        .complete_album_program(Reply {
            input,
            result: Ok(programs(true)),
        })
        .unwrap()
    else {
        panic!()
    };
    assert!(!matches!(rows[0].1, TrackOutcome::ManuallyMatched(_)));
    assert_eq!(f.count("library_membership"), 3);
}
#[test]
fn independent_confirmation_protects_identity_after_clear() {
    for before in [false, true] {
        let mut f = Fixture::new();
        let r = f.lib.recording_for_track(&f.track()).unwrap().recording_id;
        if before {
            f.lib
                .attach_recording_external_identity(&r, &id("performance", "2"))
                .unwrap();
        }
        f.choose(true);
        if !before {
            f.lib
                .attach_recording_external_identity(&r, &id("performance", "2"))
                .unwrap();
        }
        assert_eq!(f.count("recording_manual_identity"), 0);
        let track = f.track();
        f.lib.clear_manual_track(&f.album, &track).unwrap();
        assert_eq!(
            f.lib.list_recording_external_identities(&r).unwrap(),
            vec![id("performance", "2")]
        );
    }
}
#[test]
fn manual_confirmation_removes_provenance_ownership() {
    let mut f = Fixture::new();
    let r = f.lib.recording_for_track(&f.track()).unwrap().recording_id;
    f.lib
        .attach_recording_external_identity(&r, &id("performance", "2"))
        .unwrap();
    f.db()
        .execute_batch(
            "INSERT INTO recording_provenance_identity SELECT * FROM recording_external_identity",
        )
        .unwrap();
    f.choose(true);
    assert_eq!(f.count("recording_provenance_identity"), 0);
    assert_eq!(f.count("recording_manual_identity"), 1);
    f.lib.reconcile_local_provenance(&f.album).unwrap();
    assert_eq!(f.count("recording_external_identity"), 1);
}
#[test]
fn multiple_manual_claims_keep_shared_recording_until_last_clear() {
    let mut f = Fixture::new();
    let tracks = f.lib.local_album_tracks(&f.album).unwrap();
    f.db()
        .execute(
            "UPDATE track SET recording_id=?1 WHERE id=?2",
            rusqlite::params![tracks[0].recording_id.as_ref(), tracks[1].track_id.as_ref()],
        )
        .unwrap();
    for t in &tracks[..2] {
        let s = f
            .lib
            .prepare_manual_track(&f.album, &t.track_id, &programs(true))
            .unwrap();
        f.lib.confirm_manual_track(&s, 1).unwrap();
    }
    f.lib
        .clear_manual_track(&f.album, &tracks[0].track_id)
        .unwrap();
    assert_eq!(f.count("recording_external_identity"), 1);
    f.lib
        .clear_manual_track(&f.album, &tracks[1].track_id)
        .unwrap();
    assert_eq!(f.count("recording_external_identity"), 0);
}
#[test]
fn conflicts_and_failed_writes_rollback_without_replacement() {
    let mut f = Fixture::new();
    let r = f.lib.recording_for_track(&f.track()).unwrap().recording_id;
    f.lib
        .attach_recording_external_identity(&r, &id("performance", "other"))
        .unwrap();
    let s = f
        .lib
        .prepare_manual_track(&f.album, &f.track(), &programs(true))
        .unwrap();
    assert!(
        f.lib
            .confirm_manual_track(&s, 1)
            .unwrap_err()
            .to_string()
            .contains("Conflicting")
    );
    assert_eq!(f.count("manual_track_association"), 0);
    let mut f = Fixture::new();
    let s = f
        .lib
        .prepare_manual_track(&f.album, &f.track(), &programs(true))
        .unwrap();
    f.db().execute_batch("CREATE TRIGGER fail_manual BEFORE INSERT ON manual_track_recording_claim BEGIN SELECT RAISE(ABORT,'induced'); END").unwrap();
    assert!(f.lib.confirm_manual_track(&s, 1).is_err());
    assert_eq!(f.count("manual_track_association"), 0);
    assert_eq!(f.count("recording_external_identity"), 0);
    assert_eq!(f.count("recording_manual_identity"), 0);
}
#[test]
fn retag_restart_and_provider_outage_preserve_choice() {
    let mut f = Fixture::new();
    let chosen = f.choose(true);
    std::fs::write(f.temp.path().join("music/2"), b"changed size retag").unwrap();
    f.lib.scan_local_root(&f.root, &mut Tags(true)).unwrap();
    let input = f
        .lib
        .prepare_album_program(&f.album, &id("album", "known"))
        .unwrap()
        .unwrap();
    assert!(matches!(
        f.lib
            .complete_album_program(Reply {
                input,
                result: Err(music_library::catalog::CatalogError::Timeout(
                    "offline".into()
                ))
            })
            .unwrap(),
        Outcome::Deferred(_)
    ));
    assert_eq!(
        f.lib.manual_track_associations(&f.album).unwrap(),
        vec![chosen.clone()]
    );
    drop(f.lib);
    f.lib = Library::open(f.temp.path().join("db")).unwrap();
    assert_eq!(
        f.lib.manual_track_associations(&f.album).unwrap(),
        vec![chosen]
    );
}
#[test]
fn chooser_requires_current_known_album_and_valid_explicit_selection() {
    let mut f = Fixture::new();
    let mut p = programs(true);
    p.album = id("album", "unrelated");
    assert!(
        f.lib
            .prepare_manual_track(&f.album, &f.track(), &p)
            .is_err()
    );
    assert!(
        f.lib
            .prepare_manual_track(&f.album, &TrackId("outside".into()), &programs(true))
            .is_err()
    );
    let s = f
        .lib
        .prepare_manual_track(&f.album, &f.track(), &programs(true))
        .unwrap();
    assert!(f.lib.confirm_manual_track(&s, usize::MAX).is_err());
    f.db()
        .execute("DELETE FROM album_external_identity", [])
        .unwrap();
    assert!(f.lib.confirm_manual_track(&s, 1).is_err());
    assert_eq!(f.count("manual_track_association"), 0);
}
#[test]
fn migration_and_reconstruction_use_targeted_indexes() {
    let f = Fixture::new();
    let db = f.db();
    db.execute_batch(include_str!("support/drop_manual_schema.sql"))
        .unwrap();
    db.execute_batch("PRAGMA user_version=10").unwrap();
    drop(db);
    let lib = Library::open(f.temp.path().join("db")).unwrap();
    assert!(lib.manual_track_associations(&f.album).unwrap().is_empty());
    let db = f.db();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i32>(0))
            .unwrap(),
        11
    );
    let plan:Vec<String>=db.prepare("EXPLAIN QUERY PLAN SELECT m.track_id FROM release r CROSS JOIN track t ON t.release_id=r.id JOIN manual_track_association m ON m.track_id=t.id WHERE r.album_id=?1").unwrap().query_map([f.album.as_ref()],|r|r.get(3)).unwrap().map(Result::unwrap).collect();
    assert!(plan.iter().any(|s| s.contains("release_album")));
    assert!(plan.iter().any(|s| s.contains("track_release_order")));
    assert!(plan.iter().any(|s| s.contains("SEARCH m")));
    assert_eq!(f.count("track"), 3);
}
