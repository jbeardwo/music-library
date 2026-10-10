//! Unique title/identity program correspondence; positions are evidence, never identity.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub enum AlignmentKind {
    Exact,
    InsertedLocal,
    MissingLocal,
    Reordered,
    TitleNormalized,
    Unmatched,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct AlignmentRow {
    pub local_index: Option<usize>,
    pub provider_index: Option<usize>,
    pub kind: AlignmentKind,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Alignment {
    pub rows: Vec<AlignmentRow>,
    pub strong: bool,
    pub unreliable_positions: bool,
}

/// Number words are normalized only when the entire title is an unambiguous integer.
/// This deliberately leaves phrases such as "One More" and "One Direction" intact.
pub fn numeric_title(value: &str) -> String {
    let value = crate::matching::normalize(value).replace('-', " ");
    let words = [
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
    ];
    if let Some(n) = words.iter().position(|w| *w == value) {
        return n.to_string();
    }
    let parts: Vec<_> = value.split_whitespace().collect();
    let tens = [
        "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
    ];
    if (1..=2).contains(&parts.len())
        && let Some(n) = tens.iter().position(|w| *w == parts[0])
    {
        if parts.len() == 1 {
            return ((n + 2) * 10).to_string();
        }
        if let Some(unit) = words[1..10].iter().position(|w| *w == parts[1]) {
            return ((n + 2) * 10 + unit + 1).to_string();
        }
    }
    value
}

pub fn align_program(local: &[LocalTrackEvidence], program: &Program) -> Alignment {
    let mut candidates = vec![vec![]; local.len()];
    for (i, l) in local.iter().enumerate() {
        for (j, p) in program.tracks.iter().enumerate() {
            let trusted = l
                .evidence
                .identities
                .iter()
                .any(|id| p.identities.contains(id));
            let title = title_relation(&l.evidence, p) == TitleRelation::Agrees;
            let duration = l
                .evidence
                .duration_ms
                .zip(p.duration_ms)
                .is_none_or(|(a, b)| a.abs_diff(b) <= 10_000);
            if (trusted || title && duration) && !occurrence_identity_conflict(&l.evidence, p) {
                candidates[i].push(j);
            }
        }
    }
    let mut mapping = vec![None; local.len()];
    for (i, c) in candidates.iter().enumerate() {
        if let [j] = c.as_slice()
            && candidates.iter().filter(|other| other.contains(j)).count() == 1
        {
            mapping[i] = Some(*j);
        }
    }
    let matched = mapping.iter().flatten().count();
    // High coverage in BOTH programs prevents a shared small subset from joining unrelated programs.
    let strong = program.complete
        && matched >= 3
        && matched * 5 >= local.len() * 4
        && matched * 5 >= program.tracks.len() * 4;
    let unreliable_positions = local
        .iter()
        .filter(|t| {
            t.evidence
                .number
                .is_some_and(|n| n as usize > local.len().max(program.tracks.len()) * 4)
        })
        .count()
        * 2
        >= local.len().max(1);
    let ordered = mapping
        .iter()
        .flatten()
        .copied()
        .collect::<Vec<_>>()
        .windows(2)
        .all(|w| w[0] < w[1]);
    let mut rows = vec![];
    for (i, j) in mapping.iter().enumerate() {
        let kind = match j {
            None if strong && ordered && candidates[i].is_empty() => AlignmentKind::InsertedLocal,
            None => AlignmentKind::Unmatched,
            Some(j) if !ordered => AlignmentKind::Reordered,
            Some(j) if position(&local[i].evidence) == position(&program.tracks[*j]) => {
                AlignmentKind::Exact
            }
            Some(_) => AlignmentKind::TitleNormalized,
        };
        rows.push(AlignmentRow {
            local_index: Some(i),
            provider_index: *j,
            kind,
        });
    }
    for j in 0..program.tracks.len() {
        if !mapping.contains(&Some(j)) {
            rows.push(AlignmentRow {
                local_index: None,
                provider_index: Some(j),
                kind: AlignmentKind::MissingLocal,
            });
        }
    }
    Alignment {
        rows,
        strong,
        unreliable_positions,
    }
}

pub fn numeric_equivalent(a: &str, b: &str) -> bool {
    if numeric_title(a) == numeric_title(b) {
        return true;
    }
    let normalize_phrase = |value: &str| {
        let words = value.split_whitespace().collect::<Vec<_>>();
        let mut result = Vec::new();
        let mut i = 0;
        while i < words.len() {
            let pair = if i + 1 < words.len() {
                numeric_title(&format!("{} {}", words[i], words[i + 1]))
            } else {
                String::new()
            };
            if pair.parse::<u32>().is_ok() {
                result.push(pair);
                i += 2;
                continue;
            }
            let n = numeric_title(words[i]);
            // Ten and above are unambiguous numeric forms. Small number words
            // embedded in ordinary phrases retain their lexical meaning.
            if n.parse::<u32>().is_ok_and(|n| n >= 10) {
                result.push(n);
            } else {
                result.push(words[i].to_owned());
            }
            i += 1;
        }
        result.join(" ")
    };
    normalize_phrase(a) == normalize_phrase(b)
}
