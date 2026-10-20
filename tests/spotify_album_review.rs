use music_library::{
    Library,
    album_program::{Program, Programs},
    domain::*,
    edition::{ArtistEvidence, TrackEvidence},
};
fn id(kind: &str, value: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: "spotify".into(),
        kind: kind.into(),
        external_id: value.into(),
    }
}
fn fixture() -> (tempfile::TempDir, Library, ImportedRelease, Programs) {
    let tmp = tempfile::tempdir().unwrap();
    let mut l = Library::open(tmp.path().join("review.sqlite")).unwrap();
    let r = l
        .create_catalog_release(&CatalogReleaseInput {
            title: "Example".into(),
            year: None,
            artists: vec![ArtistCreditInput {
                name: "A".into(),
                role: None,
            }],
            tracks: (1..=3)
                .map(|n| CatalogTrackInput {
                    title: format!("Song {n}"),
                    disc_number: None,
                    track_number: Some(n),
                    artists: vec![ArtistCreditInput {
                        name: "A".into(),
                        role: None,
                    }],
                })
                .collect(),
        })
        .unwrap();
    for t in &r.track_ids {
        l.add_to_library(t).unwrap();
    }
    let p = Programs {
        album: id("album", "candidate"),
        note: String::new(),
        programs: vec![Program {
            identity: None,
            complete: true,
            tracks: (1..=3)
                .map(|n| TrackEvidence {
                    identities: vec![id("track", &format!("song{n}"))],
                    disc: Some(1),
                    number: Some(n),
                    title: Some(format!("Song {n}")),
                    artists: vec![ArtistEvidence {
                        name: "A".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                })
                .collect(),
        }],
    };
    (tmp, l, r, p)
}
#[test]
fn grouped_counts_expansion_and_bulk_confirmation_preserve_identity() {
    let (_tmp, mut l, r, p) = fixture();
    let album = l.album_id_for_track(&r.track_ids[0]).unwrap();
    let before = l.edition_evidence(&r.release_id).unwrap();
    let groups = l.spotify_review_groups(None, 200).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].unresolved, 3);
    let tracks = l
        .browse(&music_library::browse::Request {
            album: Some(album.clone()),
            unresolved_spotify: true,
            limit: 201,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(tracks.len(), 3);
    let plan = l.prepare_spotify_album_confirmation(&album, &p).unwrap();
    assert_eq!(plan.matches().len(), 3);
    assert!(!plan.requires_confirmation());
    assert_eq!(l.confirm_spotify_album(&plan, false).unwrap(), 3);
    assert!(l.spotify_review_groups(None, 200).unwrap().is_empty());
    assert_eq!(
        l.edition_evidence(&r.release_id)
            .unwrap()
            .tracks
            .iter()
            .map(|t| &t.track_id)
            .collect::<Vec<_>>(),
        before
            .tracks
            .iter()
            .map(|t| &t.track_id)
            .collect::<Vec<_>>()
    );
    assert_eq!(l.library_queue(&Default::default()).unwrap().len(), 3);
    assert_eq!(
        l.list_album_external_identities(&album).unwrap(),
        vec![p.album]
    );
}
#[test]
fn significant_differences_require_confirmation_but_typography_does_not() {
    let (_tmp, mut l, r, mut p) = fixture();
    let album = l.album_id_for_track(&r.track_ids[0]).unwrap();
    p.programs[0].tracks[0].title = Some("  SONG 1  ".into());
    let plan = l.prepare_spotify_album_confirmation(&album, &p).unwrap();
    assert!(!plan.requires_confirmation());
    p.programs[0].tracks[1].title = Some("Another song".into());
    p.programs[0].tracks[2].artists.push(ArtistEvidence {
        name: "B".into(),
        ..Default::default()
    });
    let plan = l.prepare_spotify_album_confirmation(&album, &p).unwrap();
    assert!(plan.requires_confirmation());
    assert!(l.confirm_spotify_album(&plan, false).is_err());
    assert_eq!(l.unresolved_spotify_count().unwrap(), 3);
    assert_eq!(l.confirm_spotify_album(&plan, true).unwrap(), 3);
}
#[test]
fn unequal_programs_never_shift_positions_and_partial_resolution_remains_visible() {
    let (_tmp, mut l, r, mut p) = fixture();
    let album = l.album_id_for_track(&r.track_ids[0]).unwrap();
    p.programs[0].tracks.remove(1); // established positions 1 and 3; no inferred shift
    let plan = l.prepare_spotify_album_confirmation(&album, &p).unwrap();
    assert_eq!(
        plan.matches().iter().map(|m| m.number).collect::<Vec<_>>(),
        vec![1, 3]
    );
    assert_eq!(plan.unmatched_local, vec![r.track_ids[1].clone()]);
    assert_eq!(l.confirm_spotify_album(&plan, false).unwrap(), 2);
    assert_eq!(l.spotify_review_groups(None, 200).unwrap()[0].unresolved, 1);
    assert!(
        l.track_provider_occurrences(&r.track_ids[1], "spotify")
            .unwrap()
            .is_empty()
    );
}
#[test]
fn shifted_titles_stay_at_positions_and_duplicate_positions_are_unmapped() {
    let (_tmp, l, r, mut p) = fixture();
    let album = l.album_id_for_track(&r.track_ids[0]).unwrap();
    p.programs[0].tracks.remove(1);
    p.programs[0].tracks[1].number = Some(2);
    let plan = l.prepare_spotify_album_confirmation(&album, &p).unwrap();
    assert_eq!(
        plan.matches().len(),
        1,
        "omission evidence withholds the misleading position too"
    );
    assert_eq!(plan.matches()[0].track_id, r.track_ids[0]);
    assert_eq!(
        plan.unmatched_local,
        vec![r.track_ids[1].clone(), r.track_ids[2].clone()]
    );
    assert_eq!(plan.unmatched_provider, 1);
    p.programs[0].tracks[1].number = Some(1);
    let plan = l.prepare_spotify_album_confirmation(&album, &p).unwrap();
    assert!(plan.matches().is_empty());
}
#[test]
fn exclusions_cooldown_and_retry_eligibility_are_explicit() {
    let (tmp, mut l, r, p) = fixture();
    let album = l.album_id_for_track(&r.track_ids[0]).unwrap();
    l.reevaluate_stale_spotify(32).unwrap();
    assert!(l.spotify_album_needs_retry(&album).unwrap());
    assert_eq!(
        l.spotify_album_review_lifecycle(&album).unwrap(),
        l.spotify_review_groups(None, 200).unwrap()[0].status
    );
    assert!(l.spotify_retry_albums(None, 20).unwrap().contains(&album));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    l.retain_spotify_cooldown(now + 600).unwrap();
    assert_eq!(
        l.spotify_review_groups(None, 200).unwrap()[0].status,
        "Waiting for Spotify"
    );
    assert_eq!(
        l.spotify_album_review_lifecycle(&album).unwrap(),
        "Waiting for Spotify"
    );
    let rows = l
        .browse(&music_library::browse::Request {
            unresolved_spotify: true,
            ..Default::default()
        })
        .unwrap();
    assert!(
        rows.iter()
            .all(|r| !r.connection_reason.starts_with("Needs retry"))
    );
    l.mark_tracks_not_on_spotify(&r.track_ids).unwrap();
    assert!(l.spotify_retry_albums(None, 20).unwrap().is_empty());
    assert!(l.spotify_review_groups(None, 200).unwrap().is_empty());
    assert_eq!(
        l.spotify_album_review_lifecycle(&album).unwrap(),
        "No unresolved Tracks"
    );
    let plan = l.prepare_spotify_album_confirmation(&album, &p).unwrap();
    assert!(plan.matches().is_empty());
    let db = rusqlite::Connection::open(tmp.path().join("review.sqlite")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM library_membership", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        3
    );
}
#[test]
fn changed_evidence_cannot_be_confirmed_from_old_plan() {
    let (_tmp, mut l, r, p) = fixture();
    let album = l.album_id_for_track(&r.track_ids[0]).unwrap();
    let plan = l.prepare_spotify_album_confirmation(&album, &p).unwrap();
    l.attach_track_external_identity(&r.track_ids[0], &id("track", "another"))
        .unwrap();
    assert!(l.confirm_spotify_album(&plan, true).is_err());
    assert!(l.list_album_external_identities(&album).unwrap().is_empty());
}

#[test]
fn fresh_album_report_clears_retry_state_and_needed_contexts_ignore_old_cursor() {
    let (_tmp, mut l, r, _p) = fixture();
    let album = l.album_id_for_track(&r.track_ids[0]).unwrap();
    l.reevaluate_stale_spotify(32).unwrap();
    assert!(l.spotify_retry_albums(Some(&album), 20).unwrap().is_empty());
    assert_eq!(
        l.spotify_retry_needed_albums(20).unwrap(),
        vec![album.clone()]
    );
    let report = music_library::album_candidates::Report {
        musicbrainz_discovery: None,
        discovery_queries: vec![],
        local_title: "Example".into(),
        established_artist: None,
        candidate_count: 0,
        more_candidates: false,
        candidates: vec![],
        decision: "Withheld".into(),
        reasons: vec![music_library::album_candidates::Reason::NoCandidates],
    };
    l.persist_spotify_review(&album, &report).unwrap();
    assert!(!l.spotify_album_needs_retry(&album).unwrap());
    assert!(l.spotify_retry_needed_albums(20).unwrap().is_empty());
    assert_eq!(
        l.spotify_review_groups(None, 200).unwrap()[0].status,
        "No candidates"
    );
    assert_eq!(
        l.reevaluate_stale_spotify(32).unwrap().examined,
        0,
        "current Album report is not re-labelled Needs retry"
    );
}

#[test]
fn untitled_and_missing_disc_are_harmless_during_manual_album_mapping() {
    let (_tmp, mut l, _r, mut p) = fixture();
    let credit = ArtistCreditInput {
        name: "A".into(),
        role: None,
    };
    let r = l
        .create_catalog_release(&CatalogReleaseInput {
            title: "Untitled Album".into(),
            year: None,
            artists: vec![credit.clone()],
            tracks: vec![CatalogTrackInput {
                title: "[untitled]".into(),
                disc_number: None,
                track_number: Some(1),
                artists: vec![credit],
            }],
        })
        .unwrap();
    l.add_to_library(&r.track_ids[0]).unwrap();
    let album = l.album_id_for_track(&r.track_ids[0]).unwrap();
    p.programs[0].tracks[0].title = Some("Untitled".into());
    let plan = l.prepare_spotify_album_confirmation(&album, &p).unwrap();
    assert_eq!(plan.matches().len(), 1);
    assert!(!plan.requires_confirmation());
    assert_eq!(plan.matches()[0].disc, 1);
}

#[test]
fn album_exclusion_is_snapshot_only_and_preserves_connected_tracks() {
    let (_tmp, mut l, r, p) = fixture();
    let album = l.album_id_for_track(&r.track_ids[0]).unwrap();

    // Prepare a partial explicit mapping, then mark the rest.
    let candidate = music_library::song_resolution::Candidate {
        recording: Default::default(),
        identity: id("track", "manual1"),
        album_identity: Some(p.album),
        title: "Song 1".into(),
        artist: "A".into(),
        artists: p.programs[0].tracks[0].artists.clone(),
        album: "Example".into(),
        date: String::new(),
        album_artists: vec![],
        album_type: String::new(),
        album_total_tracks: Some(3),
        duration_ms: 0,
        disc: 1,
        number: 1,
    };
    let plan = l
        .prepare_spotify_track_mapping(&album, &[(r.track_ids[0].clone(), candidate)])
        .unwrap();
    assert_eq!(l.confirm_spotify_album(&plan, true).unwrap(), 1);
    assert_eq!(l.mark_album_not_on_spotify(&album).unwrap(), 2);
    assert!(!l.spotify_manually_excluded(&r.track_ids[0]).unwrap());
    assert!(l.spotify_manually_excluded(&r.track_ids[1]).unwrap());
    l.check_spotify_again(&r.track_ids[1]).unwrap();
    assert!(!l.spotify_manually_excluded(&r.track_ids[1]).unwrap());
}
#[test]
fn explicit_partial_mapping_spans_provider_albums_and_rejects_duplicates() {
    let (_tmp, mut l, r, p) = fixture();
    let album = l.album_id_for_track(&r.track_ids[0]).unwrap();
    let make = |n: u32, provider_album: &str| music_library::song_resolution::Candidate {
        recording: Default::default(),
        identity: id("track", &format!("manual{n}")),
        album_identity: Some(id("album", provider_album)),
        title: format!("Song {n}"),
        artist: "A".into(),
        artists: p.programs[0].tracks[0].artists.clone(),
        album: "Different Album".into(),
        date: String::new(),
        album_artists: vec![],
        album_type: String::new(),
        album_total_tracks: None,
        duration_ms: 0,
        disc: 1,
        number: n,
    };
    let one = make(1, "providerA");
    let two = make(2, "providerB");
    assert!(
        l.prepare_spotify_track_mapping(
            &album,
            &[
                (r.track_ids[0].clone(), one.clone()),
                (r.track_ids[1].clone(), one.clone())
            ]
        )
        .is_err()
    );
    let plan = l
        .prepare_spotify_track_mapping(
            &album,
            &[(r.track_ids[0].clone(), one), (r.track_ids[1].clone(), two)],
        )
        .unwrap();
    assert_eq!(plan.unmatched_local.len(), 1);
    assert!(l.confirm_spotify_album(&plan, false).is_err());
    assert_eq!(l.confirm_spotify_album(&plan, true).unwrap(), 2);
    assert!(l.list_album_external_identities(&album).unwrap().is_empty());
    assert_eq!(l.library_queue(&Default::default()).unwrap().len(), 3);
    assert_eq!(l.unresolved_spotify_count().unwrap(), 1);
    assert!(
        l.confirm_spotify_album(&plan, true).is_err(),
        "stale snapshot cannot duplicate trusted associations"
    );
}
#[test]
fn pasted_object_parsing_is_strict_and_supports_url_uri_id() {
    use music_library::spotify_review::parse_spotify_object;
    let id = "1234567890123456789012";
    for kind in ["album", "track"] {
        for value in [
            id.to_owned(),
            format!("spotify:{kind}:{id}"),
            format!("https://open.spotify.com/{kind}/{id}?si=abc"),
            format!("https://open.spotify.com/intl-ja/{kind}/{id}"),
        ] {
            assert_eq!(parse_spotify_object(&value, kind).unwrap().external_id, id);
        }
    }
    for value in [
        "https://evil.example/album/1234567890123456789012",
        "spotify:playlist:1234567890123456789012",
        "spotify:track:1234567890123456789012",
        "1234",
        "1234567890123456789012/extra",
    ] {
        assert!(parse_spotify_object(value, "album").is_err());
    }
}

#[test]
fn established_program_completion_persists_explicit_sequence_mapping_and_retains_local_only_track()
{
    for reorder in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db");
        let mut l = Library::open(&path).unwrap();
        let names = ["A", "B", "C", "Extra", "D", "E"];
        let r = l
            .create_catalog_release(&CatalogReleaseInput {
                title: "Example".into(),
                year: None,
                artists: vec![ArtistCreditInput {
                    name: "Artist".into(),
                    role: None,
                }],
                tracks: names
                    .iter()
                    .enumerate()
                    .map(|(i, n)| CatalogTrackInput {
                        title: (*n).into(),
                        disc_number: Some(1),
                        track_number: Some(i as u32 + 1),
                        artists: vec![],
                    })
                    .collect(),
            })
            .unwrap();
        for t in &r.track_ids {
            l.add_to_library(t).unwrap();
        }
        let album = l.album_id_for_track(&r.track_ids[0]).unwrap();
        let provider = id("album", "candidate");
        l.attach_album_external_identity(&album, &provider).unwrap();
        let order = if reorder {
            vec!["E", "B", "A", "D", "C"]
        } else {
            vec!["A", "B", "C", "D", "E"]
        };
        let p = Programs {
            album: provider.clone(),
            note: String::new(),
            programs: vec![Program {
                identity: None,
                complete: true,
                tracks: order
                    .iter()
                    .enumerate()
                    .map(|(i, n)| TrackEvidence {
                        title: Some((*n).into()),
                        disc: Some(1),
                        number: Some(i as u32 + 1),
                        identities: vec![id("track", n)],
                        ..Default::default()
                    })
                    .collect(),
            }],
        };
        let input = l.prepare_album_program(&album, &provider).unwrap().unwrap();
        let outcome = l
            .complete_album_program(music_library::album_program::Reply {
                input,
                result: Ok(p),
            })
            .unwrap();
        assert!(matches!(
            outcome,
            music_library::album_program::Outcome::Complete(_)
        ));
        drop(l);
        let l = Library::open(&path).unwrap();
        for (t, name) in r.track_ids.iter().zip(names) {
            assert_eq!(
                l.track_provider_occurrences(t, "spotify").unwrap(),
                if name == "Extra" {
                    vec![]
                } else {
                    vec![id("track", name)]
                }
            );
        }
        assert_eq!(l.library_queue(&Default::default()).unwrap().len(), 6);
        assert_eq!(l.unresolved_spotify_count().unwrap(), 1);
    }
}

#[test]
fn connections_plan_excludes_version_conflicts_and_preserves_explicit_mapping_escape_hatch() {
    let (_tmp, mut library, release, mut programs) = fixture();
    let album = library.album_id_for_track(&release.track_ids[0]).unwrap();
    programs.programs[0].tracks[2].title = Some("Song 3 (Live)".into());
    let legacy = library
        .prepare_spotify_album_confirmation(&album, &programs)
        .unwrap();
    assert_eq!(
        legacy.matches().len(),
        3,
        "legacy explicit review remains unchanged"
    );
    let selection = library
        .prepare_spotify_connections_confirmation(&album, &programs)
        .unwrap();
    assert_eq!(selection.matches().len(), 2);
    assert_eq!(selection.unmatched_provider, 1);
    assert!(selection.unmatched_local.contains(&release.track_ids[2]));
    assert_eq!(library.confirm_spotify_album(&selection, true).unwrap(), 2);
    assert_eq!(library.connection_coverage("spotify").unwrap().connected, 2);
    assert_eq!(
        library
            .connection_tracks("spotify", false, &album, None, 200)
            .unwrap()[0]
            .id,
        release.track_ids[2]
    );
    let explicit = library
        .prepare_spotify_track_mapping(
            &album,
            &[(
                release.track_ids[2].clone(),
                legacy.matches()[2].candidate().clone(),
            )],
        )
        .unwrap();
    assert!(explicit.requires_confirmation());
    assert_eq!(
        explicit.matches()[0].candidate().identity,
        legacy.matches()[2].candidate().identity
    );
}

#[test]
fn playable_subset_review_keeps_authoritative_reordered_mapping_across_existing_editions() {
    use music_library::album_program::{
        PositionDecision, inspect_album_program, inspect_playable_program,
    };
    let (_tmp, library, release, _) = fixture();
    let album = library.album_id_for_track(&release.track_ids[0]).unwrap();
    let template = library.local_album_tracks(&album).unwrap()[0].clone();
    let mut local: Vec<_> = (0..12)
        .map(|n| {
            let mut t = template.clone();
            t.track_id = TrackId(format!("canonical-{n}"));
            t.evidence.title = Some(format!("Song {n}"));
            t.evidence.number = Some(n + 1);
            t.evidence.duration_ms = Some(200_000);
            t.evidence.identities = vec![id("track", &format!("other-edition-{n}"))];
            t.evidence.artists[0].identities = vec![id("artist", "trusted")];
            t
        })
        .collect();
    let mut program = Program {
        identity: None,
        complete: true,
        tracks: (0..6)
            .rev()
            .enumerate()
            .map(|(j, n)| {
                let mut t = local[n].evidence.clone();
                t.identities = vec![id("track", &format!("candidate-{n}"))];
                t.number = Some(j as u32 + 1);
                t
            })
            .collect(),
    };
    for n in [6, 7] {
        let mut t = local[n].evidence.clone();
        t.title = Some(format!("Song {n} (Live)"));
        t.number = Some(n as u32 + 1);
        t.identities = vec![id("track", &format!("live-{n}"))];
        program.tracks.push(t);
    }
    // Two unparsed featured credits lack trusted Track-level Artist identity.
    for n in [2, 4] {
        local[n].evidence.artists[0].identities.clear();
        local[n].evidence.artists[0].name = "A ft. Guest".into();
    }
    assert!(
        !inspect_album_program(&local, &program, "Album")
            .alignment
            .strong
    );
    let review = inspect_playable_program(&local, &program, "Album");
    assert!(review.alignment.strong);
    assert_eq!(
        review
            .tracks
            .iter()
            .filter(|r| r.decision == PositionDecision::ReorderedTrack)
            .count(),
        6
    );
    for n in 0..6 {
        assert_eq!(
            review.tracks[n].provider.as_ref().unwrap().identities[0],
            id("track", &format!("candidate-{n}"))
        );
    }
    assert!(review.tracks[6..].iter().all(|r| r.provider.is_none()));
    // Explicit conflicting identity must never become neutral missing evidence.
    local[2].evidence.artists[0].identities = vec![id("artist", "different")];
    assert!(
        !inspect_playable_program(&local, &program, "Album")
            .alignment
            .strong
    );
}

#[test]
fn artist_identity_uses_full_case_neutral_names_or_trusted_identity_never_substrings() {
    use music_library::{artist_credit::same_artist, edition::ArtistEvidence};
    let full = ArtistEvidence {
        name: "hop along, queen ansleis".into(),
        ..Default::default()
    };
    let case = ArtistEvidence {
        name: "Hop Along, Queen Ansleis".into(),
        ..Default::default()
    };
    let mut short = ArtistEvidence {
        name: "Hop Along".into(),
        ..Default::default()
    };
    assert!(same_artist(&full, &case));
    assert!(!same_artist(&full, &short));
    let mut trusted = full;
    trusted.identities = vec![id("artist", "explicit-equivalence")];
    short.identities = trusted.identities.clone();
    assert!(same_artist(&trusted, &short));
}
