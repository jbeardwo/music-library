//! Disposable presentation adapter. The backend remains the only playback-state owner.
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use music_library::{
    Library,
    domain::{PlayableSource, SearchCursor, SearchRequest, TrackSearchResult},
    playback::{EngineError, Playback, PlaybackEngine, Volume},
};

pub const PAGE_SIZE: usize = 20;

#[derive(Default)]
pub struct EngineDiagnostics {
    pub fail_next: bool,
    pub calls: VecDeque<String>,
}

pub struct DiagnosticEngine(Rc<RefCell<EngineDiagnostics>>);
impl DiagnosticEngine {
    fn call(&self, operation: &str) -> Result<(), EngineError> {
        let mut diagnostics = self.0.borrow_mut();
        let fail = std::mem::take(&mut diagnostics.fail_next);
        if diagnostics.calls.len() == 12 {
            diagnostics.calls.pop_front();
        }
        diagnostics
            .calls
            .push_back(format!("{operation}: {}", if fail { "FAIL" } else { "ok" }));
        if fail {
            Err(EngineError(format!(
                "diagnostic fake engine rejected {operation}"
            )))
        } else {
            Ok(())
        }
    }
}
impl PlaybackEngine for DiagnosticEngine {
    fn set_volume(&mut self, volume: Volume) -> Result<(), EngineError> {
        self.call(&format!("volume {}", volume.get()))
    }
    fn start(&mut self, source: &PlayableSource) -> Result<(), EngineError> {
        self.call(&format!("start {}", source.source_id.as_ref()))
    }
    fn pause(&mut self) -> Result<(), EngineError> {
        self.call("pause")
    }
    fn resume(&mut self) -> Result<(), EngineError> {
        self.call("resume")
    }
    fn stop(&mut self) -> Result<(), EngineError> {
        self.call("stop")
    }
}

pub enum Engine {
    #[cfg(all(test, feature = "gstreamer"))]
    Deferred(DiagnosticEngine),
    Fake(DiagnosticEngine),
    #[cfg(feature = "gstreamer")]
    GStreamer(music_library_gstreamer::GStreamerEngine),
}
impl Engine {
    fn inner(&mut self) -> &mut dyn PlaybackEngine {
        match self {
            Self::Fake(engine) => engine,
            #[cfg(all(test, feature = "gstreamer"))]
            Self::Deferred(engine) => engine,
            #[cfg(feature = "gstreamer")]
            Self::GStreamer(engine) => engine,
        }
    }
}
impl PlaybackEngine for Engine {
    fn set_volume(&mut self, volume: Volume) -> Result<(), EngineError> {
        self.inner().set_volume(volume)
    }
    fn asynchronous(&self) -> bool {
        match self {
            Self::Fake(_) => false,
            #[cfg(all(test, feature = "gstreamer"))]
            Self::Deferred(_) => true,
            #[cfg(feature = "gstreamer")]
            Self::GStreamer(_) => true,
        }
    }
    fn set_event_generation(&mut self, generation: u64) {
        self.inner().set_event_generation(generation);
    }
    fn start(&mut self, source: &PlayableSource) -> Result<(), EngineError> {
        self.inner().start(source)
    }
    fn pause(&mut self) -> Result<(), EngineError> {
        self.inner().pause()
    }
    fn resume(&mut self) -> Result<(), EngineError> {
        self.inner().resume()
    }
    fn stop(&mut self) -> Result<(), EngineError> {
        self.inner().stop()
    }
}

pub struct Session {
    pub library: Library,
    pub playback: Playback<Engine>,
    pub diagnostics: Rc<RefCell<EngineDiagnostics>>,
    pub rows: Vec<TrackSearchResult>,
    // Display metadata captured when queueing; never a second queue authority.
    pub queue_labels: Vec<TrackSearchResult>,
    pub text: String,
    pub has_next: bool,
    pub page: usize,
    cursors: Vec<Option<SearchCursor>>,
    pub error: String,
    operation: String,
    pub revision: u32,
}

impl Session {
    pub fn new(library: Library) -> Self {
        let diagnostics = Rc::new(RefCell::new(EngineDiagnostics::default()));
        let mut session = Self {
            library,
            playback: Playback::new(Engine::Fake(DiagnosticEngine(diagnostics.clone()))),
            diagnostics,
            rows: vec![],
            queue_labels: vec![],
            text: String::new(),
            has_next: false,
            page: 0,
            cursors: vec![None],
            error: String::new(),
            operation: String::new(),
            revision: 0,
        };
        session.search(String::new());
        session
    }

    #[cfg(feature = "gstreamer")]
    pub fn shutdown_audio(&mut self) {
        self.playback = Playback::new(Engine::Fake(DiagnosticEngine(self.diagnostics.clone())));
    }

    #[cfg(all(test, feature = "gstreamer"))]
    pub fn defer_confirmations(&mut self) {
        self.playback = Playback::new(Engine::Deferred(DiagnosticEngine(self.diagnostics.clone())));
    }

    #[cfg(feature = "gstreamer")]
    pub fn engine_event(&mut self, event: music_library::playback::EngineEvent) -> bool {
        let terminal = matches!(
            event.kind,
            music_library::playback::EngineEventKind::EndOfStream
        );
        match self.playback.handle_event(&self.library, event) {
            Ok(false) => false,
            Ok(true) => {
                self.revision = self.revision.wrapping_add(1);
                // Position/state notifications must not erase a visible decoder/source error.
                if terminal {
                    self.error.clear();
                    self.operation = "EOS".into();
                }
                true
            }
            Err(error) => {
                self.finish("engine event", Err(error.to_string()));
                true
            }
        }
    }

    fn load(&mut self, text: String, after: Option<SearchCursor>) -> Result<(), String> {
        let mut rows = self
            .library
            .search(&SearchRequest {
                text: text.clone(),
                after,
                limit: (PAGE_SIZE + 1) as u32,
                ..Default::default()
            })
            .map_err(|e| e.to_string())?;
        self.has_next = rows.len() > PAGE_SIZE;
        rows.truncate(PAGE_SIZE);
        self.rows = rows;
        self.text = text;
        Ok(())
    }

    // Keep only the last action, never a cached copy of confirmed playback state.
    // Async confirmations already notify QML; every snapshot must render their state.
    pub fn outcome(&self) -> String {
        let state = self.playback.state();
        let pending = state
            .pending
            .map_or(String::new(), |target| format!(" → {target:?} pending"));
        format!("{} → {:?}{pending}", self.operation, state.status)
    }

    fn finish(&mut self, operation: &str, result: Result<(), String>) {
        self.revision = self.revision.wrapping_add(1);
        self.error = result.err().unwrap_or_default();
        self.operation = operation.into();
    }

    pub fn set_volume(&mut self, value: f64) {
        let result = self
            .playback
            .set_volume(value)
            .map_err(|error| error.to_string());
        self.finish("volume", result);
    }

    pub fn search(&mut self, text: String) {
        let result = self.load(text, None);
        if result.is_ok() {
            self.page = 0;
            self.cursors = vec![None];
        }
        self.finish("search", result);
    }

    pub fn page_next(&mut self) {
        if !self.has_next {
            return;
        }
        let after = self.rows.last().map(|row| SearchCursor {
            title: row.title.clone(),
            track_id: row.track_id.clone(),
        });
        let result = self.load(self.text.clone(), after.clone());
        if result.is_ok() {
            self.cursors.truncate(self.page + 1);
            self.cursors.push(after);
            self.page += 1;
        }
        self.finish("next search page", result);
    }

    pub fn page_previous(&mut self) {
        if self.page == 0 {
            return;
        }
        let result = self.load(self.text.clone(), self.cursors[self.page - 1].clone());
        if result.is_ok() {
            self.page -= 1;
        }
        self.finish("previous search page", result);
    }

    fn replace_queue(&mut self, rows: Vec<TrackSearchResult>) -> Result<(), String> {
        self.playback
            .set_queue(rows.iter().map(|row| row.track_id.clone()).collect())
            .map_err(|e| e.to_string())?;
        self.queue_labels = rows;
        Ok(())
    }

    pub fn queue_page(&mut self) {
        let result = self.replace_queue(self.rows.clone());
        self.finish("replace queue with page (stops)", result);
    }

    pub fn enqueue_row(&mut self, id: &str) {
        let result = if let Some(row) = self
            .rows
            .iter()
            .find(|row| row.track_id.as_ref() == id)
            .cloned()
        {
            self.playback.enqueue(row.track_id.clone());
            self.queue_labels.push(row);
            Ok(())
        } else {
            Err("Track is no longer on this search page; search again".into())
        };
        self.finish("add to queue", result);
    }

    pub fn clear_queue(&mut self) {
        let result = self.playback.clear_queue().map_err(|e| e.to_string());
        if result.is_ok() {
            self.queue_labels.clear();
        }
        self.finish("clear queue", result);
    }

    pub fn play_row(&mut self, id: &str) {
        // Use opaque identity, not a delegate index that might refer to a different page.
        let result = if let Some(row) = self
            .rows
            .iter()
            .find(|row| row.track_id.as_ref() == id)
            .cloned()
        {
            self.replace_queue(vec![row])
                .and_then(|()| self.playback.play(&self.library).map_err(|e| e.to_string()))
        } else {
            Err("Track is no longer on this search page; search again".into())
        };
        self.finish("replace queue and play Track", result);
    }

    pub fn command(&mut self, command: &str) {
        let result = match command {
            "play" => self.playback.play(&self.library),
            "pause" => self.playback.pause(),
            "stop" => self.playback.stop(),
            "next" => self.playback.next(&self.library).map(|_| ()),
            "previous" => self.playback.previous(&self.library).map(|_| ()),
            _ => {
                self.finish(command, Err("Unknown playback command".into()));
                return;
            }
        }
        .map_err(|e| e.to_string());
        self.finish(command, result);
    }

    pub fn toggle_failure(&mut self) {
        let mut diagnostics = self.diagnostics.borrow_mut();
        diagnostics.fail_next = !diagnostics.fail_next;
        drop(diagnostics);
        self.finish("toggle next engine-call failure", Ok(()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample;
    use music_library::playback::PlaybackStatus;
    use std::collections::HashSet;

    #[test]
    fn pagination_is_bounded_complete_reversible_and_resets_on_search() {
        let (_temp, library) = sample::create().unwrap();
        let mut s = Session::new(library);
        let first = s.rows.clone();
        let mut ids = HashSet::new();
        loop {
            assert!(s.rows.len() <= PAGE_SIZE);
            for row in &s.rows {
                assert!(ids.insert(row.track_id.clone()));
            }
            if !s.has_next {
                break;
            }
            s.page_next();
        }
        assert_eq!(ids.len(), 45);
        s.page_previous();
        s.page_previous();
        assert_eq!(s.rows, first);
        s.page_next();
        s.search("Available".into());
        assert_eq!(s.page, 0);
        assert!(
            s.rows
                .iter()
                .all(|row| row.title.to_lowercase().contains("available"))
        );
        s.search("no_such_title".into());
        assert!(s.rows.is_empty());
        assert!(!s.has_next);
    }

    #[test]
    fn errors_recovery_and_queue_labels_follow_backend_state() {
        let (_temp, library) = sample::create().unwrap();
        let mut s = Session::new(library);
        s.search("Available".into());
        s.queue_page();
        s.command("play");
        assert_eq!(s.playback.state().status, PlaybackStatus::Playing);
        let queue = s.playback.state().queue.clone();
        let labels = s.queue_labels.clone();
        s.toggle_failure();
        s.search("Sourceless".into());
        s.queue_page(); // Stop fails: labels must still describe the old queue.
        assert!(!s.error.is_empty());
        assert_eq!(s.playback.state().queue, queue);
        assert_eq!(s.queue_labels, labels);
        assert_eq!(s.playback.state().status, PlaybackStatus::Failed);
        s.command("play"); // Successful stop, then start.
        assert!(s.error.is_empty());
        assert_eq!(s.playback.state().status, PlaybackStatus::Playing);
        let calls = &s.diagnostics.borrow().calls;
        assert_eq!(calls[calls.len() - 2], "stop: ok");
    }

    #[test]
    fn source_errors_and_stale_selection_are_visible() {
        let (_temp, library) = sample::create().unwrap();
        let mut s = Session::new(library);
        for query in ["Sourceless", "Unavailable"] {
            s.search(query.into());
            let id = s.rows[0].track_id.clone();
            s.play_row(id.as_ref());
            assert!(s.error.contains("no available supported playable source"));
            assert_eq!(s.playback.state().current_track(), Some(&id));
            assert_eq!(s.playback.state().status, PlaybackStatus::Stopped);
            assert_eq!(s.rows.len(), 1);
        }
        s.play_row("stale-id");
        assert!(s.error.contains("no longer"));
        assert!(s.diagnostics.borrow().calls.is_empty());
    }

    #[test]
    fn append_navigation_errors_and_clear_preserve_queue_labels() {
        let (_temp, library) = sample::create().unwrap();
        let mut s = Session::new(library);
        let missing = s.rows[0].track_id.clone();
        let playable = s.rows[2].track_id.clone();
        s.enqueue_row(missing.as_ref());
        s.enqueue_row(playable.as_ref());
        s.enqueue_row(playable.as_ref());
        assert_eq!(s.playback.state().position, Some(0));
        assert_eq!(s.playback.state().status, PlaybackStatus::Stopped);
        assert!(s.diagnostics.borrow().calls.is_empty());
        let ids = s.playback.state().queue.clone();
        let labels = s.queue_labels.clone();
        s.command("play");
        assert!(!s.error.is_empty());
        assert_eq!(s.playback.state().current_track(), Some(&missing));
        s.command("next");
        assert_eq!(s.playback.state().position, Some(1));
        assert_eq!(s.playback.state().status, PlaybackStatus::Playing);
        assert!(s.error.is_empty());
        s.toggle_failure();
        s.command("pause");
        assert_eq!(s.playback.state().status, PlaybackStatus::Failed);
        assert!(s.playback.state().can_next() && s.playback.state().can_previous());
        assert!(!s.error.is_empty());
        s.command("next");
        assert_eq!(s.playback.state().position, Some(2));
        assert_eq!(s.playback.state().status, PlaybackStatus::Playing);
        s.toggle_failure();
        s.command("pause");
        s.command("previous");
        assert_eq!(s.playback.state().position, Some(1));
        assert_eq!(s.playback.state().status, PlaybackStatus::Playing);
        assert_eq!(s.playback.state().queue, ids);
        assert_eq!(s.queue_labels, labels);
        s.enqueue_row("stale-id");
        assert!(!s.error.is_empty());
        assert_eq!(s.playback.state().queue, ids);
        assert_eq!(s.queue_labels, labels);
        s.toggle_failure();
        s.clear_queue();
        assert!(!s.error.is_empty());
        assert_eq!(s.playback.state().status, PlaybackStatus::Failed);
        assert_eq!(s.playback.state().queue, ids);
        assert_eq!(s.queue_labels, labels);
        s.clear_queue();
        assert!(s.error.is_empty());
        assert!(s.playback.state().queue.is_empty());
        assert!(s.queue_labels.is_empty());
        assert_eq!(s.playback.state().position, None);
        assert_eq!(s.playback.state().status, PlaybackStatus::Stopped);
        s.search(String::new());
        assert_eq!(s.rows.len(), PAGE_SIZE);
        assert!(s.rows.iter().any(|row| row.track_id == missing));
    }
}
