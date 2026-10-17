//! Album-scoped playback evidence. Never attaches canonical Album identities.
use crate::{
    Error, Library, Result,
    album_program::{PositionDecision, Programs},
    catalog::ArtistAlbumCandidate,
    domain::AlbumId,
    song_resolution::Candidate,
};

impl Library {
    /// Bounded source observations for release-difference diagnostics, not identity.
    pub fn spotify_album_source_years(&self, album: &AlbumId) -> Result<Vec<u16>> {
        Ok(self.store.connection.prepare(
            "SELECT DISTINCT f.year FROM file_metadata_observation f JOIN track_source s ON s.source_id=f.source_id JOIN track t ON t.id=s.track_id JOIN release r ON r.id=t.release_id WHERE r.album_id=?1 AND f.year IS NOT NULL ORDER BY f.year LIMIT 8"
        )?.query_map([album.as_ref()], |r| r.get::<_, u16>(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// Compare outside the writer, then validate the canonical snapshot in a
    /// short transaction. A compatible provider program may supply a subset.
    pub fn reconcile_spotify_program_playables(
        &mut self,
        album: &AlbumId,
        candidates: &[(ArtistAlbumCandidate, Programs)],
    ) -> Result<Vec<crate::domain::TrackId>> {
        if candidates.len() > crate::album_candidates::MAX_PROGRAM_CANDIDATES {
            return Err(Error::Invalid(
                "Playable program candidate bound exceeded".into(),
            ));
        }
        let local = self.local_album_tracks(album)?;
        let source_years = self.spotify_album_source_years(album)?;
        let preparation = self.spotify_album_review_input(album)?;
        let crate::album_matching::Preparation::Ready(input) = preparation else {
            return Ok(vec![]);
        };
        let established_artist = |candidate: &ArtistAlbumCandidate| {
            input
                .known_artist
                .as_ref()
                .is_some_and(|id| candidate.artist_ids.contains(id))
                || input
                    .equivalent_artists
                    .iter()
                    .any(|id| candidate.artist_ids.contains(id))
        };
        // A differing edition is a fallback, never a preference over an original
        // or a way to resolve competing materially different representations.
        let sole_representation = candidates
            .iter()
            .filter(|(c, _)| established_artist(c))
            .count()
            == 1;
        let mut choices = vec![Vec::<(usize, Candidate, bool)>::new(); local.len()];
        for (index, (candidate, programs)) in candidates.iter().enumerate() {
            if programs.album != candidate.identity
                || programs.album.provider != "spotify"
                || programs.album.kind != "album"
            {
                continue;
            }
            let established = input
                .known_artist
                .as_ref()
                .is_some_and(|id| candidate.artist_ids.contains(id))
                || input
                    .equivalent_artists
                    .iter()
                    .any(|id| candidate.artist_ids.contains(id));
            let title = crate::album_candidates::title_evidence(&input.raw_title, candidate);
            let edition_fallback = sole_representation
                && crate::album_candidates::playable_edition_pair(
                    &input.raw_title,
                    &candidate.title,
                );
            let title_compatible = matches!(
                title.comparison,
                crate::album_candidates::TitleComparison::Exact
                    | crate::album_candidates::TitleComparison::Typography
                    | crate::album_candidates::TitleComparison::ReleaseTypeSuffix
            ) || title.comparison
                == crate::album_candidates::TitleComparison::Conflict
                && crate::matching::shared_title_span(&input.raw_title, &candidate.title).is_some()
                && crate::album_matching::qualifiers(&input.raw_title)
                    == crate::album_matching::qualifiers(&candidate.title);
            if !established || !(title_compatible || edition_fallback) {
                continue;
            }
            for program in &programs.programs {
                let inspected = crate::album_program::inspect_playable_program(
                    &local,
                    program,
                    &candidate.title,
                );
                if !inspected.alignment.strong || inspected.duplicate_positions > 0 {
                    continue;
                }
                if edition_fallback
                    && (!program.complete
                        || inspected.anchors < 3
                        || inspected.anchors * 5 < local.len() * 4)
                {
                    continue;
                }
                for (i, row) in inspected.tracks.iter().enumerate() {
                    if !matches!(
                        row.decision,
                        PositionDecision::ExactAnchor
                            | PositionDecision::ReorderedTrack
                            | PositionDecision::AlignedAfterInsertion
                    ) {
                        continue;
                    }
                    let Some(track) = &row.provider else { continue };
                    let Some(identity) = track
                        .identities
                        .iter()
                        .find(|id| id.provider == "spotify" && id.kind == "track")
                    else {
                        continue;
                    };
                    let c = Candidate {
                        recording: track.recording.clone(),
                        identity: identity.clone(),
                        album_identity: Some(candidate.identity.clone()),
                        title: track.title.clone().unwrap_or_default(),
                        artist: track
                            .artists
                            .iter()
                            .map(|a| a.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                        artists: track.artists.clone(),
                        album: candidate.title.clone(),
                        date: candidate.date.clone(),
                        album_artists: candidate
                            .artist_ids
                            .iter()
                            .map(|id| crate::edition::ArtistEvidence {
                                name: candidate.artist.clone(),
                                identities: vec![id.clone()],
                                join_phrase: String::new(),
                            })
                            .collect(),
                        album_type: candidate.primary_type.clone(),
                        album_total_tracks: Some(program.tracks.len() as u32),
                        duration_ms: track.duration_ms.unwrap_or(0),
                        disc: track.disc.unwrap_or(1),
                        number: track.number.unwrap_or(0),
                    };
                    let full = crate::album_program::title_relation(&local[i].evidence, track)
                        == crate::album_program::TitleRelation::Agrees;
                    choices[i].push((index, c, full));
                }
            }
        }
        // Decide with current Track evidence outside the writer. Full title
        // correspondence wins; family ties use a stable playback locator only.
        let mut selected = vec![];
        for (i, options) in choices.iter_mut().enumerate() {
            let track = &local[i].track_id;
            let snapshot = self.song_resolution_input(track)?;
            if snapshot.spotify_excluded
                || snapshot
                    .association_providers
                    .iter()
                    .any(|p| p == "spotify")
            {
                continue;
            }
            options.retain(|(_, c, _)| {
                let mut comparison = snapshot.clone();
                comparison.title = c.title.clone();
                comparison.evidence.titles.clear();
                crate::song_resolution::playable_eligible(&comparison, c)
            });
            let score = |c: &(usize, Candidate, bool)| {
                (
                    c.2,
                    crate::artist_credit::compare(&snapshot.artists, &c.1.artists)
                        == crate::artist_credit::Compatibility::Equivalent,
                )
            };
            let Some(best) = options.iter().map(score).max() else {
                continue;
            };
            options.retain(|c| score(c) == best);
            options.sort_by(|a, b| a.1.identity.external_id.cmp(&b.1.identity.external_id));
            options.dedup_by(|a, b| a.1.identity == b.1.identity);
            let first = &options[0];
            let compatible = options.iter().enumerate().all(|(i, left)| {
                options[i + 1..].iter().all(|right| {
                    let (a, ap) = &candidates[left.0];
                    let (b, bp) = &candidates[right.0];
                    a.artist_ids == b.artist_ids
                        && a.primary_type == b.primary_type
                        && crate::matching::album_representation_title(&a.title)
                            == crate::matching::album_representation_title(&b.title)
                        && matches!((ap.programs.as_slice(), bp.programs.as_slice()), ([a], [b])
                            if crate::album_program::compatible_provider_programs(a,b)
                                && crate::album_program::compatible_provider_programs(b,a))
                })
            });
            if compatible {
                let page = crate::catalog::Page {
                    items: options.iter().map(|(_, c, _)| c.clone()).collect(),
                    next_offset: None,
                };
                let mut reason =
                    "Connected from compatible Spotify program; canonical Release retained"
                        .to_owned();
                if !first.2 {
                    reason.push_str(&format!(
                        ". Contextual title correspondence: {} ↔ {}; raw titles retained",
                        snapshot.title, first.1.title
                    ));
                }
                if snapshot.album != first.1.album {
                    reason.push_str(&format!(
                        ". Provider release differs: {} ↔ {}",
                        snapshot.album, first.1.album
                    ));
                }
                if crate::album_candidates::playable_edition_pair(&snapshot.album, &first.1.album) {
                    reason.push_str(". Warning: Spotify remaster/reissue or edition differs; established Artist and unique strong program, no competing provider representation");
                }
                if let Some(provider_date) = crate::catalog_date::Date::parse(&first.1.date)
                    && let Some(local_year) = snapshot
                        .album_date
                        .map(|d| d.year)
                        .or_else(|| (source_years.len() == 1).then(|| source_years[0]))
                    && local_year != provider_date.year
                {
                    reason.push_str(&format!(
                        ". Warning: release year differs: {} ↔ {}",
                        local_year, provider_date.year
                    ));
                }
                if options.len() > 1 {
                    reason.push_str(". Compatible provider release family used for playback");
                }
                selected.push((snapshot, first.1.clone(), page, reason));
            }
        }
        if selected.is_empty() {
            return Ok(vec![]);
        }
        let tx = self
            .store
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if crate::album_program::local_tracks(&tx, album)?.0 != local {
            return Err(Error::ReconciliationEvidenceChanged);
        }
        let mut connected = vec![];
        for (snapshot, candidate, page, reason) in selected {
            if crate::song_resolution::load_input(&tx, &snapshot.track_id)? != snapshot {
                return Err(Error::ReconciliationEvidenceChanged);
            }
            let saved: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM library_membership WHERE track_id=?1)",
                [snapshot.track_id.as_ref()],
                |r| r.get(0),
            )?;
            if !saved {
                continue;
            }
            // Retain raw provider evidence and alternatives for offline review.
            Self::cache_spotify_attempt_tx(&tx, &snapshot, &page)?;
            crate::song_resolution::persist_occurrence(&tx, &snapshot.track_id, &candidate)?;
            tx.execute("UPDATE spotify_connection_review SET stale=0,evaluation_version=?2,state='unresolved',reason_code='already_associated',reason=?3 WHERE track_id=?1",rusqlite::params![snapshot.track_id.as_ref(),crate::spotify_lifecycle::EVALUATION_VERSION,reason])?;
            connected.push(snapshot.track_id);
        }
        tx.commit()?;
        Ok(connected)
    }
}
