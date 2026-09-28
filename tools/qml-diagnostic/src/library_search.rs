//! Coalesced local database work. This module cannot invoke provider workers.
use crate::{Bridge, string};
use music_library::library_search::{Hit, Kind};
use qmetaobject::{QPointer, QVariant, QVariantList, QVariantMap};

#[derive(Default)]
pub struct Search {
    hits: Vec<Hit>,
    generation: u64,
    busy: bool,
    latest: Option<(String, Kind)>,
    worker: Option<std::thread::JoinHandle<()>>,
    error: String,
}
impl Bridge {
    pub fn local_search_start(&mut self, text: String, filter: i32) {
        self.library_search.generation += 1;
        self.library_search.hits.clear();
        self.library_search.error.clear();
        let kind = match filter {
            1 => Kind::Artist,
            2 => Kind::Album,
            3 => Kind::Song,
            4 => Kind::Playlist,
            _ => Kind::All,
        };
        self.library_search.latest = Some((text, kind));
        if !self.library_search.busy {
            self.local_search_next();
        }
        self.local_search_changed();
    }
    pub fn local_search_cancel(&mut self) {
        self.library_search.generation += 1;
        self.library_search.latest = None;
        self.library_search.hits.clear();
        self.local_search_changed();
    }
    fn local_search_next(&mut self) {
        let Some((text, kind)) = self.library_search.latest.take() else {
            return;
        };
        let reader = match self.session.library.local_search_reader() {
            Ok(reader) => reader,
            Err(e) => {
                self.library_search.error = e.to_string();
                return;
            }
        };
        let generation = self.library_search.generation;
        let pointer = QPointer::from(&*self);
        let deliver = qmetaobject::queued_callback(move |result: Result<Vec<Hit>, String>| {
            if let Some(object) = pointer.as_pinned() {
                let mut b = object.borrow_mut();
                if let Some(worker) = b.library_search.worker.take() {
                    let _ = worker.join();
                }
                b.library_search.busy = false;
                if generation == b.library_search.generation {
                    match result {
                        Ok(hits) => b.library_search.hits = hits,
                        Err(e) => b.library_search.error = e,
                    }
                }
                b.local_search_next();
                b.local_search_changed();
            }
        });
        self.library_search.busy = true;
        self.library_search.worker = Some(std::thread::spawn(move || {
            deliver(reader.search(&text, kind).map_err(|e| e.to_string()))
        }));
    }
    pub fn local_search_go(&mut self, index: i32) -> bool {
        let Some(hit) = self.library_search.hits.get(index as usize).cloned() else {
            return false;
        };
        match self.navigate_search(&hit) {
            Ok(()) => true,
            Err(e) => {
                self.library_search.error = e;
                self.local_search_changed();
                false
            }
        }
    }
    pub fn local_search_value(&self) -> QVariantMap {
        let hits: QVariantList = self
            .library_search
            .hits
            .iter()
            .map(|h| {
                let section = match h.kind {
                    Kind::Artist => "ARTISTS",
                    Kind::Album => "ALBUMS",
                    Kind::Song => "SONGS",
                    Kind::Playlist => "PLAYLISTS",
                    Kind::All => "RESULTS",
                };
                let context = match h.kind {
                    Kind::Artist => String::new(),
                    Kind::Album => h.artist.clone(),
                    _ if h.artist.is_empty() => h.album.clone(),
                    _ => format!("{} — {}", h.artist, h.album),
                };
                QVariant::from(
                    [
                        ("section", string(section)),
                        ("id", string(&h.id)),
                        ("title", string(&h.title)),
                        ("context", string(context)),
                    ]
                    .into_iter()
                    .collect::<QVariantMap>(),
                )
            })
            .collect();
        [
            ("rows", hits.into()),
            ("busy", self.library_search.busy.into()),
            ("error", string(&self.library_search.error)),
        ]
        .into_iter()
        .collect()
    }
}
impl Drop for Search {
    fn drop(&mut self) {
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}
