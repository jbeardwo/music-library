use music_library::{
    album_program::{Program, align_program, numeric_title},
    domain::*,
    edition::*,
};
fn local(names: &[&str]) -> Vec<LocalTrackEvidence> {
    names
        .iter()
        .enumerate()
        .map(|(i, name)| LocalTrackEvidence {
            track_id: TrackId(format!("t{i}")),
            recording_id: RecordingId(format!("r{i}")),
            evidence: TrackEvidence {
                title: Some((*name).into()),
                number: Some(i as u32 + 1),
                disc: Some(1),
                ..Default::default()
            },
        })
        .collect()
}
fn program(names: &[&str]) -> Program {
    Program {
        identity: None,
        tracks: local(names).into_iter().map(|t| t.evidence).collect(),
        complete: true,
    }
}
#[test]
fn insertion_proves_later_correspondence() {
    let l = local(&["A", "B", "C", "Extra", "D", "E"]);
    let a = align_program(&l, &program(&["A", "B", "C", "D", "E"]));
    assert!(a.strong);
    assert_eq!(a.rows[3].provider_index, None);
    assert_eq!(a.rows[4].provider_index, Some(3));
    assert_eq!(a.rows[5].provider_index, Some(4));
}
#[test]
fn unique_reorder_maps_titles_not_indexes() {
    let a = align_program(&local(&["A", "B", "C"]), &program(&["C", "A", "B"]));
    assert!(a.strong);
    assert_eq!(a.rows[0].provider_index, Some(1));
}
#[test]
fn ambiguous_duplicates_never_guess() {
    let a = align_program(
        &local(&["Untitled", "Untitled", "A"]),
        &program(&["Untitled", "A", "Untitled"]),
    );
    assert!(!a.strong);
    assert_eq!(a.rows[0].provider_index, None);
}
#[test]
fn superficial_overlap_and_live_versions_are_not_correspondence() {
    assert!(
        !align_program(
            &local(&["A", "B", "C", "D", "E", "F"]),
            &program(&["A", "B", "C", "X", "Y", "Z"])
        )
        .strong
    );
    assert!(
        !align_program(
            &local(&["A", "B", "C"]),
            &program(&["A Live", "B Live", "C Live"])
        )
        .strong
    );
}
#[test]
fn malformed_positions_require_strong_title_program() {
    let mut l = local(&["A", "B", "C"]);
    for (i, t) in l.iter_mut().enumerate() {
        t.evidence.number = Some(101 + i as u32);
    }
    let a = align_program(&l, &program(&["A", "B", "C"]));
    assert!(a.unreliable_positions && a.strong);
    assert!(!align_program(&l, &program(&["X", "Y", "Z"])).strong);
}
#[test]
fn numeric_titles_are_conservative() {
    for (a, b) in [("One", "1"), ("Thirteen", "13"), ("Twenty One", "21")] {
        assert_eq!(numeric_title(a), numeric_title(b));
    }
    for title in ["One More", "One Direction", "Twenty One Pilots", "Someone"] {
        assert_eq!(numeric_title(title), title.to_lowercase());
    }
}

#[test]
fn bracketed_stylized_titles_ignore_case_and_symbol_spacing_only() {
    let a = align_program(
        &local(&["I Am S/H(im)e[r]", "A", "B"]),
        &program(&["i am s / H(Im)E[R]", "A", "B"]),
    );
    assert!(a.strong);
    for title in ["I Am SHimer", "I Am S/H(im)e{x}", "IAm S/H(im)e[r]"] {
        assert!(
            align_program(
                &local(&["I Am S/H(im)e[r]", "A", "B"]),
                &program(&[title, "A", "B"])
            )
            .rows[0]
                .provider_index
                .is_none()
        );
    }
}

#[test]
fn contextual_systematic_annotations_preserve_raw_titles() {
    use music_library::album_program::inspect_album_program;
    let local = local(&["A", "B", "C"]);
    for (album, suffix) in [
        ("Animals Acoustic", "Acoustic"),
        ("Audiotree Live", "Audiotree Live Version"),
        ("Audiotree", "Audiotree Live Version"),
    ] {
        let mut p = program(&["A", "B", "C"]);
        for t in &mut p.tracks {
            t.title = Some(format!("{} - {suffix}", t.title.as_deref().unwrap()));
        }
        let result = inspect_album_program(&local, &p, album);
        assert!(result.compatible());
        assert!(
            result.tracks[0]
                .provider
                .as_ref()
                .unwrap()
                .title
                .as_deref()
                .unwrap()
                .contains(suffix)
        );
        assert!(!inspect_album_program(&local, &p, "Studio Album").compatible());
        p.tracks[1].title = Some("Unrelated".into());
        assert!(!inspect_album_program(&local, &p, album).compatible());
    }
}
#[test]
fn movement_annotation_requires_program_evidence() {
    use music_library::album_program::inspect_album_program;
    let mut l = local(&["A", "B", "C", "D", "E", "F"]);
    for (i, t) in l.iter_mut().take(3).enumerate() {
        t.evidence.title = Some(format!(
            "{} (Movement {})",
            t.evidence.title.as_deref().unwrap(),
            i + 1
        ));
    }
    assert!(
        inspect_album_program(&l, &program(&["A", "B", "C", "D", "E", "F"]), "Album").compatible()
    );
    assert!(
        !inspect_album_program(&l, &program(&["X", "Y", "Z", "D", "E", "F"]), "Album").compatible()
    );
}

#[test]
fn systematic_local_remaster_requires_matching_album_edition() {
    use music_library::album_program::inspect_album_program;
    let l = local(&[
        "A (2015 Remaster)",
        "B (2015 Remaster)",
        "C (2015 Remaster)",
    ]);
    let p = program(&["A", "B", "C"]);
    let result = inspect_album_program(&l, &p, "Example (2015 Remaster)");
    assert!(result.alignment.strong && result.compatible());
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(result.tracks[0].local.title, l[0].evidence.title);
    assert!(!inspect_album_program(&l, &p, "Example").compatible());
    assert!(!inspect_album_program(&l, &p, "Example (2020 Remaster)").compatible());
    let mixed = local(&[
        "A (2015 Remaster)",
        "B (2020 Remaster)",
        "C (2015 Remaster)",
    ]);
    assert!(
        !inspect_album_program(&mixed, &p, "Example (2015 Remaster) (2020 Remaster)")
            .alignment
            .strong
    );
    for suffix in ["Live", "Demo", "A Subtitle", "feat. Guest"] {
        let names = [
            format!("A ({suffix})"),
            format!("B ({suffix})"),
            format!("C ({suffix})"),
        ];
        let l = local(&names.iter().map(String::as_str).collect::<Vec<_>>());
        assert!(
            !inspect_album_program(&l, &p, &format!("Example ({suffix})"))
                .alignment
                .strong
        );
    }
}

#[test]
fn systematic_short_prefix_requires_unique_program_and_consistency() {
    use music_library::album_program::inspect_album_program;
    let l = local(&["字 A", "字 B", "字 C"]);
    let p = program(&["A", "B", "C"]);
    let result = inspect_album_program(&l, &p, "Established Album");
    assert!(result.alignment.strong && result.compatible());
    assert!(result.warnings[0].contains("字"));
    for names in [
        ["鏡子 A", "中途 B", "春雨 C"],
        ["Unrelated A", "Unrelated B", "Unrelated C"],
        ["字 A", "字 A", "字 B"],
        ["字Meaning A", "字Meaning B", "字Meaning C"],
    ] {
        assert!(
            !inspect_album_program(&local(&names), &p, "Established Album")
                .alignment
                .strong
        );
    }
    assert!(
        !inspect_album_program(&l, &program(&["A", "B", "Unrelated"]), "Established Album")
            .alignment
            .strong
    );
}
#[test]
fn numeric_remainder_agrees_without_rewriting_lexical_one() {
    use music_library::album_program::{TitleRelation, title_relation};
    let a = program(&["On the Count of Thirteen", "One More"]);
    let b = program(&["On the Count of 13", "1 More"]);
    assert_eq!(
        title_relation(&a.tracks[0], &b.tracks[0]),
        TitleRelation::Agrees
    );
    assert_ne!(
        title_relation(&a.tracks[1], &b.tracks[1]),
        TitleRelation::Agrees
    );
}

#[test]
fn conflicting_trusted_collaborators_block_unique_title_reorder() {
    let mut l = local(&["A", "B", "C"]);
    let mut p = program(&["C", "A", "B"]);
    let artist = |name: &str, key: &str| ArtistEvidence {
        name: name.into(),
        identities: vec![ExternalIdentity {
            provider: "spotify".into(),
            kind: "artist".into(),
            external_id: key.into(),
        }],
        ..Default::default()
    };
    for t in &mut l {
        t.evidence.artists = vec![artist("Primary", "primary"), artist("Collaborator", "one")];
    }
    for t in &mut p.tracks {
        t.artists = vec![artist("Primary", "primary"), artist("Collaborator", "two")];
    }
    assert!(!align_program(&l, &p).strong);
}

#[test]
fn recorded_real_programs_exercise_general_alignment_rules() {
    #[derive(serde::Deserialize)]
    struct Song {
        title: String,
        disc: Option<u32>,
        number: Option<u32>,
        duration_ms: Option<u64>,
    }
    impl Song {
        fn evidence(&self) -> TrackEvidence {
            TrackEvidence {
                title: Some(self.title.clone()),
                disc: self.disc,
                number: self.number,
                duration_ms: self.duration_ms,
                ..Default::default()
            }
        }
    }
    #[derive(serde::Deserialize)]
    struct Provider {
        title: String,
        tracks: Vec<Song>,
    }
    #[derive(serde::Deserialize)]
    struct Fixture {
        album: String,
        local: Vec<Song>,
        cached_provider_programs: Vec<Provider>,
    }
    let fixtures: Vec<Fixture> =
        serde_json::from_str(include_str!("fixtures/spotify/robustness-programs.json")).unwrap();
    for fixture in fixtures {
        let local = fixture
            .local
            .iter()
            .enumerate()
            .map(|(i, s)| LocalTrackEvidence {
                track_id: TrackId(format!("local{i}")),
                recording_id: RecordingId(format!("recording{i}")),
                evidence: s.evidence(),
            })
            .collect::<Vec<_>>();
        for provider in fixture.cached_provider_programs {
            let program = Program {
                identity: None,
                complete: true,
                tracks: provider.tracks.iter().map(Song::evidence).collect(),
            };
            let inspected = music_library::album_program::inspect_album_program(
                &local,
                &program,
                &fixture.album,
            );
            match fixture.album.as_str() {
                "Animals" if provider.title == "Animals" => {
                    assert!(inspected.compatible());
                    assert_eq!(inspected.alignment.rows[0].provider_index, Some(9));
                }
                "Animals" => assert!(
                    !inspected.compatible(),
                    "studio vs Acoustic remains a conflict"
                ),
                "Animals Acoustic" | "Audiotree Live" | "Church Gone Wild/Chirpin Hard" => {
                    assert!(inspected.compatible(), "{}: {inspected:?}", fixture.album)
                }
                "Underwater" => assert!(!inspected.compatible()),
                "Critical Meat" => assert_eq!(inspected.anchors, 8),
                "Dreams" if provider.title == "Dreams" => assert!(inspected.compatible()),
                "Freshman Year" => {
                    assert!(inspected.alignment.strong);
                    assert_eq!(inspected.alignment.rows[14].provider_index, Some(13));
                    assert_eq!(inspected.alignment.rows[15].provider_index, Some(14));
                }
                _ => {}
            }
        }
    }
}
