# Focused Spotify robustness pass — 2026-10-10

No commit was made. The original fresh-import database was not modified. Real
validation used disposable SQLite backups; [structured evidence](spotify-focused-robustness-evidence.json)
contains raw provider programs, canonical Album/Release IDs, trusted identities,
positions, durations, decisions, and the resulting Track → Spotify Track mappings.
This report supersedes older evaluator/search-version and batch-limit statements
in the reconciliation audits. Evaluation version is **12**, Album search strategy **4**.

## SQLite contention and writer coordination

The lock-holder pattern was a **deferred transaction with an existing read snapshot**
in reconciliation persistence, overlapping an adapter discovery-cache writer on a
second connection. `persist_spotify_review` and candidate-attempt persistence read
canonical/review state and then attempted to upgrade to a writer. In WAL mode, a
cache commit between those operations invalidates that snapshot; SQLite returns
`SQLITE_BUSY_SNAPSHOT` immediately. The ordinary “database is locked” text hid this
cause. A longer busy timeout cannot repair the stale snapshot. A deterministic
regression reproduces this failure even with a five-second timeout.

Reconciliation summary/candidate writes now acquire `BEGIN IMMEDIATE` before their
transactional snapshot reads. Related candidate/review writes share one short
transaction. Album/Track negative-state writes use the same discipline. Matching,
program comparison, and HTTP happen outside the writer; snapshot revalidation and
association persistence happen inside the short write transaction. Existing sensible
busy-timeout behavior remains a safety net.

Audited writers include bulk retry, individual Album/Track retries, Album confirmation,
manual mapping/object attachment, candidate snapshots/reevaluation, association and
status completion, discovery-cache writes, exclusions, and UI-triggered refreshes.
HTTP workers keep independent cache connections. Bulk and individual Album retry
requests now join one UI-owned coordinator (including the legacy Track diagnostic's
Album retry), rather than creating parallel bulk passes. Manual review actions use
short serialized SQLite transactions; ordinary inspection uses cache-only reads.
No writer remains open while a provider request or QML callback is pending.

Stress evidence:

- Deterministic stale-WAL upgrade reproduction and three-connection concurrent
  reconciliation/cache tests: **240 writes, zero lock errors after the fix**.
- Disposable fresh-data replay: **11 Album targets, 50 review opens, 50 concurrent
  cache writes, 64 new connections, zero lock errors, zero provider requests**.
  This copy intentionally retained the fresh database's absent Seawater equivalence.
- Actual coordinator fixture: Retry Unresolved + opening Album inspection + Retry
  Album overlap; the extra Album joins the queue and completes without lock errors.
- Adapter mock performs a second-connection `BEGIN IMMEDIATE` during an actual HTTP
  request, proving the request does not own a SQLite write transaction.

## Retry Unresolved and stable decisions

The [coordinator follow-up](spotify-reconciliation-coordinator-audit.md) supersedes
the continuation proof below: the earlier unconfigured-provider fixtures missed an
asynchronous Track-fallback return that still ended runs after one chunk. Configured
asynchronous fixtures now cover 65/101 Albums and automatic startup/import readiness.

A click starts cached reevaluation, then keyset-paged **20-Album batches**, not a
20-Album total limit. The coordinator retains prepared batches across async cache
completion, deduplicates additions, and accepts an explicit Album retry while running.
An established Artist revision schedules affected earlier contexts with the existing
Artist index and another bounded cursor. Newly persisted programs are replayed before
requesting dependent Track searches. Local reevaluation runs until no acceptance can
change another decision; the former three-sweep limit is removed.

Provider work remains bounded: at most three compared Album candidates, existing
program-page/search caps, and **two standalone Track fallback searches for the entire
click**. This last bound is deliberately retained: exhausted global-search work is
outside the pass and can be reviewed individually. The queue does not repeatedly
refresh a completed unsuccessful scope. Errors abort the pass; cooldown/quota preserve
Waiting for Spotify and stop provider work. Progress shows current batch and processed/
scheduled counts. Cached work runs off the UI thread.

The queue regressions demonstrate:

| Case | One-click result | Immediate second click |
| --- | --- | --- |
| 23 eligible Albums, beyond one page | all 23 processed | zero fresh provider operations |
| overlapping extra Retry Album | extra context completed without duplication | no omitted context |
| new Artist evidence enables 22 earlier contexts | 45 context considerations, only 23 fresh provider operations | zero additional fresh work |
| program arrives after earlier Album evaluation | all three newly eligible Tracks connect | zero additional calls/connections |

On the real representative set, **85 initially unresolved Tracks across 11 Albums**
produced **79 connections and six remaining Tracks**. Initial bounded discovery used
**one token request and 16 catalog requests** in the recorded live run. A subsequent
cache-only spelling correction connected Floral LP's final Track. The final second
pass over the same contexts connected **zero** and issued **zero token/catalog requests**.
An additional actual Qt coordinator run used a scoped fresh-import copy containing
these 11 Albums, current discovery cache and the explicitly confirmed Artist equivalence.
One Retry invocation checked all **85 Tracks**, connected **79**, and stopped with **six
Waiting for Spotify** under a retained cooldown, making **zero requests**. Its immediate
second click connected zero and made zero requests; all temporary queue buffers were
cleared. Newly added provider contexts were zero in this cache-complete run. The growing
queue and overlapping Retry Album behavior are exercised by the Qt fixtures above. No claim is made that synthetic fixture
provider operations are real Spotify HTTP requests.

Needs Review now has inspectable candidates/mapping evidence. An Artist-only blocker
is labeled “Artist identity needs review” rather than pretending Album candidates exist.
An empty successful candidate report persists No Candidates. Needs Retry denotes stale/
new evidence or eligible bounded discovery, rather than an invisible queued task.
Queued reevaluation is cleared by completion or stopped-pass handling. Retry actions
start/join useful work or return configuration/cooldown/running explanations. Inspection
reloads current persisted evidence before presenting a decision. If a historical review
summary has no remaining inspectable evidence, opening it repairs the summary to Needs
Retry without destroying a current successful No Candidates decision.

## Connections and initial-page UX

The Album context menu's **Mark all unresolved tracks as not on Spotify** takes a
transactional snapshot of current saved unresolved Tracks. Connected Tracks survive;
no inherited Album rule is created and later additions remain eligible. Confirmation
counts only unresolved Tracks. Targeted model refresh updates unresolved/marked counts
and both management views while retaining surviving stable-ID rows and scroll anchors.
The regression marks 446 unresolved Tracks while preserving four connections, then
restores the marks through existing management actions.

Album rows use a full-row HoverHandler and existing highlight styling. Hover over
Review/Retry retains the parent highlight; keyboard focus remains visible. The list
has a real Qt Controls vertical ScrollBar with a draggable thumb and normal wheel
behavior. It reflects the bounded current Album page; existing keyset Previous/Next
navigation remains. Tests drag the thumb and verify actual content-position changes,
including the 200k fixture. Review/retry/exclusion retain the current anchor where possible.

The first Artists page uses a QML-derived ListModel. The root's first refresh could
arrive before child model bindings settled, leaving that ListModel empty until a later
navigation signal. Initial setup now performs the normal efficient browse refresh and
schedules derived-pane synchronization with `Qt.callLater`, using the already-fetched
page. It does not query the library twice. The regression asserts Artists and delegates
before any navigation, then checks Albums, Genres, Songs and Playlists through the
existing model paths. Import/change subscriptions retain their normal refresh path.

Changing the Review target immediately clears candidate/program selection, counts,
status, manual mapping and Artist proposals; it also invalidates pending Track lookup.
Async callbacks check both generation and canonical target, including error callbacks.
Inspection exposes target ID and generation. Rapid Album/Track switching, manual mapping,
and late-result tests pass; a previous object's response cannot overwrite the new target.

## Real-case decisions

| Artist / canonical Album | Initially unresolved | Newly connected | Remaining |
| --- | ---: | ---: | ---: |
| 100 Onces / 100 One Says | 8 | 8 | 0 |
| Elephant Gym / Angle | 11 | 11 | 0 |
| Elephant Gym / Balance | 3 | 3 | 0 |
| The Speed of Sound in Seawater / Blue Version | 5 | 5 | 0 |
| The Speed of Sound in Seawater / First Contact | 10 | 10 | 0 |
| Floral / Floral EP | 6 | 6 | 0 |
| Floral / Floral LP | 9 | 9 | 0 |
| Hella / Homeboy | 4 | 4 | 0 |
| Elephant Gym / Underwater | 12 | 6 | 6 |
| Elephant Gym / Work | 5 | 5 | 0 |
| Tera Melos / X’ed Out | 12 | 12 | 0 |

### Tera Melos — X’ed Out

Raw canonical title is `X’ed Out` (U+2019). The initial generated query is
`album:"x'ed out" artist:"Tera Melos"`. Spotify returned no useful result; the one
bounded alternate is **`xed out Tera Melos`**. This fresh discovery finds
`5PhJFFymcvBOpabwa77s6S`, raw provider title `X'ed Out`, with all 12 Tracks.
The candidate is accepted through typography-normalized title + Artist + program.
The remix EP competitor is rejected. All 12 Tracks connect.

The existing shared typography normalization covers straight, left/right curly,
modifier-letter and fullwidth apostrophes with NFC. Canonical comparisons, ranking,
candidate evaluation and search generation use it. Equivalent typography generates
the same semantic query/fingerprint; the changed alternate query has its own exact
request key. Search-strategy version was bumped because discovery semantics changed.
Regression starts with an empty discovery cache and proves discovery plus cached replay.

### Hella — Homeboy

The old persisted blocker is **`album_version_mismatch`**. It disagreed with the
retained current candidate/program: cache evaluation already recognized `Homeboy EP`
as a release-type annotation, not a musical version. The bug was stale decision/
completion orchestration leaving that accepted evidence unapplied, not a missing fuzzy
identity heuristic. The fixed queue replays and completes that evidence automatically.

Candidate `5TgI4E1qDUKJqc9zqcVfMV`: Hella, `Homeboy EP`, Spotify Single, 2005-11-08,
four complete ordered Tracks. Established Artist ID `1n861RIk6CTAWncgHR9UHg` agrees.
Local year/type are unknown; MusicBrainz Release Group
`43f834d4-60ed-3d22-ba4b-6ec07e60cc79` is retained, with no conflicting Release ID.

| Position / title | Local duration ms | Spotify duration ms |
| --- | ---: | ---: |
| 1 Gothspel for You Not Them | 671922 | 671826 |
| 2 Madonna Approaches R&B Blonde Wreckages | 142681 | 142600 |
| 3 BC But Not Before Christ | 229329 | 229240 |
| 4 If I Were in Hella I Would Eat Lick | 720980 | 720893 |

Four exact anchors, positions/count/Artist agree, no trusted Recording contradiction.
Final decision: accepted release-type suffix; **all four connect**. Structured evidence
contains each local Recording/Track identity as well as the provider program.

### 100 Onces — 100 One Says

The actual catalog evidence differs from the apparent “missing Track” interpretation:
both provider candidates contain **11 occurrences**. Candidate A
`1ql6q89vMISNRgTafQNNhr` (2013-05-30) has `Conspiracy Keanue` and `Gary Bucey`;
candidate B `4Dry8d9ExiK8k08YVquhV7` (2013-05-31) has `Conpiracy Keanu` and
`Gary Busey`, matching the local spellings. The other nine ordered titles agree,
provider durations are identical, Artist IDs/type/title agree, and there is no
edition/Recording contradiction. Both complete raw programs are in the JSON artifact.

They form a compatible playback family. Family comparison is candidate-set scoped,
requires strong ordered overlap, compatible trusted Artists/title/version/type,
noncontradictory Recording IDs, and compatible shared durations. Different occurrences
can supply different canonical Tracks. Minor complementary spelling observations are
family evidence only; they do not rename Tracks or declare canonical Albums equal.
Every pair must pass; a transitive chain cannot accidentally union conflicting editions.
Live/studio/remix/program/Artist contradictions veto families. Synthetic complementary
missing-Track tests also union both sources; incompatible-edition tests remain unresolved.

Resulting mapping (includes three previously connected Tracks):

| Raw canonical Track | Spotify Track ID |
| --- | --- |
| Oh Me Gee | 1jn1FIAcziHhd1UJ0pLrQo |
| Copornopocalypse | 0WCM5UMpMKxhQEg3nzlfG1 |
| Conpiracy Keanu | 4cvmbDjMATlwofr7AzZUBb |
| Fucking Guy, No Offense | 7LLotTZWVFbKi5zvBR1EVO |
| No | 4cX0ADGRFRoDzzkuLhbL2s |
| Organicore | 1nVumZQIcTT74P26dEJigy |
| Djimi Hendrix | 1InjlegI9SDhyBY4ipFbVw |
| Gary Busey | 4Y4cIrvVxbImGzY5BcFlzn |
| Cannibals as Leaders | 0g7th6ROms3m8txyFtCOgr |
| El Chupabillbrah | 0fz4jVKc0BLMFuCG9xl07M |
| Sick Ninja | 2TNTVogGcICxbpcjkxqZkd |

Eight unresolved Tracks connect and none remain. Canonical Album equality remains
withheld because exact editions differ; playback succeeds without choosing one wrapper.

### Elephant Gym and language-neutral evidence

Full normalized title correspondence is always first and strongest. Non-Latin titles
are neither stripped nor down-ranked. Exact Chinese `水中漫步`, Japanese `夜の海`,
Korean `수영`, Cyrillic `Привет`, and Arabic `مرحبا` use the ordinary full-title path.
Only after full-title comparison fails does a meaningful Unicode-boundary-aware shared
span become possible, under strong Artist/program context and unique correspondence.
Either script may be the shared component. There is no fixed number of characters,
shared repeated prefix requirement, language preference, translation or transliteration.

Actual multiple-character/multiple-word cases:

| Raw local title | Raw Spotify title | Full-title equal | Shared span / extra span | Context / result |
| --- | --- | --- | --- | --- |
| Mirror | 鏡子 Mirror | no | mirror / 鏡子 before | Work 5-Track program; connected |
| Midway | 中途 Midway | no | midway / 中途 before | Work 5-Track program; connected |
| Spring Rain | 春雨 Spring Rain | no | spring rain / 春雨 before | Work 5-Track program; connected |
| Strong Ladder - 堅固耐用的梯子 | Strong Ladder | no | strong ladder / 堅固耐用的梯子 after | Angle 11-Track program; connected |
| Ordinary Route - 日常的航道 | Ordinary Route | no | ordinary route / 日常的航道 after | Angle 11-Track program; connected |
| Ocean in the Night - 夜洋風景 | Ocean in the Night | no | ocean in the night / 夜洋風景 after | Balance 4-Track program; connected; extra Artist warning |
| Dance Together - 一起跳舞 | Dance Together | no | dance together / 一起跳舞 after | Balance 4-Track program; connected |
| Galaxy - 銀河 | Galaxy | no | galaxy / 銀河 after | Balance 4-Track program; connected |
| D | D | yes | no fallback | Work ordinary exact full-title match |
| Finger | Finger | yes | no fallback | Balance ordinary exact full-title match |

JSON records full comparison, shared/extra spans, positions, durations, complete program
anchors and final mapping for every evaluated Elephant Gym Track. Work's provider Album
is `工作 Work`; its strong program supplies playback without asserting canonical Album
identity for that raw title difference. Work connects 5, Angle 11, and Balance's remaining
3. Underwater connects 6 of 12; four local Tracks are absent from the returned US program
and two have meaningful International Collaboration Version differences, so six remain.

**No actual Elephant Gym pair in this fresh snapshot has the same foreign-only title on
both sides.** That requested live subcase cannot be reported as observed. The ordinary
path is explicitly covered by five-script regressions; actual D/Finger confirm the
non-fallback path on this catalog. Synthetic fixtures cover Chinese/Japanese shared
components, extra multiword text before/after, varying spans, punctuation/boundaries,
exact outranking partial, duplicate Intro ambiguity, version retention, arbitrary
substring rejection, and unchanged raw metadata. No translated-only match is attempted.

### Seawater / tsosis — First Contact and Blue Version

The fresh DB contained **zero Artist-equivalence records**. The relationship stated in
this request was established through the existing explicit equivalence API on the
disposable copy; it is not an invented alias or product hardcode. That trusted provider
ID participates in prepared Album/Track Artist evidence, cached reevaluation, candidate
filtering/ranking and discovery. A regression verifies discovery without an own provider
ID and preservation of the canonical Artist. Multiple distinct provider search names
can now use one bounded scoped fallback if the initial title page has no useful result.

Provider query names are `tsosis`: `album:"first contact" artist:"tsosis"` and
`album:"blue version" artist:"tsosis"`. Candidates `6E4PRWgOET65TzwbWsvwfJ` and
`7EElOaXs5gwIkkQAvTUhyb` accept their complete programs; **10 + 5 Tracks connect**.
Persisted list/detail state agrees; neither Album remains a false Needs Review/No
Candidates pair. The original fresh DB still needs that explicit user relationship
restored/confirmed; this pass does not silently write it into the user's original data.

### Floral EP / Floral LP

Correct complete programs rank ahead of similarly titled wrong releases independently
of Spotify API order. Floral EP candidate `4oH7pwltJtcV5Kg9hdb6Kr` has the correct six
Tracks and is accepted. Floral LP `1YF1hAfo9oDDOiWDTPK7jX` ranks first for its nine-Track
program; EP competitors fail type/count/program evidence. The former broad-page veto
no longer blocks a uniquely complete full exact program. In LP's actual case, the final
Track is local `A Truckers Wife` versus Spotify `A Trucker's Wife`: a playback-only
apostrophe-omission correspondence with matching position and known duration connects
it under the otherwise complete program. Canonical comparison stays conservative;
its incomplete broad-page Album decision is still withheld. All six EP and nine LP
playable sources connect without modifying canonical titles or IDs. Generic same-type
similar-name fixtures prove program coverage dominates name similarity and API order.

## Best-available playback and preservation

A unique sensible Track with established Artist, strong title and no trusted Recording
contradiction can supply playback across remasters, provider Releases, regional wrappers,
split singles, additional credits and different provider positions/program sizes.
These differences remain warnings rather than Track-level vetoes. Exact title/credits
outrank weaker alternatives; genuinely competing live/acoustic/demo/remix versions remain
manual review. A source-only Album-program completion path never attaches Album identity,
changes canonical Track membership/Release, rewrites raw titles, or establishes Recording
identity. Existing associations, exclusions and user edits remain authoritative.

Discovery cache/TTL, Retry-After and quota abort, scoped/bounded fallback, exact manual
attachment, replacement/clearing, advanced/split mapping, reordered/inserted programs,
MusicBrainz evidence, overrides, Library Artist assignment, local-first playback,
playlists, Hidden Artists and ignored playback preferences remain covered by the core,
adapters and focused Qt regression checks. No schema/EAV/framework change was introduced.
Existing keyset/Artist indexes support targeted queue work; no full-library reevaluation
occurs after each association. The 200k Connections test checks bounded scrolling.

## Verification and remaining limits

Core full suite: **474 passed, 18 ignored**; final lifecycle suite after the missing-
review-evidence regression: **29 passed**. Spotify: **72 passed, 3 ignored**, plus one
ignored live-playlist fixture. MusicBrainz: **20 passed, 3 ignored**. Qt queue/retry checks:
**6 passed**; scoped real coordinator, initial Library views, stale/offline Album review,
Connections hover/drag/exclusion, and explicit 200k Connections checks each pass. The
200k run took **13.39 seconds**, including its UI interaction waits. Strict Clippy passes
for core, Spotify and GStreamer-enabled Qt targets; formatting and diff checks pass.

The earlier full Qt run had **32 passed, 3 failed, 18 ignored**. It has three unrelated failures that reproduce with the original HEAD Main.qml:
`fixed_album_tiles_wrap_and_split_handles_preserve_interactions`,
`mouse_keyboard_selection_queue_and_drawer`, and
`song_album_headers_identity_paging_and_interactions`. Their existing selection/reflow
assertions are outside this reconciliation pass; no full Qt-suite pass is claimed.

Deferred: translation-only/romanization/transliteration, automatic language detection,
machine translation, locale-specific title databases/tokenization, translated regional
Release names, market/region catalog selection, and expanding provider search caps.
The six Underwater Tracks and absent original DB equivalence are explicitly unresolved
conditions rather than fabricated successes. No commit was made.
