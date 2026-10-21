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
fn punctuation_placeholders_need_independent_program_and_position_evidence() {
    use music_library::album_program::{
        TitleRelation, inspect_album_program, punctuation_placeholder, title_relation,
    };
    let mut l = local(&["_", "Second", "Third", "Fourth"]);
    let mut p = program(&["—", "Second", "Third", "Fourth"]);
    let artist = ArtistEvidence {
        name: "Band".into(),
        identities: vec![ExternalIdentity {
            provider: "spotify".into(),
            kind: "artist".into(),
            external_id: "band".into(),
        }],
        join_phrase: String::new(),
    };
    for t in &mut l {
        t.evidence.artists = vec![artist.clone()];
    }
    for t in &mut p.tracks {
        t.artists = vec![artist.clone()];
    }
    assert_ne!(
        title_relation(&l[0].evidence, &p.tracks[0]),
        TitleRelation::Agrees
    );
    let result = inspect_album_program(&l, &p, "Album");
    assert!(result.compatible(), "{result:?}");
    assert_eq!(result.alignment.rows[0].provider_index, Some(0));
    assert_eq!(l[0].evidence.title.as_deref(), Some("_"));
    for s in ["_", "-", "…", "＊＊＊", "／"] {
        assert!(punctuation_placeholder(s), "{s}");
    }
    for s in ["A_", "水", "〸", "\u{202e}", "\u{0301}"] {
        assert!(!punctuation_placeholder(s), "{s}");
    }
    assert!(
        !inspect_album_program(&local(&["_", "***"]), &program(&["-", "…"]), "Album")
            .alignment
            .strong
    );
    let mut ambiguous = program(&["-", "…", "Third", "Fourth"]);
    ambiguous.tracks[1].number = Some(1);
    assert!(
        !inspect_album_program(
            &local(&["_", "***", "Third", "Fourth"]),
            &ambiguous,
            "Album"
        )
        .compatible()
    );
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

fn credited_local(names: &[&str]) -> Vec<LocalTrackEvidence> {
    let mut rows = local(names);
    for t in &mut rows {
        t.evidence.artists = vec![ArtistEvidence {
            name: "Band".into(),
            identities: vec![ExternalIdentity {
                provider: "spotify".into(),
                kind: "artist".into(),
                external_id: "band".into(),
            }],
            join_phrase: String::new(),
        }];
        t.evidence.duration_ms = Some(200000);
    }
    rows
}
fn credited_program(names: &[&str]) -> Program {
    Program {
        tracks: credited_local(names)
            .into_iter()
            .map(|t| t.evidence)
            .collect(),
        ..program(names)
    }
}
#[test]
fn every_script_uses_the_ordinary_full_title_path() {
    use music_library::album_program::{TitleRelation, inspect_album_program, title_relation};
    for title in ["水中漫步", "夜の海", "수영", "Привет", "مرحبا"] {
        let p = credited_program(&[title, "Other", "Third"]);
        assert_eq!(
            title_relation(&p.tracks[0], &p.tracks[0]),
            TitleRelation::Agrees
        );
        let result =
            inspect_album_program(&credited_local(&[title, "Other", "Third"]), &p, "Album");
        assert!(result.alignment.strong);
        assert!(!result.warnings.iter().any(|w| w.contains("shared title")));
    }
}
#[test]
fn shared_title_spans_are_language_neutral_contextual_unique_and_preserve_raw_data() {
    use music_library::album_program::inspect_album_program;
    let provider = ["Underwater", "Moonset", "Lights", "Night Time"];
    for names in [
        [
            "水中漫步 Underwater",
            "月落 Moonset",
            "燈火 Lights",
            "夜晚的竹北 Night Time",
        ],
        [
            "Underwater 水中漫步",
            "Moonset 月落",
            "Lights 燈火",
            "Night Time 夜晚的竹北",
        ],
        [
            "Underwater（第一章 夜晚）",
            "Moonset / 月落",
            "燈火 / Lights",
            "第一章 夜晚 Night Time",
        ],
    ] {
        let local = credited_local(&names);
        let p = credited_program(&provider);
        let result = inspect_album_program(&local, &p, "Album");
        assert!(
            result.alignment.strong && result.compatible(),
            "{names:?}: {result:?}"
        );
        for (row, raw) in result.tracks.iter().zip(names) {
            assert_eq!(row.local.title.as_deref(), Some(raw));
        }
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("shared title span")
                    || w.contains("exact alternate-title component"))
        );
    }
    let names = [
        "水中漫步 Underwater",
        "夜の海 Moonset",
        "수영 Lights",
        "Привет Night Time",
    ];
    let p = credited_program(&["水中漫步", "夜の海", "수영", "Привет"]);
    assert!(
        inspect_album_program(&credited_local(&names), &p, "Album")
            .alignment
            .strong
    );
}
#[test]
fn shared_title_spans_reject_substrings_versions_weak_context_and_ambiguity() {
    use music_library::album_program::inspect_album_program;
    let p = credited_program(&["Underwater", "Moonset", "Lights", "Night Time"]);
    for names in [
        ["NotUnderwater", "NotMoonset", "NotLights", "NotNight Time"],
        [
            "Underwater Live 夜晚",
            "Moonset Demo 月落",
            "Lights Remix 燈火",
            "Night Time Acoustic 夜晚",
        ],
    ] {
        assert!(
            !inspect_album_program(&credited_local(&names), &p, "Album")
                .alignment
                .strong
        );
    }
    assert!(
        !inspect_album_program(
            &credited_local(&["水中漫步 Underwater"]),
            &credited_program(&["Underwater"]),
            "Album"
        )
        .alignment
        .strong
    );
    let result = inspect_album_program(
        &credited_local(&["Intro 第一章", "Intro 第二章", "Other"]),
        &credited_program(&["Intro", "Intro", "Other"]),
        "Album",
    );
    assert!(!result.alignment.strong);
    // Exact correspondence wins before containment is considered.
    let result = inspect_album_program(
        &credited_local(&["水中漫步", "Other", "Third"]),
        &credited_program(&["水中漫步", "水中漫步 Underwater", "Other", "Third"]),
        "Album",
    );
    assert_eq!(
        result.tracks[0].provider.as_ref().unwrap().title.as_deref(),
        Some("水中漫步")
    );
    assert!(
        !result
            .warnings
            .iter()
            .any(|w| w.contains("shared title span"))
    );
}
#[test]
fn compatible_playback_families_require_strong_ordered_programs() {
    use music_library::album_program::compatible_provider_programs;
    let a = credited_program(&["First", "Second", "Third", "Fourth", "Fifth", "Seventh"]);
    let b = credited_program(&["First", "Second", "Third", "Fourth", "Sixth", "Seventh"]);
    // Contradictory songs occupying the same slot block pooling.
    assert!(!compatible_provider_programs(&a, &b));
    let mut b = a.clone();
    b.tracks.remove(2);
    assert!(compatible_provider_programs(&a, &b));
    let mut c = a.clone();
    c.tracks.remove(1);
    assert!(compatible_provider_programs(&b, &c));
    for mutation in 0..4 {
        let mut b = a.clone();
        match mutation {
            0 => b.tracks.reverse(),
            1 => b.tracks[0].title = Some("First Live".into()),
            2 => b.tracks[0].artists[0].identities[0].external_id = "other".into(),
            _ => b.tracks[0].duration_ms = Some(230000),
        }
        assert!(!compatible_provider_programs(&a, &b));
    }
}

#[test]
fn omitted_apostrophe_requires_position_duration_and_strong_playback_context() {
    use music_library::album_program::{inspect_album_program, inspect_playable_program};
    let local = credited_local(&["First", "Second", "A Truckers Wife", "Fourth"]);
    let p = credited_program(&["First", "Second", "A Trucker's Wife", "Fourth"]);
    let playback = inspect_playable_program(&local, &p, "Album");
    assert!(playback.alignment.strong && playback.compatible());
    assert!(
        playback
            .warnings
            .iter()
            .any(|w| w.contains("apostrophe typography"))
    );
    assert_eq!(
        playback.tracks[2].local.title.as_deref(),
        Some("A Truckers Wife")
    );
    assert!(
        !inspect_album_program(&local, &p, "Album")
            .warnings
            .iter()
            .any(|w| w.contains("apostrophe typography"))
    );
    let mut changed = p.clone();
    changed.tracks[2].duration_ms = Some(240000);
    assert!(
        !inspect_playable_program(&local, &changed, "Album")
            .warnings
            .iter()
            .any(|w| w.contains("apostrophe typography"))
    );
    assert!(
        !inspect_playable_program(
            &credited_local(&["A Truckers Wife"]),
            &credited_program(&["A Trucker's Wife"]),
            "Album"
        )
        .warnings
        .iter()
        .any(|w| w.contains("apostrophe typography"))
    );
}

fn strong_context(names: &[&str], remote: &[&str]) -> (Vec<LocalTrackEvidence>, Program) {
    let mut l = local(names);
    let mut p = program(remote);
    let artist = ArtistEvidence {
        name: "Band".into(),
        identities: vec![ExternalIdentity {
            provider: "spotify".into(),
            kind: "artist".into(),
            external_id: "band".into(),
        }],
        join_phrase: String::new(),
    };
    for t in &mut l {
        t.evidence.artists = vec![artist.clone()];
        t.evidence.duration_ms = Some(100_000);
    }
    for t in &mut p.tracks {
        t.artists = vec![artist.clone()];
        t.duration_ms = Some(100_000);
    }
    (l, p)
}
#[test]
fn contextual_title_policy_recovers_unique_program_occurrences() {
    use music_library::{
        album_program::inspect_playable_program,
        matching::{ContextualTitleClass as C, contextual_title_class},
    };
    for (a, b, class) in [
        ("Guy Vs. D.C. Sniper", "Guy vs DC Sniper", C::Punctuation),
        (
            "Soundtrack to Insecurity",
            "Sound Track to Insecurity",
            C::TokenBoundary,
        ),
        ("Mini Rockers", "Minirockers", C::TokenBoundary),
        ("fifzteen", "Fiftzeen", C::TinyTypo),
        ("Dodr", "Dotr", C::TinyTypo),
        (
            "Gaze at Blue",
            "Gaze at Blue (Album Version)",
            C::AlbumVersion,
        ),
    ] {
        assert_eq!(contextual_title_class(a, b), Some(class));
        let (l, p) = strong_context(
            &["First", "Second", "Third", "Fourth", a],
            &["First", "Second", "Third", "Fourth", b],
        );
        let result = inspect_playable_program(&l, &p, "Album");
        assert!(result.compatible(), "{a} {b}: {result:?}");
        assert_eq!(
            result.tracks[4].provider.as_ref().unwrap().title.as_deref(),
            Some(b)
        );
        assert!(
            result.warnings.iter().any(|w| w.contains(class.label())),
            "{result:?}"
        );
        assert_eq!(l[4].evidence.title.as_deref(), Some(a));
    }
}
#[test]
fn contextual_titles_cannot_establish_identity_or_resolve_competition() {
    use music_library::album_program::inspect_playable_program;
    for (a, b) in [
        ("A B", "AB"),
        ("Cat", "Bat"),
        ("Song", "Song (Live)"),
        ("Song", "Song (Acoustic)"),
        ("Song", "Song (Demo)"),
    ] {
        let (l, p) = strong_context(
            &["First", "Second", "Third", "Fourth", a],
            &["First", "Second", "Third", "Fourth", b],
        );
        assert!(
            !matches!(
                inspect_playable_program(&l, &p, "Album").tracks[4].decision,
                music_library::album_program::PositionDecision::ExactAnchor
            ),
            "{a} {b}"
        );
    }
    let (l, p) = strong_context(
        &[
            "Dodr", "Dotx", "First", "Second", "Third", "Fourth", "Fifth", "Sixth", "Seventh",
            "Eighth",
        ],
        &[
            "Dotr", "Other", "First", "Second", "Third", "Fourth", "Fifth", "Sixth", "Seventh",
            "Eighth",
        ],
    );
    assert!(
        !inspect_playable_program(&l, &p, "Album")
            .warnings
            .iter()
            .any(|w| w.contains("tiny typo"))
    );
    let (mut l, p) = strong_context(
        &["Dodr", "First", "Second", "Third", "Fourth"],
        &["Dotr", "Other A", "Other B", "Other C", "Other D"],
    );
    assert!(!inspect_playable_program(&l, &p, "Album").alignment.strong);
    for t in &mut l {
        t.evidence.artists[0].identities.clear();
    }
    let (_, p) = strong_context(&[], &["Dotr", "First", "Second", "Third", "Fourth"]);
    assert!(
        !inspect_playable_program(&l, &p, "Album")
            .warnings
            .iter()
            .any(|w| w.contains("tiny typo"))
    );
}
#[test]
fn literal_multiscript_search_preserves_punctuation_without_translation() {
    use music_library::matching::literal_search_component;
    assert_eq!(
        literal_search_component("日本語 / WAO!!").as_deref(),
        Some("WAO!!")
    );
    assert!(literal_search_component("日本語").is_none());
    assert!(literal_search_component("WAO!!").is_none());
}

#[test]
fn retained_occurrences_corroborate_playback_editions_without_replacing_identity() {
    use music_library::album_program::{inspect_album_program, inspect_playable_program};
    let (mut l, mut p) = strong_context(
        &["First", "Second", "Third", "Fourth", "Mini Rockers"],
        &["First", "Second", "Third", "Fourth", "Minirockers"],
    );
    for (i, t) in l.iter_mut().take(4).enumerate() {
        t.evidence.identities.push(ExternalIdentity {
            provider: "spotify".into(),
            kind: "track".into(),
            external_id: format!("old{i}"),
        });
        p.tracks[i].identities.push(ExternalIdentity {
            provider: "spotify".into(),
            kind: "track".into(),
            external_id: format!("alternate{i}"),
        });
    }
    let retained = l.clone();
    assert!(!inspect_album_program(&l, &p, "Album").compatible());
    let playable = inspect_playable_program(&l, &p, "Album");
    assert!(playable.compatible(), "{playable:?}");
    assert_eq!(l, retained);
    assert_eq!(
        playable.tracks[0].local.identities,
        l[0].evidence.identities
    );
    l[4].evidence.recording.identities.push(ExternalIdentity {
        provider: "musicbrainz".into(),
        kind: "recording".into(),
        external_id: "local".into(),
    });
    p.tracks[4].recording.identities.push(ExternalIdentity {
        provider: "musicbrainz".into(),
        kind: "recording".into(),
        external_id: "different".into(),
    });
    assert!(!inspect_playable_program(&l, &p, "Album").compatible());
}
#[test]
fn punctuation_uses_existing_playback_duration_tolerance_but_typos_remain_bounded() {
    use music_library::album_program::inspect_playable_program;
    for (a, b, allowed) in [
        ("Guy Vs. D.C. Sniper", "Guy vs DC Sniper", true),
        ("Fifteen", "Fiften", false),
    ] {
        let (l, mut p) = strong_context(
            &["First", "Second", "Third", "Fourth", a],
            &["First", "Second", "Third", "Fourth", b],
        );
        p.tracks[4].duration_ms = Some(103_093);
        let result = inspect_playable_program(&l, &p, "Album");
        assert_eq!(
            result.warnings.iter().any(|w| w.contains("contextual")),
            allowed,
            "{result:?}"
        );
    }
}

#[test]
fn existing_shared_span_and_apostrophe_anchors_amplify_token_boundary_evidence() {
    use music_library::album_program::inspect_playable_program;
    let (l, p) = strong_context(
        &[
            "First",
            "Second",
            "Third",
            "Fourth",
            "Fifth",
            "Sixth",
            "Seventh",
            "No One’s Looking",
            "Hand Rocks",
            "Soundtrack to Insecurity",
            "Eleventh",
        ],
        &[
            "First",
            "Second",
            "Third",
            "Fourth",
            "Fifth",
            "Sixth",
            "Seventh",
            "No Ones Looking",
            "The Hand Rocks",
            "Sound Track to Insecurity",
            "Eleventh",
        ],
    );
    let result = inspect_playable_program(&l, &p, "Album");
    assert!(result.compatible(), "{result:?}");
    assert_eq!(result.anchors, 11);
    assert!(result.warnings.iter().any(|w| w.contains("token-boundary")));
}
