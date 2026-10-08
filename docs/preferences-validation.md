# Hidden Artists and ignored Songs validation

Implemented without committing. Migration 0032 adds profile-keyed hidden-Artist and
ignored-Track associations; canonical entity tables and metadata are unchanged.
The current preference owner is `local`. No accounts or independent Album/Artist
playback-ignore rules were added.

## Behavior

- Main Artists browsing omits hidden canonical Artists. Credits, search, explicit
  navigation, playback, source resolution, and matching keep their existing behavior.
  Search navigation can expose the referenced hidden Artist; clearing that selection
  restores passive browsing. Context menus switch between Hide and Unhide.
- ⋯ → Hidden Artists lists hidden canonical Artists with an Unhide button. ⋯ →
  Ignored Songs lists Song, Artist, and Album with an Unignore button. Both use
  bounded keyset pages (200 displayed rows, 201-row probe), with First/Next actions.
  Ignored Songs includes canonical Tracks outside Library membership.
- Ignore song targets the clicked canonical Track, including Playlist rows whose
  row identity differs from their Track identity. Ignore Album/Artist runs one
  transactional bulk insert/delete over current Tracks. Artist scope uses the
  established union of Track, Release, and Album credits, including secondary credits.
- Album/Artist appearance is derived from the current canonical Track set. Fully
  ignored nonempty objects and ignored Song rows use reduced opacity and retain
  selection/context interaction. Partial and empty groupings remain normal. New
  Tracks after bulk ignore remain eligible; unignoring one Track clears full state.
- The existing in-memory canonical queue retains ordering and adds one
  `QueueIntent` per occurrence. Explicit Track selection/enqueue permits the entry;
  expanded contexts are generated. Choosing one Playlist occurrence does not permit
  other generated ignored occurrences of the same Track. Queue persistence does not
  exist and was not added.
- Background queue preparation filters ignored Tracks for Album, Artist, Playlist,
  and Songs snapshots. Explicit multi-selection remains allowed. Next/Previous/EOS
  recheck preferences in bounded batches; the mixed-backend controller rechecks after
  asynchronous stop and before starting a prepared program. Current playback is not
  stopped by an ignore change. An entirely ignored context clears the playback queue.

## Tests and desktop validation

Passed focused backend suites: `preferences` (6), `playback` (22), `browse` (9),
`playlists` (9), `library_search` (7), `canonical_reconciliation` (10), and
`spotify_lifecycle` (22). The latest preference/playback tests were rerun after
adding bounded stale-queue traversal.

Passed Qt/QML tests, built with the `gstreamer` feature:

- `library_preferences_context_managers_and_generated_playback`
- `multi_selection_and_pane_local_container_actions`
- `library_removal_confirmation_and_queue_snapshot`

The preference workflow also ran on the actual XCB desktop with GStreamer and
WSLg PulseAudio against a disposable SQLite database and generated silent WAV
sources. QtTest exercised menus, the manager restore button, search navigation,
ignore/unignore operations, queue construction, and explicit ignored-Song playback.
The test confirmed GStreamer's Playing state and local source resolution. This was
agent-driven desktop automation, not a human listening test or validation against
the user's live collection. A screenshot was inspected at
`/tmp/music-library-preferences-ui.png`.

Backend tests cover preference restart durability, secondary-Artist hiding without
credit changes, independent membership/search/playlists, bulk snapshot semantics,
new Tracks, empty groups, explicit/generated occurrences, stale Next/Previous,
current-playback preservation, manager keysets, and 600-row scrolling batches.
Existing matching/reconciliation and source-resolver suites passed.

`cargo check` for the desktop tool with GStreamer, formatting checks for both
manifests, and `git diff --check` passed. The desktop tests emit the existing Qt
shutdown diagnostic about timers from another thread after passing.

## 200k measurement

`examples/preferences_performance.rs` ran in release mode on a SQLite backup of the
repository's deterministic 200k fixture at `/tmp/preferences-200k.sqlite`.
Measurements below are single warm-process observations, not percentile budgets.

| Operation | Result | Time |
|---|---:|---:|
| Artists with hidden filter | 200 rows | 1.17 ms |
| Albums browse + fully ignored derivation | 200 rows | 11.82 ms |
| Artists browse + fully ignored derivation | 200 rows | 24.45 ms |
| Ignored Songs manager | 200 rows | 3.86 ms |
| Hidden Artists manager | 1 row | 0.35 ms |
| Bulk Artist ignore + count verification | 20,000 Tracks | 33.54 ms |
| Generated Artist queue | 0 eligible of 20,000 | 183.34 ms |
| Generated full Songs queue | 180,000 eligible of 200,000 | 2,409.20 ms |

Presentation queries use batches of at most 200 targets, including 600-row scrolling
windows, rather than per-row database calls. Traversal also batches preference reads.
Complete queue snapshot construction remains proportional to its result size and
runs in background workers. Managers do not materialize the full Library.

## Deferred

Shuffle, ratings/favorites, smart playlists, permanent Album/Artist ignore rules,
automatic ignoring of future Tracks, per-device preferences, accounts, and
provider-specific ignore behavior remain unimplemented. Manager sorting/filtering
and partial-state decoration were optional and were not added. Identity, metadata,
source priority, provider matching, reconciliation, and PlaylistEntry semantics were
not redesigned.
