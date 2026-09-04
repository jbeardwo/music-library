use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use music_library::Library;
use music_library::domain::{SearchCursor, SearchRequest, TrackId, TrackSearchResult};
use rusqlite::{Connection, params};

const DEFAULT_DATABASE: &str = "target/performance/library-200k.sqlite";
const ITERATIONS: usize = 20;

const PROTOTYPE_FIRST_PAGE: &str = "
    SELECT e.track_id, t.release_id, e.title, e.release_title, e.artist_names, e.year,
           EXISTS(
               SELECT 1 FROM track_source ts
               JOIN local_file_observation l ON l.source_id = ts.source_id
               WHERE ts.track_id = t.id AND l.available = 1
           ) AS available
    FROM track_search s
    JOIN effective_track_metadata e ON e.rowid = s.rowid
    JOIN track t ON t.id = e.track_id
    JOIN library_membership lm ON lm.track_id = t.id
    WHERE track_search MATCH ?1
    ORDER BY e.title, e.track_id
    LIMIT ?2";

const PROTOTYPE_AFTER_CURSOR: &str = "
    SELECT e.track_id, t.release_id, e.title, e.release_title, e.artist_names, e.year,
           EXISTS(
               SELECT 1 FROM track_source ts
               JOIN local_file_observation l ON l.source_id = ts.source_id
               WHERE ts.track_id = t.id AND l.available = 1
           ) AS available
    FROM track_search s
    JOIN effective_track_metadata e ON e.rowid = s.rowid
    JOIN track t ON t.id = e.track_id
    JOIN library_membership lm ON lm.track_id = t.id
    WHERE track_search MATCH ?1
      AND (e.title, e.track_id) > (?2, ?3)
    ORDER BY e.title, e.track_id
    LIMIT ?4";

const CURRENT_RELEVANT_PLAN: &str = "
    SELECT e.track_id
    FROM effective_track_metadata e
    JOIN track t ON t.id = e.track_id
    JOIN library_membership lm ON lm.track_id = t.id
    WHERE (?1 = '' OR e.rowid IN (
              SELECT rowid FROM track_search WHERE track_search MATCH ?1
          ))
      AND (?2 = '' OR e.title > ?2 OR (e.title = ?2 AND e.track_id > ?3))
    ORDER BY e.title, e.track_id
    LIMIT ?4";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database = database_argument()?;
    if !database.exists() {
        return Err(format!(
            "{} does not exist; build it with the database_performance example first",
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
        "current title-ordered shape",
        CURRENT_RELEVANT_PLAN,
        params!["\"Quasar\"*", "", "", 50],
    )?;
    print_plan(
        &connection,
        "FTS-candidate-driven first page",
        PROTOTYPE_FIRST_PAGE,
        params!["\"Quasar\"*", 50],
    )?;
    print_plan(
        &connection,
        "FTS-candidate-driven cursor page",
        PROTOTYPE_AFTER_CURSOR,
        params!["\"Nocturne\"*", "Song 180000", "track-180000", 50],
    )?;

    println!("\nFirst-page comparison (median of {ITERATIONS}):");
    for case in [
        Case::new("rare Quasar", "Quasar", 20),
        Case::new("moderate Nocturne", "Nocturne", 50),
        Case::new("common Love", "Love", 50),
        Case::new("Love plus unique", "Love 123450", 1),
        Case::new("pathological Track", "Track", 50),
    ] {
        compare(&library, &connection, &case, None)?;
        verify_two_pages(&library, &connection, &case)?;
    }
    println!("  ordered first/second-page equivalence verified for every query");

    println!("\nDeep-cursor comparison (median of {ITERATIONS}):");
    for (case, cursor) in [
        (
            Case::new("rare Quasar", "Quasar", 10),
            cursor(100_000, " Quasar Nocturne Love"),
        ),
        (
            Case::new("moderate Nocturne", "Nocturne", 50),
            cursor(180_000, " Quasar Nocturne Love"),
        ),
        (
            Case::new("common Love", "Love", 50),
            cursor(190_000, " Quasar Nocturne Love"),
        ),
        (
            Case::new("pathological Track", "Track", 50),
            cursor(190_000, " Quasar Nocturne Love"),
        ),
    ] {
        compare(&library, &connection, &case, Some(&cursor))?;
    }
    Ok(())
}

struct Case {
    label: &'static str,
    text: &'static str,
    expected: usize,
}

impl Case {
    const fn new(label: &'static str, text: &'static str, expected: usize) -> Self {
        Self {
            label,
            text,
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
    let current = current_search(library, case.text, after)?;
    let prototype = prototype_search(connection, case.text, after)?;
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

    let current_time = median(|| current_search(library, case.text, after))?;
    let prototype_time = median(|| prototype_search(connection, case.text, after))?;
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
    let first = current_search_with_limit(library, case.text, None, 10)?;
    let Some(last) = first.last() else {
        return Ok(());
    };
    let cursor = SearchCursor {
        title: last.title.clone(),
        track_id: last.track_id.clone(),
    };
    let current_second = current_search_with_limit(library, case.text, Some(&cursor), 10)?;
    let prototype_second = prototype_search_with_limit(connection, case.text, Some(&cursor), 10)?;
    if current_second != prototype_second {
        return Err(format!("{} second page differs", case.label).into());
    }
    if current_second.first().is_some_and(|next| {
        (next.title.as_str(), next.track_id.as_ref())
            <= (cursor.title.as_str(), cursor.track_id.as_ref())
    }) {
        return Err(format!("{} cursor order did not advance", case.label).into());
    }
    Ok(())
}

fn current_search(
    library: &Library,
    text: &str,
    after: Option<&SearchCursor>,
) -> music_library::Result<Vec<TrackSearchResult>> {
    current_search_with_limit(library, text, after, 50)
}

fn current_search_with_limit(
    library: &Library,
    text: &str,
    after: Option<&SearchCursor>,
    limit: u32,
) -> music_library::Result<Vec<TrackSearchResult>> {
    library.search(&SearchRequest {
        text: text.into(),
        after: after.cloned(),
        limit,
        ..SearchRequest::default()
    })
}

fn prototype_search(
    connection: &Connection,
    text: &str,
    after: Option<&SearchCursor>,
) -> rusqlite::Result<Vec<TrackSearchResult>> {
    prototype_search_with_limit(connection, text, after, 50)
}

fn prototype_search_with_limit(
    connection: &Connection,
    text: &str,
    after: Option<&SearchCursor>,
    limit: u32,
) -> rusqlite::Result<Vec<TrackSearchResult>> {
    let fts_query = fts_prefix_query(text);
    let mut statement = connection.prepare(if after.is_some() {
        PROTOTYPE_AFTER_CURSOR
    } else {
        PROTOTYPE_FIRST_PAGE
    })?;
    let rows = if let Some(cursor) = after {
        statement.query_map(
            params![fts_query, cursor.title, cursor.track_id.as_ref(), limit],
            map_result,
        )?
    } else {
        statement.query_map(params![fts_query, limit], map_result)?
    };
    rows.collect()
}

fn map_result(row: &rusqlite::Row<'_>) -> rusqlite::Result<TrackSearchResult> {
    Ok(TrackSearchResult {
        track_id: TrackId(row.get(0)?),
        release_id: music_library::domain::ReleaseId(row.get(1)?),
        title: row.get(2)?,
        release_title: row.get(3)?,
        artist_names: row.get(4)?,
        year: row.get(5)?,
        available: row.get(6)?,
    })
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
