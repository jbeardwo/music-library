//! Bounded, provider-neutral library panes. Only saved Tracks contribute rows.
use crate::{Library, Result, domain::*, storage::Store};
use rusqlite::params;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pane {
    Artists,
    Genres,
    Albums,
    #[default]
    Songs,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    #[default]
    Default,
    Title,
    Descending,
    Year,
    Artist,
    Album,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SongColumn {
    #[default]
    Song,
    Artist,
    Album,
    Genre,
    Reason,
}
#[derive(Clone, Debug, Default)]
pub struct Request {
    /// Explicit Spotify identity review; never performs provider work.
    pub unresolved_spotify: bool,
    pub marked_spotify: bool,
    pub song_column: Option<SongColumn>,
    pub descending: bool,
    pub pane: Pane,
    pub sort: Sort,
    /// Ordering of Album groups when Songs use Album mode. Default is newest first.
    pub album_sort: Sort,
    pub artist: Option<ArtistId>,
    pub genre: Option<String>,
    pub album: Option<AlbumId>,
    pub track: Option<TrackId>,
    /// OR within a filter; different filter kinds intersect. Empty means unrestricted.
    pub artists: Vec<String>,
    pub genres: Vec<String>,
    pub albums: Vec<String>,
    pub tracks: Vec<String>,
    /// Restrict returned row identities (used to prune selection without walking pages).
    pub ids: Vec<String>,
    pub after: Option<Cursor>,
    pub limit: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    pub title: String,
    pub release: String,
    pub album_key: String,
    pub edition: String,
    pub disc: i64,
    pub position: i64,
    pub id: String,
}

#[derive(Clone, Debug)]
pub struct Row {
    pub connection_reason: String,
    pub connection_reason_code: String,
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub year: Option<i32>,
    pub group: String,
    pub group_label: String,
    pub cursor: Cursor,
    pub track: Option<TrackSearchResult>,
    /// Absolute 1-based ordinal in a playlist, independent of cursor gaps and Track ordering.
    pub playlist_position: Option<u64>,
    pub duration_ms: Option<u64>,
    pub duration_approximate: bool,
    pub genres: String,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub multi_disc: bool,
}

/// A read-only connection for preparing an explicit queue away from the UI thread.
/// It observes library membership when `read` runs and never performs migrations.
pub struct QueueReader(pub(crate) rusqlite::Connection);
impl QueueReader {
    pub fn marked_spotify_count(&self) -> Result<u64> {
        Ok(self.0.query_row("SELECT count(*) FROM track_provider_exclusion x INDEXED BY provider_exclusion_list JOIN library_membership m ON m.track_id=x.track_id WHERE x.provider='spotify'",[],|r|r.get::<_,i64>(0))? as u64)
    }
    pub fn unresolved_spotify_count(&self) -> Result<u64> {
        Ok(self.0.query_row("SELECT (SELECT count(*) FROM library_membership) - (SELECT count(DISTINCT s.track_id) FROM trusted_spotify_track s JOIN library_membership lm ON lm.track_id=s.track_id) - (SELECT count(*) FROM track_provider_exclusion x JOIN library_membership m ON m.track_id=x.track_id WHERE x.provider='spotify')", [], |r| r.get::<_, i64>(0))? as u64)
    }

    pub fn read(self, request: &Request) -> Result<Vec<TrackSearchResult>> {
        self.read_request(request)
    }
    pub(crate) fn read_request(&self, request: &Request) -> Result<Vec<TrackSearchResult>> {
        let request = Request {
            pane: Pane::Songs,
            after: None,
            ..request.clone()
        };
        Ok(query(&self.0, &request, true, false, false, None)?
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
        let request = Request {
            after: None,
            ..request.clone()
        };
        let cursor = query(
            &self.store.connection,
            &request,
            false,
            false,
            false,
            Some(id),
        )?
        .into_iter()
        .next()
        .ok_or_else(|| crate::storage::Error::Invalid("Target is no longer saved".into()))?
        .cursor;
        Ok(cursor)
    }

    pub fn browse_from(&self, request: &Request, id: &str) -> Result<Vec<Row>> {
        let request = Request {
            after: Some(self.browse_cursor(request, id)?),
            ..request.clone()
        };
        let rows = query(&self.store.connection, &request, false, true, false, None)?;
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
            query(&self.store.connection, &before, false, false, true, None)?
        };
        let first = preceding.last().map(|r| r.cursor.clone()).unwrap_or(cursor);
        let request = Request {
            after: Some(first),
            ..request.clone()
        };
        let rows = query(&self.store.connection, &request, false, true, false, None)?;
        if !rows.iter().any(|r| r.id == id) {
            return Err(crate::storage::Error::Invalid(
                "Search result is no longer in this library view".into(),
            ));
        }
        Ok(rows)
    }

    /// Fetch predecessors nearest-first for a bounded scrolling window.
    pub fn browse_before(&self, request: &Request) -> Result<Vec<Row>> {
        query(&self.store.connection, request, false, false, true, None)
    }

    pub fn unresolved_spotify_count(&self) -> Result<u64> {
        Ok(self.store.connection.query_row("SELECT (SELECT count(*) FROM library_membership) - (SELECT count(DISTINCT s.track_id) FROM trusted_spotify_track s JOIN library_membership lm ON lm.track_id=s.track_id) - (SELECT count(*) FROM track_provider_exclusion x JOIN library_membership m ON m.track_id=x.track_id WHERE x.provider='spotify')", [], |r| r.get::<_,i64>(0))? as u64)
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
        query(&self.connection, request, queue, false, false, None)
    }
}

fn query(
    connection: &rusqlite::Connection,
    request: &Request,
    queue: bool,
    inclusive: bool,
    reverse: bool,
    target: Option<&str>,
) -> Result<Vec<Row>> {
    query_projection(
        connection, request, queue, inclusive, reverse, target, false,
    )
}
fn query_projection(
    connection: &rusqlite::Connection,
    request: &Request,
    queue: bool,
    inclusive: bool,
    reverse: bool,
    target: Option<&str>,
    ids_only: bool,
) -> Result<Vec<Row>> {
    let mut normalized = request.clone();
    for (values, single) in [(&mut normalized.artists, &mut normalized.artist)] {
        if values.len() == 1 {
            *single = Some(ArtistId(values.remove(0)));
        }
    }
    if normalized.genres.len() == 1 {
        normalized.genre = normalized.genres.pop();
    }
    if normalized.albums.len() == 1 {
        normalized.album = normalized.albums.pop().map(AlbumId);
    }
    if normalized.tracks.len() == 1 {
        normalized.track = normalized.tracks.pop().map(TrackId);
    }
    let request = &normalized;
    // Credit relationships are indexed. UNION prevents duplicate Tracks when
    // an Artist is credited at several levels. No filesystem reads or HTTP here.
    let artist_tracks = "SELECT track_id FROM track_artist_credit WHERE artist_id=?1
            UNION SELECT t.id FROM album_artist_credit c JOIN release r ON r.album_id=c.album_id
                JOIN track t ON t.release_id=r.id WHERE c.artist_id=?1
            UNION SELECT t.id FROM release_artist_credit c JOIN track t ON t.release_id=c.release_id WHERE c.artist_id=?1";
    // Small Artists are cheaper to gather by credit; large Artists are cheaper
    // to stream in indexed display order. The probe stops after 1,001 credits.
    let large_artist = if let Some(artist) = &request.artist {
        connection.query_row("SELECT count(*)>1000 FROM (SELECT 1 FROM track_artist_credit WHERE artist_id=?1 UNION ALL SELECT 1 FROM album_artist_credit WHERE artist_id=?1 UNION ALL SELECT 1 FROM release_artist_credit WHERE artist_id=?1 LIMIT 1001)", [artist.as_ref()], |r|r.get::<_,bool>(0))?
    } else if !request.artists.is_empty() {
        connection.query_row("SELECT count(*)>1000 FROM (SELECT 1 FROM track_artist_credit WHERE artist_id IN (SELECT value FROM json_each(?1)) UNION ALL SELECT 1 FROM album_artist_credit WHERE artist_id IN (SELECT value FROM json_each(?1)) UNION ALL SELECT 1 FROM release_artist_credit WHERE artist_id IN (SELECT value FROM json_each(?1)) LIMIT 1001)",[json_ids(&request.artists)],|r|r.get::<_,bool>(0))?
    } else {
        false
    };
    // Gather small Genres; stream large Genres in indexed display order.
    let large_genre = if let Some(genre) = &request.genre {
        connection.query_row("SELECT count(*)>1000 FROM (SELECT 1 FROM file_genre_observation WHERE genre=?1 LIMIT 1001)", [genre], |r| r.get::<_, bool>(0))?
    } else if !request.genres.is_empty() {
        connection.query_row("SELECT count(*)>1000 FROM (SELECT 1 FROM file_genre_observation WHERE genre IN (SELECT value FROM json_each(?1)) LIMIT 1001)",[json_ids(&request.genres)],|r|r.get::<_,bool>(0))?
    } else {
        false
    };
    let stream_titles = request.pane == Pane::Songs
        && (large_artist || large_genre)
        && request.album.is_none()
        && request.albums.is_empty()
        && !matches!(request.sort, Sort::Album)
        && !queue;
    let mut scope = match request.pane {
        Pane::Artists => String::new(),
        _ if request.artist.is_none() => String::new(),
        _ if stream_titles || target.is_some() => " AND (EXISTS(SELECT 1 FROM track_artist_credit c WHERE c.track_id=t.id AND c.artist_id=?1) OR EXISTS(SELECT 1 FROM album_artist_credit c WHERE c.album_id=r.album_id AND c.artist_id=?1) OR EXISTS(SELECT 1 FROM release_artist_credit c WHERE c.release_id=r.id AND c.artist_id=?1))".into(),
        _ => format!(" AND t.id IN ({artist_tracks})"),
    };
    let mut album_scope = if request.artist.is_some() && !large_artist && target.is_none() {
        format!(
            " AND a.album_id IN (SELECT r.album_id FROM track t JOIN release r ON r.id=t.release_id JOIN library_membership lm ON lm.track_id=t.id WHERE t.id IN ({artist_tracks}))"
        )
    } else if request.artist.is_some() {
        " AND (EXISTS(SELECT 1 FROM album_artist_credit c WHERE c.album_id=a.album_id AND c.artist_id=?1)
        OR EXISTS(SELECT 1 FROM release r CROSS JOIN release_artist_credit c ON c.release_id=r.id CROSS JOIN track t ON t.release_id=r.id JOIN library_membership lm ON lm.track_id=t.id WHERE r.album_id=a.album_id AND c.artist_id=?1)
        OR EXISTS(SELECT 1 FROM release r CROSS JOIN track t ON t.release_id=r.id CROSS JOIN library_membership lm ON lm.track_id=t.id CROSS JOIN track_artist_credit c ON c.track_id=t.id WHERE r.album_id=a.album_id AND c.artist_id=?1))".into()
    } else {
        String::new()
    };
    if request.genre.is_some() {
        if large_genre {
            scope.push_str(" AND EXISTS(SELECT 1 FROM track_source gs CROSS JOIN file_genre_observation g ON g.source_id=gs.source_id WHERE gs.track_id=t.id AND g.genre=?14)");
            album_scope.push_str(" AND EXISTS(SELECT 1 FROM release gr CROSS JOIN track gt ON gt.release_id=gr.id CROSS JOIN library_membership gl ON gl.track_id=gt.id CROSS JOIN track_source gs ON gs.track_id=gt.id CROSS JOIN file_genre_observation g ON g.source_id=gs.source_id WHERE gr.album_id=a.album_id AND g.genre=?14)");
        } else {
            scope.push_str(" AND t.id IN (SELECT gs.track_id FROM file_genre_observation g JOIN track_source gs ON gs.source_id=g.source_id WHERE g.genre=?14)");
            album_scope.push_str(" AND a.album_id IN (SELECT gr.album_id FROM file_genre_observation g JOIN track_source gs ON gs.source_id=g.source_id JOIN library_membership gl ON gl.track_id=gs.track_id JOIN track gt ON gt.id=gs.track_id JOIN release gr ON gr.id=gt.release_id WHERE g.genre=?14)");
        }
    }
    if !request.artists.is_empty() {
        let selected = "SELECT value FROM json_each(?15)";
        let tracks = format!(
            "SELECT track_id FROM track_artist_credit WHERE artist_id IN ({selected}) UNION SELECT t.id FROM album_artist_credit c JOIN release r ON r.album_id=c.album_id JOIN track t ON t.release_id=r.id WHERE c.artist_id IN ({selected}) UNION SELECT t.id FROM release_artist_credit c JOIN track t ON t.release_id=c.release_id WHERE c.artist_id IN ({selected})"
        );
        if large_artist && !queue {
            scope.push_str(" AND (EXISTS(SELECT 1 FROM track_artist_credit c WHERE c.track_id=t.id AND c.artist_id IN (SELECT value FROM json_each(?15))) OR EXISTS(SELECT 1 FROM album_artist_credit c WHERE c.album_id=r.album_id AND c.artist_id IN (SELECT value FROM json_each(?15))) OR EXISTS(SELECT 1 FROM release_artist_credit c WHERE c.release_id=r.id AND c.artist_id IN (SELECT value FROM json_each(?15))))");
            album_scope.push_str(" AND (EXISTS(SELECT 1 FROM album_artist_credit c WHERE c.album_id=a.album_id AND c.artist_id IN (SELECT value FROM json_each(?15))) OR EXISTS(SELECT 1 FROM release r CROSS JOIN release_artist_credit c ON c.release_id=r.id CROSS JOIN track t ON t.release_id=r.id JOIN library_membership lm ON lm.track_id=t.id WHERE r.album_id=a.album_id AND c.artist_id IN (SELECT value FROM json_each(?15))) OR EXISTS(SELECT 1 FROM release r CROSS JOIN track t ON t.release_id=r.id CROSS JOIN library_membership lm ON lm.track_id=t.id CROSS JOIN track_artist_credit c ON c.track_id=t.id WHERE r.album_id=a.album_id AND c.artist_id IN (SELECT value FROM json_each(?15))))");
        } else {
            scope.push_str(&format!(" AND t.id IN ({tracks})"));
            album_scope.push_str(&format!(" AND a.album_id IN (SELECT r.album_id FROM track t JOIN release r ON r.id=t.release_id JOIN library_membership lm ON lm.track_id=t.id WHERE t.id IN ({tracks}))"));
        }
    }
    if !request.genres.is_empty() {
        let tracks = "SELECT gs.track_id FROM file_genre_observation g JOIN track_source gs ON gs.source_id=g.source_id WHERE g.genre IN (SELECT value FROM json_each(?16))";
        if large_genre && !queue {
            scope.push_str(" AND EXISTS(SELECT 1 FROM track_source gs CROSS JOIN file_genre_observation g ON g.source_id=gs.source_id WHERE gs.track_id=t.id AND g.genre IN (SELECT value FROM json_each(?16)))");
            album_scope.push_str(" AND EXISTS(SELECT 1 FROM release gr CROSS JOIN track gt ON gt.release_id=gr.id CROSS JOIN library_membership gl ON gl.track_id=gt.id CROSS JOIN track_source gs ON gs.track_id=gt.id CROSS JOIN file_genre_observation g ON g.source_id=gs.source_id WHERE gr.album_id=a.album_id AND g.genre IN (SELECT value FROM json_each(?16)))");
        } else {
            scope.push_str(&format!(" AND t.id IN ({tracks})"));
            album_scope.push_str(&format!(" AND a.album_id IN (SELECT r.album_id FROM track t JOIN release r ON r.id=t.release_id JOIN library_membership lm ON lm.track_id=t.id WHERE t.id IN ({tracks}))"));
        }
    }
    if !request.albums.is_empty() {
        scope.push_str(" AND r.album_id IN (SELECT value FROM json_each(?17))");
    }
    if !request.tracks.is_empty() {
        scope.push_str(" AND t.id IN (SELECT value FROM json_each(?18))");
    }
    // Display fallback only: some imported files have Album credits but no
    // Track credits. Preserve the effective Track value whenever present.
    let album_credit = album_artist_credit_sql();
    let track_credit = song_artist_credit_sql();
    if request.unresolved_spotify && request.pane == Pane::Songs {
        scope.push_str(" AND NOT EXISTS(SELECT 1 FROM trusted_spotify_track spotify WHERE spotify.track_id=t.id)");
        scope.push_str(if request.marked_spotify { " AND EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=t.id AND x.provider='spotify')" } else { " AND NOT EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=t.id AND x.provider='spotify')" });
    }
    let saved = "JOIN library_membership lm ON lm.track_id=t.id";

    let album_order = request.sort == Sort::Album
        || (request.sort == Sort::Default
            && request.album.is_some()
            && request.song_column.is_none());
    let sql = match request.pane {
            Pane::Genres => "SELECT g.genre, g.genre, '', lower(g.genre), '', 0, 0, '', '', '', NULL, '', '' FROM (SELECT DISTINCT genre FROM file_genre_observation) g WHERE EXISTS(SELECT 1 FROM file_genre_observation observation JOIN track_source ts ON ts.source_id=observation.source_id JOIN library_membership lm ON lm.track_id=ts.track_id WHERE observation.genre=g.genre)".into(),
            Pane::Artists => format!("SELECT a.id, a.name, '', lower(a.name), '', 0, 0, '', '', '', NULL, '', ''
                FROM artist a WHERE (EXISTS(SELECT 1 FROM track_artist_credit c JOIN track t ON t.id=c.track_id {saved} WHERE c.artist_id=a.id)
                OR EXISTS(SELECT 1 FROM album_artist_credit c JOIN release r ON r.album_id=c.album_id JOIN track t ON t.release_id=r.id {saved} WHERE c.artist_id=a.id)
                OR EXISTS(SELECT 1 FROM release_artist_credit c JOIN track t ON t.release_id=c.release_id {saved} WHERE c.artist_id=a.id))"),
            Pane::Albums => {
                let keys: &str = match request.sort {
                    Sort::Year => "b.year_key, b.title_key, 0, 0",
                    Sort::Artist => "b.artist_key, b.artist_id, 0, 0",
                    _ => "b.title_key, '', 0, 0",
                };
                let from = if (large_genre || large_artist) && target.is_none() {
                    match request.sort {
                        Sort::Year => "album_browse_order b INDEXED BY album_order_year",
                        Sort::Artist => "album_browse_order b INDEXED BY album_order_artist",
                        _ => "album_browse_order b INDEXED BY album_order_title",
                    }
                } else { "album_browse_order b" };
                format!("SELECT b.album_id, a.title, {album_credit}, {keys}, '', '', '', b.year, b.year_key || char(31) || b.title_key, ''
                FROM {from} CROSS JOIN album_application_metadata a ON a.album_id=b.album_id WHERE EXISTS(SELECT 1 FROM release r JOIN track t ON t.release_id=r.id {saved} WHERE r.album_id=a.album_id) {album_scope}")
            },
            Pane::Songs => {
                let (title, release, disc, position) = if album_order && request.album_sort == Sort::Title {
                    ("b.title_key", "''", "COALESCE(t.disc_number, 1)", "COALESCE(t.track_number, 2147483647)")
                } else if album_order {
                    ("b.year_key", "b.title_key", "COALESCE(t.disc_number, 1)", "COALESCE(t.track_number, 2147483647)")
                } else { (match request.song_column {Some(SongColumn::Artist)=>"lower(e.artist_names)",Some(SongColumn::Album)=>"lower(e.release_title)",Some(SongColumn::Genre)=>"lower(e.genre_names)",Some(SongColumn::Reason)=>"lower(review.reason)",_=>"lower(e.title)"}, "''", "0", "0") };
                let album_filter = if request.album.is_some() { " AND r.album_id=?2" }
                    else if album_order && target.is_some() { " AND b.album_id=(SELECT r2.album_id FROM track t2 JOIN release r2 ON r2.id=t2.release_id WHERE t2.id=?11)" }
                    else { "" };
                let track_filter = if request.track.is_some() { " AND t.id=?3" } else { "" };
                let reason_stream = !request.marked_spotify && request.song_column == Some(SongColumn::Reason) && request.album.is_none() && request.albums.is_empty() && request.ids.is_empty();
                let from = if request.marked_spotify && !album_order {
                    "track_provider_exclusion excluded INDEXED BY provider_exclusion_list CROSS JOIN track t ON t.id=excluded.track_id JOIN release r ON r.id=t.release_id JOIN effective_track_metadata e ON e.track_id=t.id"
                } else if reason_stream {
                    "spotify_connection_review review INDEXED BY spotify_connection_reason CROSS JOIN track t ON t.id=review.track_id JOIN release r ON r.id=t.release_id JOIN effective_track_metadata e ON e.track_id=t.id"
                } else if album_order && request.album.is_none() && target.is_none() && request.album_sort == Sort::Title {
                    "album_browse_order b INDEXED BY album_order_title CROSS JOIN release r ON r.album_id=b.album_id CROSS JOIN track t ON t.release_id=r.id"
                } else if album_order {
                    "album_browse_order b CROSS JOIN release r ON r.album_id=b.album_id CROSS JOIN track t ON t.release_id=r.id"
                } else if (stream_titles || (request.artist.is_none() && request.genre.is_none() && request.album.is_none() && request.track.is_none() && request.artists.is_empty() && request.genres.is_empty() && request.albums.is_empty() && request.tracks.is_empty())) && target.is_none() {
                    match request.song_column {Some(SongColumn::Artist)=>"effective_track_metadata e INDEXED BY song_details_artist CROSS JOIN track t ON t.id=e.track_id JOIN release r ON r.id=t.release_id",Some(SongColumn::Album)=>"effective_track_metadata e INDEXED BY song_details_album CROSS JOIN track t ON t.id=e.track_id JOIN release r ON r.id=t.release_id",Some(SongColumn::Genre)=>"effective_track_metadata e INDEXED BY song_details_genre CROSS JOIN track t ON t.id=e.track_id JOIN release r ON r.id=t.release_id",_=>"effective_track_metadata e INDEXED BY track_browse_title CROSS JOIN track t ON t.id=e.track_id JOIN release r ON r.id=t.release_id"}
                } else if request.album.is_some() || !request.albums.is_empty() {
                    "release r CROSS JOIN track t ON t.release_id=r.id CROSS JOIN effective_track_metadata e ON e.track_id=t.id"
                } else if request.artist.is_some() || request.genre.is_some() || request.track.is_some() || !request.artists.is_empty() || !request.genres.is_empty() || !request.tracks.is_empty() {
                    "track t CROSS JOIN effective_track_metadata e ON e.track_id=t.id JOIN release r ON r.id=t.release_id"
                } else {
                    "effective_track_metadata e JOIN track t ON t.id=e.track_id JOIN release r ON r.id=t.release_id"
                };
                let from = if request.song_column == Some(SongColumn::Reason) && !reason_stream { format!("{from} JOIN spotify_connection_review review ON review.track_id=t.id") } else { from.to_owned() };
                let (display_title, display_year) = if album_order { ("''", "NULL") } else { ("e.title", "e.year") };
                let (album_title, album_key, edition, metadata_join) = if album_order {
                    ("''", "b.album_id", "r.id", "")
                } else { ("a.title", if request.unresolved_spotify {"r.album_id"} else {"''"}, "''", "JOIN album_application_metadata a ON a.album_id=r.album_id") };
                let track_id = if request.song_column == Some(SongColumn::Reason) { "review.track_id" } else if album_order { "t.id" } else { "e.track_id" };
                format!("SELECT {track_id}, {display_title}, '', {title}, {release}, {disc}, {position}, t.release_id, {album_title}, '', {display_year}, {album_key}, {edition}
                    FROM {from} {metadata_join} {saved}
                    WHERE 1=1 {album_filter} {track_filter} {scope} {}", if request.marked_spotify && !album_order {"AND excluded.provider='spotify'"} else {""})
            }
        };
    let c = request.after.clone().unwrap_or_default();
    let program = request.pane == Pane::Songs && album_order;
    let title_program = program && request.album_sort == Sort::Title;
    let year = request.pane == Pane::Albums && request.sort == Sort::Year;
    let grouped = request.pane == Pane::Albums && request.sort == Sort::Artist;
    let order = if grouped {
        "sort_title,release_key,album_key,id"
    } else if title_program {
        "sort_title,album_key,edition,disc,position,id"
    } else if program {
        "sort_title,release_key,album_key,edition,disc,position,id"
    } else if year {
        "sort_title,release_key,id"
    } else {
        "sort_title,id"
    };
    let cursor = if request.after.is_none() {
        ""
    } else if grouped {
        "WHERE (sort_title,release_key,album_key,id) > (?5,?6,?12,?9)"
    } else if title_program {
        "WHERE sort_title >= ?5 AND (sort_title,album_key,edition,disc,position,id) > (?5,?12,?10,?7,?8,?9)"
    } else if program {
        "WHERE sort_title >= ?5 AND (sort_title,release_key,album_key,edition,disc,position,id) > (?5,?6,?12,?10,?7,?8,?9)"
    } else if year {
        "WHERE sort_title >= ?5 AND (sort_title,release_key,id) > (?5,?6,?9)"
    } else {
        "WHERE sort_title >= ?5 AND (sort_title,id) > (?5,?9)"
    };
    let cursor = if inclusive {
        cursor.replace(" > ", " >= ")
    } else {
        cursor.to_string()
    };
    let descending = request.sort == Sort::Descending || request.descending;
    let cursor = if reverse != descending {
        cursor.replace(" >= ", " <= ").replace(" > ", " < ")
    } else {
        cursor
    };
    let order = if reverse != descending {
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
    let target_filter = if target.is_some() {
        if cursor.is_empty() {
            "WHERE id=?11"
        } else {
            "AND id=?11"
        }
    } else {
        "AND ?11 IS NULL"
    };
    let target_filter = if target.is_none() && cursor.is_empty() {
        "WHERE ?11 IS NULL"
    } else {
        target_filter
    };
    let projection = if ids_only {
        "chosen.id".into()
    } else if request.pane == Pane::Songs {
        format!(
            "chosen.id,e.title,{track_credit},chosen.sort_title,chosen.release_key,chosen.disc,chosen.position,chosen.release_id,a.title,{track_credit},e.year,chosen.album_key,chosen.edition, {}, ''",
            available.replace("rows.id", "chosen.id")
        )
    } else if grouped {
        "chosen.*, 0, COALESCE(group_artist.name, 'Unknown Artist')".into()
    } else {
        "chosen.*, 0, ''".into()
    };
    let joins = if ids_only {
        ""
    } else if request.pane == Pane::Songs && queue {
        "JOIN effective_track_metadata e ON e.track_id=chosen.id JOIN release r ON r.id=chosen.release_id JOIN album_application_metadata a ON a.album_id=r.album_id"
    } else if request.pane == Pane::Songs {
        "JOIN track display_track ON display_track.id=chosen.id JOIN effective_track_metadata e ON e.track_id=chosen.id JOIN release r ON r.id=chosen.release_id JOIN album_application_metadata a ON a.album_id=r.album_id"
    } else if grouped {
        "LEFT JOIN artist group_artist ON group_artist.id=chosen.release_key"
    } else {
        ""
    };
    let joins = if request.unresolved_spotify && request.pane == Pane::Songs && !queue && !ids_only
    {
        format!(
            "{joins} JOIN spotify_connection_review review_display ON review_display.track_id=chosen.id"
        )
    } else {
        joins.to_owned()
    };
    let final_order = order
        .split(',')
        .map(|key| format!("chosen.{key}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "WITH rows(id,title,subtitle,sort_title,release_key,disc,position,release_id,album_title,artist,year,album_key,edition) AS ({sql}), chosen AS MATERIALIZED (SELECT * FROM rows {cursor} {target_filter} AND (?19 IS NULL OR id IN (SELECT value FROM json_each(?19))) ORDER BY {order} LIMIT ?13) SELECT {projection}, {} FROM chosen {joins} WHERE (?14 IS NULL OR 1) AND (?15 IS NULL OR 1) AND (?16 IS NULL OR 1) AND (?17 IS NULL OR 1) AND (?18 IS NULL OR 1) ORDER BY {final_order}",
        if request.pane == Pane::Songs && !queue {
            if request.unresolved_spotify {
                "display_track.track_number, display_track.disc_number, EXISTS(SELECT 1 FROM track other_disc WHERE other_disc.release_id=chosen.release_id AND other_disc.disc_number>1),e.genre_names,CASE WHEN EXISTS(SELECT 1 FROM track_provider_exclusion x WHERE x.track_id=chosen.id AND x.provider='spotify') THEN 'Manually marked not on Spotify' WHEN review_display.state='needs_retry' THEN 'Needs retry · ' || review_display.reason ELSE review_display.reason END,review_display.reason_code"
            } else {
                "display_track.track_number, display_track.disc_number, EXISTS(SELECT 1 FROM track other_disc WHERE other_disc.release_id=chosen.release_id AND other_disc.disc_number>1),e.genre_names,'',''"
            }
        } else {
            "NULL, NULL, 0, '', '', ''"
        }
    );
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
            c.edition,
            target,
            c.album_key,
            if queue {
                -1_i64
            } else {
                i64::from(request.limit.clamp(1, 201))
            },
            request.genre,
            json_ids(&request.artists),
            json_ids(&request.genres),
            json_ids(&request.albums),
            json_ids(&request.tracks),
            json_ids(&request.ids)
        ],
        |r| {
            let id: String = r.get(0)?;
            if ids_only {
                return Ok(Row {
                    connection_reason: String::new(),
                    connection_reason_code: String::new(),
                    id: id.clone(),
                    title: String::new(),
                    subtitle: String::new(),
                    year: None,
                    group: String::new(),
                    group_label: String::new(),
                    cursor: Cursor {
                        id,
                        ..Default::default()
                    },
                    track: None,
                    playlist_position: None,
                    duration_ms: None,
                    duration_approximate: false,
                    genres: String::new(),
                    track_number: None,
                    disc_number: None,
                    multi_disc: false,
                });
            }
            let title: String = r.get(1)?;
            Ok(Row {
                connection_reason: r.get(19)?,
                connection_reason_code: r.get(20)?,
                id: id.clone(),
                title: title.clone(),
                subtitle: r.get(2)?,
                year: r.get(10)?,
                group: r.get(4)?,
                group_label: r.get(14)?,
                playlist_position: None,
                duration_ms: None,
                duration_approximate: false,
                genres: r.get(18)?,
                track_number: r.get(15)?,
                disc_number: r.get(16)?,
                multi_disc: r.get(17)?,
                cursor: Cursor {
                    title: r.get(3)?,
                    release: r.get(4)?,
                    album_key: r.get(11)?,
                    edition: r.get(12)?,
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
                        available: r.get(13)?,
                    })
                } else {
                    None
                },
            })
        },
    )?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn json_ids(ids: &[String]) -> Option<String> {
    (!ids.is_empty()).then(|| serde_json::to_string(ids).expect("string list"))
}

impl QueueReader {
    pub fn browse_ids(&self, request: &Request, ids: &[String]) -> Result<Vec<String>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let request = Request {
            ids: ids.to_vec(),
            after: None,
            ..request.clone()
        };
        Ok(
            query_projection(&self.0, &request, true, false, false, None, true)?
                .into_iter()
                .map(|r| r.id)
                .collect(),
        )
    }
    /// Expand only a range of IDs on the reader thread, never into QML.
    pub fn range(&self, request: &Request, anchor: &str, target: &str) -> Result<Vec<String>> {
        let key = |id| {
            query(
                &self.0,
                &Request {
                    after: None,
                    ..request.clone()
                },
                false,
                false,
                false,
                Some(id),
            )?
            .into_iter()
            .next()
            .ok_or_else(|| crate::Error::Invalid("Selection endpoint no longer visible".into()))
        };
        let a = key(anchor)?;
        let b = key(target)?;
        // Complete cursor keys match every supported display order.
        let tuple = |r: &Row| {
            (
                r.cursor.title.clone(),
                r.cursor.release.clone(),
                r.cursor.album_key.clone(),
                r.cursor.edition.clone(),
                r.cursor.disc,
                r.cursor.position,
                r.id.clone(),
            )
        };
        let descending = request.sort == Sort::Descending || request.descending;
        let (first, last) = if (tuple(&a) <= tuple(&b)) != descending {
            (a, b)
        } else {
            (b, a)
        };
        let mut request = Request {
            after: Some(first.cursor),
            limit: 201,
            ..request.clone()
        };
        let mut ids = vec![];
        loop {
            let rows = query(&self.0, &request, false, ids.is_empty(), false, None)?;
            if rows.is_empty() {
                return Err(crate::Error::Invalid(
                    "Selection range changed; try again".into(),
                ));
            }
            for row in &rows {
                ids.push(row.id.clone());
                if row.id == last.id {
                    return Ok(ids);
                }
            }
            request.after = rows.last().map(|r| r.cursor.clone());
        }
    }
}

/// Shared display-only fallback. SQL aliases e/a are canonical Track/Album metadata.
/// Explicit Track display credit wins; ordered Album credits are used only if absent.
pub(crate) fn album_artist_credit_sql() -> &'static str {
    "COALESCE((SELECT group_concat(name, '') FROM (SELECT COALESCE(c.credited_name, ar.name) || COALESCE(c.join_phrase, CASE WHEN EXISTS(SELECT 1 FROM album_artist_credit next WHERE next.album_id=c.album_id AND next.position>c.position) THEN ' / ' ELSE '' END) AS name FROM album_artist_credit c JOIN artist ar ON ar.id=c.artist_id WHERE c.album_id=a.album_id ORDER BY c.position)), '')"
}
pub(crate) fn song_artist_credit_sql() -> String {
    format!(
        "COALESCE(NULLIF(e.artist_names, ''), {})",
        album_artist_credit_sql()
    )
}
