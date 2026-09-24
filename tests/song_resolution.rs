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
