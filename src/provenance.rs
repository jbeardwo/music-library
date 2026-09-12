//! Unverified observations, not attached external identities or canonical metadata.
//! Provider adapters classify identifiers; aggregation never interprets provider names.
use std::collections::{HashMap, HashSet};

use crate::{domain::ExternalIdentity, edition::Completeness};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Scope {
    AlbumArtist,
    TrackArtist,
    Album,
    Edition,
    Recording,
    Occurrence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Semantics {
    ArtistIdentity,
    AlbumIdentity,
    EditionIdentity,
    RecordingIdentity,
    OccurrenceIdentity,
    /// Supporting recording evidence, never a universal identity.
    Isrc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Origin {
    EmbeddedTag { format: String, field: String },
    ProviderResponse,
    ManualSelection,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Observation {
    pub identity: ExternalIdentity,
    pub scope: Scope,
    pub semantics: Semantics,
    pub origin: Origin,
}

/// Keep repeated/raw declarations: a first-value accessor would hide conflicts.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Positions {
    pub disc: Vec<String>,
    pub discs: Vec<String>,
    pub track: Vec<String>,
    pub tracks: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FileProvenance {
    /// Assigned after scan persistence commits, absent for a stand-alone file probe.
    pub source_id: Option<crate::domain::SourceId>,
    pub file_type: Option<String>,
    pub formats: Vec<String>,
    pub observations: Vec<Observation>,
    pub positions: Positions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityConflict {
    pub scope: Scope,
    pub semantics: Semantics,
    pub provider: String,
    pub kind: String,
    pub values: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Aggregate {
    /// Consistent observations only; still unverified, never automatically attached.
    pub consistent: Vec<Observation>,
    pub conflicts: Vec<IdentityConflict>,
}

/// Group only observations of the same semantic namespace, never majority-vote.
pub fn aggregate<'a>(observations: impl IntoIterator<Item = &'a Observation>) -> Aggregate {
    let mut indexes = HashMap::new();
    let mut groups: Vec<Vec<&Observation>> = Vec::new();
    for o in observations {
        let index = *indexes
            .entry((o.scope, o.semantics, &o.identity.provider, &o.identity.kind))
            .or_insert_with(|| {
                groups.push(Vec::new());
                groups.len() - 1
            });
        groups[index].push(o);
    }
    let mut result = Aggregate::default();
    for observations in groups {
        let first = observations[0];
        let (scope, semantics) = (first.scope, first.semantics);
        let mut seen = HashSet::new();
        let values: Vec<_> = observations
            .iter()
            .filter(|o| seen.insert(&o.identity.external_id))
            .map(|o| o.identity.external_id.clone())
            .collect();
        // ISRCs can legitimately be multiple; they are supporting identifiers.
        if values.len() > 1 && semantics != Semantics::Isrc {
            result.conflicts.push(IdentityConflict {
                scope,
                semantics,
                provider: first.identity.provider.clone(),
                kind: first.identity.kind.clone(),
                values,
            });
        } else {
            result.consistent.extend(observations.into_iter().cloned());
        }
    }
    result
}

/// One entry per Track, retaining each source separately. Empty entries mean no
/// session observation. Multiple sources deliberately cannot prove completeness.
#[derive(Clone, Debug, Default)]
pub struct EditionProvenance {
    pub tracks: Vec<Vec<FileProvenance>>,
    pub album_and_edition: Aggregate,
    pub per_track: Vec<Aggregate>,
    pub completeness: Completeness,
}

impl EditionProvenance {
    pub fn new(tracks: Vec<Vec<FileProvenance>>) -> Self {
        let album_and_edition = aggregate(
            tracks
                .iter()
                .flatten()
                .flat_map(|f| &f.observations)
                .filter(|o| matches!(o.scope, Scope::Album | Scope::Edition)),
        );
        let per_track = tracks
            .iter()
            .map(|sources| {
                aggregate(
                    sources
                        .iter()
                        .flat_map(|f| &f.observations)
                        .filter(|o| matches!(o.scope, Scope::Recording | Scope::Occurrence)),
                )
            })
            .collect();
        let completeness = if tracks.iter().all(|s| s.len() == 1) {
            completeness(tracks.iter().map(|s| &s[0].positions))
        } else {
            Completeness::Unknown
        };
        Self {
            tracks,
            album_and_edition,
            per_track,
            completeness,
        }
    }
}

fn positive(values: &[String]) -> Option<u32> {
    let first = values
        .first()?
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|v| *v > 0)?;
    values
        .iter()
        .all(|v| v.trim().parse::<u32>() == Ok(first))
        .then_some(first)
}

impl Positions {
    pub fn agrees_with(&self, disc: Option<u32>, track: Option<u32>) -> bool {
        positive(&self.disc).is_some_and(|d| Some(d) == disc)
            && positive(&self.track).is_some_and(|t| Some(t) == track)
    }
}

/// Linear in observed Track count; declared totals never allocate/iterate ranges.
pub fn completeness<'a>(positions: impl IntoIterator<Item = &'a Positions>) -> Completeness {
    let mut disc_total = None;
    let mut discs = std::collections::HashMap::new();
    for p in positions {
        let (Some(disc), Some(total), Some(track), Some(tracks)) = (
            positive(&p.disc),
            positive(&p.discs),
            positive(&p.track),
            positive(&p.tracks),
        ) else {
            return Completeness::Unknown;
        };
        if disc > total || track > tracks || disc_total.is_some_and(|old| old != total) {
            return Completeness::Unknown;
        }
        disc_total = Some(total);
        let (expected, seen) = discs
            .entry(disc)
            .or_insert_with(|| (tracks, HashSet::new()));
        if *expected != tracks || !seen.insert(track) {
            return Completeness::Unknown;
        }
    }
    if disc_total.is_some_and(|n| n as usize == discs.len())
        && discs.values().all(|(n, seen)| *n as usize == seen.len())
    {
        Completeness::TrustedComplete
    } else {
        Completeness::Unknown
    }
}
