//! Disposable worker glue. OAuth and HTTP never run on the Qt owner thread.
use music_library_spotify::playback::{
    AuthorizationState, Error, POLL_INTERVAL, Playback, Snapshot, Song,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

pub enum Command {
    Connect,
    Cancel,
    Refresh,
    Select(String),
    Play(Song),
    Pause,
    Seek(u64, u64),
    Visible(bool),
    /// Acknowledged commands used only by explicit application backend handoff.
    ApplicationPlay(Song, u64, bool),
    ApplicationPause,
}
pub struct Update {
    pub snapshot: Snapshot,
    pub application_command: bool,
    pub completion: Option<crate::spotify_completion::Outcome>,
    pub generation: u64,
}
pub struct Worker {
    sender: Option<SyncSender<Command>>,
    join: Option<thread::JoinHandle<()>>,
    stopped: Arc<AtomicBool>,
}
impl Worker {
    #[cfg(test)]
    pub fn fake() -> (Self, Receiver<Command>) {
        let (sender, receiver) = mpsc::sync_channel(8);
        (
            Self {
                sender: Some(sender),
                join: None,
                stopped: Arc::new(AtomicBool::new(false)),
            },
            receiver,
        )
    }
    pub fn new(notify: impl Fn(Update) + Send + 'static) -> Self {
        let (sender, receiver) = mpsc::sync_channel(8);
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let join = thread::spawn(move || run(receiver, stop, notify));
        Self {
            sender: Some(sender),
            join: Some(join),
            stopped,
        }
    }
    pub fn send(&self, command: Command) -> bool {
        self.sender
            .as_ref()
            .is_some_and(|s| s.try_send(command).is_ok())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        self.sender.take();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
fn run(receiver: Receiver<Command>, stopped: Arc<AtomicBool>, notify: impl Fn(Update)) {
    let mut playback = match Playback::from_env() {
        Ok(p) => p,
        Err(error) => {
            notify(Update {
                snapshot: Snapshot {
                    error: Some(error),
                    ..Default::default()
                },
                application_command: false,
                completion: None,
                generation: 0,
            });
            return;
        }
    };
    notify(Update {
        snapshot: playback.snapshot.clone(),
        application_command: false,
        completion: None,
        generation: 0,
    });
    let mut auth = None;
    let mut visible = false;
    let mut application_active = false;
    let mut next_poll = Instant::now();
    let clock = Instant::now();
    let mut detector: Option<crate::spotify_completion::Detector> = None;
    let mut generation = 0;
    loop {
        if stopped.load(Ordering::Acquire) {
            return;
        }
        let command = match receiver.recv_timeout(Duration::from_millis(200)) {
            Ok(c) => Some(c),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };
        let mut result = Ok(());
        let mut application_command = false;
        let mut completion = None;
        if stopped.load(Ordering::Acquire) {
            return;
        }
        let mut changed = command.is_some();
        if let Some(command) = command {
            result = match command {
                Command::Connect => {
                    auth = None;
                    playback.begin_authorization().map(|a| {
                        auth = Some(a);
                    })
                }
                Command::Cancel => {
                    auth = None;
                    playback.snapshot.authorization_url = None;
                    playback.snapshot.authorization = AuthorizationState::Disconnected;
                    Ok(())
                }
                Command::Refresh => playback.devices(),
                Command::Select(id) => playback.select_device(&id),
                Command::Play(song) => playback.play(&song),
                Command::ApplicationPlay(song, current_generation, restart) => {
                    generation = current_generation;
                    application_command = true;
                    application_active = false;
                    detector = None;
                    let result = if restart {
                        playback.play_from_start(&song)
                    } else {
                        playback.play(&song)
                    };
                    result.inspect(|()| {
                        application_active = true;
                        detector = Some(crate::spotify_completion::Detector::new(&song.uri()));
                    })
                }
                Command::ApplicationPause => {
                    detector = None;
                    application_command = true;
                    playback
                        .pause_for_handoff()
                        .inspect(|()| application_active = false)
                }
                Command::Pause => {
                    detector = None;
                    playback.pause()
                }
                Command::Seek(ms, current_generation) => {
                    generation = current_generation;
                    if let Some(d) = &mut detector {
                        d.reset_evidence();
                    }
                    playback.seek(ms)
                }
                Command::Visible(v) => {
                    visible = v;
                    if v && playback.snapshot.authorization == AuthorizationState::Connected {
                        playback.devices()
                    } else {
                        Ok(())
                    }
                }
            };
            next_poll = Instant::now() + Duration::from_secs(1);
        }
        if let Some(a) = &auth {
            match playback.finish_authorization(a) {
                Ok(false) => {}
                Ok(true) => {
                    auth = None;
                    changed = true;
                    result = playback.devices();
                }
                Err(error) => {
                    auth = None;
                    changed = true;
                    playback.snapshot.authorization_url = None;
                    if playback.snapshot.authorization == AuthorizationState::Authorizing {
                        playback.snapshot.authorization = AuthorizationState::Disconnected;
                    }
                    result = Err(error);
                }
            }
        }
        if (visible || application_active)
            && playback.snapshot.authorization == AuthorizationState::Connected
            && Instant::now() >= next_poll
        {
            result = playback.poll();
            if result.is_ok() {
                if let Some(d) = &mut detector {
                    completion =
                        d.observe(&playback.snapshot.state, clock.elapsed().as_millis() as u64);
                    if completion.is_some() {
                        application_active = false;
                    }
                }
            } else if let Some(d) = &mut detector {
                d.reset_evidence();
            }
            changed = true;
            next_poll = Instant::now() + POLL_INTERVAL;
        }
        if result.is_err()
            && let Some(d) = &mut detector
        {
            d.reset_evidence();
        }
        if let Err(Error::RateLimited(seconds)) = &result {
            next_poll = Instant::now() + Duration::from_secs((*seconds).max(5));
        }
        if let Err(Error::ServiceUnavailable | Error::Transport) = &result {
            next_poll = Instant::now() + Duration::from_secs(30);
        }
        if let Err(
            Error::InsufficientScope
            | Error::CapabilityUnavailable
            | Error::ApiRejected(_)
            | Error::Configuration,
        ) = &result
        {
            visible = false;
            application_active = false;
        }
        if changed {
            playback.snapshot.error = result.err();
            notify(Update {
                snapshot: playback.snapshot.clone(),
                application_command,
                completion,
                generation,
            });
        }
    }
}
