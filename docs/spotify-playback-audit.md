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
