//! Candidate-scoped lazy inspection, separate from whole-library discovery.
use super::*;
use std::sync::mpsc;
#[derive(Default)]
pub(super) struct State {
    job: Option<mpsc::Receiver<Result<Programs, String>>>,
    requested: Option<ExternalIdentity>,
    attempted: Vec<ExternalIdentity>,
    pub(super) status: String,
    refreshed: Option<std::time::Instant>,
    discovery_requested: bool,
    was_running: bool,
}
impl State {
    pub(super) fn pending(&self) -> bool {
        self.job.is_some()
    }
}
impl Bridge {
    pub(super) fn connection_discovery_status(&self, target: &Target) -> String {
        if target.provider == "musicbrainz" {
            return self
                .session
                .library
                .musicbrainz_review(&target.album)
                .map(|r| {
                    if r.candidates.is_empty() {
                        r.reason.label().into()
                    } else {
                        format!("Retained candidates — {}", r.reason.label())
                    }
                })
                .unwrap_or_else(|e| e.to_string());
        }
        if target.provider != "spotify" || target.track.is_some() {
            return "No candidates found".into();
        }
        if self.browser.review_run_active
            || self
                .spotify_album_matcher
                .as_ref()
                .is_some_and(|m| m.pending_count() > 0)
        {
            return "Matching in progress…".into();
        }
        let status = self
            .session
            .library
            .spotify_album_review_lifecycle(&target.album)
            .unwrap_or_default();
        if status.contains("No candidates") {
            "No candidates found".into()
        } else if status.contains("Spotify") || status.contains("deferred") {
            status
        } else if self
            .spotify_review_report
            .as_ref()
            .is_some_and(|r| r.candidates.is_empty())
        {
            "Candidate search completed; no candidates found".into()
        } else {
            "No candidate search has completed yet".into()
        }
    }
    pub(super) fn sync_connection_program(&mut self, target: &Target) -> Result<(), String> {
        if target.provider != "spotify" || target.track.is_some() {
            return Ok(());
        }
        let path = self.session.library.database_path();
        let market = std::env::var("SPOTIFY_MARKET")
            .ok()
            .or_else(|| {
                self.session
                    .library
                    .retained_spotify_market()
                    .ok()
                    .flatten()
            })
            .unwrap_or_else(|| "US".into());
        let running = self.browser.review_run_active
            || self
                .spotify_album_matcher
                .as_ref()
                .is_some_and(|m| m.pending_count() > 0);
        let just_finished = self.connection_review_state.program.was_running && !running;
        self.connection_review_state.program.was_running = running;
        let refresh = self.spotify_review_report.is_none() || running || just_finished;
        if refresh
            && self.connection_review_state.inspection["manual_candidate"] != true
            && self.connection_review_state.inspection["manual_mapping"] != true
            && (just_finished
                || self
                    .connection_review_state
                    .program
                    .refreshed
                    .is_none_or(|t| t.elapsed() >= std::time::Duration::from_millis(750)))
        {
            self.connection_review_state.program.refreshed = Some(std::time::Instant::now());
            if let Some(path) = &path
                && let Ok(music_library::album_matching::Preparation::Ready(input)) = self
                    .session
                    .library
                    .spotify_album_review_input(&target.album)
                && let Ok(Some((report, programs))) =
                    music_library_spotify::Spotify::cached_album_connection_review(
                        path, &market, &input,
                    )
            {
                let selected = self
                    .spotify_review_report
                    .as_ref()
                    .and_then(|r| r.candidates.get(self.connection_review_state.index))
                    .map(|c| c.identity.clone());
                self.connection_review_state.index = selected
                    .and_then(|id| report.candidates.iter().position(|c| c.identity == id))
                    .unwrap_or(0);
                self.spotify_review_report = Some(report);
                self.spotify_review_programs = programs;
            }
        }
        if let Some(job) = &self.connection_review_state.program.job {
            match job.try_recv() {
                Ok(result) => {
                    self.connection_review_state.program.job = None;
                    match result {
                        Ok(p)
                            if Some(&p.album)
                                == self.connection_review_state.program.requested.as_ref() =>
                        {
                            self.spotify_review_programs
                                .retain(|old| old.album != p.album);
                            self.spotify_review_programs.push(p);
                            self.connection_review_state.program.status.clear();
                        }
                        Ok(_) => {
                            self.connection_review_state.program.status =
                                "Candidate tracks unavailable: mismatched provider response".into()
                        }
                        Err(error) => {
                            self.connection_review_state.program.status =
                                format!("Candidate tracks unavailable: {error}")
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(e) => {
                    self.connection_review_state.program.job = None;
                    self.connection_review_state.program.status = e.to_string();
                }
            }
        }
        let Some(candidate) = self
            .spotify_review_report
            .as_ref()
            .and_then(|r| r.candidates.get(self.connection_review_state.index))
            .map(|c| c.identity.clone())
        else {
            if !self.connection_review_state.program.discovery_requested
                && !self.browser.review_run_active
                && music_library_spotify::Spotify::from_env().is_ok()
            {
                self.connection_review_state.program.discovery_requested = true;
                let _ = self.spotify_review_action_impl("retry", target.album.as_ref(), 0);
            }
            return Ok(());
        };
        if !self
            .spotify_review_programs
            .iter()
            .any(|p| p.album == candidate)
        {
            if let Some(path) = &path
                && let Ok(Some(p)) = music_library_spotify::Spotify::cached_candidate_program(
                    path, &market, &candidate,
                )
            {
                self.spotify_review_programs.push(p);
            }
        }
        if let Some(p) = self
            .spotify_review_programs
            .iter()
            .find(|p| p.album == candidate)
        {
            let local = self
                .session
                .library
                .local_album_tracks(&target.album)
                .map_err(e_string)?;
            if let Some(row) = self
                .spotify_review_report
                .as_mut()
                .and_then(|r| r.candidates.iter_mut().find(|r| r.identity == candidate))
            {
                row.programs = p
                    .programs
                    .iter()
                    .map(|p| {
                        music_library::album_program::inspect_playable_program(
                            &local,
                            p,
                            self.connection_review_state.known["title"]
                                .as_str()
                                .unwrap_or(""),
                        )
                    })
                    .collect();
            }
            self.connection_review_state.program.status.clear();
        } else if self.connection_review_state.program.job.is_none()
            && !self
                .connection_review_state
                .program
                .attempted
                .contains(&candidate)
        {
            self.connection_review_state
                .program
                .attempted
                .push(candidate.clone());
            self.connection_review_state.program.requested = Some(candidate.clone());
            let wait = self
                .session
                .library
                .spotify_cooldown_remaining(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64,
                )
                .map_err(e_string)?;
            if wait > 0 {
                self.connection_review_state.program.status =
                    format!("Waiting for provider cooldown ({wait} seconds)");
            } else if let Err(e) = music_library_spotify::Spotify::from_env() {
                self.connection_review_state.program.status =
                    format!("Candidate tracks unavailable: {e}");
            } else {
                let (send, recv) = mpsc::channel();
                self.connection_review_state.program.job = Some(recv);
                self.connection_review_state.program.status = "Loading candidate tracks…".into();
                std::thread::spawn(move || {
                    use music_library::catalog::CatalogProvider;
                    let result = music_library_spotify::Spotify::from_env()
                        .and_then(|p| p.with_library(path.as_deref()))
                        .and_then(|mut p| p.album_programs(&candidate))
                        .map_err(e_string);
                    let _ = send.send(result);
                });
            }
        }
        if self.connection_review_state.inspection["manual_mapping"] != true {
            let manual = self.connection_review_state.inspection["manual_candidate"].clone();
            self.connection_review_state.inspection =
                serde_json::from_str(&self.spotify_review_action_impl(
                    "inspect",
                    target.album.as_ref(),
                    self.connection_review_state.index as i32,
                ))
                .map_err(e_string)?;
            self.connection_review_state.inspection["manual_candidate"] = manual;
        }
        Ok(())
    }
}

#[cfg(test)]
impl Bridge {
    pub(crate) fn test_candidate_program_reply(&mut self, programs: Programs) {
        let (send, recv) = mpsc::channel();
        self.connection_review_state.program.requested = Some(programs.album.clone());
        send.send(Ok(programs)).unwrap();
        self.connection_review_state.program.job = Some(recv);
    }
}
