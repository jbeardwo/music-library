use music_library::{
    Library, Result,
    domain::*,
    edition::{Completeness, LocalEditionEvidence},
    filesystem::MetadataExtractor,
    provenance::*,
};
use rusqlite::{Connection, params};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

fn observation(scope: Scope, semantics: Semantics, value: &str) -> Observation {
    Observation {
        identity: ExternalIdentity {
            provider: "synthetic".into(),
            kind: format!("{semantics:?}"),
            external_id: value.into(),
        },
        scope,
        semantics,
        origin: Origin::EmbeddedTag {
            format: "TestFormat".into(),
            field: "TestField".into(),
        },
    }
}

const X: &str = "12345678-1234-4234-8234-123456789abc";
const Y: &str = "12345678-1234-4234-8234-123456789abd";
fn promotable(value: &str) -> FileProvenance {
    let mut p = file(1, 1, 1, 1);
    for o in &mut p.observations {
        if matches!(
            o.semantics,
            Semantics::AlbumIdentity | Semantics::RecordingIdentity
        ) {
            o.identity = ExternalIdentity {
                provider: "musicbrainz".into(),
                kind: if o.scope == Scope::Album {
                    "release_group"
                } else {
                    "recording"
                }
                .into(),
                external_id: value.into(),
            };
        }
    }
    // Album identity and Recording identity need neither complete positions nor edition.
    p.positions = Positions::default();
    p.observations.retain(|o| o.scope != Scope::Edition);
    p
}

fn accepted(f: &Fixture, entity: &str) -> Vec<(String, bool)> {
    f.db().prepare(&format!("SELECT i.external_id,m.{entity}_id IS NOT NULL FROM {entity}_external_identity i LEFT JOIN {entity}_provenance_identity m USING({entity}_id,provider,kind,external_id) ORDER BY i.external_id"))
        .unwrap().query_map([], |r|Ok((r.get(0)?,r.get(1)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap()
}

#[test]
fn canonical_acceptance_restart_retag_removal_and_independent_confirmation() {
    let mut f = Fixture::new(vec![promotable(X)]);
    for entity in ["album", "recording"] {
        assert_eq!(accepted(&f, entity), vec![(X.into(), true)]);
    }
    assert_eq!(f.evidence().completeness, Completeness::Unknown);
    let raw = f.snapshots();
    f.restart();
    let reads = f.extractor.reads;
    f.scan();
    assert_eq!(f.extractor.reads, reads);
    assert_eq!(f.snapshots(), raw);
    f.retag(0, promotable(Y));
    for entity in ["album", "recording"] {
        assert_eq!(accepted(&f, entity), vec![(Y.into(), true)]);
    }
    let album = f.evidence().album_id.clone();
    let recording = f
        .lib()
        .recording_for_track(&f.release.track_ids[0])
        .unwrap()
        .recording_id;
    let lib = f.library.as_mut().unwrap();
    assert!(
        !lib.attach_album_external_identity(
            &album,
            &ExternalIdentity {
                provider: "musicbrainz".into(),
                kind: "release_group".into(),
                external_id: Y.into()
            }
        )
        .unwrap()
    );
    assert!(
        !lib.attach_recording_external_identity(
            &recording,
            &ExternalIdentity {
                provider: "musicbrainz".into(),
                kind: "recording".into(),
                external_id: Y.into()
            }
        )
        .unwrap()
    );
    f.retag(0, FileProvenance::default());
    for entity in ["album", "recording"] {
        assert_eq!(accepted(&f, entity), vec![(Y.into(), false)]);
    }
    f.retag(0, promotable(X));
    let reports = f
        .library
        .as_mut()
        .unwrap()
        .reconcile_local_provenance(&album)
        .unwrap();
    assert!(
        reports
            .iter()
            .all(|r| r.assessments.iter().all(|a| a.decision
                == music_library::provenance_acceptance::Decision::ConflictingIndependentIdentity))
    );
    for entity in ["album", "recording"] {
        assert_eq!(accepted(&f, entity), vec![(Y.into(), false)]);
    }
}

#[test]
fn current_conflicts_retract_and_missing_claims_do_not_vote() {
    let mut f = Fixture::new(vec![promotable(X), promotable(X)]);
    // Two independent files now support one application Track, as permitted by the model.
    let db = f.db();
    let first = &f.release.track_ids[0];
    let second = &f.release.track_ids[1];
    db.execute(
        "UPDATE track_source SET track_id=?1 WHERE track_id=?2",
        params![first.as_ref(), second.as_ref()],
    )
    .unwrap();
    let album = f.evidence().album_id.clone();
    f.library
        .as_mut()
        .unwrap()
        .reconcile_local_provenance(&album)
        .unwrap();
    assert_eq!(accepted(&f, "recording"), vec![(X.into(), true)]);
    f.retag(0, FileProvenance::default());
    for entity in ["album", "recording"] {
        assert_eq!(accepted(&f, entity), vec![(X.into(), true)]);
    }
    f.retag(0, promotable(Y));
    for entity in ["album", "recording"] {
        assert!(accepted(&f, entity).is_empty());
    }
    f.retag(0, promotable(X));
    for entity in ["album", "recording"] {
        assert_eq!(accepted(&f, entity), vec![(X.into(), true)]);
    }
    f.retag(0, FileProvenance::default());
    f.retag(1, FileProvenance::default());
    for entity in ["album", "recording"] {
        assert!(accepted(&f, entity).is_empty());
    }
}

#[test]
fn malformed_claims_stay_raw_and_source_unavailability_retracts_only_managed() {
    let mut f = Fixture::new(vec![promotable("malformed")]);
    assert!(!f.snapshots().is_empty());
    f.assert_no_canonical_ids();
    f.retag(0, promotable(X));
    std::fs::remove_file(f.path(0)).unwrap();
    f.scan();
    for entity in ["album", "recording"] {
        assert!(accepted(&f, entity).is_empty());
    }
    assert!(f.snapshots()[0].contains(X));
    assert_eq!(f.evidence().tracks.len(), 1);
}

#[test]
fn synthetic_capabilities_and_deferred_categories() {
    use music_library::provenance_acceptance::Validation;
    fn capability(o: &Observation) -> Validation {
        if o.identity.provider == "synthetic"
            && matches!(
                o.semantics,
                Semantics::AlbumIdentity | Semantics::RecordingIdentity
            )
        {
            Validation::Valid
        } else {
            Validation::Unsupported
        }
    }
    let mut p = file(1, 1, 1, 1);
    p.observations
        .retain(|o| o.semantics != Semantics::RecordingIdentity);
    let mut f = Fixture::new(vec![p]);
    let album = f.evidence().album_id.clone();
    f.library
        .as_mut()
        .unwrap()
        .set_provenance_validator(capability);
    f.library
        .as_mut()
        .unwrap()
        .reconcile_local_provenance(&album)
        .unwrap();
    assert_eq!(accepted(&f, "album"), vec![("group".into(), true)]);
    assert!(accepted(&f, "recording").is_empty());
    f.retag(0, file(1, 1, 1, 1));
    assert_eq!(
        accepted(&f, "recording"),
        vec![("recording-1-1".into(), true)]
    );
    for table in [
        "artist_external_identity",
        "track_external_identity",
        "release_external_identity",
    ] {
        assert_eq!(
            f.db()
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                    .get::<_, u32>(0))
                .unwrap(),
            0
        );
    }
}

#[test]
fn shared_recording_conflict_is_not_hidden_by_album_boundaries_and_no_entities_merge() {
    let mut f = Fixture::new(vec![promotable(X), promotable(Y)]);
    assert!(accepted(&f, "album").is_empty());
    assert_eq!(accepted(&f, "recording").len(), 2);
    let db = f.db();
    db.execute("INSERT INTO album(id) VALUES ('other-album')", [])
        .unwrap();
    db.execute(
        "INSERT INTO release(id,album_id) VALUES ('other-release','other-album')",
        [],
    )
    .unwrap();
    db.execute(
        "UPDATE track SET release_id='other-release' WHERE id=?1",
        [f.release.track_ids[1].as_ref()],
    )
    .unwrap();
    let recording = f
        .lib()
        .recording_for_track(&f.release.track_ids[0])
        .unwrap()
        .recording_id;
    db.execute(
        "UPDATE track SET recording_id=?1 WHERE id=?2",
        params![recording.as_ref(), f.release.track_ids[1].as_ref()],
    )
    .unwrap();
    let album = f.evidence().album_id.clone();
    let reports = f
        .library
        .as_mut()
        .unwrap()
        .reconcile_local_provenance(&album)
        .unwrap();
    let report = reports.iter().find(|r| r.entity == "recording").unwrap();
    assert!(report.managed.is_empty());
    assert_eq!(
        report.assessments[0].decision,
        music_library::provenance_acceptance::Decision::ConflictingObservations
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM track", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        2
    );
}

#[test]
fn acceptance_failure_rolls_back_source_snapshot_and_canonical_retraction() {
    let mut f = Fixture::new(vec![promotable(X)]);
    let before = f.snapshots();
    f.db().execute_batch("CREATE TRIGGER fail_acceptance BEFORE INSERT ON recording_provenance_identity BEGIN SELECT RAISE(ABORT,'induced failure'); END;").unwrap();
    let path = f.path(0);
    f.extractor.values.get_mut(&path).unwrap().provenance = promotable(Y);
    std::fs::write(path, b"changed test metadata").unwrap();
    assert!(
        f.library
            .as_mut()
            .unwrap()
            .scan_local_root(&f.root, &mut f.extractor)
            .is_err()
    );
    assert_eq!(f.snapshots(), before);
    for entity in ["album", "recording"] {
        assert_eq!(accepted(&f, entity), vec![(X.into(), true)]);
    }
}

#[test]
fn explicit_recording_merge_preserves_managed_ownership_without_merging_tracks() {
    let mut f = Fixture::new(vec![promotable(X), promotable(X)]);
    let a = f
        .lib()
        .recording_for_track(&f.release.track_ids[0])
        .unwrap()
        .recording_id;
    let b = f
        .lib()
        .recording_for_track(&f.release.track_ids[1])
        .unwrap()
        .recording_id;
    assert_ne!(a, b);
    f.library.as_mut().unwrap().merge_recording(&a, &b).unwrap();
    assert_eq!(accepted(&f, "recording"), vec![(X.into(), true)]);
    f.retag(0, FileProvenance::default());
    assert_eq!(accepted(&f, "recording"), vec![(X.into(), true)]);
    f.retag(1, FileProvenance::default());
    assert!(accepted(&f, "recording").is_empty());
    assert_eq!(f.evidence().tracks.len(), 2);
}

#[test]
fn acceptance_reconstruction_uses_entity_and_source_indexes() {
    let f = Fixture::new(vec![promotable(X)]);
    let db = f.db();
    for (query, expected) in [
        (
            "SELECT r.album_id,ts.source_id,m.provenance_json FROM release r CROSS JOIN track t ON t.release_id=r.id CROSS JOIN track_source ts ON ts.track_id=t.id CROSS JOIN local_file_observation l ON l.source_id=ts.source_id AND l.available=1 LEFT JOIN file_metadata_observation m ON m.source_id=ts.source_id WHERE r.album_id IN (SELECT value FROM json_each(?1))",
            "release_album",
        ),
        (
            "SELECT t.recording_id,ts.source_id,m.provenance_json FROM track t CROSS JOIN track_source ts ON ts.track_id=t.id CROSS JOIN local_file_observation l ON l.source_id=ts.source_id AND l.available=1 LEFT JOIN file_metadata_observation m ON m.source_id=ts.source_id WHERE t.recording_id IN (SELECT value FROM json_each(?1))",
            "track_recording",
        ),
    ] {
        let plan = db
            .prepare(&format!("EXPLAIN QUERY PLAN {query}"))
            .unwrap()
            .query_map(["[]"], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        println!("{}", plan.join("\n"));
        assert!(plan.iter().any(|l| l.contains(expected)));
        for table in ["r", "t", "ts", "l", "m"] {
            assert!(
                !plan
                    .iter()
                    .any(|l| l == &format!("SCAN {table}")
                        || l.starts_with(&format!("SCAN {table} "))),
                "{plan:?}"
            );
        }
    }
}

#[test]
fn v9_upgrade_protects_prior_canonical_rows_and_ownership_is_cascaded() {
    let mut f = Fixture::new(vec![promotable(X)]);
    drop(f.library.take());
    let db = f.db();
    db.execute_batch(include_str!("support/drop_manual_schema.sql"))
        .unwrap();
    db.execute_batch("DROP TRIGGER album_identity_confirmation; DROP TRIGGER recording_identity_confirmation; DROP TABLE album_provenance_identity; DROP TABLE recording_provenance_identity; PRAGMA user_version=9;").unwrap();
    f.restart();
    for entity in ["album", "recording"] {
        assert_eq!(accepted(&f, entity), vec![(X.into(), false)]);
    }
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        11
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, u32>(
            0
        ))
        .unwrap(),
        0
    );
}

#[test]
fn failed_ownership_migration_rolls_back_without_changing_prior_identities() {
    let mut f = Fixture::new(vec![promotable(X)]);
    drop(f.library.take());
    let db = f.db();
    db.execute_batch(include_str!("support/drop_manual_schema.sql"))
        .unwrap();
    db.execute_batch("DROP TRIGGER album_identity_confirmation; DROP TRIGGER recording_identity_confirmation; DROP TABLE album_provenance_identity; DROP TABLE recording_provenance_identity; CREATE TABLE recording_provenance_identity(sentinel TEXT); PRAGMA user_version=9;").unwrap();
    assert!(Library::open(f.temp.path().join("db")).is_err());
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        9
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='album_provenance_identity'",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT external_id FROM album_external_identity", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        X
    );
}

#[test]
#[ignore = "local acceptance/retraction timings; no network"]
fn acceptance_timings() {
    use std::time::Instant;
    for count in [1, 3, 15, 30] {
        let mut f = Fixture::new((0..count).map(|_| promotable(X)).collect());
        let album = f.evidence().album_id.clone();
        let mut times = vec![];
        for _ in 0..9 {
            // Measure actual new canonical writes as well as batched reconstruction.
            f.db()
                .execute_batch(
                    "DELETE FROM album_external_identity; DELETE FROM recording_external_identity;",
                )
                .unwrap();
            let start = Instant::now();
            f.library
                .as_mut()
                .unwrap()
                .reconcile_local_provenance(&album)
                .unwrap();
            times.push(start.elapsed());
        }
        times.sort();
        println!(
            "{count} Tracks acceptance incl transaction median {:?}",
            times[4]
        );
    }
}
fn file(disc: u32, discs: u32, number: u32, total: u32) -> FileProvenance {
    FileProvenance {
        file_type: Some("TestAudio".into()),
        formats: vec!["TestFormat".into()],
        positions: Positions {
            disc: vec![disc.to_string()],
            discs: vec![discs.to_string()],
            track: vec![number.to_string()],
            tracks: vec![total.to_string()],
        },
        observations: vec![
            observation(Scope::Album, Semantics::AlbumIdentity, "group"),
            observation(Scope::Edition, Semantics::EditionIdentity, "edition"),
            observation(
                Scope::Recording,
                Semantics::RecordingIdentity,
                &format!("recording-{disc}-{number}"),
            ),
            observation(
                Scope::Occurrence,
                Semantics::OccurrenceIdentity,
                &format!("song-{disc}-{number}"),
            ),
            observation(Scope::Recording, Semantics::Isrc, "ISRC-a"),
            observation(Scope::Recording, Semantics::Isrc, "ISRC-b"),
        ],
        ..Default::default()
    }
}
#[derive(Default)]
struct Extractor {
    values: HashMap<PathBuf, ObservedMetadata>,
    reads: usize,
}
impl MetadataExtractor for Extractor {
    fn supports(&self, path: &Path) -> bool {
        path.extension().is_some_and(|e| e == "mp3")
    }
    fn read(&mut self, path: &Path) -> Result<ObservedMetadata> {
        self.reads += 1;
        Ok(self.values[path].clone())
    }
}
struct Fixture {
    temp: tempfile::TempDir,
    library: Option<Library>,
    root: RootId,
    extractor: Extractor,
    release: ImportedRelease,
}
impl Fixture {
    fn new(files: Vec<FileProvenance>) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let music = temp.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let mut library = Library::open(temp.path().join("db")).unwrap();
        let root = library.register_local_root(&music).unwrap();
        let mut extractor = Extractor::default();
        for (i, provenance) in files.into_iter().enumerate() {
            let path = music.join(format!("{i}.mp3"));
            std::fs::write(&path, b"test source").unwrap();
            extractor.values.insert(
                path,
                ObservedMetadata {
                    disc_number: provenance
                        .positions
                        .disc
                        .first()
                        .and_then(|v| v.parse().ok()),
                    track_number: provenance
                        .positions
                        .track
                        .first()
                        .and_then(|v| v.parse().ok()),
                    provenance,
                    track_title: Some(format!("Song {i}")),
                    release_title: Some("Album".into()),
                    track_artists: vec!["Local artist".into()],
                    release_artists: vec!["Local artist".into()],
                    duration_ms: Some(180_000),
                    ..Default::default()
                },
            );
        }
        library.scan_local_root(&root, &mut extractor).unwrap();
        let candidates = library.list_discovery_candidates(None, 200).unwrap();
        let release = library
            .import_release(&ImportReleaseRequest {
                release_title: "Album".into(),
                release_artists: vec![],
                tracks: candidates
                    .into_iter()
                    .map(|c| ImportTrackInput {
                        source_id: c.source_id,
                        title_fallback: None,
                        artists: vec![],
                        disc_number: c.metadata.disc_number,
                        track_number: c.metadata.track_number,
                    })
                    .collect(),
            })
            .unwrap();
        Self {
            temp,
            library: Some(library),
            root,
            extractor,
            release,
        }
    }
    fn lib(&self) -> &Library {
        self.library.as_ref().unwrap()
    }
    fn db(&self) -> Connection {
        let db = Connection::open(self.temp.path().join("db")).unwrap();
        db.pragma_update(None, "foreign_keys", true).unwrap();
        db
    }
    fn path(&self, i: usize) -> PathBuf {
        self.temp.path().join("music").join(format!("{i}.mp3"))
    }
    fn restart(&mut self) {
        drop(self.library.take());
        self.library = Some(Library::open(self.temp.path().join("db")).unwrap());
    }
    fn evidence(&self) -> LocalEditionEvidence {
        self.lib()
            .edition_evidence(&self.release.release_id)
            .unwrap()
    }
    fn scan(&mut self) {
        self.library
            .as_mut()
            .unwrap()
            .scan_local_root(&self.root, &mut self.extractor)
            .unwrap();
    }
    fn retag(&mut self, i: usize, provenance: FileProvenance) {
        let path = self.path(i);
        self.extractor.values.get_mut(&path).unwrap().provenance = provenance;
        // A different length reliably triggers the existing changed-source path.
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.push(b'x');
        std::fs::write(&path, bytes).unwrap();
        self.scan();
    }
    fn snapshots(&self) -> Vec<String> {
        self.db()
            .prepare("SELECT provenance_json FROM file_metadata_observation ORDER BY source_id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }
    fn assert_no_canonical_ids(&self) {
        for table in [
            "artist_external_identity",
            "album_external_identity",
            "release_external_identity",
            "track_external_identity",
            "recording_external_identity",
        ] {
            let n: i64 = self
                .db()
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 0, "{table}");
        }
    }
}

#[test]
fn restart_and_unchanged_scan_preserve_all_observations_without_reads_or_duplicates() {
    let mut f = Fixture::new(vec![file(1, 1, 1, 2), file(1, 1, 2, 2)]);
    let before = f.evidence();
    let snapshots = f.snapshots();
    assert_eq!(before.completeness, Completeness::TrustedComplete);
    assert!(before.provenance.album_and_edition.conflicts.is_empty());
    f.restart();
    let after = f.evidence();
    assert_eq!(before.provenance.tracks, after.provenance.tracks);
    assert_eq!(after.completeness, before.completeness);
    assert_eq!(f.extractor.reads, 2);
    f.scan();
    assert_eq!(f.extractor.reads, 2);
    assert_eq!(f.snapshots(), snapshots);
    // Physical files can even be absent until availability reconciliation: this
    // read-only API uses database observations, never probes the filesystem.
    std::fs::remove_file(f.path(0)).unwrap();
    let readonly =
        music_library::edition_storage::read_only(f.temp.path().join("db"), &f.release.release_id)
            .unwrap();
    assert_eq!(readonly.provenance.tracks, before.provenance.tracks);
    f.assert_no_canonical_ids();
}

#[test]
fn retag_replaces_and_removes_claims_atomically_preserving_other_sources() {
    let mut f = Fixture::new(vec![file(1, 1, 1, 2), file(1, 1, 2, 2)]);
    let mut changed = file(1, 1, 1, 2);
    changed.observations.retain(|o| o.scope != Scope::Recording);
    changed.observations.push(observation(
        Scope::Recording,
        Semantics::RecordingIdentity,
        "Y",
    ));
    f.retag(0, changed);
    f.restart();
    let e = f.evidence();
    let ids: Vec<_> = e
        .provenance
        .per_track
        .iter()
        .flat_map(|p| &p.consistent)
        .map(|o| o.identity.external_id.as_str())
        .collect();
    assert!(ids.contains(&"Y"));
    assert!(!ids.contains(&"recording-1-1"));
    assert!(ids.contains(&"recording-1-2"));
    f.retag(0, FileProvenance::default());
    f.restart();
    let e = f.evidence();
    assert_eq!(e.completeness, Completeness::Unknown);
    assert!(!e.provenance.album_and_edition.consistent.is_empty());
    assert!(e.provenance.album_and_edition.conflicts.is_empty());
    assert!(
        e.provenance
            .per_track
            .iter()
            .any(|p| p.consistent.is_empty())
    );
    assert!(!f.snapshots().iter().any(|j| j.contains("\"Y\"")));
    assert_eq!(e.tracks[0].evidence.title.as_deref(), Some("Song 0"));
    f.assert_no_canonical_ids();
}

#[test]
fn multiple_sources_preserve_conflicts_and_availability_removes_only_current_evidence() {
    let mut f = Fixture::new(vec![file(1, 1, 1, 1)]);
    let original = f.evidence();
    let first_source = original.provenance.tracks[0][0].source_id.clone().unwrap();
    let track = original.tracks[0].track_id.clone();
    let accepted = ExternalIdentity {
        provider: "accepted-provider".into(),
        kind: "recording".into(),
        external_id: "accepted".into(),
    };
    f.library
        .as_mut()
        .unwrap()
        .attach_recording_external_identity(&original.tracks[0].recording_id, &accepted)
        .unwrap();
    let path = f.path(1);
    std::fs::write(&path, b"second source").unwrap();
    let mut p = file(1, 1, 1, 1);
    for o in &mut p.observations {
        if o.semantics != Semantics::Isrc {
            o.identity.external_id.push_str("-different");
        }
    }
    f.extractor.values.insert(
        path.clone(),
        ObservedMetadata {
            provenance: p,
            ..Default::default()
        },
    );
    f.scan();
    let second = f
        .lib()
        .list_discovery_candidates(None, 20)
        .unwrap()
        .pop()
        .unwrap();
    f.db()
        .execute(
            "INSERT INTO track_source(track_id,source_id) VALUES (?1,?2)",
            params![track.as_ref(), second.source_id.as_ref()],
        )
        .unwrap();
    f.restart();
    let both = f.evidence();
    assert_eq!(both.provenance.tracks[0].len(), 2);
    assert_eq!(both.provenance.album_and_edition.conflicts.len(), 2);
    assert_eq!(both.provenance.per_track[0].conflicts.len(), 2);
    assert_eq!(both.completeness, Completeness::Unknown);
    std::fs::remove_file(&path).unwrap();
    f.scan();
    f.restart();
    let current = f.evidence();
    assert_eq!(current.provenance.tracks, original.provenance.tracks);
    assert_eq!(current.completeness, Completeness::TrustedComplete);
    assert_eq!(f.snapshots().len(), 2); // unavailable observation retained for recovery
    // Reappearance without changed metadata reuses the durable snapshot.
    std::fs::write(&path, b"second source").unwrap();
    f.scan();
    assert_eq!(f.evidence().provenance.album_and_edition.conflicts.len(), 2);
    // Source deletion cascades only its metadata/association, not domain entities.
    f.db()
        .execute(
            "DELETE FROM playable_source WHERE id=?1",
            [second.source_id.as_ref()],
        )
        .unwrap();
    assert_eq!(f.snapshots().len(), 1);
    assert_eq!(
        f.evidence().provenance.tracks[0][0].source_id.as_ref(),
        Some(&first_source)
    );
    assert_eq!(f.evidence().tracks[0].track_id, track);
    let members: i64 = f
        .db()
        .query_row("SELECT count(*) FROM library_membership", [], |r| r.get(0))
        .unwrap();
    assert_eq!(members, 1);
    std::fs::remove_file(f.path(0)).unwrap();
    f.scan();
    let absent = f.evidence();
    assert_eq!(absent.completeness, Completeness::Unknown);
    assert!(absent.provenance.tracks[0].is_empty());
    assert!(absent.provenance.album_and_edition.consistent.is_empty());
    assert!(
        absent.tracks[0]
            .evidence
            .recording
            .identities
            .contains(&accepted)
    );
}

#[test]
fn restart_recomputes_complete_and_incomplete_programs_not_a_persisted_verdict() {
    let cases = [
        (vec![file(1, 1, 1, 1)], Completeness::TrustedComplete),
        (
            vec![file(1, 2, 1, 2), file(1, 2, 2, 2), file(2, 2, 1, 1)],
            Completeness::TrustedComplete,
        ),
        (vec![file(1, 1, 2, 15)], Completeness::Unknown),
        (
            vec![file(1, 1, 1, 2), file(1, 1, 1, 2)],
            Completeness::Unknown,
        ),
        (
            vec![file(1, 1, 1, 2), file(1, 1, 2, 3)],
            Completeness::Unknown,
        ),
        (vec![FileProvenance::default()], Completeness::Unknown),
    ];
    for (files, expected) in cases {
        let mut f = Fixture::new(files);
        let before = f.evidence();
        f.restart();
        let after = f.evidence();
        assert_eq!(after.completeness, expected);
        assert_eq!(after.provenance.tracks, before.provenance.tracks);
        assert!(
            !f.snapshots()
                .iter()
                .any(|j| j.contains("TrustedComplete") || j.contains("completeness"))
        );
        f.assert_no_canonical_ids();
    }
}

#[test]
fn v8_upgrade_preserves_existing_metadata_and_never_invents_provenance() {
    let mut f = Fixture::new(vec![file(1, 1, 1, 1)]);
    drop(f.library.take());
    let db = f.db();
    db.execute_batch(include_str!("support/drop_manual_schema.sql"))
        .unwrap();
    db.execute_batch(
        "DROP TRIGGER album_identity_confirmation; DROP TRIGGER recording_identity_confirmation; DROP TABLE album_provenance_identity; DROP TABLE recording_provenance_identity; ALTER TABLE file_metadata_observation DROP COLUMN provenance_json; PRAGMA user_version=8;",
    )
    .unwrap();
    f.restart();
    let e = f.evidence();
    assert_eq!(e.completeness, Completeness::Unknown);
    assert_eq!(e.tracks[0].evidence.title.as_deref(), Some("Song 0"));
    assert!(e.provenance.album_and_edition.consistent.is_empty());
    let json: Option<String> = db
        .query_row(
            "SELECT provenance_json FROM file_metadata_observation",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(json.is_none());
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        11
    );
    f.scan();
    assert_eq!(f.extractor.reads, 1); // no forced reparse of legacy files
    assert_eq!(
        db.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, u32>(
            0
        ))
        .unwrap(),
        0
    );
    assert!(
        db.execute(
            "UPDATE file_metadata_observation SET provenance_json='not json'",
            []
        )
        .is_err()
    );
}

#[test]
fn origins_opaque_values_duplicates_and_conflicting_declarations_round_trip() {
    let mut p = file(1, 1, 1, 1);
    p.observations[0].identity.external_id = "  opaque:/Ä value  ".into();
    p.observations[0].origin = Origin::ProviderResponse;
    p.observations[1].origin = Origin::ManualSelection;
    p.observations.push(p.observations[2].clone());
    p.positions.tracks.push("2".into());
    let mut f = Fixture::new(vec![p]);
    let before = f.evidence();
    f.restart();
    let after = f.evidence();
    assert_eq!(after.provenance.tracks, before.provenance.tracks);
    assert_eq!(after.completeness, Completeness::Unknown);
    f.assert_no_canonical_ids();
}

#[test]
fn provider_without_edition_or_recording_concepts_remains_useful_after_restart() {
    let p = FileProvenance {
        observations: vec![
            observation(Scope::Album, Semantics::AlbumIdentity, "album-object"),
            observation(
                Scope::Occurrence,
                Semantics::OccurrenceIdentity,
                "song-object",
            ),
        ],
        ..Default::default()
    };
    let mut f = Fixture::new(vec![p]);
    f.restart();
    let e = f.evidence();
    assert_eq!(e.provenance.album_and_edition.consistent.len(), 1);
    assert_eq!(
        e.provenance.per_track[0].consistent[0].semantics,
        Semantics::OccurrenceIdentity
    );
    assert_eq!(e.completeness, Completeness::Unknown);
    f.assert_no_canonical_ids();
}

#[test]
fn failed_migration_does_not_advance_version_or_change_observations() {
    let mut f = Fixture::new(vec![file(1, 1, 1, 1)]);
    let before = f.snapshots();
    drop(f.library.take());
    let db = f.db();
    // Simulate an incompatible preexisting column at version 8.
    db.pragma_update(None, "user_version", 8).unwrap();
    assert!(Library::open(f.temp.path().join("db")).is_err());
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        8
    );
    assert_eq!(f.snapshots(), before);
}

#[test]
#[ignore = "opt-in local scan/write and database reconstruction timings; no network"]
fn durable_provenance_timings() {
    use std::time::Instant;
    for count in [1, 3, 15, 30] {
        let mut f = Fixture::new((1..=count).map(|n| file(1, 1, n, count)).collect());
        let mut writes = Vec::new();
        let mut reads = Vec::new();
        for _ in 0..9 {
            for i in 0..count as usize {
                let path = f.path(i);
                let mut bytes = std::fs::read(&path).unwrap();
                bytes.push(b'x');
                std::fs::write(path, bytes).unwrap();
            }
            let start = Instant::now();
            f.scan();
            writes.push(start.elapsed());
            let start = Instant::now();
            assert_eq!(f.evidence().completeness, Completeness::TrustedComplete);
            reads.push(start.elapsed());
        }
        writes.sort();
        reads.sort();
        let bytes: usize = f.snapshots().iter().map(String::len).sum();
        println!(
            "{count} Tracks, {} identity observations, {bytes} JSON bytes: changed-source scan/write median {:?}; full edition reconstruction median {:?}",
            count * 6,
            writes[4],
            reads[4]
        );
    }
}

#[test]
fn batched_evidence_preserves_shared_recordings_and_repeated_ordered_artist_credits() {
    let f = Fixture::new(vec![file(1, 1, 1, 2), file(1, 1, 2, 2)]);
    let before = f.evidence();
    let first = &before.tracks[0];
    let second = &before.tracks[1];
    let db = f.db();
    let artist = "fixture-artist";
    db.execute(
        "INSERT INTO artist(id,name) VALUES (?1,'Entity name')",
        [artist],
    )
    .unwrap();
    db.execute("INSERT INTO track_artist_credit(track_id,position,artist_id,credited_name,join_phrase) VALUES (?1,0,?2,'Alias one',' feat. ')", params![first.track_id.as_ref(),artist]).unwrap();
    db.execute("INSERT INTO track_artist_credit(track_id,position,artist_id,credited_name,join_phrase) VALUES (?1,1,?2,'Alias two','')", params![first.track_id.as_ref(),artist]).unwrap();
    for external in ["artist-a", "artist-b"] {
        db.execute(
            "INSERT INTO artist_external_identity VALUES (?1,'synthetic','artist',?2)",
            params![artist, external],
        )
        .unwrap();
    }
    db.execute(
        "UPDATE track SET recording_id=?1 WHERE id=?2",
        params![first.recording_id.as_ref(), second.track_id.as_ref()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO recording_external_identity VALUES (?1,'synthetic','recording','shared')",
        [first.recording_id.as_ref()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO recording_external_identity VALUES (?1,'isrc','recording','ISRC')",
        [first.recording_id.as_ref()],
    )
    .unwrap();
    for (t, id) in [(&first.track_id, "one"), (&second.track_id, "two")] {
        db.execute(
            "INSERT INTO track_external_identity VALUES (?1,'synthetic','song',?2)",
            params![t.as_ref(), id],
        )
        .unwrap();
    }
    let after = f.evidence();
    let credits = &after.tracks[0].evidence.artists;
    assert_eq!(credits.len(), 2);
    assert_eq!(
        (
            &*credits[0].name,
            &*credits[0].join_phrase,
            &*credits[1].name
        ),
        ("Alias one", " feat. ", "Alias two")
    );
    assert_eq!(credits[0].identities.len(), 2);
    assert_eq!(credits[0].identities, credits[1].identities);
    for (t, id) in after.tracks.iter().zip(["one", "two"]) {
        assert_eq!(t.evidence.identities[0].external_id, id);
        assert_eq!(t.evidence.recording.identities[0].external_id, "shared");
        assert_eq!(t.evidence.recording.isrcs, ["ISRC"]);
    }
    assert_eq!(after.provenance.tracks, before.provenance.tracks);
}
