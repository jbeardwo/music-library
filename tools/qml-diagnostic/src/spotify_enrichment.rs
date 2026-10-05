//! Catalog-import Spotify enrichment is independent of the metadata-provider chain.
//! Reuses the application Album matcher; never called by transport or polling.
use crate::Bridge;
use music_library::{
    album_matching::{AlbumMatcher, AutoMatchPolicy, MatchReply},
    album_program,
    domain::ImportedRelease,
};

enum Event {
    Album(Box<MatchReply>),
    Programs(Box<album_program::Reply>),
    Retry(u64),
    Incoming(
        String,
        music_library::domain::ReleaseId,
        Result<album_program::Programs, music_library::catalog::CatalogError>,
    ),
}
fn callback(weak: qmetaobject::QPointer<Bridge>) -> impl Fn(Event) + Send + 'static {
    qmetaobject::queued_callback(move |event| {
        let Some(pinned) = weak.as_pinned() else {
            return;
        };
        let mut bridge = pinned.borrow_mut();
        if let Event::Incoming(playlist, release, result) = event {
            match result.and_then(|programs| {
                bridge
                    .session
                    .library
                    .reconcile_playlist_album_program(&playlist, &release, &programs)
                    .map_err(|e| music_library::catalog::CatalogError::Other(e.to_string()))
            }) {
                Ok(changed) => {
                    bridge.spotify_resolution_message =
                        format!("Reconciled {changed} playlist entries with local Tracks");
                    if changed > 0 {
                        bridge.browse_action_impl("playlist-revision-check", 0, String::new());
                    }
                }
                Err(error) => {
                    bridge.spotify_resolution_message = format!("Playlist reconciliation: {error}")
                }
            }
            bridge.spotify_playback_changed();
            bridge.changed();
            return;
        }
        let Some(mut matcher) = bridge.spotify_album_matcher.take() else {
            return;
        };
        let album = match event {
            Event::Album(reply) => {
                let album = reply.input.album_id.clone();
                matcher.complete(&mut bridge.session.library, *reply);
                Some(album)
            }
            Event::Programs(reply) => {
                let album = reply.input.album_id.clone();
                matcher.complete_programs(&mut bridge.session.library, *reply);
                Some(album)
            }
            Event::Incoming(..) => unreachable!(),
            Event::Retry(token) => {
                if let Err(error) = matcher.cooldown_elapsed(&bridge.session.library, token) {
                    bridge.session.error = error.to_string();
                }
                None
            }
        };
        if let Some(album) = album {
            let tracks = bridge
                .session
                .library
                .local_album_tracks(&album)
                .unwrap_or_default();
            let associated = bridge
                .session
                .library
                .provider_track_associations(&album, "spotify")
                .unwrap_or_default()
                .iter()
                .filter(|(_, m)| !m.occurrences.is_empty())
                .count();
            use music_library::album_matching::MatchOutcome;
            let status = match matcher.outcome(&album) {
                Some(MatchOutcome::Pending) => "matching Album",
                Some(MatchOutcome::Deferred(_)) => "provider unavailable; queued for retry",
                Some(MatchOutcome::Error(_) | MatchOutcome::ConfigurationError(_)) => {
                    "matching failed"
                }
                Some(
                    MatchOutcome::Matched(_)
                    | MatchOutcome::MatchedClose(_)
                    | MatchOutcome::AlreadyMatched,
                ) => "Album found",
                _ => "Album representation unresolved",
            };
            let message = format!(
                "Spotify: {associated}/{} Tracks associated; {status}",
                tracks.len()
            );
            if !bridge.spotify_resolution_pending && bridge.spotify_resolution_selection.is_none() {
                bridge.spotify_resolution_message = message;
            }
        }
        let playlists: Vec<_> = bridge.spotify_reconcile_playlists.iter().cloned().collect();
        for playlist in playlists {
            match bridge.session.library.reconcile_playlist_tracks(&playlist) {
                Ok(changed) if changed > 0 => {
                    bridge.browse_action_impl("playlist-revision-check", 0, String::new())
                }
                Err(error) => bridge.session.error = error.to_string(),
                _ => {}
            }
        }
        if matcher.pending_count() == 0 {
            let playlists: Vec<_> = bridge.spotify_reconcile_playlists.drain().collect();
            for playlist in playlists {
                bridge.reconcile_incoming_programs(&playlist);
            }
        }
        bridge.spotify_album_matcher = Some(matcher);
        bridge.spotify_playback_changed();
        bridge.changed();
    })
}
impl Bridge {
    fn reconcile_incoming_programs(&mut self, playlist: &str) {
        let requests = match self
            .session
            .library
            .playlist_reconciliation_programs(playlist)
        {
            Ok(requests) => requests,
            Err(error) => {
                self.session.error = error.to_string();
                return;
            }
        };
        if requests.is_empty() {
            return;
        }
        let Ok(mut provider) = music_library_spotify::Spotify::from_env() else {
            self.spotify_resolution_message =
                "Spotify catalog configuration is needed to reconcile local Tracks".into();
            return;
        };
        let finish = callback(qmetaobject::QPointer::from(&*self));
        let playlist = playlist.to_owned();
        std::thread::spawn(move || {
            use music_library::catalog::CatalogProvider;
            let mut cache = std::collections::HashMap::<String, album_program::Programs>::new();
            for (release, identity) in requests {
                let key = format!(
                    "{}:{}:{}",
                    identity.provider, identity.kind, identity.external_id
                );
                let result = if let Some(programs) = cache.get(&key) {
                    Ok(programs.clone())
                } else {
                    provider.album_programs(&identity)
                };
                if let Ok(programs) = &result {
                    cache.insert(key, programs.clone());
                }
                let failed = result.is_err();
                finish(Event::Incoming(playlist.clone(), release, result));
                if failed {
                    break;
                }
            }
        });
    }
    pub(crate) fn reconcile_spotify_playlist(&mut self, playlist: &str) {
        match self.session.library.reconcile_playlist_tracks(playlist) {
            Ok(changed) if changed > 0 => {
                self.browse_action_impl("playlist-revision-check", 0, String::new())
            }
            Err(error) => {
                self.session.error = error.to_string();
                return;
            }
            _ => {}
        }
        let requests = match self
            .session
            .library
            .playlist_reconciliation_programs(playlist)
        {
            Ok(requests) => requests,
            Err(error) => {
                self.session.error = error.to_string();
                return;
            }
        };
        if requests.is_empty() {
            return;
        }
        let needed: std::collections::HashSet<_> = requests.iter().map(|r| r.0.clone()).collect();
        let candidates = match self
            .session
            .library
            .playlist_local_match_candidates(playlist)
        {
            Ok(candidates) => candidates,
            Err(error) => {
                self.session.error = error.to_string();
                return;
            }
        };
        self.spotify_reconcile_playlists.insert(playlist.to_owned());
        for candidate in candidates
            .into_iter()
            .filter(|c| needed.contains(&c.release_id))
        {
            self.enrich_catalog_spotify(&candidate);
        }
        if self
            .spotify_album_matcher
            .as_ref()
            .is_some_and(|m| m.pending_count() == 0)
        {
            self.spotify_reconcile_playlists.remove(playlist);
            self.reconcile_incoming_programs(playlist);
        }
    }

    pub(crate) fn enrich_catalog_spotify(&mut self, imported: &ImportedRelease) {
        if !self.auto_match {
            return;
        }
        if self.spotify_album_matcher.is_none() {
            // from_env only reads configuration; token/API work stays on the worker.
            let Ok(provider) = music_library_spotify::Spotify::from_env() else {
                return;
            };
            let albums = callback(qmetaobject::QPointer::from(&*self));
            let programs = callback(qmetaobject::QPointer::from(&*self));
            let retry = callback(qmetaobject::QPointer::from(&*self));
            match AlbumMatcher::for_provider(
                provider,
                music_library_spotify::matching_scope(),
                move |r| albums(Event::Album(Box::new(r))),
                move |r| programs(Event::Programs(Box::new(r))),
                move |t| retry(Event::Retry(t)),
            ) {
                Ok(matcher) => self.spotify_album_matcher = Some(matcher),
                Err(error) => {
                    self.session.error = error.to_string();
                    return;
                }
            }
        }
        if let Err(error) = self.spotify_album_matcher.as_mut().unwrap().after_import(
            &self.session.library,
            std::slice::from_ref(imported),
            AutoMatchPolicy::default(),
        ) {
            self.session.error = error.to_string();
        }
    }
}
