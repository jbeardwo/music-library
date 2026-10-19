//! Offline Album review and explicit positioned confirmation. No discovery policy here.
use crate::{
    Library,
    album_program::Programs,
    domain::*,
    edition::LocalTrackEvidence,
    storage::{Error, Result},
};
use rusqlite::params;

const STATUS_SQL: &str = "CASE WHEN max(v.state='needs_retry') AND EXISTS(SELECT 1 FROM provider_cooldown WHERE provider='spotify' AND until_epoch>unixepoch()) THEN 'Waiting for Spotify' WHEN max(v.reason_code='provider_unavailable') THEN 'Waiting for Spotify' WHEN max(v.reason_code='provider_search_deferred') THEN 'Provider search deferred' WHEN max(v.state='needs_retry') AND a.match_title!='' AND a.match_artist_credit!='' AND (SELECT count(*) FROM album_artist_credit ac WHERE ac.album_id=a.album_id)=1 THEN 'Needs retry' WHEN max(v.state='needs_retry') THEN 'Manual review required' WHEN max(v.reason_code='no_candidates') THEN 'No candidates' WHEN max(v.reason_code='artist_unresolved') THEN 'Artist identity needs review' WHEN min(v.reason_code='not_evaluated') THEN 'Not searched' ELSE 'Needs review' END";

#[derive(Clone, Debug, serde::Serialize)]
pub struct Group {
    pub id: AlbumId,
    pub title: String,
    pub artist: String,
    pub unresolved: usize,
    pub status: String,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct PositionedMatch {
    pub track_id: TrackId,
    pub disc: u32,
    pub number: u32,
    pub local_title: String,
    pub provider_title: String,
    pub warnings: Vec<String>,
    #[serde(skip)]
    candidate: crate::song_resolution::Candidate,
}
impl PositionedMatch {
    /// Exact provider occurrence in the explicit confirmation plan.
    pub fn candidate(&self) -> &crate::song_resolution::Candidate {
        &self.candidate
    }
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct AlbumSelection {
    album: AlbumId,
    provider: Option<ExternalIdentity>,
    matches: Vec<PositionedMatch>,
    pub unmatched_local: Vec<TrackId>,
    pub unmatched_provider: usize,
    pub warnings: Vec<String>,
    #[serde(skip)]
    snapshot: Vec<LocalTrackEvidence>,
}
impl AlbumSelection {
    pub fn matches(&self) -> &[PositionedMatch] {
        &self.matches
    }
    pub fn requires_confirmation(&self) -> bool {
        !self.warnings.is_empty() || self.matches.iter().any(|m| !m.warnings.is_empty())
    }
}
impl Library {
    /// Read review context even for a partially connected Album. This never
    /// changes the automatic matcher’s already-associated short circuit.
    pub fn spotify_album_review_input(
        &self,
        album: &AlbumId,
    ) -> Result<crate::album_matching::Preparation> {
        crate::storage::prepare_album_match(
            &self.store.connection,
            album,
            &crate::catalog::MatchingScope {
                provider: "spotify".into(),
                artist_kind: "artist".into(),
                album_kind: "album".into(),
            },
            true,
        )
    }
    /// Keyset bounded groups. Count only saved unresolved Tracks; no per-Track reads.
    pub fn spotify_review_groups(&self, after: Option<&AlbumId>, limit: u32) -> Result<Vec<Group>> {
        let eligible = "NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=t.id) AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=t.id AND x.provider='spotify')";
        let sql = format!(
            "WITH chosen AS MATERIALIZED (SELECT b.album_id,b.title_key FROM album_browse_order b INDEXED BY album_order_title WHERE (?1 IS NULL OR (b.title_key,b.album_id)>(SELECT title_key,album_id FROM album_browse_order WHERE album_id=?1)) AND EXISTS(SELECT 1 FROM release r JOIN track t ON t.release_id=r.id JOIN library_membership m ON m.track_id=t.id WHERE r.album_id=b.album_id AND {eligible}) ORDER BY b.title_key,b.album_id LIMIT ?2) SELECT a.album_id,a.title,COALESCE(a.artist_credit,a.match_artist_credit),count(*),{STATUS_SQL} FROM chosen b JOIN effective_album_metadata a ON a.album_id=b.album_id JOIN release r ON r.album_id=b.album_id JOIN track t ON t.release_id=r.id JOIN library_membership m ON m.track_id=t.id JOIN spotify_connection_review v ON v.track_id=t.id WHERE {eligible} GROUP BY b.title_key,b.album_id ORDER BY b.title_key,b.album_id"
        );
        let mut q = self.store.connection.prepare(&sql)?;
        Ok(q.query_map(
            params![after.map(AsRef::as_ref), limit.clamp(1, 200)],
            |r| {
                Ok(Group {
                    id: AlbumId(r.get(0)?),
                    title: r.get(1)?,
                    artist: r.get(2)?,
                    unresolved: r.get::<_, i64>(3)? as usize,
                    status: r.get(4)?,
                })
            },
        )?
        .collect::<rusqlite::Result<_>>()?)
    }
    /// Same aggregate lifecycle as Connections, scoped to one Album.
    pub fn spotify_album_review_lifecycle(&self, album: &AlbumId) -> Result<String> {
        let sql = format!(
            "SELECT CASE WHEN count(*)=0 THEN 'No unresolved Tracks' ELSE {STATUS_SQL} END FROM effective_album_metadata a JOIN release r ON r.album_id=a.album_id JOIN track t ON t.release_id=r.id JOIN library_membership m ON m.track_id=t.id JOIN spotify_connection_review v ON v.track_id=t.id WHERE a.album_id=?1 AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track s WHERE s.track_id=t.id) AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=t.id AND x.provider='spotify')"
        );
        Ok(self
            .store
            .connection
            .query_row(&sql, [album.as_ref()], |r| r.get(0))?)
    }
    /// Prepare only unique established positions. Never align by title or shift order.
    pub fn prepare_spotify_album_confirmation(
        &self,
        album: &AlbumId,
        programs: &Programs,
    ) -> Result<AlbumSelection> {
        if programs.album.provider != "spotify"
            || programs.album.kind != "album"
            || programs.album.external_id.is_empty()
        {
            return Err(Error::Invalid("Select a Spotify Album candidate".into()));
        }
        let [program] = programs.programs.as_slice() else {
            return Err(Error::Invalid(
                "One unambiguous Spotify program is required".into(),
            ));
        };
        let local = self.local_album_tracks(album)?;
        let position = |e: &crate::edition::TrackEvidence| {
            e.number
                .filter(|n| *n > 0)
                .map(|n| (e.disc.filter(|d| *d > 0).unwrap_or(1), n))
        };
        let mut local_positions = std::collections::HashMap::new();
        for t in &local {
            if let Some(key) = position(&t.evidence) {
                *local_positions.entry(key).or_insert(0usize) += 1;
            }
        }
        let mut provider_positions = std::collections::HashMap::<_, Vec<_>>::new();
        for t in &program.tracks {
            if let Some(key) = position(t) {
                provider_positions.entry(key).or_default().push(t);
            }
        }
        let mut selection = AlbumSelection {
            album: album.clone(),
            provider: Some(programs.album.clone()),
            matches: vec![],
            unmatched_local: vec![],
            unmatched_provider: 0,
            warnings: vec![],
            snapshot: local.clone(),
        };
        let album_credits =
            crate::artist_credit::load(&self.store.connection, "album", album.as_ref())?
                .into_iter()
                .map(|c| c.evidence)
                .collect::<Vec<_>>();
        let album_title: String = self.store.connection.query_row(
            "SELECT title FROM effective_album_metadata WHERE album_id=?1",
            [album.as_ref()],
            |r| r.get(0),
        )?;
        let local_ids = serde_json::to_string(
            &local
                .iter()
                .map(|t| t.track_id.as_ref())
                .collect::<Vec<_>>(),
        )
        .map_err(|e| Error::Invalid(e.to_string()))?;
        let ineligible:std::collections::HashSet<String>=self.store.connection.prepare("SELECT track_id FROM track_provider_exclusion WHERE provider='spotify' AND track_id IN(SELECT value FROM json_each(?1)) UNION SELECT track_id FROM trusted_spotify_track WHERE track_id IN(SELECT value FROM json_each(?1))")?.query_map([local_ids],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
        let aligned = crate::album_program::inspect_album_program(&local, program, &album_title);
        // Provider Album credit is represented by its positioned Track credits;
        // Track differences are reported individually, never establish equivalence.
        let mut usable_positions = std::collections::HashSet::new();
        for track in &local {
            let Some(key) = position(&track.evidence) else {
                selection.unmatched_local.push(track.track_id.clone());
                continue;
            };
            let providers = provider_positions
                .get(&key)
                .map(Vec::as_slice)
                .unwrap_or_default();
            if !aligned.alignment.strong
                && (providers.len() != 1 || local_positions.get(&key) != Some(&1))
            {
                selection.unmatched_local.push(track.track_id.clone());
                continue;
            }
            let p = if aligned.alignment.strong {
                let Some(p) = aligned
                    .tracks
                    .iter()
                    .find(|t| t.track_id == track.track_id.as_ref())
                    .and_then(|t| t.provider.as_ref())
                else {
                    selection.unmatched_local.push(track.track_id.clone());
                    continue;
                };
                p
            } else {
                providers[0]
            };
            let ids: Vec<_> = p
                .identities
                .iter()
                .filter(|id| {
                    id.provider == "spotify" && id.kind == "track" && !id.external_id.is_empty()
                })
                .collect();
            if ids.len() != 1 {
                selection.unmatched_local.push(track.track_id.clone());
                continue;
            }
            // A provider title that belongs at a missing canonical position is
            // evidence of an omission/shift, not a reasonable metadata variant.
            // Use that evidence only as a veto, never to assign across positions.
            if !aligned.alignment.strong
                && crate::album_program::title_relation(&track.evidence, p)
                    != crate::album_program::TitleRelation::Agrees
                && local.iter().any(|other| {
                    position(&other.evidence).is_some_and(|other_key| {
                        other_key != key && !provider_positions.contains_key(&other_key)
                    }) && crate::album_program::title_relation(&other.evidence, p)
                        == crate::album_program::TitleRelation::Agrees
                })
            {
                selection.unmatched_local.push(track.track_id.clone());
                continue;
            }
            if let Some(provider_key) = position(p) {
                usable_positions.insert(provider_key);
            }
            if ineligible.contains(track.track_id.as_ref()) {
                continue;
            }
            let mut warnings = vec![];
            if position(p) != Some(key) {
                warnings.push(format!(
                    "Program alignment: {}.{} → {}.{}",
                    key.0,
                    key.1,
                    p.disc.unwrap_or(1),
                    p.number.unwrap_or(0)
                ));
            }
            let local_title = track.evidence.title.clone().unwrap_or_default();
            let provider_title = p.title.clone().unwrap_or_default();
            if crate::album_program::title_relation(&track.evidence, p)
                != crate::album_program::TitleRelation::Agrees
            {
                warnings.push(format!("Title: {local_title} → {provider_title}"));
            }
            if !track.evidence.artists.is_empty()
                && crate::artist_credit::compare(&track.evidence.artists, &p.artists)
                    != crate::artist_credit::Compatibility::Equivalent
            {
                warnings.push(format!(
                    "Artist credit: {} → {}",
                    track
                        .evidence
                        .artists
                        .iter()
                        .map(|a| a.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    p.artists
                        .iter()
                        .map(|a| a.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if track
                .evidence
                .duration_ms
                .zip(p.duration_ms)
                .is_some_and(|(a, b)| a > 0 && b > 0 && a.abs_diff(b) > 10_000)
            {
                warnings
                    .push("Duration differs by more than the existing Spotify tolerance".into());
            }
            selection.matches.push(PositionedMatch {
                track_id: track.track_id.clone(),
                disc: key.0,
                number: key.1,
                local_title,
                provider_title: provider_title.clone(),
                warnings,
                candidate: crate::song_resolution::Candidate {
                    recording: Default::default(),
                    identity: ids[0].clone(),
                    album_identity: Some(programs.album.clone()),
                    title: provider_title,
                    artist: p
                        .artists
                        .iter()
                        .map(|a| a.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    artists: p.artists.clone(),
                    album: album_title.clone(),
                    date: String::new(),
                    album_artists: album_credits.clone(),
                    album_type: String::new(),
                    album_total_tracks: Some(program.tracks.len() as u32),
                    duration_ms: p.duration_ms.unwrap_or(0),
                    disc: p.disc.unwrap_or(1),
                    number: p.number.unwrap_or(0),
                },
            });
        }
        selection.unmatched_provider = program
            .tracks
            .iter()
            .filter(|p| position(p).is_none_or(|key| !usable_positions.contains(&key)))
            .count();
        if !program.complete {
            selection.warnings.push(
                "Provider program is incomplete; only established positions can be connected"
                    .into(),
            );
        }
        Ok(selection)
    }
    /// Connections proposes only correspondences supported by the existing
    /// program evaluator. Legacy explicit positioned review remains available;
    /// deliberately adjusted mappings use prepare_spotify_track_mapping.
    pub fn prepare_spotify_connections_confirmation(
        &self,
        album: &AlbumId,
        programs: &Programs,
    ) -> Result<AlbumSelection> {
        let base = self.prepare_spotify_album_confirmation(album, programs)?;
        let local = self.local_album_tracks(album)?;
        let title: String = self.store.connection.query_row(
            "SELECT title FROM effective_album_metadata WHERE album_id=?1",
            [album.as_ref()],
            |r| r.get(0),
        )?;
        let comparison = self.spotify_connections_program_alignment(album, programs)?;
        let mut mappings = Vec::new();
        for row in &comparison.tracks {
            if !matches!(
                row.decision,
                crate::album_program::PositionDecision::ExactAnchor
                    | crate::album_program::PositionDecision::CorroboratedPosition
                    | crate::album_program::PositionDecision::AlignedAfterInsertion
                    | crate::album_program::PositionDecision::ReorderedTrack
            ) {
                continue;
            }
            let Some(p) = &row.provider else { continue };
            let ids = p
                .identities
                .iter()
                .filter(|id| id.provider == "spotify" && id.kind == "track")
                .collect::<Vec<_>>();
            if ids.len() != 1 {
                continue;
            }
            mappings.push((
                TrackId(row.track_id.clone()),
                crate::song_resolution::Candidate {
                    identity: ids[0].clone(),
                    recording: p.recording.clone(),
                    album_identity: Some(programs.album.clone()),
                    title: p.title.clone().unwrap_or_default(),
                    artist: p
                        .artists
                        .iter()
                        .map(|a| a.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    artists: p.artists.clone(),
                    album: title.clone(),
                    date: String::new(),
                    album_artists: vec![],
                    album_type: String::new(),
                    album_total_tracks: Some(programs.programs[0].tracks.len() as u32),
                    duration_ms: p.duration_ms.unwrap_or(0),
                    disc: p.disc.unwrap_or(1),
                    number: p.number.unwrap_or(0),
                },
            ));
        }
        // One indexed eligibility read; display and confirmation share this exact plan.
        let ineligible:std::collections::HashSet<String>=self.store.connection.prepare("SELECT s.track_id FROM trusted_spotify_track s JOIN track t ON t.id=s.track_id JOIN release r ON r.id=t.release_id WHERE r.album_id=?1 UNION SELECT x.track_id FROM track_provider_exclusion x JOIN track t ON t.id=x.track_id JOIN release r ON r.id=t.release_id WHERE x.provider='spotify' AND r.album_id=?1")?.query_map([album.as_ref()],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
        mappings.retain(|(track, _)| !ineligible.contains(track.as_ref()));
        let mut selection = self.prepare_spotify_track_mapping(album, &mappings)?;
        selection.warnings = base.warnings;
        let standard =
            crate::album_program::inspect_album_program(&local, &programs.programs[0], &title);
        selection.provider = if standard.alignment.strong || !comparison.alignment.strong {
            Some(programs.album.clone())
        } else {
            selection
                .warnings
                .push("Partial playable program; canonical Release identity retained".into());
            None
        };
        selection.unmatched_provider = programs.programs[0]
            .tracks
            .iter()
            .filter(|p| {
                !comparison.tracks.iter().any(|r| {
                    r.provider
                        .as_ref()
                        .is_some_and(|candidate| candidate.identities == p.identities)
                        && matches!(
                            r.decision,
                            crate::album_program::PositionDecision::ExactAnchor
                                | crate::album_program::PositionDecision::CorroboratedPosition
                                | crate::album_program::PositionDecision::AlignedAfterInsertion
                                | crate::album_program::PositionDecision::ReorderedTrack
                        )
                })
            })
            .count();
        Ok(selection)
    }
    /// The same best-available program evaluator used by automatic source reconciliation.
    pub fn spotify_connections_program_alignment(
        &self,
        album: &AlbumId,
        programs: &Programs,
    ) -> Result<crate::album_program::PositionedProgram> {
        let [program] = programs.programs.as_slice() else {
            return Err(Error::Invalid(
                "One unambiguous Spotify program is required".into(),
            ));
        };
        let local = self.local_album_tracks(album)?;
        let title: String = self.store.connection.query_row(
            "SELECT title FROM effective_album_metadata WHERE album_id=?1",
            [album.as_ref()],
            |r| r.get(0),
        )?;
        Ok(crate::album_program::inspect_playable_program(
            &local, program, &title,
        ))
    }

    /// Explicit user mapping can span provider Albums. No Album-wide identity is
    /// inferred for this operation; unused canonical/provider Tracks are allowed.
    pub fn prepare_spotify_track_mapping(
        &self,
        album: &AlbumId,
        mappings: &[(TrackId, crate::song_resolution::Candidate)],
    ) -> Result<AlbumSelection> {
        let snapshot = self.local_album_tracks(album)?;
        let mut tracks = std::collections::HashSet::new();
        let mut occurrences = std::collections::HashSet::new();
        let mut matches = vec![];
        for (track, candidate) in mappings {
            if candidate.identity.provider != "spotify"
                || candidate.identity.kind != "track"
                || candidate.identity.external_id.is_empty()
            {
                return Err(Error::Invalid("Choose a Spotify Track occurrence".into()));
            }
            if !tracks.insert(track.as_ref())
                || !occurrences.insert(candidate.identity.external_id.as_str())
            {
                return Err(Error::Invalid(
                    "Each canonical and Spotify Track may be mapped only once".into(),
                ));
            }
            let local = snapshot
                .iter()
                .find(|t| t.track_id == *track)
                .ok_or_else(|| Error::Invalid("Track is outside the reviewed Album".into()))?;
            let mut warnings = vec![];
            if crate::matching::normalize(local.evidence.title.as_deref().unwrap_or(""))
                != crate::matching::normalize(&candidate.title)
            {
                warnings.push(format!(
                    "Title: {} → {}",
                    local.evidence.title.as_deref().unwrap_or(""),
                    candidate.title
                ));
            }
            if crate::artist_credit::compare(&local.evidence.artists, &candidate.artists)
                != crate::artist_credit::Compatibility::Equivalent
            {
                warnings.push(format!("Artist credit: provider {}", candidate.artist));
            }
            if local.evidence.duration_ms.is_some_and(|d| {
                candidate.duration_ms > 0 && d.abs_diff(candidate.duration_ms) > 10_000
            }) {
                warnings.push("Duration differs by more than 10 seconds".into());
            }
            if local.evidence.number != Some(candidate.number)
                || local.evidence.disc.unwrap_or(1) != candidate.disc
            {
                warnings.push(format!(
                    "Position: {:?}.{:?} → {}.{}",
                    local.evidence.disc, local.evidence.number, candidate.disc, candidate.number
                ));
            }
            matches.push(PositionedMatch {
                track_id: track.clone(),
                disc: candidate.disc,
                number: candidate.number,
                local_title: local.evidence.title.clone().unwrap_or_default(),
                provider_title: candidate.title.clone(),
                warnings,
                candidate: candidate.clone(),
            });
        }
        Ok(AlbumSelection {
            album: album.clone(),
            provider: None,
            unmatched_local: snapshot
                .iter()
                .filter(|t| !tracks.contains(t.track_id.as_ref()))
                .map(|t| t.track_id.clone())
                .collect(),
            unmatched_provider: 0,
            warnings: vec![
                "Explicit Track mapping; provider Albums do not redefine the canonical Album"
                    .into(),
            ],
            matches,
            snapshot,
        })
    }

    /// Snapshot operation only: future Tracks inherit no exclusion.
    pub fn mark_album_not_on_spotify(&mut self, album: &AlbumId) -> Result<usize> {
        let tracks = self
            .local_album_tracks(album)?
            .into_iter()
            .map(|t| t.track_id)
            .collect::<Vec<_>>();
        self.mark_tracks_not_on_spotify(&tracks)
    }

    /// Atomic explicit confirmation, guarded against changed canonical evidence.
    pub fn confirm_spotify_album(
        &mut self,
        selection: &AlbumSelection,
        confirm_differences: bool,
    ) -> Result<usize> {
        if selection.requires_confirmation() && !confirm_differences {
            return Err(Error::Invalid(
                "Confirm significant positioned differences before connecting".into(),
            ));
        }
        let tx = self
            .store
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if crate::album_program::local_tracks(&tx, &selection.album)?.0 != selection.snapshot {
            return Err(Error::ReconciliationEvidenceChanged);
        }
        if let Some(provider) = &selection.provider {
            tx.execute("INSERT INTO album_external_identity(album_id,provider,kind,external_id) VALUES(?1,'spotify','album',?2) ON CONFLICT DO NOTHING",params![selection.album.as_ref(),provider.external_id])?;
        }
        let mut connected = 0;
        let ids = serde_json::to_string(
            &selection
                .matches
                .iter()
                .map(|m| m.track_id.as_ref())
                .collect::<Vec<_>>(),
        )
        .map_err(|e| Error::Invalid(e.to_string()))?;
        let ineligible:std::collections::HashSet<String>=tx.prepare("SELECT track_id FROM track_provider_exclusion WHERE provider='spotify' AND ?2 AND track_id IN(SELECT value FROM json_each(?1)) UNION SELECT track_id FROM trusted_spotify_track WHERE track_id IN(SELECT value FROM json_each(?1))")?.query_map(params![ids,selection.provider.is_some()],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
        for m in &selection.matches {
            if ineligible.contains(m.track_id.as_ref()) {
                continue;
            }
            crate::song_resolution::persist_occurrence(&tx, &m.track_id, &m.candidate)?;
            if selection.provider.is_none() {
                tx.execute(
                    "DELETE FROM track_provider_exclusion WHERE track_id=?1 AND provider='spotify'",
                    [m.track_id.as_ref()],
                )?;
            }
            connected += 1;
        }
        tx.commit()?;
        Ok(connected)
    }
}

/// Strict object parsing; input never becomes an arbitrary provider URL.
pub fn parse_spotify_object(value: &str, kind: &str) -> Result<ExternalIdentity> {
    if !matches!(kind, "album" | "track") {
        return Err(Error::Invalid("Expected Album or Track".into()));
    }
    let value = value.trim();
    let key = if let Some(uri) = value.strip_prefix("spotify:") {
        uri.strip_prefix(&format!("{kind}:"))
    } else if let Some(url) = value.strip_prefix("https://open.spotify.com/") {
        let path = url.split(['?', '#']).next().unwrap_or("");
        let path = if path.starts_with("intl-") {
            path.split_once('/').map(|(_, p)| p).unwrap_or("")
        } else {
            path
        };
        path.strip_prefix(&format!("{kind}/"))
    } else {
        Some(value)
    };
    let key = key
        .filter(|key| key.len() == 22 && key.bytes().all(|b| b.is_ascii_alphanumeric()))
        .ok_or_else(|| {
            Error::Invalid(format!(
                "Paste a Spotify {kind} URL, URI, or 22-character ID"
            ))
        })?;
    Ok(ExternalIdentity {
        provider: "spotify".into(),
        kind: kind.into(),
        external_id: key.into(),
    })
}
