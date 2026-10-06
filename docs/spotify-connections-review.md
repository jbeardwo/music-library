# Spotify Connections review

**Settings and diagnostics (⋯) → Spotify Connections** opens the management view.
It is separate from Artists, Genres, Albums, Songs and Playlists. Each row is one
saved canonical Track, identified by its opaque application Track ID.

## Inclusion and ownership

The bounded Songs query joins `library_membership` and excludes any Track present
in the `trusted_spotify_track` SQL view. That view mirrors
`track_provider_occurrences` precedence:

1. A manual Spotify decision's `evidence.identities` contains a Spotify `track` ID.
2. Without a manual decision, a durable `track_external_identity` contains that ID.
3. Without either overriding identity, `provider_track_association.occurrences`
   contains that ID and its Album representation still has the corresponding
   trusted `album_external_identity`.

A nonempty Spotify Track identity resolves the Track. Spotify Album metadata,
Recording evidence, authentication, network and playback availability do not
control inclusion. No local file is required. Local availability and duplicate
PlaylistEntry occurrences do not change the result. Unsaved playlist-only Tracks
are excluded. No matching rule, metadata precedence or manual confirmation
semantics changes.

## Table and workflow

The shared details table displays **Song, Artist, Album, Reason**. Each header
sorts the whole SQL result ascending or descending, with opaque Track IDs as stable
ties. It retains the existing 200-row page / 600-row window and bidirectional
keyset scrolling. Reason has its own indexed ordering. Columns resize independently
with finite minima/maxima; review widths are separate from Songs and Playlists.
Widths, sort and filtering persist within the existing window/session, not across
application restarts.

Right-click **Review this Album** narrows the unresolved query to that Album;
**All Albums** clears the filter. The count is the total unresolved Library set,
independent of that filter. The default empty state reads
**All Library Tracks are connected to Spotify.** A filtered empty state identifies
that Album instead.

Double-click or right-click **Spotify connection…** opens the existing diagnostic
for the row's canonical Track ID, independently of Now Playing. Its existing
explicit search, manual confirmation and bounded Album re-evaluation are reused.
Inspection, scrolling and sorting never request provider data.

Migration 0026 adds `spotify_connection_review`, initialized for existing/new
Tracks to `not_evaluated`. It stores typed reason codes and concise labels from the
existing structured Album/positional diagnostics and song feasibility decisions.
Artist rejection, incompatible program, missing position, version conflict,
competing candidates and not-yet-evaluated states remain distinct. Version evidence
is surfaced even when Artist uncertainty also withholds the candidate; the full
combination stays in the diagnostic. Provider failures preserve the last meaningful
summary. Historic session-only reports cannot be reconstructed offline, so old
unresolved Tracks initially show **No reconciliation attempted yet** until an
explicit or normally scheduled evaluation supplies evidence.

Existing association completion callbacks invalidate the review, without polling.
They re-read only loaded Track IDs in bounded chunks and remove newly connected
rows, including multiple Tracks from an Album operation. QML patches/removes/moves
rows by ID while retaining the ListModel and surviving delegates. A surviving
viewport anchor keeps its pixel offset; removing the anchor chooses its nearest
surviving neighbour. If the entire window resolves, a bounded successor/predecessor
window is read. Changed Reason ordering seeks a bounded window around the current
anchor, preventing stale keyset boundaries. Sort, Album filter and column widths
remain intact. Inactive views are invalidated too.

The indexed count subtracts distinct trusted saved Tracks from SQLite's membership
count. It does not walk all Track identities in application code. A background
read coalesces completion events; visible row removals decrement the cached count
immediately, then the authoritative count includes offscreen resolutions.

## Validation, 2026-10-05

The offscreen Qt interaction harness exercised the real Library at
`/tmp/music-library-ui-test.sqlite`, with real Spotify catalog requests and the
actual diagnostic/confirmation actions. No playback command was issued.

- **Hop Along — Freshman Year:** all 16 Tracks remain unresolved, including
  **Organ Song**. The summary identifies Artist/trusted identity conflict;
  the diagnostic retains the differing credit and incompatible program evidence.
- **Piglet — Songs:** all 9 Tracks remain unresolved. The diagnostic shows the
  **Songs EP (Live in Chicago)** version conflict; the review summary exposes
  the version qualifier conflict. No single-candidate automatic shortcut was added.
- **Floral — The Second Floral EP**, **Bygones — Spiritual Bankruptcy** and
  **Tabar — Hugs EP:** explicit existing Album re-evaluation safely connected
  their remaining Tracks. All three filtered unresolved sets are empty, including
  after reopening. Multiple rows disappeared together.
- **Bygones — Anxiety Attack**, canonical Track
  `01c47f85-5c33-4760-89ba-421389e8f128`: the bounded search returned a uniquely
  supported candidate `1nbkCFBm55dcMW9PgFRO3k`, explicitly confirmed through the
  existing manual action. Its row disappeared and unresolved count changed
  **426 → 425**. Library membership remained saved and queue/current Track/status
  were unchanged. After the additional safe Album resolutions, the total was
  **412 unresolved / 484 saved Tracks**. Fresh Library connections retain trust.

Deterministic fixture: `target/performance/library-200k.sqlite`, 200,000 saved
unresolved Tracks. `cargo run --offline --example spotify_connections_performance`
measured 20 bounded pages per sort (4,000 rows), with zero provider requests:

| Sort | First 201 rows | Next 201 rows | Page p95 | Album filter (10 rows) |
| --- | ---: | ---: | ---: | ---: |
| Song | 5.70 ms | 5.54 ms | 6.39 ms | 1.47 ms |
| Artist | 12.51 ms | 12.80 ms | 15.38 ms | 1.79 ms |
| Album | 6.77 ms | 5.66 ms | 7.48 ms | 1.51 ms |
| Reason | 5.92 ms | 5.92 ms | 8.33 ms | 1.50 ms |

The indexed count took **3.47 ms**; it runs off the UI thread. The opt-in 200k QML
review test passed, with at most 600 rows and no queued provider search. Timings
are machine/load dependent, not new product budgets.

SQLite coverage includes membership/source independence, playlist duplicates,
provider-only saved Tracks, manual/durable/Album occurrence trust precedence,
structured persisted reasons, both sort directions and keyset traversal, filters,
count changes and migration from v25. Qt coverage includes canonical double-click
and context targets, a different Now Playing Track, manual confirmation,
multi-Track removals, viewport/sort/filter/width preservation, count and empty state,
plus existing details resizing, scrolling, diagnostic and Playlist regressions.

The full core test run encounters the pre-existing
`catalog_playlist_identity_membership_reload_and_duplicates` assertion (expects
an empty Artist display, receives `Artist A feat. Artist B`). The identical failure
was reproduced on the unchanged HEAD checkout. The remaining **343 core tests** pass with
that case excluded. The five focused review tests and 48 affected core tests also
pass after the final changes. Eight isolated ordinary Qt regressions, the real
Library review and the 200k review test pass; Spotify adapter tests pass **47**,
and MusicBrainz adapter tests pass **20**. Core and diagnostic strict
Clippy/format checks pass.

## Deferred

No automatic retry on page open, batch retry, new manual association UI, generic
multi-provider review framework, Library reveal action or grouped Artist/Album
ordering was added. Metadata correction suggestions, provider consensus, Artist
credit edits and tag/file rewriting are explicitly deferred. The page is a workflow
around existing conservative association behavior.

The review now includes versioned local-only stale re-evaluation, an explicit bounded Retry unresolved action and a shared Marked Not on Spotify view. Manual marks are excluded from the main query/count. The empty-state wording is now “All eligible Library Tracks are connected or reviewed.” See [current lifecycle semantics and validation](spotify-reconciliation-lifecycle.md); the measurements above describe the original review slice.
