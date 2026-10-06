//! One application seek/volume entry point; QML never selects a backend.
use crate::{
    Bridge,
    spotify_playback::{Command, Update},
};
use music_library::{playback::Volume, playback_resolver::ActiveBackend};
use std::time::Instant;

#[derive(Default)]
pub struct Controls {
    revision: u64,
    pub trims: music_library::output::OutputTrims,
    volume: Option<u64>,
    seek: Option<(u64, u64)>,
    observed: Option<Instant>,
}
impl Controls {
    pub fn new(trims: music_library::output::OutputTrims) -> Self {
        Self {
            trims,
            ..Self::default()
        }
    }
}
impl Bridge {
    fn remote_controls(&self) -> bool {
        matches!(self.active_backend, ActiveBackend::Remote(_))
    }
    fn remote_device(&self) -> Option<&music_library_spotify::playback::Device> {
        let s = &self.spotify_playback_state;
        s.state
            .device
            .as_ref()
            .filter(|d| d.id == s.selected_device)
            .or_else(|| s.devices.iter().find(|d| d.id == s.selected_device))
    }
    pub fn player_volume_available(&self) -> bool {
        self.route_pending.is_none()
            && (!self.remote_controls()
                || self
                    .remote_device()
                    .is_some_and(|d| d.supports_volume && !d.is_restricted))
    }
    pub fn player_volume_value(&self) -> f64 {
        self.session.playback.state().volume.get()
    }
    pub fn remote_output_percent(&self) -> u8 {
        (music_library::output::effective(
            self.session.playback.state().volume,
            self.controls.trims.spotify_db,
        )
        .get()
            * 100.)
            .round() as u8
    }
    pub fn set_output_trims(&mut self, local: f64, spotify: f64) {
        let trims = music_library::output::OutputTrims {
            local_db: local,
            spotify_db: spotify,
        };
        if let Err(error) = self.session.library.set_output_trims(trims) {
            self.session.error = error.to_string();
            return;
        }
        self.controls.trims = trims;
        if let Err(error) = self.session.playback.set_output_trim(local) {
            self.session.error = error.to_string();
            return;
        }
        self.player_volume(self.player_volume_value());
    }
    pub fn player_volume(&mut self, value: f64) {
        if let Err(error) = Volume::new(value) {
            self.session.error = error.to_string();
            return;
        }
        self.session.set_volume(value);
        if !self.session.error.is_empty() || !self.remote_controls() {
            return;
        }
        if !self.player_volume_available() {
            self.session.error =
                "This playback device does not currently support volume control".into();
            return;
        }
        let percent = self.remote_output_percent();
        self.controls.revision += 1;
        let revision = self.controls.revision;
        if self
            .spotify_playback_worker
            .as_ref()
            .is_some_and(|w| w.send(Command::Volume(percent, revision)))
        {
            self.controls.volume = Some(revision);
            self.session.error.clear();
        } else {
            self.session.error = "Playback device is busy; please retry volume".into();
        }
    }
    pub fn player_clock(&self) -> (u64, Option<u64>, bool) {
        if self.remote_controls() {
            let state = &self.spotify_playback_state.state;
            let same = self.spotify_active_song.as_ref().is_some_and(|song| {
                Some(song.uri())
                    == state
                        .track_id
                        .as_ref()
                        .map(|id| format!("spotify:track:{id}"))
            });
            if !same {
                return (0, None, false);
            }
            let progress = self
                .controls
                .seek
                .filter(|(g, _)| *g == self.playback_generation)
                .map_or_else(
                    || {
                        state.progress_ms.saturating_add(if state.playing {
                            self.controls
                                .observed
                                .map_or(0, |t| t.elapsed().as_millis() as u64)
                        } else {
                            0
                        })
                    },
                    |(_, ms)| ms,
                );
            (
                state.duration_ms.map_or(progress, |d| progress.min(d)),
                state.duration_ms,
                self.route_pending.is_none() && state.duration_ms.is_some_and(|d| d > 0),
            )
        } else {
            let s = self.session.playback.state();
            (
                s.pending_seek_ms.unwrap_or(s.media_position_ms),
                s.duration_ms,
                self.route_pending.is_none()
                    && s.pending.is_none()
                    && s.source.is_some()
                    && s.duration_ms.is_some_and(|d| d > 0),
            )
        }
    }
    pub fn player_seek(&mut self, value: f64) {
        if !value.is_finite() || value < 0. {
            self.session.error = "Invalid seek position".into();
            return;
        }
        let (_, duration, available) = self.player_clock();
        if !available {
            return;
        }
        let position = (value as u64).min(duration.unwrap_or(u64::MAX).saturating_sub(1));
        if self.remote_controls() {
            self.playback_generation += 1;
            if self
                .spotify_playback_worker
                .as_ref()
                .is_some_and(|w| w.send(Command::Seek(position, self.playback_generation)))
            {
                self.controls.seek = Some((self.playback_generation, position));
                self.session.error.clear();
            } else {
                self.session.error = "Playback device is busy; please retry seeking".into();
            }
        } else {
            self.session.error = self
                .session
                .playback
                .seek(position)
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
        }
    }
    pub fn apply_player_update(&mut self, update: Update) {
        let playlist_authorized = self.spotify_playback_state.authorization
            == music_library_spotify::playback::AuthorizationState::Authorizing
            && update.snapshot.authorization
                == music_library_spotify::playback::AuthorizationState::Connected;
        let old = &self.spotify_playback_state.state;
        let new = &update.snapshot.state;
        if old.track_id != new.track_id
            || old.progress_ms != new.progress_ms
            || old.playing != new.playing
        {
            self.controls.observed = Some(Instant::now());
        }
        if let Some((generation, _)) = self.controls.seek
            && generation == update.generation
        {
            self.controls.seek = None;
            if let Some(error) = &update.snapshot.error {
                self.session.error = error.to_string();
            }
        }
        if let Some(revision) = &self.controls.volume
            && update.volume_ack == Some(*revision)
        {
            if let Some(error) = &update.snapshot.error {
                self.session.error = error.to_string();
            }
            self.controls.volume = None;
        }
        let old_device = &self.spotify_playback_state.state.device;
        let new_device = &update.snapshot.state.device;
        if self.remote_controls()
            && self.route_pending.is_none()
            && self.controls.volume.is_none()
            && update.volume_ack.is_none()
            && !update.application_command
            && let (Some(old), Some(new)) = (old_device, new_device)
            && old.id == new.id
            && new.id == update.snapshot.selected_device
            && old.volume_percent != new.volume_percent
            && let Some(percent) = new.volume_percent
        {
            let gain = 10_f64.powf(self.controls.trims.spotify_db / 20.);
            self.session
                .set_volume((f64::from(percent) / 100. / gain).clamp(0., 1.));
        }
        self.spotify_playback_state = update.snapshot;
        self.resume_spotify_playlist(playlist_authorized);
        if update.application_command {
            self.finish_remote_handoff();
        }
        self.observe_remote_completion(update.generation, update.completion);
        self.spotify_playback_changed();
        self.changed();
    }
}

#[cfg(all(test, feature = "gstreamer"))]
pub(crate) fn test_controls(
    bridge: &qmetaobject::QObjectBox<Bridge>,
    engine: &mut qmetaobject::QmlEngine,
) {
    use music_library::playback::{EngineEvent, EngineEventKind};
    use music_library_spotify::playback::{AuthorizationState, Device, Song};
    let pinned = bridge.pinned();
    {
        let mut b = pinned.borrow_mut();
        b.session.shutdown_audio();
        let track = b.session.rows[2].track_id.clone();
        b.session.playback.enqueue(track);
        b.session.command("play");
        b.session.engine_event(EngineEvent {
            generation: 1,
            kind: EngineEventKind::Duration(Some(120000)),
        });
        b.changed();
    }
    assert_eq!(
        engine
            .invoke_method("exercisePlayerControls".into(), &[])
            .to_qstring()
            .to_string(),
        "ok"
    );
    {
        let b = pinned.borrow();
        assert!((50000..70000).contains(&b.session.playback.state().media_position_ms));
        assert_eq!(b.session.playback.state().volume.get(), 0.35);
    }
    let (worker, commands) = crate::spotify_playback::Worker::fake();
    {
        let mut b = pinned.borrow_mut();
        b.session.command("stop");
        b.real_audio = true;
        b.active_backend = ActiveBackend::Remote("spotify".into());
        b.spotify_playback_worker = Some(worker);
        b.spotify_active_song = Some(
            Song::from_associations(&[music_library::domain::ExternalIdentity {
                provider: "spotify".into(),
                kind: "track".into(),
                external_id: "1234567890123456789012".into(),
            }])
            .unwrap(),
        );
        b.spotify_playback_state.authorization = AuthorizationState::Connected;
        b.spotify_playback_state.selected_device = Some("desktop".into());
        b.spotify_playback_state.state.device = Some(Device {
            id: Some("desktop".into()),
            name: "Desktop".into(),
            kind: "Computer".into(),
            is_active: true,
            is_restricted: false,
            supports_volume: true,
            volume_percent: Some(80),
        });
        b.spotify_playback_state.state.track_id = Some("1234567890123456789012".into());
        b.spotify_playback_state.state.duration_ms = Some(120000);
        b.changed();
        let clock = b.player_clock();
        let playing = b.session.playback.state().current_track().cloned();
        let selected = b
            .session
            .rows
            .iter()
            .find(|r| Some(&r.track_id) != playing.as_ref())
            .unwrap()
            .track_id
            .clone();
        b.spotify_playback_action("track".into(), selected.as_ref().into());
        assert_eq!(b.spotify_playback_track.as_ref(), Some(&selected));
        assert_eq!(
            b.player_clock(),
            clock,
            "diagnostic selection preserves remote clock and seek"
        );
        assert_eq!(b.session.playback.state().current_track(), playing.as_ref());
        assert!(
            commands.try_recv().is_err(),
            "inspection sends no playback commands"
        );
    }
    assert_eq!(
        engine
            .invoke_method("exercisePlayerControls".into(), &[])
            .to_qstring()
            .to_string(),
        "ok"
    );
    let Command::Seek(position, generation) = commands.try_recv().unwrap() else {
        panic!("seek not routed")
    };
    assert!(
        (50000..70000).contains(&position),
        "seek position {position}"
    );
    let Command::Volume(percent, revision) = commands.try_recv().unwrap() else {
        panic!("volume not routed")
    };
    assert_eq!(percent, 35);
    let mut b = pinned.borrow_mut();
    assert_eq!(b.player_volume_value(), 0.35);
    let mut snapshot = b.spotify_playback_state.clone();
    snapshot.state.device.as_mut().unwrap().volume_percent = Some(80);
    b.apply_player_update(Update {
        snapshot: snapshot.clone(),
        generation: generation.saturating_sub(1),
        volume_ack: None,
        application_command: false,
        completion: None,
    });
    assert_eq!(b.player_volume_value(), 0.35); // pre-command poll cannot undo intent
    assert_eq!(b.player_clock().0, position);
    snapshot.state.device.as_mut().unwrap().volume_percent = Some(35);
    snapshot.state.progress_ms = position;
    b.apply_player_update(Update {
        snapshot: snapshot.clone(),
        generation,
        volume_ack: Some(revision),
        application_command: false,
        completion: None,
    });
    assert_eq!(b.player_volume_value(), 0.35);
    assert!(b.controls.volume.is_none());
    assert!(b.controls.seek.is_none());
    b.player_volume(0.7);
    let Command::Volume(_, revision) = commands.try_recv().unwrap() else {
        panic!("volume")
    };
    snapshot.error = Some(music_library_spotify::playback::Error::RateLimited(5));
    b.apply_player_update(Update {
        snapshot,
        generation,
        volume_ack: Some(revision),
        application_command: false,
        completion: None,
    });
    assert_eq!(b.player_volume_value(), 0.7);
    assert!(!b.session.error.is_empty());
    // External changes on the stable selected device still update logical volume.
    let mut external = b.spotify_playback_state.clone();
    external.error = None;
    external.state.device.as_mut().unwrap().volume_percent = Some(25);
    b.apply_player_update(Update {
        snapshot: external.clone(),
        generation,
        volume_ack: None,
        application_command: false,
        completion: None,
    });
    assert_eq!(b.player_volume_value(), 0.25);
    b.set_output_trims(-6., -6.);
    let Command::Volume(percent, revision) = commands.try_recv().unwrap() else {
        panic!("trim output");
    };
    assert_eq!(percent, 13);
    assert_eq!(b.player_volume_value(), 0.25);
    // A stale poll during the trim request cannot reinterpret old output as master.
    external.state.device.as_mut().unwrap().volume_percent = Some(35);
    b.apply_player_update(Update {
        snapshot: external.clone(),
        generation,
        volume_ack: None,
        application_command: false,
        completion: None,
    });
    assert_eq!(b.player_volume_value(), 0.25);
    external.state.device.as_mut().unwrap().volume_percent = Some(13);
    b.apply_player_update(Update {
        snapshot: external.clone(),
        generation,
        volume_ack: Some(revision),
        application_command: false,
        completion: None,
    });
    assert_eq!(b.player_volume_value(), 0.25); // integer device rounding never feeds back on ack
    external.state.device.as_mut().unwrap().volume_percent = Some(20);
    b.apply_player_update(Update {
        snapshot: external,
        generation,
        volume_ack: None,
        application_command: false,
        completion: None,
    });
    assert!((b.player_volume_value() - 0.2 / 10_f64.powf(-6. / 20.)).abs() < 0.00001);
    b.active_backend = ActiveBackend::Local;
    b.set_output_trims(0., 0.);
    b.player_volume(0.7);
    assert_eq!(b.player_volume_value(), 0.7);
    assert_eq!(b.player_clock(), (0, None, false)); // clock reset across handoff
    b.spotify_playback_worker.take();
    b.real_audio = false;
    b.changed();
}
