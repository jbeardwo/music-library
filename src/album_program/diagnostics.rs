//! The positional decision path used by established Spotify occurrence matching.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PositionDecision {
    ExactAnchor,
    CorroboratedPosition,
    PositionMismatch,
    ShiftedProgram,
    VersionConflict,
    TrustedIdentityConflict,
    InsufficientAnchors,
}
impl PositionDecision {
    pub fn label(self) -> &'static str {
        match self {
            Self::ExactAnchor => "title and position corroborate",
            Self::CorroboratedPosition => "position corroborated by independent program anchors",
            Self::PositionMismatch => "missing or duplicate position",
            Self::ShiftedProgram => "program is shifted across positions",
            Self::VersionConflict => "meaningful version qualifier differs",
            Self::TrustedIdentityConflict => "trusted identity conflicts",
            Self::InsufficientAnchors => "insufficient independent program anchors",
        }
    }
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct PositionedTrack {
    pub track_id: String,
    pub local: TrackEvidence,
    pub provider: Option<TrackEvidence>,
    pub normalized_disc: u32,
    pub decision: PositionDecision,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct PositionedProgram {
    pub complete: bool,
    pub provider_count: usize,
    pub anchors: usize,
    pub duplicate_positions: usize,
    pub tracks: Vec<PositionedTrack>,
}
impl PositionedProgram {
    pub fn compatible(&self) -> bool {
        self.complete
            && self.duplicate_positions == 0
            && self.tracks.iter().all(|t| {
                matches!(
                    t.decision,
                    PositionDecision::ExactAnchor | PositionDecision::CorroboratedPosition
                )
            })
    }
}
pub fn inspect_positioned_program(
    local: &[LocalTrackEvidence],
    program: &Program,
) -> PositionedProgram {
    let mut positions = BTreeMap::new();
    let mut duplicate_positions = 0;
    for t in &program.tracks {
        if let Some(p) = position(t)
            && positions.insert(p, t).is_some()
        {
            duplicate_positions += 1;
        }
    }
    let mut anchors = BTreeSet::new();
    let mut shifted = false;
    for t in local {
        let Some(pos) = position(&t.evidence) else {
            continue;
        };
        if let Some(c) = positions.get(&pos) {
            let relation = title_relation(&t.evidence, c);
            if relation != TitleRelation::Agrees
                && program.tracks.iter().any(|other| {
                    position(other) != Some(pos)
                        && title_relation(&t.evidence, other) == TitleRelation::Agrees
                })
            {
                shifted = true;
            }
            if relation == TitleRelation::Agrees && !occurrence_identity_conflict(&t.evidence, c) {
                anchors.insert(pos);
            }
        }
    }
    let tracks = local
        .iter()
        .map(|t| {
            let c = position(&t.evidence)
                .and_then(|p| positions.get(&p))
                .copied();
            let decision = match c {
                None => PositionDecision::PositionMismatch,
                Some(_) if duplicate_positions > 0 => PositionDecision::PositionMismatch,
                Some(c) if occurrence_identity_conflict(&t.evidence, c) => {
                    PositionDecision::TrustedIdentityConflict
                }
                Some(_) if shifted => PositionDecision::ShiftedProgram,
                Some(c) => match title_relation(&t.evidence, c) {
                    TitleRelation::Contradictory => PositionDecision::VersionConflict,
                    TitleRelation::Agrees => PositionDecision::ExactAnchor,
                    TitleRelation::Uncorroborated if anchors.len() >= 3 => {
                        PositionDecision::CorroboratedPosition
                    }
                    TitleRelation::Uncorroborated => PositionDecision::InsufficientAnchors,
                },
            };
            PositionedTrack {
                track_id: t.track_id.as_ref().into(),
                local: t.evidence.clone(),
                provider: c.cloned(),
                normalized_disc: t.evidence.disc.filter(|d| *d > 0).unwrap_or(1),
                decision,
            }
        })
        .collect();
    PositionedProgram {
        complete: program.complete,
        provider_count: program.tracks.len(),
        anchors: anchors.len(),
        duplicate_positions,
        tracks,
    }
}
