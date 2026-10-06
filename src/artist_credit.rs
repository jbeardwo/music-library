//! Non-destructive Artist evidence comparison. Display join phrases are not identity.
use crate::{domain::ExternalIdentity, edition::ArtistEvidence, matching::normalize_album_title};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compatibility {
    Equivalent,
    /// One provider omits additional credits; it does not name replacements.
    Incomplete,
    /// Primary agrees, but contributor names/identities cannot be reconciled.
    DifferentAdditional,
    Contradictory,
    Unspecified,
}
fn name(value: &str) -> String {
    // Unicode hyphen typography only. Never split names on &, feat., or with.
    normalize_album_title(value)
}
fn conflict(a: &[ExternalIdentity], b: &[ExternalIdentity]) -> bool {
    a.iter().any(|id| {
        b.iter()
            .any(|other| id.provider == other.provider && id.kind == other.kind)
            && !a.iter().any(|same| {
                same.provider == id.provider && same.kind == id.kind && b.contains(same)
            })
    })
}
pub fn same_artist(a: &ArtistEvidence, b: &ArtistEvidence) -> bool {
    !conflict(&a.identities, &b.identities)
        && (a.identities.iter().any(|id| b.identities.contains(id))
            || !a.name.trim().is_empty() && name(&a.name) == name(&b.name))
}
pub fn compare(a: &[ArtistEvidence], b: &[ArtistEvidence]) -> Compatibility {
    let (Some(primary_a), Some(primary_b)) = (a.first(), b.first()) else {
        return Compatibility::Unspecified;
    };
    if !same_artist(primary_a, primary_b) {
        return Compatibility::Contradictory;
    }
    let left = a
        .iter()
        .all(|artist| b.iter().any(|other| same_artist(artist, other)));
    let right = b
        .iter()
        .all(|artist| a.iter().any(|other| same_artist(artist, other)));
    if left && right {
        return Compatibility::Equivalent;
    }
    if left || right {
        return Compatibility::Incomplete;
    }
    // A different known identity in a shared namespace is stronger than spelling.
    if a[1..]
        .iter()
        .filter(|artist| !b.iter().any(|other| same_artist(artist, other)))
        .any(|artist| {
            b[1..]
                .iter()
                .filter(|other| !a.iter().any(|known| same_artist(known, other)))
                .any(|other| conflict(&artist.identities, &other.identities))
        })
    {
        Compatibility::Contradictory
    } else {
        Compatibility::DifferentAdditional
    }
}

/// Only remove an explicit trailing featured-credit annotation that is fully
/// corroborated by that provider's own ordered additional Artist names.
/// This is a comparison view; the source title and credits remain untouched.
pub fn musical_title<'a>(title: &'a str, artists: &[ArtistEvidence]) -> &'a str {
    let trimmed = title.trim();
    let Some(open) = trimmed.rfind(" (") else {
        return title;
    };
    if !trimmed.ends_with(')') || artists.len() < 2 {
        return title;
    }
    let suffix = name(&trimmed[open + 2..trimmed.len() - 1]);
    let Some(credited) = suffix
        .strip_prefix("feat. ")
        .or_else(|| suffix.strip_prefix("featuring "))
    else {
        return title;
    };
    let names: Vec<_> = artists[1..].iter().map(|a| name(&a.name)).collect();
    if names.iter().any(String::is_empty) {
        return title;
    }
    if [", ", " & ", " and "]
        .iter()
        .any(|separator| names.join(separator) == credited)
    {
        &trimmed[..open]
    } else {
        title
    }
}

pub(crate) struct LoadedCredit {
    pub canonical_name: String,
    pub evidence: ArtistEvidence,
}
pub(crate) fn load(
    db: &rusqlite::Connection,
    scope: &str,
    key: &str,
) -> crate::storage::Result<Vec<LoadedCredit>> {
    let (table, column) = match scope {
        "track" => ("track_artist_credit", "track_id"),
        "album" => ("album_artist_credit", "album_id"),
        _ => {
            return Err(crate::storage::Error::Invalid(
                "invalid internal Artist-credit scope".into(),
            ));
        }
    };
    let rows=db.prepare(&format!("SELECT a.id,a.name,COALESCE(c.credited_name,a.name),COALESCE(c.join_phrase,'') FROM {table} c JOIN artist a ON a.id=c.artist_id WHERE c.{column}=?1 ORDER BY c.position"))?
        .query_map([key],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let ids = rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>();
    let expanded = crate::artist_equivalence::identities(db, &ids)?;
    Ok(rows
        .into_iter()
        .map(|(id, canonical_name, name, join_phrase)| LoadedCredit {
            canonical_name,
            evidence: ArtistEvidence {
                name,
                join_phrase,
                identities: expanded.get(&id).cloned().unwrap_or_default(),
            },
        })
        .collect())
}
