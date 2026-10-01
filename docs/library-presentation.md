# Artwork tiles, browse sorts and queue snapshots

## Library navigation

Lightweight Artists / Genres / Albums / Songs / Playlists links replace the Music
heading. Artists is the default; active navigation is bold and inactive navigation
is grey. The existing player and queue shell remains instantiated across views.

| View | Panes and scope |
| --- | --- |
| Artists | Artists / Albums / Songs; existing credit and selection hierarchy |
| Genres | Genres / Albums / Songs; saved Tracks carrying the selected local genre |
| Albums | Albums / Songs; all saved Albums, then optional Album filter |
| Songs | One full-width, continuously scrolling saved Songs pane |
| Playlists | Playlists / selected playlist entries, with duplicate entry identities |

Selections, sort choices, loaded windows and visible-row anchors are remembered independently per view
for the session. Search navigation returns to Artists and retains its existing
keyset seek behavior. Navigation, selection and sorts never replace or reorder a
queue. Explicit Play captures the applicable complete Songs request, including
both Genre and Album when selected. Single-Song append stays a single Track.
Playlist behavior is documented in [playlists.md](playlists.md).

Migration 20 adds indexed `file_genre_observation`, owned by the local source.
Repeated tag values remain separate strings; surrounding whitespace and empty
values are discarded. Comma/slash text is not interpreted as a provider taxonomy.
Genres query distinct indexed names and require at least one associated saved
Track. Tracks without tags contribute no invented Unknown genre. Multiple sources
contribute their observed genres, including preserved unavailable sources.
Genre Albums require a saved matching Track; an Album with mixed genres appears
once. Genre Songs retain the same Track predicate when narrowed to an Album.
Small scopes gather indexed IDs; a probe bounded to 1,001 observations selects
indexed streaming for large scopes. No delegate queries or network lookup occur.

The old extractor did not retain genre tags. A `genres_observed` marker distinguishes
an extracted empty genre from an older observation. Older files are parsed once on
their next ordinary scan/admission, then unchanged scans skip them again. Opening
the database performs no filesystem work. Genre updates replace only that source's
genre observations; identity, user overrides and membership rules are unchanged.

Year sorting inserts newest-first year headers and an Unknown section last. Each
section has its own wrapping Flow of fixed 120×174 tiles with 120px artwork. Headers
and sections are built only from the database-ordered bounded window. A retained continuation group has one heading at
its window start, outside the preserved viewport when its start was discarded. The Album ListView retains a bounded maximum
page-height cache so transient Flow estimates cannot evict/recreate sections while
resizing. Album ordering, artwork scheduling and keyboard navigation stay intact.

## Presentation

The QML prototype shows Albums as fixed 120×120 logical-pixel artwork
inside 120×174 tiles, with 12px horizontal and vertical gaps. Tiles wrap to the
available width without stretching. Captions are one-line elided text.
The second line is the Album artist credit globally and the release year within
an Artist. Unknown years have an explicit label. Artist grouping inserts visible
section headings keyed by the primary credited Artist's application ID; equal
names do not merge sections. Album IDs remain internal.

Tiles retain selection, double-click, context-menu Play/append, Enter, arrow-key
focus and direct local-search navigation. QML receives at most 600 logical rows
per pane; a Flow per Artist or year section lays out that bounded window without changing
its logical order. The window stays instantiated during reflow, preserving delegates
and avoiding artwork requests caused by width changes.
Partial Albums and Tracks without playable sources use the same queries.

The current Track's primary Songs text is red (`#c6283e`). The comparison uses
`currentId`, the application's current queue Track ID, independently of selection
or playback provider. Next, Previous, queue replacement and engine/route events
already publish this identity. The bottom current-track button includes a 46px
cover and still toggles the Now Playing drawer. Its shared placeholder also works
for missing or malformed artwork. The player bar has room for all transport,
seek and volume controls.

## Pane widths

Qt Quick Controls SplitView supplies two standard draggable handles, each 16px
wide with a centered separator. The center Album pane is the fill pane: moving
either divider resizes its two neighbors while leaving the other divider fixed.
Default proportions follow 230:320:470 for
Artists:Albums:Songs. Minimum widths are 160, 148 and 260 logical pixels. The Album
minimum is derived from 120px artwork + two 6px insets + 16px scrollbar allowance.
The window's existing 800px minimum accommodates all three minima and separators.
Arrow Up/Down advances by the current Album column count; Left/Right advances one
logical Album. Navigation scrolls the actual tile within its Artist section, even
when that section exceeds the viewport. Resizing does not change library state,
sorts, selection, queue or playback. Artist/year caption rules are unchanged.

Widths remain session-local. There is no existing general UI-preference mechanism;
persistent layout settings are deferred rather than adding a settings subsystem.

## Image-quality inspection and shared renderer

The backend decodes original bytes once and caches a lossless PNG, reducing only
images exceeding 500px. Smaller original images are retained. No encoding, quality,
size cap or source-priority changes were made. Sampled real cache files were
300–500px; no unexpectedly tiny thumbnails explained the tested covers.

Previously QML requested 500×500 for both tiles and the 46px player, then shrank
that texture while painting. A fine-line stress image showed strong aliasing in
that render path. Detailed real covers retained existing printed grain/halftone,
lettering and deliberate collage effects already visible in the PNG originals;
these were not new lossy cache artifacts.

The shared AlbumArt renderer requests only its physical display size
(logical area × device pixel ratio), asynchronously, with smooth and mipmap
filtering. Its sourceSize is tied to the fixed artwork area, not pane width.
Inspection also reproduced Qt PreserveAspectFit's loading behavior overriding
the requested bounding size: a 500×300 source loaded as 200×120, and a 24×16 source
was enlarged. This behavior is described in the
[Qt Image sourceSize documentation](https://doc.qt.io/qt-6/qml-qtquick-image.html#sourceSize-prop).
The renderer now uses bounded loading and centers the Image with geometry derived
from its decoded aspect ratio. Stretch mode operates only within that same ratio,
so there is no distortion. This uses one Image/decode, retains small originals,
and fits non-square images within the square area. The same 500×300 source now
loads as 120×72; the small source stays 24×16 and is not enlarged to fill the tile.

The player keeps its compact 46px cover and behavior, receiving only the shared
loading fix. Missing/error placeholders and artwork retry behavior are unchanged.
Rendering reads caches; it does not resize/recompress/rewrite them on disk.

## Layout and render validation

`fixed_album_tiles_wrap_and_split_handles_preserve_interactions` exercises actual
SplitView dragging, all minima, fixed dimensions, one/two/four-column wrapping,
retained delegates across normal and Artist-section reflow, keyboard movement,
selection, context menus, double-click Play, wrapped search navigation and queue
preservation. A test intercepts artwork scheduling and verifies that no requests
are made by reflow. Existing UI tests cover queue append/removal, caption rules,
filtering, sorting and distant search navigation.

`tools/qml-diagnostic/tests/album_rendering.py OUT.png [CACHED.png ...]` compiles a
Qt harness using the shipped AlbumArt component. It renders the old and new paths
side by side, verifies loaded/decoded dimensions including small and non-square
sources, checks cache bytes and modification times, and checks no requests on
resize. Run with `QT_QPA_PLATFORM=offscreen` if needed. Visual inspection used
five existing covers containing fine text, photography, bright/dark artwork and
detailed illustration, plus a high-contrast fine-line stress image. The new path
removed the stress image's aliasing without changing encoded artwork. The compact
player was inspected alongside tiles. Decode bounds and small-image preservation
also passed at device pixel ratios 1 and 2. This was inspection of actual offscreen
renders, not a human desktop/GPU/Windows audit.

The opt-in `album_layout_200k_bounded_resize_and_scroll` takes
`MUSIC_LIBRARY_ALBUM_LAYOUT_STRESS_COPY` pointing to a disposable SQLite backup of
the deterministic 200k fixture. It keeps only 200 logical Albums, checks that
repeated resize/scroll preserves delegates and does not schedule artwork or change
state, and reports timing. A debug run measured 60 iterations in about 75ms
(including 1ms event-loop waits per iteration), with 200 retained Albums.
This is a measured layout test, not a GPU frame-rate guarantee. At DPR 1, square
tile pixels require roughly 15MiB for a full 200-Album page including mipmaps,
instead of 500px decoded textures for each tile; identical URLs/sizes can share
Qt's image cache. Native DPR scales that memory accordingly.

## Sorts and explicit programs

| Pane/scope | Cycle |
| --- | --- |
| Artists | A–Z, Z–A |
| Albums, global | A–Z, Year, Artist |
| Albums, Artist selected | A–Z, Year |
| Songs, global | A–Z, Z–A |
| Songs, Artist or Genre selected | A–Z, Z–A, Album |
| Songs, Album selected | Album, A–Z, Z–A |

Year is newest first, with unknown values last. Artist sections are alphabetical,
then newest Album first. Artist Songs in Album mode follow the applicable Album
pane ordering (title or year); each Album contains Release/disc/Track order.
Stable application IDs break ties. Album-scoped Songs in Album mode retain normal
Release/disc/Track order. Songs A–Z uses title then Track ID; Z–A reverses both keys.
Album order shows a separate number immediately before the title: Track number
for single-disc Releases, disc.Track (for example 1.02 and 2.01) for multi-disc
Releases. Missing positions remain blank. Alphabetical modes hide the number.

The Artist- and Album-Songs choices are independent session state. Playback does
not reset them. Entering an Artist from global Album grouping converts Artist to
Year, so no invalid active mode remains. Local Search seeks by the target ID using
the same current ordering keys, including reverse order and pages beyond 200.

Only explicit Play creates a replacement program. A copied browse request is sent
to the background queue reader, which reads the **complete** saved logical result
in the displayed order. The selected Track ID locates the exact start in that
snapshot, including a later-page Track or one with a duplicate title. Artist and
Album Play use the applicable Songs modes and start at zero. Artist/Album append
uses the same ordering; Song append adds just one Track.

Changing sorts, selection, search navigation or pages never calls queue replacement
or append. An in-flight queue request also retains the modes captured at the time
of Play. A subsequent Play takes a new snapshot; it does not reuse a previous
program or a QML page.

## Artwork service and cache

`artwork::Resolver` is an application feature with an adapter-owned `Provider`
interface. It batch-loads source/identity associations and cache records. The QML
adapter batches requests from instantiated tiles and the current-track area.
A local/cache worker and a separate network worker own their SQLite connections;
slow provider requests cannot hold up cached covers. Neither QML bindings nor the
browse queries perform HTTP, audio-file parsing or image decoding.

Resolution priority:

1. Embedded pictures from saved, available local sources, preferring front art.
2. Local `cover`, `folder`, `front` or `album` sidecars (case-insensitive filename,
   JPEG, PNG or WebP).
3. Cover Art Archive front-500, using an established MusicBrainz Release Group or
   Release association.
4. Spotify Album images, using an established Spotify Album association.

No matching/search relaxation or identity writes occur. CAA uses the existing
MusicBrainz process request gate. Spotify Album requests use its existing catalog
authentication and typed error infrastructure. Image transfers are credential-free,
bounded to 20 MiB and ten seconds, with bounded redirects. Provider cooldowns honor
Retry-After; failures leave the shared placeholder. API references:
[CAA](https://musicbrainz.org/doc/Cover_Art_Archive/API) and
[Spotify Album](https://developer.spotify.com/documentation/web-api/reference/get-an-album).

The `directories` platform abstraction selects the application cache location
(Windows Local AppData, Linux XDG cache, macOS user cache). No music-folder writes
or primary-database image blobs are used. Decoded images become PNG files with a
maximum edge of 500px, preserving aspect ratio and avoiding upscaling. Decoder
allocation and dimension limits reject unreasonable/malformed input.

Migration 17 records Album association, origin, locator, generated cache filename
and check time. Missing files regenerate from the preserved source associations;
failed attempts preserve prior provenance. Negative results are cached for 24 hours,
and requests are deduplicated in the UI adapter. Membership, source and external
identity changes invalidate affected records. Superseded cache files are removed.
An optimistic write check prevents an old provider reply from overwriting newer
embedded artwork or source invalidation. There is no global disk quota or cache
management UI in this slice.

## Queries and migration

Migration 17 also adds `album_browse_order`: indexed, materialized title, year and
primary-Artist ordering keys. Metadata and credit triggers update affected Albums.
Application Album year has precedence. If absent, the browse projection uses the
earliest known Release metadata year, then the earliest effective Track year. This
is a presentation fallback; it does not overwrite Album metadata, assert an exact
edition or change matching evidence.

All browse sorts use keyset cursors with complete tie-break keys. Album-grouped
Songs walk the Album and Release ordering indexes; only the bounded chosen page
is expanded into display metadata. A bounded credit-count probe chooses direct credit gathering for small Artists
and indexed streaming for large Artists. Both plans use identical ordering and
membership semantics. Queue construction uses the same query with
its row limit removed, off the UI thread. No entire result is loaded into QML to
sort or prepare playback.

## Validation (2026-09-29)

- Core suite, focused artwork/browse tests, QML interaction/search/player tests,
  MusicBrainz and Spotify adapter tests, and GStreamer playback/EOS tests.
- Real temporary SQLite tests cover duplicate identities/titles, year fallback,
  both program orders, partial membership, keyset pages, direct seeks and complete
  programs beyond 200 Tracks.
- QML tests inspect red text and selection independently, Next/Previous updates,
  sort cycles, queue preservation, new sorted snapshots, exact starts and additive
  appends. Existing tile mouse/context/keyboard, drawer and Add Music tests pass.
- Artwork tests cover embedded/sidecar/provider priority, Spotify-only and missing
  art, malformed input, missing cache, cache reuse, scaling, source invalidation,
  gated asynchronous retrieval and a late provider reply racing newer embedded art.
- Strict Clippy and formatting checks cover the affected crates; `git diff --check`.

Release build, deterministic 200k fixture, warm combined first-page + next-page +
direct-seek measurements:

| View | Combined time |
| --- | --- |
| Artists Z–A | 3ms |
| Global Albums Year / Artist | 10ms / 6–7ms |
| Large Artist's Albums A–Z / Year | 11ms / 17–18ms |
| Large Artist's Songs A–Z | 16–22ms |
| Large Artist's Songs by Album, title / year groups | 27ms / 34–36ms |

Individual warm page queries in this matrix took roughly 1–9ms. Initial reads
were slower (up to about 70ms per page in the last run). A complete 20k-Track queue
took 0.15–0.45 seconds across runs, off the UI thread. Global Songs after 20k rows
took about 1ms. These are local measurements, not cross-machine budgets.

The desktop audit used a disposable copy of the 499-Track real library with local
GStreamer playback. It exercised selection, double-click, append, Next/Previous,
seek, volume, sort cycling, queue preservation during browsing, and explicit replay
from the newly ordered pane. Captures were visually inspected for the grid,
year/artist captions, placeholder, red selected current Track and player artwork.
One native-renderer rerun crashed after graphics-driver warnings; the software
renderer audit passed with the final layout. Live Spotify handoff was not rerun;
its adapter and playback tests passed. No Windows runtime test was available.

Screenshots from the final audit are in `/tmp/music-library-presentation-scoped.png`,
`/tmp/music-library-presentation-grouped.png`, `/tmp/music-library-ui-library.png` and
`/tmp/music-library-ui-queue.png`. No commits were made. Matching edge cases,
catalog-search redesign, playlists, skins and unrelated bugs remain outside scope.

## Library view validation (2026-09-30)

Focused core tests cover small and large Genre query plans, saved membership,
mixed tags, duplicate source contributions, Album intersections, both alphabetical
orders, keyset pages/direct seeks and complete programs. A real tagged FLAC test
covers discovery before admission, migration from 19, incremental rescans, missing
sources, membership removal, retagging and user-title preservation. Migration
fixtures now expect schema 20 and still check rollback behavior.

QML interaction tests click every navigation link, including at the 800px minimum,
check all layouts and blank playlist Songs, restore view state, navigate search
from Songs, check number placement and visibility, compare exact reverse order,
and preserve an active queue. Year sections are tested with multiple wrapped tiles,
missing years, retained delegates and no artwork requests during reflow. Existing
context-menu, selection, Play/append, search, player and resize tests also run.

Offscreen software-rendered inspection used a disposable copy of the 499-Track
real library. Its local tags yielded Acoustic and other genres; Acoustic Albums
and Songs matched saved Shoes and Socks Off music. All five pages, year headings,
120px covers, numbers, alphabetical number hiding, reverse Songs and blank playlist
Songs were exercised and captured under `/tmp/library-views-real-*.png`. This was
actual rendered inspection, not a native desktop/GPU or Windows audit.

The core suite, all-feature QML suite, formatting, strict Clippy for both affected
crates and `git diff --check` passed. The opt-in 200k QML audit checks all five
views, Songs cursor paging and reverse sorting, bounded Genre filtering and blank
playlist Songs. Its 60 resize/scroll iterations took about 80ms while retaining
200 logical Albums and preserving artwork request counts and queue state.

The `library_views_performance` example accepts a disposable deterministic 200k
SQLite copy; optional `--seed-genres` adds 200k synthetic source observations spread
across 50 genres. A release-build warm first-page + next-page + direct-seek run
measured about 5ms for global Songs A–Z/Z–A, 11–12ms for Albums Year, 21ms for Genres,
66–73ms for Genre Albums, 49–50ms for Genre Songs and 68ms for Genre Album order.
Each browse result remains at most 201 rows, with at most 200 expanded in QML.
These local measurements are not cross-machine budgets. Persistent navigation,
playlist functionality, provider genre enrichment and a larger Song metadata table
remain deferred.

Playlist persistence and editing now supersede the placeholder described above;
see [Playlists](playlists.md). Other view, sort and paging behavior is unchanged.

Main-pane loading and scroll state now follow [continuous scrolling](continuous-scrolling.md). Earlier page-specific measurements below describe the underlying 200-row query chunks.
