//! Explicit-only catalog work. No playback token, polling or automatic retries.
use music_library::{
    catalog::{CatalogError, Page},
    song_resolution::{Candidate, Input, SongSearch},
};
use music_library_spotify::Spotify;
use std::{sync::mpsc, thread};
pub struct Reply {
    pub generation: u64,
    pub input: Input,
    pub result: Result<Page<Candidate>, CatalogError>,
    pub counts: (u64, u64),
}
pub struct Worker {
    send: Option<mpsc::SyncSender<(u64, Input, Option<String>)>>,
    join: Option<thread::JoinHandle<()>>,
    refresh: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl Worker {
    #[cfg(test)]
    pub fn fake() -> (Self, mpsc::Receiver<(u64, Input, Option<String>)>) {
        let (send, receive) = mpsc::sync_channel(1);
        (
            Self {
                send: Some(send),
                join: None,
                refresh: Default::default(),
            },
            receive,
        )
    }
    pub fn with_library(emit: impl Fn(Reply) + Send + 'static, path: Option<String>) -> Self {
        let (send, recv) = mpsc::sync_channel::<(u64, Input, Option<String>)>(1);
        let refresh = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_refresh = refresh.clone();
        let join = thread::spawn(move || {
            let mut client = Spotify::from_env().and_then(|p| p.with_library(path.as_deref()));
            while let Ok((generation, input, artist_query)) = recv.recv() {
                let result = match &mut client {
                    Ok(client) => {
                        client.refresh_discovery(
                            worker_refresh.swap(false, std::sync::atomic::Ordering::AcqRel),
                        );
                        let mut lookup = input.clone();
                        if let Some(name) = artist_query {
                            if let Some(primary) = &mut lookup.primary_artist {
                                primary.name = name.clone();
                                // An explicit lookup name overrides discovery only.
                                primary.identities.clear();
                            }
                            lookup.artist = name;
                        }
                        client.search_songs(&lookup)
                    }
                    Err(error) => Err(error.clone()),
                };
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
            refresh,
        }
    }
    pub fn refresh_next_search(&self) {
        self.refresh
            .store(true, std::sync::atomic::Ordering::Release);
    }
    pub fn search_with_artist(
        &self,
        generation: u64,
        input: Input,
        artist: Option<String>,
    ) -> bool {
        self.send
            .as_ref()
            .is_some_and(|s| s.try_send((generation, input, artist)).is_ok())
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
