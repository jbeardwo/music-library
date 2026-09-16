//! Album-scoped program evidence. No provider hierarchy or exact edition required.
use crate::{
    catalog::CatalogError,
    domain::*,
    edition::*,
    storage::{Result, Store},
};
use rusqlite::{Connection, params};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    /// Diagnostic provenance only; never accepted as the local edition identity.
    pub identity: Option<ExternalIdentity>,
    /// Musical order flattened by the adapter; positions remain diagnostic evidence.
    pub tracks: Vec<TrackEvidence>,
    pub complete: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Programs {
    pub album: ExternalIdentity,
    pub programs: Vec<Program>,
    /// Sampling is bounded; this is not a claim to have examined every edition.
    pub note: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    pub album_id: AlbumId,
    pub album: ExternalIdentity,
    pub tracks: Vec<LocalTrackEvidence>,
    pub artist_conflicts: Vec<TrackId>,
}
#[derive(Clone, Debug)]
pub struct Reply {
    pub input: Input,
    pub result: std::result::Result<Programs, CatalogError>,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Match {
    pub title: String,
    pub recording: RecordingEvidence,
    pub recording_status: RecordingStatus,
    pub occurrences: Vec<ExternalIdentity>,
    pub explanation: String,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RecordingStatus {
    Identified,
    Ambiguous,
    NotProvided,
    DurationMismatch,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrackOutcome {
    Matched(Match),
    AlreadyMatched(Match),
    ManuallyMatched(Match),
    Ambiguous,
    NoConfidentMatch,
    ConflictingIdentity,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Pending,
    Complete(Vec<(LocalTrackEvidence, TrackOutcome)>),
    Deferred(CatalogError),
    Error(String),
}

pub const DURATION_TOLERANCE_MS: u64 = 3_000;
pub fn comparison_title(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '&' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .map(|s| if s == "&" { "and" } else { s })
        .collect::<Vec<_>>()
        .join(" ")
}
fn qualifiers(s: &str) -> Vec<&str> {
    s.split_whitespace()
        .filter(|s| {
            matches!(
                *s,
                "live"
                    | "remix"
                    | "demo"
                    | "acoustic"
                    | "edit"
                    | "remaster"
                    | "remastered"
                    | "mix"
                    | "version"
            )
        })
        .collect()
}
fn close(a: &str, b: &str) -> bool {
    let a: Vec<_> = a.chars().collect();
    let b: Vec<_> = b.chars().collect();
    if a.len() < 5 || b.len() < 5 || a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let (mut i, mut j, mut edits) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            i += 1;
            j += 1;
        } else {
            edits += 1;
            if edits > 1 {
                return false;
            }
            if a.len() >= b.len() {
                i += 1;
            }
            if b.len() >= a.len() {
                j += 1;
            }
        }
    }
    edits + (a.len() - i) + (b.len() - j) <= 1
}
fn conflict(a: &[ExternalIdentity], b: &[ExternalIdentity]) -> bool {
    a.iter().any(|x| {
        b.iter()
            .any(|y| x.provider == y.provider && x.kind == y.kind)
            && !b.contains(x)
    })
}
fn shared(a: &[ExternalIdentity], b: &[ExternalIdentity]) -> bool {
    a.iter().any(|i| b.contains(i))
}
fn artist_name(a: &[ArtistEvidence]) -> String {
    crate::matching::normalize(
        &a.iter()
            .map(|c| format!("{}{}", c.name, c.join_phrase))
            .collect::<String>(),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateCheck {
    pub exact_title: bool,
    pub position: bool,
    pub trusted_identity: bool,
    pub identity_conflict: bool,
    pub duration_mismatch: bool,
    pub considered: bool,
    pub reason: &'static str,
}

/// Shared by the comparator and the opt-in probe: rejection reasons cannot drift.
pub fn inspect_candidate(
    local: &LocalTrackEvidence,
    c: &TrackEvidence,
    index: usize,
) -> CandidateCheck {
    let l = &local.evidence;
    let name = comparison_title(l.title.as_deref().unwrap_or(""));
    let candidate = comparison_title(c.title.as_deref().unwrap_or(""));
    // Preserve the previous punctuation-only comparison as well as conjunction
    // equivalence (e.g. the established R & R / R+R comparison).
    let punctuation = |s: &str| {
        s.to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { ' ' })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    let exact = !name.is_empty()
        && (name == candidate
            || punctuation(l.title.as_deref().unwrap_or(""))
                == punctuation(c.title.as_deref().unwrap_or("")));
    let trusted = shared(&l.recording.identities, &c.recording.identities);
    let position = l.number.is_some()
        && (l.number == c.number && l.disc.unwrap_or(1) == c.disc.unwrap_or(1)
            || l.disc.unwrap_or(1) == 1 && l.number == Some(index as u32 + 1));
    let mut check = CandidateCheck {
        exact_title: exact,
        position,
        trusted_identity: trusted,
        identity_conflict: conflict(&l.recording.identities, &c.recording.identities),
        duration_mismatch: l
            .duration_ms
            .zip(c.duration_ms)
            .is_some_and(|(a, b)| a.abs_diff(b) > DURATION_TOLERANCE_MS),
        considered: false,
        reason: "title/semantic qualifier disagreement",
    };
    if !trusted
        && !exact
        && !(position && qualifiers(&name) == qualifiers(&candidate) && close(&name, &candidate))
    {
        return check;
    }
    let a: Vec<_> = l
        .artists
        .iter()
        .flat_map(|a| a.identities.clone())
        .collect();
    let b: Vec<_> = c
        .artists
        .iter()
        .flat_map(|a| a.identities.clone())
        .collect();
    if !trusted
        && (conflict(&a, &b)
            || !shared(&a, &b)
                && !a.iter().any(|x| b.iter().any(|y| x.provider == y.provider))
                && !l.artists.is_empty()
                && !c.artists.is_empty()
                && artist_name(&l.artists) != artist_name(&c.artists))
    {
        check.reason = "Artist disagreement";
        return check;
    }
    if !trusted && check.duration_mismatch && !check.identity_conflict && !exact {
        check.reason = "duration outside 3000 ms tolerance for near-title association";
        return check;
    }
    check.considered = true;
    check.reason = if check.identity_conflict {
        "conflicting existing Recording identity"
    } else if trusted {
        "trusted Recording identity (duration is diagnostic)"
    } else if check.duration_mismatch {
        "exact normalized title; duration diagnostic if unique in program"
    } else if exact {
        "exact normalized title"
    } else {
        "positional one-edit title tolerance"
    };
    check
}

fn mappings(local: &LocalTrackEvidence, program: &Program) -> Vec<(usize, CandidateCheck)> {
    let mut eligible: Vec<_> = program
        .tracks
        .iter()
        .enumerate()
        .map(|(i, c)| (i, inspect_candidate(local, c, i)))
        .filter(|(_, c)| c.considered)
        .collect();
    if eligible.iter().any(|(_, c)| c.trusted_identity) {
        eligible.retain(|(_, c)| c.trusted_identity);
    } else if eligible.iter().any(|(_, c)| c.exact_title) {
        eligible.retain(|(_, c)| c.exact_title);
    }
    if eligible.iter().any(|(_, c)| c.position) {
        eligible.retain(|(_, c)| c.position);
    }
    eligible
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProgramFit {
    pub identity_conflicts: usize,
    pub exact_positions: usize,
    pub tolerant_positions: usize,
    pub duration_mismatches: usize,
    pub unmatched: usize,
    pub agreements: Vec<Agreement>,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Agreement {
    Missing,
    ExactTitle,
    PositionalTolerance,
    ExactPosition,
    TrustedRecording,
}
pub fn program_fit(local: &[LocalTrackEvidence], program: &Program) -> ProgramFit {
    let mut fit = ProgramFit::default();
    let mut exact_positions = std::collections::BTreeSet::new();
    for track in local {
        let choices = mappings(track, program);
        if choices.len() != 1 {
            fit.unmatched += 1;
            fit.agreements.push(Agreement::Missing);
            continue;
        }
        let c = &choices[0].1;
        fit.identity_conflicts += usize::from(c.identity_conflict);
        fit.duration_mismatches += usize::from(c.duration_mismatch && !c.trusted_identity);
        if c.exact_title && c.position {
            exact_positions.insert((track.evidence.disc.unwrap_or(1), track.evidence.number));
        }
        fit.tolerant_positions += usize::from(!c.exact_title && c.position);
        fit.agreements.push(if c.identity_conflict {
            Agreement::Missing
        } else if c.trusted_identity {
            Agreement::TrustedRecording
        } else if c.exact_title && c.position {
            Agreement::ExactPosition
        } else if c.position {
            Agreement::PositionalTolerance
        } else {
            Agreement::ExactTitle
        });
    }
    fit.exact_positions = exact_positions.len();
    fit
}

/// Retain ties. A better template must improve exact positional evidence, have
/// at least three corroborating present Tracks, and never worsen another Track.
/// Conflicting programs are excluded only if a nonconflicting program exists.
pub fn selected_programs(local: &[LocalTrackEvidence], programs: &Programs) -> Vec<usize> {
    let fits: Vec<_> = programs
        .programs
        .iter()
        .map(|p| program_fit(local, p))
        .collect();
    let clean = programs.programs.iter().enumerate().any(|(i, p)| {
        p.complete
            && fits[i].identity_conflicts == 0
            && fits[i].agreements.iter().any(|a| *a != Agreement::Missing)
    });
    let viable: Vec<_> = programs
        .programs
        .iter()
        .enumerate()
        .filter(|(i, p)| p.complete && (!clean || fits[*i].identity_conflicts == 0))
        .map(|(i, _)| i)
        .collect();
    viable
        .iter()
        .copied()
        .filter(|&i| {
            !viable.iter().any(|&j| {
                let (a, b) = (&fits[j], &fits[i]);
                j != i
                    && a.exact_positions >= 3
                    && a.exact_positions > b.exact_positions
                    && a.duration_mismatches <= b.duration_mismatches
                    && a.agreements.iter().zip(&b.agreements).all(|(x, y)| x >= y)
            })
        })
        .collect()
}

pub fn compare_album(local: &[LocalTrackEvidence], programs: &Programs) -> Vec<TrackOutcome> {
    if programs.programs.iter().any(|p| !p.complete) {
        return local
            .iter()
            .map(|_| TrackOutcome::NoConfidentMatch)
            .collect();
    }
    let selected = selected_programs(local, programs);
    let retained = Programs {
        album: programs.album.clone(),
        programs: selected
            .iter()
            .map(|&i| programs.programs[i].clone())
            .collect(),
        note: format!("Retained program indices {selected:?}. {}", programs.note),
    };
    local.iter().map(|t| compare(t, &retained)).collect()
}

/// Track association and Recording certainty are separate. The Album comparator
/// selects mapping templates first; this function compares the retained programs.
pub fn compare(local: &LocalTrackEvidence, programs: &Programs) -> TrackOutcome {
    let l = &local.evidence;
    let name = comparison_title(l.title.as_deref().unwrap_or(""));
    if name.is_empty() && l.recording.identities.is_empty() {
        return TrackOutcome::NoConfidentMatch;
    }
    if programs.programs.iter().any(|p| !p.complete) {
        return TrackOutcome::NoConfidentMatch;
    }
    let mut mappings = vec![];
    let mut duration_blocks_recording = false;
    for program in &programs.programs {
        let unique_exact = program
            .tracks
            .iter()
            .enumerate()
            .filter(|(i, candidate)| inspect_candidate(local, candidate, *i).exact_title)
            .count()
            == 1;
        let eligible = self::mappings(local, program);
        if eligible.len() > 1 {
            return TrackOutcome::Ambiguous;
        }
        for (i, c) in eligible {
            duration_blocks_recording |=
                c.duration_mismatch && !c.trusted_identity && !(unique_exact && c.exact_title);
            mappings.push(&program.tracks[i]);
        }
    }
    if mappings.is_empty() {
        return TrackOutcome::NoConfidentMatch;
    }
    if mappings
        .iter()
        .any(|c| conflict(&l.recording.identities, &c.recording.identities))
    {
        return TrackOutcome::ConflictingIdentity;
    }
    // Display choice is deterministic and prefers the exact local spelling.
    // It does not select a Recording identity or an edition.
    mappings.sort_by_key(|c| {
        (
            comparison_title(c.title.as_deref().unwrap_or("")) != name,
            c.title.as_deref().unwrap_or(""),
        )
    });
    let first = mappings[0];
    let recording_disagreement = mappings.iter().any(|a| {
        mappings
            .iter()
            .any(|b| conflict(&a.recording.identities, &b.recording.identities))
    });
    let mut identities: Vec<_> = first
        .recording
        .identities
        .iter()
        .filter(|i| mappings.iter().all(|c| c.recording.identities.contains(i)))
        .cloned()
        .collect();
    let occurrences: Vec<_> = first
        .identities
        .iter()
        .filter(|i| mappings.iter().all(|c| c.identities.contains(i)))
        .cloned()
        .collect();
    if identities.is_empty()
        && occurrences.is_empty()
        && !mappings
            .iter()
            .any(|c| comparison_title(c.title.as_deref().unwrap_or("")) == name)
        && mappings.iter().any(|c| {
            comparison_title(c.title.as_deref().unwrap_or(""))
                != comparison_title(first.title.as_deref().unwrap_or(""))
        })
    {
        return TrackOutcome::Ambiguous;
    }
    let recording_status = if mappings.iter().all(|c| c.recording.identities.is_empty()) {
        // A provider without Recording identity has no Recording certainty to
        // withhold. The Track association above has already passed its checks.
        RecordingStatus::NotProvided
    } else if recording_disagreement {
        RecordingStatus::Ambiguous
    } else if duration_blocks_recording {
        RecordingStatus::DurationMismatch
    } else if !identities.is_empty() {
        RecordingStatus::Identified
    } else {
        RecordingStatus::Ambiguous
    };
    if recording_status != RecordingStatus::Identified {
        identities.clear();
    }
    let mut isrcs = vec![];
    for c in &mappings {
        for i in &c.recording.isrcs {
            if !isrcs.contains(i) {
                isrcs.push(i.clone());
            }
        }
    }
    let result = Match {
        title: first.title.clone().unwrap_or_default(),
        recording: RecordingEvidence {
            identities,
            isrcs: if recording_status == RecordingStatus::Identified {
                isrcs
            } else {
                vec![]
            },
        },
        recording_status: recording_status.clone(),
        occurrences,
        explanation: format!(
            "{} plausible Track mappings; Recording status {:?}; title/position and Artist checked; ISRC corroboration: {}; duration mismatch (diagnostic for unique exact title or trusted identity): {}. {}",
            mappings.len(),
            recording_status,
            mappings.iter().any(|c| c
                .recording
                .isrcs
                .iter()
                .any(|i| l.recording.isrcs.contains(i))),
            mappings.iter().any(|c| l
                .duration_ms
                .zip(c.duration_ms)
                .is_some_and(|(a, b)| a.abs_diff(b) > DURATION_TOLERANCE_MS)),
            programs.note
        ),
    };
    if !result.recording.identities.is_empty()
        && result
            .recording
            .identities
            .iter()
            .all(|i| l.recording.identities.contains(i))
    {
        TrackOutcome::AlreadyMatched(result)
    } else {
        TrackOutcome::Matched(result)
    }
}

fn local_tracks(
    db: &Connection,
    album: &AlbumId,
) -> Result<(Vec<LocalTrackEvidence>, Vec<TrackId>)> {
    let mut artist_conflicts = vec![];
    let mut tracks=db.prepare("SELECT t.id,t.recording_id,t.disc_number,t.track_number,e.title,e.duration_ms,e.artist_names FROM release r CROSS JOIN track t ON t.release_id=r.id JOIN effective_track_metadata e ON e.track_id=t.id WHERE r.album_id=?1 AND EXISTS(SELECT 1 FROM track_source ts JOIN local_file_observation l ON l.source_id=ts.source_id WHERE ts.track_id=t.id) ORDER BY r.id,t.disc_number,t.track_number,t.id")?
        .query_map([album.as_ref()],|r|Ok((LocalTrackEvidence{track_id:TrackId(r.get(0)?),recording_id:RecordingId(r.get(1)?),evidence:TrackEvidence{disc:r.get(2)?,number:r.get(3)?,title:r.get(4)?,duration_ms:r.get::<_,Option<i64>>(5)?.and_then(|v|u64::try_from(v).ok()),..Default::default()}},r.get::<_,String>(6)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let names: Vec<_> = tracks.iter().map(|t| t.1.clone()).collect();
    let mut tracks: Vec<_> = tracks.drain(..).map(|t| t.0).collect();
    crate::edition_storage::load_track_evidence(db, &mut tracks)?;
    let track_ids = serde_json::to_string(
        &tracks
            .iter()
            .map(|t| t.track_id.as_ref())
            .collect::<Vec<_>>(),
    )
    .map_err(|e| crate::storage::Error::Invalid(e.to_string()))?;
    let mut source_artists =
        std::collections::HashMap::<String, std::collections::BTreeMap<String, Vec<String>>>::new();
    for row in db.prepare("SELECT ts.track_id,f.source_id,f.name FROM track_source ts JOIN file_artist_observation f ON f.source_id=ts.source_id AND f.scope='track' JOIN local_file_observation l ON l.source_id=ts.source_id AND l.available=1 WHERE ts.track_id IN (SELECT value FROM json_each(?1)) ORDER BY ts.track_id,f.source_id,f.position")?.query_map([track_ids],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))? {
        let (track,source,name)=row?;source_artists.entry(track).or_default().entry(source).or_default().push(name);
    }
    // File-only Track credits often contain a former/credited Artist name while
    // the Album credit already establishes its provider identity. Reuse that
    // evidence only for exact conservative display-credit agreement, never by
    // fuzzy Artist names or merely by sharing an Album.
    let mut album_artists: Vec<ArtistEvidence> = vec![];
    let mut last = None;
    for row in db.prepare("SELECT c.position,COALESCE(c.credited_name,a.name),COALESCE(c.join_phrase,''),i.provider,i.kind,i.external_id FROM album_artist_credit c JOIN artist a ON a.id=c.artist_id LEFT JOIN artist_external_identity i ON i.artist_id=a.id WHERE c.album_id=?1 ORDER BY c.position,i.provider,i.kind,i.external_id")?.query_map([album.as_ref()],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,Option<String>>(4)?,r.get::<_,Option<String>>(5)?)))? {
        let (position,name,join_phrase,provider,kind,external_id)=row?;
        if last!=Some(position){album_artists.push(ArtistEvidence{name,join_phrase,identities:vec![]});last=Some(position);}
        if let (Some(provider),Some(kind),Some(external_id))=(provider,kind,external_id){album_artists.last_mut().unwrap().identities.push(ExternalIdentity{provider,kind,external_id});}
    }
    for (track, name) in tracks.iter_mut().zip(names) {
        if track.evidence.artists.is_empty() {
            if name.is_empty()
                && let Some(sources) = source_artists.get(track.track_id.as_ref())
                && let Some(first) = sources.values().next()
                && sources.values().any(|names| names != first)
            {
                artist_conflicts.push(track.track_id.clone());
            }
            let local_names = if !name.trim().is_empty() {
                vec![name]
            } else {
                source_artists
                    .get(track.track_id.as_ref())
                    .and_then(|sources| {
                        sources
                            .values()
                            .next()
                            .filter(|first| sources.values().all(|names| names == *first))
                    })
                    .cloned()
                    .unwrap_or_default()
            };
            let names_agree = local_names
                .iter()
                .map(|n| crate::matching::normalize(n))
                .collect::<Vec<_>>()
                == album_artists
                    .iter()
                    .map(|a| crate::matching::normalize(&a.name))
                    .collect::<Vec<_>>();
            if !album_artists.is_empty()
                && (names_agree
                    || local_names.len() == 1
                        && crate::matching::normalize(&local_names[0])
                            == artist_name(&album_artists))
            {
                track.evidence.artists = album_artists.clone();
            } else {
                track.evidence.artists = local_names
                    .into_iter()
                    .map(|name| ArtistEvidence {
                        name,
                        ..Default::default()
                    })
                    .collect();
            }
        }
    }
    Ok((tracks, artist_conflicts))
}
impl Store {
    pub fn track_provider_occurrences(
        &self,
        track: &TrackId,
        provider: &str,
    ) -> Result<Vec<ExternalIdentity>> {
        use rusqlite::OptionalExtension;
        let manual: Option<String> = self.connection.query_row(
            "SELECT candidate_json FROM manual_track_association WHERE track_id=?1 AND album_provider=?2",
            params![track.as_ref(), provider], |r| r.get(0)).optional()?;
        if let Some(json) = manual {
            let candidate: crate::manual_track::Candidate = serde_json::from_str(&json)
                .map_err(|e| crate::storage::Error::Invalid(e.to_string()))?;
            return Ok(candidate.evidence.identities);
        }
        let accepted = self.connection.prepare("SELECT provider,kind,external_id FROM track_external_identity WHERE track_id=?1 AND provider=?2 ORDER BY kind,external_id")?
            .query_map(params![track.as_ref(),provider], |r| Ok(ExternalIdentity{provider:r.get(0)?,kind:r.get(1)?,external_id:r.get(2)?}))?
            .collect::<std::result::Result<Vec<_>,_>>()?;
        if !accepted.is_empty() {
            return Ok(accepted);
        }
        let automatic: Option<String> = self.connection.query_row(
            "SELECT a.match_json FROM provider_track_association a JOIN track t ON t.id=a.track_id JOIN release r ON r.id=t.release_id JOIN album_external_identity i ON i.album_id=r.album_id AND i.provider=a.album_provider AND i.kind=a.album_kind AND i.external_id=a.album_external_id WHERE a.track_id=?1 AND a.album_provider=?2",
            params![track.as_ref(), provider], |r| r.get(0)).optional()?;
        automatic
            .map(|json| {
                serde_json::from_str::<Match>(&json)
                    .map(|m| m.occurrences)
                    .map_err(|e| crate::storage::Error::Invalid(e.to_string()))
            })
            .transpose()
            .map(Option::unwrap_or_default)
    }
    pub fn local_releases_for_root(&self, root: &RootId) -> Result<Vec<ImportedRelease>> {
        let mut releases = std::collections::BTreeMap::<String, Vec<TrackId>>::new();
        for row in self.connection.prepare("SELECT DISTINCT t.release_id,t.id,t.disc_number,t.track_number FROM local_file_observation l JOIN track_source ts ON ts.source_id=l.source_id JOIN track t ON t.id=ts.track_id WHERE l.root_id=?1 ORDER BY t.release_id,t.disc_number,t.track_number,t.id")?.query_map([root.as_ref()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))? {
            let (release,track)=row?;releases.entry(release).or_default().push(TrackId(track));
        }
        Ok(releases
            .into_iter()
            .map(|(release_id, track_ids)| ImportedRelease {
                release_id: ReleaseId(release_id),
                track_ids,
            })
            .collect())
    }
    pub fn local_album_tracks(&self, album: &AlbumId) -> Result<Vec<LocalTrackEvidence>> {
        let tx = self.connection.unchecked_transaction()?;
        let (tracks, _) = local_tracks(&tx, album)?;
        tx.commit()?;
        Ok(tracks)
    }
    pub fn prepare_album_program(
        &self,
        album: &AlbumId,
        identity: &ExternalIdentity,
    ) -> Result<Option<Input>> {
        if !self
            .list_album_external_identities(album)?
            .contains(identity)
        {
            return Ok(None);
        }
        let tx = self.connection.unchecked_transaction()?;
        let (tracks, artist_conflicts) = local_tracks(&tx, album)?;
        tx.commit()?;
        Ok((!tracks.is_empty()).then(|| Input {
            album_id: album.clone(),
            album: identity.clone(),
            tracks,
            artist_conflicts,
        }))
    }
    pub fn complete_album_program(&mut self, reply: Reply) -> Result<Outcome> {
        let programs = match reply.result {
            Ok(p) => p,
            Err(e) if e.is_provider_unavailable() => return Ok(Outcome::Deferred(e)),
            Err(e) => return Ok(Outcome::Error(e.to_string())),
        };
        if programs.album != reply.input.album {
            return Ok(Outcome::Error("provider returned another Album".into()));
        }
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let album_exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM album_external_identity WHERE album_id=?1 AND provider=?2 AND kind=?3 AND external_id=?4)",params![reply.input.album_id.as_ref(),reply.input.album.provider,reply.input.album.kind,reply.input.album.external_id],|r|r.get(0))?;
        let current = local_tracks(&tx, &reply.input.album_id)?;
        if !album_exists
            || current.0 != reply.input.tracks
            || current.1 != reply.input.artist_conflicts
        {
            return Ok(Outcome::Error(
                "local evidence changed; retry enrichment".into(),
            ));
        }
        // Contradictory source Artist metadata must not vote for a template.
        let eligible: Vec<_> = reply
            .input
            .tracks
            .iter()
            .filter(|t| {
                !reply.input.artist_conflicts.contains(&t.track_id)
                    || !t.evidence.recording.identities.is_empty()
            })
            .cloned()
            .collect();
        let comparisons = compare_album(&eligible, &programs);
        let manual: std::collections::HashMap<_, _> =
            crate::manual_track::load(&tx, &reply.input.album_id)?
                .into_iter()
                .filter(|m| m.album.provider == reply.input.album.provider)
                .map(|m| (m.track_id.clone(), m))
                .collect();
        let mut comparisons: std::collections::HashMap<_, _> = eligible
            .into_iter()
            .map(|t| t.track_id)
            .zip(comparisons)
            .collect();
        let mut results: Vec<_> = reply
            .input
            .tracks
            .into_iter()
            .map(|t| {
                let outcome = if let Some(m) = manual.get(&t.track_id) {
                    TrackOutcome::ManuallyMatched(m.matched())
                } else {
                    comparisons
                        .remove(&t.track_id)
                        .unwrap_or(TrackOutcome::NoConfidentMatch)
                };
                (t, outcome)
            })
            .collect();
        // Shared application Recordings cannot receive conflicting results from
        // two different Track contexts in the same transaction.
        let mut claims = std::collections::HashMap::<RecordingId, Vec<ExternalIdentity>>::new();
        for (t, o) in &results {
            if let TrackOutcome::Matched(m) | TrackOutcome::AlreadyMatched(m) = o {
                claims
                    .entry(t.recording_id.clone())
                    .or_default()
                    .extend(m.recording.identities.clone());
            }
        }
        for (t, o) in &mut results {
            if !matches!(o, TrackOutcome::ManuallyMatched(_))
                && let Some(ids) = claims.get(&t.recording_id)
                && ids.iter().any(|a| {
                    ids.iter().any(|b| {
                        a.provider == b.provider
                            && a.kind == b.kind
                            && a.external_id != b.external_id
                    })
                })
            {
                *o = TrackOutcome::ConflictingIdentity;
            }
            if let TrackOutcome::Matched(m) | TrackOutcome::AlreadyMatched(m) = o {
                for id in &m.recording.identities {
                    tx.execute("INSERT INTO recording_external_identity(recording_id,provider,kind,external_id) VALUES (?1,?2,?3,?4) ON CONFLICT DO NOTHING",params![t.recording_id.as_ref(),id.provider,id.kind,id.external_id])?;
                }
                // Retain the Album-scoped association, without converting sampled
                // occurrences into exact edition or Recording identities.
                let json = serde_json::to_string(m)
                    .map_err(|e| crate::storage::Error::Invalid(e.to_string()))?;
                tx.execute("INSERT INTO provider_track_association(track_id,album_provider,album_kind,album_external_id,match_json) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(track_id,album_provider) DO UPDATE SET album_kind=excluded.album_kind,album_external_id=excluded.album_external_id,match_json=excluded.match_json",params![t.track_id.as_ref(),reply.input.album.provider,reply.input.album.kind,reply.input.album.external_id,json])?;
            } else if !matches!(o, TrackOutcome::ManuallyMatched(_)) {
                tx.execute("DELETE FROM provider_track_association WHERE track_id=?1 AND album_provider=?2",params![t.track_id.as_ref(),reply.input.album.provider])?;
            }
        }
        tx.commit()?;
        Ok(Outcome::Complete(results))
    }

    /// Bounded to this Album through indexed Release/Track relationships. Does
    /// not reopen sources or require the provider to be available after restart.
    pub fn provider_track_associations(
        &self,
        album: &AlbumId,
        provider: &str,
    ) -> Result<Vec<(TrackId, Match)>> {
        self.connection.prepare("SELECT a.track_id,a.match_json FROM release r CROSS JOIN track t ON t.release_id=r.id JOIN provider_track_association a ON a.track_id=t.id AND a.album_provider=?2 JOIN album_external_identity i ON i.album_id=r.album_id AND i.provider=a.album_provider AND i.kind=a.album_kind AND i.external_id=a.album_external_id WHERE r.album_id=?1 ORDER BY t.disc_number,t.track_number,t.id")?.query_map(params![album.as_ref(),provider], |r| Ok((TrackId(r.get(0)?),r.get::<_,String>(1)?)))?.map(|row| {
            let (id,json) = row?;
            Ok((id,serde_json::from_str(&json).map_err(|e| crate::storage::Error::Invalid(e.to_string()))?))
        }).collect()
    }
}
