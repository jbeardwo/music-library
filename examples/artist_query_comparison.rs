use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use music_library::Library;
use music_library::domain::{ArtistId, ReleaseId, SearchCursor, SearchRequest, TrackId};
use rusqlite::{Connection, Row, params};

const DEFAULT_DATABASE: &str = "target/performance/library-200k.sqlite";
const ITERATIONS: usize = 20;

const PROTOTYPE_FIRST_PAGE: &str = "
    SELECT DISTINCT e.track_id, e.title
    FROM track_artist_credit credit
    JOIN track t ON t.id = credit.track_id
    JOIN effective_track_metadata e ON e.track_id = t.id
    JOIN library_membership lm ON lm.track_id = t.id
    WHERE credit.artist_id = ?1
      AND (?2 = '' OR e.rowid IN (
              SELECT rowid FROM track_search WHERE track_search MATCH ?2
          ))
      AND (?3 IS NULL OR t.release_id = ?3)
      AND (?4 IS NULL OR EXISTS(
              SELECT 1 FROM track_source ts
              JOIN local_file_observation l ON l.source_id = ts.source_id
              WHERE ts.track_id = t.id AND l.available = 1
          ) = ?4)
    ORDER BY e.title, e.track_id
    LIMIT ?5";

const PROTOTYPE_AFTER_CURSOR: &str = "
    SELECT DISTINCT e.track_id, e.title
    FROM track_artist_credit credit
    JOIN track t ON t.id = credit.track_id
    JOIN effective_track_metadata e ON e.track_id = t.id
    JOIN library_membership lm ON lm.track_id = t.id
    WHERE credit.artist_id = ?1
      AND (?2 = '' OR e.rowid IN (
              SELECT rowid FROM track_search WHERE track_search MATCH ?2
          ))
      AND (?3 IS NULL OR t.release_id = ?3)
      AND (?4 IS NULL OR EXISTS(
              SELECT 1 FROM track_source ts
              JOIN local_file_observation l ON l.source_id = ts.source_id
              WHERE ts.track_id = t.id AND l.available = 1
          ) = ?4)
      AND (e.title, e.track_id) > (?5, ?6)
    ORDER BY e.title, e.track_id
    LIMIT ?7";

const CURRENT_RELEVANT_PLAN: &str = "
    SELECT e.track_id
    FROM effective_track_metadata e
    JOIN track t ON t.id = e.track_id
    JOIN library_membership lm ON lm.track_id = t.id
    WHERE (?1 IS NULL OR EXISTS (
              SELECT 1 FROM track_artist_credit credit
              WHERE credit.track_id = t.id AND credit.artist_id = ?1
          ))
      AND (?2 = '' OR e.rowid IN (
              SELECT rowid FROM track_search WHERE track_search MATCH ?2
          ))
      AND (?3 IS NULL OR t.release_id = ?3)
      AND (?4 IS NULL OR EXISTS(
              SELECT 1 FROM track_source ts
              JOIN local_file_observation l ON l.source_id = ts.source_id
              WHERE ts.track_id = t.id AND l.available = 1
          ) = ?4)
      AND (?5 = '' OR e.title > ?5 OR (e.title = ?5 AND e.track_id > ?6))
    ORDER BY e.title, e.track_id
    LIMIT ?7";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database = database_argument()?;
    if !database.exists() {
        return Err(format!(
            "{} does not exist; rebuild it with the database_performance example first",
            database.display()
        )
        .into());
    }
    println!(
        "environment: {} {} | SQLite {} | database: {} ({:.1} MiB)",
        std::env::consts::OS,
        std::env::consts::ARCH,
        rusqlite::version(),
        database.display(),
        fs::metadata(&database)?.len() as f64 / 1_048_576.0
    );

    let library = Library::open(&database)?;
    let connection = Connection::open(&database)?;
    connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")?;

    print_plan(
        &connection,
        "current title-driven Artist filter",
        CURRENT_RELEVANT_PLAN,
        params![
            "artist-moderate",
            "",
            Option::<String>::None,
            Option::<bool>::None,
            "",
            "",
            50
        ],
    )?;
    print_plan(
        &connection,
        "Artist-credit-driven first page",
        PROTOTYPE_FIRST_PAGE,
        params![
            "artist-moderate",
            "",
            Option::<String>::None,
            Option::<bool>::None,
            50
        ],
    )?;
    print_plan(
        &connection,
        "Artist-credit-driven cursor page",
        PROTOTYPE_AFTER_CURSOR,
        params![
            "artist-moderate",
            "",
            Option::<String>::None,
            Option::<bool>::None,
            "Song 150000 Track Quasar Nocturne Love",
            "track-150000",
            50
        ],
    )?;

    println!("\nFirst-page comparison (median of {ITERATIONS}):");
    for case in [
        Case::new("rare Artist", "artist-rare", 20),
        Case::new("moderate Artist", "artist-moderate", 50),
        Case::new("common Artist", "artist-common", 50),
    ] {
        compare(&library, &connection, &case, None)?;
        verify_two_pages(&library, &connection, &case)?;
    }
    println!("  ordered first/second-page equivalence verified for every Artist");

    println!("\nDeep-cursor comparison (median of {ITERATIONS}):");
    for (case, cursor) in [
        (
            Case::new("rare Artist", "artist-rare", 10),
            cursor(100_000, " Quasar Nocturne Love"),
        ),
        (
            Case::new("moderate Artist", "artist-moderate", 50),
            cursor(150_000, " Quasar Nocturne Love"),
        ),
        (
            Case::new("common Artist", "artist-common", 50),
            cursor(190_000, " Quasar Nocturne Love"),
        ),
    ] {
        compare(&library, &connection, &case, Some(&cursor))?;
    }

    verify_combined_filters(&library, &connection)?;
    println!("combined FTS, Release, availability, and membership semantics verified");
    Ok(())
}

struct Case {
    label: &'static str,
    artist_id: &'static str,
    expected: usize,
}

impl Case {
    const fn new(label: &'static str, artist_id: &'static str, expected: usize) -> Self {
        Self {
            label,
            artist_id,
            expected,
        }
    }
}

fn compare(
    library: &Library,
    connection: &Connection,
    case: &Case,
    after: Option<&SearchCursor>,
) -> Result<(), Box<dyn std::error::Error>> {
    let current = current_ids(library, case.artist_id, "", None, None, after, 50)?;
    let prototype = prototype_ids(connection, case.artist_id, "", None, None, after, 50)?;
    if current != prototype {
        return Err(format!("{} returned different ordered results", case.label).into());
    }
    if current.len() != case.expected {
        return Err(format!(
            "{} returned {} rows, expected {}",
            case.label,
            current.len(),
            case.expected
        )
        .into());
    }

    let current_time = median(|| current_ids(library, case.artist_id, "", None, None, after, 50))?;
    let prototype_time =
        median(|| prototype_ids(connection, case.artist_id, "", None, None, after, 50))?;
    println!(
        "  {}{}: current {}, prototype {}, {:.2}x prototype/current",
        case.label,
        if after.is_some() { " after cursor" } else { "" },
        format_duration(current_time),
        format_duration(prototype_time),
        prototype_time.as_secs_f64() / current_time.as_secs_f64()
    );
    Ok(())
}

fn verify_two_pages(
    library: &Library,
    connection: &Connection,
    case: &Case,
) -> Result<(), Box<dyn std::error::Error>> {
    let first = current_ids(library, case.artist_id, "", None, None, None, 10)?;
    let Some(last_id) = first.last() else {
        return Ok(());
    };
    let title = title_for_track(connection, last_id)?;
    let cursor = SearchCursor {
        title,
        track_id: TrackId(last_id.clone()),
    };
    let current = current_ids(library, case.artist_id, "", None, None, Some(&cursor), 10)?;
    let prototype = prototype_ids(
        connection,
        case.artist_id,
        "",
        None,
        None,
        Some(&cursor),
        10,
    )?;
    if current != prototype {
        return Err(format!("{} second page differs", case.label).into());
    }
    Ok(())
}

fn verify_combined_filters(
    library: &Library,
    connection: &Connection,
) -> Result<(), Box<dyn std::error::Error>> {
    let release = Some("release-10000");
    let current = current_ids(
        library,
        "artist-common",
        "Love",
        release,
        Some(false),
        None,
        50,
    )?;
    let prototype = prototype_ids(
        connection,
        "artist-common",
        "Love",
        release,
        Some(false),
        None,
        50,
    )?;
    if current != prototype || current != ["track-100000"] {
        return Err("combined-filter results differ".into());
    }
    Ok(())
}

fn current_ids(
    library: &Library,
    artist_id: &str,
    text: &str,
    release_id: Option<&str>,
    availability: Option<bool>,
    after: Option<&SearchCursor>,
    limit: u32,
) -> music_library::Result<Vec<String>> {
    Ok(library
        .search(&SearchRequest {
            text: text.into(),
            release_id: release_id.map(|id| ReleaseId(id.into())),
            artist_id: Some(ArtistId(artist_id.into())),
            availability,
            after: after.cloned(),
            limit,
        })?
        .into_iter()
        .map(|track| track.track_id.0)
        .collect())
}

fn prototype_ids(
    connection: &Connection,
    artist_id: &str,
    text: &str,
    release_id: Option<&str>,
    availability: Option<bool>,
    after: Option<&SearchCursor>,
    limit: u32,
) -> rusqlite::Result<Vec<String>> {
    let fts_query = fts_prefix_query(text);
    let mut statement = connection.prepare(if after.is_some() {
        PROTOTYPE_AFTER_CURSOR
    } else {
        PROTOTYPE_FIRST_PAGE
    })?;
    let rows = if let Some(cursor) = after {
        statement.query_map(
            params![
                artist_id,
                fts_query,
                release_id,
                availability,
                cursor.title,
                cursor.track_id.as_ref(),
                limit
            ],
            map_track_id,
        )?
    } else {
        statement.query_map(
            params![artist_id, fts_query, release_id, availability, limit],
            map_track_id,
        )?
    };
    rows.collect()
}

fn map_track_id(row: &Row<'_>) -> rusqlite::Result<String> {
    row.get(0)
}

fn title_for_track(connection: &Connection, track_id: &str) -> rusqlite::Result<String> {
    connection.query_row(
        "SELECT title FROM effective_track_metadata WHERE track_id = ?1",
        [track_id],
        |row| row.get(0),
    )
}

fn median<T, E>(mut operation: impl FnMut() -> Result<T, E>) -> Result<Duration, E> {
    black_box(operation()?);
    let mut samples = Vec::with_capacity(ITERATIONS);
    for _ in 0..ITERATIONS {
        let started = Instant::now();
        black_box(operation()?);
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    Ok(samples[samples.len() / 2])
}

fn cursor(number: u32, suffix: &str) -> SearchCursor {
    SearchCursor {
        title: format!("Song {number:06} Track{suffix}"),
        track_id: TrackId(format!("track-{number:06}")),
    }
}

fn fts_prefix_query(input: &str) -> String {
    input
        .split_whitespace()
        .filter(|part| !part.is_empty())
        .map(|part| format!("\"{}\"*", part.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn print_plan<P: rusqlite::Params>(
    connection: &Connection,
    label: &str,
    query: &str,
    parameters: P,
) -> rusqlite::Result<()> {
    println!("\nEXPLAIN QUERY PLAN — {label}:");
    let mut statement = connection.prepare(&format!("EXPLAIN QUERY PLAN {query}"))?;
    let rows = statement.query_map(parameters, |row| row.get::<_, String>(3))?;
    for row in rows {
        println!("  {}", row?);
    }
    Ok(())
}

fn database_argument() -> Result<PathBuf, String> {
    let mut arguments = std::env::args().skip(1);
    let Some(argument) = arguments.next() else {
        return Ok(PathBuf::from(DEFAULT_DATABASE));
    };
    if argument == "--database" {
        return arguments
            .next()
            .map(PathBuf::from)
            .ok_or_else(|| "--database requires a path".into());
    }
    Err(format!("unknown argument: {argument}"))
}

fn format_duration(duration: Duration) -> String {
    if duration.as_millis() >= 1 {
        format!("{:.3} ms", duration.as_secs_f64() * 1_000.0)
    } else {
        format!("{:.3} µs", duration.as_secs_f64() * 1_000_000.0)
    }
}
