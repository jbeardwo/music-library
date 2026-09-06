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
            ("canNext", state.can_next().into()),
            ("canPrevious", state.can_previous().into()),
            (
                "source",
                string(state.source.as_ref().map_or("—", |s| s.source_id.as_ref())),
            ),
            ("error", string(&s.error)),
            ("outcome", string(&s.outcome)),
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let smoke =
        match args.as_slice() {
            [] => false,
            [arg] if arg == "--smoke-test" => true,
            _ => return Err(
                "usage: qml-diagnostic [--smoke-test] (always uses a disposable sample database)"
                    .into(),
            ),
        };
    let (_temp, library) = sample::create()?;
    // The pinned QObject outlives the QML engine, including QML object destruction.
    let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
    let mut engine = QmlEngine::new();
    engine.set_object_property("diagnostic".into(), bridge.pinned());
    engine.load_data(include_str!("../Main.qml").into());
    if !engine.invoke_method("ready".into(), &[]).to_bool() {
        return Err(
            "QML did not create the diagnostic window; check Qt module errors above".into(),
        );
    }
    if smoke {
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
}
