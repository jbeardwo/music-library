# Local import and library locations

Add Music offers **From catalog** (the existing catalog workflow) and **From file**.
From file offers native **Choose files** and **Choose folder** dialogs. Both use
`Qt.labs.platform`, the same explicit parent window and window modality. On systems
without a native platform helper both use Qt Widgets dialogs, avoiding the former
file-widget versus folder-Quick-popup discrepancy. Cancel explicitly closes the
helper and clears selection; fresh opening clears prior accepted/current selection.
Files retain standard OpenFiles Ctrl/Shift extended selection. Files and folders
remain different standard dialog modes, so OS-specific control differences are
expected; there is no custom browser. File selection
supports multiple files; folder selection registers that exact directory as a
persistent library location and scans only that location. Selecting an existing
location rescans it without duplicating it. Selecting individual files never
registers their parent directory. Nothing starts playback or modifies source files.

## One admission pipeline

`Library::ingest_local` accepts `Request::Files`, `Folder`, `Rescan`, or
`ConfiguredLocations`. Automatic desktop startup and explicit import use this
same pipeline: bounded discovery, the shared filesystem `observe` operation,
normal source observation persistence, normal candidate grouping and
`import_release` reconciliation, then Track membership. Unchanged sources reuse
size/mtime observations instead of rereading tags. The lower-level
`scan_local_root` API remains discovery-only: it does not admit library members.

The backend returns imported Releases. The desktop feeds them into its existing
`post_import` matching/enrichment hook in bounded chunks after local persistence.
Provider outages cannot prevent local admission, and no provider requests occur
inside ingestion transactions. Artwork uses the existing resolver and priority
rules, including embedded and sidecar evidence; there is no manual-import variant.

## Artist reconciliation and Artist-folder roots

The Hella regression was a scope-of-evidence issue, not a separate manual parser
or missing provider request. Ordinary scanning of `Music/Hella/Album/file` had
`Music` as its root; explicitly selecting Hella registered `Music/Hella` instead.
Both called `import_release` and `artist_identity::reconcile_album`, but the latter
required three relative path components and deliberately excluded the root name.
The explicit scope supplied only `Album/file`, so context registration returned
without consolidating each Album's independently created Artist.

The shared Artist reconciler now recognizes the same physical Artist/Album
layout whether the location is its parent or the Artist directory itself. A flat
folder is still insufficient; singleton Album/Release/Track credit evidence and
provider identity compatibility remain required. Overlapping roots use the same
canonical Artist-directory evidence; unrelated same-name directories do not merge.
Display credits, opaque entity IDs and source provenance remain intact.

After local candidate imports, all affected associated Albums—including unchanged
previously imported files—pass through `reconcile_albums` in one finalization
transaction before returning scheduling input. This is the same register-evidence,
then-consolidate operation used by migration backfill, scoped by the ingestion's
indexed source set. Both automatic and explicit requests execute it. It can repair
pre-fix split identities without reparsing files, creating Tracks, or contacting
providers. No manual-only identity rule or whole-library backfill was added.

## Durable roots and source identity

Migration 19 permits a local observation without a root and adds indexed
`local_root_source` scan observations. Existing roots and sources are preserved.
A directly selected file outside configured roots is detached; later selecting
its enclosing folder adopts the same source and Track. Overlapping locations
reuse a unique existing source at the same canonical path. Ambiguous historical
path duplicates are deferred rather than silently merged. Root-specific scan
observations allow availability reconciliation after a successful relevant scan.
Interrupted traversal does not mark unvisited sources unavailable.

Canonical native paths are lookup evidence, not Track identity. Files still use
opaque persisted source identities; Tracks and Releases retain their domain IDs.
Moved/renamed files are not automatically identified as the old suppressed source.
No hash-based move tracking was introduced. Paths use native encoding and
`Path`/`PathBuf`; QML file URLs are converted with `Url::to_file_path`.

`local_locations`, `register_local_root`, `remove_local_location`, and
`Request::Rescan` provide future location-management hooks. Removing a location
retains source records, Track associations, memberships, and suppressions.
Removing music does not remove its location. Locations persist in the library
SQLite database: use the prototype's persistent `--library` mode for restart
persistence; its existing demo/temporary modes remain disposable.

## Suppression and explicit intent

Automatic admission skips suppressed source identities before metadata parsing
or membership restoration. Presence can still update availability. Explicit file
or folder admission overrides suppression only for successfully admitted sources
encountered in that request. Other exclusions remain intact, including files
outside the selected folder and selected files whose metadata cannot be read.
Existing associated Tracks are restored without duplication. New candidates use
the normal import operation, which clears their suppression transactionally.
The lower-level clear/re-add APIs remain available; see
[Library removal](library-removal.md).

## Work, errors and scale

Traversal, tag reads and ingestion run on a worker connection. The UI coalesces
progress updates and reports imported Tracks, unsupported files, unreadable files
and incomplete locations. A malformed file does not abort the remaining files.
Successful admissions survive later errors. No raw internal errors are shown.
Browser panes, active local search and catalog membership indicators refresh
on completion; playback and its queue snapshot remain unchanged.

Path/suppression lookups are indexed and batched at 128 paths. Candidate work is
scoped to admitted sources, and database writes are batched; filesystem and network
work stay outside write transactions. Choosing one location does not scan others.
In a debug-build fixture with 200k unrelated source records and 100k suppressions,
a one-file folder admission measured approximately 6–111 ms, a cached single-root
rescan 104 ms, and direct file admission 4 ms. These are measurements, not budgets;
the fixture is a source-lookup stress case, not 200k fully populated saved Tracks.

## Validation and deferred work

`tests/local_ingestion.rs` covers direct/multiple/folder admission, independent and
overlapping locations, restart and migration preservation/rollback, scoped
suppression override, failed files, metadata/provenance parity, unchanged caching,
normal matching during provider outage, embedded/sidecar artwork parity, local
search, source-file preservation and indexed/bounded lookup. Its opt-in 200k test
is `targeted_import_with_200k_unrelated_sources`.

The offscreen Qt test `local_file_chooser_native_dialogs_suppression_and_folder_import`
drives Add Music, the unchanged catalog entry, platform file-picker opening,
single/multi-file acceptance handling, native folder acceptance,
suppression/removal/rescan/re-add, immediate browsing, queue preservation
and a second configured location. It uses copied real FLAC containers with test
tags/art and disposable SQLite/filesystem paths, then reopens the database.
Backend tests cover multiple-file ingestion; the QML test supplies native acceptance
results through the shipped handler. Additionally, run:

```sh
QT_QPA_PLATFORM=offscreen python3 tools/qml-diagnostic/tests/native_picker_lifecycle.py
```

This compiles a QtTest harness against the actual shipped picker declarations.
It clicks visible QFileDialog Cancel/Open buttons, sends Escape, closes actual
windows, checks actual QWidget visibility and transient ownership, verifies clean
reopening, and exercises real Ctrl-select/deselect and Shift-range clicks in the
standard Qt file view. All selected URLs must reach the import handler once.
It validates the Qt Widgets fallback; OS-specific native helpers and a human
visual desktop audit remain unverified, including Windows runtime behavior.

Artist tests compare six Albums each for Hella, Bygones/bygones and Floral under
parent-root scans versus explicit Artist-folder selection, including storage-level
Album/Release/Track credit partitions. A cached-reimport regression recreates six
old Hella identities and verifies consolidation with the trusted provider Artist
ID and original Track IDs preserved. The opt-in
`real_artist_folder_import_matches_configured_root` test accepts
`MUSIC_LIBRARY_ARTIST_IMPORT_AUDIT` pointing to **disposable copied** Artist folders.
It passed with real audio samples from nine Hella, two Bygones and four Floral
directories, including suppression/rescan/re-add. No original audio was modified.

Final verification includes the complete core and 10-test desktop suites, strict
Clippy for core and the all-features desktop, formatting and whitespace checks.
The opt-in source-lookup stress test also passed after scoped Artist finalization.

A full Library Locations/ignored-files settings screen, cancellation and filesystem
move inference are deferred. Existing matching and artwork policy are unchanged.

## Later local-source attachment

Before a candidate group creates any entities, the shared ingestion finalizer now
reconciles its unassociated sources against persisted canonical Tracks. Files,
folders, rescans and configured locations execute this same step. Previously the
importer could reuse an Album but always created a new Release and Track; adding
local files after catalog/Spotify import could therefore duplicate music.

Exact embedded MusicBrainz occurrence identity is checked first. The file adapter
validates typed Artist, edition and occurrence MBIDs as well as the existing Album
and Recording claims; this does not promote those raw fields to canonical
identities globally. Invalid or conflicting supported claims defer attachment.
An exact Recording identity still requires Album/edition context because a
Recording can occur on multiple release-specific Tracks. Independent identity
conflicts block attachment. No provider connection is required.

When exact occurrence identity is unavailable, the existing indexed Album
reconciliation supplies context: ordered credit agreement plus positioned title
support within one stored edition, without contradictory overlaps. The existing
`album_program::inspect_candidate` comparator then checks each plausible persisted
edition's Tracks. Attachment requires trusted Recording agreement or corroborated
title and explicit position, compatible contributor evidence and no identity
conflict. Multiple plausible canonical Tracks are unresolved; row order never
selects one. Existing metadata-only local editions retain conservative local/local
import behavior unless persisted provider identity supports attachment. Album agreement does not create or bind an edition identity. There
is no new fuzzy matcher, title/Artist-only merge rule, or live provider request.

Plans are evaluated before source writes so one arriving file cannot change the
evidence used for the next. The source association, suppression override,
effective metadata refresh and existing provenance reconciliation commit together.
Existing Artist, Album, Release, Recording and Track entities are retained; no
provider identity or user override is replaced. File observations retain their
source ownership and use existing effective-metadata precedence. Artwork rules
are unchanged. Two distinct paths can attach two distinct sources to one Track;
repeated canonical paths reuse the existing source.

Attachment itself never changes saved membership, including playlist-only Tracks.
Repeated automatic admission no longer restores membership on already-associated
Tracks; otherwise the scan after attachment would save a playlist-only Track.
The existing explicit re-add of an associated source still expresses save intent
and clears suppression. Automatic suppression remains effective; explicitly
selected unassociated suppressed sources can attach to existing Tracks normally.
New unmatched local music retains the existing separate-Track admission behavior.
Reports distinguish attached sources from unmatched candidates imported separately.

Matching narrows through indexed identities or the existing Album match key. Album
context and edition evidence are cached per candidate group; Track identities and
ordered credits use the existing batched evidence loader. More than 64 plausible
editions or more than 4096 Tracks in an edition defer attachment rather than
accepting a truncated set. Network and filesystem work never occur in the write
transaction. No matching runs at startup to restore an existing association.

Playback already resolves the current logical Track ID when starting an entry.
Its readable-local preference sees the new association without queue rewriting,
playlist-specific routing, or changes to source priority. Source disappearance,
availability/fallback lifecycle changes, historic staging cleanup and configurable
source preferences remain outside this slice.

Validation: `tests/local_source_attachment.rs` covers catalog and playlist-only
Tracks, exact offline identities, ambiguity/conflicts, all admission modes,
suppression, multiple copies, search and reopen. Its ignored real-audio test copies
all ten `test-media/Get Disowned` files into a temporary root after catalog Tracks
are established from the checked-in offline Spotify snapshot. All ten sources
attach without changing IDs or memberships. The ignored GStreamer test
`attached_catalog_source_plays_real_audio_and_survives_reopen` queues Tracks before
attachment, plays a copied MP3 through actual GStreamer audio output, checks queue
order, then reopens the library and checks persisted local preference. The fixture
has no embedded provider IDs, so these real-audio checks exercise Album/program
corroboration. The ignored directory and original audio remain unchanged.

The 200k unrelated-source stress case passed with 7–116 ms folder admissions,
104 ms cached rescan and 5 ms direct admission in this debug run. The regular core
suite passed 327 tests with one unrelated baseline failure excluded:
`catalog_playlist_identity_membership_reload_and_duplicates` expects an empty
subtitle despite existing Album-credit fallback. The same failure was reproduced
against the unchanged `HEAD` source. All eight focused attachment tests, including
the real album and rollback test, passed; GStreamer real output, adapter tests,
core Clippy and desktop compilation passed.
