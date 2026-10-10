//! Cache-only propagation of Album-program occurrences to canonical Track review.
use crate::Spotify;
use music_library::{
    Library, Result,
    album_matching::Preparation,
    catalog::Page,
    domain::{AlbumId, TrackId},
    song_resolution::{Candidate, Selection},
};

pub struct TrackCandidates {
    pub track: TrackId,
    pub candidates: Vec<Candidate>,
    pub more: bool,
}

pub fn cached_candidates(
    library: &Library,
    album: &AlbumId,
    market: &str,
) -> Result<Vec<TrackCandidates>> {
    let Some(path) = library.database_path() else {
        return Ok(vec![]);
    };
    let Preparation::Ready(input) = library.spotify_album_review_input(album)? else {
        return Ok(vec![]);
    };
    let Some((report, programs)) = Spotify::cached_album_review(&path, market, &input)
        .map_err(|e| music_library::Error::Invalid(e.to_string()))?
    else {
        return Ok(vec![]);
    };
    let local = library.local_album_tracks(album)?;
    let mut result = vec![];
    for (index, track) in local.iter().enumerate() {
        let mut candidates = vec![];
        for row in report
            .candidates
            .iter()
            .filter(|c| c.artist_accepted && c.equivalent_to.is_none())
        {
            let Some(programs) = programs.iter().find(|p| p.album == row.identity) else {
                continue;
            };
            for (program_index, program) in programs.programs.iter().enumerate() {
                let comparison = row.programs.get(program_index);
                let mapped = comparison
                    .into_iter()
                    .flat_map(|c| c.alignment.rows.iter())
                    .find(|r| r.local_index == Some(index))
                    .and_then(|r| r.provider_index);
                let used = comparison
                    .into_iter()
                    .flat_map(|c| c.alignment.rows.iter())
                    .filter(|r| r.local_index.is_some())
                    .filter_map(|r| r.provider_index)
                    .collect::<std::collections::HashSet<_>>();
                for (_, t) in
                    program.tracks.iter().enumerate().filter(|(i, _)| {
                        mapped == Some(*i) || mapped.is_none() && !used.contains(i)
                    })
                {
                    let Some(identity) = t
                        .identities
                        .iter()
                        .find(|id| id.provider == "spotify" && id.kind == "track")
                    else {
                        continue;
                    };
                    if candidates
                        .iter()
                        .any(|c: &Candidate| c.identity == *identity)
                    {
                        continue;
                    }
                    candidates.push(Candidate {
                        recording: t.recording.clone(),
                        identity: identity.clone(),
                        album_identity: Some(row.identity.clone()),
                        title: t.title.clone().unwrap_or_default(),
                        artist: t
                            .artists
                            .iter()
                            .map(|a| a.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                        artists: t.artists.clone(),
                        album: row.title.provider_raw.clone(),
                        date: row.provider_date.clone(),
                        album_artists: row
                            .artist_ids
                            .iter()
                            .enumerate()
                            .map(|(i, id)| music_library::edition::ArtistEvidence {
                                name: if i == 0 {
                                    row.artist.clone()
                                } else {
                                    String::new()
                                },
                                identities: vec![id.clone()],
                                join_phrase: String::new(),
                            })
                            .collect(),
                        album_type: row.title.provider_type.clone(),
                        album_total_tracks: Some(program.tracks.len() as u32),
                        duration_ms: t.duration_ms.unwrap_or(0),
                        disc: t.disc.unwrap_or(1),
                        number: t.number.unwrap_or(0),
                    });
                }
            }
        }
        let more = candidates.len() > 10;
        candidates.truncate(10);
        if !candidates.is_empty() {
            result.push(TrackCandidates {
                track: track.track_id.clone(),
                candidates,
                more,
            });
        }
    }
    Ok(result)
}

/// Merge without discarding Track-search competition. Inspection does not persist.
pub fn selection(
    library: &Library,
    track: &TrackId,
    market: &str,
) -> Result<Option<(Selection, bool)>> {
    let input = library.song_resolution_input(track)?;
    let album = library.album_id_for_track(track)?;
    let inherited = cached_candidates(library, &album, market)?
        .into_iter()
        .find(|c| &c.track == track);
    let cached = library.cached_spotify_track_attempt(track)?;
    if cached.is_none() && inherited.is_none() {
        return Ok(None);
    }
    let page = cached.map(|(_, page)| page).unwrap_or(Page {
        items: vec![],
        next_offset: None,
    });
    let mut more = page.next_offset.is_some();
    let mut selection = Selection::new(input, page.items);
    if let Some(inherited) = inherited {
        more |= inherited.more;
        selection.inherit_album_candidates(inherited.candidates);
    }
    selection.show_all();
    selection.set_trusted_identities(library.diagnostic_track_identities(track)?);
    Ok(Some((selection, more)))
}

/// Targeted second phase after provider work. Each discovery is replayed once;
/// the core then converges locally without additional provider requests.
pub fn reconcile(
    library: &mut Library,
    albums: &[AlbumId],
    market: &str,
) -> Result<music_library::spotify_lifecycle::LocalBatch> {
    if albums.len() > music_library::spotify_lifecycle::RETRY_ALBUMS as usize {
        return Err(music_library::Error::Invalid(
            "Local retry Album bound exceeded".into(),
        ));
    }
    let mut before = vec![];
    for album in albums {
        for track in library.local_album_tracks(album)? {
            if !library.spotify_manually_excluded(&track.track_id)?
                && library
                    .track_provider_occurrences(&track.track_id, "spotify")?
                    .is_empty()
            {
                before.push(track.track_id);
            }
        }
        // Run the same canonical Album completion boundary, not a Track-only
        // confidence bypass. It revalidates the current canonical snapshot.
        if let Some(path) = library.database_path()
            && let Preparation::Ready(input) = library.spotify_album_review_input(album)?
            && let Some((report, programs)) = Spotify::cached_album_review(&path, market, &input)
                .map_err(|e| music_library::Error::Invalid(e.to_string()))?
            && report.decision == "Accepted"
            && let Some(row) = report
                .candidates
                .iter()
                .find(|c| c.decision == "Accepted" && c.equivalent_to.is_none())
            && let Some(program) = programs.iter().find(|p| p.album == row.identity)
        {
            let candidate = music_library::catalog::ArtistAlbumCandidate {
                identity: row.identity.clone(),
                title: row.title.provider_raw.clone(),
                artist: row.artist.clone(),
                artist_ids: row.artist_ids.clone(),
                primary_type: row.title.provider_type.clone(),
                date: row.provider_date.clone(),
                comment: String::new(),
            };
            let outcome = if row.title.comparison
                == music_library::album_candidates::TitleComparison::Exact
            {
                music_library::album_matching::MatchOutcome::Matched(row.identity.clone())
            } else {
                music_library::album_matching::MatchOutcome::MatchedClose(row.identity.clone())
            };
            let identity = row.identity.clone();
            let reply = music_library::album_matching::MatchReply {
                input,
                artist: report.established_artist.clone(),
                outcome,
                matched_album: Some(candidate),
                diagnostic: Some(report),
            };
            let outcome = library.complete_album_match_for(reply, &crate::matching_scope())?;
            if let music_library::album_matching::MatchOutcome::Error(error) = outcome {
                return Err(music_library::Error::Invalid(error));
            }
            if matches!(
                outcome,
                music_library::album_matching::MatchOutcome::Matched(_)
                    | music_library::album_matching::MatchOutcome::MatchedClose(_)
                    | music_library::album_matching::MatchOutcome::AlreadyMatched
            ) && let Some(input) = library.prepare_album_program(album, &identity)?
            {
                match library.complete_album_program(music_library::album_program::Reply {
                    input,
                    result: Ok(program.clone()),
                })? {
                    music_library::album_program::Outcome::Error(error) => {
                        return Err(music_library::Error::Invalid(error));
                    }
                    music_library::album_program::Outcome::Deferred(error) => {
                        return Err(music_library::Error::Invalid(error.to_string()));
                    }
                    _ => {}
                }
            }
        }
        for inherited in cached_candidates(library, album, market)? {
            let input = library.song_resolution_input(&inherited.track)?;
            if input.spotify_excluded
                || !library
                    .track_provider_occurrences(&inherited.track, "spotify")?
                    .is_empty()
            {
                continue;
            }
            let page = library
                .cached_spotify_track_attempt(&inherited.track)?
                .map(|(_, page)| page)
                .unwrap_or(Page {
                    items: vec![],
                    next_offset: None,
                });
            let more = page.next_offset.is_some() || inherited.more;
            let mut selection = Selection::new(input.clone(), page.items);
            selection.inherit_album_candidates(inherited.candidates);
            library.persist_spotify_song_review(
                &input,
                &Page {
                    items: selection.candidates().to_vec(),
                    next_offset: more.then_some(10),
                },
            )?;
        }
    }
    let mut batch = library.reevaluate_spotify_albums_locally(albums)?;
    for track in before {
        if !library
            .track_provider_occurrences(&track, "spotify")?
            .is_empty()
            && !batch.accepted.contains(&track)
        {
            batch.updated.push(track.clone());
            batch.accepted.push(track);
        }
    }
    Ok(batch)
}

/// Deliberately small Track-discovery budget, independent of Album size. Local
/// evidence is exhausted first; callers must not invoke this once per Album.
pub const PLAYABLE_SEARCH_BUDGET: usize = 2;
pub fn reconcile_best_available(
    library: &mut Library,
    albums: &[AlbumId],
    market: &str,
    search: &mut dyn music_library::song_resolution::SongSearch,
) -> Result<music_library::spotify_lifecycle::LocalBatch> {
    let mut batch = reconcile(library, albums, market)?;
    let mut searched = 0;
    let mut seen = std::collections::HashSet::new();
    'albums: for album in albums {
        for track in library.spotify_retry_tracks(std::slice::from_ref(album), 20)? {
            if searched == PLAYABLE_SEARCH_BUDGET {
                break 'albums;
            }
            if !seen.insert(track.clone()) {
                continue;
            }
            let input = library.song_resolution_input(&track)?;
            if input.spotify_excluded
                || !input.association_providers.is_empty()
                || music_library::song_resolution::established_playable_artist(&input).is_none()
            {
                continue;
            }
            if library.spotify_playable_discovery_fresh(&input)? {
                continue;
            }
            if let Some((_, page)) = library.cached_spotify_track_attempt(&track)?
                && music_library::song_resolution::assess_playable(&input, &page)
                    != music_library::song_resolution::Assessment::NoMatch
            {
                continue;
            }
            searched += 1;
            match search.search_songs(&input) {
                Ok(page) => {
                    library.record_spotify_playable_discovery(&input)?;
                    library.persist_spotify_song_review(&input, &page)?;
                    if library.apply_song_evaluation(&input, &page)?.is_some() {
                        batch.accepted.push(track.clone());
                        batch.still_unresolved = batch.still_unresolved.saturating_sub(1);
                        batch.needs_retry = batch.needs_retry.saturating_sub(1);
                    }
                    batch.updated.push(track);
                }
                Err(error) => {
                    batch.errors.push(error.to_string());
                    // Includes Retry-After/quota exhaustion: stop the whole bounded
                    // provider phase, never try the next Track after an error.
                    break 'albums;
                }
            }
        }
    }
    Ok(batch)
}
