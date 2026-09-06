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
SQLite's [query-planning discussion of searching and sorting](https://www.sqlite.org/queryplanner.html)
explains the competing costs of selective lookups followed by sorting versus scanning an index in
`ORDER BY` order. This is the tradeoff measured here and in the Artist and availability experiments.

## Release-Filtered Search Optimization

The generic search shape expressed Release filtering as an optional predicate:

```sql
? IS NULL OR track.release_id = ?
```

On the 200,000-Track fixture, SQLite scanned `effective_track_metadata` globally through
`effective_track_title`, looked up each Track and membership record, and exhausted the result set
because the selected Release had only 10 Tracks for a 50-row page. The existing
`track_release_order` index was not used to select candidates.

The production search operation now uses a dedicated SQL shape when a Release ID is present. Its
direct `track.release_id = ?` predicate starts with `track_release_order`, looks up effective and
membership rows by their existing indexes, and uses a temporary B-tree to sort only the matched
Release Tracks into stable `(title, Track ID)` order.

The 2026-09-03 release-mode comparison measured:

| Release-filter shape | Median | p95 |
| --- | ---: | ---: |
| Legacy optional predicate | 61.315 ms | 70.388 ms |
| Dedicated direct predicate | 0.110 ms | 0.144 ms |

The dedicated query retains free-text FTS, Artist, availability, membership, and keyset-cursor
predicates. No schema or index change was needed.

Separate diagnostics found equivalent global title-scan behavior for other selective optional
filters. An Artist matching 200 Tracks took 281.185 ms to return its first 50 rows. An
`available=true` query with no matches in the source-less fixture took 694.188 ms. Those paths were
not changed as part of the Release-focused optimization.

## Artist Candidate-Driven Query Experiment

The fixture includes three additional deterministic ordered Track credits for Artist-filter
diagnostics: a rare Artist credited on 20 Tracks, a moderate Artist credited on 200 Tracks, and a
common Artist credited on 20,000 Tracks. They do not replace the fixture's ordinary primary
credits. Rebuild the fixture, then run the comparison with:

```sh
cargo run --release --example database_performance -- --rebuild
cargo run --release --example artist_query_comparison
```

The experiment compares the production title-driven query with a direct Artist-credit-driven
shape. It asserts identical ordered results for first and subsequent keyset pages, and also checks
combined FTS, Release, availability, and membership filtering. The 2026-09-03 comparison measured:

| Artist cardinality and page | Current | Artist-driven |
| --- | ---: | ---: |
| Rare, 20 matches, first page | 106.959 ms | 0.099 ms |
| Moderate, 200 matches, first page | 255.945 ms | 0.403 ms |
| Common, 20,000 matches, first page | 3.384 ms | 33.780 ms |
| Rare, cursor halfway through library | 573.963 ms | 0.100 ms |
| Moderate, cursor at 75% of library | 295.899 ms | 0.334 ms |
| Common, cursor at 95% of library | 18.066 ms | 22.973 ms |

The production plan scans `effective_track_title` in output order and performs a correlated probe
of `track_artist_credit_artist` for each candidate Track. Its cost therefore depends on how much of
the globally ordered library it must inspect before filling the page. A sparse Artist can require a
large scan or full exhaustion, while a common Artist fills an early page quickly without sorting.

The candidate-driven plan searches `track_artist_credit_artist` directly by Artist ID, performs
indexed point lookups for Track, effective metadata, and membership, then uses temporary B-trees
to de-duplicate Tracks and establish stable `(title, Track ID)` ordering. De-duplication preserves
the production behavior if one Artist occupies multiple credit positions on a Track. Candidate
work and sorting grow with the Artist's total cardinality. The existing indexes support this plan;
no schema change is needed.

This is a meaningful execution-strategy crossover, not one universally superior query. An
unconditional Artist-driven production shape would fix sparse Artists but regress common-Artist
first pages substantially. Production SQL remains unchanged pending evidence that an adaptive or
explicitly selected strategy is worth its complexity. The availability predicate remains unchanged
and was only checked for semantic equivalence in this experiment.

## Availability Query Experiment

**Historical timings superseded:** this experiment used simultaneous SQLite connections whose
page caches interfered. Keep the candidate-driven queries as strategy diagnostics, but use the
[single-connection audit](#availability-benchmark-validity-audit-2026-09-05) below for production
before/after decisions. The earlier numeric comparisons are retained as investigation history.

The availability diagnostic preserves `library-200k.sqlite` as the source-less baseline. It copies
that database to `availability-diagnostic.sqlite`, then replaces only the copy's synthetic local
sources for each deterministic distribution:

| Distribution | Available Tracks | Percentage | Selection |
| --- | ---: | ---: | --- |
| Source-less | 0 | 0% | No sources |
| Rare | 20 | 0.01% | Every 10,000th Track |
| Moderate | 1,000 | 0.5% | Every 200th Track |
| Common | 20,000 | 10% | Every 10th Track |
| Mostly available | 180,000 | 90% | Every Track except every 10th |
| Entirely available | 200,000 | 100% | Every Track |

Run it after building the primary fixture:

```sh
cargo run --release --example database_performance -- --rebuild
cargo run --release --example availability_query_comparison
```

The `available=true` prototype starts with `local_file_available(available, source_id)`, joins
through the source association to Tracks, and sorts the matching Tracks. The `available=false`
prototype builds the set of available Track IDs once and excludes it while retaining the existing
title-ordered scan. A Track with no source is therefore still correctly unavailable. The comparison
uses three measured iterations because the adverse production plans can take seconds per query.

The 2026-09-03 release-mode comparison measured these medians:

| Available distribution | Filter/page | Current | Prototype |
| --- | --- | ---: | ---: |
| 0% | `true`, first | 605.889 ms | 0.059 ms |
| 0% | `false`, first | 0.374 ms | 0.091 ms |
| 0% | `true`, deep | 365.899 ms | 0.072 ms |
| 0% | `false`, deep | 8.364 ms | 7.136 ms |
| 0.01% | `true`, first | 360.267 ms | 0.095 ms |
| 0.01% | `false`, first | 0.303 ms | 0.088 ms |
| 0.5% | `true`, first | 1,113.839 ms | 6.791 ms |
| 0.5% | `false`, first | 13.719 ms | 0.344 ms |
| 10% | `true`, first | 1,394.867 ms | 40.069 ms |
| 10% | `false`, first | 375.237 ms | 7.230 ms |
| 10% | `true`, deep | 2,304.125 ms | 38.448 ms |
| 10% | `false`, deep | 404.057 ms | 15.132 ms |
| 90% | `true`, first | 169.405 ms | 260.905 ms |
| 90% | `true`, deep | 2,166.128 ms | 235.444 ms |
| 90% | `false`, first/deep | More than 90 seconds; aborted | 77.295/82.144 ms |
| 100% | `true`, first | 0.643 ms | 281.746 ms |
| 100% | `true`, deep | 1,985.611 ms | 252.031 ms |
| 100% | `false`, first/deep | Not repeated after the 90% case | 192.216/148.139 ms |

A second prototype retained the production title-driven outer query but forced the correlated
subquery to look up `track_source` by the current Track before looking up each source observation by
its primary key. Selected medians were:

| Available distribution | Filter/page | Current | Track-first correlated |
| --- | --- | ---: | ---: |
| 0% | `true`, first | 581.445 ms | 72.670 ms |
| 0.01% | `true`, first | 367.322 ms | 77.695 ms |
| 0.01% | `true`, deep | 200.154 ms | 317.621 ms |
| 0.5% | `true`, first | 1,127.726 ms | 4.281 ms |
| 10% | `true`, first/deep | 1,396.598/2,326.108 ms | 0.463/7.533 ms |
| 90% | `true`, first/deep | 168.274/2,156.689 ms | 0.114/7.323 ms |
| 90% | `false`, first/deep | More than 90 seconds; aborted | 0.468/7.568 ms |
| 100% | `true`, first/deep | 0.668/2,010.894 ms | 0.110/7.567 ms |
| 100% | `false`, first/deep | Not repeated | 205.903/105.601 ms |

The pre-fix production plan scans `effective_track_title`, but its correlated availability subquery starts
from `local_file_available(available, source_id)` and then probes the Track/source association. For
each outer Track it may walk a large portion of all available source rows before finding that
Track's source or proving no association exists. Consequently, the cost depends on both how far the
outer title scan runs and the global number/order of available sources. It can grow far worse than
one cheap indexed probe per candidate.

For `available=true`, the candidate-driven plan uses the same availability index once, then performs
indexed source-association, Track, effective-metadata, and membership lookups. It needs temporary
B-trees for de-duplication and stable `(title, Track ID)` ordering, so dense first pages expose a
real crossover: the production title scan can stop almost immediately, while the prototype sorts
most or all of the library. Before the join-order fix, deep cursors favored the candidate-driven shape even at high density.

The Track-first correlated shape avoids the production plan's global available-source walk and
retains early termination, making it substantially faster in most distributions. It is still not
universal: for a rare availability set after a deep cursor, repeated per-Track source lookups took
317.621 ms versus 200.154 ms for the current plan and 0.106 ms for the candidate-driven plan. Empty
`available=false` results also require exhausting the outer Track set. Correcting the inner join
order therefore does not remove the execution-strategy crossover.

[Navidrome issue #4592](https://github.com/navidrome/navidrome/issues/4592) reports related prior art:
adding `missing=false` selected a boolean-filter index plus a temporary sort instead of an index
preserving song order, slowing the query. It supports checking availability filters together with
`ORDER BY`; our Track-first fix follows our own correlated-subquery measurements, rather than
copying Navidrome's schema or its reported index-removal workaround.

For `available=false`, the prototype uses the availability index to create a list subquery with a
Bloom filter, then scans in title order and excludes available Track IDs. It was faster throughout
the unstructured-filter matrix. However, materializing a large available set can conflict with a
highly selective Release, FTS, or Artist path, so it is not evidence for an unconditional combined-
filter strategy. Ordered first, second, and deep pages and combinations of FTS, Release, Artist,
membership, availability, and cursor semantics were verified against a semantically equivalent
Track-first reference query.

The existing indexes support all prototypes. The original experiment made no schema/index or
production-query change. The following production fix addresses only correlated join order;
selecting among execution strategies remains deferred.


## Production correlated availability join-order fix (2026-09-05)

The production change is limited to the four availability `EXISTS` expressions: the filter and
projected `available` value in both general and Release-filtered search. Each now uses:

```sql
SELECT 1 FROM track_source ts
CROSS JOIN local_file_observation l
WHERE ts.track_id = t.id AND l.source_id = ts.source_id AND l.available = 1
```

This intentionally constrains SQLite join order because measurements demonstrated a severe planner
mischoice. [SQLite Query Optimizer Overview, section 7.1.2](https://www.sqlite.org/optoverview.html#manual_control_of_query_plans_using_cross_join)
is the authority: SQLite does not reorder the two sides of a `CROSS JOIN`. An ordinary inner join,
even written with `track_source` first, does not impose that ordering. The association predicate is
retained, so this does not generate an unconstrained Cartesian product.
The [Next-Generation Query Planner discussion](https://www.sqlite.org/queryplanner-ng.html)
also documents this loop-order mechanism and cautions against routine manual planner hints.
Our constraint is specific to the measured inner-subquery mischoice, not a general query policy.

The observed inner plans on bundled SQLite 3.53.2 are:

```text
Before:
SEARCH l USING COVERING INDEX local_file_available (available=?)
SEARCH ts USING COVERING INDEX sqlite_autoindex_track_source_2 (track_id=? AND source_id=?)

After (also the Track-first diagnostic):
SEARCH ts USING COVERING INDEX sqlite_autoindex_track_source_2 (track_id=?)
SEARCH l USING COVERING INDEX local_file_available (available=? AND source_id=?)
```

The second lookup uses both columns of the existing covering availability index; it is a lookup of
the associated source, not a walk of all available sources. No primary-key index hint is needed.
The benchmark extracts the exact production SQL and asserts this order for both availability
checks, both search branches, both boolean filters, and all six distributions. The general outer
plan still scans `effective_track_title`; the Release branch still starts with `track_release_order`.
FTS and Artist predicates, membership, cursor logic, schema, and indexes are unchanged.

**Superseded multi-connection measurements:** the following table caused the initial commit hold.
The audit below explains its cache contention and replaces its performance conclusions.

The before/after runs reused the deterministic 200,000-Track fixture on Linux x86-64 with SQLite
3.53.2, in release mode, with one warm-up and three measured iterations per case. Values below are
medians in milliseconds. “Track-first” is the original narrow diagnostic; “full SQL control” runs
the exact production SQL on a separately initialized connection and reads returned IDs. Production
runs through `Library::search` and materializes its normal result. These are warm measurements,
not cold-start results or performance budgets.

| Available | Filter/page | Before production | After production | Track-first | Full SQL control | Candidate diagnostic |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 0% | `true`, first | 566.838 | 684.342 | 70.003 | 72.521 | 0.060 |
| 0% | `true`, deep | 345.056 | 352.481 | 43.245 | 46.083 | 0.054 |
| 0% | `false`, first | 0.393 | 0.145 | 0.076 | 0.138 | 0.079 |
| 0% | `false`, deep | 7.756 | 7.726 | 6.526 | 6.465 | 6.482 |
| 0.01% | `true`, first | 344.722 | 498.368 | 76.605 | 77.268 | 0.096 |
| 0.01% | `true`, deep | 184.054 | 265.367 | 46.632 | 48.010 | 0.092 |
| 0.01% | `false`, first | 0.336 | 0.182 | 0.246 | 0.135 | 0.087 |
| 0.01% | `false`, deep | 7.669 | 7.182 | 6.580 | 6.611 | 7.010 |
| 0.5% | `true`, first | 1083.139 | 4.254 | 4.172 | 4.274 | 6.671 |
| 0.5% | `true`, deep | 1473.709 | 33.914 | 11.604 | 11.101 | 6.522 |
| 0.5% | `false`, first | 13.218 | 0.153 | 0.089 | 0.146 | 0.354 |
| 0.5% | `false`, deep | 22.495 | 7.551 | 6.679 | 6.361 | 7.041 |
| 10% | `true`, first | 1336.862 | 0.433 | 0.329 | 0.404 | 38.987 |
| 10% | `true`, deep | 2204.711 | 9.046 | 7.141 | 6.735 | 36.620 |
| 10% | `false`, first | 359.593 | 0.172 | 0.097 | 0.151 | 6.305 |
| 10% | `false`, deep | 378.633 | 7.811 | 6.660 | 6.391 | 14.651 |
| 90% | `true`, first | 159.301 | 0.205 | 0.130 | 0.180 | 247.544 |
| 90% | `true`, deep | 2054.367 | 7.213 | 6.777 | 6.352 | 224.728 |
| 90% | `false`, first | Aborted historically; not rerun | 0.560 | 0.493 | 0.537 | 70.971 |
| 90% | `false`, deep | Aborted historically; not rerun | 10.531 | 7.256 | 6.866 | 79.585 |
| 100% | `true`, first | 0.632 | 0.288 | 0.110 | 0.177 | 268.496 |
| 100% | `true`, deep | 1889.286 | 7.423 | 6.976 | 6.396 | 243.480 |
| 100% | `false`, first | Not rerun | 1784.716 | 174.257 | 177.365 | 182.262 |
| 100% | `false`, deep | Not rerun | 896.234 | 96.029 | 97.920 | 139.517 |

The pre-fix harness intentionally skipped dense `available=false` cases after the earlier 90-second
abort; its printed “>90 s/not repeated” is not a new measurement or a measured lower bound for the
100% case. All post-fix cases completed, and ordered first/deep results, second pages, and combined
FTS/Release/Artist filters were checked against production and both diagnostics.

### Historical acceptance hold

The matching inner plans and approximately four million VM steps initially failed to explain the
large wall-clock gap. In particular, the table above suggested source-less and rare-query
regressions and a roughly tenfold exhaustive-query gap. Those conclusions were confounded by
connection-level page-cache behavior. They are superseded by the audit below; they are not
remaining regressions of the production patch.

The availability regression test remains unchanged. It covers no source, unavailable and available
sources, mixed and multiple available sources, loss of the last available source, loss of only one
available source, nonmembers, returned availability without a filter, both boolean filters, and
one-row keyset pagination with tied titles. It exercises all combinations of FTS, Artist, and Release
filters before and after availability changes.

## Availability benchmark-validity audit (2026-09-05)

The root cause was interference between simultaneously live connections in the benchmark process.
The bundled `libsqlite3-sys 0.38.2` build enables `SQLITE_ENABLE_MEMORY_MANAGEMENT` (confirmed through
`sqlite_compileoption_used`). In [SQLite 3.53.2's page-cache implementation](https://raw.githubusercontent.com/sqlite/sqlite/version-3.53.2/src/pcache1.c),
that option selects a single process-wide `PGroup`. Private page caches recycle one another's
unpinned pages. Here, one cache accumulated approximately 4,209,152 bytes while another retained
only 5,120 bytes. The observed behavior follows the shared group's eviction rules in
`pcache1FetchStage2`, `pcache1Unpin`, and `pcache1Destroy`: one connection can retain the group's
cache allowance while another repeatedly discards newly read pages.

This is a benchmark-methodology finding, not a defect in the Track-first query. Reversing warm-up
and measurement order reversed which connection was slow. Closing the competing connection
restored normal caching without changing SQL, bound values, PRAGMAs, or planner statistics.
The retained `--contention` reproducer measured:

| Source-less `true`, first page | Wall time | VM steps | Full-scan steps | Cache hits | Cache misses |
| --- | ---: | ---: | ---: | ---: | ---: |
| Fast connection | about 73 ms | 4,000,019 | 199,999 | 598,455 | 3,359 |
| Cache-starved connection | about 697 ms | 4,000,019 | 199,999 | 0 | 601,814 |
| Same slow connection after closing competitor | 72–73 ms | 4,000,019 | 199,999 | 598,455 | 3,359 |

Both connections reported the same settings: `cache_size=-2000`, `page_size=4096`, `mmap_size=0`,
`journal_mode=wal`, `synchronous=2`, `temp_store=0`, `cache_spill=483`, `automatic_index=1`,
`foreign_keys=1`, `busy_timeout=5000`, `query_only=0`, and `read_uncommitted=0`.
An identical configured cache size was therefore insufficient evidence of equivalent cache state.

Linux `/proc/self/io` reported zero physical `read_bytes` during the measured queries, including
the contention reproducer. Extra SQLite cache misses caused many more read system calls served
from the OS filesystem cache. Repeating a query or warming the OS cache could not repair the
starved SQLite cache. No system caches were flushed and no cold-disk performance claim is made.

### Corrected methodology

The page-cache contention finding is project-specific evidence from our measurements with this
SQLite build and workload. It should not be generalized to all SQLite builds or multi-connection
workloads; the source reference explains the mechanism behind our observed result.

Run the audit against the existing deterministic fixture:

```sh
cargo run --release --example availability_benchmark_audit
# Optional reproduction of the rejected simultaneous-connection methodology:
cargo run --release --example availability_benchmark_audit -- --contention
cargo run --release --example availability_benchmark_audit -- --contention --reverse
# Faster revalidation excluding the expensive pre-fix baseline:
cargo run --release --example availability_benchmark_audit -- --post-only
```

The normal audit uses one live SQLite connection at a time, including fixture setup and validation.
Every arm opens its own connection, performs four warm-ups and twelve measured queries, then closes
it. A second block reverses arm order, giving 24 measured queries per arm/case. Measurements include
fresh statement preparation, binding, stepping, full result mapping, and statement destruction.
Connection opening, result equality checks, counter reads, and trace verification are outside the
timed region. Each block has the same database state, distribution, SQL projection, filter values,
50-row limit, and first-page or fixed 100,000-Track cursor. No `ANALYZE`, `PRAGMA optimize`, index,
migration, production PRAGMA, or SQLite build configuration change was made.

The arms are the public `Library::search`, the unchanged Store implementation compiled into the
benchmark, an exact full-result SQL control, and the pre-fix SQL with ordinary inner joins. The
benchmark-only inclusion of Store/domain source exposes its connection and original row mapper;
production encapsulation and source files are unchanged. The public Library is measured in a
separate block with no other open connection. Its counters are explicitly unavailable, rather than
reported as zero. The Store and SQL control use the actual production initialization and mapper.
An untimed, scoped PROFILE trace asserts byte-for-byte equal expanded SQL (including bound values)
and equal VM/full-scan/sort/autoindex/reprepare counters between Store search and the control.
This is permanent audit verification; no temporary instrumentation remains in production.

The historical narrow prototypes used `prepare_cached`, selected fewer columns, consumed only IDs,
ran in a fixed order, and had just one warm-up and three samples. Those comparisons did not isolate
query strategy. Corrected arms all recreate statements and consume all seven result fields. Separate
preparation observations are recorded in the raw results; preparation is included in every timed
arm, and does not explain the exhaustive-query gap. Statement and connection counters are collected
in a separate untimed query after warm-up. Sorts, automatic-index inserts, and temporary-buffer spills
are checked alongside VM steps and page-cache hits/misses, using the
[SQLite statement counter definitions](https://www.sqlite.org/c3ref/c_stmtstatus_counter.html) and
[connection counter definitions](https://www.sqlite.org/c3ref/c_dbstatus_options.html).

Pre-fix cases receive a two-second deadline on the first untimed warm-up only. If it expires, the
whole baseline block is censored and has no median or complete execution counters. The deadline is
removed before measured samples, so timed baseline queries have no progress callback overhead.
This is an experiment runtime bound, not a product performance budget.

### Corrected results

Values are milliseconds, shown as the range of the two block medians (twelve samples per
block). This exposes order sensitivity rather than averaging it away. “Production” is the public
`Library::search`; “control” is the full-result, freshly prepared SQL control. The
[measurement CSV](measurements/availability-audit-2026-09-05.csv) contains both block
medians, sample extrema, preparation observations, and counters for every case and arm. Censored
baseline blocks have no timing median or complete counters.

| Available | Filter/page | Before | Production after | Matched control |
| --- | --- | ---: | ---: | ---: |
| 0% | `true`, first | 71.206–74.026 | 72.993–73.102 | 72.371–72.825 |
| 0% | `true`, deep | 45.039–45.123 | 48.386–49.010 | 46.301–47.150 |
| 0% | `false`, first | 0.149–0.169 | 0.150–0.151 | 0.151–0.169 |
| 0% | `false`, deep | 7.393–7.520 | 7.465–7.582 | 7.649–7.672 |
| 0.01% | `true`, first | 353.092–355.092 | 75.305–75.417 | 76.029–76.373 |
| 0.01% | `true`, deep | 184.928–188.029 | 47.439–49.830 | 48.001–48.266 |
| 0.01% | `false`, first | 0.301–0.312 | 0.162–0.168 | 0.152–0.172 |
| 0.01% | `false`, deep | 7.671–7.834 | 7.565–7.707 | 7.671–8.080 |
| 10% | `true`, first | 1361.597–1364.847 | 0.430–0.447 | 0.451–0.457 |
| 10% | `true`, deep | 1553.173–1573.300 | 7.882–7.886 | 7.886–7.895 |
| 10% | `false`, first | 298.657–318.689 | 0.173–0.263 | 0.171–0.211 |
| 10% | `false`, deep | 324.035–328.098 | 7.535–7.599 | 7.434–8.178 |
| 90% | `true`, first | 160.543–160.820 | 0.197–0.203 | 0.199–0.223 |
| 90% | `true`, deep | 1934.913–1938.828 | 7.499–7.535 | 7.460–7.465 |
| 90% | `false`, first | 2 s timeout | 0.573–0.574 | 0.558–0.559 |
| 90% | `false`, deep | 2 s timeout | 7.959–8.079 | 7.874–8.188 |
| 100% | `true`, first | 0.633–0.642 | 0.298–0.309 | 0.194–0.197 |
| 100% | `true`, deep | 1906.352–1915.739 | 7.424–7.475 | 7.477–7.645 |
| 100% | `false`, first | 2 s timeout | 180.164–181.420 | 175.290–175.665 |
| 100% | `false`, deep | 2 s timeout | 96.807–97.593 | 97.231–97.241 |

The former exhaustive-query timing gap is gone. Store/control SQL and bound values are identical,
all seven result fields agree with the public API, and execution counters match. Small wall-clock
differences remain: source-less deep `true` measured 45 ms before versus 48–49 ms through the public
API after (46–47 ms in the control). Its VM/scan/cache counters are identical before and after;
this run does not establish a query-caused regression for that small difference. Sub-millisecond
pages also show timing variation. These residual differences are reported rather than hidden, and
are distinct from the explained order-of-magnitude cache gap. Source-less `true` has the same VM
work before and after, so a large speedup is not expected once the cache artifact is removed.

Selected execution counters follow. These are complete-query observations from the unchanged
Store implementation, verified by PROFILE against the matched control; public API counters remain
encapsulated. The CSV includes the complete matrix. All completed cases had zero sorts, zero
automatic-index rows, and zero temporary-buffer spill bytes; automatic reprepare count was one
for each newly prepared, bound statement in both matched paths.

| Available | Filter/page | Before VM steps | After VM steps | After full-scan steps | After cache hits/misses |
| --- | --- | ---: | ---: | ---: | ---: |
| 0% | `true`, first | 4,000,019 | 4,000,019 | 199,999 | 598,455 / 3,359 |
| 0% | `true`, deep | 3,000,021 | 3,000,021 | 199,999 | 299,031 / 2,783 |
| 0.01% | `true`, first | 23,800,579 | 4,000,659 | 199,999 | 598,556 / 3,441 |
| 0.01% | `true`, deep | 12,900,801 | 3,000,341 | 199,999 | 299,078 / 2,829 |
| 10% | `true`, first | 45,023,418 | 11,618 | 499 | 3,263 / 0 |
| 10% | `false`, first | 10,002,193 | 2,243 | 54 | 572 / 0 |
| 10% | `true`, deep | 50,824,420 | 812,620 | 100,499 | 3,216 / 1,137 |
| 90% | `true`, first | 4,514,963 | 2,718 | 54 | 857 / 0 |
| 90% | `false`, first | censored | 13,368 | 499 | 4,313 / 0 |
| 100% | `true`, first | 14,868 | 2,618 | 49 | 812 / 0 |
| 100% | `false`, first | censored | 5,000,019 | 199,999 | 1,593,090 / 8,724 |
| 100% | `false`, deep | censored | 3,500,021 | 199,999 | 796,334 / 5,480 |

The apparent rare-query regression was a benchmark artifact. With equivalent caching, rare
`available=true` improves substantially on both first and deep pages, supported by the reduction
from 23,800,579 to 4,000,659 VM steps on the first page and 12,900,801 to 3,000,341 on the deep page.
The corrected results support committing the existing Track-first production patch and its tests.
No production query, PRAGMA, schema, index, migration, or build setting was changed during this audit.

The candidate-driven experiment remains in `availability_query_comparison.rs`. Sparse availability,
especially deep cursors or an empty candidate set, still exposes the execution-strategy crossover:
a candidate path can inspect a small available set while title-driven execution exhausts many
Tracks. Dense first pages favor Track-first early termination. Entirely available `false` still
requires a full outer scan. The historical multi-connection candidate timings are superseded too;
use equivalent cache isolation before making new quantitative strategy comparisons. This audit
does not add adaptive selection or establish a universal candidate-driven advantage.

Remaining limits: these are warm Linux x86-64 measurements with bundled SQLite 3.53.2. They do not
measure cold physical storage or fix/characterize multi-connection application workloads. No SQLite
upgrade or memory-management build change was attempted. All timed blocks recorded zero physical
read bytes, and reported medians are observations, not product performance budgets.

Validation: `cargo fmt --all -- --check`, `cargo test`,
`cargo clippy --all-targets --all-features -- -D warnings`, and `git diff --check` pass.
The production patch and availability regression tests are byte-for-byte unchanged by the audit.
No Git commit was made.
