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
    playlist_revisions: Vec<(String, i64)>,
    loaded: bool,
}
type PlaylistViewResult = Result<(Vec<Row>, Vec<String>), String>;

struct PlaylistViewRequest {
    generation: u64,
    playlists: Vec<String>,
    sort: music_library::playlist::ViewSort,
    cursor: Option<Cursor>,
    reverse: Option<bool>,
    refresh: Option<(Cursor, usize, bool)>,
    revisions: Vec<(String, i64)>,
    selection: Vec<String>,
}
#[derive(Default)]
pub struct Browser {
    pub(crate) queue_explicit: Vec<String>,
    pub(crate) queue_explicit_start_only: bool,
    preference_rows: Vec<Row>,
    preference_cursor: Option<Cursor>,
    preference_hidden: bool,
    preference_more: bool,
    context_hidden: bool,
    context_ignored: bool,
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
    views: [ViewState; 6],
    unresolved_count: Option<u64>,
    marked_count: Option<u64>,
    review_marked: bool,
    pub(crate) review_local_active: bool,
    pub(crate) review_retry_requested: bool,
    pub(crate) review_run_active: bool,
    pub(crate) review_run_automatic: bool,
    review_startup_checked: bool,
    review_auto_requested: bool,
    review_auto_scheduled: bool,
    pub(crate) review_provider_blocked: bool,
    review_run_batches: usize,
    review_catalog_names_attempted: std::sync::Arc<
        std::sync::Mutex<std::collections::HashSet<music_library::domain::ArtistId>>,
    >,
    review_run_started: Option<std::time::Instant>,
    review_first_progress_ms: Option<u128>,
    review_first_transition_ms: Option<u128>,
    review_local_totals: [usize; 3],
    review_progress_at: Option<std::time::Instant>,
    review_message: String,
    review_retry_message: String,
    review_retry_after: Option<music_library::domain::AlbumId>,
    pub(crate) review_retry_albums: Vec<music_library::domain::AlbumId>,
    pub(crate) review_retry_playable_albums: Vec<music_library::domain::AlbumId>,
    pub(crate) review_retry_playable_started: bool,
    pub(crate) review_playable_searches: usize,
    #[cfg(test)]
    review_test_async_fallback: bool,
    pub(crate) review_retry_album_started: bool,
    pub(crate) review_retry_local_started: bool,
    pub(crate) review_retry_local_active: bool,
    pub(crate) review_retry_prepared: bool,
    pub(crate) review_retry_preparing: Option<(Vec<AlbumId>, Option<AlbumId>)>,
    pub(crate) review_retry_force: std::collections::HashSet<AlbumId>,
    pub(crate) review_retry_single: bool,
    pub(crate) review_retry_seen: std::collections::HashSet<AlbumId>,
    review_retry_page_after: Option<AlbumId>,
    pub(crate) review_retry_extra: Vec<AlbumId>,
    review_retry_artists:
        std::collections::VecDeque<(music_library::domain::ArtistId, Option<AlbumId>)>,
    review_retry_generation: u64,
    pub(crate) review_retry_totals: [u64; 5],
    review_count_pending: bool,
    review_count_dirty: bool,
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
        if action.starts_with("preference-") {
            use music_library::preferences::IgnoreTarget;
            let result = (|| -> music_library::Result<()> {
                match action {
                    "preference-context" => {
                        self.browser.context_hidden = pane == 0
                            && self.session.library.artist_hidden(&ArtistId(id.clone()))?;
                        let target = match pane {
                            0 => IgnoreTarget::Artist(id.clone()),
                            1 => IgnoreTarget::Album(id.clone()),
                            _ => IgnoreTarget::Track(id.clone()),
                        };
                        let (total, ignored) = self.session.library.ignore_counts(&target)?;
                        self.browser.context_ignored = total > 0 && total == ignored;
                    }
                    "preference-hide" | "preference-unhide" => self
                        .session
                        .library
                        .set_artist_hidden(&ArtistId(id.clone()), action == "preference-hide")?,
                    "preference-ignore" | "preference-unignore" => {
                        let target = match pane {
                            0 => IgnoreTarget::Artist(id.clone()),
                            1 => IgnoreTarget::Album(id.clone()),
                            _ => IgnoreTarget::Track(id.clone()),
                        };
                        self.session
                            .library
                            .set_tracks_ignored(&target, action == "preference-ignore")?;
                    }
                    "preference-hidden" | "preference-ignored" => {
                        self.browser.preference_hidden = action == "preference-hidden";
                        self.browser.preference_cursor = None;
                    }
                    "preference-next" => {
                        self.browser.preference_cursor = self
                            .browser
                            .preference_rows
                            .last()
                            .map(|r| r.cursor.clone())
                    }
                    "preference-first" => self.browser.preference_cursor = None,
                    _ => {}
                }
                if action != "preference-context" {
                    let request = Request {
                        pane: if self.browser.preference_hidden {
                            Pane::Artists
                        } else {
                            Pane::Songs
                        },
                        hidden_artists_only: self.browser.preference_hidden,
                        ignored_tracks_only: !self.browser.preference_hidden,
                        after: self.browser.preference_cursor.clone(),
                        limit: 201,
                        ..Default::default()
                    };
                    let mut rows = self.session.library.browse(&request)?;
                    self.browser.preference_more = rows.len() > 200;
                    rows.truncate(200);
                    self.browser.preference_rows = rows;
                    for state in &mut self.browser.views {
                        for page in &mut state.pages {
                            *page = Page::default();
                        }
                    }
                    for pane in 0..3 {
                        self.load_pane(pane, true);
                    }
                }
                Ok(())
            })();
            if let Err(e) = result {
                self.browser.error = e.to_string();
            }
            self.browse_changed();
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
        if action == "songs-sort" && matches!(self.browser.view, 3 | 5) && pane == 2 {
            use music_library::browse::SongColumn;
            let column = match id.as_str() {
                "song" => SongColumn::Song,
                "artist" => SongColumn::Artist,
                "album" => SongColumn::Album,
                "genre" => SongColumn::Genre,
                "reason" if self.browser.view == 5 => SongColumn::Reason,
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
        if action == "playlist-revision-check" {
            self.refresh_changed_playlist();
            self.browse_changed();
            return;
        }
        if action == "playlist-reconcile" {
            let automatic = self.auto_match;
            self.auto_match = true;
            self.reconcile_spotify_playlist(&id);
            self.auto_match = automatic;
            self.changed();
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
                    if action == "playlist-rename" {
                        self.load_pane(0, false);
                    } else {
                        self.refresh_selection_with_details(!matches!(
                            action,
                            "playlist-up" | "playlist-down"
                        ));
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
        if matches!(action, "spotify-mark-selection" | "spotify-check-selection") {
            if self.browser.view != 5 || pane != 2 {
                return;
            }
            let tracks: Vec<TrackId> = self
                .browser
                .action_ids(2, &id)
                .into_iter()
                .map(TrackId)
                .collect();
            let marking = action == "spotify-mark-selection";
            let result = if marking {
                self.session.library.mark_tracks_not_on_spotify(&tracks)
            } else {
                self.session.library.check_tracks_spotify_again(&tracks)
            };
            match result {
                Ok(count) => {
                    self.browser.review_retry_message = if marking {
                        format!("Marked {count} Tracks as not on Spotify")
                    } else {
                        format!("Re-enabled Spotify checking for {count} Tracks")
                    };
                    if self
                        .spotify_playback_track
                        .as_ref()
                        .is_some_and(|t| tracks.contains(t))
                    {
                        self.spotify_resolution_generation += 1;
                        self.spotify_resolution_pending = false;
                        self.spotify_resolution_selection = None;
                        self.spotify_resolution_message = if marking {
                            "Manually marked not on Spotify"
                        } else {
                            "Spotify checking re-enabled"
                        }
                        .into();
                        self.spotify_playback_changed();
                    }
                }
                Err(e) => self.browser.error = e.to_string(),
            }
            if !marking {
                self.request_automatic_spotify_reconciliation();
            }
            self.refresh_spotify_review();
            self.browse_changed();
            return;
        }
        if matches!(action, "spotify-mark" | "spotify-check") {
            let track = music_library::domain::TrackId(id.clone());
            let result = if action == "spotify-mark" {
                self.session.library.mark_not_on_spotify(&track)
            } else {
                self.session.library.check_spotify_again(&track)
            };
            if let Err(e) = result {
                self.browser.error = e.to_string();
            } else if self.spotify_playback_track.as_ref() == Some(&track) {
                self.spotify_resolution_generation += 1;
                self.spotify_resolution_pending = false;
                self.spotify_resolution_selection = None;
                self.spotify_resolution_message = if action == "spotify-mark" {
                    "Manually marked not on Spotify"
                } else {
                    "Spotify checking re-enabled"
                }
                .into();
                self.spotify_playback_changed();
            }
            if action == "spotify-check" {
                self.request_automatic_spotify_reconciliation();
            }
            self.refresh_spotify_review();
            self.browse_changed();
            return;
        }
        if self.browser.view == 5 && action == "review-mode" {
            self.browser.review_marked = id == "marked";
            self.load_pane(2, true);
            self.request_review_count();
            self.browse_changed();
            return;
        }
        if self.browser.view == 5 && action == "review-retry" {
            self.retry_unresolved_spotify();
            self.browse_changed();
            return;
        }
        if self.browser.view == 5 && action == "review-album" {
            self.browser.selections[1].ids = if id.is_empty() {
                Default::default()
            } else {
                [id.clone()].into_iter().collect()
            };
            self.browser.album = id;
            self.load_pane(2, true);
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
                let Some(view) = [
                    "Artists",
                    "Genres",
                    "Albums",
                    "Songs",
                    "Playlists",
                    "Spotify Connections",
                ]
                .iter()
                .position(|v| *v == id) else {
                    return;
                };
                self.browser.switch_view(view);
                if view == 5 {
                    self.refresh_spotify_review();
                }
                self.refresh_changed_playlist();
                self.refresh_playlist_details(false);
                for i in 0..3 {
                    if if view == 4 && i == 2 {
                        !self.browser.pages[i].loaded
                    } else {
                        self.browser.pages[i].rows.is_empty()
                    } {
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
                let passive_artists = pane == 0 && self.browser.view == 0 && id.is_empty();
                self.select_items(action, pane, id);
                if passive_artists {
                    self.load_pane(0, true);
                }
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
            "refresh" => {
                self.refresh_selection();
                if !self.browser.review_startup_checked {
                    self.request_automatic_spotify_reconciliation();
                } else {
                    self.maybe_schedule_spotify_reconciliation();
                }
            }
            "spotify-reconcile-wake" => {
                self.browser.review_provider_blocked = false;
                self.request_automatic_spotify_reconciliation();
            }
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
                if self.browser.pending
                    || self.route_pending.is_some()
                    || self.automatic_song_search
                {
                    return;
                }
                self.browser.queue_explicit_start_only = pane == 2;
                self.browser.queue_explicit = if pane == 2 {
                    vec![
                        self.browser.pages[2]
                            .rows
                            .iter()
                            .find(|r| r.id == id)
                            .and_then(|r| r.track.as_ref())
                            .map(|t| t.track_id.0.clone())
                            .unwrap_or(id.clone()),
                    ]
                } else {
                    vec![]
                };
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
                    let explicit = self.browser.queue_explicit.clone();
                    let explicit_start_only = self.browser.queue_explicit_start_only;
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
                        deliver(result.and_then(|(tracks, position)| {
                            reader
                                .prepare_playback_program(
                                    tracks,
                                    position,
                                    &explicit,
                                    explicit_start_only,
                                )
                                .map_err(|e| e.to_string())
                        }));
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
                    let explicit = self.browser.queue_explicit.clone();
                    let explicit_start_only = self.browser.queue_explicit_start_only;
                    self.browser.worker = Some(std::thread::spawn(move || {
                        deliver(
                            reader
                                .read(&request)
                                .map_err(|e| e.to_string())
                                .and_then(|tracks| prepare_program(tracks, start.as_ref()))
                                .and_then(|(tracks, position)| {
                                    reader
                                        .prepare_playback_program(
                                            tracks,
                                            position,
                                            &explicit,
                                            explicit_start_only,
                                        )
                                        .map_err(|e| e.to_string())
                                }),
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
        let explicit = self.browser.queue_explicit.clone();
        match result {
            Err(error) => self.browser.error = error,
            Ok((tracks, _)) if append => {
                let start = self.session.playback.state().queue.len();
                for row in tracks {
                    self.session.playback.enqueue(row.track_id.clone());
                    self.session.queue_labels.push(row);
                }
                self.session.playback.mark_generated_from(start, &explicit);
                self.session.error.clear();
            }
            Ok(_) if self.route_pending.is_some() || self.automatic_song_search => {
                self.browser.error = "Playback is switching. Please try Play again.".into();
            }
            Ok((tracks, position)) => {
                if tracks.is_empty() {
                    self.clear_resolved_queue();
                } else {
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
            refresh: None,
            revisions: self.current_playlist_revisions(),
            selection: self.browser.selections[2].ids.iter().cloned().collect(),
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
        let revisions = request.revisions.clone();
        let selection = request.selection.clone();
        let page_limit = request
            .refresh
            .as_ref()
            .map_or(PAGE, |(_, count, _)| *count);
        let pointer = QPointer::from(&*self);
        let deliver = qmetaobject::queued_callback(
            move |(reader, result): (music_library::browse::QueueReader, PlaylistViewResult)| {
                if let Some(object) = pointer.as_pinned() {
                    let mut b = object.borrow_mut();
                    if let Some(worker) = b.browser.playlist_view_worker.take() {
                        let _ = worker.join();
                    }
                    b.browser.playlist_view_reader = Some(reader);
                    if b.browser.view == 4 && generation == b.browser.playlist_view_generation {
                        match result {
                            Ok((mut rows, valid)) => {
                                let valid: std::collections::HashSet<_> =
                                    valid.into_iter().collect();
                                let mut retained = b.browser.selections[2].ids.clone();
                                for id in &selection {
                                    if !valid.contains(id) {
                                        retained.remove(id);
                                    }
                                }
                                b.browser.selections[2].retain(&retained);
                                b.browser.sync_selection_focus();
                                let more = rows.len() > page_limit;
                                rows.truncate(page_limit);
                                let page = &mut b.browser.pages[2];
                                page.playlist_revisions = revisions.clone();
                                page.loaded = true;
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
            let result = if let Some((first, count, from_start)) = request.refresh {
                (|| {
                    let first = reader
                        .playlist_view_cursor(&request.playlists, request.sort, &first.id)?
                        .unwrap_or(first);
                    let previous = reader.playlist_view(
                        &request.playlists,
                        request.sort,
                        Some(&first),
                        1,
                        true,
                    )?;
                    let mut cursor = if from_start {
                        None
                    } else {
                        previous.first().map(|r| r.cursor.clone())
                    };
                    let mut all = Vec::new();
                    while all.len() <= count {
                        let mut rows = reader.playlist_view(
                            &request.playlists,
                            request.sort,
                            cursor.as_ref(),
                            ((count + 1 - all.len()).min(201)) as u32,
                            false,
                        )?;
                        if rows.is_empty() {
                            break;
                        }
                        cursor = rows.last().map(|r| r.cursor.clone());
                        all.append(&mut rows);
                    }
                    Ok(all)
                })()
                .map_err(|e: music_library::storage::Error| e.to_string())
            } else {
                reader
                    .playlist_view(
                        &request.playlists,
                        request.sort,
                        request.cursor.as_ref(),
                        201,
                        request.reverse.unwrap_or(false),
                    )
                    .map_err(|e| e.to_string())
            };
            let result = result.and_then(|rows| {
                reader
                    .visible_playlist_entry_ids(&request.playlists, &request.selection)
                    .map(|valid| (rows, valid))
                    .map_err(|e| e.to_string())
            });
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
                page.loaded = true;
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
            || (pane == 1 && matches!(self.browser.view, 3 | 5))
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
            page.loaded = false;
            page.cursors = vec![None];
            page.seek = None;
            page.before = false;
            page.epoch = page.epoch.wrapping_add(1);
            page.scroll_id.clear();
            page.scroll_pixel = 0.0;
        }
        if self.browser.view == 4 && pane == 2 {
            self.browser.pages[2].playlist_revisions = self.current_playlist_revisions();
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
                page.loaded = true;
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
                let targets = p
                    .rows
                    .iter()
                    .map(|r| match pane {
                        0 => music_library::preferences::IgnoreTarget::Artist(r.id.clone()),
                        1 => music_library::preferences::IgnoreTarget::Album(r.id.clone()),
                        _ => music_library::preferences::IgnoreTarget::Track(
                            r.track
                                .as_ref()
                                .map(|t| t.track_id.0.clone())
                                .unwrap_or_default(),
                        ),
                    })
                    .collect::<Vec<_>>();
                let ignored = self
                    .session
                    .library
                    .ignored_in_targets(&targets)
                    .unwrap_or_default();
                let rows: QVariantList = p
                    .rows
                    .iter()
                    .enumerate()
                    .map(|(index, r)| {
                        let mut map: QVariantMap = [
                            (
                                "ignored",
                                ignored.get(index).copied().unwrap_or(false).into(),
                            ),
                            ("id", string(&r.id)),
                            ("title", string(&r.title)),
                            ("subtitle", string(&r.subtitle)),
                            ("genres", string(&r.genres)),
                            ("connectionReason", string(&r.connection_reason)),
                            ("connectionReasonCode", string(&r.connection_reason_code)),
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
            ("contextHidden", self.browser.context_hidden.into()),
            ("contextIgnored", self.browser.context_ignored.into()),
            ("preferenceMore", self.browser.preference_more.into()),
            (
                "preferenceRows",
                QVariant::from(
                    self.browser
                        .preference_rows
                        .iter()
                        .map(|r| {
                            QVariant::from(
                                [
                                    ("id", string(&r.id)),
                                    ("title", string(&r.title)),
                                    ("artist", string(&r.subtitle)),
                                    (
                                        "album",
                                        string(
                                            r.track
                                                .as_ref()
                                                .map(|t| t.release_title.as_str())
                                                .unwrap_or(""),
                                        ),
                                    ),
                                ]
                                .into_iter()
                                .collect::<QVariantMap>(),
                            )
                        })
                        .collect::<QVariantList>(),
                ),
            ),
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
                    music_library::browse::SongColumn::Reason => "reason",
                }),
            ),
            (
                "songsDescending",
                (self.browser.global_songs_sort != 0).into(),
            ),
            (
                "unresolvedCount",
                string(
                    self.browser
                        .unresolved_count
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "…".into()),
                ),
            ),
            ("reviewMarked", self.browser.review_marked.into()),
            (
                "markedCount",
                string(
                    self.browser
                        .marked_count
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "…".into()),
                ),
            ),
            (
                "reviewMessage",
                string(if self.browser.review_retry_message.is_empty() {
                    &self.browser.review_message
                } else {
                    &self.browser.review_retry_message
                }),
            ),
            ("reviewLocalActive", self.browser.review_local_active.into()),
            (
                "reviewRetryPending",
                (self.browser.review_run_active
                    || self.browser.review_auto_scheduled
                    || !self.browser.review_retry_albums.is_empty()
                    || self.browser.review_retry_local_active
                    || self.browser.review_retry_requested
                    || self
                        .spotify_album_matcher
                        .as_ref()
                        .is_some_and(|m| m.pending_count() > 0))
                .into(),
            ),
            (
                "reviewCooldownDelay",
                if self.browser.review_provider_blocked {
                    self.session
                        .library
                        .spotify_cooldown_remaining(spotify_now())
                        .unwrap_or(0)
                        .max(0) as i32
                } else {
                    0
                }
                .into(),
            ),
            ("reviewAlbum", string(&self.browser.album)),
            ("playlistDetails", self.playlist_details_value().into()),
            ("panes", panes.into()),
            (
                "view",
                string(
                    [
                        "Artists",
                        "Genres",
                        "Albums",
                        "Songs",
                        "Playlists",
                        "Spotify Connections",
                    ][self.browser.view],
                ),
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
            _ if self.view == 5 => [Sort::Title, Sort::Descending][self.global_songs_sort],
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
            omit_hidden_artists: pane == 0 && self.view == 0 && self.pages[0].seek.is_none(),
            unresolved_spotify: self.view == 5 && pane == 2,
            marked_spotify: self.view == 5 && pane == 2 && self.review_marked,
            song_column: if matches!(self.view, 3 | 5) && pane == 2 {
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
    fn current_playlist_revisions(&self) -> Vec<(String, i64)> {
        self.session
            .library
            .playlist_content_revisions(
                &self.browser.selections[0]
                    .ids
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_default()
    }
    fn refresh_changed_playlist(&mut self) {
        if self.browser.view == 4
            && self.browser.pages[2].loaded
            && self.browser.pages[2].playlist_revisions != self.current_playlist_revisions()
        {
            self.refresh_selection();
        }
    }
    pub(crate) fn metadata_move_album_context(&mut self, source: &str, destination: &str) {
        fn remap(s: &mut music_library::selection::Selection, source: &str, destination: &str) {
            if s.ids.remove(source) {
                s.ids.insert(destination.to_owned());
            }
            if s.anchor.as_deref() == Some(source) {
                s.anchor = Some(destination.to_owned());
            }
            if s.focus.as_deref() == Some(source) {
                s.focus = Some(destination.to_owned());
            }
        }
        remap(&mut self.browser.selections[1], source, destination);
        if self.browser.album == source {
            self.browser.album = destination.to_owned();
        }
        for view in &mut self.browser.views {
            remap(&mut view.selections[1], source, destination);
            if view.album == source {
                view.album = destination.to_owned();
            }
        }
    }
    pub(crate) fn metadata_refresh_browse(&mut self) {
        // Inactive windows keep their identity anchor and reload on navigation.
        for state in &mut self.browser.views {
            for page in &mut state.pages {
                page.seek = if page.scroll_id.is_empty() {
                    page.rows.first().map(|r| r.id.clone())
                } else {
                    Some(page.scroll_id.clone())
                };
                page.loaded = false;
            }
        }
        if self.browser.view == 4 {
            self.refresh_selection_with_details(false);
            return;
        }
        for pane in 0..3 {
            if self.browser.pages[pane].rows.is_empty() {
                self.load_pane(pane, false);
                continue;
            }
            let page = &self.browser.pages[pane];
            let anchor = if page.scroll_id.is_empty() {
                page.rows[0].id.clone()
            } else {
                page.scroll_id.clone()
            };
            let request = self.browser.pane_request(pane);
            let first = &page.rows[0];
            // Preserve an unchanged window boundary and its paging history.
            // Center around the viewport only if sorting/removal moved that boundary.
            let stable = (|| -> music_library::Result<Option<Vec<Row>>> {
                if page.cursors.len() == 1 && page.seek.is_none() && !page.before {
                    let rows = self.session.library.browse(&request)?;
                    return Ok(rows.iter().any(|row| row.id == anchor).then_some(rows));
                }
                let mut rows = self.session.library.browse(&Request {
                    ids: vec![first.id.clone()],
                    limit: 1,
                    ..request.clone()
                })?;
                if rows.first().is_none_or(|row| row.cursor != first.cursor) {
                    return Ok(None);
                }
                let cursor = rows[0].cursor.clone();
                rows.extend(self.session.library.browse(&Request {
                    after: Some(cursor),
                    limit: PAGE as u32,
                    ..request.clone()
                })?);
                Ok(rows.iter().any(|row| row.id == anchor).then_some(rows))
            })();
            let (rows, preserve_paging) = match stable {
                Ok(Some(rows)) => (Ok(rows), true),
                _ => (
                    self.session
                        .library
                        .browse_around(&request, &anchor)
                        .or_else(|_| self.session.library.browse(&request)),
                    false,
                ),
            };
            match rows {
                Ok(mut rows) => {
                    let page = &mut self.browser.pages[pane];
                    page.more = rows.len() > PAGE;
                    rows.truncate(PAGE);
                    page.rows = rows;
                    if !preserve_paging {
                        page.cursors = vec![None];
                        page.before = true;
                        page.seek = Some(anchor);
                    }
                }
                Err(e) => self.browser.error = e.to_string(),
            }
        }
        self.refresh_spotify_review();
        self.browse_changed();
    }
    fn refresh_selection(&mut self) {
        self.refresh_selection_with_details(true);
    }
    fn request_review_count(&mut self) {
        if self.browser.review_count_pending {
            self.browser.review_count_dirty = true;
            return;
        }
        let Ok(reader) = self.session.library.library_queue_reader() else {
            return;
        };
        self.browser.review_count_pending = true;
        self.browser.review_count_dirty = false;
        let pointer = QPointer::from(&*self);
        let deliver = qmetaobject::queued_callback(move |result: Result<(u64, u64), String>| {
            if let Some(object) = pointer.as_pinned() {
                let mut b = object.borrow_mut();
                b.browser.review_count_pending = false;
                if b.browser.review_count_dirty {
                    b.request_review_count();
                } else {
                    if let Ok((count, marked)) = result {
                        b.browser.unresolved_count = Some(count);
                        b.browser.marked_count = Some(marked);
                    }
                    b.browse_changed();
                }
            }
        });
        std::thread::spawn(move || {
            deliver(
                (|| {
                    Ok((
                        reader.unresolved_spotify_count()?,
                        reader.marked_spotify_count()?,
                    ))
                })()
                .map_err(|e: music_library::storage::Error| e.to_string()),
            )
        });
    }

    pub(crate) fn refresh_spotify_review(&mut self) {
        if self.browser.view != 5 {
            let page = &mut self.browser.views[5].pages[2];
            page.seek = if page.scroll_id.is_empty() {
                page.rows.first().map(|r| r.id.clone())
            } else {
                Some(page.scroll_id.clone())
            };
            page.loaded = false;
            page.rows.clear();
            return;
        }
        self.start_local_spotify_review();
        self.request_review_count();
        let request = self.browser.pane_request(2);
        let ids: Vec<_> = self.browser.pages[2]
            .rows
            .iter()
            .map(|r| r.id.clone())
            .collect();
        let mut refreshed = std::collections::HashMap::new();
        for chunk in ids.chunks(PAGE) {
            match self.session.library.browse(&Request {
                ids: chunk.to_vec(),
                after: None,
                ..request.clone()
            }) {
                Ok(rows) => refreshed.extend(rows.into_iter().map(|r| (r.id.clone(), r))),
                Err(e) => {
                    self.browser.error = e.to_string();
                    return;
                }
            }
        }
        let reason_order_changed = request.song_column
            == Some(music_library::browse::SongColumn::Reason)
            && self.browser.pages[2].rows.iter().any(|old| {
                refreshed
                    .get(&old.id)
                    .is_some_and(|new| new.cursor.title != old.cursor.title)
            });
        let old_last = self.browser.pages[2].rows.last().map(|r| r.cursor.clone());
        let old_first = self.browser.pages[2].rows.first().map(|r| r.cursor.clone());
        let page = &mut self.browser.pages[2];
        page.rows = ids.iter().filter_map(|id| refreshed.remove(id)).collect();
        let removed = ids.len().saturating_sub(page.rows.len()) as u64;
        if let Some(count) = if self.browser.review_marked {
            &mut self.browser.marked_count
        } else {
            &mut self.browser.unresolved_count
        } {
            *count = count.saturating_sub(removed);
        }
        page.rows.sort_by(|a, b| {
            let order = (&a.cursor.title, &a.id).cmp(&(&b.cursor.title, &b.id));
            if request.sort == Sort::Descending {
                order.reverse()
            } else {
                order
            }
        });
        self.browser.selections[2].retain(&page.rows.iter().map(|r| r.id.clone()).collect());
        self.browser.song = self.browser.selections[2].focus.clone().unwrap_or_default();
        if reason_order_changed && !page.rows.is_empty() {
            let anchor = if page.rows.iter().any(|r| r.id == page.scroll_id) {
                page.scroll_id.clone()
            } else {
                page.rows[0].id.clone()
            };
            match self.session.library.browse_around(&request, &anchor) {
                Ok(mut rows) => {
                    page.more = rows.len() > PAGE;
                    rows.truncate(PAGE);
                    page.rows = rows;
                    page.before = true;
                }
                Err(e) => self.browser.error = e.to_string(),
            }
        } else if page.rows.is_empty() {
            let next = self.session.library.browse(&Request {
                after: old_last,
                ..request.clone()
            });
            match next {
                Ok(mut rows) => {
                    page.more = rows.len() > PAGE;
                    rows.truncate(PAGE);
                    if rows.is_empty()
                        && let Some(first) = old_first
                    {
                        rows = self
                            .session
                            .library
                            .browse_before(&Request {
                                after: Some(first),
                                ..request
                            })
                            .unwrap_or_default();
                        rows.truncate(PAGE);
                        rows.reverse();
                        page.before = !rows.is_empty();
                    }
                    page.rows = rows;
                }
                Err(e) => self.browser.error = e.to_string(),
            }
        }
        page.loaded = true;
        if page.cursors.is_empty() {
            page.cursors = vec![None];
        }
        self.browse_changed();
    }

    fn refresh_selection_with_details(&mut self, refresh_details: bool) {
        if refresh_details {
            self.refresh_playlist_details(true);
        }
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
                page.loaded = false;
                page.cursors = vec![None];
                page.seek = target;
                page.before = page.seek.is_some();
                page.epoch = page.epoch.wrapping_add(1);
            }
        }
        if self.browser.view == 5 {
            self.refresh_spotify_review();
            return;
        }
        if self.browser.view == 4 && !self.browser.pages[2].rows.is_empty() {
            // Enrichment invalidates metadata, not the selected playlist or window.
            // Keep the old rows displayed until the bounded replacement is ready.
            let page = &self.browser.pages[2];
            let refresh = (
                page.rows[0].cursor.clone(),
                page.rows.len().max(PAGE),
                page.cursors.last().is_none_or(Option::is_none) && page.seek.is_none(),
            );
            self.browser.playlist_view_generation =
                self.browser.playlist_view_generation.wrapping_add(1);
            self.browser.playlist_view_pending = Some(PlaylistViewRequest {
                generation: self.browser.playlist_view_generation,
                playlists: self.browser.selections[0].ids.iter().cloned().collect(),
                sort: self.browser.playlist_sort,
                cursor: None,
                reverse: None,
                refresh: Some(refresh),
                revisions: self.current_playlist_revisions(),
                selection: self.browser.selections[2].ids.iter().cloned().collect(),
            });
            self.start_playlist_view();
            self.load_pane(0, false);
            return;
        }
        if self.browser.view == 4 {
            self.load_pane(0, true);
            self.selection_changed(0);
            return;
        }
        // Refreshing evidence is not navigation. Keep each visible window anchored
        // while pruning selections whose canonical objects are no longer visible.
        match self.session.library.library_queue_reader() {
            Ok(reader) => {
                for pane in 0..3 {
                    let selected = self.browser.selections[pane]
                        .ids
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>();
                    match reader.browse_ids(&self.browser.pane_request(pane), &selected) {
                        Ok(valid) => {
                            self.browser.selections[pane].retain(&valid.into_iter().collect())
                        }
                        Err(e) => self.browser.error = e.to_string(),
                    }
                    self.browser.sync_selection_focus();
                }
            }
            Err(e) => self.browser.error = e.to_string(),
        }
        self.metadata_refresh_browse();
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
        if self.browser.pending || self.route_pending.is_some() || self.automatic_song_search {
            return;
        }
        self.browser.queue_explicit_start_only = false;
        self.browser.queue_explicit = match &target {
            music_library::track_container::Target::Songs(ids) => ids.clone(),
            music_library::track_container::Target::PlaylistEntries { entries, .. } => {
                self.browser.pages[2]
                    .rows
                    .iter()
                    .filter(|r| entries.contains(&r.id))
                    .filter_map(|r| r.track.as_ref().map(|t| t.track_id.0.clone()))
                    .collect()
            }
            _ => vec![],
        };
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
        let explicit = self.browser.queue_explicit.clone();
        self.browser.worker = Some(std::thread::spawn(move || {
            deliver(
                reader
                    .resolve(&target, &order)
                    .map_err(|e| e.to_string())
                    .and_then(|tracks| {
                        reader
                            .prepare_playback_program(tracks, 0, &explicit, false)
                            .map_err(|e| e.to_string())
                    }),
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
                            b.refresh_changed_playlist();
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
                        b.refresh_changed_playlist();
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

fn spotify_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
impl Bridge {
    pub(crate) fn request_automatic_spotify_reconciliation(&mut self) {
        self.browser.review_auto_requested = true;
        self.maybe_schedule_spotify_reconciliation();
    }
    pub(crate) fn maybe_schedule_spotify_reconciliation(&mut self) {
        if !self.browser.review_auto_requested
            || self.browser.review_auto_scheduled
            || self.browser.review_run_active
            || self.browser.review_local_active
            || self.browser.review_retry_local_active
            || self.local_import_state.busy
            || self.catalog.pending
            || !self.auto_match
            || self.matcher.as_ref().is_some_and(|m| m.pending_count() > 0)
            || self
                .spotify_album_matcher
                .as_ref()
                .is_some_and(|m| m.pending_count() > 0)
        {
            return;
        }
        // Configuration reads never authenticate or perform HTTP. An access error
        // aborts the run; no timer or refresh silently retries that error.
        if self.spotify_album_matcher.is_none()
            && music_library_spotify::Spotify::from_env().is_err()
        {
            return;
        }
        self.browser.review_auto_scheduled = true;
        self.browser.review_retry_message = "Preparing Spotify reconciliation…".into();
        self.browse_changed();
        let pointer = QPointer::from(&*self);
        let deliver = qmetaobject::queued_callback(move |_: ()| {
            if let Some(object) = pointer.as_pinned() {
                let mut b = object.borrow_mut();
                b.browser.review_auto_scheduled = false;
                if b.local_import_state.busy
                    || b.catalog.pending
                    || b.browser.review_run_active
                    || b.matcher.as_ref().is_some_and(|m| m.pending_count() > 0)
                {
                    return;
                }
                b.browser.review_startup_checked = true;
                b.browser.review_auto_requested = false;
                if b.session
                    .library
                    .spotify_reconciliation_albums(None, 1, true)
                    .is_ok_and(|a| !a.is_empty())
                {
                    b.retry_unresolved_spotify();
                    b.browser.review_run_automatic = true;
                } else {
                    b.browser.review_retry_message = "Spotify reconciliation is current; stable exceptions remain available for review.".into();
                    b.browse_changed();
                }
            }
        });
        deliver(());
    }
    fn schedule_next_spotify_batch(&mut self) {
        let generation = self.browser.review_retry_generation;
        let pointer = QPointer::from(&*self);
        let deliver = qmetaobject::queued_callback(move |_: ()| {
            if let Some(object) = pointer.as_pinned() {
                let mut b = object.borrow_mut();
                if b.browser.review_retry_generation == generation {
                    b.begin_retry_unresolved_spotify();
                    b.browse_changed();
                }
            }
        });
        deliver(());
    }
    pub(crate) fn defer_spotify_provider_work(&mut self) {
        self.browser.review_provider_blocked = true;
        self.browser.review_retry_message =
            "Spotify provider work paused; continuing cached/local reconciliation…".into();
    }
    pub(crate) fn record_spotify_progress(&mut self, transitions: usize) {
        if transitions > 0 && self.browser.review_first_transition_ms.is_none() {
            self.browser.review_first_transition_ms = self
                .browser
                .review_run_started
                .map(|s| s.elapsed().as_millis());
        }
        if self.browser.review_run_active {
            self.browser.review_retry_message = format!(
                "Matching Spotify… {} cached Tracks checked; {} cached connections; {} internal batches processed",
                self.browser.review_local_totals[0],
                self.browser.review_local_totals[1],
                self.browser.review_run_batches
            );
        }
    }
}

impl Bridge {
    fn start_local_spotify_review(&mut self) {
        if self.browser.review_local_active {
            return;
        }
        let Some(path) = self
            .session
            .library
            .database_path()
            .filter(|p| !p.is_empty())
        else {
            return;
        };
        self.browser.review_local_active = true;
        self.browser.review_local_totals = [0; 3];
        self.browser.review_progress_at = None;
        let pointer = QPointer::from(&*self);
        let (ack_tx, ack_rx) = std::sync::mpsc::channel();
        let deliver = qmetaobject::queued_callback(
            move |result: Result<Option<music_library::spotify_lifecycle::LocalBatch>, String>| {
                if let Some(object) = pointer.as_pinned() {
                    let mut b = object.borrow_mut();
                    match result {
                        Ok(Some(batch)) => {
                            b.browser.review_local_totals[0] += batch.examined;
                            b.browser.review_local_totals[1] += batch.accepted.len();
                            b.browser.review_local_totals[2] += batch.needs_retry;
                            b.record_spotify_progress(batch.updated.len());
                            let totals = b.browser.review_local_totals;
                            b.browser.review_message = format!(
                                "Local review: {} checked, {} connected, {} need retry",
                                totals[0], totals[1], totals[2]
                            );
                            if !batch.errors.is_empty() {
                                b.browser.error = batch.errors.join("\n");
                                let reason = b.browser.error.clone();
                                b.stop_spotify_retry_batch(reason);
                            }
                            if b.spotify_playback_track
                                .as_ref()
                                .is_some_and(|t| batch.accepted.contains(t))
                            {
                                b.refresh_spotify_album_diagnostic();
                                b.spotify_playback_changed();
                            }
                            let visible_changed = b.browser.pages[2]
                                .rows
                                .iter()
                                .any(|r| batch.updated.iter().any(|t| t.as_ref() == r.id));
                            if visible_changed || !batch.accepted.is_empty() {
                                b.refresh_spotify_review();
                            }
                            if visible_changed
                                || !batch.accepted.is_empty()
                                || b.browser.review_progress_at.is_none_or(|t| {
                                    t.elapsed() >= std::time::Duration::from_millis(250)
                                })
                            {
                                b.browser.review_progress_at = Some(std::time::Instant::now());
                                b.browse_changed();
                            }
                            let _ = ack_tx.send(());
                            return;
                        }
                        Ok(None) => {
                            b.browser.review_local_active = false;
                            if std::mem::take(&mut b.browser.review_retry_requested) {
                                b.begin_retry_unresolved_spotify();
                            }
                            let totals = b.browser.review_local_totals;
                            b.browser.review_message = if totals[0] == 0 {
                                "Cached evaluations are current.".into()
                            } else {
                                format!(
                                    "Cached review: {} checked, {} connected. Spotify reconciliation continues through eligible Album contexts.",
                                    totals[0], totals[1]
                                )
                            };
                        }
                        Err(e) => {
                            b.browser.review_local_active = false;
                            b.stop_spotify_retry_batch(e.clone());
                            b.browser.error = e;
                        }
                    }
                    b.browse_changed();
                }
                let _ = ack_tx.send(());
            },
        );
        std::thread::spawn(move || {
            let mut worker = match music_library::Library::open(path) {
                Ok(worker) => worker,
                Err(error) => {
                    deliver(Err(error.to_string()));
                    return;
                }
            };
            loop {
                match worker.reevaluate_stale_spotify(music_library::spotify_lifecycle::LOCAL_BATCH)
                {
                    Ok(batch) if batch.examined > 0 => {
                        let failed = !batch.errors.is_empty();
                        deliver(Ok(Some(batch)));
                        if ack_rx
                            .recv_timeout(std::time::Duration::from_secs(10))
                            .is_err()
                        {
                            return;
                        }
                        if failed {
                            break;
                        }
                    }
                    Ok(_) => break,
                    Err(e) => {
                        deliver(Err(e.to_string()));
                        return;
                    }
                }
            }
            deliver(Ok(None));
        });
    }
    fn retry_unresolved_spotify(&mut self) {
        if self.browser.review_run_active
            || !self.browser.review_retry_albums.is_empty()
            || self.browser.review_retry_local_active
            || self
                .spotify_album_matcher
                .as_ref()
                .is_some_and(|m| m.pending_count() > 0)
        {
            return;
        }
        self.browser.review_run_active = true;
        self.browser.review_run_automatic = false;
        self.browser.review_provider_blocked = false;
        self.browser.review_run_batches = 0;
        self.browser.review_catalog_names_attempted = Default::default();
        self.browser.review_run_started = Some(std::time::Instant::now());
        self.browser.review_first_progress_ms = Some(0);
        self.browser.review_first_transition_ms = None;
        self.browser.review_retry_message = "Preparing Spotify reconciliation…".into();
        self.browse_changed();
        self.browser.review_retry_single = false;
        self.browser.error.clear();
        self.browser.review_retry_requested = true;
        self.browser.review_retry_prepared = false;
        self.browser.review_retry_after = None;
        self.browser.review_retry_preparing = None;
        self.browser.review_retry_force.clear();
        self.browser.review_retry_seen.clear();
        self.browser.review_retry_extra.clear();
        self.browser.review_retry_artists.clear();
        self.browser.review_retry_totals = [
            0,
            0,
            0,
            self.session.library.unresolved_spotify_count().unwrap_or(0),
            0,
        ];
        self.browser.review_playable_searches = 0;
        self.start_local_spotify_review();
        if !self.browser.review_local_active {
            self.browser.review_retry_requested = false;
            self.begin_retry_unresolved_spotify();
        }
    }
    pub(crate) fn begin_retry_unresolved_spotify(&mut self) {
        if self.browser.review_retry_local_active {
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        // Advance by the database page, including already-enqueued rows. An
        // entire page of duplicates must not hide later actionable Albums.
        let mut page_after = self.browser.review_retry_after.clone();
        let mut albums =
            if self.browser.review_retry_single || self.browser.review_retry_preparing.is_some() {
                vec![]
            } else {
                loop {
                    let page = match self.session.library.spotify_reconciliation_albums(
                        page_after.as_ref(),
                        music_library::spotify_lifecycle::RECONCILIATION_ALBUM_BATCH_SIZE,
                        self.browser.review_run_automatic,
                    ) {
                        Ok(page) => page,
                        Err(error) => {
                            self.stop_spotify_retry_batch(error.to_string());
                            self.browser.error = error.to_string();
                            return;
                        }
                    };
                    if page.is_empty() {
                        break vec![];
                    }
                    page_after = page.last().cloned();
                    let fresh: Vec<_> = page
                        .into_iter()
                        .filter(|album| !self.browser.review_retry_seen.contains(album))
                        .collect();
                    if !fresh.is_empty() {
                        break fresh;
                    }
                }
            };
        if let Some((prepared, after)) = &self.browser.review_retry_preparing {
            albums = prepared.clone();
            page_after = after.clone();
        } else if albums.is_empty() && !self.browser.review_retry_extra.is_empty() {
            albums = self
                .browser
                .review_retry_extra
                .iter()
                .take(music_library::spotify_lifecycle::RETRY_ALBUMS as usize)
                .cloned()
                .collect();
        }
        while albums.is_empty() && !self.browser.review_retry_single {
            let Some((artist, cursor)) = self.browser.review_retry_artists.front().cloned() else {
                break;
            };
            albums = match self.session.library.spotify_artist_retry_albums(
                &artist,
                cursor.as_ref(),
                music_library::spotify_lifecycle::RETRY_ALBUMS,
            ) {
                Ok(albums) => albums,
                Err(error) => {
                    self.stop_spotify_retry_batch(error.to_string());
                    return;
                }
            };
            if albums.is_empty() {
                self.browser.review_retry_artists.pop_front();
            } else {
                self.browser.review_retry_artists.front_mut().unwrap().1 = albums.last().cloned();
                // A changed Artist revision permits one more context evaluation.
                for album in &albums {
                    self.browser.review_retry_seen.remove(album);
                }
            }
        }
        if albums.is_empty() {
            self.browser.review_run_active = false;
            self.browser.review_retry_requested = false;
            self.maybe_schedule_spotify_reconciliation();
            self.browser.review_retry_after = None;
            self.browser.review_retry_preparing = None;
            self.browser.review_retry_prepared = false;
            self.browser.review_retry_message = if let Ok(seconds) =
                self.session.library.spotify_cooldown_remaining(now)
                && seconds > 0
            {
                format!(
                    "Waiting for Spotify: retry available in {} minutes. No provider requests issued.",
                    (seconds + 59) / 60
                )
            } else {
                if self.browser.review_provider_blocked {
                    "Cached reconciliation complete; provider discovery is unavailable. Retry after restoring Spotify access.".into()
                } else {
                    format!(
                        "Spotify reconciliation finished: {} internal batches; {} Tracks connected. Stable exceptions remain available for review.",
                        self.browser.review_run_batches, self.browser.review_local_totals[1]
                    )
                }
            };
            return;
        }
        if !self.browser.review_retry_prepared {
            self.browser.review_retry_prepared = true;
            self.browser.review_retry_preparing = Some((albums.clone(), page_after.clone()));
            if let Some(market) = std::env::var("SPOTIFY_MARKET").ok().or_else(|| {
                self.session
                    .library
                    .retained_spotify_market()
                    .ok()
                    .flatten()
            }) && let Some(path) = self
                .session
                .library
                .database_path()
                .filter(|p| !p.is_empty())
            {
                self.browser.review_retry_local_active = true;
                let generation = self.browser.review_retry_generation;
                let pointer = QPointer::from(&*self);
                let deliver = qmetaobject::queued_callback(
                    move |result: Result<
                        (music_library::spotify_lifecycle::LocalBatch, bool),
                        String,
                    >| {
                        if let Some(object) = pointer.as_pinned() {
                            let mut b = object.borrow_mut();
                            if b.browser.review_retry_generation != generation {
                                return;
                            }
                            b.browser.review_retry_local_active = false;
                            match result {
                                Ok((batch, last)) => {
                                    for artist in &batch.artist_revisions {
                                        b.enqueue_spotify_artist_revision(artist.clone());
                                    }
                                    b.browser.review_local_totals[1] += batch.accepted.len();
                                    if !batch.errors.is_empty() {
                                        b.browser.error = batch.errors.join("\n");
                                        let error = b.browser.error.clone();
                                        b.stop_spotify_retry_batch(error);
                                    } else if last {
                                        b.begin_retry_unresolved_spotify();
                                    } else {
                                        b.browser.review_retry_local_active = true;
                                    }
                                    b.record_spotify_progress(batch.examined + batch.updated.len());
                                }
                                Err(error) => {
                                    b.browser.error = error.clone();
                                    b.stop_spotify_retry_batch(error.clone());
                                    b.browser.review_retry_message = format!(
                                        "Cached local review stopped: {error}. Retry remains available."
                                    );
                                }
                            }
                            b.refresh_spotify_review();
                            b.browse_changed();
                        }
                    },
                );
                self.browser.review_retry_message =
                    "Completing cached Album/Track evidence before scheduling provider work".into();
                let catalog_probes = self.browser.review_catalog_names_attempted.clone();
                std::thread::spawn(move || {
                    let mut worker = match music_library::Library::open(path) {
                        Ok(worker) => worker,
                        Err(error) => {
                            deliver(Err(error.to_string()));
                            return;
                        }
                    };
                    let count = albums.len();
                    for (index, album) in albums.into_iter().enumerate() {
                        let prior_artist =
                            worker
                                .spotify_album_review_input(&album)
                                .ok()
                                .and_then(|p| {
                                    if let music_library::album_matching::Preparation::Ready(
                                        input,
                                    ) = p
                                    {
                                        Some((input.artist_id, input.equivalent_artists))
                                    } else {
                                        None
                                    }
                                });
                        // Fill identity-scoped catalog names only when ordinary
                        // cached Spotify discovery has no useful candidate.
                        // HTTP is outside transactions; success invalidates only
                        // this catalog Artist's unresolved contexts.
                        if music_library_spotify::Spotify::from_env().is_ok()
                            && worker
                                .spotify_cooldown_remaining(spotify_now())
                                .unwrap_or(0)
                                == 0
                            && let Ok(music_library::album_matching::Preparation::Ready(input)) =
                                worker.spotify_album_review_input(&album)
                            && music_library_spotify::Spotify::cached_album_review(
                                &worker.database_path().unwrap_or_default(),
                                &market,
                                &input,
                            )
                            .ok()
                            .flatten()
                            .is_none_or(|(report, _)| {
                                report.candidates.iter().all(|c| !c.artist_accepted)
                            })
                            && catalog_probes
                                .lock()
                                .unwrap()
                                .insert(input.artist_id.clone())
                        {
                            let _ = music_library_musicbrainz::MusicBrainz::new()
                                .enrich_artist_names(&mut worker, &album);
                            if let Ok(client) = music_library_spotify::Spotify::from_env()
                                && let Ok(mut client) = client.with_library(worker.database_path().as_deref())
                                && let Err(error) = music_library_spotify::album_context::discover_catalog_credit_names(&mut worker, &album, &mut client)
                            {
                                deliver(Err(error.to_string()));
                                return;
                            }
                        }
                        let catalog_changed =
                            music_library_spotify::album_context::reconcile_catalog_credit_names(
                                &mut worker,
                                &album,
                                &market,
                            );
                        let catalog_changed = match catalog_changed {
                            Ok(changed) => changed,
                            Err(error) => {
                                deliver(Err(error.to_string()));
                                return;
                            }
                        };
                        let identity_changed = prior_artist.as_ref().is_some_and(|(_, before)| {
                            worker.spotify_album_review_input(&album).ok().is_some_and(|p| {
                                matches!(p, music_library::album_matching::Preparation::Ready(input) if input.equivalent_artists != *before)
                            })
                        });
                        let result =
                            music_library_spotify::album_context::reconcile(
                                &mut worker,
                                std::slice::from_ref(&album),
                                &market,
                            )
                            .map(|mut batch| {
                                if (catalog_changed || identity_changed)
                                    && let Ok(music_library::album_matching::Preparation::Ready(
                                        input,
                                    )) = worker.spotify_album_review_input(&album)
                                {
                                    batch.artist_revisions.push(input.artist_id);
                                }
                                batch
                            })
                            .map_err(|e| e.to_string());
                        let failed = result
                            .as_ref()
                            .map_or(true, |batch| !batch.errors.is_empty());
                        deliver(result.map(|batch| (batch, index + 1 == count)));
                        if failed {
                            break;
                        }
                    }
                });
                return;
            }
        }
        let prepared = albums.clone();
        albums.retain(|album| {
            self.browser.review_retry_force.contains(album)
                || self
                    .session
                    .library
                    .spotify_album_has_eligible(album)
                    .unwrap_or(true)
        });
        if albums.is_empty() {
            self.browser.review_retry_preparing = None;
            self.browser
                .review_retry_extra
                .retain(|album| !prepared.contains(album));
            self.browser.review_retry_seen.extend(prepared);
            self.browser.review_retry_after = page_after;
            self.browser.review_retry_prepared = false;
            self.browser.review_run_batches += 1;
            self.schedule_next_spotify_batch();
            return;
        }
        if !self.browser.review_provider_blocked
            && let Err(error) = self.ensure_spotify_album_matcher()
        {
            self.browser.review_provider_blocked = true;
            self.browser.review_retry_message = format!(
                "Spotify discovery unavailable: {error}; continuing cached/local reconciliation…"
            );
        }
        if self.browser.review_provider_blocked
            || self
                .session
                .library
                .spotify_cooldown_remaining(now)
                .unwrap_or(0)
                > 0
        {
            self.browser.review_provider_blocked = true;
            for album in &prepared {
                if let Err(error) = self.session.library.defer_spotify_album_provider(album) {
                    self.stop_spotify_retry_batch(error.to_string());
                    self.browser.error = error.to_string();
                    return;
                }
            }
            self.browser.review_retry_preparing = None;
            self.browser.review_retry_prepared = false;
            self.browser.review_retry_seen.extend(prepared);
            self.browser.review_retry_after = page_after;
            self.browser
                .review_retry_extra
                .retain(|album| !albums.contains(album));
            self.browser.review_run_batches += 1;
            self.browser.review_retry_message = format!(
                "Matching Spotify: {} internal batches checked locally; provider work is waiting for Spotify",
                self.browser.review_run_batches
            );
            self.browse_changed();
            self.schedule_next_spotify_batch();
            return;
        }
        if self.browser.review_retry_totals[0] == 0 {
            self.browser.review_retry_totals = [
                0,
                0,
                0,
                self.session.library.unresolved_spotify_count().unwrap_or(0),
                0,
            ];
        }
        self.browser.review_retry_totals[0] += albums
            .iter()
            .filter(|a| !self.browser.review_retry_extra.contains(a))
            .count() as u64;
        self.browser.review_run_batches += 1;
        self.browser.review_retry_preparing = None;
        self.browser
            .review_retry_extra
            .retain(|album| !albums.contains(album));
        self.browser.review_retry_page_after = page_after;
        self.browser
            .review_retry_seen
            .extend(albums.iter().cloned());
        self.browser.review_retry_albums = albums.clone();
        // Explicit additions join a later bounded batch; never duplicate a
        // context already scheduled by the normal cursor.
        self.browser.review_retry_playable_albums = albums.clone();
        self.browser.review_retry_playable_started = self.browser.review_playable_searches
            >= music_library_spotify::album_context::PLAYABLE_SEARCH_BUDGET;
        self.browser.review_retry_album_started = false;
        self.browser.review_retry_generation = self.browser.review_retry_generation.wrapping_add(1);
        self.browser.review_retry_local_started = false;
        self.browser.review_retry_message = format!(
            "Reconciling {} Albums in this batch; {} Albums scheduled so far",
            albums.len(),
            self.browser.review_retry_seen.len()
        );
        self.start_review_track_retry();
    }
}

impl Bridge {
    pub(crate) fn enqueue_spotify_artist_revision(
        &mut self,
        artist: music_library::domain::ArtistId,
    ) {
        if !self.browser.review_retry_single
            && !self.browser.review_retry_albums.is_empty()
            && !self
                .browser
                .review_retry_artists
                .iter()
                .any(|(id, _)| id == &artist)
        {
            self.browser.review_retry_artists.push_back((artist, None));
        }
    }
    pub(crate) fn stop_spotify_retry_batch(&mut self, reason: String) {
        let skipped = self.browser.review_retry_albums.len();
        let totals = self.browser.review_retry_totals;
        self.browser.review_run_active = false;
        self.browser.review_retry_albums.clear();
        self.browser.review_retry_extra.clear();
        self.browser.review_retry_artists.clear();
        self.browser.review_retry_preparing = None;
        self.browser.review_retry_force.clear();
        self.browser.review_retry_requested = false;
        self.browser.review_retry_generation = self.browser.review_retry_generation.wrapping_add(1);
        self.browser.review_retry_local_active = false;
        self.browser.review_retry_local_started = false;
        self.browser.review_retry_album_started = false;
        self.browser.review_retry_message = format!(
            "{} Albums planned; {} attempted, {} deferred/skipped: {}",
            totals[0],
            totals[1] + totals[2],
            skipped,
            reason
        );
    }
    pub(crate) fn start_review_track_retry(&mut self) {
        if self.browser.review_retry_local_active {
            return;
        }
        if self
            .spotify_album_matcher
            .as_ref()
            .is_some_and(|m| m.pending_count() > 0)
        {
            return;
        }
        loop {
            if !self.browser.review_retry_album_started
                && !self.browser.review_retry_albums.is_empty()
            {
                let Some(album) = self.browser.review_retry_albums.first().cloned() else {
                    return;
                };
                self.browser.review_retry_album_started = true;
                if !self.browser.review_provider_blocked
                    && let Some(matcher) = self.spotify_album_matcher.as_mut()
                {
                    let fresh = self.browser.review_retry_force.remove(&album)
                        || self
                            .session
                            .library
                            .spotify_album_needs_retry(&album)
                            .unwrap_or(false);
                    let ready = matches!(
                        self.session.library.spotify_album_review_input(&album),
                        Ok(music_library::album_matching::Preparation::Ready(_))
                    );
                    let result = if !ready {
                        self.browser.review_retry_totals[4] += 1;
                        Ok(music_library::album_matching::MatchOutcome::Skipped)
                    } else if fresh {
                        self.browser.review_retry_totals[1] += 1;
                        matcher.refresh_album(&self.session.library, &album)
                    } else {
                        self.browser.review_retry_totals[2] += 1;
                        matcher.match_album(&self.session.library, &album)
                    };
                    if let Err(e) = result {
                        self.browser.review_retry_albums.clear();
                        self.browser.review_retry_message = format!("Spotify retry stopped: {e}");
                        self.browser.error = e.to_string();
                        return;
                    }
                    if matcher.pending_count() > 0 {
                        return;
                    }
                }
            }
            if !self.browser.review_retry_local_started
                && let Some(album) = self.browser.review_retry_albums.first().cloned()
            {
                self.browser.review_retry_local_started = true;
                if self
                    .session
                    .library
                    .spotify_album_has_eligible(&album)
                    .unwrap_or(false)
                    && let Some(market) = std::env::var("SPOTIFY_MARKET").ok().or_else(|| {
                        self.session
                            .library
                            .retained_spotify_market()
                            .ok()
                            .flatten()
                    })
                    && let Ok(Some(mut worker)) = self.session.library.spotify_review_worker()
                {
                    self.browser.review_retry_local_active = true;
                    let generation = self.browser.review_retry_generation;
                    let pointer = QPointer::from(&*self);
                    let deliver = qmetaobject::queued_callback(
                        move |result: Result<
                            music_library::spotify_lifecycle::LocalBatch,
                            String,
                        >| {
                            if let Some(object) = pointer.as_pinned() {
                                let mut b = object.borrow_mut();
                                if b.browser.review_retry_generation != generation {
                                    return;
                                }
                                b.browser.review_retry_local_active = false;
                                match result {
                                    Ok(batch) => {
                                        b.browser.review_local_totals[1] += batch.accepted.len();
                                        if !batch.errors.is_empty() {
                                            b.browser.error = batch.errors.join("\n");
                                        }
                                    }
                                    Err(error) => b.browser.error = error,
                                }
                                if b.browser.error.is_empty() {
                                    b.start_review_track_retry();
                                } else {
                                    let error = b.browser.error.clone();
                                    b.stop_spotify_retry_batch(error);
                                }
                                b.refresh_spotify_review();
                                b.browse_changed();
                            }
                        },
                    );
                    std::thread::spawn(move || {
                        deliver(
                            music_library_spotify::album_context::reconcile(
                                &mut worker,
                                &[album],
                                &market,
                            )
                            .map_err(|e| e.to_string()),
                        )
                    });
                    return;
                }
            }
            // Album discovery is the batch unit. Absence/rejection must not fan out
            // into one provider search per Track; individual review remains explicit.
            if !self.browser.review_retry_albums.is_empty() {
                self.browser.review_retry_albums.remove(0);
                self.browser.review_retry_album_started = false;
                self.browser.review_retry_local_started = false;
                self.browser.review_retry_message = format!(
                    "Reconciling… {} / {} scheduled Albums processed; {} remain in this batch",
                    self.browser.review_retry_totals[0]
                        .saturating_sub(self.browser.review_retry_albums.len() as u64),
                    self.browser.review_retry_totals[0],
                    self.browser.review_retry_albums.len()
                );
                if !self.browser.review_retry_albums.is_empty() {
                    continue;
                }
            }
            if !self.browser.review_retry_playable_started && !self.browser.review_provider_blocked
            {
                self.browser.review_retry_playable_started = true;
                #[cfg(test)]
                if self.browser.review_test_async_fallback {
                    self.browser.review_retry_local_active = true;
                    let pointer = QPointer::from(&*self);
                    let deliver = qmetaobject::queued_callback(move |_: ()| {
                        if let Some(object) = pointer.as_pinned() {
                            let mut b = object.borrow_mut();
                            b.browser.review_retry_local_active = false;
                            b.start_review_track_retry();
                            b.browse_changed();
                        }
                    });
                    deliver(());
                    return;
                }
                let budget = music_library_spotify::album_context::PLAYABLE_SEARCH_BUDGET
                    .saturating_sub(self.browser.review_playable_searches);
                let albums = std::mem::take(&mut self.browser.review_retry_playable_albums);
                if let Some(market) = std::env::var("SPOTIFY_MARKET").ok().or_else(|| {
                    self.session
                        .library
                        .retained_spotify_market()
                        .ok()
                        .flatten()
                }) && let Ok(Some(mut library)) = self.session.library.spotify_review_worker()
                    && let Ok(mut spotify) = music_library_spotify::Spotify::from_env()
                        .and_then(|p| p.with_library(library.database_path().as_deref()))
                {
                    self.browser.review_retry_local_active = true;
                    let generation = self.browser.review_retry_generation;
                    let pointer = QPointer::from(&*self);
                    let deliver = qmetaobject::queued_callback(
                        move |result: Result<
                            music_library::spotify_lifecycle::LocalBatch,
                            String,
                        >| {
                            if let Some(object) = pointer.as_pinned() {
                                let mut b = object.borrow_mut();
                                if b.browser.review_retry_generation != generation {
                                    return;
                                }
                                b.browser.review_retry_local_active = false;
                                match result {
                                    Ok(batch) => {
                                        b.browser.review_playable_searches +=
                                            batch.provider_searches;
                                        b.browser.review_local_totals[1] += batch.accepted.len();
                                        if !batch.errors.is_empty() {
                                            if b.session
                                                .library
                                                .spotify_cooldown_remaining(spotify_now())
                                                .unwrap_or(0)
                                                > 0
                                            {
                                                b.defer_spotify_provider_work();
                                            } else {
                                                b.browser.error = batch.errors.join("\n");
                                            }
                                        }
                                    }
                                    Err(error) => b.browser.error = error,
                                }
                                if b.browser.error.is_empty() {
                                    b.start_review_track_retry();
                                } else {
                                    let error = b.browser.error.clone();
                                    b.stop_spotify_retry_batch(error);
                                }
                                b.refresh_spotify_review();
                                b.browse_changed();
                            }
                        },
                    );
                    std::thread::spawn(move || {
                        deliver(
                            music_library_spotify::album_context::reconcile_best_available_with_budget(
                                &mut library,
                                &albums,
                                &market,
                                &mut spotify,
                                budget,
                            )
                            .map_err(|e| e.to_string()),
                        )
                    });
                    return;
                }
            }
            if self.browser.review_retry_single {
                self.browser.review_run_active = false;
                self.browser.review_retry_single = false;
                self.browser.review_retry_message =
                    "Album reconciliation complete; current cached decisions applied".into();
                return;
            }
            self.browser.review_retry_after = self.browser.review_retry_page_after.take();
            if !self.browser.review_retry_extra.is_empty()
                || !self.browser.review_retry_artists.is_empty()
            {
                self.browser.review_retry_prepared = false;
                self.begin_retry_unresolved_spotify();
                return;
            }
            if let Some(after) = self.browser.review_retry_after.as_ref() {
                match self.session.library.spotify_reconciliation_albums(
                    Some(after),
                    music_library::spotify_lifecycle::RECONCILIATION_ALBUM_BATCH_SIZE,
                    self.browser.review_run_automatic,
                ) {
                    Ok(next) if !next.is_empty() => {
                        self.browser.review_retry_prepared = false;
                        self.begin_retry_unresolved_spotify();
                        return;
                    }
                    Err(error) => {
                        self.stop_spotify_retry_batch(error.to_string());
                        self.browser.error = error.to_string();
                        return;
                    }
                    _ => {}
                }
            }
            self.browser.review_run_active = false;
            self.refresh_spotify_review();
            self.maybe_schedule_spotify_reconciliation();
            let totals = self.browser.review_retry_totals;
            let remaining = self
                .session
                .library
                .unresolved_spotify_count()
                .unwrap_or(totals[3]);
            let marked = self.session.library.marked_spotify_count().unwrap_or(0);
            self.browser.review_retry_message = format!(
                "{} Albums considered: {} requested fresh discovery, {} evaluated with normal cache policy. {} Tracks resolved; {} remain unresolved. {} contexts need manual review; {} marked Not on Spotify excluded. Local evidence and bounded Track fallback connected {} Tracks. The actionable queue is drained; remaining Tracks can be reviewed individually.",
                totals[0],
                totals[1],
                totals[2],
                totals[3].saturating_sub(remaining),
                remaining,
                totals[4],
                marked,
                self.browser.review_local_totals[1]
            );
            return;
        }
    }
}

#[cfg(test)]
mod spotify_retry_tests {
    use super::*;
    use music_library::domain::*;
    use qmetaobject::QObjectBox;
    #[test]
    #[ignore = "full disposable fresh-import copy; explicit manual/startup/import/profile validation"]
    fn disposable_fresh_run_drains_all_internal_batches() {
        let path = std::env::var("MUSIC_LIBRARY_DIAGNOSTIC_DATABASE").unwrap();
        assert!(path.starts_with("/tmp/"));
        let mode = std::env::var("MUSIC_LIBRARY_RECONCILIATION_TRIGGER").unwrap_or("manual".into());
        let mut library = music_library::Library::open(&path).unwrap();
        let initial = library.unresolved_spotify_count().unwrap();
        let targets = library.spotify_retry_albums(None, 200).unwrap();
        if mode == "profile" {
            let started = std::time::Instant::now();
            let batch =
                music_library_spotify::album_context::reconcile(&mut library, &targets, "US")
                    .unwrap();
            println!(
                "OLD_PREFLIGHT albums={} elapsed_before_first_delivery_ms={} connected={} errors={:?}",
                targets.len(),
                started.elapsed().as_millis(),
                batch.accepted.len(),
                batch.errors
            );
            return;
        }
        let mut engine = qmetaobject::QmlEngine::new();
        engine.load_data("import QtQuick\nimport QtTest\nItem {TestCase {id:t;when:false} function pump(){t.wait(5);return true;}}".into());
        let bridge = QObjectBox::new(Bridge::new(crate::session::Session::new(library)));
        engine.set_object_property("bridge".into(), bridge.pinned());
        let start = std::time::Instant::now();
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.browse_action_impl("view", 2, "Spotify Connections".into());
            if mode == "startup" {
                b.browse_action_impl("refresh", 0, String::new());
            } else if mode == "import" {
                b.local_import_state.busy = true;
                b.request_automatic_spotify_reconciliation();
                b.local_import_schedule_next();
            } else {
                b.retry_unresolved_spotify();
            }
            println!(
                "RUN_START mode={mode} initial_unresolved={initial} call_return_ms={} initial_message={}",
                start.elapsed().as_millis(),
                b.browser.review_retry_message
            );
        }
        let mut transitions = None;
        for _ in 0..if mode == "live" { 120000 } else { 12000 } {
            engine.invoke_method("pump".into(), &[]);
            let pinned = bridge.pinned();
            let b = pinned.borrow();
            if transitions.is_none() && b.browser.review_first_transition_ms.is_some() {
                transitions = Some(start.elapsed().as_millis());
            }
            if !b.browser.review_run_active
                && !b.browser.review_local_active
                && !b.browser.review_retry_local_active
                && !b.browser.review_auto_scheduled
            {
                break;
            }
        }
        let pinned = bridge.pinned();
        let mut b = pinned.borrow_mut();
        assert!(b.browser.error.is_empty(), "{}", b.browser.error);
        assert!(
            !b.browser.review_run_active
                && !b.browser.review_local_active
                && !b.browser.review_retry_local_active
        );
        assert!(
            b.browser.review_run_batches > 0,
            "must finish all currently eligible contexts; snapshot size may shrink after automatic work"
        );
        let remaining = b.session.library.unresolved_spotify_count().unwrap();
        let batches = b.browser.review_run_batches;
        println!(
            "RUN_RESULT mode={mode} initial={initial} batch_size=20 batches={batches} connected={} remaining={remaining} provider_search_windows={} first_progress_ms={:?} first_transition_visible_ms={transitions:?} user_presses={} elapsed_ms={}",
            initial - remaining,
            b.browser.review_retry_totals[1] + b.browser.review_retry_totals[2],
            b.browser.review_first_progress_ms,
            if mode == "manual" || mode == "live" {
                1
            } else {
                0
            },
            start.elapsed().as_millis()
        );
        let request = b.browser.pane_request(2);
        for displayed in &b.browser.pages[2].rows {
            let actual = b
                .session
                .library
                .browse(&Request {
                    ids: vec![displayed.id.clone()],
                    after: None,
                    ..request.clone()
                })
                .unwrap();
            assert_eq!(
                actual
                    .first()
                    .map(|r| (&r.connection_reason, &r.connection_reason_code)),
                Some((
                    &displayed.connection_reason,
                    &displayed.connection_reason_code
                )),
                "stale Connections row {}",
                displayed.id
            );
        }
        println!(
            "DISPLAYED_STATE_CHECK rows={} stale_rows=0 message={}",
            b.browser.pages[2].rows.len(),
            b.browser.review_retry_message
        );
        let db = rusqlite::Connection::open(&path).unwrap();
        let states = db.prepare("SELECT CASE WHEN EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=v.track_id) THEN 'Connected' WHEN EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=v.track_id AND x.provider='spotify') THEN 'Not on Spotify' WHEN v.reason_code='provider_unavailable' THEN 'Waiting for Spotify' WHEN v.state='needs_retry' THEN 'Needs Retry' WHEN v.reason_code='no_candidates' THEN 'No Candidates' WHEN v.reason_code='provider_search_deferred' THEN 'Provider search deferred' ELSE 'Needs Review' END,count(*) FROM spotify_connection_review v JOIN library_membership m ON m.track_id=v.track_id GROUP BY 1").unwrap().query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?))).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
        println!("FINAL_STATES {}", serde_json::to_string(&states).unwrap());
        let retries=db.prepare("SELECT a.title,e.title,v.reason_code,v.reason FROM spotify_connection_review v JOIN library_membership m ON m.track_id=v.track_id JOIN effective_track_metadata e ON e.track_id=v.track_id JOIN track t ON t.id=v.track_id JOIN release r ON r.id=t.release_id JOIN effective_album_metadata a ON a.album_id=r.album_id WHERE v.state='needs_retry' AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=v.track_id) AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=v.track_id AND x.provider='spotify')").unwrap().query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?))).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
        println!("FINAL_RETRIES {}", serde_json::to_string(&retries).unwrap());
        if mode == "live" {
            return;
        }
        b.retry_unresolved_spotify();
        drop(b);
        for _ in 0..12000 {
            engine.invoke_method("pump".into(), &[]);
            let pinned = bridge.pinned();
            let b = pinned.borrow();
            if !b.browser.review_run_active
                && !b.browser.review_local_active
                && !b.browser.review_retry_local_active
            {
                break;
            }
        }
        assert_eq!(
            bridge
                .pinned()
                .borrow()
                .session
                .library
                .unresolved_spotify_count()
                .unwrap(),
            remaining
        );
    }
    #[test]
    #[ignore = "explicit scoped fresh-import copy with current cache and provider cooldown"]
    fn disposable_fresh_cached_retry_reaches_fixed_point() {
        let path = std::env::var("MUSIC_LIBRARY_DIAGNOSTIC_DATABASE").unwrap();
        assert!(path.starts_with("/tmp/"), "use a disposable SQLite copy");
        let library = music_library::Library::open(&path).unwrap();
        let initial = library.unresolved_spotify_count().unwrap();
        assert_eq!(initial, 85);
        let mut engine = qmetaobject::QmlEngine::new();
        engine.load_data("import QtQuick\nimport QtTest\nItem {TestCase {id:t;when:false} function pump(){t.wait(10);return true;}}".into());
        let bridge = QObjectBox::new(Bridge::new(crate::session::Session::new(library)));
        engine.set_object_property("bridge".into(), bridge.pinned());
        bridge.pinned().borrow_mut().retry_unresolved_spotify();
        for _ in 0..3000 {
            engine.invoke_method("pump".into(), &[]);
            let pinned = bridge.pinned();
            let b = pinned.borrow();
            if !b.browser.review_local_active
                && !b.browser.review_retry_local_active
                && !b.browser.review_retry_requested
                && b.browser.review_retry_albums.is_empty()
            {
                break;
            }
        }
        let remaining;
        let first_total;
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            assert!(b.browser.error.is_empty(), "{}", b.browser.error);
            assert!(!b.browser.review_local_active && !b.browser.review_retry_local_active);
            assert!(b.browser.review_retry_preparing.is_none());
            assert!(b.browser.review_retry_extra.is_empty());
            remaining = b.session.library.unresolved_spotify_count().unwrap();
            assert_eq!(remaining, 6);
            first_total = b.browser.review_local_totals;
            println!(
                "REAL_RETRY initial_actionable_tracks={initial} local_checked={} connected={} remaining={remaining} provider_requests=0 (retained cooldown) message={}",
                first_total[0],
                initial - remaining,
                b.browser.review_retry_message
            );
            b.retry_unresolved_spotify();
        }
        for _ in 0..3000 {
            engine.invoke_method("pump".into(), &[]);
            let pinned = bridge.pinned();
            let b = pinned.borrow();
            if !b.browser.review_local_active
                && !b.browser.review_retry_local_active
                && !b.browser.review_retry_requested
                && b.browser.review_retry_albums.is_empty()
            {
                break;
            }
        }
        let pinned = bridge.pinned();
        let b = pinned.borrow();
        assert!(b.browser.error.is_empty(), "{}", b.browser.error);
        assert_eq!(
            b.session.library.unresolved_spotify_count().unwrap(),
            remaining
        );
        assert_eq!(b.browser.review_local_totals[1], 0);
        println!("REAL_SECOND_RETRY connected=0 provider_requests=0 remaining={remaining}");
    }
    #[test]
    fn one_retry_replays_program_discovered_after_earlier_album_decision() {
        use music_library::{
            Library,
            album_matching::{AlbumMatcher, MatchOutcome},
            catalog::*,
        };
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        };
        struct ArrivingProgram {
            path: String,
            requests: Arc<AtomicUsize>,
        }
        impl CatalogProvider for ArrivingProgram {
            fn search_albums(
                &mut self,
                _: &str,
                _: u32,
            ) -> Result<Page<AlbumCandidate>, CatalogError> {
                unreachable!()
            }
            fn releases(
                &mut self,
                _: &ExternalIdentity,
                _: u32,
            ) -> Result<Page<ReleaseCandidate>, CatalogError> {
                unreachable!()
            }
            fn release(&mut self, _: &ExternalIdentity) -> Result<Release, CatalogError> {
                unreachable!()
            }
            fn artist_albums(
                &mut self,
                _: &ExternalIdentity,
                _: &str,
            ) -> Result<Page<ArtistAlbumCandidate>, CatalogError> {
                self.requests.fetch_add(1, Ordering::SeqCst);
                // Simulate an earlier empty discovery decision whose dependent
                // program observation arrives before batch completion.
                let db = Library::open(&self.path).unwrap();
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64;
                db.persist_spotify_discovery("spotify-name:1:US:artist1", "\"Artist\"", now)
                    .unwrap();
                db.persist_spotify_discovery("spotify:2:https://api.spotify.com/v1/search?market=US&q=album%3A%22album%22+artist%3A%22Artist%22&type=album&limit=10&offset=0",&serde_json::json!({"albums":{"items":[{"id":"album1","name":"Album","artists":[{"id":"artist1","name":"Artist"}],"album_type":"album","total_tracks":3}],"total":1,"offset":0,"next":null}}).to_string(),now).unwrap();
                let tracks=(1..=3).map(|n|serde_json::json!({"id":format!("track{n}"),"name":format!("Song {n}"),"disc_number":1,"track_number":n,"duration_ms":200000,"artists":[{"id":"artist1","name":"Artist"}]})).collect::<Vec<_>>();
                db.persist_spotify_discovery("spotify:1:https://api.spotify.com/v1/albums/album1/tracks?market=US&limit=50&offset=0",&serde_json::json!({"items":tracks,"total":3,"offset":0,"next":null}).to_string(),now).unwrap();
                Ok(Page {
                    items: vec![],
                    next_offset: None,
                })
            }
        }
        let mut engine = qmetaobject::QmlEngine::new();
        engine.load_data("import QtQuick\nimport QtTest\nItem {TestCase {id:t;when:false} function pump(){t.wait(10);return true;}}".into());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("fixed-point.sqlite");
        let mut library = Library::open(&path).unwrap();
        let credit = ArtistCreditInput {
            name: "Artist".into(),
            role: None,
        };
        let r = library
            .create_catalog_release(&CatalogReleaseInput {
                title: "Album".into(),
                year: None,
                artists: vec![credit.clone()],
                tracks: (1..=3)
                    .map(|n| CatalogTrackInput {
                        title: format!("Song {n}"),
                        disc_number: Some(1),
                        track_number: Some(n),
                        artists: vec![credit.clone()],
                    })
                    .collect(),
            })
            .unwrap();
        for track in &r.track_ids {
            library.add_to_library(track).unwrap();
        }
        let album = library.album_id_for_track(&r.track_ids[0]).unwrap();
        let db = rusqlite::Connection::open(&path).unwrap();
        let artist = ArtistId(
            db.query_row(
                "SELECT artist_id FROM album_artist_credit WHERE album_id=?1",
                [album.as_ref()],
                |r| r.get(0),
            )
            .unwrap(),
        );
        library
            .attach_artist_external_identity(
                &artist,
                &ExternalIdentity {
                    provider: "spotify".into(),
                    kind: "artist".into(),
                    external_id: "artist1".into(),
                },
            )
            .unwrap();
        library.reevaluate_stale_spotify(32).unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = mpsc::channel();
        let matcher = AlbumMatcher::for_provider(
            ArrivingProgram {
                path: path.to_string_lossy().into(),
                requests: requests.clone(),
            },
            music_library_spotify::matching_scope(),
            move |reply| {
                tx.send(reply).unwrap();
            },
            |_| {},
            |_| {},
        )
        .unwrap();
        let bridge = QObjectBox::new(Bridge::new(crate::session::Session::new(library)));
        engine.set_object_property("bridge".into(), bridge.pinned());
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.spotify_album_matcher = Some(matcher);
            b.begin_retry_unresolved_spotify();
            let reply = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            assert_eq!(reply.outcome, MatchOutcome::NoConfidentMatch);
            let mut matcher = b.spotify_album_matcher.take().unwrap();
            matcher.complete(&mut b.session.library, reply);
            b.spotify_album_matcher = Some(matcher);
            assert_eq!(
                b.session.library.unresolved_spotify_count().unwrap(),
                3,
                "earlier Album decision has not consumed newly retained program"
            );
            b.start_review_track_retry();
        }
        for _ in 0..500 {
            engine.invoke_method("pump".into(), &[]);
            let pinned = bridge.pinned();
            let b = pinned.borrow();
            if !b.browser.review_retry_local_active && b.browser.review_retry_albums.is_empty() {
                break;
            }
        }
        let pinned = bridge.pinned();
        let mut b = pinned.borrow_mut();
        assert_eq!(
            b.session.library.unresolved_spotify_count().unwrap(),
            0,
            "one invocation reaches all three dependent Tracks"
        );
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM spotify_connection_review WHERE stale=1",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        b.begin_retry_unresolved_spotify();
        assert_eq!(
            requests.load(Ordering::SeqCst),
            1,
            "second retry does no provider work"
        );
        assert!(b.browser.review_retry_albums.is_empty());
    }
    #[test]
    fn needs_retry_bypasses_cached_policy_once_per_album_and_cooldown_blocks_batch() {
        use music_library::{
            album_matching::{AlbumMatcher, MatchOutcome},
            catalog::*,
        };
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        };
        struct CachedProvider {
            refresh: bool,
            requests: Arc<AtomicUsize>,
        }
        impl CatalogProvider for CachedProvider {
            fn refresh_discovery(&mut self, refresh: bool) {
                self.refresh = refresh;
            }
            fn search_albums(
                &mut self,
                _: &str,
                _: u32,
            ) -> Result<Page<AlbumCandidate>, CatalogError> {
                unreachable!()
            }
            fn releases(
                &mut self,
                _: &ExternalIdentity,
                _: u32,
            ) -> Result<Page<ReleaseCandidate>, CatalogError> {
                unreachable!()
            }
            fn release(&mut self, _: &ExternalIdentity) -> Result<Release, CatalogError> {
                unreachable!()
            }
            fn search_artists(&mut self, _: &str) -> Result<Page<ArtistCandidate>, CatalogError> {
                Ok(Page {
                    items: vec![ArtistCandidate {
                        identity: ExternalIdentity {
                            provider: "spotify".into(),
                            kind: "artist".into(),
                            external_id: "a".into(),
                        },
                        name: "A".into(),
                        aliases: vec![],
                        comment: String::new(),
                        country: String::new(),
                        artist_type: String::new(),
                        score: None,
                    }],
                    next_offset: None,
                })
            }
            fn artist_albums(
                &mut self,
                _: &ExternalIdentity,
                _: &str,
            ) -> Result<Page<ArtistAlbumCandidate>, CatalogError> {
                // A normal fresh-cache lookup does no provider work. Explicit
                // Needs retry must force discovery despite that normal policy.
                if self.refresh {
                    self.requests.fetch_add(1, Ordering::SeqCst);
                }
                Ok(Page {
                    items: vec![],
                    next_offset: None,
                })
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let mut library = music_library::Library::open(temp.path().join("retry.sqlite")).unwrap();
        let credit = ArtistCreditInput {
            name: "A".into(),
            role: None,
        };
        let imported = library
            .create_catalog_release(&CatalogReleaseInput {
                title: "Example".into(),
                year: None,
                artists: vec![credit.clone()],
                tracks: (1..=12)
                    .map(|n| CatalogTrackInput {
                        title: format!("Song {n}"),
                        disc_number: Some(1),
                        track_number: Some(n),
                        artists: vec![credit.clone()],
                    })
                    .collect(),
            })
            .unwrap();
        for track in &imported.track_ids {
            library.add_to_library(track).unwrap();
        }
        library.reevaluate_stale_spotify(32).unwrap();
        let album = library.album_id_for_track(&imported.track_ids[0]).unwrap();
        assert_eq!(
            library.spotify_review_groups(None, 200).unwrap()[0].status,
            "Needs retry"
        );
        let requests = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = mpsc::channel();
        let matcher = AlbumMatcher::for_provider(
            CachedProvider {
                refresh: false,
                requests: requests.clone(),
            },
            music_library_spotify::matching_scope(),
            move |r| {
                tx.send(r).unwrap();
            },
            |_| {},
            |_| {},
        )
        .unwrap();
        let bridge = QObjectBox::new(Bridge::new(crate::session::Session::new(library)));
        let pinned = bridge.pinned();
        let mut b = pinned.borrow_mut();
        b.spotify_album_matcher = Some(matcher);
        b.browser.review_retry_after = None; // a new click starts a fresh cursor pass
        b.begin_retry_unresolved_spotify();
        let reply = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(reply.outcome, MatchOutcome::NoConfidentMatch);
        let mut matcher = b.spotify_album_matcher.take().unwrap();
        matcher.complete(&mut b.session.library, reply);
        b.spotify_album_matcher = Some(matcher);
        b.start_review_track_retry();
        assert_eq!(
            requests.load(Ordering::SeqCst),
            1,
            "twelve Tracks share one explicit Album discovery"
        );
        assert!(
            b.browser
                .review_retry_message
                .contains("1 Albums considered")
        );
        assert!(
            b.browser
                .review_retry_message
                .contains("1 requested fresh discovery")
        );
        assert!(
            !b.session.library.spotify_album_needs_retry(&album).unwrap(),
            "completed Album review clears Needs retry"
        );
        assert!(
            b.spotify_resolution_worker.is_none(),
            "absent Album never fans out to Track searches"
        );
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        b.session
            .library
            .retain_spotify_cooldown(now + 600)
            .unwrap();
        b.begin_retry_unresolved_spotify();
        assert!(
            b.browser
                .review_retry_message
                .contains("Waiting for Spotify")
        );
        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn album_batch_does_not_fan_out_to_individual_track_searches() {
        let temp = tempfile::tempdir().unwrap();
        let mut library = music_library::Library::open(temp.path().join("retry.sqlite")).unwrap();
        let artist = ArtistCreditInput {
            name: "Band".into(),
            role: None,
        };
        let imported = library
            .create_catalog_release(&CatalogReleaseInput {
                title: "Album".into(),
                year: None,
                artists: vec![artist.clone()],
                tracks: (1..=40)
                    .map(|n| CatalogTrackInput {
                        title: format!("Song {n:02}"),
                        disc_number: Some(1),
                        track_number: Some(n),
                        artists: vec![artist.clone()],
                    })
                    .collect(),
            })
            .unwrap();
        for t in &imported.track_ids {
            library.add_to_library(t).unwrap();
        }
        let later = library
            .create_catalog_release(&CatalogReleaseInput {
                title: "X’ed Out".into(),
                year: None,
                artists: vec![artist.clone()],
                tracks: (1..=8)
                    .map(|n| CatalogTrackInput {
                        title: format!("Later Song {n}"),
                        disc_number: Some(1),
                        track_number: Some(n),
                        artists: vec![artist.clone()],
                    })
                    .collect(),
            })
            .unwrap();
        for t in &later.track_ids {
            library.add_to_library(t).unwrap();
        }
        let later_album = library.album_id_for_track(&later.track_ids[0]).unwrap();
        library.mark_not_on_spotify(&imported.track_ids[0]).unwrap();
        library
            .attach_track_external_identity(
                &imported.track_ids[1],
                &ExternalIdentity {
                    provider: "spotify".into(),
                    kind: "track".into(),
                    external_id: "trusted".into(),
                },
            )
            .unwrap();
        let album = library.album_id_for_track(&imported.track_ids[0]).unwrap();
        let bridge = QObjectBox::new(Bridge::new(crate::session::Session::new(library)));
        let (_worker, requests) = crate::spotify_resolution::Worker::fake();
        let pinned = bridge.pinned();
        let mut b = pinned.borrow_mut();
        b.browser.review_retry_albums = vec![album, later_album.clone()];
        b.start_review_track_retry();
        assert!(
            requests.try_recv().is_err(),
            "Album failures must not fan out to Track searches"
        );
        assert!(b.browser.review_retry_albums.is_empty());
        assert_eq!(b.browser.review_retry_after, None);
        assert!(b.browser.review_retry_message.contains("individually"));
    }
    #[test]
    fn asynchronous_fallback_handoff_must_pull_the_next_internal_batch() {
        let mut library = music_library::Library::open_in_memory().unwrap();
        let mut albums = vec![];
        for n in 0..65 {
            let release = library
                .create_catalog_release(&CatalogReleaseInput {
                    title: format!("Album {n:03}"),
                    year: None,
                    artists: vec![ArtistCreditInput {
                        name: "Band".into(),
                        role: None,
                    }],
                    tracks: vec![CatalogTrackInput {
                        title: format!("Song {n}"),
                        disc_number: Some(1),
                        track_number: Some(1),
                        artists: vec![],
                    }],
                })
                .unwrap();
            library.add_to_library(&release.track_ids[0]).unwrap();
            albums.push(library.album_id_for_track(&release.track_ids[0]).unwrap());
        }
        let bridge = QObjectBox::new(Bridge::new(crate::session::Session::new(library)));
        let pinned = bridge.pinned();
        let mut b = pinned.borrow_mut();
        // Exact state delivered by the async fallback callback after batch 1.
        b.browser.review_retry_page_after = Some(albums[19].clone());
        b.browser
            .review_retry_seen
            .extend(albums[..20].iter().cloned());
        b.browser.review_retry_playable_started = true;
        b.start_review_track_retry();
        assert!(
            b.browser.review_retry_after.as_ref().is_some_and(|after| {
                albums
                    .iter()
                    .position(|a| a == after)
                    .is_some_and(|i| i >= 19)
            }),
            "the callback must reach batch continuation rather than returning on an empty current batch"
        );
    }
    #[test]
    fn one_click_crosses_batch_boundaries_and_overlapping_album_actions_are_coordinated() {
        run_overlapping_retry_queue(false, 23, 0);
    }
    #[test]
    fn new_artist_identity_reenqueues_earlier_contexts_through_bounded_artist_cursor() {
        run_overlapping_retry_queue(true, 23, 0);
    }
    #[test]
    fn one_start_drains_65_albums_across_four_async_batches() {
        run_overlapping_retry_queue(false, 65, 3);
    }
    #[test]
    fn one_start_drains_101_albums_across_six_async_batches() {
        run_overlapping_retry_queue(false, 101, 3);
    }
    #[test]
    fn populated_startup_automatically_drains_65_albums_without_duplicate_runs() {
        run_overlapping_retry_queue(false, 65, 1);
    }
    #[test]
    fn import_completion_automatically_drains_65_albums_only_when_ready() {
        run_overlapping_retry_queue(false, 65, 2);
    }
    fn run_overlapping_retry_queue(artist_revision: bool, count: usize, kickoff: usize) {
        use music_library::{
            Library,
            album_matching::{AlbumMatcher, MatchOutcome},
            catalog::*,
        };
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        };
        struct EmptyProvider {
            fresh: bool,
            requests: Arc<AtomicUsize>,
        }
        impl CatalogProvider for EmptyProvider {
            fn refresh_discovery(&mut self, fresh: bool) {
                self.fresh = fresh;
            }
            fn search_albums(
                &mut self,
                _: &str,
                _: u32,
            ) -> Result<Page<AlbumCandidate>, CatalogError> {
                unreachable!()
            }
            fn releases(
                &mut self,
                _: &ExternalIdentity,
                _: u32,
            ) -> Result<Page<ReleaseCandidate>, CatalogError> {
                unreachable!()
            }
            fn release(&mut self, _: &ExternalIdentity) -> Result<Release, CatalogError> {
                unreachable!()
            }
            fn search_artists(&mut self, _: &str) -> Result<Page<ArtistCandidate>, CatalogError> {
                Ok(Page {
                    items: vec![ArtistCandidate {
                        identity: ExternalIdentity {
                            provider: "spotify".into(),
                            kind: "artist".into(),
                            external_id: "band".into(),
                        },
                        name: "Band".into(),
                        aliases: vec![],
                        comment: String::new(),
                        country: String::new(),
                        artist_type: String::new(),
                        score: None,
                    }],
                    next_offset: None,
                })
            }
            fn artist_albums(
                &mut self,
                _: &ExternalIdentity,
                _: &str,
            ) -> Result<Page<ArtistAlbumCandidate>, CatalogError> {
                if self.fresh {
                    self.requests.fetch_add(1, Ordering::SeqCst);
                }
                Ok(Page {
                    items: vec![],
                    next_offset: None,
                })
            }
        }
        let mut engine = qmetaobject::QmlEngine::new();
        engine.load_data("import QtQuick\nimport QtTest\nItem { TestCase{id:t;when:false} function pump(){t.wait(10);return true;} }".into());
        let temp = tempfile::tempdir().unwrap();
        let mut library = Library::open(temp.path().join("queue.sqlite")).unwrap();
        let credit = ArtistCreditInput {
            name: "Band".into(),
            role: None,
        };
        let mut albums = vec![];
        for n in 0..count {
            let imported = library
                .create_catalog_release(&CatalogReleaseInput {
                    title: format!("Album {n:03}"),
                    year: None,
                    artists: vec![credit.clone()],
                    tracks: vec![CatalogTrackInput {
                        title: format!("Song {n:02}"),
                        artists: vec![credit.clone()],
                        disc_number: Some(1),
                        track_number: Some(1),
                    }],
                })
                .unwrap();
            library.add_to_library(&imported.track_ids[0]).unwrap();
            albums.push(library.album_id_for_track(&imported.track_ids[0]).unwrap());
        }
        library.reevaluate_stale_spotify(32).unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = mpsc::channel();
        let matcher = AlbumMatcher::for_provider(
            EmptyProvider {
                fresh: false,
                requests: requests.clone(),
            },
            music_library_spotify::matching_scope(),
            move |r| {
                tx.send(r).unwrap();
            },
            |_| {},
            |_| {},
        )
        .unwrap();
        let bridge = QObjectBox::new(Bridge::new(crate::session::Session::new(library)));
        engine.set_object_property("bridge".into(), bridge.pinned());
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.spotify_album_matcher = Some(matcher);
            b.browser.review_test_async_fallback = true;
            if kickoff == 1 {
                b.browse_action_impl("refresh", 0, String::new());
                b.browse_action_impl("refresh", 0, String::new());
            } else if kickoff == 2 {
                b.local_import_state.busy = true;
                b.request_automatic_spotify_reconciliation();
                assert!(!b.browser.review_auto_scheduled && !b.browser.review_run_active);
                b.local_import_schedule_next();
            } else if kickoff == 3 {
                b.retry_unresolved_spotify();
            } else {
                b.begin_retry_unresolved_spotify();
                assert!(b.browser.review_retry_albums.len() <= 20);
                let opened = b.spotify_review_action_impl("open", albums[0].as_ref(), 0);
                assert!(!opened.contains("database is locked"));
                let queued = b.spotify_review_action_impl("retry", albums[count - 1].as_ref(), 0);
                assert!(queued.contains("running reconciliation queue"));
            }
        }
        let mut processed = 0;
        for _ in 0..3000 {
            engine.invoke_method("pump".into(), &[]);
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            if let Ok(reply) = rx.try_recv() {
                assert_eq!(reply.outcome, MatchOutcome::NoConfidentMatch);
                let mut matcher = b.spotify_album_matcher.take().unwrap();
                matcher.complete(&mut b.session.library, reply);
                b.spotify_album_matcher = Some(matcher);
                processed += 1;
                if artist_revision && processed == 22 {
                    let music_library::album_matching::Preparation::Ready(input) = b
                        .session
                        .library
                        .spotify_album_review_input(&albums[0])
                        .unwrap()
                    else {
                        panic!("fixture Artist required");
                    };
                    b.session
                        .library
                        .attach_artist_external_identity(
                            &input.artist_id,
                            &ExternalIdentity {
                                provider: "spotify".into(),
                                kind: "artist".into(),
                                external_id: "established".into(),
                            },
                        )
                        .unwrap();
                    b.enqueue_spotify_artist_revision(input.artist_id);
                }
                b.start_review_track_retry();
                b.spotify_review_action_impl("open", albums[processed % count].as_ref(), 0);
            }
            assert!(b.browser.error.is_empty(), "{}", b.browser.error);
            if !b.browser.review_run_active
                && !b.browser.review_local_active
                && !b.browser.review_auto_scheduled
                && b.browser.review_retry_albums.is_empty()
                && !b.browser.review_retry_local_active
                && b.browser.review_retry_extra.is_empty()
            {
                break;
            }
        }
        let pinned = bridge.pinned();
        let b = pinned.borrow();
        assert_eq!(
            processed, count,
            "one click visits every context, including the Album enqueued during work"
        );
        assert_eq!(requests.load(Ordering::SeqCst), count);
        assert_eq!(
            b.browser.review_retry_totals[0],
            if artist_revision { 45 } else { count as u64 },
            "new Artist evidence revisits contexts locally without more discovery requests"
        );
        assert!(b.browser.review_retry_artists.is_empty());
        assert_eq!(b.browser.review_retry_seen.len(), count);
        assert_eq!(
            b.session.library.unresolved_spotify_count().unwrap(),
            count as u64
        );
        assert!(
            b.session
                .library
                .spotify_review_groups(None, 200)
                .unwrap()
                .iter()
                .all(|g| g.status == "No candidates")
        );
        assert!(!b.browser.review_retry_message.contains("click Retry"));
        if kickoff > 0 {
            assert_eq!(b.browser.review_run_batches, count.div_ceil(20));
        }
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.request_automatic_spotify_reconciliation();
        }
        for _ in 0..20 {
            engine.invoke_method("pump".into(), &[]);
        }
        assert_eq!(
            requests.load(Ordering::SeqCst),
            count,
            "stable negative discovery is not automatically searched again"
        );
    }
}
