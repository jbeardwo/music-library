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
