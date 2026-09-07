//! Local-audio adapter. GstPlay and its bus live only on an owned worker thread.
use std::{
    path::Path,
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use gst::prelude::*;
use gstreamer as gst;
use gstreamer_play as gst_play;
use music_library::{
    domain::{PlayableSource, SourceLocation},
    playback::{EngineError, EngineEvent, EngineEventKind, PlaybackEngine, PlaybackStatus, Volume},
};

enum Command {
    Volume(Volume),
    Start(u64, PlayableSource),
    Pause(u64),
    Resume(u64),
    Stop(u64),
    Shutdown,
}

pub struct GStreamerEngine {
    generation: u64,
    commands: Sender<Command>,
    worker: Option<JoinHandle<()>>,
}

impl GStreamerEngine {
    /// The callback runs on the worker. Marshal it to the application's owning thread.
    pub fn new(
        on_event: impl Fn(EngineEvent) + Send + Sync + 'static,
    ) -> Result<Self, EngineError> {
        Self::spawn(Arc::new(on_event), false)
    }

    fn spawn(
        on_event: Arc<dyn Fn(EngineEvent) + Send + Sync>,
        silent_test_sink: bool,
    ) -> Result<Self, EngineError> {
        let (commands, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("local-audio".into())
            .spawn(move || run(receiver, on_event, silent_test_sink))
            .map_err(|e| EngineError(e.to_string()))?;
        Ok(Self {
            generation: 0,
            commands,
            worker: Some(worker),
        })
    }

    fn send(&self, command: Command) -> Result<(), EngineError> {
        self.commands
            .send(command)
            .map_err(|_| EngineError("GStreamer worker has exited".into()))
    }
}

impl PlaybackEngine for GStreamerEngine {
    fn set_volume(&mut self, volume: Volume) -> Result<(), EngineError> {
        self.send(Command::Volume(volume))
    }
    fn asynchronous(&self) -> bool {
        true
    }
    fn set_event_generation(&mut self, generation: u64) {
        self.generation = generation;
    }
    fn start(&mut self, source: &PlayableSource) -> Result<(), EngineError> {
        self.send(Command::Start(self.generation, source.clone()))
    }
    fn pause(&mut self) -> Result<(), EngineError> {
        self.send(Command::Pause(self.generation))
    }
    fn resume(&mut self) -> Result<(), EngineError> {
        self.send(Command::Resume(self.generation))
    }
    fn stop(&mut self) -> Result<(), EngineError> {
        self.send(Command::Stop(self.generation))
    }
}

impl Drop for GStreamerEngine {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            // Only shutdown joins; interactive commands never wait for media/device operations.
            if worker.join().is_err() {
                eprintln!("GStreamer worker panicked during shutdown");
            }
        }
    }
}

fn file_uri(path: &Path) -> Result<gst::glib::GString, EngineError> {
    let absolute = std::path::absolute(path).map_err(|e| EngineError(e.to_string()))?;
    gst::glib::filename_to_uri(absolute, None).map_err(|e| EngineError(e.to_string()))
}

struct Input {
    generation: u64,
    player: gst_play::Play,
    bus: gst::Bus,
    output_warning: Option<String>,
}

impl Input {
    fn new(
        generation: u64,
        source: &PlayableSource,
        silent_test_sink: bool,
        volume: Volume,
    ) -> Result<Self, EngineError> {
        gst::init().map_err(|e| EngineError(e.to_string()))?;
        let uri = match &source.location {
            SourceLocation::LocalFile(path) => file_uri(path)?,
            _ => {
                return Err(EngineError(
                    "GStreamer adapter supports local files only".into(),
                ));
            }
        };
        let player = gst_play::Play::new(None::<gst_play::PlayVideoRenderer>);
        let input = Self {
            generation,
            bus: player.message_bus(),
            player,
            output_warning: None,
        };
        input.player.set_video_track_enabled(false);
        input.player.set_subtitle_track_enabled(false);
        let mut config = input.player.config();
        config.set_position_update_interval(200);
        input
            .player
            .set_config(config)
            .map_err(|e| EngineError(e.to_string()))?;
        if silent_test_sink {
            let sink = gst::ElementFactory::make("fakesink")
                .property("sync", true)
                .build()
                .map_err(|e| EngineError(e.to_string()))?;
            input.player.pipeline().set_property("audio-sink", &sink);
        }
        input.player.set_volume(volume.get());
        input.player.set_uri(Some(uri.as_str()));
        input.player.play();
        Ok(input)
    }

    fn drain(&mut self, emit: &dyn Fn(EngineEvent)) {
        // Bound each drain so a noisy bus cannot starve commands or shutdown.
        for _ in 0..128 {
            let Some(message) = self.bus.pop() else {
                break;
            };
            let Ok(message) = gst_play::PlayMessage::parse(&message) else {
                continue;
            };
            let kind = match message {
                gst_play::PlayMessage::StateChanged(state) => match state.state() {
                    gst_play::PlayState::Playing => {
                        // autoaudiosink can succeed with a silent fallback after every
                        // real device fails. Check only after sink selection completes.
                        let sink = self
                            .player
                            .pipeline()
                            .property::<Option<gst::Element>>("audio-sink");
                        if sink.as_ref().is_some_and(silent_auto_sink) {
                            self.player.stop();
                            emit(EngineEvent {
                                generation: self.generation,
                                kind: EngineEventKind::Error(EngineError(format!(
                                    "No usable audio output: GStreamer selected a silent fallback. {}",
                                    self.output_warning
                                        .as_deref()
                                        .unwrap_or("Check the system audio service.")
                                ))),
                            });
                            return;
                        }
                        Some(EngineEventKind::State(PlaybackStatus::Playing))
                    }
                    gst_play::PlayState::Paused => {
                        Some(EngineEventKind::State(PlaybackStatus::Paused))
                    }
                    // GstPlay can post Stopped before EOS/error. Forward it; orchestration
                    // only acknowledges its requested target and handles terminal events separately.
                    gst_play::PlayState::Stopped => {
                        Some(EngineEventKind::State(PlaybackStatus::Stopped))
                    }
                    _ => None,
                },
                gst_play::PlayMessage::Warning(warning) => {
                    self.output_warning =
                        Some(format!("{} ({:?})", warning.error(), warning.details()));
                    None
                }
                gst_play::PlayMessage::EndOfStream(_) => Some(EngineEventKind::EndOfStream),
                gst_play::PlayMessage::Error(error) => Some(EngineEventKind::Error(EngineError(
                    format!("{} ({:?})", error.error(), error.details()),
                ))),
                gst_play::PlayMessage::DurationChanged(value) => Some(EngineEventKind::Duration(
                    value.duration().map(|t| t.mseconds()),
                )),
                gst_play::PlayMessage::PositionUpdated(value) => value
                    .position()
                    .map(|t| EngineEventKind::Position(t.mseconds())),
                _ => None,
            };
            if let Some(kind) = kind {
                emit(EngineEvent {
                    generation: self.generation,
                    kind,
                });
            }
        }
    }
}

// GstAutoDetect deliberately substitutes fakesink when no real sink works.
// https://github.com/GStreamer/gstreamer/blob/1.28.2/subprojects/gst-plugins-good/gst/autodetect/gstautodetect.c
// An explicit fakesink used by hardware-independent tests is not this fallback.
fn silent_auto_sink(sink: &gst::Element) -> bool {
    sink.factory()
        .is_some_and(|factory| factory.name() == "autoaudiosink")
        && sink.downcast_ref::<gst::Bin>().is_some_and(|bin| {
            bin.children().iter().any(|child| {
                child
                    .factory()
                    .is_some_and(|factory| factory.name() == "fakesink")
            })
        })
}

impl Drop for Input {
    fn drop(&mut self) {
        self.player.stop();
        // GstPlay bus messages own references to the player. Flush before the last
        // player reference is released, as required by GstPlay's bus cleanup contract.
        // https://gstreamer.freedesktop.org/documentation/play/gstplay.html
        self.bus.set_flushing(true);
    }
}

fn run(
    commands: Receiver<Command>,
    emit: Arc<dyn Fn(EngineEvent) + Send + Sync>,
    silent_test_sink: bool,
) {
    let mut input: Option<Input> = None;
    let mut volume = Volume::default();
    loop {
        match commands.recv_timeout(Duration::from_millis(20)) {
            Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(Command::Volume(value)) => {
                volume = value;
                if let Some(active) = &input {
                    active.player.set_volume(volume.get());
                }
            }
            Ok(Command::Start(generation, source)) => {
                drop(input.take());
                match Input::new(generation, &source, silent_test_sink, volume) {
                    Ok(new) => input = Some(new),
                    Err(error) => emit(EngineEvent {
                        generation,
                        kind: EngineEventKind::Error(error),
                    }),
                }
            }
            Ok(Command::Stop(generation)) => {
                drop(input.take()); // Destruction finishes on this worker before confirmation.
                emit(EngineEvent {
                    generation,
                    kind: EngineEventKind::State(PlaybackStatus::Stopped),
                });
            }
            Ok(command @ (Command::Pause(_) | Command::Resume(_))) => {
                let generation = match command {
                    Command::Pause(g) | Command::Resume(g) => g,
                    _ => unreachable!(),
                };
                if let Some(active) = input
                    .as_ref()
                    .filter(|input| input.generation == generation)
                {
                    if matches!(command, Command::Pause(_)) {
                        active.player.pause();
                    } else {
                        active.player.play();
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if let Some(input) = &mut input {
            input.drain(emit.as_ref());
        }
    }
    drop(input); // No detached bus watches, GLib main loops, or application worker threads.
}

#[cfg(test)]
mod tests {
    use super::*;
    use music_library::domain::SourceId;
    use std::{cell::RefCell, time::Instant};

    #[test]
    fn automatic_silent_fallback_is_distinct_from_explicit_test_output() {
        gst::init().unwrap();
        // AutoAudioSink initially contains its fallback child, without opening hardware.
        let automatic = gst::ElementFactory::make("autoaudiosink").build().unwrap();
        assert!(silent_auto_sink(&automatic));
        let explicit = gst::ElementFactory::make("fakesink").build().unwrap();
        assert!(!silent_auto_sink(&explicit));
    }

    fn wav(path: &Path, seconds: u32) {
        let size = 8000 * 2 * seconds;
        let mut bytes = Vec::new();
        bytes.extend(b"RIFF");
        bytes.extend((36 + size).to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16u32.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(8000u32.to_le_bytes());
        bytes.extend(16000u32.to_le_bytes());
        bytes.extend(2u16.to_le_bytes());
        bytes.extend(16u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend(size.to_le_bytes());
        bytes.resize(44 + size as usize, 0);
        std::fs::write(path, bytes).unwrap();
    }
    fn source(path: &Path) -> PlayableSource {
        PlayableSource {
            source_id: SourceId("audio-test".into()),
            location: SourceLocation::LocalFile(path.to_owned()),
        }
    }
    fn wait(input: &mut Input, mut predicate: impl FnMut(&EngineEventKind) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let events = RefCell::new(Vec::new());
            input.drain(&|event| events.borrow_mut().push(event.kind));
            for event in events.into_inner() {
                if let EngineEventKind::Error(error) = &event {
                    panic!("{error}");
                }
                if predicate(&event) {
                    return;
                }
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for GstPlay event"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn native_file_uris_round_trip_reserved_unicode_and_mounted_paths() {
        for path in [
            "/tmp/music space #100% ? é.wav",
            "/mnt/c/Music/space #100% ? é.wav",
        ] {
            let uri = file_uri(Path::new(path)).unwrap();
            assert!(uri.starts_with("file:///"));
            assert!(
                uri.contains("%20")
                    && uri.contains("%23")
                    && uri.contains("%25")
                    && uri.contains("%3F")
            );
            assert_eq!(
                gst::glib::filename_from_uri(&uri).unwrap().0,
                Path::new(path)
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let path = std::path::PathBuf::from(std::ffi::OsString::from_vec(
                b"/tmp/non-utf8-\xff.wav".to_vec(),
            ));
            assert_eq!(
                gst::glib::filename_from_uri(&file_uri(&path).unwrap())
                    .unwrap()
                    .0,
                path
            );
        }
    }

    #[test]
    fn actual_gstplay_pause_retains_position_resume_continues_restart_resets_and_disposes() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("space #100% é.wav");
        wav(&path, 4);
        let mut input = Input::new(7, &source(&path), true, Volume::new(0.25).unwrap()).unwrap();
        wait(
            &mut input,
            |event| matches!(event, EngineEventKind::Position(ms) if *ms >= 400),
        );
        assert_eq!(input.player.volume(), 0.25);
        assert_eq!(input.player.duration().unwrap().mseconds(), 4000);
        input.player.pause();
        wait(&mut input, |event| {
            matches!(event, EngineEventKind::State(PlaybackStatus::Paused))
        });
        let paused = input.player.position().unwrap().mseconds();
        input.player.set_volume(0.5);
        assert_eq!(input.player.volume(), 0.5);
        assert_eq!(input.generation, 7);
        thread::sleep(Duration::from_millis(300));
        let still = input.player.position().unwrap().mseconds();
        assert!(
            paused.abs_diff(still) <= 20,
            "pause moved from {paused} to {still}"
        );
        input.player.play();
        wait(
            &mut input,
            |event| matches!(event, EngineEventKind::Position(ms) if *ms > paused + 200),
        );
        input.player.stop();
        wait(&mut input, |event| {
            matches!(event, EngineEventKind::State(PlaybackStatus::Stopped))
        });
        // GstPlay 1.28 retains a cached position after Stopped. Do not mistake
        // that property for an active clock; verify that a subsequent Play starts at zero.
        input.player.play();
        wait(&mut input, |event| {
            matches!(event, EngineEventKind::State(PlaybackStatus::Playing))
        });
        assert_eq!(input.player.volume(), 0.5);
        let restarted = input.player.position().unwrap().mseconds();
        assert!(restarted < 400, "restart position {restarted}");
        let weak = input.player.downgrade();
        drop(input);
        assert!(
            weak.upgrade().is_none(),
            "GstPlay retained after bus flush/drop"
        );
    }

    #[test]
    fn worker_reports_duration_eos_errors_and_stop_without_audio_hardware() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("short.wav");
        wav(&path, 1);
        let (tx, rx) = mpsc::channel();
        let mut engine = GStreamerEngine::spawn(
            Arc::new(move |e| {
                let _ = tx.send(e);
            }),
            true,
        )
        .unwrap();
        engine.set_event_generation(1);
        engine.start(&source(&path)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut duration = None;
        loop {
            let event = rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap();
            assert_eq!(event.generation, 1);
            match event.kind {
                EngineEventKind::Duration(ms) => duration = ms,
                EngineEventKind::EndOfStream => break,
                EngineEventKind::Error(error) => panic!("{error}"),
                _ => {}
            }
        }
        assert_eq!(duration, Some(1000));
        engine.set_event_generation(2);
        engine
            .start(&source(&temp.path().join("missing.wav")))
            .unwrap();
        loop {
            let event = rx.recv_timeout(Duration::from_secs(10)).unwrap();
            if event.generation == 2 && matches!(event.kind, EngineEventKind::Error(_)) {
                break;
            }
        }
        engine.set_event_generation(3);
        engine.stop().unwrap();
        loop {
            let event = rx.recv_timeout(Duration::from_secs(10)).unwrap();
            if event.generation == 3 {
                assert_eq!(event.kind, EngineEventKind::State(PlaybackStatus::Stopped));
                break;
            }
        }
        drop(engine); // Joins the worker; no device needed.
    }

    #[test]
    fn real_eos_drives_application_queue_to_stopped_without_consuming_entries() {
        use music_library::{
            Library,
            domain::{ImportReleaseRequest, ImportTrackInput},
            filesystem::LoftyMetadataExtractor,
            playback::Playback,
        };
        let temp = tempfile::TempDir::new().unwrap();
        wav(&temp.path().join("first.wav"), 1);
        wav(&temp.path().join("second.wav"), 1);
        let mut library = Library::open_in_memory().unwrap();
        let root = library.register_local_root(temp.path()).unwrap();
        library
            .scan_local_root(&root, &mut LoftyMetadataExtractor)
            .unwrap();
        let mut queue = Vec::new();
        for source in library.list_discovery_candidates(None, 10).unwrap() {
            let release = library
                .import_release(&ImportReleaseRequest {
                    release_title: "Audio test".into(),
                    release_artists: vec![],
                    tracks: vec![ImportTrackInput {
                        source_id: source.source_id,
                        title_fallback: Some("Test".into()),
                        artists: vec![],
                        disc_number: None,
                        track_number: None,
                    }],
                })
                .unwrap();
            queue.extend(release.track_ids);
        }
        assert_eq!(queue.len(), 2);
        let (tx, rx) = mpsc::channel();
        let engine = GStreamerEngine::spawn(
            Arc::new(move |e| {
                let _ = tx.send(e);
            }),
            true,
        )
        .unwrap();
        let mut playback = Playback::new(engine);
        playback.set_queue(queue.clone()).unwrap();
        playback.play(&library).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut ended = 0;
        while ended < 2 || playback.state().pending.is_some() {
            let event = rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap();
            if matches!(event.kind, EngineEventKind::EndOfStream) {
                ended += 1;
            }
            playback.handle_event(&library, event).unwrap();
        }
        assert_eq!(playback.state().queue, queue);
        assert_eq!(playback.state().position, Some(1));
        assert_eq!(playback.state().status, PlaybackStatus::Stopped);
        assert_eq!(playback.state().media_position_ms, 0);
        assert!(playback.state().source.is_none());
    }

    #[test]
    #[ignore = "optional read-only mounted-file probe; set MUSIC_LIBRARY_TEST_AUDIO"]
    fn supplied_file_decodes_to_eos_without_hardware() {
        let path =
            std::env::var_os("MUSIC_LIBRARY_TEST_AUDIO").expect("set MUSIC_LIBRARY_TEST_AUDIO");
        let mut input = Input::new(1, &source(Path::new(&path)), true, Volume::default()).unwrap();
        wait(&mut input, |event| {
            matches!(event, EngineEventKind::EndOfStream)
        });
    }
}
