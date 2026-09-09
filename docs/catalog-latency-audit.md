# Add Album latency audit (2026-09-08)

The initial audit below was measurement-only. Its rich Add Album browse is now
superseded by the [request-shape follow-up](#request-shape-follow-up). Historical
measurements are retained; SQL, transaction boundaries and import behavior were
not changed by either pass. The subsequent [search/detail audit](catalog-endpoint-audit.md)
changes search pages to ten results and retains the full lookup, including ISRCs.

## Reproduction and boundaries

Enable `MUSIC_LIBRARY_CATALOG_TIMING=1` when launching the diagnostic. It writes
phase timings and HTTP attempt/status/retry counts to stderr; normal operation is
silent. `MUSIC_LIBRARY_CATALOG_TIMING_DETAIL=1` additionally logs persistence
subphases (per-Track entries can be summed). Timings include errors and retry waits.

```sh
MUSIC_LIBRARY_CATALOG_TIMING=1 cargo run --manifest-path tools/qml-diagnostic/Cargo.toml
```

An opt-in ignored test runs the actual QML Add Album function, catalog worker,
queued callback, import, local search and property notifications with a temporary
sample library. It selects the first Album-type search result for the probe only.
It never writes a user's library and is excluded from normal tests:

```sh
MUSIC_LIBRARY_CATALOG_TIMING=1 \
MUSIC_LIBRARY_CATALOG_LIVE_QUERY='releasegroup:Animals AND artist:"Pink Floyd"' \
QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software \
  cargo test --manifest-path tools/qml-diagnostic/Cargo.toml --all-features \
  live_catalog_latency_audit -- --ignored --nocapture
```

The button timer starts at entry to the Rust catalog action and ends after final
snapshot property notifications. It includes worker dispatch and queued callback
latency, but not the subsequent rendered frame. HTTP includes response headers and
body reading; JSON parsing and application conversion are separate. Later probes
also split headers/body. The one-second gate timer includes lock acquisition and
any retry cooldown. Search is measured separately and excluded from Add totals.
No user think-time was inserted between search completion and Add.

## Live measurements

Linux/WSL, existing debug build/profile, production SQLite/HTTP configuration,
one application SQLite connection. Each run has an empty application catalog
cache; upstream MusicBrainz caching and network conditions are uncontrolled.

| Phase (milliseconds) | Demon Days | Animals, slow | Animals, repeat |
|---|---:|---:|---:|
| Edition browse rate wait | 424.684 | 444.268 | 0.002 |
| Edition browse HTTP | 294.635 | **21,896.903** | 517.487 |
| Edition JSON parse + conversion | 0.904 | 2.337 | 2.428 |
| Representative selection | 0.031 | 0.064 | 0.064 |
| Release lookup rate/retry waits | 704.407 | 0.001 | 1,304.815 |
| Release lookup HTTP, all attempts | 1,088.710 | 226.137 | 3,294.591 |
| Release JSON parse + conversion | 0.450 | 0.217 | 0.176 |
| Library::add_catalog_release | 11.659 | 4.685 | 4.517 |
| Post-import local search | 0.555 | 0.439 | 0.452 |
| **Button to final notifications** | **2,529.620** | **22,578.399** | **5,127.623** |

Small residuals include dispatch, callback, cache cloning, notification and logging
cost. All successful imports have one medium. Demon Days imported 15 Tracks and
47 identity associations; Animals imported 5 Tracks and 29 associations.

The slow Animals Add made **two attempts total**, browse 200 then lookup 200,
with **no 503/backoff**. Including its preceding search gives three HTTP requests.
The same is true for the Demon Days run. The repeat had one browse 200 plus lookup
503 → 200 (three Add attempts); its preceding search also retried 503 → 200, for
five attempts over the entire search/add interaction. Both retries supplied
Retry-After 0, so the existing rate gate still enforced one-second spacing.

The repeated Animals browse returned 89 edition summaries (134,176 decoded bytes)
in one page. Its header wait was 516.226 ms, body read 1.215 ms. Lookup's successful
attempt spent 3,119.071 ms awaiting headers and 0.205 ms reading 6,025 bytes. These
splits were added after the 21.9-second browse, so that slow request cannot be
retrospectively divided into connection/header/body time. Another preliminary run
timed out in search before Add began; that is not included in Add measurements.

The selected Animals Release was `0a567b61-f09d-4549-9d43-1c3e81c21b26`.
No request occurs inside Track/ISRC processing. The resource sequence remains:
`release-group` search → `release?release-group=…&inc=artist-credits+labels+media&limit=100`
→ selected `release/<id>?inc=release-groups+recordings+artist-credits+isrcs+media`.

## Import-only scaling

`examples/catalog_import_timing.rs` constructs deterministic application-owned
values, with two ordered credits and four identities per Track. Each size has 11
independent temporary file-backed SQLite runs, using the application's normal
connection setup and a single live connection. DB creation/open and DTO construction
are outside the timer. Each warm re-import reuses the Release just imported.
Summary measurements have detailed tracing disabled.

```sh
cargo run --example catalog_import_timing
# Optional: copy an already closed/checkpointed diagnostic seed into each temporary DB.
cargo run --example catalog_import_timing -- /path/to/diagnostic-200k.sqlite
```

| Tracks | Empty DB import median ms | Warm re-import ms | Search refresh ms | 200k seed import ms | Seed warm re-import ms |
|---|---:|---:|---:|---:|---:|
| 5 | 3.911 | 0.125 | 0.494 | 5.496 | 0.239 |
| 15 | 10.533 | 0.191 | 0.530 | 28.092 | 0.323 |
| 30 | 20.618 | 0.294 | 0.564 | 55.908 | 0.335 |
| 100 | 70.132 | 0.797 | 0.672 | 181.455 | 0.861 |

The 200k seed is the existing deterministic performance fixture, copied while
closed, not a user's database. Its post-import search medians were 75.326/72.897/
1.238/1.648 ms respectively. That search selectivity/cache variation is separate
from import, and still nowhere near 20 seconds. Copying a seed warms filesystem
cache; these are not cold-disk or cross-filesystem portability measurements.

## Persistence breakdown and inspection

Separate detailed runs aggregate per-Track spans. For a 15-Track import, medians
were as follows; tracing/printing raises measured total from 10.533 to 11.684 ms.
Individual phase medians need not sum to the total median.

| Persistence phase | ms |
|---|---:|
| Begin immediate transaction | 0.010 |
| Album resolution | 0.015 |
| Album creation/metadata/credits | 0.071 |
| Album external identity | 0.011 |
| Release resolution + creation | 0.037 |
| Release metadata/credits, including join phrases | 0.071 |
| Release external identity | 0.015 |
| Track creation | 0.277 |
| Track metadata/credits, including join phrases | 1.039 |
| Track external identities | 0.725 |
| Membership insertion | 0.106 |
| Effective metadata/FTS maintenance | 7.818 |
| Commit | 0.231 |

There is one transaction and one connection per import. Album/Release resolution
is outside the Track loops. SQL is prepared repeatedly through existing `execute`
calls, but no connection/transaction is opened per Track. Effective metadata/FTS
is refreshed **twice per new Track**: once in catalog creation, then again after
exact join phrases are applied. These are targeted indexed Track/FTS-row updates,
not global rebuilds. Warm re-import skips Track/FTS creation and only restores
membership. Local search/snapshot refresh runs once after the entire import.
There are no sleeps or network operations in persistence or Track conversion.

## Initial finding (before the request-shape change)

The reproduced 22.58-second delay is dominated by the **edition-browse HTTP call
(97% of Add time)**, independently of retries and selected Track count. Persistence
scales roughly linearly with small per-Track costs, even with a 200k seed. Its
repeated FTS updates are real work, but removing them would not fix this delay.

The smallest likely fix to investigate is reducing the cost of the initial edition
browse for automatic Add (a smaller page and/or deferring edition-only expansion),
while keeping explicit pagination/details for Editions. The current browse asks
for up to 100 editions plus credits, labels and media, even to import five Tracks.
At that point no request parameters had changed. The comparison below followed: these timings localize the bottleneck to HTTP,
but do not establish whether server-side query cost, upstream cache/load, or
transport stalls caused the slow response. The much faster repeat demonstrates
variability, not proof of a particular caching mechanism. Rate limits, retries,
correctness checks and SQL remain unchanged.

## Request-shape follow-up

Normal Add Album now uses **F: `status=official&inc=media&limit=100`**. Advanced
Editions keeps A unchanged. The selected-Release lookup, rank function, SQLite,
FTS, Track insertion, rate gate, retries and request timeout are unchanged.

### Comparison method

Three samples per shape/Album, rotating shape order and reversing Album order on
alternate rounds, through one production MusicBrainz client and its process-wide
one-second gate. Timing is debug-build HTTP wall time, including body consumption;
rate waits and parsing are recorded separately. Bytes are decoded response-body
bytes, not compressed wire traffic. No application response cache is used here;
upstream caches, server load and transport conditions are uncontrolled. Three
samples reveal variability but cannot establish a tail-latency guarantee.

Animals, Lava Land (four editions) and Abbey Road (73 editions) were measured in
one batch. Demon Days was measured separately after correcting the probe's
same-title search selection: the initial result was a promotional single, not the
Album. Those 15 single-group samples are excluded below. The corrected probe
prefers Album-type search results and prints the selected group ID for inspection.

| Shape | Request |
|---|---|
| A (old Add; still Editions) | Browse, `inc=artist-credits+labels+media` |
| B | Browse, `inc=media` |
| C | Browse, no `inc` |
| D | Browse, no `inc`, `status=official` |
| F (new Add) | Browse, `inc=media`, `status=official` |
| E (Demon Days diagnostic only) | Release search, `query=rgid:<exact MBID> AND status:official` |

All shapes use `limit=100&offset=0`. HTTP seconds below list all three attempts'
operation times (including failed calls); medians are not success-only estimates.

| Album | Shape | HTTP seconds, rounds 1 / 2 / 3 | Median s | Response bytes | Releases returned / reported |
|---|---|---|---:|---:|---:|
| Animals | A | 2.021 / 24.500 / 0.342 | 2.021 | 134176 | 89 / 89 |
| Animals | B | 30.751 timeout / 14.135 / 0.174 | 14.135 | 76474 | 89 / 89 |
| Animals | C | 0.646 / 0.329 / 0.175 | 0.329 | 59765 | 89 / 89 |
| Animals | D | 7.071 / 0.468 / 9.413 | 7.071 | 51370 | 75 / 75 |
| Animals | F | 0.296 / 24.680 / 0.183 | 0.296 | 65046 | 75 / 75 |
| Demon Days | A | 0.456 / 1.442 / 0.190 | 0.456 | 46329 | 31 / 31 |
| Demon Days | B | 0.244 / 0.252 / 0.178 | 0.244 | 26903 | 31 / 31 |
| Demon Days | C | 0.245 / 0.241 / 0.176 | 0.241 | 20713 | 31 / 31 |
| Demon Days | D | 16.484 / 24.656 / 21.684 | 21.684 | 17595 | 26 / 26 |
| Demon Days | F | 19.413 / 0.242 / 0.178 | 0.242 | 22841 | 26 / 26 |
| Demon Days | E | 0.234 / 0.179 / 1.047 | 0.234 | 33844 | 26 / 26 |
| Lava Land | A | 13.969 / 7.884 / 0.182 | 7.884 | 5295 | 4 / 4 |
| Lava Land | B | 14.919 / 0.221 / 0.698 | 0.698 | 3424 | 4 / 4 |
| Lava Land | C | 10.830 / 23.419 / 17.996 | 17.996 | 2750 | 4 / 4 |
| Lava Land | D | 0.198 / 0.309 / 0.184 | 0.198 | 2750 | 4 / 4 |
| Lava Land | F | 27.857 / 7.762 / 5.057 | 7.762 | 3424 | 4 / 4 |
| Abbey Road | A | 0.875 / 4.832 / 9.781 | 4.832 | 117132 | 73 / 73 |
| Abbey Road | B | 0.594 / 19.206 / 8.673 | 8.673 | 64042 | 73 / 73 |
| Abbey Road | C | 17.650 / 0.427 / 0.349 | 0.427 | 49401 | 73 / 73 |
| Abbey Road | D | 0.353 / 7.671 / 0.176 | 0.353 | 44518 | 65 / 65 |
| Abbey Road | F | 0.836 / 0.297 / 7.587 | 0.836 | 57475 | 65 / 65 |

Only Lava Land B's third sample retried (503 → 200); its 0.698s HTTP total plus
rate/retry wait gave 2.002s wall time. Animals B's first sample timed out at 30.751s
without an HTTP status/body and was not retried. Every other listed sample was one
HTTP 200 attempt. Successful responses had identical byte sizes across rounds.

All successful pages returned their full reported candidate counts in these groups.
Unfiltered A/B/C had matching identity sets; D/F matched their Official subset.
These summary-only responses did not demonstrate a shortened page. We still retain
actual-count offsets and bounded first-page selection: the official
[browse/paging documentation](https://musicbrainz.org/doc/MusicBrainz_API#Paging)
permits short Release pages under the Track-count cap. A mock verifies that a
2-result page reporting 103 candidates advances to offset 2, not 100. Nothing uses
Release Group lookup's 25-linked-entity subquery as a browse replacement.

### Fields and representative choice

All browse shapes provide identity, title, status, date and disambiguation, plus
some unavoidable base fields such as barcode. A/B/F additionally provide per-medium
format and track count; only A requests release artist credits and labels. No
candidate request asks for Tracks, recordings or ISRCs. Full import data still
comes from the existing detailed selected-Release lookup.

F retains the unchanged nonzero-track/media check and simpler-disc tie-break.
For example, Demon Days has original-era two-disc editions with no deluxe wording;
media distinguishes them even when the title/comment does not. A regression fixture
places an earlier-ID two-disc edition beside a one-disc edition and an empty
candidate to keep this distinction testable. C/D selected the same IDs in these
live datasets when ranked without the unavailable media inputs, but cannot make
those checks. F roughly halves the larger payloads and generally improves sampled
medians without discarding those ranking inputs. Its 19–28s outliers, and Lava
Land's still-slow median, mean **reliable sub-second service latency is not proven**.

| Album | Old A and new F representative (same in every successful sample) |
|---|---|
| Animals | `0a567b61-f09d-4549-9d43-1c3e81c21b26`, 1977, one medium / 5 Tracks |
| Demon Days | `14190090-8b00-4c4a-a861-50d7734c16fd`, 2005, one medium / 15 Tracks |
| Lava Land | `372cd3b7-96ad-4139-becb-953d64a41751`, 2005, one medium / 6 Tracks |
| Abbey Road | `03437e02-835f-3a0a-a37c-48a36c2e852a`, 1969, one medium / 17 Tracks |

The optional E search probe returned the same 26 Official Demon Days candidates
and the same representative. Its media/date/comment fields were usable, but its
33,844-byte response included credits, labels and tags versus F's 22,841 bytes.
All search scores were 100; returned order was not our representative ranking.
[Release search](https://musicbrainz.org/doc/MusicBrainz_API/Search#Release)
supports exact `rgid` and `status` fields, but this single-group comparison does not
justify switching to search-index candidates or assuming its tied-score page order
is canonical. Production retains browse and a deterministic local ranking.

### Request and cache behavior

With no compatible cache, search-and-add remains three successful requests:
Album search → Official media-summary browse → unchanged full Release lookup.
If the Official page is empty, one additional unfiltered media-summary browse is
made. A request error never triggers that fallback. If both pages are empty,
selection returns the existing recoverable error; no retry loop or forced edition
choice is added. All HTTP attempts still use the same rate/retry gate off Qt.

Default candidates are cached separately from four rich edition pages and one
Release detail. Add then Editions fetches rich data; repeated Add reuses candidates
and detail. A complete all-Official rich page is compatible with default selection;
an incomplete or mixed-status rich page does not replace the provider's filtered
first page. Explicit Editions remains rich, unfiltered and paginated, including
non-Official editions. No persistent cache or background requests were added.

Reproduce just the request comparison (opt-in; not a normal test):

```sh
MUSIC_LIBRARY_CATALOG_TIMING=1 MUSIC_LIBRARY_SHAPE_OUTPUT=/tmp/mb-shapes \
  cargo test --manifest-path adapters/musicbrainz/Cargo.toml \
  live_request_shapes -- --ignored --nocapture
```

`MUSIC_LIBRARY_SHAPE_ALBUM` can restrict this to `animals`, `demon-days`,
`lava-land` or `abbey-road`; `MUSIC_LIBRARY_SHAPE` restricts to A/B/C/D/E/F. The
probe saves raw JSON for field/identity-set inspection and logs HTTP bytes, status,
retries and phase timings. Leave at least a second between independent live probe
processes, since a process-wide gate cannot coordinate separate invocations.

### Fresh-application-cache QML search and Add

The final probes used fresh temporary sample libraries/sessions and the actual
QML worker/callback path. There was no user think-time between search and Add.
Independent processes were separated by an additional one-second pause; this is
outside the reported timings. Compilation is excluded. Both searches and the full
selected-Release lookup retained their production parameters. All six attempts
are shown, including the failed search; results are not selected for fast success.

| Album/run | Search s | Candidate HTTP s | Lookup HTTP s (all attempts) | Add total s | Full search-and-add s | Status sequence |
|---|---:|---:|---:|---:|---:|---|
| animals-1 | 0.597 | 0.288 | 17.037 | 18.449 | 19.047 | 200 → 200 → 200 |
| animals-2 | 0.512 | 0.263 | 0.310 | 1.807 | 2.320 | 200 → 200 → 200 |
| animals-3 | 0.528 | 0.176 | 2.067 | 3.548 | 4.076 | 200 → 200 → 503 → 200 |
| demon-days-1 | 0.573 | 0.228 | 17.280 | 18.722 | 19.296 | 200 → 200 → 200 |
| demon-days-2 | 27.139 | 0.288 | 0.725 | 2.574 | 29.713 | 200 → 200 → 503 → 200 |
| demon-days-3 | 2.508 | — | — | — | failed before Add | 503 → 503 → 503 |

Successful runs made exactly two logical Add requests, plus search. Animals 3 and
Demon Days 2 had one lookup retry each (four attempts including search); other
successful runs had three attempts total. Demon Days 3 exhausted three search
attempts and never reached Add. There were no no-Official fallbacks in these Albums.
The final selected IDs remained those above. Persistence stayed at 4.5–4.8ms for
Animals and 10.9–11.5ms for Demon Days; post-import search stayed below 1ms.

Compared with the historical 21.90s Animals browse, these three new Animals browses
were 0.176–0.288s. The interleaved request-shape table is the fairer A/F comparison:
F also had a 24.68s Animals outlier there. Nor is full search-and-add reliably fast:
unchanged lookup spent about 17 seconds in two runs, and unchanged search spent
27 seconds in one. We have reduced unnecessary request data, **not established a
fix for all long-latency MusicBrainz calls**. No further lookup/network-policy or
storage optimization was made on the basis of these few observations.

An earlier six-launch exploratory QML batch omitted the extra inter-process pause
(though each process still used its own gate). It had three search retry-exhaustion
failures; the three successes took 11.415s / 3.238s for Animals and 3.314s
for Demon Days. Those runs are not used as the final timing comparison because
fresh-process startup can otherwise shorten spacing across invocations.

### Validation

Deterministic tests cover lightweight Official query parameters, empty-Official
fallback, HTTP failure without fallback, actual-count paging, media-sensitive
ranking, candidate/rich-cache separation and compatible-page reuse. The real QML
test verifies Add then Editions fetches rich credits separately, re-add reuses the
Release, and pending/error delivery stays asynchronous. Existing backend/storage,
MusicBrainz, GStreamer, fake/QML tests, format checks, strict Clippy, QML lint/format
and fake/real-feature build smoke checks pass. Live service failures above are
explicit opt-in probe outcomes, not dependencies of the automated suite. The
existing Qt test-exit timer warning remains outside this request-shape change.
