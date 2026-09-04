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

Titles contain controlled terms distributed evenly by Track number:

| Term | Matching Tracks | Purpose |
| --- | ---: | --- |
| `Quasar` | 20 (0.01%) | Rare query with fewer matches than one 50-row page |
| `Nocturne` | 1,000 (0.5%) | Moderate-frequency query |
| `Love` | 20,000 (10%) | Common but plausible library term |
| `Track` | 200,000 (100%) | Pathological stress case, not representative |

Terms can overlap deliberately. Their placement is deterministic and distributed through title
sort order rather than clustered into a single title prefix.

Run a clean release-mode measurement with:

```sh
cargo run --release --example database_performance -- --rebuild
```

The default database is `target/performance/library-200k.sqlite`. A different location can be used
with `--database PATH`. Omit `--rebuild` to reuse and validate an existing fixture.

The harness measures existing operations without setting performance thresholds:

* Initial and repeated database open.
* First-page library navigation.
* Rare, moderate, common, and pathological FTS searches.
* Release filtering.
* Availability filtering.
* Deep keyset pagination.
* Membership removal/addition.
* Effective-metadata and FTS updates caused by a title override.

Results include one untimed warm-up followed by 20 measured iterations. OS filesystem caching,
hardware, build profile, background activity, and SQLite version all affect results. “First open in
process” is not a controlled cold-start measurement because the fixture has just been validated.

## Historical Initial Observation

The first release-mode run on 2026-09-02 used an earlier, less representative title distribution.
It produced the following results in the development container on Linux x86-64 with SQLite 3.53.2.
These values are retained as historical context, not accepted budgets.

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

## Representative FTS Distribution Observation

After introducing the controlled search-term distribution, a release-mode run on 2026-09-03 in
the same development environment produced:

| Query | Matches | Median | p95 |
| --- | ---: | ---: | ---: |
| Rare `Quasar` prefix | 20 | 11.120 ms | 11.798 ms |
| Moderate `Nocturne` prefix | 1,000 | 0.769 ms | 0.809 ms |
| Common `Love` prefix | 20,000 | 2.602 ms | 2.723 ms |
| Common `Love` plus unique number | 1 | 11.115 ms | 11.898 ms |
| Pathological `Track` prefix | 200,000 | 25.490 ms | 26.178 ms |
| Pathological `Track` plus unique number | 1 | 16.764 ms | 18.135 ms |

The moderate and common single-term cases do not show a general FTS performance problem at this
scale. Rare and selective queries that return fewer than the requested 50 rows still expose the
existing ordered outer-scan behavior: the query exhausts the title-ordered library to prove that
there are no more matches. The universal `Track` term additionally pays for its 200,000-entry FTS
posting list and remains a deliberately non-representative stress case. No production query was
changed in response to these measurements.

## FTS Candidate-Driven Query Experiment

The diagnostic comparison can be run after creating the fixture:

```sh
cargo run --release --example fts_query_comparison
```

It compares the production operation with an experimental query that starts at matching FTS rows,
looks up effective metadata by the shared rowid, applies the cursor, and sorts the candidates by
the existing stable `(title, Track ID)` order. It asserts identical ordered results across first,
second, and selected deep pages.

The 2026-09-03 comparison produced these medians:

| Query | Current | Candidate-driven |
| --- | ---: | ---: |
| Rare `Quasar`, first page | 11.132 ms | 0.118 ms |
| Moderate `Nocturne`, first page | 0.777 ms | 4.966 ms |
| Common `Love`, first page | 2.708 ms | 23.065 ms |
| `Love` plus unique number, first page | 11.389 ms | 0.598 ms |
| Pathological `Track`, first page | 26.081 ms | 120.766 ms |
| Rare `Quasar`, deep cursor | 11.078 ms | 0.111 ms |
| Moderate `Nocturne`, deep cursor | 10.864 ms | 1.946 ms |
| Common `Love`, deep cursor | 15.947 ms | 9.899 ms |
| Pathological `Track`, deep cursor | 69.319 ms | 29.503 ms |

The experiment demonstrates a real tradeoff. The production shape preserves title order by
scanning the title index and can stop as soon as a page is full; it is strong for early pages of
moderate and broad result sets. The candidate-driven shape avoids global exhaustion for small
result sets, but must sort all qualifying FTS candidates before returning an early page. It is not
a suitable unconditional replacement. No production SQL or schema has been changed.
