//! Benchmark-only access to the unchanged Store implementation and its connection.
use music_library::matching;
use std::hint::black_box;
use std::time::Instant;
#[allow(dead_code)]
#[path = "../src/catalog.rs"]
mod catalog;
#[allow(dead_code)]
#[path = "../src/domain.rs"]
mod domain;
use domain::{SearchRequest, TrackSearchResult};
use rusqlite::{Connection, StatementStatus, params};

#[allow(dead_code)]
mod storage {
    include!("../src/storage.rs");
    impl Store {
        pub fn audit_connection(&self) -> &rusqlite::Connection {
            &self.connection
        }
    }
    pub fn audit_map(
        row: &rusqlite::Row<'_>,
    ) -> rusqlite::Result<crate::domain::TrackSearchResult> {
        map_search_result(row)
    }
}
#[allow(dead_code)]
mod fixture {
    include!("availability_query_comparison.rs");
    pub fn setup() -> Result<(), Box<dyn std::error::Error>> {
        copy_fixture(
            Path::new(SOURCELESS_DATABASE),
            Path::new(DIAGNOSTIC_DATABASE),
        )
    }
    pub fn distribution(index: usize) -> Result<&'static str, Box<dyn std::error::Error>> {
        let d = Distribution::ALL[index];
        install_distribution(Path::new(DIAGNOSTIC_DATABASE), d)?;
        Ok(d.label)
    }
    pub fn sql() -> String {
        production_queries()[1].clone()
    }
    pub fn cursor() -> SearchCursor {
        deep_cursor()
    }
    pub const PATH: &str = DIAGNOSTIC_DATABASE;
}
fn db_status(c: &Connection, op: i32) -> i32 {
    let (mut current, mut high) = (0, 0);
    // The borrowed connection stays alive and is used only on this thread.
    let rc =
        unsafe { rusqlite::ffi::sqlite3_db_status(c.handle(), op, &mut current, &mut high, 0) };
    assert_eq!(rc, rusqlite::ffi::SQLITE_OK);
    current
}
fn state(c: &Connection) -> Vec<(String, String)> {
    [
        "cache_size",
        "page_size",
        "mmap_size",
        "journal_mode",
        "synchronous",
        "temp_store",
        "cache_spill",
        "automatic_index",
        "foreign_keys",
        "busy_timeout",
        "query_only",
        "read_uncommitted",
    ]
    .into_iter()
    .map(|p| {
        let mut s = c.prepare(&format!("PRAGMA {p}")).unwrap();
        let value = s
            .query_row([], |r| Ok(format!("{:?}", r.get_ref(0)?)))
            .unwrap();
        (p.into(), value)
    })
    .collect()
}
fn execute(
    c: &Connection,
    sql: &str,
    request: &SearchRequest,
) -> (Vec<TrackSearchResult>, [i32; 5], f64) {
    let before = [db_status(c, 7), db_status(c, 8)];
    let start = Instant::now();
    let mut s = c.prepare(sql).unwrap();
    let prepared = start.elapsed().as_secs_f64() * 1000.;
    let rows = s
        .query_map(
            params![
                "",
                Option::<String>::None,
                Option::<String>::None,
                request.availability,
                request
                    .after
                    .as_ref()
                    .map(|c| c.title.as_str())
                    .unwrap_or(""),
                request
                    .after
                    .as_ref()
                    .map(|c| c.track_id.as_ref())
                    .unwrap_or(""),
                50
            ],
            storage::audit_map,
        )
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let counts = [
        s.get_status(StatementStatus::VmStep),
        s.get_status(StatementStatus::FullscanStep),
        s.get_status(StatementStatus::Sort),
        db_status(c, 7) - before[0],
        db_status(c, 8) - before[1],
    ];
    (rows, counts, prepared)
}
fn contention() -> Result<(), Box<dyn std::error::Error>> {
    fixture::setup()?;
    let sql = fixture::sql();
    let reverse = std::env::args().any(|a| a == "--reverse");
    for d in [0] {
        let label = fixture::distribution(d)?;
        let store = storage::Store::open(fixture::PATH)?;
        let c = Connection::open(fixture::PATH)?;
        c.execute_batch(
            "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;",
        )?;
        let _: u32 = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
        println!(
            "{label} app {:?} control {:?}",
            state(store.audit_connection()),
            state(&c)
        );
        for available in [true, false] {
            for deep in [false, true] {
                let request = SearchRequest {
                    availability: Some(available),
                    after: deep.then(|| {
                        let c = fixture::cursor();
                        domain::SearchCursor {
                            title: c.title,
                            track_id: domain::TrackId(c.track_id.0),
                        }
                    }),
                    limit: 50,
                    ..Default::default()
                };
                let expected = if reverse {
                    raw(&c, &sql, &request)?
                } else {
                    store.search(&request)?
                };
                let io_before = io();
                let mut times = [vec![], vec![], vec![]];
                let mut counters = [[0; 5]; 2];
                for round in 0..6 {
                    for arm in if (round % 2 == 0) != reverse {
                        [0, 1, 2]
                    } else {
                        [2, 1, 0]
                    } {
                        let start = Instant::now();
                        let result = match arm {
                            0 => store.search(&request)?,
                            _ => {
                                let (r, cnt, _) = execute(
                                    if arm == 1 {
                                        store.audit_connection()
                                    } else {
                                        &c
                                    },
                                    &sql,
                                    &request,
                                );
                                counters[arm - 1] = cnt;
                                r
                            }
                        };
                        let elapsed = start.elapsed().as_secs_f64() * 1000.;
                        assert_eq!(result, expected);
                        black_box(result);
                        if round >= 4 {
                            times[arm].push(elapsed);
                        }
                    }
                }
                for t in &mut times {
                    t.sort_by(f64::total_cmp);
                }
                println!("OS IO delta {:?}", {
                    let end = io();
                    (end.0 - io_before.0, end.1 - io_before.1)
                });
                println!(
                    "cache bytes app={} control={}",
                    db_status(store.audit_connection(), 1),
                    db_status(&c, 1)
                );
                println!(
                    "{label} available={available} deep={deep} ms={:?} counters={counters:?}",
                    times.map(|t| t[t.len() / 2])
                );
            }
        }
        drop(store);
        let request = SearchRequest {
            availability: Some(true),
            limit: 50,
            ..Default::default()
        };
        for _ in 0..3 {
            let start = Instant::now();
            let (_, cnt, _) = execute(&c, &sql, &request);
            println!(
                "after closing competitor ms={} counters={cnt:?} cache={}",
                start.elapsed().as_secs_f64() * 1000.,
                db_status(&c, 1)
            );
        }
    }
    Ok(())
}

const WARMUPS: usize = 4;
const SAMPLES: usize = 12;
#[derive(Debug)]
struct Measurement {
    ms: Vec<f64>,
    counts: [i32; 8], // VM, full scan, sort, autoindex, reprepare, hit, miss, temp spill
    prep_ms: f64,
    read_bytes: u64,
    read_calls: u64,
    cache_bytes: i32,
    censored: bool,
}
fn io() -> (u64, u64) {
    let text = std::fs::read_to_string("/proc/self/io").unwrap_or_default();
    let get = |key| {
        text.lines()
            .find_map(|l| l.strip_prefix(key))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0)
    };
    (get("read_bytes:"), get("syscr:"))
}
unsafe extern "C" fn deadline(pointer: *mut std::ffi::c_void) -> i32 {
    // Installed synchronously below; Instant outlives every callback.
    let started = unsafe { &*pointer.cast::<Instant>() };
    i32::from(started.elapsed().as_secs_f64() >= 2.)
}
fn legacy_sql(sql: &str) -> String {
    sql.replace(
        "CROSS JOIN local_file_observation l",
        "JOIN local_file_observation l ON l.source_id = ts.source_id",
    )
    .replace(" AND l.source_id = ts.source_id AND", " AND")
}
fn raw(
    c: &Connection,
    sql: &str,
    request: &SearchRequest,
) -> rusqlite::Result<Vec<TrackSearchResult>> {
    let mut s = c.prepare(sql)?;
    s.query_map(
        params![
            "",
            Option::<String>::None,
            Option::<String>::None,
            request.availability,
            request
                .after
                .as_ref()
                .map(|c| c.title.as_str())
                .unwrap_or(""),
            request
                .after
                .as_ref()
                .map(|c| c.track_id.as_ref())
                .unwrap_or(""),
            50u32
        ],
        storage::audit_map,
    )?
    .collect()
}
fn counters(c: &Connection, sql: &str, request: &SearchRequest) -> ([i32; 8], f64) {
    let before = [db_status(c, 7), db_status(c, 8), db_status(c, 13)];
    let start = Instant::now();
    let mut s = c.prepare(sql).unwrap();
    let prep = start.elapsed().as_secs_f64() * 1000.;
    let rows = s
        .query_map(
            params![
                "",
                Option::<String>::None,
                Option::<String>::None,
                request.availability,
                request
                    .after
                    .as_ref()
                    .map(|c| c.title.as_str())
                    .unwrap_or(""),
                request
                    .after
                    .as_ref()
                    .map(|c| c.track_id.as_ref())
                    .unwrap_or(""),
                50u32
            ],
            storage::audit_map,
        )
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    black_box(rows);
    (
        [
            s.get_status(StatementStatus::VmStep),
            s.get_status(StatementStatus::FullscanStep),
            s.get_status(StatementStatus::Sort),
            s.get_status(StatementStatus::AutoIndex),
            s.get_status(StatementStatus::RePrepare),
            db_status(c, 7) - before[0],
            db_status(c, 8) - before[1],
            db_status(c, 13) - before[2],
        ],
        prep,
    )
}
fn block(
    arm: &str,
    sql: &str,
    request: &SearchRequest,
    expected: &mut Option<String>,
) -> Measurement {
    let mut m = Measurement {
        ms: vec![],
        counts: [0; 8],
        prep_ms: 0.,
        read_bytes: 0,
        read_calls: 0,
        cache_bytes: 0,
        censored: false,
    };
    if arm == "public" {
        // The public Library remains encapsulated. This block opens no other connection.
        let library = music_library::Library::open(fixture::PATH).unwrap();
        let req = music_library::domain::SearchRequest {
            availability: request.availability,
            after: request
                .after
                .as_ref()
                .map(|c| music_library::domain::SearchCursor {
                    title: c.title.clone(),
                    track_id: music_library::domain::TrackId(c.track_id.0.clone()),
                }),
            limit: 50,
            ..Default::default()
        };
        for i in 0..WARMUPS + SAMPLES {
            let before = io();
            let start = Instant::now();
            let rows = black_box(library.search(&req).unwrap());
            let ms = start.elapsed().as_secs_f64() * 1000.;
            let after = io();
            assert_eq!(expected.as_ref().unwrap(), &format!("{rows:?}"));
            if i >= WARMUPS {
                m.ms.push(ms);
                m.read_bytes += after.0 - before.0;
                m.read_calls += after.1 - before.1;
            }
        }
        return m;
    }
    // Compile the unchanged Store source into this benchmark to inspect its private connection.
    // No accessor or instrumentation is added to the production crate.
    let store = storage::Store::open(fixture::PATH).unwrap();
    let c = store.audit_connection();
    let old = legacy_sql(sql);
    let query = if arm == "before" { &old } else { sql };
    for i in 0..WARMUPS + SAMPLES {
        let timeout = Instant::now();
        if arm == "before" && i == 0 {
            // Censor pathological historical cases; never treat interrupted work as a timing sample.
            unsafe {
                rusqlite::ffi::sqlite3_progress_handler(
                    c.handle(),
                    10000,
                    Some(deadline),
                    std::ptr::from_ref(&timeout).cast_mut().cast(),
                );
            }
        }
        let before = io();
        let start = Instant::now();
        let result = if arm == "store" {
            store.search(request).map_err(|e| e.to_string())
        } else {
            raw(c, query, request).map_err(|e| e.to_string())
        };
        let ms = start.elapsed().as_secs_f64() * 1000.;
        let after = io();
        if arm == "before" && i == 0 {
            unsafe {
                rusqlite::ffi::sqlite3_progress_handler(c.handle(), 0, None, std::ptr::null_mut());
            }
        }
        let rows = match result {
            Ok(r) => r,
            Err(e) if arm == "before" && e.contains("interrupted") => {
                m.censored = true;
                m.ms.clear();
                break;
            }
            Err(e) => panic!("{e}"),
        };
        let value = format!("{rows:?}");
        if let Some(expected) = expected {
            assert_eq!(expected, &value);
        } else {
            *expected = Some(value);
        }
        black_box(rows);
        if i >= WARMUPS {
            m.ms.push(ms);
            m.read_bytes += after.0 - before.0;
            m.read_calls += after.1 - before.1;
        }
    }
    if !m.censored {
        (m.counts, m.prep_ms) = counters(c, query, request);
        m.cache_bytes = db_status(c, 1);
    }
    if arm == "store" {
        verify_trace(&store, sql, request, m.counts[..5].try_into().unwrap());
    }
    m
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|a| a == "--contention") {
        return contention();
    }
    fixture::setup()?;
    let sql = fixture::sql();
    println!(
        "SQLite {} warmups={WARMUPS} samples/block={SAMPLES}; two reverse-order blocks; one live connection",
        rusqlite::version()
    );
    {
        let store = storage::Store::open(fixture::PATH)?;
        println!("PRAGMAs {:?}", state(store.audit_connection()));
        println!(
            "memory_management={}",
            store.audit_connection().query_row(
                "SELECT sqlite_compileoption_used('ENABLE_MEMORY_MANAGEMENT')",
                [],
                |r| r.get::<_, i32>(0)
            )?
        );
    }
    for d in [0, 1, 3, 4, 5] {
        let label = fixture::distribution(d)?;
        for available in [true, false] {
            for deep in [false, true] {
                let request = SearchRequest {
                    availability: Some(available),
                    after: deep.then(|| {
                        let c = fixture::cursor();
                        domain::SearchCursor {
                            title: c.title,
                            track_id: domain::TrackId(c.track_id.0),
                        }
                    }),
                    limit: 50,
                    ..Default::default()
                };
                let mut expected = None;
                // Establish expected values without keeping a connection open during any block.
                {
                    let store = storage::Store::open(fixture::PATH)?;
                    expected.replace(format!("{:?}", store.search(&request)?));
                }
                for order in [
                    ["store", "control", "before", "public"],
                    ["public", "before", "control", "store"],
                ] {
                    for arm in order {
                        if arm == "before" && std::env::args().any(|a| a == "--post-only") {
                            continue;
                        }
                        let mut m = block(arm, &sql, &request, &mut expected);
                        m.ms.sort_by(f64::total_cmp);
                        println!(
                            "{label}|{available}|{deep}|{arm}|median={}|range={:?}|counts={}|prep={}|read_bytes={}|read_calls={}|cache={}|censored={}",
                            if m.censored {
                                "n/a".into()
                            } else {
                                format!("{:.6}", m.ms[m.ms.len() / 2])
                            },
                            m.ms.first().zip(m.ms.last()),
                            if arm == "public" || m.censored {
                                "n/a".into()
                            } else {
                                format!("{:?}", m.counts)
                            },
                            if arm == "public" || m.censored {
                                "n/a".into()
                            } else {
                                format!("{:.6}", m.prep_ms)
                            },
                            m.read_bytes,
                            m.read_calls,
                            m.cache_bytes,
                            m.censored
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

#[derive(Default)]
struct Trace {
    sql: String,
    counts: [i32; 5],
}
unsafe extern "C" fn profile(
    _: u32,
    context: *mut std::ffi::c_void,
    statement: *mut std::ffi::c_void,
    _: *mut std::ffi::c_void,
) -> i32 {
    // SQLite invokes PROFILE synchronously while the statement and callback context are alive.
    unsafe {
        let stmt = statement.cast::<rusqlite::ffi::sqlite3_stmt>();
        let sql = rusqlite::ffi::sqlite3_sql(stmt);
        if sql.is_null()
            || !std::ffi::CStr::from_ptr(sql)
                .to_bytes()
                .starts_with(b"SELECT e.track_id")
        {
            return 0;
        }
        let expanded = rusqlite::ffi::sqlite3_expanded_sql(stmt);
        if expanded.is_null() {
            return 0;
        }
        let trace = &mut *context.cast::<Trace>();
        trace.sql = std::ffi::CStr::from_ptr(expanded)
            .to_string_lossy()
            .into_owned();
        rusqlite::ffi::sqlite3_free(expanded.cast());
        trace.counts = [4, 1, 2, 3, 5].map(|op| rusqlite::ffi::sqlite3_stmt_status(stmt, op, 0));
    }
    0
}
fn verify_trace(
    store: &storage::Store,
    sql: &str,
    request: &SearchRequest,
    expected_counts: [i32; 5],
) {
    let c = store.audit_connection();
    let mut trace = Trace::default();
    // Register only for an untimed verification. No callback remains installed afterward.
    unsafe {
        assert_eq!(
            rusqlite::ffi::sqlite3_trace_v2(
                c.handle(),
                rusqlite::ffi::SQLITE_TRACE_PROFILE,
                Some(profile),
                std::ptr::from_mut(&mut trace).cast()
            ),
            0
        );
    }
    let result = store.search(request);
    unsafe {
        rusqlite::ffi::sqlite3_trace_v2(c.handle(), 0, None, std::ptr::null_mut());
    }
    result.unwrap();
    let mut statement = c.prepare(sql).unwrap();
    let rows = statement
        .query_map(
            params![
                "",
                Option::<String>::None,
                Option::<String>::None,
                request.availability,
                request
                    .after
                    .as_ref()
                    .map(|c| c.title.as_str())
                    .unwrap_or(""),
                request
                    .after
                    .as_ref()
                    .map(|c| c.track_id.as_ref())
                    .unwrap_or(""),
                50u32
            ],
            storage::audit_map,
        )
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    black_box(rows);
    assert_eq!(
        trace.sql,
        statement.expanded_sql().unwrap(),
        "production SQL/bound values differ"
    );
    assert_eq!(
        trace.counts, expected_counts,
        "production execution counters differ"
    );
}
