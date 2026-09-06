use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use music_library::Library;
use music_library::domain::{ArtistId, ReleaseId, SearchCursor, SearchRequest, TrackId};
use rusqlite::{Connection, Row, params};

const SOURCELESS_DATABASE: &str = "target/performance/library-200k.sqlite";
const DIAGNOSTIC_DATABASE: &str = "target/performance/availability-diagnostic.sqlite";
const ITERATIONS: usize = 3;

const AVAILABLE_CANDIDATES: &str = "
    SELECT DISTINCT e.track_id, e.title
    FROM local_file_observation l
    JOIN track_source ts ON ts.source_id = l.source_id
    JOIN track t ON t.id = ts.track_id
    JOIN effective_track_metadata e ON e.track_id = t.id
    JOIN library_membership lm ON lm.track_id = t.id
    WHERE l.available = 1
      AND (?1 = '' OR e.rowid IN (
              SELECT rowid FROM track_search WHERE track_search MATCH ?1
          ))
      AND (?2 IS NULL OR t.release_id = ?2)
      AND (?3 IS NULL OR EXISTS (
              SELECT 1 FROM track_artist_credit credit
              WHERE credit.track_id = t.id AND credit.artist_id = ?3
          ))
      AND (?4 = '' OR e.title > ?4 OR (e.title = ?4 AND e.track_id > ?5))
    ORDER BY e.title, e.track_id
    LIMIT ?6";

const UNAVAILABLE_CANDIDATES: &str = "
    SELECT e.track_id, e.title
    FROM effective_track_metadata e
    JOIN track t ON t.id = e.track_id
    JOIN library_membership lm ON lm.track_id = t.id
    WHERE t.id NOT IN (
              SELECT ts.track_id
              FROM local_file_observation l
              JOIN track_source ts ON ts.source_id = l.source_id
              WHERE l.available = 1
          )
      AND (?1 = '' OR e.rowid IN (
              SELECT rowid FROM track_search WHERE track_search MATCH ?1
          ))
      AND (?2 IS NULL OR t.release_id = ?2)
      AND (?3 IS NULL OR EXISTS (
              SELECT 1 FROM track_artist_credit credit
              WHERE credit.track_id = t.id AND credit.artist_id = ?3
          ))
      AND (?4 = '' OR e.title > ?4 OR (e.title = ?4 AND e.track_id > ?5))
    ORDER BY e.title, e.track_id
    LIMIT ?6";

const TRACK_FIRST_REFERENCE: &str = "
    SELECT e.track_id, e.title
    FROM effective_track_metadata e
    JOIN track t ON t.id = e.track_id
    JOIN library_membership lm ON lm.track_id = t.id
    WHERE EXISTS(
              SELECT 1 FROM track_source ts
              CROSS JOIN local_file_observation l
              WHERE ts.track_id = t.id
                AND l.source_id = ts.source_id
                AND l.available = 1
          ) = ?1
      AND (?2 = '' OR e.rowid IN (
              SELECT rowid FROM track_search WHERE track_search MATCH ?2
          ))
      AND (?3 IS NULL OR t.release_id = ?3)
      AND (?4 IS NULL OR EXISTS (
              SELECT 1 FROM track_artist_credit credit
              WHERE credit.track_id = t.id AND credit.artist_id = ?4
          ))
      AND (?5 = '' OR e.title > ?5 OR (e.title = ?5 AND e.track_id > ?6))
    ORDER BY e.title, e.track_id
    LIMIT ?7";

const CURRENT_PLAN: &str = "
    SELECT e.track_id
    FROM effective_track_metadata e
    JOIN track t ON t.id = e.track_id
    JOIN library_membership lm ON lm.track_id = t.id
    WHERE (?1 IS NULL OR EXISTS(
              SELECT 1 FROM track_source ts
              JOIN local_file_observation l ON l.source_id = ts.source_id
              WHERE ts.track_id = t.id AND l.available = 1
          ) = ?1)
      AND (?2 = '' OR e.rowid IN (
              SELECT rowid FROM track_search WHERE track_search MATCH ?2
          ))
      AND (?3 IS NULL OR t.release_id = ?3)
      AND (?4 IS NULL OR EXISTS (
              SELECT 1 FROM track_artist_credit credit
              WHERE credit.track_id = t.id AND credit.artist_id = ?4
          ))
      AND (?5 = '' OR e.title > ?5 OR (e.title = ?5 AND e.track_id > ?6))
    ORDER BY e.title, e.track_id
    LIMIT ?7";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!(
        "Historical strategy diagnostic: simultaneous-connection timings are cache-confounded. Use availability_benchmark_audit for production before/after decisions; see docs/performance.md."
    );
    let source = PathBuf::from(SOURCELESS_DATABASE);
    let diagnostic = PathBuf::from(DIAGNOSTIC_DATABASE);
    if !source.exists() {
        return Err(format!(
            "{} does not exist; rebuild it with the database_performance example first",
            source.display()
        )
        .into());
    }
    copy_fixture(&source, &diagnostic)?;
    println!(
        "environment: {} {} | SQLite {} | source-less baseline: {} | overlay: {}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        rusqlite::version(),
        source.display(),
        diagnostic.display()
    );

    for distribution in Distribution::ALL {
        install_distribution(&diagnostic, distribution)?;
        let library = Library::open(&diagnostic)?;
        let connection = Connection::open(&diagnostic)?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;",
        )?;
        let count: i64 = connection.query_row(
            "SELECT count(DISTINCT ts.track_id)
             FROM track_source ts
             JOIN local_file_observation l ON l.source_id = ts.source_id
             WHERE l.available = 1",
            [],
            |row| row.get(0),
        )?;
        if count != distribution.available_tracks {
            return Err(format!(
                "{} created {count} available Tracks, expected {}",
                distribution.label, distribution.available_tracks
            )
            .into());
        }

        println!(
            "\n{}: {} available ({:.2}%)",
            distribution.label,
            count,
            count as f64 / 2_000.0
        );
        if distribution.print_plan {
            print_plans(&connection)?;
        }
        for availability in [true, false] {
            compare(&library, &connection, distribution, availability, None)?;
            compare(
                &library,
                &connection,
                distribution,
                availability,
                Some(&deep_cursor()),
            )?;
            verify_two_pages(&library, &connection, availability)?;
        }
        verify_combined_filters(&library, &connection)?;
    }
    println!("\nordered first/second/deep-page and combined-filter equivalence verified");
    Ok(())
}

#[derive(Clone, Copy)]
struct Distribution {
    label: &'static str,
    predicate: &'static str,
    available_tracks: i64,
    print_plan: bool,
}

impl Distribution {
    const ALL: [Self; 6] = [
        Self::new("source-less baseline", "0", 0, true),
        Self::new("rare", "track_number % 10000 = 0", 20, true),
        Self::new("moderate", "track_number % 200 = 0", 1_000, true),
        Self::new("common", "track_number % 10 = 0", 20_000, true),
        Self::new("mostly available", "track_number % 10 <> 0", 180_000, true),
        Self::new("entirely available", "1", 200_000, true),
    ];

    const fn new(
        label: &'static str,
        predicate: &'static str,
        available_tracks: i64,
        print_plan: bool,
    ) -> Self {
        Self {
            label,
            predicate,
            available_tracks,
            print_plan,
        }
    }
}

fn copy_fixture(source: &Path, diagnostic: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = diagnostic.parent() {
        fs::create_dir_all(parent)?;
    }
    let connection = Connection::open(source)?;
    connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    drop(connection);
    remove_database_files(diagnostic)?;
    fs::copy(source, diagnostic)?;
    Ok(())
}

fn remove_database_files(database: &Path) -> std::io::Result<()> {
    for path in [
        database.to_path_buf(),
        PathBuf::from(format!("{}-wal", database.display())),
        PathBuf::from(format!("{}-shm", database.display())),
    ] {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn install_distribution(
    database: &Path,
    distribution: Distribution,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut connection = Connection::open(database)?;
    connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
    let transaction = connection.transaction()?;
    transaction.execute("DELETE FROM playable_source", [])?;
    transaction.execute(
        "INSERT OR IGNORE INTO discovery_root(id, kind, location)
         VALUES ('availability-root', 'local_filesystem', X'2F646961676E6F73746963')",
        [],
    )?;
    let selection = format!(
        "WITH selected AS (
             SELECT id AS track_id, CAST(substr(id, 7) AS INTEGER) AS track_number
             FROM track
         )
         INSERT INTO playable_source(id, kind)
         SELECT 'availability-source-' || track_id, 'local_file'
         FROM selected WHERE {}",
        distribution.predicate
    );
    transaction.execute(&selection, [])?;
    transaction.execute(
        "INSERT INTO track_source(track_id, source_id)
         SELECT substr(id, 21), id FROM playable_source
         WHERE id LIKE 'availability-source-%'",
        [],
    )?;
    transaction.execute(
        "INSERT INTO local_file_observation(
             source_id, root_id, path, size_bytes, modified_ns, available
         )
         SELECT id, 'availability-root', CAST('/diagnostic/' || id AS BLOB), 1, 1, 1
         FROM playable_source WHERE id LIKE 'availability-source-%'",
        [],
    )?;
    transaction.commit()?;
    Ok(())
}

fn compare(
    library: &Library,
    connection: &Connection,
    distribution: Distribution,
    availability: bool,
    after: Option<&SearchCursor>,
) -> Result<(), Box<dyn std::error::Error>> {
    let reference = reference_ids(connection, availability, "", None, None, after, 50)?;
    let prototype = prototype_ids(connection, availability, "", None, None, after, 50)?;
    if reference != prototype {
        return Err(format!(
            "{} available={availability} returned different ordered results",
            distribution.label
        )
        .into());
    }
    let prototype_time =
        median(|| prototype_ids(connection, availability, "", None, None, after, 50))?;
    let track_first_time =
        median(|| reference_ids(connection, availability, "", None, None, after, 50))?;
    let page = if after.is_some() {
        "deep page"
    } else {
        "first page"
    };
    let current = current_ids(library, availability, "", None, None, after, 50)?;
    if current != reference {
        return Err(format!("{} production and reference differ", distribution.label).into());
    }
    let control_connection = Connection::open(DIAGNOSTIC_DATABASE)?;
    control_connection.execute_batch(
        "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;",
    )?;
    let _: u32 = control_connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
    // Exact production SQL on an independently opened connection with the same initialization.
    let query = production_queries()[1].clone();
    let mut control = || -> rusqlite::Result<Vec<String>> {
        let mut statement = control_connection.prepare(&query)?;
        statement
            .query_map(
                params![
                    "",
                    Option::<String>::None,
                    Option::<String>::None,
                    availability,
                    after.map(|c| c.title.as_str()).unwrap_or(""),
                    after.map(|c| c.track_id.as_ref()).unwrap_or(""),
                    50
                ],
                map_track_id,
            )?
            .collect()
    };
    assert_eq!(control()?, reference);
    let control_time = median(&mut control)?;
    let current_time = median(|| current_ids(library, availability, "", None, None, after, 50))?;
    println!(
        "  available={availability}, {page} ({} rows): production {}, candidate {}, Track-first {}, full-result control {}",
        current.len(),
        format_duration(current_time),
        format_duration(prototype_time),
        format_duration(track_first_time),
        format_duration(control_time)
    );
    Ok(())
}

fn verify_two_pages(
    library: &Library,
    connection: &Connection,
    availability: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let first = reference_ids(connection, availability, "", None, None, None, 10)?;
    let Some(last_id) = first.last() else {
        return Ok(());
    };
    let cursor = SearchCursor {
        title: title_for_track(connection, last_id)?,
        track_id: TrackId(last_id.clone()),
    };
    let reference = reference_ids(connection, availability, "", None, None, Some(&cursor), 10)?;
    let prototype = prototype_ids(connection, availability, "", None, None, Some(&cursor), 10)?;
    let production = current_ids(library, availability, "", None, None, Some(&cursor), 10)?;
    if reference != prototype || reference != production {
        return Err(format!("available={availability} second page differs").into());
    }
    Ok(())
}

fn verify_combined_filters(
    library: &Library,
    connection: &Connection,
) -> Result<(), Box<dyn std::error::Error>> {
    for availability in [true, false] {
        let reference = reference_ids(
            connection,
            availability,
            "Love",
            Some("release-10000"),
            Some("artist-common"),
            None,
            50,
        )?;
        let prototype = prototype_ids(
            connection,
            availability,
            "Love",
            Some("release-10000"),
            Some("artist-common"),
            None,
            50,
        )?;
        let production = current_ids(
            library,
            availability,
            "Love",
            Some("release-10000"),
            Some("artist-common"),
            None,
            50,
        )?;
        if reference != prototype || reference != production {
            return Err(format!("available={availability} combined filters differ").into());
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn reference_ids(
    connection: &Connection,
    availability: bool,
    text: &str,
    release_id: Option<&str>,
    artist_id: Option<&str>,
    after: Option<&SearchCursor>,
    limit: u32,
) -> rusqlite::Result<Vec<String>> {
    let cursor_title = after.map(|cursor| cursor.title.as_str()).unwrap_or("");
    let cursor_id = after.map(|cursor| cursor.track_id.as_ref()).unwrap_or("");
    let mut statement = connection.prepare_cached(TRACK_FIRST_REFERENCE)?;
    let rows = statement.query_map(
        params![
            availability,
            fts_prefix_query(text),
            release_id,
            artist_id,
            cursor_title,
            cursor_id,
            limit.clamp(1, 200)
        ],
        map_track_id,
    )?;
    rows.collect()
}

#[allow(clippy::too_many_arguments)]
fn current_ids(
    library: &Library,
    availability: bool,
    text: &str,
    release_id: Option<&str>,
    artist_id: Option<&str>,
    after: Option<&SearchCursor>,
    limit: u32,
) -> music_library::Result<Vec<String>> {
    Ok(library
        .search(&SearchRequest {
            text: text.into(),
            release_id: release_id.map(|id| ReleaseId(id.into())),
            artist_id: artist_id.map(|id| ArtistId(id.into())),
            availability: Some(availability),
            after: after.cloned(),
            limit,
        })?
        .into_iter()
        .map(|track| track.track_id.0)
        .collect())
}

#[allow(clippy::too_many_arguments)]
fn prototype_ids(
    connection: &Connection,
    availability: bool,
    text: &str,
    release_id: Option<&str>,
    artist_id: Option<&str>,
    after: Option<&SearchCursor>,
    limit: u32,
) -> rusqlite::Result<Vec<String>> {
    let cursor_title = after.map(|cursor| cursor.title.as_str()).unwrap_or("");
    let cursor_id = after.map(|cursor| cursor.track_id.as_ref()).unwrap_or("");
    let query = if availability {
        AVAILABLE_CANDIDATES
    } else {
        UNAVAILABLE_CANDIDATES
    };
    let mut statement = connection.prepare_cached(query)?;
    let rows = statement.query_map(
        params![
            fts_prefix_query(text),
            release_id,
            artist_id,
            cursor_title,
            cursor_id,
            limit.clamp(1, 200)
        ],
        map_track_id,
    )?;
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

fn deep_cursor() -> SearchCursor {
    SearchCursor {
        title: "Song 100000 Track Quasar Nocturne Love".into(),
        track_id: TrackId("track-100000".into()),
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

fn production_queries() -> Vec<String> {
    let storage = include_str!("../src/storage.rs");
    let queries: Vec<_> = storage.split("\"SELECT e.track_id, t.release_id, e.title, e.release_title, e.artist_names, e.year,")
        .skip(1).map(|tail| format!("SELECT e.track_id, t.release_id, e.title, e.release_title, e.artist_names, e.year,{}", tail.split("\"").next().unwrap())).collect();
    assert_eq!(queries.len(), 2);
    queries
}

fn print_plans(connection: &Connection) -> rusqlite::Result<()> {
    // Read the exact production statements, including the projected availability check.
    // This keeps EXPLAIN tied to production without exposing a query API.
    let queries = production_queries();
    for availability in [true, false] {
        for (index, query) in queries.iter().enumerate() {
            print_plan(
                connection,
                &format!("production branch {index} available={availability}"),
                query,
                params![
                    if index == 0 { "release-10000" } else { "" },
                    if index == 0 { Some("") } else { None },
                    Option::<String>::None,
                    availability,
                    "",
                    "",
                    50
                ],
            )?;
        }
        print_plan(
            connection,
            &format!("pre-fix full production available={availability}"),
            &queries[1].replace(
                "CROSS JOIN local_file_observation",
                "JOIN local_file_observation",
            ),
            params![
                "",
                Option::<String>::None,
                Option::<String>::None,
                availability,
                "",
                "",
                50
            ],
        )?;
        print_plan(
            connection,
            &format!("Track-first reference available={availability}"),
            TRACK_FIRST_REFERENCE,
            params![
                availability,
                "",
                Option::<String>::None,
                Option::<String>::None,
                "",
                "",
                50
            ],
        )?;
        print_plan(
            connection,
            &format!("pre-fix title-driven available={availability}"),
            CURRENT_PLAN,
            params![
                availability,
                "",
                Option::<String>::None,
                Option::<String>::None,
                "",
                "",
                50
            ],
        )?;
        print_plan(
            connection,
            &format!("prototype available={availability}"),
            if availability {
                AVAILABLE_CANDIDATES
            } else {
                UNAVAILABLE_CANDIDATES
            },
            params![
                "",
                Option::<String>::None,
                Option::<String>::None,
                "",
                "",
                50
            ],
        )?;
    }
    Ok(())
}

fn print_plan<P: rusqlite::Params>(
    connection: &Connection,
    label: &str,
    query: &str,
    parameters: P,
) -> rusqlite::Result<()> {
    println!("  EXPLAIN QUERY PLAN — {label}:");
    let mut statement = connection.prepare(&format!("EXPLAIN QUERY PLAN {query}"))?;
    let rows = statement.query_map(parameters, |row| row.get::<_, String>(3))?;
    let details = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    if label.starts_with("production branch") {
        let lookups: Vec<_> = details
            .iter()
            .filter(|line| line.starts_with("SEARCH ts ") || line.starts_with("SEARCH l "))
            .collect();
        assert_eq!(
            lookups.len(),
            4,
            "{label}: both availability checks must be present"
        );
        for pair in lookups.as_chunks::<2>().0 {
            assert!(
                pair[0].starts_with("SEARCH ts ") && pair[0].contains("(track_id=?)"),
                "{label}: {pair:?}"
            );
            assert!(
                pair[1].starts_with("SEARCH l ") && pair[1].contains("source_id=?"),
                "{label}: {pair:?}"
            );
        }
    }
    for row in details {
        println!("    {row}");
    }
    Ok(())
}

fn format_duration(duration: Duration) -> String {
    if duration.as_millis() >= 1 {
        format!("{:.3} ms", duration.as_secs_f64() * 1_000.0)
    } else {
        format!("{:.3} µs", duration.as_secs_f64() * 1_000_000.0)
    }
}
