//! Bounded MusicBrainz observations and lifecycle. No catalog identity is inferred here.
use crate::{Error, Library, Result, album_program::Programs, catalog::AlbumCandidate, domain::*};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
pub const EVALUATION_VERSION: i64 = 2;
fn run_id() -> &'static str {
    static RUN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    RUN.get_or_init(|| uuid::Uuid::new_v4().to_string())
}

pub const OBSERVATION_TTL: i64 = 30 * 24 * 60 * 60;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    NotAttempted,
    Matching,
    CandidatesFound,
    AlbumTitleConflict,
    ArtistUnresolved,
    NoCandidates,
    DiscoveryBudget,
    CompetingGroups,
    ProviderDeferred,
    ProviderError,
    ProgramIncomplete,
    RecordingAmbiguous,
    PartialMapping,
    EvidenceChanged,
    ProgramRetained,
    VersionUncertain,
    InsufficientEvidence,
    IdentityConflict,
    ExistingGroupConflict,
    DurationConflict,
}
impl Reason {
    pub fn code(self) -> String {
        serde_json::to_value(self).unwrap().as_str().unwrap().into()
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::NotAttempted => "Not checked yet",
            Self::Matching => "Matching…",
            Self::CandidatesFound => "Candidate found — needs review",
            Self::AlbumTitleConflict => "Candidate rejected: Album title conflict",
            Self::ArtistUnresolved => "Artist identity needs review",
            Self::DiscoveryBudget => "Discovery deferred — bounded search exhausted",
            Self::CompetingGroups => "Candidate found — competing Release Groups",
            Self::NoCandidates => "No candidates after bounded search",
            Self::ProviderDeferred => "Provider temporarily unavailable",
            Self::ProviderError => "Provider request failed",
            Self::ProgramIncomplete => "Candidate program incomplete",
            Self::RecordingAmbiguous => "Recording correspondence ambiguous",
            Self::PartialMapping => "Candidate retained — partial mapping",
            Self::EvidenceChanged => "Evidence changed — re-evaluate",
            Self::ProgramRetained => "Candidate program retained",
            Self::VersionUncertain => "Track version needs review",
            Self::InsufficientEvidence => {
                "Candidate retained — insufficient correspondence evidence"
            }
            Self::ExistingGroupConflict => {
                "Existing trusted Album identity conflicts with alternate candidate"
            }
            Self::IdentityConflict => "Trusted identity conflicts",
            Self::DurationConflict => "Track duration conflicts",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Candidate {
    pub metadata: AlbumCandidate,
    pub programs: Option<Programs>,
    pub inspection: Option<Programs>,
    pub inspection_observed_at: Option<i64>,
    pub origin: String,
    pub provider_rank: i64,
    pub reason: String,
    pub observed_at: i64,
    pub program_observed_at: Option<i64>,
}
impl Candidate {
    pub fn review_programs(&self) -> Option<&Programs> {
        self.inspection.as_ref().or(self.programs.as_ref())
    }
    pub fn review_observed_at(&self) -> i64 {
        self.inspection_observed_at
            .or(self.program_observed_at)
            .unwrap_or(self.observed_at)
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Review {
    pub reason: Reason,
    pub candidates: Vec<Candidate>,
    pub stale: bool,
}
fn encode(v: &impl Serialize) -> Result<String> {
    serde_json::to_string(v).map_err(|e| Error::Invalid(e.to_string()))
}
fn decode<T: serde::de::DeserializeOwned>(s: &str) -> Result<T> {
    serde_json::from_str(s).map_err(|e| Error::Invalid(e.to_string()))
}
/// This is an evaluation fingerprint, never a durable entity identifier. All work is Album-scoped.
pub(crate) fn fingerprint(db: &Connection, album: &AlbumId) -> Result<String> {
    let metadata:(String,String)=db.query_row("SELECT title,COALESCE(artist_credit,match_artist_credit) FROM effective_album_metadata WHERE album_id=?1",[album.as_ref()],|r|Ok((r.get(0)?,r.get(1)?)))?;
    let tracks = crate::album_program::local_tracks(db, album)?.0;
    let membership=db.prepare("SELECT m.track_id FROM library_membership m JOIN track t ON t.id=m.track_id JOIN release r ON r.id=t.release_id WHERE r.album_id=?1 ORDER BY m.track_id")?.query_map([album.as_ref()],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let values = tracks
        .iter()
        .map(|t| {
            (
                &t.track_id,
                &t.evidence.title,
                &t.evidence.title_observations,
                &t.evidence.position_observations,
                t.evidence.disc,
                t.evidence.number,
                t.evidence.duration_ms,
                &t.evidence.recording.identities,
                &t.evidence.identities,
                &t.evidence.artists,
            )
        })
        .collect::<Vec<_>>();
    let groups=db.prepare("SELECT kind,external_id FROM album_external_identity WHERE album_id=?1 AND provider='musicbrainz' ORDER BY kind,external_id")?.query_map([album.as_ref()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let artists=db.prepare("SELECT DISTINCT a.id,a.name FROM artist a WHERE a.id IN(SELECT artist_id FROM album_artist_credit WHERE album_id=?1 UNION SELECT c.artist_id FROM track_artist_credit c JOIN track t ON t.id=c.track_id JOIN release r ON r.id=t.release_id WHERE r.album_id=?1) ORDER BY a.id")?.query_map([album.as_ref()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let bytes = encode(&(metadata, values, membership, groups, artists))?;
    Ok(uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, bytes.as_bytes()).to_string())
}
pub(crate) fn state(
    db: &Connection,
    album: &AlbumId,
    reason: Reason,
    track_reasons: &serde_json::Value,
) -> Result<()> {
    db.execute("INSERT INTO musicbrainz_match_state(album_id,fingerprint,state,reason,track_reasons,evaluation_version,run_id) VALUES(?1,?2,?3,?3,?4,?5,?6) ON CONFLICT(album_id) DO UPDATE SET run_id=excluded.run_id,fingerprint=excluded.fingerprint,state=excluded.state,reason=excluded.reason,track_reasons=excluded.track_reasons,evaluation_version=excluded.evaluation_version,updated_at=unixepoch()",params![album.as_ref(),fingerprint(db,album)?,reason.code(),encode(track_reasons)?,EVALUATION_VERSION,run_id()])?;
    Ok(())
}
pub(crate) fn observe(
    db: &Connection,
    album: &AlbumId,
    candidate: &AlbumCandidate,
    origin: &str,
    rank: i64,
    reason: Reason,
) -> Result<()> {
    if candidate.identity.provider != "musicbrainz"
        || candidate.identity.kind != "release_group"
        || candidate.identity.external_id.is_empty()
    {
        return Err(Error::Invalid(
            "Expected MusicBrainz Release Group candidate".into(),
        ));
    }
    db.execute("INSERT INTO musicbrainz_candidate(album_id,group_id,candidate_json,origin,provider_rank,reason) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(album_id,group_id) DO UPDATE SET candidate_json=excluded.candidate_json,origin=excluded.origin,provider_rank=excluded.provider_rank,reason=excluded.reason,observed_at=unixepoch()",params![album.as_ref(),candidate.identity.external_id,encode(candidate)?,origin,rank,reason.code()])?;
    db.execute("DELETE FROM musicbrainz_candidate WHERE album_id=?1 AND group_id NOT IN(SELECT group_id FROM musicbrainz_candidate WHERE album_id=?1 ORDER BY (programs_json IS NOT NULL OR inspection_json IS NOT NULL) DESC,provider_rank,group_id LIMIT 40)",params![album.as_ref()])?;
    Ok(())
}
pub(crate) fn retain_programs(db: &Connection, album: &AlbumId, p: &Programs) -> Result<()> {
    if p.programs.len() > 3 || encode(p)?.len() > 8 * 1024 * 1024 {
        return Err(Error::Invalid(
            "MusicBrainz program observation exceeds bounded cache".into(),
        ));
    }
    if p.album.provider != "musicbrainz" || p.album.kind != "release_group" {
        return Err(Error::Invalid("Expected MusicBrainz Group program".into()));
    }
    // Program-only observations remain useful even when historical search metadata is unknown.
    if !db.query_row(
        "SELECT EXISTS(SELECT 1 FROM musicbrainz_candidate WHERE album_id=?1 AND group_id=?2)",
        params![album.as_ref(), p.album.external_id],
        |r| r.get::<_, bool>(0),
    )? {
        let title:String=db.query_row("SELECT COALESCE((SELECT title FROM album_provider_evidence WHERE album_id=?1 AND provider='musicbrainz' AND kind='release_group' AND external_id=?2),'')",params![album.as_ref(),p.album.external_id],|r|r.get(0))?;
        let artist = String::new();
        let c = AlbumCandidate {
            identity: p.album.clone(),
            title,
            artist,
            credits: vec![],
            date: String::new(),
            primary_type: String::new(),
            secondary_types: vec![],
            comment: "Group program; provider presentation not retained".into(),
            score: None,
        };
        observe(db, album, &c, "retained", 0, Reason::ProgramRetained)?;
    }
    db.execute("UPDATE musicbrainz_candidate SET programs_json=?3,program_observed_at=unixepoch(),reason=CASE WHEN json_valid(reason) THEN reason ELSE ?4 END WHERE album_id=?1 AND group_id=?2",params![album.as_ref(),p.album.external_id,encode(p)?,Reason::ProgramRetained.code()])?;
    evaluate(db, album, p)?;
    Ok(())
}
/// Re-evaluates current canonical evidence. No mapping decision is cached as immutable truth.
pub(crate) fn evaluate(db: &Connection, album: &AlbumId, p: &Programs) -> Result<()> {
    use crate::musicbrainz_recording::{RecordingCertainty as C, assess};
    let local = crate::album_program::local_tracks(db, album)?.0;
    let title: String = db.query_row(
        "SELECT title FROM effective_album_metadata WHERE album_id=?1",
        [album.as_ref()],
        |r| r.get(0),
    )?;
    let a = assess(&local, p, &title);
    let mut reasons = serde_json::Map::new();
    let mut unresolved = vec![];
    for d in a.recordings {
        if matches!(d.certainty, C::Supported | C::AlreadyKnown) {
            continue;
        }
        let r = match d.certainty {
            C::IncompleteEvidence => Reason::ProgramIncomplete,
            C::RecordingAmbiguous => Reason::RecordingAmbiguous,
            C::ConflictingIdentity => Reason::IdentityConflict,
            C::ArtistUntrusted => Reason::ArtistUnresolved,
            C::VersionUncertain => Reason::VersionUncertain,
            C::NoCorrespondence | C::InsufficientContext => Reason::InsufficientEvidence,
            C::DurationConflict => Reason::DurationConflict,
            _ => Reason::PartialMapping,
        };
        reasons.insert(
            d.track.0,
            serde_json::json!({"code": r.code(), "label": r.label()}),
        );
        unresolved.push(r);
    }
    let reason = if unresolved.contains(&Reason::RecordingAmbiguous) {
        Reason::RecordingAmbiguous
    } else if unresolved.contains(&Reason::PartialMapping) {
        Reason::PartialMapping
    } else if !unresolved.is_empty() {
        unresolved[0]
    } else if a.incomplete_samples > 0 {
        Reason::ProgramIncomplete
    } else {
        Reason::ProgramRetained
    };
    let group_conflict:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM musicbrainz_candidate c WHERE c.album_id=?1 AND EXISTS(SELECT 1 FROM json_each(CASE WHEN json_valid(c.reason) THEN c.reason ELSE '{}' END,'$.evaluation.reasons') WHERE value='existing_trusted_group_conflict') AND EXISTS(SELECT 1 FROM json_each(CASE WHEN json_valid(c.reason) THEN c.reason ELSE '{}' END,'$.evaluation.mappings') WHERE value>=2))",[album.as_ref()],|r|r.get(0))?;
    let reason = if group_conflict && !reasons.is_empty() {
        for value in reasons.values_mut() {
            *value = serde_json::json!({"code":Reason::ExistingGroupConflict.code(),"label":Reason::ExistingGroupConflict.label()});
        }
        Reason::ExistingGroupConflict
    } else {
        reason
    };
    state(db, album, reason, &serde_json::Value::Object(reasons))
}
impl Library {
    pub fn musicbrainz_review(&self, album: &AlbumId) -> Result<Review> {
        let db = &self.store.connection;
        let saved=db.query_row("SELECT fingerprint,reason,evaluation_version,updated_at,run_id FROM musicbrainz_match_state WHERE album_id=?1",[album.as_ref()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?,r.get::<_,String>(4)?))).optional()?;
        let now = db.query_row("SELECT unixepoch()", [], |r| r.get::<_, i64>(0))?;
        let stale = match &saved {
            Some((key, r, v, at, owner)) => {
                *key != fingerprint(db, album)?
                    || *v != EVALUATION_VERSION
                    || now - *at > OBSERVATION_TTL
                    || r == "matching" && owner != run_id()
            }
            None => false,
        };
        let reason = if stale {
            Reason::EvidenceChanged
        } else if let Some((_, r, _, _, _)) = saved {
            decode(&format!("\"{r}\""))?
        } else {
            Reason::NotAttempted
        };
        let mut q=db.prepare("SELECT candidate_json,programs_json,origin,provider_rank,reason,observed_at,program_observed_at,inspection_json,inspection_observed_at FROM musicbrainz_candidate WHERE album_id=?1 ORDER BY COALESCE(json_extract(CASE WHEN json_valid(reason) THEN reason ELSE '{}' END,'$.application_rank'),100), (programs_json IS NOT NULL OR inspection_json IS NOT NULL) DESC,provider_rank,group_id LIMIT 40")?;
        let mut candidates = vec![];
        for row in q.query_map([album.as_ref()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, Option<i64>>(8)?,
            ))
        })? {
            let (meta, p, origin, rank, reason, at, pat, inspection, inspection_at) = row?;
            candidates.push(Candidate {
                metadata: decode(&meta)?,
                programs: p.map(|s| decode(&s)).transpose()?,
                inspection: inspection.map(|s| decode(&s)).transpose()?,
                inspection_observed_at: inspection_at,
                origin,
                provider_rank: rank,
                reason,
                observed_at: at,
                program_observed_at: pat,
            });
        }
        let stale = stale
            || candidates
                .iter()
                .any(|c| now - c.review_observed_at() > OBSERVATION_TTL);
        Ok(Review {
            reason: if stale {
                Reason::EvidenceChanged
            } else {
                reason
            },
            candidates,
            stale,
        })
    }
    pub fn musicbrainz_has_gaps(&self, album: &AlbumId) -> Result<bool> {
        Ok(self.store.connection.query_row("SELECT EXISTS(SELECT 1 FROM release r JOIN track t ON t.release_id=r.id JOIN library_membership m ON m.track_id=t.id WHERE r.album_id=?1 AND NOT EXISTS(SELECT 1 FROM recording_external_identity i WHERE i.recording_id=t.recording_id AND i.provider='musicbrainz' AND i.kind='recording'))",[album.as_ref()],|r|r.get(0))?)
    }
    pub fn prepare_musicbrainz_discovery(
        &self,
        album: &AlbumId,
    ) -> Result<crate::album_matching::Preparation> {
        crate::storage::prepare_album_match(
            &self.store.connection,
            album,
            &crate::catalog::MatchingScope::musicbrainz(),
            true,
        )
    }
    pub fn retained_musicbrainz_discovery(
        &self,
        album: &AlbumId,
    ) -> Result<Option<crate::musicbrainz_discovery::Snapshot>> {
        let db = &self.store.connection;
        let json: Option<String> = db.query_row("SELECT snapshot_json FROM musicbrainz_discovery WHERE album_id=?1 AND unixepoch()-observed_at<?2",params![album.as_ref(),OBSERVATION_TTL],|r|r.get(0)).optional()?;
        let crate::album_matching::Preparation::Ready(input) =
            self.prepare_musicbrainz_discovery(album)?
        else {
            return Ok(None);
        };
        let Some(artist) = input.known_artist else {
            return Ok(None);
        };
        if let Some(json) = json {
            let snapshot: crate::musicbrainz_discovery::Snapshot = decode(&json)?;
            if snapshot.artist.as_ref() == Some(&artist) && snapshot.title == input.raw_title {
                return Ok(Some(snapshot));
            }
        }
        let mut q=db.prepare("SELECT page_offset,page_json,observed_at FROM musicbrainz_artist_page WHERE artist_id=?1 AND unixepoch()-observed_at<?2 ORDER BY page_offset LIMIT 3")?;
        let mut snapshot = crate::musicbrainz_discovery::Snapshot {
            artist: Some(artist.clone()),
            title: input.raw_title,
            ..Default::default()
        };
        for row in q.query_map(params![artist.external_id, OBSERVATION_TTL], |r| {
            Ok((
                r.get::<_, u32>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })? {
            let (offset, json, at) = row?;
            snapshot.observed_at = if snapshot.observed_at == 0 {
                at
            } else {
                snapshot.observed_at.min(at)
            };
            snapshot
                .pages
                .push(crate::musicbrainz_discovery::PageObservation {
                    offset,
                    page: decode(&json)?,
                });
        }
        Ok((!snapshot.pages.is_empty()).then_some(snapshot))
    }
    pub fn has_musicbrainz_discovery(&self, album: &AlbumId) -> Result<bool> {
        Ok(self.store.connection.query_row("SELECT EXISTS(SELECT 1 FROM musicbrainz_discovery WHERE album_id=?1 AND unixepoch()-observed_at<?2)",params![album.as_ref(),OBSERVATION_TTL],|r|r.get(0))?)
    }
    /// Same normal discovery/evaluation/completion path as the worker, useful to
    /// non-UI callers. Existing trusted Group identity remains authoritative.
    pub fn reconcile_musicbrainz(
        &mut self,
        album: &AlbumId,
        provider: &mut impl crate::catalog::CatalogProvider,
    ) -> Result<crate::album_candidates::Resolution> {
        use crate::album_matching::{MatchReply, Preparation, resolve_artist};
        let Preparation::Ready(input) = crate::storage::prepare_album_match(
            &self.store.connection,
            album,
            &crate::catalog::MatchingScope::musicbrainz(),
            true,
        )?
        else {
            return Err(Error::Invalid("Album is not eligible".into()));
        };
        let artist = match input.known_artist.clone() {
            Some(a) => a,
            None => resolve_artist(
                &input.artist,
                &provider
                    .search_artists(&input.artist)
                    .map_err(|e| Error::Invalid(e.to_string()))?,
            )
            .map_err(|_| Error::Invalid("Artist identity needs review".into()))?,
        };
        let local = self.local_album_tracks(album)?;
        let mut resolution = crate::musicbrainz_discovery::discover(
            provider,
            &input,
            &artist,
            &local,
            self.retained_musicbrainz_discovery(album)?,
        );
        let trusted = self
            .list_album_external_identities(album)?
            .into_iter()
            .find(|i| i.provider == "musicbrainz" && i.kind == "release_group");
        if let Some(existing) = &trusted {
            if matches!(&resolution.outcome,crate::album_matching::MatchOutcome::MatchedClose(id)|crate::album_matching::MatchOutcome::Matched(id) if id!=existing)
            {
                resolution.outcome = crate::album_matching::MatchOutcome::NoConfidentMatch;
                resolution.report.reasons =
                    vec![crate::album_candidates::Reason::TrustedIdentityConflict];
            }
        }
        let selected = match &resolution.outcome {
            crate::album_matching::MatchOutcome::Matched(id)
            | crate::album_matching::MatchOutcome::MatchedClose(id) => resolution
                .report
                .candidates
                .iter()
                .find(|c| &c.identity == id)
                .and_then(|c| c.observation.as_ref())
                .map(crate::musicbrainz_discovery::artist_candidate),
            _ => None,
        };
        self.complete_album_match(MatchReply {
            input,
            artist: Some(artist),
            artist_observation: None,
            outcome: resolution.outcome.clone(),
            matched_album: selected,
            diagnostic: Some(resolution.report.clone()),
        })?;
        if let Some(group) = self
            .list_album_external_identities(album)?
            .into_iter()
            .find(|i| i.provider == "musicbrainz" && i.kind == "release_group")
        {
            if self
                .musicbrainz_review(album)?
                .candidates
                .iter()
                .any(|c| c.programs.as_ref().is_some_and(|p| p.album == group))
            {
                self.evaluate_retained_musicbrainz(album, &group)?;
            }
        }
        Ok(resolution)
    }
    pub fn musicbrainz_state(&self, album: &AlbumId, reason: Reason) -> Result<()> {
        state(
            &self.store.connection,
            album,
            reason,
            &serde_json::json!({}),
        )
    }
    pub fn retain_musicbrainz_candidates(
        &mut self,
        album: &AlbumId,
        candidates: &[AlbumCandidate],
        manual: bool,
    ) -> Result<()> {
        let tx = self.store.connection.transaction()?;
        for (rank, c) in candidates.iter().take(10).enumerate() {
            observe(
                &tx,
                album,
                c,
                if manual { "manual" } else { "automatic" },
                rank as i64,
                Reason::CandidatesFound,
            )?;
        }
        state(
            &tx,
            album,
            if candidates.is_empty() {
                Reason::NoCandidates
            } else {
                Reason::CandidatesFound
            },
            &serde_json::json!({}),
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn retain_musicbrainz_programs(&mut self, album: &AlbumId, p: &Programs) -> Result<()> {
        let tx = self.store.connection.transaction()?;
        retain_programs(&tx, album, p)?;
        tx.commit()?;
        Ok(())
    }
    /// Manual Release choice never replaces the ensemble used for automatic certainty.
    pub fn retain_musicbrainz_inspection(&mut self, album: &AlbumId, p: &Programs) -> Result<()> {
        if p.album.provider != "musicbrainz"
            || p.album.kind != "release_group"
            || p.programs.len() != 1
            || encode(p)?.len() > 8 * 1024 * 1024
        {
            return Err(Error::Invalid(
                "Expected one bounded inspected MusicBrainz Release".into(),
            ));
        }
        let tx = self.store.connection.transaction()?;
        let ensemble: Option<String> = tx
            .query_row(
                "SELECT programs_json FROM musicbrainz_candidate WHERE album_id=?1 AND group_id=?2",
                params![album.as_ref(), p.album.external_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        if !tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM musicbrainz_candidate WHERE album_id=?1 AND group_id=?2)",
            params![album.as_ref(), p.album.external_id],
            |r| r.get::<_, bool>(0),
        )? {
            let c = AlbumCandidate {
                identity: p.album.clone(),
                title: String::new(),
                artist: String::new(),
                credits: vec![],
                date: String::new(),
                primary_type: String::new(),
                secondary_types: vec![],
                score: None,
                comment: "Inspected program; presentation unknown".into(),
            };
            observe(&tx, album, &c, "manual", 0, Reason::ProgramRetained)?;
        }
        tx.execute("UPDATE musicbrainz_candidate SET inspection_json=?3,inspection_observed_at=unixepoch() WHERE album_id=?1 AND group_id=?2",params![album.as_ref(),p.album.external_id,encode(p)?])?;
        evaluate(
            &tx,
            album,
            &ensemble
                .map(|s| decode(&s))
                .transpose()?
                .unwrap_or_else(|| p.clone()),
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn evaluate_retained_musicbrainz(
        &mut self,
        album: &AlbumId,
        group: &ExternalIdentity,
    ) -> Result<crate::album_program::Outcome> {
        let review = self.musicbrainz_review(album)?;
        let observed_at = review
            .candidates
            .iter()
            .find(|c| &c.metadata.identity == group)
            .and_then(|c| c.program_observed_at);
        let p = review
            .candidates
            .iter()
            .filter_map(|c| c.programs.as_ref())
            .find(|p| &p.album == group)
            .cloned()
            .ok_or_else(|| Error::Invalid("No retained Group program".into()))?;
        let mut input = self.prepare_album_program(album, group)?.ok_or_else(|| {
            Error::Invalid("Group is not associated; candidate remains review-only".into())
        })?;
        input.provider_observed_at = observed_at;
        self.complete_album_program(crate::album_program::Reply {
            input,
            result: Ok(p),
        })
    }
}

pub(crate) fn record_match(
    db: &Connection,
    album: &AlbumId,
    outcome: &crate::album_matching::MatchOutcome,
    report: Option<&crate::album_candidates::Report>,
    selected: Option<&crate::catalog::ArtistAlbumCandidate>,
) -> Result<()> {
    use crate::album_matching::MatchOutcome as M;
    if let Some(s) = report.and_then(|r| r.musicbrainz_discovery.as_ref()) {
        db.execute("INSERT INTO musicbrainz_discovery(album_id,snapshot_json,observed_at) VALUES(?1,?2,?3) ON CONFLICT(album_id) DO UPDATE SET snapshot_json=excluded.snapshot_json,observed_at=excluded.observed_at",params![album.as_ref(),encode(s)?,s.observed_at])?;
        if let Some(artist) = &s.artist {
            for p in &s.pages {
                db.execute("INSERT INTO musicbrainz_artist_page(artist_id,page_offset,page_json,observed_at) VALUES(?1,?2,?3,?4) ON CONFLICT(artist_id,page_offset) DO UPDATE SET page_json=excluded.page_json,observed_at=excluded.observed_at",params![artist.external_id,p.offset,encode(&p.page)?,s.observed_at])?;
            }
        }
    }
    let count = report.map_or(0, |r| r.candidate_count);
    let reason = match outcome {
        M::Pending => Reason::Matching,
        M::Deferred(_) => Reason::ProviderDeferred,
        M::Error(_) | M::ConfigurationError(_) => Reason::ProviderError,
        M::ArtistAmbiguous(_) => Reason::ArtistUnresolved,
        M::Skipped => Reason::EvidenceChanged,
        M::NoConfidentMatch
            if report
                .and_then(|r| r.musicbrainz_discovery.as_ref())
                .is_some() =>
        {
            let report = report.unwrap();
            let s = report.musicbrainz_discovery.as_ref().unwrap();
            let best = report
                .candidates
                .first()
                .and_then(|c| s.evaluations.iter().find(|e| e.group == c.identity));
            if s.budget_exhausted {
                Reason::DiscoveryBudget
            } else if best.is_some_and(|e| {
                e.reasons
                    .iter()
                    .any(|r| r == "existing_trusted_group_conflict")
            }) {
                Reason::ExistingGroupConflict
            } else if best.is_some_and(|e| e.reasons.iter().any(|r| r == "version_conflict")) {
                Reason::VersionUncertain
            } else if count == 0 {
                Reason::NoCandidates
            } else {
                Reason::InsufficientEvidence
            }
        }
        M::NoConfidentMatch
            if count > 0
                && report.is_some_and(|r| r.candidates.iter().all(|c| !c.artist_accepted)) =>
        {
            Reason::IdentityConflict
        }
        M::NoConfidentMatch
            if count > 0
                && report.is_some_and(|r| {
                    r.candidates.iter().any(|c| {
                        c.title.comparison
                            == crate::album_candidates::TitleComparison::VersionConflict
                    })
                }) =>
        {
            Reason::VersionUncertain
        }
        M::NoConfidentMatch if count > 0 => Reason::AlbumTitleConflict,
        M::AlbumAmbiguous(_) => Reason::CompetingGroups,
        M::NoConfidentMatch if report.is_none() => Reason::ArtistUnresolved,
        M::NoConfidentMatch => Reason::NoCandidates,
        _ => Reason::CandidatesFound,
    };
    if let Some(report) = report {
        for c in report.candidates.iter().take(40) {
            if c.identity.provider != "musicbrainz" || c.identity.kind != "release_group" {
                continue;
            }
            let metadata = c.observation.clone().unwrap_or_else(|| AlbumCandidate {
                identity: c.identity.clone(),
                title: c.title.provider_raw.clone(),
                artist: c.artist.clone(),
                credits: c
                    .artist_ids
                    .iter()
                    .map(|i| crate::catalog::Credit {
                        identity: Some(i.clone()),
                        name: c.artist.clone(),
                        join_phrase: String::new(),
                    })
                    .collect(),
                date: c.provider_date.clone(),
                primary_type: c.title.provider_type.clone(),
                secondary_types: vec![],
                comment: String::new(),
                score: None,
            });
            observe(
                db,
                album,
                &metadata,
                "automatic",
                c.provider_rank.unwrap_or(10) as i64,
                reason,
            )?;
        }
    }
    if let Some(c) = selected.filter(|c| {
        report.is_none_or(|r| !r.candidates.iter().any(|row| row.identity == c.identity))
    }) {
        let metadata = AlbumCandidate {
            identity: c.identity.clone(),
            title: c.title.clone(),
            artist: c.artist.clone(),
            credits: c
                .artist_ids
                .iter()
                .map(|i| crate::catalog::Credit {
                    identity: Some(i.clone()),
                    name: c.artist.clone(),
                    join_phrase: String::new(),
                })
                .collect(),
            date: c.date.clone(),
            primary_type: c.primary_type.clone(),
            secondary_types: vec![],
            comment: c.comment.clone(),
            score: None,
        };
        observe(db, album, &metadata, "automatic", 0, reason)?;
    }
    if let Some(s) = report.and_then(|r| r.musicbrainz_discovery.as_ref()) {
        for p in &s.programs {
            retain_programs(db, album, p)?;
        }
        // Re-evaluation cannot renew provider freshness without provider work.
        db.execute("UPDATE musicbrainz_candidate SET observed_at=min(observed_at,?2),program_observed_at=min(program_observed_at,?2) WHERE album_id=?1",params![album.as_ref(),s.observed_at])?;
        for (rank, row) in report.unwrap().candidates.iter().enumerate() {
            let evaluation = s.evaluations.iter().find(|e| e.group == row.identity);
            let discovery=s.title_page.as_ref().and_then(|p|p.items.iter().position(|c|c.identity==row.identity).map(|n|serde_json::json!({"origin":"title_search","rank":n+1})))
                .or_else(||s.pages.iter().find_map(|p|p.page.items.iter().position(|c|c.identity==row.identity).map(|n|serde_json::json!({"origin":"artist_catalog","offset":p.offset,"page":p.offset/10+1,"rank":n+1}))));
            db.execute("UPDATE musicbrainz_candidate SET reason=?3 WHERE album_id=?1 AND group_id=?2",params![album.as_ref(),row.identity.external_id,encode(&serde_json::json!({"application_rank":rank,"evaluation":evaluation,"discovery":discovery,"reasons":row.reasons}))?])?;
        }
    }
    state(db, album, reason, &serde_json::json!({}))
}
