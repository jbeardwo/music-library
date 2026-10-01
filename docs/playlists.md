# Playlists

The Playlists view shows persisted lists on the left, ordered Songs in the center,
and selected-playlist Details on the right. Create with +; right-click a playlist to rename/delete. Right-click a
Song to Add to Playlist using a paged chooser, with Create playlist available.
Entry context menus remove the selected occurrence or move it up/down. Double
click and Enter use the existing playback path; Play starts at the chosen entry,
and Add to queue appends just that entry. Playlist-level Play/append captures the
whole ordered list. No selection leaves Songs blank. Playlist Songs has a compact details view with clickable headers.
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
Main panes retain at most 600 rows in continuously scrolling windows; the chooser
retains its separate 200-row paging. See [continuous scrolling](continuous-scrolling.md). Complete queue reads
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

## Direct catalog additions

Selected-playlist Details exposes **Add Tracks from Catalog**, which opens Add
Music's bounded search workers, Artist/Album/Song navigation and edition/program
detail directly. Library content continues to use the existing Artist, Genre,
Album and Song **Add to Playlist** context actions and shared batch insertion.
Checkbox selection is local to the dialog until **Add selected tracks**. Search
and Back clear that selection; closing leaves Library navigation in place.

The data flow is catalog Release/program → canonical application Release and Track
UUIDs plus existing provider identities/credits/effective metadata → selected Track
IDs → shared `AppendPlan` → ordered playlist-entry UUIDs. `ensure_catalog_release`
uses the existing conservative edition identity reconciliation without inserting
Library membership. Known unselected program Tracks may also persist outside the
Library. No schema migration or metadata copy in playlist entries is needed:
migration 21 already references `track`, independently of `library_membership` and
sources. Removing an entry preserves canonical identity. Later explicit catalog
saving reuses the same edition/Track IDs and adds only requested membership.

Both routes share one aggregate canonical-ID duplicate confirmation: Yes inserts
the whole batch; No skips identities currently in the destination. Entry IDs remain
independent, preserving duplicates and order after reload. Playlist rows read
canonical effective metadata directly, with playlist entry ordinals for display;
missing credits/year/source availability remain valid. Playback and queueing use
the existing transient snapshot and local/provider resolution paths. Catalog
provider identities and optional Spotify enrichment are retained without saving.
An unavailable Track stays in the playlist. **Save to Library** on a single entry
is an explicit membership action and invalidates cached Library views.

Focused SQLite and offscreen tests cover direct multi-track addition, canonical
reuse, duplicate Yes/No, row rendering, queueing, reload/order and membership
independence. The opt-in `add_music::tests::live_catalog_only_playlist_qml` check
fetches a real MusicBrainz result on a disposable database. On this run, “DARE”
from Gorillaz's *Demon Days* rendered and survived reload with zero saved Tracks;
resolution correctly returned unavailable with remote playback unconfigured.

Validation for this addition (2026-09-30): core 298 passed; MusicBrainz 20,
Spotify 37 and GStreamer 6 passed (their opt-in/live tests remain separately
ignored). Formatting, whitespace and strict Clippy passed for core, QML and all
three adapters. Offscreen checks ran individually to avoid Qt's process-wide
thread-affinity failures. Catalog/Library insertion, missing-credit rendering,
duplicate confirmation, queueing, explicit Save and the updated Details-pane
navigation checks passed. The live catalog check also captured
`/tmp/catalog-only-playlist.png`; actual authenticated streaming was not exercised.

Database/query-plan, browsing, Library search/views, selection, playlist and
continuous-scrolling diagnostics ran on the deterministic 200k fixture and a
SQLite backup under `/tmp`. Forward/reverse playlist pages used
`playlist_entry_order`; full forward scrolling retained at most 600 rows and
covered all 200k identities. The offscreen 200k continuous-scrolling test passed
on the fixture with synthetic genres. Two existing Album layout checks still
failed: ordinary section reflow retained-delegate assertions and the 200k resize
retained-delegate assertion. Both failures reproduced using the committed
`Main.qml`, so these checks are recorded as remaining validation limitations.
Local-unavailability text reports local source observations while the existing
provider resolution path remains available.


## Playlist Details and position display

The Playlist page order is **Playlists | Songs | Details**. Songs is the expanding
center pane; Details is the resizable right pane. With no single selected playlist,
Details content is blank; with no selection, Songs is also blank. The redundant
From Library picker is removed. Library context actions and shared insertion remain
unchanged. The catalog control appears with selected-playlist statistics.

`playlist_details` aggregates the selected playlist's entry count, known effective
Track duration sum and unknown-duration count. Each entry contributes separately,
including duplicate and catalog-only entries. Empty lists show zero Tracks and
`00:00`; all-unknown durations show `Unknown`; mixed durations show the known sum
with an explicit partial/unknown-count annotation. No duration is invented for
catalog identity that currently lacks effective duration observations.

The presentation caches Details and reads this aggregate on the existing read-only
application connection in a background worker. Rapid selections/edits coalesce to
one active and one latest request; generation checks discard stale replies. Name,
count and duration refresh on selection, rename, insertion, removal, deletion and
explicit metadata refresh. Reordering and scrolling reuse the cached aggregate.
No playlist contents are loaded into QML to calculate totals, and no persistent
aggregate or schema migration is introduced.

`Row.playlist_position` is a 1-based absolute ordinal, separate from canonical
album Track numbers, persisted cursor positions and playlist-entry identity.
Stored ordering positions can have gaps. A bounded page read obtains one indexed
prefix count per playlist at its first row, then counts forward or backward across
the chunk. The page and rank share a read transaction so they observe the same
order. This closes displayed gaps after removal, numbers appended and duplicate
occurrences individually, and retains absolute numbering in deep/reverse windows
and after reload. Queue snapshots skip rank calculation. Album views retain their
existing disc/Track numbering.

Focused SQLite and offscreen coverage includes duplicate-aware known/partial/
unknown durations, catalog-only counts, mutation updates, blank/deleted selection,
column order, the direct catalog control, cached statistics on reorder, absolute
positions across bounded scrolling and reload, and unchanged album numbering.
The playlist performance harness also checks the 200k-entry aggregate and the
`playlist_entry_order` index used by prefix ranks and bounded forward/reverse pages.
The two previously recorded Album delegate-retention failures remain baseline
validation limitations.


Polish validation (2026-09-30): core and all three adapter suites passed, along
with the catalog/context-action/Details tests, normal album-numbering checks,
bounded playlist multi-selection/duplicate playback test and 200k offscreen
continuous scrolling. The ordinary and 200k Album delegate-retention checks
continue to report their recorded baseline failures. Formatting, whitespace and
strict Clippy passed. The new 200k aggregate measured 93–96ms; combined first/deep
pages measured 9.7–13.2ms and reverse chunks 10.8–11.6ms, including absolute
numbering. The aggregate explicitly uses `playlist_entry_order` to avoid SQLite
choosing a whole-entry-table scan with misleading fixture statistics. Prefix
counts use its covering index; queue snapshots omit display-number calculations.
These are local measurements, not newly imposed budgets.


## Playlist Songs details view and display sorting

Only Playlist Songs uses **# | Title | Artist | Album | Length**. Rows remain compact,
with elided text, neutral unknown durations and local-unavailability opacity/tooltip.
Library song lists and album track/disc numbering retain their previous presentation.
Headers select ascending order; clicking the active header reverses it. Initial order
is # ascending. The Playlist sort remains in session state when navigating to another
page; restart persistence remains deferred.

`ViewSort` is a display-only application query. # reads gap-safe absolute entry
ordinals from persisted order, never a local QML index. Text uses SQLite NOCASE;
Length uses effective numeric milliseconds, with unknown/negative observations
represented by -1 for deterministic sorting. Equal primary values use playlist ID,
stored position and entry UUID as tie-breakers, reversing consistently with direction.
No sort operation writes playlist entries or Library membership. Manual Move up/down
is disabled both in the menu and action handler outside # ascending.

Alternate sorts prepare a connection-local SQLite TEMP projection of selected
playlist entry IDs, stored positions, absolute ordinals and one active sort key.
A temporary index provides forward/reverse tuple-keyset pages across the complete
logical playlist. This is a reconstructible display cache, not another durable
metadata owner or a schema migration. Canonical metadata is fetched in one bounded
batch in the same read transaction. External commits (`data_version`) and
same-connection writes (`total_changes`) invalidate the projection before reuse.
Preparation and alternate-sort paging run in a background worker with one active
and one latest request; generation guards discard outdated replies. QML retains
200-row fetches and at most 600 rows. Canonical # queries keep the existing order
index and immediate reorder updates.

Shift-selection uses the sorted logical entry range, including entries outside the
loaded window. Selection, context actions, removal and Save use entry UUIDs. Queue
construction and playback continue using canonical playlist order and exact chosen
entry identity; a display sort cannot change persisted order or transient playback
order. Catalog-only entries use the same metadata, sorting and action paths.

Focused tests exercise every field/direction, numeric Length, missing metadata,
case-insensitive ties, distinct duplicates, global chunks, cache invalidation,
canonical ordinals, reload, reorder gating, session state and exact duplicate
playback. The offscreen scrolling fixture covers a reversed mixed playlist and an
extra duplicate, exhausting every sort beyond the 600-row window. The opt-in
`playlist_table_200k_bounded_sorting` test accepts
`MUSIC_LIBRARY_PLAYLIST_TABLE_STRESS_COPY` pointing to a disposable 200k fixture.
It adds catalog-only duplicate occurrences and checks all ten sorts with forward
and reverse bounded QML windows without changing Library counts.


Validation for this details-view slice (2026-09-30): 303 core tests passed (13
opt-in tests ignored); MusicBrainz 20, Spotify 37 and GStreamer 6 passed. All 17
non-opt-in QML tests passed in isolated offscreen processes; nine opt-in tests
remained separately ignored in that sweep. The new 200k Playlist table audit and
existing 200k continuous-scrolling audit passed when explicitly enabled. Formatting,
whitespace and strict Clippy passed for core, QML and all adapters. No commit was made.

On the disposable deterministic 200k playlist, alternate-sort projection/first-page
preparation measured about 0.4–0.7 seconds off the UI thread. Indexed next pages
measured 0.9–4 ms and deep forward/reverse pages about 0.8–1.2 ms. Query-plan tests
require the temporary tuple-order index and no page-time temporary sort. Canonical
first/deep pages plus ordinal calculation stayed around 10–12 ms, aggregates around
90–111 ms and a complete transient queue snapshot around 648 ms. These are local
measurements, not new cross-machine budgets. Screenshots include
`/tmp/playlist-table.png` and `/tmp/playlist-table-200k.png`.

The two earlier Album-layout baseline failures remain documented above. The
ordinary retained-delegate check passed in this isolated sweep; the opt-in 200k
Album resize check again failed with its existing "bounded delegate recreated on
resize" assertion. No Album layout or album-numbering behavior was intentionally
changed as part of Playlist table sorting.

### Duration enrichment

Canonical duration observations now survive independently of saved membership.
Catalog additions capture available length before playback, and accepted provider
matching can improve approximate values on the same Track. Length and Details
consume shared effective metadata; approximate totals show `≈`, and incomplete
totals retain their partial annotation. Migration 22 backfills already-persisted
manual duration evidence offline; normal catalog refresh/provider resolution
lazily enriches other existing unknown Tracks. Rendering performs no network
lookups. See [Songs details and canonical duration](songs-details-and-duration.md).

### Stable catalog action header

The catalog button lives at the top of Details in a toolbar with a reserved
24-pixel spinner slot. Aggregate loading changes spinner visibility inside that
slot, never toolbar geometry. Statistics are below the toolbar.

Geometry tracing reproduced the previous movement: `playlistDetails.pending`
changed the standalone BusyIndicator's visibility; its 24-pixel height plus the
inner ColumnLayout's 5-pixel spacing increased the parent height/implicitHeight
from 97 to 126 pixels and moved the button's local y from 57 to 86. The outer
Details height, margins, button height/visibility and 200-row song window stayed
unchanged. This was transient loading content entering the layout, rather than
playlist row-count or playback-clock changes.

The focused offscreen geometry regression uses a large duplicate-entry playlist
to keep actual asynchronous aggregates pending across frames. It refreshes the
aggregate with same-name renames every two seconds, samples geometry for 20
seconds idle and 20 seconds with diagnostic playback active, and also watches
button y changes between samples. Both pending/ready states must occur. No
absolute button position is imposed; layout and resizing remain responsible for
its placement. Playback is the diagnostic engine, without audio output.

### Resizable table columns

The shared details header now offers draggable separators and session-retained
widths. The existing `# / Title / Artist / Album / Length` columns and global
sorting semantics are retained. The position header has sufficient content space
to show `#` instead of an elided label. Narrow panes scroll horizontally with
headers and cells aligned; position and entry identity remain independent of
widths. See [resizable details columns](songs-details-and-duration.md#resizable-details-columns).
