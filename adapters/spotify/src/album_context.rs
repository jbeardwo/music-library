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
    pub superseded: Vec<music_library::domain::ExternalIdentity>,
}
/// A provider credit may use a catalog-attested former/preferred name. This is
/// not alias inference: exact retained catalog names and a strong cached program
/// are mandatory, and competing provider Artist identities remain unresolved.
pub fn reconcile_catalog_credit_names(
    library: &mut Library,
    album: &AlbumId,
    market: &str,
) -> Result<bool> {
    let Some(path) = library.database_path() else {
        return Ok(false);
    };
    let Preparation::Ready(input) = library.spotify_album_review_input(album)? else {
        return Ok(false);
    };
    let names = library.trusted_artist_credit_names(&input.artist_id)?;
    let Some((report, programs)) = Spotify::cached_album_review(&path, market, &input)
        .map_err(|e| music_library::Error::Invalid(e.to_string()))?
    else {
        return Ok(false);
    };
    let local = library.local_album_tracks(album)?;
    let mut candidates = vec![];
    for row in &report.candidates {
        if row.artist_ids.len() == 1
            && names.iter().any(|n| {
                music_library::matching::normalize(n)
                    == music_library::matching::normalize(&row.artist)
            })
            && programs.iter().filter(|p| p.album == row.identity).any(|p| {
                let mut contextual = local.clone();
                for track in &mut contextual {
                    if let Some(artist) = track.evidence.artists.first_mut()
                        && names.iter().any(|n| music_library::matching::normalize(n) == music_library::matching::normalize(&artist.name))
                        && !artist.identities.contains(&row.artist_ids[0])
                    {
                        // Attested catalog name, not a guessed provider alias.
                        // Add the proposed ID only to comparison evidence while
                        // checking independent program/Recording corroboration.
                        artist.identities.push(row.artist_ids[0].clone());
                    }
                }
                p.programs.iter().any(|program| {
                    let evidence = music_library::album_program::inspect_album_program(&contextual, program, &input.raw_title);
                    evidence.complete && evidence.alignment.strong && evidence.anchors >= 3
                        && !evidence.tracks.iter().any(|t| t.decision == music_library::album_program::PositionDecision::TrustedIdentityConflict)
                })
            })
            && !candidates
                .iter()
                .any(|c: &music_library::catalog::ArtistCandidate| c.identity == row.artist_ids[0])
        {
            candidates.push(music_library::catalog::ArtistCandidate {
                identity: row.artist_ids[0].clone(),
                name: row.artist.clone(),
                aliases: vec![],
                comment: String::new(),
                country: String::new(),
                artist_type: String::new(),
                score: None,
            });
        }
    }
    if let [candidate] = candidates.as_slice() {
        return library.ingest_catalog_artist_provider_name(&input.artist_id, candidate);
    }
    Ok(false)
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
        for row in report.candidates.iter().filter(|c| {
            c.artist_accepted
                && c.equivalent_to.is_none()
                && !c.reasons.iter().any(|r| {
                    matches!(
                        r,
                        music_library::album_candidates::Reason::ArtistMismatch
                            | music_library::album_candidates::Reason::AlbumTitleMismatch
                            | music_library::album_candidates::Reason::AlbumVersionMismatch
                            | music_library::album_candidates::Reason::ReleaseTypeMismatch
                    )
                })
        }) {
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
        // Deterministic occurrence choice within a corroborated playback family.
        // Unrelated candidates remain competitors. Prefer full title evidence.
        candidates.sort_by(|a, b| {
            let exact = |c: &Candidate| {
                music_library::matching::normalize_album_title(&c.title)
                    == music_library::matching::normalize_album_title(
                        track.evidence.title.as_deref().unwrap_or(""),
                    )
            };
            exact(b)
                .cmp(&exact(a))
                .then_with(|| a.identity.external_id.cmp(&b.identity.external_id))
        });
        let mut families = std::collections::HashSet::new();
        let mut superseded = vec![];
        candidates.retain(|candidate| {
            let family = report
                .candidates
                .iter()
                .find(|r| Some(&r.identity) == candidate.album_identity.as_ref())
                .and_then(|r| r.playable_family.as_ref());
            if let Some(family) = family {
                let title = music_library::matching::normalize_album_title(&candidate.title);
                let retain = families.insert((family.external_id.clone(), title));
                if !retain {
                    superseded.push(candidate.identity.clone());
                }
                retain
            } else {
                true
            }
        });
        let more = candidates.len() > 10;
        candidates.truncate(10);
        if !candidates.is_empty() {
            result.push(TrackCandidates {
                track: track.track_id.clone(),
                candidates,
                more,
                superseded,
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
    let mut page = cached.map(|(_, page)| page).unwrap_or(Page {
        items: vec![],
        next_offset: None,
    });
    if let Some(inherited) = &inherited {
        page.items
            .retain(|c| !inherited.superseded.contains(&c.identity));
    }
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
                artist_observation: None,
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
        if let Some(path) = library.database_path()
            && let Preparation::Ready(input) = library.spotify_album_review_input(album)?
            && let Some((report, programs)) = Spotify::cached_album_review(&path, market, &input)
                .map_err(|e| music_library::Error::Invalid(e.to_string()))?
        {
            let sole_artist_representation = report
                .candidates
                .iter()
                .filter(|r| r.artist_accepted)
                .count()
                == 1;
            let contexts = report
                .candidates
                .iter()
                .filter(|r| r.artist_accepted)
                .filter(|r| {
                    !music_library::album_candidates::playable_edition_pair(
                        &input.raw_title,
                        &r.title.provider_raw,
                    ) || !report.more_candidates && sole_artist_representation
                })
                .filter_map(|r| {
                    programs.iter().find(|p| p.album == r.identity).map(|p| {
                        (
                            music_library::catalog::ArtistAlbumCandidate {
                                identity: r.identity.clone(),
                                title: r.title.provider_raw.clone(),
                                artist: r.artist.clone(),
                                artist_ids: r.artist_ids.clone(),
                                primary_type: r.title.provider_type.clone(),
                                date: r.provider_date.clone(),
                                comment: String::new(),
                            },
                            p.clone(),
                        )
                    })
                })
                .take(music_library::album_candidates::MAX_PROGRAM_CANDIDATES)
                .collect::<Vec<_>>();
            library.reconcile_spotify_program_playables(album, &contexts)?;
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
            let mut page = library
                .cached_spotify_track_attempt(&inherited.track)?
                .map(|(_, page)| page)
                .unwrap_or(Page {
                    items: vec![],
                    next_offset: None,
                });
            page.items
                .retain(|c| !inherited.superseded.contains(&c.identity));
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
    // Local replay can flag absent individual Track pages as retryable even
    // after successful Album discovery. Restore the authoritative terminal
    // Album decision; retained candidates are reviewable, absence is negative.
    for album in albums {
        if let Some(path) = library.database_path()
            && let Preparation::Ready(input) = library.spotify_album_review_input(album)?
            && let Some((report, _)) = Spotify::cached_album_review(&path, market, &input)
                .map_err(|e| music_library::Error::Invalid(e.to_string()))?
        {
            library.persist_spotify_review(album, &report)?;
        }
    }
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
    reconcile_best_available_with_budget(library, albums, market, search, PLAYABLE_SEARCH_BUDGET)
}
/// Remaining per-run allowance, not an allowance to reset at a batch boundary.
pub fn reconcile_best_available_with_budget(
    library: &mut Library,
    albums: &[AlbumId],
    market: &str,
    search: &mut dyn music_library::song_resolution::SongSearch,
    budget: usize,
) -> Result<music_library::spotify_lifecycle::LocalBatch> {
    let mut batch = reconcile(library, albums, market)?;
    let mut searched = 0;
    let mut seen = std::collections::HashSet::new();
    'albums: for album in albums {
        if library.spotify_bulk_window_current(album)? {
            continue;
        }
        let mut after = None;
        loop {
            let tracks = library.spotify_retry_tracks_after(
                std::slice::from_ref(album),
                after.as_ref(),
                20,
            )?;
            if tracks.is_empty() {
                break;
            }
            after = tracks.last().cloned();
            for track in tracks {
                if searched == budget.min(PLAYABLE_SEARCH_BUDGET) {
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
                batch.provider_searches += 1;
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
    }
    if batch.errors.is_empty() && (budget == 0 || searched == budget.min(PLAYABLE_SEARCH_BUDGET)) {
        for album in albums {
            library.record_spotify_bulk_window(album)?;
        }
    }
    Ok(batch)
}

/// Canonical Artist first; one catalog-attested alternate is corroborated by
/// the corresponding Album program when its provider identity is ambiguous.
/// All HTTP occurs before the short name-observation write.
pub fn discover_catalog_credit_names(
    library: &mut Library,
    album: &AlbumId,
    provider: &mut Spotify,
) -> Result<bool> {
    use music_library::catalog::CatalogProvider;
    let Preparation::Ready(input) = library.spotify_album_review_input(album)? else {
        return Ok(false);
    };
    let mut names = vec![input.artist.clone()];
    names.extend(
        library
            .trusted_artist_search_names(&input.artist)?
            .into_iter()
            .take(1),
    );
    let mut changed = false;
    for name in names {
        let page = provider
            .search_artists(&name)
            .map_err(|e| music_library::Error::Invalid(e.to_string()))?;
        if name == input.artist
            && let Ok(identity) = music_library::album_matching::resolve_artist(&name, &page)
            && let Some(candidate) = page.items.iter().find(|c| {
                c.identity == identity
                    && music_library::matching::normalize(&c.name)
                        == music_library::matching::normalize(&input.artist)
            })
        {
            changed |= library.ingest_catalog_artist_provider_name(&input.artist_id, candidate)?;
            continue;
        }
        let plausible = page
            .items
            .iter()
            .filter(|c| {
                music_library::matching::normalize(&c.name)
                    == music_library::matching::normalize(&name)
            })
            .take(10)
            .collect::<Vec<_>>();
        if plausible.is_empty() {
            continue;
        }
        let ids = plausible
            .iter()
            .map(|c| c.identity.clone())
            .collect::<Vec<_>>();
        let albums = provider
            .albums_for_artists(&ids, &input.title)
            .map_err(|e| music_library::Error::Invalid(e.to_string()))?;
        let local = library.local_album_tracks(album)?;
        let mut supported = vec![];
        for candidate in albums
            .items
            .iter()
            .filter(|c| {
                matches!(
                    music_library::album_candidates::title_evidence(&input.raw_title, c).comparison,
                    music_library::album_candidates::TitleComparison::Exact
                        | music_library::album_candidates::TitleComparison::Typography
                        | music_library::album_candidates::TitleComparison::ReleaseTypeSuffix
                )
            })
            .take(music_library::album_candidates::MAX_PROGRAM_CANDIDATES)
        {
            let programs = provider
                .album_programs(&candidate.identity)
                .map_err(|e| music_library::Error::Invalid(e.to_string()))?;
            for artist in plausible
                .iter()
                .filter(|a| candidate.artist_ids.contains(&a.identity))
            {
                let mut contextual = local.clone();
                for track in &mut contextual {
                    if let Some(credit) = track.evidence.artists.first_mut() {
                        credit.identities.push(artist.identity.clone());
                    }
                }
                if programs.programs.iter().any(|p| {
                    let inspected = music_library::album_program::inspect_album_program(
                        &contextual,
                        p,
                        &input.raw_title,
                    );
                    inspected.complete
                        && inspected.alignment.strong
                        && inspected.anchors >= 3
                        && inspected.compatible()
                }) && !supported
                    .iter()
                    .any(|c: &&music_library::catalog::ArtistCandidate| {
                        c.identity == artist.identity
                    })
                {
                    supported.push(*artist);
                }
            }
        }
        if let [candidate] = supported.as_slice() {
            changed |= library.ingest_catalog_artist_provider_name(&input.artist_id, candidate)?;
        }
    }
    Ok(changed)
}
