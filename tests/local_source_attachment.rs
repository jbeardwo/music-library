#[path = "support/local_attachment.rs"]
mod support;
use music_library::{
    Library, Result, catalog,
    domain::*,
    filesystem::{LoftyMetadataExtractor, MetadataExtractor},
    local_ingestion::{Report, Request},
    playback_resolver::{RemoteCapability, Route},
    provenance::*,
};
use rusqlite::Connection;
use std::path::Path;
const GROUP: &str = "11111111-1111-4111-8111-111111111111";
const EDITION: &str = "22222222-2222-4222-8222-222222222222";
const RECORDING: &str = "33333333-3333-4333-8333-333333333333";
const OCCURRENCE: &str = "44444444-4444-4444-8444-444444444444";
const ARTIST: &str = "55555555-5555-4555-8555-555555555555";
fn id(provider: &str, kind: &str, value: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: provider.into(),
        kind: kind.into(),
        external_id: value.into(),
    }
}
fn catalog_release() -> catalog::Release {
    let credits = vec![catalog::Credit {
        identity: Some(id("musicbrainz", "artist", ARTIST)),
        name: "Artist".into(),
        join_phrase: String::new(),
    }];
    catalog::Release {
        album: catalog::Album {
            release_type: None,
            identity: id("musicbrainz", "release_group", GROUP),
            title: "Album".into(),
            date: "2012".into(),
            credits: credits.clone(),
        },
        identity: id("musicbrainz", "release", EDITION),
        identities: vec![],
        title: "Album".into(),
        date: "2012".into(),
        credits: credits.clone(),
        media: vec![catalog::Medium {
            position: 1,
            tracks: vec![catalog::Track {
                title: "Song".into(),
                position: 1,
                credits,
                identities: vec![
                    id("musicbrainz", "recording", RECORDING),
                    id("musicbrainz", "track", OCCURRENCE),
                    id("spotify", "track", "38CLjvzuqaIADFFZrThgn5"),
                ],
                duration: Some(catalog::Duration {
                    milliseconds: 180000,
                    approximate: false,
                }),
            }],
        }],
    }
}
fn tags() -> ObservedMetadata {
    ObservedMetadata {
        track_title: Some("Song".into()),
        release_title: Some("Album".into()),
        track_artists: vec!["Artist".into()],
        release_artists: vec!["Artist".into()],
        disc_number: Some(1),
        track_number: Some(1),
        duration_ms: Some(180000),
        ..Default::default()
    }
}
fn claim(m: &mut ObservedMetadata, scope: Scope, semantics: Semantics, kind: &str, value: &str) {
    m.provenance.observations.push(Observation {
        identity: id("musicbrainz", kind, value),
        scope,
        semantics,
        origin: Origin::EmbeddedTag {
            format: "test".into(),
            field: kind.into(),
        },
    });
}
struct Tags(ObservedMetadata);
impl MetadataExtractor for Tags {
    fn supports(&self, p: &Path) -> bool {
        p.extension().is_some_and(|e| e == "mp3")
    }
    fn read(&mut self, _: &Path) -> Result<ObservedMetadata> {
        Ok(self.0.clone())
    }
}
fn run(l: &mut Library, r: Request, m: ObservedMetadata) -> Report {
    l.ingest_local(&r, &mut Tags(m), &mut |_| {}).unwrap()
}
fn count(db: &Connection, table: &str) -> i64 {
    db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
fn remote() -> RemoteCapability<'static> {
    RemoteCapability {
        provider: "spotify",
        unavailable: Some("provider outage".into()),
        catalog_available: false,
        accepts: |_| true,
    }
}
fn snapshot(db: &Connection) -> Vec<(String, String, String, i64)> {
    db.prepare("SELECT t.id,t.release_id,r.album_id,EXISTS(SELECT 1 FROM library_membership m WHERE m.track_id=t.id) FROM track t JOIN release r ON r.id=t.release_id ORDER BY t.id").unwrap().query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap()
}
fn provider_snapshot(db: &Connection) -> Vec<String> {
    ["artist","album","release","track","recording"].iter().flat_map(|entity|{
        db.prepare(&format!("SELECT {entity}_id || ':' || provider || ':' || kind || ':' || external_id FROM {entity}_external_identity ORDER BY {entity}_id,provider,kind,external_id")).unwrap().query_map([],|r|r.get::<_,String>(0)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap()
    }).collect()
}
fn playlist_tracks(l: &Library, p: &str) -> Vec<(String, TrackId, Option<u64>)> {
    l.playlist_entries(p, None, 200)
        .unwrap()
        .iter()
        .map(|r| {
            (
                r.id.clone(),
                r.track.as_ref().unwrap().track_id.clone(),
                r.playlist_position,
            )
        })
        .collect()
}
#[test]
fn catalog_saved_identity_gains_sources_through_every_admission_mode_and_restart() {
    for mode in 0..3 {
        let tmp = tempfile::tempdir().unwrap();
        let dbpath = tmp.path().join("db");
        let root = tmp.path().join("music");
        std::fs::create_dir(&root).unwrap();
        let mut l = Library::open(&dbpath).unwrap();
        let original = l.add_catalog_release(&catalog_release()).unwrap();
        let track = &original.track_ids[0];
        let db = Connection::open(&dbpath).unwrap();
        let before = snapshot(&db);
        let providers = provider_snapshot(&db);
        let artists = count(&db, "artist");
        assert_eq!(count(&db, "track_source"), 0);
        let playlist = l.create_playlist("Saved songs").unwrap();
        l.append_playlist_track(&playlist, track).unwrap();
        let entries = l.playlist_entries(&playlist, None, 200).unwrap();
        let f = root.join("song.mp3");
        std::fs::write(&f, b"disposable").unwrap();
        let request = match mode {
            0 => Request::Files(vec![f.clone()]),
            1 => Request::Folder(root.clone()),
            _ => Request::Rescan(l.register_local_root(&root).unwrap()),
        };
        let report = run(&mut l, request, tags());
        assert_eq!(
            (report.attached, report.imported, report.unresolved),
            (1, 0, 0)
        );
        assert_eq!(snapshot(&db), before);
        assert_eq!(provider_snapshot(&db), providers);
        assert_eq!(count(&db, "artist"), artists);
        assert_eq!(count(&db, "track"), 1);
        assert_eq!(count(&db, "release"), 1);
        assert_eq!(count(&db, "album"), 1);
        assert_eq!(count(&db, "track_external_identity"), 2);
        assert_eq!(count(&db, "recording_external_identity"), 1);
        assert_eq!(
            playlist_tracks(&l, &playlist),
            entries
                .iter()
                .map(|r| (
                    r.id.clone(),
                    r.track.as_ref().unwrap().track_id.clone(),
                    r.playlist_position
                ))
                .collect::<Vec<_>>()
        );
        assert!(matches!(
            l.playback_route(track, &remote()).unwrap(),
            Route::Local(_)
        ));
        assert_eq!(
            l.search(&SearchRequest {
                text: "Song".into(),
                ..Default::default()
            })
            .unwrap()
            .len(),
            1
        );
        let root_id = l.register_local_root(&root).unwrap();
        let repeat = run(&mut l, Request::Rescan(root_id.clone()), tags());
        assert_eq!(repeat.attached, 0);
        assert_eq!(count(&db, "track_source"), 1);
        assert_eq!(run(&mut l, Request::Files(vec![f]), tags()).attached, 0);
        assert_eq!(count(&db, "track_source"), 1);
        let second = root.join("copy.mp3");
        std::fs::write(&second, b"second copy").unwrap();
        assert_eq!(run(&mut l, Request::Rescan(root_id), tags()).attached, 1);
        assert_eq!(count(&db, "track_source"), 2);
        assert_eq!(snapshot(&db), before);
        assert_eq!(provider_snapshot(&db), providers);
        drop(l);
        let l = Library::open(&dbpath).unwrap();
        assert_eq!(snapshot(&db), before);
        assert_eq!(provider_snapshot(&db), providers);
        assert!(matches!(
            l.playback_route(track, &remote()).unwrap(),
            Route::Local(_)
        ));
    }
}
#[test]
fn attachment_and_automatic_rescan_preserve_explicitly_unsaved_catalog_track() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("db");
    let mut library = Library::open(&database).unwrap();
    let imported = library.add_catalog_release(&catalog_release()).unwrap();
    let track = &imported.track_ids[0];
    library.remove_from_library(track).unwrap();
    let root = temp.path().join("music");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("song.mp3"), b"disposable").unwrap();
    let rootid = library.register_local_root(&root).unwrap();
    assert_eq!(
        run(&mut library, Request::Rescan(rootid.clone()), tags()).attached,
        1
    );
    run(&mut library, Request::Rescan(rootid), tags());
    let db = Connection::open(&database).unwrap();
    assert_eq!(count(&db, "library_membership"), 0);
    assert_eq!(count(&db, "track"), 1);
}
#[test]
fn attachment_failure_rolls_back_all_associations_and_suppression_override() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("db");
    let root = temp.path().join("music");
    std::fs::create_dir(&root).unwrap();
    for name in ["a.mp3", "b.mp3"] {
        std::fs::write(root.join(name), b"file").unwrap();
    }
    let mut library = Library::open(&database).unwrap();
    library.add_catalog_release(&catalog_release()).unwrap();
    let rootid = library.register_local_root(&root).unwrap();
    library.scan_local_root(&rootid, &mut Tags(tags())).unwrap();
    let db = Connection::open(&database).unwrap();
    let before = snapshot(&db);
    let providers = provider_snapshot(&db);
    db.execute("INSERT INTO local_source_suppression(source_id) SELECT source_id FROM local_file_observation",[]).unwrap();
    db.execute_batch("CREATE TRIGGER reject_second_attachment BEFORE INSERT ON track_source WHEN (SELECT count(*) FROM track_source)>0 BEGIN SELECT RAISE(ABORT,'attachment test failure'); END;").unwrap();
    assert!(
        library
            .ingest_local(&Request::Folder(root), &mut Tags(tags()), &mut |_| {})
            .is_err()
    );
    assert_eq!(count(&db, "track_source"), 0);
    assert_eq!(count(&db, "local_source_suppression"), 2);
    assert_eq!(snapshot(&db), before);
    assert_eq!(provider_snapshot(&db), providers);
}

#[test]
fn exact_occurrence_and_album_recording_attach_offline_despite_display_override() {
    for occurrence in [true, false] {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db");
        let mut l = Library::open(&path).unwrap();
        let original = l.add_catalog_release(&catalog_release()).unwrap();
        let track = &original.track_ids[0];
        l.set_track_title_override(track, "My title").unwrap();
        let mut m = tags();
        m.track_title = Some("Different tag title".into());
        if occurrence {
            claim(
                &mut m,
                Scope::Occurrence,
                Semantics::OccurrenceIdentity,
                "track",
                OCCURRENCE,
            );
        } else {
            claim(
                &mut m,
                Scope::Album,
                Semantics::AlbumIdentity,
                "release_group",
                GROUP,
            );
            claim(
                &mut m,
                Scope::Recording,
                Semantics::RecordingIdentity,
                "recording",
                RECORDING,
            );
        }
        let file = tmp.path().join("song.mp3");
        std::fs::write(&file, b"disposable").unwrap();
        let report = run(&mut l, Request::Files(vec![file]), m);
        assert_eq!(report.attached, 1);
        assert_eq!(
            l.search(&SearchRequest {
                text: "My title".into(),
                ..Default::default()
            })
            .unwrap()
            .len(),
            1
        );
        assert!(matches!(
            l.playback_route(track, &remote()).unwrap(),
            Route::Local(_)
        ));
    }
}
#[test]
fn uncertain_or_conflicting_evidence_preserves_separate_tracks() {
    for case in 0..6 {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db");
        let mut l = Library::open(&path).unwrap();
        l.add_catalog_release(&catalog_release()).unwrap();
        let mut m = tags();
        match case {
            0 => {
                m.track_number = None;
            }
            1 => {
                m.track_title = Some("Song (Live)".into());
            }
            2 => {
                let mut r = catalog_release();
                r.identity.external_id = "66666666-6666-4666-8666-666666666666".into();
                l.add_catalog_release(&r).unwrap();
            }
            3 => {
                claim(
                    &mut m,
                    Scope::Recording,
                    Semantics::RecordingIdentity,
                    "recording",
                    "66666666-6666-4666-8666-666666666666",
                );
            }
            4 => {
                claim(
                    &mut m,
                    Scope::Occurrence,
                    Semantics::OccurrenceIdentity,
                    "track",
                    "malformed",
                );
            }
            _ => {
                claim(
                    &mut m,
                    Scope::Occurrence,
                    Semantics::OccurrenceIdentity,
                    "track",
                    OCCURRENCE,
                );
                claim(
                    &mut m,
                    Scope::Edition,
                    Semantics::EditionIdentity,
                    "release",
                    "66666666-6666-4666-8666-666666666666",
                );
            }
        }
        let db = Connection::open(&path).unwrap();
        let n = count(&db, "track");
        let f = tmp.path().join("song.mp3");
        std::fs::write(&f, b"file").unwrap();
        let report = run(&mut l, Request::Files(vec![f]), m);
        assert_eq!((report.attached, report.unresolved), (0, 1), "case {case}");
        assert_eq!(count(&db, "track"), n + 1);
    }
}
#[test]
fn suppressed_unassociated_source_is_ignored_then_explicitly_attached() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("db");
    let root = tmp.path().join("music");
    std::fs::create_dir(&root).unwrap();
    let f = root.join("song.mp3");
    std::fs::write(&f, b"file").unwrap();
    let mut l = Library::open(&path).unwrap();
    let original = l.add_catalog_release(&catalog_release()).unwrap();
    let rootid = l.register_local_root(&root).unwrap();
    l.scan_local_root(&rootid, &mut Tags(tags())).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute("INSERT INTO local_source_suppression(source_id) SELECT source_id FROM local_file_observation",[]).unwrap();
    assert_eq!(run(&mut l, Request::Rescan(rootid), tags()).attached, 0);
    assert_eq!(count(&db, "track_source"), 0);
    assert_eq!(run(&mut l, Request::Files(vec![f]), tags()).attached, 1);
    assert_eq!(count(&db, "local_source_suppression"), 0);
    assert_eq!(count(&db, "track"), 1);
    assert_eq!(
        l.search(&SearchRequest {
            limit: 200,
            ..Default::default()
        })
        .unwrap()[0]
            .track_id,
        original.track_ids[0]
    );
}
#[test]
fn spotify_playlist_only_track_attaches_without_membership_or_provenance_changes() {
    use music_library::playlist_import::{Item, Outcome, Plan};
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("db");
    let root = tmp.path().join("music");
    std::fs::create_dir(&root).unwrap();
    let mut l = Library::open(&path).unwrap();
    let credits = vec![catalog::Credit {
        identity: Some(id("spotify", "artist", "artist")),
        name: "Artist".into(),
        join_phrase: String::new(),
    }];
    let plan = Plan {
        provider: "spotify".into(),
        external_id: "playlist".into(),
        source_url: "https://open.spotify.com/playlist/playlist".into(),
        version: Some("v1".into()),
        owner: "Owner".into(),
        name: "Playlist".into(),
        unsupported: 0,
        unavailable: 0,
        items: vec![Item {
            identity: id("spotify", "track", "38CLjvzuqaIADFFZrThgn5"),
            title: "Song".into(),
            credits: credits.clone(),
            release_identity: id("spotify", "album", "album"),
            release_title: "Album".into(),
            release_credits: credits,
            year: Some(2012),
            disc: Some(1),
            number: Some(1),
            duration: None,
        }],
    };
    let Outcome::Imported { playlist_id, .. } = l.import_playlist_snapshot(&plan).unwrap() else {
        panic!()
    };
    let entries = l.playlist_entries(&playlist_id, None, 200).unwrap();
    let track = entries[0].track.as_ref().unwrap().track_id.clone();
    let db = Connection::open(&path).unwrap();
    let before = snapshot(&db);
    let providers = provider_snapshot(&db);
    let source: (String, String, String) = db
        .query_row(
            "SELECT provider,external_id,version FROM playlist_source",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    let f = root.join("song.mp3");
    std::fs::write(f, b"file").unwrap();
    let rootid = l.register_local_root(&root).unwrap();
    assert_eq!(
        run(&mut l, Request::Rescan(rootid.clone()), tags()).attached,
        1
    );
    assert_eq!(snapshot(&db), before);
    assert_eq!(provider_snapshot(&db), providers);
    assert_eq!(count(&db, "library_membership"), 0);
    assert_eq!(
        playlist_tracks(&l, &playlist_id),
        entries
            .iter()
            .map(|r| (
                r.id.clone(),
                r.track.as_ref().unwrap().track_id.clone(),
                r.playlist_position
            ))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        db.query_row(
            "SELECT provider,external_id,version FROM playlist_source",
            [],
            |r| Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?
            ))
        )
        .unwrap(),
        source
    );
    run(&mut l, Request::Rescan(rootid), tags());
    assert_eq!(count(&db, "library_membership"), 0);
    assert!(matches!(
        l.playback_route(&track, &remote()).unwrap(),
        Route::Local(_)
    ));
    drop(l);
    let l = Library::open(path).unwrap();
    assert!(matches!(
        l.playback_route(&track, &remote()).unwrap(),
        Route::Local(_)
    ));
    assert_eq!(snapshot(&db), before);
    assert_eq!(provider_snapshot(&db), providers);
}
#[test]
#[ignore = "uses ignored real-audio test-media fixture"]
fn real_get_disowned_catalog_tracks_gain_local_sources() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-media/Get Disowned");
    let tmp = tempfile::tempdir().unwrap();
    let dbpath = tmp.path().join("library.sqlite");
    let root = tmp.path().join("Hop Along/Get Disowned");
    std::fs::create_dir_all(&root).unwrap();
    let mut observations = vec![];
    for entry in std::fs::read_dir(&fixture).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|e| e == "mp3") {
            observations.push((
                entry.path(),
                LoftyMetadataExtractor.read(&entry.path()).unwrap(),
            ));
        }
    }
    observations.sort_by_key(|(_, m)| m.track_number);
    assert_eq!(observations.len(), 10);
    let release = support::get_disowned();
    let mut l = Library::open(&dbpath).unwrap();
    let original = l.add_catalog_release(&release).unwrap();
    let db = Connection::open(&dbpath).unwrap();
    let before = snapshot(&db);
    let providers = provider_snapshot(&db);
    let artists = count(&db, "artist");
    assert_eq!(count(&db, "track_source"), 0);
    println!(
        "before: {before:?}; provider identities: {}; local sources: 0",
        count(&db, "track_external_identity")
    );
    for (p, _) in observations {
        std::fs::copy(&p, root.join(p.file_name().unwrap())).unwrap();
    }
    let report = l
        .ingest_local(
            &Request::Folder(root),
            &mut LoftyMetadataExtractor,
            &mut |_| {},
        )
        .unwrap();
    assert_eq!((report.attached, report.imported), (10, 0));
    assert_eq!(snapshot(&db), before);
    assert_eq!(provider_snapshot(&db), providers);
    assert_eq!(count(&db, "track_source"), 10);
    assert_eq!(count(&db, "artist"), artists);
    assert_eq!(count(&db, "track_external_identity"), 10);
    assert_eq!(
        l.search(&SearchRequest {
            limit: 200,
            ..Default::default()
        })
        .unwrap()
        .len(),
        10
    );
    assert!(matches!(
        l.playback_route(&original.track_ids[0], &remote()).unwrap(),
        Route::Local(_)
    ));
    drop(l);
    let l = Library::open(&dbpath).unwrap();
    assert_eq!(snapshot(&db), before);
    assert_eq!(provider_snapshot(&db), providers);
    assert!(matches!(
        l.playback_route(&original.track_ids[0], &remote()).unwrap(),
        Route::Local(_)
    ));
    println!(
        "after/reopen: unchanged identities and membership; 10 local sources; local route without another scan"
    );
}

fn disappearance_cycle(real: bool) {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("db");
    let root = temp.path().join("music");
    std::fs::create_dir(&root).unwrap();
    let file = root.join("song.mp3");
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("test-media/Get Disowned/01 Some Grace.mp3");
    if real {
        std::fs::copy(&fixture, &file).unwrap();
    } else {
        std::fs::write(&file, b"disposable").unwrap();
    }
    let mut library = Library::open(&database).unwrap();
    let release = if real {
        support::get_disowned()
    } else {
        catalog_release()
    };
    let imported = library.add_catalog_release(&release).unwrap();
    let track = imported.track_ids[0].clone();
    let playlist = library.create_playlist("Preserved").unwrap();
    library.append_playlist_track(&playlist, &track).unwrap();
    let entries = playlist_tracks(&library, &playlist);
    let db = Connection::open(&database).unwrap();
    let identity = snapshot(&db);
    let providers = provider_snapshot(&db);
    let artists = count(&db, "artist");
    let mut extractor: Box<dyn MetadataExtractor> = if real {
        Box::new(LoftyMetadataExtractor)
    } else {
        Box::new(Tags(tags()))
    };
    let mut scan = |library: &mut Library, request| {
        library
            .ingest_local(&request, extractor.as_mut(), &mut |_| {})
            .unwrap()
    };
    scan(&mut library, Request::Folder(root.clone()));
    let root_id = library.local_locations().unwrap()[0].id.clone();
    let mut capability = remote();
    capability.unavailable = None;
    capability.catalog_available = true;
    let Route::Local(source) = library.playback_route(&track, &capability).unwrap() else {
        panic!("local")
    };
    println!(
        "before: Track={} source={} route=local",
        track.as_ref(),
        source.source_id.as_ref()
    );
    assert_eq!(
        scan(&mut library, Request::Rescan(root_id.clone())).unavailable,
        0
    );
    // Root offline and a partial traversal preserve unseen source availability.
    let offline = temp.path().join("offline");
    std::fs::rename(&root, &offline).unwrap();
    assert!(
        library
            .scan_local_root(&root_id, &mut Tags(tags()))
            .is_err()
    );
    assert_eq!(
        scan(&mut library, Request::Rescan(root_id.clone())).locations_failed,
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT available FROM local_file_observation WHERE source_id=?1",
            [source.source_id.as_ref()],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    std::fs::write(&root, b"root replaced by non-directory").unwrap();
    assert_eq!(
        scan(&mut library, Request::Rescan(root_id.clone())).locations_failed,
        1
    );
    std::fs::remove_file(&root).unwrap();
    std::fs::rename(&offline, &root).unwrap();
    std::fs::remove_file(&file).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let blocked = root.join("blocked");
        std::fs::create_dir(&blocked).unwrap();
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o0)).unwrap();
        assert!(
            library
                .scan_local_root(&root_id, &mut Tags(tags()))
                .is_err()
        );
        let partial = scan(&mut library, Request::Rescan(root_id.clone()));
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(partial.locations_failed, 1);
        assert_eq!(partial.unavailable, 0);
        assert_eq!(
            db.query_row(
                "SELECT available FROM local_file_observation WHERE source_id=?1",
                [source.source_id.as_ref()],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }
    assert_eq!(
        scan(&mut library, Request::Rescan(root_id.clone())).unavailable,
        1
    );
    let Route::Remote(spotify) = library.playback_route(&track, &capability).unwrap() else {
        panic!("persisted Spotify")
    };
    println!(
        "absent: Track={} source={} route=Spotify {}",
        track.as_ref(),
        source.source_id.as_ref(),
        spotify.external_id
    );
    assert_eq!(snapshot(&db), identity);
    assert_eq!(provider_snapshot(&db), providers);
    assert_eq!(count(&db, "artist"), artists);
    assert_eq!(playlist_tracks(&library, &playlist), entries);
    assert!(
        !library
            .search(&SearchRequest {
                text: release.media[0].tracks[0].title.clone(),
                ..Default::default()
            })
            .unwrap()
            .is_empty()
    );
    assert_eq!(count(&db, "local_source_suppression"), 0);
    assert_eq!(
        scan(&mut library, Request::Rescan(root_id.clone())).unavailable,
        0
    );
    drop(library);
    let mut library = Library::open(&database).unwrap();
    assert_eq!(
        library.playback_route(&track, &capability).unwrap(),
        Route::Remote(spotify)
    );
    if real {
        std::fs::copy(&fixture, &file).unwrap();
    } else {
        std::fs::write(&file, b"disposable").unwrap();
    }
    scan(&mut library, Request::Rescan(root_id));
    assert_eq!(
        library.playback_route(&track, &capability).unwrap(),
        Route::Local(source.clone())
    );
    assert_eq!(count(&db, "local_file_observation"), 1);
    assert_eq!(snapshot(&db), identity);
    assert_eq!(provider_snapshot(&db), providers);
    println!(
        "returned: Track={} source={} route=local",
        track.as_ref(),
        source.source_id.as_ref()
    );
}
#[test]
fn source_disappearance_preserves_saved_identity_playlist_search_and_restart() {
    disappearance_cycle(false);
}
#[test]
#[ignore = "uses disposable copy of ignored real-audio test-media"]
fn real_source_disappearance_local_spotify_local() {
    disappearance_cycle(true);
}

#[test]
fn availability_is_per_source_root_scoped_and_independent_of_suppression() {
    use music_library::library_removal::Target;
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("db");
    let mut l = Library::open(&database).unwrap();
    let track = l.add_catalog_release(&catalog_release()).unwrap().track_ids[0].clone();
    let mut roots = vec![];
    let mut paths = vec![];
    for name in ["one", "two"] {
        let root = temp.path().join(name);
        std::fs::create_dir(&root).unwrap();
        let file = root.join("song.mp3");
        std::fs::write(&file, b"disposable").unwrap();
        run(&mut l, Request::Folder(root.clone()), tags());
        roots.push(l.register_local_root(&root).unwrap());
        paths.push(file);
    }
    let db = Connection::open(&database).unwrap();
    let mut capability = remote();
    capability.unavailable = None;
    std::fs::remove_file(&paths[0]).unwrap();
    assert_eq!(
        run(&mut l, Request::Rescan(roots[0].clone()), tags()).unavailable,
        1
    );
    assert!(matches!(
        l.playback_route(&track, &capability).unwrap(),
        Route::Local(_)
    ));
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM local_file_observation WHERE available=1",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Same bytes and mtime: the incremental fast path must still check readability.
        std::fs::set_permissions(&paths[1], std::fs::Permissions::from_mode(0o0)).unwrap();
        assert!(matches!(
            l.playback_route(&track, &capability).unwrap(),
            Route::Remote(_)
        ));
        assert!(l.scan_local_root(&roots[1], &mut Tags(tags())).is_err());
        assert_eq!(
            db.query_row(
                "SELECT available FROM local_file_observation WHERE root_id=?1",
                [roots[1].as_ref()],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(
            run(&mut l, Request::Rescan(roots[1].clone()), tags()).unavailable,
            0
        );
        std::fs::set_permissions(&paths[1], std::fs::Permissions::from_mode(0o600)).unwrap();
        // Known-unavailable routes stay skipped until a new observation reactivates them.
        assert!(matches!(
            l.playback_route(&track, &capability).unwrap(),
            Route::Remote(_)
        ));
        run(&mut l, Request::Rescan(roots[1].clone()), tags());
        assert!(matches!(
            l.playback_route(&track, &capability).unwrap(),
            Route::Local(_)
        ));
    }
    l.remove_library_object(&Target::Track(track.clone()), true)
        .unwrap();
    std::fs::remove_file(&paths[1]).unwrap();
    run(&mut l, Request::Rescan(roots[1].clone()), tags());
    assert_eq!(count(&db, "local_source_suppression"), 2);
    std::fs::write(&paths[1], b"disposable").unwrap();
    run(&mut l, Request::Rescan(roots[1].clone()), tags());
    assert_eq!(count(&db, "library_membership"), 0);
    assert_eq!(count(&db, "local_source_suppression"), 2);
    run(&mut l, Request::Files(vec![paths[1].clone()]), tags());
    assert_eq!(count(&db, "library_membership"), 1);
    assert_eq!(count(&db, "local_source_suppression"), 1);
    assert_eq!(count(&db, "local_file_observation"), 2);
}

#[test]
fn reconciliation_and_playback_use_root_and_track_indexes() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("db");
    let _library = Library::open(&database).unwrap();
    let db = Connection::open(database).unwrap();
    let plan = |sql: &str| {
        db.prepare(sql)
            .unwrap()
            .query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
            .join("\n")
    };
    let reconciliation = plan(
        "EXPLAIN QUERY PLAN UPDATE local_file_observation SET available=0 WHERE available=1 AND source_id IN (SELECT source_id FROM local_root_source WHERE root_id='root' AND (last_seen_scan_id IS NULL OR last_seen_scan_id<>123))",
    );
    assert!(
        reconciliation.contains(
            "SEARCH local_root_source USING INDEX sqlite_autoindex_local_root_source_1 (root_id=?)"
        ),
        "{reconciliation}"
    );
    assert!(
        !reconciliation.contains("SCAN local_file_observation"),
        "{reconciliation}"
    );
    let playback = plan(
        "EXPLAIN QUERY PLAN SELECT ps.id,l.path FROM track_source ts CROSS JOIN playable_source ps ON ps.id=ts.source_id CROSS JOIN local_file_observation l ON l.source_id=ps.id WHERE ts.track_id='track' AND ps.kind='local_file' AND l.available=1 ORDER BY ts.source_id COLLATE BINARY",
    );
    assert!(
        playback.contains(
            "SEARCH ts USING COVERING INDEX sqlite_autoindex_track_source_2 (track_id=?)"
        ),
        "{playback}"
    );
    assert!(!playback.contains("SCAN l"), "{playback}");
}
