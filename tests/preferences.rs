use music_library::{
    Library,
    browse::{Pane, Request},
    domain::*,
    library_search::Kind,
    playback::{EngineError, Playback, PlaybackEngine, Volume},
    preferences::IgnoreTarget,
    track_container::Target,
};
use rusqlite::Connection;
struct Fixture {
    _tmp: tempfile::TempDir,
    path: std::path::PathBuf,
    l: Library,
    tracks: Vec<TrackId>,
    album: AlbumId,
    artist: ArtistId,
    featured: ArtistId,
}
impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("library.sqlite");
        let mut l = Library::open(&path).unwrap();
        let r = l
            .create_catalog_release(&CatalogReleaseInput {
                title: "Test Album".into(),
                year: None,
                artists: vec![ArtistCreditInput {
                    name: "Main Artist".into(),
                    role: None,
                }],
                tracks: (1..=3)
                    .map(|i| CatalogTrackInput {
                        title: format!("Song {i}"),
                        disc_number: Some(1),
                        track_number: Some(i),
                        artists: if i == 1 {
                            vec![ArtistCreditInput {
                                name: "Featured Person".into(),
                                role: None,
                            }]
                        } else {
                            vec![]
                        },
                    })
                    .collect(),
            })
            .unwrap();
        for t in &r.track_ids {
            l.add_to_library(t).unwrap();
        }
        let album = l.album_for_release(&r.release_id).unwrap().album_id;
        let db = Connection::open(&path).unwrap();
        let artist = ArtistId(
            db.query_row(
                "SELECT artist_id FROM album_artist_credit WHERE album_id=?1",
                [album.as_ref()],
                |r| r.get(0),
            )
            .unwrap(),
        );
        let featured = ArtistId(
            db.query_row(
                "SELECT artist_id FROM track_artist_credit WHERE track_id=?1",
                [r.track_ids[0].as_ref()],
                |r| r.get(0),
            )
            .unwrap(),
        );
        Self {
            _tmp: tmp,
            path,
            l,
            tracks: r.track_ids,
            album,
            artist,
            featured,
        }
    }
    fn ignored(&self, target: IgnoreTarget) -> bool {
        let (n, i) = self.l.ignore_counts(&target).unwrap();
        n > 0 && n == i
    }
}
#[test]
fn hidden_secondary_artist_is_only_a_passive_browse_preference() {
    let mut f = Fixture::new();
    let req = Request {
        pane: Pane::Artists,
        omit_hidden_artists: true,
        limit: 200,
        ..Default::default()
    };
    assert!(
        f.l.browse(&req)
            .unwrap()
            .iter()
            .any(|r| r.id == f.featured.0)
    );
    let before = f.l.library_queue(&Request::default()).unwrap();
    let credits =
        f.l.browse(&Request {
            pane: Pane::Albums,
            limit: 200,
            ..Default::default()
        })
        .unwrap()[0]
            .subtitle
            .clone();
    f.l.set_artist_hidden(&f.featured, true).unwrap();
    assert!(
        !f.l.browse(&req)
            .unwrap()
            .iter()
            .any(|r| r.id == f.featured.0)
    );
    assert_eq!(f.l.library_queue(&Request::default()).unwrap(), before);
    assert_eq!(
        f.l.browse(&Request {
            pane: Pane::Albums,
            limit: 200,
            ..Default::default()
        })
        .unwrap()[0]
            .subtitle,
        credits
    );
    assert!(
        f.l.local_search("Featured Person", Kind::All)
            .unwrap()
            .iter()
            .any(|r| r.kind == Kind::Artist && r.id == f.featured.0)
    );
    assert!(
        f.l.browse_around(
            &Request {
                pane: Pane::Artists,
                limit: 20,
                ..Default::default()
            },
            &f.featured.0
        )
        .unwrap()
        .iter()
        .any(|r| r.id == f.featured.0)
    );
    assert!(
        f.l.is_track_eligible_for_generated_playback(&f.tracks[0])
            .unwrap()
    );
    let manager =
        f.l.browse(&Request {
            pane: Pane::Artists,
            hidden_artists_only: true,
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(manager.len(), 1);
    assert_eq!(manager[0].id, f.featured.0);
    drop(f.l);
    f.l = Library::open(&f.path).unwrap();
    assert!(f.l.artist_hidden(&f.featured).unwrap());
    f.l.set_artist_hidden(&f.featured, false).unwrap();
    assert!(
        f.l.browse(&req)
            .unwrap()
            .iter()
            .any(|r| r.id == f.featured.0)
    );
}
#[test]
fn ignore_is_track_only_snapshot_and_managers_include_unsaved_tracks() {
    let mut f = Fixture::new();
    let album = IgnoreTarget::Album(f.album.0.clone());
    let artist = IgnoreTarget::Artist(f.artist.0.clone());
    f.l.set_tracks_ignored(&album, true).unwrap();
    assert!(f.ignored(album.clone()));
    assert!(f.ignored(artist.clone()));
    assert_eq!(
        f.l.browse(&Request {
            limit: 200,
            ..Default::default()
        })
        .unwrap()
        .len(),
        3
    );
    assert!(!f.l.local_search("Song 1", Kind::Song).unwrap().is_empty());
    f.l.set_tracks_ignored(&IgnoreTarget::Track(f.tracks[0].0.clone()), false)
        .unwrap();
    assert!(!f.ignored(album.clone()));
    assert!(!f.ignored(artist.clone()));
    assert!(
        f.l.is_track_eligible_for_generated_playback(&f.tracks[0])
            .unwrap()
    );
    f.l.set_tracks_ignored(&artist, true).unwrap();
    assert!(f.ignored(album.clone()));
    let db = Connection::open(&f.path).unwrap();
    let release: String = db
        .query_row(
            "SELECT release_id FROM track WHERE id=?1",
            [f.tracks[0].as_ref()],
            |r| r.get(0),
        )
        .unwrap();
    db.execute("INSERT INTO track(id,release_id,recording_id) SELECT 'later',?1,recording_id FROM track WHERE id=?2",rusqlite::params![release,f.tracks[0].as_ref()]).unwrap();
    assert!(!f.ignored(album.clone()));
    assert!(!f.ignored(artist.clone()));
    assert!(
        f.l.is_track_eligible_for_generated_playback(&TrackId("later".into()))
            .unwrap()
    );
    f.l.remove_from_library(&f.tracks[1]).unwrap();
    let manager =
        f.l.browse(&Request {
            ignored_tracks_only: true,
            limit: 200,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(manager.len(), 3);
    drop(f.l);
    f.l = Library::open(&f.path).unwrap();
    assert!(
        !f.l.is_track_eligible_for_generated_playback(&f.tracks[1])
            .unwrap()
    );
    f.l.set_tracks_ignored(&album, false).unwrap();
    assert_eq!(f.l.ignore_counts(&album).unwrap(), (4, 0));
    assert_eq!(
        f.l.ignore_counts(&IgnoreTarget::Artist("missing".into()))
            .unwrap(),
        (0, 0)
    );
    assert_eq!(
        f.l.ignored_in_targets(&[album, artist, IgnoreTarget::Artist("missing".into())])
            .unwrap(),
        vec![false, false, false]
    );
}
#[test]
fn generated_contexts_filter_but_explicit_selection_preserves_ignored_tracks() {
    let mut f = Fixture::new();
    let p = f.l.create_playlist("Playlist").unwrap();
    for t in &f.tracks {
        f.l.append_playlist_track(&p, t).unwrap();
    }
    f.l.set_tracks_ignored(&IgnoreTarget::Track(f.tracks[1].0.clone()), true)
        .unwrap();
    let reader = f.l.library_queue_reader().unwrap();
    for target in [
        Target::Albums(vec![f.album.0.clone()]),
        Target::Artists(vec![f.artist.0.clone()]),
        Target::Playlists(vec![p]),
        Target::Songs(f.tracks.iter().map(|t| t.0.clone()).collect()),
    ] {
        let all = reader.resolve(&target, &Request::default()).unwrap();
        let generated = reader.filter_generated_tracks(all.clone(), &[]).unwrap();
        assert_eq!(generated.len(), 2);
        assert!(!generated.iter().any(|r| r.track_id == f.tracks[1]));
        assert_eq!(
            reader
                .filter_generated_tracks(all.clone(), &[f.tracks[1].0.clone()])
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            reader
                .filter_generated_tracks(
                    all,
                    &f.tracks.iter().map(|t| t.0.clone()).collect::<Vec<_>>()
                )
                .unwrap()
                .len(),
            3
        );
    }
    assert_eq!(
        f.l.playlist_entries(&reader_playlists(&f.l), None, 200)
            .unwrap()
            .len(),
        3
    );
}
fn reader_playlists(l: &Library) -> String {
    l.playlists(None, 1).unwrap()[0].id.clone()
}
struct Engine;
impl PlaybackEngine for Engine {
    fn set_volume(&mut self, _: Volume) -> Result<(), EngineError> {
        Ok(())
    }
    fn start(&mut self, _: &PlayableSource) -> Result<(), EngineError> {
        Ok(())
    }
    fn pause(&mut self) -> Result<(), EngineError> {
        Ok(())
    }
    fn resume(&mut self) -> Result<(), EngineError> {
        Ok(())
    }
    fn stop(&mut self) -> Result<(), EngineError> {
        Ok(())
    }
}
#[test]
fn stale_generated_entries_skip_in_both_directions_but_explicit_entries_remain() {
    let mut f = Fixture::new();
    let mut p = Playback::new(Engine);
    p.set_queue(f.tracks.clone()).unwrap();
    p.set_entry_intents(&[]);
    f.l.set_tracks_ignored(&IgnoreTarget::Track(f.tracks[1].0.clone()), true)
        .unwrap();
    assert_eq!(p.adjacent_eligible_position(&f.l, false).unwrap(), Some(2));
    p.select_queue_position(2).unwrap();
    assert_eq!(p.adjacent_eligible_position(&f.l, true).unwrap(), Some(0));
    p.set_entry_intents(&[f.tracks[1].0.clone()]);
    assert_eq!(p.adjacent_eligible_position(&f.l, true).unwrap(), Some(1));
    p.select_queue_position(0).unwrap();
    assert_eq!(p.adjacent_eligible_position(&f.l, false).unwrap(), Some(1));
    let state = p.state().clone();
    f.l.set_tracks_ignored(&IgnoreTarget::Track(f.tracks[0].0.clone()), true)
        .unwrap();
    assert_eq!(p.state(), &state);
}

#[test]
fn manager_keysets_are_bounded_and_scrolling_windows_derive_in_batches() {
    let mut f = Fixture::new();
    let db = Connection::open(&f.path).unwrap();
    db.execute_batch("BEGIN; WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<220) INSERT INTO artist(id,name) SELECT printf('hidden-%03d',x),printf('Hidden %03d',x) FROM n; INSERT INTO hidden_artist_preference(profile_id,artist_id) SELECT 'local',id FROM artist WHERE id LIKE 'hidden-%'; COMMIT;").unwrap();
    let request = Request {
        pane: Pane::Artists,
        hidden_artists_only: true,
        limit: 201,
        ..Default::default()
    };
    let first = f.l.browse(&request).unwrap();
    assert_eq!(first.len(), 201);
    let last =
        f.l.browse(&Request {
            after: Some(first.last().unwrap().cursor.clone()),
            ..request
        })
        .unwrap();
    assert_eq!(last.len(), 19);
    assert!(
        !last
            .iter()
            .any(|r| first.iter().any(|first| first.id == r.id))
    );
    f.l.set_tracks_ignored(&IgnoreTarget::Track(f.tracks[0].0.clone()), true)
        .unwrap();
    let targets = (0..600)
        .map(|i| IgnoreTarget::Track(f.tracks[i % 3].0.clone()))
        .collect::<Vec<_>>();
    let ignored = f.l.ignored_in_targets(&targets).unwrap();
    assert_eq!(ignored.len(), 600);
    for (i, value) in ignored.iter().enumerate() {
        assert_eq!(*value, i % 3 == 0);
    }
    let names = db
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name LIKE '%ignore%'")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(names, vec!["ignored_track_preference"]);
}

#[test]
fn explicitly_chosen_playlist_occurrence_does_not_permit_other_ignored_duplicates() {
    let mut f = Fixture::new();
    let p = f.l.create_playlist("Duplicates").unwrap();
    for t in [&f.tracks[0], &f.tracks[1], &f.tracks[1], &f.tracks[2]] {
        f.l.append_playlist_track(&p, t).unwrap();
    }
    f.l.set_tracks_ignored(&IgnoreTarget::Track(f.tracks[1].0.clone()), true)
        .unwrap();
    let reader = f.l.library_queue_reader().unwrap();
    let (rows, _) = reader.read_playlist(&p, None).unwrap();
    let (rows, position) = reader
        .prepare_playback_program(rows, 1, &[f.tracks[1].0.clone()], true)
        .unwrap();
    assert_eq!(position, 1);
    assert_eq!(
        rows.iter().map(|r| r.track_id.clone()).collect::<Vec<_>>(),
        f.tracks
    );
}
