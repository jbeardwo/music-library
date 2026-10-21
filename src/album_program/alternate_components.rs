//! Comparison-only alternate representations, corroborated by a complete program.
use super::*;
use crate::matching::{AlternateTitleLayout as Layout, exact_alternate_component};

pub(super) fn apply(local: &mut [LocalTrackEvidence], provider: &mut Program) -> Vec<String> {
    if !provider.complete {
        return vec![];
    }
    let mut proposed = vec![];
    for (i, l) in local.iter().enumerate() {
        if provider
            .tracks
            .iter()
            .any(|p| title_relation(&l.evidence, p) == TitleRelation::Agrees)
        {
            continue;
        }
        for (j, p) in provider.tracks.iter().enumerate() {
            if local
                .iter()
                .any(|l| title_relation(&l.evidence, p) == TitleRelation::Agrees)
            {
                continue;
            }
            let a = l.evidence.title.as_deref().unwrap_or("");
            let b = p.title.as_deref().unwrap_or("");
            let Some(relation) = exact_alternate_component(a, b) else {
                continue;
            };
            if qualifiers(a) != qualifiers(b)
                || l.evidence
                    .title_observations
                    .iter()
                    .any(|o| qualifiers(&o.value) != qualifiers(b))
                || p.title_observations
                    .iter()
                    .any(|o| qualifiers(&o.value) != qualifiers(a))
                || occurrence_identity_conflict(&l.evidence, p)
                || !l
                    .evidence
                    .artists
                    .first()
                    .zip(p.artists.first())
                    .is_some_and(|(a, b)| crate::artist_credit::trusted_overlap(a, b))
            {
                continue;
            }
            // Count competitors regardless of position; sequence cannot turn a
            // duplicate component into a unique correspondence.
            if provider
                .tracks
                .iter()
                .filter(|p| {
                    exact_alternate_component(a, p.title.as_deref().unwrap_or("")).is_some()
                })
                .count()
                != 1
                || local
                    .iter()
                    .filter(|l| {
                        exact_alternate_component(l.evidence.title.as_deref().unwrap_or(""), b)
                            .is_some()
                    })
                    .count()
                    != 1
            {
                continue;
            }
            proposed.push((i, j, relation));
        }
    }
    let independent = align_program(local, provider);
    let anchors = independent
        .rows
        .iter()
        .filter(|r| r.local_index.is_some() && r.provider_index.is_some())
        .count();
    let same_pattern =
        |a: &crate::matching::AlternateComponentMatch,
         b: &crate::matching::AlternateComponentMatch| { a.layout == b.layout };
    let qualified = proposed
        .iter()
        .filter(|(_, _, r)| {
            let repeats = proposed
                .iter()
                .filter(|(_, _, other)| same_pattern(r, other))
                .count();
            repeats >= 3
                || r.layout == Layout::WhitespaceBlocks
                    && r.component.chars().filter(|c| c.is_alphanumeric()).count() >= 2
                    && anchors >= 3
        })
        .collect::<Vec<_>>();
    let mut known = local.to_vec();
    let mut derived = provider.clone();
    for (i, j, r) in &qualified {
        known[*i].evidence.title = Some(r.component.clone());
        known[*i].evidence.title_observations.clear();
        derived.tracks[*j].title = Some(r.component.clone());
        derived.tracks[*j].title_observations.clear();
    }
    let proof = align_program(&known, &derived);
    let mapped = proof
        .rows
        .iter()
        .filter(|r| r.local_index.is_some() && r.provider_index.is_some())
        .count();
    let compatible = proof
        .rows
        .iter()
        .filter(|r| {
            r.local_index.zip(r.provider_index).is_some_and(|(i, j)| {
                local[i]
                    .evidence
                    .duration_ms
                    .zip(provider.tracks[j].duration_ms)
                    .is_none_or(|(a, b)| a.abs_diff(b) <= 3_000)
            })
        })
        .count();
    let size = local.len().max(provider.tracks.len());
    if !proof.strong
        || mapped < 3
        || mapped * 5 < size * 4
        || compatible < 3
        || compatible * 5 < size * 4
    {
        return vec![];
    }
    let mut warnings = vec![];
    for (i, j, r) in qualified {
        if !proof
            .rows
            .iter()
            .any(|row| row.local_index == Some(*i) && row.provider_index == Some(*j))
            && !(local[*i].evidence.number.is_some()
                && local[*i].evidence.number == provider.tracks[*j].number
                && local[*i].evidence.disc.unwrap_or(1) == provider.tracks[*j].disc.unwrap_or(1))
        {
            continue;
        }
        warnings.push(format!("{} ↔ {}: exact alternate-title component '{}' ({:?}); repeated pattern / trusted Artist / unique aligned program, {mapped}/{size} coverage",local[*i].evidence.title.as_deref().unwrap_or(""),provider.tracks[*j].title.as_deref().unwrap_or(""),r.component,r.layout));
        local[*i] = known[*i].clone();
        provider.tracks[*j] = derived.tracks[*j].clone();
    }
    warnings
}
