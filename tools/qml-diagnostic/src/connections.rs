use crate::Bridge;
use music_library::domain::{AlbumId, TrackId};
impl Bridge {
    pub fn connections_impl(
        &mut self,
        action: &str,
        provider: &str,
        id: &str,
        marked: bool,
    ) -> String {
        let result = (|| -> Result<serde_json::Value, String> {
            let lib = &mut self.session.library;
            let album = AlbumId(id.into());
            let track = TrackId(id.into());
            let value = match action {
                "header" => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64;
                    serde_json::json!({"cooldown": if provider=="spotify" {lib.spotify_cooldown_remaining(now).map_err(|e|e.to_string())?} else {0}})
                }
                "providers" => serde_json::json!(music_library::connections::PROVIDERS),
                "groups" => serde_json::to_value(
                    lib.connection_groups(
                        provider,
                        marked,
                        if id.is_empty() { None } else { Some(&album) },
                        100,
                    )
                    .map_err(|e| e.to_string())?,
                )
                .unwrap(),
                "tracks" => serde_json::to_value(
                    lib.connection_tracks(
                        provider,
                        marked,
                        &AlbumId(id.split('\n').next().unwrap_or("").into()),
                        id.split_once('\n').map(|(_, t)| TrackId(t.into())).as_ref(),
                        200,
                    )
                    .map_err(|e| e.to_string())?,
                )
                .unwrap(),
                "retry" if provider == "spotify" => {
                    self.retry_unresolved_spotify();
                    serde_json::json!({})
                }
                "coverage" => {
                    let database = lib
                        .database_path()
                        .ok_or("Coverage requires a durable Library")?;
                    let provider = provider.to_owned();
                    let requested_provider = provider.clone();
                    let (send, recv) = std::sync::mpsc::channel();
                    std::thread::spawn(move || {
                        let result = music_library::Library::open(database)
                            .and_then(|library| library.connection_coverage(&provider))
                            .map_err(|e| e.to_string());
                        let _ = send.send(result);
                    });
                    self.connection_catalog_state.coverage = Some((requested_provider, recv));
                    serde_json::json!({"pending":true})
                }
                "poll-coverage" => {
                    if let Some((provider, receiver)) = &self.connection_catalog_state.coverage {
                        let provider = provider.clone();
                        match receiver.try_recv() {
                            Ok(result) => {
                                self.connection_catalog_state.coverage = None;
                                serde_json::json!({"provider":provider,"coverage":result?,"pending":false})
                            }
                            Err(std::sync::mpsc::TryRecvError::Empty) => {
                                serde_json::json!({"pending":true})
                            }
                            Err(e) => {
                                self.connection_catalog_state.coverage = None;
                                return Err(e.to_string());
                            }
                        }
                    } else {
                        serde_json::json!({"pending":false})
                    }
                }
                "roots" => serde_json::json!(
                    lib.local_locations()
                        .map_err(|e| e.to_string())?
                        .iter()
                        .map(|r| serde_json::json!({"id":r.id,"path":r.path.to_string_lossy()}))
                        .collect::<Vec<_>>()
                ),
                "add-root" => {
                    let path = url::Url::parse(id)
                        .map_err(|e| e.to_string())?
                        .to_file_path()
                        .map_err(|_| "Invalid folder")?;
                    let root = lib.register_local_root(path).map_err(|e| e.to_string())?;
                    serde_json::json!({"id":root})
                }
                "rescan-root" if provider == "local" => {
                    self.start_local_import(
                        music_library::local_ingestion::Request::Rescan(
                            music_library::domain::RootId(id.into()),
                        ),
                        0,
                    );
                    serde_json::json!({})
                }
                "remove-root" => {
                    lib.remove_local_location(&music_library::domain::RootId(id.into()))
                        .map_err(|e| e.to_string())?;
                    serde_json::json!({})
                }
                "mark-album" => {
                    serde_json::json!({"changed":lib.mark_connection_unavailable(provider,Some(&album),None).map_err(|e|e.to_string())?})
                }
                "mark-track" => {
                    serde_json::json!({"changed":lib.mark_connection_unavailable(provider,None,Some(&track)).map_err(|e|e.to_string())?})
                }
                "clear" => {
                    lib.clear_connection_unavailable(provider, &track)
                        .map_err(|e| e.to_string())?;
                    serde_json::json!({})
                }
                "attach-file" => {
                    let (track, url) = id.split_once('\n').ok_or("Choose a Track and file")?;
                    let path = url::Url::parse(url)
                        .map_err(|e| e.to_string())?
                        .to_file_path()
                        .map_err(|_| "Invalid file")?;
                    if self.connection_catalog_state.attachment.is_some() {
                        return Err("A file attachment is already running".into());
                    }
                    let database = lib
                        .database_path()
                        .ok_or("File attachment requires a durable Library")?;
                    let track = TrackId(track.into());
                    self.connection_catalog_state.attachment_target =
                        Some((track.clone(), self.connection_review_state.generation));
                    let (send, recv) = std::sync::mpsc::channel();
                    std::thread::spawn(move || {
                        let result = music_library::Library::open(database)
                            .and_then(|mut library| {
                                library.attach_local_file(
                                    &track,
                                    &path,
                                    &mut music_library::filesystem::LoftyMetadataExtractor,
                                )
                            })
                            .map_err(|e| e.to_string());
                        let _ = send.send(result);
                    });
                    self.connection_catalog_state.attachment = Some(recv);
                    serde_json::json!({"pending":true})
                }
                "poll-attachment" => {
                    if let Some(receiver) = &self.connection_catalog_state.attachment {
                        match receiver.try_recv() {
                            Ok(result) => {
                                self.connection_catalog_state.attachment = None;
                                let target = self.connection_catalog_state.attachment_target.take();
                                if let Err(error) = result {
                                    if target.as_ref().is_some_and(|(_, g)| {
                                        *g == self.connection_review_state.generation
                                    }) {
                                        self.connection_review_state.error = error.clone();
                                    }
                                    return Err(error);
                                }
                                self.browse_action_impl("refresh", 0, String::new());
                                if let Some((track, generation)) = target {
                                    self.finish_connection_review(
                                        "local",
                                        Some(&track),
                                        None,
                                        Some(generation),
                                    );
                                }
                                serde_json::json!({"pending":false})
                            }
                            Err(std::sync::mpsc::TryRecvError::Empty) => {
                                serde_json::json!({"pending":true})
                            }
                            Err(e) => {
                                self.connection_catalog_state.attachment = None;
                                return Err(e.to_string());
                            }
                        }
                    } else {
                        serde_json::json!({"pending":false})
                    }
                }
                _ => return Err("Unknown connection action".into()),
            };
            Ok(value)
        })();
        match result {
            Ok(v) => v.to_string(),
            Err(e) => serde_json::json!({"error":e}).to_string(),
        }
    }
}

enum CatalogReply {
    Search(Vec<music_library::catalog::AlbumCandidate>),
    Inspect(
        Option<music_library::catalog::Release>,
        music_library::album_program::Programs,
    ),
}
#[derive(Default)]
pub struct CatalogState {
    review_album: Option<AlbumId>,
    attachment_target: Option<(TrackId, u64)>,
    coverage: Option<(
        String,
        std::sync::mpsc::Receiver<Result<music_library::connections::Coverage, String>>,
    )>,
    attachment: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    receiver: Option<
        std::sync::mpsc::Receiver<Result<CatalogReply, music_library::catalog::CatalogError>>,
    >,
    candidates: Vec<music_library::catalog::AlbumCandidate>,
    release: Option<music_library::catalog::Release>,
    programs: Option<music_library::album_program::Programs>,
}
impl Bridge {
    pub fn connection_catalog_impl(&mut self, action: &str, value: &str, index: i32) -> String {
        let result = (|| -> Result<serde_json::Value, String> {
            if action == "poll" {
                if let Some(receiver) = &self.connection_catalog_state.receiver {
                    match receiver.try_recv() {
                        Ok(result) => {
                            self.connection_catalog_state.receiver = None;
                            let result = result.map_err(|e| {
                                if let Some(album) = &self.connection_catalog_state.review_album {
                                    let _ = self.session.library.musicbrainz_state(
                                        album,
                                        if e.is_provider_unavailable() {music_library::musicbrainz_lifecycle::Reason::ProviderDeferred}else{music_library::musicbrainz_lifecycle::Reason::ProviderError},
                                    );
                                }
                                e.to_string()
                            })?;
                            match result {
                                CatalogReply::Search(rows) => {
                                    if let Some(album) = &self.connection_catalog_state.review_album
                                    {
                                        self.session
                                            .library
                                            .retain_musicbrainz_candidates(album, &rows, true)
                                            .map_err(|e| e.to_string())?;
                                    }
                                    self.connection_catalog_state.candidates = rows;
                                    self.connection_catalog_state.release = None;
                                    self.connection_catalog_state.programs = None;
                                }
                                CatalogReply::Inspect(release, programs) => {
                                    if let Some(album) = &self.connection_catalog_state.review_album
                                    {
                                        self.session
                                            .library
                                            .retain_musicbrainz_inspection(album, &programs)
                                            .map_err(|e| e.to_string())?;
                                    }
                                    self.connection_catalog_state.programs = Some(programs);
                                    self.connection_catalog_state.release = release
                                }
                            }
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => {
                            return Ok(serde_json::json!({"pending":true}));
                        }
                        Err(e) => {
                            self.connection_catalog_state.receiver = None;
                            return Err(e.to_string());
                        }
                    }
                }
                let rows=self.connection_catalog_state.candidates.iter().map(|a|serde_json::json!({"title":a.title,"artist":music_library::catalog::credit_display(&a.credits),"id":a.identity.external_id,"date":a.date})).collect::<Vec<_>>();
                let tracks=self.connection_catalog_state.release.iter().flat_map(|r|r.media.iter()).flat_map(|m|m.tracks.iter().map(move|t|serde_json::json!({"title":t.title,"disc":m.position,"number":t.position,"artist":music_library::catalog::credit_display(&t.credits),"ids":t.identities}))).collect::<Vec<_>>();
                let release = self.connection_catalog_state.release.as_ref().map(
                    |r| serde_json::json!({"title":r.title,"date":r.date,"identity":r.identity}),
                );
                return Ok(
                    serde_json::json!({"pending":false,"candidates":rows,"tracks":tracks,"release":release}),
                );
            }
            if action == "confirm" {
                let release = self
                    .connection_catalog_state
                    .release
                    .as_ref()
                    .ok_or("Inspect an Album first")?;
                let t = release
                    .media
                    .iter()
                    .flat_map(|m| m.tracks.iter())
                    .nth(index as usize)
                    .ok_or("Select a Recording")?;
                let ids = t
                    .identities
                    .iter()
                    .filter(|i| i.provider == "musicbrainz" && i.kind == "recording")
                    .collect::<Vec<_>>();
                if ids.len() != 1 {
                    return Err("Candidate requires one MusicBrainz Recording identity".into());
                }
                self.session
                    .library
                    .confirm_musicbrainz_connection(&TrackId(value.into()), ids[0])
                    .map_err(|e| e.to_string())?;
                self.finish_connection_review(
                    "musicbrainz",
                    Some(&TrackId(value.into())),
                    None,
                    None,
                );
                return Ok(serde_json::json!({"confirmed":true}));
            }
            if self.connection_catalog_state.receiver.is_some() {
                return Err("Catalog request is already running".into());
            }
            let candidate = if action == "inspect" {
                Some(
                    self.connection_catalog_state
                        .candidates
                        .get(index as usize)
                        .ok_or("Select an Album")?
                        .clone(),
                )
            } else {
                None
            };
            if let (Some(album), Some(candidate)) =
                (&self.connection_catalog_state.review_album, &candidate)
            {
                let retained = self
                    .session
                    .library
                    .musicbrainz_review(album)
                    .map_err(|e| e.to_string())?;
                if let Some(c) = retained.candidates.into_iter().find(|c| {
                    c.metadata.identity == candidate.identity
                        && Some(c.review_observed_at()).is_some_and(|at| {
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs() as i64
                                - at
                                < music_library::musicbrainz_lifecycle::OBSERVATION_TTL
                        })
                }) {
                    if let Some(p) = c.review_programs().cloned() {
                        self.connection_catalog_state.programs = Some(p);
                        return Ok(serde_json::json!({"pending":false,"retained":true}));
                    }
                }
            }
            if action != "search" && action != "search-global" && action != "inspect" {
                return Err("Unknown catalog action".into());
            }
            if let Some(album) = &self.connection_catalog_state.review_album {
                if let Some(c) = &candidate {
                    self.session
                        .library
                        .retain_musicbrainz_candidates(album, std::slice::from_ref(c), true)
                        .map_err(|e| e.to_string())?;
                }
            }
            let context = if action != "search-global" {
                self.connection_catalog_state
                    .review_album
                    .as_ref()
                    .and_then(|album| {
                        match self
                            .session
                            .library
                            .prepare_musicbrainz_discovery(album)
                            .ok()?
                        {
                            music_library::album_matching::Preparation::Ready(input) => Some(input),
                            _ => None,
                        }
                    })
            } else {
                None
            };
            let retained = if context.as_ref().is_some_and(|c| {
                music_library::matching::normalize_album_title(&c.raw_title)
                    == music_library::matching::normalize_album_title(value)
            }) {
                self.connection_catalog_state
                    .review_album
                    .as_ref()
                    .and_then(|a| self.session.library.musicbrainz_review(a).ok())
                    .map(|r| r.candidates)
                    .unwrap_or_default()
            } else {
                vec![]
            };
            let query = value.to_owned();
            let (send, recv) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                use music_library::catalog::CatalogProvider;
                let mut catalog = music_library_musicbrainz::MusicBrainz::new();
                let result = if let Some(candidate) = candidate {
                    catalog
                        .inspect_album(&candidate)
                        .map(|(r, p)| CatalogReply::Inspect(r, p))
                } else {
                    if let Some(context) = context {
                        let artist = match context.known_artist {
                            Some(artist) => Ok(artist),
                            None => catalog.search_artists(&context.artist).and_then(|artists| {
                                music_library::album_matching::resolve_artist(
                                    &context.artist,
                                    &artists,
                                )
                                .map_err(|_| {
                                    music_library::catalog::CatalogError::Other(
                                        "Artist identity needs review".into(),
                                    )
                                })
                            }),
                        };
                        artist.and_then(|artist| {
                            catalog.artist_albums(&artist, &query).map(|p| {
                                let mut rows = retained
                                    .into_iter()
                                    .filter(|c| {
                                        c.metadata
                                            .credits
                                            .iter()
                                            .any(|credit| credit.identity.as_ref() == Some(&artist))
                                    })
                                    .map(|c| c.metadata)
                                    .collect::<Vec<_>>();
                                for c in p.items {
                                    if !rows.iter().any(|r| r.identity == c.identity) {
                                        if let Some(c) =
                                            catalog.album_candidate_observation(&c.identity)
                                        {
                                            rows.push(c);
                                        }
                                    }
                                }
                                CatalogReply::Search(rows)
                            })
                        })
                    } else {
                        catalog
                            .search_albums(&query, 0)
                            .map(|p| CatalogReply::Search(p.items))
                    }
                };
                let _ = send.send(result);
            });
            self.connection_catalog_state.receiver = Some(recv);
            Ok(serde_json::json!({"pending":true}))
        })();
        match result {
            Ok(v) => v.to_string(),
            Err(e) => serde_json::json!({"error":e}).to_string(),
        }
    }
}

impl CatalogState {
    #[cfg(test)]
    pub fn idle(&self) -> bool {
        self.receiver.is_none()
    }
}

impl Bridge {
    pub fn reset_connection_catalog_review(
        &mut self,
        album: Option<&AlbumId>,
    ) -> Option<music_library::album_program::Programs> {
        let retained = if self.connection_catalog_state.review_album.as_ref() == album {
            self.connection_catalog_state
                .release
                .as_ref()
                .and_then(|_| self.connection_catalog_state.programs.clone())
        } else {
            None
        };
        // Dropping the receiver invalidates old async replies without cancelling an authorized write.
        self.connection_catalog_state.receiver = None;
        self.connection_catalog_state.candidates.clear();
        if album.is_some() && self.connection_catalog_state.review_album.as_ref() != album {
            self.connection_catalog_state.release = None;
            self.connection_catalog_state.programs = None;
        }
        if let Some(album) = album {
            if let Ok(review) = self.session.library.musicbrainz_review(album) {
                self.connection_catalog_state.candidates =
                    review.candidates.into_iter().map(|c| c.metadata).collect();
            }
            self.connection_catalog_state.review_album = Some(album.clone());
        }
        retained
    }
}

impl Bridge {
    pub fn connection_catalog_review_candidate(
        &self,
        album: &AlbumId,
    ) -> Option<(music_library::album_program::Programs, serde_json::Value)> {
        if self.connection_catalog_state.review_album.as_ref() != Some(album) {
            return None;
        }
        if let Some(p) = &self.connection_catalog_state.programs {
            let c = self
                .connection_catalog_state
                .candidates
                .iter()
                .find(|c| c.identity == p.album);
            return Some((p.clone(),c.map(|c|serde_json::json!({"title":c.title,"album":c.title,"artist":c.artist,"date":c.date,"kind":c.primary_type})).unwrap_or(serde_json::Value::Null)));
        }
        self.connection_catalog_state.release.as_ref().map(|release|(
            self.connection_catalog_state.programs.clone().unwrap_or_else(||crate::connection_review::release_program(release)),
            serde_json::json!({"title":release.album.title,"album":release.album.title,"artist":music_library::catalog::credit_display(&release.album.credits),"date":release.date,"kind":release.album.release_type})
        ))
    }
}
