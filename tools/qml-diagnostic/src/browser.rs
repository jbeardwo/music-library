//! Presentation state for independent, bidirectional library windows.
use crate::{Bridge, row_value, string};
use music_library::{
    browse::{Cursor, Pane, Request, Row, Sort},
    domain::{AlbumId, ArtistId, TrackId},
};
use qmetaobject::{QPointer, QVariant, QVariantList, QVariantMap};

const PAGE: usize = 200;
const WINDOW: usize = PAGE * 3;
#[derive(Clone, Default)]
pub struct Page {
    rows: Vec<Row>,
    cursors: Vec<Option<Cursor>>,
    more: bool,
    seek: Option<String>,
    before: bool,
    epoch: u32,
    scroll_id: String,
    scroll_pixel: f64,
}
struct PlaylistViewRequest {
    generation: u64,
    playlists: Vec<String>,
    sort: music_library::playlist::ViewSort,
    cursor: Option<Cursor>,
    reverse: Option<bool>,
}
#[derive(Default)]
pub struct Browser {
    playlist_sort: music_library::playlist::ViewSort,
    playlist_view_generation: u64,
    playlist_view_pending: Option<PlaylistViewRequest>,
    playlist_view_worker: Option<std::thread::JoinHandle<()>>,
    playlist_view_reader: Option<music_library::browse::QueueReader>,
    playlist_details: Option<music_library::playlist::Details>,
    playlist_details_id: Option<String>,
    playlist_details_generation: u64,
    playlist_details_pending: Option<(u64, String)>,
    playlist_details_worker: Option<std::thread::JoinHandle<()>>,
    picker: Page,
    picker_target: Option<music_library::track_container::Target>,
    picker_order: Request,
    pub(crate) duplicate_plan: Option<music_library::playlist::AppendPlan>,
    selections: [music_library::selection::Selection; 3],
    selection_revision: u64,
    view: usize,
    views: [ViewState; 5],
    genre: String,
    global_songs_sort: usize,
    songs_column: music_library::browse::SongColumn,
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
    selections: [music_library::selection::Selection; 3],
    pages: [Page; 3],
    artist: String,
    genre: String,
    album: String,
    song: String,
    sorts: [usize; 5],
    songs_column: music_library::browse::SongColumn,
}
impl Browser {
    fn switch_view(&mut self, view: usize) {
        if self.view == view {
            return;
        }
        self.views[self.view] = ViewState {
            songs_column: self.songs_column,
            selections: std::mem::take(&mut self.selections),
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
        self.songs_column = state.songs_column;
        self.selections = state.selections;
        self.pages = state.pages;
        self.selection_revision = self.selection_revision.wrapping_add(1);
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
        self.playlist_view_generation = self.playlist_view_generation.wrapping_add(1);
        self.playlist_view_pending = None;
        self.view = view;
        self.navigation = self.navigation.wrapping_add(1);
    }
}
impl Bridge {
    pub fn browse_action_impl(&mut self, action: &str, pane: usize, id: String) {
        if pane > 2 {
            return;
        }
        if action.starts_with("picker-") {
            if action == "picker-open" {
                self.browser.picker_target = Some(self.browser.action_target(pane, &id));
                self.browser.picker_order = self.browser.action_order(pane);
                self.browser.picker.cursors = vec![None];
            } else if action == "picker-next" && self.browser.picker.more {
                self.browser
                    .picker
                    .cursors
                    .push(self.browser.picker.rows.last().map(|r| r.cursor.clone()));
            } else if action == "picker-previous" && self.browser.picker.cursors.len() > 1 {
                self.browser.picker.cursors.pop();
            } else if action == "picker-add" {
                self.prepare_selection_append(id);
                return;
            } else if action == "picker-yes" || action == "picker-no" {
                if let Some(plan) = self.browser.duplicate_plan.take() {
                    self.commit_selection_append(plan, action == "picker-yes");
                    return;
                }
            } else if action == "picker-cancel" {
                self.browser.duplicate_plan = None;
            }
            match self.session.library.playlists(
                self.browser.picker.cursors.last().and_then(|c| c.as_ref()),
                201,
            ) {
                Ok(mut rows) => {
                    self.browser.picker.more = rows.len() > 200;
                    rows.truncate(200);
                    self.browser.picker.rows = rows;
                }
                Err(e) => self.browser.error = e.to_string(),
            }
            self.browse_changed();
            return;
        }
        if action == "songs-sort" && self.browser.view == 3 && pane == 2 {
            use music_library::browse::SongColumn;
            let column = match id.as_str() {
                "song" => SongColumn::Song,
                "artist" => SongColumn::Artist,
                "album" => SongColumn::Album,
                "genre" => SongColumn::Genre,
                _ => return,
            };
            if self.browser.songs_column == column {
                self.browser.global_songs_sort ^= 1;
            } else {
                self.browser.songs_column = column;
                self.browser.global_songs_sort = 0;
            }
            self.browser.selection_revision = self.browser.selection_revision.wrapping_add(1);
            self.load_pane(2, true);
            self.browse_changed();
            return;
        }
        if action == "playlist-sort" && self.browser.view == 4 && pane == 2 {
            use music_library::playlist::Column;
            let column = match id.as_str() {
                "position" => Column::Position,
                "title" => Column::Title,
                "artist" => Column::Artist,
                "album" => Column::Album,
                "length" => Column::Length,
                _ => return,
            };
            if self.browser.playlist_sort.column == column {
                self.browser.playlist_sort.descending ^= true;
            } else {
                self.browser.playlist_sort.column = column;
                self.browser.playlist_sort.descending = false;
            }
            self.browser.selection_revision = self.browser.selection_revision.wrapping_add(1);
            self.load_pane(2, true);
            self.browse_changed();
            return;
        }
        if action.starts_with("playlist-") {
            if matches!(action, "playlist-up" | "playlist-down")
                && !self.browser.playlist_sort.allows_reordering()
            {
                return;
            }
            let playlist = if matches!(action, "playlist-up" | "playlist-down") {
                self.browser.pages[2]
                    .rows
                    .iter()
                    .find(|r| r.id == id)
                    .map(|r| r.group.clone())
                    .unwrap_or_else(|| self.browser.artist.clone())
            } else {
                self.browser.artist.clone()
            };
            let result = match action {
                "playlist-create" => self.session.library.create_playlist(&id).map(|_| ()),
                "playlist-rename" => self.session.library.rename_playlist(&playlist, &id),
                "playlist-delete" => self
                    .session
                    .library
                    .delete_playlists(&self.browser.action_ids(0, &id)),
                "playlist-remove" => self
                    .session
                    .library
                    .remove_playlist_entries(&self.browser.action_ids(2, &id)),
                "playlist-up" | "playlist-down" => self.session.library.move_playlist_entry(
                    &playlist,
                    &id,
                    action == "playlist-down",
                ),
                _ => return,
            };
            match result {
                Ok(()) => {
                    if action == "playlist-delete" {
                        self.browser.selections[0].single("");
                        self.selection_changed(0);
                    }
                    for i in 0..3 {
                        self.load_pane(i, true);
                    }
                    if action == "playlist-remove" {
                        self.browser.selections[2].single("");
                        self.browser.sync_selection_focus();
                    }
                    if action == "playlist-rename"
                        && let Some(details) = &mut self.browser.playlist_details
                        && details.id == playlist
                    {
                        details.name = id.trim().into();
                    }
                    if !matches!(action, "playlist-up" | "playlist-down") {
                        self.refresh_playlist_details(true);
                    }
                    if action == "playlist-create" {
                        self.browser.picker.cursors = vec![None];
                        self.browse_action_impl("picker-refresh", 0, String::new());
                    }
                }
                Err(e) => self.browser.error = e.to_string(),
            }
            self.browse_changed();
            return;
        }
        match action {
            "save-playlist-track" => {
                if self.browser.view != 4 {
                    return;
                }
                if let Some(track) = self.browser.pages[2]
                    .rows
                    .iter()
                    .find(|r| r.id == id)
                    .and_then(|r| r.track.as_ref())
                    .map(|t| t.track_id.clone())
                    && let Err(e) = self.session.library.add_to_library(&track)
                {
                    self.browser.error = e.to_string();
                }
                self.refresh_selection();
            }
            "view" => {
                let Some(view) = ["Artists", "Genres", "Albums", "Songs", "Playlists"]
                    .iter()
                    .position(|v| *v == id)
                else {
                    return;
                };
                self.browser.switch_view(view);
                self.refresh_playlist_details(false);
                for i in 0..3 {
                    if self.browser.pages[i].rows.is_empty() {
                        self.load_pane(i, false);
                    }
                }
            }
            "remove-preview" => {
                if pane == 0 && self.browser.view != 0 && self.browser.view != 1 {
                    return;
                }
                if self.local_import_state.busy {
                    return;
                }
                use music_library::library_removal::Target;
                let target = match pane {
                    0 => Target::Artist(ArtistId(id.clone())),
                    1 => Target::Album(AlbumId(id.clone())),
                    _ => Target::Track(TrackId(id.clone())),
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
                let expansion = (self.browser.selections[pane].ids.len() > 1
                    || (pane == 0 && self.browser.view == 1))
                    .then(|| {
                        (
                            self.browser.action_target(pane, &id),
                            self.browser.action_order(pane),
                        )
                    });
                let reader = match self.session.library.library_queue_reader() {
                    Ok(r) => r,
                    Err(e) => {
                        self.browser.error = e.to_string();
                        return;
                    }
                };
                let pointer = QPointer::from(&*self);
                let deliver = qmetaobject::queued_callback(move |result: Result<_, String>| {
                    if let Some(object) = pointer.as_pinned() {
                        let mut b = object.borrow_mut();
                        b.browser.pending = false;
                        if let Some(w) = b.browser.worker.take() {
                            let _ = w.join();
                        }
                        match result {
                            Ok((target, preview)) => b.browser.removal = Some((target, preview)),
                            Err(e) => b.browser.error = e,
                        }
                        b.browse_changed();
                    }
                });
                self.browser.pending = true;
                self.browser.error.clear();
                self.browser.worker = Some(std::thread::spawn(move || {
                    let result = (|| {
                        let target = if let Some((target, order)) = expansion {
                            Target::Tracks(
                                reader
                                    .resolve(&target, &order)?
                                    .into_iter()
                                    .map(|r| r.track_id.0)
                                    .collect(),
                            )
                        } else {
                            preview_target
                        };
                        let preview = worker.preview(&target)?;
                        Ok::<_, music_library::Error>((target, preview))
                    })();
                    deliver(result.map_err(|e| e.to_string()));
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
                                b.browser.navigation = b.browser.navigation.wrapping_add(1);
                                b.local_search_cancel();
                                if let Err(e) = b.music_context() {
                                    b.browser.error = e;
                                }
                                b.music_changed();
                                b.refresh_selection();
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
                self.browser.selection_revision = self.browser.selection_revision.wrapping_add(1);
                if self.browser.view == 4 {
                    return;
                }
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
            "select" | "select-toggle" | "select-range" | "context" => {
                self.select_items(action, pane, id);
            }
            "scroll-position" => {
                if let Some((row, pixel)) = id.rsplit_once('\n') {
                    self.browser.pages[pane].scroll_id = row.to_string();
                    self.browser.pages[pane].scroll_pixel = pixel.parse().unwrap_or(0.0);
                }
                return;
            }
            "scroll-forward" | "scroll-backward" => {
                self.scroll_pane(pane, action == "scroll-backward");
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
            "refresh" => self.refresh_selection(),
            "append" | "context-play" => {
                if action == "context-play"
                    && pane == 2
                    && self.browser.action_ids(pane, &id).len() == 1
                {
                    self.browse_action_impl("play", pane, id);
                    return;
                }
                let target = self.browser.action_target(pane, &id);
                self.start_container_queue(
                    target,
                    self.browser.action_order(pane),
                    action == "append",
                );
            }
            "play" => {
                if self.browser.view == 4 {
                    if self.browser.pending
                        || self.route_pending.is_some()
                        || self.automatic_song_search
                    {
                        return;
                    }
                    let playlist = if pane == 0 {
                        id.clone()
                    } else {
                        self.browser.artist.clone()
                    };
                    let playlists = if pane == 0 {
                        vec![playlist]
                    } else {
                        self.browser.selections[0]
                            .ids
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                    };
                    let start = (pane == 2).then_some(id);
                    let reader = match self.session.library.library_queue_reader() {
                        Ok(r) => r,
                        Err(e) => {
                            self.browser.error = e.to_string();
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
                    self.browser.worker = Some(std::thread::spawn(move || {
                        let result = reader
                            .read_playlists(&playlists, start.as_deref())
                            .map_err(|e| e.to_string())
                            .map(|(tracks, position)| {
                                if append && start.is_some() {
                                    (tracks.into_iter().skip(position).take(1).collect(), 0)
                                } else {
                                    (tracks, position)
                                }
                            });
                        deliver(result);
                    }));
                    self.browse_changed();
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
                if pane == 2 && action == "play" {
                    request = self.browser.pane_request(2);
                }
                request.album_sort = self.browser.album_program_sort();
                request.sort = self.browser.song_sort(
                    request.album.is_some() || !request.albums.is_empty(),
                    request.artist.is_some()
                        || request.genre.is_some()
                        || !request.artists.is_empty()
                        || !request.genres.is_empty(),
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

    fn request_playlist_view(&mut self, cursor: Option<Cursor>, reverse: Option<bool>) {
        self.browser.playlist_view_generation =
            self.browser.playlist_view_generation.wrapping_add(1);
        self.browser.playlist_view_pending = Some(PlaylistViewRequest {
            generation: self.browser.playlist_view_generation,
            playlists: self.browser.selections[0].ids.iter().cloned().collect(),
            sort: self.browser.playlist_sort,
            cursor,
            reverse,
        });
        self.start_playlist_view();
    }
    fn start_playlist_view(&mut self) {
        if self.browser.playlist_view_worker.is_some() {
            return;
        }
        let Some(request) = self.browser.playlist_view_pending.take() else {
            return;
        };
        let reader = match self
            .browser
            .playlist_view_reader
            .take()
            .map(Ok)
            .unwrap_or_else(|| self.session.library.library_queue_reader())
        {
            Ok(reader) => reader,
            Err(e) => {
                self.browser.error = e.to_string();
                return;
            }
        };
        let generation = request.generation;
        let reverse = request.reverse;
        let pointer = QPointer::from(&*self);
        let deliver = qmetaobject::queued_callback(
            move |(reader, result): (
                music_library::browse::QueueReader,
                Result<Vec<Row>, String>,
            )| {
                if let Some(object) = pointer.as_pinned() {
                    let mut b = object.borrow_mut();
                    if let Some(worker) = b.browser.playlist_view_worker.take() {
                        let _ = worker.join();
                    }
                    b.browser.playlist_view_reader = Some(reader);
                    if b.browser.view == 4 && generation == b.browser.playlist_view_generation {
                        match result {
                            Ok(mut rows) => {
                                let more = rows.len() > PAGE;
                                rows.truncate(PAGE);
                                let page = &mut b.browser.pages[2];
                                match reverse {
                                    None => {
                                        page.rows = rows;
                                        page.more = more;
                                    }
                                    Some(true) => {
                                        rows.reverse();
                                        rows.append(&mut page.rows);
                                        page.before = more;
                                        if rows.len() > WINDOW {
                                            rows.truncate(WINDOW);
                                            page.more = true;
                                        }
                                        page.rows = rows;
                                    }
                                    Some(false) => {
                                        page.rows.append(&mut rows);
                                        page.more = more;
                                        if page.rows.len() > WINDOW {
                                            let discard = page.rows.len() - WINDOW;
                                            page.rows.drain(..discard);
                                            page.before = true;
                                        }
                                    }
                                }
                                b.browser.error.clear();
                            }
                            Err(e) => b.browser.error = e,
                        }
                    }
                    b.start_playlist_view();
                    b.browse_changed();
                }
            },
        );
        self.browser.playlist_view_worker = Some(std::thread::spawn(move || {
            let result = reader
                .playlist_view(
                    &request.playlists,
                    request.sort,
                    request.cursor.as_ref(),
                    201,
                    request.reverse.unwrap_or(false),
                )
                .map_err(|e| e.to_string());
            deliver((reader, result));
        }));
    }

    fn scroll_pane(&mut self, pane: usize, reverse: bool) {
        let page = &self.browser.pages[pane];
        if (reverse && !page.before) || (!reverse && !page.more) || page.rows.is_empty() {
            return;
        }
        let cursor = if reverse {
            page.rows.first()
        } else {
            page.rows.last()
        }
        .map(|r| r.cursor.clone())
        .expect("nonempty window");
        if self.browser.view == 4
            && pane == 2
            && self.browser.playlist_sort.column != music_library::playlist::Column::Position
        {
            if self.browser.playlist_view_worker.is_none() {
                self.request_playlist_view(Some(cursor), Some(reverse));
            }
            return;
        }
        let request = Request {
            after: Some(cursor.clone()),
            limit: 201,
            ..self.browser.pane_request(pane)
        };
        let playlists = self.browser.selections[0]
            .ids
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let result = if self.browser.view == 4 {
            match (pane, reverse) {
                (0, true) => self.session.library.playlists_before(&cursor, 201),
                (0, false) => self.session.library.playlists(Some(&cursor), 201),
                (_, reverse) => self.session.library.playlist_view(
                    &playlists,
                    self.browser.playlist_sort,
                    Some(&cursor),
                    201,
                    reverse,
                ),
            }
        } else if reverse {
            self.session.library.browse_before(&request)
        } else {
            self.session.library.browse(&request)
        };
        match result {
            Ok(mut rows) => {
                let more = rows.len() > PAGE;
                rows.truncate(PAGE);
                let page = &mut self.browser.pages[pane];
                if reverse {
                    rows.reverse();
                    rows.append(&mut page.rows);
                    page.before = more;
                    if rows.len() > WINDOW {
                        rows.truncate(WINDOW);
                        page.more = true;
                    }
                    page.rows = rows;
                } else {
                    page.rows.append(&mut rows);
                    page.more = more;
                    if page.rows.len() > WINDOW {
                        page.rows.drain(..page.rows.len() - WINDOW);
                        page.before = true;
                    }
                }
                self.browser.error.clear();
            }
            Err(e) => self.browser.error = e.to_string(),
        }
    }

    fn load_pane(&mut self, pane: usize, reset: bool) {
        if (self.browser.view == 4 && (pane == 1 || (pane == 2 && self.browser.artist.is_empty())))
            || (pane == 0 && self.browser.view >= 2 && self.browser.view != 4)
            || (pane == 1 && self.browser.view == 3)
        {
            if self.browser.view == 4 && pane == 2 {
                self.browser.playlist_view_generation =
                    self.browser.playlist_view_generation.wrapping_add(1);
                self.browser.playlist_view_pending = None;
            }
            self.browser.pages[pane] = Page::default();
            return;
        }
        let page = &mut self.browser.pages[pane];
        if reset || page.cursors.is_empty() {
            page.cursors = vec![None];
            page.seek = None;
            page.before = false;
            page.epoch = page.epoch.wrapping_add(1);
            page.scroll_id.clear();
            page.scroll_pixel = 0.0;
        }
        let mut request = self.browser.pane_request(pane);
        request.after = self.browser.pages[pane].cursors.last().cloned().flatten();
        request.limit = 201;
        if self.browser.view == 4
            && pane == 2
            && self.browser.playlist_sort.column != music_library::playlist::Column::Position
        {
            self.browser.pages[pane].rows.clear();
            self.browser.pages[pane].more = false;
            self.request_playlist_view(request.after, None);
            return;
        }
        if self.browser.view == 4 && pane == 2 {
            self.browser.playlist_view_generation =
                self.browser.playlist_view_generation.wrapping_add(1);
            self.browser.playlist_view_pending = None;
        }
        let page = &mut self.browser.pages[pane];
        let result = if self.browser.view == 4 {
            if pane == 0 {
                self.session.library.playlists(request.after.as_ref(), 201)
            } else {
                self.session.library.playlist_view(
                    &self.browser.selections[0]
                        .ids
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>(),
                    self.browser.playlist_sort,
                    request.after.as_ref(),
                    201,
                    false,
                )
            }
        } else if page.cursors.len() == 1
            && let Some(id) = &page.seek
        {
            match self.session.library.browse_around(&request, id) {
                Ok(rows) => Ok(rows),
                Err(_) => {
                    // The preserved anchor may have been removed by a durable edit.
                    page.seek = None;
                    page.before = false;
                    page.scroll_id.clear();
                    self.session.library.browse(&request)
                }
            }
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
                before: !target.is_empty(),
                epoch: self.browser.pages[pane].epoch.wrapping_add(1),
                ..Default::default()
            });
        }
        self.browser.pages = pages
            .try_into()
            .map_err(|_| "Invalid navigation".to_string())?;
        self.browser.artist = artist;
        self.browser.album = album;
        self.browser.song = song;
        self.browser.selections[0].single(&self.browser.artist);
        self.browser.selections[1].single(&self.browser.album);
        self.browser.selections[2].single(&self.browser.song);

        self.browser.navigation = self.browser.navigation.wrapping_add(1);
        self.browser.error.clear();
        self.browse_changed();
        Ok(())
    }

    fn refresh_playlist_details(&mut self, force: bool) {
        let id = (self.browser.view == 4 && self.browser.selections[0].ids.len() == 1).then(|| {
            self.browser.selections[0]
                .ids
                .iter()
                .next()
                .unwrap()
                .clone()
        });
        if !force && id == self.browser.playlist_details_id {
            return;
        }
        self.browser.playlist_details_generation =
            self.browser.playlist_details_generation.wrapping_add(1);
        if id != self.browser.playlist_details_id {
            self.browser.playlist_details = None;
        }
        self.browser.playlist_details_id = id.clone();
        self.browser.playlist_details_pending =
            id.map(|id| (self.browser.playlist_details_generation, id));
        self.start_playlist_details();
    }

    fn start_playlist_details(&mut self) {
        if self.browser.playlist_details_worker.is_some() {
            return;
        }
        let Some((generation, id)) = self.browser.playlist_details_pending.take() else {
            return;
        };
        let reader = match self.session.library.library_queue_reader() {
            Ok(reader) => reader,
            Err(e) => {
                self.browser.error = e.to_string();
                return;
            }
        };
        let pointer = QPointer::from(&*self);
        let deliver = qmetaobject::queued_callback(
            move |result: Result<Option<music_library::playlist::Details>, String>| {
                if let Some(object) = pointer.as_pinned() {
                    let mut b = object.borrow_mut();
                    if let Some(worker) = b.browser.playlist_details_worker.take() {
                        let _ = worker.join();
                    }
                    if generation == b.browser.playlist_details_generation {
                        match result {
                            Ok(details) => b.browser.playlist_details = details,
                            Err(e) => b.browser.error = e,
                        }
                    }
                    // Coalesce rapid selections/edits to one active and one latest request.
                    b.start_playlist_details();
                    b.browse_changed();
                }
            },
        );
        self.browser.playlist_details_worker = Some(std::thread::spawn(move || {
            deliver(reader.playlist_details(&id).map_err(|e| e.to_string()));
        }));
    }

    fn playlist_details_value(&self) -> QVariantMap {
        let Some(id) = &self.browser.playlist_details_id else {
            return QVariantMap::default();
        };
        let details = self.browser.playlist_details.as_ref();
        let name = details
            .map(|d| d.name.as_str())
            .or_else(|| {
                self.browser.pages[0]
                    .rows
                    .iter()
                    .find(|r| &r.id == id)
                    .map(|r| r.title.as_str())
            })
            .unwrap_or("");
        let duration = details
            .map(|d| {
                if d.entry_count > 0 && d.unknown_duration_count == d.entry_count {
                    "Unknown".into()
                } else if d.unknown_duration_count > 0 {
                    format!(
                        "{}{} (partial; {} unknown)",
                        if d.approximate_duration_count > 0 {
                            "≈ "
                        } else {
                            ""
                        },
                        crate::time_label(Some(d.known_duration_ms)),
                        d.unknown_duration_count
                    )
                } else {
                    format!(
                        "{}{}",
                        if d.approximate_duration_count > 0 {
                            "≈ "
                        } else {
                            ""
                        },
                        crate::time_label(Some(d.known_duration_ms))
                    )
                }
            })
            .unwrap_or_default();
        [
            ("name", string(name)),
            (
                "count",
                string(
                    details
                        .map(|d| d.entry_count.to_string())
                        .unwrap_or_default(),
                ),
            ),
            ("duration", string(duration)),
            (
                "pending",
                (self.browser.playlist_details_worker.is_some()
                    || self.browser.playlist_details_pending.is_some())
                .into(),
            ),
        ]
        .into_iter()
        .collect()
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
                            ("genres", string(&r.genres)),
                            (
                                "length",
                                string(format!(
                                    "{}{}",
                                    if r.duration_approximate { "≈ " } else { "" },
                                    crate::time_label(r.duration_ms)
                                )),
                            ),
                            (
                                "durationMs",
                                r.duration_ms.map(|v| v as i64).unwrap_or(-1).into(),
                            ),
                            (
                                "year",
                                string(r.year.map(|y| y.to_string()).unwrap_or_default()),
                            ),
                            ("group", string(&r.group)),
                            ("groupLabel", string(&r.group_label)),
                            // Songs in Album mode already carry canonical grouping in their cursor.
                            (
                                "albumId",
                                string(if pane == 2 { &r.cursor.album_key } else { "" }),
                            ),
                            (
                                "number",
                                string(if pane == 2 && self.browser.view == 4 {
                                    r.playlist_position
                                        .map(|n| n.to_string())
                                        .unwrap_or_default()
                                } else if pane == 2 && self.browser.sort(2) == Sort::Album {
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
                        (
                            "selectedIds",
                            QVariant::from(
                                p.rows
                                    .iter()
                                    .filter(|r| self.browser.selections[pane].ids.contains(&r.id))
                                    .map(|r| string(&r.id))
                                    .collect::<QVariantList>(),
                            ),
                        ),
                        (
                            "selectionCount",
                            (self.browser.selections[pane].ids.len() as u32).into(),
                        ),
                        (
                            "sort",
                            string(if self.browser.view == 4 {
                                ""
                            } else {
                                self.browser.sort_label(pane)
                            }),
                        ),
                        ("more", p.more.into()),
                        ("before", p.before.into()),
                        ("epoch", p.epoch.into()),
                        ("scrollId", string(&p.scroll_id)),
                        ("scrollPixel", p.scroll_pixel.into()),
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
                "duplicateMessage",
                string(
                    self.browser
                        .duplicate_plan
                        .as_ref()
                        .map(|p| {
                            if p.tracks.len() == 1 {
                                format!("This song is already in {}. Add another copy?", p.name)
                            } else {
                                format!(
                                    "{} of {} tracks are already in {}. Add duplicate copies?",
                                    p.duplicate_entries,
                                    p.tracks.len(),
                                    p.name
                                )
                            }
                        })
                        .unwrap_or_default(),
                ),
            ),
            (
                "playlistChoices",
                QVariant::from(
                    self.browser
                        .picker
                        .rows
                        .iter()
                        .map(|r| {
                            QVariant::from(
                                [("id", string(&r.id)), ("name", string(&r.title))]
                                    .into_iter()
                                    .collect::<QVariantMap>(),
                            )
                        })
                        .collect::<QVariantList>(),
                ),
            ),
            ("playlistChoicesMore", self.browser.picker.more.into()),
            (
                "playlistChoicesPrevious",
                (self.browser.picker.cursors.len() > 1).into(),
            ),
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
            (
                "playlistSort",
                string(match self.browser.playlist_sort.column {
                    music_library::playlist::Column::Position => "position",
                    music_library::playlist::Column::Title => "title",
                    music_library::playlist::Column::Artist => "artist",
                    music_library::playlist::Column::Album => "album",
                    music_library::playlist::Column::Length => "length",
                }),
            ),
            (
                "playlistDescending",
                self.browser.playlist_sort.descending.into(),
            ),
            (
                "playlistReorderAllowed",
                self.browser.playlist_sort.allows_reordering().into(),
            ),
            (
                "songsColumn",
                string(match self.browser.songs_column {
                    music_library::browse::SongColumn::Song => "song",
                    music_library::browse::SongColumn::Artist => "artist",
                    music_library::browse::SongColumn::Album => "album",
                    music_library::browse::SongColumn::Genre => "genre",
                }),
            ),
            (
                "songsDescending",
                (self.browser.global_songs_sort != 0).into(),
            ),
            ("playlistDetails", self.playlist_details_value().into()),
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
            (
                "pending",
                (self.browser.pending
                    || (self.browser.view == 4
                        && (self.browser.playlist_view_worker.is_some()
                            || self.browser.playlist_view_pending.is_some())))
                .into(),
            ),
        ]
        .into_iter()
        .collect()
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        if let Some(worker) = self.playlist_view_worker.take() {
            let _ = worker.join();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if let Some(worker) = self.playlist_details_worker.take() {
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

impl Browser {
    fn sync_selection_focus(&mut self) {
        let primary = |pane: usize| self.selections[pane].focus.clone().unwrap_or_default();
        if self.view == 1 {
            self.genre = primary(0);
            self.artist.clear();
        } else {
            self.artist = primary(0);
            self.genre.clear();
        }
        self.album = primary(1);
        self.song = primary(2);
        self.normalize_sorts();
    }
    fn pane_request(&self, pane: usize) -> Request {
        let ids = |p: usize| self.selections[p].ids.iter().cloned().collect::<Vec<_>>();
        Request {
            song_column: if self.view == 3 && pane == 2 {
                Some(self.songs_column)
            } else {
                None
            },
            pane: if pane == 0 && self.view == 1 {
                Pane::Genres
            } else {
                [Pane::Artists, Pane::Albums, Pane::Songs][pane]
            },
            artists: if pane > 0 && self.view == 0 {
                ids(0)
            } else {
                vec![]
            },
            genres: if pane > 0 && self.view == 1 {
                ids(0)
            } else {
                vec![]
            },
            albums: if pane == 2 { ids(1) } else { vec![] },
            sort: self.sort(pane),
            album_sort: self.album_program_sort(),
            limit: 201,
            ..Default::default()
        }
    }
    fn action_ids(&self, pane: usize, fallback: &str) -> Vec<String> {
        let selected = &self.selections[pane].ids;
        if selected.contains(fallback) || fallback.is_empty() {
            selected.iter().cloned().collect()
        } else {
            vec![fallback.to_string()]
        }
    }
    fn action_target(&self, pane: usize, fallback: &str) -> music_library::track_container::Target {
        use music_library::track_container::Target;
        let ids = self.action_ids(pane, fallback);
        match (self.view, pane) {
            (4, 0) => Target::Playlists(ids),
            (4, 2) => Target::PlaylistEntries {
                playlists: self.selections[0].ids.iter().cloned().collect(),
                entries: ids,
            },
            (1, 0) => Target::Genres(ids),
            (_, 0) => Target::Artists(ids),
            (_, 1) => Target::Albums(ids),
            _ => Target::Songs(ids),
        }
    }
    fn action_order(&self, pane: usize) -> Request {
        Request {
            sort: if pane == 2 {
                self.sort(2)
            } else {
                self.song_sort(pane == 1, pane == 0)
            },
            album_sort: self.album_program_sort(),
            ..Default::default()
        }
    }
}
impl Bridge {
    fn refresh_selection(&mut self) {
        self.refresh_playlist_details(true);
        // Durable edits/import/removal can change inactive views too. Retain an
        // identity anchor, but never return to a stale materialized window.
        for state in &mut self.browser.views {
            for page in &mut state.pages {
                let target = if page.scroll_id.is_empty() {
                    page.rows.first().map(|r| r.id.clone())
                } else {
                    Some(page.scroll_id.clone())
                };
                page.rows.clear();
                page.cursors = vec![None];
                page.seek = target;
                page.before = page.seek.is_some();
                page.epoch = page.epoch.wrapping_add(1);
            }
        }
        if self.browser.view < 2 {
            match self
                .session
                .library
                .library_queue_reader()
                .and_then(|reader| {
                    reader.browse_ids(
                        &self.browser.pane_request(0),
                        &self.browser.selections[0]
                            .ids
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>(),
                    )
                }) {
                Ok(ids) => self.browser.selections[0].retain(&ids.into_iter().collect()),
                Err(e) => self.browser.error = e.to_string(),
            }
        }
        self.browser.sync_selection_focus();
        self.load_pane(0, true);
        self.selection_changed(0);
    }
    fn select_items(&mut self, action: &str, pane: usize, id: String) {
        self.browser.selection_revision = self.browser.selection_revision.wrapping_add(1);
        if action == "select-range" && !id.is_empty() {
            let Some(anchor) = self.browser.selections[pane].anchor.clone() else {
                self.select_items("select", pane, id);
                return;
            };
            if self.browser.pending {
                return;
            }
            let reader = match self.session.library.library_queue_reader() {
                Ok(r) => r,
                Err(e) => {
                    self.browser.error = e.to_string();
                    return;
                }
            };
            let request = self.browser.pane_request(pane);
            let playlists = self.browser.selections[0]
                .ids
                .iter()
                .cloned()
                .collect::<Vec<_>>();
            let view = self.browser.view;
            let playlist_sort = self.browser.playlist_sort;
            let revision = self.browser.selection_revision;
            let target = id.clone();
            let pointer = QPointer::from(&*self);
            let deliver =
                qmetaobject::queued_callback(move |result: Result<Vec<String>, String>| {
                    if let Some(object) = pointer.as_pinned() {
                        let mut b = object.borrow_mut();
                        b.browser.pending = false;
                        if let Some(w) = b.browser.worker.take() {
                            let _ = w.join();
                        }
                        if b.browser.selection_revision == revision {
                            match result {
                                Ok(ids) => {
                                    b.browser.selections[pane].range(ids, &target);
                                    b.selection_changed(pane);
                                }
                                Err(e) => b.browser.error = e,
                            }
                        }
                        b.browse_changed();
                    }
                });
            self.browser.pending = true;
            self.browser.worker = Some(std::thread::spawn(move || {
                let result = if view == 4 && pane == 0 {
                    reader.playlists_range(&anchor, &id)
                } else if view == 4 {
                    reader.playlist_view_range(&playlists, playlist_sort, &anchor, &id)
                } else {
                    reader.range(&request, &anchor, &id)
                };
                deliver(result.map_err(|e| e.to_string()));
            }));
            return;
        }
        if action == "context" && !self.browser.selections[pane].context(&id) {
            return;
        } else if action == "select-toggle" {
            self.browser.selections[pane].toggle(&id);
        } else if action != "context" {
            self.browser.selections[pane].single(&id);
        }
        self.selection_changed(pane);
    }
    fn selection_changed(&mut self, pane: usize) {
        self.browser.sync_selection_focus();
        if pane == 0 {
            self.refresh_playlist_details(false);
        }
        if pane < 2 {
            if self.prune_large_selection(pane) {
                return;
            }
            // One indexed query per downstream pane, limited to previously selected IDs.
            // Off-page valid selections survive paging and filter changes.
            if let Ok(reader) = self.session.library.library_queue_reader() {
                for downstream in pane + 1..3 {
                    let mut selected = self.browser.selections[downstream]
                        .ids
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>();
                    if let Some(anchor) = &self.browser.selections[downstream].anchor {
                        selected.push(anchor.clone());
                    }
                    let valid = if self.browser.view == 4 {
                        if downstream == 1 {
                            Ok(vec![])
                        } else {
                            reader.visible_playlist_entry_ids(
                                &self.browser.selections[0]
                                    .ids
                                    .iter()
                                    .cloned()
                                    .collect::<Vec<_>>(),
                                &selected,
                            )
                        }
                    } else {
                        reader.browse_ids(&self.browser.pane_request(downstream), &selected)
                    };
                    match valid {
                        Ok(ids) => {
                            self.browser.selections[downstream].retain(&ids.into_iter().collect())
                        }
                        Err(e) => self.browser.error = e.to_string(),
                    }
                    self.browser.sync_selection_focus();
                    self.load_pane(downstream, true);
                }
            }
        }
    }
    fn prune_large_selection(&mut self, pane: usize) -> bool {
        if self.browser.pending
            || self.browser.selections[pane + 1..]
                .iter()
                .map(|s| s.ids.len())
                .sum::<usize>()
                <= 1000
        {
            return false;
        }
        let reader = match self.session.library.library_queue_reader() {
            Ok(r) => r,
            Err(e) => {
                self.browser.error = e.to_string();
                return false;
            }
        };
        let mut selections = self.browser.selections.clone();
        let album_request = self.browser.pane_request(1);
        let mut song_request = self.browser.pane_request(2);
        let view = self.browser.view;
        let revision = self.browser.selection_revision;
        let pointer = QPointer::from(&*self);
        let deliver = qmetaobject::queued_callback(
            move |result: Result<[music_library::selection::Selection; 3], String>| {
                if let Some(object) = pointer.as_pinned() {
                    let mut b = object.borrow_mut();
                    b.browser.pending = false;
                    if let Some(w) = b.browser.worker.take() {
                        let _ = w.join();
                    }
                    if b.browser.selection_revision == revision {
                        match result {
                            Ok(selections) => {
                                b.browser.selections = selections;
                                b.browser.sync_selection_focus();
                                for d in pane + 1..3 {
                                    b.load_pane(d, true);
                                }
                            }
                            Err(e) => b.browser.error = e,
                        }
                    } else if b.browser.view == view {
                        b.selection_changed(pane);
                    }

                    b.browse_changed();
                }
            },
        );
        self.browser.pending = true;
        self.browser.worker = Some(std::thread::spawn(move || {
            let result = (|| {
                for downstream in pane + 1..3 {
                    let mut selected = selections[downstream]
                        .ids
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>();
                    if let Some(anchor) = &selections[downstream].anchor {
                        selected.push(anchor.clone());
                    }
                    let valid = if view == 4 {
                        if downstream == 1 {
                            vec![]
                        } else {
                            reader.visible_playlist_entry_ids(
                                &selections[0].ids.iter().cloned().collect::<Vec<_>>(),
                                &selected,
                            )?
                        }
                    } else {
                        reader.browse_ids(
                            if downstream == 1 {
                                &album_request
                            } else {
                                &song_request
                            },
                            &selected,
                        )?
                    };
                    selections[downstream].retain(&valid.into_iter().collect());
                    if downstream == 1 {
                        song_request.albums = selections[1].ids.iter().cloned().collect();
                    }
                }
                Ok::<_, music_library::Error>(selections)
            })();
            deliver(result.map_err(|e| e.to_string()));
        }));
        for downstream in pane + 1..3 {
            self.load_pane(downstream, true);
        }
        true
    }
    fn start_container_queue(
        &mut self,
        target: music_library::track_container::Target,
        order: Request,
        append: bool,
    ) {
        if self.browser.pending
            || (!append && (self.route_pending.is_some() || self.automatic_song_search))
        {
            return;
        }
        let reader = match self.session.library.library_queue_reader() {
            Ok(r) => r,
            Err(e) => {
                self.browser.error = e.to_string();
                return;
            }
        };
        if append
            && matches!(&target,music_library::track_container::Target::Songs(ids) if ids.len()==1)
        {
            let result = reader
                .resolve(&target, &order)
                .map(|rows| (rows, 0))
                .map_err(|e| e.to_string());
            self.finish_library_queue(true, result);
            return;
        }
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
                    .resolve(&target, &order)
                    .map(|tracks| (tracks, 0))
                    .map_err(|e| e.to_string()),
            );
        }));
    }
    fn prepare_selection_append(&mut self, destination: String) {
        if self.browser.pending || self.browser.duplicate_plan.is_some() {
            return;
        }
        let Some(target) = self.browser.picker_target.clone() else {
            return;
        };
        let reader = match self.session.library.library_queue_reader() {
            Ok(r) => r,
            Err(e) => {
                self.browser.error = e.to_string();
                self.browse_changed();
                return;
            }
        };
        let mut worker = match self.session.library.playlist_append_worker() {
            Ok(w) => w,
            Err(e) => {
                self.browser.error = e.to_string();
                self.browse_changed();
                return;
            }
        };
        let order = self.browser.picker_order.clone();
        let pointer = QPointer::from(&*self);
        let deliver = qmetaobject::queued_callback(
            move |result: Result<Option<music_library::playlist::AppendPlan>, String>| {
                if let Some(object) = pointer.as_pinned() {
                    let mut b = object.borrow_mut();
                    b.browser.pending = false;
                    if let Some(w) = b.browser.worker.take() {
                        let _ = w.join();
                    }
                    match result {
                        Ok(Some(plan)) => b.browser.duplicate_plan = Some(plan),
                        Ok(None) => {
                            if b.browser.view == 4 {
                                b.load_pane(2, true);
                                b.refresh_playlist_details(true);
                            }
                        }
                        Err(e) => b.browser.error = e,
                    }
                    b.browse_changed();
                }
            },
        );
        self.browser.pending = true;
        self.browser.error.clear();
        self.browser.worker = Some(std::thread::spawn(move || {
            let result = (|| {
                let tracks = reader
                    .resolve(&target, &order)?
                    .into_iter()
                    .map(|r| r.track_id)
                    .collect();
                let plan = worker.prepare(&destination, tracks)?;
                if plan.duplicate_entries > 0 {
                    Ok(Some(plan))
                } else {
                    worker.apply(&plan, true)?;
                    Ok(None)
                }
            })();
            deliver(result.map_err(|e: music_library::Error| e.to_string()));
        }));
        self.browse_changed();
    }
    pub(crate) fn commit_selection_append(
        &mut self,
        plan: music_library::playlist::AppendPlan,
        include_duplicates: bool,
    ) {
        if self.browser.pending {
            return;
        }
        let mut worker = match self.session.library.playlist_append_worker() {
            Ok(w) => w,
            Err(e) => {
                self.browser.error = e.to_string();
                self.browse_changed();
                return;
            }
        };
        let pointer = QPointer::from(&*self);
        let deliver = qmetaobject::queued_callback(move |result: Result<usize, String>| {
            if let Some(object) = pointer.as_pinned() {
                let mut b = object.borrow_mut();
                b.browser.pending = false;
                if let Some(w) = b.browser.worker.take() {
                    let _ = w.join();
                }
                match result {
                    Ok(_) => {
                        if b.browser.view == 4 {
                            b.load_pane(2, true);
                            b.refresh_playlist_details(true);
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
                    .apply(&plan, include_duplicates)
                    .map_err(|e| e.to_string()),
            )
        }));
        self.browse_changed();
    }
}
