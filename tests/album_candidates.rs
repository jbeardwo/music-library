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
            MatchOutcome::AlbumAmbiguous(_)
        ),
        "one mismatch cannot reject an otherwise plausible program"
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
