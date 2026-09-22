//! Diagnostic execution of the UI-independent explicit-Play route decision.
use crate::{Bridge, spotify_playback::Command};
use music_library::{
    catalog::Page,
    domain::{ExternalIdentity, PlayableSource, TrackId},
    playback::PlaybackStatus,
    playback_resolver::{ActiveBackend, RemoteCapability, Route},
    song_resolution::{Assessment, Candidate, Input, Selection, assess},
};
use music_library_spotify::playback::{AuthorizationState, Error, Song};

pub enum Pending {
    StopLocal {
        track: TrackId,
        identity: ExternalIdentity,
    },
    PauseRemote {
        track: TrackId,
        source: PlayableSource,
    },
    StartRemote,
}

#[cfg(test)]
pub(crate) fn test_handoffs() {
    use crate::{sample, session::Session, spotify_playback::Worker};
    use music_library_spotify::playback::Device;
    let (_temp, library) = sample::create().unwrap();
    let object = qmetaobject::QObjectBox::new(Bridge::new(Session::new(library)));
    let pinned = object.pinned();
    let mut b = pinned.borrow_mut();
    b.real_audio = true;
    let local = b.session.rows[2].track_id.clone();
    let remote = b.session.rows[0].track_id.clone();
    let source = b
        .session
        .library
        .available_playback_source(&local)
        .unwrap()
        .unwrap();
    let music_library::domain::SourceLocation::LocalFile(path) = &source.location else {
        unreachable!()
    };
    std::fs::write(path, b"fake decoder input").unwrap();
    let identity = ExternalIdentity {
        provider: "spotify".into(),
        kind: "track".into(),
        external_id: "1234567890123456789012".into(),
    };
    b.session
        .library
        .attach_track_external_identity(&remote, &identity)
        .unwrap();
    b.spotify_playback_state.authorization = AuthorizationState::Connected;
    b.spotify_playback_state.selected_device = Some("desktop".into());
    b.spotify_playback_state.devices = vec![Device {
        id: Some("desktop".into()),
        name: "Desktop".into(),
        kind: "Computer".into(),
        is_active: true,
        is_restricted: false,
        supports_volume: false,
    }];
    let (worker, commands) = Worker::fake();
    b.spotify_playback_worker = Some(worker);

    b.resolve_play(local.as_ref());
    assert_eq!(b.active_backend, ActiveBackend::Local);
    assert!(commands.try_recv().is_err()); // zero Spotify playback requests
    assert!(b.spotify_resolution_worker.is_none()); // zero catalog requests
    b.resolve_play(local.as_ref()); // Local -> Local
    assert_eq!(b.session.playback.state().status, PlaybackStatus::Playing);
    assert!(commands.try_recv().is_err());

    b.resolve_play(remote.as_ref()); // Local -> Spotify, Stop precedes Play
    assert_eq!(b.session.playback.state().status, PlaybackStatus::Stopped);
    assert!(matches!(
        commands.try_recv().unwrap(),
        Command::ApplicationPlay(_)
    ));
    b.finish_remote_handoff();
    assert_eq!(b.active_backend, ActiveBackend::Remote("spotify".into()));
    assert!(b.spotify_resolution_worker.is_none());
    b.resolve_play(remote.as_ref()); // Spotify -> Spotify
    assert!(matches!(
        commands.try_recv().unwrap(),
        Command::ApplicationPlay(_)
    ));
    b.finish_remote_handoff();
    b.resolve_play(local.as_ref()); // Spotify -> Local waits for Pause ack
    assert!(matches!(
        commands.try_recv().unwrap(),
        Command::ApplicationPause
    ));
    assert_eq!(b.session.playback.state().status, PlaybackStatus::Stopped);
    b.finish_remote_handoff();
    assert_eq!(b.active_backend, ActiveBackend::Local);
    assert_eq!(b.session.playback.state().current_track(), Some(&local));
    assert_eq!(b.session.playback.state().status, PlaybackStatus::Playing);
    assert!(commands.try_recv().is_err());

    b.resolve_play(remote.as_ref());
    commands.try_recv().unwrap();
    b.finish_remote_handoff();
    b.session.error = "No usable local source; old Track error".into();
    b.resolve_play(local.as_ref());
    assert!(b.session.error.is_empty());
    commands.try_recv().unwrap();
    b.spotify_playback_state.error = Some(Error::Transport);
    b.finish_remote_handoff();
    assert_eq!(b.session.playback.state().status, PlaybackStatus::Stopped);
    assert!(matches!(b.active_backend, ActiveBackend::Remote(_))); // uncertain remote output retained
    assert!(b.route_message.contains("Local source is usable"));
    assert_eq!(b.session.error, b.route_message);
    assert!(!b.session.error.contains("No usable local source"));
    b.spotify_playback_state.error = None;
    b.resolve_play(local.as_ref());
    commands.try_recv().unwrap();
    b.finish_remote_handoff();
    b.session.diagnostics.borrow_mut().fail_next = true;
    b.resolve_play(remote.as_ref());
    assert!(commands.try_recv().is_err()); // failed local Stop never starts remote
    assert_eq!(b.session.playback.state().status, PlaybackStatus::Failed);

    b.session.playback.stop().unwrap();
    let unknown = b.session.rows[1].track_id.clone();
    let (catalog_worker, searches) = crate::spotify_resolution::Worker::fake();
    b.spotify_resolution_worker = Some(catalog_worker);
    b.spotify_playback_action("track".into(), unknown.as_ref().into());
    assert!(searches.try_recv().is_err()); // opening/selection never searches
    b.resolve_play(unknown.as_ref());
    let (_, input) = searches.try_recv().unwrap();
    assert!(searches.try_recv().is_err()); // exactly one explicit-Play search
    assert!(commands.try_recv().is_err());
    let song = Candidate {
        identity: identity.clone(),
        title: input.title.clone(),
        artist: input.artist.clone(),
        album: input.album.clone(),
        date: String::new(),
        duration_ms: 200000,
        disc: 1,
        number: input.number.unwrap(),
    };
    b.automatic_song_search = false;
    b.spotify_resolution_pending = false;
    b.finish_automatic_song(
        input,
        Page {
            items: vec![song],
            next_offset: None,
        },
    );
    assert!(matches!(
        commands.try_recv().unwrap(),
        Command::ApplicationPlay(_)
    ));
    b.finish_remote_handoff();
    assert_eq!(
        b.session
            .library
            .track_provider_occurrences(&unknown, "spotify")
            .unwrap(),
        vec![identity.clone()]
    );
    b.resolve_play(unknown.as_ref());
    assert!(searches.try_recv().is_err()); // persisted association bypasses search
    commands.try_recv().unwrap();
    b.finish_remote_handoff();

    let ambiguous = b.session.rows[4].track_id.clone();
    b.resolve_play(ambiguous.as_ref());
    let (_, input) = searches.try_recv().unwrap();
    let song = Candidate {
        identity: identity.clone(),
        title: input.title.clone(),
        artist: input.artist.clone(),
        album: input.album.clone(),
        date: String::new(),
        duration_ms: 200000,
        disc: 1,
        number: input.number.unwrap(),
    };
    let mut other = song.clone();
    other.identity.external_id = "2234567890123456789012".into();
    b.automatic_song_search = false;
    b.spotify_resolution_pending = false;
    b.finish_automatic_song(
        input,
        Page {
            items: vec![song, other],
            next_offset: None,
        },
    );
    assert!(b.spotify_resolution_selection.is_some());
    assert!(
        b.session
            .library
            .track_provider_occurrences(&ambiguous, "spotify")
            .unwrap()
            .is_empty()
    );
    assert!(commands.try_recv().is_err());
    b.spotify_resolve("confirm".into(), 1); // existing chooser, no second search
    assert!(searches.try_recv().is_err());
    b.resolve_play(ambiguous.as_ref());
    assert!(matches!(
        commands.try_recv().unwrap(),
        Command::ApplicationPlay(_)
    ));
    b.finish_remote_handoff();
    assert!(searches.try_recv().is_err());

    // A rejected first remote Play never claims ownership of audio it did not
    // start, and therefore cannot force a later local Play through Spotify Pause.
    b.resolve_play(local.as_ref());
    assert!(matches!(
        commands.try_recv().unwrap(),
        Command::ApplicationPause
    ));
    b.finish_remote_handoff();
    b.resolve_play(remote.as_ref());
    assert!(matches!(
        commands.try_recv().unwrap(),
        Command::ApplicationPlay(_)
    ));
    b.spotify_playback_state.error = Some(Error::ApiRejected(403));
    b.finish_remote_handoff();
    assert_eq!(b.active_backend, ActiveBackend::None);
    b.resolve_play(local.as_ref());
    assert_eq!(b.active_backend, ActiveBackend::Local);
    assert_eq!(b.session.playback.state().current_track(), Some(&local));
    assert!(b.session.error.is_empty());
    assert!(commands.try_recv().is_err());
    assert!(searches.try_recv().is_err());
}

impl Bridge {
    fn remote_unavailable(&self) -> Option<String> {
        let s = &self.spotify_playback_state;
        if s.authorization != AuthorizationState::Connected {
            return Some("Spotify playback not connected; open Spotify… and connect".into());
        }
        let Some(device) = s.selected_device.as_ref() else {
            return Some("No Spotify playback device selected; open Spotify…".into());
        };
        let Some(device) = s.devices.iter().find(|d| d.id.as_ref() == Some(device)) else {
            return Some(Error::DeviceUnavailable.to_string());
        };
        if device.is_restricted {
            return Some(Error::RestrictedDevice.to_string());
        }
        s.error
            .as_ref()
            .filter(|e| !matches!(e, Error::NoAssociation))
            .map(ToString::to_string)
    }

    pub(crate) fn resolve_play(&mut self, id: &str) {
        if self.route_pending.is_none() && !self.automatic_song_search {
            self.preserve_play_queue = false;
        }
        self.resolve_play_inner(id);
    }

    pub(crate) fn resolve_current_play(&mut self, id: &str) {
        if self.route_pending.is_none() && !self.automatic_song_search {
            self.preserve_play_queue = true;
        }
        self.resolve_play_inner(id);
    }

    fn resolve_play_inner(&mut self, id: &str) {
        // The fake-engine smoke exercises its historical virtual paths. Real audio
        // uses the filesystem resolver; resolver tests create actual temp files.
        if !self.real_audio {
            self.session.play_row(id);
            return;
        }
        if self.route_pending.is_some() || self.automatic_song_search {
            self.route_message =
                "Playback request in progress; wait or cancel the catalog selection".into();
            return;
        }
        let track = TrackId(id.into());
        // A new explicit request must not display the preceding Track's error
        // while its own route or backend handoff is being evaluated.
        self.session.error.clear();
        let capability = RemoteCapability {
            provider: "spotify",
            unavailable: self.remote_unavailable(),
            catalog_available: self.spotify_resolution_worker.is_some()
                || (std::env::var("SPOTIFY_CLIENT_ID").is_ok_and(|s| !s.is_empty())
                    && std::env::var("SPOTIFY_CLIENT_SECRET").is_ok_and(|s| !s.is_empty())
                    && std::env::var("SPOTIFY_MARKET").is_ok_and(|s| !s.is_empty())),
            accepts: |id| Song::from_associations(std::slice::from_ref(id)).is_ok(),
        };
        let route = match self.session.library.playback_route(&track, &capability) {
            Ok(route) => route,
            Err(e) => {
                self.route_message = e.to_string();
                self.session.error = self.route_message.clone();
                return;
            }
        };
        eprintln!("playback-route track={} decision={route:?}", track.as_ref());
        match route {
            Route::Local(source) => {
                if matches!(self.active_backend, ActiveBackend::Remote(_)) {
                    self.route_pending = Some(Pending::PauseRemote { track, source });
                    if !self.send_remote(Command::ApplicationPause) {
                        self.route_pending = None;
                    }
                } else {
                    self.start_resolved_local(track, source);
                }
            }
            Route::Remote(identity) => self.start_resolved_remote(track, identity),
            Route::NeedsEnrichment => {
                self.spotify_playback_action("track".into(), id.into());
                self.spotify_resolve("search".into(), -1);
                self.automatic_song_search = self.spotify_resolution_pending;
                self.route_message = self.spotify_resolution_message.clone();
            }
            Route::Unavailable(reason) => {
                self.route_message = reason;
                self.session.error = self.route_message.clone();
                self.spotify_playback_action("track".into(), id.into());
                self.resolver_dialog_requested();
            }
        }
    }

    fn select_application_track(&mut self, track: TrackId) -> bool {
        if self.preserve_play_queue && self.session.playback.state().current_track() == Some(&track)
        {
            if let Err(error) = self.session.playback.stop() {
                self.route_message = format!("Local backend stop failed: {error}");
                return false;
            }
            return true;
        }
        match self.session.playback.set_queue(vec![track.clone()]) {
            Ok(()) => {
                self.session.queue_labels = self
                    .session
                    .rows
                    .iter()
                    .filter(|r| r.track_id == track)
                    .cloned()
                    .collect();
                true
            }
            Err(e) => {
                self.route_message = format!("Local backend stop failed: {e}");
                false
            }
        }
    }

    fn start_resolved_local(&mut self, track: TrackId, source: PlayableSource) {
        if self.preserve_play_queue
            && self.session.playback.state().source.as_ref() == Some(&source)
            && matches!(
                self.session.playback.state().status,
                PlaybackStatus::Paused | PlaybackStatus::Playing
            )
        {
            self.session.command("play");
            self.route_message = if self.session.error.is_empty() {
                "Playing via Local".into()
            } else {
                self.session.error.clone()
            };
            return;
        }
        if !self.select_application_track(track) {
            return;
        }
        self.active_backend = ActiveBackend::Local;
        match self.session.playback.start_source(source) {
            Ok(()) => {
                self.route_message = "Playing via Local".into();
                self.session.error.clear();
            }
            Err(error) => {
                self.route_message = format!("Local playback engine failed: {error}");
                self.session.error = self.route_message.clone();
            }
        }
    }

    pub(crate) fn start_resolved_remote(&mut self, track: TrackId, identity: ExternalIdentity) {
        if let Some(reason) = self.remote_unavailable() {
            self.route_message = reason;
            return;
        }
        if !self.select_application_track(track.clone()) {
            return;
        }
        self.route_pending = Some(Pending::StopLocal { track, identity });
        self.continue_local_stop();
    }

    /// Do not start remote audio until GStreamer acknowledges Stop.
    pub(crate) fn continue_local_stop(&mut self) {
        if !matches!(self.route_pending, Some(Pending::StopLocal { .. })) {
            return;
        }
        let state = self.session.playback.state();
        if state.status == PlaybackStatus::Failed {
            self.route_pending = None;
            self.route_message = "Local engine failed to stop; remote playback withheld".into();
            return;
        }
        if state.pending.is_some() || state.status != PlaybackStatus::Stopped {
            return;
        }
        if let Some(Pending::StopLocal { track, identity }) = self.route_pending.take() {
            let Ok(song) = Song::from_associations(&[identity]) else {
                return;
            };
            self.spotify_playback_track = Some(track);
            self.spotify_playback_song = Some(song.clone());
            if !matches!(self.active_backend, ActiveBackend::Remote(_)) {
                self.active_backend = ActiveBackend::None;
            }
            self.route_pending = Some(Pending::StartRemote);
            if !self.send_remote(Command::ApplicationPlay(song)) {
                self.route_pending = None;
            }
        }
    }

    fn send_remote(&mut self, command: Command) -> bool {
        if self
            .spotify_playback_worker
            .as_ref()
            .is_some_and(|w| w.send(command))
        {
            self.route_message = "Switching playback backend…".into();
            true
        } else {
            self.route_message =
                "Spotify playback worker unavailable or busy; retry explicitly".into();
            false
        }
    }

    pub(crate) fn finish_remote_handoff(&mut self) {
        let pending = self.route_pending.take();
        if let Some(error) = &self.spotify_playback_state.error {
            self.route_message = if matches!(pending, Some(Pending::PauseRemote { .. })) {
                format!("Local source is usable, but Spotify could not be stopped: {error}")
            } else {
                format!("Spotify playback rejected: {error}")
            };
            self.session.error = self.route_message.clone();
            // Output may be unknown after a transport error: retain remote
            // ownership so a subsequent local switch must acknowledge Pause.
            if matches!(pending, Some(Pending::PauseRemote { .. }))
                || matches!(self.active_backend, ActiveBackend::Remote(_))
                || matches!(
                    error,
                    Error::Transport | Error::ServiceUnavailable | Error::InvalidResponse
                )
            {
                self.active_backend = ActiveBackend::Remote("spotify".into());
            }
            return;
        }
        match pending {
            Some(Pending::PauseRemote { track, source }) => {
                self.active_backend = ActiveBackend::None;
                self.start_resolved_local(track, source);
            }
            Some(Pending::StartRemote) => {
                self.active_backend = ActiveBackend::Remote("spotify".into());
                self.route_message = "Playing via Spotify (no usable local source)".into();
                self.session.error.clear();
            }
            _ => {}
        }
    }

    pub(crate) fn finish_automatic_song(&mut self, input: Input, page: Page<Candidate>) {
        match assess(&input, &page) {
            Assessment::Unique(index) => {
                let track = input.track_id.clone();
                let selection = Selection::new(input, page.items);
                match self
                    .session
                    .library
                    .confirm_song_resolution(&selection, index)
                {
                    Ok(_) => {
                        self.spotify_resolution_message =
                            "Unique Spotify song accepted and saved".into();
                        // Availability may have changed while HTTP was in flight.
                        self.resolve_play_inner(track.as_ref());
                    }
                    Err(error) => {
                        self.route_message = format!("Spotify association not accepted: {error}")
                    }
                }
            }
            Assessment::NeedsSelection => {
                self.route_message =
                    "Spotify match requires selection; no identity automatically accepted".into();
                self.spotify_resolution_message = self.route_message.clone();
                self.spotify_resolution_selection = Some(Selection::new(input, page.items));
                self.resolver_dialog_requested();
            }
            Assessment::NoMatch => {
                self.route_message = "No playable source; Spotify association not found".into();
                self.spotify_resolution_message = self.route_message.clone();
            }
        }
    }
}
