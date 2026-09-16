//! Explicit-only catalog work. No playback token, polling or automatic retries.
use music_library::{
    catalog::{CatalogError, Page},
    song_resolution::{Candidate, Input, SongSearch},
};
use music_library_spotify::Spotify;
use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
pub struct Reply {
    pub generation: u64,
    pub input: Input,
    pub result: Result<Page<Candidate>, CatalogError>,
    pub counts: (u64, u64),
}
pub struct Worker {
    send: Option<mpsc::SyncSender<(u64, Input)>>,
    join: Option<thread::JoinHandle<()>>,
}
impl Worker {
    pub fn new(emit: impl Fn(Reply) + Send + 'static) -> Self {
        let (send, recv) = mpsc::sync_channel::<(u64, Input)>(1);
        let join = thread::spawn(move || {
            let mut client = Spotify::from_env();
            let mut unavailable: Option<(Instant, CatalogError)> = None;
            while let Ok((generation, input)) = recv.recv() {
                let result = if let Some((until, error)) = &unavailable
                    && *until > Instant::now()
                {
                    Err(error.clone())
                } else {
                    match &mut client {
                        Ok(client) => client.search_songs(&input),
                        Err(error) => Err(error.clone()),
                    }
                };
                if let Err(error) = &result {
                    if error.is_provider_unavailable()
                        && unavailable
                            .as_ref()
                            .is_none_or(|(until, _)| *until <= Instant::now())
                    {
                        let seconds = match error {
                            CatalogError::RateLimited { retry_after, .. }
                            | CatalogError::ServiceUnavailable { retry_after, .. } => retry_after
                                .as_ref()
                                .and_then(|s| s.parse::<u64>().ok())
                                .unwrap_or(30),
                            _ => 30,
                        }
                        .max(1);
                        unavailable = Some((
                            Instant::now() + Duration::from_secs(seconds.min(86400)),
                            error.clone(),
                        ));
                    }
                } else {
                    unavailable = None;
                }
                let counts = client
                    .as_ref()
                    .map(Spotify::request_counts)
                    .unwrap_or_default();
                emit(Reply {
                    generation,
                    input,
                    result,
                    counts,
                });
            }
        });
        Self {
            send: Some(send),
            join: Some(join),
        }
    }
    pub fn search(&self, generation: u64, input: Input) -> bool {
        self.send
            .as_ref()
            .is_some_and(|s| s.try_send((generation, input)).is_ok())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.send.take();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
