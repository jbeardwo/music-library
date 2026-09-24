use music_library::{
    album_candidates::*,
    album_matching::MatchOutcome,
    album_program::{Program, Programs, TrackOutcome},
    catalog::*,
    domain::*,
    edition::*,
};
use std::collections::HashMap;

#[test]
#[ignore = "local candidate comparison timing, no network"]
fn candidate_fit_timing() {
    for count in [5, 15, 30] {
        let local = locals(&(1..=count).collect::<Vec<_>>());
        let mut p = program("a");
        p.programs[0].tracks = local.iter().map(|t| t.evidence.clone()).collect();
        let start = std::time::Instant::now();
        for _ in 0..1000 {
            for _ in 0..3 {
                std::hint::black_box(fit(&local, &p));
            }
        }
        println!(
            "{count} present Tracks / three candidate programs: {:?}",
            start.elapsed() / 1000
        );
    }
}
fn id(kind: &str, value: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: "plain-catalog".into(),
        kind: kind.into(),
        external_id: value.into(),
    }
}
fn candidate(value: &str) -> ArtistAlbumCandidate {
    ArtistAlbumCandidate {
        identity: id("album", value),
        title: "Album".into(),
        artist: "Artist".into(),
        artist_ids: vec![id("artist", "artist")],
        primary_type: String::new(),
        date: String::new(),
        comment: String::new(),
    }
}
fn program(value: &str) -> Programs {
    Programs {
        album: id("album", value),
        programs: vec![Program {
            identity: None,
            complete: true,
            tracks: (1..=12)
                .map(|n| TrackEvidence {
                    title: Some(format!("Song {n}")),
                    disc: Some(1),
                    number: Some(n),
                    duration_ms: Some(200000),
                    identities: vec![id("song", &format!("song{n}"))],
                    ..Default::default()
                })
                .collect(),
        }],
        note: String::new(),
    }
}
fn locals(numbers: &[u32]) -> Vec<LocalTrackEvidence> {
    numbers
        .iter()
        .map(|n| LocalTrackEvidence {
            track_id: TrackId(format!("t{n}")),
            recording_id: RecordingId(format!("r{n}")),
            evidence: TrackEvidence {
                title: Some(format!("Song {n}")),
                disc: Some(1),
                number: Some(*n),
                duration_ms: Some(200000),
                ..Default::default()
            },
        })
        .collect()
}
struct Provider {
    counts: HashMap<String, u32>,
    programs: HashMap<String, Programs>,
    calls: Vec<String>,
}
impl Provider {
    fn new() -> Self {
        Self {
            counts: HashMap::new(),
            programs: [("a".into(), program("a")), ("b".into(), program("b"))].into(),
            calls: vec![],
        }
    }
}
impl CatalogProvider for Provider {
    fn album_candidate_programs(&self) -> bool {
        true
    }
    fn album_candidate_track_count(&self, i: &ExternalIdentity) -> Option<u32> {
        self.counts.get(&i.external_id).copied()
    }
    fn album_programs(&mut self, i: &ExternalIdentity) -> Result<Programs, CatalogError> {
        self.calls.push(i.external_id.clone());
        Ok(self.programs[&i.external_id].clone())
    }
    fn search_albums(&mut self, _: &str, _: u32) -> Result<Page<AlbumCandidate>, CatalogError> {
        panic!("no search")
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
fn run(p: &mut Provider, local: &[LocalTrackEvidence], order: &[&str]) -> MatchOutcome {
    resolve(
        p,
        "Album",
        &id("artist", "artist"),
        &Page {
            items: order.iter().map(|i| candidate(i)).collect(),
            next_offset: None,
        },
        false,
        local,
        &mut vec![],
    )
    .unwrap()
}
#[test]
fn cheap_impossibility_filters_single_and_respects_partial_positions() {
    let mut p = Provider::new();
    p.counts = [("a".into(), 12), ("b".into(), 1)].into();
    assert_eq!(
        run(&mut p, &locals(&(1..=12).collect::<Vec<_>>()), &["b", "a"]),
        MatchOutcome::Matched(id("album", "a"))
    );
    assert!(p.calls.is_empty());
    assert_eq!(
        run(&mut p, &locals(&[2, 6, 9]), &["b", "a"]),
        MatchOutcome::Matched(id("album", "a"))
    );
    p.counts.insert("a".into(), 8);
    assert_eq!(
        run(&mut p, &locals(&[9]), &["a"]),
        MatchOutcome::NoConfidentMatch
    );
    assert_eq!(
        required_tracks(&[locals(&[2])[0].clone(), locals(&[2])[0].clone()]),
        2
    );
}
#[test]
fn programs_resolve_only_clear_incompatibility_and_reuse_cache() {
    let local = locals(&[2, 6, 9]);
    let mut p = Provider::new();
    for n in [1, 5] {
        p.programs.get_mut("b").unwrap().programs[0].tracks[n].title =
            Some("Different content".into());
    }
    let mut cache = vec![];
    let page = Page {
        items: vec![candidate("b"), candidate("a")],
        next_offset: None,
    };
    assert_eq!(
        resolve(
            &mut p,
            "Album",
            &id("artist", "artist"),
            &page,
            false,
            &local,
            &mut cache
        )
        .unwrap(),
        MatchOutcome::Matched(id("album", "a"))
    );
    assert_eq!(p.calls, vec!["b", "a"]);
    assert_eq!(
        resolve(
            &mut p,
            "Album",
            &id("artist", "artist"),
            &page,
            false,
            &local,
            &mut cache
        )
        .unwrap(),
        MatchOutcome::Matched(id("album", "a"))
    );
    assert_eq!(p.calls.len(), 2);
    assert_eq!(
        run(&mut p, &local, &["a", "b"]),
        MatchOutcome::Matched(id("album", "a"))
    );
    p.programs.get_mut("b").unwrap().programs[0].tracks[5].title = Some("Song 6".into());
    assert!(
        matches!(
            run(&mut p, &local, &["a", "b"]),
            MatchOutcome::AlbumEquivalent { candidates, .. } if candidates.len() == 2
        ),
        "one mismatch retains both Albums while other Tracks can agree"
    );
}
#[test]
fn equivalent_representations_preserve_track_association_without_inventing_identity() {
    let mut p = Provider::new();
    let local = locals(&[2, 6, 9]);
    for different_ids in [false, true] {
        if different_ids {
            for t in &mut p.programs.get_mut("b").unwrap().programs[0].tracks {
                t.identities = vec![id("song", &format!("alternate-{}", t.number.unwrap()))];
            }
        }
        for order in [["a", "b"], ["b", "a"]] {
            let MatchOutcome::AlbumEquivalent { candidates, tracks } = run(&mut p, &local, &order)
            else {
                panic!("must not arbitrarily select one Album ID")
            };
            assert_eq!(candidates.len(), 2);
            assert_eq!(tracks.len(), 3);
            for (_, o) in tracks {
                let TrackOutcome::Matched(m) = o else {
                    panic!()
                };
                assert!(m.recording.identities.is_empty());
                assert_eq!(m.occurrences.is_empty(), different_ids);
            }
        }
    }
}
#[test]
fn bounds_incomplete_results_and_strong_contradictions_are_conservative() {
    let mut p = Provider::new();
    let mut local = locals(&[2, 6, 9]);
    assert!(matches!(
        run(&mut p, &local, &["a", "b", "c", "d"]),
        MatchOutcome::AlbumAmbiguous(_)
    ));
    assert!(p.calls.is_empty());
    p.programs.get_mut("b").unwrap().programs[0].complete = false;
    assert!(matches!(
        run(&mut p, &local, &["a", "b"]),
        MatchOutcome::AlbumAmbiguous(_)
    ));
    p.programs.get_mut("b").unwrap().programs[0].complete = true;
    local[0].evidence.recording.identities = vec![id("performance", "known")];
    p.programs.get_mut("b").unwrap().programs[0].tracks[1]
        .recording
        .identities = vec![id("performance", "different")];
    assert_eq!(
        fit(&local, &p.programs["b"]),
        Fit::Rejected("conflicting trusted Recording identity")
    );
    assert_eq!(
        run(&mut p, &local, &["a", "b"]),
        MatchOutcome::Matched(id("album", "a"))
    );
}

fn catalog_fixture(path: &std::path::Path) -> (music_library::Library, ImportedRelease, AlbumId) {
    let mut lib = music_library::Library::open(path).unwrap();
    let imported = lib
        .create_catalog_release(&CatalogReleaseInput {
            title: "Album".into(),
            year: None,
            artists: vec![ArtistCreditInput {
                name: "Artist".into(),
                role: None,
            }],
            tracks: [2, 6, 9]
                .into_iter()
                .map(|n| CatalogTrackInput {
                    title: format!("Song {n}"),
                    artists: vec![],
                    disc_number: Some(1),
                    track_number: Some(n),
                })
                .collect(),
        })
        .unwrap();
    let album = lib
        .album_for_release(&imported.release_id)
        .unwrap()
        .album_id;
    (lib, imported, album)
}
fn persist_candidates(
    lib: &mut music_library::Library,
    album: &AlbumId,
    p: &mut Provider,
    order: &[&str],
) -> MatchOutcome {
    let scope = MatchingScope {
        provider: "plain-catalog".into(),
        artist_kind: "artist".into(),
        album_kind: "album".into(),
    };
    let music_library::album_matching::Preparation::Ready(input) =
        lib.prepare_album_match_for(album, &scope).unwrap()
    else {
        panic!("catalog-only Album must be eligible")
    };
    let local = lib.local_album_tracks(album).unwrap();
    let outcome = run(p, &local, order);
    lib.complete_album_match_for(
        music_library::album_matching::MatchReply {
            input,
            artist: Some(id("artist", "artist")),
            outcome,
            matched_album: None,
        },
        &scope,
    )
    .unwrap()
}

#[test]
fn agreed_occurrences_persist_independently_for_two_and_three_partial_catalog_programs() {
    for count in [2, 3] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("db");
        let (mut lib, imported, album) = catalog_fixture(&path);
        let mut p = Provider::new();
        p.programs.insert("c".into(), program("c"));
        let order = &["a", "b", "c"][..count];
        assert!(matches!(
            persist_candidates(&mut lib, &album, &mut p, order),
            MatchOutcome::AlbumEquivalent { .. }
        ));
        assert!(
            lib.list_album_external_identities(&album)
                .unwrap()
                .is_empty()
        );
        assert!(
            lib.list_release_external_identities(&imported.release_id)
                .unwrap()
                .is_empty()
        );
        let tracks = lib.local_album_tracks(&album).unwrap();
        assert_eq!(tracks.len(), 3); // no absent Tracks created, no completeness requirement
        for track in &tracks {
            assert!(
                lib.list_recording_external_identities(&track.recording_id)
                    .unwrap()
                    .is_empty()
            );
        }
        assert_eq!(p.calls.len(), count);
        drop(lib);
        let lib = music_library::Library::open(&path).unwrap();
        assert_eq!(
            lib.provider_track_associations(&album, "plain-catalog")
                .unwrap()
                .len(),
            3
        );
        // Playback resolver has no CatalogProvider; repeated lookups cannot search.
        for _ in 0..3 {
            for (track, n) in imported.track_ids.iter().zip([2, 6, 9]) {
                let expected = id("song", &format!("song{n}"));
                assert_eq!(
                    lib.track_provider_occurrences(track, "plain-catalog")
                        .unwrap(),
                    vec![expected.clone()]
                );
                assert_eq!(
                    lib.playback_route(
                        track,
                        &music_library::playback_resolver::RemoteCapability {
                            provider: "plain-catalog",
                            unavailable: None,
                            catalog_available: false,
                            accepts: |i| i.kind == "song"
                        }
                    )
                    .unwrap(),
                    music_library::playback_resolver::Route::Remote(expected)
                );
            }
        }
    }
}

#[test]
fn three_programs_agree_per_track_and_disagreement_or_missing_mapping_veto_only_that_track() {
    for missing in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (mut lib, imported, album) = catalog_fixture(&temp.path().join("db"));
        let mut p = Provider::new();
        let mut c = program("c");
        c.programs[0].tracks[5].identities = vec![id("song", "different-six")];
        if missing {
            c.programs[0].tracks[8].title = Some("Different title".into());
        } else {
            c.programs[0].tracks[8].identities = vec![id("song", "different-nine")];
        }
        p.programs.insert("c".into(), c);
        let outcome = persist_candidates(&mut lib, &album, &mut p, &["a", "b", "c"]);
        assert!(
            matches!(outcome,MatchOutcome::AlbumEquivalent { candidates,.. } if candidates.len()==3)
        );
        assert_eq!(
            lib.track_provider_occurrences(&imported.track_ids[0], "plain-catalog")
                .unwrap(),
            vec![id("song", "song2")]
        );
        for track in &imported.track_ids[1..] {
            assert!(
                lib.track_provider_occurrences(track, "plain-catalog")
                    .unwrap()
                    .is_empty()
            );
        }
        assert!(
            lib.list_album_external_identities(&album)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn structurally_rejected_program_does_not_veto_agreed_track_ids() {
    let temp = tempfile::tempdir().unwrap();
    let (mut lib, _, album) = catalog_fixture(&temp.path().join("db"));
    let mut p = Provider::new();
    let mut c = program("c");
    for t in &mut c.programs[0].tracks {
        t.identities = vec![id("song", "wrong")];
        t.title = Some("Unrelated song".into());
    }
    p.programs.insert("c".into(), c);
    assert!(
        matches!(persist_candidates(&mut lib,&album,&mut p,&["a","c","b"]),MatchOutcome::AlbumEquivalent {candidates,..} if candidates.len()==2)
    );
    assert_eq!(
        lib.provider_track_associations(&album, "plain-catalog")
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn manual_and_independent_existing_track_choices_remain_authoritative() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let (mut lib, imported, album) = catalog_fixture(&path);
    let db = rusqlite::Connection::open(&path).unwrap();
    let chosen = music_library::manual_track::Candidate {
        evidence: TrackEvidence {
            title: Some("Manual".into()),
            identities: vec![id("song", "manual")],
            ..Default::default()
        },
        supporting_programs: 1,
    };
    db.execute("INSERT INTO manual_track_association(track_id,album_provider,album_kind,album_external_id,candidate_json) VALUES(?1,'plain-catalog','album','manual-context',?2)",rusqlite::params![imported.track_ids[0].as_ref(),serde_json::to_string(&chosen).unwrap()]).unwrap();
    db.execute("INSERT INTO track_external_identity(track_id,provider,kind,external_id) VALUES(?1,'plain-catalog','song','existing')",[imported.track_ids[1].as_ref()]).unwrap();
    let outcome = persist_candidates(&mut lib, &album, &mut Provider::new(), &["a", "b"]);
    assert!(
        matches!(outcome,MatchOutcome::AlbumEquivalent {tracks,..} if matches!(tracks[0].1,TrackOutcome::ManuallyMatched(_)))
    );
    assert_eq!(
        lib.track_provider_occurrences(&imported.track_ids[0], "plain-catalog")
            .unwrap(),
        vec![id("song", "manual")]
    );
    assert_eq!(
        lib.track_provider_occurrences(&imported.track_ids[1], "plain-catalog")
            .unwrap(),
        vec![id("song", "existing")]
    );
    assert_eq!(
        lib.track_provider_occurrences(&imported.track_ids[2], "plain-catalog")
            .unwrap(),
        vec![id("song", "song9")]
    );
}

#[test]
fn stale_catalog_track_evidence_rejects_independent_persistence() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let (mut lib, _, album) = catalog_fixture(&path);
    let scope = MatchingScope {
        provider: "plain-catalog".into(),
        artist_kind: "artist".into(),
        album_kind: "album".into(),
    };
    let music_library::album_matching::Preparation::Ready(input) =
        lib.prepare_album_match_for(&album, &scope).unwrap()
    else {
        panic!()
    };
    let local = lib.local_album_tracks(&album).unwrap();
    let outcome = run(&mut Provider::new(), &local, &["a", "b"]);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute("UPDATE effective_track_metadata SET title='Changed'", [])
        .unwrap();
    assert_eq!(
        lib.complete_album_match_for(
            music_library::album_matching::MatchReply {
                input,
                artist: Some(id("artist", "artist")),
                outcome,
                matched_album: None
            },
            &scope
        )
        .unwrap(),
        MatchOutcome::Skipped
    );
    assert!(
        lib.provider_track_associations(&album, "plain-catalog")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn all_provider_track_ids_disagree_so_none_are_persisted() {
    let temp = tempfile::tempdir().unwrap();
    let (mut lib, imported, album) = catalog_fixture(&temp.path().join("db"));
    let mut p = Provider::new();
    for t in &mut p.programs.get_mut("b").unwrap().programs[0].tracks {
        t.identities = vec![id("song", &format!("other-{}", t.number.unwrap()))];
    }
    assert!(matches!(
        persist_candidates(&mut lib, &album, &mut p, &["a", "b"]),
        MatchOutcome::AlbumEquivalent { .. }
    ));
    for track in imported.track_ids {
        assert!(
            lib.track_provider_occurrences(&track, "plain-catalog")
                .unwrap()
                .is_empty()
        );
    }
    assert!(
        lib.list_album_external_identities(&album)
            .unwrap()
            .is_empty()
    );
}

#[test]
#[ignore = "local independent occurrence persistence timing, no network"]
fn independent_occurrence_persistence_timing() {
    let mut samples = vec![];
    let mut comparison = vec![];
    for _ in 0..20 {
        let temp = tempfile::tempdir().unwrap();
        let (mut lib, _, album) = catalog_fixture(&temp.path().join("db"));
        let scope = MatchingScope {
            provider: "plain-catalog".into(),
            artist_kind: "artist".into(),
            album_kind: "album".into(),
        };
        let music_library::album_matching::Preparation::Ready(input) =
            lib.prepare_album_match_for(&album, &scope).unwrap()
        else {
            panic!()
        };
        let local = lib.local_album_tracks(&album).unwrap();
        let mut p = Provider::new();
        p.programs.insert("c".into(), program("c"));
        let started = std::time::Instant::now();
        let outcome = run(&mut p, &local, &["a", "b", "c"]);
        comparison.push(started.elapsed());
        let started = std::time::Instant::now();
        lib.complete_album_match_for(
            music_library::album_matching::MatchReply {
                input,
                artist: Some(id("artist", "artist")),
                outcome,
                matched_album: None,
            },
            &scope,
        )
        .unwrap();
        samples.push(started.elapsed());
    }
    comparison.sort();
    samples.sort();
    println!(
        "Three plausible programs / three present Tracks: comparison median {:?}; independent persistence (Artist + 3 Track IDs, snapshot checks and commit) median {:?}",
        comparison[10], samples[10]
    );
}

#[derive(serde::Deserialize)]
struct DemonFixture {
    artist: String,
    title: String,
    year: u32,
    tracks: Vec<TrackEvidence>,
    programs: Vec<DemonProgram>,
}
#[derive(serde::Deserialize)]
struct DemonProgram {
    album: ExternalIdentity,
    date: String,
    tracks: Vec<TrackEvidence>,
}
#[test]
fn demon_days_live_credit_fixture_correlates_both_objects_without_arbitrary_ids() {
    let f: DemonFixture =
        serde_json::from_str(include_str!("fixtures/spotify/demon-days-credits.json")).unwrap();
    assert_eq!(f.year, 2005);
    let local: Vec<_> = f
        .tracks
        .iter()
        .enumerate()
        .map(|(i, evidence)| LocalTrackEvidence {
            track_id: TrackId(format!("t{i}")),
            recording_id: RecordingId(format!("r{i}")),
            evidence: evidence.clone(),
        })
        .collect();
    let artist = f.programs[0].tracks[0].artists[0].identities[0].clone();
    let candidates = Page {
        items: f
            .programs
            .iter()
            .map(|p| ArtistAlbumCandidate {
                identity: p.album.clone(),
                title: f.title.clone(),
                artist: f.artist.clone(),
                artist_ids: vec![artist.clone()],
                primary_type: "Album".into(),
                date: p.date.clone(),
                comment: String::new(),
            })
            .collect(),
        next_offset: None,
    };
    let mut provider = Provider::new();
    provider.programs = f
        .programs
        .iter()
        .map(|p| {
            (
                p.album.external_id.clone(),
                Programs {
                    album: p.album.clone(),
                    programs: vec![Program {
                        identity: None,
                        complete: true,
                        tracks: p.tracks.clone(),
                    }],
                    note: String::new(),
                },
            )
        })
        .collect();
    for p in provider.programs.values() {
        assert_eq!(fit(&local, p), Fit::Supported);
        let program_fit = music_library::album_program::program_fit(&local, &p.programs[0]);
        assert_eq!(program_fit.exact_positions, 15);
        assert_eq!(program_fit.unmatched, 0);
        assert_eq!(program_fit.duration_mismatches, 0);
        for (i, t) in local.iter().enumerate() {
            let check =
                music_library::album_program::inspect_candidate(t, &p.programs[0].tracks[i], i);
            assert!(
                check.considered && check.exact_title && !check.duration_mismatch,
                "position {}: {check:?}",
                i + 1
            );
        }
    }
    let outcome = resolve(
        &mut provider,
        &f.title,
        &artist,
        &candidates,
        false,
        &local,
        &mut vec![],
    )
    .unwrap();
    let MatchOutcome::AlbumEquivalent { candidates, tracks } = outcome else {
        panic!("both objects are human Album representations")
    };
    assert_eq!(candidates.len(), 2);
    assert_eq!(tracks.len(), 15);
    for (_, outcome) in tracks {
        let TrackOutcome::Matched(m) = outcome else {
            panic!()
        };
        assert!(m.occurrences.is_empty());
        assert!(m.recording.identities.is_empty());
    }
    assert_eq!(provider.calls.len(), 2);
    assert_eq!(local[5].evidence.artists[0].join_phrase, " feat. ");
    // Genuine primary performer contradictions still reject a whole program.
    let mut contradictory = provider.programs.values().next().unwrap().clone();
    for t in &mut contradictory.programs[0].tracks[..2] {
        t.artists = vec![ArtistEvidence {
            name: "Unrelated".into(),
            ..Default::default()
        }];
    }
    assert!(matches!(fit(&local, &contradictory), Fit::Rejected(_)));
}
