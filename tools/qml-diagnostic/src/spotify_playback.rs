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
    Seek(u64),
    Visible(bool),
}
pub struct Worker {
    sender: Option<SyncSender<Command>>,
    join: Option<thread::JoinHandle<()>>,
    stopped: Arc<AtomicBool>,
}
impl Worker {
    pub fn new(notify: impl Fn(Snapshot) + Send + 'static) -> Self {
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
fn run(receiver: Receiver<Command>, stopped: Arc<AtomicBool>, notify: impl Fn(Snapshot)) {
    let mut playback = match Playback::from_env() {
        Ok(p) => p,
        Err(error) => {
            notify(Snapshot {
                error: Some(error),
                ..Default::default()
            });
            return;
        }
    };
    notify(playback.snapshot.clone());
    let mut auth = None;
    let mut visible = false;
    let mut next_poll = Instant::now();
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
                Command::Pause => playback.pause(),
                Command::Seek(ms) => playback.seek(ms),
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
        if visible
            && playback.snapshot.authorization == AuthorizationState::Connected
            && Instant::now() >= next_poll
        {
            result = playback.poll();
            changed = true;
            next_poll = Instant::now() + POLL_INTERVAL;
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
        }
        if changed {
            playback.snapshot.error = result.err();
            notify(playback.snapshot.clone());
        }
    }
}
