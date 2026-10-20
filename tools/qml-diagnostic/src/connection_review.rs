//! Shared offline review presentation. Provider persistence and ranking remain owned by providers.
#[path = "connection_review_manual.rs"]
mod manual;
#[path = "connection_review_program.rs"]
mod program;
use crate::Bridge;
use music_library::{
    album_program::{Program, Programs},
    domain::{AlbumId, ExternalIdentity, TrackId},
    edition::TrackEvidence,
};
use serde_json::{Value, json};
#[derive(Clone)]
pub struct Target {
    pub provider: String,
    pub album: AlbumId,
    pub track: Option<TrackId>,
}
pub struct FileCandidate {
    pub track: TrackId,
    pub path: std::path::PathBuf,
    pub metadata: music_library::domain::ObservedMetadata,
}
#[derive(Default)]
pub struct State {
    pub target: Option<Target>,
    pub generation: u64,
    pub index: usize,
    known: Value,
    provider_header: Value,
    inspection: Value,
    programs: Option<Programs>,
    retained_candidates: Vec<music_library::musicbrainz_lifecycle::Candidate>,
    recordings: Vec<TrackEvidence>,
    recording_best: Option<ExternalIdentity>,
    file: Option<FileCandidate>,
    file_job: Option<std::sync::mpsc::Receiver<(u64, Result<FileCandidate, String>)>>,
    pub error: String,
    pub completed: bool,
    track_generation: u64,
    manual: manual::State,
    manual_recording: Option<TrackEvidence>,
    program: program::State,
}
fn field(label: &str, library: Value, candidate: Value) -> Value {
    json!({"label":label,"library":library,"candidate":candidate,"status":""})
}
fn rows_from_comparison(programs: &Value) -> Vec<Value> {
    programs.as_array().into_iter().flatten().flat_map(|p|p["rows"].as_array().into_iter().flatten()).take(200).map(|r|json!({"library":r["local"],"candidate":r["candidate"],"status":r["decision"].as_str().unwrap_or("").replace("local only","Library only").replace("local-only","Library-only")})).collect()
}
pub fn release_program(release: &music_library::catalog::Release) -> Programs {
    Programs {
        album: release.album.identity.clone(),
        note: "Explicitly inspected MusicBrainz Release".into(),
        programs: vec![Program {
            identity: Some(release.identity.clone()),
            complete: false,
            tracks: release
                .media
                .iter()
                .flat_map(|m| {
                    m.tracks.iter().map(move |t| TrackEvidence {
                        title: Some(t.title.clone()),
                        disc: Some(m.position),
                        number: Some(t.position),
                        duration_ms: t.duration.map(|d| d.milliseconds),
                        artists: t
                            .credits
                            .iter()
                            .map(|c| music_library::edition::ArtistEvidence {
                                name: c.name.clone(),
                                identities: c.identity.clone().into_iter().collect(),
                                join_phrase: c.join_phrase.clone(),
                            })
                            .collect(),
                        recording: music_library::edition::RecordingEvidence {
                            identities: t
                                .identities
                                .iter()
                                .filter(|i| i.kind == "recording")
                                .cloned()
                                .collect(),
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                })
                .collect(),
        }],
    }
}
impl Bridge {
    pub fn finish_connection_review(
        &mut self,
        provider: &str,
        track: Option<&TrackId>,
        album: Option<&AlbumId>,
        generation: Option<u64>,
    ) {
        let Some(target) = &self.connection_review_state.target else {
            return;
        };
        if target.provider != provider
            || generation.is_some_and(|g| g != self.connection_review_state.generation)
        {
            return;
        }
        let matches = if let Some(track) = track {
            target
                .track
                .as_ref()
                .map(|id| id == track)
                .unwrap_or_else(|| {
                    self.session.library.album_id_for_track(track).ok().as_ref()
                        == Some(&target.album)
                })
        } else {
            album == Some(&target.album) && target.track.is_none()
        };
        if matches {
            self.connection_review_state.completed = true;
            self.connection_review_finished();
        }
    }
    pub fn connection_review_impl(&mut self, action: &str, payload: &str, index: i32) -> String {
        let result = self.connection_review_result(action, payload, index);
        match result {
            Ok(v) => v.to_string(),
            Err(error) => {
                self.connection_review_state.error = error.clone();
                json!({"error":error}).to_string()
            }
        }
    }
    fn connection_review_result(
        &mut self,
        action: &str,
        payload: &str,
        index: i32,
    ) -> Result<Value, String> {
        if action == "cancel" {
            self.connection_review_state.target = None;
            self.connection_review_state.file_job = None;
            self.spotify_review_generation = self.spotify_review_generation.wrapping_add(1);
            self.spotify_resolution_generation = self.spotify_resolution_generation.wrapping_add(1);
            self.spotify_resolution_pending = false;
            self.reset_connection_catalog_review(None);
            return Ok(json!({}));
        }
        if action == "open" {
            let request: Value = serde_json::from_str(payload).map_err(|e| e.to_string())?;
            let provider = request["provider"]
                .as_str()
                .ok_or("Choose a provider")?
                .to_owned();
            if !music_library::connections::PROVIDERS
                .iter()
                .any(|p| p.key == provider)
            {
                return Err("Unknown provider".into());
            }
            let album = AlbumId(request["album"].as_str().ok_or("Choose an Album")?.into());
            let track = request["track"]
                .as_str()
                .filter(|t| !t.is_empty())
                .map(|t| TrackId(t.into()));
            let known = self
                .session
                .library
                .connection_review_known(&album, track.as_ref())
                .map_err(|e| e.to_string())?;
            let retained = self.reset_connection_catalog_review(Some(&album));
            let programs = if provider == "musicbrainz" {
                self.matcher
                    .as_ref()
                    .and_then(|m| m.cached_programs(&album))
                    .filter(|p| p.album.provider == "musicbrainz")
                    .cloned()
                    .or(retained)
            } else {
                None
            };
            let durable = if provider == "musicbrainz" {
                Some(
                    self.session
                        .library
                        .musicbrainz_review(&album)
                        .map_err(e_string)?,
                )
            } else {
                None
            };
            let best = durable.as_ref().and_then(|r| {
                r.candidates
                    .iter()
                    .find(|c| c.review_programs().is_some())
                    .or_else(|| r.candidates.first())
            });
            let programs = best.and_then(|c| c.review_programs().cloned()).or(programs);
            let provider_header=best.map(|c|json!({"title":c.metadata.title,"album":c.metadata.title,"artist":c.metadata.artist,"date":c.metadata.date,"kind":c.metadata.primary_type})).unwrap_or(Value::Null);
            let generation = self.connection_review_state.generation.wrapping_add(1);
            self.connection_review_state = State {
                target: Some(Target {
                    provider: provider.clone(),
                    album: album.clone(),
                    track: track.clone(),
                }),
                known,
                programs,
                provider_header,
                retained_candidates: durable.map(|r| r.candidates).unwrap_or_default(),
                generation,
                ..Default::default()
            };
            self.spotify_review_generation = self.spotify_review_generation.wrapping_add(1);
            self.spotify_resolution_generation = self.spotify_resolution_generation.wrapping_add(1);
            self.spotify_resolution_pending = false;
            if provider == "spotify" {
                if let Some(track) = &track {
                    self.spotify_playback_action("track".into(), track.as_ref().into());
                } else {
                    let raw = self.spotify_review_action_impl("open", album.as_ref(), 0);
                    self.connection_review_state.inspection =
                        serde_json::from_str(&raw).map_err(|e| e.to_string())?;
                }
            }
            if provider == "spotify" {
                if let Some(track) = &track {
                    let album_candidates = std::env::var("SPOTIFY_MARKET")
                        .ok()
                        .or_else(|| {
                            self.session
                                .library
                                .retained_spotify_market()
                                .ok()
                                .flatten()
                        })
                        .and_then(|market| {
                            music_library_spotify::album_context::cached_review_candidates(
                                &self.session.library,
                                &album,
                                &market,
                            )
                            .ok()
                        })
                        .and_then(|rows| rows.into_iter().find(|row| &row.track == track))
                        .map(|row| row.candidates)
                        .unwrap_or_default();
                    if self.spotify_resolution_selection.is_none() && !album_candidates.is_empty() {
                        self.spotify_resolution_selection =
                            Some(music_library::song_resolution::Selection::new(
                                self.session
                                    .library
                                    .song_resolution_input(track)
                                    .map_err(e_string)?,
                                vec![],
                            ));
                    }
                    if let Some(old) = self.spotify_resolution_selection.take() {
                        let mut inherited: Vec<_> = old
                            .candidates()
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| {
                                old.candidate_provenance(*i) == "Found through Album candidate"
                            })
                            .map(|(_, c)| c.clone())
                            .collect();
                        inherited.extend(album_candidates);
                        let input = self
                            .session
                            .library
                            .song_resolution_input(track)
                            .map_err(e_string)?;
                        let mut selection = music_library::song_resolution::Selection::new(
                            input,
                            old.candidates().to_vec(),
                        );
                        selection.inherit_album_candidates(inherited);
                        selection.show_all();
                        selection.set_trusted_identities(
                            self.session
                                .library
                                .diagnostic_track_identities(track)
                                .map_err(e_string)?,
                        );
                        self.spotify_resolution_selection = Some(selection);
                    }
                }
            }
            self.connection_review_state.track_generation = self.spotify_resolution_generation;
            self.prepare_connection_recordings()?;
        }
        let target = self
            .connection_review_state
            .target
            .clone()
            .ok_or("Open a Library review first")?;
        self.connection_manual_action(action, payload, index, &target)?;
        if action == "spotify-inspection" {
            return Ok(self.connection_review_state.inspection.clone());
        }
        if action == "navigate" {
            let count = self.connection_review_snapshot(&target)?["count"]
                .as_u64()
                .unwrap_or(0) as i32;
            if index < 0 || index >= count {
                return Err("Candidate is no longer available".into());
            }
            self.connection_review_state.error.clear();
            self.connection_review_state.index = index as usize;
            if target.provider == "musicbrainz" && target.track.is_none() {
                if let Some(c) = self
                    .connection_review_state
                    .retained_candidates
                    .get(index as usize)
                {
                    self.connection_review_state.programs = c.review_programs().cloned();
                    self.connection_review_state.provider_header = json!({"title":c.metadata.title,"album":c.metadata.title,"artist":c.metadata.artist,"date":c.metadata.date,"kind":c.metadata.primary_type});
                }
            }
            if target.provider == "spotify" && target.track.is_none() {
                let raw = self.spotify_review_action_impl("inspect", target.album.as_ref(), index);
                let value: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
                if let Some(e) = value["error"].as_str() {
                    return Err(e.into());
                }
                self.connection_review_state.inspection = value;
            }
        }
        if action == "mark" {
            self.session
                .library
                .mark_connection_unavailable(
                    &target.provider,
                    if target.track.is_none() {
                        Some(&target.album)
                    } else {
                        None
                    },
                    target.track.as_ref(),
                )
                .map_err(|e| e.to_string())?;
            self.connection_review_state.completed = true;
            self.connections_changed();
            self.connection_review_finished();
        }
        if action == "local-file" {
            let request: Value = serde_json::from_str(payload).map_err(|e| e.to_string())?;
            let track = TrackId(request["track"].as_str().ok_or("Choose a Track")?.into());
            if self
                .session
                .library
                .album_id_for_track(&track)
                .map_err(|e| e.to_string())?
                != target.album
                || target.track.as_ref().is_some_and(|id| id != &track)
            {
                return Err("File belongs to a different review target".into());
            }
            let path = url::Url::parse(request["url"].as_str().ok_or("Choose an audio file")?)
                .map_err(|e| e.to_string())?
                .to_file_path()
                .map_err(|_| "Choose a local file")?;
            let generation = self.connection_review_state.generation;
            let (send, recv) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                use music_library::filesystem::MetadataExtractor;
                let mut extractor = music_library::filesystem::LoftyMetadataExtractor;
                let result = if extractor.supports(&path) {
                    extractor
                        .read(&path)
                        .map(|metadata| FileCandidate {
                            track,
                            path,
                            metadata,
                        })
                        .map_err(|e| e.to_string())
                } else {
                    Err("Choose a supported audio file".into())
                };
                let _ = send.send((generation, result));
            });
            self.connection_review_state.file_job = Some(recv);
            self.connection_review_state.error.clear();
        }
        if action == "connect" || action == "connect-differences" {
            self.connection_review_state.error.clear();
            if target.provider == "spotify" {
                if target.track.is_some() {
                    self.spotify_resolve(
                        "confirm".into(),
                        self.connection_review_state.index as i32,
                    );
                    if !self.connection_review_state.completed {
                        return Err(self.spotify_resolution_message.clone());
                    }
                } else {
                    let value: Value = serde_json::from_str(&self.spotify_review_action_impl(
                        if action == "connect-differences" {
                            "confirm-differences"
                        } else {
                            "confirm"
                        },
                        target.album.as_ref(),
                        0,
                    ))
                    .map_err(|e| e.to_string())?;
                    if let Some(e) = value["error"].as_str() {
                        return Err(e.into());
                    }
                    if value["connected"].as_u64() == Some(0) {
                        return Err("No unresolved Tracks correspond; use manual mapping".into());
                    }
                    self.finish_connection_review("spotify", None, Some(&target.album), None);
                }
            } else if target.provider == "musicbrainz" {
                if target.track.is_some() {
                    self.prepare_connection_recordings()?;
                }
                let mappings = if let Some(track) = &target.track {
                    let candidate = self
                        .connection_review_state
                        .recordings
                        .get(self.connection_review_state.index)
                        .ok_or("Choose a Recording candidate")?;
                    let ids = &candidate.recording.identities;
                    if ids.len() != 1 {
                        return Err("Choose one unambiguous Recording identity".into());
                    }
                    vec![(track.clone(), ids[0].clone())]
                } else {
                    self.musicbrainz_review_mappings()?
                };
                if mappings.is_empty() {
                    return Err("No established Recording correspondence; match manually".into());
                }
                self.session
                    .library
                    .confirm_musicbrainz_connections(&mappings)
                    .map_err(e_string)?;
                self.connections_changed();
                self.finish_connection_review(
                    "musicbrainz",
                    target.track.as_ref(),
                    Some(&target.album),
                    None,
                );
            } else {
                let file = self
                    .connection_review_state
                    .file
                    .as_ref()
                    .ok_or("Choose a local source manually")?;
                let url = url::Url::from_file_path(&file.path).map_err(|_| "Invalid local path")?;
                let raw = self.connections_impl(
                    "attach-file",
                    "local",
                    &format!("{}\n{}", file.track.as_ref(), url),
                    false,
                );
                let result: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
                if result["error"].is_string() {
                    return Err(result["error"].as_str().unwrap().into());
                }
                return Ok(json!({"pending":true}));
            }
        }
        if let Some(receiver) = &self.connection_review_state.file_job {
            match receiver.try_recv() {
                Ok((generation, result)) => {
                    self.connection_review_state.file_job = None;
                    if generation == self.connection_review_state.generation {
                        self.connection_review_state.file = Some(result?);
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(e) => {
                    self.connection_review_state.file_job = None;
                    return Err(e.to_string());
                }
            }
        }
        self.connection_review_snapshot(&target)
    }
    fn prepare_connection_recordings(&mut self) -> Result<(), String> {
        let Some(target) = self.connection_review_state.target.clone() else {
            return Ok(());
        };
        if target.provider != "musicbrainz" {
            return Ok(());
        }
        if let Some(recording) = &self.connection_review_state.manual_recording {
            self.connection_review_state.recordings = vec![recording.clone()];
            self.connection_review_state.recording_best = None;
            return Ok(());
        }
        let Some(programs) = &self.connection_review_state.programs else {
            return Ok(());
        };
        self.connection_review_state.recording_best = None;
        let mut rows = music_library::manual_track::candidates(programs)
            .into_iter()
            .map(|c| c.evidence)
            .collect::<Vec<_>>();
        if let Some(track) = &target.track {
            let local_program = self
                .session
                .library
                .local_album_tracks(&target.album)
                .map_err(e_string)?;
            let mut relevant = Vec::new();
            for program in &programs.programs {
                let comparison = music_library::program_correspondence::correspond(
                    &local_program,
                    program,
                    self.connection_review_state.known["album"]
                        .as_str()
                        .unwrap_or(""),
                )
                .program;
                for row in &comparison.tracks {
                    if row.track_id == track.as_ref()
                        && matches!(row.decision,
                        music_library::album_program::PositionDecision::ExactAnchor
                        | music_library::album_program::PositionDecision::CorroboratedPosition
                        | music_library::album_program::PositionDecision::AlignedAfterInsertion
                        | music_library::album_program::PositionDecision::ReorderedTrack)
                    {
                        if let Some(provider) = &row.provider {
                            relevant.extend(provider.recording.identities.clone());
                        }
                    }
                }
            }
            rows.retain(|row| {
                row.recording
                    .identities
                    .iter()
                    .any(|id| relevant.contains(id))
            });
            let assessment = music_library::musicbrainz_recording::assess(
                &local_program,
                programs,
                self.connection_review_state.known["album"]
                    .as_str()
                    .unwrap_or(""),
            );
            if let Some(best) = assessment
                .recordings
                .iter()
                .find(|r| &r.track == track)
                .and_then(|r| r.recording.as_ref())
            {
                self.connection_review_state.recording_best = Some(best.clone());
                if let Some(index) = rows
                    .iter()
                    .position(|r| r.recording.identities.contains(best))
                {
                    let first = rows.remove(index);
                    rows.insert(0, first);
                }
            }
        }
        self.connection_review_state.recordings = rows;
        Ok(())
    }
    fn musicbrainz_review_mappings(&self) -> Result<Vec<(TrackId, ExternalIdentity)>, String> {
        let target = self
            .connection_review_state
            .target
            .as_ref()
            .ok_or("Open a review")?;
        let Some(programs) = &self.connection_review_state.programs else {
            return Ok(vec![]);
        };
        let local = self
            .session
            .library
            .local_album_tracks(&target.album)
            .map_err(e_string)?;
        let Some((_, comparison)) = music_library::musicbrainz_recording::review_program(
            &local,
            programs,
            self.connection_review_state.known["title"]
                .as_str()
                .unwrap_or(""),
        ) else {
            return Ok(vec![]);
        };
        let mut relevant = vec![];
        let mut after = None;
        loop {
            let rows = self
                .session
                .library
                .connection_tracks("musicbrainz", false, &target.album, after.as_ref(), 200)
                .map_err(e_string)?;
            relevant.extend(rows.iter().map(|r| r.id.clone()));
            after = rows.last().map(|r| r.id.clone());
            if rows.len() < 200 {
                break;
            }
        }
        Ok(
            music_library::musicbrainz_recording::confirmation(&comparison)
                .into_iter()
                .filter(|(track, _)| relevant.contains(track))
                .collect(),
        )
    }
    fn connection_review_snapshot(&mut self, target: &Target) -> Result<Value, String> {
        self.poll_connection_manual(target)?;
        self.sync_connection_program(target)?;
        if target.provider == "musicbrainz" {
            if let Some((programs, header)) = self
                .connection_catalog_review_candidate(&target.album)
                .filter(|_| self.connection_review_state.retained_candidates.is_empty())
            {
                self.connection_review_state.programs = Some(programs);
                self.connection_review_state.provider_header = header;
            }
            self.prepare_connection_recordings()?;
        }
        let known = self.connection_review_state.known.clone();
        let mut fields = vec![];
        let mut program = vec![];
        let mut evidence = vec![];
        let count;
        let mut decision: String;
        let mut can_connect = false;
        let mut confirm = false;
        if target.provider == "spotify" && target.track.is_none() {
            let value = &self.connection_review_state.inspection;
            count = value["count"].as_u64().unwrap_or(0) as usize;
            let c = &value["candidate"];
            for (label, k, v) in [
                ("Artist", "artist", c["artist"].clone()),
                ("Album", "title", c["title"]["provider_raw"].clone()),
                ("Date", "date", c["provider_date"].clone()),
                ("Type", "kind", c["title"]["provider_type"].clone()),
            ] {
                let right = if value["manual_mapping"] == true {
                    if k == "title" {
                        json!("Explicit Track mapping")
                    } else {
                        Value::Null
                    }
                } else {
                    v
                };
                fields.push(field(label, known[k].clone(), right));
            }
            program = rows_from_comparison(&value["programs"]);
            decision = match value["decision"].as_str().unwrap_or("") {
                "Accepted" => "Accepted Album candidate",
                other => other,
            }
            .into();
            for key in ["primary_reason", "warnings"] {
                // An alternative candidate's global retry reason does not
                // describe this accepted candidate. Its exact plan below does.
                if key == "primary_reason" && value["decision"] == "Accepted" {
                    continue;
                }
                if let Some(rows) = value[key].as_array() {
                    evidence.extend(rows.iter().filter_map(|v| v.as_str().map(str::to_owned)));
                } else if let Some(v) = value[key].as_str() {
                    if !v.is_empty() {
                        evidence.push(v.into());
                    }
                }
            }
            if let Some(candidate) = self
                .spotify_review_report
                .as_ref()
                .and_then(|r| r.candidates.get(self.connection_review_state.index))
            {
                evidence.insert(
                    0,
                    format!(
                        "{} Artist identity",
                        if candidate.artist_accepted {
                            "✓"
                        } else {
                            "! Review"
                        }
                    ),
                );
                evidence.insert(
                    1,
                    format!("Album title: {}", candidate.title.comparison.label()),
                );
                if let Some(p) = candidate.programs.first() {
                    let aligned = p
                        .tracks
                        .iter()
                        .filter(|r| {
                            matches!(r.decision,
                        music_library::album_program::PositionDecision::ExactAnchor
                        | music_library::album_program::PositionDecision::CorroboratedPosition
                        | music_library::album_program::PositionDecision::AlignedAfterInsertion
                        | music_library::album_program::PositionDecision::ReorderedTrack)
                        })
                        .count();
                    evidence.insert(2, format!("{aligned}/{} Tracks correspond", p.tracks.len()));
                }
            }
            can_connect = value["plan"]["matches"]
                .as_array()
                .is_some_and(|r| !r.is_empty());
            if let Some(matches) = value["plan"]["matches"].as_array() {
                evidence.push(format!(
                    "{} new Track connections proposed; {} unmapped",
                    matches.len(),
                    value["plan"]["unmatched_local"].as_u64().unwrap_or(0)
                ));
            }
            confirm = value["plan"]["requires_confirmation"]
                .as_bool()
                .unwrap_or(false);
            if value["manual_mapping"] == true {
                decision = "Explicit Track mapping".into();
                evidence = self
                    .spotify_review_selection
                    .as_ref()
                    .map(|s| s.warnings.clone())
                    .unwrap_or_default();
            }
        } else if target.provider == "spotify" {
            if self.connection_review_state.track_generation != self.spotify_resolution_generation {
                self.connection_review_state.index = 0;
                self.connection_review_state.track_generation = self.spotify_resolution_generation;
            }
            let trace: Value =
                serde_json::from_str(&self.spotify_comparison_json()).map_err(e_string)?;
            count = trace["candidates"].as_array().map(Vec::len).unwrap_or(0);
            self.connection_review_state.index = self
                .connection_review_state
                .index
                .min(count.saturating_sub(1));
            let evaluation = &trace["candidates"][self.connection_review_state.index];
            if let Some(rows) = evaluation["fields"].as_array() {
                for row in rows.iter().filter(|r| {
                    matches!(
                        r["label"].as_str(),
                        Some(
                            "Title"
                                | "Song"
                                | "Artist"
                                | "Artist credit"
                                | "Release date"
                                | "Release type"
                                | "Album"
                                | "Duration"
                                | "Disc"
                                | "Track"
                                | "Date"
                        )
                    )
                }) {
                    fields.push(json!({"label":row["label"],"library":row["local"],"candidate":row["candidate"],"status":row["status"]}));
                }
            }
            decision = evaluation["decision"].as_str().unwrap_or("").into();
            if let Some(reason) = evaluation["primary_blocker"].as_str() {
                evidence.push(reason.into());
            }
            evidence.extend(
                evaluation["warnings"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str().map(str::to_owned)),
            );
            can_connect = count > 0;
            confirm = count > 0 && evaluation["final_decision"].as_str() != Some("accept");
        } else if target.provider == "musicbrainz" {
            decision = self
                .session
                .library
                .musicbrainz_review(&target.album)
                .map_err(e_string)?
                .reason
                .label()
                .into();
            let candidate = if target.track.is_some() {
                count = self.connection_review_state.recordings.len();
                self.connection_review_state
                    .recordings
                    .get(self.connection_review_state.index)
                    .cloned()
            } else {
                count = self
                    .connection_review_state
                    .retained_candidates
                    .len()
                    .max(usize::from(self.connection_review_state.programs.is_some()));
                None
            };
            for (label, key) in if target.track.is_some() {
                vec![
                    ("Title", "title"),
                    ("Artist", "artist"),
                    ("Album", "album"),
                    ("Length", "length"),
                    ("Position", "position"),
                ]
            } else {
                vec![
                    ("Artist", "artist"),
                    ("Album", "title"),
                    ("Date", "date"),
                    ("Type", "kind"),
                ]
            } {
                let right = if let Some(c) = &candidate {
                    match key {
                        "title" => json!(c.title),
                        "artist" => json!(
                            c.artists
                                .iter()
                                .map(|a| a.name.clone())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        "length" => json!(c.duration_ms.map(|d| format!(
                            "{}:{:02}",
                            d / 60000,
                            d / 1000 % 60
                        ))),
                        "position" => {
                            json!(c.number.map(|n| format!("{}.{}", c.disc.unwrap_or(1), n)))
                        }
                        _ => self.connection_review_state.provider_header[key].clone(),
                    }
                } else if target.track.is_none() {
                    self.connection_review_state.provider_header[key].clone()
                } else {
                    Value::Null
                };
                fields.push(field(label, known[key].clone(), right));
            }
            if let Some(programs) = &self.connection_review_state.programs {
                if target.track.is_none() {
                    let local = self
                        .session
                        .library
                        .local_album_tracks(&target.album)
                        .map_err(e_string)?;
                    for p in &programs.programs {
                        let comparison = music_library::program_correspondence::correspond(
                            &local,
                            p,
                            known["title"].as_str().unwrap_or(""),
                        )
                        .program;
                        program.extend(rows_from_comparison(&json!([
                            Self::spotify_program_comparison("MusicBrainz", &comparison)
                        ])));
                        evidence.extend(comparison.warnings.clone());
                    }
                    can_connect = !self.musicbrainz_review_mappings()?.is_empty();
                } else {
                    can_connect = candidate.is_some();
                }
                decision = "Needs explicit Recording confirmation".into();
                confirm =
                    if let Some(candidate) = &candidate {
                        !candidate.recording.identities.iter().any(|id| {
                            Some(id) == self.connection_review_state.recording_best.as_ref()
                        })
                    } else {
                        !evidence.is_empty()
                    };
                if !confirm && count > 0 {
                    decision = "Established program / Recording correspondence".into();
                }
                if count == 0 {
                    decision = self
                        .session
                        .library
                        .musicbrainz_review(&target.album)
                        .map_err(e_string)?
                        .reason
                        .label()
                        .into();
                }
                evidence.push(programs.note.clone());
            }
        } else {
            count = usize::from(self.connection_review_state.file.is_some());
            for (label, key) in [
                ("Title", "title"),
                ("Artist", "artist"),
                ("Album", "album"),
                ("Length", "length"),
                ("Position", "position"),
            ] {
                let right = self
                    .connection_review_state
                    .file
                    .as_ref()
                    .map(|f| match key {
                        "title" => json!(f.metadata.track_title),
                        "artist" => json!(f.metadata.track_artists.join(", ")),
                        "album" => json!(f.metadata.release_title),
                        "length" => json!(f.metadata.duration_ms.map(|d| format!(
                            "{}:{:02}",
                            d / 60000,
                            d / 1000 % 60
                        ))),
                        "position" => json!(f.metadata.track_number.map(|n| format!(
                            "{}.{}",
                            f.metadata.disc_number.unwrap_or(1),
                            n
                        ))),
                        _ => Value::Null,
                    })
                    .unwrap_or(Value::Null);
                fields.push(field(label, known[key].clone(), right));
            }
            if target.track.is_none() {
                fields.clear();
                for (label, key) in [
                    ("Artist", "artist"),
                    ("Album", "title"),
                    ("Date", "date"),
                    ("Type", "kind"),
                ] {
                    let right = self
                        .connection_review_state
                        .file
                        .as_ref()
                        .map(|f| match key {
                            "artist" => json!(f.metadata.release_artists.join(", ")),
                            "title" => json!(f.metadata.release_title),
                            "date" => json!(f.metadata.year.map(|y| y.to_string())),
                            _ => Value::Null,
                        })
                        .unwrap_or(Value::Null);
                    fields.push(field(label, known[key].clone(), right));
                }
                if let Some(file) = &self.connection_review_state.file {
                    let local = self
                        .session
                        .library
                        .local_album_tracks(&target.album)
                        .map_err(e_string)?;
                    for t in local.iter().take(200) {
                        let selected = t.track_id == file.track;
                        program.push(json!({"track":t.track_id,"connects":selected,"provider_id":if selected{Some(file.path.to_string_lossy())}else{None},"library":format!("{}.{} {}",t.evidence.disc.unwrap_or(1),t.evidence.number.unwrap_or(0),t.evidence.title.as_deref().unwrap_or("—")),"candidate":if selected{format!("{}.{} {}",file.metadata.disc_number.unwrap_or(1),file.metadata.track_number.unwrap_or(0),file.metadata.track_title.as_deref().unwrap_or("—"))}else{String::new()},"status":if selected{"Explicit file attachment to selected Library Track"}else{"Library only"}}));
                    }
                }
            }
            can_connect = count > 0;
            decision = "Explicit local file attachment".into();
            if let Some(file) = &self.connection_review_state.file {
                evidence.push(file.path.display().to_string());
            }
        }
        if fields.is_empty() {
            for (label, key) in [
                ("Title", "title"),
                ("Artist", "artist"),
                ("Album", "album"),
                ("Length", "length"),
                ("Position", "position"),
            ] {
                fields.push(field(label, known[key].clone(), Value::Null));
            }
        }
        evidence.truncate(8);
        if target.provider == "spotify" && target.track.is_none() {
            program = self.connection_spotify_mapping()?;
        }
        if target.provider == "musicbrainz" && target.track.is_none() {
            program = self.connection_musicbrainz_mapping(target)?;
        }
        for row in &mut program {
            if let Some(status) = row["status"].as_str() {
                row["status"] = json!(
                    status
                        .replace("local only", "Library only")
                        .replace("local-only", "Library-only")
                );
            }
        }
        let library_program = known["program"].as_array().cloned().unwrap_or_default();
        // Existing diagnostics call canonical objects “local”; keep that
        // implementation terminology out of this source-neutral presentation.
        for line in &mut evidence {
            *line = line.replace("Local album:", "Library Album:").replace(
                "source values are shown under Evidence sources.",
                "inspect source evidence in Match manually…",
            );
        }
        Ok(
            json!({"candidate_loading":self.connection_review_state.program.pending(),"program_status":self.connection_review_state.program.status,"discovery_status":self.connection_discovery_status(target),"awaiting_discovery":self.browser.review_run_active || self.spotify_album_matcher.as_ref().is_some_and(|m|m.pending_count()>0),"mapping_available":target.track.is_some()||target.provider!="spotify"||self.spotify_review_selection.is_some(),"candidate_id":if target.provider=="spotify" && target.track.is_none(){self.spotify_review_report.as_ref().and_then(|r|r.candidates.get(self.connection_review_state.index)).map(|c|c.identity.external_id.as_str())}else{None},"manual":self.connection_manual_snapshot(target)?,"provider":target.provider,"album":target.album,"track":target.track,"generation":self.connection_review_state.generation,"index":self.connection_review_state.index,"count":count,"fields":fields,"program":program,"library_program":library_program,"decision":decision,"evidence":evidence,"can_connect":can_connect,"requires_confirmation":confirm,"pending":self.connection_review_state.file_job.is_some() || self.connection_review_state.manual.pending() || self.connection_review_state.program.pending(),"completed":self.connection_review_state.completed,"error":self.connection_review_state.error}),
        )
    }
}
fn e_string(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Session, sample};
    use music_library::{
        catalog::Page,
        song_resolution::{Candidate, Selection},
    };
    use qmetaobject::QObjectBox;
    fn begin(b: &mut Bridge, provider: &str, album: &AlbumId, track: Option<&TrackId>) -> Value {
        let value: Value = serde_json::from_str(&b.connection_review_impl(
            "open",
            &json!({"provider":provider,"album":album,"track":track}).to_string(),
            0,
        ))
        .unwrap();
        assert!(value["error"].as_str().is_none_or(str::is_empty), "{value}");
        value
    }
    fn candidate(input: &music_library::song_resolution::Input, key: &str) -> Candidate {
        Candidate {
            recording: Default::default(),
            identity: ExternalIdentity {
                provider: "spotify".into(),
                kind: "track".into(),
                external_id: key.into(),
            },
            album_identity: None,
            title: input.title.clone(),
            artist: input.artist.clone(),
            artists: input.artists.clone(),
            album: input.album.clone(),
            date: String::new(),
            album_artists: input.album_artists.clone(),
            album_type: String::new(),
            album_total_tracks: Some(45),
            duration_ms: input.duration_ms.unwrap_or(0),
            disc: input.disc.unwrap_or(1),
            number: input.number.unwrap_or(1),
        }
    }
    #[test]
    fn persisted_track_candidates_open_ranked_and_navigation_has_no_side_effects() {
        let (_temp, mut library) = sample::create().unwrap();
        let row = library
            .browse(&music_library::browse::Request {
                limit: 1,
                ..Default::default()
            })
            .unwrap()
            .remove(0);
        let track = TrackId(row.id);
        let album = library.album_id_for_track(&track).unwrap();
        let input = library.song_resolution_input(&track).unwrap();
        let exact = candidate(&input, "ExactCandidate123456789");
        let mut poor = candidate(&input, "WrongCandidate123456789");
        poor.title = "Completely different song".into();
        let page = Page {
            items: vec![poor, exact.clone()],
            next_offset: None,
        };
        library.persist_spotify_song_review(&input, &page).unwrap();
        let mut selection = Selection::new(input, page.items.clone());
        selection.show_all();
        let best = selection.visible_indices()[0];
        assert_eq!(selection.candidates()[best].identity, exact.identity);
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let (worker, requests) = crate::spotify_resolution::Worker::fake();
        let pinned = bridge.pinned();
        let mut b = pinned.borrow_mut();
        b.auto_match = false;
        b.spotify_resolution_worker = Some(worker);
        let initial = begin(&mut b, "spotify", &album, Some(&track));
        assert_eq!(initial["count"], 2);
        assert_eq!(initial["index"], 0);
        let fields = initial["fields"].as_array().unwrap();
        assert!(fields.iter().any(|f| f["candidate"] == exact.title));
        let original = b.session.library.unresolved_spotify_count().unwrap();
        let next: Value =
            serde_json::from_str(&b.connection_review_impl("navigate", "", 1)).unwrap();
        assert_eq!(next["index"], 1);
        let back: Value =
            serde_json::from_str(&b.connection_review_impl("navigate", "", 0)).unwrap();
        assert_eq!(back["fields"], initial["fields"]);
        assert_eq!(
            b.session.library.unresolved_spotify_count().unwrap(),
            original
        );
        assert!(requests.try_recv().is_err());
        let connected: Value =
            serde_json::from_str(&b.connection_review_impl("connect", "", 0)).unwrap();
        assert_eq!(connected["completed"], true);
        assert_eq!(
            b.session.library.unresolved_spotify_count().unwrap(),
            original - 1
        );
    }
    #[test]
    fn album_programs_autoload_cached_candidates_and_guard_async_navigation() {
        use music_library::{album_program::Program, catalog::ArtistAlbumCandidate};
        let _temp = tempfile::tempdir().unwrap();
        let mut library =
            music_library::Library::open(_temp.path().join("programs.sqlite")).unwrap();
        let release = library
            .create_catalog_release(&music_library::domain::CatalogReleaseInput {
                title: "Album".into(),
                year: None,
                artists: vec![music_library::domain::ArtistCreditInput {
                    name: "Artist".into(),
                    role: None,
                }],
                tracks: (1..=3)
                    .map(|n| music_library::domain::CatalogTrackInput {
                        title: format!("Song {n}"),
                        disc_number: Some(1),
                        track_number: Some(n),
                        artists: vec![music_library::domain::ArtistCreditInput {
                            name: "Artist".into(),
                            role: None,
                        }],
                    })
                    .collect(),
            })
            .unwrap();
        for track in &release.track_ids {
            library.add_to_library(track).unwrap();
        }
        let album = library.album_id_for_track(&release.track_ids[0]).unwrap();
        let local = library.local_album_tracks(&album).unwrap();
        library.retain_spotify_cooldown(i64::MAX / 2).unwrap();
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let pinned = bridge.pinned();
        let mut b = pinned.borrow_mut();
        b.auto_match = false;
        begin(&mut b, "spotify", &album, None);
        let identity = ExternalIdentity {
            provider: "spotify".into(),
            kind: "album".into(),
            external_id: "A".repeat(22),
        };
        let candidate = ArtistAlbumCandidate {
            identity: identity.clone(),
            title: "Candidate".into(),
            artist: "Artist".into(),
            artist_ids: vec![],
            primary_type: "album".into(),
            date: String::new(),
            comment: String::new(),
        };
        let programs = Programs {
            album: identity,
            programs: vec![Program {
                identity: None,
                complete: true,
                tracks: local
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        let mut e = t.evidence.clone();
                        e.identities = vec![ExternalIdentity {
                            provider: "spotify".into(),
                            kind: "track".into(),
                            external_id: format!("{i:022}"),
                        }];
                        e
                    })
                    .collect(),
            }],
            note: String::new(),
        };
        b.test_manual_album_result(candidate, programs.clone());
        let selected: Value =
            serde_json::from_str(&b.connection_review_impl("read", "", 0)).unwrap();
        assert!(
            selected["program"]
                .as_array()
                .is_some_and(|p| p.len() >= local.len()),
            "{selected}"
        );
        b.connection_review_impl("cancel", "", 0);
        let cached = begin(&mut b, "spotify", &album, None);
        assert_eq!(
            cached["count"], 1,
            "manual discovery retained for first-layer review"
        );
        assert_eq!(cached["candidate_loading"], false);
        assert!(cached["program_status"].as_str().unwrap().is_empty());
        assert!(cached["program"].as_array().unwrap().len() >= local.len());
        let mut second = b.spotify_review_report.as_ref().unwrap().candidates[0].clone();
        second.identity.external_id = "B".repeat(22);
        b.spotify_review_report
            .as_mut()
            .unwrap()
            .candidates
            .push(second.clone());
        let blocked: Value =
            serde_json::from_str(&b.connection_review_impl("navigate", "", 1)).unwrap();
        assert!(
            blocked["program_status"]
                .as_str()
                .unwrap()
                .contains("cooldown")
        );
        let back: Value =
            serde_json::from_str(&b.connection_review_impl("navigate", "", 0)).unwrap();
        assert_eq!(back["candidate_id"], programs.album.external_id);
        let mut next_program = programs.clone();
        next_program.album = second.identity.clone();
        b.test_candidate_program_reply(next_program);
        let late: Value = serde_json::from_str(&b.connection_review_impl("read", "", 0)).unwrap();
        assert_eq!(
            late["candidate_id"], programs.album.external_id,
            "late result cannot replace inspected candidate"
        );
        let next: Value =
            serde_json::from_str(&b.connection_review_impl("navigate", "", 1)).unwrap();
        assert_eq!(next["candidate_id"], second.identity.external_id);
        assert!(next["program"].as_array().unwrap().len() >= local.len());
        assert!(next["program_status"].as_str().unwrap().is_empty());
        assert!(!include_str!("../Main.qml").contains("Load candidate track mapping"));
    }

    #[test]
    fn zero_candidates_provider_specific_marking_and_stale_preview_are_isolated() {
        let (_temp, library) = sample::create().unwrap();
        let rows = library
            .browse(&music_library::browse::Request {
                limit: 2,
                ..Default::default()
            })
            .unwrap();
        let track = TrackId(rows[0].id.clone());
        let other = TrackId(rows[1].id.clone());
        let album = library.album_id_for_track(&track).unwrap();
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let pinned = bridge.pinned();
        let mut b = pinned.borrow_mut();
        b.auto_match = false;
        for provider in ["local", "musicbrainz", "spotify"] {
            let v = begin(&mut b, provider, &album, Some(&track));
            assert_eq!(v["count"], 0);
            assert_eq!(v["can_connect"], false);
            assert!(
                v["fields"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|f| f["library"].as_str().is_some_and(|s| !s.is_empty()))
            );
        }
        let v: Value = serde_json::from_str(&b.connection_review_impl("mark", "", 0)).unwrap();
        assert_eq!(v["completed"], true);
        assert!(b.session.library.spotify_manually_excluded(&track).unwrap());
        assert_eq!(
            b.session
                .library
                .connection_coverage("musicbrainz")
                .unwrap()
                .marked,
            0
        );
        let obsolete_song_generation = b.spotify_resolution_generation;
        let obsolete_input = b.session.library.song_resolution_input(&track).unwrap();
        begin(&mut b, "local", &album, Some(&other));
        b.finish_song_lookup(crate::spotify_resolution::Reply {
            generation: obsolete_song_generation,
            input: obsolete_input,
            result: Ok(Page {
                items: vec![],
                next_offset: None,
            }),
            counts: (99, 99),
        });
        assert_ne!(b.spotify_resolution_counts, (99, 99));
        let generation = b.connection_review_state.generation;
        let (send, receive) = std::sync::mpsc::channel();
        b.connection_review_state.file_job = Some(receive);
        send.send((
            generation - 1,
            Ok(FileCandidate {
                track: track.clone(),
                path: "/wrong-old-target".into(),
                metadata: Default::default(),
            }),
        ))
        .ok();
        let read: Value = serde_json::from_str(&b.connection_review_impl("read", "", 0)).unwrap();
        assert_eq!(read["count"], 0);
        assert_eq!(read["track"], json!(other));
    }
    #[test]
    fn musicbrainz_review_restores_retained_program_after_bridge_and_library_restart() {
        let (_temp, mut library) = sample::create().unwrap();
        let track = TrackId(
            library
                .browse(&music_library::browse::Request {
                    limit: 1,
                    ..Default::default()
                })
                .unwrap()[0]
                .id
                .clone(),
        );
        let album = library.album_id_for_track(&track).unwrap();
        let local = library.local_album_tracks(&album).unwrap();
        let path = library.database_path().unwrap();
        let programs = Programs {
            album: ExternalIdentity {
                provider: "musicbrainz".into(),
                kind: "release_group".into(),
                external_id: "retained".into(),
            },
            programs: vec![Program {
                identity: None,
                complete: true,
                tracks: local
                    .iter()
                    .map(|t| {
                        let mut e = t.evidence.clone();
                        e.recording.identities = vec![ExternalIdentity {
                            provider: "musicbrainz".into(),
                            kind: "recording".into(),
                            external_id: t.track_id.0.clone(),
                        }];
                        e
                    })
                    .collect(),
            }],
            note: "cached fixture".into(),
        };
        library
            .retain_musicbrainz_programs(&album, &programs)
            .unwrap();
        library
            .retain_musicbrainz_inspection(&album, &programs)
            .unwrap();
        drop(library);
        for _ in 0..2 {
            let bridge = QObjectBox::new(Bridge::new(Session::new(
                music_library::Library::open(&path).unwrap(),
            )));
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.auto_match = false;
            begin(&mut b, "musicbrainz", &album, None);
            let snapshot: Value =
                serde_json::from_str(&b.connection_review_impl("read", "", 0)).unwrap();
            assert_eq!(snapshot["count"], 1);
            assert!(!snapshot["program"].as_array().unwrap().is_empty());
            assert!(b.connection_catalog_state.idle());
            assert!(!b.connection_review_state.manual.pending());
            let expected = music_library::musicbrainz_recording::confirmation(
                &music_library::program_correspondence::correspond(
                    &b.session.library.local_album_tracks(&album).unwrap(),
                    &programs.programs[0],
                    "Diagnostic edition",
                ),
            );
            let displayed = b.musicbrainz_review_mappings().unwrap();
            assert_eq!(displayed, expected);
        }
    }
    #[test]
    #[ignore = "audit-only: disposable database and retained MusicBrainz audit JSON directory required"]
    fn musicbrainz_audit_retained_real_programs() {
        let source = std::env::var("MUSIC_LIBRARY_DIAGNOSTIC_DATABASE").unwrap();
        assert!(source.starts_with("/tmp/mb-audit-"));
        let directory = std::env::var("MUSIC_LIBRARY_MB_AUDIT_EVIDENCE").unwrap();
        let selected = [
            "Freshman Year",
            "The Devil Isn't Red",
            "Business & Pleasure",
            "The EP of a Band Called Adebisi Shank",
        ];
        let mut inspected = 0;
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            let data: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            let Some(title) = data["title"].as_str().filter(|s| selected.contains(s)) else {
                continue;
            };
            let temp = tempfile::tempdir().unwrap();
            let copy = temp.path().join("audit.sqlite");
            // VACUUM INTO is read-only on the disposable source; each confirmation is isolated.
            rusqlite::Connection::open(&source)
                .unwrap()
                .execute("VACUUM INTO ?1", [copy.to_str().unwrap()])
                .unwrap();
            let library = music_library::Library::open(&copy).unwrap();
            let before = library.connection_coverage("musicbrainz").unwrap();
            let album = AlbumId(data["album_id"].as_str().unwrap().into());
            let album_ids = library.list_album_external_identities(&album).unwrap();
            let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.auto_match = false;
            begin(&mut b, "musicbrainz", &album, None);
            b.connection_review_state.programs =
                Some(serde_json::from_value(data["programs"].clone()).unwrap());
            let mappings = b.musicbrainz_review_mappings().unwrap();
            let snapshot: Value =
                serde_json::from_str(&b.connection_review_impl("read", "", 0)).unwrap();
            assert!(!snapshot["program"].as_array().unwrap().is_empty());
            assert!(!mappings.is_empty(), "{title}: {snapshot}");
            let result: Value =
                serde_json::from_str(&b.connection_review_impl("connect", "", 0)).unwrap();
            assert_eq!(result["completed"], true, "{title}: {result}");
            let after = b
                .session
                .library
                .connection_coverage("musicbrainz")
                .unwrap();
            assert_eq!(after.connected - before.connected, mappings.len() as i64);
            assert_eq!(after.saved, before.saved);
            assert_eq!(
                b.session
                    .library
                    .list_album_external_identities(&album)
                    .unwrap(),
                album_ids
            );
            eprintln!(
                "MB_AUDIT_REVIEW {title}: persisted {} Recording mappings; coverage {} -> {}; Album identities unchanged",
                mappings.len(),
                before.connected,
                after.connected
            );
            inspected += 1;
        }
        assert_eq!(inspected, selected.len());
    }
    #[test]
    fn cached_musicbrainz_program_uses_shared_correspondence_and_partial_album_confirmation() {
        let (_temp, library) = sample::create().unwrap();
        let rows = library
            .browse(&music_library::browse::Request {
                limit: 2,
                ..Default::default()
            })
            .unwrap();
        let track = TrackId(rows[0].id.clone());
        let album = library.album_id_for_track(&track).unwrap();
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let pinned = bridge.pinned();
        let mut b = pinned.borrow_mut();
        b.auto_match = false;
        begin(&mut b, "musicbrainz", &album, Some(&track));
        let local = b.session.library.local_album_tracks(&album).unwrap();
        let mut exact = local
            .iter()
            .find(|t| t.track_id == track)
            .unwrap()
            .evidence
            .clone();
        let identity = ExternalIdentity {
            provider: "musicbrainz".into(),
            kind: "recording".into(),
            external_id: "correct-recording".into(),
        };
        exact.recording.identities = vec![identity.clone()];
        let mut bad = exact.clone();
        bad.title = Some("Wrong recording".into());
        bad.number = Some(999); // A distinct candidate position, not contradictory duplicate evidence.
        bad.recording.identities[0].external_id = "aaa-sorts-first".into();
        let programs = Programs {
            album: ExternalIdentity {
                provider: "musicbrainz".into(),
                kind: "release_group".into(),
                external_id: "group".into(),
            },
            programs: vec![Program {
                identity: None,
                tracks: vec![bad, exact.clone()],
                complete: true,
            }],
            note: "Retained provider program".into(),
        };
        b.connection_review_state.programs = Some(programs);
        b.prepare_connection_recordings().unwrap();
        assert_eq!(
            b.connection_review_state.recordings[0].recording.identities[0],
            identity
        );
        let v: Value = serde_json::from_str(&b.connection_review_impl("read", "", 0)).unwrap();
        assert_eq!(v["count"], 1);
        let v: Value = serde_json::from_str(&b.connection_review_impl("connect", "", 0)).unwrap();
        assert_eq!(v["completed"], true);
        assert_eq!(
            b.session
                .library
                .connection_coverage("musicbrainz")
                .unwrap()
                .connected,
            1
        );
        begin(&mut b, "musicbrainz", &album, None);
        b.connection_review_state.programs = Some(Programs {
            album: ExternalIdentity {
                provider: "musicbrainz".into(),
                kind: "release_group".into(),
                external_id: "group".into(),
            },
            programs: vec![Program {
                identity: None,
                tracks: local
                    .iter()
                    .take(4)
                    .enumerate()
                    .map(|(i, t)| {
                        let mut e = t.evidence.clone();
                        e.recording.identities = vec![ExternalIdentity {
                            provider: "musicbrainz".into(),
                            kind: "recording".into(),
                            external_id: if t.track_id == track {
                                "correct-recording".into()
                            } else {
                                format!("recording-{i}")
                            },
                        }];
                        e
                    })
                    .collect(),
                complete: true,
            }],
            note: "Retained partial program".into(),
        });
        let v: Value = serde_json::from_str(&b.connection_review_impl("connect", "", 0)).unwrap();
        assert_eq!(v["completed"], true);
        let c = b
            .session
            .library
            .connection_coverage("musicbrainz")
            .unwrap();
        assert!(c.connected > 1 && c.connected < c.saved);
    }
    #[test]
    fn album_review_uses_authoritative_ranking_and_existing_partial_confirmation() {
        use music_library::album_candidates::{CandidateEvidence, Reason, Report};
        let (_temp, library) = sample::create().unwrap();
        let track = TrackId(
            library
                .browse(&music_library::browse::Request {
                    limit: 1,
                    ..Default::default()
                })
                .unwrap()[0]
                .id
                .clone(),
        );
        let album = library.album_id_for_track(&track).unwrap();
        let local = library.local_album_tracks(&album).unwrap();
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let pinned = bridge.pinned();
        let mut b = pinned.borrow_mut();
        b.auto_match = false;
        begin(&mut b, "spotify", &album, None);
        let mut candidates = vec![];
        let mut programs = vec![];
        for (rank, key, title) in [
            (1, "poor", "Unrelated Album"),
            (2, "good", "Diagnostic edition"),
        ] {
            let identity = ExternalIdentity {
                provider: "spotify".into(),
                kind: "album".into(),
                external_id: key.into(),
            };
            let tracks = local
                .iter()
                .take(3)
                .enumerate()
                .map(|(n, t)| {
                    let mut e = t.evidence.clone();
                    e.identities = vec![ExternalIdentity {
                        provider: "spotify".into(),
                        kind: "track".into(),
                        external_id: format!("{key}{n:018}"),
                    }];
                    e.recording = Default::default();
                    e
                })
                .collect();
            let p = Programs {
                album: identity.clone(),
                programs: vec![Program {
                    identity: None,
                    tracks,
                    complete: true,
                }],
                note: String::new(),
            };
            let candidate = music_library::catalog::ArtistAlbumCandidate {
                identity: identity.clone(),
                title: title.into(),
                artist: "Diagnostic artist".into(),
                artist_ids: vec![],
                primary_type: "album".into(),
                date: String::new(),
                comment: String::new(),
            };
            candidates.push(CandidateEvidence {
                observation: None,
                identity,
                artist: candidate.artist.clone(),
                artist_ids: vec![],
                artist_accepted: true,
                warnings: vec![],
                provider_rank: Some(rank),
                title: music_library::album_candidates::title_evidence(
                    "Diagnostic edition",
                    &candidate,
                ),
                local_date: None,
                provider_date: String::new(),
                date_agreement: "unknown".into(),
                local_tracks: 45,
                required_tracks: 45,
                provider_tracks: Some(3),
                programs: vec![music_library::album_program::inspect_album_program(
                    &local,
                    &p.programs[0],
                    "Diagnostic edition",
                )],
                equivalent_to: None,
                playable_family: None,
                decision: if key == "good" {
                    "Withheld".into()
                } else {
                    "Rejected".into()
                },
                reasons: vec![Reason::CompetingCandidates],
            });
            programs.push(p);
        }
        let mut report = Report {
            musicbrainz_discovery: None,
            discovery_queries: vec![],
            local_title: "Diagnostic edition".into(),
            established_artist: None,
            candidate_count: 2,
            more_candidates: false,
            candidates,
            decision: "Withheld".into(),
            reasons: vec![Reason::CompetingCandidates],
        };
        report.rank_candidates();
        assert_eq!(report.candidates[0].provider_rank, Some(2));
        b.spotify_review_report = Some(report);
        b.spotify_review_programs = programs;
        b.connection_review_state.inspection =
            serde_json::from_str(&b.spotify_review_action_impl("inspect", album.as_ref(), 0))
                .unwrap();
        let first: Value = serde_json::from_str(&b.connection_review_impl("read", "", 0)).unwrap();
        assert_eq!(first["index"], 0);
        assert_eq!(first["count"], 2);
        assert!(
            first["fields"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["candidate"] == "Diagnostic edition")
        );
        assert!(first["program"].as_array().unwrap().len() >= 3);
        let proposed = first["program"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["connects"] == true)
            .collect::<Vec<_>>();
        assert_eq!(
            proposed.len(),
            b.spotify_review_selection.as_ref().unwrap().matches().len()
        );
        for (row, m) in proposed
            .iter()
            .zip(b.spotify_review_selection.as_ref().unwrap().matches())
        {
            assert_eq!(row["track"], json!(m.track_id));
            assert_eq!(row["provider_id"], m.candidate().identity.external_id);
        }

        let next: Value =
            serde_json::from_str(&b.connection_review_impl("navigate", "", 1)).unwrap();
        assert_eq!(next["index"], 1);
        assert_ne!(first["fields"], next["fields"]);
        assert_ne!(first["program"], next["program"]);
        assert_eq!(b.session.library.unresolved_spotify_count().unwrap(), 45);
        let _: Value = serde_json::from_str(&b.connection_review_impl("navigate", "", 0)).unwrap();
        let confirmed: Value =
            serde_json::from_str(&b.connection_review_impl("connect-differences", "", 0)).unwrap();
        assert_eq!(confirmed["completed"], true, "{confirmed}");
        let groups = b
            .session
            .library
            .connection_groups("spotify", false, None, 100)
            .unwrap();
        assert_eq!(
            (groups[0].matched, groups[0].total, groups[0].relevant),
            (3, 45, 42)
        );
        begin(&mut b, "spotify", &album, None);
        let _: Value = serde_json::from_str(&b.connection_review_impl("mark", "", 0)).unwrap();
        assert!(
            b.session
                .library
                .connection_groups("spotify", false, None, 100)
                .unwrap()
                .is_empty()
        );
        assert_eq!(b.session.library.marked_spotify_count().unwrap(), 42);
    }

    #[test]
    fn failed_track_confirmation_keeps_target_and_candidate() {
        let (_temp, mut library) = sample::create().unwrap();
        let track = TrackId(
            library
                .browse(&music_library::browse::Request {
                    limit: 1,
                    ..Default::default()
                })
                .unwrap()[0]
                .id
                .clone(),
        );
        let album = library.album_id_for_track(&track).unwrap();
        let input = library.song_resolution_input(&track).unwrap();
        let page = Page {
            items: vec![candidate(&input, "C".repeat(22).as_str())],
            next_offset: None,
        };
        library.persist_spotify_song_review(&input, &page).unwrap();
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let pinned = bridge.pinned();
        let mut b = pinned.borrow_mut();
        b.auto_match = false;
        let first = begin(&mut b, "spotify", &album, Some(&track));
        b.session
            .library
            .set_track_title_override(&track, "Changed during review")
            .unwrap();
        let failed: Value =
            serde_json::from_str(&b.connection_review_impl("connect", "", 0)).unwrap();
        assert!(failed["error"].as_str().is_some_and(|e| !e.is_empty()));
        assert!(!b.connection_review_state.completed);
        assert_eq!(
            b.connection_review_state
                .target
                .as_ref()
                .unwrap()
                .track
                .as_ref(),
            Some(&track)
        );
        assert_eq!(b.connection_review_state.index, 0);
        assert_eq!(first["count"], 1);
    }
}
