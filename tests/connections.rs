use music_library::{Library, domain::*};
fn album(l: &mut Library, title: &str, artist: &str) -> ImportedRelease {
    let r = l
        .create_catalog_release(&CatalogReleaseInput {
            title: title.into(),
            year: None,
            artists: vec![ArtistCreditInput {
                name: artist.into(),
                role: None,
            }],
            tracks: (1..=3)
                .map(|n| CatalogTrackInput {
                    title: format!("Track {n}"),
                    disc_number: Some(1),
                    track_number: Some(n),
                    artists: vec![ArtistCreditInput {
                        name: artist.into(),
                        role: None,
                    }],
                })
                .collect(),
        })
        .unwrap();
    for t in &r.track_ids {
        l.add_to_library(t).unwrap();
    }
    r
}
#[test]
fn provider_scoped_snapshot_actions_counts_and_immediate_queries() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let mut l = Library::open(&path).unwrap();
    let r = album(&mut l, "Animals", "TTNG");
    let a = l.album_id_for_track(&r.track_ids[0]).unwrap();
    l.attach_track_external_identity(
        &r.track_ids[0],
        &ExternalIdentity {
            provider: "spotify".into(),
            kind: "track".into(),
            external_id: "existing".into(),
        },
    )
    .unwrap();
    let groups = l.connection_groups("spotify", false, None, 100).unwrap();
    assert_eq!(
        (groups[0].matched, groups[0].total, groups[0].relevant),
        (1, 3, 2)
    );
    assert_eq!(
        l.connection_tracks("spotify", false, &a, None, 200)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        l.mark_connection_unavailable("spotify", Some(&a), None)
            .unwrap(),
        2
    );
    assert!(
        l.connection_groups("spotify", false, None, 100)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        l.connection_tracks("spotify", true, &a, None, 200)
            .unwrap()
            .len(),
        2
    );
    for provider in ["local", "musicbrainz"] {
        assert_eq!(
            l.connection_tracks(provider, false, &a, None, 200)
                .unwrap()
                .len(),
            3
        );
        assert!(
            l.connection_groups(provider, true, None, 100)
                .unwrap()
                .is_empty()
        );
    }
    l.clear_connection_unavailable("spotify", &r.track_ids[1])
        .unwrap();
    assert_eq!(
        l.connection_tracks("spotify", false, &a, None, 200)
            .unwrap()
            .len(),
        1
    );
    l.attach_track_external_identity(
        &r.track_ids[2],
        &ExternalIdentity {
            provider: "spotify".into(),
            kind: "track".into(),
            external_id: "manual".into(),
        },
    )
    .unwrap();
    assert!(
        l.connection_tracks("spotify", true, &a, None, 200)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        l.connection_coverage("spotify").unwrap().missing,
        l.unresolved_spotify_count().unwrap() as i64
    );
    let extra = album(&mut l, "Another", "TTNG");
    assert_eq!(l.connection_coverage("spotify").unwrap().marked, 0);
    assert!(!l.add_to_library(&extra.track_ids[0]).unwrap());
}
#[test]
fn local_and_recording_identity_semantics_and_clear_on_connection() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let mut l = Library::open(&path).unwrap();
    let r = album(&mut l, "Album", "Artist");
    let a = l.album_id_for_track(&r.track_ids[0]).unwrap();
    for p in ["local", "musicbrainz"] {
        l.mark_connection_unavailable(p, None, Some(&r.track_ids[0]))
            .unwrap();
    }
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "INSERT INTO playable_source(id,kind) VALUES('file','local_file')",
        [],
    )
    .unwrap();
    db.execute("INSERT INTO local_file_observation(source_id,path,size_bytes,modified_ns,available) VALUES('file',x'2f746d702f66696c65',1,1,0)",[]).unwrap();
    db.execute(
        "INSERT INTO track_source(track_id,source_id) VALUES(?1,'file')",
        [r.track_ids[0].as_ref()],
    )
    .unwrap();
    assert_eq!(l.connection_coverage("local").unwrap().connected, 0);
    db.execute("UPDATE local_file_observation SET available=1", [])
        .unwrap();
    assert_eq!(l.connection_coverage("local").unwrap().connected, 1);
    assert_eq!(l.connection_coverage("local").unwrap().marked, 0);
    assert_eq!(l.connection_coverage("musicbrainz").unwrap().marked, 1);
    // A release-specific occurrence is not a Recording identity.
    l.attach_track_external_identity(
        &r.track_ids[1],
        &ExternalIdentity {
            provider: "musicbrainz".into(),
            kind: "track".into(),
            external_id: "occurrence".into(),
        },
    )
    .unwrap();
    assert_eq!(l.connection_coverage("musicbrainz").unwrap().connected, 0);
    let recording = l.recording_for_track(&r.track_ids[0]).unwrap();
    l.attach_recording_external_identity(
        &recording.recording_id,
        &ExternalIdentity {
            provider: "musicbrainz".into(),
            kind: "recording".into(),
            external_id: "recording".into(),
        },
    )
    .unwrap();
    assert_eq!(l.connection_coverage("musicbrainz").unwrap().connected, 1);
    assert_eq!(l.connection_coverage("musicbrainz").unwrap().marked, 0);
    db.execute("UPDATE local_file_observation SET available=0", [])
        .unwrap();
    assert_eq!(
        l.connection_tracks("local", false, &a, None, 200)
            .unwrap()
            .len(),
        3
    );
    assert!(!l.add_to_library(&r.track_ids[0]).unwrap());
}
#[test]
fn artist_then_album_order_keyset_and_lossless_upgrade() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let mut l = Library::open(&path).unwrap();
    let z = album(&mut l, "A", "Zebra");
    let a = album(&mut l, "Z", "Alpha");
    let b = album(&mut l, "A", "Alpha");
    l.mark_not_on_spotify(&z.track_ids[0]).unwrap();
    let rows = l.connection_groups("local", false, None, 1).unwrap();
    assert_eq!(rows[0].id, l.album_id_for_track(&b.track_ids[0]).unwrap());
    let rows = l
        .connection_groups("local", false, Some(&rows[0].id), 1)
        .unwrap();
    assert_eq!(rows[0].id, l.album_id_for_track(&a.track_ids[0]).unwrap());
    drop(l);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("DROP INDEX connections_album_order; DROP VIEW connected_local_track; DROP VIEW connected_musicbrainz_track; DROP TRIGGER connections_local_attached; DROP TRIGGER connections_local_available; DROP TRIGGER connections_recording_attached; PRAGMA user_version=35;").unwrap();
    drop(db);
    let l = Library::open(&path).unwrap();
    assert_eq!(l.connection_coverage("spotify").unwrap().marked, 1);
    assert!(l.spotify_manually_excluded(&z.track_ids[0]).unwrap());
    assert_eq!(l.connection_coverage("spotify").unwrap().saved, 9);
}

#[test]
fn explicit_local_file_attachment_preserves_track_and_membership() {
    struct Tags;
    impl music_library::filesystem::MetadataExtractor for Tags {
        fn supports(&self, _: &std::path::Path) -> bool {
            true
        }
        fn read(&mut self, _: &std::path::Path) -> music_library::Result<ObservedMetadata> {
            Ok(ObservedMetadata {
                track_title: Some("File title".into()),
                ..Default::default()
            })
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let mut l = Library::open(&path).unwrap();
    let r = album(&mut l, "Canonical Album", "Artist");
    let file = temp.path().join("audio.wav");
    std::fs::write(&file, b"test audio observation").unwrap();
    l.mark_connection_unavailable("local", None, Some(&r.track_ids[0]))
        .unwrap();
    l.attach_local_file(&r.track_ids[0], &file, &mut Tags)
        .unwrap();
    assert_eq!(l.connection_coverage("local").unwrap().connected, 1);
    assert_eq!(l.connection_coverage("local").unwrap().marked, 0);
    assert_eq!(l.connection_coverage("local").unwrap().saved, 3);
    assert!(
        l.attach_local_file(&r.track_ids[1], &file, &mut Tags)
            .is_err()
    );
    l.attach_local_file(&r.track_ids[0], &file, &mut Tags)
        .unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM track", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM track_source", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn bulk_unavailable_does_not_inherit_on_later_membership() {
    let temp = tempfile::tempdir().unwrap();
    let mut l = Library::open(temp.path().join("db")).unwrap();
    let r = album(&mut l, "Album", "Artist");
    let a = l.album_id_for_track(&r.track_ids[0]).unwrap();
    l.remove_from_library(&r.track_ids[2]).unwrap();
    for provider in ["local", "musicbrainz", "spotify"] {
        assert_eq!(
            l.mark_connection_unavailable(provider, Some(&a), None)
                .unwrap(),
            2
        );
    }
    l.add_to_library(&r.track_ids[2]).unwrap();
    for provider in ["local", "musicbrainz", "spotify"] {
        let tracks = l.connection_tracks(provider, false, &a, None, 200).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].id, r.track_ids[2]);
    }
}

#[test]
fn explicit_musicbrainz_confirmation_rejects_conflicts_and_preserves_other_choices() {
    let temp = tempfile::tempdir().unwrap();
    let mut l = Library::open(temp.path().join("db")).unwrap();
    let r = album(&mut l, "Album", "Artist");
    let track = &r.track_ids[0];
    for p in ["local", "musicbrainz", "spotify"] {
        l.mark_connection_unavailable(p, None, Some(track)).unwrap();
    }
    let identity = ExternalIdentity {
        provider: "musicbrainz".into(),
        kind: "recording".into(),
        external_id: "correct-recording".into(),
    };
    l.confirm_musicbrainz_connection(track, &identity).unwrap();
    assert_eq!(l.connection_coverage("musicbrainz").unwrap().marked, 0);
    for p in ["local", "spotify"] {
        assert_eq!(l.connection_coverage(p).unwrap().marked, 1);
    }
    assert!(
        l.confirm_musicbrainz_connection(
            track,
            &ExternalIdentity {
                external_id: "conflict".into(),
                ..identity.clone()
            }
        )
        .is_err()
    );
    assert!(
        l.confirm_musicbrainz_connection(
            track,
            &ExternalIdentity {
                kind: "track".into(),
                ..identity
            }
        )
        .is_err()
    );
}

#[test]
fn album_recording_confirmation_is_atomic_and_library_projection_needs_no_source() {
    let temp = tempfile::tempdir().unwrap();
    let mut l = Library::open(temp.path().join("db")).unwrap();
    let r = album(&mut l, "Canonical Album", "Canonical Artist");
    let a = l.album_id_for_track(&r.track_ids[0]).unwrap();
    let known = l
        .connection_review_known(&a, Some(&r.track_ids[0]))
        .unwrap();
    assert_eq!(known["title"], "Track 1");
    assert_eq!(known["artist"], "Canonical Artist");
    assert_eq!(known["album"], "Canonical Album");
    assert_eq!(l.connection_coverage("local").unwrap().connected, 0);
    let identity = |id: &str| ExternalIdentity {
        provider: "musicbrainz".into(),
        kind: "recording".into(),
        external_id: id.into(),
    };
    l.confirm_musicbrainz_connection(&r.track_ids[1], &identity("trusted"))
        .unwrap();
    l.mark_connection_unavailable("musicbrainz", None, Some(&r.track_ids[0]))
        .unwrap();
    l.mark_connection_unavailable("spotify", None, Some(&r.track_ids[0]))
        .unwrap();
    assert!(
        l.confirm_musicbrainz_connections(&[
            (r.track_ids[0].clone(), identity("new")),
            (r.track_ids[1].clone(), identity("conflict")),
        ])
        .is_err()
    );
    assert_eq!(l.connection_coverage("musicbrainz").unwrap().connected, 1);
    assert_eq!(l.connection_coverage("musicbrainz").unwrap().marked, 1);
    assert_eq!(
        l.confirm_musicbrainz_connections(&[(r.track_ids[0].clone(), identity("new"))])
            .unwrap(),
        1
    );
    assert_eq!(l.connection_coverage("musicbrainz").unwrap().marked, 0);
    assert_eq!(l.connection_coverage("spotify").unwrap().marked, 1);
    assert_eq!(l.connection_coverage("musicbrainz").unwrap().saved, 3);
}
