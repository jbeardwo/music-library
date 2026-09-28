# First library/player interface

The QML diagnostic shell now serves as a usable desktop library/player. It remains
an implementation prototype, not the public skin format or a final frontend decision.

## Interaction

- Artists and Albums are derived exclusively from saved Tracks. No source is required.
- Initially all three panes are populated; Artists, Albums and Songs are alphabetical.
- Artist selection preserves the Artist list and filters Albums and Songs. A Track
  belongs to an Artist through a Track, Release or Album credit; overlapping credits
  contribute it only once. Different Artist IDs remain different rows, even when their
  names match. Identity reconciliation is outside this slice.
- Album selection leaves Artists and the current Album list intact. Songs show every
  saved Track in that Album, grouped by Release and ordered by disc/Track position.
  Unknown disc defaults to disc 1 for ordering; unknown Track positions sort last.
  No external program supplies missing Tracks.
- Song selection only highlights a row. Artist/Album “Show all” controls and Escape
  clear filters. Clearing Artist also clears Album; clearing Album retains Artist.
- Up/Down selects in the focused pane; Enter plays; Tab traverses controls.
- Double-click and context-menu Play now replace the queue and start its first Track.
  Artist queues use Album title, Release, disc and Track ordering. Album queues use
  Release, disc and Track ordering. Song queues contain one Track.
- Add to queue appends the same complete saved Track set, preserving duplicates and
  existing playback. Queue construction never contacts a catalog. Playback uses the
  existing resolver, including its existing explicit-Play enrichment policy.
- The persistent player shows title, Artist and Album, Previous/Play/Pause/Next,
  elapsed/duration text and local volume. Missing Track artist text falls back to
  Album credits for presentation only; effective metadata and credits are unchanged.
  Remote volume stays with the playback device. Seeking has no common existing
  engine boundary, so this slice does not add it.
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
background Artist/Album queue preparation. It runs no migrations or network work.
Only one preparation is accepted at a time. A visible “Preparing queue…” state keeps
browsing and playback controls usable; the existing queue remains until replacement
can proceed. Remote replacement waits for the existing remote-stop acknowledgement.
The captured program is independent of later browsing selections.

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

Search is an explicit read-only “Search library — coming next” hook for future global
search independent of selections. Add Music opens the existing Add Album workflow;
configurations without catalog Add support explain that in a small dialog. Local
folder import still uses the existing launcher path. Provider setup and matching
remain available under the small settings menu, outside ordinary row actions.

Next UX work: global search, a unified Add Music/local-folder flow, smoother continuous
browsing across page boundaries, and queue editing. Duplicate Artist labels reflect
existing distinct identities; this UI does not merge them. Artwork, playlists, skins,
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
