//! Presentation state for three independent, bounded library panes.
use crate::{Bridge, row_value, string};
use music_library::{
    browse::{Cursor, Pane, Request, Row, Sort},
    domain::{AlbumId, ArtistId, TrackId},
};
use qmetaobject::{QPointer, QVariant, QVariantList, QVariantMap};

const PAGE: usize = 200;
#[derive(Clone, Default)]
pub struct Page {
    rows: Vec<Row>,
    cursors: Vec<Option<Cursor>>,
    more: bool,
    seek: Option<String>,
}
#[derive(Default)]
pub struct Browser {
    view: usize,
    views: [ViewState; 5],
    genre: String,
    global_songs_sort: usize,
    pages: [Page; 3],
    artist: String,
    album: String,
    song: String,
    navigation: u32,
    artist_sort: usize,
    album_sort: usize,
    artist_songs_sort: usize,
    album_songs_sort: usize,
    pub error: String,
    pub pending: bool,
    worker: Option<std::thread::JoinHandle<()>>,
    removal: Option<(
        music_library::library_removal::Target,
        music_library::library_removal::Preview,
    )>,
}
#[derive(Default)]
struct ViewState {
    pages: [Page; 3],
    artist: String,
    genre: String,
    album: String,
    song: String,
    sorts: [usize; 5],
}
impl Browser {
    fn switch_view(&mut self, view: usize) {
        if self.view == view {
            return;
        }
        self.views[self.view] = ViewState {
            pages: std::mem::take(&mut self.pages),
            artist: std::mem::take(&mut self.artist),
            genre: std::mem::take(&mut self.genre),
            album: std::mem::take(&mut self.album),
            song: std::mem::take(&mut self.song),
            sorts: [
                self.artist_sort,
                self.album_sort,
                self.artist_songs_sort,
                self.album_songs_sort,
                self.global_songs_sort,
            ],
        };
        let state = std::mem::take(&mut self.views[view]);
        self.pages = state.pages;
        self.artist = state.artist;
        self.genre = state.genre;
        self.album = state.album;
        self.song = state.song;
        [
            self.artist_sort,
            self.album_sort,
            self.artist_songs_sort,
            self.album_songs_sort,
            self.global_songs_sort,
        ] = state.sorts;
        self.view = view;
        self.navigation = self.navigation.wrapping_add(1);
    }
}
impl Bridge {
    pub fn browse_action_impl(&mut self, action: &str, pane: usize, id: String) {
        if pane > 2 {
            return;
        }
        match action {
            "view" => {
                let Some(view) = ["Artists", "Genres", "Albums", "Songs", "Playlists"]
                    .iter()
                    .position(|v| *v == id)
                else {
                    return;
                };
                self.browser.switch_view(view);
                for i in 0..3 {
                    self.load_pane(i, false);
                }
            }
            "remove-preview" => {
                if pane == 0 && self.browser.view != 0 {
                    return;
                }
                if self.local_import_state.busy {
                    return;
                }
                use music_library::library_removal::Target;
                let target = match pane {
                    0 => Target::Artist(ArtistId(id)),
                    1 => Target::Album(AlbumId(id)),
                    _ => Target::Track(TrackId(id)),
                };
                if self.browser.pending {
                    return;
                }
                let worker = match self.session.library.removal_worker() {
                    Ok(w) => w,
                    Err(e) => {
                        self.browser.error = e.to_string();
                        self.browse_changed();
                        return;
                    }
                };
                let preview_target = target.clone();
                let pointer = QPointer::from(&*self);
                let deliver = qmetaobject::queued_callback(move |result: Result<_, String>| {
                    if let Some(object) = pointer.as_pinned() {
                        let mut b = object.borrow_mut();
                        b.browser.pending = false;
                        if let Some(w) = b.browser.worker.take() {
                            let _ = w.join();
                        }
                        match result {
                            Ok(preview) => b.browser.removal = Some((target.clone(), preview)),
                            Err(e) => b.browser.error = e,
                        }
                        b.browse_changed();
                    }
                });
                self.browser.pending = true;
                self.browser.error.clear();
                self.browser.worker = Some(std::thread::spawn(move || {
                    deliver(worker.preview(&preview_target).map_err(|e| e.to_string()));
                }));
            }
            "remove-cancel" => self.browser.removal = None,
            "remove-confirm" => {
                if self.local_import_state.busy {
                    return;
                }
                if self.browser.pending {
                    return;
                }
                let Some((target, _)) = self.browser.removal.take() else {
                    return;
                };
                let worker = match self.session.library.removal_worker() {
                    Ok(w) => w,
                    Err(e) => {
                        self.browser.error = e.to_string();
                        self.browse_changed();
                        return;
                    }
                };
                let pointer = QPointer::from(&*self);
                let deliver = qmetaobject::queued_callback(move |result: Result<u64, String>| {
                    if let Some(object) = pointer.as_pinned() {
                        let mut b = object.borrow_mut();
                        b.browser.pending = false;
                        if let Some(w) = b.browser.worker.take() {
                            let _ = w.join();
                        }
                        match result {
                            Ok(_) => {
                                b.browser.artist.clear();
                                b.browser.album.clear();
                                b.browser.song.clear();
                                b.browser.navigation = b.browser.navigation.wrapping_add(1);
                                b.local_search_cancel();
                                if let Err(e) = b.music_context() {
                                    b.browser.error = e;
                                }
                                b.music_changed();
                                for i in 0..3 {
                                    b.load_pane(i, true);
                                }
                            }
                            Err(e) => b.browser.error = e,
                        }
                        b.browse_changed();
                    }
                });
                self.browser.pending = true;
                self.browser.worker = Some(std::thread::spawn(move || {
                    deliver(
                        worker
                            .remove(&target, id == "suppress")
                            .map_err(|e| e.to_string()),
                    );
                }));
            }

            "sort" => {
                match pane {
                    0 => self.browser.artist_sort = (self.browser.artist_sort + 1) % 2,
                    1 => {
                        self.browser.album_sort = (self.browser.album_sort + 1)
                            % if self.browser.artist.is_empty() && self.browser.genre.is_empty() {
                                3
                            } else {
                                2
                            }
                    }
                    _ if !self.browser.album.is_empty() => {
                        self.browser.album_songs_sort = (self.browser.album_songs_sort + 1) % 3
                    }
                    _ if !self.browser.artist.is_empty() || !self.browser.genre.is_empty() => {
                        self.browser.artist_songs_sort = (self.browser.artist_songs_sort + 1) % 3
                    }
                    _ => self.browser.global_songs_sort ^= 1,
                }
                self.load_pane(pane, true);
                if pane == 1
                    && (!self.browser.artist.is_empty() || !self.browser.genre.is_empty())
                    && self.browser.album.is_empty()
                    && self.browser.artist_songs_sort == 2
                {
                    self.load_pane(2, true);
                }
            }
            "select" => {
                self.browser.song = if pane == 2 { id.clone() } else { String::new() };
                if pane == 0 {
                    if self.browser.view == 1 {
                        self.browser.genre = id;
                    } else {
                        self.browser.artist = id;
                    }
                    self.browser.normalize_sorts();
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
                } else if self.browser.pages[pane].seek.is_some() {
                    self.load_pane(pane, true);
                }
            }
            "refresh" => {
                for i in 0..3 {
                    self.load_pane(i, true);
                }
            }
            "play" | "append" => {
                if self.browser.view == 4 {
                    return;
                }
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
                if action == "play" && pane < 2 {
                    self.browse_action_impl("select", pane, id.clone());
                }
                let mut request = Request {
                    pane: Pane::Songs,
                    limit: 200,
                    ..Default::default()
                };
                match pane {
                    0 if self.browser.view == 1 => request.genre = Some(id.clone()),
                    0 => request.artist = Some(ArtistId(id.clone())),
                    1 => request.album = Some(AlbumId(id.clone())),
                    _ if action == "append" => request.track = Some(TrackId(id.clone())),
                    _ if !self.browser.album.is_empty() => {
                        request.album = Some(AlbumId(self.browser.album.clone()))
                    }
                    _ if !self.browser.artist.is_empty() => {
                        request.artist = Some(ArtistId(self.browser.artist.clone()))
                    }
                    _ => {}
                }
                if self.browser.view == 1
                    && pane > 0
                    && !(pane == 2 && action == "append")
                    && !self.browser.genre.is_empty()
                {
                    request.genre = Some(self.browser.genre.clone());
                }
                request.album_sort = self.browser.album_program_sort();
                request.sort = self.browser.song_sort(
                    request.album.is_some(),
                    request.artist.is_some() || request.genre.is_some(),
                );
                let start = (pane == 2 && action == "play").then_some(TrackId(id));
                if pane == 2 && action == "append" {
                    let result = self
                        .session
                        .library
                        .library_queue(&request)
                        .map_err(|e| e.to_string());
                    self.finish_library_queue(
                        true,
                        result.and_then(|tracks| prepare_program(tracks, None)),
                    );
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
                        deliver(
                            reader
                                .read(&request)
                                .map_err(|e| e.to_string())
                                .and_then(|tracks| prepare_program(tracks, start.as_ref())),
                        );
                    }));
                }
            }
            _ => {}
        }
        self.browse_changed();
    }
    fn finish_library_queue(&mut self, append: bool, result: Result<Program, String>) {
        self.browser.pending = false;
        if let Some(worker) = self.browser.worker.take() {
            let _ = worker.join();
        }
        match result {
            Err(error) => self.browser.error = error,
            Ok((tracks, _)) if append => {
                for row in tracks {
                    self.session.playback.enqueue(row.track_id.clone());
                    self.session.queue_labels.push(row);
                }
                self.session.error.clear();
            }
            Ok(_) if self.route_pending.is_some() || self.automatic_song_search => {
                self.browser.error = "Playback is switching. Please try Play again.".into();
            }
            Ok((tracks, position)) => {
                if !tracks.is_empty() {
                    self.replace_library_program(tracks, position);
                }
            }
        }
        self.browse_changed();
        self.changed();
    }

    fn load_pane(&mut self, pane: usize, reset: bool) {
        if self.browser.view == 4
            || (pane == 0 && self.browser.view >= 2)
            || (pane == 1 && self.browser.view == 3)
        {
            self.browser.pages[pane] = Page::default();
            return;
        }
        let sort = self.browser.sort(pane);
        let album_sort = self.browser.album_program_sort();
        let page = &mut self.browser.pages[pane];
        if reset || page.cursors.is_empty() {
            page.cursors = vec![None];
            page.seek = None;
        }
        let request = Request {
            pane: if pane == 0 && self.browser.view == 1 {
                Pane::Genres
            } else {
                [Pane::Artists, Pane::Albums, Pane::Songs][pane]
            },
            genre: (pane > 0 && !self.browser.genre.is_empty()).then(|| self.browser.genre.clone()),
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
            sort,
            album_sort,
            after: page.cursors.last().cloned().flatten(),
            limit: 201,
            ..Default::default()
        };
        let result = if page.cursors.len() == 1
            && let Some(id) = &page.seek
        {
            self.session.library.browse_around(&request, id)
        } else {
            self.session.library.browse(&request)
        };
        match result {
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
    pub fn navigate_search(
        &mut self,
        hit: &music_library::library_search::Hit,
    ) -> Result<(), String> {
        use music_library::library_search::Kind;
        self.browser.switch_view(0);
        let artist = hit
            .artist_id
            .as_ref()
            .map(|a| a.as_ref().to_string())
            .unwrap_or_default();
        let album = if hit.kind == Kind::Artist {
            String::new()
        } else {
            hit.album_id
                .as_ref()
                .map(|a| a.as_ref().to_string())
                .unwrap_or_default()
        };
        let song = if hit.kind == Kind::Song {
            hit.id.clone()
        } else {
            String::new()
        };
        let previous_scope = (
            self.browser.artist.clone(),
            self.browser.album.clone(),
            self.browser.album_sort,
        );
        self.browser.artist = artist.clone();
        self.browser.album = album.clone();
        self.browser.normalize_sorts();
        let targets = [artist.clone(), album.clone(), song.clone()];
        let mut pages = Vec::new();
        for (pane, target) in targets.iter().enumerate() {
            let request = Request {
                pane: [Pane::Artists, Pane::Albums, Pane::Songs][pane],
                artist: if pane > 0 && !artist.is_empty() && (pane == 1 || album.is_empty()) {
                    Some(ArtistId(artist.clone()))
                } else {
                    None
                },
                album: if pane == 2 && !album.is_empty() {
                    Some(AlbumId(album.clone()))
                } else {
                    None
                },
                sort: self.browser.sort(pane),
                album_sort: self.browser.album_program_sort(),
                limit: 201,
                ..Default::default()
            };
            let mut rows = if target.is_empty() {
                self.session.library.browse(&request)
            } else {
                self.session.library.browse_around(&request, target)
            }
            .map_err(|e| {
                self.browser.artist = previous_scope.0.clone();
                self.browser.album = previous_scope.1.clone();
                self.browser.album_sort = previous_scope.2;
                e.to_string()
            })?;
            let more = rows.len() > PAGE;
            rows.truncate(PAGE);
            pages.push(Page {
                rows,
                more,
                cursors: vec![None],
                seek: (!target.is_empty()).then_some(target.clone()),
            });
        }
        self.browser.pages = pages
            .try_into()
            .map_err(|_| "Invalid navigation".to_string())?;
        self.browser.artist = artist;
        self.browser.album = album;
        self.browser.song = song;

        self.browser.navigation = self.browser.navigation.wrapping_add(1);
        self.browser.error.clear();
        self.browse_changed();
        Ok(())
    }

    pub fn browse_value(&self) -> QVariantMap {
        let panes: QVariantList = self
            .browser
            .pages
            .iter()
            .enumerate()
            .map(|(pane, p)| {
                let rows: QVariantList = p
                    .rows
                    .iter()
                    .map(|r| {
                        let mut map: QVariantMap = [
                            ("id", string(&r.id)),
                            ("title", string(&r.title)),
                            ("subtitle", string(&r.subtitle)),
                            (
                                "year",
                                string(r.year.map(|y| y.to_string()).unwrap_or_default()),
                            ),
                            ("group", string(&r.group)),
                            ("groupLabel", string(&r.group_label)),
                            (
                                "number",
                                string(if pane == 2 && self.browser.sort(2) == Sort::Album {
                                    r.track_number
                                        .map(|n| {
                                            if r.multi_disc {
                                                format!("{}.{n:02}", r.disc_number.unwrap_or(1))
                                            } else {
                                                n.to_string()
                                            }
                                        })
                                        .unwrap_or_default()
                                } else {
                                    String::new()
                                }),
                            ),
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
                        ("sort", string(self.browser.sort_label(pane))),
                        ("more", p.more.into()),
                        ("anchored", p.seek.is_some().into()),
                        ("page", (p.cursors.len() as i32).into()),
                    ]
                    .into_iter()
                    .collect::<QVariantMap>(),
                )
            })
            .collect();
        [
            (
                "removal",
                QVariant::from(
                    self.browser
                        .removal
                        .as_ref()
                        .map(|(target, p)| {
                            use music_library::library_removal::Target;
                            let message = match target {
                                Target::Track(_) => {
                                    format!("Remove \"{}\" from your library?", p.title)
                                }
                                _ => format!(
                                    "Remove \"{}\" and its {} saved Tracks from your library?",
                                    p.title, p.saved_tracks
                                ),
                            };
                            [
                                ("message", string(message)),
                                ("local", p.has_local_sources.into()),
                            ]
                            .into_iter()
                            .collect::<QVariantMap>()
                        })
                        .unwrap_or_default(),
                ),
            ),
            ("panes", panes.into()),
            (
                "view",
                string(["Artists", "Genres", "Albums", "Songs", "Playlists"][self.browser.view]),
            ),
            ("genre", string(&self.browser.genre)),
            ("artist", string(&self.browser.artist)),
            ("album", string(&self.browser.album)),
            ("song", string(&self.browser.song)),
            ("navigation", self.browser.navigation.into()),
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

type Program = (Vec<music_library::domain::TrackSearchResult>, usize);

fn prepare_program(
    tracks: Vec<music_library::domain::TrackSearchResult>,
    start: Option<&TrackId>,
) -> Result<Program, String> {
    let position = match start {
        Some(id) => tracks
            .iter()
            .position(|r| &r.track_id == id)
            .ok_or_else(|| {
                "This Track is no longer in the library program. Please refresh.".to_string()
            })?,
        None => 0,
    };
    Ok((tracks, position))
}

impl Browser {
    fn album_program_sort(&self) -> Sort {
        if self.album_sort == 0 {
            Sort::Title
        } else {
            Sort::Year
        }
    }
    fn normalize_sorts(&mut self) {
        if (!self.artist.is_empty() || !self.genre.is_empty()) && self.album_sort == 2 {
            self.album_sort = 1;
        }
    }
    fn song_sort(&self, album: bool, scoped: bool) -> Sort {
        if album {
            [Sort::Album, Sort::Title, Sort::Descending][self.album_songs_sort]
        } else if scoped {
            [Sort::Title, Sort::Descending, Sort::Album][self.artist_songs_sort]
        } else {
            [Sort::Title, Sort::Descending][self.global_songs_sort]
        }
    }
    fn sort(&self, pane: usize) -> Sort {
        match pane {
            0 => {
                if self.artist_sort == 0 {
                    Sort::Title
                } else {
                    Sort::Descending
                }
            }
            1 => match self.album_sort {
                1 => Sort::Year,
                2 => Sort::Artist,
                _ => Sort::Title,
            },
            _ => self.song_sort(
                !self.album.is_empty(),
                !self.artist.is_empty() || !self.genre.is_empty(),
            ),
        }
    }
    fn sort_label(&self, pane: usize) -> &'static str {
        if self.view == 4 {
            return "";
        }
        match self.sort(pane) {
            Sort::Descending => "Z-A",
            Sort::Year => "Year",
            Sort::Artist => "Artist",
            Sort::Album => "Album",
            _ => "A-Z",
        }
    }
}
