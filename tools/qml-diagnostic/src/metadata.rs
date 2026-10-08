//! Metadata dialog state. File work runs on a separate Library connection/thread.
use crate::Bridge;
use music_library::metadata::{Change, Inspection, RenameMatch, SaveOutcome, Target};
use qmetaobject::QPointer;
#[derive(Default)]
pub struct State {
    pub inspection: Option<Inspection>,
    pub message: String,
    pub busy: bool,
    pub save_revision: u64,
    pub rename_matches: Vec<RenameMatch>,
    pub artist_choices: Vec<music_library::metadata::ArtistChoice>,
    pending_save: Option<String>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Bridge {
    pub fn metadata_value(&self) -> qmetaobject::QString {
        serde_json::json!({"inspection":self.metadata_state.inspection,"message":self.metadata_state.message,"busy":self.metadata_state.busy,"saveRevision":self.metadata_state.save_revision,"renameMatches":self.metadata_state.rename_matches,"artistChoices":self.metadata_state.artist_choices}).to_string().into()
    }
    pub fn metadata_action_impl(&mut self, action: &str, kind: &str, id: String, payload: String) {
        if self.metadata_state.busy {
            return;
        }
        if action == "artist-lookup" {
            self.metadata_state.artist_choices = self
                .session
                .library
                .metadata_artist_choices(&payload)
                .unwrap_or_default();
            self.metadata_changed();
            return;
        }
        if action.starts_with("rename-") {
            let Some(payload) = self.metadata_state.pending_save.take() else {
                return;
            };
            if action == "rename-move"
                && !self
                    .metadata_state
                    .rename_matches
                    .iter()
                    .any(|m| m.album_id == id && m.blocked_reason.is_empty())
            {
                return;
            }
            self.metadata_state.rename_matches.clear();
            if action == "rename-cancel" {
                self.metadata_state.message.clear();
                self.metadata_changed();
                return;
            }
            let mut request: serde_json::Value = serde_json::from_str(&payload).unwrap();
            if action == "rename-move" {
                request["move_to"] = id.into();
            } else if action == "rename-keep" {
                request["keep_separate"] = true.into();
            } else {
                return;
            }
            self.metadata_action_impl("save", "", String::new(), request.to_string());
            return;
        }
        if action == "connect-spotify" {
            let permitted = self
                .metadata_state
                .inspection
                .as_ref()
                .is_some_and(|inspection| {
                    inspection.target == Target::Track(id.clone())
                        || inspection
                            .track_evidence
                            .iter()
                            .any(|group| group.track_id == id)
                });
            if !permitted {
                return;
            }
            match self.session.library.connect_metadata_spotify_candidate(
                &music_library::domain::TrackId(id.clone()),
                &payload,
            ) {
                Ok(identity) => {
                    if self
                        .spotify_playback_track
                        .as_ref()
                        .is_some_and(|track| track.0 == id)
                    {
                        self.spotify_playback_song =
                            music_library_spotify::playback::Song::from_associations(&[identity])
                                .ok();
                    }
                    self.metadata_state.message = "Spotify candidate connected ✓".into();
                    if let Some(inspection) = &self.metadata_state.inspection {
                        self.metadata_state.inspection = self
                            .session
                            .library
                            .inspect_metadata(&inspection.target)
                            .ok();
                    }
                    self.metadata_refresh_browse();
                    self.spotify_playback_changed();
                }
                Err(e) => self.metadata_state.message = e.to_string(),
            }
            self.metadata_changed();
            return;
        }
        if action == "open" {
            self.metadata_state.rename_matches.clear();
            self.metadata_state.pending_save = None;
            let target = match kind {
                "track" => Target::Track(id),
                "album" => Target::Album(id),
                _ => return,
            };
            self.metadata_state.message.clear();
            match self.session.library.inspect_metadata(&target) {
                Ok(v) => self.metadata_state.inspection = Some(v),
                Err(e) => {
                    self.metadata_state.inspection = None;
                    self.metadata_state.message = e.to_string();
                }
            }
            self.metadata_changed();
            return;
        }
        if action != "save" {
            return;
        }
        let Some(inspection) = &self.metadata_state.inspection else {
            return;
        };
        let target = inspection.target.clone();
        #[derive(serde::Deserialize)]
        struct Request {
            changes: Vec<Change>,
            files: Vec<String>,
            #[serde(default)]
            move_to: Option<String>,
            #[serde(default)]
            keep_separate: bool,
        }
        let request: Request = match serde_json::from_str(&payload) {
            Ok(v) => v,
            Err(e) => {
                self.metadata_state.message = e.to_string();
                self.metadata_changed();
                return;
            }
        };
        if request.move_to.is_none() && !request.keep_separate {
            match self
                .session
                .library
                .metadata_album_rename_matches(&target, &request.changes)
            {
                Ok(matches) if !matches.is_empty() => {
                    self.metadata_state.rename_matches = matches;
                    self.metadata_state.pending_save = Some(payload);
                    self.metadata_state.message =
                        "Choose whether to move Tracks to an existing Album.".into();
                    self.metadata_changed();
                    return;
                }
                Ok(_) => {}
                Err(e) => {
                    self.metadata_state.message = e.to_string();
                    self.metadata_changed();
                    return;
                }
            }
        }
        let worker = match self.session.library.spotify_review_worker() {
            Ok(Some(w)) => w,
            Ok(None) => {
                let result = self
                    .session
                    .library
                    .save_metadata_with_album_move(
                        &target,
                        &request.changes,
                        &request.files,
                        request.move_to.as_deref(),
                    )
                    .map_err(|e| e.to_string());
                self.finish_metadata(target, result);
                return;
            }
            Err(e) => {
                self.metadata_state.message = e.to_string();
                self.metadata_changed();
                return;
            }
        };
        let pointer = QPointer::from(&*self);
        let result_target = target.clone();
        let deliver = qmetaobject::queued_callback(move |result: Result<SaveOutcome, String>| {
            if let Some(object) = pointer.as_pinned() {
                let mut b = object.borrow_mut();
                if let Some(w) = b.metadata_state.worker.take() {
                    let _ = w.join();
                }
                b.finish_metadata(result_target.clone(), result);
            }
        });
        self.metadata_state.busy = true;
        self.metadata_state.message = "Saving Library metadata…".into();
        self.metadata_changed();
        self.metadata_state.worker = Some(std::thread::spawn(move || {
            let mut library = worker;
            deliver(
                library
                    .save_metadata_with_album_move(
                        &target,
                        &request.changes,
                        &request.files,
                        request.move_to.as_deref(),
                    )
                    .map_err(|e| e.to_string()),
            );
        }));
    }
    fn finish_metadata(&mut self, target: Target, result: Result<SaveOutcome, String>) {
        self.metadata_state.busy = false;
        match result {
            Ok(outcome) => {
                self.metadata_state.save_revision += 1;
                let updated = outcome.files.iter().filter(|f| f.error.is_none()).count();
                let failures: Vec<_> = outcome
                    .files
                    .iter()
                    .filter_map(|f| f.error.as_ref().map(|e| format!("{}: {e}", f.path)))
                    .collect();
                self.metadata_state.message =
                    format!("Library metadata saved ✓ ({} Tracks)", outcome.tracks);
                if !outcome.files.is_empty() {
                    self.metadata_state.message.push_str(&format!(
                        "\nLocal tags: {updated} updated, {} failed",
                        failures.len()
                    ));
                }
                if !failures.is_empty() {
                    self.metadata_state
                        .message
                        .push_str(&format!("\n{}", failures.join("\n")));
                }
                let display_target = if let Some(destination) = &outcome.destination_album {
                    if let Target::Album(source) = &target {
                        self.metadata_move_album_context(source, destination);
                    }
                    self.metadata_state
                        .message
                        .push_str("\nTracks moved to the selected existing Album ✓");
                    Target::Album(destination.clone())
                } else {
                    target
                };
                self.metadata_state.inspection =
                    self.session.library.inspect_metadata(&display_target).ok();
                // Refresh affected labels in one set query; preserve queue/resolver state.
                if !self.session.queue_labels.is_empty()
                    && let Ok(labels) = self
                        .session
                        .library
                        .metadata_track_labels(&outcome.track_ids)
                {
                    let labels = labels
                        .into_iter()
                        .map(|l| (l.track_id.clone(), l))
                        .collect::<std::collections::HashMap<_, _>>();
                    for label in &mut self.session.queue_labels {
                        if let Some(updated) = labels.get(&label.track_id) {
                            *label = updated.clone();
                        }
                    }
                }
                self.metadata_refresh_browse();
                self.refresh_local_search();
                self.changed();
            }
            Err(e) => self.metadata_state.message = e,
        }
        self.metadata_changed();
    }
}
impl Drop for State {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
