//! One structural program truth. Recording certainty and explicit confirmation are separate policies.
use crate::{album_program::*, edition::*};

#[derive(Clone, Debug, serde::Serialize)]
pub struct Correspondence {
    pub program: PositionedProgram,
    pub provider_only: Vec<TrackEvidence>,
}
impl PositionDecision {
    pub fn is_correspondence(self) -> bool {
        matches!(
            self,
            Self::ExactAnchor
                | Self::CorroboratedPosition
                | Self::AlignedAfterInsertion
                | Self::ReorderedTrack
        )
    }
}
/// Both automatic enrichment and review call this function. It never attaches identity.
pub fn correspond(local: &[LocalTrackEvidence], provider: &Program, title: &str) -> Correspondence {
    let program = inspect_album_program(local, provider, title);
    let provider_only = provider
        .tracks
        .iter()
        .filter(|t| {
            !program
                .tracks
                .iter()
                .any(|r| r.decision.is_correspondence() && r.provider.as_ref() == Some(*t))
        })
        .cloned()
        .collect();
    Correspondence {
        program,
        provider_only,
    }
}
