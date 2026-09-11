# Disposable Qt Quick diagnostic

Ambiguous Artist matching now attempts one Album-corroboration search before showing
the Artist picker. Only a uniquely supported Artist/Album pair is accepted. Remaining
candidates appear with Album-supported candidates first; ordering is not a selection
or confidence guarantee. Local display metadata remains unchanged.
The local matching list retains delegates by Album ID and updates changed rows in
place, preserving scroll/focus and first-seen display order while the matching queue
continues independently. Literal titles such as `Floral EP` and `Floral LP` win over
type-checked suffix fallback; `Hugs EP` can fall back to `Hugs` of type EP.

Successful matches retain the selected provider title and Artist name for this
session. Rows always show both `Local: title — artist` and `Matched: title — artist`
(or `Matched (close): …`), including when the values are identical.
Canonical provider Artist names make alias resolutions visible. No extra requests
or durable metadata updates are made; Already matched after restart still uses
the existing identity-only status because provider presentation is not persisted.

Read-only live checks (each uses the same rate-limited client, without importing):

```sh
cargo run --manifest-path adapters/musicbrainz/Cargo.toml --example artist_match_probe -- Floral 'Floral EP'
cargo run --manifest-path adapters/musicbrainz/Cargo.toml --example artist_match_probe -- Floral 'Floral LP'
cargo run --manifest-path adapters/musicbrainz/Cargo.toml --example artist_match_probe -- Hella "Bitches Ain't Shit but Good People"
cargo run --manifest-path adapters/musicbrainz/Cargo.toml --example artist_match_probe -- Tabar 'Hugs EP'
```

On 2026-09-10 all four resolved through one combined corroboration request after
Artist search: Floral's two literal titles and Hella matched exactly; Tabar used
the EP fallback. A truncated Artist search no longer suppresses corroboration of
its returned plausible candidates. This remains bounded evidence, not an exhaustive
catalog scan; truncated Album results still require manual resolution.

This is integration tooling, not the product UI, skin format, or a final frontend
choice. The default fake engine produces no audio. An optional GstPlay mode plays
local files; see [real-audio setup and manual checks](../../adapters/gstreamer/README.md).
Delete this directory to remove the experiment; the backend has no Qt dependency.

## Build and launch

Requires Rust, a C++17 compiler, and Qt 6.5+ with development headers, qmake,
Qt Quick, Quick Controls, Layouts, Window, Templates, and WorkerScript modules.
Validated with Qt 6.10.2 on Linux. On Debian/Ubuntu:

```sh
sudo apt-get install qt6-base-dev qt6-declarative-dev \
  qml6-module-qtquick qml6-module-qtquick-controls qml6-module-qtquick-layouts \
  qml6-module-qtquick-window qml6-module-qtquick-templates qml6-module-qtqml-workerscript
cargo run --manifest-path tools/qml-diagnostic/Cargo.toml
```

Run from the repository root. QML is embedded at compile time, so rebuild after
editing it. The package has its own lockfile and build directory; ordinary backend
builds do not need Qt. `QMAKE=/path/to/qmake6` selects a non-default Qt installation
([binding build configuration](https://github.com/woboq/qmetaobject-rs/blob/master/qttypes/build.rs)).

Every default/fake launch creates a disposable 45-Track database and deletes it on exit.
It includes named source-less, unavailable, available, and multiple-source cases,
plus duplicate titles spanning search pages. Availability observations and paths
are synthetic: no local collection, actual files, or network access is required.
The fixture alone inserts synthetic observations with SQL; runtime commands use
`Library` and `Playback` and do not edit durable data.

## Exercise the boundary

* Search, then navigate 20-row pages. The adapter requests 21 rows for lookahead
  and uses the last displayed title/ID as the next cursor. A new search resets
  cursor history; previous page reissues its saved cursor. Only one page is kept.
* “Add to queue” appends one result without disturbing playback. Duplicate IDs
  are separate entries. “Queue this page (stops)” replaces the queue without
  starting it. “Replace queue & play” replaces it with one Track and attempts
  playback. Replacement failure
  retains the old queue; successful replacement followed by source-resolution
  failure leaves the newly selected Track stopped.
* Try Play/resume, Pause, Stop, Next, and Previous. All entries remain in the
  scrollable queue, including played entries. The marker means current selection,
  including a failed attempt; it does not assert that audio is playing. Navigation
  buttons depend only on whether a neighbor exists, never on playback status.
* An unplayable neighbor becomes current, stops prior playback, and reports an
  error. Another Next/Previous attempts the next neighbor; no automatic skipping.
  Source-resolution failure leaves `Stopped` unless stopping the old input fails.
* “Clear queue” stops, empties the queue, and clears current position/source. If
  Stop fails, the queue and labels stay intact with a visible error; retry Clear.
* Play the source-less and unavailable examples. Both stay visible in the library
  and report the backend's same `NoAvailableSource` error. The fixture titles
  identify the cases; the adapter does not invent a distinction the API lacks.
* Arm “Fail next engine call,” then Pause or Stop while playing. Observe `Failed`,
  the error, and the engine-call log. Retry Play: the log shows successful Stop
  before Start. Next/Previous also recover internally without a separate Stop.
  Arm another failure while Failed to test recovery Stop failing. Only engine
  calls consume an armed failure; an unplayable selection may still stop old input.

Each command shows its resulting state and an update counter; errors remain until
the next command. The last 12 engine calls show intermediate recovery operations
without pretending those calls are separate observable application states.

## Integration choice and findings

One Rust QObject exposes methods and a read-only `QVariantMap` snapshot with a
NOTIFY signal; search pages and the complete queue use `QVariantList` snapshots. `qmetaobject` provides
this directly without handwritten C++ or a project build script
([binding example and pinning rules](https://docs.rs/qmetaobject/0.2.10/qmetaobject/)).
[CXX-Qt](https://kdab.github.io/cxx-qt/book/) offers a generated C++/Rust bridge and
threading support; the small bridge still uses qmetaobject
queued callbacks without adding another bridge/build setup. This choice is local to the experiment.

Rust owns `Library`, `Playback`, and the pinned QObject. After every command the
QObject notifies QML, which re-reads one snapshot; QML never optimistically changes
playback state. The object outlives the QML engine. NOTIFY signals are required for
QML bindings to react to property changes
([Qt property integration](https://doc.qt.io/qt-6/qtqml-cppintegration-exposecppattributes.html)).
Real audio events use `queued_callback` with a weak QObject pointer to reach the
Qt owning thread. The callback factory first constructs the C++ QObject, as
required by [QPointer](https://docs.rs/qmetaobject/0.2.10/qmetaobject/struct.QPointer.html).
Capturing it earlier produced a permanently null pointer: GstPlay reached Playing,
but every notification was dropped and the UI stayed Stopped with Play pending.
Both commands and accepted events publish state. Routine
position/state events preserve visible errors; stale events do not publish.
There is no GUI timer/polling, generic event bus, or second playback-state model.
The audio worker itself checks its bus at bounded intervals.

The deliberately small context-object bridge is dynamic and limits static QML
tooling ([Qt context-property limitations](https://doc.qt.io/qt-6/qtqml-cppintegration-contextproperties.html)).
One lint suppression covers its injection; an executable QML smoke test checks
method calls, property notifications, and list bindings. A typed QML module and
incremental list model should be reconsidered if this grows beyond tooling.

Friction exposed so far:

* No Track-ID metadata lookup: queue labels retain the search-row metadata captured
  on append/replacement. IDs and position always come from `PlaybackState`. Labels could
  become stale with future metadata edits; add a bounded backend lookup when needed.
* The original backend never consumed played entries, and the adapter did not
  filter them out. The old “Play only this” action replaced the entire queue;
  its label now makes that explicit, alongside the new append action.
* The original position advanced only after successful playback. This trapped
  navigation at an unplayable neighbor. Position now records the attempted entry,
  independently of playback success. Reordering/select-position remain deferred.
* Full snapshot replacement can reset list scrolling. This is still a disposable
  diagnostic model; an incremental model remains a future integration concern.
* Source absence and unavailability share one error. User-facing distinctions would
  need structured backend results; do not parse error strings to infer them.
* Failed engine output is unknown. QML shows Failed, and recovery remains entirely
  inside `Playback`; no UI-specific recovery state was added.
* Search/source resolution still run synchronously on the GUI thread; this does not
  validate responsiveness at 200,000 Tracks. GStreamer work runs on an owned worker
  and event delivery is queued. Folder scanning/import happens before window creation.
* Real-mode state may show a pending target until confirmed (for example,
  Playing → Paused pending). Queue edits commit when the engine accepts the stop,
  not after its completion. Later asynchronous failure remains visible without
  silently restoring an earlier queue.
* Pagination has no snapshot isolation across external edits. Reset search to
  refresh; this fixed fixture cannot assess concurrent library changes.

The application-facing additions are `enqueue`, `clear_queue`, movement queries,
and the attempted-entry position semantics. No schema, search strategy, or
source-selection rule changed.
Real mode adds generation-tagged engine events, pending/confirmed state, media
position/duration, and EOS advancement. Seeking remains outside this probe.

The experiment modestly increases confidence in QML for command/state integration:
controls bind to Rust-owned state without polling. It provides no evidence yet for
skinning, large-library responsiveness, or packaging across platforms. Audible
WSLg output still requires the manual checks linked above.

## Checks

In addition to the repository's usual format/test/strict-Clippy/diff checks
(the all-feature checks require the GStreamer development packages):

```sh
cargo fmt --manifest-path tools/qml-diagnostic/Cargo.toml --all -- --check
cargo test --manifest-path tools/qml-diagnostic/Cargo.toml
QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software \
  cargo test --manifest-path tools/qml-diagnostic/Cargo.toml --all-features
cargo clippy --manifest-path tools/qml-diagnostic/Cargo.toml --all-targets --all-features -- -D warnings
cargo build --manifest-path tools/qml-diagnostic/Cargo.toml
cargo build --manifest-path tools/qml-diagnostic/Cargo.toml --features gstreamer
/usr/lib/qt6/bin/qmllint tools/qml-diagnostic/Main.qml
QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software \
  tools/qml-diagnostic/target/debug/qml-diagnostic --smoke-test
```

Adjust the Qt tool path for your installation. The smoke test loads the actual
window and exercises the Rust/QML boundary without audio or a desktop display.
Rust adapter tests cover complete duplicate-title pagination, cursor reset,
source errors, stale selections, retained/duplicate entries, append/clear, queue-label
preservation, and failure recovery. The QML smoke test also checks full queue
snapshots, current markers, and navigation-button bounds in failed states.

The all-feature Qt regression test constructs the callback before QML exposure
and sends controlled events from a worker. It requires Qt's offscreen platform.
The action/result label retains only the last action name and derives confirmed
state and the pending target from Playback on every snapshot read. Previously it
cached state at command return: QML's Current label correctly changed to Paused,
but the bottom label still said “pause → Playing” until another command.
Accepted engine events apply state, increment revision, and emit `changed()`;
QML then rereads the snapshot. No polling or extra notification is needed.
The regression loads the actual QML window and confirms Playing, Paused, resumed
Playing, Stopped, and Failed from queued test events without a subsequent command
or position update. It checks both visible labels, pending targets, and stale
event rejection; no audio hardware is used.


Volume is a basic 0–100% slider, enabled in all transport states. It calls
`Playback::set_volume` through the diagnostic adapter and shows the accepted
session setting. Invalid/non-finite values are rejected without changing it.
Volume defaults to 100%, survives Stop/queue changes, and is never persisted.
The direct linear gain mapping is intentional for this probe; a perceptual
slider curve remains deferred (see the GStreamer guide).


## Catalog diagnostic

Launch the fake-mode sample library (no collection required):

```sh
cargo run --manifest-path tools/qml-diagnostic/Cargo.toml
```

1. Open **Catalog…**, enter an Album/artist query and click **Search Albums**.
   Friendly results show title, artist, original date and type. Search makes one
   request for up to ten results and does not fetch editions. **Next Album page**
   retrieves further matches.
2. Click **Add Album** on a result. The worker loads a bounded Official candidate
   page with media summaries (without labels or release credits),
   provisionally selects a representative Release and fetches its complete Tracks.
   Expect pending feedback, then success and unavailable Tracks in local search.
   No playback begins. This normally uses two additional requests; no Official
   candidates triggers one extra unfiltered media-only browse.
3. Use **Editions…** only when edition choice matters. Country, format, label,
   barcode, date and comments appear here. Choose an edition and **Add this edition**.
   Further pages remain explicit. Editions of the same catalog Album share its
   application Album but retain separate Tracks.
4. Re-add an Album/edition: entities are reused and membership restored. Recent
   compatible candidate/edition pages and details are reused in memory. Opening
   Editions after Add still fetches rich display metadata. Errors stay visible and no partially
   imported Album is left behind. Try queueing an imported Track to exercise the
   existing no-source playback error.

This also works in real-engine mode; catalog import does not match scanned files.
The diagnostic database remains temporary and disappears on exit.

One owned worker performs explicit network requests and rate waits off the Qt
thread. Pending controls and an adapter guard reject duplicate submissions. Queued
callbacks apply results before property notifications. Catalog properties remain
separate from playback so timing updates cannot reset edition selection. There is
no polling or sample-mode startup network activity. Real folder import can dispatch
post-import matching as described below. Catalog import is a short atomic SQLite transaction
on the application thread. Closing during HTTP waits for the bounded request before
joining the worker; cancellation is deferred.

The all-feature Qt test uses a gated fake provider to verify responsiveness while
pending, Add Album, advanced selection, cache reuse, source-less results, unchanged
playback, re-add idempotency and visible HTTP errors. See the
[MusicBrainz guide](../../adapters/musicbrainz/README.md) for HTTP fixtures and a
read-only live probe.

For opt-in Add Album phase logging and the ignored live Qt timing probe, see the
[catalog latency audit](../../docs/catalog-latency-audit.md). Normal launches emit
no timing logs and perform no extra requests.

## Post-import local Album matching

Build and launch with an actual local Album folder (WSLg audio must already work):

```sh
cargo run --manifest-path tools/qml-diagnostic/Cargo.toml --features gstreamer -- \
  --gstreamer '/mnt/f/music/Gorillaz/Demon Days'
```

Replace that example with an existing directory; `/mnt/f/music` itself also works,
but scanning the whole collection still occurs before window creation. The tool
now groups files by directory, normalized tagged Album title and artist evidence.
It does not infer editions across disc subdirectories. The database is disposable;
no source files are changed. Import commits before any matching request is sent.

1. Search/queue/play local Tracks as soon as the window appears. Open **Local Album
   matches…** to see each imported Album's pending, matched (with MBID), skipped,
   ambiguous/no-match or error state. Playback and search remain usable while HTTP
   and rate/retry waits run on the worker.
2. For a partial/one-Track test, copy one or several well-tagged files into a small
   scratch folder and launch against that folder. No complete tracklist is required.
3. Launch against weakly tagged files: missing/placeholder Album, artist or Track
   title is skipped without HTTP. Filename fallback is not matching evidence.
4. **Retry Match** repeats a failed/no-match attempt; already matched Albums require
   no request. The button is disabled while that Album is pending. During a provider
   outage, it preserves work without starting HTTP; use **Retry Matching** to probe.
5. To test manual-only operation, add `--no-auto-match` to the command. Local import
   is identical, and Retry Match remains available. For a network-failure test,
   launch a fresh instance with `HTTPS_PROXY=http://127.0.0.1:9` before `cargo run`.
   Local playback must still work while matching reports a paused provider circuit. Remove that environment variable and relaunch for real-service testing.
   Deterministic tests additionally cover failure followed by successful retry in
   the same session, without using the public service.

Artist identity is established first, then Album search is restricted by its MBID.
Local Release/Track identity and friendly metadata remain unchanged. The first
unknown Artist normally requires two logical requests; a stored Artist MBID needs
only the scoped Album request. Scores never resolve ambiguity. Artist names require exact primary/alias evidence (with competing-Artist veto);
Album titles allow controlled trailing packaging variants, then one edit for titles
of at least five characters, only within the established Artist. Explicit Use Artist
additionally permits two edits for titles of at least eight characters in this session. Incomplete pages remain ambiguous.
An Artist identity survives later Album errors. Retry does not repeat that Artist
search. Existing 503 retries may add HTTP attempts. Closing cancels queued work but
may wait for the in-flight bounded request/retry operation.

### Hella / Acoustic diagnostic

Launch against a local folder tagged Artist `Hella`, Album `Acoustic` using the
command above, then open **Local Album matches…**. Live MusicBrainz testing found
several exact-name Hella artists, so the correct automatic result is
**Artist ambiguous**, not a guess based on the top score. Select **Hella — American
band — US — Group** in the candidate dropdown and click **Use Artist**. This stores
`1ecbc7e5-6e33-4062-997d-abc550dd63e6`, then retries only Album discovery. The live
probe found unique `Acoustics` (`546b457a-b7f7-3591-a88b-7ed7916f74b6`), reported as
**Matched close title**. The library still displays the original `Acoustic` tags.
Use correctly tagged `Acoustics` files as an exact-title control. No manual Album
selection, tag editing or edition identity is added.

The opt-in read-only probe is independent of local files and automated tests. A
close match also triggers an exact-title control request through the same rate gate:

```sh
cargo run --manifest-path adapters/musicbrainz/Cargo.toml --example artist_match_probe -- Hella Acoustic
# Explicitly identify the American band after inspecting the Artist ambiguity:
cargo run --manifest-path adapters/musicbrainz/Cargo.toml --example artist_match_probe --   Hella Acoustic 1ecbc7e5-6e33-4062-997d-abc550dd63e6
```

Artists are currently created per credit. Stored MBIDs save requests for the same
Artist ID; independently resolved name searches are also reused in this session.
Different newly created Artist IDs may need discovery again in a later session.
Multi-artist/complex credits are left unresolved; **Use Artist** handles search
ambiguity for one Artist credit, not restructuring credits or replacing conflicting
stored identities.

### Provider outage testing

A final HTTP 503, configured request/read timeout, or connection/DNS/I/O failure
shows **MusicBrainz unavailable — matching paused; retrying automatically**, the preserved work
count, and **Retry Matching**. Untouched Albums remain pending; the failed Album
is deferred. New imports are retained without network requests. Retry Matching
probes the failed Album first; repeated clicks cannot launch parallel probes.
Recovery clears the banner and resumes the queue, even if the probe yields an
ambiguous/no-match result. Another outage arms the next automatic cooldown; manual retry is also available. Use Artist still saves the selection locally and queues the continuation.
These states are ephemeral; closing the diagnostic discards pending session work.
The deterministic Qt test exercises outage notification and recovery without live
MusicBrainz. Explicit catalog search/Add Album remains a separate user action.

Automatic recovery probes after 15/30/60/120 seconds, capped at 120. Positive
Retry-After seconds/dates can extend the normal delay up to 120 seconds; zero or
invalid headers cannot trigger immediate retries. **Retry Matching** can probe
sooner, safely invalidating the pending automatic wake. Recovery resets backoff
and clears the banner without a user action. QML has no cooldown timer or polling.

Deterministic outage/recovery simulation (no live service or 15-second test sleep):

```sh
QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software \
  cargo test --manifest-path tools/qml-diagnostic/Cargo.toml --all-features \
  async_confirmations_refresh_actual_qml_labels_without_another_command -- --nocapture
```

The existing diagnostic test imports a local fixture, selects an ambiguous Artist,
then its gated fake provider fails with 503. It verifies local playback remains
usable and the pause banner appears, delivers a fake cooldown expiry through the
actual queued Qt callback, and verifies automatic matching/recovery with no Retry
button press. Duplicate expiry delivery must not issue a second probe. Backend
fake-time tests separately verify the real delay sequence and worker cancellation.

### Alias and packaging comparison probes

These opt-in probes exercise EP suffix comparison and exact alias discovery:

```sh
cargo run --manifest-path adapters/musicbrainz/Cargo.toml --example artist_match_probe -- Tabar 'Hugs EP'
cargo run --manifest-path adapters/musicbrainz/Cargo.toml --example artist_match_probe -- 'The Speed of Sound in Seawater' 'Red Version'
```

The probe prints returned canonical names/aliases and the scoped Album decision; it never
writes the library. A third explicit Artist MBID uses the manual-confirmation
threshold. Its optional exact-title control adds a diagnostic request beyond the
normal two-search flow. The latest Tabar probe exhausted three HTTP 503 attempts
with Retry-After 0; deterministic tests cover both cases without the live service.
