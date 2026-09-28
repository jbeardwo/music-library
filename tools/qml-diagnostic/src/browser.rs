//! Presentation state for three independent, bounded library panes.
use crate::{Bridge, row_value, string};
use music_library::{
    browse::{Cursor, Pane, Request, Row},
    domain::{AlbumId, ArtistId, TrackId},
};
use qmetaobject::{QPointer, QVariant, QVariantList, QVariantMap};

const PAGE: usize = 200;
#[derive(Default)]
pub struct Page {
    rows: Vec<Row>,
    cursors: Vec<Option<Cursor>>,
    more: bool,
}
#[derive(Default)]
pub struct Browser {
    pages: [Page; 3],
    artist: String,
    album: String,
    pub error: String,
    pub pending: bool,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Bridge {
    pub fn browse_action_impl(&mut self, action: &str, pane: usize, id: String) {
        if pane > 2 {
            return;
        }
        match action {
            "select" => {
                if pane == 0 {
                    self.browser.artist = id;
                    self.browser.album.clear();
                    self.load_pane(1, true);
                    self.load_pane(2, true);
                } else if pane == 1 {
                    self.browser.album = id;
                    self.load_pane(2, true);
                }
            }
            "next" => {
                if self.browser.pages[pane].more {
                    let cursor = self.browser.pages[pane]
                        .rows
                        .last()
                        .map(|r| r.cursor.clone());
                    self.browser.pages[pane].cursors.push(cursor);
                    self.load_pane(pane, false);
                }
            }
            "previous" => {
                if self.browser.pages[pane].cursors.len() > 1 {
                    self.browser.pages[pane].cursors.pop();
                    self.load_pane(pane, false);
                }
            }
            "refresh" => {
                for i in 0..3 {
                    self.load_pane(i, true);
                }
            }
            "play" | "append" => {
                if self.browser.pending {
                    return;
                }
                if action == "play" && (self.route_pending.is_some() || self.automatic_song_search)
                {
                    self.session.error =
                        "A playback request is in progress. Please wait or cancel it.".into();
                    self.changed();
                    return;
                }
                let mut request = Request {
                    pane: Pane::Songs,
                    program: true,
                    limit: 200,
                    ..Default::default()
                };
                match pane {
                    0 => request.artist = Some(ArtistId(id)),
                    1 => request.album = Some(AlbumId(id)),
                    _ => request.track = Some(TrackId(id)),
                }
                if pane == 2 {
                    let result = self
                        .session
                        .library
                        .library_queue(&request)
                        .map_err(|e| e.to_string());
                    self.finish_library_queue(action == "append", result);
                } else {
                    let reader = match self.session.library.library_queue_reader() {
                        Ok(reader) => reader,
                        Err(error) => {
                            self.browser.error = error.to_string();
                            self.browse_changed();
                            return;
                        }
                    };
                    let append = action == "append";
                    let pointer = QPointer::from(&*self);
                    let deliver = qmetaobject::queued_callback(move |result| {
                        if let Some(object) = pointer.as_pinned() {
                            object.borrow_mut().finish_library_queue(append, result);
                        }
                    });
                    self.browser.pending = true;
                    self.browser.error.clear();
                    self.browser.worker = Some(std::thread::spawn(move || {
                        deliver(reader.read(&request).map_err(|e| e.to_string()));
                    }));
                }
            }
            _ => {}
        }
        self.browse_changed();
    }
    fn finish_library_queue(
        &mut self,
        append: bool,
        result: Result<Vec<music_library::domain::TrackSearchResult>, String>,
    ) {
        self.browser.pending = false;
        if let Some(worker) = self.browser.worker.take() {
            let _ = worker.join();
        }
        match result {
            Err(error) => self.browser.error = error,
            Ok(tracks) if append => {
                for row in tracks {
                    self.session.playback.enqueue(row.track_id.clone());
                    self.session.queue_labels.push(row);
                }
                self.session.error.clear();
            }
            Ok(_) if self.route_pending.is_some() || self.automatic_song_search => {
                self.browser.error = "Playback is switching. Please try Play again.".into();
            }
            Ok(tracks) => {
                if !tracks.is_empty() {
                    self.replace_library_queue(tracks);
                }
            }
        }
        self.browse_changed();
        self.changed();
    }

    fn load_pane(&mut self, pane: usize, reset: bool) {
        let page = &mut self.browser.pages[pane];
        if reset {
            page.cursors = vec![None];
        }
        let request = Request {
            pane: [Pane::Artists, Pane::Albums, Pane::Songs][pane],
            artist: if pane > 0
                && !self.browser.artist.is_empty()
                && (pane == 1 || self.browser.album.is_empty())
            {
                Some(ArtistId(self.browser.artist.clone()))
            } else {
                None
            },
            album: if pane == 2 && !self.browser.album.is_empty() {
                Some(AlbumId(self.browser.album.clone()))
            } else {
                None
            },
            after: page.cursors.last().cloned().flatten(),
            limit: 201,
            ..Default::default()
        };
        match self.session.library.browse(&request) {
            Ok(mut rows) => {
                page.more = rows.len() > PAGE;
                rows.truncate(PAGE);
                page.rows = rows;
                self.browser.error.clear();
            }
            Err(e) => {
                page.rows.clear();
                page.more = false;
                self.browser.error = e.to_string();
            }
        }
    }
    pub fn browse_value(&self) -> QVariantMap {
        let panes: QVariantList = self
            .browser
            .pages
            .iter()
            .map(|p| {
                let rows: QVariantList = p
                    .rows
                    .iter()
                    .map(|r| {
                        let mut map: QVariantMap = [
                            ("id", string(&r.id)),
                            ("title", string(&r.title)),
                            ("subtitle", string(&r.subtitle)),
                        ]
                        .into_iter()
                        .collect();
                        if let Some(track) = &r.track {
                            map.insert("track".into(), row_value(track));
                        }
                        QVariant::from(map)
                    })
                    .collect();
                QVariant::from(
                    [
                        ("rows", QVariant::from(rows)),
                        ("more", p.more.into()),
                        ("page", (p.cursors.len() as i32).into()),
                    ]
                    .into_iter()
                    .collect::<QVariantMap>(),
                )
            })
            .collect();
        [
            ("panes", panes.into()),
            ("artist", string(&self.browser.artist)),
            ("album", string(&self.browser.album)),
            ("error", string(&self.browser.error)),
            ("pending", self.browser.pending.into()),
        ]
        .into_iter()
        .collect()
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
