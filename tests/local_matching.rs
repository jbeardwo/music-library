use music_library::{Library, Result, catalog, domain::*, filesystem::MetadataExtractor};
use rusqlite::Connection;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

struct Extractor(HashMap<PathBuf, ObservedMetadata>);
impl MetadataExtractor for Extractor {
    fn supports(&self, _: &Path) -> bool {
        true
    }
    fn read(&mut self, path: &Path) -> Result<ObservedMetadata> {
        Ok(self.0[path].clone())
    }
}
struct Fixture {
    temp: tempfile::TempDir,
    library: Library,
    root: RootId,
    extractor: Extractor,
}
fn credit(name: &str) -> catalog::Credit {
    catalog::Credit {
        identity: None,
        name: name.into(),
        join_phrase: String::new(),
    }
}
fn identity(kind: &str, id: &str) -> ExternalIdentity {
    ExternalIdentity {
        provider: "musicbrainz".into(),
        kind: kind.into(),
        external_id: id.into(),
    }
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("music")).unwrap();
        let mut library = Library::open(temp.path().join("db")).unwrap();
        let root = library
            .register_local_root(temp.path().join("music"))
            .unwrap();
        Self {
            temp,
            library,
            root,
            extractor: Extractor(HashMap::new()),
        }
    }
    fn db(&self) -> Connection {
        Connection::open(self.temp.path().join("db")).unwrap()
    }
    fn catalog(
        &mut self,
        title: &str,
        artist: &str,
        group: &str,
        edition: &str,
        count: u32,
    ) -> ImportedRelease {
        self.library
            .add_catalog_release(&catalog::Release {
                album: catalog::Album {
                    identity: identity("release_group", group),
                    title: title.into(),
                    date: "2005".into(),
                    credits: vec![credit(artist)],
                },
                identity: identity("release", edition),
                identities: vec![],
                title: title.into(),
                date: "2005".into(),
                credits: vec![credit(artist)],
                media: vec![catalog::Medium {
                    position: 1,
                    tracks: (1..=count)
                        .map(|n| catalog::Track {
                            position: n,
                            title: format!("Song {n}"),
                            credits: vec![credit(artist)],
                            identities: vec![
                                identity("track", &format!("{edition}-{n}")),
                                identity("recording", &format!("recording-{n}")),
                            ],
                        })
                        .collect(),
                }],
            })
            .unwrap()
    }
    fn files(&mut self, title: &str, artist: &str, numbers: &[u32]) -> ImportReleaseRequest {
        for &n in numbers {
            let path = self
                .temp
                .path()
                .join("music")
                .join(format!("{}-{n}.mp3", self.extractor.0.len()));
            fs::write(&path, b"test source").unwrap();
            self.extractor.0.insert(
                path,
                ObservedMetadata {
                    track_title: Some(format!("Song {n}")),
                    release_title: Some(title.into()),
                    track_artists: vec![artist.into()],
                    disc_number: Some(1),
                    track_number: Some(n),
                    year: Some(2025),
                    ..Default::default()
                },
            );
        }
        self.library
            .scan_local_root(&self.root, &mut self.extractor)
            .unwrap();
        let candidates = self.library.list_discovery_candidates(None, 200).unwrap();
        ImportReleaseRequest {
            release_title: title.into(),
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
        }
    }
    fn count(&self, table: &str) -> i64 {
        self.db()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
}

#[test]
fn complete_three_track_and_one_track_subsets_reuse_album_but_not_edition() {
    for numbers in [(1..=15).collect::<Vec<_>>(), vec![2, 5, 10], vec![5]] {
        let mut f = Fixture::new();
        let old = f.catalog("Demon Days", "Gorillaz", "group", "edition", 15);
        let album = f.library.album_for_release(&old.release_id).unwrap();
        for track in &old.track_ids {
            f.library.remove_from_library(track).unwrap();
        }
        let request = f.files("  DEMON\t Days  ", " GORILLAZ ", &numbers);
        let imported = f.library.import_release(&request).unwrap();
        assert_eq!(
            f.library.album_for_release(&imported.release_id).unwrap(),
            album
        );
        assert_ne!(imported.release_id, old.release_id);
        assert_eq!(imported.track_ids.len(), numbers.len());
        assert_eq!(f.count("album"), 1);
        assert_eq!(f.count("release"), 2);
        assert_eq!(f.count("track"), 15 + numbers.len() as i64);
        assert_eq!(f.count("library_membership"), numbers.len() as i64);
        assert_eq!(f.count("track_source"), numbers.len() as i64);
        assert!(
            f.library
                .list_release_external_identities(&imported.release_id)
                .unwrap()
                .is_empty()
        );
        for track in &imported.track_ids {
            assert!(!old.track_ids.contains(track));
            assert!(
                f.library
                    .list_track_external_identities(track)
                    .unwrap()
                    .is_empty()
            );
            assert!(
                f.library
                    .available_playback_source(track)
                    .unwrap()
                    .is_some()
            );
        }
        for track in &old.track_ids {
            assert!(
                f.library
                    .available_playback_source(track)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                f.library
                    .list_track_external_identities(track)
                    .unwrap()
                    .len(),
                2
            );
        }
        assert_eq!(
            f.library
                .list_release_external_identities(&old.release_id)
                .unwrap(),
            vec![identity("release", "edition")]
        );
        let scan = f
            .library
            .scan_local_root(&f.root, &mut f.extractor)
            .unwrap();
        assert_eq!(scan.unchanged, numbers.len() as u64);
        assert!(
            f.library
                .list_discovery_candidates(None, 200)
                .unwrap()
                .is_empty()
        );
        // Explicit replay remains rejected, atomically, as before; scanner idempotence
        // comes from durable source identity/association, never metadata Track guessing.
        assert!(f.library.import_release(&request).is_err());
        assert_eq!(f.count("track"), 15 + numbers.len() as i64);
    }
}

#[test]
fn additional_local_tracks_reuse_album_and_multiple_catalog_editions_are_not_evidence() {
    let mut f = Fixture::new();
    let first = f.catalog("Album", "Artist", "group", "original", 15);
    f.catalog("Album", "Artist", "group", "reissue", 16);
    let request = f.files("Album", "Artist", &[2, 5, 10]);
    let partial = f.library.import_release(&request).unwrap();
    let request = f.files("Album", "Artist", &[11]);
    let later = f.library.import_release(&request).unwrap();
    let album = f.library.album_for_release(&first.release_id).unwrap();
    assert_eq!(
        f.library.album_for_release(&partial.release_id).unwrap(),
        album
    );
    assert_eq!(
        f.library.album_for_release(&later.release_id).unwrap(),
        album
    );
    assert_ne!(partial.release_id, later.release_id);
    assert_eq!(f.count("album"), 1);
    assert_eq!(f.count("release"), 4);
    assert_eq!(f.count("track"), 35);
}

#[test]
fn local_first_partial_then_overlapping_more_tracks_does_not_need_catalog_identity() {
    let mut f = Fixture::new();
    let request = f.files("Local Album", "Artist", &[2]);
    let first = f.library.import_release(&request).unwrap();
    let request = f.files("Local Album", "Artist", &[2, 5, 10]);
    let next = f.library.import_release(&request).unwrap();
    assert_eq!(
        f.library.album_for_release(&first.release_id).unwrap(),
        f.library.album_for_release(&next.release_id).unwrap()
    );
    assert_eq!(f.count("album"), 1);
    assert_eq!(f.count("track"), 4);
}

#[test]
fn artist_disambiguates_but_identical_album_candidates_are_not_ranked_arbitrarily() {
    let mut f = Fixture::new();
    f.catalog("Home", "Other Artist", "other", "other", 4);
    let target = f.catalog("Home", "Artist", "target", "target", 15);
    let request = f.files("Home", "Artist", &[2]);
    let found = f.library.import_release(&request).unwrap();
    assert_eq!(
        f.library.album_for_release(&found.release_id).unwrap(),
        f.library.album_for_release(&target.release_id).unwrap()
    );
    f.catalog("Home", "Artist", "ambiguous", "ambiguous", 15);
    let request = f.files("Home", "Artist", &[5]);
    let uncertain = f.library.import_release(&request).unwrap();
    assert_ne!(
        f.library
            .album_for_release(&uncertain.release_id)
            .unwrap()
            .album_id,
        f.library
            .album_for_release(&target.release_id)
            .unwrap()
            .album_id
    );
    assert_eq!(f.count("album"), 4);
}

#[test]
fn poor_unrelated_qualified_and_contradictory_metadata_import_without_matching() {
    for (title, artist) in [
        ("Unknown Album", "Artist"),
        ("Album", "Unknown Artist"),
        ("", "Artist"),
        ("Unrelated", "Artist"),
        ("Album (Live)", "Artist"),
        ("Album [Remix]", "Artist"),
    ] {
        let mut f = Fixture::new();
        f.catalog("Album", "Artist", "group", "edition", 15);
        let request = f.files(title, artist, &[1]);
        f.library.import_release(&request).unwrap();
        assert_eq!(f.count("album"), 2);
    }
    let mut f = Fixture::new();
    f.catalog("Album", "Artist", "group", "edition", 15);
    let mut request = f.files("Album", "Artist", &[1, 2]);
    request.tracks[0].artists = vec![ArtistCreditInput {
        name: "Contradiction".into(),
        role: None,
    }];
    f.library.import_release(&request).unwrap();
    assert_eq!(f.count("album"), 2);
}

#[test]
fn eligibility_uses_tagged_track_titles_not_filename_fallbacks_and_import_rolls_back() {
    let mut f = Fixture::new();
    f.catalog("Album", "Artist", "group", "edition", 15);
    let mut request = f.files("Album", "Artist", &[1]);
    f.db()
        .execute("UPDATE file_metadata_observation SET track_title=NULL", [])
        .unwrap();
    request.tracks[0].title_fallback = Some("file.mp3".into());
    f.library.import_release(&request).unwrap();
    assert_eq!(f.count("album"), 2);
    let request = f.files("Album", "Artist", &[3]);
    let before = (
        f.count("album"),
        f.count("release"),
        f.count("track"),
        f.count("track_source"),
        f.count("library_membership"),
    );
    f.db().execute_batch("CREATE TRIGGER fail_local BEFORE INSERT ON library_membership BEGIN SELECT RAISE(ABORT,'induced'); END;").unwrap();
    assert!(f.library.import_release(&request).is_err());
    assert_eq!(
        before,
        (
            f.count("album"),
            f.count("release"),
            f.count("track"),
            f.count("track_source"),
            f.count("library_membership")
        )
    );
    assert_eq!(
        f.library
            .list_discovery_candidates(None, 200)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn album_artist_display_matches_catalog_join_phrases_despite_different_track_artists() {
    let mut f = Fixture::new();
    let old = f
        .library
        .add_catalog_release(&catalog::Release {
            album: catalog::Album {
                identity: identity("release_group", "group"),
                title: "Album".into(),
                date: "2005".into(),
                credits: vec![
                    catalog::Credit {
                        identity: None,
                        name: "Artist A".into(),
                        join_phrase: " feat. ".into(),
                    },
                    credit("Artist B"),
                ],
            },
            identity: identity("release", "edition"),
            identities: vec![],
            title: "Album".into(),
            date: "2005".into(),
            credits: vec![credit("Artist A")],
            media: vec![catalog::Medium {
                position: 1,
                tracks: vec![catalog::Track {
                    position: 1,
                    title: "Song 1".into(),
                    credits: vec![credit("Artist A")],
                    identities: vec![identity("track", "track")],
                }],
            }],
        })
        .unwrap();
    let request = f.files("Album", "Different soloist", &[1]);
    // Simulate the AlbumArtist observations read by the normal tag extractor.
    for track in &request.tracks {
        f.db().execute("INSERT INTO file_artist_observation(source_id,scope,position,name) VALUES (?1,'release',0,' Artist A feat. Artist B ')", [track.source_id.as_ref()]).unwrap();
    }
    let imported = f.library.import_release(&request).unwrap();
    assert_eq!(
        f.library.album_for_release(&old.release_id).unwrap(),
        f.library.album_for_release(&imported.release_id).unwrap()
    );
    assert_ne!(old.release_id, imported.release_id);
}

#[test]
fn lofty_reads_album_artist_without_rewriting_credit() {
    use lofty::{
        config::WriteOptions,
        prelude::TagExt,
        tag::{ItemKey, Tag, TagType},
    };
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tagged.wav");
    // A tiny real PCM WAV keeps the parser test independent of external tools/collections.
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
    fs::write(&path, wav).unwrap();
    let mut tag = Tag::new(TagType::Id3v2);
    tag.insert_text(ItemKey::AlbumArtist, "Artist A feat. Artist B".into());
    tag.insert_text(ItemKey::TrackArtist, "Soloist".into());
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    let observed = music_library::filesystem::LoftyMetadataExtractor
        .read(&path)
        .unwrap();
    assert_eq!(observed.release_artists, vec!["Artist A feat. Artist B"]);
    assert_eq!(observed.track_artists, vec!["Soloist"]);
}

#[test]
fn contradictory_album_tags_and_placeholder_track_titles_decline_matching() {
    for update in [
        "UPDATE file_metadata_observation SET release_title='Different Album' WHERE track_number=2",
        "UPDATE file_metadata_observation SET track_title='Unknown Track'",
        "INSERT INTO file_artist_observation(source_id,scope,position,name) SELECT source_id,'release',0,CASE track_number WHEN 1 THEN 'Artist' ELSE 'Different Artist' END FROM file_metadata_observation",
    ] {
        let mut f = Fixture::new();
        f.catalog("Album", "Artist", "group", "edition", 15);
        let request = f.files("Album", "Artist", &[1, 2]);
        f.db().execute_batch(update).unwrap();
        let imported = f.library.import_release(&request).unwrap();
        assert_eq!(f.count("album"), 2);
        assert_eq!(imported.track_ids.len(), 2);
    }
}

#[test]
fn title_artist_candidates_require_track_support_and_can_be_disambiguated() {
    for update in [
        "UPDATE file_metadata_observation SET track_title='Unrelated song'",
        "UPDATE file_metadata_observation SET track_title='Song 1 (Live)'",
    ] {
        let mut f = Fixture::new();
        f.catalog("Album", "Artist", "group", "edition", 15);
        let request = f.files("Album", "Artist", &[1]);
        f.db().execute_batch(update).unwrap();
        f.library.import_release(&request).unwrap();
        assert_eq!(f.count("album"), 2);
    }
    let mut f = Fixture::new();
    let target = f.catalog("Album", "Artist", "group", "edition", 15);
    let other = f.catalog("Album", "Artist", "other", "other", 15);
    f.db().execute("UPDATE effective_track_metadata SET title='Different' WHERE track_id IN (SELECT id FROM track WHERE release_id=?1)", [other.release_id.as_ref()]).unwrap();
    let request = f.files("Album", "Artist", &[2, 5, 10]);
    let imported = f.library.import_release(&request).unwrap();
    assert_eq!(
        f.library.album_for_release(&imported.release_id).unwrap(),
        f.library.album_for_release(&target.release_id).unwrap()
    );
    assert_ne!(imported.release_id, target.release_id);
    assert!(
        imported
            .track_ids
            .iter()
            .all(|t| !target.track_ids.contains(t))
    );
}

#[test]
fn no_overlap_missing_positions_and_contradictory_overlap_do_not_support_an_album() {
    for numbers in [vec![20], vec![1, 2]] {
        let mut f = Fixture::new();
        f.catalog("Album", "Artist", "group", "edition", 15);
        let request = f.files("Album", "Artist", &numbers);
        if numbers.len() == 2 {
            f.db().execute("UPDATE file_metadata_observation SET track_title='Contradiction' WHERE track_number=2", []).unwrap();
        }
        f.library.import_release(&request).unwrap();
        assert_eq!(f.count("album"), 2);
    }
    let mut f = Fixture::new();
    f.catalog("Album", "Artist", "group", "edition", 15);
    let mut request = f.files("Album", "Artist", &[1]);
    request.tracks[0].disc_number = None;
    f.library.import_release(&request).unwrap();
    assert_eq!(f.count("album"), 2);
}

#[test]
fn track_support_cannot_be_assembled_across_incompatible_editions() {
    let mut f = Fixture::new();
    let first = f.catalog("Album", "Artist", "group", "first", 2);
    let second = f.catalog("Album", "Artist", "group", "second", 2);
    for (edition, position) in [(first.release_id, 2), (second.release_id, 1)] {
        f.db().execute("UPDATE effective_track_metadata SET title='Contradiction' WHERE track_id IN (SELECT id FROM track WHERE release_id=?1 AND track_number=?2)", rusqlite::params![edition.as_ref(),position]).unwrap();
    }
    let request = f.files("Album", "Artist", &[1, 2]);
    f.library.import_release(&request).unwrap();
    assert_eq!(f.count("album"), 2);
}

#[test]
fn oversized_candidate_group_is_declined_instead_of_accepting_a_truncated_set() {
    let mut f = Fixture::new();
    for n in 0..65 {
        f.catalog("Album", "Artist", &format!("g{n}"), &format!("r{n}"), 1);
    }
    let request = f.files("Album", "Artist", &[1]);
    f.library.import_release(&request).unwrap();
    assert_eq!(f.count("album"), 66);
}

mod external {
    use super::*;
    use music_library::album_matching::{AlbumMatcher, AutoMatchPolicy, MatchOutcome, MatchReply};
    use std::{
        collections::VecDeque,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        time::Duration,
    };
    struct Provider {
        pages: VecDeque<
            std::result::Result<catalog::Page<catalog::AlbumCandidate>, catalog::CatalogError>,
        >,
        calls: Arc<AtomicUsize>,
        gate: Option<mpsc::Receiver<()>>,
    }
    impl catalog::CatalogProvider for Provider {
        fn search_artists(
            &mut self,
            name: &str,
        ) -> std::result::Result<catalog::Page<catalog::ArtistCandidate>, catalog::CatalogError>
        {
            Ok(catalog::Page {
                next_offset: None,
                items: vec![catalog::ArtistCandidate {
                    identity: identity("artist", "artist-id"),
                    name: name.into(),
                    comment: String::new(),
                    country: String::new(),
                    artist_type: String::new(),
                    score: None,
                }],
            })
        }
        fn artist_albums(
            &mut self,
            artist: &ExternalIdentity,
            title: &str,
        ) -> std::result::Result<catalog::Page<catalog::ArtistAlbumCandidate>, catalog::CatalogError>
        {
            assert_eq!(title, "album");
            assert_eq!(artist.kind, "artist");
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(gate) = &self.gate {
                gate.recv().unwrap();
            }
            self.pages.pop_front().unwrap().map(|page| catalog::Page {
                next_offset: page.next_offset,
                items: page
                    .items
                    .into_iter()
                    .map(|c| catalog::ArtistAlbumCandidate {
                        identity: c.identity,
                        title: c.title,
                        artist_ids: vec![artist.clone()],
                        date: c.date,
                        comment: c.comment,
                    })
                    .collect(),
            })
        }
        fn search_albums(
            &mut self,
            _: &str,
            _: u32,
        ) -> std::result::Result<catalog::Page<catalog::AlbumCandidate>, catalog::CatalogError>
        {
            panic!("use structured boundary")
        }
        fn releases(
            &mut self,
            _: &ExternalIdentity,
            _: u32,
        ) -> std::result::Result<catalog::Page<catalog::ReleaseCandidate>, catalog::CatalogError>
        {
            panic!("no edition requests")
        }
        fn release(
            &mut self,
            _: &ExternalIdentity,
        ) -> std::result::Result<catalog::Release, catalog::CatalogError> {
            panic!("no track requests")
        }
    }
    fn page(ids: &[&str]) -> catalog::Page<catalog::AlbumCandidate> {
        catalog::Page {
            next_offset: None,
            items: ids
                .iter()
                .enumerate()
                .map(|(n, id)| catalog::AlbumCandidate {
                    credits: vec![credit("Artist")],
                    identity: identity("release_group", id),
                    title: " ALBUM ".into(),
                    artist: " Artist ".into(),
                    date: "2000".into(),
                    primary_type: "Album".into(),
                    secondary_types: vec![],
                    comment: String::new(),
                    score: Some(100 - n as u32),
                })
                .collect(),
        }
    }
    fn worker(
        pages: Vec<
            std::result::Result<catalog::Page<catalog::AlbumCandidate>, catalog::CatalogError>,
        >,
        gate: Option<mpsc::Receiver<()>>,
    ) -> (AlbumMatcher, mpsc::Receiver<MatchReply>, Arc<AtomicUsize>) {
        let (send, recv) = mpsc::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let worker = AlbumMatcher::new(
            Provider {
                pages: pages.into(),
                calls: calls.clone(),
                gate,
            },
            move |r| send.send(r).unwrap(),
        )
        .unwrap();
        (worker, recv, calls)
    }
    #[test]
    fn committed_partial_import_is_usable_while_matching_is_pending_and_only_album_identity_changes()
     {
        for count in [1, 3, 15] {
            let mut f = Fixture::new();
            let request = f.files("Album", "Artist", &(1..=count).collect::<Vec<_>>());
            let imported = f.library.import_release(&request).unwrap();
            let album = f.library.album_for_release(&imported.release_id).unwrap();
            let (gate, wait) = mpsc::channel();
            let (mut matcher, results, calls) = worker(vec![Ok(page(&["group"]))], Some(wait));
            let updates = matcher
                .after_import(
                    &f.library,
                    std::slice::from_ref(&imported),
                    AutoMatchPolicy::default(),
                )
                .unwrap();
            assert_eq!(updates[0].1, MatchOutcome::Pending);
            assert_eq!(f.count("library_membership"), count as i64); // separate connection sees committed import
            for track in &imported.track_ids {
                assert!(
                    f.library
                        .available_playback_source(track)
                        .unwrap()
                        .is_some()
                );
            }
            assert!(results.try_recv().is_err());
            gate.send(()).unwrap();
            assert!(matches!(
                matcher.complete(
                    &mut f.library,
                    results.recv_timeout(Duration::from_secs(5)).unwrap()
                ),
                MatchOutcome::Matched(_)
            ));
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(
                f.library.album_for_release(&imported.release_id).unwrap(),
                album
            );
            assert_eq!(
                f.library
                    .list_album_external_identities(&album.album_id)
                    .unwrap(),
                vec![identity("release_group", "group")]
            );
            assert_eq!(f.count("release_external_identity"), 0);
            assert_eq!(f.count("track_external_identity"), 0);
            assert_eq!(
                matcher.match_album(&f.library, &album.album_id).unwrap(),
                MatchOutcome::AlreadyMatched
            );
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }
    #[test]
    fn errors_retry_ambiguity_truncation_and_scores_never_damage_local_music() {
        let mut f = Fixture::new();
        let request = f.files("Album", "Artist", &[1]);
        let imported = f.library.import_release(&request).unwrap();
        let id = f
            .library
            .album_for_release(&imported.release_id)
            .unwrap()
            .album_id;
        let mut truncated = page(&["group"]);
        truncated.next_offset = Some(10);
        let mut unrelated = page(&["wrong"]);
        unrelated.items[0].title = "Album (Live)".into();
        let (mut matcher, results, calls) = worker(
            vec![
                Err(catalog::CatalogError("HTTP 503".into())),
                Ok(page(&["first", "second"])),
                Ok(truncated),
                Ok(unrelated),
                Ok(page(&["group"])),
            ],
            None,
        );
        for expected in ["error", "no", "no", "no", "matched"] {
            assert_eq!(
                matcher.match_album(&f.library, &id).unwrap(),
                MatchOutcome::Pending
            );
            let result = matcher.complete(
                &mut f.library,
                results.recv_timeout(Duration::from_secs(5)).unwrap(),
            );
            match expected {
                "error" => assert!(matches!(result, MatchOutcome::Error(_))),
                "no" => assert!(matches!(
                    result,
                    MatchOutcome::NoConfidentMatch | MatchOutcome::AlbumAmbiguous(_)
                )),
                _ => assert!(matches!(result, MatchOutcome::Matched(_))),
            }
            assert_eq!(f.count("album"), 1);
            assert_eq!(f.count("track_source"), 1);
            assert_eq!(f.count("library_membership"), 1);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 5);
    }
    #[test]
    fn policy_deduplication_poor_metadata_and_existing_library_matching_avoid_requests() {
        let mut f = Fixture::new();
        let (mut matcher, results, calls) =
            worker(vec![Ok(page(&["group"])), Ok(page(&["group"]))], None);
        let request = f.files("Album", "Artist", &[1]);
        let imported = f.library.import_release(&request).unwrap();
        assert!(
            matcher
                .after_import(
                    &f.library,
                    std::slice::from_ref(&imported),
                    AutoMatchPolicy { enabled: false }
                )
                .unwrap()
                .is_empty()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let id = f
            .library
            .album_for_release(&imported.release_id)
            .unwrap()
            .album_id;
        matcher.match_album(&f.library, &id).unwrap(); // manual works when auto disabled
        matcher.complete(
            &mut f.library,
            results.recv_timeout(Duration::from_secs(5)).unwrap(),
        );
        let request = f.files("Album", "Artist", &[1, 2]);
        let reused = f.library.import_release(&request).unwrap();
        assert_eq!(
            f.library
                .album_for_release(&reused.release_id)
                .unwrap()
                .album_id,
            id
        );
        matcher
            .after_import(&f.library, &[imported, reused], AutoMatchPolicy::default())
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let request = f.files("Unknown Album", "Unknown Artist", &[1]);
        let poor = f.library.import_release(&request).unwrap();
        assert_eq!(
            matcher
                .after_import(&f.library, &[poor], AutoMatchPolicy::default())
                .unwrap()[0]
                .1,
            MatchOutcome::Skipped
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let request = f.files("Album", "Artist", &[20]);
        let second = f.library.import_release(&request).unwrap();
        let request = f.files("Album", "Artist", &[20]);
        let same = f.library.import_release(&request).unwrap();
        let outcomes = matcher
            .after_import(&f.library, &[second, same], AutoMatchPolicy::default())
            .unwrap();
        assert_eq!(outcomes.len(), 1);
        matcher.complete(
            &mut f.library,
            results.recv_timeout(Duration::from_secs(5)).unwrap(),
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn stale_metadata_and_concurrent_identity_are_rechecked_before_attachment() {
        let mut f = Fixture::new();
        let request = f.files("Album", "Artist", &[1]);
        let imported = f.library.import_release(&request).unwrap();
        let id = f
            .library
            .album_for_release(&imported.release_id)
            .unwrap()
            .album_id;
        let (mut matcher, results, _) = worker(vec![Ok(page(&["group"]))], None);
        matcher.match_album(&f.library, &id).unwrap();
        let reply = results.recv_timeout(Duration::from_secs(5)).unwrap();
        f.library
            .attach_album_external_identity(&id, &identity("release_group", "other"))
            .unwrap();
        assert_eq!(
            matcher.complete(&mut f.library, reply),
            MatchOutcome::AlreadyMatched
        );
        assert_eq!(
            f.library.list_album_external_identities(&id).unwrap(),
            vec![identity("release_group", "other")]
        );
    }
    #[test]
    fn absent_local_track_titles_skip_and_changed_metadata_rejects_a_late_reply() {
        let mut f = Fixture::new();
        let request = f.files("Album", "Artist", &[1]);
        let imported = f.library.import_release(&request).unwrap();
        let id = f
            .library
            .album_for_release(&imported.release_id)
            .unwrap()
            .album_id;
        let (mut matcher, results, calls) = worker(vec![Ok(page(&["group"]))], None);
        f.db()
            .execute("UPDATE file_metadata_observation SET track_title=NULL", [])
            .unwrap();
        assert_eq!(
            matcher.match_album(&f.library, &id).unwrap(),
            MatchOutcome::Skipped
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        f.db()
            .execute(
                "UPDATE file_metadata_observation SET track_title='Song 1'",
                [],
            )
            .unwrap();
        matcher.match_album(&f.library, &id).unwrap();
        let reply = results.recv_timeout(Duration::from_secs(5)).unwrap();
        // Simulate a future metadata edit while the request is in flight.
        f.db().execute("UPDATE album_application_metadata SET title='Changed',match_title='changed' WHERE album_id=?1",[id.as_ref()]).unwrap();
        assert_eq!(
            matcher.complete(&mut f.library, reply),
            MatchOutcome::Skipped
        );
        assert!(
            f.library
                .list_album_external_identities(&id)
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn multi_album_dispatch_coalesces_repeated_submissions() {
        let mut f = Fixture::new();
        let request = f.files("Album", "Artist", &[1]);
        let first = f.library.import_release(&request).unwrap();
        let request = f.files("Album", "Artist", &[20]);
        let second = f.library.import_release(&request).unwrap();
        let imports = [first, second];
        let (gate, wait) = mpsc::channel();
        let (mut matcher, results, calls) =
            worker(vec![Ok(page(&["one"])), Ok(page(&["two"]))], Some(wait));
        assert_eq!(
            matcher
                .after_import(&f.library, &imports, AutoMatchPolicy::default())
                .unwrap()
                .len(),
            2
        );
        matcher
            .after_import(&f.library, &imports, AutoMatchPolicy::default())
            .unwrap();
        for _ in 0..2 {
            gate.send(()).unwrap();
            let reply = results.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(matches!(
                matcher.complete(&mut f.library, reply),
                MatchOutcome::Matched(_)
            ));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}

mod artist_first {
    use super::*;
    use music_library::album_matching::{
        AlbumMatcher, AutoMatchPolicy, MatchOutcome, Preparation, accepted_album,
        close_album_title, resolve_artist,
    };
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex, mpsc},
        time::Duration,
    };
    fn artist(name: &str, id: &str) -> catalog::ArtistCandidate {
        catalog::ArtistCandidate {
            identity: identity("artist", id),
            name: name.into(),
            comment: "diagnostic".into(),
            country: String::new(),
            artist_type: "Group".into(),
            score: Some(100),
        }
    }
    fn artists(names: &[(&str, &str)]) -> catalog::Page<catalog::ArtistCandidate> {
        catalog::Page {
            items: names.iter().map(|(n, id)| artist(n, id)).collect(),
            next_offset: None,
        }
    }
    fn albums(titles: &[&str]) -> catalog::Page<catalog::ArtistAlbumCandidate> {
        catalog::Page {
            items: titles
                .iter()
                .enumerate()
                .map(|(n, title)| catalog::ArtistAlbumCandidate {
                    identity: identity("release_group", &format!("group-{n}")),
                    title: (*title).into(),
                    artist_ids: vec![identity("artist", "hella")],
                    comment: String::new(),
                    date: "2004".into(),
                })
                .collect(),
            next_offset: None,
        }
    }
    #[test]
    fn artist_names_are_exact_and_scores_do_not_resolve_duplicate_names() {
        assert!(resolve_artist(" Hella ", &artists(&[("HELLA", "hella")])).is_ok());
        assert_eq!(
            resolve_artist("Hella", &artists(&[("Hellä", "other"), ("Hell", "close")]))
                .unwrap_err(),
            MatchOutcome::NoConfidentMatch
        );
        let mut page = artists(&[("Hella", "one"), ("hella", "two")]);
        page.items[1].score = Some(1);
        assert!(
            matches!(resolve_artist("Hella",&page),Err(MatchOutcome::ArtistAmbiguous(c)) if c.len()==2)
        );
        let mut page = artists(&[("Hella", "one")]);
        page.next_offset = Some(10);
        assert!(matches!(
            resolve_artist("Hella", &page),
            Err(MatchOutcome::ArtistAmbiguous(_))
        ));
    }
    #[test]
    fn close_titles_require_artist_scope_exact_precedes_close_and_ambiguity_is_retained() {
        let id = identity("artist", "hella");
        assert!(matches!(
            accepted_album("Acoustic", &id, &albums(&["Acoustics"])),
            MatchOutcome::MatchedClose(_)
        ));
        assert!(matches!(
            accepted_album("Acoustic", &id, &albums(&["Acoustic", "Acoustics"])),
            MatchOutcome::Matched(_)
        ));
        assert!(
            matches!(accepted_album("Acoustic",&id,&albums(&["Acoustics","Acousti"])),MatchOutcome::AlbumAmbiguous(c) if c.len()==2)
        );
        assert_eq!(
            accepted_album(
                "Acoustic",
                &identity("artist", "another"),
                &albums(&["Acoustics"])
            ),
            MatchOutcome::NoConfidentMatch
        );
        assert_eq!(
            accepted_album("Acoustic", &identity("artist", ""), &albums(&["Acoustics"])),
            MatchOutcome::NoConfidentMatch
        );
        for (a, b) in [
            ("Art", "Arts"),
            ("Acoustic", "Electric"),
            ("Album", "Album (Live)"),
            ("Album Remix", "Album"),
            ("Album Deluxe", "Album"),
            ("Album Remaster", "Album"),
            ("Album Edit", "Album"),
        ] {
            assert!(!close_album_title(a, b), "{a} / {b}");
        }
        assert!(close_album_title("  Échoes ", "Échoe"));
    }
    struct Provider {
        artists: VecDeque<
            std::result::Result<catalog::Page<catalog::ArtistCandidate>, catalog::CatalogError>,
        >,
        albums: VecDeque<
            std::result::Result<
                catalog::Page<catalog::ArtistAlbumCandidate>,
                catalog::CatalogError,
            >,
        >,
        calls: Arc<Mutex<Vec<String>>>,
    }
    impl catalog::CatalogProvider for Provider {
        fn search_artists(
            &mut self,
            name: &str,
        ) -> std::result::Result<catalog::Page<catalog::ArtistCandidate>, catalog::CatalogError>
        {
            self.calls.lock().unwrap().push(format!("artist:{name}"));
            self.artists.pop_front().expect("unexpected Artist request")
        }
        fn artist_albums(
            &mut self,
            id: &ExternalIdentity,
            title: &str,
        ) -> std::result::Result<catalog::Page<catalog::ArtistAlbumCandidate>, catalog::CatalogError>
        {
            assert_eq!(id, &identity("artist", "hella"));
            self.calls.lock().unwrap().push(format!("album:{title}"));
            self.albums.pop_front().expect("unexpected Album request")
        }
        fn search_albums(
            &mut self,
            _: &str,
            _: u32,
        ) -> std::result::Result<catalog::Page<catalog::AlbumCandidate>, catalog::CatalogError>
        {
            panic!("no broad search")
        }
        fn releases(
            &mut self,
            _: &ExternalIdentity,
            _: u32,
        ) -> std::result::Result<catalog::Page<catalog::ReleaseCandidate>, catalog::CatalogError>
        {
            unreachable!()
        }
        fn release(
            &mut self,
            _: &ExternalIdentity,
        ) -> std::result::Result<catalog::Release, catalog::CatalogError> {
            unreachable!()
        }
    }
    fn input(f: &Fixture, imported: &ImportedRelease) -> music_library::album_matching::MatchInput {
        let id = f
            .library
            .album_for_release(&imported.release_id)
            .unwrap()
            .album_id;
        match f.library.prepare_album_match(&id).unwrap() {
            Preparation::Ready(input) => input,
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn matcher_canonicalizes_existing_identity_and_accepts_queued_reassigned_credit() {
        use music_library::album_matching::MatchReply;
        use music_library::domain::ArtistId;
        let mut f = Fixture::new();
        let request = f.files("Acoustic", "Hella", &[1]);
        let imported = f.library.import_release(&request).unwrap();
        let first = input(&f, &imported);
        let request = f.files("Control", "Hella", &[1]);
        let second = f.library.import_release(&request).unwrap();
        let second = input(&f, &second);
        // Existing strong identity wins even when its canonical name differs.
        f.db()
            .execute_batch("INSERT INTO artist(id,name) VALUES ('existing','Canonical alias')")
            .unwrap();
        let canonical = ArtistId("existing".into());
        let identity = ExternalIdentity {
            provider: "musicbrainz".into(),
            kind: "artist".into(),
            external_id: "hella".into(),
        };
        f.library
            .attach_artist_external_identity(&canonical, &identity)
            .unwrap();
        let reply = |input| MatchReply {
            input,
            artist: Some(identity.clone()),
            outcome: MatchOutcome::NoConfidentMatch,
        };
        assert_eq!(
            f.library
                .complete_album_match(reply(first.clone()))
                .unwrap(),
            MatchOutcome::NoConfidentMatch
        );
        assert_eq!(
            f.library
                .complete_album_match(reply(second.clone()))
                .unwrap(),
            MatchOutcome::NoConfidentMatch
        );
        // The old Artist IDs are gone, but a previously queued matching reply is
        // still valid because its current credit now carries the same strong ID.
        assert_eq!(
            f.library
                .complete_album_match(reply(first.clone()))
                .unwrap(),
            MatchOutcome::NoConfidentMatch
        );
        for old in [&first, &second] {
            let current = match f.library.prepare_album_match(&old.album_id).unwrap() {
                Preparation::Ready(i) => i,
                _ => panic!(),
            };
            assert_eq!(current.artist_id, canonical);
            assert_eq!(current.artist, "hella");
            assert_eq!(current.known_artist, Some(identity.clone()));
            assert!(
                f.library
                    .list_artist_external_identities(&old.artist_id)
                    .unwrap()
                    .is_empty()
            );
        }
        assert_eq!(
            f.library
                .resolve_artists_external_identity(&identity)
                .unwrap(),
            vec![canonical]
        );
        assert_eq!(f.count("release_external_identity"), 0);
        assert_eq!(f.count("track_external_identity"), 0);
    }
    #[test]
    fn artist_persists_after_album_error_fresh_worker_reuses_it_and_metadata_is_preserved() {
        let mut f = Fixture::new();
        let request = f.files("Acoustic", "Hella", &[1]);
        let imported = f.library.import_release(&request).unwrap();
        let input = input(&f, &imported);
        let calls = Arc::new(Mutex::new(Vec::new()));
        for (n, response) in [
            Err(catalog::CatalogError("HTTP 503 timeout".into())),
            Ok(albums(&["Acoustics"])),
        ]
        .into_iter()
        .enumerate()
        {
            let (send, recv) = mpsc::channel();
            let provider = Provider {
                artists: if n == 0 {
                    vec![Ok(artists(&[("Hella", "hella")]))].into()
                } else {
                    VecDeque::new()
                },
                albums: vec![response].into(),
                calls: calls.clone(),
            };
            let mut matcher = AlbumMatcher::new(provider, move |r| send.send(r).unwrap()).unwrap();
            matcher.match_album(&f.library, &input.album_id).unwrap();
            let outcome = matcher.complete(
                &mut f.library,
                recv.recv_timeout(Duration::from_secs(5)).unwrap(),
            );
            if n == 0 {
                assert!(matches!(outcome, MatchOutcome::Error(_)));
                assert_eq!(f.count("album_external_identity"), 0);
            } else {
                assert!(matches!(outcome, MatchOutcome::MatchedClose(_)));
            }
            assert_eq!(
                f.library
                    .list_artist_external_identities(&input.artist_id)
                    .unwrap(),
                vec![identity("artist", "hella")]
            );
        }
        assert_eq!(
            *calls.lock().unwrap(),
            vec!["artist:hella", "album:acoustic", "album:acoustic"]
        );
        assert_eq!(
            f.library
                .album_for_release(&imported.release_id)
                .unwrap()
                .title,
            "Acoustic"
        );
        assert_eq!(f.count("release_external_identity"), 0);
        assert_eq!(f.count("track_external_identity"), 0);
        assert_eq!(f.count("track_source"), 1);
    }
    #[test]
    fn ambiguous_artist_never_triggers_album_search_and_complex_credits_are_not_flattened() {
        let mut f = Fixture::new();
        let request = f.files("Acoustic", "Hella", &[1]);
        let imported = f.library.import_release(&request).unwrap();
        let input = input(&f, &imported);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (send, recv) = mpsc::channel();
        let provider = Provider {
            artists: vec![Ok(artists(&[("Hella", "one"), ("Hella", "two")]))].into(),
            albums: VecDeque::new(),
            calls: calls.clone(),
        };
        let mut matcher = AlbumMatcher::new(provider, move |r| send.send(r).unwrap()).unwrap();
        matcher.match_album(&f.library, &input.album_id).unwrap();
        assert!(matches!(
            matcher.complete(
                &mut f.library,
                recv.recv_timeout(Duration::from_secs(5)).unwrap()
            ),
            MatchOutcome::ArtistAmbiguous(_)
        ));
        assert_eq!(f.count("artist_external_identity"), 0);
        assert_eq!(calls.lock().unwrap().len(), 1);
        f.db()
            .execute_batch("INSERT INTO artist(id,name) VALUES ('other','Someone');")
            .unwrap();
        f.db().execute("INSERT INTO album_artist_credit(album_id,position,artist_id) VALUES (?1,1,'other')",[input.album_id.as_ref()]).unwrap();
        assert!(matches!(
            matcher.match_album(&f.library, &input.album_id).unwrap(),
            MatchOutcome::ArtistAmbiguous(_)
        ));
        assert_eq!(calls.lock().unwrap().len(), 1);
    }
    #[test]
    fn queued_album_failure_does_not_stall_next_and_exact_artist_resolution_is_cached() {
        let mut f = Fixture::new();
        let request = f.files("Acoustic", "Hella", &[1]);
        let first = f.library.import_release(&request).unwrap();
        let request = f.files("Control", "Hella", &[1]);
        let second = f.library.import_release(&request).unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (send, recv) = mpsc::channel();
        let provider = Provider {
            artists: vec![Ok(artists(&[("Hella", "hella")]))].into(),
            albums: vec![
                Err(catalog::CatalogError("HTTP 503".into())),
                Ok(albums(&["Control"])),
            ]
            .into(),
            calls: calls.clone(),
        };
        let mut matcher = AlbumMatcher::new(provider, move |r| send.send(r).unwrap()).unwrap();
        matcher
            .after_import(&f.library, &[first, second], AutoMatchPolicy::default())
            .unwrap();
        assert!(matches!(
            matcher.complete(
                &mut f.library,
                recv.recv_timeout(Duration::from_secs(5)).unwrap()
            ),
            MatchOutcome::Error(_)
        ));
        assert!(matches!(
            matcher.complete(
                &mut f.library,
                recv.recv_timeout(Duration::from_secs(5)).unwrap()
            ),
            MatchOutcome::Matched(_)
        ));
        assert_eq!(
            *calls.lock().unwrap(),
            vec!["artist:hella", "album:acoustic", "album:control"]
        );
        assert_eq!(f.count("artist_external_identity"), 1);
        assert_eq!(
            f.db()
                .query_row(
                    "SELECT count(DISTINCT artist_id) FROM album_artist_credit",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert_eq!(f.count("album_external_identity"), 1);
    }
    #[test]
    fn manual_artist_choice_retries_scoped_album_without_name_guessing() {
        let mut f = Fixture::new();
        let request = f.files("Acoustic", "Hella", &[1]);
        let imported = f.library.import_release(&request).unwrap();
        let input = input(&f, &imported);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (send, recv) = mpsc::channel();
        let provider = Provider {
            artists: vec![Ok(artists(&[("Hella", "hella"), ("Hella", "other")]))].into(),
            albums: vec![Ok(albums(&["Acoustics"]))].into(),
            calls: calls.clone(),
        };
        let mut matcher = AlbumMatcher::new(provider, move |r| send.send(r).unwrap()).unwrap();
        matcher.match_album(&f.library, &input.album_id).unwrap();
        assert!(matches!(
            matcher.complete(
                &mut f.library,
                recv.recv_timeout(Duration::from_secs(5)).unwrap()
            ),
            MatchOutcome::ArtistAmbiguous(_)
        ));
        assert_eq!(
            matcher
                .select_artist(&mut f.library, &input.album_id, 0)
                .unwrap(),
            MatchOutcome::Pending
        );
        assert_eq!(
            f.library
                .list_artist_external_identities(&input.artist_id)
                .unwrap(),
            vec![identity("artist", "hella")]
        );
        assert!(matches!(
            matcher.complete(
                &mut f.library,
                recv.recv_timeout(Duration::from_secs(5)).unwrap()
            ),
            MatchOutcome::MatchedClose(_)
        ));
        assert_eq!(
            *calls.lock().unwrap(),
            vec!["artist:hella", "album:acoustic"]
        );
    }
}
