//! Bounded, provider-neutral library panes. Only saved Tracks contribute rows.
use crate::{Library, Result, domain::*, storage::Store};
use rusqlite::params;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pane {
    Artists,
    Albums,
    #[default]
    Songs,
}

#[derive(Clone, Debug, Default)]
pub struct Request {
    pub pane: Pane,
    pub artist: Option<ArtistId>,
    pub album: Option<AlbumId>,
    pub track: Option<TrackId>,
    pub after: Option<Cursor>,
    pub limit: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    pub title: String,
    pub release: String,
    pub disc: i64,
    pub position: i64,
    pub id: String,
}

#[derive(Clone, Debug)]
pub struct Row {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub cursor: Cursor,
    pub track: Option<TrackSearchResult>,
}

/// A read-only connection for preparing an explicit queue away from the UI thread.
/// It observes library membership when `read` runs and never performs migrations.
pub struct QueueReader(rusqlite::Connection);
impl QueueReader {
    pub fn read(self, request: &Request) -> Result<Vec<TrackSearchResult>> {
        let request = Request {
            pane: Pane::Songs,
            after: None,
            ..request.clone()
        };
        Ok(query(&self.0, &request, true, false, false)?
            .into_iter()
            .filter_map(|r| r.track)
            .collect())
    }
}

impl Library {
    pub fn library_queue_reader(&self) -> Result<QueueReader> {
        let path = self
            .store
            .connection
            .path()
            .filter(|p| !p.is_empty())
            .ok_or_else(|| {
                crate::storage::Error::Invalid(
                    "Background queue reading requires a file-backed library".into(),
                )
            })?;
        let connection = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        connection.busy_timeout(std::time::Duration::from_secs(2))?;
        Ok(QueueReader(connection))
    }

    /// Seek directly to an identity's ordering key, including the target row.
    fn browse_cursor(&self, request: &Request, id: &str) -> Result<Cursor> {
        let sql = match request.pane {
            Pane::Artists => "SELECT lower(name),'',0,0,id FROM artist WHERE id=?1",
            Pane::Albums => {
                "SELECT lower(title),'',0,0,album_id FROM album_application_metadata WHERE album_id=?1"
            }
            Pane::Songs if request.album.is_some() => {
                "SELECT lower(a.title),r.id,COALESCE(t.disc_number,1),COALESCE(t.track_number,2147483647),t.id FROM track t JOIN release r ON r.id=t.release_id JOIN album_application_metadata a ON a.album_id=r.album_id WHERE t.id=?1"
            }
            Pane::Songs => {
                "SELECT lower(title),'',0,0,track_id FROM effective_track_metadata WHERE track_id=?1"
            }
        };
        let cursor = self.store.connection.query_row(sql, [id], |r| {
            Ok(Cursor {
                title: r.get(0)?,
                release: r.get(1)?,
                disc: r.get(2)?,
                position: r.get(3)?,
                id: r.get(4)?,
            })
        })?;
        Ok(cursor)
    }

    pub fn browse_from(&self, request: &Request, id: &str) -> Result<Vec<Row>> {
        let request = Request {
            after: Some(self.browse_cursor(request, id)?),
            ..request.clone()
        };
        let rows = query(&self.store.connection, &request, false, true, false)?;
        if rows.first().is_none_or(|r| r.id != id) {
            return Err(crate::storage::Error::Invalid(
                "Search result is no longer in this library view".into(),
            ));
        }
        Ok(rows)
    }

    /// Bounded keyset window around a target, without counting or walking pages.
    pub fn browse_around(&self, request: &Request, id: &str) -> Result<Vec<Row>> {
        let cursor = self.browse_cursor(request, id)?;
        let before = Request {
            after: Some(cursor.clone()),
            limit: request.limit.clamp(1, 201).saturating_sub(1).min(100),
            ..request.clone()
        };
        let preceding = if before.limit == 0 {
            vec![]
        } else {
            query(&self.store.connection, &before, false, false, true)?
        };
        let first = preceding.last().map(|r| r.cursor.clone()).unwrap_or(cursor);
        let request = Request {
            after: Some(first),
            ..request.clone()
        };
        let rows = query(&self.store.connection, &request, false, true, false)?;
        if !rows.iter().any(|r| r.id == id) {
            return Err(crate::storage::Error::Invalid(
                "Search result is no longer in this library view".into(),
            ));
        }
        Ok(rows)
    }

    pub fn browse(&self, request: &Request) -> Result<Vec<Row>> {
        self.store.browse(request, false)
    }

    /// Read the complete saved Track set for an explicit queue operation in one
    /// query. Metadata is retained by the player, never expanded wholesale in QML.
    pub fn library_queue(&self, request: &Request) -> Result<Vec<TrackSearchResult>> {
        let request = Request {
            pane: Pane::Songs,
            after: None,
            ..request.clone()
        };
        Ok(self
            .store
            .browse(&request, true)?
            .into_iter()
            .filter_map(|r| r.track)
            .collect())
    }
}

impl Store {
    pub(crate) fn browse(&self, request: &Request, queue: bool) -> Result<Vec<Row>> {
        query(&self.connection, request, queue, false, false)
    }
}

fn query(
    connection: &rusqlite::Connection,
    request: &Request,
    queue: bool,
    inclusive: bool,
    reverse: bool,
) -> Result<Vec<Row>> {
    // Credit relationships are indexed. UNION prevents duplicate Tracks when
    // an Artist is credited at several levels. No filesystem reads or HTTP here.
    let artist_tracks = "SELECT track_id FROM track_artist_credit WHERE artist_id=?1
            UNION SELECT t.id FROM album_artist_credit c JOIN release r ON r.album_id=c.album_id
                JOIN track t ON t.release_id=r.id WHERE c.artist_id=?1
            UNION SELECT t.id FROM release_artist_credit c JOIN track t ON t.release_id=c.release_id WHERE c.artist_id=?1";
    let scope = match request.pane {
        Pane::Artists => String::new(),
        _ if request.artist.is_none() => String::new(),
        _ => format!(" AND t.id IN ({artist_tracks})"),
    };
    let album_scope = if request.artist.is_some() {
        format!(
            " AND a.album_id IN (SELECT r.album_id FROM track t JOIN release r ON r.id=t.release_id JOIN library_membership lm ON lm.track_id=t.id WHERE t.id IN ({artist_tracks}))"
        )
    } else {
        String::new()
    };
    // Display fallback only: some imported files have Album credits but no
    // Track credits. Preserve the effective Track value whenever present.
    let album_credit = "COALESCE((SELECT group_concat(name, '') FROM (SELECT COALESCE(c.credited_name, ar.name) || COALESCE(c.join_phrase, CASE WHEN EXISTS(SELECT 1 FROM album_artist_credit next WHERE next.album_id=c.album_id AND next.position>c.position) THEN ' / ' ELSE '' END) AS name FROM album_artist_credit c JOIN artist ar ON ar.id=c.artist_id WHERE c.album_id=a.album_id ORDER BY c.position)), '')";
    let track_credit = format!("COALESCE(NULLIF(e.artist_names, ''), {album_credit})");
    let saved = "JOIN library_membership lm ON lm.track_id=t.id";
    let sql = match request.pane {
            Pane::Artists => format!("SELECT a.id, a.name, '', lower(a.name), '', 0, 0, '', '', '', NULL
                FROM artist a WHERE (EXISTS(SELECT 1 FROM track_artist_credit c JOIN track t ON t.id=c.track_id {saved} WHERE c.artist_id=a.id)
                OR EXISTS(SELECT 1 FROM album_artist_credit c JOIN release r ON r.album_id=c.album_id JOIN track t ON t.release_id=r.id {saved} WHERE c.artist_id=a.id)
                OR EXISTS(SELECT 1 FROM release_artist_credit c JOIN track t ON t.release_id=c.release_id {saved} WHERE c.artist_id=a.id))"),
            Pane::Albums => format!("SELECT a.album_id, a.title,
                {album_credit},
                lower(a.title), '', 0, 0, '', '', '', NULL
                FROM album_application_metadata a WHERE EXISTS(SELECT 1 FROM release r JOIN track t ON t.release_id=r.id {saved} WHERE r.album_id=a.album_id) {album_scope}"),
            Pane::Songs => {
                let (title, release, disc, position) = if request.album.is_some() {
                    ("lower(a.title)", "r.id", "COALESCE(t.disc_number, 1)", "COALESCE(t.track_number, 2147483647)")
                } else { ("lower(e.title)", "''", "0", "0") };
                let album_filter = if request.album.is_some() { " AND r.album_id=?2" } else { "" };
                let track_filter = if request.track.is_some() { " AND t.id=?3" } else { "" };
                let from = if request.album.is_some() {
                    "release r CROSS JOIN track t ON t.release_id=r.id CROSS JOIN effective_track_metadata e ON e.track_id=t.id"
                } else if request.artist.is_some() || request.track.is_some() {
                    "track t CROSS JOIN effective_track_metadata e ON e.track_id=t.id JOIN release r ON r.id=t.release_id"
                } else {
                    "effective_track_metadata e JOIN track t ON t.id=e.track_id JOIN release r ON r.id=t.release_id"
                };
                format!("SELECT t.id, e.title, {track_credit}, {title}, {release}, {disc}, {position}, t.release_id, a.title, {track_credit}, e.year
                    FROM {from} JOIN album_application_metadata a ON a.album_id=r.album_id {saved}
                    WHERE 1=1 {album_filter} {track_filter} {scope}")
            }
        };
    let c = request.after.clone().unwrap_or_default();
    let program = request.pane == Pane::Songs && request.album.is_some();
    let order = if program {
        "sort_title,release_key,disc,position,id"
    } else {
        "sort_title,id"
    };
    let cursor = if request.after.is_none() {
        ""
    } else if program {
        "WHERE sort_title >= ?5 AND (sort_title,release_key,disc,position,id) > (?5,?6,?7,?8,?9)"
    } else {
        "WHERE sort_title >= ?5 AND (sort_title,id) > (?5,?9)"
    };
    let cursor = if inclusive {
        cursor.replace(" > ", " >= ")
    } else {
        cursor.to_string()
    };
    let cursor = if reverse {
        cursor.replace(" >= ", " <= ").replace(" > ", " < ")
    } else {
        cursor
    };
    let order = if reverse {
        order
            .split(',')
            .map(|key| format!("{key} DESC"))
            .collect::<Vec<_>>()
            .join(",")
    } else {
        order.to_string()
    };
    let available = if request.pane == Pane::Songs {
        "EXISTS(SELECT 1 FROM track_source ts CROSS JOIN local_file_observation l WHERE ts.track_id=rows.id AND l.source_id=ts.source_id AND l.available=1)"
    } else {
        "0"
    };
    let sql = format!("WITH rows(id,title,subtitle,sort_title,release_key,disc,position,release_id,album_title,artist,year) AS ({sql})
            SELECT *, {available} FROM rows {cursor} ORDER BY {order} LIMIT ?10");
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params![
            request.artist.as_ref().map(AsRef::as_ref),
            request.album.as_ref().map(AsRef::as_ref),
            request.track.as_ref().map(AsRef::as_ref),
            request.after.is_some(),
            c.title,
            c.release,
            c.disc,
            c.position,
            c.id,
            if queue {
                -1_i64
            } else {
                i64::from(request.limit.clamp(1, 201))
            }
        ],
        |r| {
            let id: String = r.get(0)?;
            let title: String = r.get(1)?;
            Ok(Row {
                id: id.clone(),
                title: title.clone(),
                subtitle: r.get(2)?,
                cursor: Cursor {
                    title: r.get(3)?,
                    release: r.get(4)?,
                    disc: r.get(5)?,
                    position: r.get(6)?,
                    id: id.clone(),
                },
                track: if request.pane == Pane::Songs {
                    Some(TrackSearchResult {
                        track_id: TrackId(id),
                        release_id: ReleaseId(r.get(7)?),
                        title,
                        release_title: r.get(8)?,
                        artist_names: r.get(9)?,
                        year: r.get(10)?,
                        available: r.get(11)?,
                    })
                } else {
                    None
                },
            })
        },
    )?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}
