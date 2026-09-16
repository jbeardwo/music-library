use music_library::album_program::compare;
use music_library::{album_program::*, domain::*, edition::*};
fn id(kind: &str, value: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: "example".into(),
        kind: kind.into(),
        external_id: value.into(),
    }
}
fn entry(n: u32) -> TrackEvidence {
    TrackEvidence {
        disc: Some(1),
        number: Some(n),
        title: Some(format!("Song {n}")),
        duration_ms: Some(180_000),
        recording: RecordingEvidence {
            identities: vec![id("performance", &n.to_string())],
            isrcs: vec![],
        },
        identities: vec![id("song", &n.to_string())],
        ..Default::default()
    }
}
fn local(n: u32) -> LocalTrackEvidence {
    let mut evidence = entry(n);
    evidence.recording = Default::default();
    evidence.identities.clear();
    LocalTrackEvidence {
        track_id: TrackId(n.to_string()),
        recording_id: RecordingId(n.to_string()),
        evidence,
    }
}
fn programs() -> Programs {
    Programs {
        album: id("album", "A"),
        programs: vec![Program {
            identity: None,
            tracks: (1..=15).map(entry).collect(),
            complete: true,
        }],
        note: "synthetic provider, no edition hierarchy".into(),
    }
}
fn matched(outcome: TrackOutcome) -> Match {
    match outcome {
        TrackOutcome::Matched(m) | TrackOutcome::AlreadyMatched(m) => m,
        o => panic!("{o:?}"),
    }
}
#[test]
fn complete_partial_and_one_track_inputs_need_no_edition_or_completeness() {
    for n in [1, 2, 5, 10, 15] {
        assert_eq!(
            matched(compare(&local(n), &programs())).title,
            format!("Song {n}")
        );
    }
}
#[test]
fn multiple_programs_must_agree_and_order_cannot_choose_a_version() {
    let mut p = programs();
    p.programs.push(p.programs[0].clone());
    assert_eq!(
        matched(compare(&local(5), &p)).recording.identities,
        vec![id("performance", "5")]
    );
    p.programs[1].tracks[4].recording.identities = vec![id("performance", "alternate")];
    assert_eq!(
        matched(compare(&local(5), &p)).recording_status,
        RecordingStatus::Ambiguous
    );
    p.programs.reverse();
    assert_eq!(
        matched(compare(&local(5), &p)).recording_status,
        RecordingStatus::Ambiguous
    );
}
#[test]
fn occurrence_only_provider_is_useful_without_inventing_a_recording() {
    let mut p = programs();
    for t in &mut p.programs[0].tracks {
        t.recording = Default::default();
    }
    let m = matched(compare(&local(5), &p));
    assert!(m.recording.identities.is_empty());
    assert_eq!(m.recording_status, RecordingStatus::NotProvided);
    assert_eq!(m.occurrences, vec![id("song", "5")]);
}
#[test]
fn title_tolerance_is_album_scoped_and_version_qualifiers_remain() {
    let mut l = local(5);
    let mut p = programs();
    p.programs[0].tracks[4].title = Some("Feel Good Inc.".into());
    l.evidence.title = Some("  FEEL  GOOD Inc ".into());
    matched(compare(&l, &p));
    l.evidence.title = Some("Feel Good Incc".into());
    matched(compare(&l, &p));
    for qualifier in ["live", "remix", "demo", "acoustic", "edit", "remastered"] {
        l.evidence.title = Some(format!("Feel Good Inc {qualifier}"));
        assert_eq!(compare(&l, &p), TrackOutcome::NoConfidentMatch);
    }
}
#[test]
fn identity_outranks_duration_but_isrc_never_overrides_conflict() {
    let mut l = local(5);
    let mut p = programs();
    l.evidence.duration_ms = Some(900_000);
    assert_eq!(
        matched(compare(&l, &p)).recording_status,
        RecordingStatus::Identified
    );
    l.evidence.recording.identities = vec![id("performance", "5")];
    matched(compare(&l, &p));
    l.evidence.duration_ms = Some(180_000);
    l.evidence.recording.isrcs = vec!["CODE".into()];
    p.programs[0].tracks[4].recording.isrcs = vec!["CODE".into()];
    l.evidence.recording.identities = vec![id("performance", "other")];
    assert_eq!(compare(&l, &p), TrackOutcome::ConflictingIdentity);
}
#[test]
fn multidisc_and_flat_programs_align_partial_titles_unambiguously() {
    let mut p = programs();
    for t in p.programs[0].tracks.iter_mut().skip(10) {
        t.disc = Some(2);
        t.number = Some(t.number.unwrap() - 10);
    }
    matched(compare(&local(12), &p));
    let mut l = local(12);
    l.evidence.disc = Some(2);
    l.evidence.number = Some(2);
    matched(compare(&l, &programs()));
    p.programs[0].complete = false;
    assert_eq!(compare(&l, &p), TrackOutcome::NoConfidentMatch);
}

#[test]
fn standalone_conjunction_is_comparison_only() {
    let mut l = local(1);
    l.evidence.title = Some("Bride and Groom".into());
    let mut p = programs();
    p.programs[0].tracks[0].title = Some("Bride & Groom".into());
    let before = (l.clone(), p.clone());
    assert_eq!(matched(compare(&l, &p)).title, "Bride & Groom");
    assert_eq!((l, p), before);
    assert_eq!(comparison_title("Sand and Candy"), "sand and candy");
    assert_ne!(comparison_title("Sand"), comparison_title("S&"));
    assert_ne!(comparison_title("Super Fx"), comparison_title("Super Fxx"));
}

#[test]
fn rich_kid_unique_exact_title_accepts_recording_with_diagnostic_duration_mismatch() {
    let mut l = local(3);
    l.evidence.title = Some("Rich Kid".into());
    l.evidence.duration_ms = Some(276_898);
    let mut p = programs();
    let c = &mut p.programs[0].tracks[2];
    c.title = Some("Rich Kid".into());
    c.duration_ms = Some(280_000);
    let check = inspect_candidate(&l, c, 2);
    assert!(check.exact_title && check.position && check.duration_mismatch && check.considered);
    assert!(check.reason.contains("duration diagnostic"));
    let m = matched(compare(&l, &p));
    assert_eq!(m.title, "Rich Kid");
    assert_eq!(m.recording_status, RecordingStatus::Identified);
    assert_eq!(m.recording.identities, vec![id("performance", "3")]);
    assert!(m.explanation.contains(
        "duration mismatch (diagnostic for unique exact title or trusted identity): true"
    ));
    l.evidence.duration_ms = Some(277_000);
    assert_eq!(
        matched(compare(&l, &p)).recording_status,
        RecordingStatus::Identified
    );
    l.evidence.duration_ms = Some(276_898);
    l.evidence.number = None;
    assert_eq!(
        matched(compare(&l, &p)).recording_status,
        RecordingStatus::Identified
    );
    l.evidence.duration_ms = Some(900_000);
    assert_eq!(
        matched(compare(&l, &p)).recording_status,
        RecordingStatus::Identified
    );
    l.evidence.title = Some("Rich Kidd".into());
    l.evidence.number = Some(3);
    assert_eq!(compare(&l, &p), TrackOutcome::NoConfidentMatch);
    l.evidence.duration_ms = Some(280_000);
    assert_eq!(
        matched(compare(&l, &p)).recording_status,
        RecordingStatus::Identified
    );
}

#[test]
fn duplicate_exact_titles_do_not_gain_the_unique_title_duration_exception() {
    let mut l = local(3);
    l.evidence.duration_ms = Some(900_000);
    let mut p = programs();
    let duplicate = p.programs[0].tracks[2].clone();
    p.programs[0].tracks.push(duplicate);
    assert_eq!(compare(&l, &p), TrackOutcome::Ambiguous);
    p.programs[0].tracks.last_mut().unwrap().number = Some(16);
    p.programs[0]
        .tracks
        .last_mut()
        .unwrap()
        .recording
        .identities = vec![id("performance", "alternate")];
    assert_eq!(
        matched(compare(&l, &p)).recording_status,
        RecordingStatus::DurationMismatch
    );
    for track in &mut p.programs[0].tracks {
        track.recording.identities.clear();
    }
    assert_eq!(
        matched(compare(&l, &p)).recording_status,
        RecordingStatus::NotProvided,
        "an occurrence-only provider's successful positional Track association has no Recording claim to withhold"
    );
}

fn variant_fixture() -> (Vec<LocalTrackEvidence>, Programs) {
    let mut local: Vec<_> = (1..=12).map(local).collect();
    local[11].evidence.title = Some("Super Fx".into());
    let mut p = programs();
    p.programs[0].tracks.truncate(12);
    p.programs[0].tracks[11].title = Some("Super Fx".into());
    let mut alternate = p.programs[0].clone();
    alternate.tracks[11].title = Some("Super Fxx".into());
    alternate.tracks[11].recording.identities = vec![id("performance", "alternate")];
    p.programs.push(alternate);
    (local, p)
}

#[test]
fn superior_whole_program_is_a_template_not_an_edition_claim() {
    let (local, mut p) = variant_fixture();
    assert_eq!(selected_programs(&local, &p), vec![0]);
    let m = matched(compare_album(&local, &p).remove(11));
    assert_eq!(m.title, "Super Fx");
    assert_eq!(m.recording.identities, vec![id("performance", "12")]);
    p.programs.reverse();
    assert_eq!(selected_programs(&local, &p), vec![1]);
    assert_eq!(
        matched(compare_album(&local, &p).remove(11)).recording,
        m.recording
    );
}

#[test]
fn tied_templates_keep_track_association_but_withhold_disputed_recording() {
    let (local, mut p) = variant_fixture();
    p.programs[1].tracks[11].title = Some("Super Fx".into());
    assert_eq!(selected_programs(&local, &p), vec![0, 1]);
    let m = matched(compare_album(&local, &p).remove(11));
    assert_eq!(m.title, "Super Fx");
    assert_eq!(m.recording_status, RecordingStatus::Ambiguous);
    assert!(m.recording.identities.is_empty() && m.recording.isrcs.is_empty());
}

#[test]
fn partial_program_fit_uses_only_present_tracks_and_needs_corroboration() {
    let (all, p) = variant_fixture();
    let partial = vec![all[1].clone(), all[4].clone(), all[11].clone()];
    assert_eq!(selected_programs(&partial, &p), vec![0]);
    assert_eq!(program_fit(&partial, &p.programs[0]).unmatched, 0);
    assert_eq!(selected_programs(&[all[11].clone()], &p), vec![0, 1]);
    assert_eq!(
        matched(compare_album(&[all[11].clone()], &p).remove(0)).recording_status,
        RecordingStatus::Ambiguous
    );
    assert_eq!(
        selected_programs(&[all[1].clone(), all[4].clone(), all[9].clone()], &p),
        vec![0, 1]
    );
}

#[test]
fn incompatible_canonical_program_cannot_veto_a_confirming_program() {
    let (mut local, p) = variant_fixture();
    local[11].evidence.recording.identities = vec![id("performance", "12")];
    assert_eq!(selected_programs(&local, &p), vec![0]);
    assert!(matches!(
        compare_album(&local, &p).remove(11),
        TrackOutcome::AlreadyMatched(_)
    ));
    let mut only_conflict = p.clone();
    only_conflict.programs.remove(0);
    assert_eq!(
        compare_album(&local, &only_conflict).remove(11),
        TrackOutcome::ConflictingIdentity
    );
}

#[test]
fn ambiguous_occurrences_within_one_program_remain_track_ambiguous() {
    let mut p = programs();
    p.programs[0].tracks.push(entry(5));
    assert_eq!(compare(&local(5), &p), TrackOutcome::Ambiguous);
}

#[test]
fn conflicting_program_strengths_do_not_arbitrarily_choose_a_template() {
    let (local, mut p) = variant_fixture();
    p.programs[0].tracks[1].title = Some("Song 22".into());
    assert_eq!(selected_programs(&local, &p), vec![0, 1]);
}

#[test]
fn duplicate_local_positions_cannot_manufacture_three_track_corroboration() {
    let (local, p) = variant_fixture();
    assert_eq!(
        selected_programs(&vec![local[11].clone(); 3], &p),
        vec![0, 1]
    );
}

#[test]
fn different_fuzzy_titles_without_shared_identity_are_track_ambiguous() {
    let (local, mut p) = variant_fixture();
    p.programs[0].tracks[11].title = Some("Super Fxy".into());
    p.programs[1].tracks[11].identities.clear();
    assert_eq!(
        compare_album(&local, &p).remove(11),
        TrackOutcome::Ambiguous
    );
}

#[test]
fn recording_ambiguity_persists_no_claim_while_track_association_succeeds() {
    let (_temp, mut lib, imported, album) = fixture();
    let input = lib
        .prepare_album_program(&album, &id("album", "A"))
        .unwrap()
        .unwrap();
    let recording = input.tracks[0].recording_id.clone();
    let mut p = programs();
    let mut other = p.programs[0].clone();
    other.tracks[1].recording.identities = vec![id("performance", "different")];
    p.programs.push(other);
    let result = lib
        .complete_album_program(Reply {
            input,
            result: Ok(p),
        })
        .unwrap();
    let Outcome::Complete(rows) = result else {
        panic!("expected Track results")
    };
    assert_eq!(
        matched(rows[0].1.clone()).recording_status,
        RecordingStatus::Ambiguous
    );
    assert!(
        lib.list_recording_external_identities(&recording)
            .unwrap()
            .is_empty()
    );
    assert!(
        lib.list_release_external_identities(&imported.release_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn preferred_program_persistence_never_attaches_edition_or_occurrence() {
    let (_temp, mut lib, imported, album) = fixture();
    let input = lib
        .prepare_album_program(&album, &id("album", "A"))
        .unwrap()
        .unwrap();
    let mut p = programs();
    p.programs[0].identity = Some(id("edition", "standard"));
    let mut other = p.programs[0].clone();
    other.identity = Some(id("edition", "alternate"));
    other.tracks[9].title = Some("Song 100".into());
    p.programs.push(other);
    assert_eq!(selected_programs(&input.tracks, &p), vec![0]);
    lib.complete_album_program(Reply {
        input,
        result: Ok(p),
    })
    .unwrap();
    assert!(
        lib.list_release_external_identities(&imported.release_id)
            .unwrap()
            .is_empty()
    );
    for t in imported.track_ids {
        assert!(lib.list_track_external_identities(&t).unwrap().is_empty());
    }
}

struct Tags;
impl music_library::filesystem::MetadataExtractor for Tags {
    fn supports(&self, _: &std::path::Path) -> bool {
        true
    }
    fn read(&mut self, p: &std::path::Path) -> music_library::Result<ObservedMetadata> {
        let n = p.file_name().unwrap().to_str().unwrap().parse().unwrap();
        Ok(ObservedMetadata {
            track_title: Some(format!("Song {n}")),
            release_title: Some("Album".into()),
            release_artists: vec!["Local Alias".into()],
            track_artists: vec!["Local Alias".into()],
            track_number: Some(n),
            disc_number: Some(1),
            duration_ms: Some(180_000),
            ..Default::default()
        })
    }
}
fn fixture() -> (
    tempfile::TempDir,
    music_library::Library,
    ImportedRelease,
    AlbumId,
) {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("music");
    std::fs::create_dir(&folder).unwrap();
    for n in [2, 5, 10] {
        std::fs::write(folder.join(n.to_string()), b"audio").unwrap();
    }
    let mut lib = music_library::Library::open(temp.path().join("db")).unwrap();
    let root = lib.register_local_root(folder).unwrap();
    lib.scan_local_root(&root, &mut Tags).unwrap();
    let tracks = lib
        .list_discovery_candidates(None, 100)
        .unwrap()
        .into_iter()
        .map(|c| ImportTrackInput {
            source_id: c.source_id,
            title_fallback: None,
            artists: vec![],
            disc_number: c.metadata.disc_number,
            track_number: c.metadata.track_number,
        })
        .collect();
    let imported = lib
        .import_release(&ImportReleaseRequest {
            release_title: "Album".into(),
            release_artists: vec![],
            tracks,
        })
        .unwrap();
    let album = lib
        .album_for_release(&imported.release_id)
        .unwrap()
        .album_id;
    lib.attach_album_external_identity(&album, &id("album", "A"))
        .unwrap();
    (temp, lib, imported, album)
}
#[test]
fn unique_exact_title_duration_exception_persists_recording_without_edition() {
    let (_temp, mut lib, imported, album) = fixture();
    let input = lib
        .prepare_album_program(&album, &id("album", "A"))
        .unwrap()
        .unwrap();
    let mut p = programs();
    for t in &mut p.programs[0].tracks {
        t.duration_ms = Some(900_000);
    }
    let Outcome::Complete(rows) = lib
        .complete_album_program(Reply {
            input,
            result: Ok(p),
        })
        .unwrap()
    else {
        panic!()
    };
    for (local, outcome) in rows {
        let m = matched(outcome);
        assert_eq!(m.recording_status, RecordingStatus::Identified);
        assert_eq!(
            lib.list_recording_external_identities(&local.recording_id)
                .unwrap(),
            m.recording.identities
        );
    }
    assert!(
        lib.list_release_external_identities(&imported.release_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn persist_only_recordings_confirm_ownership_and_preserve_local_state_across_restart() {
    let (temp, mut lib, imported, album) = fixture();
    let before = lib.edition_evidence(&imported.release_id).unwrap();
    let input = lib
        .prepare_album_program(&album, &id("album", "A"))
        .unwrap()
        .unwrap();
    let result = lib
        .complete_album_program(Reply {
            input,
            result: Ok(programs()),
        })
        .unwrap();
    assert!(matches!(result,Outcome::Complete(ref rows) if rows.len()==3));
    let db = rusqlite::Connection::open(temp.path().join("db")).unwrap();
    db.execute(
        "INSERT INTO recording_provenance_identity SELECT * FROM recording_external_identity",
        [],
    )
    .unwrap();
    let input = lib
        .prepare_album_program(&album, &id("album", "A"))
        .unwrap()
        .unwrap();
    let result = lib
        .complete_album_program(Reply {
            input,
            result: Ok(programs()),
        })
        .unwrap();
    assert!(
        matches!(result,Outcome::Complete(ref rows) if rows.iter().all(|(_,o)|matches!(o,TrackOutcome::AlreadyMatched(_))))
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM recording_provenance_identity",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        0
    );
    for table in ["release_external_identity", "track_external_identity"] {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, u32>(0))
                .unwrap(),
            0
        );
    }
    for table in ["track", "library_membership", "track_source"] {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, u32>(0))
                .unwrap(),
            3
        );
    }
    drop(lib);
    let lib = music_library::Library::open(temp.path().join("db")).unwrap();
    let after = lib.edition_evidence(&imported.release_id).unwrap();
    assert_eq!(before.completeness, Completeness::Unknown);
    assert_eq!(after.completeness, Completeness::Unknown);
    for (a, b) in before.tracks.iter().zip(after.tracks) {
        assert_eq!(a.track_id, b.track_id);
        assert_eq!(a.evidence.title, b.evidence.title);
        assert_eq!(b.evidence.recording.identities.len(), 1);
    }
}
#[test]
fn provider_disagreement_and_stale_reply_do_not_change_independent_identity() {
    let (_temp, mut lib, _imported, album) = fixture();
    let mut p = programs();
    let input = lib
        .prepare_album_program(&album, &id("album", "A"))
        .unwrap()
        .unwrap();
    let recording = input.tracks[0].recording_id.clone();
    lib.attach_recording_external_identity(&recording, &id("performance", "different"))
        .unwrap();
    assert!(matches!(
        lib.complete_album_program(Reply {
            input,
            result: Ok(p.clone())
        })
        .unwrap(),
        Outcome::Error(_)
    ));
    p.programs[0].tracks[1].recording.identities = vec![id("performance", "provider")];
    let input = lib
        .prepare_album_program(&album, &id("album", "A"))
        .unwrap()
        .unwrap();
    let result = lib
        .complete_album_program(Reply {
            input,
            result: Ok(p),
        })
        .unwrap();
    assert!(
        matches!(result,Outcome::Complete(ref rows) if rows.iter().any(|(_,o)|*o==TrackOutcome::ConflictingIdentity))
    );
    assert_eq!(
        lib.list_recording_external_identities(&recording).unwrap(),
        vec![id("performance", "different")]
    );
}

#[test]
fn established_artist_identity_supports_exact_local_alias_without_rewriting_credits() {
    let (temp, mut lib, _imported, album) = fixture();
    let db = rusqlite::Connection::open(temp.path().join("db")).unwrap();
    let artist = ArtistId(
        db.query_row(
            "SELECT artist_id FROM album_artist_credit WHERE album_id=?1",
            [album.as_ref()],
            |r| r.get(0),
        )
        .unwrap(),
    );
    lib.attach_artist_external_identity(&artist, &id("artist", "performer"))
        .unwrap();
    let input = lib
        .prepare_album_program(&album, &id("album", "A"))
        .unwrap()
        .unwrap();
    assert_eq!(input.tracks[0].evidence.artists[0].name, "Local Alias");
    let mut p = programs();
    for t in &mut p.programs[0].tracks {
        t.artists = vec![ArtistEvidence {
            identities: vec![id("artist", "performer")],
            name: "Canonical name".into(),
            join_phrase: String::new(),
        }];
    }
    assert!(
        matches!(lib.complete_album_program(Reply{input,result:Ok(p)}).unwrap(),Outcome::Complete(ref rows) if rows.iter().all(|(_,o)|matches!(o,TrackOutcome::Matched(_))))
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM track_artist_credit", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row(
            "SELECT name FROM artist WHERE id=?1",
            [artist.as_ref()],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "Local Alias"
    );
}

#[test]
fn contradictory_source_artist_credits_do_not_become_missing_positive_evidence() {
    let (temp, mut lib, _imported, album) = fixture();
    let input = lib
        .prepare_album_program(&album, &id("album", "A"))
        .unwrap()
        .unwrap();
    let target = &input.tracks[0].track_id;
    let other = &input.tracks[1].track_id;
    let db = rusqlite::Connection::open(temp.path().join("db")).unwrap();
    db.execute("UPDATE file_artist_observation SET name='Different Artist' WHERE scope='track' AND source_id IN (SELECT source_id FROM track_source WHERE track_id=?1)",[other.as_ref()]).unwrap();
    db.execute(
        "UPDATE track_source SET track_id=?1 WHERE track_id=?2",
        rusqlite::params![target.as_ref(), other.as_ref()],
    )
    .unwrap();
    let input = lib
        .prepare_album_program(&album, &id("album", "A"))
        .unwrap()
        .unwrap();
    assert!(input.artist_conflicts.contains(target));
    let result = lib
        .complete_album_program(Reply {
            input,
            result: Ok(programs()),
        })
        .unwrap();
    assert!(
        matches!(result,Outcome::Complete(ref rows) if rows.iter().any(|(t,o)|t.track_id==*target && *o==TrackOutcome::NoConfidentMatch))
    );
}
#[test]
#[ignore = "local program comparison timings; no network"]
fn comparison_timings() {
    for count in [5, 15, 30] {
        let p = Programs {
            album: id("album", "A"),
            programs: vec![
                Program {
                    identity: None,
                    tracks: (1..=count).map(entry).collect(),
                    complete: true
                };
                3
            ],
            note: String::new(),
        };
        let tracks: Vec<_> = (1..=count).map(local).collect();
        let start = std::time::Instant::now();
        for _ in 0..1000 {
            std::hint::black_box(compare_album(&tracks, &p));
        }
        println!(
            "{count} Tracks / 3 programs: {:?} per Album",
            start.elapsed() / 1000
        );
    }
}

#[test]
fn one_owned_worker_defers_programs_and_manual_probe_resumes_without_artist_searches() {
    use music_library::{
        album_matching::{AlbumMatcher, AutoMatchPolicy, CircuitState},
        catalog::*,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };
    struct Provider(Arc<AtomicUsize>);
    impl CatalogProvider for Provider {
        fn album_program_namespaces(&self) -> Vec<(String, String)> {
            vec![("example".into(), "album".into())]
        }
        fn album_programs(&mut self, _: &ExternalIdentity) -> Result<Programs, CatalogError> {
            if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(CatalogError::Timeout("outage".into()))
            } else {
                Ok(programs())
            }
        }
        fn search_albums(&mut self, _: &str, _: u32) -> Result<Page<AlbumCandidate>, CatalogError> {
            panic!("known Album must not search")
        }
        fn releases(
            &mut self,
            _: &ExternalIdentity,
            _: u32,
        ) -> Result<Page<ReleaseCandidate>, CatalogError> {
            panic!("no edition concept")
        }
        fn release(&mut self, _: &ExternalIdentity) -> Result<Release, CatalogError> {
            panic!("no edition concept")
        }
    }
    let (_temp, mut lib, imported, album) = fixture();
    let calls = Arc::new(AtomicUsize::new(0));
    let (send, recv) = mpsc::channel();
    let mut matcher = AlbumMatcher::new_with_programs(
        Provider(calls.clone()),
        |_| panic!("no Artist discovery"),
        move |r| send.send(r).unwrap(),
        |_| {},
    )
    .unwrap();
    matcher
        .after_import(
            &lib,
            std::slice::from_ref(&imported),
            AutoMatchPolicy { enabled: false },
        )
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    matcher
        .after_import(
            &lib,
            std::slice::from_ref(&imported),
            AutoMatchPolicy::default(),
        )
        .unwrap();
    let reply = recv
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    assert!(matches!(
        matcher.complete_programs(&mut lib, reply),
        Outcome::Deferred(_)
    ));
    assert!(matches!(
        matcher.circuit_state(),
        CircuitState::Unavailable(_)
    ));
    assert_eq!(matcher.pending_count(), 1);
    matcher
        .after_import(&lib, &[imported], AutoMatchPolicy::default())
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let token = matcher.retry_schedule().unwrap().token;
    matcher.cooldown_elapsed(&lib, token).unwrap(); // injectable expiry, no 15-second sleep
    matcher.retry_catalog_matching(&lib).unwrap(); // coalesces with the one active probe
    let reply = recv
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    assert!(
        matches!(matcher.complete_programs(&mut lib,reply),Outcome::Complete(ref rows) if rows.len()==3)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    matcher.cooldown_elapsed(&lib, token).unwrap(); // obsolete expiry cannot duplicate recovery
    assert_eq!(matcher.pending_count(), 0);
    assert!(matches!(matcher.circuit_state(), CircuitState::Available));
    assert_eq!(
        lib.list_album_external_identities(&album).unwrap(),
        vec![id("album", "A")]
    );
}
