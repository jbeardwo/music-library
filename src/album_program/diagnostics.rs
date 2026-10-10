//! The positional decision path used by established Spotify occurrence matching.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PositionDecision {
    ExactAnchor,
    LocalOnly,
    AmbiguousCorrespondence,
    AlignedAfterInsertion,
    ReorderedTrack,
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
            Self::AlignedAfterInsertion => {
                "title/identity correspondence across an insertion or missing position"
            }
            Self::ReorderedTrack => "unique title/identity correspondence at another position",
            Self::AmbiguousCorrespondence => {
                "ambiguous title/identity correspondence; manual mapping required"
            }
            Self::LocalOnly => "local only; no unique provider counterpart",
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
    pub warnings: Vec<String>,
    pub alignment: Alignment,
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
                    PositionDecision::ExactAnchor
                        | PositionDecision::CorroboratedPosition
                        | PositionDecision::LocalOnly
                        | PositionDecision::AlignedAfterInsertion
                        | PositionDecision::ReorderedTrack
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
    let alignment = align_program(local, program);
    if alignment.strong {
        anchors.clear();
        for row in &alignment.rows {
            if let (Some(i), Some(j)) = (row.local_index, row.provider_index) {
                let t = &local[i];
                let c = &program.tracks[j];
                if !occurrence_identity_conflict(&t.evidence, c) {
                    anchors.insert((1, i as u32 + 1));
                }
            }
        }
    }
    let sequence_differs = alignment.rows.iter().any(|row| {
        row.local_index
            .zip(row.provider_index)
            .is_some_and(|(i, j)| position(&local[i].evidence) != position(&program.tracks[j]))
    });
    let tracks = local
        .iter()
        .map(|t| {
            let mapped = alignment
                .rows
                .iter()
                .find(|row| {
                    row.local_index
                        .is_some_and(|i| local[i].track_id == t.track_id)
                })
                .and_then(|row| row.provider_index);
            let c = if alignment.strong || sequence_differs {
                mapped.map(|j| &program.tracks[j])
            } else {
                position(&t.evidence)
                    .and_then(|p| positions.get(&p))
                    .copied()
            };
            let decision = match c {
                None if alignment.strong
                    && program
                        .tracks
                        .iter()
                        .any(|c| title_relation(&t.evidence, c) == TitleRelation::Agrees) =>
                {
                    PositionDecision::AmbiguousCorrespondence
                }
                None if alignment.strong => PositionDecision::LocalOnly,
                None => PositionDecision::PositionMismatch,
                Some(_) if duplicate_positions > 0 => PositionDecision::PositionMismatch,
                Some(c) if occurrence_identity_conflict(&t.evidence, c) => {
                    PositionDecision::TrustedIdentityConflict
                }
                Some(c)
                    if !alignment.strong
                        && (position(&t.evidence).is_none()
                            || position(c).is_none()
                            || position(&t.evidence) != position(c)) =>
                {
                    PositionDecision::PositionMismatch
                }
                Some(_) if shifted && !alignment.strong => PositionDecision::ShiftedProgram,
                Some(c) => match title_relation(&t.evidence, c) {
                    TitleRelation::Contradictory => PositionDecision::VersionConflict,
                    TitleRelation::Agrees
                        if alignment.strong && position(&t.evidence) != position(c) =>
                    {
                        if alignment
                            .rows
                            .iter()
                            .any(|row| row.kind == AlignmentKind::Reordered)
                        {
                            PositionDecision::ReorderedTrack
                        } else {
                            PositionDecision::AlignedAfterInsertion
                        }
                    }
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
        warnings: vec![],
        alignment,
        complete: program.complete,
        provider_count: program.tracks.len(),
        anchors: anchors.len(),
        duplicate_positions,
        tracks,
    }
}

/// Systematic edition annotations require Album context AND strong program-wide
/// correspondence after removal. Raw evidence is restored in diagnostic rows.
pub fn inspect_album_program(
    local: &[LocalTrackEvidence],
    program: &Program,
    album_title: &str,
) -> PositionedProgram {
    let album = crate::matching::album_representation_title(album_title);
    let mut derived = program.clone();
    let mut known = local.to_vec();
    let mut changed = 0;
    for p in &mut derived.tracks {
        if let Some(title) = &p.title
            && let Some((base, suffix)) = title.rsplit_once(" - ")
        {
            let annotation = crate::matching::album_representation_title(suffix);
            let annotation = annotation.strip_suffix(" version").unwrap_or(&annotation);
            if !annotation.is_empty()
                && (album.ends_with(&format!(" {annotation}"))
                    || album == annotation
                    || format!("{album} live") == annotation)
            {
                p.title = Some(base.into());
                changed += 1;
            }
        }
    }
    let mut movements = 0;
    let mut edition_annotations = 0;
    let mut edition_indices = vec![];
    let mut edition_patterns = std::collections::HashSet::new();
    for (index, t) in known.iter_mut().enumerate() {
        if let Some(title) = &t.evidence.title
            && let Some((base, tail)) = title.rsplit_once(" (")
        {
            let tail = tail.to_lowercase();
            if let Some(annotation) = tail.strip_suffix(')')
                && annotation.ends_with(" remaster")
                && annotation
                    .strip_suffix(" remaster")
                    .is_some_and(|year| year.len() == 4 && year.parse::<u16>().is_ok())
                && album.contains(&crate::matching::album_representation_title(annotation))
            {
                t.evidence.title = Some(base.into());
                t.evidence.title_observations.clear();
                edition_annotations += 1;
                edition_indices.push(index);
                edition_patterns.insert(annotation.to_owned());
                continue;
            }
            if let Some(n) = tail
                .strip_prefix("movement ")
                .and_then(|s| s.strip_suffix(')'))
                && n.parse::<u32>().is_ok()
            {
                t.evidence.title = Some(base.into());
                t.evidence.title_observations.clear();
                movements += 1;
            }
        }
    }
    let edition_systematic = edition_patterns.len() == 1
        && edition_annotations >= 3
        && edition_annotations * 5 >= local.len() * 4;
    if !edition_systematic {
        for index in edition_indices {
            known[index] = local[index].clone();
        }
    }
    // A repeated short non-Latin token is an annotation only when the remainder
    // uniquely reconstructs an already corroborated program. Varying translated
    // titles, meaningful suffixes and arbitrary subtitles are left untouched.
    let prefixes: Vec<_> = known
        .iter()
        .filter_map(|t| {
            let (prefix, _) = t.evidence.title.as_deref()?.split_once(' ')?;
            (prefix.chars().count() <= 3
                && !prefix.is_empty()
                && prefix.chars().all(|c| c.is_alphabetic() && !c.is_ascii()))
            .then_some(prefix.to_owned())
        })
        .collect();
    let prefix = prefixes.first().filter(|p| {
        prefixes.len() >= 3
            && prefixes.len() * 5 >= local.len() * 4
            && prefixes.iter().all(|other| other == *p)
    });
    let mut prefixes_changed = 0;
    if let Some(prefix) = prefix {
        for t in &mut known {
            if let Some(base) = t
                .evidence
                .title
                .as_deref()
                .and_then(|s| s.strip_prefix(&format!("{prefix} ")))
            {
                t.evidence.title = Some(base.into());
                t.evidence.title_observations.clear();
                prefixes_changed += 1;
            }
        }
    }
    let prefix_systematic = prefixes_changed >= 3 && prefixes_changed * 5 >= local.len() * 4;
    let systematic = changed >= 3 && changed * 5 >= program.tracks.len() * 4
        || movements >= 3
        || edition_systematic
        || prefix_systematic;
    if !systematic || !align_program(&known, &derived).strong {
        let mut result = inspect_positioned_program(local, program);
        if movements >= 3 || edition_annotations > 0 || prefixes_changed > 0 {
            for row in &mut result.tracks {
                if row
                    .provider
                    .as_ref()
                    .is_some_and(|p| title_relation(&row.local, p) != TitleRelation::Agrees)
                {
                    row.decision = PositionDecision::InsufficientAnchors;
                }
            }
        }
        return result;
    }
    let mut result = inspect_positioned_program(&known, &derived);
    if edition_systematic {
        result.warnings.push(
            "Systematic edition annotation ignored for program comparison; raw titles retained"
                .into(),
        );
    }
    if prefix_systematic {
        result.warnings.push(format!(
            "Systematic local prefix '{}' ignored for program comparison; raw titles retained",
            prefix.unwrap()
        ));
    }
    for row in &mut result.tracks {
        if let Some(t) = local.iter().find(|t| t.track_id.as_ref() == row.track_id) {
            row.local = t.evidence.clone();
        }
        if let Some(p) = &row.provider
            && let Some(original) = derived.tracks.iter().position(|t| t == p)
        {
            row.provider = Some(program.tracks[original].clone());
        }
    }
    result
}
