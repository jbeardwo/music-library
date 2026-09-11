use music_library::{album_matching::*, catalog::*, domain::ExternalIdentity};
fn id(kind: &str, value: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: "musicbrainz".into(),
        kind: kind.into(),
        external_id: value.into(),
    }
}
fn artist(key: &str, name: &str, aliases: &[&str]) -> ArtistCandidate {
    ArtistCandidate {
        identity: id("artist", key),
        name: name.into(),
        aliases: aliases.iter().map(|s| s.to_string()).collect(),
        comment: String::new(),
        country: String::new(),
        artist_type: String::new(),
        score: Some(100),
    }
}
fn page<T>(items: Vec<T>) -> Page<T> {
    Page {
        items,
        next_offset: None,
    }
}
fn album(key: &str, title: &str, kind: &str) -> ArtistAlbumCandidate {
    ArtistAlbumCandidate {
        artist: "Artist".into(),
        identity: id("release_group", key),
        title: title.into(),
        primary_type: kind.into(),
        artist_ids: vec![id("artist", "a")],
        comment: String::new(),
        date: String::new(),
    }
}
#[test]
fn exact_primary_alias_and_competitor_veto() {
    let local = "The Speed of Sound in Seawater";
    let alias = artist("a", "tsosis", &[local]);
    assert_eq!(
        resolve_artist(local, &page(vec![alias.clone()])).unwrap(),
        id("artist", "a")
    );
    assert_eq!(
        resolve_artist(" TABAR ", &page(vec![artist("a", "Tabar", &[])])).unwrap(),
        id("artist", "a")
    );
    for competitor in [
        artist("b", local, &[]),
        artist("b", "Another", &[local]),
        artist("b", "The Speed of Sound in Seawaters", &[]),
        artist("b", "Other", &["The Speed of Sound in Seawaters"]),
    ] {
        assert!(
            matches!(resolve_artist(local,&page(vec![alias.clone(),competitor])),Err(MatchOutcome::ArtistAmbiguous(v)) if v.len()==2)
        );
    }
    assert_eq!(
        resolve_artist(
            local,
            &page(vec![artist("b", "The Speed of Sound in Seawaters", &[])])
        ),
        Err(MatchOutcome::NoConfidentMatch)
    );
    // Primary equality is stronger than merely close competition (never a positive fuzzy match).
    assert_eq!(
        resolve_artist(
            "Tabar",
            &page(vec![artist("a", "Tabar", &[]), artist("b", "Tabars", &[])])
        )
        .unwrap(),
        id("artist", "a")
    );
    assert!(matches!(
        resolve_artist(
            "ABC",
            &page(vec![
                artist("a", "Canonical", &["ABC"]),
                artist("b", "ABD", &[])
            ])
        ),
        Err(MatchOutcome::ArtistAmbiguous(_))
    ));
    let mut incomplete = page(vec![alias]);
    incomplete.next_offset = Some(10);
    assert!(matches!(
        resolve_artist(local, &incomplete),
        Err(MatchOutcome::ArtistAmbiguous(_))
    ));
}
#[test]
fn suffixes_require_boundaries_and_ep_type_and_never_rewrite_titles() {
    for suffix in [
        "EP", "(EP)", "[EP]", "- EP", "CD", "CD1", "CD 1", "CD2", "CD 2", "Disc 1", "Disc 2",
        "2CD", "2xCD",
    ] {
        let title = format!("Hugs {suffix}");
        let original = title.clone();
        assert_eq!(
            accepted_album(
                &title,
                &id("artist", "a"),
                &page(vec![album("g", "Hugs", "EP")])
            ),
            MatchOutcome::MatchedClose(id("release_group", "g")),
            "{title}"
        );
        assert_eq!(title, original);
    }
    for kind in ["Album", ""] {
        assert_eq!(
            accepted_album(
                "Hugs EP",
                &id("artist", "a"),
                &page(vec![album("g", "Hugs", kind)])
            ),
            MatchOutcome::NoConfidentMatch
        );
    }
    for title in [
        "EP Hugs",
        "HugsEP",
        "Hugs EP Again",
        "Hugs Live",
        "Hugs Remix",
        "Hugs Remixed",
        "Hugs Acoustic",
        "Hugs Deluxe",
        "Hugs Remaster",
        "Hugs Remastered",
        "Hugs Demo",
        "Hugs Edit",
    ] {
        assert!(album_title_variants(title).is_empty(), "{title}");
        assert_eq!(
            accepted_album_confirmed(
                title,
                &id("artist", "a"),
                &page(vec![album("g", "Hugs", "EP")]),
                true
            ),
            MatchOutcome::NoConfidentMatch
        );
    }
    assert_eq!(
        accepted_album(
            "Hugs EP",
            &id("artist", "unknown"),
            &page(vec![album("g", "Hugs", "EP")])
        ),
        MatchOutcome::NoConfidentMatch
    );
}
#[test]
fn exact_then_structural_then_tiny_edits_and_ambiguity() {
    let artist = id("artist", "a");
    let p = page(vec![
        album("exact", "Hugs EP", "EP"),
        album("variant", "Hugs", "EP"),
    ]);
    assert_eq!(
        accepted_album("Hugs EP", &artist, &p),
        MatchOutcome::Matched(id("release_group", "exact"))
    );
    let p = page(vec![album("one", "Hugs", "EP"), album("two", "Hugs", "EP")]);
    assert!(matches!(
        accepted_album("Hugs EP", &artist, &p),
        MatchOutcome::AlbumAmbiguous(_)
    ));
    let p = page(vec![album("one", "Acoustics", "Album")]);
    assert!(matches!(
        accepted_album("Acoustic", &artist, &p),
        MatchOutcome::MatchedClose(_)
    ));
    let p = page(vec![album("one", "Nevermind", "Album")]);
    assert_eq!(
        accepted_album("Nevermimd!", &artist, &p),
        MatchOutcome::NoConfidentMatch
    );
    assert!(matches!(
        accepted_album_confirmed("Nevermimd!", &artist, &p, true),
        MatchOutcome::MatchedClose(_)
    ));
    let p = page(vec![
        album("one", "Nevermind", "Album"),
        album("two", "Nevermind!", "Album"),
    ]);
    // Both within the manual threshold: no choosing the nearest score/distance.
    assert!(matches!(
        accepted_album_confirmed("Nevermimd!", &artist, &p, true),
        MatchOutcome::AlbumAmbiguous(_)
    ));
    assert!(!close_album_title("Four", "Fours"));
    assert!(!close_album_title("Album Live", "Album Love"));
}

fn owned_album(owner: &str, key: &str, title: &str, kind: &str) -> ArtistAlbumCandidate {
    let mut result = album(key, title, kind);
    result.artist_ids = vec![id("artist", owner)];
    result
}

#[test]
fn literal_ep_lp_titles_precede_type_checked_fallback() {
    for (title, base, kind) in [
        ("Floral EP", "Floral", "EP"),
        ("Floral LP", "Floral", "Album"),
        ("Hugs EP", "Hugs", "EP"),
    ] {
        let original = title.to_string();
        let exact = album("exact", title, kind);
        let fallback = album("fallback", base, kind);
        assert_eq!(
            accepted_album(
                title,
                &id("artist", "a"),
                &page(vec![fallback.clone(), exact])
            ),
            MatchOutcome::Matched(id("release_group", "exact"))
        );
        assert_eq!(
            accepted_album(title, &id("artist", "a"), &page(vec![fallback])),
            MatchOutcome::MatchedClose(id("release_group", "fallback"))
        );
        assert_eq!(title, original);
    }
    assert_eq!(
        accepted_album(
            "Floral LP",
            &id("artist", "a"),
            &page(vec![album("wrong-type", "Floral", "EP")])
        ),
        MatchOutcome::NoConfidentMatch
    );
}

#[test]
fn same_name_hella_artists_are_resolved_only_by_unique_album_ownership() {
    let title = "Bitches Ain't Shit but Good People";
    let candidates = vec![artist("other", "Hella", &[]), artist("band", "Hella", &[])];
    let owned = owned_album("band", "record", title, "Album");
    assert_eq!(
        corroborate_artist_album("Hella", title, &candidates, &page(vec![owned.clone()])),
        (
            Some(id("artist", "band")),
            MatchOutcome::Matched(id("release_group", "record"))
        )
    );
    assert!(matches!(
        corroborate_artist_album(
            "Hella",
            title,
            &candidates,
            &page(vec![
                owned,
                owned_album("other", "competing", title, "Album")
            ])
        ),
        (None, MatchOutcome::ArtistAmbiguous(_))
    ));
}
#[test]
fn floral_pair_is_order_independent_and_cannot_invent_artist_candidates() {
    let original = vec![
        artist("kid", "Kid Floral", &["Floral"]),
        artist("band", "Floral", &[]),
    ];
    for candidates in [original.clone(), original.iter().rev().cloned().collect()] {
        let groups = page(vec![
            owned_album("band", "lp", "Floral LP", "Album"),
            owned_album("outsider", "other", "Floral LP", "Album"),
        ]);
        assert_eq!(
            corroborate_artist_album("Floral", "Floral LP", &candidates, &groups),
            (
                Some(id("artist", "band")),
                MatchOutcome::Matched(id("release_group", "lp"))
            )
        );
    }
    let groups = page(vec![owned_album("outsider", "other", "Floral LP", "Album")]);
    assert!(matches!(
        corroborate_artist_album("Floral", "Floral LP", &original, &groups),
        (None, MatchOutcome::ArtistAmbiguous(_))
    ));
    let near = vec![
        artist("near", "Flora", &[]),
        artist("alias", "Other", &["Floral"]),
    ];
    let groups = page(vec![owned_album("near", "lp", "Floral LP", "Album")]);
    assert!(matches!(
        corroborate_artist_album("Floral", "Floral LP", &near, &groups),
        (None, MatchOutcome::ArtistAmbiguous(_))
    ));
}
#[test]
fn pair_ambiguity_includes_competing_artists_and_ambiguous_albums() {
    let candidates = vec![
        artist("a", "Floral", &[]),
        artist("b", "Floral", &[]),
        artist("c", "Floral", &[]),
    ];
    for groups in [
        vec![],
        vec![
            owned_album("a", "one", "Floral LP", "Album"),
            owned_album("b", "two", "Floral LPs", "Album"),
        ],
        vec![
            owned_album("a", "one", "Floral LP", "Album"),
            owned_album("a", "two", "Floral LP", "Album"),
        ],
    ] {
        assert!(matches!(
            corroborate_artist_album("Floral", "Floral LP", &candidates, &page(groups)),
            (None, MatchOutcome::ArtistAmbiguous(_))
        ));
    }
    let groups = page(vec![
        owned_album("b", "one", "Floral LP", "Album"),
        owned_album("b", "two", "Floral LP", "Album"),
    ]);
    let (_, MatchOutcome::ArtistAmbiguous(ordered)) =
        corroborate_artist_album("Floral", "Floral LP", &candidates, &groups)
    else {
        panic!()
    };
    assert_eq!(ordered[0].identity, id("artist", "b")); // Helpful order, still no accepted identity.
    let mut incomplete = page(vec![owned_album("a", "one", "Floral LP", "Album")]);
    incomplete.next_offset = Some(10);
    assert!(matches!(
        corroborate_artist_album("Floral", "Floral LP", &candidates, &incomplete),
        (None, MatchOutcome::ArtistAmbiguous(_))
    ));
}
#[test]
fn pair_comparison_uses_normal_album_rules_not_manual_tolerance() {
    let candidates = vec![artist("a", "Tabar", &[]), artist("b", "Tabar", &[])];
    assert!(matches!(
        corroborate_artist_album(
            "Tabar",
            "Hugs EP",
            &candidates,
            &page(vec![owned_album("a", "ep", "Hugs", "EP")])
        ),
        (Some(_), MatchOutcome::MatchedClose(_))
    ));
    for (local, title, kind) in [
        ("Hugs EP", "Hugs", "Album"),
        ("Hugs Live", "Hugs", "Album"),
        ("Nevermimd!", "Nevermind", "Album"),
    ] {
        assert!(matches!(
            corroborate_artist_album(
                "Tabar",
                local,
                &candidates,
                &page(vec![owned_album("a", "g", title, kind)])
            ),
            (None, MatchOutcome::ArtistAmbiguous(_))
        ));
    }
}
