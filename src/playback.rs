//! In-memory, UI-independent playback orchestration. No decoding or audio output lives here.
use crate::Library;
use crate::domain::{PlayableSource, TrackId};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq, Error)]
#[error("{0}")]
pub struct EngineError(pub String);

/// Synchronous acknowledgements, not a queue/state model.
/// `start` replaces any previous input and starts the given source from its beginning.
/// On error, output state may be unknown; the controller requires a successful stop to recover.
pub trait PlaybackEngine {
    fn start(&mut self, source: &PlayableSource) -> Result<(), EngineError>;
    fn pause(&mut self) -> Result<(), EngineError>;
    fn resume(&mut self) -> Result<(), EngineError>;
    fn stop(&mut self) -> Result<(), EngineError>;
}

#[derive(Debug, Error)]
pub enum PlaybackError {
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlaybackState {
    pub queue: Vec<TrackId>,
    /// Selected queue entry, including an entry whose playback attempt failed.
    pub position: Option<usize>,
    pub status: PlaybackStatus,
    /// Last successfully loaded source, cleared on stop or engine failure.
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
}

impl<E: PlaybackEngine> Playback<E> {
    pub fn new(engine: E) -> Self {
        Self {
            engine,
            state: PlaybackState::default(),
        }
    }

    pub fn state(&self) -> &PlaybackState {
        &self.state
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
        match self.state.status {
            PlaybackStatus::Playing => Ok(()),
            PlaybackStatus::Paused => {
                let result = self.engine.resume();
                self.acknowledge(result)?;
                self.state.status = PlaybackStatus::Playing;
                Ok(())
            }
            PlaybackStatus::Stopped | PlaybackStatus::Failed => self.start_at(library, position),
        }
    }

    pub fn pause(&mut self) -> Result<(), PlaybackError> {
        match self.state.status {
            PlaybackStatus::Playing => {
                let result = self.engine.pause();
                self.acknowledge(result)?;
                self.state.status = PlaybackStatus::Paused;
                Ok(())
            }
            PlaybackStatus::Failed => Err(PlaybackError::Failed),
            _ => Ok(()),
        }
    }

    /// Stop output, retaining the queue and current position. Play will resolve a source afresh.
    pub fn stop(&mut self) -> Result<(), PlaybackError> {
        if self.state.status != PlaybackStatus::Stopped {
            let result = self.engine.stop();
            self.acknowledge(result)?;
        }
        self.state.status = PlaybackStatus::Stopped;
        self.state.source = None;
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
        let result = self.engine.start(&source);
        self.acknowledge(result)?;
        self.state.source = Some(source);
        self.state.status = PlaybackStatus::Playing;
        Ok(())
    }

    fn acknowledge(&mut self, result: Result<(), EngineError>) -> Result<(), PlaybackError> {
        result.map_err(|error| {
            self.state.status = PlaybackStatus::Failed;
            self.state.source = None;
            PlaybackError::Engine(error)
        })
    }
}
