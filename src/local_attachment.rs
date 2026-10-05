//! Persisted, bounded identity reconciliation before local import creates entities.
//! Uses the existing program comparator; no network or display-only identity rule.
use crate::{
    domain::*,
    edition::*,
    provenance::{Origin, Scope},
    storage::{Result, Store},
};
use rusqlite::{Transaction, params};
use std::collections::{BTreeSet, HashMap};

// Validate claims with the adapter capability, retaining conflicting namespaces.
fn claims(
    metadata: &ObservedMetadata,
    scope: Scope,
    validator: crate::provenance_acceptance::Validator,
) -> Vec<ExternalIdentity> {
    let observations = &metadata.provenance.observations;
    observations
        .iter()
        .filter(|o| {
            o.scope == scope
                && matches!(o.origin, Origin::EmbeddedTag { .. })
                && validator(o) == crate::provenance_acceptance::Validation::Valid
                && !observations.iter().any(|other| {
                    other.scope == scope
                        && other.identity.provider == o.identity.provider
                        && other.identity.kind == o.identity.kind
                        && (other.identity != o.identity
                            || validator(other)
                                == crate::provenance_acceptance::Validation::Invalid)
                })
        })
        .map(|o| o.identity.clone())
        .fold(Vec::new(), |mut ids, id| {
            if !ids.contains(&id) {
                ids.push(id);
            }
            ids
        })
}

fn owners(
    tx: &Transaction<'_>,
    entity: &str,
    ids: &[ExternalIdentity],
) -> Result<BTreeSet<String>> {
    let mut result = BTreeSet::new();
    let mut q = tx.prepare(&format!("SELECT {entity}_id FROM {entity}_external_identity WHERE provider=?1 AND kind=?2 AND external_id=?3 LIMIT 65"))?;
    for id in ids {
        for row in q.query_map(params![id.provider, id.kind, id.external_id], |r| {
            r.get::<_, String>(0)
        })? {
            result.insert(row?);
        }
    }
    Ok(result)
}

fn compatible(
    tx: &Transaction<'_>,
    entity: &str,
    owner: &str,
    ids: &[ExternalIdentity],
) -> Result<bool> {
    for id in ids {
        let conflict: bool = tx.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {entity}_external_identity WHERE {entity}_id=?1 AND provider=?2 AND kind=?3 AND external_id<>?4)"), params![owner,id.provider,id.kind,id.external_id], |r|r.get(0))?;
        if conflict {
            return Ok(false);
        }
    }
    Ok(true)
}

fn target(
    tx: &Transaction<'_>,
    c: &DiscoveryCandidate,
    validator: crate::provenance_acceptance::Validator,
    context: &mut Option<Option<AlbumId>>,
    request: &ImportReleaseRequest,
    observations: &[crate::storage::ImportObservation],
    editions: &mut HashMap<String, Vec<LocalTrackEvidence>>,
) -> Result<Option<TrackId>> {
    let m = &c.metadata;
    // Invalid/conflicting supported embedded claims are uncertainty, never a
    // reason to discard identity evidence and fall back to names/positions.
    for o in &m.provenance.observations {
        if matches!(o.origin, Origin::EmbeddedTag { .. })
            && (validator(o) == crate::provenance_acceptance::Validation::Invalid
                || (validator(o) == crate::provenance_acceptance::Validation::Valid
                    && m.provenance.observations.iter().any(|other| {
                        other.scope == o.scope
                            && other.identity.provider == o.identity.provider
                            && other.identity.kind == o.identity.kind
                            && other.identity != o.identity
                    })))
        {
            return Ok(None);
        }
    }
    let occurrence = claims(m, Scope::Occurrence, validator);
    let recording = claims(m, Scope::Recording, validator);
    let album_claims = claims(m, Scope::Album, validator);
    let edition_claims = claims(m, Scope::Edition, validator);
    let albums = owners(tx, "album", &album_claims)?;
    let releases = owners(tx, "release", &edition_claims)?;
    let direct = owners(tx, "track", &occurrence)?;
    // A release-specific occurrence is sufficient, but contradictory Album/edition
    // claims must not be ignored. Recording alone never establishes Track edition.
    if !direct.is_empty() {
        if let Some(id) = direct.iter().next().filter(|_| direct.len() == 1) {
            let (release, album): (String,String) = tx.query_row("SELECT t.release_id,r.album_id FROM track t JOIN release r ON r.id=t.release_id WHERE t.id=?1", [id], |r|Ok((r.get(0)?,r.get(1)?)))?;
            if (!albums.is_empty() && (albums.len() != 1 || !albums.contains(&album)))
                || (!releases.is_empty() && (releases.len() != 1 || !releases.contains(&release)))
            {
                return Ok(None);
            }
            if !compatible(tx, "album", &album, &album_claims)?
                || !compatible(tx, "release", &release, &edition_claims)?
            {
                return Ok(None);
            }
            // Check Recording contradictions through the shared comparator below.
            if recording.is_empty() {
                return Ok(Some(TrackId(id.clone())));
            }
        } else {
            return Ok(None);
        }
    }
    let mut candidates = releases;
    for album in &albums {
        for row in tx
            .prepare("SELECT id FROM release WHERE album_id=?1 LIMIT 65")?
            .query_map([album], |r| r.get::<_, String>(0))?
        {
            candidates.insert(row?);
        }
    }
    if candidates.is_empty() && !direct.is_empty() {
        for id in &direct {
            candidates.insert(tx.query_row(
                "SELECT release_id FROM track WHERE id=?1",
                [id],
                |r| r.get(0),
            )?);
        }
    }
    // Reuse the importer's established Album context (credit + positioned title
    // support with no contradictory overlaps). Album agreement alone still does
    // not select an edition or Track; every plausible edition is checked below.
    if candidates.is_empty() {
        if context.is_none() {
            *context = Some(crate::storage::supported_local_album(
                tx,
                request,
                observations,
            )?);
        }
        if let Some(Some(album)) = context {
            for row in tx
                .prepare("SELECT id FROM release WHERE album_id=?1 LIMIT 65")?
                .query_map([album.as_ref()], |r| r.get::<_, String>(0))?
            {
                candidates.insert(row?);
            }
        }
    }
    if candidates.is_empty() || candidates.len() > 64 {
        return Ok(None);
    }
    let artists = m
        .track_artists
        .iter()
        .enumerate()
        .map(|(i, name)| ArtistEvidence {
            name: name.clone(),
            join_phrase: if i + 1 < m.track_artists.len() {
                ", ".into()
            } else {
                String::new()
            },
            identities: if m.track_artists.len() == 1 {
                claims(m, Scope::TrackArtist, validator)
            } else {
                vec![]
            },
        })
        .collect();
    let local = LocalTrackEvidence {
        track_id: TrackId::new(),
        recording_id: RecordingId::new(),
        evidence: TrackEvidence {
            title: m.track_title.clone(),
            disc: m.disc_number,
            number: m.track_number,
            duration_ms: m.duration_ms,
            artists,
            identities: occurrence.clone(),
            recording: RecordingEvidence {
                identities: recording.clone(),
                isrcs: vec![],
            },
            ..Default::default()
        },
    };
    let mut supported = BTreeSet::new();
    for release in candidates {
        let album: String = tx.query_row(
            "SELECT album_id FROM release WHERE id=?1",
            [&release],
            |r| r.get(0),
        )?;
        if (!albums.is_empty() && !albums.contains(&album))
            || !compatible(tx, "album", &album, &album_claims)?
            || !compatible(tx, "release", &release, &edition_claims)?
        {
            continue;
        }

        // Metadata-only local editions do not become canonical merely because
        // another copy has identical tags. Retain the importer's conservative
        // local/local behavior unless persisted provider identity supports it.
        if !editions.contains_key(&release) {
            let mut tracks = tx.prepare("SELECT t.id,t.recording_id,t.disc_number,t.track_number,e.title,e.duration_ms FROM track t JOIN effective_track_metadata e ON e.track_id=t.id WHERE t.release_id=?1
                    AND (NOT EXISTS(SELECT 1 FROM track_source ts JOIN playable_source ps ON ps.id=ts.source_id WHERE ts.track_id=t.id AND ps.kind='local_file')
                        OR EXISTS(SELECT 1 FROM track_external_identity i WHERE i.track_id=t.id)
                        OR EXISTS(SELECT 1 FROM recording_external_identity i WHERE i.recording_id=t.recording_id)
                        OR EXISTS(SELECT 1 FROM release_external_identity i WHERE i.release_id=t.release_id))
                    ORDER BY t.disc_number,t.track_number,t.id LIMIT 4097")?.query_map([&release], |r|Ok(LocalTrackEvidence{track_id:TrackId(r.get(0)?),recording_id:RecordingId(r.get(1)?),evidence:TrackEvidence{disc:r.get(2)?,number:r.get(3)?,title:r.get(4)?,duration_ms:r.get::<_,Option<i64>>(5)?.and_then(|v|u64::try_from(v).ok()),..Default::default()}}))?.collect::<rusqlite::Result<Vec<_>>>()?;
            if tracks.len() > 4096 {
                return Ok(None);
            }
            crate::edition_storage::load_track_evidence(tx, &mut tracks)?;
            editions.insert(release.clone(), tracks);
        }
        let tracks = &editions[&release];
        for (index, t) in tracks.iter().enumerate() {
            let check = crate::album_program::inspect_candidate(&local, &t.evidence, index);
            if check.considered
                && !check.identity_conflict
                && check.occurrence_supported
                && (direct.is_empty() || direct.contains(t.track_id.as_ref()))
                && compatible(tx, "track", t.track_id.as_ref(), &occurrence)?
                && (check.trusted_identity || (check.exact_title && check.position))
            {
                // Explicit positions must agree, even when a comparator supports
                // flattened provider programs. Persisted Tracks are edition-specific.
                if m.disc_number
                    .zip(t.evidence.disc)
                    .is_some_and(|(a, b)| a != b)
                    || m.track_number
                        .zip(t.evidence.number)
                        .is_some_and(|(a, b)| a != b)
                {
                    continue;
                }
                supported.insert(t.track_id.0.clone());
            }
        }
    }
    Ok(if supported.len() == 1 {
        supported.into_iter().next().map(TrackId)
    } else {
        None
    })
}

impl Store {
    /// Shared local import preflight. Source attachment never modifies membership.
    pub(crate) fn attach_local_candidates(
        &mut self,
        candidates: &[DiscoveryCandidate],
        request: &ImportReleaseRequest,
    ) -> Result<Vec<SourceId>> {
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let observations = crate::storage::import_observations(&tx, request)?;
        let mut context = None;
        let mut editions = HashMap::new();
        let mut plan = vec![];
        for c in candidates {
            if let Some(track) = target(
                &tx,
                c,
                self.provenance_validator,
                &mut context,
                request,
                &observations,
                &mut editions,
            )? {
                plan.push((c.source_id.clone(), track));
            }
        }
        let mut attached = vec![];
        for (source, track) in plan {
            tx.execute(
                "INSERT INTO track_source(track_id,source_id) VALUES(?1,?2)",
                params![track.as_ref(), source.as_ref()],
            )?;
            tx.execute(
                "DELETE FROM local_source_suppression WHERE source_id=?1",
                [source.as_ref()],
            )?;
            crate::storage::refresh_effective_track_tx(&tx, &track)?;
            attached.push(source);
        }
        let albums = crate::provenance_acceptance::albums_for_sources(
            &tx,
            &attached.iter().map(|s| s.0.clone()).collect::<Vec<_>>(),
        )?;
        crate::provenance_acceptance::reconcile(&tx, &albums, self.provenance_validator, true)?;
        tx.commit()?;
        Ok(attached)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attachment_narrowing_uses_persisted_identity_and_album_indexes() {
        let store = Store::open_in_memory().unwrap();
        for sql in [
            "SELECT track_id FROM track_external_identity WHERE provider=?1 AND kind=?2 AND external_id=?3 LIMIT 65",
            "SELECT album_id FROM album_external_identity WHERE provider=?1 AND kind=?2 AND external_id=?3 LIMIT 65",
            "SELECT release_id FROM release_external_identity WHERE provider=?1 AND kind=?2 AND external_id=?3 LIMIT 65",
            "SELECT album_id FROM album_application_metadata WHERE match_title=?1 AND match_artist_credit=?2 LIMIT 65",
            "SELECT t.id,e.title FROM track t JOIN effective_track_metadata e ON e.track_id=t.id WHERE t.release_id=?1 ORDER BY t.disc_number,t.track_number,t.id LIMIT 4097",
        ] {
            let mut query = store
                .connection
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap();
            for i in 1..=query.parameter_count() {
                query.raw_bind_parameter(i, "value").unwrap();
            }
            let mut rows = query.raw_query();
            let mut plan = String::new();
            while let Some(row) = rows.next().unwrap() {
                plan.push_str(&row.get::<_, String>(3).unwrap());
                plan.push('\n');
            }
            assert!(plan.contains("SEARCH"), "{plan}");
            assert!(!plan.contains("SCAN"), "{plan}");
        }
    }
}
