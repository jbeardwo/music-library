//! Conservative observation of an application-owned single-song request.
//! Empty responses are not EOS unless preceded by fresh, near-end playback.
use crate::playback::State;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Completed,
    Interrupted,
}

pub struct Detector {
    song: String,
    seen: bool,
    device: Option<String>,
    near_end: Option<(u64, u64)>,
    empty: u8,
    finished: bool,
}
impl Detector {
    pub fn new(uri: &str) -> Self {
        Self {
            song: uri.trim_start_matches("spotify:track:").into(),
            seen: false,
            device: None,
            near_end: None,
            empty: 0,
            finished: false,
        }
    }
    pub fn reset_evidence(&mut self) {
        self.near_end = None;
        self.empty = 0;
    }
    pub fn observe(&mut self, state: &State, now: u64) -> Option<Outcome> {
        if self.finished {
            return None;
        }
        let device = state.device.as_ref().and_then(|d| d.id.as_ref());
        if self.seen && device != self.device.as_ref() {
            self.reset_evidence();
            if device.is_some() {
                self.finished = true;
                return Some(Outcome::Interrupted);
            }
            return None; // Device loss / HTTP 204 cannot prove completion.
        }
        if let Some(id) = &state.track_id {
            self.empty = 0;
            if id != &self.song {
                self.reset_evidence();
                if self.seen {
                    self.finished = true;
                    return Some(Outcome::Interrupted);
                }
                return None; // eventual consistency immediately after Play
            }
            self.seen = true;
            self.device = device.cloned();
            if self.near_end.is_some()
                && state.playing
                && state
                    .duration_ms
                    .is_some_and(|duration| duration.saturating_sub(state.progress_ms) > 6000)
            {
                self.finished = true;
                return Some(Outcome::Interrupted); // repeat or external seek, not proven EOS
            }
            self.near_end = state
                .duration_ms
                .filter(|d| {
                    state.playing
                        && *d > 0
                        && state.progress_ms <= *d
                        && d.saturating_sub(state.progress_ms) <= 6000
                })
                .map(|d| (now, now + d.saturating_sub(state.progress_ms)));
            return None; // Paused, even at duration, is never EOS.
        }
        if !state.playing
            && device.is_some()
            && let Some((observed, end)) = self.near_end
        {
            if now.saturating_sub(observed) > 15000 {
                self.reset_evidence();
            } else if now >= end {
                self.empty += 1;
                if self.empty >= 2 {
                    self.finished = true;
                    return Some(Outcome::Completed);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn playing() -> State {
        State {
            track_id: Some("song".into()),
            playing: true,
            progress_ms: 98000,
            duration_ms: Some(100000),
            device: Some(crate::playback::Device {
                id: Some("desktop".into()),
                name: "Desktop".into(),
                kind: "Computer".into(),
                is_active: true,
                is_restricted: false,
                supports_volume: false,
            }),
            ..Default::default()
        }
    }
    #[test]
    fn terminal_evidence_is_consumed_once() {
        let mut d = Detector::new("spotify:track:song");
        let terminal = State {
            device: playing().device,
            ..Default::default()
        };
        assert_eq!(d.observe(&playing(), 0), None);
        assert_eq!(d.observe(&terminal, 2000), None);
        assert_eq!(d.observe(&terminal, 7000), Some(Outcome::Completed));
        assert_eq!(d.observe(&terminal, 12000), None);
    }
    #[test]
    fn pause_error_stale_or_missing_state_cannot_finish() {
        for mode in 0..4 {
            let mut d = Detector::new("song");
            d.observe(&playing(), 0);
            match mode {
                0 => {
                    let mut s = playing();
                    s.playing = false;
                    d.observe(&s, 1000);
                }
                1 => d.reset_evidence(),
                2 => {
                    d.observe(&State::default(), 20000);
                }
                _ => {
                    d = Detector::new("song");
                }
            }
            assert_eq!(d.observe(&State::default(), 21000), None);
            assert_eq!(d.observe(&State::default(), 26000), None);
        }
    }
    #[test]
    fn external_song_interrupts_instead_of_advancing() {
        let mut d = Detector::new("song");
        d.observe(&playing(), 0);
        let mut external = playing();
        external.track_id = Some("other".into());
        assert_eq!(d.observe(&external, 5000), Some(Outcome::Interrupted));
        assert_eq!(d.observe(&State::default(), 10000), None);
    }

    #[test]
    fn repeat_reset_and_device_change_are_not_completion() {
        for device_change in [false, true] {
            let mut d = Detector::new("song");
            d.observe(&playing(), 0);
            let mut changed = playing();
            if device_change {
                changed.device.as_mut().unwrap().id = Some("phone".into());
            } else {
                changed.progress_ms = 100;
            }
            assert_eq!(d.observe(&changed, 5000), Some(Outcome::Interrupted));
        }
    }
}
