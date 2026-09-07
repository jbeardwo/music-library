#[cfg(feature = "gstreamer")]
mod local;
mod sample;
mod session;

use qmetaobject::prelude::*;
use qmetaobject::{QVariantList, QVariantMap};
use session::Session;

#[derive(QObject)]
struct Bridge {
    base: qt_base_class!(trait QObject),
    snapshot: qt_property!(QVariantMap; READ snapshot_value NOTIFY changed),
    changed: qt_signal!(),
    set_volume: qt_method!(
        fn set_volume(&mut self, value: f64) {
            self.session.set_volume(value);
            self.changed();
        }
    ),
    search: qt_method!(
        fn search(&mut self, text: String) {
            self.session.search(text);
            self.changed();
        }
    ),
    page_next: qt_method!(
        fn page_next(&mut self) {
            self.session.page_next();
            self.changed();
        }
    ),
    page_previous: qt_method!(
        fn page_previous(&mut self) {
            self.session.page_previous();
            self.changed();
        }
    ),
    queue_page: qt_method!(
        fn queue_page(&mut self) {
            self.session.queue_page();
            self.changed();
        }
    ),
    enqueue_row: qt_method!(
        fn enqueue_row(&mut self, id: String) {
            self.session.enqueue_row(&id);
            self.changed();
        }
    ),
    clear_queue: qt_method!(
        fn clear_queue(&mut self) {
            self.session.clear_queue();
            self.changed();
        }
    ),
    play_row: qt_method!(
        fn play_row(&mut self, id: String) {
            self.session.play_row(&id);
            self.changed();
        }
    ),
    command: qt_method!(
        fn command(&mut self, command: String) {
            self.session.command(&command);
            self.changed();
        }
    ),
    toggle_failure: qt_method!(
        fn toggle_failure(&mut self) {
            self.session.toggle_failure();
            self.changed();
        }
    ),
    session: Session,
    real_audio: bool,
}

fn string(value: impl AsRef<str>) -> QVariant {
    QString::from(value.as_ref()).into()
}

fn row_value(row: &music_library::domain::TrackSearchResult) -> QVariant {
    let map: QVariantMap = [
        ("trackId", string(row.track_id.as_ref())),
        ("title", string(&row.title)),
        ("artist", string(&row.artist_names)),
        ("release", string(&row.release_title)),
        ("available", row.available.into()),
    ]
    .into_iter()
    .collect();
    map.into()
}

impl Bridge {
    fn new(session: Session) -> Self {
        Self {
            base: Default::default(),
            snapshot: Default::default(),
            changed: Default::default(),
            set_volume: Default::default(),
            search: Default::default(),
            page_next: Default::default(),
            page_previous: Default::default(),
            queue_page: Default::default(),
            play_row: Default::default(),
            enqueue_row: Default::default(),
            clear_queue: Default::default(),
            command: Default::default(),
            toggle_failure: Default::default(),
            session,
            real_audio: false,
        }
    }

    fn snapshot_value(&self) -> QVariantMap {
        let s = &self.session;
        let state = s.playback.state();
        let rows: QVariantList = s.rows.iter().map(row_value).collect();
        let queue: QVariantList = state
            .queue
            .iter()
            .enumerate()
            .map(|(i, id)| {
                // IDs/position come from PlaybackState, labels are presentation-only snapshots.
                let label = s.queue_labels.get(i).filter(|row| &row.track_id == id);
                let row: QVariantMap = [
                    ("title", string(label.map_or(id.as_ref(), |row| &row.title))),
                    ("trackId", string(id.as_ref())),
                    ("current", (state.position == Some(i)).into()),
                ]
                .into_iter()
                .collect();
                QVariant::from(row)
            })
            .collect();
        let current = state.position.and_then(|i| s.queue_labels.get(i));
        [
            ("rows", rows.into()),
            ("queue", queue.into()),
            (
                "currentTitle",
                string(current.map_or("—", |row| &row.title)),
            ),
            (
                "currentId",
                string(state.current_track().map_or("", |id| id.as_ref())),
            ),
            ("position", (state.position.map_or(-1, |i| i as i32)).into()),
            ("status", string(format!("{:?}", state.status))),
            (
                "pending",
                string(
                    state
                        .pending
                        .map_or(String::new(), |s| format!(" → {s:?} pending")),
                ),
            ),
            ("volume", state.volume.get().into()),
            ("realAudio", self.real_audio.into()),
            (
                "time",
                string(format!(
                    "{} / {}",
                    time_label(Some(state.media_position_ms)),
                    time_label(state.duration_ms)
                )),
            ),
            ("canNext", state.can_next().into()),
            ("canPrevious", state.can_previous().into()),
            (
                "source",
                string(state.source.as_ref().map_or("—", |s| s.source_id.as_ref())),
            ),
            ("error", string(&s.error)),
            ("outcome", string(s.outcome())),
            ("revision", s.revision.into()),
            ("searchText", string(&s.text)),
            ("page", (s.page as u32 + 1).into()),
            ("hasNext", s.has_next.into()),
            ("failureArmed", s.diagnostics.borrow().fail_next.into()),
            (
                "engineCalls",
                string(
                    s.diagnostics
                        .borrow()
                        .calls
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
            ),
        ]
        .into_iter()
        .collect()
    }
}

fn time_label(ms: Option<u64>) -> String {
    ms.map_or_else(
        || "--:--".into(),
        |ms| format!("{:02}:{:02}", ms / 60_000, (ms / 1000) % 60),
    )
}

// Construct the C++ QObject before capturing a QPointer: otherwise it stays null
// and silently discards every engine notification, even while GstPlay is Playing.
// https://docs.rs/qmetaobject/0.2.10/qmetaobject/struct.QPointer.html
#[cfg(feature = "gstreamer")]
fn engine_callback(
    bridge: qmetaobject::QObjectPinned<'_, Bridge>,
) -> impl Fn(music_library::playback::EngineEvent) + Send + Sync + 'static {
    bridge.get_or_create_cpp_object();
    let weak = qmetaobject::QPointer::from(bridge.borrow());
    qmetaobject::queued_callback(move |event| {
        if let Some(bridge) = weak.as_pinned() {
            let mut bridge = bridge.borrow_mut();
            if bridge.session.engine_event(event) {
                bridge.changed();
            }
        }
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let (smoke, folder) = match args.as_slice() {
        [] => (false, None),
        [arg] if arg == "--smoke-test" => (true, None),
        [arg, folder] if arg == "--gstreamer" => (false, Some(std::path::PathBuf::from(folder))),
        [arg, folder, check] if arg == "--gstreamer" && check == "--smoke-test" => {
            (true, Some(std::path::PathBuf::from(folder)))
        }
        _ => {
            return Err(
                "usage: qml-diagnostic [--smoke-test | --gstreamer FOLDER [--smoke-test]]".into(),
            );
        }
    };
    #[cfg(not(feature = "gstreamer"))]
    if folder.is_some() {
        return Err("rebuild with --features gstreamer to use real audio".into());
    }
    #[cfg(feature = "gstreamer")]
    let (_temp, library) = if let Some(folder) = &folder {
        local::load(folder)?
    } else {
        sample::create()?
    };
    #[cfg(not(feature = "gstreamer"))]
    let (_temp, library) = sample::create()?;
    // Pin before exposing to QML, and keep the QObject alive until QML destruction.
    let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
    let mut engine = QmlEngine::new();
    #[cfg(feature = "gstreamer")]
    if folder.is_some() {
        let deliver = engine_callback(bridge.pinned());
        let audio = music_library_gstreamer::GStreamerEngine::new(deliver)?;
        let pinned = bridge.pinned();
        let mut bridge = pinned.borrow_mut();
        bridge.real_audio = true;
        bridge.session.playback =
            music_library::playback::Playback::new(session::Engine::GStreamer(audio));
    }
    engine.set_object_property("diagnostic".into(), bridge.pinned());
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        engine.load_data(include_str!("../Main.qml").into());
        if !engine.invoke_method("ready".into(), &[]).to_bool() {
            return Err(
                "QML did not create the diagnostic window; check Qt module errors above".into(),
            );
        }
        if smoke && folder.is_some() {
            // Real-mode startup/teardown check, deliberately without audible playback.
            println!("QML real-engine startup smoke test passed (no playback)");
        } else if smoke {
            let result = engine
                .invoke_method("smokeTest".into(), &[])
                .to_qstring()
                .to_string();
            if result != "ok" {
                return Err(format!("QML smoke test failed: {result}").into());
            }
            println!("QML integration smoke test passed");
        } else {
            engine.exec();
        }
        Ok(())
    })();
    // Join the audio worker while Qt and its callback target still exist, even on QML load failure.
    #[cfg(feature = "gstreamer")]
    bridge.pinned().borrow_mut().session.shutdown_audio();
    result
}

#[cfg(all(test, feature = "gstreamer"))]
mod event_delivery_tests {
    use super::*;
    use music_library::playback::{EngineError, EngineEvent, EngineEventKind as Event};
    use std::{cell::RefCell, rc::Rc};

    fn report(engine: &Rc<RefCell<QmlEngine>>) -> String {
        engine
            .borrow_mut()
            .invoke_method("testView".into(), &[])
            .to_qstring()
            .to_string()
    }

    fn command(engine: &Rc<RefCell<QmlEngine>>, name: &str) {
        engine
            .borrow_mut()
            .invoke_method("testCommand".into(), &[string(name)]);
    }

    fn confirm(
        engine: &Rc<RefCell<QmlEngine>>,
        deliver: impl Fn(EngineEvent) + Send + 'static,
        event: EngineEvent,
    ) {
        // Quit is queued after delivery, without a command, clock update, or polling.
        let quit = engine.clone();
        let done = qmetaobject::queued_callback(move |()| quit.borrow().quit());
        let worker = std::thread::spawn(move || {
            deliver(event);
            done(());
        });
        engine.borrow().exec();
        worker.join().unwrap();
    }

    #[test]
    fn async_confirmations_refresh_actual_qml_labels_without_another_command() {
        use music_library::playback::PlaybackStatus::{Paused, Playing, Stopped};
        let engine = Rc::new(RefCell::new(QmlEngine::new()));
        let (_temp, library) = sample::create().unwrap();
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        // Exercise callback construction before QML exposure, too.
        let first_delivery = engine_callback(bridge.pinned());
        {
            let pinned = bridge.pinned();
            let mut bridge = pinned.borrow_mut();
            bridge.session.defer_confirmations();
            bridge.session.search("Available".into());
            bridge.session.queue_page();
        }
        engine
            .borrow_mut()
            .set_object_property("diagnostic".into(), bridge.pinned());
        // Load the real window/bindings; only add test accessors and label IDs.
        let qml = include_str!("../Main.qml")
            .replace(
                "    function ready() {",
                r#"
    function testView() {
        return view.status + "|" + view.pending + "|" + view.outcome
            + "|" + playbackLabel.text + "|" + outcomeLabel.text;
    }
    function testCommand(name) { window.bridge.command(name); }
    function ready() {"#,
            )
            .replace(
                r#"text: "Current: ""#,
                r#"id: playbackLabel
            text: "Current: ""#,
            )
            .replace(
                r#"text: "State update ""#,
                r#"id: outcomeLabel
            text: "State update ""#,
            );
        engine.borrow_mut().load_data(qml.into());
        command(&engine, "play");
        assert!(report(&engine).contains("Playing pending"));
        confirm(
            &engine,
            first_delivery,
            EngineEvent {
                generation: 1,
                kind: Event::State(Playing),
            },
        );
        assert!(report(&engine).starts_with("Playing||"));
        command(&engine, "pause");
        let pending = report(&engine);
        assert!(pending.starts_with("Playing| → Paused pending|pause → Playing → Paused pending|"));
        confirm(
            &engine,
            engine_callback(bridge.pinned()),
            EngineEvent {
                generation: 1,
                kind: Event::State(Paused),
            },
        );
        let paused = report(&engine);
        assert!(paused.starts_with("Paused||"));
        assert!(paused.contains(" · Paused · "));
        assert!(paused.contains("pause → Paused"));
        assert!(!paused.contains("pending"));
        command(&engine, "play");
        assert!(report(&engine).starts_with("Paused| → Playing pending|"));
        confirm(
            &engine,
            engine_callback(bridge.pinned()),
            EngineEvent {
                generation: 1,
                kind: Event::State(Playing),
            },
        );
        assert!(report(&engine).starts_with("Playing||play → Playing|"));
        command(&engine, "stop");
        assert!(report(&engine).contains("Stopped pending"));
        confirm(
            &engine,
            engine_callback(bridge.pinned()),
            EngineEvent {
                generation: 2,
                kind: Event::State(Stopped),
            },
        );
        assert!(report(&engine).starts_with("Stopped||stop → Stopped|"));
        command(&engine, "play");
        confirm(
            &engine,
            engine_callback(bridge.pinned()),
            EngineEvent {
                generation: 3,
                kind: Event::Error(EngineError("test output failure".into())),
            },
        );
        assert!(report(&engine).starts_with("Failed||engine event → Failed|"));
        let failed = report(&engine);
        confirm(
            &engine,
            engine_callback(bridge.pinned()),
            EngineEvent {
                generation: 1,
                kind: Event::State(Playing),
            },
        );
        assert_eq!(report(&engine), failed);
        drop(engine); // Destroy QML bindings while their QObject still exists.
    }
}
