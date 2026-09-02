use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use music_library::domain::{
    ArtistCreditInput, ImportReleaseRequest, ImportTrackInput, ObservedMetadata, SearchRequest,
};
use music_library::filesystem::MetadataExtractor;
use music_library::{Error, Library, Result};
use tempfile::TempDir;

#[derive(Default)]
struct FakeExtractor {
    metadata: HashMap<PathBuf, ObservedMetadata>,
    reads: usize,
    fail_on: Option<PathBuf>,
}

impl MetadataExtractor for FakeExtractor {
    fn supports(&self, path: &Path) -> bool {
        path.extension().is_some_and(|extension| extension == "mp3")
    }

    fn read(&mut self, path: &Path) -> Result<ObservedMetadata> {
        self.reads += 1;
        if self.fail_on.as_deref() == Some(path) {
            return Err(Error::Metadata {
                path: path.to_path_buf(),
                message: "intentional test interruption".into(),
            });
        }
        self.metadata
            .get(path)
            .cloned()
            .ok_or_else(|| Error::Metadata {
                path: path.to_path_buf(),
                message: "missing fake metadata".into(),
            })
    }
}

fn observed(title: &str, track_number: u32) -> ObservedMetadata {
    ObservedMetadata {
        track_title: Some(title.into()),
        release_title: Some("Observed Release".into()),
        track_artists: vec!["Observed Artist".into()],
        release_artists: vec!["Observed Artist".into()],
        disc_number: Some(1),
        track_number: Some(track_number),
        year: Some(2026),
        duration_ms: Some(180_000),
        format: Some("mp3".into()),
    }
}

fn search(library: &Library, text: &str) -> Vec<music_library::domain::TrackSearchResult> {
    library
        .search(&SearchRequest {
            text: text.into(),
            limit: 20,
            ..SearchRequest::default()
        })
        .unwrap()
}

#[test]
fn discover_import_search_override_membership_and_reconcile() {
    let temp = TempDir::new().unwrap();
    let root_path = temp.path().join("music");
    fs::create_dir(&root_path).unwrap();
    let first_path = root_path.join("01-first.mp3");
    let second_path = root_path.join("02-second.mp3");
    fs::write(&first_path, b"first fake audio").unwrap();
    fs::write(&second_path, b"second fake audio").unwrap();

    let mut extractor = FakeExtractor::default();
    extractor
        .metadata
        .insert(first_path.clone(), observed("First Song", 1));
    let mut second_observation = observed("Second Song", 2);
    second_observation.track_title = None;
    extractor
        .metadata
        .insert(second_path.clone(), second_observation);

    let mut library = Library::open(temp.path().join("library.sqlite")).unwrap();
    let root_id = library.register_local_root(&root_path).unwrap();
    let first_scan = library.scan_local_root(&root_id, &mut extractor).unwrap();
    assert_eq!(first_scan.discovered, 2);
    assert_eq!(first_scan.parsed, 2);
    assert_eq!(first_scan.unchanged, 0);
    assert_eq!(extractor.reads, 2);

    let second_scan = library.scan_local_root(&root_id, &mut extractor).unwrap();
    assert_eq!(second_scan.unchanged, 2);
    assert_eq!(second_scan.parsed, 0);
    assert_eq!(extractor.reads, 2, "unchanged files must not be reparsed");

    let candidates = library.list_discovery_candidates(None, 20).unwrap();
    assert_eq!(candidates.len(), 2);
    let first = candidates
        .iter()
        .find(|item| item.path == first_path)
        .unwrap();
    let second = candidates
        .iter()
        .find(|item| item.path == second_path)
        .unwrap();
    assert_eq!(first.metadata.track_title.as_deref(), Some("First Song"));

    let imported = library
        .import_release(&ImportReleaseRequest {
            release_title: "Chosen Edition".into(),
            release_artists: vec![ArtistCreditInput {
                name: "Release Artist".into(),
                role: Some("primary".into()),
            }],
            tracks: vec![
                ImportTrackInput {
                    source_id: first.source_id.clone(),
                    title_fallback: None,
                    artists: vec![ArtistCreditInput {
                        name: "Track Artist".into(),
                        role: Some("primary".into()),
                    }],
                    disc_number: Some(1),
                    track_number: Some(1),
                },
                ImportTrackInput {
                    source_id: second.source_id.clone(),
                    title_fallback: Some("Second Song".into()),
                    artists: vec![ArtistCreditInput {
                        name: "Track Artist".into(),
                        role: Some("primary".into()),
                    }],
                    disc_number: Some(1),
                    track_number: Some(2),
                },
            ],
        })
        .unwrap();
    assert_eq!(imported.track_ids.len(), 2);
    assert!(
        library
            .list_discovery_candidates(None, 20)
            .unwrap()
            .is_empty()
    );

    let result = search(&library, "First Track Artist Chosen");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].title, "First Song");
    assert_eq!(result[0].release_title, "Chosen Edition");
    assert_eq!(result[0].artist_names, "Track Artist");
    assert_eq!(search(&library, "").len(), 2);

    let first_track = result[0].track_id.clone();
    library.set_track_title_override(&first_track, "").unwrap();
    assert_eq!(search(&library, "Chosen Edition").len(), 2);
    library.clear_track_title_override(&first_track).unwrap();
    library
        .set_track_title_override(&first_track, "Personal Name")
        .unwrap();
    assert!(search(&library, "First Song").is_empty());
    assert_eq!(search(&library, "Personal")[0].title, "Personal Name");
    library.clear_track_title_override(&first_track).unwrap();
    assert_eq!(search(&library, "First Song")[0].title, "First Song");

    assert!(library.remove_from_library(&first_track).unwrap());
    library.scan_local_root(&root_id, &mut extractor).unwrap();
    assert!(
        search(&library, "First Song").is_empty(),
        "scan must not restore membership"
    );

    fs::remove_file(&second_path).unwrap();
    let missing_scan = library.scan_local_root(&root_id, &mut extractor).unwrap();
    assert_eq!(missing_scan.unavailable, 1);
    let unavailable = search(&library, "Second Song");
    assert_eq!(unavailable.len(), 1);
    assert!(!unavailable[0].available);

    fs::write(&second_path, b"restored fake audio with changed size").unwrap();
    let restored_scan = library.scan_local_root(&root_id, &mut extractor).unwrap();
    assert_eq!(restored_scan.parsed, 1);
    assert!(search(&library, "Second Song")[0].available);
}

#[test]
fn interrupted_scan_does_not_reconcile_unvisited_sources() {
    let temp = TempDir::new().unwrap();
    let root_path = temp.path().join("music");
    fs::create_dir(&root_path).unwrap();
    let keep_path = root_path.join("01-keep.mp3");
    let missing_path = root_path.join("02-missing.mp3");
    fs::write(&keep_path, b"keep").unwrap();
    fs::write(&missing_path, b"missing").unwrap();

    let mut extractor = FakeExtractor::default();
    extractor
        .metadata
        .insert(keep_path.clone(), observed("Keep", 1));
    extractor
        .metadata
        .insert(missing_path.clone(), observed("Still Known", 2));
    let mut library = Library::open_in_memory().unwrap();
    let root_id = library.register_local_root(&root_path).unwrap();
    library.scan_local_root(&root_id, &mut extractor).unwrap();
    let candidates = library.list_discovery_candidates(None, 20).unwrap();
    let missing = candidates
        .iter()
        .find(|item| item.path == missing_path)
        .unwrap();
    library
        .import_release(&ImportReleaseRequest {
            release_title: "Release".into(),
            release_artists: Vec::new(),
            tracks: vec![ImportTrackInput {
                source_id: missing.source_id.clone(),
                title_fallback: None,
                artists: Vec::new(),
                disc_number: Some(1),
                track_number: Some(2),
            }],
        })
        .unwrap();

    fs::remove_file(&missing_path).unwrap();
    let failing_path = root_path.join("03-fail.mp3");
    fs::write(&failing_path, b"fail").unwrap();
    extractor.fail_on = Some(failing_path.clone());
    let error = library
        .scan_local_root(&root_id, &mut extractor)
        .unwrap_err();
    assert!(matches!(error, Error::Metadata { .. }));

    let result = search(&library, "Still Known");
    assert_eq!(result.len(), 1);
    assert!(
        result[0].available,
        "failed scan must not reconcile unseen files"
    );
}

#[test]
fn candidate_and_search_results_are_bounded() {
    let temp = TempDir::new().unwrap();
    let root_path = temp.path().join("music");
    fs::create_dir(&root_path).unwrap();
    let mut extractor = FakeExtractor::default();
    for number in 0..3 {
        let path = root_path.join(format!("{number}.mp3"));
        fs::write(&path, number.to_string()).unwrap();
        extractor
            .metadata
            .insert(path, observed(&format!("Song {number}"), number + 1));
    }
    let mut library = Library::open_in_memory().unwrap();
    let root_id = library.register_local_root(&root_path).unwrap();
    library.scan_local_root(&root_id, &mut extractor).unwrap();
    assert_eq!(library.list_discovery_candidates(None, 1).unwrap().len(), 1);
}
