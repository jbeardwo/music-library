//! Explicit user choices inside an established Album. No discovery or edition claims.
use crate::{
    album_program::{Match, Programs, RecordingStatus},
    domain::*,
    edition::TrackEvidence,
    storage::{Error, Result, Store},
};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Candidate {
    pub evidence: TrackEvidence,
    pub supporting_programs: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Association {
    pub track_id: TrackId,
    pub album: ExternalIdentity,
    pub candidate: Candidate,
}
impl Association {
    pub fn matched(&self) -> Match {
        Match {
            title: self.candidate.evidence.title.clone().unwrap_or_default(),
            recording: self.candidate.evidence.recording.clone(),
            recording_status: if self.candidate.evidence.recording.identities.is_empty() {
                RecordingStatus::NotProvided
            } else {
                RecordingStatus::Identified
            },
            occurrences: self.candidate.evidence.identities.clone(),
            explanation:
                "Explicit user confirmation within the known Album; no exact edition claim".into(),
        }
    }
}

/// Prepared from bounded provider programs. Private context prevents frontend
/// callers substituting a candidate from an unrelated Album at confirmation.
#[derive(Clone, Debug)]
pub struct Selection {
    album_id: AlbumId,
    track_id: TrackId,
    album: ExternalIdentity,
    candidates: Vec<Candidate>,
}
impl Selection {
    pub fn candidates(&self) -> &[Candidate] {
        &self.candidates
    }
    pub fn track_id(&self) -> &TrackId {
        &self.track_id
    }
    pub fn album_id(&self) -> &AlbumId {
        &self.album_id
    }
}

/// Group only by equal strong Recording identity sets, else equal occurrence
/// identity sets. Text alone never merges candidates. Program IDs stay diagnostic
/// upstream; they are not part of the persisted manual association.
pub fn candidates(programs: &Programs) -> Vec<Candidate> {
    type Key = (u8, Vec<(String, String, String)>, usize, usize);
    let mut groups: BTreeMap<Key, (Vec<TrackEvidence>, BTreeSet<usize>)> = BTreeMap::new();
    for (p, program) in programs.programs.iter().enumerate() {
        for (n, track) in program.tracks.iter().enumerate() {
            let (kind, ids) = if !track.recording.identities.is_empty() {
                (0, &track.recording.identities)
            } else {
                (1, &track.identities)
            };
            let mut ids: Vec<_> = ids
                .iter()
                .map(|i| (i.provider.clone(), i.kind.clone(), i.external_id.clone()))
                .collect();
            ids.sort();
            ids.dedup();
            let key = if ids.is_empty() {
                (2, ids, p, n)
            } else {
                (kind, ids, 0, 0)
            };
            let entry = groups.entry(key).or_default();
            entry.0.push(track.clone());
            entry.1.insert(p);
        }
    }
    let mut result = vec![];
    for (_, (mut evidence, programs)) in groups {
        evidence.sort_by_key(|e| (e.title.clone(), e.duration_ms, e.disc, e.number));
        let mut primary = evidence[0].clone();
        for e in &evidence {
            for id in &e.identities {
                if !primary.identities.contains(id) {
                    primary.identities.push(id.clone());
                }
            }
            for isrc in &e.recording.isrcs {
                if !primary.recording.isrcs.contains(isrc) {
                    primary.recording.isrcs.push(isrc.clone());
                }
            }
        }
        result.push(Candidate {
            evidence: primary,
            supporting_programs: programs.len(),
        });
    }
    result.sort_by_key(|c| {
        (
            c.evidence.disc,
            c.evidence.number,
            c.evidence.title.clone(),
            c.evidence.duration_ms,
        )
    });
    result
}

fn context(db: &Connection, album: &AlbumId, track: &TrackId) -> Result<RecordingId> {
    db.query_row("SELECT t.recording_id FROM track t JOIN release r ON r.id=t.release_id WHERE t.id=?1 AND r.album_id=?2",params![track.as_ref(),album.as_ref()],|r|r.get(0).map(RecordingId)).optional()?.ok_or_else(||Error::Invalid("Track is not in this Album".into()))
}
fn established(db: &Connection, album: &AlbumId, id: &ExternalIdentity) -> Result<()> {
    let known:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM album_external_identity WHERE album_id=?1 AND provider=?2 AND kind=?3 AND external_id=?4)",params![album.as_ref(),id.provider,id.kind,id.external_id],|r|r.get(0))?;
    if !known {
        return Err(Error::Invalid(
            "Provider Album identity is no longer established".into(),
        ));
    }
    Ok(())
}
pub(crate) fn load(db: &Connection, album: &AlbumId) -> Result<Vec<Association>> {
    db.prepare("SELECT m.track_id,m.album_provider,m.album_kind,m.album_external_id,m.candidate_json FROM release r CROSS JOIN track t ON t.release_id=r.id JOIN manual_track_association m ON m.track_id=t.id WHERE r.album_id=?1 ORDER BY t.disc_number,t.track_number,t.id")?.query_map([album.as_ref()],|r|Ok((TrackId(r.get(0)?),ExternalIdentity{provider:r.get(1)?,kind:r.get(2)?,external_id:r.get(3)?},r.get::<_,String>(4)?)))?.map(|row|{
        let (track_id,album,json)=row?;
        let candidate=serde_json::from_str(&json).map_err(|e|Error::Invalid(format!("Invalid manual association snapshot: {e}")))?;
        Ok(Association{track_id,album,candidate})
    }).collect()
}

impl Store {
    pub fn manual_track_associations(&self, album: &AlbumId) -> Result<Vec<Association>> {
        load(&self.connection, album)
    }
    pub fn prepare_manual_track(
        &self,
        album: &AlbumId,
        track: &TrackId,
        programs: &Programs,
    ) -> Result<Selection> {
        context(&self.connection, album, track)?;
        established(&self.connection, album, &programs.album)?;
        Ok(Selection {
            album_id: album.clone(),
            track_id: track.clone(),
            album: programs.album.clone(),
            candidates: candidates(programs),
        })
    }
    pub fn confirm_manual_track(
        &mut self,
        selection: &Selection,
        index: usize,
    ) -> Result<Association> {
        let candidate = selection
            .candidates
            .get(index)
            .ok_or_else(|| Error::Invalid("Select a candidate before confirming".into()))?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let recording = context(&tx, &selection.album_id, &selection.track_id)?;
        established(&tx, &selection.album_id, &selection.album)?;
        let association = Association {
            track_id: selection.track_id.clone(),
            album: selection.album.clone(),
            candidate: candidate.clone(),
        };
        if let Some(existing) = load(&tx, &selection.album_id)?
            .into_iter()
            .find(|a| a.track_id == selection.track_id)
        {
            if existing == association {
                return Ok(existing);
            }
            return Err(Error::Invalid("Clear the current manual association first; conflicting canonical identity replacement is not supported".into()));
        }
        let ids = &candidate.evidence.recording.identities;
        for id in ids {
            if ids.iter().any(|other| {
                other.provider == id.provider
                    && other.kind == id.kind
                    && other.external_id != id.external_id
            }) {
                return Err(Error::Invalid(
                    "Candidate has contradictory Recording identities".into(),
                ));
            }
            let conflict:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM recording_external_identity WHERE recording_id=?1 AND provider=?2 AND kind=?3 AND external_id<>?4)",params![recording.as_ref(),id.provider,id.kind,id.external_id],|r|r.get(0))?;
            if conflict {
                return Err(Error::Invalid("Conflicting existing canonical Recording identity; replacement requires a separate correction path".into()));
            }
        }
        let json = serde_json::to_string(candidate).map_err(|e| Error::Invalid(e.to_string()))?;
        tx.execute("INSERT INTO manual_track_association(track_id,album_provider,album_kind,album_external_id,candidate_json) VALUES(?1,?2,?3,?4,?5)",params![selection.track_id.as_ref(),selection.album.provider,selection.album.kind,selection.album.external_id,json])?;
        for id in ids {
            let (exists,provenance):(bool,bool)=tx.query_row("SELECT EXISTS(SELECT 1 FROM recording_external_identity WHERE recording_id=?1 AND provider=?2 AND kind=?3 AND external_id=?4),EXISTS(SELECT 1 FROM recording_provenance_identity WHERE recording_id=?1 AND provider=?2 AND kind=?3 AND external_id=?4)",params![recording.as_ref(),id.provider,id.kind,id.external_id],|r|Ok((r.get(0)?,r.get(1)?)))?;
            // Another manual choice is not a new independent confirmation. Keep
            // its ownership marker until the last manual claim is cleared.
            if !exists || provenance {
                tx.execute("INSERT INTO recording_external_identity(recording_id,provider,kind,external_id) VALUES(?1,?2,?3,?4) ON CONFLICT DO NOTHING",params![recording.as_ref(),id.provider,id.kind,id.external_id])?;
                tx.execute("INSERT INTO recording_manual_identity(recording_id,provider,kind,external_id) VALUES(?1,?2,?3,?4) ON CONFLICT DO NOTHING",params![recording.as_ref(),id.provider,id.kind,id.external_id])?;
            }
            tx.execute("INSERT INTO manual_track_recording_claim(track_id,recording_id,provider,kind,external_id) VALUES(?1,?2,?3,?4,?5) ON CONFLICT DO NOTHING",params![selection.track_id.as_ref(),recording.as_ref(),id.provider,id.kind,id.external_id])?;
        }
        tx.commit()?;
        Ok(association)
    }
    pub fn clear_manual_track(&mut self, album: &AlbumId, track: &TrackId) -> Result<bool> {
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        context(&tx, album, track)?;
        let changed = tx.execute(
            "DELETE FROM manual_track_association WHERE track_id=?1",
            [track.as_ref()],
        )? > 0;
        // Triggers release solely manual canonical claims, retaining independent
        // confirmations and claims held by other manually associated Tracks.
        if changed {
            crate::provenance_acceptance::reconcile(
                &tx,
                &[album.as_ref().to_owned()],
                self.provenance_validator,
                true,
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }
}
