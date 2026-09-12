use music_library::{domain::*, edition::Completeness::*, provenance::*};

fn observation(scope: Scope, semantics: Semantics, value: &str) -> Observation {
    Observation {
        identity: ExternalIdentity {
            provider: "example".into(),
            kind: format!("{semantics:?}"),
            external_id: value.into(),
        },
        scope,
        semantics,
        origin: Origin::ProviderResponse,
    }
}
fn file(disc: u32, discs: u32, track: u32, tracks: u32) -> FileProvenance {
    FileProvenance {
        positions: Positions {
            disc: vec![disc.to_string()],
            discs: vec![discs.to_string()],
            track: vec![track.to_string()],
            tracks: vec![tracks.to_string()],
        },
        ..Default::default()
    }
}
fn summary(files: Vec<FileProvenance>) -> EditionProvenance {
    EditionProvenance::new(files.into_iter().map(|f| vec![f]).collect())
}

#[test]
fn provider_without_external_hierarchy_or_recording_entity_is_useful() {
    let f = FileProvenance {
        observations: vec![
            observation(Scope::AlbumArtist, Semantics::ArtistIdentity, "artist"),
            observation(Scope::Album, Semantics::AlbumIdentity, "album"),
            observation(Scope::Occurrence, Semantics::OccurrenceIdentity, "song"),
        ],
        ..Default::default()
    };
    let p = summary(vec![f]);
    assert_eq!(p.completeness, Unknown);
    assert_eq!(p.album_and_edition.consistent.len(), 1);
    assert_eq!(
        p.per_track[0].consistent[0].semantics,
        Semantics::OccurrenceIdentity
    );
    assert!(
        !p.per_track[0]
            .consistent
            .iter()
            .any(|o| o.scope == Scope::Recording)
    );
}

#[test]
fn recording_occurrence_and_multiple_isrcs_coexist_without_edition() {
    let f = FileProvenance {
        observations: vec![
            observation(Scope::Recording, Semantics::RecordingIdentity, "recording"),
            observation(Scope::Recording, Semantics::Isrc, "isrc1"),
            observation(Scope::Recording, Semantics::Isrc, "isrc2"),
            observation(Scope::Occurrence, Semantics::OccurrenceIdentity, "song"),
        ],
        ..Default::default()
    };
    let p = summary(vec![f, FileProvenance::default()]);
    assert_eq!(p.per_track[0].consistent.len(), 4);
    assert!(p.per_track[0].conflicts.is_empty());
    assert!(p.per_track[1].consistent.is_empty());
    assert!(p.album_and_edition.consistent.is_empty());
    assert_eq!(p.completeness, Unknown);
}

#[test]
fn conflicting_album_and_edition_identifiers_are_not_majority_voted() {
    let make = |id| FileProvenance {
        observations: vec![
            observation(Scope::Album, Semantics::AlbumIdentity, id),
            observation(Scope::Edition, Semantics::EditionIdentity, id),
        ],
        ..Default::default()
    };
    let p = summary(vec![make("a"), make("a"), make("b")]);
    assert_eq!(p.album_and_edition.conflicts.len(), 2);
    assert!(p.album_and_edition.consistent.is_empty());
    assert_eq!(p.album_and_edition.conflicts[0].values.len(), 2);
    // Missing tags on another file do not conflict with an observed identity.
    let p = summary(vec![make("a"), FileProvenance::default()]);
    assert_eq!(p.album_and_edition.consistent.len(), 2);
    assert_eq!(p.completeness, Unknown);
}

#[test]
fn partial_edition_identity_and_completeness_are_independent() {
    let mut f = file(1, 1, 5, 15);
    f.observations.push(observation(
        Scope::Edition,
        Semantics::EditionIdentity,
        "edition",
    ));
    let p = summary(vec![f]);
    assert_eq!(p.completeness, Unknown);
    assert_eq!(p.album_and_edition.consistent[0].scope, Scope::Edition);
    let p = summary((1..=3).map(|n| file(1, 1, n, 3)).collect());
    assert_eq!(p.completeness, TrustedComplete);
    assert!(p.album_and_edition.consistent.is_empty());
}

#[test]
fn explicit_two_disc_program_proves_completeness_but_bad_declarations_do_not() {
    let good = vec![file(1, 2, 1, 2), file(1, 2, 2, 2), file(2, 2, 1, 1)];
    assert_eq!(summary(good.clone()).completeness, TrustedComplete);
    for bad in [
        good[..2].to_vec(),                                      // missing disc
        vec![good[0].clone(), good[2].clone()],                  // missing position
        vec![good[0].clone(), good[0].clone(), good[2].clone()], // duplicate
        vec![file(1, 1, 1, 1), file(1, 1, 2, 1)],                // exceeds total
        vec![file(1, 1, 1, 2), file(1, 1, 2, 3)],                // inconsistent totals
        vec![file(1, 1, 0, 1)],
        vec![file(1, 1, 1, u32::MAX)],
    ] {
        assert_eq!(summary(bad).completeness, Unknown);
    }
    for field in 0..4 {
        let mut incomplete = file(1, 1, 1, 1);
        let p = &mut incomplete.positions;
        [&mut p.disc, &mut p.discs, &mut p.track, &mut p.tracks][field].clear();
        assert_eq!(summary(vec![incomplete]).completeness, Unknown);
    }
    let mut conflicting = file(1, 1, 1, 1);
    conflicting.positions.discs.push("2".into());
    assert_eq!(summary(vec![conflicting]).completeness, Unknown);
    assert_eq!(summary(vec![]).completeness, Unknown);
    assert_eq!(
        EditionProvenance::new(vec![vec![file(1, 1, 1, 1), file(1, 1, 1, 1)]]).completeness,
        Unknown
    );
}

#[test]
fn scan_import_snapshot_uses_one_read_and_never_attaches_observed_ids() {
    use music_library::{Library, Result, filesystem::MetadataExtractor};
    use std::path::Path;
    struct Extractor(usize);
    impl MetadataExtractor for Extractor {
        fn supports(&self, _: &Path) -> bool {
            true
        }
        fn read(&mut self, _: &Path) -> Result<ObservedMetadata> {
            self.0 += 1;
            let mut provenance = file(1, 1, 1, 1);
            provenance.observations.push(observation(
                Scope::Album,
                Semantics::AlbumIdentity,
                &format!("album-read-{}", self.0),
            ));
            provenance.observations.push(observation(
                Scope::Edition,
                Semantics::EditionIdentity,
                "edition",
            ));
            Ok(ObservedMetadata {
                provenance,
                track_title: Some("Local song".into()),
                release_title: Some("Local album".into()),
                disc_number: Some(1),
                track_number: Some(1),
                ..Default::default()
            })
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("music");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("song.mp3"), b"fake source").unwrap();
    let db_path = temp.path().join("db");
    let mut library = Library::open(&db_path).unwrap();
    let root_id = library.register_local_root(&root).unwrap();
    let mut extractor = Extractor(0);
    library.scan_local_root(&root_id, &mut extractor).unwrap();
    let found = library.list_discovery_candidates(None, 20).unwrap();
    assert_eq!(found[0].metadata.provenance.observations.len(), 2);
    let imported = library
        .import_release(&ImportReleaseRequest {
            release_title: "Local album".into(),
            release_artists: vec![],
            tracks: vec![ImportTrackInput {
                source_id: found[0].source_id.clone(),
                title_fallback: None,
                artists: vec![],
                disc_number: Some(1),
                track_number: Some(1),
            }],
        })
        .unwrap();
    library.scan_local_root(&root_id, &mut extractor).unwrap();
    let evidence = library.edition_evidence(&imported.release_id).unwrap();
    assert_eq!(extractor.0, 1);
    assert_eq!(evidence.completeness, TrustedComplete);
    assert_eq!(evidence.provenance.album_and_edition.consistent.len(), 2);
    assert!(evidence.exact_identities.is_empty()); // observations are not verified IDs
    assert!(evidence.grouping_identities.is_empty());
    assert_eq!(
        evidence.tracks[0].evidence.title.as_deref(),
        Some("Local song")
    );
    let db = rusqlite::Connection::open(&db_path).unwrap();
    let plan = db.prepare("EXPLAIN QUERY PLAN SELECT t.id,ts.source_id FROM track t JOIN track_source ts ON ts.track_id=t.id WHERE t.release_id=?1 ORDER BY t.id,ts.source_id").unwrap()
        .query_map([imported.release_id.as_ref()], |r| r.get::<_,String>(3)).unwrap()
        .collect::<rusqlite::Result<Vec<_>>>().unwrap().join("\n");
    assert!(
        plan.contains("SEARCH t USING COVERING INDEX track_release_order"),
        "{plan}"
    );
    assert!(plan.contains("SEARCH ts USING COVERING INDEX"), "{plan}");
    db.execute(
        "UPDATE track SET track_number=2 WHERE release_id=?1",
        [imported.release_id.as_ref()],
    )
    .unwrap();
    assert_eq!(
        library
            .edition_evidence(&imported.release_id)
            .unwrap()
            .completeness,
        Unknown
    );
    db.execute(
        "UPDATE track SET track_number=1 WHERE release_id=?1",
        [imported.release_id.as_ref()],
    )
    .unwrap();
    for table in [
        "album_external_identity",
        "release_external_identity",
        "track_external_identity",
        "recording_external_identity",
    ] {
        let n: i64 = db
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }
    // Failed persistence must not publish newly read provenance.
    db.execute_batch("CREATE TRIGGER fail_scan BEFORE INSERT ON file_metadata_observation BEGIN SELECT RAISE(ABORT,'induced failure'); END;").unwrap();
    std::fs::write(root.join("song.mp3"), b"changed source bytes").unwrap();
    assert!(library.scan_local_root(&root_id, &mut extractor).is_err());
    let after_failure = library.edition_evidence(&imported.release_id).unwrap();
    assert_eq!(
        after_failure.provenance.album_and_edition.consistent[0]
            .identity
            .external_id,
        "album-read-1"
    );
    db.execute_batch("DROP TRIGGER fail_scan").unwrap();
    drop(library);
    let library = Library::open(&db_path).unwrap();
    let evidence = library.edition_evidence(&imported.release_id).unwrap();
    assert_eq!(evidence.completeness, TrustedComplete);
    assert_eq!(evidence.provenance.tracks, after_failure.provenance.tracks);
}

#[test]
fn real_mp3_flac_and_mp4_reads_extract_tags_without_modifying_files() {
    use lofty::{
        config::WriteOptions,
        prelude::TagExt,
        tag::{ItemKey, ItemValue, Tag, TagItem, TagType},
    };
    use music_library::filesystem::{LoftyMetadataExtractor, MetadataExtractor};
    for (extension, tag_type, bytes) in [
        (
            "mp3",
            TagType::Id3v2,
            include_bytes!("fixtures/provenance/silence.mp3").as_slice(),
        ),
        (
            "flac",
            TagType::VorbisComments,
            include_bytes!("fixtures/provenance/silence.flac").as_slice(),
        ),
        (
            "m4a",
            TagType::Mp4Ilst,
            include_bytes!("fixtures/provenance/silence.m4a").as_slice(),
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(format!("test.{extension}"));
        std::fs::write(&path, bytes).unwrap();
        let mut tag = Tag::new(tag_type);
        for (key, value) in [
            (ItemKey::TrackTitle, "Local song"),
            (ItemKey::AlbumTitle, "Local album"),
            (ItemKey::MusicBrainzRecordingId, "recording-id"),
            (ItemKey::MusicBrainzTrackId, "occurrence-id"),
            (ItemKey::MusicBrainzReleaseId, "edition-id"),
            (ItemKey::MusicBrainzReleaseGroupId, "album-id"),
            (ItemKey::Isrc, "USABC1234567"),
            (ItemKey::TrackNumber, "1"),
            (ItemKey::TrackTotal, "1"),
            (ItemKey::DiscNumber, "1"),
            (ItemKey::DiscTotal, "1"),
        ] {
            tag.insert_unchecked(TagItem::new(key, ItemValue::Text(value.into())));
        }
        // ID3's Recording key has a special UFID conversion rather than a
        // simple frame-ID mapping. Use the native writer for this fixture.
        if tag_type == TagType::Id3v2 {
            use lofty::id3::v2::{ExtendedTextFrame, Frame, Id3v2Tag};
            let mut native = Id3v2Tag::from(tag);
            for (field, value) in [
                ("MusicBrainz Release Track Id", "occurrence-id"),
                ("MusicBrainz Album Id", "edition-id"),
                ("MusicBrainz Release Group Id", "album-id"),
            ] {
                native.insert(Frame::UserText(ExtendedTextFrame::new(
                    lofty::TextEncoding::UTF8,
                    field,
                    value,
                )));
            }
            native.save_to_path(&path, WriteOptions::default()).unwrap();
        } else {
            tag.save_to_path(&path, WriteOptions::default()).unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        let metadata = LoftyMetadataExtractor.read(&path).unwrap();
        assert_eq!(metadata.track_title.as_deref(), Some("Local song"));
        let p = &metadata.provenance;
        assert_eq!(
            completeness([&p.positions]),
            TrustedComplete,
            "{extension}: {p:?}"
        );
        assert!(
            p.observations
                .iter()
                .any(|o| o.semantics == Semantics::RecordingIdentity
                    && o.identity.external_id == "recording-id"),
            "{extension}: {p:?}"
        );
        assert!(
            p.observations
                .iter()
                .any(|o| o.semantics == Semantics::OccurrenceIdentity
                    && o.identity.external_id == "occurrence-id"),
            "{extension}: {p:?}"
        );
        assert!(
            p.observations
                .iter()
                .any(|o| o.semantics == Semantics::Isrc)
        );
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
}
