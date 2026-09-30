# First library/player interface

The QML diagnostic shell now serves as a usable desktop library/player. It remains
an implementation prototype, not the public skin format or a final frontend decision.

Current top-level views, sorting and local Genre behavior are described in
[library presentation](library-presentation.md#library-navigation). The interaction
hierarchy below describes the default Artists view.

## Interaction

- Artists and Albums are derived exclusively from saved Tracks. No source is required.
- Initially all three panes are populated; Artists, Albums and Songs are alphabetical.
- Artist selection preserves the Artist list and filters Albums and Songs. A Track
  belongs to an Artist through a Track, Release or Album credit; overlapping credits
  contribute it only once. Different Artist IDs remain different rows, even when their
  names match. Identity reconciliation uses the evidence described below.
- Album selection leaves Artists and the current Album list intact. Songs show every
  saved Track in that Album, grouped by Release and ordered by disc/Track position.
  Unknown disc defaults to disc 1 for ordering; unknown Track positions sort last.
  No external program supplies missing Tracks.
- Song selection only highlights a row. Artist/Album “Show all” controls and Escape
  clear filters. Clearing Artist also clears Album; clearing Album retains Artist.
- Up/Down selects in the focused pane; Enter plays; Tab traverses controls.
- Double-click, Enter and context-menu Play now replace the queue with the complete
  Songs program. A Song starts at its Track ID's position; Artist/Album Play selects
  that scope and starts at the first Track. All Songs and Artist programs are
  alphabetical by title with Track ID as the tie-breaker. Album programs retain
  Release/disc/Track order. Single-clicking a Song remains selection only.
- Add to queue appends one Song, an Artist's complete alphabetical program, or an
  Album's complete ordered program, preserving duplicates and existing playback. Queue construction never contacts a catalog. Playback uses the
  existing resolver, including its existing explicit-Play enrichment policy.
- The persistent player shows title, Artist and Album, Previous/Play/Pause/Next,
  elapsed/seek/duration controls and a shared 0–100 volume slider. Missing Track artist text falls back to
  Album credits for presentation only; effective metadata and credits are unchanged.
  Seeking and volume route through common application controls to GStreamer or the
  selected Spotify device. Unknown duration disables seeking. Accepted remote volume
  is retained through stale polls and carried into the next local playback handoff.
- Clicking the current Track toggles an upward-opening Now Playing panel. Escape or
  its close button dismisses it. The panel shows ordered titles and Artist/Album,
  marks the current occurrence, and supports clearing. Outside clicks remain usable
  for browsing; queue reordering/removal is deferred.

## Bounded data access

`Library::browse` provides cursor-based pages of at most 201 rows (200 visible and
one lookahead), with indexes introduced by migration 13. SQL performs credit and
membership filtering; no query runs separately for each QML delegate. SQLite's
existing built-in case handling provides stable alphabetical ordering; locale-aware
collation is not introduced here.

Each pane retains only its current page and a small cursor history. Unchanged pane
models retain focus and scroll state across selection and playback notifications.
The queue presentation is separately bounded to 200 entries with page controls.
Playback still owns the complete explicitly requested queue and presentation labels.

`Library::library_queue` reads a complete requested program in one query.
`Library::library_queue_reader` supplies a read-only connection for the QML adapter's
background preparation of all replacement programs and Artist/Album appends. It runs no migrations or network work.
Only one preparation is accepted at a time. A visible “Preparing queue…” state keeps
browsing and playback controls usable; the existing queue remains until replacement
can proceed. Remote replacement waits for the existing remote-stop acknowledgement.
The captured program and starting index survive later browsing selections and remote
stop acknowledgements. The chosen Track is found by opaque ID in the complete
result, never by title or visible-page index. If it has left the library, Play
reports an error and retains the old queue. The drawer opens on the starting
occurrence's page. Full program labels remain in the player, not QML.

Measurements on the existing deterministic 200k fixture, debug build, warm filesystem:

| Query | Observed time |
| --- | ---: |
| First Artists / Albums / Songs page | 2–4 ms |
| Artist Albums, 200 results | 9–13 ms |
| Artist Songs, 201 results | 7–9 ms |
| Album Songs, 10 results | under 1 ms |
| Songs after 20k results | about 2–3 ms |
| Complete 20k-Track Artist queue | about 280–480 ms, off the UI thread |

These are measurements, not cross-machine budgets. Reproduce with
`cargo run --offline --example browse_performance` after creating the existing
`database_performance` fixture. Large explicit queues still consume memory
proportional to their Track count in the player, not in QML.

## Running and deferred work

```sh
cargo run --offline --manifest-path tools/qml-diagnostic/Cargo.toml --features gstreamer -- \
  --library /absolute/path/library.sqlite --no-auto-match
```

`--library` opens durable state without requiring local files or rescanning a folder.
Existing `--gstreamer FOLDER` import/startup and environment-based database selection
remain available. Launch without arguments for the fake-audio demo. The application
does not set Qt rendering, platform or cursor environment variables.

Search opens a temporary global local-library panel with grouped results, type
filters and exact browser navigation. See [local-library search](local-library-search.md).
Add Music opens the existing Add Album workflow;
configurations without catalog Add support explain that in a small dialog. Local
folder import still uses the existing launcher path. Provider setup and matching
remain available under the small settings menu, outside ordinary row actions.

Next UX work: a unified Add Music/local-folder flow, smoother continuous
browsing across page boundaries, and queue editing. Duplicate Artist labels reflect
distinct identities when insufficient evidence exists; the UI never groups by name. Artwork, playlists, skins,
locale collation, matching changes and catalog redesign are deliberately deferred.

## Validation

- Core SQLite tests cover membership-derived panes, source-less Tracks, partial
  Albums, Artist filters, title/position ordering, duplicate cursor keys, complete
  queues beyond a page and presentation-only credit fallback. Migration tests were
  updated for schema 13.
- QtTest sends actual mouse/key events to the shipped delegates: selection, double
  clicks, context menus, Up/Down, Enter, Escape, drawer toggling and bounded queue pages.
- Existing QML playback/callback tests cover mixed local/Spotify queues; added
  replacement coverage verifies a program survives browse changes during the remote
  pause handshake. These use controlled Spotify worker responses, not live Spotify.
- The opt-in desktop audit used a copy of a real 499-Track library with GStreamer,
  brief low-volume local playback, all three queue actions, player controls and
  screenshots. Screenshot review checked layout and readable controls. Mouse events
  worked on the normal desktop backend. Hardware cursor visibility still needs a
  human check on the user's desktop; no rendering workaround is installed.
- The same interface audit passed on the 200k fixture with fake audio. Browsing and
  queue presentation stayed bounded. No unrelated live catalog/network suites ran.

```sh
QT_QPA_PLATFORM=offscreen cargo test --offline --manifest-path tools/qml-diagnostic/Cargo.toml \
  --all-features -- --test-threads=1

# Opt-in desktop audit; use a COPY because opening may apply migrations.
MUSIC_LIBRARY_UI_AUDIT_DATABASE=/tmp/library-copy.sqlite MUSIC_LIBRARY_UI_AUDIT_AUDIO=1 \
  cargo test --offline --manifest-path tools/qml-diagnostic/Cargo.toml --all-features \
  real_library_desktop_audit -- --ignored --nocapture
```

The desktop audit writes `/tmp/music-library-ui-library.png` and
`/tmp/music-library-ui-queue.png`. Its audio path assumes the first displayed Artist
has a playable local program; omit the audio variable for a source-less fixture.

## Artist identity repair and player controls

Migration 14 repairs existing Artist identities and records local import context.
An explicit provider Artist identity can unify its owners, provided their other
provider Artist identities do not conflict. Catalog imports now reuse Artist IDs
for every provider with an explicit Artist identity, including Spotify.

For local imports, a single Album Artist credit can share an identity when all
local sources agree on the same registered root and immediate Artist directory,
and that directory matches the trimmed, case-insensitive credit. The layout must
contain Artist/Album/file components. Matching singleton Release/Track credits
participate too; credited spelling, join phrases, positions and roles are preserved.
All participants are checked for conflicting IDs within each provider namespace
before merging. Equal names in different roots/directories, flat collections,
and ambiguous multi-Artist credits are not sufficient evidence.

The real-library audit copy consolidated Hella's local-only, MusicBrainz and Spotify
identities into one Artist with ten saved Albums. Bygones has both Albums under one
identity; Floral retains its four Albums. Previously, imports created fresh Artist
IDs per credit, matching only unified successfully resolved participants, and
catalog credit reuse was restricted to MusicBrainz.

Core seek requests preserve queue/current-source identity and await engine
confirmation before accepting further position observations. GStreamer reports
SeekDone; Spotify uses its existing seek endpoint. The common slider sends one
request on release, ignores a drag when the Track changes, and previews progress.
Spotify volume targets the selected device, checks capability, and acknowledges
requests by revision. Older polls cannot overwrite the pending slider value.
After a successful command the adapter tolerates stale device volume for up to
eight seconds, then accepts external device changes again. Errors are shown in the existing error area. The authoritative-volume follow-up
below defines how requested master volume is retained on failure.

Follow-up validation: core tests, QML tests, focused GStreamer and Spotify playback
tests, strict Clippy for all four crates, formatting and diff whitespace checks
passed. The desktop audit on the migrated real-library copy exercised actual
local audio seek/volume and the existing browsing/queue/drawer interactions.
Controlled Spotify HTTP tests cover selected-device volume, stale polling,
capability errors and rejected commands; QML control tests cover remote seek,
volume acknowledgements and local/remote handoff. Live Spotify device seek/volume
and a human cursor-visibility check remain unverified. Qt test teardown still
prints an existing timer-thread warning; the normal desktop backend also reports
Mesa/EGL fallback warnings. No rendering environment changes were added.

## Authoritative volume and source calibration

The application owns one logical session volume. The player slider always shows
that value, including during a backend switch. Starting Spotify sends its calibrated
volume before Play; local starts apply the current master and local trim. Rejected
remote volume requests report an error but retain the requested logical master, so
a later handoff cannot resurrect the provider's old value. Existing pending request
and stale-poll protections remain. Genuine changed-volume observations on the same
active Spotify device can update the master, after reversing the trim and clamping
to the logical range; handoff and acknowledgement snapshots cannot do so.

Settings → Output calibration stores Local and Spotify trims in migration 15's
singleton SQLite settings row. Both default to 0.0 dB and accept −24.0 to 0.0 dB.
They persist per library database; master volume remains session state.
Effective output = master × 10^(trim dB / 20). GStreamer receives that bounded
linear gain. Spotify receives its nearest integer percentage, clamped to 0–100.
The range provides attenuation only. Unsupported Spotify volume devices or failed
volume requests block the automatic remote start and report the existing error.

Calibration does not normalize Tracks, change relative levels between recordings,
alter stored audio, or analyze loudness. No ReplayGain, LUFS, per-Track gain,
Spotify normalization setting, or audio DSP was added.

Validation for this slice includes complete 451-Track programs against concatenated
browse pages in every scope, duplicate titles, actual QML page-two Play at global
index 203, a bounded queue drawer showing that occurrence, all primary Play actions,
and additive actions. Controlled mixed-source tests cover 80% → 70% handoffs with
zero and −6 dB trims, pending acknowledgements and stale polls. A real-library desktop
audit exercised Album Track 3, Artist Track, global Track Play, and Local trim with
actual GStreamer audio. Live Spotify device handoff remains unverified in this slice.

Library membership removal and durable local-source exclusions are described in
[Library removal](library-removal.md).

The shared local admission pipeline, native From file workflow and durable multiple
library locations are described in [Local ingestion](local-ingestion.md).
