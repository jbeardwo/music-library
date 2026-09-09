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
