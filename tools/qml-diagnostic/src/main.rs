mod catalog;
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
    catalog_snapshot: qt_property!(QVariantMap; READ catalog_snapshot_value NOTIFY catalog_changed),
    catalog_changed: qt_signal!(),
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
    catalog_action: qt_method!(
        fn catalog_action(&mut self, action: String, value: String) {
            if self.catalog.pending {
                return;
            }
            let result = self.start_catalog(&action, &value);
            if let Err(error) = result {
                self.catalog.status = error;
                self.catalog.pending = false;
            }
            self.catalog_changed();
        }
    ),
    catalog: catalog::State,
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
            catalog_snapshot: Default::default(),
            catalog_changed: Default::default(),
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
            catalog_action: Default::default(),
            catalog: Default::default(),
            session,
            real_audio: false,
        }
    }

    fn start_catalog(&mut self, action: &str, value: &str) -> Result<(), String> {
        self.catalog.timing = Some(music_library::catalog::Timing::new(format!(
            "qml.{action}.button_to_completion"
        )));
        let request = self.catalog.request(action, value)?;
        if self.catalog.worker.is_none() {
            // Invoked through a live QML QObject, so the C++ target already exists.
            let weak = qmetaobject::QPointer::from(&*self);
            let deliver = catalog_callback(weak);
            self.catalog.worker = Some(catalog::Worker::new(
                music_library_musicbrainz::MusicBrainz::new(),
                deliver,
            )?);
        }
        self.catalog.worker.as_ref().unwrap().send(request)?;
        self.catalog.pending = true;
        self.catalog.status = "Waiting for MusicBrainz…".into();
        Ok(())
    }

    fn catalog_reply(
        &mut self,
        reply: Result<catalog::Reply, music_library::catalog::CatalogError>,
    ) {
        self.catalog.pending = false;
        self.catalog.status = match reply {
            Ok(catalog::Reply::Groups(page)) => {
                self.catalog.groups = page.items;
                self.catalog.group_next = page.next_offset;
                "Select Add Album, or inspect Editions.".into()
            }
            Ok(catalog::Reply::Editions(page)) => {
                self.catalog.editions = page.items;
                self.catalog.edition_next = page.next_offset;
                "Select an edition. Add this edition imports its complete tracklist.".into()
            }
            Ok(catalog::Reply::Add(release)) => {
                match self.session.library.add_catalog_release(&release) {
                    Ok(imported) => {
                        let refresh =
                            music_library::catalog::Timing::new("post_import.search_refresh");
                        self.session.search(release.album.title);
                        drop(refresh);
                        format!(
                            "Added Album: {} Tracks (no playback started).",
                            imported.track_ids.len()
                        )
                    }
                    Err(error) => error.to_string(),
                }
            }
            Err(error) => error.to_string(),
        };
    }

    fn catalog_snapshot_value(&self) -> QVariantMap {
        [
            ("catalogPending", self.catalog.pending.into()),
            ("catalogStatus", string(&self.catalog.status)),
            (
                "catalogMoreGroups",
                self.catalog.group_next.is_some().into(),
            ),
            (
                "catalogMoreEditions",
                self.catalog.edition_next.is_some().into(),
            ),
            (
                "catalogGroups",
                self.catalog
                    .groups
                    .iter()
                    .map(|g| {
                        let label = format!(
                            "{} · {} · {} · {} {} · {}",
                            g.title,
                            g.artist,
                            g.date,
                            g.primary_type,
                            g.secondary_types.join(", "),
                            g.comment
                        );
                        QVariant::from(
                            [("label", string(label))]
                                .into_iter()
                                .collect::<QVariantMap>(),
                        )
                    })
                    .collect::<QVariantList>()
                    .into(),
            ),
            (
                "catalogEditions",
                self.catalog
                    .editions
                    .iter()
                    .map(|r| {
                        let label = format!(
                            "{} · {} · {} {} · {} · {} · {} · {} discs/{} tracks · {} · barcode {}",
                            r.title,
                            r.artist,
                            r.date,
                            r.country,
                            r.status,
                            r.comment,
                            r.formats.join(", "),
                            r.disc_count,
                            r.track_count,
                            r.labels.join(", "),
                            r.barcode
                        );
                        QVariant::from(
                            [("label", string(label))]
                                .into_iter()
                                .collect::<QVariantMap>(),
                        )
                    })
                    .collect::<QVariantList>()
                    .into(),
            ),
        ]
        .into_iter()
        .collect()
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

fn catalog_callback(
    weak: qmetaobject::QPointer<Bridge>,
) -> impl Fn(Result<catalog::Reply, music_library::catalog::CatalogError>) + Send + Sync + 'static {
    qmetaobject::queued_callback(move |reply| {
        if let Some(bridge) = weak.as_pinned() {
            let mut bridge = bridge.borrow_mut();
            bridge.catalog_reply(reply);
            bridge.catalog_changed();
            bridge.changed();
            bridge.catalog.timing.take();
        }
    })
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
    bridge.pinned().borrow_mut().catalog.worker.take();
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

    #[test]
    #[ignore = "opt-in live MusicBrainz timing; requires MUSIC_LIBRARY_CATALOG_LIVE_QUERY"]
    fn live_catalog_latency_audit() {
        let query = std::env::var("MUSIC_LIBRARY_CATALOG_LIVE_QUERY").expect("set a catalog query");
        let (_temp, library) = sample::create().unwrap();
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml").replace(
            "    function ready() {",
            r#"
    Connections {
        target: window.bridge
        function onCatalog_changed() {
            if (!window.bridge.catalog_snapshot.catalogPending) Qt.quit();
        }
    }
    function auditSearch(query) { window.bridge.catalog_action("search", query); }
    function ready() {
"#,
        );
        engine.load_data(qml.into());
        let search_and_add = std::time::Instant::now();
        engine.invoke_method("auditSearch".into(), &[string(query)]);
        engine.exec();
        let index = bridge
            .pinned()
            .borrow()
            .catalog
            .groups
            .iter()
            .position(|a| a.primary_type == "Album")
            .unwrap_or_else(|| {
                panic!(
                    "No Album-type result: {}",
                    bridge.pinned().borrow().catalog.status
                )
            });
        engine.invoke_method("addAlbum".into(), &[(index as u32).into()]);
        engine.exec();
        let status = bridge.pinned().borrow().catalog.status.clone();
        eprintln!("catalog-timing completion={status}");
        eprintln!(
            "catalog-timing full_search_and_add_ms={:.3}",
            search_and_add.elapsed().as_secs_f64() * 1000.0
        );
        bridge.pinned().borrow_mut().catalog.worker.take();
        assert!(status.starts_with("Added Album"), "{status}");
    }

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

    fn catalog_flow(engine: &Rc<RefCell<QmlEngine>>, bridge: &QObjectBox<Bridge>) {
        use music_library::catalog::{
            AlbumCandidate, CatalogError, CatalogProvider, Medium, Page, Release, ReleaseCandidate,
            Track,
        };
        use music_library::domain::ExternalIdentity;
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        };
        struct Provider {
            gate: mpsc::Receiver<()>,
            calls: Arc<AtomicUsize>,
        }
        fn id(kind: &str) -> ExternalIdentity {
            ExternalIdentity {
                provider: "test".into(),
                kind: kind.into(),
                external_id: "id".into(),
            }
        }
        impl CatalogProvider for Provider {
            fn search_albums(
                &mut self,
                q: &str,
                _: u32,
            ) -> Result<Page<AlbumCandidate>, CatalogError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.gate.recv().unwrap();
                if q == "failure" {
                    return Err(CatalogError("HTTP 503 test service unavailable".into()));
                }
                Ok(Page {
                    next_offset: None,
                    items: vec![AlbumCandidate {
                        credits: vec![],
                        identity: id("group"),
                        title: "Catalog Fixture".into(),
                        artist: "Artist".into(),
                        date: "2001".into(),
                        primary_type: "Album".into(),
                        secondary_types: vec![],
                        comment: "".into(),
                        score: None,
                    }],
                })
            }
            fn releases(
                &mut self,
                _: &ExternalIdentity,
                _: u32,
            ) -> Result<Page<ReleaseCandidate>, CatalogError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(Page {
                    next_offset: None,
                    items: vec![ReleaseCandidate {
                        identity: id("release"),
                        title: "Catalog Fixture".into(),
                        artist: "Artist".into(),
                        date: "2001".into(),
                        country: "US".into(),
                        status: "Official".into(),
                        comment: "Edition".into(),
                        barcode: "".into(),
                        labels: vec![],
                        formats: vec!["CD".into()],
                        disc_count: 1,
                        track_count: 1,
                    }],
                })
            }
            fn representative_releases(
                &mut self,
                group: &ExternalIdentity,
            ) -> Result<Page<ReleaseCandidate>, CatalogError> {
                let mut page = self.releases(group, 0)?;
                for candidate in &mut page.items {
                    candidate.artist.clear();
                    candidate.labels.clear();
                }
                Ok(page)
            }
            fn release(&mut self, _: &ExternalIdentity) -> Result<Release, CatalogError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(Release {
                    album: music_library::catalog::Album {
                        identity: id("group"),
                        title: "Catalog Fixture".into(),
                        date: "2001".into(),
                        credits: vec![],
                    },
                    identity: id("release"),
                    identities: vec![],
                    title: "Catalog Fixture".into(),
                    date: "2001".into(),
                    credits: vec![],
                    media: vec![Medium {
                        position: 1,
                        tracks: vec![Track {
                            position: 1,
                            title: "Catalog Song".into(),
                            credits: vec![],
                            identities: vec![id("track")],
                        }],
                    }],
                })
            }
        }
        let (gate, recv) = mpsc::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let deliver = catalog_callback(qmetaobject::QPointer::from(bridge.pinned().borrow()));
        let quit = engine.clone();
        let done = qmetaobject::queued_callback(move |()| quit.borrow().quit());
        bridge.pinned().borrow_mut().catalog.worker = Some(
            catalog::Worker::new(
                Provider {
                    gate: recv,
                    calls: calls.clone(),
                },
                move |reply| {
                    deliver(reply);
                    done(());
                },
            )
            .unwrap(),
        );
        let invoke = |action: &str, value: &str| {
            engine
                .borrow_mut()
                .invoke_method("testCatalog".into(), &[string(action), string(value)]);
        };
        let snapshot = || {
            engine
                .borrow_mut()
                .invoke_method("testCatalogView".into(), &[])
                .to_qstring()
                .to_string()
        };
        let layout = || {
            engine
                .borrow_mut()
                .invoke_method("testCatalogLayout".into(), &[])
                .to_qstring()
                .to_string()
        };
        assert!(layout().starts_with("false|0|"));
        invoke("search", "Catalog");
        assert!(snapshot().starts_with("true|"));
        invoke("search", "duplicate"); // Must be rejected even if called directly through QML.
        // Process a Qt callback while the provider is deliberately blocked.
        let quit = engine.clone();
        let ping = qmetaobject::queued_callback(move |()| quit.borrow().quit());
        std::thread::spawn(move || ping(())).join().unwrap();
        engine.borrow().exec();
        assert!(snapshot().starts_with("true|"));
        gate.send(()).unwrap();
        engine.borrow().exec();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(snapshot().ends_with("|1|0"));
        assert!(layout().starts_with("false|1|Catalog Fixture"));
        assert!(
            layout().contains("Artist") && layout().contains("2001") && layout().contains("Album")
        );
        assert!(!layout().contains("barcode"));
        let transport_before_add = bridge.pinned().borrow().session.playback.state().clone();
        invoke("add_album", "0");
        engine.borrow().exec();
        assert!(snapshot().contains("Added Album: 1 Tracks"));
        assert_eq!(
            bridge.pinned().borrow().session.playback.state(),
            &transport_before_add
        );
        assert!(!bridge.pinned().borrow().session.rows[0].available);
        assert_eq!(calls.load(Ordering::SeqCst), 3); // Search + lightweight candidates + lookup.
        assert!(bridge.pinned().borrow().catalog.editions.is_empty());
        let first_import = bridge.pinned().borrow().session.rows[0].track_id.clone();
        invoke("editions", "0");
        engine.borrow().exec();
        assert!(snapshot().ends_with("|1|1"));
        assert!(layout().starts_with("true|1|"));
        assert_eq!(calls.load(Ordering::SeqCst), 4); // Editions fetched rich metadata separately.
        assert_eq!(
            bridge.pinned().borrow().catalog.editions[0].artist,
            "Artist"
        );
        assert_eq!(
            engine
                .borrow_mut()
                .invoke_method("testEditionSelected".into(), &[])
                .to_qstring()
                .to_string(),
            "-1"
        );
        engine
            .borrow_mut()
            .invoke_method("testEditionIndex".into(), &[0.into()]);
        bridge.pinned().borrow().changed(); // Playback/clock notifications must not reset edition selection.
        assert_eq!(
            engine
                .borrow_mut()
                .invoke_method("testEditionSelected".into(), &[])
                .to_int(),
            0
        );
        let transport = bridge.pinned().borrow().session.playback.state().clone();
        invoke("add", "0");
        engine.borrow().exec();
        assert!(snapshot().contains("Added Album: 1 Tracks"));
        assert_eq!(
            bridge.pinned().borrow().session.playback.state(),
            &transport
        );
        assert_eq!(bridge.pinned().borrow().session.rows.len(), 1);
        assert!(!bridge.pinned().borrow().session.rows[0].available);
        let imported = bridge.pinned().borrow().session.rows[0].track_id.clone();
        assert_eq!(first_import, imported);
        assert_eq!(calls.load(Ordering::SeqCst), 4); // Re-add reused the detail, too.
        invoke("search", "failure");
        gate.send(()).unwrap();
        engine.borrow().exec();
        assert!(snapshot().contains("HTTP 503"));
        assert_eq!(bridge.pinned().borrow().session.rows[0].track_id, imported);
        bridge.pinned().borrow_mut().catalog.worker.take();
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
    function testCatalog(action, value) {
        if (action === "add_album") window.addAlbum(Number(value));
        else if (action === "editions") window.chooseEditions(Number(value));
        else window.bridge.catalog_action(action, value);
    }
    function testCatalogLayout() {
        return catalogDialog.showEditions + "|" + albumResults.count + "|" +
            (albumResults.count ? window.catalogView.catalogGroups[0].label : "");
    }
    function testCatalogView() {
        return catalogView.catalogPending + "|" + catalogView.catalogStatus + "|"
            + catalogView.catalogGroups.length + "|" + catalogView.catalogEditions.length;
    }
    function testEditionIndex(value) { editions.currentIndex = value; return editions.currentIndex; }
    function testEditionSelected() { return editions.currentIndex; }
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
        catalog_flow(&engine, &bridge);
        drop(engine); // Destroy QML bindings while their QObject still exists.
    }
}
