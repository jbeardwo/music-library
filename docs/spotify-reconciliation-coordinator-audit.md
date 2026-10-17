# Self-draining Spotify reconciliation coordinator

This follow-up corrects the asynchronous continuation defect left in the first
[robustness pass](spotify-focused-robustness-audit.md). No changes were committed.

## Cause and continuation

After the last Album in a chunk, asynchronous best-available Track fallback
resumed `start_review_track_retry` with an empty Album vector. The Album dispatch
branch returned immediately, before its batch-completion/keyset continuation.
The 20-Album query bound therefore became an accidental run boundary. Earlier
unconfigured-provider tests skipped this asynchronous path and missed the defect.
The new handoff regression fails against that old branch.

Album dispatch now requires a nonempty batch. Empty asynchronous handoffs reach
batch completion and pull the next keyset page. Cache-only and provider-blocked
chunks post a generation-guarded Qt continuation, yielding between chunks. A
separate run-active flag spans all chunks, cached workers and provider callbacks.
Retry during a run does not create another coordinator; individual Album requests
join the existing work. Artist-revision work can revisit affected earlier contexts.

Limits have separate meanings:

| Limit | Meaning |
| --- | --- |
| `RECONCILIATION_ALBUM_BATCH_SIZE = 20` | Album query/processing chunk, never run lifetime |
| `LOCAL_BATCH = 32` | stale cached Track reevaluation chunk |
| three Album programs | candidate comparison safety bound |
| ten search candidates; bounded alternate strategies | provider discovery bounds, unchanged |
| 50 program entries/page, 20 pages maximum | Album program pagination, independent of run chunks |
| `PLAYABLE_SEARCH_BUDGET = 2` | actual standalone Track searches across the whole run |
| Connections UI page/window sizes | presentation bounds, independent of reconciliation |
| run lifetime | no numeric item cap; stop at stable actionable-work exhaustion |

Standalone searches now debit actual calls, rather than consuming the entire
allowance after the first chunk regardless of whether it searched. Exhausting that
allowance does not stop Album-context or cached work. It intentionally leaves
Tracks needing another provider-search window; it does not grant unlimited traffic.

## Automatic lifecycle and termination

Startup refresh, completed canonical local ingestion, committed matching metadata,
confirmed Artist equivalence and clearing Spotify exclusions request reconciliation.
Requests coalesce into a queued callback. Configuration, auto-match preference,
import readiness, catalog/matcher activity and active-run guards prevent premature
or duplicate coordinators. Import scheduling must finish before reconciliation
starts. A single-shot UI timer wakes deferred work at a persisted cooldown deadline;
ordinary refreshes do not continually start new runs.

Automatic queries select stale/outdated/not-evaluated/Needs Retry work. Current
Needs Review and No Candidates are stable, and exclusions/connected Tracks are
omitted. Explicit Retry can reconsider stable failures using retained discovery
cache. Provider configuration/access/cooldown failures suppress HTTP while all
remaining local/cached chunks continue. Actual processing errors terminate the
run and clear active state. No periodic retry loop is introduced.

## Progress and initial delay

The command immediately publishes **Preparing Spotify reconciliation…**. Opening
worker database connections moved off the UI thread. Previously the cached
preflight delivered only after evaluating all 20 Albums; the disposable-copy
measurement took **4,090 ms** before that delivery. It now delivers each Album
result independently and publishes targeted status/progress updates before the
whole chunk completes. Main-window status is visible without opening Connections.

The explicit copy run returned in **2 ms**; first completed local result was visible
at **1,693 ms** while other test processes were running. Warmed startup/import-ready
copies delivered their first result at **100/83 ms**. Immediate preparation is
distinct from completing an association: these copies produced no new connections,
so those timings measure completed local evaluation/status results, not a successful
new source attachment. Finish messages distinguish provider-unavailable/waiting work.

## Disposable current-fresh-database validation

The current snapshot contains 930 saved Tracks, 677 unresolved Tracks and 70
unresolved Albums. It differs from the older snapshot used in the first audit.
All tests used SQLite backup copies; the original database was not changed.

| Trigger | Internal chunks | Connected | Remaining unresolved | User presses |
| --- | ---: | ---: | ---: | ---: |
| explicit Retry, retained cooldown copy | 4 | 0 | 677 | 1 |
| explicit Retry, configured copy with current discovery cache | 4 | 0 | 677 | 1 |
| automatic populated startup | 3 | 0 | 677 | 0 |
| automatic import-ready event | 3 | 0 | 677 | 0 |

Automatic scope contains 59 initially eligible Albums, hence three chunks; explicit
Retry includes all 70 unresolved Album contexts. Immediate second Retry made zero
additional connections. The configured copy used cached discovery: **zero token
or catalog HTTP timing events**. Its two recorded provider search windows are
coordinator attempts, not two HTTP calls. The cooldown copy issued zero HTTP.
Final bookkeeping has 492 Needs Retry and 438 unresolved rows across all saved
Tracks; these totals include connected Tracks and must not be presented as
disjoint unresolved exception counts. Remaining unresolved identities did not change.
Restricting to the 677 unresolved, non-excluded library Tracks gives 492 Needs Retry,
181 No Candidates and four incomplete-program exceptions. No new connections or
new manual-review mappings were claimed by this validation.

This demonstrates full traversal of the real current workload, not new live-catalog
matching. The import-copy check exercises readiness scheduling on already imported
data; the filesystem-import regression separately verifies actual ingestion. These
limitations are deliberate and are not claimed as fresh live-provider acceptance.

## Verification and SQLite

Core tests: **476 passed**. Spotify adapter tests: **73 passed**, with live/manual
tests ignored by default. Coordinator regressions cover 65 Albums in four chunks,
101 in six chunks, asynchronous empty-batch handoff, overlapping Retry/review,
new Artist evidence revisiting earlier contexts, cached program fixed point,
automatic startup/import readiness, duplicate starts and stable negative decisions.
Adapter coverage verifies actual search allowance across chunks and HTTP with a
second connection acquiring `BEGIN IMMEDIATE` during the request.

Final focused Qt coordinator run: **10 passed, two manual tests ignored**. Connections,
review/bulk confirmation, imported-playlist playback, initial Library views and
filesystem-import tests passed. The explicit **200k-Track Connections scrolling**
regression passed. Strict all-target Qt Clippy, Rust formatting checks and
`git diff --check` passed. A final configured-copy repeat completed four chunks in
8,625 ms, with immediate preparation (2 ms command return), first local result at
1,697 ms, zero HTTP, zero additional second-pass connections and no lock errors.

No transaction spans provider HTTP or multiple internal batches. The previous short
immediate-write/WAL-snapshot fix is retained. Multi-chunk coordinator tests open
review state and enqueue Album retry during the run; no SQLite lock errors occurred.
The core concurrent cache/reconciliation write regressions also pass.

Translation/transliteration, regional catalogs, unlimited Track search and unrelated
layout failures remain outside this follow-up. Canonical Release identity, source
selection and prior matching/Connections behavior remain separate from scheduling.
