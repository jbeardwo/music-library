//! In-memory, UI-independent playback orchestration. No decoding or audio output lives here.
use crate::Library;
use crate::domain::{PlayableSource, TrackId};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq, Error)]
#[error("{0}")]
pub struct EngineError(pub String);

/// Finite normalized linear gain: 0 is silence, 1 is unity gain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Volume(f64);

impl Default for Volume {
    fn default() -> Self {
        Self(1.0)
    }
}

impl Volume {
    pub fn new(value: f64) -> Result<Self, PlaybackError> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(PlaybackError::InvalidVolume);
        }
        Ok(Self(value))
    }
    pub fn get(self) -> f64 {
        self.0
    }
}

/// Commands, not a queue/state model. Synchronous engines complete on return;
/// asynchronous engines accept on return and confirm through generation-tagged events.
/// `start` replaces any previous input and starts the given source from its beginning.
/// On transport error, output state may be unknown; the controller requires a successful stop to recover.
pub trait PlaybackEngine {
    /// Set before a new media lifetime or Stop. Events must retain their originating generation.
    fn set_event_generation(&mut self, _generation: u64) {}
    fn asynchronous(&self) -> bool {
        false
    }
    /// Accept session gain without changing transport or media generation.
    /// Engines start at unity gain and retain accepted volume across inputs and Stop.
    /// Rejection preserves the previously accepted volume and transport state.
    fn set_volume(&mut self, volume: Volume) -> Result<(), EngineError>;
    fn start(&mut self, source: &PlayableSource) -> Result<(), EngineError>;
    fn pause(&mut self) -> Result<(), EngineError>;
    fn resume(&mut self) -> Result<(), EngineError>;
    fn stop(&mut self) -> Result<(), EngineError>;
}

/// Plain application data; no decoder/backend types cross this boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngineEvent {
    pub generation: u64,
    pub kind: EngineEventKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EngineEventKind {
    State(PlaybackStatus),
    EndOfStream,
    Error(EngineError),
    Duration(Option<u64>),
    Position(u64),
}

#[derive(Debug, Error)]
pub enum PlaybackError {
    #[error("volume must be finite and between 0 and 1")]
    InvalidVolume,
    #[error("the playback queue is empty")]
    EmptyQueue,
    #[error("Track {0:?} has no available supported playable source")]
    NoAvailableSource(TrackId),
    #[error("engine state is unknown; stop, play, or navigate to recover")]
    Failed,
    #[error(transparent)]
    Storage(#[from] crate::storage::Error),
    #[error(transparent)]
    Engine(#[from] EngineError),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlaybackStatus {
    #[default]
    Stopped,
    Playing,
    Paused,
    /// An engine command failed; this does not claim that audio output has stopped.
    Failed,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlaybackState {
    /// Accepted session gain; independent of media/transport confirmation.
    pub volume: Volume,
    pub queue: Vec<TrackId>,
    /// Selected queue entry, including an entry whose playback attempt failed.
    pub position: Option<usize>,
    /// Last confirmed state (Stopped for a newly selected input until it starts).
    pub status: PlaybackStatus,
    /// Accepted asynchronous command awaiting confirmation; never an optimistic actual state.
    pub pending: Option<PlaybackStatus>,
    /// Media clock, distinct from the queue index. Stop resets it immediately.
    pub media_position_ms: u64,
    pub duration_ms: Option<u64>,
    /// Selected input source (possibly still loading), cleared on stop or engine failure.
    pub source: Option<PlayableSource>,
}

impl PlaybackState {
    pub fn can_next(&self) -> bool {
        self.position
            .is_some_and(|position| position + 1 < self.queue.len())
    }

    pub fn can_previous(&self) -> bool {
        self.position.is_some_and(|position| position > 0)
    }

    pub fn current_track(&self) -> Option<&TrackId> {
        self.position.and_then(|position| self.queue.get(position))
    }
}

/// Owns the engine and session state. Mutations are available only through application commands.
pub struct Playback<E> {
    engine: E,
    state: PlaybackState,
    generation: u64,
}

impl<E: PlaybackEngine> Playback<E> {
    pub fn new(engine: E) -> Self {
        Self {
            engine,
            state: PlaybackState::default(),
            generation: 0,
        }
    }

    pub fn state(&self) -> &PlaybackState {
        &self.state
    }

    pub fn set_volume(&mut self, value: f64) -> Result<(), PlaybackError> {
        let volume = Volume::new(value)?;
        self.engine.set_volume(volume)?;
        self.state.volume = volume;
        Ok(())
    }

    /// Stop first; if stopping fails the existing queue and position are preserved.
    pub fn set_queue(&mut self, queue: Vec<TrackId>) -> Result<(), PlaybackError> {
        self.stop()?;
        self.state.position = (!queue.is_empty()).then_some(0);
        self.state.queue = queue;
        Ok(())
    }

    /// Append without disturbing playback or consuming earlier entries. Duplicates are allowed.
    /// The first append selects position zero without starting playback.
    pub fn enqueue(&mut self, track_id: TrackId) {
        self.state.queue.push(track_id);
        if self.state.position.is_none() {
            self.state.position = Some(0);
        }
    }

    /// Stop and empty the queue. If the engine cannot stop, retain the queue and report failure.
    pub fn clear_queue(&mut self) -> Result<(), PlaybackError> {
        self.set_queue(Vec::new())
    }

    /// Resume a paused input, or resolve and start the current queue entry.
    pub fn play(&mut self, library: &Library) -> Result<(), PlaybackError> {
        let position = self.state.position.ok_or(PlaybackError::EmptyQueue)?;
        match self.requested_status() {
            PlaybackStatus::Playing => Ok(()),
            PlaybackStatus::Paused => {
                let result = self.engine.resume();
                self.acknowledge(result)?;
                self.accept_command(PlaybackStatus::Playing);
                Ok(())
            }
            PlaybackStatus::Stopped | PlaybackStatus::Failed => self.start_at(library, position),
        }
    }

    pub fn pause(&mut self) -> Result<(), PlaybackError> {
        match self.requested_status() {
            PlaybackStatus::Playing => {
                let result = self.engine.pause();
                self.acknowledge(result)?;
                self.accept_command(PlaybackStatus::Paused);
                Ok(())
            }
            PlaybackStatus::Failed => Err(PlaybackError::Failed),
            _ => Ok(()),
        }
    }

    /// Stop output, retaining the queue and current position. Play will resolve a source afresh.
    pub fn stop(&mut self) -> Result<(), PlaybackError> {
        if self.state.status != PlaybackStatus::Stopped || self.state.pending.is_some() {
            self.new_generation(); // Invalidate old EOS/errors even before Stop completes.
            let result = self.engine.stop();
            self.acknowledge(result)?;
            self.accept_command(PlaybackStatus::Stopped);
        }
        self.state.source = None;
        self.state.media_position_ms = 0;
        self.state.duration_ms = None;
        Ok(())
    }

    /// Start the next entry. At the end, stop without wrapping and return false.
    pub fn next(&mut self, library: &Library) -> Result<bool, PlaybackError> {
        let position = self.state.position.ok_or(PlaybackError::EmptyQueue)?;
        if position + 1 == self.state.queue.len() {
            self.stop()?;
            return Ok(false);
        }
        self.start_at(library, position + 1)?;
        Ok(true)
    }

    /// Start the preceding entry; at the first entry return false without changing playback.
    pub fn previous(&mut self, library: &Library) -> Result<bool, PlaybackError> {
        let position = self.state.position.ok_or(PlaybackError::EmptyQueue)?;
        if position == 0 {
            return Ok(false);
        }
        self.start_at(library, position - 1)?;
        Ok(true)
    }

    fn start_at(&mut self, library: &Library, position: usize) -> Result<(), PlaybackError> {
        // Selection is independent of successful playback: another Next/Previous must
        // move past a failed entry rather than repeatedly retrying the same neighbor.
        self.state.position = Some(position);
        let track_id = &self.state.queue[position];
        let source = library
            .available_playback_source(track_id)
            .map_err(PlaybackError::from)
            .and_then(|source| {
                source.ok_or_else(|| PlaybackError::NoAvailableSource(track_id.clone()))
            });
        let source = match source {
            Ok(source) => source,
            Err(error) => {
                // Do not leave the old Track playing under the newly selected entry.
                // A failed stop takes precedence: actual engine output is then unknown.
                self.stop()?;
                return Err(error);
            }
        };
        if self.state.status == PlaybackStatus::Failed {
            self.stop()?;
        }
        self.new_generation();
        self.state.status = PlaybackStatus::Stopped;
        self.state.pending = None;
        self.state.media_position_ms = 0;
        self.state.duration_ms = None;
        let result = self.engine.start(&source);
        self.acknowledge(result)?;
        self.state.source = Some(source);
        self.accept_command(PlaybackStatus::Playing);
        Ok(())
    }

    fn requested_status(&self) -> PlaybackStatus {
        self.state.pending.unwrap_or(self.state.status)
    }

    fn accept_command(&mut self, target: PlaybackStatus) {
        if self.engine.asynchronous() {
            self.state.pending = Some(target);
        } else {
            self.state.status = target;
            self.state.pending = None;
        }
    }

    fn new_generation(&mut self) {
        self.generation = self
            .generation
            .checked_add(1)
            .expect("playback generation exhausted");
        self.engine.set_event_generation(self.generation);
    }

    /// Apply on the application's owning thread. Returns false for stale/irrelevant events.
    /// EOS advances exactly once and surfaces an unplayable neighbor without skipping it.
    pub fn handle_event(
        &mut self,
        library: &Library,
        event: EngineEvent,
    ) -> Result<bool, PlaybackError> {
        if event.generation != self.generation {
            return Ok(false);
        }
        match event.kind {
            EngineEventKind::State(status) => {
                // GstPlay state messages have no operation ID. Only confirm the latest
                // requested target; e.g. delayed Playing must not undo a pending Pause.
                if status == PlaybackStatus::Failed || status != self.requested_status() {
                    return Ok(false);
                }
                self.state.status = status;
                self.state.pending = None;
            }
            EngineEventKind::Position(ms) => {
                if self.state.source.is_none() {
                    return Ok(false);
                }
                self.state.media_position_ms = ms;
            }
            EngineEventKind::Duration(ms) => {
                if self.state.source.is_none() {
                    return Ok(false);
                }
                self.state.duration_ms = ms;
            }
            EngineEventKind::Error(error) => {
                self.acknowledge(Err(error))?;
            }
            EngineEventKind::EndOfStream => {
                if self.state.source.is_none() {
                    return Ok(false);
                }
                self.new_generation(); // Consume EOS before automatic navigation, including errors.
                self.next(library)?;
            }
        }
        Ok(true)
    }

    fn acknowledge(&mut self, result: Result<(), EngineError>) -> Result<(), PlaybackError> {
        result.map_err(|error| {
            self.new_generation();
            self.state.status = PlaybackStatus::Failed;
            self.state.pending = None;
            self.state.source = None;
            self.state.media_position_ms = 0;
            self.state.duration_ms = None;
            PlaybackError::Engine(error)
        })
    }
}
