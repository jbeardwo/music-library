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
    albums: Vec<ArtistAlbumCandidate>,
}
impl Provider {
    fn new() -> Self {
        Self {
            counts: HashMap::new(),
            programs: [("a".into(), program("a")), ("b".into(), program("b"))].into(),
            calls: vec![],
            albums: vec![],
        }
    }
}
impl CatalogProvider for Provider {
    fn album_program_namespaces(&self) -> Vec<(String, String)> {
        vec![("plain-catalog".into(), "album".into())]
    }
    fn search_artists(&mut self, _: &str) -> Result<Page<ArtistCandidate>, CatalogError> {
        Ok(Page {
            next_offset: None,
            items: vec![ArtistCandidate {
                identity: id("artist", "artist"),
                name: "Artist".into(),
                aliases: vec![],
                comment: String::new(),
                country: String::new(),
                artist_type: String::new(),
                score: None,
            }],
        })
    }
    fn artist_albums(
        &mut self,
        _: &ExternalIdentity,
        _: &str,
    ) -> Result<Page<ArtistAlbumCandidate>, CatalogError> {
        Ok(Page {
            next_offset: None,
            items: self.albums.clone(),
        })
    }

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
        ("Album", None),
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
            ("Album", None),
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
            ("Album", None),
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
        (&f.title, None),
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

#[test]
fn date_preference_follows_program_compatibility_and_preserves_ties() {
    use music_library::catalog_date::Date;
    let local = locals(&[1, 2, 3, 4, 5]);
    let page = |a: &str, b: &str| Page {
        items: vec![
            ArtistAlbumCandidate {
                date: a.into(),
                ..candidate("a")
            },
            ArtistAlbumCandidate {
                date: b.into(),
                ..candidate("b")
            },
        ],
        next_offset: None,
    };
    let resolve_date = |p: &mut Provider, date: &str, page: &Page<ArtistAlbumCandidate>| {
        resolve(
            p,
            ("Album", Date::parse(date)),
            &id("artist", "artist"),
            page,
            false,
            &local,
            &mut vec![],
        )
        .unwrap()
    };
    assert_eq!(
        resolve_date(
            &mut Provider::new(),
            "2005",
            &page("2005-05-23", "2014-04-11")
        ),
        MatchOutcome::Matched(id("album", "a"))
    );
    for (date, a, b) in [
        ("", "2005", "2014"),
        ("2005", "2005-05-23", "2005-01-01"),
        ("2000", "2005", "2014"),
    ] {
        assert!(matches!(
            resolve_date(&mut Provider::new(), date, &page(a, b)),
            MatchOutcome::AlbumEquivalent { .. }
        ));
    }
    assert_eq!(
        resolve_date(
            &mut Provider::new(),
            "2005-05-23",
            &page("2005-05-23", "2005-04-11")
        ),
        MatchOutcome::Matched(id("album", "a"))
    );
    let mut p = Provider::new();
    for n in [0, 1] {
        p.programs.get_mut("a").unwrap().programs[0].tracks[n].title = Some("Wrong song".into());
    }
    assert_eq!(
        resolve_date(&mut p, "2005", &page("2005", "2014")),
        MatchOutcome::Matched(id("album", "b"))
    );
    let mut p = Provider::new();
    p.programs.get_mut("b").unwrap().programs[0].complete = false;
    assert!(!matches!(
        resolve_date(&mut p, "2005", &page("2005", "2014")),
        MatchOutcome::Matched(_)
    ));
    let mut p = Provider::new();
    let mut wrong_artist = page("2005", "2014");
    wrong_artist.items[0].artist_ids = vec![id("artist", "other")];
    assert_eq!(
        resolve_date(&mut p, "2005", &wrong_artist),
        MatchOutcome::Matched(id("album", "b"))
    );
    assert_eq!(fit(&local, &program("b")), Fit::Supported); // later date never changes human compatibility
}

#[test]
fn catalog_dates_compare_only_known_precision() {
    use music_library::catalog_date::{Agreement, Date, agreement};
    assert_eq!(
        agreement(Date::parse("2005"), Date::parse("2005-05-23")),
        Agreement::Year
    );
    assert_eq!(
        agreement(Date::parse("2005-05"), Date::parse("2005-05-23")),
        Agreement::Month
    );
    assert_eq!(
        agreement(Date::parse("2005-05-23"), Date::parse("2005-05-23")),
        Agreement::Day
    );
    for invalid in [
        "",
        "2005-99",
        "2005-02-29",
        "2005-05-00",
        "2005-x",
        "2005-05-23-extra",
    ] {
        assert!(Date::parse(invalid).is_none());
    }
    assert!(Date::parse("2004-02-29").is_some());
}

#[test]
fn preferred_catalog_program_persists_occurrences_without_exact_edition_claim() {
    use music_library::album_matching::{MatchReply, Preparation};
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let (mut lib, imported, album) = catalog_fixture(&path);
    let scope = MatchingScope {
        provider: "plain-catalog".into(),
        artist_kind: "artist".into(),
        album_kind: "album".into(),
    };
    let Preparation::Ready(input) = lib.prepare_album_match_for(&album, &scope).unwrap() else {
        panic!()
    };
    let local = lib.local_album_tracks(&album).unwrap();
    let mut p = Provider::new();
    for t in &mut p.programs.get_mut("b").unwrap().programs[0].tracks {
        t.identities = vec![id("song", &format!("alternate{}", t.number.unwrap()))];
    }
    let page = Page {
        items: vec![
            ArtistAlbumCandidate {
                date: "2005-05-23".into(),
                ..candidate("a")
            },
            ArtistAlbumCandidate {
                date: "2014-04-11".into(),
                ..candidate("b")
            },
        ],
        next_offset: None,
    };
    let outcome = resolve(
        &mut p,
        ("Album", music_library::catalog_date::Date::parse("2005")),
        &id("artist", "artist"),
        &page,
        false,
        &local,
        &mut vec![],
    )
    .unwrap();
    assert_eq!(outcome, MatchOutcome::Matched(id("album", "a")));
    lib.complete_album_match_for(
        MatchReply {
            input,
            artist: Some(id("artist", "artist")),
            outcome,
            matched_album: Some(page.items[0].clone()),
        },
        &scope,
    )
    .unwrap();
    let input = lib
        .prepare_album_program(&album, &id("album", "a"))
        .unwrap()
        .unwrap();
    lib.complete_album_program(music_library::album_program::Reply {
        input,
        result: Ok(program("a")),
    })
    .unwrap();
    assert!(
        lib.list_release_external_identities(&imported.release_id)
            .unwrap()
            .is_empty()
    );
    drop(lib);
    let lib = music_library::Library::open(&path).unwrap();
    for track in &local {
        assert_eq!(
            lib.track_provider_occurrences(&track.track_id, "plain-catalog")
                .unwrap(),
            vec![id(
                "song",
                &format!("song{}", track.evidence.number.unwrap())
            )]
        );
        assert!(
            lib.list_recording_external_identities(&track.recording_id)
                .unwrap()
                .is_empty()
        );
    }
    // Membership is independent: the established partial program still has
    // positions 2,6,9, and does not claim a complete nine-Track edition.
    let input = lib.song_resolution_input(&imported.track_ids[0]).unwrap();
    assert_eq!(input.album_required_tracks, 9);
}

#[test]
#[ignore = "local date-discrimination comparison timing, no network"]
fn date_representation_comparison_timing() {
    for count in [5, 15, 30] {
        let local = locals(&(1..=count).collect::<Vec<_>>());
        let mut p = Provider::new();
        for program in p.programs.values_mut() {
            program.programs[0].tracks = local.iter().map(|t| t.evidence.clone()).collect();
        }
        let page = Page {
            items: vec![
                ArtistAlbumCandidate {
                    date: "2005-05-23".into(),
                    ..candidate("a")
                },
                ArtistAlbumCandidate {
                    date: "2014-04-11".into(),
                    ..candidate("b")
                },
            ],
            next_offset: None,
        };
        let mut cache = p.programs.values().cloned().collect();
        let start = std::time::Instant::now();
        for _ in 0..1000 {
            std::hint::black_box(
                resolve(
                    &mut p,
                    ("Album", music_library::catalog_date::Date::parse("2005")),
                    &id("artist", "artist"),
                    &page,
                    false,
                    &local,
                    &mut cache,
                )
                .unwrap(),
            );
        }
        println!(
            "Date preference, two cached programs / {count} Tracks: {:?}",
            start.elapsed() / 1000
        );
        assert!(p.calls.is_empty());
    }
}

#[test]
fn changed_album_date_rejects_stale_representation_choice() {
    use music_library::album_matching::{MatchReply, Preparation};
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let (mut lib, _, album) = catalog_fixture(&path);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "UPDATE album_application_metadata SET year=2005 WHERE album_id=?1",
        [album.as_ref()],
    )
    .unwrap();
    let scope = MatchingScope {
        provider: "plain-catalog".into(),
        artist_kind: "artist".into(),
        album_kind: "album".into(),
    };
    let Preparation::Ready(input) = lib.prepare_album_match_for(&album, &scope).unwrap() else {
        panic!()
    };
    assert_eq!(input.date, music_library::catalog_date::Date::parse("2005"));
    db.execute(
        "UPDATE album_application_metadata SET year=2014 WHERE album_id=?1",
        [album.as_ref()],
    )
    .unwrap();
    let result = lib
        .complete_album_match_for(
            MatchReply {
                input,
                artist: Some(id("artist", "artist")),
                outcome: MatchOutcome::Matched(id("album", "a")),
                matched_album: Some(candidate("a")),
            },
            &scope,
        )
        .unwrap();
    assert_eq!(result, MatchOutcome::Skipped);
    assert!(
        lib.list_album_external_identities(&album)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn established_spotify_occurrence_ignores_credit_only_with_position_and_title() {
    use music_library::album_program::{RecordingStatus, Reply};
    let spotify = |kind: &str, value: &str| ExternalIdentity {
        provider: "spotify".into(),
        kind: kind.into(),
        external_id: value.into(),
    };
    for case in [
        "accepted",
        "position",
        "disc",
        "unknown",
        "unknown_local",
        "title",
        "remix",
        "live",
        "unresolved",
        "album",
        "manual",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("db");
        let mut lib = music_library::Library::open(&path).unwrap();
        let imported = lib
            .create_catalog_release(&CatalogReleaseInput {
                title: "Album".into(),
                year: None,
                artists: vec![ArtistCreditInput {
                    name: "Artist".into(),
                    role: None,
                }],
                tracks: vec![CatalogTrackInput {
                    title: "Song 2".into(),
                    artists: vec![ArtistCreditInput {
                        name: "Rosie Wilson".into(),
                        role: None,
                    }],
                    disc_number: Some(1),
                    track_number: (case != "unknown_local").then_some(2),
                }],
            })
            .unwrap();
        let album = lib
            .album_for_release(&imported.release_id)
            .unwrap()
            .album_id;
        let accepted = spotify("album", "preferred");
        let mut p = program("a");
        p.album = accepted.clone();
        for t in &mut p.programs[0].tracks {
            t.artists = vec![ArtistEvidence {
                name: "Different contributor".into(),
                identities: vec![spotify("artist", "unresolved")],
                ..Default::default()
            }];
            t.identities = vec![spotify("track", &format!("track{}", t.number.unwrap()))];
        }
        match case {
            "position" => p.programs[0].tracks[1].number = Some(3),
            "disc" => p.programs[0].tracks[1].disc = Some(2),
            "unknown" => p.programs[0].tracks[1].number = None,
            "title" => p.programs[0].tracks[1].title = Some("Other song".into()),
            "remix" => p.programs[0].tracks[1].title = Some("Song 2 (Remix)".into()),
            "live" => p.programs[0].tracks[1].title = Some("Song 2 (Live)".into()),
            "album" => p.album = spotify("album", "other"),
            _ => {}
        }
        if case == "unresolved" {
            assert!(
                lib.prepare_album_program(&album, &accepted)
                    .unwrap()
                    .is_none()
            );
            assert!(
                music_library::album_program::compare_album(
                    &lib.local_album_tracks(&album).unwrap(),
                    &p
                )
                .iter()
                .all(|o| !matches!(o, TrackOutcome::Matched(m) if !m.occurrences.is_empty()))
            );
            continue;
        }
        lib.attach_album_external_identity(&album, &accepted)
            .unwrap();
        let input = lib
            .prepare_album_program(&album, &accepted)
            .unwrap()
            .unwrap();
        let before = input.tracks.clone();
        if case == "manual" {
            let chosen = music_library::manual_track::Candidate {
                supporting_programs: 1,
                evidence: TrackEvidence {
                    identities: vec![spotify("track", "manual")],
                    ..Default::default()
                },
            };
            let db = rusqlite::Connection::open(&path).unwrap();
            db.execute("INSERT INTO manual_track_association(track_id,album_provider,album_kind,album_external_id,candidate_json) VALUES(?1,'spotify','album','preferred',?2)", rusqlite::params![imported.track_ids[0].as_ref(), serde_json::to_string(&chosen).unwrap()]).unwrap();
        }
        let result = lib
            .complete_album_program(Reply {
                input,
                result: Ok(p),
            })
            .unwrap();
        if case == "accepted" {
            assert!(
                matches!(&result, music_library::album_program::Outcome::Complete(rows) if rows.iter().all(|(_, o)| matches!(o, TrackOutcome::Matched(m) if m.recording_status == RecordingStatus::NotProvided && m.recording.identities.is_empty())))
            );
        }
        assert_eq!(before, lib.local_album_tracks(&album).unwrap());
        assert!(
            lib.list_release_external_identities(&imported.release_id)
                .unwrap()
                .is_empty()
        );
        for t in &before {
            assert!(
                lib.list_recording_external_identities(&t.recording_id)
                    .unwrap()
                    .is_empty()
            );
        }
        let db = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM artist_external_identity WHERE provider='spotify'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        drop(lib);
        let lib = music_library::Library::open(&path).unwrap();
        let ids = lib
            .track_provider_occurrences(&imported.track_ids[0], "spotify")
            .unwrap();
        if case == "accepted" || case == "manual" {
            let expected = spotify("track", if case == "manual" { "manual" } else { "track2" });
            assert_eq!(ids, vec![expected.clone()]);
            assert_eq!(
                lib.playback_route(
                    &imported.track_ids[0],
                    &music_library::playback_resolver::RemoteCapability {
                        provider: "spotify",
                        unavailable: None,
                        catalog_available: false,
                        accepts: |i| i.kind == "track"
                    }
                )
                .unwrap(),
                music_library::playback_resolver::Route::Remote(expected)
            );
        } else {
            assert!(ids.is_empty(), "{case}: {ids:?}");
        }
    }
}

#[test]
fn preferred_demon_days_persists_dare_with_unresolved_contributors() {
    let f: DemonFixture =
        serde_json::from_str(include_str!("fixtures/spotify/demon-days-credits.json")).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let mut lib = music_library::Library::open(&path).unwrap();
    let imported = lib
        .create_catalog_release(&CatalogReleaseInput {
            title: f.title,
            year: Some(f.year.try_into().unwrap()),
            artists: vec![ArtistCreditInput {
                name: f.artist,
                role: None,
            }],
            tracks: f
                .tracks
                .iter()
                .map(|t| CatalogTrackInput {
                    title: t.title.clone().unwrap(),
                    artists: t
                        .artists
                        .iter()
                        .map(|a| ArtistCreditInput {
                            name: a.name.clone(),
                            role: None,
                        })
                        .collect(),
                    disc_number: t.disc,
                    track_number: t.number,
                })
                .collect(),
        })
        .unwrap();
    let album = lib
        .album_for_release(&imported.release_id)
        .unwrap()
        .album_id;
    let preferred = &f.programs[0];
    lib.attach_album_external_identity(&album, &preferred.album)
        .unwrap();
    let input = lib
        .prepare_album_program(&album, &preferred.album)
        .unwrap()
        .unwrap();
    let before = input.tracks.clone();
    let check =
        music_library::album_program::inspect_candidate(&before[11], &preferred.tracks[11], 11);
    assert!(check.considered && check.exact_title && check.position && !check.occurrence_supported);
    lib.complete_album_program(music_library::album_program::Reply {
        input,
        result: Ok(Programs {
            album: preferred.album.clone(),
            programs: vec![Program {
                identity: None,
                complete: true,
                tracks: preferred.tracks.clone(),
            }],
            note: String::new(),
        }),
    })
    .unwrap();
    assert_eq!(before, lib.local_album_tracks(&album).unwrap());
    drop(lib);
    let lib = music_library::Library::open(&path).unwrap();
    for (t, expected) in before.iter().zip(&preferred.tracks) {
        assert_eq!(
            lib.track_provider_occurrences(&t.track_id, "spotify")
                .unwrap(),
            expected.identities
        );
        assert!(
            lib.list_recording_external_identities(&t.recording_id)
                .unwrap()
                .is_empty()
        );
    }
    assert!(
        lib.list_release_external_identities(&imported.release_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn preferred_representation_survives_import_worker_completion_and_restart() {
    use music_library::album_matching::{AlbumMatcher, AutoMatchPolicy, MatchReply};
    enum Event {
        Album(Box<MatchReply>),
        Program(Box<music_library::album_program::Reply>),
    }
    for (year, alternate_date, expected) in [
        (Some(2005), "2014-04-11", true),
        (Some(2005), "2005-01-01", false),
        (None, "2014-04-11", false),
        (Some(2000), "2014-04-11", false),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("db");
        let mut lib = music_library::Library::open(&path).unwrap();
        let imported = lib
            .create_catalog_release(&CatalogReleaseInput {
                title: "Album".into(),
                year,
                artists: vec![ArtistCreditInput {
                    name: "Artist".into(),
                    role: None,
                }],
                tracks: (1..=5)
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
        let mut provider = Provider::new();
        provider.albums = vec![
            ArtistAlbumCandidate {
                date: alternate_date.into(),
                ..candidate("b")
            },
            ArtistAlbumCandidate {
                date: "2005-05-23".into(),
                ..candidate("a")
            },
        ];
        for t in &mut provider.programs.get_mut("b").unwrap().programs[0].tracks {
            t.identities = vec![id("song", &format!("alternate{}", t.number.unwrap()))];
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let program_tx = tx.clone();
        let mut matcher = AlbumMatcher::for_provider(
            provider,
            MatchingScope {
                provider: "plain-catalog".into(),
                artist_kind: "artist".into(),
                album_kind: "album".into(),
            },
            move |r| {
                tx.send(Event::Album(Box::new(r))).unwrap();
            },
            move |r| {
                program_tx.send(Event::Program(Box::new(r))).unwrap();
            },
            |_| {},
        )
        .unwrap();
        matcher
            .after_import(
                &lib,
                std::slice::from_ref(&imported),
                AutoMatchPolicy::default(),
            )
            .unwrap();
        assert!(matcher.pending_count() > 0);
        while matcher.pending_count() > 0 {
            match rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap() {
                Event::Album(reply) => {
                    if expected {
                        assert_eq!(reply.outcome, MatchOutcome::Matched(id("album", "a")));
                    } else {
                        assert!(matches!(
                            reply.outcome,
                            MatchOutcome::AlbumEquivalent { .. }
                        ));
                    }
                    matcher.complete(&mut lib, *reply);
                }
                Event::Program(reply) => {
                    assert!(expected);
                    assert_eq!(reply.input.album, id("album", "a"));
                    matcher.complete_programs(&mut lib, *reply);
                }
            }
        }
        drop(matcher);
        drop(lib);
        let lib = music_library::Library::open(&path).unwrap();
        assert_eq!(
            lib.list_album_external_identities(&album).unwrap().len(),
            usize::from(expected)
        );
        assert!(
            lib.list_release_external_identities(&imported.release_id)
                .unwrap()
                .is_empty()
        );
        for (n, track) in imported.track_ids.iter().enumerate() {
            let ids = lib
                .track_provider_occurrences(track, "plain-catalog")
                .unwrap();
            if expected {
                let occurrence = id("song", &format!("song{}", n + 1));
                assert_eq!(ids, vec![occurrence.clone()]);
                // No provider exists after reopen, and catalog access is disabled.
                assert_eq!(
                    lib.playback_route(
                        track,
                        &music_library::playback_resolver::RemoteCapability {
                            provider: "plain-catalog",
                            unavailable: None,
                            catalog_available: false,
                            accepts: |_| true,
                        }
                    )
                    .unwrap(),
                    music_library::playback_resolver::Route::Remote(occurrence)
                );
            } else {
                assert!(ids.is_empty());
            }
        }
    }
}
