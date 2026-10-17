//! Offline group/candidate inspection and explicit Album confirmation.
use crate::Bridge;
use music_library::{album_matching::Preparation, domain::AlbumId};
enum ExplicitObject {
    Album(
        music_library::catalog::ArtistAlbumCandidate,
        music_library::album_program::Programs,
    ),
    Track(music_library::song_resolution::Candidate),
}
impl Bridge {
    pub(crate) fn spotify_review_result_is_current(
        &self,
        generation: u64,
        song_generation: Option<u64>,
    ) -> bool {
        generation == self.spotify_review_generation
            && song_generation
                .is_none_or(|generation| generation == self.spotify_resolution_generation)
    }
    pub(crate) fn spotify_review_action_impl(
        &mut self,
        action: &str,
        id: &str,
        index: i32,
    ) -> String {
        let result = self.spotify_review_action_result(action, id, index);
        match result {
            Ok(value) => value.to_string(),
            Err(error) => serde_json::json!({"error":error}).to_string(),
        }
    }
    fn spotify_review_action_result(
        &mut self,
        action: &str,
        id: &str,
        index: i32,
    ) -> Result<serde_json::Value, String> {
        let album = AlbumId(id.into());
        match action {
            "groups" => Ok(serde_json::json!(
                self.session
                    .library
                    .spotify_review_groups(if id.is_empty() { None } else { Some(&album) }, 200)
                    .map_err(|e| e.to_string())?
            )),
            "mark-unresolved" => {
                let count = self
                    .session
                    .library
                    .mark_album_unresolved_not_on_spotify(&album)
                    .map_err(|e| e.to_string())?;
                self.refresh_spotify_review();
                self.browse_changed();
                Ok(serde_json::json!({"marked": count}))
            }
            "tracks" => {
                let request = music_library::browse::Request {
                    pane: music_library::browse::Pane::Songs,
                    album: Some(album),
                    unresolved_spotify: true,
                    limit: 201,
                    ..Default::default()
                };
                let rows = self
                    .session
                    .library
                    .browse(&request)
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::json!(rows.iter().map(|r|serde_json::json!({"id":r.id,"title":r.title,"artist":r.subtitle,"reason":r.connection_reason})).collect::<Vec<_>>()))
            }
            "open" => {
                self.spotify_review_generation = self.spotify_review_generation.wrapping_add(1);
                self.spotify_review_fetch_pending = false;
                self.spotify_resolution_message.clear();
                self.spotify_resolution_generation =
                    self.spotify_resolution_generation.wrapping_add(1);
                self.spotify_resolution_pending = false;
                self.spotify_resolution_selection = None;
                self.spotify_resolution_more = false;
                self.spotify_resolution_counts = (0, 0);
                self.spotify_artist_proposal = None;
                self.spotify_review_album = Some(album.clone());
                self.spotify_review_selection = None;
                self.spotify_review_programs.clear();
                self.spotify_review_report = None;
                if let Preparation::Ready(input) = self
                    .session
                    .library
                    .spotify_album_review_input(&album)
                    .map_err(|e| e.to_string())?
                    && let Some(path) = self.session.library.database_path()
                    && let Some(market) = std::env::var("SPOTIFY_MARKET").ok().or(self
                        .session
                        .library
                        .retained_spotify_market()
                        .map_err(|e| e.to_string())?)
                    && let Some((report, programs)) =
                        music_library_spotify::Spotify::cached_album_review(&path, &market, &input)
                            .map_err(|e| e.to_string())?
                {
                    self.session
                        .library
                        .persist_spotify_review(&album, &report)
                        .map_err(|e| e.to_string())?;
                    self.spotify_review_report = Some(report);
                    self.spotify_review_programs = programs;
                }
                if self.spotify_review_report.is_none() {
                    // A still-established pairing can remain partially unresolved.
                    if let Some(matcher) = &self.spotify_album_matcher {
                        self.spotify_review_report = matcher.diagnostic(&album).cloned();
                        if let Some(p) = matcher.cached_programs(&album) {
                            self.spotify_review_programs.push(p.clone());
                        }
                    }
                }
                if self.spotify_review_report.is_none() {
                    self.session
                        .library
                        .spotify_album_review_evidence_unavailable(&album)
                        .map_err(|e| e.to_string())?;
                    self.refresh_spotify_review();
                    self.browse_changed();
                }
                // Inspection never turns an expired/incomplete cached evaluation
                // into the current reconciliation lifecycle decision.
                self.spotify_review_action_result("inspect", id, 0)
            }
            "paste-album" | "paste-track" => {
                if self.spotify_review_fetch_pending {
                    return Err("Spotify object lookup is already running".into());
                }
                let kind = if action == "paste-album" {
                    "album"
                } else {
                    "track"
                };
                let identity = music_library::spotify_review::parse_spotify_object(id, kind)
                    .map_err(|e| e.to_string())?;
                let album = self.spotify_review_album.clone();
                let track = self.spotify_playback_track.clone();
                if kind == "album" && album.is_none() || kind == "track" && track.is_none() {
                    return Err("Open the relevant connection review first".into());
                }
                let path = self.session.library.database_path();
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                let seconds = self
                    .session
                    .library
                    .spotify_cooldown_remaining(now)
                    .map_err(|e| e.to_string())?;
                if seconds > 0 {
                    return Err(format!(
                        "Waiting for Spotify: retry in {} minutes",
                        (seconds + 59) / 60
                    ));
                }
                self.spotify_review_fetch_pending = true;
                let generation = self.spotify_review_generation;
                let song_generation = self.spotify_resolution_generation;
                let pointer = qmetaobject::QPointer::from(&*self);
                let deliver = qmetaobject::queued_callback(
                    move |result: Result<ExplicitObject, String>| {
                        if let Some(object) = pointer.as_pinned() {
                            let mut b = object.borrow_mut();
                            if !b.spotify_review_result_is_current(
                                generation,
                                (kind == "track").then_some(song_generation),
                            ) || (kind == "album" && b.spotify_review_album != album)
                                || (kind == "track" && b.spotify_playback_track != track)
                            {
                                return;
                            }
                            b.spotify_review_fetch_pending = false;
                            match result {
                                Ok(ExplicitObject::Album(candidate, programs))
                                    if b.spotify_review_album == album =>
                                {
                                    if let Some(album) = &album
                                        && let Ok(local) =
                                            b.session.library.local_album_tracks(album)
                                    {
                                        let input = local.first().and_then(|t| {
                                            b.session
                                                .library
                                                .song_resolution_input(&t.track_id)
                                                .ok()
                                        });
                                        if let Some(input) = input {
                                            let provider = music_library_spotify::Spotify::new(
                                                music_library_spotify::Config::new(
                                                    "offline".into(),
                                                    "offline".into(),
                                                    "US".into(),
                                                )
                                                .unwrap(),
                                            );
                                            let mut report =
                                                music_library::album_candidates::initial_report(
                                                    &provider,
                                                    (&input.album, input.album_date),
                                                    input
                                                        .primary_artist
                                                        .as_ref()
                                                        .and_then(|a| a.identities.first()),
                                                    &music_library::catalog::Page {
                                                        items: vec![candidate],
                                                        next_offset: None,
                                                    },
                                                    &local,
                                                );
                                            for row in &mut report.candidates {
                                                row.programs=programs.programs.iter().map(|p|music_library::album_program::inspect_album_program(&local,p,&input.album)).collect();
                                            }
                                            b.spotify_review_report = Some(report);
                                            b.spotify_review_programs
                                                .retain(|old| old.album != programs.album);
                                            b.spotify_review_programs.push(programs);
                                            b.spotify_resolution_message="Explicit Spotify Album loaded. Review evidence before connecting; advanced mapping can use all loaded Albums.".into();
                                        }
                                    }
                                }
                                Ok(ExplicitObject::Track(candidate))
                                    if b.spotify_playback_track == track =>
                                {
                                    if let Some(track) = &track
                                        && let Ok(input) =
                                            b.session.library.song_resolution_input(track)
                                    {
                                        let mut selection =
                                            music_library::song_resolution::Selection::new(
                                                input,
                                                vec![candidate],
                                            );
                                        selection.show_all();
                                        selection.mark_manual_attachment();
                                        b.spotify_resolution_selection = Some(selection);
                                        b.spotify_resolution_message="Explicit Spotify Track loaded. Review differences before confirmation.".into();
                                    }
                                }
                                Err(error) => b.spotify_resolution_message = error,
                                _ => {}
                            }
                            b.spotify_playback_changed();
                        }
                    },
                );
                std::thread::spawn(move || {
                    let result = music_library_spotify::Spotify::from_env()
                        .and_then(|p| p.with_library(path.as_deref()))
                        .and_then(|mut p| {
                            if identity.kind == "album" {
                                p.explicit_album(&identity.external_id).map(
                                    |(candidate, programs)| {
                                        ExplicitObject::Album(candidate, programs)
                                    },
                                )
                            } else {
                                p.explicit_track(&identity.external_id)
                                    .map(ExplicitObject::Track)
                            }
                        })
                        .map_err(|e| e.to_string());
                    deliver(result);
                });
                Ok(
                    serde_json::json!({"message":"Loading explicit Spotify object; existing cache and cooldown apply. No connection is saved until confirmation."}),
                )
            }
            "mapping" => {
                let album = self
                    .spotify_review_album
                    .as_ref()
                    .ok_or("Open an Album first")?;
                let local = self
                    .session
                    .library
                    .local_album_tracks(album)
                    .map_err(|e| e.to_string())?;
                let options = self.spotify_mapping_candidates();
                let rows=local.iter().map(|t| {
                    let suggestions=options.iter().enumerate().filter(|(_,p)| music_library::matching::normalize(&p.title)==music_library::matching::normalize(t.evidence.title.as_deref().unwrap_or(""))).map(|(i,_)|i).collect::<Vec<_>>();
                    let suggested=if suggestions.len()==1 {Some(suggestions[0])}else{None};
                    serde_json::json!({"id":t.track_id,"title":t.evidence.title,"suggested":suggested.map(|i|i as i32+1).unwrap_or(0)})
                }).collect::<Vec<_>>();
                Ok(serde_json::json!({"rows":rows,"options":options}))
            }
            "prepare-mapping" => {
                let album = self
                    .spotify_review_album
                    .as_ref()
                    .ok_or("Open an Album first")?;
                let indexes: Vec<(String, String)> =
                    serde_json::from_str(id).map_err(|e| e.to_string())?;
                let options = self.spotify_mapping_candidates();
                let mappings = indexes
                    .into_iter()
                    .map(|(track, identity)| {
                        options
                            .iter()
                            .find(|c| c.identity.external_id == identity)
                            .cloned()
                            .map(|c| (music_library::domain::TrackId(track), c))
                            .ok_or("Mapping option is no longer available")
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let selection = self
                    .session
                    .library
                    .prepare_spotify_track_mapping(album, &mappings)
                    .map_err(|e| e.to_string())?;
                let result = serde_json::json!({"matches":selection.matches(),"warnings":selection.warnings,"requires_confirmation":selection.requires_confirmation()});
                self.spotify_review_selection = Some(selection);
                Ok(result)
            }
            "mark-album" => {
                let changed = self
                    .session
                    .library
                    .mark_album_not_on_spotify(&AlbumId(id.into()))
                    .map_err(|e| e.to_string())?;
                self.refresh_spotify_review();
                self.browse_changed();
                Ok(
                    serde_json::json!({"message":format!("Marked {changed} unresolved Tracks as not on Spotify") }),
                )
            }
            "inspect" => {
                if let Some(report) = self
                    .spotify_review_report
                    .as_ref()
                    .filter(|r| r.candidates.is_empty())
                {
                    return Ok(
                        serde_json::json!({"target_id":self.spotify_review_album,"generation":self.spotify_review_generation,"count":0,"index":0,"known_album":report.local_title,"known_artist":self.spotify_review_album.as_ref().and_then(|a|self.session.library.spotify_album_review_input(a).ok()).and_then(|p|if let Preparation::Ready(i)=p{Some(i.artist)}else{None}),"primary_reason":report.reasons.first().map(|r|r.label()),"discovery_queries":report.discovery_queries,"message":report.explain_track("")}),
                    );
                }
                let Some(report) = self
                    .spotify_review_report
                    .as_ref()
                    .filter(|report| !report.candidates.is_empty())
                else {
                    let known = self
                        .spotify_review_album
                        .as_ref()
                        .and_then(|album| self.session.library.local_album_tracks(album).ok())
                        .and_then(|tracks| {
                            tracks.first().and_then(|t| {
                                self.session.library.song_resolution_input(&t.track_id).ok()
                            })
                        });
                    let lifecycle = self
                        .spotify_review_album
                        .as_ref()
                        .and_then(|album| {
                            self.session
                                .library
                                .spotify_album_review_lifecycle(album)
                                .ok()
                        })
                        .unwrap_or_default();
                    return Ok(
                        serde_json::json!({"target_id":self.spotify_review_album,"generation":self.spotify_review_generation,"count":0,"index":0,"known_album":known.as_ref().map(|i|i.album.as_str()),"known_artist":known.as_ref().map(|i|i.artist.as_str()),"lifecycle":lifecycle,"message":"No cached Album candidates. Retry Album performs bounded discovery."}),
                    );
                };
                let c = report
                    .candidates
                    .get(index.max(0) as usize)
                    .ok_or("Select a cached candidate")?;
                let unresolved_reason = if c.decision == "Accepted" {
                    self.spotify_review_album
                        .as_ref()
                        .and_then(|album| {
                            self.session
                                .library
                                .spotify_retry_tracks(std::slice::from_ref(album), 1)
                                .ok()
                        })
                        .and_then(|tracks| {
                            tracks
                                .first()
                                .map(|track| report.review_reason(track.as_ref()))
                        })
                } else {
                    None
                };
                let mut plan = None;
                if let Some(p) = self
                    .spotify_review_programs
                    .iter()
                    .find(|p| p.album == c.identity)
                {
                    let mut selection = self
                        .session
                        .library
                        .prepare_spotify_album_confirmation(
                            self.spotify_review_album
                                .as_ref()
                                .ok_or("Open an Album first")?,
                            p,
                        )
                        .map_err(|e| e.to_string())?;
                    for reason in &c.reasons {
                        if matches!(
                            reason,
                            music_library::album_candidates::Reason::AlbumTitleMismatch
                                | music_library::album_candidates::Reason::AlbumVersionMismatch
                                | music_library::album_candidates::Reason::ReleaseTypeMismatch
                                | music_library::album_candidates::Reason::TrustedIdentityConflict
                        ) {
                            selection.warnings.push(reason.label().into());
                        }
                    }
                    if !c.artist_accepted {
                        selection.warnings.push(format!("Album Artist credit needs confirmation: {}. Artist equivalence will remain unchanged.",c.artist));
                    }
                    plan = Some(
                        serde_json::json!({"requires_confirmation":selection.requires_confirmation(),"matches":selection.matches(),"unmatched_local":selection.unmatched_local.len(),"unmatched_provider":selection.unmatched_provider,"warnings":selection.warnings}),
                    );
                    self.spotify_review_selection = Some(selection);
                } else {
                    self.spotify_review_selection = None;
                }
                let mut comparisons = c
                    .programs
                    .iter()
                    .map(|p| Self::spotify_program_comparison(&c.title.provider_raw, p))
                    .collect::<Vec<_>>();
                if let Some(programs) = self
                    .spotify_review_programs
                    .iter()
                    .find(|p| p.album == c.identity)
                {
                    for ((comparison, known), provider) in comparisons
                        .iter_mut()
                        .zip(&c.programs)
                        .zip(&programs.programs)
                    {
                        let positions: std::collections::HashSet<_> = known
                            .tracks
                            .iter()
                            .filter_map(|t| t.local.number.map(|n| (t.local.disc.unwrap_or(1), n)))
                            .collect();
                        for track in &provider.tracks {
                            if track
                                .number
                                .is_none_or(|n| !positions.contains(&(track.disc.unwrap_or(1), n)))
                                && let Some(rows) = comparison["rows"].as_array_mut()
                                && rows.len() < 200
                            {
                                rows.push(serde_json::json!({"local":"—","candidate":format!("{}.{} {}",track.disc.unwrap_or(1),track.number.unwrap_or(0),track.title.as_deref().unwrap_or("—")),"decision":"Extra provider Track; no canonical position"}));
                            }
                        }
                    }
                }
                Ok(
                    serde_json::json!({"target_id":self.spotify_review_album,"generation":self.spotify_review_generation,"lifecycle":format!("Current lifecycle: {}. Cached comparison shown; inspection does not refresh discovery.", self.spotify_review_album.as_ref().map(|a| self.session.library.spotify_album_review_lifecycle(a)).transpose().map_err(|e|e.to_string())?.unwrap_or_default()),"our_rank":index.max(0)+1,"provider_rank":c.provider_rank,"count":report.candidates.len(),"index":index.max(0),"known_album":report.local_title,"known_artist":self.spotify_review_album.as_ref().and_then(|album|self.session.library.spotify_album_review_input(album).ok()).and_then(|input|if let Preparation::Ready(input)=input {Some(input.artist)} else {None}),"discovery_queries":report.discovery_queries,"candidate":c,"decision":if c.decision=="Withheld" {"Needs review"} else {&c.decision},"primary_reason":unresolved_reason.as_ref().or_else(|| if c.decision == "Accepted" { None } else { c.reasons.first().or(report.reasons.first()) }).map(|r|r.label()),"warnings":c.warnings.iter().cloned().chain(c.reasons.iter().skip(1).map(|r|r.label().to_owned())).collect::<Vec<_>>(),"requirements":c.acceptance_requirements(),"preferred":c.decision=="Accepted","plan":plan,"programs":comparisons}),
                )
            }
            "fetch-program" => {
                if self.spotify_review_fetch_pending {
                    return Err("Candidate program discovery is already running".into());
                }
                let candidate = self
                    .spotify_review_report
                    .as_ref()
                    .and_then(|r| r.candidates.get(index.max(0) as usize))
                    .ok_or("Select a candidate")?
                    .identity
                    .clone();
                let album = self
                    .spotify_review_album
                    .clone()
                    .ok_or("Open an Album first")?;
                let path = self.session.library.database_path();
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                let seconds = self
                    .session
                    .library
                    .spotify_cooldown_remaining(now)
                    .map_err(|e| e.to_string())?;
                if seconds > 0 {
                    return Err(format!(
                        "Waiting for Spotify: retry available in {} minutes",
                        (seconds + 59) / 60
                    ));
                }
                self.spotify_review_fetch_pending = true;
                let generation = self.spotify_review_generation;
                let pointer = qmetaobject::QPointer::from(&*self);
                let deliver = qmetaobject::queued_callback(
                    move |result: Result<music_library::album_program::Programs, String>| {
                        if let Some(object) = pointer.as_pinned() {
                            let mut b = object.borrow_mut();
                            if !b.spotify_review_result_is_current(generation, None)
                                || b.spotify_review_album.as_ref() != Some(&album)
                            {
                                return;
                            }
                            b.spotify_review_fetch_pending = false;
                            if b.spotify_review_album.as_ref() == Some(&album) {
                                match result {
                                    Ok(p) => {
                                        let title = b
                                            .session
                                            .library
                                            .local_album_tracks(&album)
                                            .ok()
                                            .and_then(|local| {
                                                local.first().and_then(|t| {
                                                    b.session
                                                        .library
                                                        .song_resolution_input(&t.track_id)
                                                        .ok()
                                                })
                                            })
                                            .map(|input| input.album)
                                            .unwrap_or_default();
                                        if let Ok(local) =
                                            b.session.library.local_album_tracks(&album)
                                            && let Some(report) = &mut b.spotify_review_report
                                            && let Some(row) = report
                                                .candidates
                                                .iter_mut()
                                                .find(|c| c.identity == p.album)
                                        {
                                            row.programs=p.programs.iter().map(|p|music_library::album_program::inspect_album_program(&local,p,&title)).collect();
                                        }
                                        b.spotify_review_programs
                                            .retain(|old| old.album != p.album);
                                        b.spotify_review_programs.push(p);
                                        b.spotify_resolution_message="Candidate program discovered; review its positioned Tracks.".into();
                                    }
                                    Err(e) => b.spotify_resolution_message = e,
                                }
                                b.spotify_playback_changed();
                            }
                        }
                    },
                );
                std::thread::spawn(move || {
                    use music_library::catalog::CatalogProvider;
                    let result = music_library_spotify::Spotify::from_env()
                        .and_then(|p| p.with_library(path.as_deref()))
                        .and_then(|mut p| p.album_programs(&candidate))
                        .map_err(|e| e.to_string());
                    deliver(result);
                });
                Ok(
                    serde_json::json!({"message":"Fetching this candidate's bounded program; existing cache is reused where current."}),
                )
            }
            "confirm" | "confirm-differences" => {
                let selection = self.spotify_review_selection.as_ref().ok_or(
                    "Candidate program is not cached. Retry Album to discover its program.",
                )?;
                let count = self
                    .session
                    .library
                    .confirm_spotify_album(selection, action == "confirm-differences")
                    .map_err(|e| e.to_string())?;
                self.refresh_spotify_review();
                self.browse_changed();
                Ok(serde_json::json!({"connected":count}))
            }
            "retry" | "refresh" => {
                if action == "refresh" {
                    self.session
                        .library
                        .clear_spotify_bulk_window(&AlbumId(id.into()))
                        .map_err(|e| e.to_string())?;
                }
                if action == "refresh" {
                    self.browser.review_retry_force.insert(album.clone());
                }
                if self.browser.review_local_active
                    || self.browser.review_retry_local_active
                    || !self.browser.review_retry_albums.is_empty()
                {
                    if self.browser.review_retry_albums.contains(&album)
                        || self.browser.review_retry_extra.contains(&album)
                    {
                        return Ok(
                            serde_json::json!({"message":"Album is already queued in the running reconciliation queue."}),
                        );
                    }
                    if self.browser.review_local_active {
                        if !self.browser.review_retry_requested {
                            self.browser.review_retry_single = true;
                        }
                        self.browser.review_retry_requested = true;
                    }
                    self.browser.review_retry_seen.insert(album.clone());
                    self.browser.review_retry_extra.push(album);
                    self.browser.review_retry_totals[0] += 1;
                    return Ok(
                        serde_json::json!({"message":"Album added to the running reconciliation queue."}),
                    );
                }
                if self
                    .spotify_album_matcher
                    .as_ref()
                    .is_some_and(|m| m.pending_count() > 0)
                {
                    return Err("Spotify Album reconciliation is running; retry this Album when it finishes".into());
                }
                self.browser.review_run_active = true;
                self.browser.review_run_automatic = false;
                self.browser.review_retry_single = true;
                self.browser.error.clear();
                self.browser.review_provider_blocked = false;
                self.browser.review_playable_searches = 0;
                self.browser.review_retry_prepared = false;
                self.browser.review_retry_preparing = None;
                self.browser.review_retry_extra = vec![album];
                self.browser.review_retry_totals = [
                    1,
                    0,
                    0,
                    self.session.library.unresolved_spotify_count().unwrap_or(0),
                    0,
                ];
                self.begin_retry_unresolved_spotify();
                Ok(
                    serde_json::json!({"message":"Album reconciliation started; cached evidence is reevaluated before bounded discovery."}),
                )
            }
            "track-album" => {
                let album = self
                    .session
                    .library
                    .album_id_for_track(&music_library::domain::TrackId(id.into()))
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::json!({"album":album}))
            }
            _ => Err("Unknown Spotify review action".into()),
        }
    }
    fn spotify_mapping_candidates(&self) -> Vec<music_library::song_resolution::Candidate> {
        let mut options = vec![];
        for programs in &self.spotify_review_programs {
            for program in &programs.programs {
                for t in &program.tracks {
                    if let Some(identity) = t
                        .identities
                        .iter()
                        .find(|id| id.provider == "spotify" && id.kind == "track")
                    {
                        options.push(music_library::song_resolution::Candidate {
                            recording: Default::default(),
                            identity: identity.clone(),
                            album_identity: Some(programs.album.clone()),
                            title: t.title.clone().unwrap_or_default(),
                            artist: t
                                .artists
                                .iter()
                                .map(|a| a.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", "),
                            artists: t.artists.clone(),
                            album: programs.album.external_id.clone(),
                            date: String::new(),
                            album_artists: vec![],
                            album_type: String::new(),
                            album_total_tracks: Some(program.tracks.len() as u32),
                            duration_ms: t.duration_ms.unwrap_or(0),
                            disc: t.disc.unwrap_or(1),
                            number: t.number.unwrap_or(0),
                        });
                    }
                }
            }
        }
        options
    }
}
