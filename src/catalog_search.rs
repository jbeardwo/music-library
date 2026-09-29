//! Bounded interactive catalog presentation and local identity awareness.
use crate::{Library, catalog::*, domain::ExternalIdentity, storage::Result};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    All,
    Artist,
    Album,
    Song,
}
#[derive(Clone, Debug)]
pub enum Hit {
    Artist(ArtistCandidate),
    Album(AlbumCandidate),
    Song(Box<SongCandidate>),
}
impl Hit {
    pub fn title(&self) -> &str {
        match self {
            Self::Artist(v) => &v.name,
            Self::Album(v) => &v.title,
            Self::Song(v) => &v.title,
        }
    }
    pub fn identity(&self) -> &ExternalIdentity {
        match self {
            Self::Artist(v) => &v.identity,
            Self::Album(v) => &v.identity,
            Self::Song(v) => &v.identity,
        }
    }
    pub fn kind(&self) -> Kind {
        match self {
            Self::Artist(_) => Kind::Artist,
            Self::Album(_) => Kind::Album,
            Self::Song(_) => Kind::Song,
        }
    }
}
fn normalized(value: &str) -> String {
    value
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
/// Stable ranking retains provider ordering within each textual tier.
pub fn rank(hits: &mut [Hit], query: &str) {
    let q = normalized(query);
    hits.sort_by_key(|h| {
        let t = normalized(h.title());
        let rank = if t == q {
            0
        } else if t.starts_with(&q) || t.contains(&q) {
            1
        } else if q.split_whitespace().all(|w| {
            t.split(|c: char| !c.is_alphanumeric())
                .any(|v| v.starts_with(w))
        }) {
            2
        } else {
            3
        };
        (
            match h.kind() {
                Kind::Artist => 0,
                Kind::Album => 1,
                _ => 2,
            },
            rank,
        )
    });
}
#[derive(Clone, Debug, Default)]
pub struct Context {
    pub key: Option<String>,
    pub saved: u32,
}
impl Library {
    /// At most four indexed set queries, independent of result count. Only established,
    /// unambiguous application identities can consolidate cross-provider rows.
    pub fn catalog_context(&self, hits: &[Hit]) -> Result<Vec<Context>> {
        let mut result = vec![Context::default(); hits.len()];
        for (kind, table, column, count) in [
            (Kind::Artist, "artist_external_identity", "artist_id", "0"),
            (
                Kind::Album,
                "album_external_identity",
                "album_id",
                "(SELECT count(*) FROM release r JOIN track t ON t.release_id=r.id JOIN library_membership m ON m.track_id=t.id WHERE r.album_id=e.album_id)",
            ),
            (
                Kind::Song,
                "track_external_identity",
                "track_id",
                "(SELECT count(*) FROM library_membership m WHERE m.track_id=e.track_id)",
            ),
        ] {
            let rows: Vec<_> = hits
                .iter()
                .enumerate()
                .filter(|(_, h)| h.kind() == kind)
                .map(|(i, h)| {
                    let id = h.identity();
                    serde_json::json!([i, id.provider, id.kind, id.external_id])
                })
                .collect();
            if rows.is_empty() {
                continue;
            }
            let sql = format!(
                "SELECT json_extract(j.value,'$[0]'), min(e.{column}), {count} FROM json_each(?1) j JOIN {table} e ON e.provider=json_extract(j.value,'$[1]') AND e.kind=json_extract(j.value,'$[2]') AND e.external_id=json_extract(j.value,'$[3]') GROUP BY j.key HAVING count(DISTINCT e.{column})=1"
            );
            let mut stmt = self.store.connection.prepare(&sql)?;
            for row in stmt.query_map([serde_json::Value::Array(rows).to_string()], |r| {
                Ok((
                    r.get::<_, u32>(0)? as usize,
                    r.get::<_, String>(1)?,
                    r.get::<_, u32>(2)?,
                ))
            })? {
                let (i, key, saved) = row?;
                result[i] = Context {
                    key: Some(format!("{kind:?}:{key}")),
                    saved,
                };
            }
        }
        // Recording search results are safe only when a unique local Track is
        // already established within this Album. Repeated occurrences stay distinct.
        let songs: Vec<_> = hits
            .iter()
            .enumerate()
            .filter_map(|(i, h)| match h {
                Hit::Song(s) if result[i].key.is_none() => Some(serde_json::json!([
                    i,
                    s.identity.provider,
                    s.identity.kind,
                    s.identity.external_id,
                    s.album.identity.provider,
                    s.album.identity.kind,
                    s.album.identity.external_id
                ])),
                _ => None,
            })
            .collect();
        if !songs.is_empty() {
            let mut stmt=self.store.connection.prepare("SELECT json_extract(j.value,'$[0]'),min(t.id),max(m.track_id IS NOT NULL) FROM json_each(?1) j JOIN recording_external_identity e ON e.provider=json_extract(j.value,'$[1]') AND e.kind=json_extract(j.value,'$[2]') AND e.external_id=json_extract(j.value,'$[3]') JOIN track t ON t.recording_id=e.recording_id JOIN release r ON r.id=t.release_id JOIN album_external_identity a ON a.album_id=r.album_id AND a.provider=json_extract(j.value,'$[4]') AND a.kind=json_extract(j.value,'$[5]') AND a.external_id=json_extract(j.value,'$[6]') LEFT JOIN library_membership m ON m.track_id=t.id GROUP BY j.key HAVING count(DISTINCT t.id)=1")?;
            for row in stmt.query_map([serde_json::Value::Array(songs).to_string()], |r| {
                Ok((
                    r.get::<_, u32>(0)? as usize,
                    r.get::<_, String>(1)?,
                    r.get::<_, u32>(2)?,
                ))
            })? {
                let (i, key, saved) = row?;
                result[i] = Context {
                    key: Some(format!("Song:{key}")),
                    saved,
                };
            }
        }
        Ok(result)
    }
    /// Reuse an established catalog reference in this Album when there is exactly
    /// one. This selects a known representation; it does not infer edition identity.
    pub fn catalog_existing_reference(
        &self,
        album: &AlbumCandidate,
    ) -> Result<Option<ExternalIdentity>> {
        let id = &album.identity;
        let mut stmt=self.store.connection.prepare("SELECT DISTINCT e.provider,e.kind,e.external_id FROM album_external_identity a JOIN release r ON r.album_id=a.album_id JOIN release_external_identity e ON e.release_id=r.id WHERE a.provider=?1 AND a.kind=?2 AND a.external_id=?3 AND e.provider=?1 LIMIT 2")?;
        let refs = stmt
            .query_map(
                rusqlite::params![id.provider, id.kind, id.external_id],
                |r| {
                    Ok(ExternalIdentity {
                        provider: r.get(0)?,
                        kind: r.get(1)?,
                        external_id: r.get(2)?,
                    })
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(if refs.len() == 1 {
            refs.into_iter().next()
        } else {
            None
        })
    }

    /// Membership for a preview uses exact edition positions or established Track
    /// identities within an established Album. Titles never establish equivalence.
    pub fn catalog_saved_positions(&self, release: &Release) -> Result<Vec<(u32, u32)>> {
        let rows: Vec<_> = release
            .media
            .iter()
            .flat_map(|m| {
                m.tracks
                    .iter()
                    .map(move |t| serde_json::json!([m.position, t.position, t.identities]))
            })
            .collect();
        let r = &release.identity;
        let a = &release.album.identity;
        let mut stmt=self.store.connection.prepare("WITH wanted AS (SELECT json_extract(value,'$[0]') disc,json_extract(value,'$[1]') number,json_extract(value,'$[2]') ids FROM json_each(?1)) SELECT DISTINCT w.disc,w.number FROM wanted w WHERE EXISTS (SELECT 1 FROM release_external_identity e JOIN track t ON t.release_id=e.release_id JOIN library_membership m ON m.track_id=t.id WHERE e.provider=?2 AND e.kind=?3 AND e.external_id=?4 AND t.disc_number=w.disc AND t.track_number=w.number) OR EXISTS (SELECT 1 FROM json_each(w.ids) i JOIN track_external_identity e ON e.provider=json_extract(i.value,'$.provider') AND e.kind=json_extract(i.value,'$.kind') AND e.external_id=json_extract(i.value,'$.external_id') JOIN track t ON t.id=e.track_id JOIN library_membership m ON m.track_id=t.id JOIN release r ON r.id=t.release_id JOIN album_external_identity a ON a.album_id=r.album_id WHERE a.provider=?5 AND a.kind=?6 AND a.external_id=?7)")?;
        stmt.query_map(
            rusqlite::params![
                serde_json::Value::Array(rows).to_string(),
                r.provider,
                r.kind,
                r.external_id,
                a.provider,
                a.kind,
                a.external_id
            ],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn artist(name: &str) -> Hit {
        Hit::Artist(ArtistCandidate {
            identity: ExternalIdentity {
                provider: "test".into(),
                kind: "artist".into(),
                external_id: name.into(),
            },
            name: name.into(),
            aliases: vec![],
            comment: String::new(),
            country: String::new(),
            artist_type: String::new(),
            score: None,
        })
    }
    #[test]
    fn ranking_is_stable_and_uses_exact_phrase_token_then_provider_order() {
        let mut hits = vec![
            artist("Unrelated one"),
            artist("Along with Hop"),
            artist("Hop Along live"),
            artist("Hop Along!"),
            artist("Unrelated two"),
        ];
        rank(&mut hits, "hop along");
        assert_eq!(
            hits.iter().map(Hit::title).collect::<Vec<_>>(),
            vec![
                "Hop Along!",
                "Hop Along live",
                "Along with Hop",
                "Unrelated one",
                "Unrelated two"
            ]
        );
    }
}
