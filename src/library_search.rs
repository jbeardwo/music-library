//! Local, membership-derived retrieval. No provider or playback dependency.
use crate::{
    Library,
    domain::*,
    storage::{Error, Result},
};
use rusqlite::{Connection, named_params};

pub const SECTION_LIMIT: usize = 40;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    All,
    Artist,
    Album,
    Song,
    Playlist,
}
#[derive(Clone, Debug)]
pub struct Hit {
    pub kind: Kind,
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artist_id: Option<ArtistId>,
    pub album_id: Option<AlbumId>,
    pub score: u32,
}
pub struct Reader(Connection);
impl Reader {
    pub fn search(&self, text: &str, kind: Kind) -> Result<Vec<Hit>> {
        search(&self.0, text, kind)
    }
}
impl Library {
    pub fn local_search(&self, text: &str, kind: Kind) -> Result<Vec<Hit>> {
        search(&self.store.connection, text, kind)
    }
    pub fn local_search_reader(&self) -> Result<Reader> {
        let path = self
            .store
            .connection
            .path()
            .filter(|p| !p.is_empty())
            .ok_or_else(|| Error::Invalid("Search requires a file-backed library".into()))?;
        let c = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        c.busy_timeout(std::time::Duration::from_secs(2))?;
        Ok(Reader(c))
    }
}

fn search(c: &Connection, text: &str, kind: Kind) -> Result<Vec<Hit>> {
    // Explicit quoted tokens prevent FTS operators from becoming user syntax.
    let tokens: Vec<_> = text
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .take(12)
        .collect();
    if tokens.is_empty() || kind == Kind::Playlist {
        return Ok(vec![]);
    }
    let q = tokens
        .iter()
        .map(|s| format!(r#""{s}"*"#))
        .collect::<Vec<_>>()
        .join(" AND ");
    let title_q = format!("title : ({q})");
    let normalized = text.trim().to_lowercase();
    let common = r#"
WITH
am AS MATERIALIZED (
 SELECT a.id,a.name,CASE WHEN lower(a.name)=:text THEN 0 WHEN substr(lower(a.name),1,length(:text))=:text THEN 1 ELSE 2 END AS score
 FROM artist_lookup f CROSS JOIN artist a ON a.rowid=f.rowid WHERE artist_lookup MATCH :query AND :title_query IS NOT NULL
 AND (EXISTS(SELECT 1 FROM album_artist_credit ac CROSS JOIN release r ON r.album_id=ac.album_id CROSS JOIN track t ON t.release_id=r.id CROSS JOIN library_membership lm ON lm.track_id=t.id WHERE ac.artist_id=a.id)
 OR EXISTS(SELECT 1 FROM release_artist_credit rc CROSS JOIN track t ON t.release_id=rc.release_id CROSS JOIN library_membership lm ON lm.track_id=t.id WHERE rc.artist_id=a.id)
 OR EXISTS(SELECT 1 FROM track_artist_credit tc CROSS JOIN library_membership lm ON lm.track_id=tc.track_id WHERE tc.artist_id=a.id))
 ORDER BY score,lower(a.name),a.id LIMIT 40
),
bm AS MATERIALIZED (
 SELECT a.album_id AS id,a.title,CASE WHEN lower(a.title)=:text THEN 0 WHEN substr(lower(a.title),1,length(:text))=:text THEN 1 ELSE 2 END AS score
 FROM album_lookup f CROSS JOIN album_application_metadata a ON a.rowid=f.rowid WHERE album_lookup MATCH :query
 AND EXISTS(SELECT 1 FROM release r CROSS JOIN track t ON t.release_id=r.id CROSS JOIN library_membership lm ON lm.track_id=t.id WHERE r.album_id=a.album_id)
 ORDER BY score,lower(a.title),a.album_id LIMIT 40
),
ax AS MATERIALIZED (SELECT * FROM am LIMIT 16),
bx AS MATERIALIZED (SELECT * FROM bm LIMIT 16),
artist_tracks AS MATERIALIZED (
 SELECT tc.track_id AS id FROM ax am CROSS JOIN track_artist_credit tc ON tc.artist_id=am.id CROSS JOIN library_membership lm ON lm.track_id=tc.track_id
 UNION SELECT t.id FROM ax am CROSS JOIN album_artist_credit ac ON ac.artist_id=am.id CROSS JOIN release r ON r.album_id=ac.album_id CROSS JOIN track t ON t.release_id=r.id CROSS JOIN library_membership lm ON lm.track_id=t.id
 UNION SELECT t.id FROM ax am CROSS JOIN release_artist_credit rc ON rc.artist_id=am.id CROSS JOIN track t ON t.release_id=rc.release_id CROSS JOIN library_membership lm ON lm.track_id=t.id
)
"#;
    let mut output = Vec::new();
    for (section, from, title, album, artist) in [
        (
            Kind::Artist,
            "artist a ON a.id=h.id",
            "a.name",
            "NULL",
            "a.id",
        ),
        (
            Kind::Album,
            "album_application_metadata a ON a.album_id=h.id",
            "a.title",
            "a.album_id",
            "NULL",
        ),
        (
            Kind::Song,
            "track t ON t.id=h.id JOIN effective_track_metadata e ON e.track_id=t.id JOIN release r ON r.id=t.release_id",
            "e.title",
            "r.album_id",
            "NULL",
        ),
    ] {
        if kind != Kind::All && kind != section {
            continue;
        }
        let candidates = match section {
            Kind::Artist => "SELECT id,score FROM am",
            Kind::Album => {
                "SELECT id,score FROM bm UNION ALL SELECT r.album_id,4 FROM artist_tracks at CROSS JOIN track t ON t.id=at.id CROSS JOIN release r ON r.id=t.release_id"
            }
            _ => {
                "SELECT e.track_id AS id,CASE WHEN lower(e.title)=:text THEN 0 WHEN substr(lower(e.title),1,length(:text))=:text THEN 1 ELSE 2 END AS score FROM track_search f CROSS JOIN effective_track_metadata e ON e.rowid=f.rowid CROSS JOIN library_membership lm ON lm.track_id=e.track_id WHERE track_search MATCH :title_query UNION ALL SELECT id,4 FROM artist_tracks UNION ALL SELECT t.id,4 FROM bx bm CROSS JOIN release r ON r.album_id=bm.id CROSS JOIN track t ON t.release_id=r.id CROSS JOIN library_membership lm ON lm.track_id=t.id"
            }
        };
        let display_credit = if section == Kind::Song {
            "e.artist_names"
        } else {
            "''"
        };
        let sql = format!(
            "{common}, candidates AS ({candidates}), ranked AS (SELECT id,min(score) AS score FROM candidates GROUP BY id) SELECT h.id,{title},{album},{artist},h.score,{display_credit} FROM ranked h CROSS JOIN {from} ORDER BY h.score,lower({title}),h.id LIMIT {SECTION_LIMIT}"
        );
        let mut stmt = c.prepare(&sql)?;
        let rows = stmt.query_map(
            named_params! {":text":normalized,":query":q,":title_query":title_q},
            |r| {
                Ok(Hit {
                    kind: section,
                    id: r.get(0)?,
                    title: r.get(1)?,
                    album_id: r.get::<_, Option<String>>(2)?.map(AlbumId),
                    artist_id: r.get::<_, Option<String>>(3)?.map(ArtistId),
                    score: r.get(4)?,
                    artist: r.get(5)?,
                    album: String::new(),
                })
            },
        )?;
        output.extend(rows.collect::<rusqlite::Result<Vec<_>>>()?);
    }
    // One bounded set query supplies presentation and navigation credits, never
    // a query per result. Prefer the Album's established credit for navigation.
    if !output.is_empty() {
        let ids = output
            .iter()
            .filter_map(|h| h.album_id.as_ref().map(|a| a.as_ref()))
            .collect::<std::collections::BTreeSet<_>>();
        if !ids.is_empty() {
            let placeholders = (1..=ids.len())
                .map(|n| format!("?{n}"))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                r#"WITH credits AS (
              SELECT ac.album_id,ac.artist_id,ac.position,COALESCE(ac.credited_name,a.name) AS name,ac.join_phrase,0 AS priority FROM album_artist_credit ac JOIN artist a ON a.id=ac.artist_id WHERE ac.album_id IN ({placeholders})
              UNION ALL SELECT r.album_id,rc.artist_id,rc.position,COALESCE(rc.credited_name,a.name),rc.join_phrase,1 FROM release_artist_credit rc JOIN release r ON r.id=rc.release_id JOIN artist a ON a.id=rc.artist_id WHERE r.album_id IN ({placeholders}) AND NOT EXISTS(SELECT 1 FROM album_artist_credit ac WHERE ac.album_id=r.album_id) AND EXISTS(SELECT 1 FROM track st JOIN library_membership sl ON sl.track_id=st.id WHERE st.release_id=r.id)
              UNION ALL SELECT r.album_id,tc.artist_id,tc.position,COALESCE(tc.credited_name,a.name),tc.join_phrase,2 FROM track_artist_credit tc JOIN track t ON t.id=tc.track_id JOIN release r ON r.id=t.release_id JOIN artist a ON a.id=tc.artist_id JOIN library_membership lm ON lm.track_id=t.id WHERE r.album_id IN ({placeholders}) AND NOT EXISTS(SELECT 1 FROM album_artist_credit ac WHERE ac.album_id=r.album_id) AND NOT EXISTS(SELECT 1 FROM release_artist_credit rc JOIN release rr ON rr.id=rc.release_id JOIN track rt ON rt.release_id=rr.id JOIN library_membership rm ON rm.track_id=rt.id WHERE rr.album_id=r.album_id)
            ) SELECT m.album_id,m.title,c.artist_id,c.name,c.priority,c.position,c.join_phrase FROM album_application_metadata m LEFT JOIN credits c ON c.album_id=m.album_id WHERE m.album_id IN ({placeholders}) ORDER BY m.album_id,c.priority,c.position,c.artist_id"#
            );
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(ids), |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<i32>>(4)?.unwrap_or(3),
                    r.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    r.get::<_, Option<String>>(6)?,
                ))
            })?;
            let mut context = std::collections::HashMap::new();
            for row in rows {
                let (id, title, artist, name, priority, position, join) = row?;
                let entry = context.entry(id).or_insert_with(|| Context {
                    title,
                    artist,
                    name: String::new(),
                    priority,
                    position: -1,
                    join: None,
                });
                if priority == entry.priority && position > entry.position {
                    if !entry.name.is_empty() {
                        entry.name.push_str(entry.join.as_deref().unwrap_or(" / "));
                    }
                    entry.name.push_str(name.as_deref().unwrap_or(""));
                    entry.position = position;
                    entry.join = join;
                }
            }
            for h in &mut output {
                if let Some(context) = h.album_id.as_ref().and_then(|a| context.get(a.as_ref())) {
                    h.album = context.title.clone();
                    if h.artist.is_empty() {
                        h.artist = context.name.clone();
                    }
                    h.artist_id = context.artist.clone().map(ArtistId);
                }
            }
        }
    }
    Ok(output)
}

struct Context {
    title: String,
    artist: Option<String>,
    name: String,
    priority: i32,
    position: i64,
    join: Option<String>,
}
