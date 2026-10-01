# Continuous library scrolling

Main Artists, Genres, Albums, Songs, Playlists and playlist Songs panes scroll
continuously. There are no main-pane Next/Previous buttons or page labels.
The queue drawer and Add to Playlist destination chooser retain their existing
separate bounded paging.

## Loading and bounds

The presentation host retains up to **600 rows per pane**, fetching **200 rows at
a time**. Each query requests 201 rows to determine whether another fetch is
possible; lookahead never enters the displayed model. Approaching within one
viewport of the loaded end automatically fetches successors. Approaching within
half a viewport of the loaded start fetches predecessors when earlier rows have
been discarded. A 32 ms timer coalesces viewport observations during scrolling.
When a fetch exceeds the window bound, rows leave the opposite end. No total
count, OFFSET walk, full-library QML array or history of discarded chunks is needed.

Queries extend the existing deterministic keyset ordering, including the complete
ID tiebreakers, Album/Release/disc/Track ordering keys and playlist-entry positions.
The reverse API returns nearest predecessors first; the host reverses that chunk
before prepending it. Both directions honor the same filters. Single-playlist
queries additionally expose a position bound directly to `playlist_entry_order`,
keeping deep forward and reverse reads indexed without a migration.

Each inactive view retains its bounded windows, sorts, selections and visible-row
identity/pixel anchor in Rust session state. A view switch restores that identity
near its previous position. Navigation buttons save the viewport before switching.
Filters and sorts reset affected windows and advance their dataset epoch. Search
retains direct identity seeks into Artists. Durable refreshes invalidate inactive
materialized windows, retaining an identity for a fresh bounded seek; a removed
anchor falls back to the new dataset's beginning. Nothing is persisted across restarts.

## Presentation and selection

Before changing a window, QML captures the first intersecting visible row or Album
tile and its pixel displacement from the viewport top. It rebuilds the bounded
projection, forces ListView/Album Flow layout and restores that same identity at
that pixel displacement within the same update. An active flick resumes with its
remaining velocity after model replacement, including when old rows are evicted. Layout settlement is synchronous:
a deferred restoration must not undo a subsequent scroll. Anchor discovery walks
instantiated delegates once rather than repeatedly searching all rows. Playback
and selection-only updates do not rebuild an unchanged projection.

Songs sections continue to use canonical Album identity, never display titles.
One section spans all retained adjacent chunks for an Album. If its beginning is
trimmed, the continuation section starts above the preserved viewport; loading
another chunk does not add an interior header. A newly encountered Album gets its
own header even when its title equals the previous Album's title.

Logical selection stays in Rust as stable IDs, independently of loaded rows.
Only selected IDs matching the current window are projected for row highlighting;
the selection count and action target include offscreen IDs. Ctrl selection is
unaffected by eviction. Shift ranges use the existing background query reader to
resolve IDs in logical order without putting intermediate metadata rows into QML.
Actual filter changes retain the existing downstream pruning rules.

Playlist rows retain entry IDs, not just Track IDs. Forward/reverse loading uses
playlist/position/entry ordering. Duplicate Tracks remain distinct selectable
entries, and explicit playback resolves the selected occurrence in the complete
persisted program through the existing playback boundary.

## Validation

Focused real-SQLite tests cover reverse chunk recovery for Artists, Albums and
Songs, both alphabetical directions, year/Artist/Album ordering, tied titles and
multi-disc positions. Playlist tests cover name ties, reverse windows, exact order,
and distinct duplicate entry identities across multiple selected playlists.

Offscreen QML tests exercise automatic fetching, complete 1,003-Song traversal,
upward restoration, exact row/pixel anchoring, kinetic scrolling through fetches
and eviction, sort/filter resets, independent
views, offscreen Ctrl and Shift selection, offscreen queue/playlist actions,
canonical headers across chunk boundaries, duplicate entry selection/playback,
and invalidation of inactive windows after library edits. Separate 803-Release
fixtures exercise Artists, Genres and Album tile scrolling. All assert bounded
models/delegates. The 200k audit exercises eight fetches and reverse recovery in
Artists, Albums and Songs, plus available Genres, navigation restoration and
screenshots well beyond the initial chunk.

Commands:

```sh
cargo test --offline
cargo test --offline --manifest-path adapters/musicbrainz/Cargo.toml
cargo test --offline --manifest-path adapters/spotify/Cargo.toml
cargo test --offline --manifest-path adapters/gstreamer/Cargo.toml
QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software cargo test --offline \
  --manifest-path tools/qml-diagnostic/Cargo.toml --all-features -- --test-threads=1
cargo run --offline --release --example continuous_scrolling_performance
# Use a SQLite backup of the deterministic fixture, with synthetic genres seeded.
QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software \
  MUSIC_LIBRARY_CONTINUOUS_STRESS_COPY=/tmp/continuous-200k.sqlite \
  cargo test --offline --manifest-path tools/qml-diagnostic/Cargo.toml \
  --all-features continuous_scrolling_200k_bounded_qml_render \
  -- --ignored --test-threads=1
```

Strict Clippy runs with `--all-targets --all-features -- -D warnings` for core,
all three adapters and QML; formatting, qmllint and whitespace checks also run.

## Measurements (2026-09-30)

Release diagnostic on the existing deterministic 200k library, 1,000 fetches per
order, verifies every saved Track exactly once while retaining at most 600 display
rows. Its identity set is diagnostic-only, outside the application/QML path.

| Order | p50 fetch | p95 fetch | Maximum | Deep reverse fetch |
| --- | ---: | ---: | ---: | ---: |
| A–Z | 1.412 ms | 2.052 ms | 5.067 ms | 1.306 ms |
| Z–A | 1.281 ms | 1.469 ms | 3.025 ms | 1.164 ms |
| Album | 1.145 ms | 1.386 ms | 5.137 ms | 1.199 ms |

The single-playlist position bound reduced combined first/deep reads on the
200k-entry fixture from 16–21 ms to 1.14–2.11 ms. Combined deep/near-head reverse
reads took 1.30–1.71 ms. Complete queue reads remain explicit background actions.
Query-plan checks use the existing Track-title and playlist-order indexes, with
no temporary ordering tree in the tested cursor shapes.

The optimized debug offscreen 200k scrolling/render test took 10.8 seconds,
including deliberate test waits and screenshots. Running the built test binary
separately from compilation peaked at 211 MiB RSS. This is the whole Qt test
process, not a new product memory budget. Models remain capped at 600 and Album
tiles at 600 regardless of the library size or logical selection size.
Screenshots inspected after multiple fetches include
`/tmp/library-continuous-artists-deep.png`,
`/tmp/library-continuous-albums-deep.png`,
`/tmp/library-continuous-songs-deep.png`,
`/tmp/library-continuous-headers.png` and
`/tmp/library-continuous-playlist.png`.
