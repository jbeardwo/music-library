//! Inline manual candidate discovery. Selection is presentation only; Connect owns writes.
use super::*;
use music_library::catalog::{AlbumCandidate, CatalogSession, Page};
use music_library::song_resolution::{Candidate, Selection, SongSearch};
use std::sync::mpsc::{self, Receiver};
#[derive(Default)]
pub(super) struct State {
    job: Option<Receiver<Result<Reply, String>>>,
    albums: Vec<AlbumCandidate>,
    tracks: Vec<Candidate>,
    catalog_pending: bool,
    catalog: Value,
    mapping: Value,
}
impl State {
    pub(super) fn pending(&self) -> bool {
        self.job.is_some() || self.catalog_pending
    }
}
enum Reply {
    Albums(Vec<AlbumCandidate>),
    Album(music_library::catalog::ArtistAlbumCandidate, Programs),
    Tracks(Vec<Candidate>),
    Track(Candidate),
}
impl Bridge {
    pub(super) fn connection_manual_action(
        &mut self,
        action: &str,
        payload: &str,
        index: i32,
        target: &Target,
    ) -> Result<(), String> {
        if !action.starts_with("manual-") {
            return Ok(());
        }
        self.connection_review_state.error.clear();
        if self.connection_review_state.manual.pending() {
            return Err("A candidate request is already running".into());
        }
        if action == "manual-options" {
            self.connection_review_state.manual.mapping = serde_json::from_str(
                &self.spotify_review_action_impl("mapping", target.album.as_ref(), 0),
            )
            .map_err(e_string)?;
            return Ok(());
        }
        if action == "manual-mapping" {
            let raw = self.spotify_review_action_impl("prepare-mapping", payload, 0);
            let result: Value = serde_json::from_str(&raw).map_err(e_string)?;
            if let Some(e) = result["error"].as_str() {
                return Err(e.into());
            }
            self.connection_review_state.inspection["plan"] = result;
            self.connection_review_state.inspection["manual_mapping"] = json!(true);
            return Ok(());
        }
        if target.provider == "musicbrainz" {
            if action == "manual-recording" {
                let recordings = self
                    .connection_review_state
                    .programs
                    .clone()
                    .or_else(|| {
                        self.connection_catalog_review_candidate(&target.album)
                            .map(|c| c.0)
                    })
                    .ok_or("Inspect a Release first")?;
                let candidate = recordings
                    .programs
                    .iter()
                    .flat_map(|p| p.tracks.iter())
                    .nth(index as usize)
                    .ok_or("Choose a Recording")?
                    .clone();
                if target.track.is_none() {
                    return Err("Choose a Library Track to associate a Recording".into());
                }
                self.connection_review_state.manual_recording = Some(candidate);
                self.connection_review_state.index = 0;
            } else {
                let raw = self.connection_catalog_impl(
                    if action == "manual-search" {
                        "search"
                    } else {
                        "inspect"
                    },
                    payload,
                    index,
                );
                let result: Value = serde_json::from_str(&raw).map_err(e_string)?;
                if let Some(e) = result["error"].as_str() {
                    return Err(e.into());
                }
                self.connection_review_state.manual.catalog_pending =
                    result["pending"].as_bool().unwrap_or(false);
                self.connection_review_state.manual_recording = None;
                self.connection_review_state.index = 0;
            }
            return Ok(());
        }
        if target.provider != "spotify" {
            return Ok(());
        }
        if action == "manual-select" && target.track.is_some() {
            let candidate = self
                .connection_review_state
                .manual
                .tracks
                .get(index as usize)
                .ok_or("Choose a Track")?
                .clone();
            let input = self
                .session
                .library
                .song_resolution_input(target.track.as_ref().unwrap())
                .map_err(e_string)?;
            let mut selection = Selection::new(input, vec![candidate]);
            selection.show_all();
            selection.mark_manual_attachment();
            self.spotify_resolution_selection = Some(selection);
            self.connection_review_state.index = 0;
            return Ok(());
        }
        let request: Value = if action == "manual-select" {
            Value::Null
        } else {
            serde_json::from_str(payload).map_err(e_string)?
        };
        let exact = request["exact"].as_str().unwrap_or("").trim().to_owned();
        let query = request["query"].as_str().unwrap_or("").trim().to_owned();
        let artist = request["artist"].as_str().unwrap_or("").trim().to_owned();
        let selected = if action == "manual-select" {
            Some(
                self.connection_review_state
                    .manual
                    .albums
                    .get(index as usize)
                    .ok_or("Choose an Album")?
                    .identity
                    .external_id
                    .clone(),
            )
        } else {
            None
        };
        let kind = if target.track.is_some() {
            "track"
        } else {
            "album"
        };
        let identity = if !exact.is_empty() {
            Some(
                music_library::spotify_review::parse_spotify_object(&exact, kind)
                    .map_err(e_string)?
                    .external_id,
            )
        } else {
            selected
        };
        // An exact object already in this review is selectable without credentials
        // or another provider fetch. Preserve its existing evaluated evidence.
        if let Some(id) = &identity {
            if target.track.is_some() {
                let candidate = self
                    .spotify_resolution_selection
                    .as_ref()
                    .and_then(|s| {
                        s.candidates()
                            .iter()
                            .find(|c| &c.identity.external_id == id)
                    })
                    .cloned();
                if let Some(candidate) = candidate {
                    self.connection_review_state.manual.tracks = vec![candidate];
                    return self.connection_manual_action("manual-select", "", 0, target);
                }
            } else if let Some(row) = self
                .spotify_review_report
                .as_ref()
                .and_then(|r| r.candidates.iter().find(|c| &c.identity.external_id == id))
                .cloned()
            {
                if self
                    .spotify_review_programs
                    .iter()
                    .any(|p| p.album == row.identity)
                {
                    if let Some(report) = &mut self.spotify_review_report {
                        report.candidates = vec![row];
                        report.candidate_count = 1;
                    }
                    self.connection_review_state.index = 0;
                    self.connection_review_state.inspection = serde_json::from_str(
                        &self.spotify_review_action_impl("inspect", target.album.as_ref(), 0),
                    )
                    .map_err(e_string)?;
                    self.connection_review_state.inspection["manual_candidate"] = json!(true);
                    return Ok(());
                }
            }
        }
        if identity.is_none() && query.is_empty() {
            return Err("Enter a title and Artist, or a Spotify URL, URI or ID".into());
        }
        let mut input = target
            .track
            .as_ref()
            .map(|t| self.session.library.song_resolution_input(t))
            .transpose()
            .map_err(e_string)?;
        if let Some(input) = &mut input {
            input.title = query.clone();
            input.artist = artist.clone();
            input.primary_artist = None;
            input.artists.clear();
            input.spotify_excluded = false;
        }
        let path = self.session.library.database_path();
        let (send, recv) = mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| {
                let mut provider = music_library_spotify::Spotify::from_env()
                    .and_then(|p| p.with_library(path.as_deref()))
                    .map_err(e_string)?;
                if let Some(id) = identity {
                    if kind == "album" {
                        let (a, p) = provider.explicit_album(&id).map_err(e_string)?;
                        Ok(Reply::Album(a, p))
                    } else {
                        Ok(Reply::Track(
                            provider.explicit_track(&id).map_err(e_string)?,
                        ))
                    }
                } else if let Some(input) = input {
                    Ok(Reply::Tracks(
                        provider.search_songs(&input).map_err(e_string)?.items,
                    ))
                } else {
                    let mut query = format!("album:\"{}\"", query.replace('"', ""));
                    if !artist.is_empty() {
                        query.push_str(&format!(" artist:\"{}\"", artist.replace('"', "")));
                    }
                    Ok(Reply::Albums(
                        CatalogSession::new(provider)
                            .search_albums(&query, 0)
                            .map_err(e_string)?
                            .items,
                    ))
                }
            })();
            let _ = send.send(result);
        });
        self.connection_review_state.manual.job = Some(recv);
        Ok(())
    }
    pub(super) fn poll_connection_manual(&mut self, target: &Target) -> Result<(), String> {
        if self.connection_review_state.manual.catalog_pending {
            let result: Value = serde_json::from_str(&self.connection_catalog_impl("poll", "", 0))
                .map_err(e_string)?;
            self.connection_review_state.manual.catalog_pending =
                result["pending"].as_bool().unwrap_or(false);
            if let Some(e) = result["error"].as_str() {
                return Err(e.into());
            }
            if !self.connection_review_state.manual.catalog_pending {
                self.connection_review_state.error.clear();
                self.connection_review_state.manual.catalog = result;
            }
        }
        let Some(job) = &self.connection_review_state.manual.job else {
            return Ok(());
        };
        let result = match job.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return Ok(()),
            Err(e) => Err(e.to_string()),
        };
        self.connection_review_state.manual.job = None;
        let reply = result?;
        self.connection_review_state.error.clear();
        match reply {
            Reply::Albums(rows) => self.connection_review_state.manual.albums = rows,
            Reply::Tracks(rows) => {
                self.connection_review_state.manual.tracks = rows;
            }
            Reply::Track(candidate) => {
                self.connection_review_state.manual.tracks = vec![candidate];
                self.connection_manual_action("manual-select", "", 0, target)?;
            }
            Reply::Album(candidate, programs) => {
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
                self.session
                    .library
                    .retain_connection_candidate(&target.album, &market, &candidate, &programs)
                    .map_err(e_string)?;
                let local = self
                    .session
                    .library
                    .local_album_tracks(&target.album)
                    .map_err(e_string)?;
                let music_library::album_matching::Preparation::Ready(input) = self
                    .session
                    .library
                    .spotify_album_review_input(&target.album)
                    .map_err(e_string)?
                else {
                    return Err("Album needs canonical Artist/title evidence".into());
                };
                let provider = music_library_spotify::Spotify::new(
                    music_library_spotify::Config::new(
                        "offline".into(),
                        "offline".into(),
                        "US".into(),
                    )
                    .map_err(e_string)?,
                );
                let mut report = music_library::album_candidates::initial_report(
                    &provider,
                    (&input.raw_title, input.date),
                    input.known_artist.as_ref(),
                    &Page {
                        items: vec![candidate],
                        next_offset: None,
                    },
                    &local,
                );
                for row in &mut report.candidates {
                    row.programs = programs
                        .programs
                        .iter()
                        .map(|p| {
                            music_library::album_program::inspect_album_program(
                                &local,
                                p,
                                &input.raw_title,
                            )
                        })
                        .collect();
                }
                self.spotify_review_report = Some(report);
                self.spotify_review_programs
                    .retain(|p| p.album != programs.album);
                self.spotify_review_programs.push(programs);
                self.spotify_review_album = Some(target.album.clone());
                self.connection_review_state.index = 0;
                self.connection_review_state.inspection = serde_json::from_str(
                    &self.spotify_review_action_impl("inspect", target.album.as_ref(), 0),
                )
                .map_err(e_string)?;
                self.connection_review_state.inspection["manual_candidate"] = json!(true);
            }
        }
        Ok(())
    }
    pub(super) fn connection_manual_snapshot(&mut self, target: &Target) -> Result<Value, String> {
        if target.provider == "musicbrainz" && !self.connection_review_state.manual.catalog_pending
        {
            let result: Value = serde_json::from_str(&self.connection_catalog_impl("poll", "", 0))
                .map_err(e_string)?;
            if result["error"].is_null() {
                self.connection_review_state.manual.catalog = result;
            }
        }
        let state = &self.connection_review_state.manual;
        let results = if target.provider == "musicbrainz" {
            state.catalog["candidates"].clone()
        } else if target.track.is_some() {
            json!(
                state
                    .tracks
                    .iter()
                    .map(|c| json!({"title":c.title,"artist":c.artist,"album":c.album}))
                    .collect::<Vec<_>>()
            )
        } else {
            json!(
                state
                    .albums
                    .iter()
                    .map(|c| json!({"title":c.title,"artist":c.artist}))
                    .collect::<Vec<_>>()
            )
        };
        let recordings = state.catalog["tracks"].clone();
        let mapping = self.connection_review_state.manual.mapping.clone();
        Ok(json!({"results":results,"recordings":recordings,"mapping":mapping}))
    }
    pub(super) fn connection_musicbrainz_mapping(
        &self,
        target: &Target,
    ) -> Result<Vec<Value>, String> {
        let Some(programs) = self.connection_review_state.programs.as_ref() else {
            return Ok(vec![]);
        };
        let mappings = self.musicbrainz_review_mappings()?;
        let local = self
            .session
            .library
            .local_album_tracks(&target.album)
            .map_err(e_string)?;
        let Some((selected, comparison)) = music_library::musicbrainz_recording::review_program(
            &local,
            programs,
            self.connection_review_state.known["title"]
                .as_str()
                .unwrap_or(""),
        ) else {
            return Ok(vec![]);
        };
        let program = &programs.programs[selected];
        let comparison = comparison.program;
        let mut rows = Vec::new();
        for t in local.iter().take(200) {
            let mapped = mappings.iter().find(|(track, _)| track == &t.track_id);
            let aligned = comparison
                .tracks
                .iter()
                .find(|r| r.track_id == t.track_id.as_ref());
            let trusted = aligned.and_then(|r| r.provider.as_ref()).filter(|p| {
                p.recording
                    .identities
                    .iter()
                    .any(|id| t.evidence.recording.identities.contains(id))
            });
            let provider = mapped
                .and_then(|(_, id)| {
                    program
                        .tracks
                        .iter()
                        .find(|p| p.recording.identities.contains(id))
                })
                .or(trusted);
            let status = if mapped.is_some() {
                aligned
                    .map(|r| r.decision.label())
                    .unwrap_or("Proposed Recording connection")
            } else if trusted.is_some() {
                "Already trusted Recording; no new connection"
            } else {
                aligned.map(|r| r.decision.label()).unwrap_or("Unmapped")
            };
            rows.push(json!({"track":t.track_id,"library":format!("{}.{} {}",t.evidence.disc.unwrap_or(1),t.evidence.number.unwrap_or(0),t.evidence.title.as_deref().unwrap_or("—")),"candidate":provider.map(|p|format!("{}.{} {}",p.disc.unwrap_or(1),p.number.unwrap_or(0),p.title.as_deref().unwrap_or("—"))).unwrap_or_else(||"—".into()),"provider_id":provider.and_then(|p|p.recording.identities.first()).map(|id|id.external_id.as_str()),"connects":mapped.is_some(),"status":status}));
        }
        for t in &program.tracks {
            if rows.len() >= 200 {
                break;
            }
            if !rows.iter().any(|row| {
                t.recording
                    .identities
                    .iter()
                    .any(|id| row["provider_id"].as_str() == Some(id.external_id.as_str()))
            }) {
                rows.push(json!({"library":"—","candidate":format!("{}.{} {}",t.disc.unwrap_or(1),t.number.unwrap_or(0),t.title.as_deref().unwrap_or("—")),"status":"Provider-only; no safe Recording correspondence proposed","connects":false}));
            }
        }
        Ok(rows)
    }
    pub(super) fn connection_spotify_mapping(&self) -> Result<Vec<Value>, String> {
        let Some(target) = &self.connection_review_state.target else {
            return Ok(vec![]);
        };
        let Some(selection) = &self.spotify_review_selection else {
            return Ok(vec![]);
        };
        let local = self
            .session
            .library
            .local_album_tracks(&target.album)
            .map_err(e_string)?;
        let mut rows = Vec::new();
        for t in local.iter().take(200) {
            let known = format!(
                "{}.{} {}",
                t.evidence.disc.unwrap_or(1),
                t.evidence.number.unwrap_or(0),
                t.evidence.title.as_deref().unwrap_or("—")
            );
            if let Some(m) = selection
                .matches()
                .iter()
                .find(|m| m.track_id == t.track_id)
            {
                let c = m.candidate();
                let mut status = vec![if m.local_title == m.provider_title {
                    "Exact title".to_owned()
                } else if !m.warnings.iter().any(|w| w.starts_with("Title:")) {
                    "Normalized-compatible title".to_owned()
                } else {
                    "Title / version difference".to_owned()
                }];
                status.extend(m.warnings.clone());
                let candidate = if self.connection_review_state.inspection["manual_mapping"] == true
                {
                    format!("{}.{} {} — {}", c.disc, c.number, c.title, c.album)
                } else {
                    format!("{}.{} {}", c.disc, c.number, c.title)
                };
                rows.push(json!({"track":t.track_id,"provider_id":c.identity.external_id,"provider_album":c.album_identity,"library":known,"candidate":candidate,"status":status.join(" · "),"connects":true}));
            } else {
                let aligned = self
                    .spotify_review_report
                    .as_ref()
                    .and_then(|r| r.candidates.get(self.connection_review_state.index))
                    .and_then(|c| c.programs.first())
                    .and_then(|p| p.tracks.iter().find(|r| r.track_id == t.track_id.as_ref()));
                let unmatched = selection.unmatched_local.contains(&t.track_id);
                let counterpart = if unmatched {
                    None
                } else {
                    aligned.and_then(|r| r.provider.as_ref())
                };
                rows.push(json!({"track":t.track_id,"provider_id":counterpart.and_then(|p|p.identities.iter().find(|id|id.provider=="spotify"&&id.kind=="track")).map(|id|id.external_id.as_str()),"library":known,"candidate":counterpart.map(|p|format!("{}.{} {}",p.disc.unwrap_or(1),p.number.unwrap_or(0),p.title.as_deref().unwrap_or("—"))).unwrap_or_else(||"—".into()),"status":if unmatched{aligned.map(|r|r.decision.label()).unwrap_or("Unmapped; no safe correspondence proposed")}else{"Outside current unresolved selection; no new connection"},"connects":false}));
            }
        }
        if let Some(programs) = self.spotify_review_programs.iter().find(|p| {
            self.spotify_review_report
                .as_ref()
                .and_then(|r| r.candidates.get(self.connection_review_state.index))
                .is_some_and(|c| c.identity == p.album)
        }) {
            for t in programs.programs.iter().flat_map(|p| p.tracks.iter()) {
                if rows.len() >= 200 {
                    break;
                }
                if !rows.iter().any(|row| {
                    t.identities
                        .iter()
                        .any(|id| row["provider_id"].as_str() == Some(id.external_id.as_str()))
                }) {
                    rows.push(json!({"library":"—","candidate":format!("{}.{} {}",t.disc.unwrap_or(1),t.number.unwrap_or(0),t.title.as_deref().unwrap_or("—")),"status":"Provider Track without a proposed new connection","connects":false}));
                }
            }
        }
        Ok(rows)
    }
}

#[cfg(test)]
impl Bridge {
    pub(crate) fn test_manual_track_results(&mut self, rows: Vec<Candidate>) {
        let (send, recv) = mpsc::channel();
        send.send(Ok(Reply::Tracks(rows))).unwrap();
        self.connection_review_state.manual.job = Some(recv);
    }
}
#[cfg(test)]
impl Bridge {
    pub(crate) fn test_manual_album_result(
        &mut self,
        row: music_library::catalog::ArtistAlbumCandidate,
        programs: Programs,
    ) {
        let (send, recv) = mpsc::channel();
        send.send(Ok(Reply::Album(row, programs))).unwrap();
        self.connection_review_state.manual.job = Some(recv);
    }
    pub(crate) fn test_manual_album_results(&mut self, rows: Vec<AlbumCandidate>) {
        let (send, recv) = mpsc::channel();
        send.send(Ok(Reply::Albums(rows))).unwrap();
        self.connection_review_state.manual.job = Some(recv);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Session, sample};
    use qmetaobject::QObjectBox;
    #[test]
    fn failed_manual_request_and_obsolete_reply_keep_current_review() {
        let (_temp, library) = sample::create().unwrap();
        let rows = library
            .browse(&music_library::browse::Request {
                limit: 2,
                ..Default::default()
            })
            .unwrap();
        let album = library
            .album_id_for_track(&music_library::domain::TrackId(rows[0].id.clone()))
            .unwrap();
        let bridge = QObjectBox::new(Bridge::new(Session::new(library)));
        let pinned = bridge.pinned();
        let mut b = pinned.borrow_mut();
        b.auto_match = false;
        let open =
            |track: &str| json!({"provider":"spotify","album":album,"track":track}).to_string();
        let before: Value =
            serde_json::from_str(&b.connection_review_impl("open", &open(&rows[0].id), 0)).unwrap();
        let (send, recv) = mpsc::channel();
        b.connection_review_state.manual.job = Some(recv);
        send.send(Err("Explicit search failed".into())).unwrap();
        let failed: Value = serde_json::from_str(&b.connection_review_impl("read", "", 0)).unwrap();
        assert_eq!(failed["error"], "Explicit search failed");
        assert!(!b.connection_review_state.manual.pending());
        assert_eq!(
            b.connection_review_state
                .target
                .as_ref()
                .unwrap()
                .track
                .as_ref()
                .unwrap()
                .as_ref(),
            rows[0].id
        );
        let reread: Value = serde_json::from_str(&b.connection_review_impl("read", "", 0)).unwrap();
        assert_eq!(reread["fields"], before["fields"]);
        let (old_send, recv) = mpsc::channel();
        b.connection_review_state.manual.job = Some(recv);
        b.connection_review_impl("open", &open(&rows[1].id), 0);
        assert!(
            old_send.send(Ok(Reply::Albums(vec![]))).is_err(),
            "reopening invalidates the previous manual request receiver"
        );
        let current: Value =
            serde_json::from_str(&b.connection_review_impl("read", "", 0)).unwrap();
        assert_eq!(current["track"], rows[1].id);
        assert_eq!(current["count"], 0);
    }
}
