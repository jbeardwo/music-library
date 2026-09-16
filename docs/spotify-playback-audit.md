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
