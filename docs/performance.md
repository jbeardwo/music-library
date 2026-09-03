# Performance Baselines

Performance measurements are observations, not budgets. Run them on representative target
systems before using them to accept or reject a change.

## Deterministic Database Fixture

The database performance harness creates:

* 200,000 source-less Tracks.
* 20,000 Releases with ten Tracks each.
* 1,000 Artists assigned deterministically to ordered credits.
* Membership and effective metadata for every Track.
* An FTS5 row for every Track.
* No PlayableSources or filesystem observations.

IDs, titles, credit assignments, years, disc numbers, and Track numbers are deterministic. Creation
uses direct SQL so fixture setup does not measure or constrain interactive application operations.

Run a clean release-mode measurement with:

```sh
cargo run --release --example database_performance -- --rebuild
```

The default database is `target/performance/library-200k.sqlite`. A different location can be used
with `--database PATH`. Omit `--rebuild` to reuse and validate an existing fixture.

The harness measures existing operations without setting performance thresholds:

* Initial and repeated database open.
* First-page library navigation.
* Common and selective FTS searches.
* Release filtering.
* Availability filtering.
* Deep keyset pagination.
* Membership removal/addition.
* Effective-metadata and FTS updates caused by a title override.

Results include one untimed warm-up followed by 20 measured iterations. OS filesystem caching,
hardware, build profile, background activity, and SQLite version all affect results. “First open in
process” is not a controlled cold-start measurement because the fixture has just been validated.

## Initial Observation

The first release-mode run on 2026-09-02 produced the following results in the development
container on Linux x86-64 with SQLite 3.53.2. These values are a baseline observation, not accepted
budgets.

| Operation | Median | p95 |
| --- | ---: | ---: |
| Subsequent database open | 0.161 ms | 0.215 ms |
| First library page, 50 rows | 0.156 ms | 0.191 ms |
| Common FTS prefix, 50 rows | 2.547 ms | 2.678 ms |
| Selective two-token FTS query, 1 row | 15.574 ms | 15.885 ms |
| Release filter, 10 rows | 74.547 ms | 77.881 ms |
| Unavailable filter, 50 rows | 0.171 ms | 0.213 ms |
| Deep keyset page, 50 rows | 8.538 ms | 8.619 ms |
| Remove and re-add membership | 4.704 ms | 5.165 ms |
| Set and clear title override | 5.431 ms | 11.999 ms |

Fixture creation took 2.635 seconds and produced a 122.7 MiB database. The first database open in
the measurement process took 0.392 ms. Release-filter and selective-search timings are recorded as
observed; no optimization was performed in response.
