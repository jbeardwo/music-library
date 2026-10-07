#[path = "support/metadata_schema.rs"]
mod metadata_schema;
use music_library::{
    Library,
    catalog::{Credit, Medium, Release, Track},
    domain::{ExternalIdentity, SearchRequest},
};
use rusqlite::Connection;

fn id(provider: &str, kind: &str, value: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: provider.into(),
        kind: kind.into(),
        external_id: value.into(),
    }
}
fn release(key: &str) -> Release {
    let credits = vec![
        Credit {
            identity: None,
            name: "Artist A".into(),
            join_phrase: " feat. ".into(),
        },
        Credit {
            identity: None,
            name: "Artist B".into(),
            join_phrase: "".into(),
        },
    ];
    Release {
        album: music_library::catalog::Album {
            release_type: None,
            identity: id("musicbrainz", "release_group", "group"),
            title: "Edition (Live)".into(),
            date: "2001-02-03".into(),
            credits: credits.clone(),
        },
        identity: id("musicbrainz", "release", key),
        identities: vec![],
        title: "Edition (Live)".into(),
        date: "2001-02-03".into(),
        credits: credits.clone(),
        media: vec![
            Medium {
                position: 1,
                tracks: vec![Track {
                    duration: None,
                    position: 1,
                    title: "Song (Live)".into(),
                    credits: credits.clone(),
                    identities: vec![
                        id("musicbrainz", "track", &format!("{key}-1")),
                        id("musicbrainz", "recording", "recording"),
                        id("isrc", "recording", "one"),
                        id("isrc", "recording", "two"),
                    ],
                }],
            },
            Medium {
                position: 2,
                tracks: vec![Track {
                    duration: None,
                    position: 1,
                    title: "Song - Remix".into(),
                    credits,
                    identities: vec![
                        id("musicbrainz", "track", &format!("{key}-2")),
                        id("musicbrainz", "recording", "recording"),
                        id("isrc", "recording", "one"),
                    ],
                }],
            },
        ],
    }
}
#[test]
fn catalog_reuse_confirms_previously_managed_album_identity() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.sqlite");
    let mut library = Library::open(&path).unwrap();
    let input = release("edition");
    library.add_catalog_release(&input).unwrap();
    let db = Connection::open(path).unwrap();
    db.execute(
        "INSERT INTO album_provenance_identity SELECT * FROM album_external_identity",
        [],
    )
    .unwrap();
    library.add_catalog_release(&input).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM album_provenance_identity", [], |r| {
            r.get::<_, u32>(0)
        })
        .unwrap(),
        0
    );
}

#[test]
fn atomic_source_less_add_preserves_credits_identities_membership_and_reimport() {
    let temp = tempfile::TempDir::new().unwrap();
    let path = temp.path().join("library.sqlite");
    let mut library = Library::open(&path).unwrap();
    let input = release("edition-one");
    let imported = library.add_catalog_release(&input).unwrap();
    assert_eq!(imported.track_ids.len(), 2);
    let album = library.album_for_release(&imported.release_id).unwrap();
    assert_eq!(album.title, input.album.title);
    assert_eq!(
        library
            .resolve_albums_external_identity(&input.album.identity)
            .unwrap(),
        vec![album.album_id.clone()]
    );
    assert!(
        library
            .resolve_releases_external_identity(&input.album.identity)
            .unwrap()
            .is_empty()
    );
    let rows = library
        .search(&SearchRequest {
            release_id: Some(imported.release_id.clone()),
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert!(!row.available);
        assert_eq!(row.artist_names, "Artist A feat. Artist B");
        assert_eq!(row.year, Some(2001));
    }
    for track in &imported.track_ids {
        assert!(library.available_playback_source(track).unwrap().is_none());
        assert_eq!(
            library.list_track_external_identities(track).unwrap().len(),
            1
        );
    }
    assert_eq!(
        library
            .resolve_releases_external_identity(&input.identity)
            .unwrap(),
        std::slice::from_ref(&imported.release_id)
    );
    assert_eq!(
        library
            .list_release_external_identities(&imported.release_id)
            .unwrap()
            .len(),
        1
    );
    library.remove_from_library(&imported.track_ids[0]).unwrap();
    library
        .set_track_title_override(&imported.track_ids[0], "User title")
        .unwrap();
    let mut changed = input.clone();
    changed.title = "Do not refresh".into();
    changed.media.clear();
    assert_eq!(library.add_catalog_release(&changed).unwrap(), imported);
    assert_eq!(
        library
            .search(&SearchRequest {
                release_id: Some(imported.release_id.clone()),
                limit: 10,
                ..Default::default()
            })
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        library
            .search(&SearchRequest {
                text: "User title".into(),
                limit: 10,
                ..Default::default()
            })
            .unwrap()[0]
            .artist_names,
        "Artist A feat. Artist B"
    );
    let mut second_input = release("edition-two");
    second_input.title = "Edition Deluxe".into();
    second_input.date = "2025".into();
    second_input.album.title = "Do not refresh Album".into();
    let second = library.add_catalog_release(&second_input).unwrap();
    assert_eq!(
        library.album_for_release(&second.release_id).unwrap(),
        album
    );
    let second_rows = library
        .search(&SearchRequest {
            release_id: Some(second.release_id.clone()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(second_rows[0].release_title, "Edition (Live)");
    assert_eq!(second_rows[0].year, Some(2001));
    assert_ne!(second.release_id, imported.release_id);
    assert_eq!(
        library
            .album_for_release(&second.release_id)
            .unwrap()
            .album_id,
        album.album_id
    );
    assert_eq!(
        library
            .resolve_recordings_external_identity(&id("musicbrainz", "recording", "recording"))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        library
            .resolve_recordings_external_identity(&id("isrc", "recording", "one"))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        library
            .resolve_albums_external_identity(&id("musicbrainz", "release_group", "group"))
            .unwrap()
            .len(),
        1
    );
    let mixed_playlist = library.create_playlist("Mixed batch").unwrap();
    let first = library
        .prepare_catalog_playlist_append(&mixed_playlist, &input, &[(1, 1)])
        .unwrap();
    library.apply_playlist_append(&first, true).unwrap();
    let mixed = library
        .prepare_catalog_playlist_append(&mixed_playlist, &input, &[(2, 1), (1, 1)])
        .unwrap();
    assert_eq!(mixed.duplicate_entries, 1);
    assert_eq!(library.apply_playlist_append(&mixed, false).unwrap(), 1);
    assert_eq!(library.apply_playlist_append(&mixed, true).unwrap(), 2);
    let db = Connection::open(&path).unwrap();
    for table in ["playable_source", "track_source"] {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, u32>(0))
                .unwrap(),
            0
        );
    }
    let positions:Vec<(u32,u32)>=db.prepare("SELECT disc_number,track_number FROM track WHERE release_id=?1 ORDER BY disc_number,track_number").unwrap().query_map([imported.release_id.as_ref()],|r|Ok((r.get(0)?,r.get(1)?))).unwrap().map(Result::unwrap).collect();
    assert_eq!(positions, [(1, 1), (2, 1)]);
    // Shared generic identities are legal, but Add must not select an ambiguous owner.
    library
        .attach_release_external_identity(&second.release_id, &input.identity)
        .unwrap();
    assert!(library.add_catalog_release(&input).is_err());
    drop(library);
    assert_eq!(
        Library::open(path)
            .unwrap()
            .resolve_recordings_external_identity(&id("isrc", "recording", "one"))
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn failed_import_rolls_back_every_entity_credit_identity_membership_and_search_row() {
    let temp = tempfile::TempDir::new().unwrap();
    let path = temp.path().join("library.sqlite");
    let mut library = Library::open(&path).unwrap();
    let db = Connection::open(path).unwrap();
    db.execute_batch("CREATE TRIGGER fail_catalog BEFORE INSERT ON track_external_identity WHEN NEW.external_id='broken-2' BEGIN SELECT RAISE(ABORT,'induced catalog failure'); END;").unwrap();
    assert!(library.add_catalog_release(&release("broken")).is_err());
    for table in [
        "album",
        "album_application_metadata",
        "album_artist_credit",
        "album_external_identity",
        "release",
        "track",
        "artist",
        "track_artist_credit",
        "release_artist_credit",
        "release_external_identity",
        "track_external_identity",
        "library_membership",
        "track_application_metadata",
        "release_application_metadata",
        "effective_track_metadata",
        "track_search",
    ] {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, u32>(0))
                .unwrap(),
            0,
            "{table}"
        );
    }
}
#[test]
fn credit_migration_upgrades_v2_and_retains_legacy_display() {
    let temp = tempfile::TempDir::new().unwrap();
    let path = temp.path().join("v2.sqlite");
    let db = Connection::open(&path).unwrap();
    db.execute_batch(include_str!("../migrations/0001_initial.sql"))
        .unwrap();
    db.execute_batch(include_str!("../migrations/0002_external_identities.sql"))
        .unwrap();
    db.execute_batch(
        "INSERT INTO release(id) VALUES ('r'); INSERT INTO track(id,release_id) VALUES ('t','r');
        INSERT INTO release_application_metadata(release_id,title) VALUES ('r','Legacy');
        INSERT INTO track_application_metadata(track_id,title) VALUES ('t','Legacy');
        INSERT INTO artist(id,name) VALUES ('a','One'),('b','Two');
        INSERT INTO track_artist_credit(track_id,position,artist_id) VALUES ('t',0,'a'),('t',1,'b');
        INSERT INTO library_membership(track_id) VALUES ('t');",
    )
    .unwrap();
    let mut library = Library::open(path).unwrap();
    library
        .set_track_title_override(&music_library::domain::TrackId("t".into()), "Legacy")
        .unwrap();
    assert_eq!(
        library
            .search(&SearchRequest {
                limit: 10,
                ..Default::default()
            })
            .unwrap()[0]
            .artist_names,
        "One, Two"
    );
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        31
    );
}

#[test]
fn exact_existing_edition_anchors_album_and_conflicting_owners_roll_back() {
    let mut library = Library::open_in_memory().unwrap();
    let input = release("edition");
    let old = library
        .create_catalog_release(&music_library::domain::CatalogReleaseInput {
            title: "Existing friendly title".into(),
            year: Some(1999),
            artists: vec![],
            tracks: vec![music_library::domain::CatalogTrackInput {
                title: "Existing track".into(),
                artists: vec![],
                disc_number: Some(1),
                track_number: Some(1),
            }],
        })
        .unwrap();
    let album = library.album_for_release(&old.release_id).unwrap();
    library
        .attach_release_external_identity(&old.release_id, &input.identity)
        .unwrap();
    assert_eq!(library.add_catalog_release(&input).unwrap(), old);
    assert_eq!(library.album_for_release(&old.release_id).unwrap(), album); // no metadata refresh
    assert_eq!(
        library
            .resolve_albums_external_identity(&input.album.identity)
            .unwrap(),
        vec![album.album_id.clone()]
    );
    let mut other = release("other-edition");
    other.album.identity.external_id = "other-album".into();
    let imported = library.add_catalog_release(&other).unwrap();
    let other_album = library.album_for_release(&imported.release_id).unwrap();
    other.identity = input.identity.clone();
    assert!(library.add_catalog_release(&other).is_err());
    assert_eq!(library.album_for_release(&old.release_id).unwrap(), album);
    assert_eq!(
        library.album_for_release(&imported.release_id).unwrap(),
        other_album
    );
    library
        .attach_album_external_identity(&other_album.album_id, &input.album.identity)
        .unwrap();
    assert!(library.add_catalog_release(&input).is_err()); // generic sharing is valid, implicit selection is not
}

#[test]
fn catalog_reuses_strong_artist_identity_without_changing_credited_presentation() {
    use music_library::domain::ArtistId;
    for provider in ["musicbrainz", "spotify", "another-catalog"] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("db");
        let mut lib = Library::open(&path).unwrap();
        let db = Connection::open(path).unwrap();
        db.execute_batch("INSERT INTO artist(id,name) VALUES ('a-existing','Canonical name'),('z-duplicate','Alternate name')").unwrap();
        for key in ["a-existing", "z-duplicate"] {
            lib.attach_artist_external_identity(
                &ArtistId(key.into()),
                &id(provider, "artist", "strong"),
            )
            .unwrap();
        }
        for key in ["first", "second"] {
            let mut data = release(key);
            data.album.identity.external_id = key.into();
            for credits in std::iter::once(&mut data.album.credits)
                .chain(std::iter::once(&mut data.credits))
                .chain(
                    data.media
                        .iter_mut()
                        .flat_map(|m| m.tracks.iter_mut().map(|t| &mut t.credits)),
                )
            {
                // Same canonical Artist can legitimately occupy two distinct positions.
                for credit in credits {
                    credit.identity = Some(id(provider, "artist", "strong"));
                }
            }
            lib.add_catalog_release(&data).unwrap();
            lib.add_catalog_release(&data).unwrap();
        }
        assert_eq!(
            lib.resolve_artists_external_identity(&id(provider, "artist", "strong"))
                .unwrap(),
            vec![ArtistId("a-existing".into())]
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM artist", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        for table in [
            "album_artist_credit",
            "release_artist_credit",
            "track_artist_credit",
        ] {
            assert_eq!(
                db.query_row(
                    &format!("SELECT count(*) FROM {table} WHERE artist_id!='a-existing'"),
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
            assert_eq!(
                db.query_row(
                    &format!(
                        "SELECT credited_name || join_phrase FROM {table} WHERE position=0 LIMIT 1"
                    ),
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                "Artist A feat. "
            );
        }
        for row in lib
            .search(&SearchRequest {
                limit: 10,
                ..Default::default()
            })
            .unwrap()
        {
            assert_eq!(row.artist_names, "Artist A feat. Artist B");
        }
    }
}

#[test]
fn catalog_text_only_same_and_similar_names_remain_separate() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let mut lib = Library::open(&path).unwrap();
    let mut data = release("first");
    for credit in &mut data.album.credits {
        credit.name = "Same".into();
    }
    lib.add_catalog_release(&data).unwrap();
    data.identity.external_id = "second".into();
    data.album.identity.external_id = "second".into();
    data.album.credits[1].name = "Samee".into();
    lib.add_catalog_release(&data).unwrap();
    let db = Connection::open(path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(DISTINCT artist_id) FROM album_artist_credit",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        4
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM artist_external_identity", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
}

#[test]
fn catalog_conflicting_artist_identity_rolls_back_prior_catalog_writes() {
    use music_library::{domain::ArtistId, storage::Error};
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let mut lib = Library::open(&path).unwrap();
    let db = Connection::open(path).unwrap();
    db.execute_batch("INSERT INTO artist(id,name) VALUES ('conflict','Artist')")
        .unwrap();
    for value in ["one", "two"] {
        lib.attach_artist_external_identity(
            &ArtistId("conflict".into()),
            &id("musicbrainz", "artist", value),
        )
        .unwrap();
    }
    let mut data = release("new");
    // Album and Release creation precede this late Track-credit conflict.
    data.media[1].tracks[0].credits[0].identity = Some(id("musicbrainz", "artist", "one"));
    assert!(matches!(
        lib.add_catalog_release(&data),
        Err(Error::ArtistIdentityConflict)
    ));
    for table in [
        "album",
        "release",
        "track",
        "library_membership",
        "album_external_identity",
        "release_external_identity",
        "track_external_identity",
    ] {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    assert_eq!(
        db.query_row("SELECT count(*) FROM artist", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM artist_external_identity", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        2
    );
}

#[test]
fn same_name_with_distinct_musicbrainz_artists_stays_distinct() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let mut lib = Library::open(&path).unwrap();
    let mut data = release("distinct");
    for credits in std::iter::once(&mut data.album.credits)
        .chain(std::iter::once(&mut data.credits))
        .chain(
            data.media
                .iter_mut()
                .flat_map(|m| m.tracks.iter_mut().map(|t| &mut t.credits)),
        )
    {
        for (position, credit) in credits.iter_mut().enumerate() {
            credit.name = "Same name".into();
            credit.identity = Some(id("musicbrainz", "artist", &position.to_string()));
        }
    }
    lib.add_catalog_release(&data).unwrap();
    let db = Connection::open(path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM artist", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        db.query_row(
            "SELECT count(DISTINCT artist_id) FROM album_artist_credit",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
}

#[test]
fn selective_catalog_add_preserves_partial_membership_and_completes_without_duplicates() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.sqlite");
    let mut library = Library::open(&path).unwrap();
    let input = release("selective");
    let imported = library.add_catalog_selection(&input, &[(2, 1)]).unwrap();
    assert_eq!(imported.track_ids.len(), 2); // Known program is separate from saved state.
    let saved = library
        .search(&SearchRequest {
            limit: 20,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].track_id, imported.track_ids[1]);
    assert_eq!(
        library.catalog_saved_positions(&input).unwrap(),
        vec![(2, 1)]
    );
    assert_eq!(
        library.add_catalog_selection(&input, &[(2, 1)]).unwrap(),
        imported
    );
    assert_eq!(
        library
            .search(&SearchRequest {
                limit: 20,
                ..Default::default()
            })
            .unwrap()
            .len(),
        1
    );
    library
        .set_track_title_override(&imported.track_ids[1], "My Song")
        .unwrap();
    assert_eq!(library.add_catalog_release(&input).unwrap(), imported);
    assert_eq!(
        library
            .search(&SearchRequest {
                limit: 20,
                ..Default::default()
            })
            .unwrap()
            .len(),
        2
    );
    library.remove_from_library(&imported.track_ids[0]).unwrap();
    library.add_catalog_selection(&input, &[(2, 1)]).unwrap();
    assert_eq!(
        library
            .search(&SearchRequest {
                limit: 20,
                ..Default::default()
            })
            .unwrap()
            .len(),
        1
    );
    let db = Connection::open(path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM track", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM album", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        library
            .search(&SearchRequest {
                limit: 20,
                ..Default::default()
            })
            .unwrap()[0]
            .title,
        "My Song"
    );
}

#[test]
fn invalid_catalog_selection_rolls_back_program_and_membership() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.sqlite");
    let mut library = Library::open(&path).unwrap();
    assert!(
        library
            .add_catalog_selection(&release("invalid"), &[(9, 9)])
            .is_err()
    );
    assert!(
        library
            .add_catalog_selection(&release("invalid"), &[])
            .is_err()
    );
    let db = Connection::open(path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM track", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM album", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        0
    );
}

#[test]
fn catalog_awareness_uses_identities_and_keeps_ambiguous_recording_occurrences_separate() {
    use music_library::{
        catalog::{AlbumCandidate, SongCandidate},
        catalog_search::Hit,
    };
    let temp = tempfile::tempdir().unwrap();
    let mut library = Library::open(temp.path().join("library.sqlite")).unwrap();
    let input = release("awareness");
    let album = AlbumCandidate {
        identity: input.album.identity.clone(),
        title: input.album.title.clone(),
        artist: "Artist".into(),
        credits: input.credits.clone(),
        date: String::new(),
        primary_type: String::new(),
        secondary_types: vec![],
        comment: String::new(),
        score: None,
    };
    let song = SongCandidate {
        identity: input.media[0].tracks[0].identities[0].clone(),
        title: "Song".into(),
        artist: "Artist".into(),
        album: album.clone(),
        release: Some(input.identity.clone()),
        disc: Some(1),
        position: Some(1),
    };
    let hits = vec![Hit::Album(album.clone()), Hit::Song(Box::new(song.clone()))];
    assert!(
        library
            .catalog_context(&hits)
            .unwrap()
            .iter()
            .all(|c| c.key.is_none() && c.saved == 0)
    );
    library.add_catalog_selection(&input, &[(1, 1)]).unwrap();
    let context = library.catalog_context(&hits).unwrap();
    assert_eq!(context[0].saved, 1);
    assert_eq!(context[1].saved, 1);
    let mut recording = song.clone();
    recording.identity = id("musicbrainz", "recording", "recording");
    assert!(
        library
            .catalog_context(&[Hit::Song(Box::new(recording))])
            .unwrap()[0]
            .key
            .is_none()
    );
    let mut unknown = album;
    unknown.identity.external_id = "same title, different identity".into();
    assert_eq!(
        library.catalog_context(&[Hit::Album(unknown)]).unwrap()[0].saved,
        0
    );
    library.add_catalog_release(&input).unwrap();
    assert_eq!(library.catalog_context(&hits).unwrap()[0].saved, 2);
}

#[test]
fn catalog_preview_reuses_only_an_unambiguous_existing_reference() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("catalog.sqlite");
    let mut library = Library::open(&path).unwrap();
    let input = release("song-context");
    let album = music_library::catalog::AlbumCandidate {
        identity: input.album.identity.clone(),
        title: input.album.title.clone(),
        artist: "Artist".into(),
        credits: input.credits.clone(),
        date: String::new(),
        primary_type: String::new(),
        secondary_types: vec![],
        comment: String::new(),
        score: None,
    };
    assert!(
        library
            .catalog_existing_reference(&album)
            .unwrap()
            .is_none()
    );
    library.add_catalog_selection(&input, &[(1, 1)]).unwrap();
    drop(library);
    let mut library = Library::open(path).unwrap();
    assert_eq!(
        library.catalog_existing_reference(&album).unwrap(),
        Some(input.identity)
    );
    library
        .add_catalog_release(&release("other-edition"))
        .unwrap();
    assert!(
        library
            .catalog_existing_reference(&album)
            .unwrap()
            .is_none()
    );
}

#[test]
fn catalog_playlist_identity_membership_reload_and_duplicates() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("playlist.sqlite");
    let mut library = Library::open(&path).unwrap();
    let playlist = library.create_playlist("Catalog only").unwrap();
    let mut input = release("playlist-edition");
    input.media[0].tracks[0]
        .identities
        .push(id("spotify", "track", "provider-playable"));
    input.date.clear();
    input.media[1].tracks[0].credits.clear();
    let plan = library
        .prepare_catalog_playlist_append(&playlist, &input, &[(2, 1), (1, 1)])
        .unwrap();
    assert_eq!(plan.duplicate_entries, 0);
    assert_eq!(library.apply_playlist_append(&plan, true).unwrap(), 2);
    let ids = plan.tracks.clone();
    use music_library::playback_resolver::{RemoteCapability, Route};
    let remote = RemoteCapability {
        provider: "spotify",
        unavailable: None,
        catalog_available: false,
        accepts: |i| i.kind == "track",
    };
    assert_eq!(
        library.playback_route(&ids[1], &remote).unwrap(),
        Route::Remote(id("spotify", "track", "provider-playable"))
    );
    assert!(matches!(
        library.playback_route(&ids[0], &remote).unwrap(),
        Route::Unavailable(_)
    ));

    let db = Connection::open(&path).unwrap();
    for table in ["library_membership", "local_file_observation"] {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    for pane in [
        music_library::browse::Pane::Artists,
        music_library::browse::Pane::Albums,
        music_library::browse::Pane::Songs,
        music_library::browse::Pane::Genres,
    ] {
        assert!(
            library
                .browse(&music_library::browse::Request {
                    pane,
                    limit: 200,
                    ..Default::default()
                })
                .unwrap()
                .is_empty()
        );
    }
    assert!(
        library
            .search(&SearchRequest::default())
            .unwrap()
            .is_empty()
    );
    let duplicate = library
        .prepare_catalog_playlist_append(&playlist, &input, &[(1, 1)])
        .unwrap();
    assert_eq!(duplicate.tracks[0], ids[1]);
    assert_eq!(duplicate.duplicate_entries, 1);
    assert_eq!(library.apply_playlist_append(&duplicate, false).unwrap(), 0);
    assert_eq!(library.apply_playlist_append(&duplicate, true).unwrap(), 1);
    let rows = library.playlist_entries(&playlist, None, 200).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].title, "Song - Remix");
    // Missing Track credits use the established Album display-credit fallback.
    assert_eq!(rows[0].subtitle, "Artist A feat. Artist B");
    assert!(!rows[0].track.as_ref().unwrap().available);
    assert_ne!(rows[1].id, rows[2].id);
    let entry_ids = rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>();
    drop(library);
    let mut library = Library::open(&path).unwrap();
    assert_eq!(
        library
            .playlist_entries(&playlist, None, 200)
            .unwrap()
            .iter()
            .map(|r| r.id.clone())
            .collect::<Vec<_>>(),
        entry_ids
    );
    let (snapshot, start) = library
        .library_queue_reader()
        .unwrap()
        .read_playlist(&playlist, Some(&entry_ids[2]))
        .unwrap();
    assert_eq!(start, 2);
    assert_eq!(
        snapshot
            .iter()
            .map(|r| r.track_id.clone())
            .collect::<Vec<_>>(),
        [ids[0].clone(), ids[1].clone(), ids[1].clone()]
    );
    let saved = library.add_catalog_selection(&input, &[(1, 1)]).unwrap();
    assert!(saved.track_ids.contains(&ids[1]));
    assert_eq!(
        db.query_row("SELECT count(*) FROM track", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    library
        .remove_playlist_entry(&playlist, &entry_ids[1])
        .unwrap();
    assert_eq!(library.search(&SearchRequest::default()).unwrap().len(), 1);
    library.remove_from_library(&ids[1]).unwrap();
    assert_eq!(
        library
            .playlist_entries(&playlist, None, 200)
            .unwrap()
            .len(),
        2
    );
    library
        .remove_playlist_entry(&playlist, &entry_ids[2])
        .unwrap();
    assert_eq!(
        library.ensure_catalog_release(&input).unwrap().track_ids,
        saved.track_ids
    );
}

#[test]
fn catalog_duration_is_durable_membership_independent_and_enriches_same_identity() {
    use music_library::catalog::Duration;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("duration.sqlite");
    let mut library = Library::open(&path).unwrap();
    let mut input = release("duration-edition");
    input.media[0].tracks[0].duration = Some(Duration {
        milliseconds: 123_000,
        approximate: true,
    });
    let playlist = library.create_playlist("Durations").unwrap();
    let plan = library
        .prepare_catalog_playlist_append(&playlist, &input, &[(1, 1), (1, 1), (2, 1)])
        .unwrap();
    library.apply_playlist_append(&plan, true).unwrap();
    let rows = library.playlist_entries(&playlist, None, 200).unwrap();
    assert_eq!(rows[0].duration_ms, Some(123_000));
    assert!(rows[0].duration_approximate);
    assert_eq!(rows[1].duration_ms, Some(123_000));
    assert_eq!(rows[2].duration_ms, None);
    let canonical = rows[0].track.as_ref().unwrap().track_id.clone();
    let details = library.playlist_details(&playlist).unwrap().unwrap();
    assert_eq!(details.known_duration_ms, 246_000);
    assert_eq!(details.unknown_duration_count, 1);
    assert_eq!(details.approximate_duration_count, 2);
    assert!(
        library
            .search(&SearchRequest::default())
            .unwrap()
            .is_empty()
    );
    drop(library);
    let mut library = Library::open(&path).unwrap();
    assert_eq!(
        library.playlist_entries(&playlist, None, 200).unwrap()[0].duration_ms,
        Some(123_000)
    );
    let imported = library.ensure_catalog_release(&input).unwrap();
    library
        .attach_release_external_identity(
            &imported.release_id,
            &id("spotify", "album", "spotify-duration-album"),
        )
        .unwrap();
    let mut spotify = input.clone();
    spotify.identity = id("spotify", "album", "spotify-duration-album");
    spotify.media[0].tracks[0].duration = Some(Duration {
        milliseconds: 124_000,
        approximate: false,
    });
    let enriched = library.ensure_catalog_release(&spotify).unwrap();
    assert_eq!(imported.track_ids, enriched.track_ids);
    let rows = library.playlist_entries(&playlist, None, 200).unwrap();
    assert_eq!(rows[0].track.as_ref().unwrap().track_id, canonical);
    assert_eq!(rows[0].duration_ms, Some(124_000));
    assert!(!rows[0].duration_approximate);
    library.ensure_catalog_release(&input).unwrap(); // lower-quality refresh cannot replace provider evidence
    let details = library.playlist_details(&playlist).unwrap().unwrap();
    assert_eq!(details.known_duration_ms, 248_000);
    assert_eq!(details.approximate_duration_count, 0);
    assert_eq!(details.unknown_duration_count, 1);
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM library_membership", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        library
            .library_queue_reader()
            .unwrap()
            .read_playlist(&playlist, None)
            .unwrap()
            .0
            .len(),
        3
    );
    drop(library);
    let library = Library::open(&path).unwrap();
    assert_eq!(
        library.playlist_entries(&playlist, None, 200).unwrap()[0].duration_ms,
        Some(124_000)
    );
}

#[test]
fn duration_migration_backfills_persisted_evidence_without_library_membership() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("backfill.sqlite");
    let mut library = Library::open(&path).unwrap();
    let imported = library
        .ensure_catalog_release(&release("backfill"))
        .unwrap();
    let track = &imported.track_ids[0];
    drop(library);
    let db = Connection::open(&path).unwrap();
    let candidate = music_library::manual_track::Candidate {
        evidence: music_library::edition::TrackEvidence {
            duration_ms: Some(123_456),
            ..Default::default()
        },
        supporting_programs: 1,
    };
    db.execute(
        "INSERT INTO manual_track_association VALUES (?1,'musicbrainz','release_group','group',?2)",
        rusqlite::params![track.as_ref(), serde_json::to_string(&candidate).unwrap()],
    )
    .unwrap();
    metadata_schema::downgrade(&db);
    db.execute_batch(include_str!("support/drop_song_details.sql"))
        .unwrap();
    metadata_schema::downgrade(&db);
    db.execute_batch(include_str!("support/drop_playlist_revision.sql"))
        .unwrap();
    db.execute_batch("DROP TABLE playlist_source; PRAGMA user_version=21")
        .unwrap();
    drop(db);
    let library = Library::open(&path).unwrap();
    let evidence = library.edition_evidence(&imported.release_id).unwrap();
    assert_eq!(evidence.tracks[0].evidence.duration_ms, Some(123_456));
    assert!(evidence.tracks[0].evidence.duration_approximate);
    let db = Connection::open(path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM library_membership", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM track_duration_observation", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
