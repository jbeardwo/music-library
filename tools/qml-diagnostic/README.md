# Disposable Qt Quick diagnostic

This is integration tooling, not the product UI, skin format, or a final frontend
choice. It uses the existing backend with a fake engine. **There is no audio.**
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

Every launch creates a disposable 45-Track database and deletes it on exit.
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
threading support, but this small synchronous probe does not yet justify its
additional bridge/build setup. This choice is local to the experiment.

Rust owns `Library`, `Playback`, and the pinned QObject. After every command the
QObject notifies QML, which re-reads one snapshot; QML never optimistically changes
playback state. The object outlives the QML engine. NOTIFY signals are required for
QML bindings to react to property changes
([Qt property integration](https://doc.qt.io/qt-6/qtqml-cppintegration-exposecppattributes.html)).
There is no timer, polling, event bus, or second playback-state model.

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
* Commands/search run synchronously on the GUI thread. The tiny sample needs no
  polling, but does not validate responsiveness at 200,000 Tracks. Real engine or
  scanner events need notifications, GUI-thread delivery, and stale-event handling;
  blocking work must move off the GUI thread without moving state ownership to QML.
* Pagination has no snapshot isolation across external edits. Reset search to
  refresh; this fixed fixture cannot assess concurrent library changes.

The application-facing additions are `enqueue`, `clear_queue`, movement queries,
and the attempted-entry position semantics. No schema, search strategy, or
source-selection rule changed.
Before real audio, specify asynchronous completion/error acknowledgement,
stop/replacement cancellation, late-event rejection, shutdown, and automatic
end-of-input advancement. Seeking remains outside this probe.

The experiment modestly increases confidence in QML for command/state integration:
controls bind to Rust-owned state without polling. It provides no evidence yet for
skinning, large-library responsiveness, packaging across platforms, or real audio.

## Checks

In addition to the repository's usual format/test/strict-Clippy/diff checks:

```sh
cargo fmt --manifest-path tools/qml-diagnostic/Cargo.toml --all -- --check
cargo test --manifest-path tools/qml-diagnostic/Cargo.toml
cargo clippy --manifest-path tools/qml-diagnostic/Cargo.toml --all-targets --all-features -- -D warnings
cargo build --manifest-path tools/qml-diagnostic/Cargo.toml
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
