# Spotify playback slice validation (2026-09-15)

This is a control/observation diagnostic, not a source resolver. Catalog auth and
identity semantics were unchanged. Playback implementation/setup is described in
[the adapter guide](../adapters/spotify/PLAYBACK.md).

Live tests used `/tmp/music-library-spotify-playback.sqlite`, a separate copy of the
closed Painted Shut fallback test database. All ten Spotify song associations
already existed; no catalog request or file retag was needed. Credentials were
stored separately under `/tmp/music-library-spotify-playback-auth/`, never in the
database. No user library database was reset.

The user registered `http://127.0.0.1:43821/callback`, authorized the two playback
scopes, and explicitly confirmed **PUHI** as the intended desktop target. Devices
returned PUHI as `Computer`, initially inactive, unrestricted, selectable and
supporting volume. No other device was selected. The first authorization listener
expired before browser interaction; the bounded diagnostic window was changed
from three to ten minutes. The subsequent PKCE exchange succeeded.

| Action | Observed result |
|---|---|
| Start **Waitress** | Requested `spotify:track:6aYsKS9XcItrOUWNMLn3Ba`; state confirmed that Track playing on PUHI |
| Pause | State confirmed Waitress paused at 3,654 ms |
| Resume same Track | State confirmed Waitress playing, without resetting to zero |
| Seek to 42,000 ms | Immediate state was briefly stale; subsequent state showed 42,272 ms |
| Start **Well-dressed** | Requested `spotify:track:1veYThxB6P45NfLlmwJ8fA`; later state confirmed the new Track |
| Restart probe and database | Authorization and ten associations reloaded; no token request; devices rediscovered with no selection restored |
| Resume after explicit device reselection | Well-dressed played on PUHI |
| Seek/pause after restart | State reflected seek progress and then paused; test left Spotify paused |

Waitress application Track: `46801fc0-7a7c-40b7-a3bc-8e86a2c01d62`.
Well-dressed application Track: `ec136efc-c228-405a-b23c-4b6f0cdc7ca6`.

The first probe made **one user token request and 16 playback API requests**:
one device discovery, three start/resume requests, two pauses, one seek, and nine
state reads (including the pre-Play observations). The restart probe made **zero
token requests and eight playback API requests**: one discovery, one resume, one
seek, one pause, and four state reads. Both made **zero catalog requests**.
The later interactive QML session has separate counters and is not included in
these controlled probe totals.

Observed Play command plus pre-command state read took **335–413 ms**; pause
**177–193 ms**, seek **178–179 ms**, state reads **119–186 ms**. These are network
round trips, not decoder latency. Initial human-mediated authorization timing was
not measured. The probes confirmed state on later explicit polls; they did not
measure exact command-to-audible-output time. A command response is not treated as
proof of observed state; the UI uses subsequent polling.

Debug-build indexed persisted-association lookup plus URI construction took about
**0.09–0.20 ms warm**, up to **0.40 ms first lookup**. The generic API uses at most
two Track/provider keyed queries, manual first, automatic only if no manual choice
exists. It does not scan the Album/library or read files. No migration was added
for playback. The 200k regression retained roughly **6 µs** Album lookup,
**15 µs** ten-Track verification, **139 µs** first 50-row library page on this run.

Deterministic tests cover PKCE RFC vector/randomness, callback/state/denial,
token exchange/refresh/invalid-grant/restart/permissions/redaction, explicit and
restricted/stale devices, one-URI start versus resume, pause/seek/external state,
401 bounds, rate limits, service/transport/timeouts, and occurrence-only URI
semantics. Core tests exercise manual precedence and restart lookup. Existing
catalog/provider-chain/local playback suites remain separate.

The initial graphical launch failed because the shell inherited unavailable SSH
display `localhost:10.0`; WSLg exposes `:0`. Explicit `DISPLAY=:0`,
`QT_QPA_PLATFORM=xcb`, `QT_QUICK_BACKEND=software` and WSLg's Pulse socket are suitable
for that diagnostic environment. This is not a catalog/OAuth failure or a change
to the application's default display policy.

After that relaunch, the user confirmed the QML window was visible and Spotify
controls worked. The panel used the same persisted authorization and explicit
PUHI selection as the probe. All core tests, 24 Spotify catalog/playback tests,
19 MusicBrainz tests, five GStreamer tests, five QML tests, offscreen QML smoke,
QML lint, formatting, strict Clippy and diff checks passed. Intentionally ignored
live tests were not enabled in normal validation. The existing Qt teardown
warning about stopping timers from another thread remains; no test failed.

## Explicit resolution of a MusicBrainz catalog Track (2026-09-16)

The next bounded slice adds a separate, explicit **Search Spotify for this Track**
action inside the existing Spotify panel. It uses catalog Client Credentials and
the existing Spotify request/token/market/parser infrastructure, never playback
OAuth. Search is one page of at most ten Tracks using current title and Artist.
Results expose Album/date/duration/position to support explicit selection; no
candidate is automatically accepted. Playback-state notifications preserve the
chooser's selection. Cancel and stale replies cannot write an association.

The selected song ID is stored in the existing `track_external_identity` table.
No migration or Album/Recording identity promotion was needed. The explicit path
does not require local files or an accepted Spotify Album. Confirmation rechecks
metadata and conflicting associations inside an immediate transaction. An existing
association blocks replacement here. Manual Album-program associations retain
precedence, followed by independently accepted Track mappings, then automatic
program associations. Playback still performs no search.

Live validation used the same disposable
`/tmp/music-library-spotify-playback.sqlite` and separate playback credentials.
The first MusicBrainz catalog search exhausted three bounded HTTP 503 attempts.
A later explicit retry succeeded with three requests: Release Group search,
representative Release browse, and Release detail. Existing catalog Add Album
created the ten source-less Tracks for the selected representative catalog Release;
it did not relabel the local files as that edition. All ten initially had no
Spotify association. Import persistence took 13.3 ms in that run.

The user selected the **Unavailable** MusicBrainz-added **Hop Along — Waitress**:

* Application Track: `a47ab975-4898-44db-a6dc-010b67980b1c`.
* Candidate explicitly selected in QML: Hop Along / Painted Shut / Waitress.
* Accepted occurrence: `spotify | track | 6aYsKS9XcItrOUWNMLn3Ba`.
* User confirmed that resolution and playback on PUHI worked.
* User recalled the catalog counter as **1 token / 1 API request**, consistent
  with the deterministic request test. The interactive playback/polling total was
  not captured and is not inferred from the catalog counter.

A separate process reopened the database **with both catalog credential variables
removed** and loaded the association without searching. The playback probe then
reopened the database and persisted OAuth session, rediscovered PUHI, explicitly
selected it and resumed the new application Track. State confirmed the intended
Spotify ID playing on PUHI, then confirmed pause. This restart verification used
the diagnostic probe; a further graphical restart was not required to establish
storage/session durability.

Restart probe requests: **zero catalog**, **zero OAuth token**, **six playback**
(one device list, pre-Play state, resume, confirmation state, pause, confirmation
state). Play including its pre-observation took 306.7 ms, confirmation state
206.6 ms, pause 178.9 ms. The new canonical mapping lookup plus URI construction
took 78 µs in the debug probe. Spotify was left paused.

The user reported a disappearing cursor in this graphical run after successfully
testing resolution/playback, then requested that investigation be deferred as a
likely temporary glitch. No cursor-related application change was made.

Validation: 211 core tests, 25 Spotify catalog/playback tests, 19 MusicBrainz tests,
five GStreamer tests and five QML tests passed, alongside strict Clippy,
formatting, QML smoke/lint and diff checks. The QML test includes explicit
selection, cancellation, preservation across playback updates, and immediate
playability without initializing either network worker. Core tests cover
source-less catalog Tracks, no premature acceptance, stale metadata/conflicts,
unchanged Album/Release/Recording identities and reopen persistence.

At 200k Tracks, selected-Track metadata lookup was **8.51 µs median** and provider
association lookup **25.20 µs median**; first 50-row library page **148 µs**,
Album matching lookup **6.19 µs**, ten-Track verification **14.54 µs**. Lookups use
existing Track/provider indexes and never scan the whole library.

Replacing an existing independent song association, automatic playback-time
enrichment, source fallback, mixed queues, next/previous integration, queue
takeover and broad cross-provider enrichment remain deliberately deferred.

## Unified explicit-Play resolver (validation completed 2026-09-22)

This section supersedes the preceding slice's deferral of explicit playback-time
source resolution. Mixed queue advancement and broad enrichment are still deferred.

`Library::playback_route` returns Local(source), Remote(occurrence identity),
NeedsEnrichment, or Unavailable(reason). An adapter supplies current capability and
identity validation; core selection does not branch on provider name. Local-first
selection probes only the Track's indexed available sources, in stable order. It
checks a regular file exists and opens, with no durable availability mutation.
Missing paths are skipped; unexpected access or engine failures are not hidden by
Spotify playback. No filesystem or network work enters a write transaction.

The existing song search gains optional duration/disc/position input. Automatic
acceptance is exact normalized Artist/title/Album plus compatible supplied positions
and duration (3,000 ms tolerance), unique on a complete bounded page. No fuzzy or
version-word removal is added. The existing transaction protects prior/manual
associations and rechecks metadata. Only a Track song ID is written; no migration,
Album mapping, Recording identity, edition identity or membership change occurs.
Ambiguous results reuse the existing chooser without another request. Explicit
Stop/cancel invalidates the pending lookup. Local availability is checked again
after a successful network response, before selecting the final backend.

The QML diagnostic's normal Play uses this resolver in real-audio mode. Its separate
Spotify debug controls remain. Local Stop acknowledgment precedes remote Play;
remote Pause acknowledgment precedes local Start. A failed pause or stop withholds
the destination backend. Ownership is separate from engine position/status. Remote
polls continue while the panel or application-owned remote backend is active;
polling never searches. Queue EOS/Next still does not resolve remote sources.

### Isolated live results

Used a SQLite backup of the previous disposable playback database:
`/tmp/music-library-playback-resolver-20260916.sqlite`. The original test/library
database and real music files were not renamed, removed or reset. Existing separate
playback credentials were reused, and **PUHI**, Computer, unrestricted, was selected
explicitly. No other returned device was selected.

| Explicit Play case | Catalog token/search | Playback token/API | Result |
|---|---:|---:|---|
| Local Waitress | 0 / 0 | 0 / 0 | GStreamer confirmed Playing; remote clients never constructed |
| Source-less Waitress, known association | 0 / 0 | 1 / 6 | Sent saved song to PUHI; observed Waitress, then paused |
| Source-less Buddy in the Parade, no association | 1 / 1 | 0 / 6 | One complete candidate, Unique(0), persisted song, sent to PUHI, then paused |
| Buddy in the Parade, separate-process reopen | 0 / 0 | 1 / 6 | Saved association reused; observed correct song Playing on PUHI, then paused |

Automatic Buddy candidate: Hop Along / Painted Shut, disc 1 Track 2,
227,355 ms, `spotify:track:5CjoGMN0n3tIG4XaCcseoS`. Catalog token+search took
425 ms; persistence 305 µs; initial Play command 303 ms. Local Waitress source
lookup/stat/open took 1.09 ms on `/mnt/f`; GStreamer played without Spotify calls.
The reopen Play command took 411 ms; the correct Playing state was observed after
1.566 s (including the probe's deliberate one-second wait before polling).
Playback's token refresh was separate from catalog authentication.

The original immediate post-command polls could still show the previous song.
This was observation latency, not a wrong association: the subsequent poll showed
the requested song. The live probe now waits/polls a bounded five times to verify
Playing; production retains its modest existing polling cadence. The six playback
API requests comprise discovery, pre-Play state, Play/resume, observed state, Pause,
and paused state. These are probe counts, not a promise that visible UI polling
will always have the same total.

The QML resolver's four explicit handoffs, pause/stop failures, one-search automatic
acceptance, reuse without search, and ambiguity → existing chooser are deterministic
tests with fake worker channels. Live probes exercise the same core resolver,
association persistence and real adapters. Visual GUI handoff feedback is separate
from those automated/probe results.

During subsequent GUI testing the user reported local playback stopping after about
one second. The exact displayed error was not captured, so clean live GUI handoffs
are **not yet confirmed**. The complete Waitress file decoded successfully to a
silent sink; that does not rule out an audio-output or orchestration failure. The
diagnostic now logs local engine errors with generation and application Track ID.
No decoder policy or automatic fallback was changed to conceal this failure.
The user will retest manually using the launch command; competing live probes were
stopped. A GLib shutdown warning alone was insufficient to establish a cause.

A subsequent report used **Replace queue & play** on an available local row after
Spotify playback. All ten local files still existed. Inspection found that a failed
Spotify pause withheld queue replacement (intentionally avoiding overlap), while
the old Track's “No usable local source” error could remain visible. New explicit
requests now clear stale errors, and handoff failures explicitly say the local
source is usable but Spotify could not be stopped. Each route decision is logged
with its application Track ID.

The handoff now checks fresh Spotify state before Pause: idle/paused requires no
redundant command, and a different active device is not commandeered. A definite
rejection of a first Spotify Play does not establish remote ownership; unknown
transport outcomes and previously owned audio remain protected. Deterministic
tests cover these cases, failed state observation/real Pause, stale UI errors,
queue replacement after a successful handoff, and local playback after a rejected
remote start. Spotify tests now total 27 passed. This does not claim the live API
rejection itself has been identified or eliminated; manual retesting is pending.

### Performance and validation

At 200k Tracks: complete route lookup including missing-source check and known
remote association **23.00 µs median** (p95 26.04 µs); regular-file stat/open/metadata
**4.90 µs** on the Linux temp filesystem; selected-Track metadata **9.68 µs**;
provider association **25.47 µs**. EXPLAIN shows indexed Track→source, source primary
key and local observation primary-key searches, with no global source scan. The
broader 200k check completed; existing slower artist/availability diagnostic query
shapes remain unrelated to the new indexed playback path.

Core regression: 216 passed, 8 ignored. Spotify: 25 passed. MusicBrainz: 19 passed,
3 ignored. GStreamer: 5 passed, 1 ignored. QML: 5 passed, 1 ignored, including the
added handoff/automatic-resolution checks inside its existing Qt integration test.
The sandbox initially prevented localhost mock binding/audio; reruns with the
required access passed. The old default performance fixture was stale; validation
used a freshly generated isolated 200k database instead.

Before mixed queue advancement, define remote completion/EOS ownership and
generation handling across sources, external Spotify changes, and queue navigation
policy. Do not turn polling into catalog enrichment or treat Spotify identity as
playback entitlement. Source preference, moved-file discovery, replacement/clearing
of independent song mappings and cross-provider Recording reconciliation remain
deferred.

## Mixed queue navigation audit (2026-09-22)

Queue Next/Previous and local EOS now stop the previous backend, select the next
application Track, and invoke the existing playback resolver. Selection remains
current on failure/ambiguity. No queue prefetch, Spotify queue mutation, identity
change, source deletion or automatic skipping was added. The existing chooser
preserves queue position when resolving the current Track. Clear and Replace Page
also acknowledge remote pause before changing queue state.

Spotify completion observations carry the application playback generation so
delayed or duplicate notifications cannot advance a replacement queue. The adapter
requires fresh playing state within six seconds of known duration, followed by
two stopped/no-Track observations on the same still-known device at/after expected
end, within 15 seconds. Pause, device loss/HTTP 204, error, changed song/device,
or repeated-song progress reset do not establish EOS. Ordinary polling remains
five seconds, with the existing one-second post-command observation and backoff.
An external change disarms automatic advancement until an application action.

### Actual live results

Used copies of the previous disposable resolver database, not the user's normal
library: `/tmp/music-library-mixed-queue-20260922.sqlite` and
`/tmp/music-library-mixed-eos-20260922.sqlite`. No user's media file was changed.
The EOS copy points one source to a generated two-second silent WAV in `/tmp`.
Playback authorization was reused, and PUHI was explicitly selected.

* Real QML bridge: The Knock (local) → Next → Waitress (Spotify) → Next →
  The Knock (local) → Previous → Waitress (Spotify) → Clear: passed, 32.08 s.
* Real local EOS: two-second WAV → automatic Waitress (Spotify) → explicit Next
  → local Waitress → Previous → Spotify → Clear: passed, 32.19 s.
* Each run: **0 catalog token requests, 0 catalog API requests**, 1 playback token
  request and 11 playback API requests, including discovery/state/control. Local
  entries issued no catalog or start-Spotify commands; leaving Spotify necessarily
  performs the acknowledged state/pause handoff. Source checks occurred lazily for
  the four selected entries, never for later queue entries.
* First headless attempts failed to acknowledge GStreamer start/stop with the
  inherited display. Explicit `DISPLAY=:0` fixed the live harness; no GStreamer
  code or sink policy was changed.
* Separate Spotify terminal probe: Waitress (`6aYsKS9XcItrOUWNMLn3Ba`) played on
  PUHI; two near-end seeks were followed by the **same song playing near zero**,
  not terminal state. The probe used 0 catalog requests, 1 playback token request,
  9 playback API requests. Play took 593 ms, seeks 192/185 ms, polls 161/173/134 ms,
  and final Pause 228 ms. Repeat/shuffle settings were not modified.

**Natural Spotify → next application Track is not live-proven on this client.**
Its repeat/restart response is deliberately not guessed to be EOS. Therefore a
fully automatic Local → Spotify → Local queue cannot yet be called a stable
foundation under the strict no-false-EOS policy. Manual Next works. External-change
and pause protection are deterministic-test results; an external-control live
sanity test remains outstanding.

### Validation and local cost

Core 217 passed (8 ignored); MusicBrainz 19 passed (3 ignored); GStreamer 5 passed
(1 ignored). QML 5 passed (2 opt-in tests ignored), including mixed navigation,
asynchronous Stop acknowledgement, stale remote completion, unresolved current
Track and no-prefetch checks inside the existing Qt integration test. Spotify
adapter tests and four completion-policy tests passed. Formatting, strict Clippy,
QML build/smoke/qmllint and diff checks passed. The existing Qt teardown timer
warning remains unchanged.

Fresh 200k fixture: selected provider association median **24.610 µs**, p95
29.539 µs; resolver including known association median **22.390 µs**, p95
25.929 µs; local stat/open/metadata median **4.850 µs**. Query plans use indexed
Track/source keys, without a global scan. Queue selection is constant-time; no
separate handoff-bookkeeping benchmark was added. Network timings above are not
included in resolver timings.
