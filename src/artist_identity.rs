//! Conservative consolidation from explicit catalog identities or a single local
//! Artist directory inside a registered root. Display credits are never rewritten.
use crate::{
    domain::ArtistId,
    storage::{Result, bytes_to_path, merge_artist_tx, path_to_bytes},
};
use rusqlite::{Transaction, params};
use std::collections::{BTreeMap, BTreeSet};

fn compatible(tx: &Transaction<'_>, ids: &[ArtistId]) -> Result<bool> {
    let mut namespaces = BTreeMap::<(String, String), BTreeSet<String>>::new();
    for id in ids {
        let mut q=tx.prepare("SELECT provider,kind,external_id FROM artist_external_identity WHERE artist_id=?1 AND kind='artist'")?;
        for row in q.query_map([id.as_ref()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })? {
            let (provider, kind, value) = row?;
            namespaces
                .entry((provider, kind))
                .or_default()
                .insert(value);
        }
    }
    Ok(namespaces.values().all(|v| v.len() == 1))
}
fn merge_group(tx: &Transaction<'_>, ids: &[ArtistId]) -> Result<()> {
    if ids.len() < 2 || !compatible(tx, ids)? {
        return Ok(());
    }
    // Prefer an already externally identified Artist, then a stable existing ID.
    let mut ranked = Vec::new();
    for id in ids {
        let count: i64 = tx.query_row(
            "SELECT count(*) FROM artist_external_identity WHERE artist_id=?1 AND kind='artist'",
            [id.as_ref()],
            |r| r.get(0),
        )?;
        ranked.push((std::cmp::Reverse(count), id.clone()));
    }
    ranked.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.as_ref().cmp(b.1.as_ref())));
    let canonical = &ranked[0].1;
    for id in ids {
        merge_artist_tx(tx, id, canonical)?;
    }
    Ok(())
}

pub(crate) fn backfill(tx: &Transaction<'_>) -> Result<()> {
    let groups=tx.prepare("SELECT provider,kind,external_id FROM artist_external_identity WHERE kind='artist' AND external_id<>'' GROUP BY provider,kind,external_id HAVING count(*)>1")?
        .query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for (p, k, v) in groups {
        let ids=tx.prepare("SELECT artist_id FROM artist_external_identity WHERE provider=?1 AND kind=?2 AND external_id=?3")?
            .query_map(params![p,k,v],|r|r.get::<_,String>(0).map(ArtistId))?.collect::<rusqlite::Result<Vec<_>>>()?;
        merge_group(tx, &ids)?;
    }
    let albums=tx.prepare("SELECT DISTINCT r.album_id FROM track_source ts JOIN track t ON t.id=ts.track_id JOIN release r ON r.id=t.release_id")?
        .query_map([],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    reconcile_albums(tx, &albums)
}

/// Shared finalization for migration backfill and scoped local admission.
pub(crate) fn reconcile_albums(tx: &Transaction<'_>, albums: &[String]) -> Result<()> {
    // Register all evidence before merging; never load unrelated library Albums.
    let mut contexts = BTreeSet::new();
    for album in albums {
        if let Some((_, directory, name)) = register_album(tx, album)? {
            contexts.insert((directory, name));
        }
    }
    for (directory, name) in contexts {
        consolidate_context(tx, &directory, &name)?;
    }
    Ok(())
}

fn consolidate_context(tx: &Transaction<'_>, directory: &[u8], name: &str) -> Result<()> {
    let ids=tx.prepare("SELECT DISTINCT artist_id FROM local_artist_context WHERE root_id IN (SELECT id FROM discovery_root WHERE kind='local_filesystem') AND directory=?1 AND name_key=?2")?
        .query_map(params![directory,name],|r|r.get::<_,String>(0).map(ArtistId))?.collect::<rusqlite::Result<Vec<_>>>()?;
    merge_group(tx, &ids)
}

pub(crate) fn reconcile_album(tx: &Transaction<'_>, album: &str) -> Result<()> {
    if let Some((_, directory, name)) = register_album(tx, album)? {
        consolidate_context(tx, &directory, &name)?;
    }
    Ok(())
}

type Context = (String, Vec<u8>, String);
fn register_album(tx: &Transaction<'_>, album: &str) -> Result<Option<Context>> {
    let credits=tx.prepare("SELECT c.artist_id,COALESCE(c.credited_name,a.name) FROM album_artist_credit c JOIN artist a ON a.id=c.artist_id WHERE c.album_id=?1 ORDER BY c.position")?
        .query_map([album],|r|Ok((ArtistId(r.get(0)?),r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let [(artist, name)] = credits.as_slice() else {
        return Ok(None);
    };
    let name = name.trim().to_lowercase();
    if name.is_empty() {
        return Ok(None);
    }
    let locations=tx.prepare("SELECT DISTINCT l.root_id,d.location,l.path FROM release r JOIN track t ON t.release_id=r.id JOIN track_source ts ON ts.track_id=t.id JOIN local_file_observation l ON l.source_id=ts.source_id JOIN discovery_root d ON d.id=l.root_id WHERE r.album_id=?1")?
        .query_map([album],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Vec<u8>>(1)?,r.get::<_,Vec<u8>>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut contexts = BTreeSet::new();
    for (root, base, path) in locations {
        let base = bytes_to_path(base);
        let path = bytes_to_path(path);
        let Ok(relative) = path.strip_prefix(&base) else {
            return Ok(None);
        };
        let parts = relative.components().collect::<Vec<_>>();
        // Artist/Album/file evidence is the same whether the location is its
        // parent or the Artist directory itself. Require an Album component;
        // a flat folder of equal-name files is still insufficient evidence.
        let directory = if parts.len() >= 2
            && base
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.trim().to_lowercase())
                .as_ref()
                == Some(&name)
        {
            base.clone()
        } else {
            if parts.len() < 3 {
                return Ok(None);
            }
            let std::path::Component::Normal(directory) = parts[0] else {
                return Ok(None);
            };
            if directory.to_str().map(|s| s.trim().to_lowercase()).as_ref() != Some(&name) {
                return Ok(None);
            }
            base.join(directory)
        };
        contexts.insert((root, path_to_bytes(&directory)));
    }
    let Some((root, directory)) = contexts.iter().next().cloned() else {
        return Ok(None);
    };
    // Overlapping locations can observe files in the same Artist directory.
    // Root IDs differ, but the canonical directory evidence must agree.
    if contexts
        .iter()
        .any(|(_, candidate)| candidate != &directory)
    {
        return Ok(None);
    }
    let mut ids = vec![artist.clone()];
    // Within this same single-Artist Album, equivalent singleton Release/Track
    // credits describe the same Artist. Guest/multi-Artist credits stay untouched.
    for (table, key, owners) in [
        (
            "release_artist_credit",
            "release_id",
            "SELECT id FROM release WHERE album_id=?1",
        ),
        (
            "track_artist_credit",
            "track_id",
            "SELECT t.id FROM track t JOIN release r ON r.id=t.release_id WHERE r.album_id=?1",
        ),
    ] {
        let sql = format!(
            "SELECT c.artist_id,COALESCE(c.credited_name,a.name) FROM {table} c JOIN artist a ON a.id=c.artist_id WHERE c.{key} IN ({owners}) AND (SELECT count(*) FROM {table} other WHERE other.{key}=c.{key})=1"
        );
        for row in tx.prepare(&sql)?.query_map([album], |r| {
            Ok((ArtistId(r.get(0)?), r.get::<_, String>(1)?))
        })? {
            let (id, credit) = row?;
            if credit.trim().to_lowercase() == name {
                ids.push(id);
            }
        }
    }
    for id in ids {
        tx.execute("INSERT OR IGNORE INTO local_artist_context(root_id,directory,name_key,artist_id) VALUES (?1,?2,?3,?4)",params![root,directory,name,id.as_ref()])?;
    }
    Ok(Some((root, directory, name)))
}
