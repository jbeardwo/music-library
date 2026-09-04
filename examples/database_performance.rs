use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use music_library::Library;
use music_library::domain::{ReleaseId, SearchCursor, SearchRequest, TrackId};
use rusqlite::Connection;

const TRACK_COUNT: i64 = 200_000;
const RELEASE_COUNT: i64 = 20_000;
const ARTIST_COUNT: i64 = 1_000;
const ITERATIONS: usize = 20;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = Options::parse()?;
    if options.rebuild {
        remove_database_files(&options.database)?;
    }
    if !options.database.exists() {
        create_fixture(&options.database)?;
    }
    validate_fixture(&options.database)?;
    run_measurements(&options.database)?;
    Ok(())
}

struct Options {
    database: PathBuf,
    rebuild: bool,
}

impl Options {
    fn parse() -> Result<Self, String> {
        let mut database = PathBuf::from("target/performance/library-200k.sqlite");
        let mut rebuild = false;
        let mut arguments = std::env::args().skip(1);
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--database" => {
                    database = arguments
                        .next()
                        .map(PathBuf::from)
                        .ok_or_else(|| "--database requires a path".to_owned())?;
                }
                "--rebuild" => rebuild = true,
                "--help" | "-h" => {
                    println!(
                        "Usage: cargo run --release --example database_performance -- \
                         [--database PATH] [--rebuild]"
                    );
                    std::process::exit(0);
                }
                unknown => return Err(format!("unknown argument: {unknown}")),
            }
        }
        Ok(Self { database, rebuild })
    }
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

fn create_fixture(database: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = database.parent() {
        fs::create_dir_all(parent)?;
    }
    let started = Instant::now();
    drop(Library::open(database)?);

    let mut connection = Connection::open(database)?;
    connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")?;
    let transaction = connection.transaction()?;
    transaction.execute_batch(
        "CREATE TEMP TABLE fixture_number(value INTEGER PRIMARY KEY);
         WITH RECURSIVE numbers(value) AS (
             SELECT 1 UNION ALL SELECT value + 1 FROM numbers WHERE value < 200000
         )
         INSERT INTO fixture_number SELECT value FROM numbers;

         INSERT INTO release(id)
         SELECT printf('release-%05d', value) FROM fixture_number WHERE value <= 20000;

         INSERT INTO release_application_metadata(release_id, title, year)
         SELECT printf('release-%05d', value),
                printf('Release %05d', value),
                1950 + (value % 76)
         FROM fixture_number WHERE value <= 20000;

         INSERT INTO artist(id, name)
         SELECT printf('artist-%04d', value), printf('Artist %04d', value)
         FROM fixture_number WHERE value <= 1000;

         INSERT INTO release_artist_credit(release_id, position, artist_id, role)
         SELECT printf('release-%05d', value), 0,
                printf('artist-%04d', ((value - 1) % 1000) + 1), 'primary'
         FROM fixture_number WHERE value <= 20000;

         INSERT INTO track(id, release_id, disc_number, track_number)
         SELECT printf('track-%06d', value),
                printf('release-%05d', ((value - 1) / 10) + 1),
                (((value - 1) % 10) / 5) + 1,
                ((value - 1) % 5) + 1
         FROM fixture_number;

         INSERT INTO track_application_metadata(track_id, title)
         SELECT printf('track-%06d', value),
                printf(
                    'Song %06d Track%s%s%s',
                    value,
                    CASE WHEN value % 10000 = 0 THEN ' Quasar' ELSE '' END,
                    CASE WHEN value % 200 = 0 THEN ' Nocturne' ELSE '' END,
                    CASE WHEN value % 10 = 0 THEN ' Love' ELSE '' END
                )
         FROM fixture_number;

         INSERT INTO track_artist_credit(track_id, position, artist_id, role)
         SELECT printf('track-%06d', value), 0,
                printf('artist-%04d', ((value - 1) % 1000) + 1), 'primary'
         FROM fixture_number;

         INSERT INTO library_membership(track_id)
         SELECT printf('track-%06d', value) FROM fixture_number;

         INSERT INTO effective_track_metadata(
             track_id, title, release_title, artist_names, year, duration_ms, format
         )
         SELECT t.id, tm.title, rm.title, a.name, rm.year, NULL, NULL
         FROM track t
         JOIN track_application_metadata tm ON tm.track_id = t.id
         JOIN release_application_metadata rm ON rm.release_id = t.release_id
         JOIN track_artist_credit credit ON credit.track_id = t.id AND credit.position = 0
         JOIN artist a ON a.id = credit.artist_id;

         INSERT INTO track_search(rowid, track_id, title, artist_names, release_title)
         SELECT rowid, track_id, title, artist_names, release_title
         FROM effective_track_metadata;

         DROP TABLE fixture_number;",
    )?;
    transaction.commit()?;
    println!(
        "fixture creation: {} Tracks, {} Releases, {} Artists in {:.3}s",
        TRACK_COUNT,
        RELEASE_COUNT,
        ARTIST_COUNT,
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

fn validate_fixture(database: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let connection = Connection::open(database)?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    for (table, expected) in [
        ("track", TRACK_COUNT),
        ("release", RELEASE_COUNT),
        ("artist", ARTIST_COUNT),
        ("library_membership", TRACK_COUNT),
        ("effective_track_metadata", TRACK_COUNT),
        ("track_search", TRACK_COUNT),
    ] {
        let sql = format!("SELECT count(*) FROM {table}");
        let actual: i64 = connection.query_row(&sql, [], |row| row.get(0))?;
        if actual != expected {
            return Err(format!("fixture {table} count is {actual}, expected {expected}").into());
        }
    }
    let sources: i64 =
        connection.query_row("SELECT count(*) FROM playable_source", [], |row| row.get(0))?;
    if sources != 0 {
        return Err(format!("fixture unexpectedly contains {sources} playable sources").into());
    }
    for (term, expected) in [
        ("Quasar", 20),
        ("Nocturne", 1_000),
        ("Love", 20_000),
        ("Track", TRACK_COUNT),
    ] {
        let query = format!("\"{term}\"*");
        let actual: i64 = connection.query_row(
            "SELECT count(*) FROM track_search WHERE track_search MATCH ?1",
            [query],
            |row| row.get(0),
        )?;
        if actual != expected {
            return Err(format!(
                "fixture search term {term} matches {actual} Tracks, expected {expected}"
            )
            .into());
        }
    }
    Ok(())
}

fn run_measurements(database: &Path) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "environment: {} {} | SQLite {} | database: {} ({:.1} MiB)",
        std::env::consts::OS,
        std::env::consts::ARCH,
        rusqlite::version(),
        database.display(),
        fs::metadata(database)?.len() as f64 / 1_048_576.0
    );

    let first_open = Instant::now();
    let mut library = Library::open(database)?;
    println!(
        "first open in process: {}",
        format_duration(first_open.elapsed())
    );

    measure("subsequent database open", ITERATIONS, || {
        black_box(Library::open(database)?);
        Ok(())
    })?;
    measure("first library page (50)", ITERATIONS, || {
        let rows = library.search(&SearchRequest {
            limit: 50,
            ..SearchRequest::default()
        })?;
        ensure_count("first library page", &rows, 50)
    })?;
    measure("rare FTS prefix (20 of 200k)", ITERATIONS, || {
        let rows = library.search(&SearchRequest {
            text: "Quasar".into(),
            limit: 50,
            ..SearchRequest::default()
        })?;
        ensure_count("rare FTS prefix", &rows, 20)
    })?;
    measure("moderate FTS prefix (1k of 200k)", ITERATIONS, || {
        let rows = library.search(&SearchRequest {
            text: "Nocturne".into(),
            limit: 50,
            ..SearchRequest::default()
        })?;
        ensure_count("moderate FTS prefix", &rows, 50)
    })?;
    measure("common FTS prefix (20k of 200k)", ITERATIONS, || {
        let rows = library.search(&SearchRequest {
            text: "Love".into(),
            limit: 50,
            ..SearchRequest::default()
        })?;
        ensure_count("common FTS prefix", &rows, 50)
    })?;
    measure("common plus unique FTS query (1)", ITERATIONS, || {
        let rows = library.search(&SearchRequest {
            text: "Love 123450".into(),
            limit: 50,
            ..SearchRequest::default()
        })?;
        ensure_count("common plus unique FTS query", &rows, 1)
    })?;
    measure("pathological FTS prefix (200k of 200k)", ITERATIONS, || {
        let rows = library.search(&SearchRequest {
            text: "Track".into(),
            limit: 50,
            ..SearchRequest::default()
        })?;
        ensure_count("pathological FTS prefix", &rows, 50)
    })?;
    measure("pathological plus unique FTS query (1)", ITERATIONS, || {
        let rows = library.search(&SearchRequest {
            text: "Track 123457".into(),
            limit: 50,
            ..SearchRequest::default()
        })?;
        ensure_count("pathological plus unique FTS query", &rows, 1)
    })?;
    measure("Release filter page (10)", ITERATIONS, || {
        let rows = library.search(&SearchRequest {
            release_id: Some(ReleaseId("release-10000".into())),
            limit: 50,
            ..SearchRequest::default()
        })?;
        ensure_count("Release filter page", &rows, 10)
    })?;
    measure("unavailable filter page (50)", ITERATIONS, || {
        let rows = library.search(&SearchRequest {
            availability: Some(false),
            limit: 50,
            ..SearchRequest::default()
        })?;
        ensure_count("unavailable filter page", &rows, 50)
    })?;
    measure("deep keyset page (50)", ITERATIONS, || {
        let rows = library.search(&SearchRequest {
            after: Some(SearchCursor {
                title: "Song 100000 Track Quasar Nocturne Love".into(),
                track_id: TrackId("track-100000".into()),
            }),
            limit: 50,
            ..SearchRequest::default()
        })?;
        ensure_count("deep keyset page", &rows, 50)
    })?;

    let track_id = TrackId("track-123457".into());
    measure("remove and re-add membership", ITERATIONS, || {
        if !library.remove_from_library(&track_id)? || !library.add_to_library(&track_id)? {
            return Err("membership mutation did not change state".into());
        }
        Ok(())
    })?;
    measure("set and clear title override", ITERATIONS, || {
        library.set_track_title_override(&track_id, "Measured Override")?;
        if !library.clear_track_title_override(&track_id)? {
            return Err("title override was not cleared".into());
        }
        Ok(())
    })?;
    Ok(())
}

fn ensure_count<T>(
    label: &str,
    rows: &[T],
    expected: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    black_box(rows);
    if rows.len() != expected {
        return Err(format!("{label} returned {}, expected {expected}", rows.len()).into());
    }
    Ok(())
}

fn measure(
    label: &str,
    iterations: usize,
    mut operation: impl FnMut() -> Result<(), Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    operation()?;
    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let started = Instant::now();
        operation()?;
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    let median = samples[samples.len() / 2];
    let p95 = samples[(samples.len() * 95).div_ceil(100) - 1];
    println!(
        "{label}: median {}, p95 {}, min {} ({iterations} iterations)",
        format_duration(median),
        format_duration(p95),
        format_duration(samples[0]),
    );
    Ok(())
}

fn format_duration(duration: Duration) -> String {
    if duration.as_millis() >= 1 {
        format!("{:.3} ms", duration.as_secs_f64() * 1_000.0)
    } else {
        format!("{:.3} µs", duration.as_secs_f64() * 1_000_000.0)
    }
}
