use music_library::{
    Library,
    album_program::{self, Program, Programs, Reply, TitleRelation},
    domain::*,
    edition::TrackEvidence,
    playback_resolver::{RemoteCapability, Route},
};
#[derive(serde::Deserialize)]
struct Fixture {
    title: String,
    artist: String,
    year: i32,
    tracks: Vec<TrackEvidence>,
    programs: Vec<FixtureProgram>,
}
#[derive(serde::Deserialize)]
struct FixtureProgram {
    album: ExternalIdentity,
    tracks: Vec<TrackEvidence>,
}
fn fixture(name: &str) -> Fixture {
    serde_json::from_str(match name {
        "toe" => include_str!("fixtures/spotify/toe-program.json"),
        _ => include_str!("fixtures/spotify/tricot-program.json"),
    })
    .unwrap()
}
fn import(lib: &mut Library, f: &Fixture) -> ImportedRelease {
    lib.create_catalog_release(&CatalogReleaseInput {
        title: f.title.clone(),
        year: Some(f.year),
        artists: vec![ArtistCreditInput {
            name: f.artist.clone(),
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
    .unwrap()
}
fn programs(f: &Fixture) -> Programs {
    Programs {
        album: f.programs[0].album.clone(),
        programs: vec![Program {
            identity: None,
            complete: true,
            tracks: f.programs[0].tracks.clone(),
        }],
        note: String::new(),
    }
}
#[test]
fn live_toe_and_tricot_programs_persist_occurrences_without_linguistic_identity() {
    for name in ["toe", "tricot"] {
        let f = fixture(name);
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("db");
        let mut lib = Library::open(&path).unwrap();
        let imported = import(&mut lib, &f);
        let album = lib
            .album_for_release(&imported.release_id)
            .unwrap()
            .album_id;
        lib.attach_album_external_identity(&album, &f.programs[0].album)
            .unwrap();
        let input = lib
            .prepare_album_program(&album, &f.programs[0].album)
            .unwrap()
            .unwrap();
        let before = input.tracks.clone();
        for (track, provider) in imported.track_ids.iter().zip(&f.programs[0].tracks) {
            let manual_input = lib.song_resolution_input(track).unwrap();
            let candidate = music_library::song_resolution::Candidate {
                album_identity: None,
                identity: provider.identities[0].clone(),
                title: provider.title.clone().unwrap(),
                artist: f.artist.clone(),
                artists: provider.artists.clone(),
                album: f.title.clone(),
                date: f.year.to_string(),
                album_artists: provider.artists.clone(),
                album_type: "album".into(),
                album_total_tracks: Some(13),
                duration_ms: provider.duration_ms.unwrap(),
                disc: provider.disc.unwrap(),
                number: provider.number.unwrap(),
            };
            let feasible = music_library::song_resolution::feasibility(&manual_input, &candidate);
            if name == "toe" {
                assert_ne!(
                    feasible.class,
                    music_library::song_resolution::FeasibilityClass::Infeasible
                );
            } else if manual_input.title == "Hatsumimi" {
                // Global bounded search lacks a revalidated provider Album ID
                // and its surrounding program. It cannot use positional rescue.
                assert_eq!(
                    feasible.class,
                    music_library::song_resolution::FeasibilityClass::Infeasible
                );
            }
        }
        let started = std::time::Instant::now();
        lib.complete_album_program(Reply {
            input,
            result: Ok(programs(&f)),
        })
        .unwrap();
        println!(
            "{name}: occurrence comparison + transaction {:?}",
            started.elapsed()
        );
        let after = lib.local_album_tracks(&album).unwrap();
        let mut expected = before.clone();
        for (old, new) in expected.iter_mut().zip(&after) {
            old.evidence.duration_ms = new.evidence.duration_ms;
            old.evidence.duration_approximate = new.evidence.duration_approximate;
        }
        assert_eq!(
            expected, after,
            "provider duration enrichment preserves canonical identity"
        );
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
        drop(lib);
        let lib = Library::open(&path).unwrap();
        for (track, expected) in imported.track_ids.iter().zip(&f.programs[0].tracks) {
            assert_eq!(
                lib.track_provider_occurrences(track, "spotify").unwrap(),
                expected.identities
            );
            assert_eq!(
                lib.playback_route(
                    track,
                    &RemoteCapability {
                        provider: "spotify",
                        unavailable: None,
                        catalog_available: false,
                        accepts: |_| true,
                    }
                )
                .unwrap(),
                Route::Remote(expected.identities[0].clone())
            );
        }
        let db = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM artist_external_identity", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            0
        );
    }
}
#[test]
fn established_occurrence_requires_context_and_blocks_contradictions() {
    for case in [
        "partial",
        "one_track",
        "unresolved",
        "disc",
        "position",
        "unknown_position",
        "incomplete",
        "short",
        "reordered",
        "duplicate",
        "other_album",
        "Live",
        "Remix",
        "Demo",
        "Acoustic",
        "Instrumental",
        "Radio Edit",
        "identity",
        "occurrence_identity",
        "primary_identity",
        "disagreeing_programs",
        "super_fx",
    ] {
        let mut f = fixture("tricot");
        if case == "partial" {
            f.tracks
                .retain(|t| [1, 2, 6, 8].contains(&t.number.unwrap()));
        }
        if case == "one_track" {
            f.tracks.retain(|t| t.number == Some(8));
        }
        let temp = tempfile::tempdir().unwrap();
        let mut lib = Library::open(temp.path().join("db")).unwrap();
        let imported = import(&mut lib, &f);
        let album = lib
            .album_for_release(&imported.release_id)
            .unwrap()
            .album_id;
        let target =
            imported.track_ids[f.tracks.iter().position(|t| t.number == Some(8)).unwrap()].clone();
        let mut p = programs(&f);
        if case == "super_fx" {
            // Exact distinct titles at different positions must never be merged
            // through fuzzy spelling or a positional rescue.
            lib.set_track_title_override(&imported.track_ids[0], "Super Fx")
                .unwrap();
            lib.set_track_title_override(&imported.track_ids[1], "Super Fxx")
                .unwrap();
            p.programs[0].tracks[0].title = Some("Super Fxx".into());
            p.programs[0].tracks[1].title = Some("Super Fx".into());
        }
        if case == "unresolved" {
            assert!(
                lib.prepare_album_program(&album, &p.album)
                    .unwrap()
                    .is_none()
            );
            assert!(matches!(
                album_program::compare(&lib.local_album_tracks(&album).unwrap()[7], &p),
                album_program::TrackOutcome::NoConfidentMatch
            ));
            continue;
        }
        lib.attach_album_external_identity(&album, &p.album)
            .unwrap();
        if case == "identity" {
            let recording = lib.local_album_tracks(&album).unwrap()[7]
                .recording_id
                .clone();
            lib.attach_recording_external_identity(
                &recording,
                &ExternalIdentity {
                    provider: "trusted".into(),
                    kind: "recording".into(),
                    external_id: "one".into(),
                },
            )
            .unwrap();
            p.programs[0].tracks[7].recording.identities = vec![ExternalIdentity {
                provider: "trusted".into(),
                kind: "recording".into(),
                external_id: "two".into(),
            }];
        }
        if case == "primary_identity" {
            let db = rusqlite::Connection::open(temp.path().join("db")).unwrap();
            db.execute("INSERT INTO artist_external_identity(artist_id,provider,kind,external_id) SELECT id,'trusted','artist','one' FROM artist WHERE name='tricot'", []).unwrap();
            p.programs[0].tracks[7].artists[0].identities = vec![ExternalIdentity {
                provider: "trusted".into(),
                kind: "artist".into(),
                external_id: "two".into(),
            }];
        }
        match case {
            "disc" => p.programs[0].tracks[7].disc = Some(2),
            "position" => p.programs[0].tracks[7].number = Some(14),
            "unknown_position" => p.programs[0].tracks[7].number = None,
            "short" => p.programs[0].tracks.truncate(8),
            "incomplete" => p.programs[0].complete = false,
            "reordered" => {
                p.programs[0].tracks[0].title = Some("POOL".into());
                p.programs[0].tracks[1].title = Some("pool side".into());
            }
            "duplicate" => {
                let extra = p.programs[0].tracks[7].clone();
                p.programs[0].tracks.push(extra);
            }
            "other_album" => p.album.external_id = "another".into(),
            "disagreeing_programs" => {
                let mut other = p.programs[0].clone();
                other.tracks[7].identities[0].external_id = "alternate".into();
                p.programs.push(other);
            }
            "Live" | "Remix" | "Demo" | "Acoustic" | "Instrumental" | "Radio Edit" => {
                p.programs[0].tracks[7].title = Some(format!("初耳 ({case})"))
            }
            "occurrence_identity" => {
                let db = rusqlite::Connection::open(temp.path().join("db")).unwrap();
                db.execute("INSERT INTO track_external_identity(track_id,provider,kind,external_id) VALUES(?1,'spotify','track','existing')", [target.as_ref()]).unwrap();
            }
            _ => {}
        }
        let input = lib
            .prepare_album_program(&album, &f.programs[0].album)
            .unwrap()
            .unwrap();
        lib.complete_album_program(Reply {
            input,
            result: Ok(p),
        })
        .unwrap();
        let ids = lib.track_provider_occurrences(&target, "spotify").unwrap();
        match case {
            "partial" => assert_eq!(ids, f.programs[0].tracks[7].identities),
            "occurrence_identity" => assert_eq!(
                ids.iter()
                    .map(|i| i.external_id.as_str())
                    .collect::<Vec<_>>(),
                vec!["existing"]
            ),
            _ => assert!(ids.is_empty(), "{case}: {ids:?}"),
        }
    }
}
#[test]
fn title_evidence_is_not_translation_or_fuzzy_identity() {
    let title = |s: &str| TrackEvidence {
        title: Some(s.into()),
        ..Default::default()
    };
    assert_eq!(
        album_program::title_relation(&title("Hatsumimi"), &title("初耳")),
        TitleRelation::Uncorroborated
    );
    assert_eq!(
        album_program::title_relation(&title("Super Fx"), &title("Super Fxx")),
        TitleRelation::Uncorroborated
    );
    assert_eq!(
        album_program::title_relation(&title("Song"), &title("Song - Live")),
        TitleRelation::Contradictory
    );
    assert_eq!(
        album_program::title_relation(&title("Say It Ain’t So"), &title("say it ain't so")),
        TitleRelation::Agrees
    );
}
