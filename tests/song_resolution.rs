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
        identity: id("song-provider", "song", "opaque-song"),
        title: "Song".into(),
        artist: "Artist".into(),
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
