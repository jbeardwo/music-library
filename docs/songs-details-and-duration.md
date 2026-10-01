# Songs details and canonical duration

## Main Songs page

The main Songs page is a flat, compact table with **Song, Artist, Album, Genre**
columns. Artist uses the canonical display credit. Genres come exclusively from
associated local tag observations, deduplicated and sorted into a compact ` · `
list. Missing album/genre values are blank; an absent artist displays the existing
unknown-artist fallback. Embedded Artists, Genres and Albums song panes retain
normal song rows and album numbering. The Playlist table retains its five columns,
persisted position display, entry identities and canonical playback ordering.

Headers select an ascending sort; clicking the active header reverses it. The main
Songs A-Z/Z-A control is hidden. Sort state follows existing session per-view state;
restart persistence is deferred. Default sorting remains alphabetical title.

`browse::Request.song_column` selects effective title, artist credit, Release title
or compact local genres. SQL sorts the entire logical result by the lowercased
field and opaque Track ID, with both directions reversed together. Cursor reads,
reverse reads, logical Shift ranges and existing queue/context operations use the
same request. Dedicated expression indexes support these queries. Stable Track IDs
remain selection identities. The table fetches 200 rows at a time and retains at
most 600; changing sort resets the window. No QML-only sorting or album grouping
is involved.

## Duration ownership and acquisition

Migration **0022_duration_and_song_details.sql** adds a typed, source-owned
`track_duration_observation` table, parented by canonical Track, plus effective
`duration_approximate` and `genre_names` fields and the Songs sorting indexes.
Playlist entries continue to reference opaque canonical Track IDs. Library
membership, playable sources and transient queue snapshots remain independent.

Effective duration prefers existing local duration, then authoritative provider
track duration (quality 0), exact catalog track duration (quality 1), then
approximate catalog/recording duration (quality 2). Observations are retained per
provider/source key; deterministic quality/provider/key precedence chooses the
non-local value. Refreshing one source does not erase another source's evidence.
An authoritative value can replace an approximate effective value without changing
Track identity or Library membership.

MusicBrainz release-track length is exact catalog evidence; recording-length
fallback is approximate. Spotify catalog track length is authoritative. Catalog
persistence captures available durations for new and reconciled existing Releases,
using a batch position lookup. Accepted song resolution, manual matching, accepted
program occurrences and recording matching capture their existing response
metadata. Recording-only evidence is approximate. These operations require no
playback; no additional duration-discovery request is made.

Old manual-association JSON with a persisted duration is backfilled offline during
migration, conservatively treating legacy MusicBrainz evidence as approximate.
Other existing unknown Tracks gain duration when their catalog metadata is normally
refreshed or provider matching normally completes. There is no eager per-row
network backfill. Ordinary bounded playlist reads consume effective metadata in the
existing joined query; rendering does not contact providers.

Playlist Length displays normal duration, `≈` for approximate duration and `--:--`
for unknown. Details aggregates entry occurrences, including duplicates, with
counts of unknown and approximate values. Known approximate totals show `≈`;
mixed known/unknown totals retain the explicit partial annotation; all-unknown
lists show Unknown. Accepted metadata refreshes the view and cached aggregate.
Reordering continues to reuse statistics.

## Validation

Focused SQLite tests cover all four sorts in both directions, complete traversal,
stable ties, reverse paging, logical Shift ranges and local multiple/empty genres.
Duration tests cover unsaved catalog insertion, duplicate totals, identity reuse,
reload, exact provider enrichment without playback, lower-quality refresh,
unknown/partial totals and offline migration backfill without membership.

Offscreen tests exercise real headers, cells, compact rows, selection/context
menus, embedded-pane preservation and duration/Details updates without playback.
All 18 ordinary diagnostic tests pass in isolated processes. A real MusicBrainz
result, Gorillaz's **DARE**, was added to a fresh playlist with duration available
before playback. Its entry and duration survived reload; Library membership and
Library views remained empty. Playback resolution correctly reported unavailable
because no remote playback provider was configured. The deterministic provider
resolution test separately demonstrates an authoritative upgrade on the same Track.

The 200k scrolling diagnostic exhausts the complete Library under the eight new
Songs sort variants and the three existing sorts, checks unique/completed IDs and
reverse reads, and caps the window at 600. Indexed artist/album/genre query plans
avoid temporary sorting. Offscreen 200k main browsing and all ten Playlist display
sorts pass. Playlist aggregate plans use `playlist_entry_order` and effective
metadata primary-key joins. Diagnostic timings are machine/load-dependent: new
Songs p95 fetches ranged approximately 3–30 ms in this run; a 200k Playlist display
projection took about 0.5–0.8 seconds on its background worker, followed by bounded
indexed reads. Core, adapter, formatting and strict Clippy checks pass.

The two earlier Album layout delegate-retention failures remain documented in
[Playlists validation](playlists.md). The ordinary Album layout test passes in this
run; the opt-in 200k resize check retains its previously reproduced baseline
failure. These changes do not alter Album tile layout.

## Resizable details columns

The shared Songs/Playlist table has thin dividers with 12-pixel pointer targets
and a horizontal split cursor. The same drag implementation covers the header
and the entire body viewport, including rows and blank space. Body targets sit
above delegates, follow horizontal scrolling, and exclude scrollbar hit areas. Dragging a divider adjusts its adjacent columns
continuously, clamped by both minimum widths. Header clicks outside the divider
continue to select/reverse global sorting. The narrow Playlist position header
retains `#`; removing redundant Button content padding keeps its glyph and sort
indicator visible at its minimum width.

Default widths allocate more space to Song/Title, medium widths to text metadata,
and narrow widths to position/Length. Main Songs minima are 180/120/120/100 pixels;
Playlist minima are 32/160/120/120/64 pixels for its existing five columns. Defaults
adapt to available pane width until adjusted; user-adjusted widths are retained
independently for both views in the current window/session. Restart persistence is
deferred; no new settings system or database layout schema is introduced.

If total widths exceed the pane, the existing ListView scrolls horizontally with
an attached scrollbar. The clipped header follows the same contentX. Row cells,
headers and column lines consume exactly the same width array; the full content
width remains the row's selection/context hit area. Columns keep their minima
rather than compressing to fit a narrow viewport. Embedded song panes retain their
existing presentation and vertical scrolling.

Resizing changes only QML width arrays; it calls no browse, catalog, matching or
playback operation and never changes persisted order or active sort. Regression
coverage uses actual pointer drags on every divider in the header and at 10%,
50% and 90% of the body height, both minimum clamps, header
clicks, horizontal alignment, exact-row selection in the last scrolled cell,
independent session restoration and unchanged model epochs/IDs and active playback
queue. Offscreen checks exercise these controls on both 200k fixtures, then retain
the existing global-sort and bounded-scrolling checks. The idle/active-playback
20-second Playlist geometry audit still passes without periodic button movement.
