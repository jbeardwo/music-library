use music_library::{
    Library,
    browse::{Pane, Request},
    domain::*,
    filesystem::{LoftyMetadataExtractor, MetadataExtractor},
    metadata::{Change, Inspection, Target},
};
use rusqlite::{Connection, params};
fn change(field: &str, value: Option<&str>) -> Change {
    Change {
        field: field.into(),
        value: value.map(str::to_owned),
    }
}
fn value(i: &Inspection, key: &str) -> String {
    i.fields
        .iter()
        .find(|f| f.key == key)
        .unwrap()
        .value
        .clone()
}
fn fixture() -> (tempfile::TempDir, Library, ImportedRelease) {
    let temp = tempfile::tempdir().unwrap();
    let mut l = Library::open(temp.path().join("db")).unwrap();
    let r = l
        .create_catalog_release(&CatalogReleaseInput {
            title: "Bad Album".into(),
            year: Some(2012),
            artists: vec![ArtistCreditInput {
                name: "Band".into(),
                role: None,
            }],
            tracks: (1..=3)
                .map(|n| CatalogTrackInput {
                    title: format!("Song {n}"),
                    artists: vec![ArtistCreditInput {
                        name: if n == 3 { "Band feat. Guest" } else { "Band" }.into(),
                        role: None,
                    }],
                    disc_number: Some(1),
                    track_number: Some(n),
                })
                .collect(),
        })
        .unwrap();
    for t in &r.track_ids {
        l.add_to_library(t).unwrap();
    }
    (temp, l, r)
}
#[test]
fn sparse_track_overrides_restart_refresh_clear_and_identity_preservation() {
    let (temp, mut l, r) = fixture();
    let t = &r.track_ids[1];
    let target = Target::Track(t.0.clone());
    let spotify = ExternalIdentity {
        provider: "spotify".into(),
        kind: "track".into(),
        external_id: "stable".into(),
    };
    l.attach_track_external_identity(t, &spotify).unwrap();
    let p = l.create_playlist("Playlist").unwrap();
    let entry = l.append_playlist_track(&p, t).unwrap();
    let before = l.inspect_metadata(&target).unwrap();
    l.save_metadata(&target, &[change("title", Some("  X’ed Out  "))], &[])
        .unwrap();
    let after = l.inspect_metadata(&target).unwrap();
    assert_eq!(value(&after, "title"), "X’ed Out");
    assert_eq!(
        value(&after, "artist_credit"),
        value(&before, "artist_credit")
    );
    assert!(after.fields[0].overridden);
    assert_eq!(after.target, target);
    assert_eq!(
        l.list_track_external_identities(t).unwrap(),
        vec![spotify.clone()]
    );
    assert!(
        l.local_search("X’ed", music_library::library_search::Kind::Song)
            .unwrap()
            .iter()
            .any(|h| h.id == t.0)
    );
    assert_eq!(
        l.browse(&Request {
            pane: Pane::Songs,
            track: Some(t.clone()),
            ..Default::default()
        })
        .unwrap()[0]
            .title,
        "X’ed Out"
    );
    drop(l);
    let mut l = Library::open(temp.path().join("db")).unwrap();
    assert_eq!(
        value(&l.inspect_metadata(&target).unwrap(), "title"),
        "X’ed Out"
    );
    let db = Connection::open(temp.path().join("db")).unwrap();
    db.execute(
        "UPDATE track_application_metadata SET title='Refreshed' WHERE track_id=?1",
        [t.as_ref()],
    )
    .unwrap();
    l.save_metadata(&target, &[change("genre", Some("Jazz"))], &[])
        .unwrap();
    assert_eq!(
        value(&l.inspect_metadata(&target).unwrap(), "title"),
        "X’ed Out"
    );
    l.save_metadata(&target, &[change("title", None)], &[])
        .unwrap();
    assert_eq!(
        value(&l.inspect_metadata(&target).unwrap(), "title"),
        "Refreshed"
    );
    assert_eq!(
        db.query_row(
            "SELECT track_id FROM playlist_entry WHERE id=?1",
            [&entry],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        t.0
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM library_membership", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(l.list_track_external_identities(t).unwrap(), vec![spotify]);
}
#[test]
fn album_shared_overrides_keep_titles_featured_credits_identities_and_playlists() {
    let (temp, mut l, r) = fixture();
    let a = l.album_for_release(&r.release_id).unwrap().album_id;
    let target = Target::Album(a.0.clone());
    let p = l.create_playlist("P").unwrap();
    let entry = l.append_playlist_track(&p, &r.track_ids[0]).unwrap();
    let identity = ExternalIdentity {
        provider: "musicbrainz".into(),
        kind: "release_group".into(),
        external_id: "group".into(),
    };
    l.attach_album_external_identity(&a, &identity).unwrap();
    l.save_metadata(
        &target,
        &[
            change("title", Some("Correct Album")),
            change("artist_credit", Some("Band, Queen Ansleis")),
            change("year", Some("2020")),
            change("genre", Some("Rock · Indie")),
        ],
        &[],
    )
    .unwrap();
    let rows = l
        .browse(&Request {
            pane: Pane::Songs,
            album: Some(a.clone()),
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(rows.len(), 3);
    for row in &rows {
        let tr = row.track.as_ref().unwrap();
        assert_eq!(tr.release_title, "Correct Album");
        assert!(tr.title.starts_with("Song"));
        assert_eq!(tr.year, Some(2020));
        assert_eq!(row.disc_number, Some(1));
    }
    assert_eq!(
        value(
            &l.inspect_metadata(&Target::Track(r.track_ids[0].0.clone()))
                .unwrap(),
            "artist_credit"
        ),
        "Band, Queen Ansleis"
    );
    assert_eq!(
        value(
            &l.inspect_metadata(&Target::Track(r.track_ids[2].0.clone()))
                .unwrap(),
            "artist_credit"
        ),
        "Band feat. Guest"
    );
    assert!(
        l.local_search("Correct Album", music_library::library_search::Kind::Album)
            .unwrap()
            .iter()
            .any(|h| h.id == a.0)
    );
    assert_eq!(l.album_for_release(&r.release_id).unwrap().album_id, a);
    assert_eq!(
        l.list_album_external_identities(&a).unwrap(),
        vec![identity]
    );
    let db = Connection::open(temp.path().join("db")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT track_id FROM playlist_entry WHERE id=?1",
            [&entry],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        r.track_ids[0].0
    );
    l.save_metadata(
        &target,
        &[
            change("title", None),
            change("artist_credit", None),
            change("genre", None),
            change("year", None),
        ],
        &[],
    )
    .unwrap();
    assert_eq!(
        value(&l.inspect_metadata(&target).unwrap(), "title"),
        "Bad Album"
    );
    assert_eq!(
        value(
            &l.inspect_metadata(&Target::Track(r.track_ids[0].0.clone()))
                .unwrap(),
            "artist_credit"
        ),
        "Band"
    );
}
#[test]
fn validation_is_atomic_and_optional_blank_is_not_automatic() {
    let (_, mut l, r) = fixture();
    let target = Target::Track(r.track_ids[0].0.clone());
    for edits in [
        vec![change("title", Some("Good")), change("year", Some("10000"))],
        vec![change("title", Some("  "))],
        vec![change("track_number", Some("-1"))],
        vec![change("duration", Some("1"))],
    ] {
        assert!(l.save_metadata(&target, &edits, &[]).is_err());
        assert_eq!(
            value(&l.inspect_metadata(&target).unwrap(), "title"),
            "Song 1"
        );
    }
    l.save_metadata(&target, &[change("year", Some(""))], &[])
        .unwrap();
    let i = l.inspect_metadata(&target).unwrap();
    assert_eq!(value(&i, "year"), "");
    assert!(
        i.fields
            .iter()
            .find(|f| f.key == "year")
            .unwrap()
            .overridden
    );
    l.save_metadata(&target, &[change("year", None)], &[])
        .unwrap();
    assert_eq!(value(&l.inspect_metadata(&target).unwrap(), "year"), "2012");
    let a = l.album_for_release(&r.release_id).unwrap().album_id;
    assert!(
        l.save_metadata(
            &Target::Album(a.0),
            &[
                change("title", Some("Correct")),
                change("track_number", Some("2"))
            ],
            &[]
        )
        .is_err()
    );
}
fn copied_source(l: &mut Library, temp: &tempfile::TempDir, track: &TrackId, name: &str) -> String {
    let root = temp.path().join("audio");
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join(name);
    std::fs::copy("tests/fixtures/provenance/silence.mp3", &file).unwrap();
    let root_id = l.register_local_root(&root).unwrap();
    l.scan_local_root(&root_id, &mut LoftyMetadataExtractor)
        .unwrap();
    let db = Connection::open(temp.path().join("db")).unwrap();
    let source: String = db
        .query_row(
            "SELECT source_id FROM local_file_observation WHERE path=?1",
            [file.as_os_str().as_encoded_bytes()],
            |r| r.get(0),
        )
        .unwrap();
    db.execute(
        "INSERT INTO track_source(track_id,source_id) VALUES(?1,?2)",
        params![track.as_ref(), source],
    )
    .unwrap();
    source
}
#[test]
fn persisted_source_comparison_and_mixed_local_album_values_are_separate() {
    let (temp, l, r) = fixture();
    let db = Connection::open(temp.path().join("db")).unwrap();
    let a = l.album_for_release(&r.release_id).unwrap().album_id;
    for (n, t) in r.track_ids.iter().enumerate() {
        let s = format!("local-{n}");
        db.execute(
            "INSERT INTO playable_source(id,kind) VALUES(?1,'local_file')",
            [&s],
        )
        .unwrap();
        db.execute("INSERT OR IGNORE INTO discovery_root(id,kind,location) VALUES('root','local_filesystem',x'2f746d70')",[]).unwrap();
        db.execute("INSERT INTO local_file_observation(source_id,root_id,path,size_bytes,modified_ns,available) VALUES(?1,'root',?2,1,1,0)",params![s,format!("/tmp/{n}").as_bytes()]).unwrap();
        db.execute(
            "INSERT INTO track_source(track_id,source_id) VALUES(?1,?2)",
            params![t.as_ref(), s],
        )
        .unwrap();
        db.execute("INSERT INTO file_metadata_observation(source_id,track_title,release_title) VALUES(?1,'Local Song',?2)",params![s,if n==2{"BAD ALBUM"}else{"Bad Album"}]).unwrap();
    }
    for provider in ["musicbrainz", "spotify"] {
        db.execute("INSERT INTO album_provider_evidence(album_id,provider,kind,external_id,title,release_date,release_type) VALUES(?1,?2,'album',?2,?3,'2012-01-02','Album')",params![a.as_ref(),provider,format!("{provider} Album")]).unwrap();
        db.execute("INSERT INTO track_provider_evidence(track_id,provider,kind,external_id,title) VALUES(?1,?2,'track',?2,?3)",params![r.track_ids[0].as_ref(),provider,format!("{provider} Song")]).unwrap();
    }
    let i = l
        .inspect_metadata(&Target::Track(r.track_ids[0].0.clone()))
        .unwrap();
    for provider in ["Local file", "MusicBrainz", "Spotify"] {
        assert!(
            i.evidence.iter().any(|e| e.source == provider),
            "{provider}"
        );
    }
    let i = l.inspect_metadata(&Target::Album(a.0)).unwrap();
    let albums: Vec<_> = i
        .track_evidence
        .iter()
        .flat_map(|track| &track.evidence)
        .filter(|e| e.source == "Local file")
        .map(|e| e.values["Album"].clone())
        .collect();
    assert_eq!(albums.len(), 3);
    assert!(albums.contains(&"BAD ALBUM".into()));
    assert!(albums.contains(&"Bad Album".into()));
}
#[test]
fn album_evidence_groups_canonical_tracks_in_effective_album_position_order() {
    let (temp, mut l, r) = fixture();
    let album = Target::Album(l.album_for_release(&r.release_id).unwrap().album_id.0);
    for (index, (disc, position)) in [(2, 1), (1, 9), (1, 2)].into_iter().enumerate() {
        let track = &r.track_ids[index];
        copied_source(&mut l, &temp, track, &format!("{index}.mp3"));
        l.save_metadata(
            &Target::Track(track.0.clone()),
            &[
                change("disc_number", Some(&disc.to_string())),
                change("track_number", Some(&position.to_string())),
            ],
            &[],
        )
        .unwrap();
        let db = Connection::open(temp.path().join("db")).unwrap();
        for provider in ["musicbrainz", "spotify"] {
            db.execute("INSERT INTO track_provider_evidence(track_id,provider,kind,external_id,title) VALUES(?1,?2,'track',?3,?4)", params![track.as_ref(),provider,format!("{provider}-{index}"),format!("{provider} title {index}")]).unwrap();
        }
    }
    let inspection = l.inspect_metadata(&album).unwrap();
    assert_eq!(
        inspection
            .track_evidence
            .iter()
            .map(|g| g.track_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            r.track_ids[2].as_ref(),
            r.track_ids[1].as_ref(),
            r.track_ids[0].as_ref()
        ]
    );
    assert!(inspection.evidence.iter().all(|e| e.track_id.is_none()));
    for group in &inspection.track_evidence {
        assert_eq!(group.evidence[0].source, "Library");
        assert_eq!(group.evidence[0].values["Track title"], group.title);
        assert!(
            group
                .evidence
                .iter()
                .all(|e| e.track_id.as_deref() == Some(&group.track_id))
        );
        for source in ["Local file", "MusicBrainz", "Spotify"] {
            assert!(group.evidence.iter().any(|e| e.source == source));
        }
    }
}

#[test]
fn metadata_connects_only_explicit_persisted_spotify_candidate_without_replacing_identity() {
    let (temp, mut l, r) = fixture();
    let track = &r.track_ids[1];
    let playlist = l.create_playlist("Stable playlist").unwrap();
    let entry = l.append_playlist_track(&playlist, track).unwrap();
    let target = Target::Track(track.0.clone());
    l.save_metadata(&target, &[change("title", Some("Manual title"))], &[])
        .unwrap();
    let candidate = |id: &str| {
        serde_json::json!({
            "identity":{"provider":"spotify","kind":"track","external_id":id},
            "album_identity":null,"title":"Persisted candidate","artist":"Band","artists":[],
            "album":"Bad Album","date":"2012","album_artists":[],"album_type":"album",
            "album_total_tracks":3,"duration_ms":120000,"disc":1,"number":2
        })
    };
    let page =
        serde_json::json!({"items":[candidate("first"),candidate("chosen")],"next_offset":null});
    let db = Connection::open(temp.path().join("db")).unwrap();
    db.execute(
        "INSERT INTO spotify_reconciliation_cache(track_id,input_json,page_json) VALUES(?1,?2,?3)",
        params![
            track.as_ref(),
            serde_json::to_string(&l.song_resolution_input(track).unwrap()).unwrap(),
            page.to_string()
        ],
    )
    .unwrap();
    let inspection = l.inspect_metadata(&target).unwrap();
    assert_eq!(
        inspection
            .evidence
            .iter()
            .filter_map(|e| e.candidate_id.as_deref())
            .collect::<Vec<_>>(),
        vec!["first", "chosen"]
    );
    assert!(l.list_track_external_identities(track).unwrap().is_empty());
    assert!(
        l.connect_metadata_spotify_candidate(track, "missing")
            .is_err()
    );
    let connected = l
        .connect_metadata_spotify_candidate(track, "chosen")
        .unwrap();
    assert_eq!(connected.external_id, "chosen");
    assert_eq!(
        l.list_track_external_identities(track).unwrap(),
        vec![connected.clone()]
    );
    assert!(
        l.connect_metadata_spotify_candidate(track, "first")
            .unwrap_err()
            .to_string()
            .contains("already has a provider association")
    );
    assert_eq!(
        l.list_track_external_identities(track).unwrap(),
        vec![connected]
    );
    assert_eq!(
        value(&l.inspect_metadata(&target).unwrap(), "title"),
        "Manual title"
    );
    assert_eq!(
        db.query_row(
            "SELECT track_id FROM playlist_entry WHERE id=?1",
            [entry.as_str()],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        track.0
    );
    assert!(
        db.query_row(
            "SELECT EXISTS(SELECT 1 FROM library_membership WHERE track_id=?1)",
            [track.as_ref()],
            |r| r.get::<_, bool>(0)
        )
        .unwrap()
    );
    assert!(
        l.connect_metadata_spotify_candidate(&r.track_ids[0], "chosen")
            .is_err()
    );
}
#[test]
fn selected_file_writes_round_trip_keep_other_tags_and_rescan_identity() {
    use lofty::prelude::Accessor;
    use lofty::{
        config::WriteOptions,
        file::{AudioFile, TaggedFileExt},
        tag::{ItemKey, Tag},
    };
    for extension in ["mp3", "flac", "m4a"] {
        let (temp, mut l, r) = fixture();
        let t = &r.track_ids[0];
        let root = temp.path().join("audio");
        std::fs::create_dir(&root).unwrap();
        let path = root.join(format!("song.{extension}"));
        std::fs::copy(
            format!("tests/fixtures/provenance/silence.{extension}"),
            &path,
        )
        .unwrap();
        let mut audio = lofty::read_from_path(&path).unwrap();
        if audio.primary_tag().is_none() {
            audio.insert_tag(Tag::new(audio.primary_tag_type()));
        }
        let tag = audio.primary_tag_mut().unwrap();
        tag.set_title("Wrong".into());
        tag.set_comment("KEEP COMMENT".into());
        tag.insert_text(ItemKey::EncoderSoftware, "KEEP ENCODER".into());
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 2)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        png.set_position(0);
        tag.push_picture(lofty::picture::Picture::from_reader(&mut png).unwrap());
        audio.save_to_path(&path, WriteOptions::default()).unwrap();
        let before = lofty::read_from_path(&path).unwrap();
        let encoder = before
            .primary_tag()
            .unwrap()
            .get_string(ItemKey::EncoderSoftware)
            .map(str::to_owned);
        let pictures = before.primary_tag().unwrap().pictures().to_vec();
        let duration = audio.properties().duration();
        let root_id = l.register_local_root(&root).unwrap();
        l.scan_local_root(&root_id, &mut LoftyMetadataExtractor)
            .unwrap();
        let db = Connection::open(temp.path().join("db")).unwrap();
        let s: String = db
            .query_row(
                "SELECT source_id FROM local_file_observation WHERE path=?1",
                [path.as_os_str().as_encoded_bytes()],
                |r| r.get(0),
            )
            .unwrap();
        db.execute(
            "INSERT INTO track_source(track_id,source_id) VALUES(?1,?2)",
            params![t.as_ref(), s],
        )
        .unwrap();
        let target = Target::Track(t.0.clone());
        let bytes = std::fs::read(&path).unwrap();
        l.save_metadata(&target, &[change("title", Some("Library Only"))], &[])
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let outcome = l
            .save_metadata(
                &target,
                &[
                    change("title", Some("X’ed Out")),
                    change("genre", Some("Indie")),
                    change("year", Some("2024")),
                    change("track_number", Some("2")),
                ],
                std::slice::from_ref(&s),
            )
            .unwrap();
        assert!(outcome.files[0].error.is_none(), "{:?}", outcome.files);
        let tags = LoftyMetadataExtractor.read(&path).unwrap();
        assert_eq!(tags.track_title.as_deref(), Some("X’ed Out"));
        assert_eq!(tags.year, Some(2024));
        assert_eq!(tags.track_number, Some(2));
        let audio = lofty::read_from_path(&path).unwrap();
        assert_eq!(audio.properties().duration(), duration);
        assert_eq!(
            audio.primary_tag().unwrap().comment().as_deref(),
            Some("KEEP COMMENT")
        );
        assert_eq!(
            audio
                .primary_tag()
                .unwrap()
                .get_string(ItemKey::EncoderSoftware),
            encoder.as_deref()
        );
        assert_eq!(audio.primary_tag().unwrap().pictures(), pictures);
        l.scan_local_root(&root_id, &mut LoftyMetadataExtractor)
            .unwrap();
        assert_eq!(
            db.query_row(
                "SELECT track_id FROM track_source WHERE source_id=?1",
                [&s],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            t.0
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM local_file_observation", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            value(&l.inspect_metadata(&target).unwrap(), "title"),
            "X’ed Out"
        );
    }
}
#[test]
fn multiple_sources_selection_and_partial_failure_do_not_rollback_library() {
    let (temp, mut l, r) = fixture();
    let t = &r.track_ids[0];
    let s1 = copied_source(&mut l, &temp, t, "one.mp3");
    let s2 = copied_source(&mut l, &temp, t, "two.mp3");
    let target = Target::Track(t.0.clone());
    l.save_metadata(
        &target,
        &[change("title", Some("Selected"))],
        std::slice::from_ref(&s1),
    )
    .unwrap();
    assert_eq!(
        LoftyMetadataExtractor
            .read(&temp.path().join("audio/one.mp3"))
            .unwrap()
            .track_title
            .as_deref(),
        Some("Selected")
    );
    assert_ne!(
        LoftyMetadataExtractor
            .read(&temp.path().join("audio/two.mp3"))
            .unwrap()
            .track_title
            .as_deref(),
        Some("Selected")
    );
    let mut permissions = std::fs::metadata(temp.path().join("audio/two.mp3"))
        .unwrap()
        .permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(temp.path().join("audio/two.mp3"), permissions).unwrap();
    let outcome = l
        .save_metadata(&target, &[change("title", Some("Partial"))], &[s1, s2])
        .unwrap();
    assert!(outcome.files[0].error.is_none());
    assert!(
        outcome.files[1]
            .error
            .as_ref()
            .unwrap()
            .contains("read-only")
    );
    assert_eq!(
        value(&l.inspect_metadata(&target).unwrap(), "title"),
        "Partial"
    );
    assert!(
        l.save_metadata(
            &target,
            &[change("title", Some("Invalid selection"))],
            &["unattached".into()]
        )
        .is_err()
    );
    assert_eq!(
        value(&l.inspect_metadata(&target).unwrap(), "title"),
        "Partial"
    );
}
#[test]
fn scanner_ignores_in_progress_metadata_copy() {
    let (temp, mut l, r) = fixture();
    copied_source(&mut l, &temp, &r.track_ids[0], "one.mp3");
    let root = temp.path().join("audio");
    std::fs::copy(root.join("one.mp3"), root.join(".metadata-copy.tmp")).unwrap();
    let root_id = l.register_local_root(&root).unwrap();
    l.scan_local_root(&root_id, &mut LoftyMetadataExtractor)
        .unwrap();
    let db = Connection::open(temp.path().join("db")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM local_file_observation", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
#[ignore = "generates disposable Vorbis/Opus audio with GStreamer"]
fn ogg_and_opus_writeback_preserve_tags_and_decoded_audio() {
    use lofty::{
        config::WriteOptions,
        file::{AudioFile, TaggedFileExt},
        prelude::Accessor,
    };
    for (extension, encoder) in [("ogg", "vorbisenc"), ("opus", "opusenc")] {
        let (temp, mut l, r) = fixture();
        let root = temp.path().join("audio");
        std::fs::create_dir(&root).unwrap();
        let path = root.join(format!("song.{extension}"));
        assert!(
            std::process::Command::new("gst-launch-1.0")
                .args([
                    "-q",
                    "audiotestsrc",
                    "num-buffers=8",
                    "!",
                    "audioconvert",
                    "!",
                    encoder,
                    "!",
                    "oggmux",
                    "!",
                    "filesink"
                ])
                .arg(format!("location={}", path.display()))
                .status()
                .unwrap()
                .success()
        );
        let mut audio = lofty::read_from_path(&path).unwrap();
        let tag = audio.primary_tag_mut().unwrap();
        tag.set_comment("KEEP COMMENT".into());
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 2)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        png.set_position(0);
        tag.push_picture(lofty::picture::Picture::from_reader(&mut png).unwrap());
        audio.save_to_path(&path, WriteOptions::default()).unwrap();
        let pictures = lofty::read_from_path(&path)
            .unwrap()
            .primary_tag()
            .unwrap()
            .pictures()
            .to_vec();
        let decode = |name: &str| {
            let output = temp.path().join(name);
            assert!(
                std::process::Command::new("gst-launch-1.0")
                    .args(["-q", "filesrc"])
                    .arg(format!("location={}", path.display()))
                    .args([
                        "!",
                        "decodebin",
                        "!",
                        "audioconvert",
                        "!",
                        "audio/x-raw,format=S16LE",
                        "!",
                        "filesink"
                    ])
                    .arg(format!("location={}", output.display()))
                    .status()
                    .unwrap()
                    .success()
            );
            std::fs::read(output).unwrap()
        };
        let before = decode("before.pcm");
        let root_id = l.register_local_root(&root).unwrap();
        l.scan_local_root(&root_id, &mut LoftyMetadataExtractor)
            .unwrap();
        let db = Connection::open(temp.path().join("db")).unwrap();
        let source: String = db
            .query_row(
                "SELECT source_id FROM local_file_observation WHERE path=?1",
                [path.as_os_str().as_encoded_bytes()],
                |r| r.get(0),
            )
            .unwrap();
        db.execute(
            "INSERT INTO track_source(track_id,source_id) VALUES(?1,?2)",
            params![r.track_ids[0].as_ref(), source],
        )
        .unwrap();
        let outcome = l
            .save_metadata(
                &Target::Track(r.track_ids[0].0.clone()),
                &[
                    change("title", Some("Unicode — correction")),
                    change("genre", Some("Jazz")),
                    change("year", Some("2024")),
                    change("disc_number", Some("2")),
                    change("track_number", Some("7")),
                ],
                &[source],
            )
            .unwrap();
        assert!(outcome.files[0].error.is_none(), "{:?}", outcome.files);
        let updated = lofty::read_from_path(&path).unwrap();
        let tag = updated.primary_tag().unwrap();
        assert_eq!(tag.title().as_deref(), Some("Unicode — correction"));
        assert_eq!(tag.genre().as_deref(), Some("Jazz"));
        assert_eq!(tag.disk(), Some(2));
        assert_eq!(tag.track(), Some(7));
        assert_eq!(tag.comment().as_deref(), Some("KEEP COMMENT"));
        assert_eq!(tag.pictures(), pictures);
        assert_eq!(decode("after.pcm"), before);
    }
}

#[test]
fn clearing_mixed_album_genre_does_not_erase_individual_file_tags() {
    let (temp, mut l, r) = fixture();
    let s1 = copied_source(&mut l, &temp, &r.track_ids[0], "one.mp3");
    let s2 = copied_source(&mut l, &temp, &r.track_ids[1], "two.mp3");
    l.save_metadata(
        &Target::Track(r.track_ids[0].0.clone()),
        &[change("genre", Some("Rock"))],
        std::slice::from_ref(&s1),
    )
    .unwrap();
    l.save_metadata(
        &Target::Track(r.track_ids[1].0.clone()),
        &[change("genre", Some("Jazz"))],
        std::slice::from_ref(&s2),
    )
    .unwrap();
    let album = Target::Album(l.album_for_release(&r.release_id).unwrap().album_id.0);
    l.save_metadata(&album, &[change("genre", Some("Shared"))], &[])
        .unwrap();
    let before = std::fs::read(temp.path().join("audio/one.mp3")).unwrap();
    let outcome = l
        .save_metadata(&album, &[change("genre", None)], &[s1, s2])
        .unwrap();
    assert!(outcome.files.iter().all(|f| {
        f.error
            .as_deref()
            .is_some_and(|e| e.contains("mixed or missing"))
    }));
    assert_eq!(
        std::fs::read(temp.path().join("audio/one.mp3")).unwrap(),
        before
    );
    assert!(
        !l.inspect_metadata(&album)
            .unwrap()
            .fields
            .iter()
            .find(|f| f.key == "genre")
            .unwrap()
            .overridden
    );
}
#[test]
fn album_writeback_only_selected_shared_fields_and_files() {
    let (temp, mut l, r) = fixture();
    let s1 = copied_source(&mut l, &temp, &r.track_ids[0], "one.mp3");
    let s2 = copied_source(&mut l, &temp, &r.track_ids[1], "two.mp3");
    let a = l.album_for_release(&r.release_id).unwrap().album_id;
    let outcome = l
        .save_metadata(
            &Target::Album(a.0),
            &[
                change("title", Some("Correct Album")),
                change("artist_credit", Some("Correct Album Credit")),
            ],
            &[s1, s2],
        )
        .unwrap();
    assert!(outcome.files.iter().all(|f| f.error.is_none()));
    for name in ["one.mp3", "two.mp3"] {
        let tags = LoftyMetadataExtractor
            .read(&temp.path().join("audio").join(name))
            .unwrap();
        assert_eq!(tags.release_title.as_deref(), Some("Correct Album"));
        assert_eq!(tags.release_artists, vec!["Correct Album Credit"]);
        assert_ne!(tags.track_title.as_deref(), Some("Correct Album"));
    }
}
#[test]
fn matching_stales_unresolved_only_and_genre_does_not_stale() {
    let (temp, mut l, r) = fixture();
    let db = Connection::open(temp.path().join("db")).unwrap();
    db.execute("UPDATE spotify_connection_review SET stale=0", [])
        .unwrap();
    let target = Target::Track(r.track_ids[0].0.clone());
    l.save_metadata(&target, &[change("genre", Some("Jazz"))], &[])
        .unwrap();
    assert_eq!(
        db.query_row(
            "SELECT sum(stale) FROM spotify_connection_review",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    l.save_metadata(&target, &[change("title", Some("Correct"))], &[])
        .unwrap();
    assert!(
        db.query_row(
            "SELECT stale FROM spotify_connection_review WHERE track_id=?1",
            [r.track_ids[0].as_ref()],
            |r| r.get::<_, bool>(0)
        )
        .unwrap()
    );
    let trusted = ExternalIdentity {
        provider: "spotify".into(),
        kind: "track".into(),
        external_id: "trusted".into(),
    };
    l.attach_track_external_identity(&r.track_ids[1], &trusted)
        .unwrap();
    db.execute("UPDATE spotify_connection_review SET stale=0", [])
        .unwrap();
    let a = l.album_for_release(&r.release_id).unwrap().album_id;
    l.save_metadata(
        &Target::Album(a.0),
        &[
            change("title", Some("Correct Album")),
            change("artist_credit", Some("Correct Credit")),
        ],
        &[],
    )
    .unwrap();
    assert_eq!(
        l.list_track_external_identities(&r.track_ids[1]).unwrap(),
        vec![trusted]
    );
    assert!(
        !db.query_row(
            "SELECT stale FROM spotify_connection_review WHERE track_id=?1",
            [r.track_ids[1].as_ref()],
            |r| r.get::<_, bool>(0)
        )
        .unwrap()
    );
    assert_eq!(
        l.song_resolution_input(&r.track_ids[0]).unwrap().title,
        "Correct"
    );
    assert_eq!(
        l.song_resolution_input(&r.track_ids[0]).unwrap().album,
        "Correct Album"
    );
}
#[test]
#[ignore = "copied real Get Disowned Album, with GStreamer PCM verification"]
fn real_track_and_album_metadata_workflow_on_disposable_copies() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("audio");
    std::fs::create_dir(&root).unwrap();
    for entry in std::fs::read_dir("test-media/Get Disowned").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "mp3") {
            std::fs::copy(&path, root.join(path.file_name().unwrap())).unwrap();
        }
    }
    let database = temp.path().join("db");
    let mut l = Library::open(&database).unwrap();
    let report = l
        .ingest_local(
            &music_library::local_ingestion::Request::Folder(root.clone()),
            &mut LoftyMetadataExtractor,
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(report.imported, 10);
    let rows = l
        .browse(&Request {
            pane: Pane::Songs,
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    let t = rows[0].track.as_ref().unwrap().track_id.clone();
    let a = l.album_id_for_track(&t).unwrap();
    let target = Target::Track(t.0.clone());
    let inspection = l.inspect_metadata(&target).unwrap();
    let source = inspection.files[0].source_id.clone();
    let path = std::path::PathBuf::from(&inspection.files[0].path);
    let bytes = std::fs::read(&path).unwrap();
    fn decode(path: &std::path::Path, destination: &std::path::Path) {
        let status = std::process::Command::new("gst-launch-1.0")
            .args(["-q", "filesrc"])
            .arg(format!("location={}", path.display()))
            .args([
                "!",
                "decodebin",
                "!",
                "audioconvert",
                "!",
                "audioresample",
                "!",
                "audio/x-raw,format=S16LE,rate=44100,channels=2",
                "!",
                "filesink",
            ])
            .arg(format!("location={}", destination.display()))
            .status()
            .unwrap();
        assert!(status.success());
    }
    let before_pcm = temp.path().join("before.pcm");
    decode(&path, &before_pcm);
    l.save_metadata(
        &target,
        &[change("title", Some("Some Grace — Library correction"))],
        &[],
    )
    .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    drop(l);
    let mut l = Library::open(&database).unwrap();
    assert_eq!(
        value(&l.inspect_metadata(&target).unwrap(), "title"),
        "Some Grace — Library correction"
    );
    assert_eq!(l.album_id_for_track(&t).unwrap(), a);
    let outcome = l
        .save_metadata(
            &target,
            &[change("title", Some("Some Grace — written correction"))],
            std::slice::from_ref(&source),
        )
        .unwrap();
    assert!(outcome.files[0].error.is_none(), "{:?}", outcome.files);
    assert_eq!(
        LoftyMetadataExtractor
            .read(&path)
            .unwrap()
            .track_title
            .as_deref(),
        Some("Some Grace — written correction")
    );
    let album_target = Target::Album(a.0.clone());
    let before = l.inspect_metadata(&album_target).unwrap();
    assert_eq!(before.track_count, 10);
    let original_titles: Vec<_> = before
        .files
        .iter()
        .map(|f| {
            LoftyMetadataExtractor
                .read(std::path::Path::new(&f.path))
                .unwrap()
                .track_title
        })
        .collect();
    l.save_metadata(
        &album_target,
        &[
            change("title", Some("Get Disowned — corrected Library Album")),
            change("artist_credit", Some("Hop Along, Queen Ansleis")),
        ],
        &[],
    )
    .unwrap();
    for f in &before.files {
        assert_eq!(
            value(
                &l.inspect_metadata(&Target::Track(f.track_id.clone()))
                    .unwrap(),
                "album"
            ),
            "Get Disowned — corrected Library Album"
        );
        assert_eq!(
            value(
                &l.inspect_metadata(&Target::Track(f.track_id.clone()))
                    .unwrap(),
                "artist_credit"
            ),
            "Hop Along, Queen Ansleis"
        );
        assert_ne!(
            LoftyMetadataExtractor
                .read(std::path::Path::new(&f.path))
                .unwrap()
                .release_title
                .as_deref(),
            Some("Get Disowned — corrected Library Album")
        );
    }
    let files = before
        .files
        .iter()
        .map(|f| f.source_id.clone())
        .collect::<Vec<_>>();
    let outcome = l
        .save_metadata(
            &album_target,
            &[
                change("title", Some("Get Disowned — corrected files")),
                change("artist_credit", Some("Hop Along, Queen Ansleis")),
            ],
            &files,
        )
        .unwrap();
    assert_eq!(outcome.files.len(), 10);
    assert!(
        outcome.files.iter().all(|f| f.error.is_none()),
        "{:?}",
        outcome.files
    );
    for (f, title) in before.files.iter().zip(&original_titles) {
        let metadata = LoftyMetadataExtractor
            .read(std::path::Path::new(&f.path))
            .unwrap();
        assert_eq!(
            metadata.release_title.as_deref(),
            Some("Get Disowned — corrected files")
        );
        assert_eq!(metadata.release_artists, vec!["Hop Along, Queen Ansleis"]);
        assert_eq!(&metadata.track_title, title);
    }
    let root_id = l.register_local_root(&root).unwrap();
    l.scan_local_root(&root_id, &mut LoftyMetadataExtractor)
        .unwrap();
    assert_eq!(l.inspect_metadata(&album_target).unwrap().track_count, 10);
    assert_eq!(
        l.inspect_metadata(&target).unwrap().files[0].source_id,
        source
    );
    assert_eq!(l.album_id_for_track(&t).unwrap(), a);
    let after_pcm = temp.path().join("after.pcm");
    decode(&path, &after_pcm);
    assert_eq!(
        std::fs::read(before_pcm).unwrap(),
        std::fs::read(after_pcm).unwrap()
    );
    eprintln!(
        "Real copied Album: 10 shared file updates; Track/Album/source IDs stable; restart and rescan preserved overrides; decoded Track PCM identical"
    );
}
#[test]
fn missing_or_unsupported_tag_fields_leave_library_saved() {
    let (temp, mut l, r) = fixture();
    let s = copied_source(&mut l, &temp, &r.track_ids[0], "missing.mp3");
    let target = Target::Track(r.track_ids[0].0.clone());
    std::fs::remove_file(temp.path().join("audio/missing.mp3")).unwrap();
    let outcome = l
        .save_metadata(&target, &[change("title", Some("Still Saved"))], &[s])
        .unwrap();
    assert!(outcome.files[0].error.is_some());
    assert_eq!(
        value(&l.inspect_metadata(&target).unwrap(), "title"),
        "Still Saved"
    );
    let s = copied_source(&mut l, &temp, &r.track_ids[1], "supported.mp3");
    let a = l.album_id_for_track(&r.track_ids[1]).unwrap();
    let target = Target::Album(a.0);
    let bytes = std::fs::read(temp.path().join("audio/supported.mp3")).unwrap();
    let outcome = l
        .save_metadata(&target, &[change("release_type", Some("EP"))], &[s])
        .unwrap();
    assert!(
        outcome.files[0]
            .error
            .as_ref()
            .unwrap()
            .contains("Release type")
    );
    assert_eq!(
        std::fs::read(temp.path().join("audio/supported.mp3")).unwrap(),
        bytes
    );
    assert_eq!(
        value(&l.inspect_metadata(&target).unwrap(), "release_type"),
        "EP"
    );
}
#[test]
fn unsupported_format_and_observed_unavailable_source_are_separate_file_failures() {
    let (temp, mut l, r) = fixture();
    let t = &r.track_ids[0];
    let s = copied_source(&mut l, &temp, t, "old.mp3");
    let target = Target::Track(t.0.clone());
    let db = Connection::open(temp.path().join("db")).unwrap();
    db.execute(
        "UPDATE local_file_observation SET available=0 WHERE source_id=?1",
        [&s],
    )
    .unwrap();
    let outcome = l
        .save_metadata(
            &target,
            &[change("title", Some("Unavailable saved"))],
            std::slice::from_ref(&s),
        )
        .unwrap();
    assert_eq!(
        outcome.files[0].error.as_deref(),
        Some("Source is unavailable")
    );
    // A disposable minimal PCM WAV is readable, but intentionally outside write-back support.
    let path = temp.path().join("audio/unsupported.wav");
    let mut wav = Vec::new();
    wav.extend(b"RIFF");
    wav.extend(38_u32.to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16_u32.to_le_bytes());
    wav.extend(1_u16.to_le_bytes());
    wav.extend(1_u16.to_le_bytes());
    wav.extend(8000_u32.to_le_bytes());
    wav.extend(16000_u32.to_le_bytes());
    wav.extend(2_u16.to_le_bytes());
    wav.extend(16_u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(2_u32.to_le_bytes());
    wav.extend([0, 0]);
    std::fs::write(&path, &wav).unwrap();
    assert!(LoftyMetadataExtractor.read(&path).is_ok());
    db.execute(
        "UPDATE local_file_observation SET available=1,path=?2 WHERE source_id=?1",
        params![s, path.as_os_str().as_encoded_bytes()],
    )
    .unwrap();
    let outcome = l
        .save_metadata(&target, &[change("title", Some("Unsupported saved"))], &[s])
        .unwrap();
    assert!(
        outcome.files[0]
            .error
            .as_ref()
            .unwrap()
            .contains("supports MP3")
    );
    assert_eq!(std::fs::read(path).unwrap(), wav);
    assert_eq!(
        value(&l.inspect_metadata(&target).unwrap(), "title"),
        "Unsupported saved"
    );
}
#[test]
fn provider_refresh_keeps_overrides_and_reconciliation_uses_corrected_credit() {
    let (temp, mut l, r) = fixture();
    let t = &r.track_ids[0];
    let target = Target::Track(t.0.clone());
    let a = l.album_id_for_track(t).unwrap();
    let album = Target::Album(a.0.clone());
    l.save_metadata(
        &target,
        &[
            change("title", Some("User Song")),
            change("genre", Some("Indie")),
        ],
        &[],
    )
    .unwrap();
    l.save_metadata(
        &album,
        &[
            change("title", Some("User Album")),
            change("artist_credit", Some("Band, Queen Ansleis")),
        ],
        &[],
    )
    .unwrap();
    let db = Connection::open(temp.path().join("db")).unwrap();
    db.execute("INSERT INTO track_provider_evidence(track_id,provider,kind,external_id,title) VALUES(?1,'spotify','track','persisted','Refreshed provider title')",[t.as_ref()]).unwrap();
    db.execute(
        "UPDATE album_application_metadata SET title='New automatic Album' WHERE album_id=?1",
        [a.as_ref()],
    )
    .unwrap();
    l.save_metadata(&target, &[change("year", Some("2024"))], &[])
        .unwrap();
    assert_eq!(
        value(&l.inspect_metadata(&target).unwrap(), "title"),
        "User Song"
    );
    assert_eq!(
        value(&l.inspect_metadata(&target).unwrap(), "album"),
        "User Album"
    );
    assert_eq!(
        value(&l.inspect_metadata(&target).unwrap(), "genre"),
        "Indie"
    );
    assert!(
        l.inspect_metadata(&target)
            .unwrap()
            .evidence
            .iter()
            .any(|e| e.values.values().any(|v| v == "Refreshed provider title"))
    );
    let input = l.song_resolution_input(t).unwrap();
    assert_eq!(input.search_artist(), "Band, Queen Ansleis");
    assert_eq!(input.album_date.unwrap().year, 2024);
    assert!(input.evidence.artist_credits.iter().any(|o| matches!(
        o.origin,
        music_library::canonical_evidence::Origin::UserOverride
    )));
    assert!(
        l.local_search("Queen Ansleis", music_library::library_search::Kind::Song)
            .unwrap()
            .iter()
            .any(|h| h.id == t.0)
    );
    let hit = l
        .local_search("User Album", music_library::library_search::Kind::Album)
        .unwrap()
        .into_iter()
        .find(|h| h.id == a.0)
        .unwrap();
    assert_eq!(hit.artist, "Band, Queen Ansleis");
}

#[test]
fn album_database_failure_rolls_back_all_fields_indexes_and_precedes_file_writes() {
    let (temp, mut library, release) = fixture();
    let source = copied_source(&mut library, &temp, &release.track_ids[0], "unchanged.mp3");
    let album = library.album_id_for_track(&release.track_ids[0]).unwrap();
    let target = Target::Album(album.0.clone());
    let before = std::fs::read(temp.path().join("audio/unchanged.mp3")).unwrap();
    let db = Connection::open(temp.path().join("db")).unwrap();
    let fail_track: String = db
        .query_row(
            "SELECT id FROM track ORDER BY id LIMIT 1 OFFSET 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    db.execute_batch(&format!("CREATE TRIGGER fail_metadata BEFORE UPDATE ON effective_track_metadata WHEN new.track_id='{fail_track}' BEGIN SELECT RAISE(ABORT,'injected metadata failure'); END;")).unwrap();
    assert!(
        library
            .save_metadata(
                &target,
                &[
                    change("title", Some("Must roll back")),
                    change("artist_credit", Some("Must roll back credit"))
                ],
                &[source]
            )
            .is_err()
    );
    assert_eq!(
        value(&library.inspect_metadata(&target).unwrap(), "title"),
        "Bad Album"
    );
    assert_eq!(
        value(&library.inspect_metadata(&target).unwrap(), "artist_credit"),
        "Band"
    );
    assert_eq!(
        std::fs::read(temp.path().join("audio/unchanged.mp3")).unwrap(),
        before
    );
    assert!(
        library
            .local_search("Must roll back", music_library::library_search::Kind::Album)
            .unwrap()
            .is_empty()
    );
    for track in &release.track_ids {
        assert_eq!(
            value(
                &library
                    .inspect_metadata(&Target::Track(track.0.clone()))
                    .unwrap(),
                "album"
            ),
            "Bad Album"
        );
    }
}

#[test]
fn album_edits_span_release_occurrences_and_genre_grouping_uses_overrides() {
    let (temp, mut library, first) = fixture();
    let album = library.album_id_for_track(&first.track_ids[0]).unwrap();
    let second = library
        .create_catalog_release(&CatalogReleaseInput {
            title: "Exact other edition".into(),
            year: Some(2014),
            artists: vec![ArtistCreditInput {
                name: "Band".into(),
                role: None,
            }],
            tracks: vec![CatalogTrackInput {
                title: "Other edition song".into(),
                artists: vec![ArtistCreditInput {
                    name: "Band".into(),
                    role: None,
                }],
                disc_number: Some(2),
                track_number: Some(5),
            }],
        })
        .unwrap();
    library.add_to_library(&second.track_ids[0]).unwrap();
    let db = Connection::open(temp.path().join("db")).unwrap();
    db.execute(
        "UPDATE release SET album_id=?2 WHERE id=?1",
        params![second.release_id.as_ref(), album.as_ref()],
    )
    .unwrap();
    library
        .save_metadata(
            &Target::Album(album.0.clone()),
            &[
                change("title", Some("Shared Album")),
                change("genre", Some("Indie")),
            ],
            &[],
        )
        .unwrap();
    let inspection = library
        .inspect_metadata(&Target::Album(album.0.clone()))
        .unwrap();
    assert_eq!(inspection.track_count, 4);
    assert!(
        inspection
            .evidence
            .iter()
            .any(|e| e.values.values().any(|v| v == "Exact other edition"))
    );
    for track in first.track_ids.iter().chain(&second.track_ids) {
        assert_eq!(
            value(
                &library
                    .inspect_metadata(&Target::Track(track.0.clone()))
                    .unwrap(),
                "album"
            ),
            "Shared Album"
        );
    }
    let rows = library
        .browse(&Request {
            pane: Pane::Songs,
            genre: Some("Indie".into()),
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(rows.len(), 4);
    library
        .save_metadata(
            &Target::Track(first.track_ids[0].0.clone()),
            &[change("genre", Some("Jazz"))],
            &[],
        )
        .unwrap();
    let rows = library
        .browse(&Request {
            pane: Pane::Genres,
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
        vec!["Indie", "Jazz"]
    );
    assert_eq!(
        library
            .browse(&Request {
                pane: Pane::Songs,
                genre: Some("Indie".into()),
                limit: 200,
                ..Default::default()
            })
            .unwrap()
            .len(),
        3
    );
    library
        .save_metadata(
            &Target::Track(first.track_ids[0].0.clone()),
            &[change("genre", None)],
            &[],
        )
        .unwrap();
    assert_eq!(
        library
            .browse(&Request {
                pane: Pane::Songs,
                genre: Some("Indie".into()),
                limit: 200,
                ..Default::default()
            })
            .unwrap()
            .len(),
        4
    );
    let original: String = db
        .query_row(
            "SELECT title FROM release_application_metadata WHERE release_id=?1",
            [second.release_id.as_ref()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(original, "Exact other edition");
}

#[test]
fn album_rename_requires_explicit_move_and_preserves_tracks_releases_and_playlists() {
    let (temp, mut library, source) = fixture();
    let destination = library
        .create_catalog_release(&CatalogReleaseInput {
            title: "Correct Album".into(),
            year: Some(2014),
            artists: vec![ArtistCreditInput {
                name: "Band".into(),
                role: None,
            }],
            tracks: vec![CatalogTrackInput {
                title: "Existing Song".into(),
                artists: vec![],
                disc_number: Some(1),
                track_number: Some(4),
            }],
        })
        .unwrap();
    let source_album = library.album_id_for_track(&source.track_ids[0]).unwrap();
    let destination_album = library
        .album_id_for_track(&destination.track_ids[0])
        .unwrap();
    let source_target = Target::Album(source_album.0.clone());
    let changes = [change("title", Some("Correct Album"))];
    let playlist = library.create_playlist("Keep references").unwrap();
    library
        .append_playlist_track(&playlist, &source.track_ids[0])
        .unwrap();
    library
        .save_metadata(
            &Target::Track(source.track_ids[0].0.clone()),
            &[change("title", Some("User title"))],
            &[],
        )
        .unwrap();
    let matches = library
        .metadata_album_rename_matches(&source_target, &changes)
        .unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].album_id, destination_album.0);
    assert!(
        library
            .metadata_album_rename_matches(&source_target, &[change("genre", Some("Rock"))])
            .unwrap()
            .is_empty()
    );
    library
        .save_metadata(&source_target, &changes, &[])
        .unwrap();
    let db = Connection::open(temp.path().join("db")).unwrap();
    let parent = || {
        db.query_row(
            "SELECT album_id FROM release WHERE id=?1",
            [&source.release_id.0],
            |r| r.get::<_, String>(0),
        )
        .unwrap()
    };
    assert_eq!(parent(), source_album.0);
    // Explicit re-saving of the corrected title also offers the destination.
    assert_eq!(
        library
            .metadata_album_rename_matches(&source_target, &changes)
            .unwrap()
            .len(),
        1
    );
    let source_identity = ExternalIdentity {
        provider: "musicbrainz".into(),
        kind: "release-group".into(),
        external_id: "source-group".into(),
    };
    let destination_identity = ExternalIdentity {
        external_id: "other-group".into(),
        ..source_identity.clone()
    };
    library
        .attach_album_external_identity(&source_album, &source_identity)
        .unwrap();
    library
        .attach_album_external_identity(&destination_album, &destination_identity)
        .unwrap();
    assert!(
        !library
            .metadata_album_rename_matches(&source_target, &changes)
            .unwrap()[0]
            .blocked_reason
            .is_empty()
    );
    assert!(
        library
            .save_metadata_with_album_move(
                &source_target,
                &changes,
                &[],
                Some(&destination_album.0)
            )
            .is_err()
    );
    assert_eq!(parent(), source_album.0);
    db.execute(
        "DELETE FROM album_external_identity WHERE album_id=?1",
        [&destination_album.0],
    )
    .unwrap();
    let outcome = library
        .save_metadata_with_album_move(&source_target, &changes, &[], Some(&destination_album.0))
        .unwrap();
    assert_eq!(
        outcome.destination_album.as_deref(),
        Some(destination_album.0.as_str())
    );
    assert_eq!(parent(), destination_album.0);
    assert_eq!(
        library
            .list_album_external_identities(&destination_album)
            .unwrap(),
        vec![source_identity]
    );
    for track in &source.track_ids {
        let inspection = library
            .inspect_metadata(&Target::Track(track.0.clone()))
            .unwrap();
        assert_eq!(inspection.album_id, destination_album.0);
        assert_eq!(value(&inspection, "album"), "Correct Album");
    }
    assert_eq!(
        value(
            &library
                .inspect_metadata(&Target::Track(source.track_ids[0].0.clone()))
                .unwrap(),
            "title"
        ),
        "User title"
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM playlist_entry WHERE playlist_id=?1 AND track_id=?2",
            params![playlist, source.track_ids[0].0],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(db.query_row("SELECT count(*) FROM library_membership WHERE track_id IN (SELECT id FROM track WHERE release_id=?1)", [&source.release_id.0], |r| r.get::<_,i64>(0)).unwrap(), 3);
}
