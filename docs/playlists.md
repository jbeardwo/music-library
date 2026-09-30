# Playlists

The Playlists view now shows persisted lists on the left and ordered entries on
the right. Create with +; right-click a playlist to rename/delete. Right-click a
Song to Add to Playlist using a paged chooser, with Create playlist available.
Entry context menus remove the selected occurrence or move it up/down. Double
click and Enter use the existing playback path; Play starts at the chosen entry,
and Add to queue appends just that entry. Playlist-level Play/append captures the
whole ordered list. No selection leaves Songs blank. Sort controls are omitted.
Selection and paging survive navigating away and back within the session.

Canonical Track identity, saved membership, playlist membership and transient
queue state are distinct. Entries refer directly to existing canonical Tracks,
including unsaved Tracks, and have independent UUIDs to preserve duplicates.
Adding/removing entries never changes saved membership. Library removal never
removes entries. Playback snapshots remain independent after playlist edits or
deletion. There is no provider-specific metadata duplicated for playlists.

Migration 21 adds playlist timestamps, names and ordered entry records. The
playlist/name index serves listing, playlist/position serves cursor paging and
adjacent moves, and Track/playlist serves membership lookup. Integer positions
persist; removal can leave gaps, and adjacent swaps preserve order atomically.
Both panes and the chooser expand at most 200 rows into QML. Complete queue reads
run off the UI thread using the same application queue reader as library playback.

Validation includes real SQLite lifecycle, restart, duplicate, membership
independence, keyset paging and snapshot tests, plus the actual QML view test for
rename/delete, duplicate order, navigation retention and exact duplicate playback.
Old migration fixtures remove playlist tables when simulating earlier schemas.

`cargo run --offline --release --example playlist_performance -- DISPOSABLE.sqlite`
seeds one entry per Track in a disposable deterministic large-library copy,
measures bounded first/deep pages and full queue reads, checks the order index,
then deletes its test playlist. On the existing 200k-Track fixture, first plus
190,000-position pages measured 1.06–1.73ms; a complete 200k-entry snapshot measured
585ms. The plan used `playlist_entry_order (playlist_id=? AND position>?)`.
These are local measurements, not cross-machine budgets.

Playlist search, provider import/export, cross-service migration, synchronization,
collaboration, folders, smart playlists and persistent UI navigation remain deferred.

Final checks passed: core suite and focused playlist/migration regressions,
all-feature offscreen QML suite and final playlist view rerun, strict Clippy for
core and QML crates, formatting and whitespace checks. Existing 200k QML
resize/scroll audit passed; existing Songs/Genre diagnostics remained in their
prior measured range (about 5ms global Songs, 43–44ms Genre Songs, 56–60ms Genre
Album order for combined page/page/seek). Qt checks used the offscreen software
renderer, not a native desktop or Windows runtime audit. Provider live-service
checks remain opt-in.


Multi-selection and shared container actions now extend this slice. All library
container menus expose Add to Playlist, with one canonical-ID duplicate dialog for
the whole batch. Multiple selected source playlists preserve persisted occurrences.
See [selection and context actions](selection-actions.md) for Yes/No, ordering,
selection and paging semantics. The persisted playlist schema is unchanged.
