use music_library::{
    Library,
    catalog::{Album, Credit, Medium, Release, Track},
    domain::ExternalIdentity,
    song_resolution::{Candidate, Selection},
};
fn id(provider: &str, kind: &str, value: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: provider.into(),
        kind: kind.into(),
        external_id: value.into(),
    }
}
fn release() -> Release {
    let credits = vec![Credit {
        identity: Some(id("musicbrainz", "artist", "artist")),
        name: "Artist".into(),
        join_phrase: String::new(),
    }];
    Release {
        album: Album {
            release_type: None,
            identity: id("musicbrainz", "release_group", "group"),
            title: "Album".into(),
            date: "2000".into(),
            credits: credits.clone(),
        },
        identity: id("musicbrainz", "release", "edition"),
        identities: vec![],
        title: "Album".into(),
        date: "2000".into(),
        credits: credits.clone(),
        media: vec![Medium {
            position: 1,
            tracks: vec![Track {
                duration: None,
                position: 1,
                title: "Song".into(),
                credits,
                identities: vec![
                    id("musicbrainz", "track", "occurrence"),
                    id("musicbrainz", "recording", "recording"),
                ],
            }],
        }],
    }
}
fn candidate() -> Candidate {
    Candidate {
        recording: Default::default(),
        album_identity: None,
        album_artists: vec![],
        album_type: String::new(),
        album_total_tracks: None,
        identity: id("song-provider", "song", "opaque-song"),
        title: "Song".into(),
        artist: "Artist".into(),
        artists: vec![],
        album: "Other release of Album".into(),
        date: "2001".into(),
        duration_ms: 200000,
        disc: 1,
        number: 1,
    }
}

#[test]
fn persisted_manual_identity_is_authoritative_in_review_without_relaxing_matching() {
    use music_library::{
        catalog::Page,
        song_resolution::{Assessment, FinalDecision, assess, evaluate},
    };
    let mut library = Library::open_in_memory().unwrap();
    let r = library.add_catalog_release(&release()).unwrap();
    let mut known = candidate();
    known.title = "Explicitly confirmed different title".into();
    library
        .attach_track_external_identity(&r.track_ids[0], &known.identity)
        .unwrap();
    let input = library.song_resolution_input(&r.track_ids[0]).unwrap();
    let page = Page {
        items: vec![known.clone()],
        next_offset: None,
    };
    assert_ne!(assess(&input, &page), Assessment::Unique(0));
    let evaluation = evaluate(&input, &page, 0, std::slice::from_ref(&known.identity)).unwrap();
    assert_eq!(evaluation.final_decision, FinalDecision::Accept);
    assert!(evaluation.association_persisted && evaluation.primary_blocker.is_none());
    assert!(!evaluation.warnings.is_empty());
    let mut alternative = known.clone();
    alternative.identity.external_id = "another occurrence".into();
    alternative.title = input.title.clone();
    let mut selection = Selection::new(input, vec![alternative, known.clone()]);
    selection.show_all();
    selection.set_trusted_identities(vec![known.identity]);
    assert_eq!(selection.visible_indices()[0], 1);
}

#[test]
fn automatic_play_acceptance_is_unique_complete_and_contradiction_sensitive() {
    use music_library::{
        catalog::Page,
        song_resolution::{Assessment, assess},
    };
    let mut library = Library::open_in_memory().unwrap();
    let imported = library.add_catalog_release(&release()).unwrap();
    let mut input = library
        .song_resolution_input(&imported.track_ids[0])
        .unwrap();
    input.duration_ms = Some(200000);
    let mut song = candidate();
    song.album = input.album.clone();
    let page = |items| Page {
        items,
        next_offset: None,
    };
    assert_eq!(
        assess(&input, &page(vec![song.clone()])),
        Assessment::Unique(0)
    );
    let mut other = song.clone();
    other.identity.external_id = "another catalog song".into();
    assert_eq!(
        assess(&input, &page(vec![other.clone(), song.clone()])),
        Assessment::NeedsSelection
    );
    other.title = "unrelated first-ranked result".into();
    assert_eq!(
        assess(&input, &page(vec![other, song.clone()])),
        Assessment::Unique(1)
    );
    for field in ["artist", "title", "album", "duration", "disc", "position"] {
        let mut wrong = song.clone();
        match field {
            "artist" => wrong.artist = "Wrong Artist".into(),
            "title" => wrong.title.push_str(" (Live)"),
            "album" => wrong.album.push_str(" Deluxe"),
            "duration" => wrong.duration_ms += 3001,
            "disc" => {
                input.disc = Some(1);
                wrong.disc = 2;
            }
            _ => wrong.number = 2,
        }
        assert_ne!(
            assess(&input, &page(vec![wrong])),
            Assessment::Unique(0),
            "{field}"
        );
    }
    assert_eq!(assess(&input, &page(vec![])), Assessment::NoMatch);
    assert_eq!(
        assess(
            &input,
            &Page {
                items: vec![song.clone()],
                next_offset: Some(10)
            }
        ),
        Assessment::NeedsSelection
    );
    song.title = "  SONG  ".into();
    song.artist = "ARTIST".into();
    assert_eq!(assess(&input, &page(vec![song])), Assessment::Unique(0));
}

#[test]
fn automatic_unique_song_persists_without_other_identity_changes() {
    use music_library::{
        catalog::Page,
        song_resolution::{Assessment, assess},
    };
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("automatic.sqlite");
    let mut library = Library::open(&path).unwrap();
    let imported = library.add_catalog_release(&release()).unwrap();
    let track = &imported.track_ids[0];
    let before = library.edition_evidence(&imported.release_id).unwrap();
    let input = library.song_resolution_input(track).unwrap();
    let mut song = candidate();
    song.album = input.album.clone();
    let page = Page {
        items: vec![song.clone()],
        next_offset: None,
    };
    let Assessment::Unique(index) = assess(&input, &page) else {
        panic!("unique")
    };
    library
        .confirm_song_resolution(&Selection::new(input, page.items), index)
        .unwrap();
    let after = library.edition_evidence(&imported.release_id).unwrap();
    assert_eq!(before.exact_identities, after.exact_identities);
    assert_eq!(before.tracks[0].recording_id, after.tracks[0].recording_id);
    drop(library);
    let library = Library::open(&path).unwrap();
    assert_eq!(
        library
            .track_provider_occurrences(track, "song-provider")
            .unwrap(),
        vec![song.identity]
    );
}
#[test]
fn catalog_track_explicit_selection_persists_without_album_or_recording_inference() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut library = Library::open(&path).unwrap();
    let imported = library.add_catalog_release(&release()).unwrap();
    let track = &imported.track_ids[0];
    assert!(library.available_playback_source(track).unwrap().is_none());
    assert!(
        library
            .track_provider_occurrences(track, "song-provider")
            .unwrap()
            .is_empty()
    );
    let input = library.song_resolution_input(track).unwrap();
    assert_eq!(
        (&*input.title, &*input.artist, &*input.album),
        ("Song", "Artist", "Album")
    );
    let album = library
        .album_for_release(&imported.release_id)
        .unwrap()
        .album_id;
    let before = library.edition_evidence(&imported.release_id).unwrap();
    // Preparing/abandoning a candidate list has no persistent effect.
    let selection = Selection::new(input, vec![candidate()]);
    assert!(
        library
            .track_provider_occurrences(track, "song-provider")
            .unwrap()
            .is_empty()
    );
    assert!(
        library
            .confirm_song_resolution(&selection, usize::MAX)
            .is_err()
    );
    let accepted = library.confirm_song_resolution(&selection, 0).unwrap();
    assert_eq!(
        library
            .track_provider_occurrences(track, "song-provider")
            .unwrap(),
        vec![accepted.clone()]
    );
    assert!(
        library.confirm_song_resolution(&selection, 0).is_err(),
        "existing association cannot be replaced"
    );
    assert_eq!(
        library.list_album_external_identities(&album).unwrap(),
        vec![id("musicbrainz", "release_group", "group")]
    );
    assert_eq!(
        library
            .list_release_external_identities(&imported.release_id)
            .unwrap(),
        vec![id("musicbrainz", "release", "edition")]
    );
    assert_eq!(
        library
            .list_recording_external_identities(&before.tracks[0].recording_id)
            .unwrap(),
        vec![id("musicbrainz", "recording", "recording")]
    );
    drop(library);
    let library = Library::open(&path).unwrap();
    assert_eq!(
        library
            .track_provider_occurrences(track, "song-provider")
            .unwrap(),
        vec![accepted]
    );
    assert_eq!(
        library
            .edition_evidence(&imported.release_id)
            .unwrap()
            .tracks
            .len(),
        1
    );
    assert!(library.available_playback_source(track).unwrap().is_none());
}
#[test]
fn stale_metadata_and_existing_provider_associations_are_not_overwritten() {
    let mut library = Library::open_in_memory().unwrap();
    let imported = library.add_catalog_release(&release()).unwrap();
    let track = &imported.track_ids[0];
    let selection = Selection::new(
        library.song_resolution_input(track).unwrap(),
        vec![candidate()],
    );
    library
        .set_track_title_override(track, "User changed title")
        .unwrap();
    assert!(library.confirm_song_resolution(&selection, 0).is_err());
    let selection = Selection::new(
        library.song_resolution_input(track).unwrap(),
        vec![candidate()],
    );
    library
        .attach_track_external_identity(track, &id("song-provider", "song", "existing"))
        .unwrap();
    assert!(library.confirm_song_resolution(&selection, 0).is_err());
    assert_eq!(
        library
            .track_provider_occurrences(track, "song-provider")
            .unwrap(),
        vec![id("song-provider", "song", "existing")]
    );
}

#[test]
fn structured_primary_search_and_conservative_featured_acceptance_preserve_display() {
    use music_library::{
        catalog::Page,
        edition::ArtistEvidence,
        song_resolution::{Assessment, assess},
    };
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let mut lib = Library::open(&path).unwrap();
    let mut r = release();
    r.media[0].tracks[0].credits[0].join_phrase = " feat. ".into();
    r.media[0].tracks[0].credits.push(Credit {
        identity: Some(id("musicbrainz", "artist", "guest")),
        name: "Guest".into(),
        join_phrase: String::new(),
    });
    let imported = lib.add_catalog_release(&r).unwrap();
    let track = &imported.track_ids[0];
    let before = lib.edition_evidence(&imported.release_id).unwrap();
    let input = lib.song_resolution_input(track).unwrap();
    assert_eq!(input.artist, "Artist feat. Guest");
    assert_eq!(input.search_artist(), "Artist");
    let mut c = candidate();
    c.artist = "Artist, Guest".into();
    c.album = input.album.clone();
    c.artists = ["Artist", "Guest"]
        .into_iter()
        .map(|name| ArtistEvidence {
            name: name.into(),
            ..Default::default()
        })
        .collect();
    assert_eq!(
        assess(
            &input,
            &Page {
                items: vec![c.clone()],
                next_offset: None
            }
        ),
        Assessment::Unique(0)
    );
    let mut unrelated = c.clone();
    unrelated.artists[1].name = "Unrelated".into();
    unrelated.artist = "Artist, Unrelated".into();
    assert_eq!(
        assess(
            &input,
            &Page {
                items: vec![unrelated.clone()],
                next_offset: None
            }
        ),
        Assessment::NeedsSelection
    );
    assert_eq!(
        assess(
            &input,
            &Page {
                items: vec![c.clone(), unrelated],
                next_offset: None
            }
        ),
        Assessment::NeedsSelection
    );
    let mut wrong = c.clone();
    wrong.artists[0].name = "Wrong primary".into();
    assert_eq!(
        assess(
            &input,
            &Page {
                items: vec![wrong],
                next_offset: None
            }
        ),
        Assessment::NoMatch
    );
    let mut omitted = c.clone();
    omitted.artists.pop();
    omitted.artist = "Artist".into();
    assert_eq!(
        assess(
            &input,
            &Page {
                items: vec![omitted],
                next_offset: None
            }
        ),
        Assessment::NeedsSelection
    );
    assert_eq!(input, lib.song_resolution_input(track).unwrap());
    let accepted = lib
        .confirm_song_resolution(&Selection::new(input, vec![c]), 0)
        .unwrap();
    drop(lib);
    let lib = Library::open(&path).unwrap();
    assert_eq!(
        lib.song_resolution_input(track).unwrap().artist,
        "Artist feat. Guest"
    );
    assert_eq!(
        lib.edition_evidence(&imported.release_id).unwrap().tracks[0]
            .evidence
            .artists,
        before.tracks[0].evidence.artists
    );
    assert_eq!(
        lib.track_provider_occurrences(track, "song-provider")
            .unwrap(),
        vec![accepted.clone()]
    );
    assert_eq!(
        lib.playback_route(
            track,
            &music_library::playback_resolver::RemoteCapability {
                provider: "song-provider",
                unavailable: None,
                catalog_available: false,
                accepts: |_| true
            }
        )
        .unwrap(),
        music_library::playback_resolver::Route::Remote(accepted)
    );
}

#[test]
fn album_context_feasibility_and_explicit_override_are_separate_from_acceptance() {
    use music_library::song_resolution::{FeasibilityClass as Class, feasibility};
    let mut library = Library::open_in_memory().unwrap();
    let imported = library.add_catalog_release(&release()).unwrap();
    let mut input = library
        .song_resolution_input(&imported.track_ids[0])
        .unwrap();
    input.album_required_tracks = 15;
    input.duration_ms = Some(200000);
    let mut good = candidate();
    good.album = input.album.clone();
    good.date = "2000-05-23".into();
    good.album_total_tracks = Some(15);
    good.artists = input.artists.clone();
    good.album_artists = input.album_artists.clone();
    assert_eq!(feasibility(&input, &good).class, Class::Preferred);
    let mut later = good.clone();
    later.date = "2014-04-11".into();
    assert_eq!(feasibility(&input, &later).class, Class::Alternate);
    let mut short = good.clone();
    short.album_total_tracks = Some(4);
    short.album_type = "single".into();
    assert_eq!(feasibility(&input, &short).class, Class::Infeasible);
    let mut compilation = good.clone();
    compilation.album = "Greatest Hits".into();
    compilation.album_type = "compilation".into();
    assert_eq!(feasibility(&input, &compilation).class, Class::Infeasible);
    let mut remix = good.clone();
    remix.title = "Song - Remix".into();
    remix.duration_ms = 400000;
    assert!(
        feasibility(&input, &remix)
            .reasons
            .iter()
            .any(|r| r.contains("duration"))
    );
    for delta in [2000, 90000] {
        let mut c = good.clone();
        c.duration_ms += delta;
        assert_eq!(feasibility(&input, &c).class, Class::Preferred);
    }
    let mut position = good.clone();
    position.number = 2;
    assert_eq!(feasibility(&input, &position).class, Class::Infeasible);
    let mut title_input = input.clone();
    title_input.title = "Super Fx".into();
    let mut fxx = good.clone();
    fxx.title = "Super Fxx".into();
    assert_eq!(feasibility(&title_input, &fxx).class, Class::Infeasible);
    let single = Selection::new(input.clone(), vec![remix.clone(), good.clone()]);
    assert_eq!(single.visible_indices(), vec![1]);
    assert!(
        library
            .track_provider_occurrences(&imported.track_ids[0], "song-provider")
            .unwrap()
            .is_empty()
    );
    let mut selection = Selection::new(input, vec![remix, later, good]);
    assert_eq!(selection.visible_indices(), vec![2, 1]);
    selection.show_all();
    assert_eq!(selection.visible_indices(), vec![2, 1, 0]);
    assert!(
        library
            .track_provider_occurrences(&imported.track_ids[0], "song-provider")
            .unwrap()
            .is_empty()
    );
    // Neither one visible item nor Show-all creates an association. An explicit
    // override can confirm a hidden result using its original bounded-page index.
    let original = library
        .song_resolution_input(&imported.track_ids[0])
        .unwrap();
    let selection = Selection::new(original, selection.candidates().to_vec());
    library.confirm_song_resolution(&selection, 0).unwrap();
}

#[test]
fn album_program_context_survives_partial_membership_and_date_precision_ranks_locally() {
    use music_library::song_resolution::{FeasibilityClass as Class, classify};
    let mut r = release();
    r.media[0].tracks = (1..=15)
        .map(|n| {
            let mut t = r.media[0].tracks[0].clone();
            t.position = n;
            t.title = format!("Song {n}");
            t.identities.clear();
            t
        })
        .collect();
    let mut lib = Library::open_in_memory().unwrap();
    let imported = lib.add_catalog_release(&r).unwrap();
    for id in &imported.track_ids[1..] {
        lib.remove_from_library(id).unwrap();
    }
    let mut input = lib.song_resolution_input(&imported.track_ids[0]).unwrap();
    assert_eq!(input.album_required_tracks, 15);
    input.album_date = music_library::catalog_date::Date::parse("2000-05-23");
    let mut c = candidate();
    c.title = input.title.clone();
    c.album = input.album.clone();
    c.artists = input.artists.clone();
    c.date = "2000-05-23".into();
    let mut other = c.clone();
    other.date = "2000-01-01".into();
    assert_eq!(
        classify(&input, &[other, c])
            .iter()
            .map(|a| a.class)
            .collect::<Vec<_>>(),
        vec![Class::Alternate, Class::Preferred]
    );
}

#[test]
fn provider_metadata_enriches_playlist_only_duration_without_playback() {
    use music_library::catalog::Duration;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("duration.sqlite");
    let mut library = Library::open(&path).unwrap();
    let mut catalog = release();
    catalog.media[0].tracks[0].duration = Some(Duration {
        milliseconds: 199_000,
        approximate: true,
    });
    let playlist = library.create_playlist("Unsaved").unwrap();
    let plan = library
        .prepare_catalog_playlist_append(&playlist, &catalog, &[(1, 1), (1, 1)])
        .unwrap();
    library.apply_playlist_append(&plan, true).unwrap();
    let before = library.playlist_entries(&playlist, None, 200).unwrap();
    let track = before[0].track.as_ref().unwrap().track_id.clone();
    let input = library.song_resolution_input(&track).unwrap();
    let mut provider = candidate();
    provider.album = input.album.clone();
    library
        .confirm_song_resolution(&Selection::new(input, vec![provider]), 0)
        .unwrap();
    drop(library);
    let library = Library::open(&path).unwrap();
    let after = library.playlist_entries(&playlist, None, 200).unwrap();
    assert_eq!(
        before.iter().map(|r| &r.id).collect::<Vec<_>>(),
        after.iter().map(|r| &r.id).collect::<Vec<_>>()
    );
    for row in after {
        assert_eq!(row.track.unwrap().track_id, track);
        assert_eq!(row.duration_ms, Some(200_000));
        assert!(!row.duration_approximate);
    }
    let details = library.playlist_details(&playlist).unwrap().unwrap();
    assert_eq!(details.known_duration_ms, 400_000);
    assert_eq!(details.approximate_duration_count, 0);
    let db = rusqlite::Connection::open(path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM library_membership", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn exact_credit_dominates_extra_credit_in_either_provider_order() {
    use music_library::{
        catalog::Page,
        edition::ArtistEvidence,
        song_resolution::{Assessment, FinalDecision, assess, evaluate},
    };
    let mut l = Library::open_in_memory().unwrap();
    let r = l.add_catalog_release(&release()).unwrap();
    let input = l.song_resolution_input(&r.track_ids[0]).unwrap();
    let mut exact = candidate();
    exact.album = input.album.clone();
    exact.artists = input.artists.clone();
    exact.album_artists = input.album_artists.clone();
    exact.identity = id("spotify", "track", "exact");
    let mut plausible = exact.clone();
    plausible.identity = id("spotify", "track", "plausible");
    plausible.artists.push(ArtistEvidence {
        name: "Additional Artist".into(),
        ..Default::default()
    });
    for items in [
        vec![plausible.clone(), exact.clone()],
        vec![exact.clone(), plausible.clone()],
    ] {
        let expected = items
            .iter()
            .position(|c| c.identity == exact.identity)
            .unwrap();
        let page = Page {
            items,
            next_offset: None,
        };
        assert_eq!(assess(&input, &page), Assessment::Unique(expected));
        for index in 0..2 {
            let evaluation = evaluate(&input, &page, index, &[]).unwrap();
            assert_eq!(
                evaluation.final_decision == FinalDecision::Accept,
                index == expected
            );
        }
    }
    let mut equal = exact.clone();
    equal.identity.external_id = "different edition occurrence".into();
    for items in [
        vec![equal.clone(), exact.clone()],
        vec![exact.clone(), equal.clone()],
    ] {
        assert_eq!(
            assess(
                &input,
                &Page {
                    items,
                    next_offset: None
                }
            ),
            Assessment::NeedsSelection
        );
    }
    // Better credit cannot hide a worse established position.
    exact.number = 2;
    assert_eq!(
        assess(
            &input,
            &Page {
                items: vec![plausible, exact],
                next_offset: None
            }
        ),
        Assessment::NeedsSelection
    );
}

fn playable_fixture() -> (Library, music_library::song_resolution::Input, Candidate) {
    let mut library = Library::open_in_memory().unwrap();
    let mut release = release();
    release.media[0].tracks[0].duration = Some(music_library::catalog::Duration {
        milliseconds: 200000,
        approximate: false,
    });
    let imported = library.add_catalog_release(&release).unwrap();
    let input = library
        .song_resolution_input(&imported.track_ids[0])
        .unwrap();
    let mut c = candidate();
    c.identity = id("spotify", "track", "playable");
    c.artists = input.artists.clone();
    c.album = "Separate single".into();
    c.album_identity = Some(id("spotify", "album", "single"));
    c.album_type = "single".into();
    c.album_total_tracks = Some(1);
    (library, input, c)
}
#[test]
fn cross_release_playback_preserves_canonical_release_and_records_difference() {
    use music_library::{
        catalog::Page,
        song_resolution::{FinalDecision, evaluate},
    };
    let (mut library, input, c) = playable_fixture();
    let before = library.album_id_for_track(&input.track_id).unwrap();
    let page = Page {
        items: vec![c.clone()],
        next_offset: None,
    };
    let result = evaluate(&input, &page, 0, &[]).unwrap();
    assert_eq!(result.final_decision, FinalDecision::Accept);
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.contains("canonical Release unchanged"))
    );
    library
        .apply_song_evaluation(&input, &page)
        .unwrap()
        .unwrap();
    assert_eq!(library.album_id_for_track(&input.track_id).unwrap(), before);
    assert_eq!(
        library
            .song_resolution_input(&input.track_id)
            .unwrap()
            .album,
        input.album
    );
}
#[test]
fn sole_version_is_playable_but_comparable_versions_require_review() {
    use music_library::{
        catalog::Page,
        song_resolution::{Assessment, assess_playable, evaluate},
    };
    let (_, input, c) = playable_fixture();
    for suffix in ["(2015 Remaster)", "(Live)", "(Acoustic)", "(Demo)"] {
        let mut version = c.clone();
        version.title = format!("Song {suffix}");
        let page = Page {
            items: vec![version],
            next_offset: None,
        };
        assert_eq!(assess_playable(&input, &page), Assessment::Unique(0));
        assert!(
            evaluate(&input, &page, 0, &[])
                .unwrap()
                .warnings
                .iter()
                .any(|w| w.contains("version differs"))
        );
    }
    let items = ["Live", "Acoustic", "Demo"]
        .into_iter()
        .map(|version| {
            let mut c = c.clone();
            c.title = format!("Song ({version})");
            c.identity.external_id = version.into();
            c
        })
        .collect();
    assert_eq!(
        assess_playable(
            &input,
            &Page {
                items,
                next_offset: None
            }
        ),
        Assessment::NeedsSelection
    );
}
#[test]
fn playable_fallback_rejects_identity_artist_duration_and_title_conflicts() {
    use music_library::{
        catalog::Page,
        song_resolution::{Assessment, assess_playable},
    };
    let (_, input, c) = playable_fixture();
    let mut cases = vec![];
    let mut bad = c.clone();
    bad.artists[0].identities = vec![id("musicbrainz", "artist", "other")];
    cases.push(bad);
    let mut bad = c.clone();
    bad.recording.identities = vec![id("musicbrainz", "recording", "other")];
    cases.push(bad);
    let mut bad = c.clone();
    bad.duration_ms = 900000;
    cases.push(bad);
    for title in [
        "Song (Subtitle)",
        "字 Song",
        "Song feat. Stranger",
        "Unrelated Song",
    ] {
        let mut bad = c.clone();
        bad.title = title.into();
        cases.push(bad);
    }
    for bad in cases {
        assert_eq!(
            assess_playable(
                &input,
                &Page {
                    items: vec![bad],
                    next_offset: None
                }
            ),
            Assessment::NoMatch
        );
    }
}
#[test]
fn playable_ranking_uses_evidence_and_collapses_confirmed_equivalent_occurrences() {
    use music_library::{
        catalog::Page,
        song_resolution::{Assessment, assess_playable},
    };
    let (_, input, c) = playable_fixture();
    let mut weaker = c.clone();
    weaker.title = "Song (Live)".into();
    weaker.identity.external_id = "first".into();
    assert_eq!(
        assess_playable(
            &input,
            &Page {
                items: vec![weaker, c.clone()],
                next_offset: None
            }
        ),
        Assessment::Unique(1)
    );
    let mut a = c.clone();
    a.recording.isrcs = vec!["same-isrc".into()];
    let mut b = a.clone();
    b.identity.external_id = "duplicate".into();
    b.album = "Other wrapper".into();
    assert!(matches!(
        assess_playable(
            &input,
            &Page {
                items: vec![a, b],
                next_offset: None
            }
        ),
        Assessment::Unique(_)
    ));
    let mut featured = c.clone();
    featured
        .artists
        .push(music_library::edition::ArtistEvidence {
            name: "Guest".into(),
            ..Default::default()
        });
    assert_eq!(
        assess_playable(
            &input,
            &Page {
                items: vec![featured],
                next_offset: None
            }
        ),
        Assessment::Unique(0)
    );
}

#[test]
fn split_release_sources_survive_reload_without_album_or_track_identity_changes() {
    use music_library::catalog::Page;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("split.sqlite");
    let mut library = Library::open(&path).unwrap();
    let mut release = release();
    let mut second = release.media[0].tracks[0].clone();
    second.title = "Second song".into();
    second.position = 2;
    second.identities = vec![id("musicbrainz", "track", "second")];
    release.media[0].tracks.push(second);
    let imported = library.add_catalog_release(&release).unwrap();
    let canonical = library.album_id_for_track(&imported.track_ids[0]).unwrap();
    for (index, track) in imported.track_ids.iter().enumerate() {
        let input = library.song_resolution_input(track).unwrap();
        let mut c = candidate();
        c.identity = id("spotify", "track", &format!("source-{index}"));
        c.title = input.title.clone();
        c.artists = input.artists.clone();
        c.album = format!("Single {index}");
        c.album_identity = Some(id("spotify", "album", &format!("single-{index}")));
        assert_eq!(
            library
                .apply_song_evaluation(
                    &input,
                    &Page {
                        items: vec![c.clone()],
                        next_offset: None
                    }
                )
                .unwrap(),
            Some(c.identity)
        );
    }
    drop(library);
    let library = Library::open(&path).unwrap();
    for track in &imported.track_ids {
        assert_eq!(library.album_id_for_track(track).unwrap(), canonical);
        assert_eq!(
            library
                .track_provider_occurrences(track, "spotify")
                .unwrap()
                .len(),
            1
        );
    }
}

#[test]
fn explicit_spotify_correction_replaces_and_clears_without_changing_canonical_context() {
    use music_library::catalog::Page;
    let (mut library, input, c) = playable_fixture();
    library
        .apply_song_evaluation(
            &input,
            &Page {
                items: vec![c.clone()],
                next_offset: None,
            },
        )
        .unwrap();
    let current = library.song_resolution_input(&input.track_id).unwrap();
    let mut corrected = c.clone();
    corrected.identity.external_id = "user-chosen-alternative".into();
    corrected.title = "Explicit different version".into();
    let selection = Selection::new(current, vec![corrected.clone()]);
    library
        .replace_spotify_song_resolution(&selection, 0)
        .unwrap();
    assert_eq!(
        library
            .track_provider_occurrences(&input.track_id, "spotify")
            .unwrap(),
        vec![corrected.identity]
    );
    assert_eq!(
        library
            .song_resolution_input(&input.track_id)
            .unwrap()
            .album,
        input.album
    );
    library
        .clear_spotify_song_resolution(&input.track_id)
        .unwrap();
    assert!(
        library
            .track_provider_occurrences(&input.track_id, "spotify")
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        library
            .song_resolution_input(&input.track_id)
            .unwrap()
            .album,
        input.album
    );
}

#[test]
fn playable_credit_order_is_presentation_but_replacement_collaborators_conflict() {
    use music_library::{
        catalog::Page,
        edition::ArtistEvidence,
        song_resolution::{Assessment, assess_playable},
    };
    let (_, mut input, mut c) = playable_fixture();
    let guest = ArtistEvidence {
        name: "Guest".into(),
        identities: vec![id("spotify", "artist", "guest")],
        join_phrase: String::new(),
    };
    c.artists.insert(0, guest.clone());
    assert_eq!(
        assess_playable(
            &input,
            &Page {
                items: vec![c.clone()],
                next_offset: None
            }
        ),
        Assessment::Unique(0)
    );
    input.artists.push(ArtistEvidence {
        name: "Different collaborator".into(),
        ..Default::default()
    });
    assert_eq!(
        assess_playable(
            &input,
            &Page {
                items: vec![c],
                next_offset: None
            }
        ),
        Assessment::NoMatch
    );
}
#[test]
fn contradictory_isrc_and_incomplete_discovery_cannot_auto_connect_playable_source() {
    use music_library::{
        catalog::Page,
        song_resolution::{Assessment, assess_playable},
    };
    let (_, mut input, mut c) = playable_fixture();
    input.recording.isrcs = vec!["trusted-isrc".into()];
    c.recording.isrcs = vec!["different-isrc".into()];
    assert_eq!(
        assess_playable(
            &input,
            &Page {
                items: vec![c.clone()],
                next_offset: None
            }
        ),
        Assessment::NoMatch
    );
    c.recording.isrcs.clear();
    assert_eq!(
        assess_playable(
            &input,
            &Page {
                items: vec![c],
                next_offset: Some(10)
            }
        ),
        Assessment::NeedsSelection
    );
}
