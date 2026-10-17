# Spotify Album reconciliation review

Current behavior and fresh-import validation are recorded in the
[focused robustness audit](spotify-focused-robustness-audit.md). Evaluation version
is 15; Album search strategy is 6. Retry drains keyset-paged Album contexts and
newly enabled local evidence; its standalone Track-search budget remains two per
click. Canonical Release identity stays separate from compatible provider-family
and contextual shared-title evidence used only for playable sources.


A sole strongly corroborated Spotify remaster or expanded/deluxe edition may
supply corresponding songs as playable sources, with an edition warning and
unchanged canonical Release metadata. Provider bonus Tracks need not be used;
unmatched local Tracks remain unresolved. Normal compatible candidates take
precedence. Competing editions, ambiguous titles and different performances
retain conservative review requirements. See the audit for exact evidence gates.

Expanded Connections Album rows expose Mark as not on Spotify next to each Track
connection button. In the marked Track list, right-click Check Spotify again
atomically re-enables every selected Track when the clicked Track belongs to
that selection. Clicking outside it affects only the clicked Track. Reversal
changes exclusions/reevaluation eligibility without fetching Spotify or changing
membership, metadata, playback, or trusted associations.

The expanded 2026-10-09 pass, current real-case outcomes and final verification are
recorded in [Local identity and Spotify robustness audit](local-spotify-robustness-audit.md).
The historical validation/checklist below describes the earlier positioned workflow;
the newer report supersedes its earlier limitations and test counts.

Advanced mapping now allows explicit partial and split-provider programs, with
duplicate-occurrence rejection and warned authoritative confirmation. Album/Track
paste accepts exact Spotify URL/URI/ID through normal cached object lookup. Album
Not on Spotify snapshots unresolved Tracks only. All ordinary cached inspection and
mapping remain provider-call-free.

Spotify Connections defaults to unresolved saved Tracks grouped by their existing
canonical Album. Groups use a bounded 200-Album keyset page ordered by Album title;
counts aggregate only those selected contexts. Expansion reads up to 201 unresolved
Tracks from the existing indexed Album query. Larger groups can open the paginated
Track view. Empty Album titles display Singles / Unassigned without creating an Album.
The Show Track list action retains the previous table, filters, sorting and widths.

An Album row opens Album review; expanded Track rows open Track review. Track review
can elevate to its Album. Both diagnostics expose their entire retained bounded
candidate set and share the positioned comparison component. Inspection, expansion
and candidate navigation never authenticate or issue provider requests. A displayed
candidate is separate from the automatic preference and from durable associations.

Track candidate selection previously required exactly one plausible candidate before
checking complete credits. An extra/omitted credit therefore blocked an otherwise
exact candidate. Selection now considers every candidate and keeps the undominated
set. The explainable partial order compares complete equivalent Track and Album
credits, exact titles, supplied positions, and precision-aware date evidence; a
candidate must be at least as strong on every axis and strictly stronger on one.
An unresolved replacement contributor cannot be dismissed by this comparison.
Only a unique fully compatible survivor of a complete bounded page is accepted.
All prior feasibility, version, trusted-identity, duration and persistence guards
still apply. Comparable distinct occurrences remain unresolved. The Album matcher
continues to evaluate its bounded candidate set; established primary Artists may
have additional provider credits, retained as warnings. Its existing complete
ordered Spotify Track-ID equivalence and different-program ambiguity protections
remain intact. Evaluator version is 7; discovery strategy stays at 1. Candidate display
order follows matcher decisions and explainable evidence, with provider rank separate.

## Explicit Album confirmation

Connect this Album prepares a private canonical-program snapshot. Strong program
alignment maps unique title/identity correspondences, including reordered and
inserted Tracks; weaker programs retain conservative unique-position comparison.
Missing/zero disc means disc 1. Provider occurrences must supply exactly one
Spotify Track identity. Connected and manually excluded Tracks are retained.
No speculative positional shift, entity recreation, membership change, Recording
identity assignment or Artist-equivalence declaration takes place.

Meaningful positioned title/Artist/duration differences require an additional
confirmation listing the differences. Album Artist/title/version blockers and an
incomplete provider program also require confirmation. Existing harmless title
normalization, untitled normalization, missing disc/date/type and established Artist
equivalence do not add warnings. Unequal programs can connect their unique positioned
subset; missing/duplicate/unidentified positions stay unresolved, and extra provider
positions are displayed. Strong correspondence can map different positions without
changing canonical ordering. Without that proof, if a differing title matches a canonical Track whose provider position is
missing, that misleading pair is withheld too. Explicit confirmation can accept other
warned differences at the chosen position.

Confirmation checks the canonical snapshot inside a short write transaction, stores
the trusted Album relationship and corresponding Track occurrences atomically, and
preserves canonical IDs and membership. Missing provider duration is not invented.
Connections patches its existing ListModels by ID; expansion keys, viewport anchors,
Track filters/sort and widths survive. A partly resolved group retains its current
unresolved count. Provider observations and user metadata overrides are unchanged.

Retained successful discovery, including rejected alternatives and expired records
not yet cleaned up, is inspectable offline. This does not renew discovery TTL.
A candidate whose program was never fetched has an explicit Fetch candidate program
button. Only this action, Retry Album and Search Spotify again may initiate provider
work. This fetch uses the existing bounded program paging/cache/cooldown path and
cannot itself attach Tracks. Ordinary query freshness and negative TTL remain intact.

## Retry state and policy

There were three misleading retry paths: local evaluation marked missing replay data
needs_retry; Album evaluation wrote a new reason without clearing that state; and the
batch then performed an ordinary TTL-governed lookup. A further keyset issue could
leave newly retryable contexts behind a previous batch cursor.

Completed Album evaluation now clears needs_retry and records current evaluator
version. Cached inspection does not persist a new evaluation or change lifecycle. Current
negative results display No candidates; compatible/rejected candidates display Needs
review with their structured reason. Contexts lacking a usable Album/Artist search
shape display Manual review required and retain explicit Track search. Needs retry
means fresh discovery is needed and the context is eligible for the explicit batch.
Active provider cooldown replaces that label with Waiting for Spotify / retry in Nm.

Retry unresolved first performs existing bounded local replay, then prioritizes true
Needs-retry Albums independently of the normal cursor and fills the remaining bounded
20-Album batch from the ordinary unresolved plan. True Needs-retry contexts request
explicit discovery refresh, bypassing ordinary TTL. Other contexts use normal cache
policy. Each Album finishes before the next starts. Absence never fans out to Track
searches. Not on Spotify remains excluded. Active cooldown prevents starting provider
work; a 429 or authorization/access failure stops the remaining batch. Busy Album
retry actions explain that another reconciliation must finish first.

The result message reports considered contexts, requested fresh discovery, normal
cache-policy evaluation, resolved/remaining Tracks, manual-review skips, excluded
marked Tracks and local replay connections. These are policy/context counts, not
invented HTTP counts: one context may need multiple bounded catalog endpoints.
Failure reports explain deferred/skipped contexts. Missing catalog configuration
reports that discovery cannot start. The adapter's existing actual request audit
remains the source of HTTP request counts.

## Validation and limits

Focused SQLite tests cover bulk/partial mapping, significant and harmless differences,
untitled/missing disc, no shift after omission, duplicate positions, stale snapshots,
identity/membership preservation, grouping counts, exclusion/cooldown and completion
clearing Needs retry. Track and Album candidate regressions reverse provider order;
existing equivalent/different-program tests still pass. Local HTTP mocks verify all
three persisted Album candidates/programs are inspectable repeatedly and after TTL
expiry with zero extra requests and no observation timestamp renewal. Qt tests cover
both candidate navigators, inspected/preferred separation, expansion, Track-to-Album
navigation, significant-difference cancellation, bulk confirmation, group removal and
retained view state. A batch test proves 12 Tracks cause one fresh Album discovery
even behind the old cursor, with no Track fan-out and zero additional calls in cooldown.

On a disposable 200,000-Track copy, initial whole-library grouping took 1.61 seconds.
Selecting Album IDs before aggregating reduced the first 200 groups to 16.73 ms.
Unresolved count: 1.95 ms; expanding ten Tracks: 2.02 ms; canonical program lookup:
1.06 ms; ten-Track bulk confirmation: 24.77 ms; targeted Album row read: 1.77 ms.
The indexed discovery lookup over 200,000 cached responses measured p50/p95
22.52/34.13 microseconds. All measurements are debug-build observations, not budgets.
`spotify_album_review_performance` additionally measures the bounded group refresh
read on a disposable database; it does not rebuild the whole Library.

A disposable real Library snapshot exposed 16 multi-candidate Track pages and eleven
cached Album candidates across five unresolved groups, including both Freshman Year
objects and four Songs alternatives, with zero Spotify requests. Reversed real Track
pages produced identical decisions. The available Freshman Year data correctly stayed
unresolved for a 16-local/15-provider program mismatch; no real later-exact automatic
winner was manufactured by discarding that evidence. Its alternative full programs
were not retained, so a real bulk confirmation could not be validated offline. The
explicit fetch path and bulk workflow were validated with fixtures. Catalog credentials
were absent from this session; no live search/refresh/program request was made.

Fetching additional search pages beyond the existing bounded candidate set remains
outside this change. The diagnostics retain/disclose the provider's further-results
indicator and support explicit refresh. This change does not alter canonical identity,
metadata editing, library membership, playlists, source priority or provider-cache
ownership. No commit is made.


## Completion checklist

1. **First/plausible blocker:** the Track matcher counted plausible candidates before
   complete-credit selection; the new dominance pass removes that early gate.
2. **Selection:** explainable evidence partial order, existing hard blockers, unique
   compatible survivor; Album evaluation retains existing canonical program rules.
3. **Order independence:** exact-second fixtures run both orders; real retained pages
   also produced identical decisions after reversal. Equal distinct candidates remain reviewable.
4. **Multi-candidate diagnostic:** both diagnostics expose retained candidates, navigation,
   visible decisions/blockers and a separate automatic-preference indicator.
5. **Album groups:** indexed bounded groups with unresolved counts, reasons and expandable
   Track rows; the existing Track table remains available.
6. **Manual Album confirmation:** prepare the visible candidate, show positioned program,
   confirm discrepancies where needed, atomically attach the safe subset.
7. **Mapping:** unique established disc/Track positions only, with one valid provider
   Track identity; preserve exclusions and existing trusted connections.
8. **Differences:** meaningful titles/credits/durations and Album blockers need a second
   explicit confirmation; existing harmless normalization does not.
9. **Unequal programs:** show missing/extra positions, withhold omission-shift evidence,
   attach only safe corresponding positions and leave leftovers unresolved.
10. **Misleading retry cause:** Album completion left an old needs_retry state; ordinary
    cache policy and a previous cursor could then skip the displayed fresh-discovery need.
11. **Corrected semantics:** completed evaluations clear that state; usable fresh-discovery
    contexts are explicitly retryable; unusable contexts say manual review.
12. **Batch:** prioritized fresh-discovery contexts bypass TTL, deduplicate by Album,
    never fan out, and report context/Track outcomes and skips.
13. **Cooldown:** waiting/retry-time presentation, no start while blocked, immediate
    batch stop after quota/access failure.
14. **Requests:** inspection/navigation/expansion and all live validation issued zero
    Spotify requests; mocks prove one Album discovery for 12 retryable Tracks and zero
    additional requests during navigation/cooldown. Explicit discovery retains bounded paging.
15. **Real validation:** offline snapshot inspected multiple candidates and reversed
    order; real programs correctly remain unresolved. Real manual bulk connection is
    unvalidated because retained programs/credentials were unavailable.
16. **200k:** bounded query measurements and indexed cache lookup are recorded above;
    standalone disposable-database benchmark is checked in.
17. **Tests:** focused core, Spotify adapter, Qt UI/batch regressions, formatting and
    strict Clippy; broader-suite migration fixture limitations are recorded below.
18. **Deferred:** live confirmation of the real Album, further search pages beyond the
    existing bound, and unrelated historical migration-fixture repairs. No redesign or commit.


Final verification: focused core suites passed (Album candidates 31, Track resolution 9,
Album review 8, lifecycle 22, canonical reconciliation 10; three Album tests intentionally
ignored). Spotify adapter: 61 passed, three ignored. Three isolated offscreen Qt tests
passed: Album candidate/navigation/bulk workflow, explicit retry/cooldown, and the existing
Connections targeted-update test. Strict core/adapter/Qt Clippy and formatting passed.
The broader core run had 408 passes, 18 ignored and 17 failures. All 17 identical failed
test names reproduce on an unchanged HEAD archive; they are historical migration/profile
fixture failures, so unrelated repairs were deferred.

A second debug 200k measurement (during baseline verification) recorded first page
30.22 ms, count 34.20 ms, expansion 2.63 ms, canonical program 1.89 ms, ten-Track bulk
connection 6.53 ms, targeted Album read 1.60 ms and bounded group refresh 15.27 ms.
The variability is reported rather than presented as a performance guarantee. Requests: zero.
