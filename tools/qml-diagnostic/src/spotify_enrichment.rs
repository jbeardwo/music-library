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
            bridge.refresh_spotify_review();
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
        let selected_changed = album.as_ref() == bridge.spotify_album_id.as_ref();
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
        bridge.start_review_track_retry();
        bridge.refresh_spotify_review();
        if selected_changed {
            bridge.refresh_spotify_album_diagnostic();
        }
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

    pub(crate) fn ensure_spotify_album_matcher(&mut self) -> Result<(), String> {
        if self.spotify_album_matcher.is_none() {
            // from_env only reads configuration; token/API work stays on the worker.
            let Ok(provider) = music_library_spotify::Spotify::from_env() else {
                return Err(
                    "Spotify catalog configuration is needed to evaluate Album evidence".into(),
                );
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
                    return Err(error.to_string());
                }
            }
        }
        Ok(())
    }
    pub(crate) fn refresh_spotify_album_diagnostic(&mut self) {
        use music_library::album_candidates::Reason;
        let Some(track) = self.spotify_playback_track.clone() else {
            return;
        };
        let album = self.session.library.album_id_for_track(&track).ok();
        self.spotify_album_id = album.clone();
        self.spotify_album_explanation = Reason::NotEvaluated.label().into();
        if let Some(album) = &album {
            if let Some(report) = self
                .spotify_album_matcher
                .as_ref()
                .and_then(|m| m.diagnostic(album))
            {
                self.spotify_album_explanation = report.explain_track(track.as_ref());
                if let Some(music_library::album_matching::MatchOutcome::Error(error)) = self
                    .spotify_album_matcher
                    .as_ref()
                    .and_then(|m| m.outcome(album))
                {
                    self.spotify_album_explanation
                        .push_str(&format!("\nCompletion error: {error}"));
                }
            } else if let Some(matcher) = self.spotify_album_matcher.as_ref() {
                use music_library::album_matching::MatchOutcome;
                self.spotify_album_explanation = match matcher.outcome(album) {
                    Some(MatchOutcome::Error(e) | MatchOutcome::ConfigurationError(e)) => {
                        format!("Provider evaluation failed: {e}")
                    }
                    Some(MatchOutcome::Deferred(e)) => {
                        format!("Provider unavailable; evaluation withheld: {e}")
                    }
                    Some(MatchOutcome::ArtistAmbiguous(_)) => {
                        Reason::ArtistUnresolved.label().into()
                    }
                    _ => Reason::NotEvaluated.label().into(),
                };
                if let Some(programs) = matcher.cached_programs(album)
                    && let Ok(local) = self.session.library.local_album_tracks(album)
                {
                    self.spotify_album_explanation = format!(
                        "Established Spotify Album: {}\n{}\n",
                        programs.album.external_id,
                        Reason::AlreadyAssociated.label()
                    );
                    for program in &programs.programs {
                        let evidence = music_library::album_program::inspect_positioned_program(
                            &local, program,
                        );
                        self.spotify_album_explanation.push_str(
                            &music_library::album_candidates::explain_positioned_program(
                                &evidence,
                                track.as_ref(),
                            ),
                        );
                    }
                }
            }
        }
        self.spotify_playback_song = self
            .session
            .library
            .track_provider_occurrences(&track, "spotify")
            .ok()
            .and_then(|ids| music_library_spotify::playback::Song::from_associations(&ids).ok());
        // Refresh from persisted identity state even when this diagnostic was
        // restored from cached Album evidence and the review window was inactive.
        self.refresh_spotify_review();
    }
    pub(crate) fn retry_selected_spotify_album(&mut self) {
        let Some(track) = self.spotify_playback_track.clone() else {
            return;
        };
        if self
            .session
            .library
            .track_provider_occurrences(&track, "spotify")
            .is_ok_and(|ids| !ids.is_empty())
        {
            self.refresh_spotify_album_diagnostic();
            self.spotify_playback_changed();
            return;
        }
        if self
            .session
            .library
            .spotify_manually_excluded(&track)
            .unwrap_or(true)
        {
            return;
        }
        let result = (|| -> Result<(), String> {
            let album = self
                .session
                .library
                .album_id_for_track(&track)
                .map_err(|e| e.to_string())?;
            // Trust is retained. match_album skips accepted Album discovery and
            // may enrich missing occurrences through its existing program path.
            self.ensure_spotify_album_matcher()?;
            self.spotify_album_matcher
                .as_mut()
                .unwrap()
                .match_album(&self.session.library, &album)
                .map_err(|e| e.to_string())?;
            Ok(())
        })();
        self.refresh_spotify_album_diagnostic();
        if let Err(error) = result {
            self.spotify_album_explanation = error;
        }
        self.spotify_playback_changed();
    }
    pub(crate) fn enrich_catalog_spotify(&mut self, imported: &ImportedRelease) {
        if !self.auto_match {
            return;
        }
        if let Err(error) = self.ensure_spotify_album_matcher() {
            self.session.error = error;
            return;
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

#[cfg(test)]
mod tests {
    use super::*;
    use music_library::{catalog::*, domain::*, edition::TrackEvidence};
    use qmetaobject::{QObjectBox, QmlEngine};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    fn id(kind: &str, n: usize) -> ExternalIdentity {
        ExternalIdentity {
            provider: "spotify".into(),
            kind: kind.into(),
            external_id: format!("{n:022}"),
        }
    }
    struct Provider {
        calls: Arc<AtomicUsize>,
        tracks: Vec<TrackEvidence>,
    }
    impl CatalogProvider for Provider {
        fn search_albums(&mut self, _: &str, _: u32) -> Result<Page<AlbumCandidate>, CatalogError> {
            unreachable!()
        }
        fn releases(
            &mut self,
            _: &ExternalIdentity,
            _: u32,
        ) -> Result<Page<ReleaseCandidate>, CatalogError> {
            unreachable!()
        }
        fn release(&mut self, _: &ExternalIdentity) -> Result<Release, CatalogError> {
            unreachable!()
        }
        fn search_artists(&mut self, _: &str) -> Result<Page<ArtistCandidate>, CatalogError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Page {
                items: vec![ArtistCandidate {
                    identity: id("artist", 1),
                    name: "Artist".into(),
                    aliases: vec![],
                    comment: String::new(),
                    country: String::new(),
                    artist_type: String::new(),
                    score: None,
                }],
                next_offset: None,
            })
        }
        fn artist_albums(
            &mut self,
            _: &ExternalIdentity,
            _: &str,
        ) -> Result<Page<ArtistAlbumCandidate>, CatalogError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Page {
                items: vec![ArtistAlbumCandidate {
                    identity: id("album", 2),
                    artist: "Artist".into(),
                    artist_ids: vec![id("artist", 1)],
                    title: "Hugs".into(),
                    primary_type: "Single".into(),
                    date: "2010-02-28".into(),
                    comment: String::new(),
                }],
                next_offset: None,
            })
        }
        fn album_candidate_programs(&self) -> bool {
            true
        }
        fn album_candidate_track_count(&self, _: &ExternalIdentity) -> Option<u32> {
            Some(3)
        }
        fn album_program_namespaces(&self) -> Vec<(String, String)> {
            vec![("spotify".into(), "album".into())]
        }
        fn album_programs(
            &mut self,
            album: &ExternalIdentity,
        ) -> Result<album_program::Programs, CatalogError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(album_program::Programs {
                album: album.clone(),
                programs: vec![album_program::Program {
                    identity: None,
                    tracks: self.tracks.clone(),
                    complete: true,
                }],
                note: String::new(),
            })
        }
    }
    #[test]
    fn selected_song_album_retry_renders_real_evidence_without_playback_or_association_churn() {
        let temp = tempfile::tempdir().unwrap();
        let mut library = music_library::Library::open(temp.path().join("db")).unwrap();
        let release = library
            .create_catalog_release(&CatalogReleaseInput {
                title: "Hugs EP".into(),
                year: None,
                artists: vec![ArtistCreditInput {
                    name: "Artist".into(),
                    role: None,
                }],
                tracks: (1..=3)
                    .map(|n| CatalogTrackInput {
                        title: format!("Song {n}"),
                        disc_number: Some(1),
                        track_number: Some(n),
                        artists: vec![],
                    })
                    .collect(),
            })
            .unwrap();
        let other = library
            .create_catalog_release(&CatalogReleaseInput {
                title: "Different Album".into(),
                year: None,
                artists: vec![],
                tracks: vec![CatalogTrackInput {
                    title: "Now Playing".into(),
                    disc_number: Some(1),
                    track_number: Some(1),
                    artists: vec![],
                }],
            })
            .unwrap()
            .track_ids
            .remove(0);
        let album = library
            .album_for_release(&release.release_id)
            .unwrap()
            .album_id;
        let local = library.local_album_tracks(&album).unwrap();
        for track in &release.track_ids {
            library.add_to_library(track).unwrap();
        }
        let tracks = local
            .iter()
            .enumerate()
            .map(|(n, t)| {
                let mut e = t.evidence.clone();
                e.identities = vec![id("track", n + 3)];
                e
            })
            .collect();
        let mut engine = QmlEngine::new();
        let bridge = QObjectBox::new(Bridge::new(crate::session::Session::new(library)));
        let calls = Arc::new(AtomicUsize::new(0));
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.auto_match = false;
            b.session.playback.enqueue(other.clone());
            b.session.playback.select_queue_position(0).unwrap();
            let albums = callback(qmetaobject::QPointer::from(&**b));
            let programs = callback(qmetaobject::QPointer::from(&**b));
            b.spotify_album_matcher = Some(
                AlbumMatcher::for_provider(
                    Provider {
                        calls: calls.clone(),
                        tracks,
                    },
                    music_library_spotify::matching_scope(),
                    move |r| albums(Event::Album(Box::new(r))),
                    move |r| programs(Event::Programs(Box::new(r))),
                    |_| {},
                )
                .unwrap(),
            );
        }
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n    function ready() {{",
                    include_str!("../SpotifyAlbumEvidenceTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        assert_eq!(
            engine
                .invoke_method(
                    "exerciseSpotifyAlbumEvidence".into(),
                    &[
                        crate::string(release.track_ids[1].as_ref()),
                        crate::string(other.as_ref())
                    ]
                )
                .to_qstring()
                .to_string(),
            "ok"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "Artist search, Album search and one reused program; no per-Track calls"
        );
        // A restored diagnostic must invalidate a review snapshot even when the
        // identity was persisted outside this matcher's completion callback.
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.session.library.add_to_library(&other).unwrap();
            b.refresh_spotify_review();
        }
        assert_eq!(
            engine
                .invoke_method("albumEvidenceReviewCount".into(), &[])
                .to_int(),
            1
        );
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.session
                .library
                .attach_track_external_identity(&other, &id("track", 500))
                .unwrap();
            b.spotify_playback_track = Some(other.clone());
            b.refresh_spotify_album_diagnostic();
        }
        assert_eq!(
            engine
                .invoke_method("albumEvidenceReviewCount".into(), &[])
                .to_int(),
            0
        );
        bridge.pinned().borrow_mut().spotify_playback_track = Some(release.track_ids[1].clone());
        let before = bridge
            .pinned()
            .borrow()
            .session
            .library
            .track_provider_occurrences(&release.track_ids[1], "spotify")
            .unwrap();
        bridge.pinned().borrow_mut().retry_selected_spotify_album();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "trusted associations skip retry requests"
        );
        assert_eq!(
            bridge
                .pinned()
                .borrow()
                .session
                .library
                .track_provider_occurrences(&release.track_ids[1], "spotify")
                .unwrap(),
            before
        );
        bridge.pinned().borrow_mut().spotify_album_matcher.take();
    }
    #[test]
    #[ignore = "live Spotify diagnostic retry on a disposable real library copy"]
    fn live_selected_album_reconciliation_evidence() {
        let path = std::env::var("MUSIC_LIBRARY_DIAGNOSTIC_DATABASE").unwrap();
        assert!(path.starts_with("/tmp/"), "disposable library required");
        let library = music_library::Library::open(path).unwrap();
        let cases = [
            (
                "d335bff0-b96d-4dbf-9022-dc01734f0487",
                true,
                "release-type suffix normalized",
            ),
            (
                "169f997a-d191-4ccd-bb27-9e1d95371fa8",
                false,
                "Candidate Artist IDs differ",
            ),
            (
                "8a4b0b05-8989-4c24-b5f2-0bd85100585a",
                true,
                "exact Album title",
            ),
            (
                "231a1019-5b1c-4234-87af-7de602da1a3c",
                true,
                "release-type suffix normalized",
            ),
            (
                "7ab7c3bf-fbee-4bd9-92e9-fe1dd802424e",
                false,
                "Meaningful Album/version qualifier differs",
            ),
        ];
        let tracks: Vec<_> = cases
            .iter()
            .map(|(id, _, _)| {
                library
                    .local_album_tracks(&AlbumId((*id).to_owned()))
                    .unwrap()[0]
                    .track_id
                    .clone()
            })
            .collect();
        let mut engine = QmlEngine::new();
        let bridge = QObjectBox::new(Bridge::new(crate::session::Session::new(library)));
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.auto_match = false;
            b.session.playback.enqueue(tracks[4].clone());
            b.session.playback.select_queue_position(0).unwrap();
        }
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!(
                    "{}\n    function ready() {{",
                    include_str!("../SpotifyAlbumEvidenceTest.qml")
                ),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        for ((album, connected, reason), track) in cases.iter().zip(&tracks) {
            let result = engine
                .invoke_method(
                    "exerciseLiveAlbumEvidence".into(),
                    &[
                        crate::string(track.as_ref()),
                        (*connected).into(),
                        crate::string(reason),
                    ],
                )
                .to_qstring()
                .to_string();
            eprintln!(
                "Real Album {album}: {}",
                bridge.pinned().borrow().spotify_album_explanation
            );
            assert_eq!(result, "ok");
        }
        bridge.pinned().borrow_mut().spotify_album_matcher.take();
    }
}
