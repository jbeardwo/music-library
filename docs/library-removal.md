# Library removal

Artist, Album, and Song context menus offer **Remove from library**, separated
from Play now and Add to queue. Every action opens a confirmation identifying the
object; Artist and Album confirmations include the currently saved Track count.
Cancel receives keyboard focus. With any local source (including unavailable
sources), **Do not automatically rescan** is shown and checked for each new dialog,
alongside “Your music files will not be deleted.” Otherwise both are omitted.

Removal deletes only Track membership, preserving entities, sparse edits,
provider identities, provenance, source associations, and availability records.
Album removal affects saved Tracks across its existing Release occurrences;
unsaved catalog Tracks are untouched. Artist removal uses the same union of
canonical Track, Release, and Album artist credits as the Artist's Songs pane.
Shared Tracks credited to that Artist are removed as whole Track memberships,
so they also disappear from other Artists' library views. Equal display names do
not imply canonical identity. No Artist or Album record is deleted.

## Local exclusions and import

Migration 18 adds `local_source_suppression`, keyed by the existing opaque local
source ID with a primary-key index. Checked removal inserts all affected local
source IDs, and unchecked removal clears any older exclusions for those sources.
Membership removal and exclusion changes share one immediate transaction.
Directories and display metadata never define exclusions.

A source retains its identity through normal scans of the same registered root
and stored path, including disappearance/reappearance and changed tags or bytes.
The scanner still updates availability and observations; it never creates
membership. Suppressed unassociated sources are excluded from automatic discovery
candidate listing. The desktop's automatic admission phase uses the shared
`ingest_local(ConfiguredLocations)` pipeline to restore associated, available,
unsuppressed Tracks and import new candidates normally. Thus unchecked removal
can be restored during a later applicable scan/import without duplicating the
original Track. A bare `scan_local_root` remains membership-neutral discovery.

Moved or renamed files normally acquire a new source identity and are not covered
by the old exclusion. This slice does not infer moves from hashes or titles.
Overlapping locations reuse unique persisted path evidence through indexed
root-to-source observations; ambiguous historical duplicates are deferred.

Explicit **Add Music → From file** takes precedence for successfully admitted
files in the selected file list or folder. It uses the normal ingestion pipeline
and clears only those sources' exclusions. `readd_local_source(source_id)` clears
exclusion and restores the existing associated Track atomically;
`add_to_library(track_id)` clears exclusions for that Track's sources. Explicit
`import_release` clears exclusion for its validated unassociated input sources.
`clear_local_suppression(source_id)` clears exclusion without changing membership,
allowing a later automatic import. See [Local ingestion](local-ingestion.md).
A general ignored-files settings screen is deferred.

## Responsiveness and playback

Large removals use set-based SQL and a transaction-local indexed Track set, not
one transaction per Track. The desktop prepares the confirmation count and performs the write on worker connections,
then delivers one completion callback. It refreshes three bounded panes, clears
stale selection and local search results, and refreshes displayed Add Music
membership from the database. It does not scan the filesystem or call providers.
Exclusion lookup uses indexed source IDs with no extra file reads or network work.
The import restoration query is scoped to one root using indexed root-to-source observations.

Removal does not change queue order, position, labels, or resolved playback source.
Existing queue occurrences can continue using retained sources; future library
browsing and queue-building see only remaining membership. Source availability
and playback failures keep their existing behavior.

## Validation

`tests/library_removal.rs` covers local and sourceless removal, partial/full Albums,
canonical/shared Artist credits, duplicate names, membership and exclusion across
restart, unchecked scan/import restoration, sibling isolation, provider identity
retention, local search filtering, explicit re-add/clear, file preservation,
indexed lookup, and atomic large removal with an injected failure.

The QML `library_removal_confirmation_and_queue_snapshot` test drives the shipped
right-click menus and dialogs, Cancel/default exclusion, pane refresh, and queue
preservation. Run with `QT_QPA_PLATFORM=offscreen` and `--test-threads=1`.
It accepts `MUSIC_LIBRARY_REMOVAL_AUDIT_COPY` for a **disposable database copy**;
it intentionally removes membership in that copy. Use SQLite backup to include
WAL contents. Never point the audit at the user's working database.

On this machine, the deterministic 200k fixture measured approximately 385 ms
for a removal preview and 1.52 s for transactional removal (debug build).
Both operations run away from the desktop thread. These are measurements, not
universal latency budgets.

Validation completed with the full core suite (283 passing tests), the 10-test
desktop suite including local import, focused removal tests,
and the opt-in 200k measurement. The offscreen removal audit also passed against a
SQLite backup of the existing 499-Track real-library audit database, using actual
menu clicks, confirmation buttons, and Enter on Cancel. Filesystem scan/import
checks used disposable paths and files. No human desktop visual audit was performed.
