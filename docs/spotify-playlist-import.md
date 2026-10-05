# Spotify playlist snapshots

Playlists → **+** → **From Spotify** opens a searchable connected-account list or
accepts a normal `https://open.spotify.com/playlist/...` link, optional share query,
localized Spotify link, `spotify:playlist:...` URI, or a 22-character Spotify ID.
Names, owners and available item counts identify account playlists. Selecting a
row imports it; pasted links use Import. Progress and results stay in the dialog.
The existing Songs table, display sorting, resizable columns, absolute position,
Length/Details and playback interactions remain unchanged.

## Authorization and remote reads

The existing separate user authorization requests
`user-read-playback-state user-modify-playback-state playlist-read-private
playlist-read-collaborative`. PKCE S256, exact loopback callback/state validation,
secure credential storage, token refresh and playback scopes are retained. Granted
scopes are now saved. Legacy credentials without recorded playlist scopes are
usable for playback, but playlist operations request reconnection. The dialog
retains the failed browse/import request and resumes it after successful user
authorization. No catalog Client Credentials or playlist-modification scopes are
used for playlist reads.

The Spotify adapter translates JSON into a provider-neutral import plan. Both
`GET /me/playlists` and `GET /playlists/{id}/items` read all pages. The latter uses
the current `item` response field and tolerates older response shapes without
using the deprecated playlist `/tracks` endpoint. Pagination validates the API
origin, endpoint and advancing offsets. Account totals/offsets can include
inaccessible or deleted playlists omitted from returned rows, so account browsing
follows every next page rather than treating totals as a visible-row count. Item
pages additionally validate contiguous offsets and the final total. Tokens are never
sent to response-provided hosts. All useful Track metadata comes from playlist
items; no per-Track metadata requests occur. A final snapshot-id read detects
edits during pagination instead of persisting a mixed remote snapshot.

Malformed links, inaccessible playlists, missing playlists, authorization failures,
network/service failures and rate limits have separate messages. Existing refresh
and Retry-After cooldown behavior applies. Restrictions are honored; no scraping
or fallback access mechanisms exist. Ownership metadata never gates browsing or
import. The app attempts the official playlist API for every selected or pasted
source and trusts the actual response. Followed non-owned playlists can work.
A scoped grant receiving 403 gets an access explanation and a suggestion to try
following a public playlist; this does not guarantee access or trigger reconnect.
401 uses the normal token refresh/reauthorization path; 404 remains a distinct
unavailable/not-found result. The dialog remains usable after a failed source.

Episodes, unknown content types and Spotify local items are unsupported. Null,
deleted, malformed and explicitly unplayable/restricted music items count as
unavailable. They are skipped individually; valid remaining items preserve order.
The result reports imported Tracks and nonzero skipped counts. Invalid pagination,
access errors or a changed snapshot fail the remote stage without persistence.

## Persistence and ownership

Migration 23 adds `playlist_source`: provider, external playlist ID, source URL,
snapshot/version and owner, with a cascading reference to the independently
generated app playlist ID. Migration 24 keeps one provenance record per local
playlist/provider and indexes provider/external ID without making it unique:
deliberately renamed copies can reference the same Spotify source. The playlist name is
imported. Description/artwork are deliberately omitted.

`Library::resolve_playlist_import` persists an explicitly resolved, fully staged
plan in one transaction,
including any new canonical entities, metadata, duration observations and entry
records. No network or filesystem probes run inside that transaction. The complete
fetch and persistence phase run on the existing user-API worker, away from QML;
progress arrives per remote page and before persistence, not per Track. The view
refreshes and selects the app playlist after success.

Identity resolution batches accepted external identities, explicit manual choices
and established provider-program associations. Manual/accepted identity precedence
matches the shared playback resolver. A uniquely trusted Spotify Track ID reuses
its existing canonical Track, preserving saved membership, metadata, overrides,
local sources and other identities. Conflicting existing canonical associations
fail safely for explicit reconciliation. Names, credits, duration and album text
never establish reuse.

Unknown Spotify Track IDs create opaque application Track/Recording identities in
the existing Spotify catalog Release representation, with typed initial title,
ordered artist credits, album/date/disc/Track metadata and trusted Spotify Track
and Album identities. Spotify identity remains provider evidence, not an internal
primary key or a claim equating Spotify Album identity with a MusicBrainz edition.
Exact existing Spotify Release identity is reusable; conflicting Release identities
fail conservatively. New duration observations use the shared effective-metadata
path, preserve existing same-source evidence, and are available before playback.
User overrides and local metadata precedence remain intact. Approximate/partial
Length and Details behavior remains shared.

The importer never inserts Library membership. New Tracks are visible inside the
playlist but contribute no saved Songs, Albums, Artists or Genres. Reused saved
Tracks stay saved. Each occurrence gets its own entry UUID, including duplicates.
Playlist playback uses the usual app-authoritative canonical ordered snapshot and
shared resolver: readable local sources retain preference; Spotify-only Tracks
resolve through persisted Spotify associations without requiring catalog access.
Import does not change the active queue. No Spotify audio is decoded.

After the complete remote snapshot is fetched, title collisions and exact Spotify
source collisions offer **Rename incoming / Overwrite existing / Cancel**.
Title comparison is case-insensitive; title is not identity. Rename requires a
non-empty usable unique local title, generates a new app playlist ID and retains
the incoming Spotify provenance. Cancel writes nothing. Overwrite requires exactly
one matching target; ambiguous collisions require Rename instead of choosing a
row arbitrarily. The persistence transaction rechecks collisions, replaces entries
and provenance, and preserves the existing app playlist ID and local title.
Remote failures leave local contents untouched; any database error rolls back the
whole replacement. Existing Track membership, sources, identities and metadata
are preserved; removing entries does not delete Tracks. Neither local edits nor
explicit overwrite send Spotify modifications. These operations are snapshots,
not synchronization. The older idempotent core import helper remains available to
noninteractive callers; the interactive flow uses explicit conflict decisions.

## Table behavior

The general browser refresh formerly reset the selected playlist's window and
scroll anchor after matching/metadata completions. `LibraryPane.syncRows` then
assigned a null model, cleared it and rebuilt all delegates, including for metadata
changes. Playback polling and clock notifications themselves do not reload the
browser. Playlist enrichment now reloads only the bounded existing window on the
worker, locating its first entry by stable identity and current sort cursor. The
old rows remain displayed while the read runs. Matching ordered entry IDs are
patched with `ListModel.setProperty`, without replacing the model or delegates.
Actual membership/order changes retain the structural refresh and entry anchor.
A deliberate overwrite may reset to the beginning.

Main Songs and Playlist detail columns resize independently, including their last
column. Only the dragged width changes; total content width changes and overflow
scrolls horizontally. Thin dividers retain wide resize targets; headers and cells
share the same width values, and dragging does not sort or call the bridge.
Adjusted widths remain separate in window state across view navigation for the
session. Existing column schemas and outer SplitView pane behavior are preserved.
The **+** chooser invokes the existing New and Spotify dialogs.

## Validation

Focused SQLite tests cover a 20-Track import with zero Library memberships and all
four Library panes empty, immediate durations, complete order, separate duplicate
entries, trusted identity reuse (direct/manual/accepted-program), existing saved
and local sources, resolver preference, conservative identity conflicts, restart,
offline reads, local edits, duplicate-source protection, queue snapshots and rollback
of all writes after an injected entry failure. Migration tests cover provenance
rollback and update older-schema fixtures for migration 23.

Mock HTTP tests cover parser boundaries, read scopes and legacy grants, token
refresh/restart, account/item pagination, current endpoint/response shape, order,
duplicates, metadata/duration, unsupported/deleted/malformed items, access/not-found
errors, remote failure mid-pagination, Retry-After, incomplete totals and changed
snapshots. Request assertions require only GET playlist reads and no Track lookup
or playlist modification requests.

The actual offscreen QML dialog test covers browse, filtering, malformed paste,
import progress/results, missing-scope reconnection/resume, selected imported rows,
duplicates, Length rendering, Library counts and active queue preservation. Explicit
Play builds the normal canonical playlist queue after changing display sorting.
Existing playlist/table/column/catalog/context-action checks run unchanged.

For a real accessible playlist, use the opt-in disposable test:

```sh
export MUSIC_LIBRARY_SPOTIFY_TEST_PLAYLIST='https://open.spotify.com/playlist/...'
cargo test --offline --manifest-path adapters/spotify/Cargo.toml \
  --test playlist_live -- --ignored --nocapture
```

It uses the existing connected user's authorization, creates only a temporary
library, checks order/identity/resolver/Library separation, then drops Spotify and
reopens the database to validate persisted offline metadata and re-import handling.
Reconnect in the app first when playlist scopes are absent. An explicit
`MUSIC_LIBRARY_SPOTIFY_TEST_AUTHORIZE=1` enables the same PKCE flow in this opt-in
test and prints a browser authorization link; it is never enabled in normal tests.
The account owner must approve scopes. Real audio/device playback and reuse of a
specific local Track also require the corresponding user account/device/library;
fixture resolver coverage is separate from live audio verification.

Automatic refresh/synchronization, write-back, cross-service migration, artwork,
column settings across restart and broader playlist redesign remain deferred.

### Validation on 2026-10-01

Core: 312 passed, 13 opt-in checks ignored. Spotify adapter: 44 passed, two opt-in
checks ignored in the regular suite. All 21 non-live QML checks passed in isolated
offscreen software-rendered processes, including the existing playlist table,
column resize, sorting, Details geometry, selection and queue checks. Strict Clippy
passed for core, Spotify and all-feature QML targets; formatting and whitespace
checks passed. The focused import test passed again after the final changes.

The account owner approved the added read scopes through the existing PKCE flow.
The supplied real playlist `72h7TZy9CgelSlzzSBFYmc` ("mally") imported through both
the adapter and actual QML dialog: 198 remote items across all pages, 197 imported
Tracks, one unavailable item skipped, no unsupported items, and no duplicate Track
occurrences in this particular playlist. All 197 durations were immediately known;
the total was 37,556,229 ms. Account browsing showed all 38 returned playlists;
Spotify reported total=39 with 38 returned rows and next=null, motivating the
filtered-total pagination regression rather than inventing an extra accessible
playlist. The UI screenshots are `/tmp/spotify-playlist-import-live.png` and
`/tmp/spotify-playlist-table-live.png`.

Real-service checks confirmed exact persisted order/identity, zero Library
memberships and empty Artists/Genres/Albums/Songs panes, unchanged active queue,
restart/offline metadata, shared Spotify resolver eligibility and duplicate-source
protection. Repeated occurrences, local-source preference and reuse of an existing
saved/local Track were validated with fixtures. No audio was sent to a real Spotify
device, and reuse of a particular song in the user's real local library was not
manually exercised. No commit was made.


## Follow-up validation (2026-10-05)

The read-only live access audit returned 38 account playlists. Owned
“my sonfs (organized) (2)” imported 56 Tracks; followed non-owned “🎶🕑💥🚗 🚙🕑”
imported 25 Tracks. Account-listed “AF4” returned 403, with the access/follow
suggestion rather than reconnection. A pasted “Today's Top Hits” ID
`37i9dQZF1DXcBWIGoYBM5M`, absent from account browsing, actually returned 404;
it was attempted and reported as not found. No follow or playlist modification
requests were made.

The real QML mally audit imported 197 Tracks with durations, scrolled to entry 81,
and retained the same model, delegate, viewport and selection for 30 seconds while
real Spotify polling and clock updates ran. Removing a local entry then explicitly
Overwriting restored 197 entries under the same local playlist ID. A subsequent
Rename imported a second local snapshot with Spotify provenance. Offline reopening,
empty Library panes and zero memberships were checked again. Pointer-driven column
resizing, header sorting, the + chooser, conflicts, rollback, simulated playback
updates and metadata/duration enrichment are covered by QML/core/adapter tests.
This headless audit did not send real-device play/pause/seek commands or match a
particular Track in the user's real local library; local source preservation and
resolver preference are covered by temporary-library tests.
