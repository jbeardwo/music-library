//! MusicBrainz Recording certainty. Shared correspondence is input, never a second mapper.
use crate::{
    album_program::*,
    artist_credit,
    domain::*,
    edition::*,
    program_correspondence::{Correspondence, correspond},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingCertainty {
    Supported,
    AlreadyKnown,
    NoCorrespondence,
    ArtistUntrusted,
    VersionUncertain,
    ConflictingIdentity,
    RecordingAmbiguous,
    IncompleteEvidence,
    DurationConflict,
    InsufficientContext,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct RecordingDecision {
    pub track: TrackId,
    pub certainty: RecordingCertainty,
    pub recording: Option<ExternalIdentity>,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Assessment {
    pub programs: Vec<Correspondence>,
    pub recordings: Vec<RecordingDecision>,
    /// Missing information is neither negative evidence nor agreement.
    pub incomplete_samples: usize,
}
fn mb_ids(t: &TrackEvidence) -> Vec<ExternalIdentity> {
    t.recording
        .identities
        .iter()
        .filter(|i| {
            i.provider == "musicbrainz" && i.kind == "recording" && !i.external_id.is_empty()
        })
        .cloned()
        .collect()
}
/// Explicit confirmation chooses a particular sampled program; no automatic certainty is implied.
pub fn confirmation(c: &Correspondence) -> Vec<(TrackId, ExternalIdentity)> {
    c.program
        .tracks
        .iter()
        .filter(|r| r.decision.is_correspondence())
        .filter_map(|r| {
            let ids = mb_ids(r.provider.as_ref()?);
            (ids.len() == 1).then(|| (TrackId(r.track_id.clone()), ids[0].clone()))
        })
        .collect()
}
/// Complete alternatives must agree wherever they offer supported correspondence.
/// Incomplete alternatives retain uncertainty; observed contradictory IDs still veto.
pub fn assess(local: &[LocalTrackEvidence], programs: &Programs, title: &str) -> Assessment {
    let rows: Vec<_> = programs
        .programs
        .iter()
        .map(|p| correspond(local, p, title))
        .collect();
    let incomplete_samples = programs.programs.iter().filter(|p| !p.complete).count();
    let recordings = local
        .iter()
        .enumerate()
        .map(|(index, l)| {
            let mut ids = Vec::new();
            let mut complete_support = 0;
            let mut artist_trusted = true;
            let mut version = false;
            let mut duration = false;
            let mut conflict = false;
            let mut missing_id = false;
            let mut context = true;
            for (raw, c) in programs.programs.iter().zip(&rows) {
                let r = &c.program.tracks[index];
                conflict |= r.decision == PositionDecision::TrustedIdentityConflict;
                version |= r.decision == PositionDecision::VersionConflict && raw.complete;
                if !r.decision.is_correspondence() {
                    continue;
                }
                let Some(p) = &r.provider else { continue };
                let observed = mb_ids(p);
                // Even an incomplete sample cannot be used to conceal an observed contradiction.
                for id in &observed {
                    if !ids.contains(id) {
                        ids.push(id.clone());
                    }
                }
                if !raw.complete {
                    continue;
                }
                complete_support += 1;
                if r.decision == PositionDecision::CorroboratedPosition {
                    context &= c.program.anchors >= 3
                        && c.program.anchors * 3 >= local.len().max(raw.tracks.len()) * 2;
                }
                missing_id |= observed.len() != 1;
                let trusted_program = c
                    .program
                    .tracks
                    .iter()
                    .filter(|anchor| {
                        anchor.decision.is_correspondence()
                            && anchor.provider.as_ref().is_some_and(|p| {
                                mb_ids(p)
                                    .iter()
                                    .any(|id| anchor.local.recording.identities.contains(id))
                            })
                    })
                    .count()
                    >= 3;
                let primary = l.evidence.artists.first().zip(p.artists.first());
                artist_trusted &= primary.is_some_and(|(a, b)| {
                    artist_credit::trusted_overlap(a, b)
                        || trusted_program && artist_credit::same_artist(a, b)
                });
                let a = l.evidence.title.as_deref().unwrap_or("");
                let b = p.title.as_deref().unwrap_or("");
                // Structural annotation equivalence does not establish original/remaster musical identity.
                version |= crate::album_matching::qualifiers(a)
                    != crate::album_matching::qualifiers(b)
                    && crate::matching::contextual_title_class(a, b)
                        != Some(crate::matching::ContextualTitleClass::AlbumVersion);
                let explained = artist_credit::musical_title(a, &p.artists);
                if a.contains(" (feat.") || a.contains(" (featuring ") {
                    version |=
                        explained == a && artist_credit::musical_title(a, &l.evidence.artists) == a;
                }
                duration |= l
                    .evidence
                    .duration_ms
                    .zip(p.duration_ms)
                    .is_some_and(|(a, b)| a.abs_diff(b) > 3_000);
            }
            let existing = mb_ids(&l.evidence);
            conflict |= ids
                .iter()
                .any(|id| existing.iter().any(|known| known != id));
            let certainty = if conflict {
                RecordingCertainty::ConflictingIdentity
            } else if ids.len() > 1 || missing_id {
                RecordingCertainty::RecordingAmbiguous
            } else if complete_support == 0 {
                if incomplete_samples > 0 {
                    RecordingCertainty::IncompleteEvidence
                } else {
                    RecordingCertainty::NoCorrespondence
                }
            } else if !context {
                RecordingCertainty::InsufficientContext
            } else if !artist_trusted {
                RecordingCertainty::ArtistUntrusted
            } else if version {
                RecordingCertainty::VersionUncertain
            } else if duration {
                RecordingCertainty::DurationConflict
            } else if ids.len() != 1 {
                RecordingCertainty::RecordingAmbiguous
            } else if existing.contains(&ids[0]) {
                RecordingCertainty::AlreadyKnown
            } else {
                RecordingCertainty::Supported
            };
            let recording = matches!(
                certainty,
                RecordingCertainty::Supported | RecordingCertainty::AlreadyKnown
            )
            .then(|| ids[0].clone());
            RecordingDecision {
                track: l.track_id.clone(),
                certainty,
                recording,
            }
        })
        .collect();
    Assessment {
        programs: rows,
        recordings,
        incomplete_samples,
    }
}
/// Convert only the separate automatic Recording policy to the existing persistence envelope.
pub fn automatic(
    local: &[LocalTrackEvidence],
    programs: &Programs,
    title: &str,
) -> Vec<TrackOutcome> {
    let assessment = assess(local, programs, title);
    assessment
        .recordings
        .iter()
        .enumerate()
        .map(|(i, decision)| {
            let Some(identity) = &decision.recording else {
                return match decision.certainty {
                    RecordingCertainty::ConflictingIdentity => TrackOutcome::ConflictingIdentity,
                    RecordingCertainty::RecordingAmbiguous => TrackOutcome::Ambiguous,
                    _ => TrackOutcome::NoConfidentMatch,
                };
            };
            let matched = Match {
                duration: None,
                title: local[i].evidence.title.clone().unwrap_or_default(),
                recording: RecordingEvidence {
                    identities: vec![identity.clone()],
                    isrcs: vec![],
                },
                recording_status: RecordingStatus::Identified,
                occurrences: vec![],
                explanation: format!(
                    "Shared program correspondence; complete sampled Recording agreement; {} incomplete samples retained as uncertainty",
                    assessment.incomplete_samples
                ),
            };
            if decision.certainty == RecordingCertainty::AlreadyKnown {
                TrackOutcome::AlreadyMatched(matched)
            } else {
                TrackOutcome::Matched(matched)
            }
        })
        .collect()
}

/// Choose which sampled Release explicit confirmation presents, not which
/// Recording is automatically certain. Incomplete observations remain retained.
pub fn review_program(
    local: &[LocalTrackEvidence],
    programs: &Programs,
    title: &str,
) -> Option<(usize, Correspondence)> {
    programs
        .programs
        .iter()
        .enumerate()
        .map(|(i, p)| (i, correspond(local, p, title)))
        .max_by_key(|(i, c)| {
            (
                c.program.complete,
                c.program
                    .tracks
                    .iter()
                    .filter(|r| r.decision.is_correspondence())
                    .count(),
                std::cmp::Reverse(*i),
            )
        })
}
