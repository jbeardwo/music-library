use music_library::{Library, domain::*, edition::*};
fn id(kind: &str, key: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: "synthetic".into(),
        kind: kind.into(),
        external_id: key.into(),
    }
}
fn fixture(count: u32) -> (LocalEditionEvidence, EditionCandidate) {
    let tracks: Vec<_> = (1..=count)
        .map(|n| TrackEvidence {
            disc: Some(1),
            number: Some(n),
            title: Some(format!("Song {n}")),
            duration_ms: Some(180_000),
            ..Default::default()
        })
        .collect();
    let metadata = EditionMetadata {
        title: Some("Album".into()),
        artists: vec![ArtistEvidence {
            name: "Artist".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    (
        LocalEditionEvidence {
            album_id: AlbumId("a".into()),
            release_id: ReleaseId("r".into()),
            album_title: "Album".into(),
            grouping_identities: vec![],
            identities: vec![],
            exact_identities: vec![],
            metadata: metadata.clone(),
            completeness: Completeness::TrustedComplete,
            tracks: tracks
                .iter()
                .enumerate()
                .map(|(n, t)| LocalTrackEvidence {
                    track_id: TrackId(n.to_string()),
                    recording_id: RecordingId(n.to_string()),
                    evidence: t.clone(),
                })
                .collect(),
        },
        EditionCandidate {
            identity: id("edition", "one"),
            exact_identities: vec![],
            grouping: None,
            metadata,
            tracks,
            tracklist_complete: true,
        },
    )
}
fn recordings(l: &mut LocalEditionEvidence, c: &mut EditionCandidate) {
    for (n, (a, b)) in l.tracks.iter_mut().zip(&mut c.tracks).enumerate() {
        a.evidence.recording.identities = vec![id("recording", &n.to_string())];
        b.recording = a.evidence.recording.clone();
    }
}
fn assessment(l: &LocalEditionEvidence, c: EditionCandidate) -> Assessment {
    compare(l, &[c], true).assessment
}
#[test]
fn exact_requires_explicit_trusted_edition_identity_not_barcode_or_generic_mapping() {
    let (mut l, mut c) = fixture(10);
    l.metadata.barcodes = vec!["123".into()];
    c.metadata.barcodes = l.metadata.barcodes.clone();
    l.identities = vec![c.identity.clone()];
    assert_eq!(assessment(&l, c.clone()), Assessment::ContentEquivalent);
    l.exact_identities = vec![c.identity.clone()];
    c.exact_identities = vec![c.identity.clone()];
    assert_eq!(assessment(&l, c.clone()), Assessment::ExactEdition);
    l.completeness = Completeness::Unknown;
    l.tracks.clear();
    assert_eq!(assessment(&l, c.clone()), Assessment::ExactEdition);
    l.exact_identities = vec![id("edition", "different")];
    assert_eq!(assessment(&l, c), Assessment::Contradictory);
}
#[test]
fn identical_recordings_establish_content_even_with_different_display_titles() {
    let (mut l, mut c) = fixture(10);
    recordings(&mut l, &mut c);
    for t in &mut c.tracks {
        t.title = Some("Provider spelling".into());
    }
    assert_eq!(assessment(&l, c), Assessment::ContentEquivalent);
}
#[test]
fn matching_ordered_titles_and_durations_establish_content_without_recording_ids() {
    let (l, c) = fixture(10);
    assert_eq!(assessment(&l, c), Assessment::ContentEquivalent);
}

#[test]
fn empty_identifiers_are_missing_evidence_not_exact_identity() {
    let (mut l, mut c) = fixture(10);
    l.exact_identities = vec![id("edition", "")];
    c.exact_identities = l.exact_identities.clone();
    assert_eq!(assessment(&l, c), Assessment::ContentEquivalent);
}
#[test]
fn country_barcode_label_date_and_media_do_not_identify_or_veto_content() {
    let (mut l, mut c) = fixture(10);
    l.metadata.country = Some("US".into());
    c.metadata.country = Some("GB".into());
    l.metadata.barcodes = vec!["123".into()];
    c.metadata.barcodes = vec!["456".into()];
    l.metadata.labels = vec![("Original".into(), Some("ONE".into()))];
    c.metadata.labels = vec![("Repress".into(), Some("TWO".into()))];
    l.metadata.date = Some("2000".into());
    c.metadata.date = Some("2020".into());
    l.metadata.media = vec!["CD".into()];
    c.metadata.media = vec!["Digital".into()];
    assert_eq!(assessment(&l, c), Assessment::ContentEquivalent);
}
#[test]
fn two_discs_flatten_to_same_digital_musical_program_without_rewriting_positions() {
    let (mut l, mut c) = fixture(10);
    recordings(&mut l, &mut c);
    for (i, t) in c.tracks.iter_mut().enumerate().skip(5) {
        t.disc = Some(2);
        t.number = Some(i as u32 - 4);
    }
    let report = compare(&l, &[c.clone()], true);
    assert_eq!(report.assessment, Assessment::ContentEquivalent);
    assert_eq!(report.candidates[0].tracks[5].candidate_index, Some(5));
    assert_eq!(c.tracks[5].disc, Some(2));
    assert_eq!(l.tracks[5].evidence.disc, Some(1));
}
#[test]
fn bonus_missing_and_reordered_content_are_contradictions() {
    let (l, mut c) = fixture(10);
    c.tracks.push(c.tracks[0].clone());
    assert_eq!(assessment(&l, c), Assessment::Contradictory);
    let (l, mut c) = fixture(10);
    c.tracks.pop();
    assert_eq!(assessment(&l, c), Assessment::Contradictory);
    let (l, mut c) = fixture(10);
    c.tracks.swap(0, 1);
    assert_eq!(assessment(&l, c), Assessment::Contradictory);
}
#[test]
fn strong_recording_conflicts_override_title_duration_and_exact_edition_support() {
    let (mut l, mut c) = fixture(10);
    recordings(&mut l, &mut c);
    c.tracks[0].recording.identities = vec![id("recording", "other")];
    l.exact_identities = vec![c.identity.clone()];
    c.exact_identities = l.exact_identities.clone();
    let report = compare(&l, &[c], true);
    assert_eq!(report.assessment, Assessment::Contradictory);
    assert!(
        report.candidates[0].tracks[0]
            .findings
            .contains(&Finding::RecordingIdentityConflict)
    );
}
#[test]
fn isrc_is_corroboration_not_exact_or_standalone_program_identity() {
    let (mut l, mut c) = fixture(10);
    for (a, b) in l.tracks.iter_mut().zip(&mut c.tracks) {
        a.evidence.recording.isrcs = vec!["CODE".into()];
        b.recording.isrcs = vec!["CODE".into()];
    }
    assert_eq!(assessment(&l, c.clone()), Assessment::ContentEquivalent);
    for t in &mut c.tracks {
        t.title = None;
    }
    assert_eq!(assessment(&l, c), Assessment::AlbumOnly);
}
#[test]
fn partial_three_of_fifteen_and_one_track_never_prove_whole_content() {
    for numbers in [vec![1, 4, 9], vec![0]] {
        let (mut l, c) = fixture(15);
        l.completeness = Completeness::Unknown;
        l.tracks = numbers.iter().map(|&n| l.tracks[n].clone()).collect();
        assert_eq!(assessment(&l, c.clone()), Assessment::AlbumOnly);
        l.tracks[0].evidence.recording.identities = vec![id("recording", "local")];
        let mut c = c;
        c.tracks[numbers[0]].recording.identities = vec![id("recording", "conflict")];
        assert_eq!(assessment(&l, c), Assessment::Contradictory);
    }
}
#[test]
fn multiple_content_equivalents_are_ambiguous_but_one_exact_can_win() {
    let (mut l, c) = fixture(10);
    let mut other = c.clone();
    other.identity = id("edition", "two");
    let mut candidates = vec![c, other];
    assert_eq!(
        compare(&l, &candidates, true).assessment,
        Assessment::Ambiguous
    );
    l.exact_identities = vec![candidates[0].identity.clone()];
    candidates[0].exact_identities = l.exact_identities.clone();
    assert_eq!(
        compare(&l, &candidates, true).candidates[1].assessment,
        Assessment::ContentEquivalent
    );
    assert_eq!(
        compare(&l, &candidates, true).assessment,
        Assessment::ExactEdition
    );
    candidates.reverse();
    assert_eq!(
        compare(&l, &candidates, true).assessment,
        Assessment::ExactEdition
    );
}
#[test]
fn internally_conflicting_exact_ids_cannot_be_scored_through() {
    let (mut l, mut c) = fixture(10);
    l.exact_identities = vec![id("edition", "one"), id("edition", "two")];
    c.exact_identities = vec![id("edition", "one")];
    assert_eq!(assessment(&l, c), Assessment::Contradictory);
}
#[test]
fn incomplete_search_reports_per_candidate_content_without_claiming_unique_resolution() {
    let (l, c) = fixture(10);
    let r = compare(&l, &[c], false);
    assert_eq!(r.assessment, Assessment::Ambiguous);
    assert!(!r.candidates_complete);
    assert_eq!(r.candidates[0].assessment, Assessment::ContentEquivalent);
}
#[test]
fn optional_and_occurrence_evidence_cannot_invent_recordings() {
    let (mut l, mut c) = fixture(10);
    for (a, b) in l.tracks.iter_mut().zip(&mut c.tracks) {
        a.evidence.identities = vec![id("song", "same")];
        b.identities = a.evidence.identities.clone();
        b.title = None;
        b.duration_ms = None;
    }
    assert_eq!(assessment(&l, c.clone()), Assessment::AlbumOnly);
    c.metadata = EditionMetadata::default();
    assert_eq!(assessment(&l, c), Assessment::InsufficientEvidence);
}
#[test]
fn spelling_year_and_version_differences_reduce_support_without_false_strong_conflicts() {
    let (l, mut c) = fixture(10);
    c.tracks[0].title = Some("Song 1!".into());
    c.metadata.date = Some("2025".into());
    assert_eq!(assessment(&l, c.clone()), Assessment::AlbumOnly);
    c.tracks[0].title = Some("Song 1 (Live)".into());
    assert_eq!(assessment(&l, c), Assessment::AlbumOnly);
}
#[test]
fn metadata_only_duration_conflicts_remain_contradictions() {
    let (l, mut c) = fixture(10);
    c.tracks[0].duration_ms = Some(183_000);
    assert_eq!(assessment(&l, c.clone()), Assessment::ContentEquivalent);
    c.tracks[0].duration_ms = Some(183_001);
    assert_eq!(assessment(&l, c), Assessment::Contradictory);
}

#[test]
fn trusted_recording_identity_outranks_duration_disagreement() {
    for duration in [183_000, 210_000] {
        let (mut local, mut candidate) = fixture(10);
        recordings(&mut local, &mut candidate);
        candidate.tracks[0].duration_ms = Some(duration);
        let report = compare(&local, &[candidate], true);
        assert_eq!(report.assessment, Assessment::ContentEquivalent);
        let track = &report.candidates[0].tracks[0];
        assert!(track.supported);
        assert!(!track.contradiction);
        assert!(track.findings.contains(if duration == 183_000 {
            &Finding::DurationSupport
        } else {
            &Finding::DurationConflict
        }));
    }
}

#[test]
fn conflicting_recording_ids_remain_contradictory_regardless_of_duration() {
    for duration in [180_000, 210_000] {
        let (mut local, mut candidate) = fixture(10);
        recordings(&mut local, &mut candidate);
        // Even partial overlap must not hide another conflicting strong assertion.
        candidate.tracks[0]
            .recording
            .identities
            .push(id("recording", "conflict"));
        candidate.tracks[0].duration_ms = Some(duration);
        assert_eq!(assessment(&local, candidate), Assessment::Contradictory);
    }
}
#[test]
fn database_snapshot_does_not_promote_generic_identity_or_contiguous_tracks_to_trusted_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.sqlite");
    let mut library = Library::open(&path).unwrap();
    let imported = library
        .create_catalog_release(&CatalogReleaseInput {
            title: "Local".into(),
            year: Some(2000),
            artists: vec![],
            tracks: vec![CatalogTrackInput {
                title: "Track".into(),
                artists: vec![],
                disc_number: Some(1),
                track_number: Some(1),
            }],
        })
        .unwrap();
    library
        .attach_release_external_identity(&imported.release_id, &id("edition", "known"))
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    let evidence = library.edition_evidence(&imported.release_id).unwrap();
    let readonly = music_library::edition_storage::read_only(&path, &imported.release_id).unwrap();
    assert_eq!(evidence.completeness, Completeness::Unknown);
    assert!(evidence.exact_identities.is_empty());
    assert_eq!(evidence.identities.len(), 1);
    assert_eq!(evidence.tracks[0].track_id, readonly.tracks[0].track_id);
    assert_eq!(before, std::fs::read(&path).unwrap());
    let db = rusqlite::Connection::open(path).unwrap();
    for sql in [
        "SELECT id FROM track WHERE release_id='x' ORDER BY disc_number,track_number,id",
        "SELECT provider,kind,external_id FROM recording_external_identity WHERE recording_id='x'",
    ] {
        let plan: String = db
            .query_row(&format!("EXPLAIN QUERY PLAN {sql}"), [], |r| r.get(3))
            .unwrap();
        assert!(plan.contains("SEARCH") && plan.contains("INDEX"), "{plan}");
    }
}
#[test]
#[ignore = "opt-in deterministic timing; no network"]
fn comparison_timing() {
    for partial in [false, true] {
        for count in [1, 10, 100] {
            let (mut l, c) = fixture(10);
            if partial {
                l.completeness = Completeness::Unknown;
                l.tracks = vec![
                    l.tracks[1].clone(),
                    l.tracks[4].clone(),
                    l.tracks[9].clone(),
                ];
            }
            let candidates = vec![c; count];
            let mut samples = vec![];
            for _ in 0..1000 {
                let start = std::time::Instant::now();
                std::hint::black_box(compare(&l, &candidates, true));
                samples.push(start.elapsed());
            }
            samples.sort();
            println!(
                "partial={partial}, {count} candidates: median {:?}",
                samples[500]
            );
        }
    }
}
