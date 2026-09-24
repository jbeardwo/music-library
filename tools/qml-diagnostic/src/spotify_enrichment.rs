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
}
fn callback(weak: qmetaobject::QPointer<Bridge>) -> impl Fn(Event) + Send + 'static {
    qmetaobject::queued_callback(move |event| {
        let Some(pinned) = weak.as_pinned() else {
            return;
        };
        let mut bridge = pinned.borrow_mut();
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
        bridge.spotify_album_matcher = Some(matcher);
        bridge.spotify_playback_changed();
        bridge.changed();
    })
}
impl Bridge {
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
