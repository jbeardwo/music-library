use std::{cell::RefCell, rc::Rc};

use music_library::{
    Library,
    domain::{
        CatalogReleaseInput, CatalogTrackInput, PlayableSource, SourceId, SourceLocation, TrackId,
    },
    playback::{EngineError, Playback, PlaybackEngine, PlaybackError, PlaybackStatus, Volume},
};
use rusqlite::{Connection, params};
use tempfile::TempDir;

#[derive(Clone, Debug, PartialEq)]
enum Call {
    Volume(Volume),
    Start(PlayableSource),
    Pause,
    Resume,
    Stop,
}

#[derive(Default)]
struct EngineState {
    calls: Vec<Call>,
    fail_next: bool,
}

struct FakeEngine(Rc<RefCell<EngineState>>);
impl FakeEngine {
    fn call(&self, call: Call) -> Result<(), EngineError> {
        let mut state = self.0.borrow_mut();
        state.calls.push(call);
        if std::mem::take(&mut state.fail_next) {
            Err(EngineError("test engine failure".into()))
        } else {
            Ok(())
        }
    }
}
impl PlaybackEngine for FakeEngine {
    fn set_volume(&mut self, volume: Volume) -> Result<(), EngineError> {
        self.call(Call::Volume(volume))
    }
    fn start(&mut self, source: &PlayableSource) -> Result<(), EngineError> {
        self.call(Call::Start(source.clone()))
    }
    fn pause(&mut self) -> Result<(), EngineError> {
        self.call(Call::Pause)
    }
    fn resume(&mut self) -> Result<(), EngineError> {
        self.call(Call::Resume)
    }
    fn stop(&mut self) -> Result<(), EngineError> {
        self.call(Call::Stop)
    }
}

struct Fixture {
    library: Library,
    db: Connection,
    tracks: Vec<TrackId>,
    _temp: TempDir,
}
impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("playback.sqlite");
        let mut library = Library::open(&path).unwrap();
        let release = library
            .create_catalog_release(&CatalogReleaseInput {
                title: "Edition".into(),
                year: None,
                artists: vec![],
                tracks: (0..3)
                    .map(|i| CatalogTrackInput {
                        title: format!("Track {i}"),
                        artists: vec![],
                        disc_number: None,
                        track_number: None,
                    })
                    .collect(),
            })
            .unwrap();
        for track in &release.track_ids[..2] {
            library.add_to_library(track).unwrap();
        }
        library
            .set_track_title_override(&release.track_ids[0], "My title")
            .unwrap();
        let db = Connection::open(path).unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON; INSERT INTO discovery_root(id,kind,location) VALUES ('root','local_filesystem',X'2F');").unwrap();
        Self {
            library,
            db,
            tracks: release.track_ids,
            _temp: temp,
        }
    }
    fn source(&self, id: &str, track: usize, available: bool) -> PlayableSource {
        let path = self._temp.path().join(id);
        #[cfg(unix)]
        let bytes = {
            use std::os::unix::ffi::OsStrExt;
            path.as_os_str().as_bytes().to_vec()
        };
        #[cfg(windows)]
        let bytes: Vec<u8> = {
            use std::os::windows::ffi::OsStrExt;
            path.as_os_str()
                .encode_wide()
                .flat_map(u16::to_le_bytes)
                .collect()
        };
        self.db
            .execute(
                "INSERT INTO playable_source(id,kind) VALUES (?1,'local_file')",
                [id],
            )
            .unwrap();
        self.db
            .execute(
                "INSERT INTO track_source(track_id,source_id) VALUES (?1,?2)",
                params![self.tracks[track].as_ref(), id],
            )
            .unwrap();
        self.db.execute("INSERT INTO local_file_observation(source_id,root_id,path,size_bytes,modified_ns,available) VALUES (?1,'root',?2,1,1,?3)", params![id,bytes,available]).unwrap();
        PlayableSource {
            source_id: SourceId(id.into()),
            location: SourceLocation::LocalFile(path),
        }
    }
    fn durable_state(&self) -> String {
        // Include every durable table, so failed commands cannot silently alter user data or associations.
        let mut tables = self
            .db
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        let names = tables
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let mut snapshot = String::new();
        for name in names {
            let mut statement = self
                .db
                .prepare(&format!(
                    "SELECT * FROM \"{}\" ORDER BY 1",
                    name.replace('"', "\"\"")
                ))
                .unwrap();
            let columns = statement.column_count();
            let mut rows = statement.query([]).unwrap();
            while let Some(row) = rows.next().unwrap() {
                for column in 0..columns {
                    snapshot.push_str(&format!("{:?};", row.get_ref(column).unwrap()));
                }
            }
        }
        snapshot
    }
}
fn controller() -> (Playback<FakeEngine>, Rc<RefCell<EngineState>>) {
    let state = Rc::new(RefCell::new(EngineState::default()));
    (Playback::new(FakeEngine(state.clone())), state)
}

#[test]
fn plays_resolved_source_even_without_membership() {
    let f = Fixture::new();
    let source = f.source("one", 2, true);
    let before = f.durable_state();
    let (mut p, engine) = controller();
    p.set_queue(vec![f.tracks[2].clone()]).unwrap();
    p.play(&f.library).unwrap();
    assert_eq!(p.state().status, PlaybackStatus::Playing);
    assert_eq!(p.state().source.as_ref(), Some(&source));
    assert_eq!(p.state().current_track(), Some(&f.tracks[2]));
    assert_eq!(engine.borrow().calls, vec![Call::Start(source)]);
    assert_eq!(f.durable_state(), before);
}

#[test]
fn missing_and_unavailable_sources_preserve_library_and_queue() {
    let f = Fixture::new();
    f.source("missing", 1, false);
    f.source("unrelated", 2, true);
    let before = f.durable_state();
    for index in [0, 1] {
        let (mut p, engine) = controller();
        p.set_queue(vec![f.tracks[index].clone()]).unwrap();
        let state = p.state().clone();
        assert!(
            matches!(p.play(&f.library), Err(PlaybackError::NoAvailableSource(id)) if id == f.tracks[index])
        );
        assert_eq!(p.state(), &state);
        assert!(engine.borrow().calls.is_empty());
        assert_eq!(f.durable_state(), before);
    }
}

#[test]
fn selection_is_deterministic_and_rechecks_availability_after_stop() {
    let f = Fixture::new();
    let z = f.source("z", 0, true);
    let a = f.source("a", 0, true);
    f.source("0", 0, false);
    let (mut p, _) = controller();
    p.set_queue(vec![f.tracks[0].clone()]).unwrap();
    p.play(&f.library).unwrap();
    assert_eq!(p.state().source.as_ref(), Some(&a));
    p.stop().unwrap();
    f.db.execute(
        "UPDATE local_file_observation SET available=0 WHERE source_id='a'",
        [],
    )
    .unwrap();
    p.play(&f.library).unwrap();
    assert_eq!(p.state().source.as_ref(), Some(&z));
    p.stop().unwrap();
    f.db.execute("UPDATE local_file_observation SET available=0", [])
        .unwrap();
    let before = f.durable_state();
    assert!(matches!(
        p.play(&f.library),
        Err(PlaybackError::NoAvailableSource(_))
    ));
    assert_eq!(f.durable_state(), before);
}

#[test]
fn pause_resume_stop_are_idempotent_and_restart_resolves_again() {
    let f = Fixture::new();
    let source = f.source("a", 0, true);
    let (mut p, engine) = controller();
    p.set_queue(vec![f.tracks[0].clone()]).unwrap();
    p.pause().unwrap();
    p.play(&f.library).unwrap();
    p.play(&f.library).unwrap();
    p.pause().unwrap();
    p.pause().unwrap();
    assert_eq!(p.state().status, PlaybackStatus::Paused);
    p.play(&f.library).unwrap();
    assert_eq!(p.state().status, PlaybackStatus::Playing);
    p.stop().unwrap();
    p.stop().unwrap();
    assert_eq!(p.state().status, PlaybackStatus::Stopped);
    assert_eq!(p.state().position, Some(0));
    assert!(p.state().source.is_none());
    p.play(&f.library).unwrap();
    assert_eq!(
        engine.borrow().calls,
        vec![
            Call::Start(source.clone()),
            Call::Pause,
            Call::Resume,
            Call::Stop,
            Call::Start(source)
        ]
    );
}

#[test]
fn navigation_and_end_of_queue_do_not_wrap_or_skip_unavailable_tracks() {
    let f = Fixture::new();
    f.source("a", 0, true);
    f.source("b", 1, true);
    let (mut p, engine) = controller();
    p.set_queue(f.tracks.clone()).unwrap();
    p.play(&f.library).unwrap();
    assert!(!p.previous(&f.library).unwrap());
    p.pause().unwrap();
    assert!(p.next(&f.library).unwrap());
    assert_eq!(p.state().position, Some(1));
    assert_eq!(p.state().status, PlaybackStatus::Playing);
    assert!(matches!(
        p.next(&f.library),
        Err(PlaybackError::NoAvailableSource(_))
    ));
    assert_eq!(p.state().position, Some(2));
    assert_eq!(p.state().status, PlaybackStatus::Stopped);
    assert!(p.state().source.is_none());
    assert_eq!(engine.borrow().calls.last(), Some(&Call::Stop));
    assert_eq!(p.state().queue, f.tracks);
    assert!(p.previous(&f.library).unwrap());
    assert_eq!(p.state().position, Some(1));
    p.set_queue(vec![f.tracks[0].clone(), f.tracks[0].clone()])
        .unwrap();
    assert!(p.next(&f.library).unwrap());
    assert!(!p.next(&f.library).unwrap());
    assert_eq!(p.state().position, Some(1));
    assert_eq!(p.state().status, PlaybackStatus::Stopped);
    p.play(&f.library).unwrap();
    assert_eq!(p.state().position, Some(1));
    p.set_queue(vec![]).unwrap();
    assert_eq!(p.state().position, None);
    assert!(matches!(p.play(&f.library), Err(PlaybackError::EmptyQueue)));
    assert!(matches!(p.next(&f.library), Err(PlaybackError::EmptyQueue)));
    assert!(matches!(
        p.previous(&f.library),
        Err(PlaybackError::EmptyQueue)
    ));
    p.pause().unwrap();
    p.stop().unwrap();
}

#[test]
fn engine_failures_preserve_queue_and_durable_state_and_require_recovery() {
    let f = Fixture::new();
    f.source("a", 0, true);
    f.source("b", 1, true);
    let before = f.durable_state();
    for command in ["start", "next", "pause", "resume", "stop", "replace"] {
        let (mut p, engine) = controller();
        p.set_queue(f.tracks[..2].to_vec()).unwrap();
        if command != "start" {
            p.play(&f.library).unwrap();
        }
        if command == "resume" {
            p.pause().unwrap();
        }
        engine.borrow_mut().fail_next = true;
        let result = match command {
            "start" | "resume" => p.play(&f.library),
            "next" => p.next(&f.library).map(|_| ()),
            "pause" => p.pause(),
            "stop" => p.stop(),
            "replace" => p.set_queue(vec![f.tracks[2].clone()]),
            _ => unreachable!(),
        };
        assert!(matches!(result, Err(PlaybackError::Engine(_))), "{command}");
        assert_eq!(p.state().queue, f.tracks[..2]);
        assert_eq!(p.state().position, Some(usize::from(command == "next")));
        assert_eq!(p.state().status, PlaybackStatus::Failed);
        assert!(p.state().source.is_none());
        assert!(matches!(p.pause(), Err(PlaybackError::Failed)));
        engine.borrow_mut().fail_next = true;
        assert!(matches!(p.play(&f.library), Err(PlaybackError::Engine(_))));
        assert_eq!(p.state().status, PlaybackStatus::Failed);
        p.play(&f.library).unwrap();
        assert_eq!(p.state().status, PlaybackStatus::Playing);
        let calls = &engine.borrow().calls;
        assert_eq!(calls[calls.len() - 2], Call::Stop);
        assert!(matches!(calls.last(), Some(Call::Start(_))));
        assert_eq!(f.durable_state(), before);
    }
}

#[test]
fn retained_queue_and_duplicates_are_navigated_by_position() {
    let f = Fixture::new();
    f.source("a", 0, true);
    f.source("b", 1, true);
    let before = f.durable_state();
    let (mut p, engine) = controller();
    assert!(!p.state().can_next());
    assert!(!p.state().can_previous());
    p.enqueue(f.tracks[0].clone());
    assert_eq!(p.state().position, Some(0));
    assert_eq!(p.state().status, PlaybackStatus::Stopped);
    assert!(engine.borrow().calls.is_empty());
    p.play(&f.library).unwrap();
    let playing = p.state().clone();
    p.enqueue(f.tracks[1].clone());
    p.enqueue(f.tracks[0].clone());
    assert_eq!(p.state().source, playing.source);
    assert_eq!(p.state().status, playing.status);
    assert_eq!(engine.borrow().calls.len(), 1);
    let expected = vec![
        f.tracks[0].clone(),
        f.tracks[1].clone(),
        f.tracks[0].clone(),
    ];
    assert!(p.state().can_next());
    assert!(!p.previous(&f.library).unwrap());
    for position in [1, 2] {
        assert!(p.next(&f.library).unwrap());
        assert_eq!(p.state().queue, expected);
        assert_eq!(p.state().position, Some(position));
        assert!(p.state().can_previous());
    }
    assert!(!p.state().can_next());
    assert!(!p.next(&f.library).unwrap());
    assert_eq!(p.state().position, Some(2));
    assert_eq!(p.state().status, PlaybackStatus::Stopped);
    for position in [1, 0] {
        assert!(p.previous(&f.library).unwrap());
        assert_eq!(p.state().position, Some(position));
        assert_eq!(p.state().queue, expected);
        assert_eq!(p.state().status, PlaybackStatus::Playing);
    }
    assert!(!p.previous(&f.library).unwrap());
    assert_eq!(f.durable_state(), before);
}

#[test]
fn unplayable_entries_are_selected_and_do_not_trap_either_direction() {
    for unavailable in [false, true] {
        let f = Fixture::new();
        f.source("a", 0, true);
        f.source("c", 2, true);
        if unavailable {
            f.source("b", 1, false);
        }
        let before = f.durable_state();
        let (mut p, _) = controller();
        p.set_queue(f.tracks.clone()).unwrap();
        p.play(&f.library).unwrap();
        assert!(
            matches!(p.next(&f.library), Err(PlaybackError::NoAvailableSource(id)) if id == f.tracks[1])
        );
        assert_eq!(p.state().position, Some(1));
        assert_eq!(p.state().status, PlaybackStatus::Stopped);
        assert!(p.state().source.is_none());
        assert!(p.state().can_next() && p.state().can_previous());
        assert!(p.next(&f.library).unwrap());
        assert_eq!(p.state().position, Some(2));
        assert_eq!(p.state().status, PlaybackStatus::Playing);
        assert!(matches!(
            p.previous(&f.library),
            Err(PlaybackError::NoAvailableSource(_))
        ));
        assert_eq!(p.state().position, Some(1));
        assert!(p.previous(&f.library).unwrap());
        assert_eq!(p.state().position, Some(0));
        // The first current entry can also be unplayable without trapping Next.
        p.set_queue(f.tracks[1..].to_vec()).unwrap();
        assert!(matches!(
            p.play(&f.library),
            Err(PlaybackError::NoAvailableSource(_))
        ));
        assert!(p.next(&f.library).unwrap());
        assert_eq!(p.state().current_track(), Some(&f.tracks[2]));
        assert_eq!(p.state().queue, f.tracks[1..]);
        assert_eq!(f.durable_state(), before);
    }
}

#[test]
fn failed_navigation_selects_attempted_entry_and_recovers_in_both_directions() {
    let f = Fixture::new();
    for i in 0..3 {
        f.source(&format!("source-{i}"), i, true);
    }
    let before = f.durable_state();
    let (mut p, engine) = controller();
    p.set_queue(f.tracks.clone()).unwrap();
    p.play(&f.library).unwrap();
    engine.borrow_mut().fail_next = true;
    assert!(matches!(p.next(&f.library), Err(PlaybackError::Engine(_))));
    assert_eq!(p.state().position, Some(1));
    assert_eq!(p.state().status, PlaybackStatus::Failed);
    assert!(p.state().can_next() && p.state().can_previous());
    assert!(p.next(&f.library).unwrap());
    assert_eq!(p.state().position, Some(2));
    assert_eq!(p.state().status, PlaybackStatus::Playing);
    engine.borrow_mut().fail_next = true;
    assert!(p.pause().is_err());
    assert!(p.previous(&f.library).unwrap());
    assert_eq!(p.state().position, Some(1));
    assert_eq!(p.state().status, PlaybackStatus::Playing);
    engine.borrow_mut().fail_next = true;
    assert!(p.pause().is_err());
    engine.borrow_mut().fail_next = true;
    // Even failed internal recovery selects the attempted entry, and does not start it.
    assert!(matches!(
        p.previous(&f.library),
        Err(PlaybackError::Engine(_))
    ));
    assert_eq!(engine.borrow().calls.last(), Some(&Call::Stop));
    assert_eq!(p.state().position, Some(0));
    assert_eq!(p.state().status, PlaybackStatus::Failed);
    assert!(p.next(&f.library).unwrap());
    assert_eq!(p.state().position, Some(1));
    assert_eq!(p.state().queue, f.tracks);
    assert_eq!(f.durable_state(), before);
}

#[test]
fn clear_stops_empties_and_preserves_durable_data_with_explicit_stop_failure() {
    let f = Fixture::new();
    f.source("a", 0, true);
    let before = f.durable_state();
    for status in [
        PlaybackStatus::Stopped,
        PlaybackStatus::Playing,
        PlaybackStatus::Paused,
        PlaybackStatus::Failed,
    ] {
        let (mut p, engine) = controller();
        p.enqueue(f.tracks[0].clone());
        if status != PlaybackStatus::Stopped {
            p.play(&f.library).unwrap();
        }
        if status == PlaybackStatus::Paused {
            p.pause().unwrap();
        }
        if status == PlaybackStatus::Failed {
            engine.borrow_mut().fail_next = true;
            assert!(p.pause().is_err());
        }
        if status != PlaybackStatus::Stopped {
            engine.borrow_mut().fail_next = true;
            assert!(matches!(p.clear_queue(), Err(PlaybackError::Engine(_))));
            assert_eq!(p.state().queue, vec![f.tracks[0].clone()]);
            assert_eq!(p.state().position, Some(0));
            assert_eq!(p.state().status, PlaybackStatus::Failed);
        }
        p.clear_queue().unwrap();
        assert!(p.state().queue.is_empty());
        assert_eq!(p.state().position, None);
        assert_eq!(p.state().current_track(), None);
        assert_eq!(p.state().source, None);
        assert_eq!(p.state().status, PlaybackStatus::Stopped);
        assert!(!p.state().can_next() && !p.state().can_previous());
        let calls = engine.borrow().calls.clone();
        p.clear_queue().unwrap();
        assert_eq!(engine.borrow().calls, calls);
        assert_eq!(f.durable_state(), before);
    }
}

struct AsyncEngine {
    fake: FakeEngine,
    generation: Rc<std::cell::Cell<u64>>,
}
impl PlaybackEngine for AsyncEngine {
    fn set_volume(&mut self, volume: Volume) -> Result<(), EngineError> {
        self.fake.set_volume(volume)
    }
    fn asynchronous(&self) -> bool {
        true
    }
    fn set_event_generation(&mut self, generation: u64) {
        self.generation.set(generation);
    }
    fn start(&mut self, source: &PlayableSource) -> Result<(), EngineError> {
        self.fake.start(source)
    }
    fn pause(&mut self) -> Result<(), EngineError> {
        self.fake.pause()
    }
    fn resume(&mut self) -> Result<(), EngineError> {
        self.fake.resume()
    }
    fn stop(&mut self) -> Result<(), EngineError> {
        self.fake.stop()
    }
}

#[test]
fn async_confirmation_pause_races_timing_and_stop_reject_stale_events() {
    use music_library::playback::{EngineEvent, EngineEventKind as Event};
    let f = Fixture::new();
    f.source("a", 0, true);
    let generation = Rc::new(std::cell::Cell::new(0));
    let mut p = Playback::new(AsyncEngine {
        fake: FakeEngine(Rc::default()),
        generation: generation.clone(),
    });
    p.enqueue(f.tracks[0].clone());
    p.play(&f.library).unwrap();
    let input = generation.get();
    let event = |kind| EngineEvent {
        generation: input,
        kind,
    };
    assert_eq!(p.state().status, PlaybackStatus::Stopped);
    assert_eq!(p.state().pending, Some(PlaybackStatus::Playing));
    // URI replacement/startup notifications must not terminate this input or
    // invalidate its generation before the Playing confirmation arrives.
    for status in [PlaybackStatus::Stopped, PlaybackStatus::Paused] {
        assert!(
            !p.handle_event(&f.library, event(Event::State(status)))
                .unwrap()
        );
        assert_eq!(generation.get(), input);
        assert_eq!(p.state().pending, Some(PlaybackStatus::Playing));
        assert!(p.state().source.is_some());
    }
    p.handle_event(&f.library, event(Event::State(PlaybackStatus::Playing)))
        .unwrap();
    assert_eq!(p.state().pending, None);
    p.handle_event(&f.library, event(Event::Duration(Some(10_000))))
        .unwrap();
    p.handle_event(&f.library, event(Event::Position(2400)))
        .unwrap();
    p.pause().unwrap();
    assert_eq!(p.state().status, PlaybackStatus::Playing);
    assert_eq!(p.state().pending, Some(PlaybackStatus::Paused));
    assert!(
        !p.handle_event(&f.library, event(Event::State(PlaybackStatus::Playing)))
            .unwrap()
    );
    p.handle_event(&f.library, event(Event::State(PlaybackStatus::Paused)))
        .unwrap();
    assert_eq!(p.state().media_position_ms, 2400);
    assert_eq!(p.state().duration_ms, Some(10_000));
    p.play(&f.library).unwrap();
    assert!(
        !p.handle_event(&f.library, event(Event::State(PlaybackStatus::Paused)))
            .unwrap()
    );
    p.handle_event(&f.library, event(Event::State(PlaybackStatus::Playing)))
        .unwrap();
    p.stop().unwrap();
    assert_eq!(p.state().media_position_ms, 0);
    assert_eq!(p.state().pending, Some(PlaybackStatus::Stopped));
    let stopped_generation = generation.get();
    for kind in [
        Event::EndOfStream,
        Event::Error(EngineError("late".into())),
        Event::Position(9900),
        Event::Duration(Some(1)),
        Event::State(PlaybackStatus::Playing),
    ] {
        assert!(!p.handle_event(&f.library, event(kind)).unwrap());
    }
    p.handle_event(
        &f.library,
        EngineEvent {
            generation: stopped_generation,
            kind: Event::State(PlaybackStatus::Stopped),
        },
    )
    .unwrap();
    assert_eq!(p.state().status, PlaybackStatus::Stopped);
    assert_eq!(p.state().pending, None);
    p.play(&f.library).unwrap();
    assert!(
        !p.handle_event(
            &f.library,
            EngineEvent {
                generation: stopped_generation,
                kind: Event::State(PlaybackStatus::Stopped)
            }
        )
        .unwrap()
    );
    let active = generation.get();
    assert!(matches!(
        p.handle_event(
            &f.library,
            EngineEvent {
                generation: active,
                kind: Event::Error(EngineError("decode".into()))
            }
        ),
        Err(PlaybackError::Engine(_))
    ));
    assert_eq!(p.state().status, PlaybackStatus::Failed);
    assert!(
        !p.handle_event(
            &f.library,
            EngineEvent {
                generation: active,
                kind: Event::State(PlaybackStatus::Playing)
            }
        )
        .unwrap()
    );
}

#[test]
fn eos_advances_once_retains_duplicates_and_surfaces_unplayable_next() {
    use music_library::playback::{EngineEvent, EngineEventKind as Event};
    let f = Fixture::new();
    f.source("a", 0, true);
    let before = f.durable_state();
    let generation = Rc::new(std::cell::Cell::new(0));
    let mut p = Playback::new(AsyncEngine {
        fake: FakeEngine(Rc::default()),
        generation: generation.clone(),
    });
    let queue = vec![
        f.tracks[0].clone(),
        f.tracks[0].clone(),
        f.tracks[1].clone(),
    ];
    p.set_queue(queue.clone()).unwrap();
    p.play(&f.library).unwrap();
    let first = generation.get();
    p.handle_event(
        &f.library,
        EngineEvent {
            generation: first,
            kind: Event::EndOfStream,
        },
    )
    .unwrap();
    assert_eq!(p.state().position, Some(1));
    for kind in [
        Event::EndOfStream,
        Event::Error(EngineError("old input".into())),
    ] {
        assert!(
            !p.handle_event(
                &f.library,
                EngineEvent {
                    generation: first,
                    kind
                }
            )
            .unwrap()
        );
    }
    assert_eq!(p.state().position, Some(1));
    let second = generation.get();
    assert!(matches!(
        p.handle_event(
            &f.library,
            EngineEvent {
                generation: second,
                kind: Event::EndOfStream
            }
        ),
        Err(PlaybackError::NoAvailableSource(_))
    ));
    assert_eq!(p.state().position, Some(2));
    assert_eq!(p.state().queue, queue);
    assert!(p.state().source.is_none());
    p.set_queue(vec![f.tracks[0].clone()]).unwrap();
    p.play(&f.library).unwrap();
    let last = generation.get();
    p.handle_event(
        &f.library,
        EngineEvent {
            generation: last,
            kind: Event::EndOfStream,
        },
    )
    .unwrap();
    assert_eq!(p.state().position, Some(0));
    assert_eq!(p.state().queue.len(), 1);
    assert_eq!(p.state().pending, Some(PlaybackStatus::Stopped));
    p.handle_event(
        &f.library,
        EngineEvent {
            generation: generation.get(),
            kind: Event::State(PlaybackStatus::Stopped),
        },
    )
    .unwrap();
    assert_eq!(p.state().status, PlaybackStatus::Stopped);
    assert_eq!(p.state().media_position_ms, 0);
    p.clear_queue().unwrap();
    assert!(
        !p.handle_event(
            &f.library,
            EngineEvent {
                generation: last,
                kind: Event::EndOfStream
            }
        )
        .unwrap()
    );
    assert!(p.state().queue.is_empty());
    assert_eq!(f.durable_state(), before);
}

#[test]
fn volume_is_bounded_ephemeral_and_does_not_change_transport_or_generation() {
    use music_library::playback::{EngineEvent, EngineEventKind};
    let f = Fixture::new();
    f.source("a", 0, true);
    let durable = f.durable_state();
    let engine = Rc::new(RefCell::new(EngineState::default()));
    let generation = Rc::new(std::cell::Cell::new(0));
    let mut p = Playback::new(AsyncEngine {
        fake: FakeEngine(engine.clone()),
        generation: generation.clone(),
    });
    p.enqueue(f.tracks[0].clone());
    assert_eq!(p.state().volume.get(), 1.0);
    for (action, value) in [
        ("stopped", 0.0),
        ("play", 0.25),
        ("pause", 0.5),
        ("stop", 1.0),
    ] {
        match action {
            "play" => p.play(&f.library).unwrap(),
            "pause" => p.pause().unwrap(),
            "stop" => p.stop().unwrap(),
            _ => {}
        }
        let before = p.state().clone();
        let input_generation = generation.get();
        p.set_volume(value).unwrap();
        let mut expected = before;
        expected.volume = Volume::new(value).unwrap();
        assert_eq!(p.state(), &expected);
        assert_eq!(generation.get(), input_generation);
        assert_eq!(
            engine.borrow().calls.last(),
            Some(&Call::Volume(expected.volume))
        );
        if let Some(target) = p.state().pending {
            assert!(
                p.handle_event(
                    &f.library,
                    EngineEvent {
                        generation: input_generation,
                        kind: EngineEventKind::State(target),
                    }
                )
                .unwrap()
            );
        }
    }
    let before = p.state().clone();
    let count = engine.borrow().calls.len();
    for invalid in [-0.01, 1.01, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(
            p.set_volume(invalid),
            Err(PlaybackError::InvalidVolume)
        ));
        assert_eq!(p.state(), &before);
    }
    assert_eq!(engine.borrow().calls.len(), count);
    engine.borrow_mut().fail_next = true;
    assert!(matches!(p.set_volume(0.3), Err(PlaybackError::Engine(_))));
    assert_eq!(p.state(), &before);
    p.set_volume(0.4).unwrap();
    p.clear_queue().unwrap();
    assert_eq!(p.state().volume.get(), 0.4);
    assert_eq!(f.durable_state(), durable);
}
