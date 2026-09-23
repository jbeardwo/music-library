mod catalog;
#[cfg(feature = "gstreamer")]
mod local;
mod playback_route;
mod sample;
mod session;
use music_library_spotify::playback_completion as spotify_completion;
mod spotify_playback;
mod spotify_resolution;

use qmetaobject::prelude::*;
use qmetaobject::{QVariantList, QVariantMap};
use session::Session;

#[derive(QObject)]
struct Bridge {
    base: qt_base_class!(trait QObject),
    active_backend: music_library::playback_resolver::ActiveBackend,
    route_pending: Option<playback_route::Pending>,
    automatic_song_search: bool,
    preserve_play_queue: bool,
    restart_playback: bool,
    playback_generation: u64,
    route_message: String,
    resolver_dialog_requested: qt_signal!(),
    spotify_resolution_worker: Option<spotify_resolution::Worker>,
    spotify_resolution_generation: u64,
    spotify_resolution_selection: Option<music_library::song_resolution::Selection>,
    spotify_resolution_pending: bool,
    spotify_resolution_message: String,
    spotify_resolution_counts: (u64, u64),
    spotify_resolve: qt_method!(
        fn spotify_resolve(&mut self, action: String, index: i32) {
            if action == "cancel" {
                self.automatic_song_search = false;
                self.spotify_resolution_generation += 1;
                self.spotify_resolution_selection = None;
                self.spotify_resolution_pending = false;
                self.spotify_resolution_message =
                    "Selection canceled; no association changed".into();
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
                            .confirm_song_resolution(selection, index as usize)
                            .map_err(|e| e.to_string())
                    });
                match result {
                    Ok(id) => {
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
            if action != "search" || self.spotify_resolution_pending {
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
                let callback = qmetaobject::queued_callback(
                    move |reply: spotify_resolution::Reply| {
                        if let Some(pinned) = weak.as_pinned() {
                            let mut b = pinned.borrow_mut();
                            b.spotify_resolution_counts = reply.counts;
                            if reply.generation == b.spotify_resolution_generation {
                                b.spotify_resolution_pending = false;
                                match reply.result {
                                    Ok(page) => {
                                        let automatic = b.automatic_song_search;
                                        b.automatic_song_search = false;
                                        if automatic {
                                            b.finish_automatic_song(reply.input, page);
                                            b.spotify_playback_changed();
                                            b.changed();
                                            return;
                                        }
                                        b.spotify_resolution_message = if page.items.is_empty() {
                                            "No candidates on this bounded page".into()
                                        } else if page.next_offset.is_some() {
                                            "First 10 results only; choose explicitly. More provider results exist.".into()
                                        } else {
                                            "Choose explicitly; no candidate is automatically accepted".into()
                                        };
                                        b.spotify_resolution_selection =
                                            Some(music_library::song_resolution::Selection::new(
                                                reply.input,
                                                page.items,
                                            ));
                                    }
                                    Err(error) => {
                                        b.automatic_song_search = false;
                                        b.route_message =
                                            format!("Spotify association lookup failed: {error}");
                                        b.spotify_resolution_message = error.to_string();
                                        b.changed();
                                    }
                                }
                            }
                            b.spotify_playback_changed();
                        }
                    },
                );
                self.spotify_resolution_worker = Some(spotify_resolution::Worker::new(callback));
            }
            self.spotify_resolution_generation += 1;
            self.spotify_resolution_selection = None;
            self.spotify_resolution_pending = self
                .spotify_resolution_worker
                .as_ref()
                .unwrap()
                .search(self.spotify_resolution_generation, input);
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
    spotify_playback_song: Option<music_library_spotify::playback::Song>,
    spotify_playback_track: Option<music_library::domain::TrackId>,
    spotify_playback_title: String,
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
                self.spotify_resolution_pending = false;
                self.spotify_resolution_message.clear();
                self.spotify_playback_track = Some(music_library::domain::TrackId(value.clone()));
                self.spotify_playback_title = self
                    .session
                    .rows
                    .iter()
                    .find(|r| r.track_id.as_ref() == value)
                    .map(|r| format!("{} — {} [{}]", r.artist_names, r.title, r.release_title))
                    .unwrap_or_else(|| value.clone());
                self.spotify_playback_song = self
                    .session
                    .library
                    .track_provider_occurrences(&music_library::domain::TrackId(value), "spotify")
                    .ok()
                    .and_then(|ids| {
                        music_library_spotify::playback::Song::from_associations(&ids).ok()
                    });
                if self.spotify_playback_state.error.as_ref().is_none_or(|e| {
                    matches!(e, music_library_spotify::playback::Error::NoAssociation)
                }) {
                    self.spotify_playback_state.error = self
                        .spotify_playback_song
                        .is_none()
                        .then_some(music_library_spotify::playback::Error::NoAssociation);
                }
                self.spotify_playback_changed();
                return;
            }
            if self.spotify_playback_worker.is_none() {
                let weak = qmetaobject::QPointer::from(&*self);
                let callback =
                    qmetaobject::queued_callback(move |update: spotify_playback::Update| {
                        if let Some(pinned) = weak.as_pinned() {
                            let mut bridge = pinned.borrow_mut();
                            bridge.spotify_playback_state = update.snapshot;
                            if update.application_command {
                                bridge.finish_remote_handoff();
                            }
                            bridge.observe_remote_completion(update.generation, update.completion);
                            bridge.spotify_playback_changed();
                            bridge.changed();
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
    set_volume: qt_method!(
        fn set_volume(&mut self, value: f64) {
            self.session.set_volume(value);
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
    fn new(session: Session) -> Self {
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
            spotify_resolution_worker: None,
            spotify_resolution_generation: 0,
            spotify_resolution_selection: None,
            spotify_resolution_pending: false,
            spotify_resolution_message: String::new(),
            spotify_resolution_counts: (0, 0),
            spotify_resolve: Default::default(),
            spotify_playback_worker: None,
            spotify_playback_state: Default::default(),
            spotify_playback_song: None,
            spotify_playback_track: None,
            spotify_playback_title: String::new(),
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
            stored_programs: Default::default(),
            matching_provider: Default::default(),
            retry_matching: Default::default(),
            retry_match: Default::default(),
            choose_artist: Default::default(),
            matcher: None,
            matching_tracks: Default::default(),
            matching_rows: Vec::new(),
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
            catalog: Default::default(),
            session,
            real_audio: false,
        }
    }

    fn spotify_playback_value(&self) -> QVariantMap {
        let s = &self.spotify_playback_state;
        let choices: QVariantList = self
            .spotify_resolution_selection
            .as_ref()
            .map(|selection| {
                selection
                    .candidates()
                    .iter()
                    .map(|c| {
                        string(format!(
                            "{} — {} | {} ({}) | {}:{:02} | disc {} track {} | {}",
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
            ("resolutionChoices", choices.into()),
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
                        .map(ToString::to_string)
                        .unwrap_or_default(),
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
                        music_library_spotify::Spotify::from_env().map_err(|e| e.to_string())?,
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
                        let refresh =
                            music_library::catalog::Timing::new("post_import.search_refresh");
                        self.session.search(release.album.title);
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
        let rows: QVariantList = s.rows.iter().map(row_value).collect();
        let queue: QVariantList = state
            .queue
            .iter()
            .enumerate()
            .map(|(i, id)| {
                // IDs/position come from PlaybackState, labels are presentation-only snapshots.
                let label = s.queue_labels.get(i).filter(|row| &row.track_id == id);
                let row: QVariantMap = [
                    ("title", string(label.map_or(id.as_ref(), |row| &row.title))),
                    ("trackId", string(id.as_ref())),
                    ("current", (state.position == Some(i)).into()),
                ]
                .into_iter()
                .collect();
                QVariant::from(row)
            })
            .collect();
        let current = state.position.and_then(|i| s.queue_labels.get(i));
        [
            ("rows", rows.into()),
            ("queue", queue.into()),
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
            ("volume", state.volume.get().into()),
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
                    time_label(Some(if remote_active {
                        self.spotify_playback_state.state.progress_ms
                    } else {
                        state.media_position_ms
                    })),
                    time_label(if remote_active {
                        self.spotify_playback_state.state.duration_ms
                    } else {
                        state.duration_ms
                    })
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
    let (smoke, folder) = match args.as_slice() {
        [] => (false, None),
        [arg] if arg == "--smoke-test" => (true, None),
        [arg, folder] if arg == "--gstreamer" => (false, Some(std::path::PathBuf::from(folder))),
        [arg, folder, check] if arg == "--gstreamer" && check == "--smoke-test" => {
            (true, Some(std::path::PathBuf::from(folder)))
        }
        _ => {
            return Err(
                "usage: qml-diagnostic [--catalog-provider musicbrainz|spotify | --catalog-providers spotify,musicbrainz] [--no-auto-match] [--smoke-test | --gstreamer FOLDER [--smoke-test]]".into(),
            );
        }
    };
    #[cfg(not(feature = "gstreamer"))]
    if folder.is_some() {
        return Err("rebuild with --features gstreamer to use real audio".into());
    }
    #[cfg(feature = "gstreamer")]
    let (_temp, library, imports) = if let Some(folder) = &folder {
        local::load(folder)?
    } else {
        let (temp, library) = sample::create()?;
        (temp, library, vec![])
    };
    #[cfg(not(feature = "gstreamer"))]
    let (_temp, library, imports) = {
        let (temp, library) = sample::create()?;
        (temp, library, vec![])
    };
    // Pin before exposing to QML, and keep the QObject alive until QML destruction.
    let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
    bridge.pinned().borrow_mut().spotify = spotify;
    bridge.pinned().borrow_mut().catalog_providers = providers;
    let mut engine = QmlEngine::new();
    #[cfg(feature = "gstreamer")]
    if folder.is_some() {
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
        if smoke && folder.is_some() {
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
            engine.exec();
        }
        Ok(())
    })();
    bridge.pinned().borrow_mut().matcher.take();
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
                        credits: vec![],
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
                        identity: id("group"),
                        title: "Catalog Fixture".into(),
                        date: "2001".into(),
                        credits: vec![],
                    },
                    identity: id("release"),
                    identities: vec![],
                    title: "Catalog Fixture".into(),
                    date: "2001".into(),
                    credits: vec![],
                    media: vec![Medium {
                        position: 1,
                        tracks: vec![Track {
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
            let row = b
                .session
                .library
                .search(&SearchRequest {
                    limit: 1,
                    ..Default::default()
                })
                .unwrap()
                .remove(0);
            b.spotify_playback_action("track".into(), row.track_id.as_ref().into());
            let input = b
                .session
                .library
                .song_resolution_input(&row.track_id)
                .unwrap();
            Selection::new(
                input,
                vec![Candidate {
                    identity: music_library::domain::ExternalIdentity {
                        provider: "spotify".into(),
                        kind: "track".into(),
                        external_id: "1234567890123456789012".into(),
                    },
                    title: "Provider Song".into(),
                    artist: "Artist".into(),
                    album: "Album variant".into(),
                    date: "2020".into(),
                    duration_ms: 200000,
                    disc: 1,
                    number: 1,
                }],
            )
        };
        let publish = || {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.spotify_resolution_selection = Some(selection.clone());
            b.spotify_playback_changed();
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
            vec![selection.candidates()[0].identity.clone()]
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
                .find(|(_, rows)| !rows.is_empty())
                .unwrap();
            let album = album.clone();
            let tracks = tracks.clone();
            let track = tracks[0].track_id.clone();
            let identity = state
                .session
                .library
                .list_album_external_identities(&album)
                .unwrap()[0]
                .clone();
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
