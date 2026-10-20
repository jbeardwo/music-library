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
    inspect_positioned_program_context(local, program, false)
}
fn playable_alignment(
    local: &[LocalTrackEvidence],
    program: &Program,
    alignment: &Alignment,
) -> bool {
    let matched = alignment
        .rows
        .iter()
        .filter_map(|r| r.local_index.zip(r.provider_index))
        .collect::<Vec<_>>();
    let trusted = |a: &TrackEvidence, b: &TrackEvidence| {
        a.artists.first().is_some_and(|artist| {
            artist.identities.iter().any(|id| {
                id.provider == "spotify"
                    && id.kind == "artist"
                    && b.artists
                        .first()
                        .is_some_and(|artist| artist.identities.contains(id))
            })
        })
    };
    let corroborated: Vec<_> = matched
        .iter()
        .filter(|(i, j)| trusted(&local[*i].evidence, &program.tracks[*j]))
        .collect();
    program.complete
        && corroborated.len() >= 3
        && (matched.len() * 4 >= local.len() * 3 || matched.len() * 4 >= program.tracks.len() * 3)
        && matched.iter().all(|(i, j)| {
            let a = &local[*i].evidence;
            let b = &program.tracks[*j];
            // Missing Track-level identity is neutral only inside a program
            // independently corroborated by at least three trusted Artist anchors.
            // It does not establish identity from a featured-credit substring.
            let artist = trusted(a, b)
                || a.artists.first().is_none_or(|artist| {
                    !artist
                        .identities
                        .iter()
                        .any(|id| id.provider == "spotify" && id.kind == "artist")
                }) && b.artists.first().is_some_and(|artist| {
                    corroborated.iter().any(|(li, _)| {
                        local[*li].evidence.artists.first().is_some_and(|known| {
                            known.identities.iter().any(|id| {
                                id.provider == "spotify"
                                    && id.kind == "artist"
                                    && artist.identities.contains(id)
                            })
                        })
                    })
                });
            artist
                && !occurrence_identity_conflict(a, b)
                && a.duration_ms
                    .zip(b.duration_ms)
                    .is_none_or(|(a, b)| a.abs_diff(b) <= 10_000)
        })
}
fn inspect_positioned_program_context(
    local: &[LocalTrackEvidence],
    program: &Program,
    playable: bool,
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
    let mut alignment = align_program(local, program);
    if playable && playable_alignment(local, program, &alignment) {
        alignment.strong = true;
    }
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
                None if program.tracks.iter().any(|p| {
                    occurrence_identity_conflict(&t.evidence, p)
                        && (title_relation(&t.evidence, p) == TitleRelation::Agrees
                            || crate::matching::contextual_title_class(
                                t.evidence.title.as_deref().unwrap_or(""),
                                p.title.as_deref().unwrap_or(""),
                            )
                            .is_some())
                }) =>
                {
                    PositionDecision::TrustedIdentityConflict
                }
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
    inspect_program_context(local, program, album_title, false)
}
/// Playback correspondence permits a well-corroborated subset without asserting
/// canonical Release identity. Artist IDs and unique program context are required.
pub fn inspect_playable_program(
    local: &[LocalTrackEvidence],
    program: &Program,
    album_title: &str,
) -> PositionedProgram {
    inspect_program_context(local, program, album_title, true)
}
fn inspect_program_context(
    local: &[LocalTrackEvidence],
    program: &Program,
    album_title: &str,
    playable: bool,
) -> PositionedProgram {
    let album = crate::matching::album_representation_title(album_title);
    let mut derived = program.clone();
    let mut known = local.to_vec();
    if playable {
        // Existing exact provider occurrences remain attached. They can still
        // corroborate another edition's program for unresolved Tracks; an
        // occurrence ID difference alone is not a Recording conflict.
        for t in &mut known {
            t.evidence
                .identities
                .retain(|id| !(id.provider == "spotify" && id.kind == "track"));
        }
    }
    // Keep the same occurrence-neutral evidence in the non-systematic path.
    // Otherwise existing occurrences from another edition veto valid playback mappings.
    let fallback_known = known.clone();
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
    // A featured annotation is comparison-only and must be explained by that
    // candidate's structured additional credits. A whole-program gate below
    // prevents this derivation from bootstrapping isolated title identity.
    let mut featured = 0;
    for t in &mut known {
        let raw = t.evidence.title.as_deref().unwrap_or("");
        let choices = derived
            .tracks
            .iter()
            .filter(|p| {
                let base = crate::artist_credit::musical_title(raw, &p.artists);
                base != raw
                    && crate::matching::normalize_album_title(base)
                        == crate::matching::normalize_album_title(p.title.as_deref().unwrap_or(""))
                    && t.evidence
                        .artists
                        .first()
                        .zip(p.artists.first())
                        .is_some_and(|(a, b)| crate::artist_credit::same_artist(a, b))
            })
            .collect::<Vec<_>>();
        if let [p] = choices.as_slice() {
            t.evidence.title = Some(crate::artist_credit::musical_title(raw, &p.artists).into());
            t.evidence.title_observations.clear();
            featured += 1;
        }
    }
    // Full correspondence always wins. Only unmatched titles may use a contained
    // component, and only a unique, Artist/duration-compatible whole program
    // can corroborate that fallback. No script is designated canonical.
    let mut shared_spans = vec![];
    let mut pairs = vec![];
    let mut contextual_pairs = vec![];
    for (i, t) in known.iter().enumerate() {
        if derived
            .tracks
            .iter()
            .any(|p| title_relation(&t.evidence, p) == TitleRelation::Agrees)
        {
            continue;
        }
        for (j, p) in derived.tracks.iter().enumerate() {
            if known
                .iter()
                .any(|other| title_relation(&other.evidence, p) == TitleRelation::Agrees)
            {
                continue;
            }
            let a = t.evidence.title.as_deref().unwrap_or("");
            let b = p.title.as_deref().unwrap_or("");
            let contextual = crate::matching::contextual_title_class(a, b);
            let weak_annotation =
                contextual == Some(crate::matching::ContextualTitleClass::AlbumVersion);
            if t.evidence.title_observations.iter().any(|o| {
                qualifiers(&comparison_title(&o.value)) != qualifiers(&comparison_title(b))
                    && !(weak_annotation
                        && crate::matching::contextual_title_class(&o.value, b) == contextual)
            }) || qualifiers(&comparison_title(a)) != qualifiers(&comparison_title(b))
                && !weak_annotation
                || occurrence_identity_conflict(&t.evidence, p)
                || t.evidence
                    .duration_ms
                    .zip(p.duration_ms)
                    .is_some_and(|(a, b)| {
                        a.abs_diff(b)
                            > if playable
                                && contextual.is_some_and(|c| {
                                    c != crate::matching::ContextualTitleClass::TinyTypo
                                })
                            {
                                10_000
                            } else {
                                3_000
                            }
                    })
                || !matches!(
                    crate::artist_credit::compare(&t.evidence.artists, &p.artists),
                    crate::artist_credit::Compatibility::Equivalent
                        | crate::artist_credit::Compatibility::Incomplete
                        | crate::artist_credit::Compatibility::DifferentAdditional
                )
            {
                continue;
            }
            if let Some(class) = contextual
                && position(&t.evidence).is_some()
                && position(&t.evidence) == position(p)
                && t.evidence.artists.first().zip(p.artists.first()).is_some_and(|(a,b)| crate::artist_credit::trusted_overlap(a,b))
                // Consider competitors even at other positions: a position cannot
                // resolve two equally close title observations by itself.
                && derived.tracks.iter().filter(|other| crate::matching::contextual_title_class(a, other.title.as_deref().unwrap_or("")) == Some(class)).count() == 1
                && known.iter().filter(|other| crate::matching::contextual_title_class(other.evidence.title.as_deref().unwrap_or(""), b) == Some(class)).count() == 1
            {
                contextual_pairs.push((i, j, class));
            } else if punctuation_placeholder(a)
                && punctuation_placeholder(b)
                && position(&t.evidence).is_some()
                && position(&t.evidence) == position(p)
            {
                pairs.push((i, j, "contextual punctuation placeholder".into(), false));
            } else if let Some(span) = crate::matching::shared_title_span(a, b) {
                pairs.push((i, j, span, false));
            } else if playable {
                let left = crate::matching::normalize_album_title(a).replace('\'', "");
                let right = crate::matching::normalize_album_title(b).replace('\'', "");
                if left == right
                    && left.chars().filter(|c| c.is_alphanumeric()).count() >= 5
                    && position(&t.evidence) == position(p)
                    && t.evidence
                        .duration_ms
                        .zip(p.duration_ms)
                        .is_some_and(|(a, b)| a.abs_diff(b) <= 3000)
                {
                    pairs.push((i, j, left, true));
                }
            }
        }
    }
    for (i, j, span, apostrophe) in &pairs {
        if pairs.iter().filter(|(a, _, _, _)| a == i).count() == 1
            && pairs.iter().filter(|(_, b, _, _)| b == j).count() == 1
        {
            shared_spans.push(format!(
                "{} ↔ {}: {} '{}'",
                known[*i].evidence.title.as_deref().unwrap_or(""),
                derived.tracks[*j].title.as_deref().unwrap_or(""),
                if *apostrophe {
                    "apostrophe typography under strong playback program"
                } else {
                    "shared title span"
                },
                span
            ));
            known[*i].evidence.title = Some(span.clone());
            known[*i].evidence.title_observations.clear();
            derived.tracks[*j].title = Some(span.clone());
        }
    }
    // Existing shared-span/apostrophe evidence can corroborate the program
    // before weaker spacing/typo rules. Weak rules cannot bootstrap each other.
    let independent = align_program(&known, &derived);
    let anchors = independent
        .rows
        .iter()
        .filter(|r| r.local_index.is_some() && r.provider_index.is_some())
        .count();
    if program.complete && anchors >= 3 && anchors * 4 >= known.len().max(derived.tracks.len()) * 3
    {
        for (i, j, class) in contextual_pairs {
            let raw_local = known[i].evidence.title.as_deref().unwrap_or("");
            let raw_provider = derived.tracks[j].title.as_deref().unwrap_or("");
            shared_spans.push(format!("{raw_local} ↔ {raw_provider}: {}", class.label()));
            let key = comparison_title(raw_local);
            known[i].evidence.title = Some(key.clone());
            known[i].evidence.title_observations.clear();
            derived.tracks[j].title = Some(key);
        }
    }
    let shared_systematic = !shared_spans.is_empty();
    let systematic = featured > 0
        || changed >= 3 && changed * 5 >= program.tracks.len() * 4
        || movements >= 3
        || edition_systematic
        || shared_systematic;
    let alignment = align_program(&known, &derived);
    if !systematic
        || !(alignment.strong || playable && playable_alignment(&known, &derived, &alignment))
    {
        let mut result = inspect_positioned_program_context(&fallback_known, program, playable);
        for row in &mut result.tracks {
            if let Some(original) = local.iter().find(|t| t.track_id.as_ref() == row.track_id) {
                row.local = original.evidence.clone();
            }
        }
        if movements >= 3 || edition_annotations > 0 || shared_systematic {
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
    let mut result = inspect_positioned_program_context(&known, &derived, playable);
    if edition_systematic {
        result.warnings.push(
            "Systematic edition annotation ignored for program comparison; raw titles retained"
                .into(),
        );
    }
    if shared_systematic {
        result.warnings.extend(shared_spans);
        result
            .warnings
            .push("Contextual shared-title comparison only; all raw metadata retained".into());
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
