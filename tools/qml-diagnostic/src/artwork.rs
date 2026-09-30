use crate::Bridge;
use music_library::{
    artwork::{self, Provider},
    catalog::CatalogError,
    domain::ExternalIdentity,
};
use qmetaobject::{QPointer, QVariantMap};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime},
};

#[derive(Default)]
pub struct Artwork {
    pub values: QVariantMap,
    requested: HashMap<String, Instant>,
    sender: Option<mpsc::Sender<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
struct Providers {
    spotify: Option<music_library_spotify::Spotify>,
    cooldown: [Option<Instant>; 2],
}
impl Provider for Providers {
    fn fetch(&mut self, id: &ExternalIdentity) -> Option<(String, Vec<u8>)> {
        let index = usize::from(id.provider == "spotify");
        if self.cooldown[index].is_some_and(|t| t > Instant::now()) {
            return None;
        }
        let result = if id.provider == "musicbrainz" {
            music_library_musicbrainz::artwork(id)
        } else if id.provider == "spotify" && id.kind == "album" {
            let client = self.spotify.as_mut()?;
            client.artwork_url(id).and_then(|url| match url {
                Some(url) => {
                    music_library_musicbrainz::download_artwork(&url).map(|b| b.map(|b| (url, b)))
                }
                None => Ok(None),
            })
        } else {
            return None;
        };
        match result {
            Ok(value) => value,
            Err(error) => {
                let header = match &error {
                    CatalogError::RateLimited { retry_after, .. }
                    | CatalogError::ServiceUnavailable { retry_after, .. } => {
                        retry_after.as_deref()
                    }
                    _ => None,
                };
                let delay = header
                    .and_then(|h| {
                        h.parse::<u64>().ok().map(Duration::from_secs).or_else(|| {
                            httpdate::parse_http_date(h)
                                .ok()?
                                .duration_since(SystemTime::now())
                                .ok()
                        })
                    })
                    .unwrap_or(Duration::from_secs(60));
                self.cooldown[index] = Some(Instant::now() + delay);
                None
            }
        }
    }
}
impl Bridge {
    pub fn request_artwork(&mut self, keys: Vec<String>) {
        let keys: Vec<_> = keys
            .into_iter()
            .filter(|k| {
                if k.is_empty()
                    || self
                        .artwork
                        .requested
                        .get(k)
                        .is_some_and(|t| t.elapsed() < Duration::from_secs(60))
                {
                    return false;
                }
                self.artwork.requested.insert(k.clone(), Instant::now());
                true
            })
            .collect();
        if keys.is_empty() {
            return;
        }
        if self.artwork.sender.is_none() {
            let Some(cache) = artwork::cache_directory() else {
                return;
            };
            let Ok(mut resolver) = self.session.library.artwork_resolver(cache.clone()) else {
                return;
            };
            let Ok(mut remote_resolver) = self.session.library.artwork_resolver(cache) else {
                return;
            };
            let (tx, rx) = mpsc::channel::<Vec<String>>();
            let (remote_tx, remote_rx) = mpsc::channel::<Vec<String>>();
            let pointer = QPointer::from(&*self);
            let deliver = qmetaobject::queued_callback(move |(key, url): (String, String)| {
                if let Some(object) = pointer.as_pinned() {
                    let mut b = object.borrow_mut();
                    b.artwork.values.insert(key.into(), crate::string(&url));
                    b.artwork_changed();
                }
            });
            let stop = self.artwork.stop.clone();
            let remote_deliver = deliver.clone();
            let remote_stop = stop.clone();
            std::thread::spawn(move || {
                let mut providers = Providers {
                    spotify: music_library_spotify::Spotify::from_env().ok(),
                    cooldown: [None, None],
                };
                while let Ok(keys) = remote_rx.recv() {
                    if remote_stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let _ =
                        remote_resolver.resolve_with(&keys, &mut providers, &mut |key, path| {
                            if !remote_stop.load(Ordering::Relaxed) {
                                remote_deliver((key, local_url(path)));
                            }
                        });
                }
            });
            self.artwork.worker = Some(std::thread::spawn(move || {
                struct Local;
                impl Provider for Local {
                    fn deferred(&self) -> bool {
                        true
                    }
                    fn fetch(&mut self, _: &ExternalIdentity) -> Option<(String, Vec<u8>)> {
                        None
                    }
                }
                while let Ok(mut keys) = rx.recv() {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    for batch in rx.try_iter() {
                        keys.extend(batch);
                    }
                    keys.sort();
                    keys.dedup();
                    let result = resolver.resolve_with(&keys, &mut Local, &mut |key, path| {
                        // A deferred miss must not overwrite a completed remote result.
                        if path.is_some() && !stop.load(Ordering::Relaxed) {
                            deliver((key, local_url(path)));
                        }
                    });
                    if let Ok(rows) = result {
                        let missing = rows
                            .into_iter()
                            .filter_map(|(key, path)| path.is_none().then_some(key))
                            .collect::<Vec<_>>();
                        if !missing.is_empty() {
                            let _ = remote_tx.send(missing);
                        }
                    }
                }
            }));
            self.artwork.sender = Some(tx);
        }
        if let Some(tx) = &self.artwork.sender {
            let _ = tx.send(keys);
        }
    }
    pub fn retry_artwork(&mut self, key: String) {
        self.artwork.requested.remove(&key);
        self.request_artwork(vec![key]);
    }
    pub fn artwork_value(&self) -> QVariantMap {
        self.artwork.values.clone()
    }
}
impl Drop for Artwork {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.sender.take();
        // Worker owns no UI state. A bounded in-flight HTTP request may finish
        // after the window closes; QPointer makes its completion harmless.
        self.worker.take();
    }
}

fn local_url(path: Option<std::path::PathBuf>) -> String {
    path.and_then(|p| url::Url::from_file_path(p).ok())
        .map(|u| u.to_string())
        .unwrap_or_default()
}
