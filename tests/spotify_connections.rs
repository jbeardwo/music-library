use music_library::{
    Library,
    browse::{Request, SongColumn},
    domain::*,
};
fn release(l: &mut Library, title: &str, saved: bool) -> ImportedRelease {
    let r = l
        .create_catalog_release(&CatalogReleaseInput {
            title: title.into(),
            year: None,
            artists: vec![],
            tracks: (0..3)
                .map(|n| CatalogTrackInput {
                    title: format!("{title} {n}"),
                    disc_number: Some(1),
                    track_number: Some(n + 1),
                    artists: vec![],
                })
                .collect(),
        })
        .unwrap();
    if saved {
        for t in &r.track_ids {
            l.add_to_library(t).unwrap();
        }
    }
    r
}
fn request() -> Request {
    Request {
        unresolved_spotify: true,
        limit: 201,
        ..Default::default()
    }
}
#[test]
fn saved_canonical_tracks_only_independent_of_sources_and_playlist_occurrences() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("library.sqlite");
    let mut l = Library::open(&path).unwrap();
    let saved = release(&mut l, "Saved", true);
    let unsaved = release(&mut l, "Playlist only", false);
    let playlist = l.create_playlist("Duplicates").unwrap();
    for _ in 0..2 {
        l.append_playlist_track(&playlist, &saved.track_ids[0])
            .unwrap();
        l.append_playlist_track(&playlist, &unsaved.track_ids[0])
            .unwrap();
    }
    // Non-Spotify identity and no local audio are valid Library states.
    l.attach_track_external_identity(
        &saved.track_ids[1],
        &ExternalIdentity {
            provider: "musicbrainz".into(),
            kind: "track".into(),
            external_id: "mb-track".into(),
        },
    )
    .unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "INSERT INTO playable_source(id,kind) VALUES('missing-source','local_file')",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO track_source(track_id,source_id) VALUES(?1,'missing-source')",
        [saved.track_ids[2].as_ref()],
    )
    .unwrap();
    db.execute("INSERT INTO local_file_observation(source_id,path,size_bytes,modified_ns,available) VALUES('missing-source',x'2f6d697373696e67',1,1,0)",[]).unwrap();
    let rows = l.browse(&request()).unwrap();
    assert_eq!(rows.len(), 3);
    db.execute("UPDATE local_file_observation SET available=1", [])
        .unwrap();
    assert_eq!(l.browse(&request()).unwrap().len(), 3);
    db.execute("UPDATE local_file_observation SET available=0", [])
        .unwrap();
    assert_eq!(l.unresolved_spotify_count().unwrap(), 3);
    assert!(
        rows.iter()
            .all(|r| r.connection_reason_code == "not_evaluated")
    );
    l.attach_track_external_identity(
        &saved.track_ids[0],
        &ExternalIdentity {
            provider: "spotify".into(),
            kind: "track".into(),
            external_id: "spotify-track".into(),
        },
    )
    .unwrap();
    assert_eq!(l.browse(&request()).unwrap().len(), 2);
    assert_eq!(l.unresolved_spotify_count().unwrap(), 2);
    assert!(
        l.browse(&Request {
            ids: vec![saved.track_ids[0].0.clone()],
            ..request()
        })
        .unwrap()
        .is_empty()
    );
    drop(l);
    let l = Library::open(path).unwrap();
    assert_eq!(l.unresolved_spotify_count().unwrap(), 2);
}
#[test]
fn persisted_structured_reason_sort_filter_and_keyset_pages() {
    use music_library::album_candidates::{Reason, Report};
    let mut l = Library::open_in_memory().unwrap();
    let a = release(&mut l, "First", true);
    let b = release(&mut l, "Second", true);
    let album = l.album_id_for_track(&a.track_ids[0]).unwrap();
    let report = Report {
        local_title: "First".into(),
        established_artist: None,
        candidate_count: 0,
        more_candidates: false,
        candidates: vec![],
        decision: "Withheld".into(),
        reasons: vec![Reason::TrackCountMismatch],
    };
    l.persist_spotify_review(&album, &report).unwrap();
    for column in [
        SongColumn::Song,
        SongColumn::Artist,
        SongColumn::Album,
        SongColumn::Reason,
    ] {
        for descending in [false, true] {
            let mut req = Request {
                song_column: Some(column),
                descending,
                limit: 2,
                ..request()
            };
            let mut ids = vec![];
            loop {
                let rows = l.browse(&req).unwrap();
                if rows.is_empty() {
                    break;
                }
                req.after = rows.last().map(|r| r.cursor.clone());
                ids.extend(rows.into_iter().map(|r| r.id));
            }
            let rows = l
                .browse(&Request {
                    after: None,
                    limit: 201,
                    ..req.clone()
                })
                .unwrap();
            let keys: Vec<_> = rows.iter().map(|r| (&r.cursor.title, &r.id)).collect();
            assert!(keys.windows(2).all(|w| if descending {
                w[0] >= w[1]
            } else {
                w[0] <= w[1]
            }));
            let back = l
                .browse_before(&Request {
                    after: rows.last().map(|r| r.cursor.clone()),
                    limit: 2,
                    ..req
                })
                .unwrap();
            assert_eq!(back[0].id, rows[rows.len() - 2].id);
            ids.sort();
            ids.dedup();
            assert_eq!(ids.len(), 6);
        }
    }
    let rows = l
        .browse(&Request {
            album: Some(album),
            song_column: Some(SongColumn::Reason),
            ..request()
        })
        .unwrap();
    assert_eq!(rows.len(), 3);
    assert!(
        rows.iter()
            .all(|r| r.connection_reason_code == "track_count_mismatch"
                && r.connection_reason == "Album program mismatch")
    );
    assert!(
        rows.iter()
            .all(|r| !b.track_ids.iter().any(|t| t.as_ref() == r.id))
    );
}
#[test]
fn manual_and_album_occurrences_obey_existing_trust_precedence() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("library.sqlite");
    let mut l = Library::open(&path).unwrap();
    let saved = release(&mut l, "Saved", true);
    let album = l.album_id_for_track(&saved.track_ids[0]).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "INSERT INTO album_external_identity VALUES(?1,'spotify','album','accepted')",
        [album.as_ref()],
    )
    .unwrap();
    let occurrences =
        r#"{"occurrences":[{"provider":"spotify","kind":"track","external_id":"song"}]}"#;
    for t in &saved.track_ids[..2] {
        db.execute(
            "INSERT INTO provider_track_association VALUES(?1,'spotify','album','accepted',?2)",
            rusqlite::params![t.as_ref(), occurrences],
        )
        .unwrap();
    }
    assert_eq!(l.unresolved_spotify_count().unwrap(), 1);
    // An obsolete Album representation no longer grants trust.
    db.execute(
        "DELETE FROM album_external_identity WHERE album_id=?1",
        [album.as_ref()],
    )
    .unwrap();
    assert_eq!(l.unresolved_spotify_count().unwrap(), 3);
    db.execute("INSERT INTO manual_track_association VALUES(?1,'spotify','album','manual',?2)",rusqlite::params![saved.track_ids[0].as_ref(),r#"{"evidence":{"identities":[{"provider":"spotify","kind":"track","external_id":"manual-song"}]}}"#]).unwrap();
    assert_eq!(l.unresolved_spotify_count().unwrap(), 2);
    l.attach_track_external_identity(
        &saved.track_ids[1],
        &ExternalIdentity {
            provider: "spotify".into(),
            kind: "track".into(),
            external_id: "durable".into(),
        },
    )
    .unwrap();
    assert_eq!(l.browse(&request()).unwrap().len(), 1);
    // Manual decisions take precedence over independent/automatic evidence.
    db.execute(
        "UPDATE manual_track_association SET candidate_json='{\"evidence\":{\"identities\":[]}}'",
        [],
    )
    .unwrap();
    assert_eq!(l.unresolved_spotify_count().unwrap(), 2);
}
#[test]
fn v25_upgrade_backfills_review_without_changing_membership_or_trust() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("library.sqlite");
    let mut l = Library::open(&path).unwrap();
    let saved = release(&mut l, "Saved", true);
    l.attach_track_external_identity(
        &saved.track_ids[0],
        &ExternalIdentity {
            provider: "spotify".into(),
            kind: "track".into(),
            external_id: "trusted".into(),
        },
    )
    .unwrap();
    drop(l);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("DROP TRIGGER IF EXISTS spotify_exclusion_unconnected;
DROP TRIGGER IF EXISTS lifecycle_track_provider_evidence_insert;
DROP TRIGGER IF EXISTS lifecycle_track_provider_evidence_update;
DROP TRIGGER IF EXISTS lifecycle_track_provider_evidence_delete;
DROP TRIGGER IF EXISTS lifecycle_track_application_metadata_insert;
DROP TRIGGER IF EXISTS lifecycle_track_application_metadata_update;
DROP TRIGGER IF EXISTS lifecycle_track_application_metadata_delete;
DROP TRIGGER IF EXISTS lifecycle_track_title_override_insert;
DROP TRIGGER IF EXISTS lifecycle_track_title_override_update;
DROP TRIGGER IF EXISTS lifecycle_track_title_override_delete;
DROP TRIGGER IF EXISTS lifecycle_track_external_identity_insert;
DROP TRIGGER IF EXISTS lifecycle_track_external_identity_update;
DROP TRIGGER IF EXISTS lifecycle_track_external_identity_delete;
DROP TRIGGER IF EXISTS lifecycle_track_artist_credit_insert;
DROP TRIGGER IF EXISTS lifecycle_track_artist_credit_update;
DROP TRIGGER IF EXISTS lifecycle_track_artist_credit_delete;
DROP TRIGGER IF EXISTS lifecycle_track_duration_observation_insert;
DROP TRIGGER IF EXISTS lifecycle_track_duration_observation_update;
DROP TRIGGER IF EXISTS lifecycle_track_duration_observation_delete;
DROP TRIGGER IF EXISTS lifecycle_track_source_insert;
DROP TRIGGER IF EXISTS lifecycle_track_source_update;
DROP TRIGGER IF EXISTS lifecycle_track_source_delete;
DROP TRIGGER IF EXISTS lifecycle_album_provider_evidence_insert;
DROP TRIGGER IF EXISTS lifecycle_album_provider_evidence_update;
DROP TRIGGER IF EXISTS lifecycle_album_provider_evidence_delete;
DROP TRIGGER IF EXISTS lifecycle_album_application_metadata_insert;
DROP TRIGGER IF EXISTS lifecycle_album_application_metadata_update;
DROP TRIGGER IF EXISTS lifecycle_album_application_metadata_delete;
DROP TRIGGER IF EXISTS lifecycle_album_external_identity_insert;
DROP TRIGGER IF EXISTS lifecycle_album_external_identity_update;
DROP TRIGGER IF EXISTS lifecycle_album_external_identity_delete;
DROP TRIGGER IF EXISTS lifecycle_album_artist_credit_insert;
DROP TRIGGER IF EXISTS lifecycle_album_artist_credit_update;
DROP TRIGGER IF EXISTS lifecycle_album_artist_credit_delete;
DROP TRIGGER IF EXISTS lifecycle_release_external_identity_insert;
DROP TRIGGER IF EXISTS lifecycle_release_external_identity_update;
DROP TRIGGER IF EXISTS lifecycle_release_external_identity_delete;
DROP TRIGGER IF EXISTS lifecycle_release_artist_credit_insert;
DROP TRIGGER IF EXISTS lifecycle_release_artist_credit_update;
DROP TRIGGER IF EXISTS lifecycle_release_artist_credit_delete;
DROP TRIGGER IF EXISTS lifecycle_file_metadata_observation_insert;
DROP TRIGGER IF EXISTS lifecycle_file_metadata_observation_update;
DROP TRIGGER IF EXISTS lifecycle_file_metadata_observation_delete;
DROP TRIGGER IF EXISTS lifecycle_file_artist_observation_insert;
DROP TRIGGER IF EXISTS lifecycle_file_artist_observation_update;
DROP TRIGGER IF EXISTS lifecycle_file_artist_observation_delete;
DROP TRIGGER IF EXISTS lifecycle_artist_insert;
DROP TRIGGER IF EXISTS lifecycle_artist_update;
DROP TRIGGER IF EXISTS lifecycle_artist_delete;
DROP TRIGGER IF EXISTS lifecycle_artist_external_identity_insert;
DROP TRIGGER IF EXISTS lifecycle_artist_external_identity_update;
DROP TRIGGER IF EXISTS lifecycle_artist_external_identity_delete;
DROP TRIGGER IF EXISTS lifecycle_artist_equivalence_insert;
DROP TRIGGER IF EXISTS lifecycle_artist_equivalence_update;
DROP TRIGGER IF EXISTS lifecycle_artist_equivalence_delete;
DROP TRIGGER IF EXISTS lifecycle_track_insert;
DROP TRIGGER IF EXISTS lifecycle_track_update;
DROP TRIGGER IF EXISTS lifecycle_track_delete;
DROP TRIGGER IF EXISTS lifecycle_release_relationship;
DROP TRIGGER IF EXISTS lifecycle_connected_track_external_identity_insert;
DROP TRIGGER IF EXISTS lifecycle_connected_track_external_identity_update;
DROP TRIGGER IF EXISTS lifecycle_connected_manual_track_association_insert;
DROP TRIGGER IF EXISTS lifecycle_connected_manual_track_association_update;
DROP TRIGGER IF EXISTS lifecycle_connected_provider_track_association_insert;
DROP TRIGGER IF EXISTS lifecycle_connected_provider_track_association_update;
DROP TRIGGER IF EXISTS spotify_exclusion_update_unconnected;
DROP TRIGGER IF EXISTS lifecycle_exclusion_added;
DROP TRIGGER IF EXISTS lifecycle_exclusion_removed;
DROP TRIGGER IF EXISTS lifecycle_album_identity_connected_insert;
DROP TRIGGER IF EXISTS lifecycle_album_identity_connected_update;
DROP TRIGGER IF EXISTS lifecycle_manual_track_association_removed;
DROP TRIGGER IF EXISTS lifecycle_provider_track_association_removed;
DROP TRIGGER IF EXISTS lifecycle_recording_identity_insert;
DROP TRIGGER IF EXISTS lifecycle_recording_identity_update;
DROP TRIGGER IF EXISTS lifecycle_recording_identity_delete;
DROP TABLE IF EXISTS spotify_program_cache;
DROP TABLE IF EXISTS spotify_reconciliation_cache;
DROP TABLE IF EXISTS track_provider_exclusion;
DROP TABLE track_provider_evidence; DROP TABLE album_provider_evidence; DROP TABLE artist_equivalence; DROP INDEX spotify_review_manual; DROP INDEX spotify_review_automatic; DROP VIEW trusted_spotify_track; DROP TRIGGER spotify_review_track_added; DROP TABLE spotify_connection_review; PRAGMA user_version=25;").unwrap();
    let l = Library::open(&path).unwrap();
    assert_eq!(l.unresolved_spotify_count().unwrap(), 2);
    assert_eq!(l.library_queue(&Default::default()).unwrap().len(), 3);
    assert_eq!(
        db.query_row("SELECT count(*) FROM spotify_connection_review", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap(),
        3
    );
    assert!(
        l.browse(&request())
            .unwrap()
            .iter()
            .all(|r| r.connection_reason_code == "not_evaluated")
    );
}
#[test]
fn explicit_song_attempt_persists_local_summary_without_provider_work() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("library.sqlite");
    let mut l = Library::open(&path).unwrap();
    let saved = release(&mut l, "Saved", true);
    let input = l.song_resolution_input(&saved.track_ids[0]).unwrap();
    l.persist_spotify_song_review(
        &input,
        &music_library::catalog::Page {
            items: vec![],
            next_offset: None,
        },
    )
    .unwrap();
    drop(l);
    let l = Library::open(&path).unwrap();
    let row = l
        .browse(&Request {
            ids: vec![saved.track_ids[0].0.clone()],
            ..request()
        })
        .unwrap()
        .remove(0);
    assert_eq!(row.connection_reason_code, "no_candidates");
    assert_eq!(row.connection_reason, "No Spotify candidate");
    assert_eq!(l.unresolved_spotify_count().unwrap(), 3);
}
