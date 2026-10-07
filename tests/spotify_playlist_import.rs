#[path = "support/metadata_schema.rs"]
mod metadata_schema;
use music_library::{
    Library,
    browse::{Pane, Request},
    catalog::{Credit, Duration},
    domain::*,
    playback_resolver::{RemoteCapability, Route},
    playlist_import::{Item, Outcome, Plan},
};
use rusqlite::{Connection, params};
fn identity(kind: &str, id: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: "spotify".into(),
        kind: kind.into(),
        external_id: id.into(),
    }
}
fn item(n: usize) -> Item {
    let credits = vec![Credit {
        identity: Some(identity("artist", "artist")),
        name: "Playlist artist".into(),
        join_phrase: String::new(),
    }];
    Item {
        identity: identity("track", &format!("{n:022}")),
        title: format!("Song {n}"),
        credits: credits.clone(),
        release_identity: identity("album", "album"),
        release_title: "Spotify edition".into(),
        release_credits: credits,
        year: Some(2001),
        disc: Some(1),
        number: Some(n as u32 + 1),
        duration: Some(Duration {
            milliseconds: 180000 + n as u64,
            approximate: false,
        }),
    }
}
fn plan() -> Plan {
    Plan {
        provider: "spotify".into(),
        external_id: "playlist".into(),
        source_url: "https://open.spotify.com/playlist/playlist".into(),
        version: Some("version".into()),
        owner: "Owner".into(),
        name: "Recommendations".into(),
        items: (0..20).map(item).collect(),
        unsupported: 2,
        unavailable: 1,
    }
}
fn imported(l: &mut Library, p: &Plan) -> String {
    let Outcome::Imported {
        playlist_id,
        entries,
    } = l.import_playlist_snapshot(p).unwrap()
    else {
        panic!()
    };
    assert_eq!(entries, p.items.len());
    playlist_id
}
fn count(db: &Connection, table: &str) -> i64 {
    db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
fn remote() -> RemoteCapability<'static> {
    RemoteCapability {
        provider: "spotify",
        unavailable: None,
        catalog_available: false,
        accepts: |id| id.provider == "spotify" && id.kind == "track",
    }
}
#[test]
fn twenty_tracks_are_playlist_only_with_duration_order_offline_restart_and_no_fuzzy_merge() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("library.sqlite");
    let mut l = Library::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    let p = plan();
    let id = imported(&mut l, &p);
    assert_eq!(count(&db, "playlist_entry"), 20);
    assert_eq!(count(&db, "library_membership"), 0);
    assert_eq!(count(&db, "release"), 1);
    for pane in [Pane::Songs, Pane::Albums, Pane::Artists, Pane::Genres] {
        assert!(
            l.browse(&Request {
                pane,
                limit: 200,
                ..Default::default()
            })
            .unwrap()
            .is_empty()
        );
    }
    let rows = l.playlist_entries(&id, None, 200).unwrap();
    assert_eq!(rows.len(), 20);
    for (n, row) in rows.iter().enumerate() {
        assert_eq!(row.title, format!("Song {n}"));
        assert_eq!(row.subtitle, "Playlist artist");
        assert_eq!(row.track.as_ref().unwrap().artist_names, "Playlist artist");
        assert_eq!(row.duration_ms, Some(180000 + n as u64));
        assert_eq!(row.playlist_position, Some(n as u64 + 1));
        assert!(matches!(
            l.playback_route(&row.track.as_ref().unwrap().track_id, &remote())
                .unwrap(),
            Route::Remote(_)
        ));
    }
    assert_eq!(
        l.playlist_details(&id).unwrap().unwrap().known_duration_ms,
        3600190
    );
    let queue = l
        .library_queue_reader()
        .unwrap()
        .read_playlist(&id, None)
        .unwrap()
        .0;
    // Identical title/artist/album with different trusted identity stays distinct.
    let mut copy = p.clone();
    copy.external_id = "different source".into();
    copy.items[0].identity.external_id = "different identity".into();
    imported(&mut l, &copy);
    assert_eq!(count(&db, "track"), 21);
    assert_eq!(queue.len(), 20);
    drop(l);
    let mut l = Library::open(&path).unwrap();
    assert_eq!(
        l.playlist_entries(&id, None, 200).unwrap()[0].duration_ms,
        Some(180000)
    );
    assert_eq!(
        l.imported_playlist("spotify", "playlist").unwrap(),
        Some(id.clone())
    );
    l.rename_playlist(&id, "Locally edited").unwrap();
    l.remove_playlist_entry(&id, &rows[0].id).unwrap();
    assert_eq!(
        l.import_playlist_snapshot(&p).unwrap(),
        Outcome::AlreadyImported {
            playlist_id: id.clone()
        }
    );
    assert_eq!(l.playlist_details(&id).unwrap().unwrap().entry_count, 19);
    assert_eq!(count(&db, "playlist"), 2);
    assert_eq!(queue[0].track_id, rows[0].track.as_ref().unwrap().track_id);
}
#[test]
fn duplicate_occurrences_reuse_saved_local_track_preserving_metadata_and_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("library.sqlite");
    let mut l = Library::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    let existing = l
        .create_catalog_release(&CatalogReleaseInput {
            title: "Local edition".into(),
            year: None,
            artists: vec![],
            tracks: vec![CatalogTrackInput {
                title: "User title".into(),
                artists: vec![],
                disc_number: Some(1),
                track_number: Some(1),
            }],
        })
        .unwrap()
        .track_ids[0]
        .clone();
    l.attach_track_external_identity(&existing, &item(0).identity)
        .unwrap();
    l.attach_track_external_identity(
        &existing,
        &ExternalIdentity {
            provider: "other".into(),
            kind: "track".into(),
            external_id: "other-id".into(),
        },
    )
    .unwrap();
    l.add_to_library(&existing).unwrap();
    let root = l.register_local_root(tmp.path()).unwrap();
    let file = tmp.path().join("source");
    std::fs::write(&file, b"test").unwrap();
    #[cfg(unix)]
    let path_bytes = {
        use std::os::unix::ffi::OsStrExt;
        file.as_os_str().as_bytes().to_vec()
    };
    #[cfg(windows)]
    let path_bytes = {
        use std::os::windows::ffi::OsStrExt;
        file.as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    };
    db.execute(
        "INSERT INTO playable_source(id,kind) VALUES('local','local_file')",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO track_source(track_id,source_id) VALUES(?1,'local')",
        [existing.as_ref()],
    )
    .unwrap();
    db.execute("INSERT INTO local_file_observation(source_id,root_id,path,size_bytes,modified_ns,available) VALUES('local',?1,?2,4,1,1)",params![root.as_ref(),path_bytes]).unwrap();
    let mut p = plan();
    p.items = vec![item(0), item(1), item(0)];
    let id = imported(&mut l, &p);
    let rows = l.playlist_entries(&id, None, 200).unwrap();
    assert_ne!(rows[0].id, rows[2].id);
    assert_eq!(rows[0].track.as_ref().unwrap().track_id, existing);
    assert_eq!(rows[2].track.as_ref().unwrap().track_id, existing);
    assert_eq!(rows[0].title, "User title");
    assert_eq!(count(&db, "track"), 2);
    assert_eq!(count(&db, "library_membership"), 1);
    assert_eq!(count(&db, "track_external_identity"), 3);
    assert_eq!(rows[0].duration_ms, Some(180000));
    assert!(matches!(
        l.playback_route(&existing, &remote()).unwrap(),
        Route::Local(_)
    ));
    assert!(matches!(
        l.playback_route(&rows[1].track.as_ref().unwrap().track_id, &remote())
            .unwrap(),
        Route::Remote(_)
    ));
    let (queue, start) = l
        .library_queue_reader()
        .unwrap()
        .read_playlist(&id, Some(&rows[2].id))
        .unwrap();
    assert_eq!(start, 2);
    assert_eq!(
        queue.iter().map(|r| r.track_id.clone()).collect::<Vec<_>>(),
        vec![
            existing.clone(),
            rows[1].track.as_ref().unwrap().track_id.clone(),
            existing.clone()
        ]
    );
    assert_eq!(
        l.browse(&Request {
            limit: 200,
            ..Default::default()
        })
        .unwrap()[0]
            .id,
        existing.as_ref()
    );
    p.items = vec![item(1), item(0), item(1)];
    p.version = Some("replacement".into());
    l.resolve_playlist_import(
        &p,
        &music_library::playlist_import::Decision::Overwrite(id.clone()),
    )
    .unwrap();
    assert_eq!(
        l.playlist_entries(&id, None, 200).unwrap()[1]
            .track
            .as_ref()
            .unwrap()
            .track_id,
        existing
    );
    assert_eq!(count(&db, "library_membership"), 1);
    assert_eq!(count(&db, "track_source"), 1);
    assert_eq!(count(&db, "track_external_identity"), 3);
    assert!(matches!(
        l.playback_route(&existing, &remote()).unwrap(),
        Route::Local(_)
    ));
}
#[test]
fn persistence_failure_rolls_back_everything_and_keeps_existing_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("library.sqlite");
    let mut l = Library::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    let first = imported(
        &mut l,
        &Plan {
            items: vec![item(0)],
            ..plan()
        },
    );
    let before = l.playlist_entries(&first, None, 20).unwrap()[0].duration_ms;
    db.execute_batch("CREATE TRIGGER fail_import BEFORE INSERT ON playlist_entry WHEN NEW.position=2 BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    let mut p = plan();
    p.external_id = "new source".into();
    p.items[0].duration = Some(Duration {
        milliseconds: 1,
        approximate: false,
    });
    assert!(l.import_playlist_snapshot(&p).is_err());
    assert_eq!(count(&db, "playlist"), 1);
    assert_eq!(count(&db, "playlist_source"), 1);
    assert_eq!(count(&db, "playlist_entry"), 1);
    assert_eq!(count(&db, "track"), 1);
    assert_eq!(count(&db, "library_membership"), 0);
    assert_eq!(
        l.playlist_entries(&first, None, 20).unwrap()[0].duration_ms,
        before
    );
    db.execute_batch("DROP TRIGGER fail_import;").unwrap();
    let id = imported(&mut l, &p);
    assert_eq!(l.playlist_entries(&id, None, 200).unwrap().len(), 20);
}
#[test]
fn migration_provenance_is_atomic_and_removing_playlist_releases_source_key() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("library.sqlite");
    let mut l = Library::open(&path).unwrap();
    let p = plan();
    let id = imported(&mut l, &p);
    l.delete_playlist(&id).unwrap();
    assert!(
        l.imported_playlist("spotify", "playlist")
            .unwrap()
            .is_none()
    );
    imported(&mut l, &p);
    drop(l);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("DROP TABLE playlist_source; PRAGMA user_version=22; CREATE TABLE playlist_source(sentinel TEXT);").unwrap();
    assert!(Library::open(&path).is_err());
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        22
    );
}
#[test]
fn manual_and_accepted_program_identities_reuse_tracks_and_ambiguity_is_safe() {
    use music_library::{
        album_program::{Match, RecordingStatus},
        edition::TrackEvidence,
        manual_track::Candidate,
    };
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("library.sqlite");
    let mut l = Library::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    let tracks = l
        .create_catalog_release(&CatalogReleaseInput {
            title: "Existing edition".into(),
            year: None,
            artists: vec![],
            tracks: (0..2)
                .map(|_| CatalogTrackInput {
                    title: "Existing".into(),
                    artists: vec![],
                    disc_number: None,
                    track_number: None,
                })
                .collect(),
        })
        .unwrap();
    let album = l.album_for_release(&tracks.release_id).unwrap();
    l.attach_album_external_identity(&album.album_id, &identity("album", "known album"))
        .unwrap();
    let candidate = Candidate {
        evidence: TrackEvidence {
            identities: vec![item(0).identity],
            ..Default::default()
        },
        supporting_programs: 1,
    };
    let matched = Match {
        duration: None,
        title: "Existing".into(),
        recording: Default::default(),
        recording_status: RecordingStatus::NotProvided,
        occurrences: vec![item(1).identity],
        explanation: "Accepted program".into(),
    };
    db.execute(
        "INSERT INTO manual_track_association VALUES(?1,'spotify','album','known album',?2)",
        params![
            tracks.track_ids[0].as_ref(),
            serde_json::to_string(&candidate).unwrap()
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO provider_track_association VALUES(?1,'spotify','album','known album',?2)",
        params![
            tracks.track_ids[1].as_ref(),
            serde_json::to_string(&matched).unwrap()
        ],
    )
    .unwrap();
    let p = Plan {
        items: vec![item(0), item(1)],
        ..plan()
    };
    let id = imported(&mut l, &p);
    let rows = l.playlist_entries(&id, None, 200).unwrap();
    assert_eq!(
        rows[0].track.as_ref().unwrap().track_id,
        tracks.track_ids[0]
    );
    assert_eq!(
        rows[1].track.as_ref().unwrap().track_id,
        tracks.track_ids[1]
    );
    assert_eq!(count(&db, "track"), 2);
    for t in &tracks.track_ids {
        assert!(matches!(
            l.playback_route(t, &remote()).unwrap(),
            Route::Remote(_)
        ));
    }
    l.attach_track_external_identity(&tracks.track_ids[1], &item(0).identity)
        .unwrap();
    let p = Plan {
        external_id: "ambiguous source".into(),
        ..p
    };
    assert!(l.import_playlist_snapshot(&p).is_err());
    assert_eq!(count(&db, "playlist"), 1);
}

#[test]
fn explicit_conflicts_rename_and_ambiguous_overwrite_keep_local_identity() {
    use music_library::playlist_import::Decision;
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("library.sqlite");
    let mut library = Library::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    let p = plan();
    assert!(library.playlist_import_conflicts(&p).unwrap().is_empty());
    let original = imported(&mut library, &p);
    let before = library.playlist_entries(&original, None, 200).unwrap();
    let conflicts = library.playlist_import_conflicts(&p).unwrap();
    assert_eq!(conflicts.len(), 1);
    assert!(conflicts[0].same_source);
    // Cancel requires no persistence. A rejected rename also leaves everything intact.
    assert!(
        library
            .resolve_playlist_import(&p, &Decision::Create)
            .is_err()
    );
    assert!(
        library
            .resolve_playlist_import(&p, &Decision::Rename(" ".into()))
            .is_err()
    );
    assert!(
        library
            .resolve_playlist_import(&p, &Decision::Rename(p.name.clone()))
            .is_err()
    );
    assert_eq!(
        library.playlist_entries(&original, None, 200).unwrap()[0].id,
        before[0].id
    );
    let Outcome::Imported {
        playlist_id: copy, ..
    } = library
        .resolve_playlist_import(&p, &Decision::Rename("Recommendations Spotify".into()))
        .unwrap()
    else {
        panic!()
    };
    assert_ne!(copy, original);
    assert_eq!(
        db.query_row(
            "SELECT external_id FROM playlist_source WHERE playlist_id=?1",
            [&copy],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        p.external_id
    );
    assert_eq!(library.playlist_import_conflicts(&p).unwrap().len(), 2);
    assert!(
        library
            .resolve_playlist_import(&p, &Decision::Overwrite(original.clone()))
            .is_err()
    );
    assert_eq!(count(&db, "playlist"), 2);
    drop(library);
    let library = Library::open(&path).unwrap();
    assert_eq!(
        library
            .playlist_details(&copy)
            .unwrap()
            .unwrap()
            .entry_count,
        20
    );
    assert_eq!(count(&db, "library_membership"), 0);
}

#[test]
fn overwrite_replaces_entries_and_provenance_transactionally_without_removing_membership() {
    use music_library::playlist_import::Decision;
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("library.sqlite");
    let mut library = Library::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    let mut p = plan();
    let id = imported(&mut library, &p);
    let old = library.playlist_entries(&id, None, 200).unwrap();
    let removed = old[0].track.as_ref().unwrap().track_id.clone();
    library.add_to_library(&removed).unwrap();
    library.remove_playlist_entry(&id, &old[1].id).unwrap();
    p.items = vec![item(3), item(2), item(3)];
    p.version = Some("new snapshot".into());
    db.execute_batch("CREATE TRIGGER fail_snapshot BEFORE INSERT ON playlist_entry WHEN NEW.position=1 BEGIN SELECT RAISE(ABORT,'simulated failure'); END;").unwrap();
    assert!(
        library
            .resolve_playlist_import(&p, &Decision::Overwrite(id.clone()))
            .is_err()
    );
    assert_eq!(
        library.playlist_details(&id).unwrap().unwrap().entry_count,
        19
    );
    assert_eq!(
        db.query_row(
            "SELECT version FROM playlist_source WHERE playlist_id=?1",
            [&id],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "version"
    );
    db.execute_batch("DROP TRIGGER fail_snapshot;").unwrap();
    let result = library
        .resolve_playlist_import(&p, &Decision::Overwrite(id.clone()))
        .unwrap();
    assert_eq!(
        result,
        Outcome::Imported {
            playlist_id: id.clone(),
            entries: 3
        }
    );
    let rows = library.playlist_entries(&id, None, 200).unwrap();
    assert_eq!(
        rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
        vec!["Song 3", "Song 2", "Song 3"]
    );
    assert_ne!(rows[0].id, rows[2].id);
    assert_eq!(
        rows[0].track.as_ref().unwrap().track_id,
        rows[2].track.as_ref().unwrap().track_id
    );
    assert_eq!(count(&db, "track"), 20);
    assert_eq!(count(&db, "library_membership"), 1);
    assert_eq!(
        library
            .browse(&Request {
                pane: Pane::Songs,
                ..Default::default()
            })
            .unwrap()[0]
            .id,
        removed.as_ref()
    );
    assert_eq!(
        db.query_row(
            "SELECT version FROM playlist_source WHERE playlist_id=?1",
            [&id],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "new snapshot"
    );
}

#[test]
fn title_collision_is_separate_from_source_and_requires_explicit_target() {
    use music_library::playlist_import::Decision;
    let mut library = Library::open_in_memory().unwrap();
    let p = plan();
    let local = library.create_playlist(&p.name).unwrap();
    let conflicts = library.playlist_import_conflicts(&p).unwrap();
    assert_eq!(conflicts[0].playlist_id, local);
    assert!(!conflicts[0].same_source);
    assert!(
        library
            .resolve_playlist_import(&p, &Decision::Create)
            .is_err()
    );
    let other = library.create_playlist(&p.name).unwrap();
    assert!(
        library
            .resolve_playlist_import(&p, &Decision::Overwrite(local.clone()))
            .is_err()
    );
    library.delete_playlist(&other).unwrap();
    library
        .resolve_playlist_import(&p, &Decision::Overwrite(local.clone()))
        .unwrap();
    assert_eq!(
        library
            .playlist_details(&local)
            .unwrap()
            .unwrap()
            .entry_count,
        20
    );
}

#[test]
fn provenance_copy_migration_preserves_existing_snapshot_and_is_atomic() {
    use music_library::playlist_import::Decision;
    for fail in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("library.sqlite");
        let mut library = Library::open(&path).unwrap();
        let p = plan();
        let id = imported(&mut library, &p);
        drop(library);
        let db = Connection::open(&path).unwrap();
        metadata_schema::downgrade(&db);
        db.execute_batch(include_str!("support/drop_playlist_revision.sql"))
            .unwrap();
        db.execute_batch("DROP INDEX playlist_source_external; ALTER TABLE playlist_source RENAME TO saved_source;").unwrap();
        db.execute_batch(include_str!("../migrations/0023_playlist_import.sql"))
            .unwrap();
        db.execute_batch(
            "INSERT INTO playlist_source SELECT * FROM saved_source; DROP TABLE saved_source;",
        )
        .unwrap();
        if fail {
            db.execute_batch("CREATE TABLE playlist_source_old(sentinel TEXT);")
                .unwrap();
        }
        let reopened = Library::open(&path);
        if fail {
            assert!(reopened.is_err());
            assert_eq!(
                db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                    .unwrap(),
                23
            );
            assert_eq!(count(&db, "playlist_source"), 1);
            assert_eq!(count(&db, "playlist_entry"), 20);
        } else {
            let mut library = reopened.unwrap();
            assert_eq!(
                library
                    .imported_playlist("spotify", &p.external_id)
                    .unwrap(),
                Some(id)
            );
            library
                .resolve_playlist_import(&p, &Decision::Rename("Explicit copy".into()))
                .unwrap();
            assert_eq!(count(&db, "playlist_source"), 2);
            assert_eq!(
                db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                    .unwrap(),
                30
            );
            assert_eq!(
                db.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
                0
            );
        }
    }
}

#[test]
fn existing_snapshot_reconciles_with_full_program_and_trusted_artist_without_touching_entry_or_queue()
 {
    use music_library::{
        album_program::{Program, Programs},
        edition::{ArtistEvidence, TrackEvidence},
    };
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.sqlite");
    let mut l = Library::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    let credit = ArtistCreditInput {
        name: "Playlist artist".into(),
        role: None,
    };
    let local = l
        .create_catalog_release(&CatalogReleaseInput {
            title: "Spotify edition".into(),
            year: Some(2001),
            artists: vec![credit.clone()],
            tracks: vec![CatalogTrackInput {
                title: "Song 0".into(),
                artists: vec![credit],
                disc_number: Some(1),
                track_number: Some(1),
            }],
        })
        .unwrap();
    let track = &local.track_ids[0];
    l.add_to_library(track).unwrap();
    let root = l.register_local_root(temp.path()).unwrap();
    let file = temp.path().join("music.flac");
    std::fs::write(&file, b"fixture").unwrap();
    db.execute(
        "INSERT INTO playable_source(id,kind) VALUES('local','local_file')",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO track_source(track_id,source_id) VALUES(?1,'local')",
        [track.as_ref()],
    )
    .unwrap();
    db.execute("INSERT INTO local_file_observation(source_id,root_id,path,size_bytes,modified_ns,available) VALUES('local',?1,?2,7,1,1)",params![root.as_ref(),file.to_string_lossy().as_bytes()]).unwrap();
    let mut p = plan();
    p.items = vec![item(0), item(0), item(1)];
    let id = imported(&mut l, &p);
    let before = l.playlist_entries(&id, None, 200).unwrap();
    let queue = l
        .library_queue_reader()
        .unwrap()
        .read_playlist(&id, None)
        .unwrap()
        .0;
    assert_ne!(before[0].track.as_ref().unwrap().track_id, *track);
    let artist = ArtistEvidence {
        identities: vec![identity("artist", "artist")],
        name: "Playlist artist".into(),
        join_phrase: String::new(),
    };
    let mut programs = Programs {
        album: identity("album", "album"),
        programs: vec![Program {
            identity: Some(identity("album", "album")),
            complete: true,
            tracks: vec![TrackEvidence {
                identities: vec![item(0).identity],
                disc: Some(1),
                number: Some(1),
                title: Some("Song 0".into()),
                artists: vec![artist],
                duration_ms: Some(180000),
                ..Default::default()
            }],
        }],
        note: String::new(),
    };
    // Display metadata alone cannot vote for a trusted Artist association.
    assert_eq!(
        l.reconcile_playlist_album_program(&id, &local.release_id, &programs)
            .unwrap(),
        0
    );
    let artists:Vec<String>=db.prepare("SELECT artist_id FROM track_artist_credit WHERE track_id=?1 UNION SELECT artist_id FROM album_artist_credit WHERE album_id=(SELECT album_id FROM release WHERE id=?2)").unwrap().query_map(params![track.as_ref(),local.release_id.as_ref()],|r|r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
    for id in artists {
        l.attach_artist_external_identity(&ArtistId(id), &identity("artist", "artist"))
            .unwrap();
    }
    programs.programs[0].complete = false;
    assert_eq!(
        l.reconcile_playlist_album_program(&id, &local.release_id, &programs)
            .unwrap(),
        0
    );
    programs.programs[0].complete = true;
    programs.programs[0].tracks[0].disc = Some(2);
    assert_eq!(
        l.reconcile_playlist_album_program(&id, &local.release_id, &programs)
            .unwrap(),
        0
    );
    programs.programs[0].tracks[0].disc = Some(1);
    l.mark_not_on_spotify(track).unwrap();
    assert!(l.playlist_reconciliation_programs(&id).unwrap().is_empty());
    assert_eq!(
        l.reconcile_playlist_album_program(&id, &local.release_id, &programs)
            .unwrap(),
        0
    );
    assert!(l.spotify_manually_excluded(track).unwrap());
    assert_eq!(
        l.playlist_entries(&id, None, 200).unwrap()[0]
            .track
            .as_ref()
            .unwrap()
            .track_id,
        before[0].track.as_ref().unwrap().track_id
    );
    l.check_spotify_again(track).unwrap();
    db.execute_batch("CREATE TRIGGER reject_repoint BEFORE UPDATE OF track_id ON playlist_entry BEGIN SELECT RAISE(ABORT,'test entry failure'); END;").unwrap();
    assert!(
        l.reconcile_playlist_album_program(&id, &local.release_id, &programs)
            .is_err()
    );
    assert_eq!(
        l.playlist_entries(&id, None, 200).unwrap()[0]
            .track
            .as_ref()
            .unwrap()
            .track_id,
        before[0].track.as_ref().unwrap().track_id
    );
    db.execute_batch("DROP TRIGGER reject_repoint").unwrap();
    assert_eq!(l.reconcile_playlist_tracks(&id).unwrap(), 2);
    let after = l.playlist_entries(&id, None, 200).unwrap();
    assert_eq!(
        after
            .iter()
            .map(|r| (&r.id, r.playlist_position))
            .collect::<Vec<_>>(),
        before
            .iter()
            .map(|r| (&r.id, r.playlist_position))
            .collect::<Vec<_>>()
    );
    assert_eq!(after[0].track.as_ref().unwrap().track_id, *track);
    assert_eq!(after[1].track.as_ref().unwrap().track_id, *track);
    assert_eq!(
        after[2].track.as_ref().unwrap().track_id,
        before[2].track.as_ref().unwrap().track_id
    );
    assert_eq!(count(&db, "library_membership"), 1);
    assert!(matches!(
        l.playback_route(track, &remote()).unwrap(),
        Route::Local(_)
    ));
    assert_eq!(
        l.track_provider_occurrences(track, "spotify").unwrap(),
        vec![item(0).identity]
    );
    assert_eq!(
        queue[0].track_id,
        before[0].track.as_ref().unwrap().track_id
    ); // Existing queue snapshot remains intact.
    let second = Plan {
        external_id: "second".into(),
        name: "Second".into(),
        ..p.clone()
    };
    let second = imported(&mut l, &second);
    assert_eq!(
        l.playlist_entries(&second, None, 200).unwrap()[0]
            .track
            .as_ref()
            .unwrap()
            .track_id,
        *track
    );
    std::fs::remove_file(file).unwrap();
    assert!(matches!(
        l.playback_route(track, &remote()).unwrap(),
        Route::Remote(_)
    ));
    drop(l);
    let l = Library::open(&path).unwrap();
    assert_eq!(
        l.playlist_entries(&id, None, 200).unwrap()[0]
            .track
            .as_ref()
            .unwrap()
            .track_id,
        *track
    );
}
