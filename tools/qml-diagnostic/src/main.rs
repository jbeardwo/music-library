mod add_music;
mod artwork;
mod browser;
mod catalog;
mod library_search;
#[cfg(feature = "gstreamer")]
mod local;
mod local_import;
mod metadata;
mod playback_route;
mod player_controls;
mod sample;
mod session;
use music_library_spotify::playback_completion as spotify_completion;
mod spotify_enrichment;
mod spotify_playback;
mod spotify_playlist;
mod spotify_resolution;

use qmetaobject::prelude::*;
use qmetaobject::{QVariantList, QVariantMap};
use session::Session;

#[derive(QObject)]
struct Bridge {
    base: qt_base_class!(trait QObject),
    spotify_playlist_state: spotify_playlist::State,
    spotify_playlist_snapshot: qt_property!(QVariantMap; READ spotify_playlist_value NOTIFY spotify_playlist_changed),
    spotify_playlist_changed: qt_signal!(),
    spotify_playlist_action: qt_method!(
        fn spotify_playlist_action(&mut self, action: String, input: String) {
            self.spotify_playlist_action_impl(action, input);
        }
    ),
    active_backend: music_library::playback_resolver::ActiveBackend,
    route_pending: Option<playback_route::Pending>,
    automatic_song_search: bool,
    preserve_play_queue: bool,
    restart_playback: bool,
    playback_generation: u64,
    route_message: String,
    resolver_dialog_requested: qt_signal!(),
    spotify_reconcile_playlists: std::collections::HashSet<String>,
    spotify_album_matcher: Option<music_library::album_matching::AlbumMatcher>,
    spotify_resolution_worker: Option<spotify_resolution::Worker>,
    spotify_resolution_generation: u64,
    spotify_resolution_selection: Option<music_library::song_resolution::Selection>,
    spotify_resolution_more: bool,
    spotify_artist_proposal: Option<music_library::artist_equivalence::Proposal>,
    spotify_artist_equivalence: qt_method!(
        fn spotify_artist_equivalence(&mut self, action: String, index: i32) {
            if action == "cancel" {
                self.spotify_artist_proposal = None;
            }
            if action == "prepare" {
                self.spotify_artist_proposal = None;
                let result = self
                    .spotify_resolution_selection
                    .as_ref()
                    .and_then(|selection| {
                        selection
                            .visible_indices()
                            .get(index as usize)
                            .copied()
                            .map(|i| (selection, i))
                    })
                    .ok_or_else(|| "Select a Spotify candidate first".to_owned())
                    .and_then(|(selection, i)| {
                        let artist = selection.candidates()[i]
                            .artists
                            .first()
                            .ok_or("Candidate has no identified Artist")?;
                        self.session
                            .library
                            .prepare_artist_equivalence(&selection.input().track_id, artist)
                            .map_err(|e| e.to_string())
                    });
                match result {
                    Ok(p) => self.spotify_artist_proposal = Some(p),
                    Err(e) => self.spotify_resolution_message = e,
                }
            }
            if action == "confirm"
                && let Some(proposal) = self.spotify_artist_proposal.take()
            {
                match self.session.library.confirm_artist_equivalence(&proposal) {
                    Ok(_) => {
                        self.spotify_resolution_message="Artist equivalence saved. Re-evaluating this Album with all other safeguards intact.".into();
                        if let Some(selection) = self.spotify_resolution_selection.take()
                            && let Ok(input) =
                                self.session.library.song_resolution_input(&proposal.track)
                        {
                            let mut refreshed = music_library::song_resolution::Selection::new(
                                input,
                                selection.candidates().to_vec(),
                            );
                            refreshed.show_all();
                            self.spotify_resolution_selection = Some(refreshed);
                        }
                        self.retry_selected_spotify_album();
                        self.refresh_spotify_review();
                    }
                    Err(e) => self.spotify_resolution_message = e.to_string(),
                }
            }
            self.spotify_playback_changed();
        }
    ),
    spotify_resolution_pending: bool,
    spotify_resolution_message: String,
    spotify_resolution_counts: (u64, u64),
    spotify_resolve: qt_method!(
        fn spotify_resolve(&mut self, action: String, index: i32) {
            if action == "cancel" {
                self.automatic_song_search = false;
                self.spotify_resolution_generation += 1;
                self.spotify_resolution_selection = None;
                self.spotify_artist_proposal = None;
                self.spotify_resolution_pending = false;
                self.spotify_resolution_message =
                    "Selection canceled; no association changed".into();
                self.spotify_playback_changed();
                return;
            }
            if action == "show-all" {
                if let Some(selection) = &mut self.spotify_resolution_selection {
                    selection.show_all();
                }
                self.spotify_playback_changed();
                return;
            }
            if action == "confirm" {
                let result = self
                    .spotify_resolution_selection
                    .as_ref()
                    .ok_or_else(|| "Search and explicitly select a candidate first".to_owned())
                    .and_then(|selection| {
                        self.session
                            .library
                            .confirm_song_resolution(
                                selection,
                                *selection
                                    .visible_indices()
                                    .get(index as usize)
                                    .ok_or_else(|| "Select a visible candidate".to_owned())?,
                            )
                            .map_err(|e| e.to_string())
                    });
                match result {
                    Ok(id) => {
                        self.browse_action_impl("refresh", 0, String::new());
                        self.spotify_playback_song =
                            music_library_spotify::playback::Song::from_associations(&[id]).ok();
                        self.spotify_resolution_selection = None;
                        self.spotify_resolution_message =
                            "Spotify song manually confirmed; ready to play".into();
                        self.spotify_playback_state.error = None;
                    }
                    Err(error) => self.spotify_resolution_message = error,
                }
                self.spotify_playback_changed();
                return;
            }
            let artist_query = action
                .strip_prefix("search-artist:")
                .filter(|s| !s.trim().is_empty())
                .map(str::to_owned);
            if (action != "search" && action != "refresh" && artist_query.is_none())
                || self.spotify_resolution_pending
            {
                return;
            }
            let Some(track) = self.spotify_playback_track.clone() else {
                return;
            };
            if self
                .session
                .library
                .track_provider_occurrences(&track, "spotify")
                .is_ok_and(|ids| !ids.is_empty())
            {
                self.spotify_resolution_message =
                    "Already associated; replacement is not supported here".into();
                self.spotify_playback_changed();
                return;
            }
            if self
                .session
                .library
                .spotify_manually_excluded(&track)
                .unwrap_or(true)
            {
                self.spotify_resolution_message =
                    "Manually marked not on Spotify. Choose Check Spotify again first.".into();
                self.spotify_playback_changed();
                return;
            }
            let input = match self.session.library.song_resolution_input(&track) {
                Ok(input) => input,
                Err(error) => {
                    self.spotify_resolution_message = error.to_string();
                    self.spotify_playback_changed();
                    return;
                }
            };
            if self.spotify_resolution_worker.is_none() {
                let weak = qmetaobject::QPointer::from(&*self);
                let callback =
                    qmetaobject::queued_callback(move |reply: spotify_resolution::Reply| {
                        if let Some(pinned) = weak.as_pinned() {
                            let mut b = pinned.borrow_mut();
                            b.finish_song_lookup(reply);
                        }
                    });
                self.spotify_resolution_worker = Some(spotify_resolution::Worker::with_library(
                    callback,
                    self.session.library.database_path(),
                ));
            }
            if action == "refresh" {
                self.spotify_resolution_worker
                    .as_ref()
                    .unwrap()
                    .refresh_next_search();
            }
            self.spotify_resolution_generation += 1;
            self.spotify_resolution_selection = None;
            self.spotify_resolution_pending = self
                .spotify_resolution_worker
                .as_ref()
                .unwrap()
                .search_with_artist(self.spotify_resolution_generation, input, artist_query);
            self.spotify_resolution_message = if self.spotify_resolution_pending {
                "Searching Spotify catalog using Client Credentials…".into()
            } else {
                "Catalog worker busy; retry after current request finishes".into()
            };
            self.spotify_playback_changed();
        }
    ),
    spotify_playback_worker: Option<spotify_playback::Worker>,
    spotify_playback_state: music_library_spotify::playback::Snapshot,
    // Diagnostic selection must not become the active remote clock identity.
    spotify_active_song: Option<music_library_spotify::playback::Song>,
    spotify_playback_song: Option<music_library_spotify::playback::Song>,
    spotify_playback_track: Option<music_library::domain::TrackId>,
    spotify_playback_title: String,
    spotify_album_id: Option<music_library::domain::AlbumId>,
    spotify_album_explanation: String,
    spotify_album_refresh: qt_method!(
        fn spotify_album_refresh(&mut self) {
            self.refresh_selected_spotify_album();
            self.spotify_playback_changed();
        }
    ),
    spotify_album_retry: qt_method!(
        fn spotify_album_retry(&mut self) {
            self.retry_selected_spotify_album();
        }
    ),
    spotify_playback_snapshot: qt_property!(QVariantMap; READ spotify_playback_value NOTIFY spotify_playback_changed),
    spotify_playback_changed: qt_signal!(),
    spotify_playback_action: qt_method!(
        fn spotify_playback_action(&mut self, action: String, value: String) {
            use spotify_playback::Command;
            if self.route_pending.is_some()
                && matches!(
                    action.as_str(),
                    "play" | "pause" | "seek" | "device" | "connect" | "cancel"
                )
            {
                return;
            }
            if action == "track" {
                self.automatic_song_search = false;
                self.spotify_resolution_generation += 1;
                self.spotify_resolution_selection = None;
                self.spotify_artist_proposal = None;
                self.spotify_resolution_pending = false;
                self.spotify_resolution_message.clear();
                self.spotify_playback_track = Some(music_library::domain::TrackId(value.clone()));
                self.spotify_playback_title = self
                    .session
                    .library
                    .song_resolution_input(&music_library::domain::TrackId(value.clone()))
                    .ok()
                    .map(|r| format!("{} — {} [{}]", r.artist, r.title, r.album))
                    .unwrap_or_else(|| value.clone());
                self.spotify_playback_song = self
                    .session
                    .library
                    .track_provider_occurrences(&music_library::domain::TrackId(value), "spotify")
                    .ok()
                    .and_then(|ids| {
                        music_library_spotify::playback::Song::from_associations(&ids).ok()
                    });
                if self.spotify_playback_song.is_none()
                    && let Some(path) = self.session.library.database_path()
                    && let Ok(market) = std::env::var("SPOTIFY_MARKET")
                    && let Some(track) = &self.spotify_playback_track
                    && let Ok(input) = self.session.library.song_resolution_input(track)
                    && !input.spotify_excluded
                {
                    match music_library_spotify::Spotify::cached_songs_in_library(
                        &path, &market, &input,
                    ) {
                        Ok(Some(page)) => {
                            self.spotify_resolution_more = page.next_offset.is_some();
                            self.spotify_resolution_message = format!(
                                "Fresh cached Spotify discovery: {} candidates. No provider requests; Search Spotify again performs a refresh.",
                                page.items.len()
                            );
                            let mut selection =
                                music_library::song_resolution::Selection::new(input, page.items);
                            selection.show_all();
                            self.spotify_resolution_selection = Some(selection);
                        }
                        Ok(None) => {}
                        Err(error) => self.spotify_resolution_message = error.to_string(),
                    }
                }
                self.refresh_spotify_album_diagnostic();
                self.spotify_playback_changed();
                return;
            }
            if self.spotify_playback_worker.is_none() {
                let weak = qmetaobject::QPointer::from(&*self);
                let callback =
                    qmetaobject::queued_callback(move |update: spotify_playback::Update| {
                        if let Some(pinned) = weak.as_pinned() {
                            let mut bridge = pinned.borrow_mut();
                            bridge.apply_player_update(update);
                        }
                    });
                self.spotify_playback_worker = Some(spotify_playback::Worker::new(callback));
            }
            let command = match action.as_str() {
                "connect" => Command::Connect,
                "cancel" => Command::Cancel,
                "refresh" => Command::Refresh,
                "device" => Command::Select(value),
                "visible" => Command::Visible(value == "true"),
                "pause" => {
                    self.playback_generation += 1;
                    Command::Pause
                }
                "seek" => {
                    let Ok(ms) = value.parse() else {
                        return;
                    };
                    self.playback_generation += 1;
                    Command::Seek(ms, self.playback_generation)
                }
                "play" => {
                    if self.route_pending.is_some() {
                        return;
                    }
                    // Re-read accepted evidence, respecting manual clear/change since
                    // the row was selected. No catalog call or global library scan.
                    self.spotify_playback_song = self
                        .spotify_playback_track
                        .as_ref()
                        .and_then(|track| {
                            self.session
                                .library
                                .track_provider_occurrences(track, "spotify")
                                .ok()
                        })
                        .and_then(|ids| {
                            music_library_spotify::playback::Song::from_associations(&ids).ok()
                        });
                    let Some(song) = self.spotify_playback_song.clone() else {
                        self.spotify_playback_state.error =
                            Some(music_library_spotify::playback::Error::NoAssociation);
                        self.spotify_playback_changed();
                        return;
                    };
                    if self.real_audio {
                        let track = self.spotify_playback_track.clone().unwrap();
                        let ids = self
                            .session
                            .library
                            .track_provider_occurrences(&track, "spotify")
                            .unwrap_or_default();
                        if let Some(id) = ids.into_iter().find(|id| {
                            music_library_spotify::playback::Song::from_associations(
                                std::slice::from_ref(id),
                            )
                            .is_ok()
                        }) {
                            self.preserve_play_queue =
                                self.session.playback.state().current_track() == Some(&track);
                            self.start_resolved_remote(track, id);
                            self.changed();
                        }
                        return;
                    }
                    Command::Play(song)
                }
                _ => return,
            };
            if !self.spotify_playback_worker.as_ref().unwrap().send(command) {
                self.spotify_playback_state.error =
                    Some(music_library_spotify::playback::Error::Configuration);
                self.spotify_playback_worker = None;
                self.spotify_playback_changed();
            }
        }
    ),
    spotify: bool,
    catalog_providers: Vec<String>,
    auto_match: bool,
    stored_programs: std::collections::HashMap<
        music_library::domain::AlbumId,
        music_library::album_program::Outcome,
    >,
    snapshot: qt_property!(QVariantMap; READ snapshot_value NOTIFY changed),
    changed: qt_signal!(),
    catalog_snapshot: qt_property!(QVariantMap; READ catalog_snapshot_value NOTIFY catalog_changed),
    catalog_changed: qt_signal!(),
    matching_snapshot: qt_property!(QVariantList; READ matching_snapshot_value NOTIFY matching_changed),
    matching_changed: qt_signal!(),
    manual_snapshot: qt_property!(QVariantMap; READ manual_snapshot_value NOTIFY manual_changed),
    manual_changed: qt_signal!(),
    choose_track: qt_method!(
        fn choose_track(&mut self, album: String, track: String) {
            self.manual_context = Some((
                music_library::domain::AlbumId(album),
                music_library::domain::TrackId(track),
            ));
            self.manual_selection = None;
            self.manual_error.clear();
            let result = self
                .ensure_matcher()
                .and_then(|()| self.load_manual_choices());
            match result {
                Ok(false) => {
                    let album = self.manual_context.as_ref().unwrap().0.clone();
                    if let Err(e) = self
                        .matcher
                        .as_mut()
                        .unwrap()
                        .request_album_programs(&self.session.library, &album)
                    {
                        self.manual_error = e.to_string();
                    }
                }
                Err(e) => self.manual_error = e,
                Ok(true) => {}
            }
            self.manual_changed();
            self.matching_changed();
        }
    ),
    confirm_track: qt_method!(
        fn confirm_track(&mut self, index: i32) -> bool {
            let result = if let Some(selection) = &self.manual_selection {
                self.session
                    .library
                    .confirm_manual_track(selection, index as usize)
                    .map(|_| selection.album_id().clone())
                    .map_err(|e| e.to_string())
            } else {
                Err("Wait for candidates and explicitly choose one".into())
            };
            match result {
                Ok(album) => {
                    self.browse_action_impl("refresh", 0, String::new());
                    if let Err(e) = self.refresh_manual_album(&album) {
                        self.manual_error = e;
                        self.manual_changed();
                        return false;
                    }
                    self.manual_selection = None;
                    self.manual_context = None;
                    self.manual_error.clear();
                    self.manual_changed();
                    self.matching_changed();
                    true
                }
                Err(e) => {
                    self.manual_error = e;
                    self.manual_changed();
                    false
                }
            }
        }
    ),
    cancel_track_choice: qt_method!(
        fn cancel_track_choice(&mut self) {
            self.manual_context = None;
            self.manual_selection = None;
            self.manual_error.clear();
            self.manual_changed();
        }
    ),
    clear_track_choice: qt_method!(
        fn clear_track_choice(&mut self, album: String, track: String) {
            let album = music_library::domain::AlbumId(album);
            let provider = self.provider_for(&album);
            let result = self
                .session
                .library
                .clear_manual_track_for(&album, &music_library::domain::TrackId(track), &provider)
                .map_err(|e| e.to_string())
                .and_then(|_| self.refresh_manual_album(&album));
            if let Err(e) = result {
                self.session.error = e;
                self.changed();
                return;
            }
            if let Some(matcher) = self.matcher.as_mut() {
                matcher.clear_program_outcome(&album);
                if let Err(e) = matcher.request_album_programs(&self.session.library, &album) {
                    self.session.error = e.to_string();
                    self.changed();
                }
            }
            self.matching_changed();
        }
    ),
    manual_context: Option<(
        music_library::domain::AlbumId,
        music_library::domain::TrackId,
    )>,
    manual_selection: Option<music_library::manual_track::Selection>,
    manual_error: String,
    manual_associations: std::collections::HashMap<
        music_library::domain::AlbumId,
        Vec<music_library::manual_track::Association>,
    >,
    matching_provider: qt_property!(QVariantMap; READ matching_provider_value NOTIFY matching_changed),
    retry_matching: qt_method!(
        fn retry_matching(&mut self) {
            if let Some(matcher) = self.matcher.as_mut() {
                if let Err(error) = matcher.retry_catalog_matching(&self.session.library) {
                    self.session.error = error.to_string();
                    self.changed();
                }
                self.matching_changed();
            }
        }
    ),

    retry_match: qt_method!(
        fn retry_match(&mut self, index: i32) {
            if let Some((id, _, _, _)) = self.matching_rows.get(index as usize) {
                let id = id.clone();
                let outcome = self.ensure_matcher().and_then(|()| {
                    self.matcher
                        .as_mut()
                        .unwrap()
                        .match_album(&self.session.library, &id)
                        .map_err(|e| e.to_string())
                });
                self.matching_rows[index as usize].2 =
                    outcome.unwrap_or_else(music_library::album_matching::MatchOutcome::Error);
                self.matching_changed();
            }
        }
    ),
    choose_artist: qt_method!(
        fn choose_artist(&mut self, row: i32, choice: i32) {
            if row < 0 || choice < 0 {
                return;
            }
            if let Some((id, _, _, _)) = self.matching_rows.get(row as usize) {
                let id = id.clone();
                if let Some(matcher) = self.matcher.as_mut() {
                    let outcome = matcher
                        .select_artist(&mut self.session.library, &id, choice as usize)
                        .unwrap_or_else(|e| {
                            music_library::album_matching::MatchOutcome::Error(e.to_string())
                        });
                    self.matching_rows[row as usize].2 = outcome;
                    self.matching_changed();
                }
            }
        }
    ),
    matcher: Option<music_library::provider_chain::ProviderChain>,
    matching_tracks: std::collections::HashMap<
        music_library::domain::AlbumId,
        Vec<music_library::edition::LocalTrackEvidence>,
    >,
    matching_rows: Vec<(
        music_library::domain::AlbumId,
        String,
        music_library::album_matching::MatchOutcome,
        String,
    )>,
    queue_offset: usize,
    queue_window: qt_method!(
        fn queue_window(&mut self, offset: i32) {
            self.queue_offset = (offset.max(0) as usize / 200) * 200;
            self.changed();
        }
    ),
    library_search: library_search::Search,
    local_search_snapshot: qt_property!(QVariantMap; READ local_search_value NOTIFY local_search_changed),
    local_search_changed: qt_signal!(),
    search_library: qt_method!(
        fn search_library(&mut self, text: String, filter: i32) {
            self.local_search_start(text, filter);
        }
    ),
    close_search: qt_method!(
        fn close_search(&mut self) {
            self.local_search_cancel();
        }
    ),
    navigate_search_result: qt_method!(
        fn navigate_search_result(&mut self, index: i32) -> bool {
            self.local_search_go(index)
        }
    ),
    artwork: artwork::Artwork,
    artwork_snapshot: qt_property!(QVariantMap; READ artwork_value NOTIFY artwork_changed),
    artwork_changed: qt_signal!(),
    artwork_retry: qt_method!(
        fn artwork_retry(&mut self, key: String) {
            self.retry_artwork(key);
        }
    ),
    artwork_batch: qt_method!(
        fn artwork_batch(&mut self, keys: QVariantList) {
            self.request_artwork(
                keys.into_iter()
                    .map(|v| v.to_qstring().to_string())
                    .collect(),
            );
        }
    ),
    metadata_state: metadata::State,
    metadata_snapshot: qt_property!(QString; READ metadata_value NOTIFY metadata_changed),
    metadata_changed: qt_signal!(),
    metadata_action: qt_method!(
        fn metadata_action(&mut self, action: String, kind: String, id: String, payload: String) {
            self.metadata_action_impl(&action, &kind, id, payload);
        }
    ),
    browser: browser::Browser,
    browse_snapshot: qt_property!(QVariantMap; READ browse_value NOTIFY browse_changed),
    browse_changed: qt_signal!(),
    browse_action: qt_method!(
        fn browse_action(&mut self, action: String, pane: i32, id: String) {
            self.browse_action_impl(&action, pane as usize, id);
        }
    ),
    local_import_state: local_import::State,
    local_import_snapshot: qt_property!(QVariantMap; READ local_import_value NOTIFY local_import_changed),
    local_import_changed: qt_signal!(),
    local_import: qt_method!(
        fn local_import(&mut self, kind: String, urls: QVariantList) {
            self.local_import_urls(&kind, urls);
        }
    ),
    local_import_schedule: qt_method!(
        fn local_import_schedule(&mut self) {
            self.local_import_schedule_next();
        }
    ),
    controls: player_controls::Controls,
    output_trims: qt_method!(
        fn output_trims(&mut self, local: f64, spotify: f64) {
            self.set_output_trims(local, spotify);
            self.changed();
        }
    ),
    seek: qt_method!(
        fn seek(&mut self, milliseconds: f64) {
            self.player_seek(milliseconds);
            self.changed();
        }
    ),
    refresh_clock: qt_method!(
        fn refresh_clock(&mut self) {
            self.changed();
        }
    ),
    set_volume: qt_method!(
        fn set_volume(&mut self, value: f64) {
            self.player_volume(value);
            self.changed();
        }
    ),
    search: qt_method!(
        fn search(&mut self, text: String) {
            self.session.search(text);
            self.changed();
        }
    ),
    page_next: qt_method!(
        fn page_next(&mut self) {
            self.session.page_next();
            self.changed();
        }
    ),
    page_previous: qt_method!(
        fn page_previous(&mut self) {
            self.session.page_previous();
            self.changed();
        }
    ),
    queue_page: qt_method!(
        fn queue_page(&mut self) {
            self.replace_resolved_queue_page();
            self.changed();
        }
    ),
    enqueue_row: qt_method!(
        fn enqueue_row(&mut self, id: String) {
            self.session.enqueue_row(&id);
            self.changed();
        }
    ),
    clear_queue: qt_method!(
        fn clear_queue(&mut self) {
            self.clear_resolved_queue();
            self.changed();
        }
    ),
    play_row: qt_method!(
        fn play_row(&mut self, id: String) {
            self.resolve_play(&id);
            self.changed();
        }
    ),
    command: qt_method!(
        fn command(&mut self, command: String) {
            if self.real_audio {
                if self.route_pending.is_some() {
                    return;
                }
                if self.automatic_song_search {
                    if command == "stop" {
                        self.spotify_resolve("cancel".into(), -1);
                    } else {
                        self.route_message =
                            "Spotify lookup in progress; Stop cancels this Play request".into();
                        self.changed();
                        return;
                    }
                }
                if command == "play"
                    && let Some(track) = self.session.playback.state().current_track().cloned()
                {
                    self.resolve_current_play(track.as_ref());
                    self.changed();
                    return;
                }
                if matches!(command.as_str(), "next" | "previous") {
                    self.navigate_queue(command == "previous");
                    self.changed();
                    return;
                }
                if matches!(
                    self.active_backend,
                    music_library::playback_resolver::ActiveBackend::Remote(_)
                ) {
                    if matches!(command.as_str(), "pause" | "stop") {
                        self.spotify_playback_action("pause".into(), String::new());
                        self.route_message = "Spotify pause requested".into();
                    } else {
                        self.route_message = "Remote queue advancement is not implemented; select a Track and press Play".into();
                    }
                    self.changed();
                    return;
                }
            }
            self.session.command(&command);
            self.changed();
        }
    ),
    toggle_failure: qt_method!(
        fn toggle_failure(&mut self) {
            self.session.toggle_failure();
            self.changed();
        }
    ),
    catalog_action: qt_method!(
        fn catalog_action(&mut self, action: String, value: String) {
            if self.catalog.pending {
                return;
            }
            let result = self.start_catalog(&action, &value);
            if let Err(error) = result {
                self.catalog.status = error;
                self.catalog.pending = false;
            }
            self.catalog_changed();
        }
    ),
    music_snapshot: qt_property!(QVariantMap; READ music_value NOTIFY music_changed),
    music_changed: qt_signal!(),
    add_music_action: qt_method!(
        fn add_music_action(&mut self, action: String, value: String) {
            self.music_action(action, value);
        }
    ),
    add_music: add_music::State,
    catalog: catalog::State,
    session: Session,
    real_audio: bool,
}

fn string(value: impl AsRef<str>) -> QVariant {
    QString::from(value.as_ref()).into()
}

fn row_value(row: &music_library::domain::TrackSearchResult) -> QVariant {
    let map: QVariantMap = [
        ("trackId", string(row.track_id.as_ref())),
        ("title", string(&row.title)),
        ("artist", string(&row.artist_names)),
        ("release", string(&row.release_title)),
        ("available", row.available.into()),
    ]
    .into_iter()
    .collect();
    map.into()
}

impl Bridge {
    fn finish_song_lookup(&mut self, reply: spotify_resolution::Reply) {
        let reused = reply.counts == self.spotify_resolution_counts;
        self.spotify_resolution_counts = reply.counts;
        if reply.generation == self.spotify_resolution_generation {
            self.spotify_resolution_pending = false;
            match reply.result {
                Ok(page) => {
                    if let Err(e) = self
                        .session
                        .library
                        .persist_spotify_song_review(&reply.input, &page)
                    {
                        self.session.error = e.to_string();
                    }
                    self.refresh_spotify_review();
                    let automatic = self.automatic_song_search;
                    self.automatic_song_search = false;
                    if automatic {
                        self.finish_automatic_song(reply.input, page);
                        self.spotify_playback_changed();
                        self.changed();
                        return;
                    }
                    let applied = self
                        .session
                        .library
                        .apply_song_evaluation(&reply.input, &page);
                    self.refresh_spotify_review();
                    self.spotify_resolution_message = match applied {
                        Ok(Some(_)) => "Accepted Spotify association saved".into(),
                        Err(error) => error.to_string(),
                        Ok(None) => {
                            if page.items.is_empty() {
                                "No candidates on this bounded page".into()
                            } else if page.next_offset.is_some() {
                                "First 10 results only; choose explicitly. More provider results exist.".into()
                            } else {
                                "No automatic match; inspect the final blocker or connect explicitly".into()
                            }
                        }
                    };
                    if reused {
                        self.spotify_resolution_message.push_str(
                            ". Fresh cached Spotify discovery reused; no provider requests.",
                        );
                    }
                    self.spotify_resolution_more = page.next_offset.is_some();
                    let mut selection =
                        music_library::song_resolution::Selection::new(reply.input, page.items);
                    // Diagnostics expose rejected candidates too; selection remains explicit.
                    selection.show_all();
                    self.spotify_resolution_selection = Some(selection);
                }
                Err(error) => {
                    self.automatic_song_search = false;
                    self.route_message = format!("Spotify association lookup failed: {error}");
                    self.spotify_resolution_message = error.to_string();
                    self.changed();
                }
            }
        }
        self.spotify_playback_changed();
    }

    fn new(mut session: Session) -> Self {
        let trims = match session.library.output_trims() {
            Ok(trims) => trims,
            Err(error) => {
                session.error = error.to_string();
                Default::default()
            }
        };
        if let Err(error) = session.playback.set_output_trim(trims.local_db) {
            session.error = error.to_string();
        }
        Self {
            base: Default::default(),
            active_backend: Default::default(),
            route_pending: None,
            automatic_song_search: false,
            preserve_play_queue: false,
            restart_playback: false,
            playback_generation: 0,
            route_message: String::new(),
            resolver_dialog_requested: Default::default(),
            spotify_reconcile_playlists: Default::default(),
            spotify_album_matcher: None,
            spotify_playlist_state: Default::default(),
            spotify_playlist_action: Default::default(),
            spotify_playlist_changed: Default::default(),
            spotify_playlist_snapshot: Default::default(),
            spotify_resolution_worker: None,
            spotify_resolution_generation: 0,
            spotify_resolution_selection: None,
            spotify_resolution_more: false,
            spotify_artist_proposal: None,
            spotify_artist_equivalence: Default::default(),
            spotify_resolution_pending: false,
            spotify_resolution_message: String::new(),
            spotify_resolution_counts: (0, 0),
            spotify_resolve: Default::default(),
            spotify_playback_worker: None,
            spotify_playback_state: Default::default(),
            spotify_active_song: None,
            spotify_playback_song: None,
            spotify_playback_track: None,
            spotify_playback_title: String::new(),
            spotify_album_id: None,
            spotify_album_explanation: String::new(),
            spotify_album_retry: Default::default(),
            spotify_album_refresh: Default::default(),
            spotify_playback_snapshot: Default::default(),
            spotify_playback_changed: Default::default(),
            spotify_playback_action: Default::default(),
            snapshot: Default::default(),
            changed: Default::default(),
            catalog_snapshot: Default::default(),
            catalog_changed: Default::default(),
            matching_snapshot: Default::default(),
            matching_changed: Default::default(),
            manual_snapshot: Default::default(),
            manual_changed: Default::default(),
            choose_track: Default::default(),
            confirm_track: Default::default(),
            cancel_track_choice: Default::default(),
            clear_track_choice: Default::default(),
            manual_context: None,
            manual_selection: None,
            manual_error: String::new(),
            manual_associations: Default::default(),
            spotify: false,
            catalog_providers: vec!["musicbrainz".into()],
            auto_match: true,
            stored_programs: Default::default(),
            matching_provider: Default::default(),
            retry_matching: Default::default(),
            retry_match: Default::default(),
            choose_artist: Default::default(),
            matcher: None,
            matching_tracks: Default::default(),
            matching_rows: Vec::new(),
            queue_offset: 0,
            queue_window: Default::default(),
            library_search: Default::default(),
            local_search_snapshot: Default::default(),
            local_search_changed: Default::default(),
            search_library: Default::default(),
            close_search: Default::default(),
            navigate_search_result: Default::default(),
            artwork: Default::default(),
            artwork_snapshot: Default::default(),
            artwork_changed: Default::default(),
            artwork_retry: Default::default(),
            artwork_batch: Default::default(),
            metadata_state: Default::default(),
            metadata_snapshot: Default::default(),
            metadata_changed: Default::default(),
            metadata_action: Default::default(),
            browser: Default::default(),
            browse_snapshot: Default::default(),
            browse_changed: Default::default(),
            browse_action: Default::default(),
            controls: player_controls::Controls::new(trims),
            output_trims: Default::default(),
            seek: Default::default(),
            refresh_clock: Default::default(),
            set_volume: Default::default(),
            search: Default::default(),
            page_next: Default::default(),
            page_previous: Default::default(),
            queue_page: Default::default(),
            play_row: Default::default(),
            enqueue_row: Default::default(),
            clear_queue: Default::default(),
            command: Default::default(),
            toggle_failure: Default::default(),
            catalog_action: Default::default(),
            local_import_state: Default::default(),
            local_import_snapshot: Default::default(),
            local_import_changed: Default::default(),
            local_import: Default::default(),
            local_import_schedule: Default::default(),
            music_snapshot: Default::default(),
            music_changed: Default::default(),
            add_music_action: Default::default(),
            add_music: Default::default(),
            catalog: Default::default(),
            session,
            real_audio: false,
        }
    }

    fn spotify_program_comparison(
        album: &str,
        p: &music_library::album_program::PositionedProgram,
    ) -> serde_json::Value {
        serde_json::json!({"album":album,"complete":p.complete,"anchors":p.anchors,"provider_count":p.provider_count,"duplicate_positions":p.duplicate_positions,"rows":p.tracks.iter().take(200).map(|t|serde_json::json!({"local":format!("{}.{} {}",t.normalized_disc,t.local.number.unwrap_or(0),t.local.title.as_deref().unwrap_or("—")),"candidate":t.provider.as_ref().map(|p|format!("{}.{} {}",p.disc.unwrap_or(1),p.number.unwrap_or(0),p.title.as_deref().unwrap_or("—"))).unwrap_or_else(||"—".into()),"decision":t.decision.label()})).collect::<Vec<_>>(),"truncated":p.tracks.len()>200})
    }
    fn spotify_comparison_json(&self) -> String {
        use music_library::{catalog::Page, song_resolution::evaluate_attempt};
        let mut result = serde_json::json!({"candidate_count":0,"candidates":[],"programs":[],"album_candidates":[]});
        if let Some(track) = &self.spotify_playback_track
            && let Ok((code, reason)) = self.session.library.spotify_review_summary(track)
        {
            result["latest_review"] = serde_json::json!({"code":code,"reason":reason});
        }
        if let Some(selection) = &self.spotify_resolution_selection {
            let page = Page {
                items: selection.candidates().to_vec(),
                next_offset: self.spotify_resolution_more.then_some(10),
            };
            let trusted = self
                .session
                .library
                .diagnostic_track_identities(&selection.input().track_id)
                .unwrap_or_default();
            let current = self
                .session
                .library
                .song_resolution_input(&selection.input().track_id)
                .unwrap_or_else(|_| selection.input().clone());
            result["candidate_count"] = serde_json::json!(page.items.len());
            result["more_candidates"] = serde_json::json!(self.spotify_resolution_more);
            result["candidates"] = serde_json::json!(
                selection
                    .visible_indices()
                    .iter()
                    .filter_map(|i| evaluate_attempt(
                        selection.input(),
                        &current,
                        &page,
                        *i,
                        &trusted
                    ))
                    .collect::<Vec<_>>()
            );
        }
        if let Some(report) = self.spotify_album_id.as_ref().and_then(|id| {
            self.spotify_album_matcher
                .as_ref()
                .and_then(|m| m.diagnostic(id))
        }) {
            result["album_candidates"]=serde_json::json!(report.candidates.iter().map(|c|serde_json::json!({"id":c.identity.external_id,"title":c.title.provider_raw,"decision":c.decision,"primary_blocker":if c.decision=="Accepted" { None } else { c.reasons.first().map(|r|r.label()).or_else(||report.reasons.first().map(|r|r.label())) },"requirements":c.acceptance_requirements(),"warnings":c.reasons.iter().skip(1).map(|r|r.label()).collect::<Vec<_>>(),"evidence":format!("Artist compatible: {} · Title: {} · Known program lower bound: {} · Provider count: {}",c.artist_accepted,c.title.comparison.label(),c.required_tracks,c.provider_tracks.map(|n|n.to_string()).unwrap_or_else(||"unknown".into()))})).collect::<Vec<_>>());
            result["programs"] = serde_json::json!(
                report
                    .candidates
                    .iter()
                    .flat_map(|c| c
                        .programs
                        .iter()
                        .map(move |p| Self::spotify_program_comparison(&c.title.provider_raw, p)))
                    .collect::<Vec<_>>()
            );
        } else if let Some(album) = &self.spotify_album_id
            && let Some(programs) = self
                .spotify_album_matcher
                .as_ref()
                .and_then(|m| m.cached_programs(album))
            && let Ok(local) = self.session.library.local_album_tracks(album)
        {
            result["programs"] = serde_json::json!(
                programs
                    .programs
                    .iter()
                    .map(|p| Self::spotify_program_comparison(
                        &programs.album.external_id,
                        &music_library::album_program::inspect_positioned_program(&local, p)
                    ))
                    .collect::<Vec<_>>()
            );
        }
        result.to_string()
    }
    fn spotify_playback_value(&self) -> QVariantMap {
        let s = &self.spotify_playback_state;
        let choices: QVariantList = self
            .spotify_resolution_selection
            .as_ref()
            .map(|selection| {
                selection
                    .visible_indices()
                    .into_iter()
                    .map(|i| {
                        let c = &selection.candidates()[i];
                        let assessment = &selection.assessments()[i];
                        let group = match assessment.class {
                            music_library::song_resolution::FeasibilityClass::Preferred => {
                                "Best match"
                            }
                            music_library::song_resolution::FeasibilityClass::Alternate => {
                                "Other compatible Album representation"
                            }
                            music_library::song_resolution::FeasibilityClass::Infeasible => {
                                "Outside Album context (override)"
                            }
                        };
                        string(format!(
                            "{group}: {} — {} | {} ({}) | {}:{:02} | disc {} track {} | {}",
                            c.title,
                            c.artist,
                            c.album,
                            c.date,
                            c.duration_ms / 60000,
                            (c.duration_ms / 1000) % 60,
                            c.disc,
                            c.number,
                            c.identity.external_id
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let devices: QVariantList = s
            .devices
            .iter()
            .map(|d| -> QVariant {
                QVariantMap::from_iter([
                    ("id", string(d.id.as_deref().unwrap_or_default())),
                    (
                        "label",
                        string(format!(
                            "{} ({}){}{}",
                            d.name,
                            d.kind,
                            if d.is_active { " · active" } else { "" },
                            if d.is_restricted {
                                " · restricted"
                            } else {
                                ""
                            }
                        )),
                    ),
                    ("selectable", (d.id.is_some() && !d.is_restricted).into()),
                ])
                .into()
            })
            .collect();
        QVariantMap::from_iter([
            ("manuallyExcluded",self.spotify_playback_track.as_ref().is_some_and(|t|self.session.library.spotify_manually_excluded(t).unwrap_or(false)).into()),
            ("resolutionChoices", choices.into()),
            ("comparisonJson", string(self.spotify_comparison_json())),
            ("artistProposalJson", string(self.spotify_artist_proposal.as_ref().map(|p|serde_json::json!({"local_name":p.local_name,"local_id":p.local.as_ref(),"candidate_name":p.candidate_name,"candidate_identity":p.candidate_identity}).to_string()).unwrap_or_else(||"null".into()))),
            (
                "resolutionCanShowAll",
                self.spotify_resolution_selection
                    .as_ref()
                    .is_some_and(|s| {
                        !s.is_showing_all() && s.assessments().iter().any(|a| {
                            a.class == music_library::song_resolution::FeasibilityClass::Infeasible
                        })
                    })
                    .into(),
            ),
            (
                "resolutionGeneration",
                string(self.spotify_resolution_generation.to_string()),
            ),
            ("resolutionPending", self.spotify_resolution_pending.into()),
            (
                "resolutionMessage",
                string(&self.spotify_resolution_message),
            ),
            (
                "resolutionCounts",
                string(format!(
                    "Explicit catalog resolution: {} token / {} API requests",
                    self.spotify_resolution_counts.0, self.spotify_resolution_counts.1
                )),
            ),
            (
                "songUri",
                string(
                    self.spotify_playback_song
                        .as_ref()
                        .map(|s| s.uri())
                        .unwrap_or_default(),
                ),
            ),
            ("status", string(format!("{:?}", s.authorization))),
            (
                "error",
                string(
                    s.error
                        .as_ref()
                        .filter(|e| {
                            !matches!(e, music_library_spotify::playback::Error::NoAssociation)
                        })
                        .map(ToString::to_string)
                        .unwrap_or_else(|| {
                            if self.spotify_playback_track.is_some()
                                && self.spotify_playback_song.is_none()
                            {
                                music_library_spotify::playback::Error::NoAssociation.to_string()
                            } else {
                                String::new()
                            }
                        }),
                ),
            ),
            (
                "url",
                string(s.authorization_url.as_deref().unwrap_or_default()),
            ),
            ("devices", devices.into()),
            (
                "selected",
                string(s.selected_device.as_deref().unwrap_or_default()),
            ),
            (
                "trackId",
                string(
                    self.spotify_playback_track
                        .as_ref()
                        .map_or("", |id| id.as_ref()),
                ),
            ),
            ("albumExplanation", string(&self.spotify_album_explanation)),
            (
                "albumPending",
                self.spotify_album_id
                    .as_ref()
                    .is_some_and(|id| {
                        self.spotify_album_matcher.as_ref().is_some_and(|m| {
                            matches!(
                                m.outcome(id),
                                Some(music_library::album_matching::MatchOutcome::Pending)
                            ) || matches!(
                                m.program_outcome(id),
                                Some(music_library::album_program::Outcome::Pending)
                            )
                        })
                    })
                    .into(),
            ),
            ("title", string(&self.spotify_playback_title)),
            ("available", self.spotify_playback_song.is_some().into()),
            (
                "observed",
                string(format!(
                    "{} · {} · {} ms · {}",
                    s.state.title,
                    if s.state.playing {
                        "playing"
                    } else {
                        "paused/no playback"
                    },
                    s.state.progress_ms,
                    s.state
                        .device
                        .as_ref()
                        .map(|d| d.name.as_str())
                        .unwrap_or("no active device")
                )),
            ),
            (
                "requests",
                string(format!(
                    "Playback token: {}; playback API: {}",
                    s.token_requests, s.api_requests
                )),
            ),
        ])
    }
    fn ensure_matcher(&mut self) -> Result<(), String> {
        if self.matcher.is_none() {
            if self.catalog_providers.len() > 1 {
                let mut slots = vec![];
                for (index, name) in self.catalog_providers.iter().enumerate() {
                    let callback = matching_callback(qmetaobject::QPointer::from(&*self));
                    let programs = program_callback(qmetaobject::QPointer::from(&*self));
                    let retry =
                        matching_retry_callback_for(qmetaobject::QPointer::from(&*self), index);
                    let scope = if name == "spotify" {
                        music_library_spotify::matching_scope()
                    } else {
                        music_library::catalog::MatchingScope::musicbrainz()
                    };
                    let matcher = if name == "spotify" {
                        music_library_spotify::Spotify::from_env()
                            .and_then(|p| {
                                p.with_library(self.session.library.database_path().as_deref())
                            })
                            .map_err(|e| e.to_string())
                            .and_then(|p| {
                                music_library::album_matching::AlbumMatcher::for_provider(
                                    p,
                                    scope.clone(),
                                    callback,
                                    programs,
                                    retry,
                                )
                                .map_err(|e| e.to_string())
                            })
                    } else {
                        music_library::album_matching::AlbumMatcher::for_provider(
                            music_library_musicbrainz::MusicBrainz::new(),
                            scope.clone(),
                            callback,
                            programs,
                            retry,
                        )
                        .map_err(|e| e.to_string())
                    };
                    slots.push(music_library::provider_chain::ProviderSlot { scope, matcher });
                }
                self.matcher = Some(
                    music_library::provider_chain::ProviderChain::new(slots)
                        .map_err(|e| e.to_string())?,
                );
                return Ok(());
            }
            let weak = qmetaobject::QPointer::from(&*self);
            let callback = matching_callback(weak);
            let programs = program_callback(qmetaobject::QPointer::from(&*self));
            let retry = matching_retry_callback(qmetaobject::QPointer::from(&*self));
            self.matcher = Some(
                if self.spotify {
                    music_library::album_matching::AlbumMatcher::for_provider(
                        music_library_spotify::Spotify::from_env()
                            .and_then(|p| {
                                p.with_library(self.session.library.database_path().as_deref())
                            })
                            .map_err(|e| e.to_string())?,
                        music_library_spotify::matching_scope(),
                        callback,
                        programs,
                        retry,
                    )
                } else {
                    music_library::album_matching::AlbumMatcher::new_with_programs(
                        music_library_musicbrainz::MusicBrainz::new(),
                        callback,
                        programs,
                        retry,
                    )
                }
                .map_err(|e| e.to_string())?
                .into(),
            );
        }
        Ok(())
    }
    fn post_import(
        &mut self,
        imports: &[music_library::domain::ImportedRelease],
        enabled: bool,
    ) -> Result<(), String> {
        use music_library::album_matching::{AutoMatchPolicy, MatchOutcome};
        for import in imports {
            let album = self
                .session
                .library
                .album_for_release(&import.release_id)
                .map_err(|e| e.to_string())?;
            if !self.matching_rows.iter().any(|r| r.0 == album.album_id) {
                self.matching_rows.push((
                    album.album_id.clone(),
                    album.title,
                    MatchOutcome::Disabled,
                    album.artist_names,
                ));
                self.refresh_manual_album(&album.album_id)?;
            }
        }
        if enabled && !imports.is_empty() {
            if let Err(error) = self.ensure_matcher() {
                self.session.error = error;
                self.changed();
                self.matching_changed();
                return Ok(());
            }
            let outcomes = self
                .matcher
                .as_mut()
                .unwrap()
                .after_import(&self.session.library, imports, AutoMatchPolicy::default())
                .map_err(|e| e.to_string())?;
            for (id, outcome) in outcomes {
                if let Some(row) = self.matching_rows.iter_mut().find(|r| r.0 == id) {
                    row.2 = outcome;
                }
            }
        }
        self.matching_changed();
        Ok(())
    }
    fn provider_for(&self, album: &music_library::domain::AlbumId) -> String {
        if let Some(m) = &self.matcher
            && m.outcome(album).is_some()
        {
            return m.provider(album).to_owned();
        }
        if self.catalog_providers.len() > 1 {
            let identities = self
                .session
                .library
                .list_album_external_identities(album)
                .unwrap_or_default();
            if let Some(provider) = self.catalog_providers.iter().find(|p| {
                self.manual_associations
                    .get(album)
                    .is_some_and(|a| a.iter().any(|a| &a.album.provider == *p))
            }) {
                return provider.clone();
            }
            if let Some(provider) = self
                .catalog_providers
                .iter()
                .find(|p| identities.iter().any(|i| &i.provider == *p))
            {
                return provider.clone();
            }
            return self.catalog_providers[0].clone();
        }
        if self.spotify {
            "spotify".into()
        } else {
            "musicbrainz".into()
        }
    }
    fn refresh_manual_album(
        &mut self,
        album: &music_library::domain::AlbumId,
    ) -> Result<(), String> {
        self.manual_associations.insert(
            album.clone(),
            self.session
                .library
                .manual_track_associations(album)
                .map_err(|e| e.to_string())?,
        );
        self.matching_tracks.insert(
            album.clone(),
            self.session
                .library
                .local_album_tracks(album)
                .map_err(|e| e.to_string())?,
        );
        let provider = self.provider_for(album);
        self.manual_associations
            .get_mut(album)
            .unwrap()
            .retain(|a| a.album.provider == provider);
        let stored = self
            .session
            .library
            .provider_track_associations(album, &provider)
            .map_err(|e| e.to_string())?;
        let rows = self.matching_tracks[album]
            .iter()
            .filter_map(|t| {
                stored
                    .iter()
                    .find(|(id, _)| *id == t.track_id)
                    .map(|(_, m)| {
                        (
                            t.clone(),
                            music_library::album_program::TrackOutcome::AlreadyMatched(m.clone()),
                        )
                    })
            })
            .collect();
        self.stored_programs.insert(
            album.clone(),
            music_library::album_program::Outcome::Complete(rows),
        );
        Ok(())
    }
    fn load_manual_choices(&mut self) -> Result<bool, String> {
        if self.manual_selection.is_some() {
            return Ok(true);
        }
        let Some((album, track)) = &self.manual_context else {
            return Ok(false);
        };
        let Some(programs) = self.matcher.as_ref().and_then(|m| m.cached_programs(album)) else {
            return Ok(false);
        };
        self.manual_selection = Some(
            self.session
                .library
                .prepare_manual_track(album, track, programs)
                .map_err(|e| e.to_string())?,
        );
        self.manual_error.clear();
        Ok(true)
    }
    fn manual_snapshot_value(&self) -> QVariantMap {
        let local = self
            .manual_context
            .as_ref()
            .and_then(|(a, t)| {
                self.matching_tracks
                    .get(a)
                    .and_then(|rows| rows.iter().find(|r| &r.track_id == t))
            })
            .and_then(|t| t.evidence.title.as_deref())
            .unwrap_or("");
        let candidates = self
            .manual_selection
            .as_ref()
            .map_or(&[][..], |s| s.candidates());
        let labels: Vec<_> = candidates
            .iter()
            .map(|c| {
                let e = &c.evidence;
                let duration = e
                    .duration_ms
                    .map(|ms| format!("{}:{:02}", ms / 60_000, (ms / 1000) % 60))
                    .unwrap_or_else(|| "duration unknown".into());
                format!(
                    "{} — {} · {}/{}",
                    e.title.as_deref().unwrap_or("Untitled"),
                    duration,
                    e.disc.map(|n| n.to_string()).unwrap_or_else(|| "?".into()),
                    e.number
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "?".into())
                )
            })
            .collect();
        let rows: QVariantList = candidates
            .iter()
            .enumerate()
            .map(|(i, c)| -> QVariant {
                let mut label = labels[i].clone();
                if labels.iter().filter(|l| *l == &label).count() > 1 {
                    let id = c
                        .evidence
                        .recording
                        .identities
                        .first()
                        .or_else(|| c.evidence.identities.first());
                    label.push_str(&format!(
                        " · {}",
                        id.map(|id| format!("ID {}", id.external_id))
                            .unwrap_or_else(|| format!("candidate {}", i + 1))
                    ));
                }
                QVariantMap::from_iter([
                    ("label", string(label)),
                    (
                        "support",
                        string(format!("{} sampled programs", c.supporting_programs)),
                    ),
                ])
                .into()
            })
            .collect();
        QVariantMap::from_iter([
            ("localTitle", string(local)),
            ("candidates", rows.into()),
            ("error", string(&self.manual_error)),
            (
                "pending",
                (self.manual_context.is_some()
                    && self.manual_selection.is_none()
                    && self.manual_error.is_empty())
                .into(),
            ),
        ])
    }
    fn matching_provider_value(&self) -> QVariantMap {
        use music_library::album_matching::CircuitState;
        let mut values = QVariantMap::default();
        let paused = self
            .matcher
            .as_ref()
            .is_some_and(|m| matches!(m.circuit_state(), CircuitState::Unavailable(_)));
        let message = if self.catalog_providers.len() > 1 {
            self.matcher
                .as_ref()
                .map(|m| m.provider_messages().join("; "))
                .unwrap_or_default()
        } else {
            match self.matcher.as_ref().map(|m| m.circuit_state()) {
                Some(CircuitState::Unavailable(error)) => {
                    format!(
                        "{} unavailable — matching paused; retrying automatically: {error}",
                        if self.spotify {
                            "Spotify"
                        } else {
                            "MusicBrainz"
                        }
                    )
                }
                _ => String::new(),
            }
        };
        values.insert("paused".into(), paused.into());
        values.insert(
            "name".into(),
            string(if self.catalog_providers.len() > 1 {
                self.catalog_providers.join(" → ")
            } else {
                if self.spotify {
                    "Spotify"
                } else {
                    "MusicBrainz"
                }
                .to_owned()
            }),
        );
        values.insert("catalogAddSupported".into(), (!self.spotify).into());
        values.insert(
            "processing".into(),
            self.matcher
                .as_ref()
                .is_some_and(|m| m.is_processing())
                .into(),
        );
        values.insert("message".into(), QString::from(message).into());
        values.insert(
            "probe".into(),
            self.matcher
                .as_ref()
                .is_some_and(|m| m.probe_pending())
                .into(),
        );
        values.insert(
            "queued".into(),
            (self.matcher.as_ref().map_or(0, |m| m.pending_count()) as i32).into(),
        );
        values
    }
    fn matching_snapshot_value(&self) -> QVariantList {
        use music_library::album_matching::MatchOutcome;
        self.matching_rows
            .iter()
            .map(|(id, title, outcome, local_artist)| -> QVariant {
                let outcome = self
                    .matcher
                    .as_ref()
                    .and_then(|m| m.outcome(id))
                    .unwrap_or(outcome);
                let matched = self.matcher.as_ref().and_then(|m| m.matched_album(id));
                let program = self.matcher.as_ref().and_then(|m| m.program_outcome(id));
                let track_rows = program_rows(
                    self.matching_tracks.get(id).map_or(&[], Vec::as_slice),
                    match program {
                        Some(music_library::album_program::Outcome::Complete(_)) => program,
                        _ => self.stored_programs.get(id).filter(|o| matches!(o, music_library::album_program::Outcome::Complete(rows) if !rows.is_empty())).or(program),
                    },
                    self.manual_associations.get(id).map_or(&[], Vec::as_slice),
                );
                let (mut recording_summary, recording_details) = recording_presentation(
                    self.matcher.as_ref().and_then(|m| m.recording_outcome(id)),
                );
                if let Some(program) = program {
                    use music_library::album_program::Outcome;
                    recording_summary = match program {
                        Outcome::Pending => "Track enrichment queued/running…".into(),
                        Outcome::Complete(rows) => format!(
                            "Track enrichment finished: {} local Tracks assessed",
                            rows.len()
                        ),
                        Outcome::Deferred(_) => {
                            "Track enrichment deferred: provider unavailable".into()
                        }
                        Outcome::Error(error) => format!("Track enrichment error: {error}"),
                    };
                }
                let status = match outcome {
                    MatchOutcome::Pending => format!("Pending {} matching…", provider_label(self.matcher.as_ref().map(|m|m.provider(id)).unwrap_or(if self.spotify { "spotify" } else { "musicbrainz" }))),
                    MatchOutcome::Matched(identity) => format!("Matched: {}", identity.external_id),
                    MatchOutcome::MatchedClose(identity) => {
                        format!("Matched close title: {}", identity.external_id)
                    }
                    MatchOutcome::ArtistAmbiguous(candidates) => format!(
                        "Artist ambiguous / complex credit: {}",
                        candidates
                            .iter()
                            .map(|c| format!(
                                "{} [{}] {}",
                                c.name, c.identity.external_id, c.comment
                            ))
                            .collect::<Vec<_>>()
                            .join("; ")
                    ),
                    MatchOutcome::AlbumAmbiguous(candidates) => format!(
                        "Album ambiguous / incomplete results: {}",
                        candidates
                            .iter()
                            .map(|c| format!(
                                "{} [{}] {}",
                                c.title, c.identity.external_id, c.comment
                            ))
                            .collect::<Vec<_>>()
                            .join("; ")
                    ),
                    MatchOutcome::AlreadyMatched => "Already matched".into(),
                    MatchOutcome::AlbumEquivalent { .. } => "Album and Tracks supported; provider catalog identity unresolved".into(),
                    MatchOutcome::Disabled => "Automatic matching disabled; Retry available".into(),
                    MatchOutcome::Skipped => {
                        "Skipped: insufficient or changed local metadata".into()
                    }
                    MatchOutcome::NoConfidentMatch => "No confident match".into(),
                    MatchOutcome::Error(error) => format!("Error: {error}"),
                    MatchOutcome::ConfigurationError(error) => format!("Configuration error: {error}"),
                    MatchOutcome::Deferred(error) => {
                        format!("Deferred — provider unavailable: {error}")
                    }
                };
                let artists: QVariantList = match outcome {
                    MatchOutcome::ArtistAmbiguous(candidates) => candidates
                        .iter()
                        .map(|c| -> QVariant {
                            QVariantMap::from_iter([(
                                "label",
                                string(format!(
                                    "{} — {} {} {} [{}]",
                                    c.name,
                                    c.comment,
                                    c.country,
                                    c.artist_type,
                                    c.identity.external_id
                                )),
                            )])
                            .into()
                        })
                        .collect(),
                    _ => QVariantList::default(),
                };
                let (equivalent_title, equivalent_artist) = if let MatchOutcome::AlbumEquivalent { candidates, .. } = &outcome {
                    let titles: std::collections::BTreeSet<_> = candidates.iter().map(|c| c.title.as_str()).collect();
                    let artists: std::collections::BTreeSet<_> = candidates.iter().map(|c| c.artist.as_str()).collect();
                    (titles.into_iter().collect::<Vec<_>>().join(" / "), artists.into_iter().collect::<Vec<_>>().join(" / "))
                } else { (String::new(), String::new()) };
                QVariantMap::from_iter([
                    ("provider",string(provider_label(self.matcher.as_ref().map(|m|m.provider(id)).unwrap_or(if self.spotify {"spotify"}else{"musicbrainz"})))),
                    ("providerHistory",string(self.matcher.as_ref().map(|m|m.attempts(id).iter().map(|a|format!("{}: {}",a.provider,attempt_label(&a.outcome))).collect::<Vec<_>>().join("; ")).unwrap_or_default())),
                    ("equivalent", matches!(outcome,MatchOutcome::AlbumEquivalent{..}).into()),
                    ("albumId", string(id.as_ref())),
                    ("tracks", track_rows.into()),
                    ("artists", artists.into()),
                    ("title", string(title)),
                    ("recordingSummary", string(&recording_summary)),
                    ("recordingDetails", string(&recording_details)),
                    ("localArtist", string(local_artist)),
                    (
                        "matchedTitle",
                        string(matched.map_or(equivalent_title.as_str(), |c| c.title.as_str())),
                    ),
                    (
                        "matchedArtist",
                        string(matched.map_or(equivalent_artist.as_str(), |c| c.artist.as_str())),
                    ),
                    (
                        "matchedClose",
                        matches!(outcome, MatchOutcome::MatchedClose(_)).into(),
                    ),
                    ("status", string(&status)),
                    ("pending", matches!(outcome, MatchOutcome::Pending).into()),
                ])
                .into()
            })
            .collect()
    }

    fn start_catalog(&mut self, action: &str, value: &str) -> Result<(), String> {
        if self.spotify {
            return Err(
                "Spotify supports local Album/Track matching; catalog Add Album is not implemented"
                    .into(),
            );
        }
        self.catalog.timing = Some(music_library::catalog::Timing::new(format!(
            "qml.{action}.button_to_completion"
        )));
        let request = self.catalog.request(action, value)?;
        if self.catalog.worker.is_none() {
            // Invoked through a live QML QObject, so the C++ target already exists.
            let weak = qmetaobject::QPointer::from(&*self);
            let deliver = catalog_callback(weak);
            self.catalog.worker = Some(catalog::Worker::new(
                music_library_musicbrainz::MusicBrainz::new(),
                deliver,
            )?);
        }
        self.catalog.worker.as_ref().unwrap().send(request)?;
        self.catalog.pending = true;
        self.catalog.status = "Waiting for MusicBrainz…".into();
        Ok(())
    }

    fn catalog_reply(
        &mut self,
        reply: Result<catalog::Reply, music_library::catalog::CatalogError>,
    ) {
        self.catalog.pending = false;
        self.catalog.status = match reply {
            Ok(catalog::Reply::Groups(page)) => {
                self.catalog.groups = page.items;
                self.catalog.group_next = page.next_offset;
                "Select Add Album, or inspect Editions.".into()
            }
            Ok(catalog::Reply::Editions(page)) => {
                self.catalog.editions = page.items;
                self.catalog.edition_next = page.next_offset;
                "Select an edition. Add this edition imports its complete tracklist.".into()
            }
            Ok(catalog::Reply::Add(release)) => {
                match self.session.library.add_catalog_release(&release) {
                    Ok(imported) => {
                        self.enrich_catalog_spotify(&imported);
                        let refresh =
                            music_library::catalog::Timing::new("post_import.search_refresh");
                        self.session.search(release.album.title);
                        self.browse_action_impl("refresh", 0, String::new());
                        drop(refresh);
                        format!(
                            "Added Album: {} Tracks (no playback started).",
                            imported.track_ids.len()
                        )
                    }
                    Err(error) => error.to_string(),
                }
            }
            Err(error) => error.to_string(),
        };
    }

    fn catalog_snapshot_value(&self) -> QVariantMap {
        [
            ("catalogPending", self.catalog.pending.into()),
            ("catalogStatus", string(&self.catalog.status)),
            (
                "catalogMoreGroups",
                self.catalog.group_next.is_some().into(),
            ),
            (
                "catalogMoreEditions",
                self.catalog.edition_next.is_some().into(),
            ),
            (
                "catalogGroups",
                self.catalog
                    .groups
                    .iter()
                    .map(|g| {
                        let label = format!(
                            "{} · {} · {} · {} {} · {}",
                            g.title,
                            g.artist,
                            g.date,
                            g.primary_type,
                            g.secondary_types.join(", "),
                            g.comment
                        );
                        QVariant::from(
                            [("label", string(label))]
                                .into_iter()
                                .collect::<QVariantMap>(),
                        )
                    })
                    .collect::<QVariantList>()
                    .into(),
            ),
            (
                "catalogEditions",
                self.catalog
                    .editions
                    .iter()
                    .map(|r| {
                        let label = format!(
                            "{} · {} · {} {} · {} · {} · {} · {} discs/{} tracks · {} · barcode {}",
                            r.title,
                            r.artist,
                            r.date,
                            r.country,
                            r.status,
                            r.comment,
                            r.formats.join(", "),
                            r.disc_count,
                            r.track_count,
                            r.labels.join(", "),
                            r.barcode
                        );
                        QVariant::from(
                            [("label", string(label))]
                                .into_iter()
                                .collect::<QVariantMap>(),
                        )
                    })
                    .collect::<QVariantList>()
                    .into(),
            ),
        ]
        .into_iter()
        .collect()
    }

    fn snapshot_value(&self) -> QVariantMap {
        let s = &self.session;
        let state = s.playback.state();
        let remote_active = matches!(
            self.active_backend,
            music_library::playback_resolver::ActiveBackend::Remote(_)
        );
        let (progress, duration, seek_available) = self.player_clock();
        let rows: QVariantList = s.rows.iter().map(row_value).collect();
        let queue: QVariantList = state
            .queue
            .iter()
            .enumerate()
            .skip(
                self.queue_offset
                    .min(state.queue.len().saturating_sub(1) / 200 * 200),
            )
            .take(200)
            .map(|(i, id)| {
                // IDs/position come from PlaybackState, labels are presentation-only snapshots.
                let label = s.queue_labels.get(i).filter(|row| &row.track_id == id);
                let row: QVariantMap = [
                    ("title", string(label.map_or(id.as_ref(), |row| &row.title))),
                    ("trackId", string(id.as_ref())),
                    ("current", (state.position == Some(i)).into()),
                    ("artist", string(label.map_or("", |row| &row.artist_names))),
                    ("album", string(label.map_or("", |row| &row.release_title))),
                ]
                .into_iter()
                .collect();
                QVariant::from(row)
            })
            .collect();
        let current = state.position.and_then(|i| s.queue_labels.get(i));
        [
            (
                "currentArtist",
                string(current.map_or("", |row| &row.artist_names)),
            ),
            (
                "currentAlbum",
                string(current.map_or("", |row| &row.release_title)),
            ),
            (
                "playing",
                (if remote_active {
                    self.spotify_playback_state.state.playing
                } else {
                    state.status == music_library::playback::PlaybackStatus::Playing
                })
                .into(),
            ),
            ("rows", rows.into()),
            ("queue", queue.into()),
            ("queueTotal", (state.queue.len() as i32).into()),
            (
                "queueOffset",
                (self
                    .queue_offset
                    .min(state.queue.len().saturating_sub(1) / 200 * 200) as i32)
                    .into(),
            ),
            (
                "currentTitle",
                string(current.map_or("—", |row| &row.title)),
            ),
            (
                "currentId",
                string(state.current_track().map_or("", |id| id.as_ref())),
            ),
            ("position", (state.position.map_or(-1, |i| i as i32)).into()),
            (
                "status",
                string(if remote_active {
                    if self.spotify_playback_state.state.playing {
                        "Playing (Spotify)".into()
                    } else {
                        "Paused / awaiting Spotify state".into()
                    }
                } else {
                    format!("{:?}", state.status)
                }),
            ),
            (
                "pending",
                string(
                    state
                        .pending
                        .map_or(String::new(), |s| format!(" → {s:?} pending")),
                ),
            ),
            ("localTrim", self.controls.trims.local_db.into()),
            ("spotifyTrim", self.controls.trims.spotify_db.into()),
            ("volume", self.player_volume_value().into()),
            ("volumeAvailable", self.player_volume_available().into()),
            ("progressMs", (progress as f64).into()),
            ("durationMs", duration.map_or(-1., |d| d as f64).into()),
            ("elapsed", string(time_label(Some(progress)))),
            ("duration", string(time_label(duration))),
            ("seekAvailable", seek_available.into()),
            (
                "clockRunning",
                (remote_active && self.spotify_playback_state.state.playing).into(),
            ),
            ("realAudio", self.real_audio.into()),
            ("route", string(&self.route_message)),
            (
                "activeBackend",
                string(format!("{:?}", self.active_backend)),
            ),
            (
                "time",
                string(format!(
                    "{} / {}",
                    time_label(Some(progress)),
                    time_label(duration)
                )),
            ),
            ("canNext", state.can_next().into()),
            ("canPrevious", state.can_previous().into()),
            (
                "source",
                string(state.source.as_ref().map_or("—", |s| s.source_id.as_ref())),
            ),
            ("error", string(&s.error)),
            ("outcome", string(s.outcome())),
            ("revision", s.revision.into()),
            ("searchText", string(&s.text)),
            ("page", (s.page as u32 + 1).into()),
            ("hasNext", s.has_next.into()),
            ("failureArmed", s.diagnostics.borrow().fail_next.into()),
            (
                "engineCalls",
                string(
                    s.diagnostics
                        .borrow()
                        .calls
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
            ),
        ]
        .into_iter()
        .collect()
    }
}

fn time_label(ms: Option<u64>) -> String {
    ms.map_or_else(
        || "--:--".into(),
        |ms| format!("{:02}:{:02}", ms / 60_000, (ms / 1000) % 60),
    )
}
fn provider_label(name: &str) -> &str {
    match name {
        "musicbrainz" => "MusicBrainz",
        "spotify" => "Spotify",
        other => other,
    }
}
fn attempt_label(outcome: &music_library::album_matching::MatchOutcome) -> String {
    use music_library::album_matching::MatchOutcome;
    match outcome {
        MatchOutcome::NoConfidentMatch => "no confident match".into(),
        MatchOutcome::ArtistAmbiguous(_) => "Artist ambiguous".into(),
        MatchOutcome::AlbumAmbiguous(_) => "Album ambiguous".into(),
        MatchOutcome::Deferred(e) => format!("unavailable: {e}"),
        MatchOutcome::ConfigurationError(e) => format!("configuration: {e}"),
        MatchOutcome::Error(e) => e.clone(),
        _ => "Album supported".into(),
    }
}

fn matching_retry_callback(weak: qmetaobject::QPointer<Bridge>) -> impl Fn(u64) + Send + 'static {
    matching_retry_callback_for(weak, 0)
}
fn matching_retry_callback_for(
    weak: qmetaobject::QPointer<Bridge>,
    provider: usize,
) -> impl Fn(u64) + Send + 'static {
    qmetaobject::queued_callback(move |token: u64| {
        if let Some(pinned) = weak.as_pinned() {
            let mut bridge = pinned.borrow_mut();
            if let Some(mut matcher) = bridge.matcher.take() {
                if let Err(error) =
                    matcher.cooldown_provider(&bridge.session.library, provider, token)
                {
                    bridge.session.error = error.to_string();
                    bridge.changed();
                }
                bridge.matcher = Some(matcher);
                bridge.matching_changed();
            }
        }
    })
}

fn program_rows(
    local: &[music_library::edition::LocalTrackEvidence],
    outcome: Option<&music_library::album_program::Outcome>,
    manual: &[music_library::manual_track::Association],
) -> QVariantList {
    use music_library::album_program::{Outcome, TrackOutcome};
    local
        .iter()
        .map(|t| -> QVariant {
            let result = match outcome {
                Some(Outcome::Complete(rows)) => rows
                    .iter()
                    .find(|(other, _)| other.track_id == t.track_id)
                    .map(|(_, o)| o),
                _ => None,
            };
            let manual_outcome = manual
                .iter()
                .find(|m| m.track_id == t.track_id)
                .map(|m| TrackOutcome::ManuallyMatched(m.matched()));
            let result = manual_outcome.as_ref().or(result);
            let mut matched = String::new();
            let mut recording_status = String::new();
            let status = match result {
                Some(
                    TrackOutcome::Matched(m)
                    | TrackOutcome::AlreadyMatched(m)
                    | TrackOutcome::ManuallyMatched(m),
                ) => {
                    matched = m.title.clone();
                    recording_status = format!("{:?}", m.recording_status);
                    if matches!(result, Some(TrackOutcome::ManuallyMatched(_))) {
                        "Manual"
                    } else if matches!(result, Some(TrackOutcome::AlreadyMatched(_))) {
                        "AlreadyMatched"
                    } else {
                        "Matched"
                    }
                }
                Some(TrackOutcome::Ambiguous) => "Ambiguous",
                Some(TrackOutcome::ConflictingIdentity) => "ConflictingIdentity",
                Some(TrackOutcome::NoConfidentMatch) => "NoConfidentMatch",
                None => match outcome {
                    Some(Outcome::Pending) => "Pending",
                    Some(Outcome::Deferred(_)) => "ProviderUnavailable",
                    Some(Outcome::Error(_)) => "Error",
                    _ if !t.evidence.recording.identities.is_empty() => {
                        "AlreadyMatched (stored identity; provider title not cached)"
                    }
                    _ => "Not matched",
                },
            };
            QVariantMap::from_iter([
                ("trackId", string(t.track_id.as_ref())),
                ("songIdentityAmbiguous", matches!(result,Some(TrackOutcome::Matched(m)|TrackOutcome::AlreadyMatched(m)) if m.recording_status==music_library::album_program::RecordingStatus::NotProvided && m.occurrences.is_empty()).into()),
                ("manual", (status == "Manual").into()),
                (
                    "canChoose",
                    (status != "Manual"
                        && status != "Pending"
                        && recording_status != "Identified"
                        && !(recording_status == "NotProvided"
                            && matches!(status, "Matched" | "AlreadyMatched")))
                    .into(),
                ),
                (
                    "storedIdentity",
                    string(
                        t.evidence
                            .recording
                            .identities
                            .iter()
                            .map(|i| format!("{}:{}:{}", i.provider, i.kind, i.external_id))
                            .collect::<Vec<_>>()
                            .join(", "),
                    ),
                ),
                (
                    "localTitle",
                    string(t.evidence.title.as_deref().unwrap_or("")),
                ),
                ("matchedTitle", string(matched)),
                ("recordingStatus", string(recording_status)),
                ("status", string(status)),
            ])
            .into()
        })
        .collect()
}
fn program_callback(
    weak: qmetaobject::QPointer<Bridge>,
) -> impl Fn(music_library::album_program::Reply) + Send + 'static {
    qmetaobject::queued_callback(move |reply: music_library::album_program::Reply| {
        if let Some(pinned) = weak.as_pinned() {
            let mut bridge = pinned.borrow_mut();
            if let Some(mut matcher) = bridge.matcher.take() {
                let id = reply.input.album_id.clone();
                bridge
                    .matching_tracks
                    .insert(id.clone(), reply.input.tracks.clone());
                matcher.complete_programs(&mut bridge.session.library, reply);
                bridge.browse_action_impl("refresh", 0, String::new());
                bridge.matcher = Some(matcher);
                if let Err(e) = bridge.refresh_manual_album(&id) {
                    bridge.session.error = e;
                    bridge.changed();
                }
                if bridge.manual_selection.is_none()
                    && bridge
                        .manual_context
                        .as_ref()
                        .is_some_and(|(album, _)| album == &id)
                {
                    if let Err(e) = bridge.load_manual_choices() {
                        bridge.manual_error = e;
                    }
                    if bridge.manual_selection.is_none() {
                        bridge.manual_error =
                            match bridge.matcher.as_ref().and_then(|m| m.program_outcome(&id)) {
                                Some(music_library::album_program::Outcome::Deferred(e)) => {
                                    format!("Provider unavailable; queued for retry: {e}")
                                }
                                Some(music_library::album_program::Outcome::Error(e)) => e.clone(),
                                _ => String::new(),
                            };
                    }
                    bridge.manual_changed();
                }
                bridge.matching_changed();
            }
        }
    })
}
fn recording_presentation(outcome: Option<&music_library::recording::Outcome>) -> (String, String) {
    use music_library::recording::{Outcome, TrackOutcome};
    match outcome {
        None => (String::new(), String::new()),
        Some(Outcome::Pending) => ("Recordings: pending…".into(), String::new()),
        Some(Outcome::Deferred(e)) => (format!("Recordings deferred: {e}"), String::new()),
        Some(Outcome::Error(e)) => (format!("Recording error: {e}"), String::new()),
        Some(Outcome::Complete(rows)) => {
            let matched = rows
                .iter()
                .filter(|(_, r)| {
                    matches!(r, TrackOutcome::Matched(_) | TrackOutcome::AlreadyMatched)
                })
                .count();
            let details = rows
                .iter()
                .map(|(t, r)| {
                    let status = match r {
                        TrackOutcome::Matched(c) => format!(
                            "Matched Recording: {} — {}\nMusicBrainz Recording: {}\nISRC: {}",
                            c.title,
                            c.artist,
                            c.identity.external_id,
                            c.isrcs.join(", ")
                        ),
                        other => format!("{other:?}"),
                    };
                    format!("Local: {} — {}\n{}", t.title, t.artist, status)
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            (
                format!("Recordings matched: {matched} / {}", rows.len()),
                details,
            )
        }
    }
}
#[cfg(all(test, feature = "gstreamer"))]
fn recording_callback(
    weak: qmetaobject::QPointer<Bridge>,
) -> impl Fn(music_library::recording::Reply) + Send + 'static {
    qmetaobject::queued_callback(move |reply: music_library::recording::Reply| {
        if let Some(pinned) = weak.as_pinned() {
            let mut bridge = pinned.borrow_mut();
            if let Some(mut matcher) = bridge.matcher.take() {
                matcher.complete_recordings(&mut bridge.session.library, reply);
                bridge.matcher = Some(matcher);
                bridge.matching_changed();
            }
        }
    })
}

fn matching_callback(
    weak: qmetaobject::QPointer<Bridge>,
) -> impl Fn(music_library::album_matching::MatchReply) + Send + Sync + 'static {
    qmetaobject::queued_callback(move |reply: music_library::album_matching::MatchReply| {
        if let Some(pinned) = weak.as_pinned() {
            let mut bridge = pinned.borrow_mut();
            let id = reply.input.album_id.clone();
            let Some(mut matcher) = bridge.matcher.take() else {
                return;
            };
            let outcome = matcher.complete(&mut bridge.session.library, reply);
            bridge.matcher = Some(matcher);
            if let Some(row) = bridge.matching_rows.iter_mut().find(|r| r.0 == id) {
                row.2 = outcome;
            }
            bridge.browse_action_impl("refresh", 0, String::new());
            bridge.matching_changed();
        }
    })
}

fn catalog_callback(
    weak: qmetaobject::QPointer<Bridge>,
) -> impl Fn(Result<catalog::Reply, music_library::catalog::CatalogError>) + Send + Sync + 'static {
    qmetaobject::queued_callback(move |reply| {
        if let Some(bridge) = weak.as_pinned() {
            let mut bridge = bridge.borrow_mut();
            bridge.catalog_reply(reply);
            bridge.catalog_changed();
            bridge.changed();
            bridge.catalog.timing.take();
        }
    })
}

// Construct the C++ QObject before capturing a QPointer: otherwise it stays null
// and silently discards every engine notification, even while GstPlay is Playing.
// https://docs.rs/qmetaobject/0.2.10/qmetaobject/struct.QPointer.html
#[cfg(feature = "gstreamer")]
fn engine_callback(
    bridge: qmetaobject::QObjectPinned<'_, Bridge>,
) -> impl Fn(music_library::playback::EngineEvent) + Send + Sync + 'static {
    bridge.get_or_create_cpp_object();
    let weak = qmetaobject::QPointer::from(bridge.borrow());
    qmetaobject::queued_callback(move |event: music_library::playback::EngineEvent| {
        if let Some(bridge) = weak.as_pinned() {
            let mut bridge = bridge.borrow_mut();
            if let music_library::playback::EngineEventKind::Error(error) = &event.kind {
                eprintln!(
                    "local-playback error generation={} track={:?}: {}",
                    event.generation,
                    bridge.session.playback.state().current_track(),
                    error
                );
            }
            if matches!(
                event.kind,
                music_library::playback::EngineEventKind::EndOfStream
            ) {
                if bridge.active_backend == music_library::playback_resolver::ActiveBackend::Local
                    && bridge.session.playback.consume_end_of_stream(&event)
                {
                    bridge.navigate_queue(false);
                    bridge.changed();
                }
            } else if bridge.session.engine_event(event) {
                if bridge.session.playback.state().source.is_some()
                    && bridge.session.playback.state().status
                        == music_library::playback::PlaybackStatus::Playing
                    && bridge.session.playback.state().pending.is_none()
                    && bridge.active_backend
                        == music_library::playback_resolver::ActiveBackend::None
                {
                    bridge.active_backend = music_library::playback_resolver::ActiveBackend::Local;
                    bridge.route_message = "Playing via Local".into();
                }
                bridge.continue_local_stop();
                bridge.changed();
            }
        }
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    let providers = if let Some(index) = args.iter().position(|a| a == "--catalog-providers") {
        if args.iter().any(|a| a == "--catalog-provider") {
            return Err("Choose --catalog-provider or --catalog-providers, not both".into());
        }
        let value = args
            .get(index + 1)
            .and_then(|s| s.to_str())
            .ok_or("--catalog-providers requires an ordered comma-separated list")?;
        let providers: Vec<String> = value.split(',').map(str::to_owned).collect();
        let mut seen = std::collections::HashSet::new();
        if providers
            .iter()
            .any(|p| !matches!(p.as_str(), "spotify" | "musicbrainz") || !seen.insert(p.clone()))
        {
            return Err(
                "Configure distinct spotify/musicbrainz providers in the desired order".into(),
            );
        }
        args.drain(index..=index + 1);
        Some(providers)
    } else {
        None
    };
    let spotify = if let Some(index) = args.iter().position(|a| a == "--catalog-provider") {
        let value = args
            .get(index + 1)
            .ok_or("--catalog-provider requires musicbrainz or spotify")?;
        let spotify = match value.to_str() {
            Some("spotify") => true,
            Some("musicbrainz") => false,
            _ => return Err("--catalog-provider requires musicbrainz or spotify".into()),
        };
        args.drain(index..=index + 1);
        spotify
    } else {
        false
    };
    let providers = providers.unwrap_or_else(|| {
        vec![if spotify {
            "spotify".into()
        } else {
            "musicbrainz".into()
        }]
    });
    let spotify = providers[0] == "spotify";
    let auto_match = !args.iter().any(|a| a == "--no-auto-match");
    args.retain(|a| a != "--no-auto-match");
    let database = if let Some(index) = args.iter().position(|a| a == "--library") {
        let path = args
            .get(index + 1)
            .ok_or("--library requires a database path")?
            .clone();
        args.drain(index..=index + 1);
        Some(std::path::PathBuf::from(path))
    } else {
        None
    };
    let (smoke, folder) = match args.as_slice() {
        [] => (false, None),
        [arg] if arg == "--smoke-test" => (true, None),
        [arg, folder] if arg == "--gstreamer" => (false, Some(std::path::PathBuf::from(folder))),
        [arg, folder, check] if arg == "--gstreamer" && check == "--smoke-test" => {
            (true, Some(std::path::PathBuf::from(folder)))
        }
        _ => {
            return Err(
                "usage: qml-diagnostic [--catalog-provider musicbrainz|spotify | --catalog-providers spotify,musicbrainz] [--no-auto-match] [--library DATABASE | --gstreamer FOLDER] [--smoke-test]".into(),
            );
        }
    };
    if database.is_some() && folder.is_some() {
        return Err("Choose --library DATABASE or --gstreamer FOLDER".into());
    }
    let persistent = database.is_some();
    #[cfg(not(feature = "gstreamer"))]
    if folder.is_some() {
        return Err("rebuild with --features gstreamer to use real audio".into());
    }
    #[cfg(feature = "gstreamer")]
    let (_temp, library, imports) = if let Some(path) = &database {
        (
            tempfile::tempdir()?,
            music_library::Library::open(path)?,
            vec![],
        )
    } else if let Some(folder) = &folder {
        local::load(folder)?
    } else {
        let (temp, library) = sample::create()?;
        (temp, library, vec![])
    };
    #[cfg(not(feature = "gstreamer"))]
    let (_temp, library, imports) = if let Some(path) = &database {
        (
            tempfile::tempdir()?,
            music_library::Library::open(path)?,
            vec![],
        )
    } else {
        let (temp, library) = sample::create()?;
        (temp, library, vec![])
    };
    // Pin before exposing to QML, and keep the QObject alive until QML destruction.
    let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
    bridge.pinned().borrow_mut().spotify = spotify;
    bridge.pinned().borrow_mut().auto_match = auto_match;
    bridge.pinned().borrow_mut().catalog_providers = providers;
    let mut engine = QmlEngine::new();
    #[cfg(feature = "gstreamer")]
    if folder.is_some() || persistent {
        let deliver = engine_callback(bridge.pinned());
        let audio = music_library_gstreamer::GStreamerEngine::new(deliver)?;
        let pinned = bridge.pinned();
        let mut bridge = pinned.borrow_mut();
        bridge.real_audio = true;
        bridge.session.playback =
            music_library::playback::Playback::new(session::Engine::GStreamer(audio));
    }
    engine.set_object_property("diagnostic".into(), bridge.pinned());
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        engine.load_data(include_str!("../Main.qml").into());
        if !engine.invoke_method("ready".into(), &[]).to_bool() {
            return Err(
                "QML did not create the diagnostic window; check Qt module errors above".into(),
            );
        }
        if smoke && (folder.is_some() || persistent) {
            // Real-mode startup/teardown check, deliberately without audible playback.
            println!("QML real-engine startup smoke test passed (no playback)");
        } else if smoke {
            let result = engine
                .invoke_method("smokeTest".into(), &[])
                .to_qstring()
                .to_string();
            if result != "ok" {
                return Err(format!("QML smoke test failed: {result}").into());
            }
            println!("QML integration smoke test passed");
        } else {
            bridge
                .pinned()
                .borrow_mut()
                .post_import(&imports, auto_match)?;
            if (persistent || folder.is_some())
                && !bridge
                    .pinned()
                    .borrow()
                    .session
                    .library
                    .local_locations()?
                    .is_empty()
            {
                bridge.pinned().borrow_mut().start_local_import(
                    music_library::local_ingestion::Request::ConfiguredLocations,
                    0,
                );
            }
            engine.exec();
        }
        Ok(())
    })();
    bridge.pinned().borrow_mut().matcher.take();
    bridge.pinned().borrow_mut().spotify_album_matcher.take();
    bridge.pinned().borrow_mut().catalog.worker.take();
    bridge.pinned().borrow_mut().spotify_playback_worker.take();
    bridge
        .pinned()
        .borrow_mut()
        .spotify_resolution_worker
        .take();
    // Join the audio worker while Qt and its callback target still exist, even on QML load failure.
    #[cfg(feature = "gstreamer")]
    bridge.pinned().borrow_mut().session.shutdown_audio();
    result
}

#[cfg(all(test, feature = "gstreamer"))]
mod event_delivery_tests {
    use super::*;
    use music_library::playback::{EngineError, EngineEvent, EngineEventKind as Event};
    use std::{cell::RefCell, rc::Rc};

    #[test]
    #[ignore = "live audio on explicitly configured device and disposable database"]
    fn live_mixed_queue_controls() {
        use music_library::domain::TrackId;
        let database = std::env::var("MUSIC_LIBRARY_DIAGNOSTIC_DATABASE").unwrap();
        let local = TrackId(std::env::var("MUSIC_LIBRARY_MIXED_LOCAL").unwrap());
        let remote = TrackId(std::env::var("MUSIC_LIBRARY_MIXED_REMOTE").unwrap());
        let after = TrackId(
            std::env::var("MUSIC_LIBRARY_MIXED_LOCAL_AFTER").unwrap_or_else(|_| local.0.clone()),
        );
        let natural_local = std::env::var_os("MUSIC_LIBRARY_MIXED_LOCAL_EOS").is_some();
        let device = std::env::var("MUSIC_LIBRARY_MIXED_DEVICE").unwrap();
        let bridge = QObjectBox::new(Bridge::new(Session::new(
            music_library::Library::open(database).unwrap(),
        )));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        engine.set_property("testDevice".into(), string(device));
        engine.set_property("naturalLocal".into(), natural_local.into());
        let audio = music_library_gstreamer::GStreamerEngine::new(engine_callback(bridge.pinned()))
            .unwrap();
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.real_audio = true;
            b.session.playback =
                music_library::playback::Playback::new(session::Engine::GStreamer(audio));
            b.session
                .playback
                .set_queue(vec![local.clone(), remote.clone(), after])
                .unwrap();
        }
        engine.load_data(r#"import QtQuick
            Item {
                property int step: 0
                property string failures: ""
                function check(expected) {
                    const actual = diagnostic.snapshot.activeBackend;
                    console.log("mixed queue phase", step, actual, diagnostic.snapshot.time);
                    if (actual !== expected) failures += step + ": " + actual + "; ";
                }
                function report() { return failures; }
                Timer { interval: 4000; running: true; repeat: true
                    onTriggered: {
                        switch (parent.step++) {
                        case 0: diagnostic.spotify_playback_action("refresh", ""); break;
                        case 1: diagnostic.spotify_playback_action("device", testDevice); break;
                        case 2: diagnostic.command("play"); break;
                        case 3:
                            if (naturalLocal) parent.check('Remote("spotify")');
                            else { parent.check("Local"); diagnostic.command("next"); }
                            break;
                        case 4: parent.check('Remote("spotify")'); diagnostic.command("next"); break;
                        case 5: parent.check("Local"); diagnostic.command("previous"); break;
                        case 6: parent.check('Remote("spotify")'); diagnostic.clear_queue(); break;
                        case 7: parent.check("None"); stop(); Qt.quit(); break;
                        }
                    }
                }
            }
        "#.into());
        engine.exec();
        let report = engine
            .invoke_method("report".into(), &[])
            .to_qstring()
            .to_string();
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            assert!(
                b.spotify_resolution_worker.is_none(),
                "Known associations must not create catalog worker"
            );
            println!(
                "Live Next/Previous/clear passed; catalog token/API=0/0; playback token/API={}/{}",
                b.spotify_playback_state.token_requests, b.spotify_playback_state.api_requests
            );
            b.spotify_playback_worker.take();
            b.session.shutdown_audio();
        }
        assert!(report.is_empty(), "{report}");
    }

    #[test]
    #[ignore = "live Spotify chooser; requires disposable /tmp MUSIC_LIBRARY_DIAGNOSTIC_DATABASE"]
    fn live_spotify_album_context_chooser() {
        use music_library::song_resolution::FeasibilityClass as Class;
        let source = std::path::PathBuf::from(
            std::env::var_os("MUSIC_LIBRARY_DIAGNOSTIC_DATABASE").expect("disposable database"),
        );
        assert!(source.starts_with("/tmp"));
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("chooser.sqlite");
        std::fs::copy(source, &path).unwrap();
        let library = music_library::Library::open(path).unwrap();
        let rows = library
            .search(&music_library::domain::SearchRequest {
                limit: 100,
                ..Default::default()
            })
            .unwrap();
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml").replace(
            "    function ready() {",
            r#"
    Timer { id: auditTimeout; interval: 30000; onTriggered: Qt.quit() }
    Connections {
        target: window.bridge
        function onSpotify_playback_changed() {
            if (!window.spotifyPlayback.resolutionPending && auditTimeout.running) {
                auditTimeout.stop(); Qt.quit();
            }
        }
    }
    function auditChooser(track) {
        window.bridge.spotify_playback_action("track", track);
        spotifyPlaybackDialog.open();
        window.bridge.spotify_resolve("search", -1);
        auditTimeout.start();
    }
    function auditCount() { return spotifySongChoice.count; }
    function ready() {
"#,
        );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        for title in ["Feel Good Inc.", "Dirty Harry", "DARE", "El mañana"] {
            let track = &rows
                .iter()
                .find(|r| r.title.eq_ignore_ascii_case(title))
                .unwrap()
                .track_id;
            engine.invoke_method("auditChooser".into(), &[string(track.as_ref())]);
            engine.exec();
            assert_eq!(
                engine
                    .invoke_method("auditCount".into(), &[])
                    .to_qstring()
                    .to_string(),
                "2"
            );
            let total = {
                let pinned = bridge.pinned();
                let mut b = pinned.borrow_mut();
                assert!(!b.spotify_resolution_pending, "provider timeout");
                let selection = b
                    .spotify_resolution_selection
                    .as_ref()
                    .expect("search results");
                let classes = selection.assessments();
                let total = selection.candidates().len();
                println!(
                    "LIVE QML {title}: total={total} preferred={} alternate={} hidden={}",
                    classes
                        .iter()
                        .filter(|a| a.class == Class::Preferred)
                        .count(),
                    classes
                        .iter()
                        .filter(|a| a.class == Class::Alternate)
                        .count(),
                    classes
                        .iter()
                        .filter(|a| a.class == Class::Infeasible)
                        .count()
                );
                assert_eq!(selection.visible_indices().len(), 2);
                let counts = b.spotify_resolution_counts;
                b.spotify_resolve("show-all".into(), -1);
                assert_eq!(b.spotify_resolution_counts, counts);
                assert!(
                    b.session
                        .library
                        .track_provider_occurrences(track, "spotify")
                        .unwrap()
                        .is_empty()
                );
                println!(
                    "LIVE QML Show-all {total}; token/API={counts:?}; no persistence, no extra HTTP"
                );
                total
            };
            assert_eq!(
                engine
                    .invoke_method("auditCount".into(), &[])
                    .to_qstring()
                    .to_string(),
                total.to_string()
            );
        }
        bridge
            .pinned()
            .borrow_mut()
            .spotify_resolution_worker
            .take();
        bridge.pinned().borrow_mut().spotify_playback_worker.take();
        drop(engine);
    }

    #[test]
    #[ignore = "live catalog Add and Spotify enrichment on a fresh disposable database"]
    fn live_catalog_add_preferred_spotify() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("fresh.sqlite");
        let library = music_library::Library::open(&path).unwrap();
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml").replace(
            "    function ready() {",
            r#"
    property bool waitingForSpotify: false
    Timer { id: deadline; interval: 60000; onTriggered: Qt.quit() }
    Connections {
        target: window.bridge
        function onCatalog_changed() {
            if (!window.waitingForSpotify && !window.bridge.catalog_snapshot.catalogPending)
                Qt.quit();
        }
        function onSpotify_playback_changed() {
            if (window.waitingForSpotify && window.bridge.spotify_playback_snapshot.resolutionMessage.indexOf("15/15") >= 0)
                Qt.quit();
        }
    }
    function auditSearch() {
        deadline.start();
        window.bridge.catalog_action("search", 'releasegroup:"Demon Days" AND artist:"Gorillaz"');
    }
    Timer { interval: 50; running: window.waitingForSpotify; repeat: true
        onTriggered: {
            if (window.bridge.spotify_playback_snapshot.resolutionMessage.indexOf("15/15") >= 0) Qt.quit();
        }
    }
    function auditWait() { waitingForSpotify = true; deadline.restart(); }
    function ready() {
"#,
        );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        engine.invoke_method("auditSearch".into(), &[]);
        engine.exec();
        let index = bridge
            .pinned()
            .borrow()
            .catalog
            .groups
            .iter()
            .position(|a| {
                a.title == "Demon Days" && a.artist == "Gorillaz" && a.primary_type == "Album"
            })
            .expect("catalog Album");
        engine.invoke_method("addAlbum".into(), &[(index as u32).into()]);
        engine.exec();
        assert!(
            bridge
                .pinned()
                .borrow()
                .catalog
                .status
                .starts_with("Added Album")
        );
        assert!(
            bridge.pinned().borrow().spotify_album_matcher.is_some(),
            "Add must schedule Spotify"
        );
        engine.invoke_method("auditWait".into(), &[]);
        engine.exec();
        let rows = bridge
            .pinned()
            .borrow()
            .session
            .library
            .search(&music_library::domain::SearchRequest {
                limit: 100,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(rows.len(), 15);
        println!(
            "LIVE catalog Add: {}",
            bridge.pinned().borrow().spotify_resolution_message
        );
        bridge.pinned().borrow_mut().spotify_album_matcher.take();
        bridge.pinned().borrow_mut().catalog.worker.take();
        drop(engine);
        drop(bridge);
        let library = music_library::Library::open(&path).unwrap();
        for row in &rows {
            let ids = library
                .track_provider_occurrences(&row.track_id, "spotify")
                .unwrap();
            assert_eq!(ids.len(), 1, "{}", row.title);
            let capability = music_library::playback_resolver::RemoteCapability {
                provider: "spotify",
                unavailable: None,
                catalog_available: true,
                accepts: |_| true,
            };
            assert!(matches!(
                library.playback_route(&row.track_id, &capability).unwrap(),
                music_library::playback_resolver::Route::Remote(_)
            ));
        }
        println!("LIVE catalog Add: reopened 15/15; playback catalog requests=0");
    }

    #[test]
    #[ignore = "opt-in live MusicBrainz timing; requires MUSIC_LIBRARY_CATALOG_LIVE_QUERY"]
    fn live_catalog_latency_audit() {
        let query = std::env::var("MUSIC_LIBRARY_CATALOG_LIVE_QUERY").expect("set a catalog query");
        let (_temp, library) = sample::create().unwrap();
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml").replace(
            "    function ready() {",
            r#"
    Connections {
        target: window.bridge
        function onCatalog_changed() {
            if (!window.bridge.catalog_snapshot.catalogPending) Qt.quit();
        }
    }
    function auditSearch(query) { window.bridge.catalog_action("search", query); }
    function ready() {
"#,
        );
        engine.load_data(qml.into());
        let search_and_add = std::time::Instant::now();
        engine.invoke_method("auditSearch".into(), &[string(query)]);
        engine.exec();
        let index = bridge
            .pinned()
            .borrow()
            .catalog
            .groups
            .iter()
            .position(|a| a.primary_type == "Album")
            .unwrap_or_else(|| {
                panic!(
                    "No Album-type result: {}",
                    bridge.pinned().borrow().catalog.status
                )
            });
        engine.invoke_method("addAlbum".into(), &[(index as u32).into()]);
        engine.exec();
        let status = bridge.pinned().borrow().catalog.status.clone();
        eprintln!("catalog-timing completion={status}");
        eprintln!(
            "catalog-timing full_search_and_add_ms={:.3}",
            search_and_add.elapsed().as_secs_f64() * 1000.0
        );
        bridge.pinned().borrow_mut().catalog.worker.take();
        assert!(status.starts_with("Added Album"), "{status}");
    }

    fn report(engine: &Rc<RefCell<QmlEngine>>) -> String {
        engine
            .borrow_mut()
            .invoke_method("testView".into(), &[])
            .to_qstring()
            .to_string()
    }

    fn command(engine: &Rc<RefCell<QmlEngine>>, name: &str) {
        engine
            .borrow_mut()
            .invoke_method("testCommand".into(), &[string(name)]);
    }

    fn confirm(
        engine: &Rc<RefCell<QmlEngine>>,
        deliver: impl Fn(EngineEvent) + Send + 'static,
        event: EngineEvent,
    ) {
        // Quit is queued after delivery, without a command, clock update, or polling.
        let quit = engine.clone();
        let done = qmetaobject::queued_callback(move |()| quit.borrow().quit());
        let worker = std::thread::spawn(move || {
            deliver(event);
            done(());
        });
        engine.borrow().exec();
        worker.join().unwrap();
    }

    fn matching_flow(engine: &Rc<RefCell<QmlEngine>>, bridge: &QObjectBox<Bridge>) {
        use music_library::{
            album_matching::AlbumMatcher, catalog::*, domain::*, filesystem::MetadataExtractor,
        };
        use std::sync::mpsc;
        struct Tags;
        impl MetadataExtractor for Tags {
            fn supports(&self, _: &std::path::Path) -> bool {
                true
            }
            fn read(&mut self, _: &std::path::Path) -> music_library::Result<ObservedMetadata> {
                Ok(ObservedMetadata {
                    track_title: Some("Song".into()),
                    release_title: Some("Local Album".into()),
                    release_artists: vec!["Artist".into()],
                    ..Default::default()
                })
            }
        }
        struct Provider(mpsc::Receiver<()>, bool);
        impl CatalogProvider for Provider {
            fn recordings(
                &mut self,
                group: &ExternalIdentity,
            ) -> Result<Page<music_library::recording::Candidate>, CatalogError> {
                assert_eq!(group.external_id, "test-group");
                self.0.recv().unwrap();
                Ok(Page {
                    next_offset: None,
                    items: vec![music_library::recording::Candidate {
                        identity: ExternalIdentity {
                            provider: "musicbrainz".into(),
                            kind: "recording".into(),
                            external_id: "test-recording".into(),
                        },
                        title: "Song".into(),
                        artist: "Artist".into(),
                        artist_ids: vec![],
                        duration_ms: None,
                        isrcs: vec!["ISRC1".into(), "ISRC2".into()],
                    }],
                })
            }
            fn search_artists(
                &mut self,
                name: &str,
            ) -> Result<Page<ArtistCandidate>, CatalogError> {
                let candidate = ArtistCandidate {
                    aliases: vec![],
                    identity: ExternalIdentity {
                        provider: "musicbrainz".into(),
                        kind: "artist".into(),
                        external_id: "00000000-0000-4000-8000-000000000002".into(),
                    },
                    name: name.into(),
                    comment: "First band".into(),
                    country: String::new(),
                    artist_type: String::new(),
                    score: Some(100),
                };
                let mut other = candidate.clone();
                other.identity.external_id = "00000000-0000-4000-8000-000000000003".into();
                other.comment = "Another artist".into();
                other.score = Some(80);
                Ok(Page {
                    items: vec![candidate, other],
                    next_offset: None,
                })
            }
            fn artist_albums(
                &mut self,
                artist: &ExternalIdentity,
                title: &str,
            ) -> Result<Page<ArtistAlbumCandidate>, CatalogError> {
                self.0.recv().unwrap();
                if !self.1 {
                    self.1 = true;
                    return Err(CatalogError::ServiceUnavailable {
                        message: "HTTP 503".into(),
                        retry_after: None,
                    });
                }
                Ok(Page {
                    next_offset: None,
                    items: vec![ArtistAlbumCandidate {
                        artist: "Canonical Artist".into(),
                        primary_type: String::new(),
                        title: format!("{title}s"),
                        artist_ids: vec![artist.clone()],
                        identity: ExternalIdentity {
                            provider: "musicbrainz".into(),
                            kind: "release_group".into(),
                            external_id: "test-group".into(),
                        },

                        date: String::new(),

                        comment: String::new(),
                    }],
                })
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
        }
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("song"), b"fixture").unwrap();
        let imported = {
            let pinned = bridge.pinned();
            let mut bridge = pinned.borrow_mut();
            let library = &mut bridge.session.library;
            let root = library.register_local_root(temp.path()).unwrap();
            library.scan_local_root(&root, &mut Tags).unwrap();
            let source = library
                .list_discovery_candidates(None, 10)
                .unwrap()
                .remove(0);
            library
                .import_release(&ImportReleaseRequest {
                    release_title: "Local Album".into(),
                    release_artists: vec![],
                    tracks: vec![ImportTrackInput {
                        source_id: source.source_id,
                        title_fallback: None,
                        artists: vec![],
                        disc_number: None,
                        track_number: None,
                    }],
                })
                .unwrap()
        };
        let (gate, wait) = mpsc::channel();
        let deliver = matching_callback(qmetaobject::QPointer::from(bridge.pinned().borrow()));
        let quit = engine.clone();
        let done = qmetaobject::queued_callback(move |()| quit.borrow().quit());
        let deliver_recording =
            recording_callback(qmetaobject::QPointer::from(bridge.pinned().borrow()));
        let quit_recording = engine.clone();
        let recording_done = qmetaobject::queued_callback(move |()| quit_recording.borrow().quit());
        bridge.pinned().borrow_mut().matcher = Some(
            AlbumMatcher::new_with_recordings(
                Provider(wait, false),
                move |reply| {
                    deliver(reply);
                    done(());
                },
                move |reply| {
                    deliver_recording(reply);
                    recording_done(());
                },
                matching_retry_callback(qmetaobject::QPointer::from(bridge.pinned().borrow())),
            )
            .unwrap()
            .into(),
        );
        bridge
            .pinned()
            .borrow_mut()
            .post_import(std::slice::from_ref(&imported), true)
            .unwrap();
        let view = || {
            engine
                .borrow_mut()
                .invoke_method("matchingViewText".into(), &[])
                .to_qstring()
                .to_string()
        };
        assert!(view().contains("Pending MusicBrainz"));
        assert!(
            bridge
                .pinned()
                .borrow()
                .session
                .library
                .available_playback_source(&imported.track_ids[0])
                .unwrap()
                .is_some()
        );
        // QML methods and library search remain available while the fake network is gated.
        bridge.pinned().borrow_mut().session.search("Song".into());
        engine.borrow().exec();
        assert!(view().contains("Artist ambiguous"));
        assert!(view().contains("First band"));
        bridge.pinned().borrow_mut().choose_artist(0, 0);
        assert!(view().contains("Pending MusicBrainz"));
        gate.send(()).unwrap();
        engine.borrow().exec();
        assert!(view().contains("Deferred — provider unavailable: HTTP 503"));
        assert!(view().contains("matching paused; retrying automatically"));
        assert!(
            bridge
                .pinned()
                .borrow()
                .session
                .library
                .available_playback_source(&imported.track_ids[0])
                .unwrap()
                .is_some()
        );
        // Fake timer expiry follows the actual queued callback path: no user action.
        let token = bridge
            .pinned()
            .borrow()
            .matcher
            .as_ref()
            .unwrap()
            .retry_schedule()
            .unwrap()
            .token;
        let wake = matching_retry_callback(qmetaobject::QPointer::from(bridge.pinned().borrow()));
        wake(token);
        wake(token); // duplicate expiry is harmless
        gate.send(()).unwrap();
        engine.borrow().exec();
        assert!(view().contains("Matched close title: test-group"));
        let displayed = engine
            .borrow_mut()
            .invoke_method("matchedRowText".into(), &[])
            .to_qstring()
            .to_string();
        assert_eq!(
            displayed,
            "Local: Local Album — Artist\nMatched (close): local albums — Canonical Artist [MusicBrainz]"
        );
        assert_eq!(
            bridge
                .pinned()
                .borrow()
                .session
                .library
                .album_for_release(&imported.release_id)
                .unwrap()
                .title,
            "Local Album"
        );
        assert!(!view().contains("matching paused; retrying automatically"));
        assert!(view().contains("Recordings: pending"));
        gate.send(()).unwrap();
        engine.borrow().exec();
        assert!(view().contains("Recordings matched: 1 / 1"));
        assert!(view().contains("MusicBrainz Recording: test-recording"));
        assert!(view().contains("ISRC1, ISRC2"));
        bridge.pinned().borrow_mut().retry_match(0);
        assert!(view().contains("Already matched"));
        bridge.pinned().borrow_mut().matcher.take();
    }

    fn catalog_flow(engine: &Rc<RefCell<QmlEngine>>, bridge: &QObjectBox<Bridge>) {
        use music_library::catalog::{
            AlbumCandidate, CatalogError, CatalogProvider, Medium, Page, Release, ReleaseCandidate,
            Track,
        };
        use music_library::domain::ExternalIdentity;
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        };
        struct Provider {
            gate: mpsc::Receiver<()>,
            calls: Arc<AtomicUsize>,
        }
        fn id(kind: &str) -> ExternalIdentity {
            ExternalIdentity {
                provider: "test".into(),
                kind: kind.into(),
                external_id: "id".into(),
            }
        }
        impl CatalogProvider for Provider {
            fn search_albums(
                &mut self,
                q: &str,
                _: u32,
            ) -> Result<Page<AlbumCandidate>, CatalogError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.gate.recv().unwrap();
                if q == "failure" {
                    return Err(CatalogError::Other(
                        "HTTP 503 test service unavailable".into(),
                    ));
                }
                Ok(Page {
                    next_offset: None,
                    items: vec![AlbumCandidate {
                        credits: vec![music_library::catalog::Credit {
                            identity: None,
                            name: "Artist".into(),
                            join_phrase: String::new(),
                        }],
                        identity: id("group"),
                        title: "Catalog Fixture".into(),
                        artist: "Artist".into(),
                        date: "2001".into(),
                        primary_type: "Album".into(),
                        secondary_types: vec![],
                        comment: "".into(),
                        score: None,
                    }],
                })
            }
            fn releases(
                &mut self,
                _: &ExternalIdentity,
                _: u32,
            ) -> Result<Page<ReleaseCandidate>, CatalogError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(Page {
                    next_offset: None,
                    items: vec![ReleaseCandidate {
                        identity: id("release"),
                        title: "Catalog Fixture".into(),
                        artist: "Artist".into(),
                        date: "2001".into(),
                        country: "US".into(),
                        status: "Official".into(),
                        comment: "Edition".into(),
                        barcode: "".into(),
                        labels: vec![],
                        formats: vec!["CD".into()],
                        disc_count: 1,
                        track_count: 1,
                    }],
                })
            }
            fn representative_releases(
                &mut self,
                group: &ExternalIdentity,
            ) -> Result<Page<ReleaseCandidate>, CatalogError> {
                let mut page = self.releases(group, 0)?;
                for candidate in &mut page.items {
                    candidate.artist.clear();
                    candidate.labels.clear();
                }
                Ok(page)
            }
            fn release(&mut self, _: &ExternalIdentity) -> Result<Release, CatalogError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(Release {
                    album: music_library::catalog::Album {
                        release_type: None,
                        identity: id("group"),
                        title: "Catalog Fixture".into(),
                        date: "2001".into(),
                        credits: vec![music_library::catalog::Credit {
                            identity: None,
                            name: "Artist".into(),
                            join_phrase: String::new(),
                        }],
                    },
                    identity: id("release"),
                    identities: vec![],
                    title: "Catalog Fixture".into(),
                    date: "2001".into(),
                    credits: vec![],
                    media: vec![Medium {
                        position: 1,
                        tracks: vec![Track {
                            duration: None,
                            position: 1,
                            title: "Catalog Song".into(),
                            credits: vec![],
                            identities: vec![id("track")],
                        }],
                    }],
                })
            }
        }
        let (gate, recv) = mpsc::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let (_, enrichment_recv) = mpsc::channel();
        bridge.pinned().borrow_mut().spotify_album_matcher = Some(
            music_library::album_matching::AlbumMatcher::for_provider(
                Provider {
                    gate: enrichment_recv,
                    calls: Arc::new(AtomicUsize::new(0)),
                },
                music_library_spotify::matching_scope(),
                |_| {},
                |_| {},
                |_| {},
            )
            .unwrap(),
        );
        let deliver = catalog_callback(qmetaobject::QPointer::from(bridge.pinned().borrow()));
        let quit = engine.clone();
        let done = qmetaobject::queued_callback(move |()| quit.borrow().quit());
        bridge.pinned().borrow_mut().catalog.worker = Some(
            catalog::Worker::new(
                Provider {
                    gate: recv,
                    calls: calls.clone(),
                },
                move |reply| {
                    deliver(reply);
                    done(());
                },
            )
            .unwrap(),
        );
        let invoke = |action: &str, value: &str| {
            engine
                .borrow_mut()
                .invoke_method("testCatalog".into(), &[string(action), string(value)]);
        };
        let snapshot = || {
            engine
                .borrow_mut()
                .invoke_method("testCatalogView".into(), &[])
                .to_qstring()
                .to_string()
        };
        let layout = || {
            engine
                .borrow_mut()
                .invoke_method("testCatalogLayout".into(), &[])
                .to_qstring()
                .to_string()
        };
        assert!(layout().starts_with("false|0|"));
        invoke("search", "Catalog");
        assert!(snapshot().starts_with("true|"));
        invoke("search", "duplicate"); // Must be rejected even if called directly through QML.
        // Process a Qt callback while the provider is deliberately blocked.
        let quit = engine.clone();
        let ping = qmetaobject::queued_callback(move |()| quit.borrow().quit());
        std::thread::spawn(move || ping(())).join().unwrap();
        engine.borrow().exec();
        assert!(snapshot().starts_with("true|"));
        gate.send(()).unwrap();
        engine.borrow().exec();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(snapshot().ends_with("|1|0"));
        assert!(layout().starts_with("false|1|Catalog Fixture"));
        assert!(
            layout().contains("Artist") && layout().contains("2001") && layout().contains("Album")
        );
        assert!(!layout().contains("barcode"));
        let transport_before_add = bridge.pinned().borrow().session.playback.state().clone();
        invoke("add_album", "0");
        engine.borrow().exec();
        assert!(snapshot().contains("Added Album: 1 Tracks"));
        // A MusicBrainz-mode catalog add schedules Spotify separately; no Play/poll.
        assert!(!bridge.pinned().borrow().spotify);
        assert_eq!(
            bridge
                .pinned()
                .borrow()
                .spotify_album_matcher
                .as_ref()
                .unwrap()
                .pending_count(),
            1
        );
        assert_eq!(
            bridge.pinned().borrow().session.playback.state(),
            &transport_before_add
        );
        assert!(!bridge.pinned().borrow().session.rows[0].available);
        assert_eq!(calls.load(Ordering::SeqCst), 3); // Search + lightweight candidates + lookup.
        assert!(bridge.pinned().borrow().catalog.editions.is_empty());
        let first_import = bridge.pinned().borrow().session.rows[0].track_id.clone();
        invoke("editions", "0");
        engine.borrow().exec();
        assert!(snapshot().ends_with("|1|1"));
        assert!(layout().starts_with("true|1|"));
        assert_eq!(calls.load(Ordering::SeqCst), 4); // Editions fetched rich metadata separately.
        assert_eq!(
            bridge.pinned().borrow().catalog.editions[0].artist,
            "Artist"
        );
        assert_eq!(
            engine
                .borrow_mut()
                .invoke_method("testEditionSelected".into(), &[])
                .to_qstring()
                .to_string(),
            "-1"
        );
        engine
            .borrow_mut()
            .invoke_method("testEditionIndex".into(), &[0.into()]);
        bridge.pinned().borrow().changed(); // Playback/clock notifications must not reset edition selection.
        assert_eq!(
            engine
                .borrow_mut()
                .invoke_method("testEditionSelected".into(), &[])
                .to_int(),
            0
        );
        let transport = bridge.pinned().borrow().session.playback.state().clone();
        invoke("add", "0");
        engine.borrow().exec();
        assert!(snapshot().contains("Added Album: 1 Tracks"));
        assert_eq!(
            bridge.pinned().borrow().session.playback.state(),
            &transport
        );
        assert_eq!(bridge.pinned().borrow().session.rows.len(), 1);
        assert!(!bridge.pinned().borrow().session.rows[0].available);
        let imported = bridge.pinned().borrow().session.rows[0].track_id.clone();
        assert_eq!(first_import, imported);
        assert_eq!(calls.load(Ordering::SeqCst), 4); // Re-add reused the detail, too.
        bridge.pinned().borrow_mut().spotify_album_matcher.take();
        bridge.pinned().borrow_mut().auto_match = false;
        // Re-add under disabled policy must not construct a Spotify client/worker.
        invoke("add_album", "0");
        engine.borrow().exec();
        assert!(bridge.pinned().borrow().spotify_album_matcher.is_none());
        bridge.pinned().borrow_mut().auto_match = true;
        invoke("search", "failure");
        gate.send(()).unwrap();
        engine.borrow().exec();
        assert!(snapshot().contains("HTTP 503"));
        assert_eq!(bridge.pinned().borrow().session.rows[0].track_id, imported);
        bridge.pinned().borrow_mut().catalog.worker.take();
    }

    #[test]
    fn async_confirmations_refresh_actual_qml_labels_without_another_command() {
        use music_library::playback::PlaybackStatus::{Paused, Playing, Stopped};
        let engine = Rc::new(RefCell::new(QmlEngine::new()));
        let (_temp, library) = sample::create().unwrap();
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        // Exercise callback construction before QML exposure, too.
        let first_delivery = engine_callback(bridge.pinned());
        {
            let pinned = bridge.pinned();
            let mut bridge = pinned.borrow_mut();
            bridge.session.defer_confirmations();
            bridge.session.search("Available".into());
            bridge.session.queue_page();
        }
        engine
            .borrow_mut()
            .set_object_property("diagnostic".into(), bridge.pinned());
        // Load the real window/bindings; only add test accessors and label IDs.
        let qml = include_str!("../Main.qml")
            .replace("onMatchingViewChanged: syncMatchingRows(matchingView)", "onMatchingViewChanged: { syncMatchingRows(matchingView); if (testPresentation) testProviderPresentation(20); }")
            .replace(
                "    function ready() {",
                r#"
    function matchingViewText() { return JSON.stringify(window.matchingView) + JSON.stringify(window.bridge.matching_provider); }
    property bool testPresentation: false
    function matchedRowText() { return window.matchingLabel(matchingModel.get(0).rowData); }
    function testProviderPresentation(index) {
        const rows = JSON.parse(JSON.stringify(window.matchingView));
        rows[index].matchedTitle = "Provider Album";
        rows[index].matchedArtist = "tsosis";
        rows[index].matchedClose = true;
        window.syncMatchingRows(rows);
    }
    function providerPresentationText() {
        for (let i = 0; i < matchingModel.count; ++i) {
            if (matchingModel.get(i).albumKey === "scroll-20")
                return window.matchingLabel(matchingModel.get(i).rowData);
        }
        return "missing row";
    }
    function prepareMatchingScrollTest() {
        testPresentation = true;
        matchingDialog.open();
        matchingList.forceLayout();
        matchingList.currentIndex = 50;
        matchingList.positionViewAtIndex(50, ListView.Center);
        matchingList.forceLayout();
        matchingList.currentItem.forceActiveFocus();
        return matchingScrollState();
    }
    function matchingScrollState() {
        matchingList.forceLayout();
        return matchingList.contentY + "|" + matchingList.currentIndex + "|"
            + matchingList.currentItem.rowData.albumId + "|" + matchingList.currentItem.activeFocus;
    }
    function testTrackExpansion(rows) {
        const index = matchingList.currentIndex;
        const row = JSON.parse(JSON.stringify(matchingModel.get(index).rowData));
        row.tracks = rows;
        matchingModel.setProperty(index, "rowData", row);
        matchingModel.setProperty(index, "expanded", true);
        matchingList.forceLayout();
        const labels = [];
        function visit(item) {
            if (item.objectName && item.objectName.indexOf("program-track-") === 0) labels.push(item.text);
            if (item.children) for (const child of item.children) visit(child);
        }
        visit(matchingList.currentItem);
        matchingModel.setProperty(index, "expanded", false);
        matchingList.forceLayout();
        matchingModel.setProperty(index, "expanded", true);
        const refreshed = [];
        for (let i = 0; i < matchingModel.count; ++i) refreshed.push(matchingModel.get(i).rowData);
        refreshed[0].status = "Unrelated background completion";
        syncMatchingRows(refreshed);
        return matchingModel.get(index).expanded + "|" + labels.join("|");
    }
    function testManualDialog() {
        const index = matchingList.currentIndex;
        const anchor = matchingModel.get(index).albumKey;
        const y = matchingList.contentY;
        manualTrackDialog.open();
        const initial = manualTrackChoice.currentIndex + "|" + manualTrackConfirm.enabled;
        manualTrackDialog.close();
        matchingList.forceLayout();
        return initial + "|" + (anchor === matchingModel.get(index).albumKey)
            + "|" + matchingModel.get(index).expanded + "|" + (y === matchingList.contentY);
    }
    function testManualConfirm() {
        manualTrackDialog.open();
        const initial = manualTrackChoice.currentIndex + "|" + manualTrackConfirm.enabled;
        manualTrackChoice.currentIndex = 0;
        const enabled = manualTrackConfirm.enabled;
        manualTrackConfirm.clicked();
        return initial + "|" + enabled + "|" + manualTrackDialog.visible;
    }
    function testClearManual(album, track) { window.bridge.clear_track_choice(album, track); }
    function testSpotifySongChoice(action) {
        if (action === "select") spotifySongChoice.currentIndex = 0;
        if (action === "confirm") window.bridge.spotify_resolve("confirm", spotifySongChoice.currentIndex);
        if (action === "cancel") window.bridge.spotify_resolve("cancel", -1);
        return spotifySongChoice.currentIndex + "|" + window.spotifyPlayback.available;
    }
    function changedMatchingRowStatus() {
        for (let i = 0; i < matchingModel.count; ++i) {
            if (matchingModel.get(i).albumKey === "scroll-1")
                return matchingModel.get(i).rowData.status;
        }
        return "missing row";
    }
    function testView() {
        return view.status + "|" + view.pending + "|" + view.outcome
            + "|" + playbackLabel.text + "|" + outcomeLabel.text;
    }
    function testCommand(name) { window.bridge.command(name); }
    function testCatalog(action, value) {
        if (action === "add_album") window.addAlbum(Number(value));
        else if (action === "editions") window.chooseEditions(Number(value));
        else window.bridge.catalog_action(action, value);
    }
    function testCatalogLayout() {
        return catalogDialog.showEditions + "|" + albumResults.count + "|" +
            (albumResults.count ? window.catalogView.catalogGroups[0].label : "");
    }
    function testCatalogView() {
        return catalogView.catalogPending + "|" + catalogView.catalogStatus + "|"
            + catalogView.catalogGroups.length + "|" + catalogView.catalogEditions.length;
    }
    function testEditionIndex(value) { editions.currentIndex = value; return editions.currentIndex; }
    function testEditionSelected() { return editions.currentIndex; }
    function ready() {"#,
            )
            .replace(
                r#"text: "Current: ""#,
                r#"id: playbackLabel
            text: "Current: ""#,
            )
            .replace(
                r#"text: "State update ""#,
                r#"id: outcomeLabel
            text: "State update ""#,
            );
        engine.borrow_mut().load_data(qml.into());
        command(&engine, "play");
        assert!(report(&engine).contains("Playing pending"));
        confirm(
            &engine,
            first_delivery,
            EngineEvent {
                generation: 1,
                kind: Event::State(Playing),
            },
        );
        assert!(report(&engine).starts_with("Playing||"));
        command(&engine, "pause");
        let pending = report(&engine);
        assert!(pending.starts_with("Playing| → Paused pending|pause → Playing → Paused pending|"));
        confirm(
            &engine,
            engine_callback(bridge.pinned()),
            EngineEvent {
                generation: 1,
                kind: Event::State(Paused),
            },
        );
        let paused = report(&engine);
        assert!(paused.starts_with("Paused||"));
        assert!(paused.contains(" · Paused · "));
        assert!(paused.contains("pause → Paused"));
        assert!(!paused.contains("pending"));
        command(&engine, "play");
        assert!(report(&engine).starts_with("Paused| → Playing pending|"));
        confirm(
            &engine,
            engine_callback(bridge.pinned()),
            EngineEvent {
                generation: 1,
                kind: Event::State(Playing),
            },
        );
        assert!(report(&engine).starts_with("Playing||play → Playing|"));
        command(&engine, "stop");
        assert!(report(&engine).contains("Stopped pending"));
        confirm(
            &engine,
            engine_callback(bridge.pinned()),
            EngineEvent {
                generation: 2,
                kind: Event::State(Stopped),
            },
        );
        assert!(report(&engine).starts_with("Stopped||stop → Stopped|"));
        command(&engine, "play");
        confirm(
            &engine,
            engine_callback(bridge.pinned()),
            EngineEvent {
                generation: 3,
                kind: Event::Error(EngineError("test output failure".into())),
            },
        );
        assert!(report(&engine).starts_with("Failed||engine event → Failed|"));
        let failed = report(&engine);
        confirm(
            &engine,
            engine_callback(bridge.pinned()),
            EngineEvent {
                generation: 1,
                kind: Event::State(Playing),
            },
        );
        assert_eq!(report(&engine), failed);
        catalog_flow(&engine, &bridge);
        matching_flow(&engine, &bridge);
        spotify_song_resolution_ui(&engine, &bridge);
        playback_route::test_handoffs();
        playback_route::test_mixed_queue();
        // A real queued QObject notification updates B while the user inspects Q.
        {
            let pinned = bridge.pinned();
            let mut state = pinned.borrow_mut();
            state.matching_rows = (0..100)
                .map(|n| {
                    (
                        music_library::domain::AlbumId(format!("scroll-{n}")),
                        format!("Album {n}"),
                        music_library::album_matching::MatchOutcome::Pending,
                        "Local Artist".into(),
                    )
                })
                .collect();
            state.matching_changed();
        }
        let before = engine
            .borrow_mut()
            .invoke_method("prepareMatchingScrollTest".into(), &[])
            .to_qstring()
            .to_string();
        let pointer = qmetaobject::QPointer::from(bridge.pinned().borrow());
        let quit = engine.clone();
        let update = qmetaobject::queued_callback(move |index: usize| {
            if let Some(pinned) = pointer.as_pinned() {
                let mut state = pinned.borrow_mut();
                state.matching_rows[index].2 =
                    music_library::album_matching::MatchOutcome::NoConfidentMatch;
                state.matching_changed();
            }
            if index == 20 {
                quit.borrow().quit();
            }
        });
        let worker = std::thread::spawn(move || {
            for index in 1..=20 {
                update(index);
            }
        });
        engine.borrow().exec();
        worker.join().unwrap();
        let after = engine
            .borrow_mut()
            .invoke_method("matchingScrollState".into(), &[])
            .to_qstring()
            .to_string();
        assert_eq!(
            before, after,
            "unrelated completion must retain viewport, selection and focus"
        );
        assert!(after.ends_with("|true"), "focused delegate: {after}");
        {
            use music_library::{
                album_program::{Match, Outcome, TrackOutcome},
                domain::{RecordingId, TrackId},
                edition::{LocalTrackEvidence, RecordingEvidence, TrackEvidence},
            };
            let local = LocalTrackEvidence {
                track_id: TrackId("ui-track".into()),
                recording_id: RecordingId("ui-recording".into()),
                evidence: TrackEvidence {
                    title: Some("Feel Good Inc".into()),
                    ..Default::default()
                },
            };
            let outcome = Outcome::Complete(vec![(
                local.clone(),
                TrackOutcome::Matched(Match {
                    duration: None,
                    title: "Feel Good Inc.".into(),
                    recording: RecordingEvidence::default(),
                    recording_status: music_library::album_program::RecordingStatus::Ambiguous,
                    occurrences: vec![],
                    explanation: "fixture".into(),
                }),
            )]);
            let rows = program_rows(std::slice::from_ref(&local), Some(&outcome), &[]);
            let rendered = engine
                .borrow_mut()
                .invoke_method("testTrackExpansion".into(), &[rows.into()])
                .to_qstring()
                .to_string();
            assert_eq!(
                rendered,
                "true|Local: Feel Good Inc → Matched: Feel Good Inc. (Matched; Recording: Ambiguous)"
            );
            assert_eq!(
                engine
                    .borrow_mut()
                    .invoke_method("testManualDialog".into(), &[])
                    .to_qstring()
                    .to_string(),
                "-1|false|true|true|true",
                "opening/cancelling must not confirm, collapse or move the Album"
            );
            let manual = music_library::manual_track::Association {
                track_id: local.track_id.clone(),
                album: music_library::domain::ExternalIdentity {
                    provider: "fixture".into(),
                    kind: "album".into(),
                    external_id: "known".into(),
                },
                candidate: music_library::manual_track::Candidate {
                    evidence: TrackEvidence {
                        title: Some("Chosen provider title".into()),
                        ..Default::default()
                    },
                    supporting_programs: 2,
                },
            };
            let song_outcome = Outcome::Complete(vec![(
                local.clone(),
                TrackOutcome::Matched(music_library::album_program::Match {
                    duration: None,
                    title: "Feel Good Inc.".into(),
                    recording: RecordingEvidence::default(),
                    recording_status: music_library::album_program::RecordingStatus::NotProvided,
                    occurrences: vec![music_library::domain::ExternalIdentity {
                        provider: "spotify".into(),
                        kind: "track".into(),
                        external_id: "song".into(),
                    }],
                    explanation: "Album-scoped song".into(),
                }),
            )]);
            let rows = program_rows(std::slice::from_ref(&local), Some(&song_outcome), &[]);
            assert!(
                !rows[0]
                    .to_qvariantmap()
                    .value("canChoose".into(), QVariant::default())
                    .to_bool()
            );
            assert_eq!(
                engine
                    .borrow_mut()
                    .invoke_method("testTrackExpansion".into(), &[rows.into()])
                    .to_qstring()
                    .to_string(),
                "true|Local: Feel Good Inc → Matched: Feel Good Inc. (Matched; provider song identified)"
            );
            let rows = program_rows(&[local], Some(&Outcome::Pending), &[manual]);
            assert_eq!(
                engine
                    .borrow_mut()
                    .invoke_method("testTrackExpansion".into(), &[rows.into()])
                    .to_qstring()
                    .to_string(),
                "true|Local: Feel Good Inc → Matched: Chosen provider title (Manual; provider song identified)",
                "stored manual presentation survives pending automatic refresh"
            );
        }
        assert_eq!(
            engine
                .borrow_mut()
                .invoke_method("providerPresentationText".into(), &[])
                .to_qstring()
                .to_string(),
            "Local: Album 20 — Local Artist\nMatched (close): Provider Album — tsosis [MusicBrainz]"
        );
        assert_eq!(
            engine
                .borrow_mut()
                .invoke_method("changedMatchingRowStatus".into(), &[])
                .to_qstring()
                .to_string(),
            "No confident match"
        );
        // Exercise the real explicit-confirm handler with a generic occurrence-only
        // candidate. Preparation/confirmation themselves perform no provider work.
        let (album, track) = {
            use music_library::{
                album_program::{Program, Programs},
                domain::ExternalIdentity,
                edition::TrackEvidence,
            };
            let pinned = bridge.pinned();
            let mut state = pinned.borrow_mut();
            let row = state.session.rows[0].clone();
            let album = state
                .session
                .library
                .album_for_release(&row.release_id)
                .unwrap()
                .album_id;
            let identity = ExternalIdentity {
                provider: "manual-ui-fixture".into(),
                kind: "album".into(),
                external_id: "known".into(),
            };
            state
                .session
                .library
                .attach_album_external_identity(&album, &identity)
                .unwrap();
            let programs = Programs {
                album: identity,
                programs: vec![Program {
                    identity: None,
                    complete: true,
                    tracks: vec![TrackEvidence {
                        title: Some("Explicitly chosen song".into()),
                        identities: vec![ExternalIdentity {
                            provider: "manual-ui-fixture".into(),
                            kind: "song".into(),
                            external_id: "chosen".into(),
                        }],
                        ..Default::default()
                    }],
                }],
                note: String::new(),
            };
            state.manual_selection = Some(
                state
                    .session
                    .library
                    .prepare_manual_track(&album, &row.track_id, &programs)
                    .unwrap(),
            );
            state.manual_context = Some((album.clone(), row.track_id.clone()));
            state.manual_changed();
            (album, row.track_id)
        };
        assert_eq!(
            engine
                .borrow_mut()
                .invoke_method("testManualConfirm".into(), &[])
                .to_qstring()
                .to_string(),
            "-1|false|true|false"
        );
        {
            let pinned = bridge.pinned();
            let mut state = pinned.borrow_mut();
            let choices = state
                .session
                .library
                .manual_track_associations(&album)
                .unwrap();
            assert_eq!(choices.len(), 1);
            assert_eq!(
                choices[0].candidate.evidence.title.as_deref(),
                Some("Explicitly chosen song")
            );
            state
                .session
                .library
                .clear_manual_track(&album, &track)
                .unwrap();
        }
        clear_manual_program_flow(&engine, &bridge);
        drop(engine); // Destroy QML bindings while their QObject still exists.
    }

    fn spotify_song_resolution_ui(engine: &Rc<RefCell<QmlEngine>>, bridge: &QObjectBox<Bridge>) {
        use music_library::{
            domain::SearchRequest,
            song_resolution::{Candidate, Selection},
        };
        let selection = {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            let other = b.session.playback.state().current_track().unwrap().clone();
            let row = b
                .session
                .library
                .search(&SearchRequest {
                    limit: 2,
                    ..Default::default()
                })
                .unwrap()
                .into_iter()
                .find(|r| r.track_id != other)
                .unwrap();
            // Selection is independent from Now Playing and the loaded Songs page.
            let loaded_rows = std::mem::take(&mut b.session.rows);
            b.spotify_playback_action("track".into(), row.track_id.as_ref().into());
            assert_eq!(b.session.playback.state().current_track(), Some(&other));
            assert_eq!(b.spotify_playback_track.as_ref(), Some(&row.track_id));
            assert!(b.spotify_playback_title.contains(&row.title));
            b.session.rows = loaded_rows;
            let input = b
                .session
                .library
                .song_resolution_input(&row.track_id)
                .unwrap();
            let initial = Selection::new(
                input.clone(),
                vec![Candidate {
                    album_identity: None,
                    album_artists: vec![],
                    album_type: String::new(),
                    album_total_tracks: None,
                    identity: music_library::domain::ExternalIdentity {
                        provider: "spotify".into(),
                        kind: "track".into(),
                        external_id: "1234567890123456789012".into(),
                    },
                    title: "Provider Song".into(),
                    artist: "Artist".into(),
                    artists: vec![],
                    album: "Album variant".into(),
                    date: "2020".into(),
                    duration_ms: 200000,
                    disc: 1,
                    number: 1,
                }],
            );
            let mut feasible = initial.candidates()[0].clone();
            feasible.identity.external_id = "2234567890123456789012".into();
            feasible.title = input.title.clone();
            feasible.artist = input.artist.clone();
            feasible.artists = input.artists.clone();
            feasible.album = input.album.clone();
            feasible.disc = input.disc.unwrap_or(1);
            feasible.number = input.number.unwrap_or(1);
            Selection::new(input, vec![initial.candidates()[0].clone(), feasible])
        };
        let publish = || {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.spotify_resolution_selection = Some(selection.clone());
            b.spotify_playback_changed();
            assert_eq!(
                b.spotify_resolution_selection
                    .as_ref()
                    .unwrap()
                    .visible_indices(),
                vec![1]
            );
            assert!(
                b.session
                    .library
                    .track_provider_occurrences(&selection.input().track_id, "spotify")
                    .unwrap()
                    .is_empty()
            );
            let counts = b.spotify_resolution_counts;
            b.spotify_resolve("show-all".into(), -1);
            assert_eq!(b.spotify_resolution_counts, counts);
            assert_eq!(
                b.spotify_resolution_selection
                    .as_ref()
                    .unwrap()
                    .visible_indices(),
                vec![1, 0]
            );
        };
        let action = |action: &str| {
            engine
                .borrow_mut()
                .invoke_method("testSpotifySongChoice".into(), &[string(action)])
                .to_qstring()
                .to_string()
        };
        publish();
        assert_eq!(action(""), "-1|false");
        assert_eq!(action("select"), "0|false");
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.spotify_playback_state.state.progress_ms = 9000;
            b.spotify_playback_changed();
        }
        assert_eq!(
            action(""),
            "0|false",
            "unrelated playback notification must preserve selection"
        );
        assert_eq!(action("cancel"), "-1|false");
        assert!(
            bridge
                .pinned()
                .borrow()
                .session
                .library
                .track_provider_occurrences(&selection.input().track_id, "spotify")
                .unwrap()
                .is_empty()
        );
        publish();
        action("select");
        assert_eq!(action("confirm"), "-1|true");
        let pinned = bridge.pinned();
        let b = pinned.borrow();
        assert_eq!(
            b.session
                .library
                .track_provider_occurrences(&selection.input().track_id, "spotify")
                .unwrap(),
            vec![selection.candidates()[1].identity.clone()]
        );
        let playing = b.session.playback.state().current_track().unwrap();
        assert_ne!(playing, &selection.input().track_id);
        assert!(
            b.session
                .library
                .track_provider_occurrences(playing, "spotify")
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            b.spotify_playback_track.as_ref(),
            Some(&selection.input().track_id)
        );
        assert!(b.spotify_resolution_worker.is_none());
        assert!(b.spotify_playback_worker.is_none());
        assert_eq!(b.spotify_resolution_counts, (0, 0));
    }
    fn clear_manual_program_flow(engine: &Rc<RefCell<QmlEngine>>, bridge: &QObjectBox<Bridge>) {
        use music_library::{
            album_matching::AlbumMatcher, album_program::*, catalog::*, domain::ExternalIdentity,
        };
        struct Provider(Programs);
        impl CatalogProvider for Provider {
            fn album_program_namespaces(&self) -> Vec<(String, String)> {
                vec![(self.0.album.provider.clone(), self.0.album.kind.clone())]
            }
            fn album_programs(&mut self, _: &ExternalIdentity) -> Result<Programs, CatalogError> {
                Ok(self.0.clone())
            }
            fn search_albums(
                &mut self,
                _: &str,
                _: u32,
            ) -> Result<Page<AlbumCandidate>, CatalogError> {
                panic!("Album already known")
            }
            fn releases(
                &mut self,
                _: &ExternalIdentity,
                _: u32,
            ) -> Result<Page<ReleaseCandidate>, CatalogError> {
                panic!()
            }
            fn release(&mut self, _: &ExternalIdentity) -> Result<Release, CatalogError> {
                panic!()
            }
        }
        let (album, track, programs) = {
            let pinned = bridge.pinned();
            let mut state = pinned.borrow_mut();
            let (album, tracks) = state
                .matching_tracks
                .iter()
                .find(|(album, rows)| {
                    !rows.is_empty()
                        && state
                            .session
                            .library
                            .list_album_external_identities(album)
                            .unwrap()
                            .iter()
                            .any(|id| id.provider == "musicbrainz" && id.kind == "release_group")
                })
                .unwrap();
            let album = album.clone();
            let tracks = tracks.clone();
            let track = tracks[0].track_id.clone();
            let identity = state
                .session
                .library
                .list_album_external_identities(&album)
                .unwrap()
                .into_iter()
                .find(|id| id.provider == "musicbrainz" && id.kind == "release_group")
                .unwrap();
            let programs = Programs {
                album: identity,
                programs: vec![Program {
                    identity: None,
                    complete: true,
                    tracks: tracks.iter().map(|t| t.evidence.clone()).collect(),
                }],
                note: String::new(),
            };
            let selection = state
                .session
                .library
                .prepare_manual_track(&album, &track, &programs)
                .unwrap();
            state
                .session
                .library
                .confirm_manual_track(&selection, 0)
                .unwrap();
            state.refresh_manual_album(&album).unwrap();
            (album, track, programs)
        };
        let deliver = program_callback(qmetaobject::QPointer::from(bridge.pinned().borrow()));
        let quit = engine.clone();
        let done = qmetaobject::queued_callback(move |()| quit.borrow().quit());
        bridge.pinned().borrow_mut().catalog_providers =
            vec!["spotify".into(), "musicbrainz".into()];
        bridge.pinned().borrow_mut().matcher = Some(
            music_library::provider_chain::ProviderChain::new(vec![
                music_library::provider_chain::ProviderSlot {
                    scope: music_library_spotify::matching_scope(),
                    matcher: Err("Unconfigured diagnostic provider".into()),
                },
                music_library::provider_chain::ProviderSlot {
                    scope: music_library::catalog::MatchingScope::musicbrainz(),
                    matcher: Ok(AlbumMatcher::new_with_programs(
                        Provider(programs),
                        |_| panic!(),
                        move |reply| {
                            deliver(reply);
                            done(());
                        },
                        |_| {},
                    )
                    .unwrap()),
                },
            ])
            .unwrap(),
        );
        engine.borrow_mut().invoke_method(
            "testClearManual".into(),
            &[
                QString::from(album.as_ref()).into(),
                QString::from(track.as_ref()).into(),
            ],
        );
        assert_eq!(
            bridge
                .pinned()
                .borrow()
                .matcher
                .as_ref()
                .unwrap()
                .program_outcome(&album),
            Some(&Outcome::Pending)
        );
        engine.borrow().exec();
        let pinned = bridge.pinned();
        let state = pinned.borrow();
        assert!(
            matches!(state.matcher.as_ref().unwrap().program_outcome(&album),Some(Outcome::Complete(rows)) if !rows.is_empty())
        );
        assert_eq!(state.matcher.as_ref().unwrap().pending_count(), 0);
        assert!(state.manual_associations[&album].is_empty());
        // Join while the test's main-thread engine owner still exists.
        bridge.pinned().borrow_mut().matcher.take();
    }
}

#[cfg(test)]
mod library_ui_tests {
    use super::*;
    #[cfg(feature = "gstreamer")]
    #[test]
    #[ignore = "opt-in desktop/audio audit on a disposable copy of a real library"]
    fn real_library_desktop_audit() {
        let path = std::env::var("MUSIC_LIBRARY_UI_AUDIT_DATABASE")
            .expect("set a disposable library copy");
        let library = music_library::Library::open(path).unwrap();
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let mut engine = QmlEngine::new();
        let audio = std::env::var_os("MUSIC_LIBRARY_UI_AUDIT_AUDIO").is_some();
        if audio {
            let deliver = engine_callback(bridge.pinned());
            let player = music_library_gstreamer::GStreamerEngine::new(deliver).unwrap();
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.real_audio = true;
            b.session.playback =
                music_library::playback::Playback::new(session::Engine::GStreamer(player));
        }
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n{}\n    function ready() {{",
                    include_str!("../LibraryDesktopAudit.qml"),
                    include_str!("../LibrarySearchTest.qml").replace("uiTest", "desktopTest")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        let result = engine
            .invoke_method("desktopAudit".into(), &[audio.into()])
            .to_qstring()
            .to_string();
        bridge.pinned().borrow_mut().session.shutdown_audio();
        assert_eq!(result, "ok");
        if std::env::var_os("MUSIC_LIBRARY_SEARCH_AUDIT").is_some() {
            assert_eq!(
                engine
                    .invoke_method("exerciseLocalSearch".into(), &[true.into()])
                    .to_qstring()
                    .to_string(),
                "ok"
            );
        }
    }

    #[test]
    fn local_file_chooser_native_dialogs_suppression_and_folder_import() {
        let temp = tempfile::tempdir().unwrap();
        let downloads = temp.path().join("Downloads");
        let music = temp.path().join("Second Music");
        std::fs::create_dir(&downloads).unwrap();
        std::fs::create_dir(&music).unwrap();
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/provenance/silence.flac");
        let file = downloads.join("song ü space.flac");
        let one = music.join("one.flac");
        let two = music.join("two.flac");
        for path in [&file, &one, &two] {
            std::fs::copy(&fixture, path).unwrap();
            use lofty::{
                config::WriteOptions,
                prelude::{Accessor, TagExt},
                tag::{ItemKey, Tag, TagType},
            };
            let mut tag = Tag::new(TagType::VorbisComments);
            tag.set_title(path.file_stem().unwrap().to_string_lossy().into());
            tag.set_album("Test Album".into());
            tag.set_artist("Test Artist".into());
            tag.insert_text(ItemKey::AlbumArtist, "Test Artist".into());
            tag.save_to_path(path, WriteOptions::default()).unwrap();
        }
        let bad = music.join("bad.flac");
        let unsupported = music.join("notes.txt");
        std::fs::write(&bad, b"malformed").unwrap();
        std::fs::write(&unsupported, b"notes").unwrap();
        let snapshots: Vec<_> = [&file, &one, &two, &bad, &unsupported]
            .into_iter()
            .map(|p| (p.clone(), std::fs::read(p).unwrap()))
            .collect();
        let path = temp.path().join("library.sqlite");
        let library = music_library::Library::open(&path).unwrap();
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        bridge.pinned().borrow_mut().auto_match = false;
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n    function ready() {{",
                    include_str!("../LocalImportTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        let urls: Vec<_> = [&file, &music, &bad, &unsupported, &one]
            .into_iter()
            .map(|p| string(url::Url::from_file_path(p).unwrap()))
            .collect();
        assert_eq!(
            engine
                .invoke_method("exerciseLocalImport".into(), &urls)
                .to_qstring()
                .to_string(),
            "ok"
        );
        {
            let pinned = bridge.pinned();
            let b = pinned.borrow();
            let roots = b.session.library.local_locations().unwrap();
            assert_eq!(roots.len(), 1);
            assert_eq!(roots[0].path, music.canonicalize().unwrap());
            assert!(!roots.iter().any(|r| r.path == downloads));
            assert!(
                !b.matching_rows.is_empty(),
                "same post_import scheduling hook ran"
            );
            assert_eq!(
                b.session
                    .library
                    .local_search("song", music_library::library_search::Kind::Song)
                    .unwrap()
                    .len(),
                1
            );
            assert!(b.session.playback.state().queue.is_empty());
        }
        assert_eq!(
            engine
                .invoke_method(
                    "exerciseSecondLocation".into(),
                    &[string(url::Url::from_file_path(&downloads).unwrap())]
                )
                .to_qstring()
                .to_string(),
            "ok"
        );
        let mut reopened = music_library::Library::open(&path).unwrap();
        let report = reopened
            .ingest_local(
                &music_library::local_ingestion::Request::ConfiguredLocations,
                &mut music_library::filesystem::LoftyMetadataExtractor,
                &mut |_| {},
            )
            .unwrap();
        assert_eq!(report.imported, 0);
        assert_eq!(reopened.local_locations().unwrap().len(), 2);
        for (path, bytes) in snapshots {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
    }

    #[test]
    fn library_removal_confirmation_and_queue_snapshot() {
        let (_temp, library) = sample::create().unwrap();
        let library = if let Some(path) = std::env::var_os("MUSIC_LIBRARY_REMOVAL_AUDIT_COPY") {
            music_library::Library::open(path).unwrap()
        } else {
            library
        };
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n    function ready() {{",
                    include_str!("../LibraryRemovalTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        assert_eq!(
            engine
                .invoke_method("exerciseRemovalUi".into(), &[])
                .to_qstring()
                .to_string(),
            "ok"
        );
    }

    #[test]
    fn fixed_album_tiles_wrap_and_split_handles_preserve_interactions() {
        use music_library::domain::{ArtistCreditInput, CatalogReleaseInput, CatalogTrackInput};
        let temp = tempfile::tempdir().unwrap();
        let mut library = music_library::Library::open(temp.path().join("tiles.db")).unwrap();
        for n in 0..30 {
            let release = library
                .create_catalog_release(&CatalogReleaseInput {
                    title: format!("Tile Album {n:03}"),
                    year: Some(2000 + n / 10),
                    artists: vec![ArtistCreditInput {
                        name: format!("Tile Artist {n:03}"),
                        role: None,
                    }],
                    tracks: vec![CatalogTrackInput {
                        title: format!("Song {n}"),
                        artists: vec![],
                        disc_number: Some(1),
                        track_number: Some(1),
                    }],
                })
                .unwrap();
            library.add_to_library(&release.track_ids[0]).unwrap();
        }
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "function requestArtwork(key) {",
                "function requestArtwork(key) { layoutArtworkRequests++; return;",
                1,
            )
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n    function ready() {{",
                    include_str!("../AlbumLayoutTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        assert_eq!(
            engine
                .invoke_method("exerciseAlbumLayout".into(), &[])
                .to_qstring()
                .to_string(),
            "ok"
        );
        assert!(bridge.pinned().borrow().matcher.is_none());
    }

    #[test]
    #[ignore = "requires disposable copy of deterministic 200k library"]
    fn album_layout_200k_bounded_resize_and_scroll() {
        let path = std::env::var_os("MUSIC_LIBRARY_ALBUM_LAYOUT_STRESS_COPY")
            .expect("disposable 200k database copy");
        let mut library = music_library::Library::open(&path).unwrap();
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute(
            "DELETE FROM playlist WHERE name='Large selection fixture'",
            [],
        )
        .unwrap();
        let playlist = library.create_playlist("Large selection fixture").unwrap();
        db.execute("INSERT INTO playlist_entry(id,playlist_id,track_id,position) SELECT 'large-selection-'||id,?1,id,row_number() OVER(ORDER BY id)-1 FROM track",[&playlist]).unwrap();
        drop(db);
        let count: i64 = rusqlite::Connection::open(&path)
            .unwrap()
            .query_row("SELECT count(*) FROM library_membership", [], |r| r.get(0))
            .unwrap();
        assert!(count >= 200_000);
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "function requestArtwork(key) {",
                "function requestArtwork(key) { layoutArtworkRequests++; return;",
                1,
            )
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n    function ready() {{",
                    include_str!("../AlbumLayoutTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        let result = engine
            .invoke_method("exerciseAlbumLayoutStress".into(), &[])
            .to_qstring()
            .to_string();
        eprintln!("{result}");
        assert!(result.starts_with("ok:"), "{result}");
    }

    #[test]
    #[ignore = "requires a disposable real library copy with readable local sources"]
    fn real_library_views_local_genre_audit() {
        let path = std::env::var("MUSIC_LIBRARY_VIEWS_AUDIT_COPY").expect("disposable copy");
        let mut library = music_library::Library::open(path).unwrap();
        for root in library.local_locations().unwrap() {
            library
                .scan_local_root(
                    &root.id,
                    &mut music_library::filesystem::LoftyMetadataExtractor,
                )
                .unwrap();
        }
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n    function ready() {{",
                    include_str!("../LibraryViewsTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        let result = engine
            .invoke_method("exerciseRealLibraryViews".into(), &[])
            .to_qstring()
            .to_string();
        eprintln!("{result}");
        assert!(result.starts_with("ok:"), "{result}");
    }

    #[test]
    fn spotify_connections_review_updates_without_losing_context() {
        use music_library::domain::{
            ArtistCreditInput, CatalogReleaseInput, CatalogTrackInput, ExternalIdentity,
        };
        let temp = tempfile::tempdir().unwrap();
        let mut library = music_library::Library::open(temp.path().join("review.sqlite")).unwrap();
        let artist = ArtistCreditInput {
            name: "Review Artist".into(),
            role: None,
        };
        let imported = library
            .create_catalog_release(&CatalogReleaseInput {
                title: "Review Album".into(),
                year: None,
                artists: vec![artist.clone()],
                tracks: (0..450)
                    .map(|n| CatalogTrackInput {
                        title: format!("Song {n:03}"),
                        disc_number: Some(1),
                        track_number: Some(n + 1),
                        artists: vec![artist.clone()],
                    })
                    .collect(),
            })
            .unwrap();
        for t in &imported.track_ids {
            library.add_to_library(t).unwrap();
        }
        let album = library.album_id_for_track(&imported.track_ids[0]).unwrap();
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let (worker, requests) = spotify_resolution::Worker::fake();
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.auto_match = false;
            b.spotify_resolution_worker = Some(worker);
            b.session.playback.enqueue(imported.track_ids[449].clone());
            b.session.playback.select_queue_position(0).unwrap();
        }
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n    function ready() {{",
                    include_str!("../SpotifyConnectionsTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        assert_eq!(
            engine
                .invoke_method("reviewOpen".into(), &[])
                .to_qstring()
                .to_string(),
            "ok"
        );
        assert!(
            requests.try_recv().is_err(),
            "page, sort and inspection must not search"
        );
        assert_eq!(
            engine
                .invoke_method(
                    "reviewLifecycle".into(),
                    &[string(imported.track_ids[0].as_ref())]
                )
                .to_qstring()
                .to_string(),
            "ok"
        );
        assert_eq!(
            bridge
                .pinned()
                .borrow()
                .session
                .library
                .marked_spotify_count()
                .unwrap(),
            0
        );
        assert!(
            requests.try_recv().is_err(),
            "manual negative knowledge performs no search"
        );
        assert!(engine.invoke_method("reviewIdle".into(), &[]).to_bool());
        engine.invoke_method("reviewSnapshot".into(), &[]);
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            let track = &imported.track_ids[3];
            let input = b.session.library.song_resolution_input(track).unwrap();
            let candidate = music_library::song_resolution::Candidate {
                identity: ExternalIdentity {
                    provider: "spotify".into(),
                    kind: "track".into(),
                    external_id: "automatic-local-cache".into(),
                },
                album_identity: None,
                title: input.title.clone(),
                artist: input.artist.clone(),
                artists: input.artists.clone(),
                album: input.album.clone(),
                album_artists: input.album_artists.clone(),
                album_type: String::new(),
                album_total_tracks: Some(450),
                date: String::new(),
                duration_ms: 200000,
                disc: 1,
                number: input.number.unwrap(),
            };
            b.session
                .library
                .persist_spotify_song_review(
                    &input,
                    &music_library::catalog::Page {
                        items: vec![candidate],
                        next_offset: None,
                    },
                )
                .unwrap();
            let db = rusqlite::Connection::open(temp.path().join("review.sqlite")).unwrap();
            db.execute("UPDATE spotify_connection_review SET evaluation_version=0,stale=0 WHERE track_id=?1",[track.as_ref()]).unwrap();
            b.refresh_spotify_review();
        }
        assert_eq!(
            engine
                .invoke_method(
                    "reviewAfter".into(),
                    &[string(imported.track_ids[3].as_ref()), 449.into()]
                )
                .to_qstring()
                .to_string(),
            "ok"
        );
        assert!(
            requests.try_recv().is_err(),
            "automatic local replay must not search Spotify"
        );
        engine.invoke_method("reviewSnapshot".into(), &[]);
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            let input = b
                .session
                .library
                .song_resolution_input(&imported.track_ids[0])
                .unwrap();
            let candidate = music_library::song_resolution::Candidate {
                album_identity: None,
                identity: ExternalIdentity {
                    provider: "spotify".into(),
                    kind: "track".into(),
                    external_id: "1234567890123456789012".into(),
                },
                title: input.title.clone(),
                artist: input.artist.clone(),
                artists: vec![],
                album: input.album.clone(),
                album_artists: vec![],
                album_type: String::new(),
                album_total_tracks: Some(450),
                date: String::new(),
                duration_ms: 200000,
                disc: 1,
                number: 1,
            };
            let mut selection =
                music_library::song_resolution::Selection::new(input, vec![candidate]);
            selection.show_all();
            b.spotify_resolution_selection = Some(selection);
            b.spotify_resolve("confirm".into(), 0);
        }
        assert_eq!(
            engine
                .invoke_method(
                    "reviewAfter".into(),
                    &[string(imported.track_ids[0].as_ref()), 448.into()]
                )
                .to_qstring()
                .to_string(),
            "ok"
        );
        assert!(
            engine
                .invoke_method("reviewFilterAlbum".into(), &[string(album.as_ref())])
                .to_bool()
        );
        engine.invoke_method("reviewSnapshot".into(), &[]);
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            for (n, t) in imported.track_ids[1..3].iter().enumerate() {
                let input = b.session.library.song_resolution_input(t).unwrap();
                let c = music_library::song_resolution::Candidate {
                    identity: ExternalIdentity {
                        provider: "spotify".into(),
                        kind: "track".into(),
                        external_id: format!("album-occurrence-{n}"),
                    },
                    album_identity: None,
                    title: input.title.clone(),
                    artist: input.artist.clone(),
                    artists: input.artists.clone(),
                    album: input.album.clone(),
                    album_artists: input.album_artists.clone(),
                    album_type: String::new(),
                    album_total_tracks: Some(450),
                    date: String::new(),
                    duration_ms: 200000,
                    disc: 1,
                    number: input.number.unwrap(),
                };
                let generation = b.spotify_resolution_generation;
                b.finish_song_lookup(spotify_resolution::Reply {
                    generation,
                    input,
                    result: Ok(music_library::catalog::Page {
                        items: vec![c],
                        next_offset: None,
                    }),
                    counts: (0, 0),
                });
                assert!(b.spotify_resolution_message.contains("saved"));
                assert_eq!(b.session.playback.state().position, Some(0));
            }
            b.refresh_spotify_review();
        }
        assert_eq!(
            engine
                .invoke_method(
                    "reviewAfter".into(),
                    &[
                        string(format!(
                            "{} {}",
                            imported.track_ids[1].as_ref(),
                            imported.track_ids[2].as_ref()
                        )),
                        446.into()
                    ]
                )
                .to_qstring()
                .to_string(),
            "ok"
        );
        {
            let pinned = bridge.pinned();
            assert_eq!(
                engine
                    .invoke_method("reviewBulkMark".into(), &[])
                    .to_qstring()
                    .to_string(),
                "ok"
            );
            let mut b = pinned.borrow_mut();
            assert_eq!(b.session.library.marked_spotify_count().unwrap(), 4);
            for (n, t) in imported.track_ids[4..].iter().enumerate() {
                b.session
                    .library
                    .attach_track_external_identity(
                        t,
                        &ExternalIdentity {
                            provider: "spotify".into(),
                            kind: "track".into(),
                            external_id: format!("remaining-{n}"),
                        },
                    )
                    .unwrap();
            }
            b.refresh_spotify_review();
            assert_eq!(b.session.library.unresolved_spotify_count().unwrap(), 0);
            assert_eq!(
                b.session
                    .library
                    .library_queue(&Default::default())
                    .unwrap()
                    .len(),
                450
            );
        }
        assert!(engine.invoke_method("reviewEmpty".into(), &[]).to_bool());
        assert!(requests.try_recv().is_err());
    }

    fn identity_engine(bridge: &QObjectBox<Bridge>) -> QmlEngine {
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n{}\n    function ready() {{",
                    include_str!("../SpotifyConnectionsTest.qml"),
                    include_str!("../SpotifyIdentityTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        engine
    }
    #[test]
    fn artist_equivalence_comparison_and_explicit_confirmation_ui() {
        use music_library::{
            domain::*,
            edition::ArtistEvidence,
            song_resolution::{Candidate, Selection},
        };
        let mut library = music_library::Library::open_in_memory().unwrap();
        let r = library
            .create_catalog_release(&CatalogReleaseInput {
                title: "Album".into(),
                year: Some(2009),
                artists: vec![ArtistCreditInput {
                    name: "Old name".into(),
                    role: None,
                }],
                tracks: vec![CatalogTrackInput {
                    title: "Song".into(),
                    disc_number: Some(1),
                    track_number: Some(1),
                    artists: vec![],
                }],
            })
            .unwrap();
        let track = r.track_ids[0].clone();
        library.add_to_library(&track).unwrap();
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        bridge.pinned().borrow_mut().auto_match = false;
        let mut engine = identity_engine(&bridge);
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.spotify_playback_action("track".into(), track.as_ref().into());
            let artist = ArtistEvidence {
                name: "New name".into(),
                identities: vec![ExternalIdentity {
                    provider: "spotify".into(),
                    kind: "artist".into(),
                    external_id: "new-artist".into(),
                }],
                join_phrase: String::new(),
            };
            let c = Candidate {
                album_identity: None,
                identity: ExternalIdentity {
                    provider: "spotify".into(),
                    kind: "track".into(),
                    external_id: "new-track".into(),
                },
                title: "Song".into(),
                artist: artist.name.clone(),
                artists: vec![artist.clone()],
                album: "Album".into(),
                date: "2009".into(),
                album_artists: vec![artist],
                album_type: "album".into(),
                album_total_tracks: Some(1),
                duration_ms: 200000,
                disc: 1,
                number: 1,
            };
            let mut selection = Selection::new(
                b.session.library.song_resolution_input(&track).unwrap(),
                vec![c],
            );
            selection.show_all();
            b.spotify_resolution_selection = Some(selection);
            b.spotify_playback_changed();
        }
        assert_eq!(
            engine
                .invoke_method("identityInspect".into(), &[])
                .to_qstring()
                .to_string(),
            "ok"
        );
        {
            let pinned = bridge.pinned();
            let b = pinned.borrow();
            assert!(b.spotify_artist_proposal.is_none());
            assert_eq!(
                b.session
                    .library
                    .song_resolution_input(&track)
                    .unwrap()
                    .artists[0]
                    .identities
                    .len(),
                0
            );
        }
        assert_eq!(
            engine
                .invoke_method("identityConfirm".into(), &[])
                .to_qstring()
                .to_string(),
            "ok"
        );
        let pinned = bridge.pinned();
        let b = pinned.borrow();
        let input = b.session.library.song_resolution_input(&track).unwrap();
        assert_eq!(input.artist, "Old name");
        assert!(
            input.artists[0]
                .identities
                .iter()
                .any(|id| id.external_id == "new-artist")
        );
        assert!(
            b.session
                .library
                .list_track_external_identities(&track)
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    #[ignore = "explicit real Library Artist equivalence confirmation and bounded Album re-evaluation"]
    fn live_tsosis_artist_equivalence() {
        use music_library::domain::TrackId;
        let path = std::env::var("MUSIC_LIBRARY_DIAGNOSTIC_DATABASE").unwrap();
        let library = music_library::Library::open(&path).unwrap();
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        bridge.pinned().borrow_mut().auto_match = false;
        let mut engine = identity_engine(&bridge);
        let mut total = 0;
        for (n, id) in [
            "4700f9fa-c4d8-4a72-b9de-0bf40dd99140",
            "a3bce69a-4b77-48b7-a1d1-cbd698aaad3c",
        ]
        .iter()
        .enumerate()
        {
            let track = TrackId((*id).into());
            let album = bridge
                .pinned()
                .borrow()
                .session
                .library
                .album_id_for_track(&track)
                .unwrap();
            let before = bridge
                .pinned()
                .borrow()
                .session
                .library
                .unresolved_spotify_count()
                .unwrap();
            let members = bridge
                .pinned()
                .borrow()
                .session
                .library
                .library_queue(&Default::default())
                .unwrap()
                .len();
            assert_eq!(
                engine
                    .invoke_method(
                        "reviewLiveOpen".into(),
                        &[string(track.as_ref()), string(album.as_ref())]
                    )
                    .to_qstring()
                    .to_string(),
                "ok"
            );
            if n == 0 {
                assert!(
                    engine
                        .invoke_method("identityLiveSearch".into(), &[string("tsosis")])
                        .to_bool()
                );
                {
                    let pinned = bridge.pinned();
                    let b = pinned.borrow();
                    println!("TSOSIS BEFORE {}", b.spotify_comparison_json());
                    assert_eq!(
                        b.spotify_resolution_selection
                            .as_ref()
                            .unwrap()
                            .candidates()
                            .len(),
                        1
                    );
                }
                assert_eq!(
                    engine
                        .invoke_method("identityInspect".into(), &[])
                        .to_qstring()
                        .to_string(),
                    "ok"
                );
                assert_eq!(
                    engine
                        .invoke_method("identityConfirm".into(), &[])
                        .to_qstring()
                        .to_string(),
                    "ok"
                );
            } else {
                bridge.pinned().borrow_mut().spotify_album_retry();
            }
            assert!(
                engine
                    .invoke_method("identityWaitAlbum".into(), &[])
                    .to_bool()
            );
            assert_eq!(
                engine
                    .invoke_method("identityProgramDetails".into(), &[])
                    .to_qstring()
                    .to_string(),
                "ok"
            );
            let (after, associated) = {
                let pinned = bridge.pinned();
                let b = pinned.borrow();
                let after = b.session.library.unresolved_spotify_count().unwrap();
                let ids = b
                    .session
                    .library
                    .track_provider_occurrences(&track, "spotify")
                    .unwrap();
                println!(
                    "TSOSIS AFTER Album {} count {} -> {} associations {:?}\n{}",
                    album.as_ref(),
                    before,
                    after,
                    ids,
                    b.spotify_album_explanation
                );
                assert_eq!(
                    b.session
                        .library
                        .library_queue(&Default::default())
                        .unwrap()
                        .len(),
                    members
                );
                (after, !ids.is_empty())
            };
            assert_eq!(
                engine
                    .invoke_method(
                        "reviewLiveState".into(),
                        &[string(track.as_ref()), associated.into()]
                    )
                    .to_qstring()
                    .to_string(),
                "ok"
            );
            total += before - after;
        }
        println!("TSOSIS total safely associated Tracks: {total}");
    }
    #[test]
    #[ignore = "200k fixture Spotify Connections bounded QML scrolling"]
    fn spotify_connections_200k_bounded_review() {
        let path = std::env::var("MUSIC_LIBRARY_DIAGNOSTIC_DATABASE").unwrap();
        let library = music_library::Library::open(path).unwrap();
        assert!(library.unresolved_spotify_count().unwrap() >= 200_000);
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let (worker, requests) = spotify_resolution::Worker::fake();
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.auto_match = false;
            b.spotify_resolution_worker = Some(worker);
        }
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n    function ready() {{",
                    include_str!("../SpotifyConnectionsTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        assert_eq!(
            engine
                .invoke_method("reviewOpen".into(), &[])
                .to_qstring()
                .to_string(),
            "ok"
        );
        assert_eq!(
            engine
                .invoke_method("reviewLarge".into(), &[])
                .to_qstring()
                .to_string(),
            "ok"
        );
        assert!(requests.try_recv().is_err());
    }

    #[test]
    #[ignore = "explicit real Library Spotify Connections validation, including a safe manual connection"]
    fn live_spotify_connections_review() {
        use music_library::domain::{AlbumId, TrackId};
        let path = std::env::var("MUSIC_LIBRARY_DIAGNOSTIC_DATABASE").unwrap();
        let library = music_library::Library::open(&path).unwrap();
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        bridge.pinned().borrow_mut().auto_match = false;
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n    function ready() {{",
                    include_str!("../SpotifyConnectionsTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        let album = AlbumId("8a4b0b05-8989-4c24-b5f2-0bd85100585a".into());
        let track = bridge
            .pinned()
            .borrow()
            .session
            .library
            .local_album_tracks(&album)
            .unwrap()[0]
            .track_id
            .clone();
        let already = bridge
            .pinned()
            .borrow()
            .session
            .library
            .track_provider_occurrences(&track, "spotify")
            .unwrap();
        if already.is_empty() {
            assert_eq!(
                engine
                    .invoke_method(
                        "reviewLiveOpen".into(),
                        &[string(track.as_ref()), string(album.as_ref())]
                    )
                    .to_qstring()
                    .to_string(),
                "ok"
            );
            let before = bridge
                .pinned()
                .borrow()
                .session
                .library
                .unresolved_spotify_count()
                .unwrap();
            assert!(
                engine
                    .invoke_method("reviewLiveSearch".into(), &[])
                    .to_bool()
            );
            let index = {
                let pinned = bridge.pinned();
                let b = pinned.borrow();
                let selection = b.spotify_resolution_selection.as_ref().unwrap_or_else(|| {
                    panic!("bounded search failed: {}", b.spotify_resolution_message)
                });
                let music_library::song_resolution::Assessment::Unique(index) =
                    music_library::song_resolution::assess(
                        selection.input(),
                        &music_library::catalog::Page {
                            items: selection.candidates().to_vec(),
                            next_offset: None,
                        },
                    )
                else {
                    panic!(
                        "safe manual example has no unique supported candidate: {:?}",
                        selection.candidates()
                    );
                };
                eprintln!("Manual real candidate: {:?}", selection.candidates()[index]);
                selection
                    .visible_indices()
                    .iter()
                    .position(|n| *n == index)
                    .unwrap()
            };
            assert_eq!(
                engine
                    .invoke_method(
                        "reviewLiveConfirm".into(),
                        &[(index as i32).into(), string(track.as_ref())]
                    )
                    .to_qstring()
                    .to_string(),
                "ok"
            );
            assert_eq!(
                bridge
                    .pinned()
                    .borrow()
                    .session
                    .library
                    .unresolved_spotify_count()
                    .unwrap(),
                before - 1
            );
            assert!(
                bridge
                    .pinned()
                    .borrow()
                    .session
                    .library
                    .library_queue(&Default::default())
                    .unwrap()
                    .iter()
                    .any(|r| r.track_id == track)
            );
            eprintln!(
                "Manual real Track {} count {} -> {}",
                track.as_ref(),
                before,
                before - 1
            );
        }
        for (album, connect, reason) in [
            (
                "169f997a-d191-4ccd-bb27-9e1d95371fa8",
                false,
                "Candidate Artist IDs differ",
            ),
            (
                "7ab7c3bf-fbee-4bd9-92e9-fe1dd802424e",
                false,
                "Meaningful Album/version qualifier differs",
            ),
            (
                "d335bff0-b96d-4dbf-9022-dc01734f0487",
                true,
                "release-type suffix normalized",
            ),
            (
                "8a4b0b05-8989-4c24-b5f2-0bd85100585a",
                true,
                "exact Album title",
            ),
            (
                "231a1019-5b1c-4234-87af-7de602da1a3c",
                true,
                "release-type suffix normalized",
            ),
        ] {
            let album = AlbumId(album.into());
            let rows = bridge
                .pinned()
                .borrow()
                .session
                .library
                .browse(&music_library::browse::Request {
                    unresolved_spotify: true,
                    album: Some(album.clone()),
                    limit: 201,
                    ..Default::default()
                })
                .unwrap();
            if connect && rows.is_empty() {
                eprintln!("Already resolved {}", album.as_ref());
                continue;
            }
            let selected = if album.as_ref() == "169f997a-d191-4ccd-bb27-9e1d95371fa8" {
                rows.iter()
                    .find(|r| r.title == "Organ Song")
                    .expect("Organ Song remains unresolved")
            } else {
                &rows[0]
            };
            let track = TrackId(selected.id.clone());
            assert_eq!(
                engine
                    .invoke_method(
                        "reviewLiveOpen".into(),
                        &[string(track.as_ref()), string(album.as_ref())]
                    )
                    .to_qstring()
                    .to_string(),
                "ok"
            );
            assert_eq!(
                engine
                    .invoke_method(
                        "reviewLiveRetry".into(),
                        &[string(track.as_ref()), connect.into(), string(reason)]
                    )
                    .to_qstring()
                    .to_string(),
                "ok"
            );
            let remaining = bridge
                .pinned()
                .borrow()
                .session
                .library
                .browse(&music_library::browse::Request {
                    unresolved_spotify: true,
                    album: Some(album.clone()),
                    limit: 201,
                    ..Default::default()
                })
                .unwrap();
            if connect {
                assert!(remaining.is_empty(), "all fixed Album Tracks connected");
            }
            eprintln!(
                "Real Album {} remaining {} reasons {:?}",
                album.as_ref(),
                remaining.len(),
                remaining
                    .iter()
                    .map(|r| (&r.title, &r.connection_reason))
                    .collect::<Vec<_>>()
            );
        }
        bridge.pinned().borrow_mut().spotify_album_matcher.take();
    }

    #[test]
    fn song_connection_uses_reconciled_track_and_supports_spotify_only_entries() {
        use music_library::domain::{CatalogReleaseInput, CatalogTrackInput, ExternalIdentity};
        let (temp, mut library) = sample::create().unwrap();
        let canonical = library
            .search(&music_library::domain::SearchRequest {
                limit: 200,
                ..Default::default()
            })
            .unwrap()
            .into_iter()
            .find(|row| row.title == "02 Available")
            .unwrap()
            .track_id;
        let staging = library
            .create_catalog_release(&CatalogReleaseInput {
                title: "Spotify edition".into(),
                year: None,
                artists: vec![],
                tracks: vec![CatalogTrackInput {
                    title: "Spotify-only Song".into(),
                    disc_number: Some(1),
                    track_number: Some(1),
                    artists: vec![],
                }],
            })
            .unwrap()
            .track_ids
            .remove(0);
        library
            .attach_track_external_identity(
                &staging,
                &ExternalIdentity {
                    provider: "spotify".into(),
                    kind: "track".into(),
                    external_id: "1234567890123456789012".into(),
                },
            )
            .unwrap();
        let playlist = library.create_playlist("Reconciled diagnostic").unwrap();
        let entry = library.append_playlist_track(&playlist, &staging).unwrap();
        library.append_playlist_track(&playlist, &staging).unwrap();
        // Model the persisted outcome of reconciliation, leaving staging alive.
        rusqlite::Connection::open(temp.path().join("diagnostic.sqlite"))
            .unwrap()
            .execute(
                "UPDATE playlist_entry SET track_id=?1 WHERE id=?2",
                rusqlite::params![canonical.as_ref(), entry],
            )
            .unwrap();
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml").replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen("    function ready() {", r#"
    TestCase { id: connectionTest; when: false }
    function checkPlaylistConnections(playlist, canonical, staging) {
        function check(value, message) { if (!value) throw new Error(message); }
        function wait() { connectionTest.wait(30); }
        try {
            bridge.browse_action("view", 0, "Playlists");
            bridge.browse_action("select", 0, playlist);
            for (let n = 0; n < 200 && library.pending; ++n) wait();
            wait();
            const list = connectionTest.findChild(songsPane, "libraryPane2"); list.forceLayout();
            check(library.panes[2].rows[0].track.trackId === canonical, "persisted canonical relationship");
            check(library.panes[2].rows[1].track.trackId === staging, "Spotify-only relationship");
            const before = JSON.stringify([view.currentId, view.queue, view.status]);
            for (let i = 0; i < 2; ++i) {
                connectionTest.mouseClick(list.itemAtIndex(i), 20, 20, Qt.RightButton); wait();
                const action = connectionTest.findChild(libraryMenu, "songSpotifyConnection");
                check(action && action.visible && action.enabled, "Spotify-only and unconnected menus available");
                action.triggered(); libraryMenu.close(); wait();
                check(spotifyPlayback.trackId === (i === 0 ? canonical : staging), "canonical Track, never obsolete staging");
                check(spotifyPlayback.available === (i === 1), "selected persisted association state");
                if (i === 1) check(spotifyPlayback.songUri === "spotify:track:1234567890123456789012", "existing association shown");
                else check(connectionTest.findChild(spotifyPlaybackDialog, "spotifyConnectionSearch").enabled, "unconnected canonical search available");
                check(JSON.stringify([view.currentId, view.queue, view.status]) === before, "inspection does not start playback");
                spotifyPlaybackDialog.close(); wait();
            }
            return "ok";
        } catch (e) { return String(e); }
    }
    function ready() {
"#, 1);
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        assert_eq!(
            engine
                .invoke_method(
                    "checkPlaylistConnections".into(),
                    &[
                        string(playlist),
                        string(canonical.as_ref()),
                        string(staging.as_ref())
                    ]
                )
                .to_qstring()
                .to_string(),
            "ok"
        );
    }

    #[test]
    fn song_album_headers_identity_paging_and_interactions() {
        use music_library::domain::{ArtistCreditInput, CatalogReleaseInput, CatalogTrackInput};
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("headers.db");
        let mut library = music_library::Library::open(&path).unwrap();
        for _ in 0..2 {
            let artist = ArtistCreditInput {
                name: "Group Artist".into(),
                role: None,
            };
            let release = library
                .create_catalog_release(&CatalogReleaseInput {
                    title: "Same album name".into(),
                    year: None,
                    artists: vec![artist.clone()],
                    tracks: (0..201)
                        .map(|n| CatalogTrackInput {
                            title: format!("Song {n:03}"),
                            disc_number: Some(if n < 100 { 1 } else { 2 }),
                            track_number: Some(if n < 100 { n + 1 } else { n - 99 }),
                            artists: vec![artist.clone()],
                        })
                        .collect(),
                })
                .unwrap();
            for t in release.track_ids {
                library.add_to_library(&t).unwrap();
            }
        }
        let db = rusqlite::Connection::open(&path).unwrap();
        for table in [
            "track_artist_credit",
            "album_artist_credit",
            "release_artist_credit",
        ] {
            db.execute_batch(&format!("UPDATE {table} SET artist_id=(SELECT min(id) FROM artist WHERE name='Group Artist')")).unwrap();
        }
        drop(db);
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n    function ready() {{",
                    include_str!("../SongAlbumHeadersTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        assert_eq!(
            engine
                .invoke_method("exerciseSongAlbumHeaders".into(), &[])
                .to_qstring()
                .to_string(),
            "ok"
        );
        bridge.pinned().borrow_mut().session.shutdown_audio();
    }

    fn run_main_browsing_scrolling(library: music_library::Library) {
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n{}\n    function ready() {{",
                    include_str!("../SongColumnResizeTest.qml"),
                    include_str!("../MainBrowsingScrollingTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        assert_eq!(
            engine
                .invoke_method("exerciseMainBrowsingScrolling".into(), &[])
                .to_qstring()
                .to_string(),
            "ok"
        );
    }

    #[test]
    fn artists_genres_and_album_tiles_scroll_continuously() {
        use music_library::domain::{ArtistCreditInput, CatalogReleaseInput, CatalogTrackInput};
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("panes.db");
        let mut library = music_library::Library::open(&path).unwrap();
        let db = rusqlite::Connection::open(&path).unwrap();
        for n in 0..803 {
            let release = library
                .create_catalog_release(&CatalogReleaseInput {
                    title: format!("Album {n:04}"),
                    year: Some(2000 + n % 5),
                    artists: vec![ArtistCreditInput {
                        name: format!("Artist {n:04}"),
                        role: None,
                    }],
                    tracks: vec![CatalogTrackInput {
                        title: format!("Song {n:04}"),
                        // Exercise the track-artist sort rather than a Release-credit display fallback.
                        artists: vec![ArtistCreditInput {
                            name: format!("Artist {n:04}"),
                            role: None,
                        }],
                        disc_number: None,
                        track_number: None,
                    }],
                })
                .unwrap();
            let t = &release.track_ids[0];
            library.add_to_library(t).unwrap();
            db.execute(
                "INSERT INTO playable_source(id,kind) VALUES(?1,'local_file')",
                [t.as_ref()],
            )
            .unwrap();
            db.execute(
                "INSERT INTO track_source(track_id,source_id) VALUES(?1,?1)",
                [t.as_ref()],
            )
            .unwrap();
            db.execute(
                "INSERT INTO file_genre_observation(source_id,genre) VALUES(?1,?2)",
                rusqlite::params![t.as_ref(), format!("Genre {n:04}")],
            )
            .unwrap();
        }
        drop(db);
        run_main_browsing_scrolling(library);
    }

    #[test]
    #[ignore = "requires disposable deterministic 200k library copy"]
    fn continuous_scrolling_200k_bounded_qml_render() {
        let path = std::env::var_os("MUSIC_LIBRARY_CONTINUOUS_STRESS_COPY")
            .expect("disposable 200k database copy");
        let count: i64 = rusqlite::Connection::open(&path)
            .unwrap()
            .query_row("SELECT count(*) FROM library_membership", [], |r| r.get(0))
            .unwrap();
        assert!(count >= 200_000);
        run_main_browsing_scrolling(music_library::Library::open(path).unwrap());
    }

    #[test]
    #[ignore = "requires disposable deterministic 200k library copy"]
    fn playlist_table_200k_bounded_sorting() {
        use music_library::domain::{CatalogReleaseInput, CatalogTrackInput};
        let path = std::env::var_os("MUSIC_LIBRARY_PLAYLIST_TABLE_STRESS_COPY")
            .expect("disposable 200k copy");
        let mut library = music_library::Library::open(&path).unwrap();
        let playlist = library.create_playlist("! Table stress").unwrap();
        let db = rusqlite::Connection::open(&path).unwrap();
        let members: i64 = db
            .query_row("SELECT COUNT(*) FROM library_membership", [], |r| r.get(0))
            .unwrap();
        assert_eq!(members, 200_000);
        db.execute("INSERT INTO playlist_entry(id,playlist_id,track_id,position) SELECT ?1||'-'||p.track_id,?1,p.track_id,row_number() OVER(ORDER BY p.title COLLATE NOCASE DESC,p.track_id)-1 FROM effective_track_metadata p",[&playlist]).unwrap();
        let catalog = library
            .create_catalog_release(&CatalogReleaseInput {
                title: "Catalog only".into(),
                year: None,
                artists: vec![],
                tracks: vec![CatalogTrackInput {
                    title: "Catalog only track".into(),
                    artists: vec![],
                    disc_number: None,
                    track_number: None,
                }],
            })
            .unwrap();
        library
            .append_playlist_track(&playlist, &catalog.track_ids[0])
            .unwrap();
        library
            .append_playlist_track(&playlist, &catalog.track_ids[0])
            .unwrap();
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n{}\n    function ready() {{",
                    include_str!("../SongColumnResizeTest.qml"),
                    include_str!("../PlaylistTableStressTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        assert_eq!(
            engine
                .invoke_method("exercisePlaylistTableStress".into(), &[])
                .to_qstring()
                .to_string(),
            "ok"
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM library_membership", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            members
        );
        let canonical = bridge
            .pinned()
            .borrow()
            .session
            .library
            .playlist_entries(&playlist, None, 1)
            .unwrap();
        assert_eq!(canonical[0].playlist_position, Some(1));
        bridge
            .pinned()
            .borrow_mut()
            .session
            .library
            .delete_playlist(&playlist)
            .unwrap();
        #[cfg(feature = "gstreamer")]
        bridge.pinned().borrow_mut().session.shutdown_audio();
    }

    #[test]
    fn continuous_scrolling_bounded_windows_selection_and_playlists() {
        use music_library::domain::{ArtistCreditInput, CatalogReleaseInput, CatalogTrackInput};
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("scrolling.db");
        let mut library = music_library::Library::open(&path).unwrap();
        let artist = ArtistCreditInput {
            name: "Scrolling Artist".into(),
            role: None,
        };
        let mut tracks = vec![];
        for (start, end) in [(0, 701), (701, 1003)] {
            let release = library
                .create_catalog_release(&CatalogReleaseInput {
                    title: "Same title".into(),
                    year: None,
                    artists: vec![artist.clone()],
                    tracks: (start..end)
                        .map(|n| CatalogTrackInput {
                            title: format!("Song {n:04}"),
                            disc_number: Some(1),
                            track_number: Some(n - start + 1),
                            artists: vec![artist.clone()],
                        })
                        .collect(),
                })
                .unwrap();
            for t in &release.track_ids {
                library.add_to_library(t).unwrap();
            }
            tracks.extend(release.track_ids);
        }
        let db = rusqlite::Connection::open(&path).unwrap();
        for table in [
            "track_artist_credit",
            "album_artist_credit",
            "release_artist_credit",
        ] {
            db.execute_batch(&format!("UPDATE {table} SET artist_id=(SELECT min(id) FROM artist WHERE name='Scrolling Artist')")).unwrap();
        }
        drop(db);
        for n in 0..205 {
            let playlist = library
                .create_playlist(&format!("Playlist {n:03}"))
                .unwrap();
            if n == 2 {
                for track in tracks.iter().rev() {
                    library.append_playlist_track(&playlist, track).unwrap();
                }
                library
                    .append_playlist_track(&playlist, &tracks[500])
                    .unwrap();
                let db = rusqlite::Connection::open(&path).unwrap();
                db.execute("UPDATE effective_track_metadata SET duration_ms=CASE WHEN CAST(substr(title,6) AS INTEGER)%17=0 THEN NULL ELSE CAST(substr(title,6) AS INTEGER)*1000 END",[]).unwrap();
            }
            if n == 0 {
                for _ in 0..1003 {
                    library
                        .append_playlist_track(&playlist, &tracks[0])
                        .unwrap();
                }
            }
        }
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n    function ready() {{",
                    include_str!("../ContinuousScrollingTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        assert_eq!(
            engine
                .invoke_method("exerciseContinuousScrolling".into(), &[])
                .to_qstring()
                .to_string(),
            "ok"
        );
        let removed = engine
            .invoke_method("cacheSongsBeforeRefresh".into(), &[])
            .to_qstring()
            .to_string();
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.session
                .library
                .remove_from_library(&music_library::domain::TrackId(removed.clone()))
                .unwrap();
            b.browse_action_impl("refresh", 0, String::new());
        }
        assert_eq!(
            engine
                .invoke_method("exerciseInactiveRefresh".into(), &[string(removed)])
                .to_qstring()
                .to_string(),
            "ok"
        );
        #[cfg(feature = "gstreamer")]
        bridge.pinned().borrow_mut().session.shutdown_audio();
    }

    #[test]
    fn multi_selection_and_pane_local_container_actions() {
        use music_library::domain::{ArtistCreditInput, CatalogReleaseInput, CatalogTrackInput};
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("library.sqlite");
        let mut library = music_library::Library::open(&path).unwrap();
        let mut releases = vec![];
        for letter in ["A", "B", "C"] {
            let artist = ArtistCreditInput {
                name: format!("Artist {letter}"),
                role: None,
            };
            let release = library
                .create_catalog_release(&CatalogReleaseInput {
                    title: format!("Album {letter}"),
                    year: None,
                    artists: vec![artist.clone()],
                    tracks: (0..4)
                        .map(|i| CatalogTrackInput {
                            title: format!("{letter} {i}"),
                            disc_number: Some(1),
                            track_number: Some(i + 1),
                            artists: vec![artist.clone()],
                        })
                        .collect(),
                })
                .unwrap();
            for t in &release.track_ids {
                library.add_to_library(t).unwrap();
            }
            releases.push(release);
        }
        let db = rusqlite::Connection::open(&path).unwrap();
        for table in [
            "track_artist_credit",
            "release_artist_credit",
            "album_artist_credit",
        ] {
            db.execute_batch(&format!("UPDATE {table} SET artist_id=(SELECT min(a.id) FROM artist a WHERE a.name=(SELECT name FROM artist WHERE id={table}.artist_id))")).unwrap();
        }
        for (i, r) in releases.iter().enumerate() {
            for t in &r.track_ids {
                db.execute(
                    "INSERT INTO playable_source(id,kind) VALUES(?1,'local_file')",
                    [t.as_ref()],
                )
                .unwrap();
                db.execute(
                    "INSERT INTO track_source(track_id,source_id) VALUES(?1,?1)",
                    [t.as_ref()],
                )
                .unwrap();
                db.execute(
                    "INSERT INTO file_genre_observation(source_id,genre) VALUES(?1,?2)",
                    rusqlite::params![t.as_ref(), ["Rock", "Pop", "Jazz"][i]],
                )
                .unwrap();
            }
        }
        drop(db);
        let one = library.create_playlist("Source One").unwrap();
        let two = library.create_playlist("Source Two").unwrap();
        for t in [
            &releases[0].track_ids[0],
            &releases[1].track_ids[0],
            &releases[0].track_ids[0],
        ] {
            library.append_playlist_track(&one, t).unwrap();
        }
        for t in [&releases[1].track_ids[1], &releases[0].track_ids[1]] {
            library.append_playlist_track(&two, t).unwrap();
        }
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n    function ready() {{",
                    include_str!("../MultiSelectionTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        assert_eq!(
            engine
                .invoke_method("exerciseMultiSelection".into(), &[])
                .to_qstring()
                .to_string(),
            "ok"
        );
        bridge.pinned().borrow_mut().session.shutdown_audio();
    }

    #[test]
    fn library_views_genres_year_sections_numbers_and_queue_preservation() {
        use music_library::domain::{CatalogReleaseInput, CatalogTrackInput};
        let (temp, mut library) = sample::create().unwrap();
        let extra = library
            .create_catalog_release(&CatalogReleaseInput {
                title: "Additional".into(),
                year: Some(2024),
                artists: vec![],
                tracks: [("Alpha", 1, 2), ("Zulu", 2, 1), ("Unsaved", 2, 2)]
                    .into_iter()
                    .map(|(title, disc, track)| CatalogTrackInput {
                        title: title.into(),
                        disc_number: Some(disc),
                        track_number: Some(track),
                        artists: vec![],
                    })
                    .collect(),
            })
            .unwrap();
        let unknown = library
            .create_catalog_release(&CatalogReleaseInput {
                title: "Unknown date".into(),
                year: None,
                artists: vec![],
                tracks: vec![CatalogTrackInput {
                    title: "Undated".into(),
                    disc_number: None,
                    track_number: None,
                    artists: vec![],
                }],
            })
            .unwrap();
        for id in extra.track_ids.iter().take(2).chain(&unknown.track_ids) {
            library.add_to_library(id).unwrap();
        }
        let db = rusqlite::Connection::open(temp.path().join("diagnostic.sqlite")).unwrap();
        db.execute(
            "UPDATE album_application_metadata SET year=2019 WHERE title='Diagnostic edition'",
            [],
        )
        .unwrap();
        db.execute_batch("INSERT INTO file_genre_observation SELECT id, 'Rock' FROM playable_source WHERE id LIKE 'diagnostic-source-01-%' OR id LIKE 'diagnostic-source-03-%'; INSERT INTO file_genre_observation SELECT id, 'Jazz' FROM playable_source WHERE id LIKE 'diagnostic-source-02-%';").unwrap();
        for (i, track) in extra.track_ids.iter().enumerate() {
            let source = format!("extra-{i}");
            db.execute(
                "INSERT INTO playable_source(id,kind) VALUES (?1,'local_file')",
                [&source],
            )
            .unwrap();
            db.execute(
                "INSERT INTO track_source(track_id,source_id) VALUES (?1,?2)",
                rusqlite::params![track.as_ref(), source],
            )
            .unwrap();
            db.execute(
                "INSERT INTO file_genre_observation(source_id,genre) VALUES (?1,?2)",
                rusqlite::params![source, if i == 1 { "Jazz" } else { "Rock" }],
            )
            .unwrap();
        }
        assert_eq!(
            db.query_row("SELECT count(*) FROM playlist", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        drop(db);
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n    function ready() {{",
                    include_str!("../LibraryViewsTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        assert_eq!(
            engine
                .invoke_method("exerciseLibraryViews".into(), &[])
                .to_qstring()
                .to_string(),
            "ok"
        );
    }

    #[test]
    fn song_details_columns_resize_without_model_or_playback_changes() {
        let (_temp, mut library) = sample::create().unwrap();
        let tracks = library
            .search(&music_library::domain::SearchRequest {
                limit: 200,
                ..Default::default()
            })
            .unwrap();
        let playlist = library.create_playlist("Columns").unwrap();
        for track in tracks.iter().take(5) {
            library
                .append_playlist_track(&playlist, &track.track_id)
                .unwrap();
        }
        library
            .append_playlist_track(&playlist, &tracks[0].track_id)
            .unwrap();
        let available = tracks.iter().find(|t| t.title == "02 Available").unwrap();
        let mut session = Session::new(library);
        session.play_row(available.track_id.as_ref());
        assert_eq!(
            session.playback.state().status,
            music_library::playback::PlaybackStatus::Playing
        );
        let bridge = QObjectBox::new(Bridge::new(session));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n    function ready() {{",
                    include_str!("../SongColumnResizeTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        assert_eq!(
            engine
                .invoke_method("exerciseSongColumnResize".into(), &[])
                .to_qstring()
                .to_string(),
            "ok"
        );
    }

    #[test]
    fn playlist_nonstructural_updates_preserve_model_delegates_viewport_and_selection() {
        let (temp, mut library) = sample::create().unwrap();
        let track = library
            .search(&music_library::domain::SearchRequest {
                limit: 200,
                ..Default::default()
            })
            .unwrap()
            .into_iter()
            .find(|t| t.title == "02 Available")
            .unwrap()
            .track_id;
        let playlist = library.create_playlist("Stability").unwrap();
        let db = rusqlite::Connection::open(temp.path().join("diagnostic.sqlite")).unwrap();
        db.execute("WITH RECURSIVE n(x) AS (SELECT 0 UNION ALL SELECT x+1 FROM n WHERE x<999) INSERT INTO playlist_entry(id,playlist_id,track_id,position) SELECT 'stability-'||x,?1,?2,x FROM n",rusqlite::params![playlist,track.as_ref()]).unwrap();
        let mut session = Session::new(library);
        session.play_row(track.as_ref());
        let bridge = QObjectBox::new(Bridge::new(session));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n    function ready() {{",
                    include_str!("../PlaylistStabilityTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        let check = |engine: &mut QmlEngine, name: &str| {
            assert_eq!(
                engine
                    .invoke_method(name.into(), &[])
                    .to_qstring()
                    .to_string(),
                "ok"
            )
        };
        check(&mut engine, "preparePlaylistStability");
        for phase in 0..6 {
            {
                let pin = bridge.pinned();
                let mut b = pin.borrow_mut();
                match phase {
                    0 => {
                        for _ in 0..20 {
                            b.changed();
                        }
                    }
                    1 => {
                        let snapshot = b.spotify_playback_state.clone();
                        b.apply_player_update(crate::spotify_playback::Update {
                            snapshot,
                            generation: 0,
                            volume_ack: None,
                            application_command: false,
                            completion: None,
                        });
                    }
                    2 => {
                        b.session.command("pause");
                        b.changed();
                    }
                    3 => {
                        b.player_seek(60000.);
                        b.changed();
                    }
                    4 => {
                        db.execute("UPDATE effective_track_metadata SET duration_ms=180000 WHERE track_id=?1",[track.as_ref()]).unwrap();
                        b.browse_action_impl("refresh", 0, String::new());
                    }
                    _ => {
                        db.execute("UPDATE effective_track_metadata SET title='Enriched title' WHERE track_id=?1",[track.as_ref()]).unwrap();
                        b.browse_action_impl("refresh", 0, String::new());
                    }
                }
            }
            check(&mut engine, "checkPlaylistStability");
        }
        check(&mut engine, "checkPlaylistEnrichmentDisplayed");
        // Repoint the selected stable entry twice: true absent credit, then a
        // canonical local-style Track using the Songs Album-credit fallback.
        {
            use music_library::domain::{
                ArtistCreditInput, CatalogReleaseInput, CatalogTrackInput,
            };
            let pin = bridge.pinned();
            let mut b = pin.borrow_mut();
            for (album, artists, artist, genre, duration) in [
                ("Uncredited", vec![], "Unknown artist", "", 180000),
                (
                    "Canonical Album",
                    vec![ArtistCreditInput {
                        name: "Hop Along".into(),
                        role: None,
                    }],
                    "Hop Along",
                    "Indie Rock",
                    123456,
                ),
            ] {
                let canonical = b
                    .session
                    .library
                    .create_catalog_release(&CatalogReleaseInput {
                        title: album.into(),
                        year: None,
                        artists,
                        tracks: vec![CatalogTrackInput {
                            title: "Enriched title".into(),
                            artists: vec![],
                            disc_number: Some(1),
                            track_number: Some(1),
                        }],
                    })
                    .unwrap();
                db.execute("UPDATE effective_track_metadata SET genre_names=?2,duration_ms=?3 WHERE track_id=?1",rusqlite::params![canonical.track_ids[0].as_ref(),genre,duration]).unwrap();
                db.execute(
                    "UPDATE playlist_entry SET track_id=?2 WHERE id='stability-80'",
                    rusqlite::params![playlist, canonical.track_ids[0].as_ref()],
                )
                .unwrap();
                b.browse_action_impl("playlist-revision-check", 0, String::new());
                // Drain the bounded worker through Qt after releasing the QObject borrow.
                drop(b);
                check(&mut engine, "checkPlaylistStability");
                assert_eq!(
                    engine
                        .invoke_method(
                            "checkPlaylistCanonicalMetadata".into(),
                            &[
                                string(artist),
                                string(album),
                                string(genre),
                                string(if duration == 123456 { "02:03" } else { "03:00" })
                            ]
                        )
                        .to_qstring()
                        .to_string(),
                    "ok"
                );
                b = pin.borrow_mut();
            }
        }
        db.execute("DELETE FROM playlist_entry WHERE id='stability-20'", [])
            .unwrap();
        bridge
            .pinned()
            .borrow_mut()
            .browse_action_impl("refresh", 0, String::new());
        check(&mut engine, "checkPlaylistStructuralUpdate");
    }

    #[test]
    fn hidden_selected_playlist_refreshes_after_context_append_remove_reorder_catalog_and_overwrite()
     {
        let (_temp, mut library) = sample::create().unwrap();
        let tracks = library
            .search(&music_library::domain::SearchRequest {
                limit: 200,
                ..Default::default()
            })
            .unwrap();
        let playlist = library.create_playlist("A revision").unwrap();
        let other = library.create_playlist("Unrelated").unwrap();
        for _ in 0..100 {
            library
                .append_playlist_track(&playlist, &tracks[0].track_id)
                .unwrap();
        }
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n    function ready() {{",
                    include_str!("../PlaylistRevisionTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        let call = |engine: &mut QmlEngine, name: &str, args: &[QVariant]| {
            assert_eq!(
                engine
                    .invoke_method(name.into(), args)
                    .to_qstring()
                    .to_string(),
                "ok",
                "{name}"
            )
        };
        call(&mut engine, "revisionOpen", &[string(&playlist)]);
        call(
            &mut engine,
            "revisionAddFromSongs",
            &[string(tracks[1].track_id.as_ref())],
        );
        call(&mut engine, "revisionReturn", &[101.into()]);
        call(&mut engine, "revisionNoReload", &[]);
        // A different playlist's revision must not invalidate the selected one.
        bridge
            .pinned()
            .borrow_mut()
            .session
            .library
            .append_playlist_track(&other, &tracks[0].track_id)
            .unwrap();
        call(&mut engine, "revisionNoReload", &[]);
        call(&mut engine, "revisionHide", &[]);
        let entries = bridge
            .pinned()
            .borrow()
            .session
            .library
            .playlist_entries(&playlist, None, 200)
            .unwrap();
        bridge
            .pinned()
            .borrow_mut()
            .session
            .library
            .remove_playlist_entry(&playlist, &entries[10].id)
            .unwrap();
        call(&mut engine, "revisionReturn", &[100.into()]);
        call(&mut engine, "revisionHide", &[]);
        bridge
            .pinned()
            .borrow_mut()
            .session
            .library
            .move_playlist_entry(&playlist, &entries[99].id, false)
            .unwrap();
        call(&mut engine, "revisionReturn", &[100.into()]);
        assert_eq!(
            bridge
                .pinned()
                .borrow()
                .session
                .library
                .playlist_entries(&playlist, None, 200)
                .unwrap()[97]
                .id,
            entries[99].id
        );
        call(&mut engine, "revisionHide", &[]);
        // Catalog and Spotify persistence share the same trigger boundary.
        {
            use music_library::catalog::{Album, Medium, Release, Track};
            let identity = music_library::domain::ExternalIdentity {
                provider: "spotify".into(),
                kind: "album".into(),
                external_id: "1234567890123456789010".into(),
            };
            let release = Release {
                album: Album {
                    release_type: None,
                    identity: identity.clone(),
                    title: "Catalog album".into(),
                    date: String::new(),
                    credits: vec![],
                },
                identity: identity.clone(),
                identities: vec![identity],
                title: "Catalog album".into(),
                date: String::new(),
                credits: vec![],
                media: vec![Medium {
                    position: 1,
                    tracks: vec![Track {
                        duration: None,
                        position: 1,
                        title: "Catalog song".into(),
                        credits: vec![],
                        identities: vec![music_library::domain::ExternalIdentity {
                            provider: "spotify".into(),
                            kind: "track".into(),
                            external_id: "1234567890123456789011".into(),
                        }],
                    }],
                }],
            };
            let pin = bridge.pinned();
            let mut bridge = pin.borrow_mut();
            let plan = bridge
                .session
                .library
                .prepare_catalog_playlist_append(&playlist, &release, &[(1, 1)])
                .unwrap();
            bridge
                .session
                .library
                .apply_playlist_append(&plan, true)
                .unwrap();
        }
        call(&mut engine, "revisionReturn", &[101.into()]);
        bridge
            .pinned()
            .borrow_mut()
            .browse_action_impl("playlist-rename", 0, "Renamed A".into());
        call(&mut engine, "revisionNoReload", &[]);
        call(&mut engine, "revisionHide", &[]);
        let plan = music_library::playlist_import::Plan {
            provider: "spotify".into(),
            external_id: "3cEYpjA9oz9GiPac4AsH4n".into(),
            source_url: "https://open.spotify.com/playlist/3cEYpjA9oz9GiPac4AsH4n".into(),
            version: Some("snapshot".into()),
            owner: "Owner".into(),
            name: "Renamed A".into(),
            items: vec![],
            unsupported: 0,
            unavailable: 0,
        };
        bridge
            .pinned()
            .borrow_mut()
            .session
            .library
            .resolve_playlist_import(
                &plan,
                &music_library::playlist_import::Decision::Overwrite(playlist),
            )
            .unwrap();
        call(&mut engine, "revisionReturn", &[0.into()]);
        call(&mut engine, "revisionNoReload", &[]);
    }

    #[test]
    fn spotify_imported_playlist_uses_local_backend_and_missing_file_spotify_fallback_without_queue_changes()
     {
        use music_library::{
            domain::ExternalIdentity,
            playback_resolver::ActiveBackend,
            playlist_import::{Item, Outcome, Plan},
        };
        let (_temp, mut library) = sample::create().unwrap();
        let track = library
            .search(&music_library::domain::SearchRequest {
                limit: 200,
                ..Default::default()
            })
            .unwrap()
            .into_iter()
            .find(|t| t.title == "02 Available")
            .unwrap()
            .track_id;
        let source = library.available_playback_source(&track).unwrap().unwrap();
        let music_library::domain::SourceLocation::LocalFile(path) = source.location else {
            unreachable!()
        };
        std::fs::write(&path, b"safe diagnostic input").unwrap();
        let identity = ExternalIdentity {
            provider: "spotify".into(),
            kind: "track".into(),
            external_id: "1234567890123456789012".into(),
        };
        library
            .attach_track_external_identity(&track, &identity)
            .unwrap();
        library.remove_from_library(&track).unwrap();
        let item = Item {
            identity: identity.clone(),
            title: "Different provider title".into(),
            credits: vec![],
            release_identity: ExternalIdentity {
                kind: "album".into(),
                ..identity
            },
            release_title: "Provider edition".into(),
            release_credits: vec![],
            year: None,
            disc: Some(1),
            number: Some(1),
            duration: None,
        };
        let plan = Plan {
            provider: "spotify".into(),
            external_id: "3cEYpjA9oz9GiPac4AsH4n".into(),
            source_url: "https://open.spotify.com/playlist/3cEYpjA9oz9GiPac4AsH4n".into(),
            version: Some("v1".into()),
            owner: "Other owner".into(),
            name: "Source policy".into(),
            items: vec![item.clone(), item],
            unsupported: 0,
            unavailable: 0,
        };
        let Outcome::Imported { playlist_id, .. } =
            library.import_playlist_snapshot(&plan).unwrap()
        else {
            panic!()
        };
        let rows = library
            .library_queue_reader()
            .unwrap()
            .read_playlist(&playlist_id, None)
            .unwrap()
            .0;
        assert_eq!(
            rows.iter().map(|t| t.track_id.clone()).collect::<Vec<_>>(),
            vec![track.clone(), track.clone()]
        );
        let mut bridge = Bridge::new(Session::new(library));
        bridge.real_audio = true;
        bridge.spotify_playback_state.authorization =
            music_library_spotify::playback::AuthorizationState::Connected;
        bridge.spotify_playback_state.selected_device = Some("desktop".into());
        bridge.spotify_playback_state.devices = vec![music_library_spotify::playback::Device {
            id: Some("desktop".into()),
            name: "Desktop".into(),
            kind: "Computer".into(),
            is_active: true,
            is_restricted: false,
            supports_volume: false,
            volume_percent: None,
        }];
        let (worker, commands) = crate::spotify_playback::Worker::fake();
        bridge.spotify_playback_worker = Some(worker);
        let bridge = QObjectBox::new(bridge);
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        engine.load_data("import QtQuick; Item {}".into());
        bridge
            .pinned()
            .borrow_mut()
            .replace_library_program(rows, 0);
        assert_eq!(
            bridge.pinned().borrow().active_backend,
            ActiveBackend::Local
        );
        assert!(commands.try_recv().is_err());
        let queue = bridge
            .pinned()
            .borrow()
            .session
            .playback
            .state()
            .queue
            .clone();
        std::fs::remove_file(&path).unwrap();
        bridge.pinned().borrow_mut().navigate_queue(false);
        assert!(matches!(
            commands.try_recv().unwrap(),
            crate::spotify_playback::Command::ApplicationPlay(..)
        ));
        bridge.pinned().borrow_mut().finish_remote_handoff();
        assert_eq!(
            bridge.pinned().borrow().active_backend,
            ActiveBackend::Remote("spotify".into())
        );
        assert_eq!(
            bridge.pinned().borrow().session.playback.state().queue,
            queue
        );
        assert_eq!(
            bridge
                .pinned()
                .borrow()
                .session
                .library
                .imported_playlist("spotify", &plan.external_id)
                .unwrap(),
            Some(playlist_id)
        );
        assert!(
            !bridge
                .pinned()
                .borrow()
                .session
                .library
                .browse(&music_library::browse::Request {
                    limit: 200,
                    ..Default::default()
                })
                .unwrap()
                .iter()
                .any(|row| row.id == track.as_ref())
        );
    }

    #[test]
    #[cfg(feature = "gstreamer")]
    #[ignore = "real Hop Along library copy and GStreamer device required"]
    fn live_hop_along_playlist_local_playback_paths() {
        let path = std::env::var("MUSIC_LIBRARY_DIAGNOSTIC_DATABASE").unwrap();
        let playlist = std::env::var("MUSIC_LIBRARY_HOP_PLAYLIST").unwrap();
        let library = music_library::Library::open(path).unwrap();
        // Compare real persisted playlist rows with the unchanged Songs projection.
        for row in library.playlist_entries(&playlist, None, 200).unwrap() {
            let track = row.track.as_ref().unwrap();
            let song = library
                .browse(&music_library::browse::Request {
                    pane: music_library::browse::Pane::Songs,
                    tracks: vec![track.track_id.as_ref().to_owned()],
                    limit: 1,
                    ..Default::default()
                })
                .unwrap()
                .remove(0);
            assert_eq!(row.subtitle, "Hop Along");
            assert_eq!(row.subtitle, song.subtitle);
            assert_eq!(
                track.release_title,
                song.track.as_ref().unwrap().release_title
            );
            assert_eq!(row.genres, song.genres);
            assert!(row.duration_ms.is_some());
        }
        let associations = QVariantMap::from_iter(
            library
                .playlist_entries(&playlist, None, 200)
                .unwrap()
                .into_iter()
                .map(|row| {
                    let track = row.track.unwrap().track_id;
                    assert!(
                        library.available_playback_source(&track).unwrap().is_some(),
                        "reconciled Track has its local source"
                    );
                    let ids = library
                        .track_provider_occurrences(&track, "spotify")
                        .unwrap();
                    let uri = music_library_spotify::playback::Song::from_associations(&ids)
                        .map(|s| s.uri())
                        .unwrap_or_default();
                    (QString::from(track.as_ref()), string(uri))
                }),
        );
        let unconnected = library
            .browse(&music_library::browse::Request {
                pane: music_library::browse::Pane::Songs,
                sort: music_library::browse::Sort::Title,
                limit: 200,
                ..Default::default()
            })
            .unwrap()
            .into_iter()
            .find(|row| {
                let id = music_library::domain::TrackId(row.id.clone());
                library.available_playback_source(&id).unwrap().is_some()
                    && library
                        .track_provider_occurrences(&id, "spotify")
                        .unwrap()
                        .is_empty()
            })
            .expect("real unconnected local Track in first Songs page")
            .id;
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let mut engine = QmlEngine::new();
        let audio = music_library_gstreamer::GStreamerEngine::new(engine_callback(bridge.pinned()))
            .unwrap();
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.auto_match = false;
            b.real_audio = true;
            b.session.playback =
                music_library::playback::Playback::new(session::Engine::GStreamer(audio));
            b.session.set_volume(0.0);
        }
        engine.set_property("hopAssociations".into(), associations.into());
        engine.set_property("hopUnconnected".into(), string(unconnected));
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n    function ready() {{",
                    include_str!("../HopAlongPlaybackTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        let result = engine
            .invoke_method("hopLivePlayback".into(), &[string(playlist)])
            .to_qstring()
            .to_string();
        bridge.pinned().borrow_mut().session.shutdown_audio();
        assert_eq!(result, "ok");
    }

    #[test]
    fn playlist_catalog_button_geometry_stays_stable() {
        let (_temp, mut library) = sample::create().unwrap();
        let track = library
            .search(&music_library::domain::SearchRequest {
                limit: 200,
                ..Default::default()
            })
            .unwrap()
            .into_iter()
            .find(|t| t.title == "02 Available")
            .unwrap()
            .track_id;
        let playlist = library.create_playlist("Geometry audit").unwrap();
        library.append_playlist_track(&playlist, &track).unwrap();
        let db = rusqlite::Connection::open(_temp.path().join("diagnostic.sqlite")).unwrap();
        db.execute("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<200000) INSERT INTO playlist_entry(id,playlist_id,track_id,position) SELECT 'geometry-'||x,?1,?2,x FROM n",rusqlite::params![playlist,track.as_ref()]).unwrap();
        drop(db);

        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.browse_action_impl("view", 0, "Playlists".into());
            b.browse_action_impl("select", 0, playlist);
        }
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n    function ready() {{",
                    include_str!("../PlaylistGeometryTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        for playing in [false, true] {
            assert_eq!(
                engine
                    .invoke_method(
                        "auditPlaylistGeometry".into(),
                        &[20000.into(), playing.into()]
                    )
                    .to_qstring()
                    .to_string(),
                "ok"
            );
        }
    }

    #[test]
    fn mouse_keyboard_selection_queue_and_drawer() {
        let (_temp, mut library) = sample::create().unwrap();
        rusqlite::Connection::open(_temp.path().join("diagnostic.sqlite"))
            .unwrap()
            .execute("UPDATE album_application_metadata SET year=2015", [])
            .unwrap();
        // Explicit fixture identities: one 44-Track Artist and one separate Artist.
        let artists = library
            .browse(&music_library::browse::Request {
                pane: music_library::browse::Pane::Artists,
                limit: 200,
                ..Default::default()
            })
            .unwrap();
        for row in artists.iter().skip(1).take(43) {
            library
                .merge_artist(
                    &music_library::domain::ArtistId(row.id.clone()),
                    &music_library::domain::ArtistId(artists[0].id.clone()),
                )
                .unwrap();
        }
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n{}\n    function ready() {{",
                    include_str!("../LibraryUiTest.qml"),
                    include_str!("../LibrarySearchTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        assert_eq!(
            engine
                .invoke_method("exerciseLibraryUi".into(), &[])
                .to_qstring()
                .to_string(),
            "ok"
        );
        #[cfg(feature = "gstreamer")]
        player_controls::test_controls(&bridge, &mut engine);
        {
            use music_library::domain::{CatalogReleaseInput, CatalogTrackInput};
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            let imported = b
                .session
                .library
                .create_catalog_release(&CatalogReleaseInput {
                    title: "Paged program".into(),
                    year: None,
                    artists: vec![],
                    tracks: (0..451)
                        .map(|i| CatalogTrackInput {
                            title: format!("Paged duplicate {}", i % 3),
                            artists: vec![],
                            disc_number: Some(1),
                            track_number: Some(i + 1),
                        })
                        .collect(),
                })
                .unwrap();
            for track in imported.track_ids {
                b.session.library.add_to_library(&track).unwrap();
            }
        }
        assert_eq!(
            engine
                .invoke_method("exercisePagedProgram".into(), &[])
                .to_qstring()
                .to_string(),
            "ok"
        );
        {
            use music_library::domain::{
                ArtistCreditInput, CatalogReleaseInput, CatalogTrackInput,
            };
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            let release = b
                .session
                .library
                .create_catalog_release(&CatalogReleaseInput {
                    title: "Painted Shut".into(),
                    year: None,
                    artists: vec![ArtistCreditInput {
                        name: "Hop Along".into(),
                        role: None,
                    }],
                    tracks: ["First", "Second", "Waitress"]
                        .iter()
                        .enumerate()
                        .map(|(i, title)| CatalogTrackInput {
                            title: (*title).into(),
                            artists: vec![],
                            disc_number: Some(1),
                            track_number: Some(i as u32 + 1),
                        })
                        .collect(),
                })
                .unwrap();
            for id in release.track_ids {
                b.session.library.add_to_library(&id).unwrap();
            }
            let artists = b
                .session
                .library
                .browse(&music_library::browse::Request {
                    pane: music_library::browse::Pane::Artists,
                    limit: 200,
                    ..Default::default()
                })
                .unwrap()
                .into_iter()
                .filter(|r| r.title == "Hop Along")
                .collect::<Vec<_>>();
            for artist in artists.iter().skip(1) {
                b.session
                    .library
                    .merge_artist(
                        &music_library::domain::ArtistId(artist.id.clone()),
                        &music_library::domain::ArtistId(artists[0].id.clone()),
                    )
                    .unwrap();
            }
            b.browse_action_impl("refresh", 0, String::new());
        }
        assert_eq!(
            engine
                .invoke_method("exerciseLocalSearch".into(), &[false.into()])
                .to_qstring()
                .to_string(),
            "ok"
        );
        {
            use music_library::domain::{
                ArtistCreditInput, CatalogReleaseInput, CatalogTrackInput,
            };
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            for i in 0..211 {
                let release = b
                    .session
                    .library
                    .create_catalog_release(&CatalogReleaseInput {
                        title: format!("ZZZ Album {i:03}"),
                        year: None,
                        artists: vec![ArtistCreditInput {
                            name: format!("ZZZ Artist {i:03}"),
                            role: None,
                        }],
                        tracks: (0..if i == 210 { 211 } else { 1 })
                            .map(|n| CatalogTrackInput {
                                title: if n == 210 {
                                    "ZZZ target".into()
                                } else {
                                    "ZZZ duplicate".into()
                                },
                                artists: vec![],
                                disc_number: Some(1),
                                track_number: Some(n + 1),
                            })
                            .collect(),
                    })
                    .unwrap();
                for id in release.track_ids {
                    b.session.library.add_to_library(&id).unwrap();
                }
            }
        }
        assert_eq!(
            engine
                .invoke_method("exerciseDistantSearch".into(), &[])
                .to_qstring()
                .to_string(),
            "ok"
        );
        let pinned = bridge.pinned();
        let b = pinned.borrow();
        assert!(b.spotify_resolution_worker.is_none());
        assert!(b.spotify_playback_worker.is_none());
        assert!(b.matcher.is_none());
    }
}

#[cfg(test)]
mod metadata_ui_tests {
    use super::*;
    use music_library::{domain::*, metadata::Target};
    #[test]
    #[ignore = "explicit offscreen Qt metadata interaction test"]
    fn metadata_context_actions_use_clicked_canonical_ids_and_keep_queue() {
        let temp = tempfile::tempdir().unwrap();
        let mut library = music_library::Library::open(temp.path().join("db")).unwrap();
        let release = library
            .create_catalog_release(&CatalogReleaseInput {
                title: "Album".into(),
                year: Some(2012),
                artists: vec![],
                tracks: (1..=3)
                    .map(|n| CatalogTrackInput {
                        title: format!("Song {n}"),
                        artists: vec![],
                        disc_number: Some(1),
                        track_number: Some(n),
                    })
                    .collect(),
            })
            .unwrap();
        for t in &release.track_ids {
            library.add_to_library(t).unwrap();
        }
        let release = if std::env::var_os("METADATA_REAL_FIXTURE").is_some() {
            let root = temp.path().join("real-audio");
            std::fs::create_dir(&root).unwrap();
            for entry in std::fs::read_dir("../../test-media/Get Disowned").unwrap() {
                let path = entry.unwrap().path();
                if path.extension().is_some_and(|e| e == "mp3") {
                    std::fs::copy(&path, root.join(path.file_name().unwrap())).unwrap();
                }
            }
            library
                .ingest_local(
                    &music_library::local_ingestion::Request::Folder(root),
                    &mut music_library::filesystem::LoftyMetadataExtractor,
                    &mut |_| {},
                )
                .unwrap();
            let rows = library
                .browse(&music_library::browse::Request {
                    pane: music_library::browse::Pane::Songs,
                    limit: 200,
                    ..Default::default()
                })
                .unwrap();
            let rows: Vec<_> = rows
                .into_iter()
                .filter(|r| {
                    r.track
                        .as_ref()
                        .is_some_and(|t| t.release_title == "Get Disowned")
                })
                .collect();
            assert_eq!(rows.len(), 10);
            ImportedRelease {
                release_id: rows[0].track.as_ref().unwrap().release_id.clone(),
                track_ids: rows
                    .iter()
                    .map(|r| r.track.as_ref().unwrap().track_id.clone())
                    .collect(),
            }
        } else {
            release
        };
        let album = library
            .album_for_release(&release.release_id)
            .unwrap()
            .album_id;
        let playlist = library.create_playlist("P").unwrap();
        let entry = library
            .append_playlist_track(&playlist, &release.track_ids[2])
            .unwrap();
        let cached = serde_json::json!({"items":[{
            "identity":{"provider":"spotify","kind":"track","external_id":"metadata-ui-candidate"},
            "album_identity":null,"title":"Persisted song","artist":"Band","artists":[],
            "album":"Album","date":"2012","album_artists":[],"album_type":"album",
            "album_total_tracks":3,"duration_ms":120000,"disc":1,"number":2
        }],"next_offset":null});
        rusqlite::Connection::open(temp.path().join("db")).unwrap().execute(
            "INSERT INTO spotify_reconciliation_cache(track_id,input_json,page_json) VALUES(?1,?2,?3)",
            rusqlite::params![release.track_ids[1].as_ref(), serde_json::to_string(&library.song_resolution_input(&release.track_ids[1]).unwrap()).unwrap(), cached.to_string()]
        ).unwrap();
        let mut session = Session::new(library);
        session
            .playback
            .set_queue(vec![release.track_ids[0].clone()])
            .unwrap();
        let bridge = QObjectBox::new(Bridge::new(session));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let qml=include_str!("../Main.qml").replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1).replace("    function ready() {",r#"
    TestCase { id: metadataTest; when: false }
    Timer { id: metadataTimeout; interval: 10000; onTriggered: Qt.quit() }
    Connections { target: window.bridge; function onMetadata_changed() { if(metadataTimeout.running && !window.metadataState.busy) {metadataTimeout.stop(); Qt.quit();} } }
    function testTrackMetadata(track,entry) {
        if(entry.length) {window.bridge.browse_action("view",0,"Playlists");window.openSongContext(2,{id:entry,track:{trackId:track}});}
        else {window.openSongContext(2,{id:track});}
        trackMetadataMenuAction.triggered();libraryMenu.close();
        metadataTest.wait(30);
        metadataTest.grabImage(window.contentItem).save("/tmp/metadata-dialog.png");
        return JSON.stringify(metadataDialog.inspection);
    }
    function testAlbumMetadata(album) {window.bridge.browse_action("view",0,"Artists");window.contextPane=1;window.contextId=album;albumMetadataMenuAction.triggered();metadataTest.wait(30);metadataTest.grabImage(window.contentItem).save("/tmp/metadata-album-dialog.png");return JSON.stringify(metadataDialog.inspection);}
    function testMetadataSave(field,value) {
        const input=metadataTest.findChild(metadataDialog.contentItem,"metadataField_"+field);
        input.forceActiveFocus();metadataTest.keyClick(Qt.Key_A,Qt.ControlModifier);
        for(const character of value) metadataTest.keyClick(character);
        metadataTimeout.start();metadataDialog.save();
    }
    function testMetadataSelectAll() {
        metadataDialog.writeFiles=true;
        const select=metadataTest.findChild(metadataDialog.contentItem,"metadataSelectAllFiles");
        const clear=metadataTest.findChild(metadataDialog.contentItem,"metadataClearFiles");
        select.clicked();
        const count=Object.keys(metadataDialog.selectedFiles).filter(key=>metadataDialog.selectedFiles[key]).length;
        clear.clicked();
        return Object.keys(metadataDialog.selectedFiles).length===0 ? count : -1;
    }
    function testMetadataWriteAlbum() {
        metadataDialog.setEdit("title","Correct Album files");
        metadataDialog.setEdit("artist_credit","Hop Along, Queen Ansleis");
        metadataDialog.writeFiles=true;
        metadataTest.findChild(metadataDialog.contentItem,"metadataSelectAllFiles").clicked();
        metadataTimeout.start();metadataDialog.save();
    }
    function testMetadataConnect(track) {
        const button=metadataTest.findChild(metadataDialog.contentItem,"metadataConnectSpotify_"+track+"_metadata-ui-candidate");
        if(!button || !button.visible || !button.enabled) return "candidate button unavailable";
        button.clicked();
        return window.metadataState.message;
    }
    function testRenameDraft(title) { metadataDialog.setEdit("title",title); metadataDialog.save(); return metadataRenameDialog.visible; }
    function testRenameChoice(action) {
        const button=metadataTest.findChild(metadataRenameDialog.contentItem,"metadataRename"+action);
        if(action!=="Cancel") metadataTimeout.start();
        button.clicked();
        return Object.keys(metadataDialog.edits).length;
    }
    function testMetadataClose() {metadataDialog.close();}
    function ready() {
"#);
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        let snapshot = engine
            .invoke_method(
                "testTrackMetadata".into(),
                &[string(release.track_ids[1].as_ref()), string("")],
            )
            .to_qstring()
            .to_string();
        let snapshot: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
        assert_eq!(snapshot["target"]["track"], release.track_ids[1].0);
        assert_eq!(
            engine
                .invoke_method(
                    "testMetadataConnect".into(),
                    &[string(release.track_ids[1].as_ref())]
                )
                .to_qstring()
                .to_string(),
            "Spotify candidate connected ✓"
        );
        assert_eq!(
            bridge
                .pinned()
                .borrow()
                .session
                .library
                .list_track_external_identities(&release.track_ids[1])
                .unwrap()[0]
                .external_id,
            "metadata-ui-candidate"
        );
        engine.invoke_method(
            "testMetadataSave".into(),
            &[string("title"), string("Correct Song")],
        );
        engine.exec();
        let snapshot = bridge
            .pinned()
            .borrow()
            .session
            .library
            .inspect_metadata(&Target::Track(release.track_ids[1].0.clone()))
            .unwrap();
        assert_eq!(snapshot.fields[0].value, "Correct Song");
        assert_eq!(
            bridge.pinned().borrow().session.playback.state().queue,
            vec![release.track_ids[0].clone()]
        );
        engine.invoke_method("testMetadataClose".into(), &[]);
        let snapshot = engine
            .invoke_method(
                "testTrackMetadata".into(),
                &[string(release.track_ids[2].as_ref()), string(&entry)],
            )
            .to_qstring()
            .to_string();
        let snapshot: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
        assert_eq!(snapshot["target"]["track"], release.track_ids[2].0);
        engine.invoke_method("testMetadataClose".into(), &[]);
        let snapshot = engine
            .invoke_method("testAlbumMetadata".into(), &[string(album.as_ref())])
            .to_qstring()
            .to_string();
        let snapshot: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
        assert_eq!(snapshot["target"]["album"], album.0);
        assert_eq!(snapshot["track_count"], release.track_ids.len());
        let groups = snapshot["track_evidence"].as_array().unwrap();
        assert_eq!(groups.len(), release.track_ids.len());
        assert!(
            groups
                .iter()
                .all(|group| group["evidence"][0]["source"] == "Library")
        );
        engine.invoke_method(
            "testMetadataSave".into(),
            &[string("title"), string("Correct Album")],
        );
        engine.exec();
        for t in &release.track_ids {
            let i = bridge
                .pinned()
                .borrow()
                .session
                .library
                .inspect_metadata(&Target::Track(t.0.clone()))
                .unwrap();
            assert_eq!(
                i.fields.iter().find(|f| f.key == "album").unwrap().value,
                "Correct Album"
            );
        }
        if std::env::var_os("METADATA_REAL_FIXTURE").is_some() {
            assert_eq!(
                engine
                    .invoke_method("testMetadataSelectAll".into(), &[])
                    .to_int(),
                10
            );
            engine.invoke_method("testMetadataWriteAlbum".into(), &[]);
            engine.exec();
            let snapshot = bridge
                .pinned()
                .borrow()
                .session
                .library
                .inspect_metadata(&Target::Album(album.0.clone()))
                .unwrap();
            use music_library::filesystem::MetadataExtractor;
            for file in snapshot.files {
                let tags = music_library::filesystem::LoftyMetadataExtractor
                    .read(std::path::Path::new(&file.path))
                    .unwrap();
                assert_eq!(tags.release_title.as_deref(), Some("Correct Album files"));
                assert_eq!(tags.release_artists, vec!["Hop Along, Queen Ansleis"]);
            }
        }
        let destination = bridge
            .pinned()
            .borrow_mut()
            .session
            .library
            .create_catalog_release(&music_library::domain::CatalogReleaseInput {
                title: "Existing rename destination".into(),
                year: Some(2000),
                artists: vec![],
                tracks: vec![music_library::domain::CatalogTrackInput {
                    title: "Destination Track".into(),
                    artists: vec![],
                    disc_number: Some(1),
                    track_number: Some(99),
                }],
            })
            .unwrap();
        let destination_album = bridge
            .pinned()
            .borrow()
            .session
            .library
            .album_id_for_track(&destination.track_ids[0])
            .unwrap();
        assert!(
            engine
                .invoke_method(
                    "testRenameDraft".into(),
                    &[string("Existing rename destination")]
                )
                .to_bool()
        );
        assert_eq!(
            bridge
                .pinned()
                .borrow()
                .session
                .library
                .album_id_for_track(&release.track_ids[0])
                .unwrap(),
            album
        );
        assert!(
            engine
                .invoke_method("testRenameChoice".into(), &[string("Cancel")])
                .to_int()
                > 0
        );
        assert!(
            engine
                .invoke_method(
                    "testRenameDraft".into(),
                    &[string("Existing rename destination")]
                )
                .to_bool()
        );
        engine.invoke_method("testRenameChoice".into(), &[string("Keep")]);
        engine.exec();
        assert_eq!(
            bridge
                .pinned()
                .borrow()
                .session
                .library
                .album_id_for_track(&release.track_ids[0])
                .unwrap(),
            album
        );
        assert!(
            engine
                .invoke_method(
                    "testRenameDraft".into(),
                    &[string("Existing rename destination")]
                )
                .to_bool()
        );
        engine.invoke_method("testRenameChoice".into(), &[string("Move")]);
        engine.exec();
        for track in &release.track_ids {
            assert_eq!(
                bridge
                    .pinned()
                    .borrow()
                    .session
                    .library
                    .album_id_for_track(track)
                    .unwrap(),
                destination_album
            );
        }
        assert_eq!(
            bridge.pinned().borrow().session.playback.state().queue,
            vec![release.track_ids[0].clone()]
        );
    }
}
