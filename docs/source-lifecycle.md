# Local source availability

A missing or unreadable local file changes its route availability, never Track,
Release, Album, Artist, saved membership, provider identities, playlist entries,
or queue references. Search continues to index effective Track metadata.

The existing `local_file_observation.available` boolean and `last_observed_at`
persist availability. Path, source ID, metadata observations and Track association
remain intact. No migration or filesystem validation during database open is needed.
`local_root_source` records root ownership and each root's last-seen scan generation.

A completed root scan updates unseen sources in that root to unavailable in one
transaction. The `(root_id, source_id)` primary key bounds reconciliation; unrelated
roots remain untouched. Failed traversal, an offline root, or interruption prevents
absence reconciliation. Successfully observed files can still update their own state
in a partial scan. A positively encountered known file that fails observation is
marked unavailable, without changing unseen siblings. Both scanner entry points
check file open and regular-file metadata, including the unchanged-metadata path.
Automatic suppressed files are checked for readability without reparsing or admitting
membership. Metadata parse failure conservatively makes an encountered route unavailable.

Rediscovery of the same path reuses its source ID and Track association and sets
availability to true. Automatic rediscovery never restores removed membership or
clears suppression. Explicit successful re-add retains existing suppression-clearing
and membership-restoration semantics. Root configuration removal retains entities,
source records and membership according to the existing location-removal API.

Identity acceptance now uses preserved file observations even when a route is
unavailable. Availability-only transitions do not reconcile or revoke accepted
identities. Actual retagging still replaces only that file's observations and can
reconcile conflicting or removed claims. Current playable-file edition-completeness
evidence continues to filter unavailable files; it is separate from durable identity.

Playback's shared resolver retains its priority: available readable local routes,
then a persisted valid Spotify occurrence, then existing bounded provider resolution.
It uses Track-scoped indexed association lookups and probes only available candidates.
It tries other local sources if one fails readability. A persisted Spotify occurrence
returns a remote route directly, without requesting provider matching. Known-unavailable
paths are skipped until ingestion observes them again. Queues remain logical Track
references and resolve on the next open operation. Mid-stream handoff, automatic move
detection, source cleanup, watcher monitoring and priority configuration remain deferred.

## Diagnostics

Ingestion reports `unavailable` transitions separately from `unreadable` files and
`locations_failed` (offline or incomplete roots). The diagnostic app shows those
counts and retains the existing playback route explanation. Suppression is a separate
table and must not be inferred from availability. These queries inspect a disposable
or read-only library snapshot without filesystem or provider access:

```sql
SELECT l.source_id, ts.track_id, l.available,
       l.last_observed_at, hex(l.path) AS native_path_bytes,
       x.source_id IS NOT NULL AS suppressed
FROM local_file_observation l
LEFT JOIN track_source ts ON ts.source_id=l.source_id
LEFT JOIN local_source_suppression x ON x.source_id=l.source_id
WHERE ts.track_id = :track_id;

SELECT id, root_id, status, completed_at
FROM scan_run WHERE root_id = :root_id ORDER BY id DESC LIMIT 10;
```

## Validation

`tests/local_source_attachment.rs` covers disappearance, same-source restoration,
restart, preserved identities/membership/playlists/search, separate roots, multiple
local routes, permission denial, partial traversal and suppression. Existing playback
tests verify queue identity/order, fresh resolution and no provider enrichment for a
persisted route. Existing migration and query-plan tests cover availability and
root/path/association indexes.

Real audio is copied from ignored `test-media/Get Disowned/01 Some Grace.mp3` into a
disposable temporary root. The fixture itself is never modified:

```sh
cargo test --offline --test local_source_attachment real_source_disappearance -- --ignored --nocapture
cargo test --offline --manifest-path adapters/gstreamer/Cargo.toml attached_catalog_source_plays_real_audio_and_survives_reopen -- --ignored --nocapture
```

The GStreamer test confirms advancing real playback before disappearance and after
restoration and keeps the queue unchanged. If `MUSIC_LIBRARY_LIFECYCLE_SPOTIFY_PROBE`
points to the Spotify adapter's `playlist_spotify_playback_probe` executable, the same
test also dispatches and verifies Spotify playback during the unavailable phase.
That opt-in check requires an authorized session and an active Connect device.

### Recorded fixture run (2026-10-05)

The actual backend cycle passed using an untouched fixture and disposable copies:

| Phase | Observed backend |
| --- | --- |
| Available | GStreamer, advancing to 296 ms |
| Deleted copy and completed rescan | Spotify Connect device `PUHI`, confirmed playing `38CLjvzuqaIADFFZrThgn5` |
| Restored copy and rescan | GStreamer, advancing to 298 ms |

Track ID: `17771ed3-494d-455c-ab76-c8f188042a67`.
Local source ID: `4455a517-e7cb-492f-851f-f34ea298580b`.
Both remained identical throughout; queue references/order and saved membership
were retained. Reopening during the absent phase still resolved Spotify, and
reopening after restoration resolved the original local source without rescanning.

The existing 200k-source test passed: targeted folder imports took approximately
7–112 ms, one configured-root rescan 106 ms, and standalone-file admission 5 ms.
These are observed timings, not new budgets. Query-plan checks confirm root-indexed
absence reconciliation and Track-indexed playback; no global Track reconciliation
or per-Track network requests were introduced. Availability-only reconciliation
also no longer loads affected domain entities for provenance reevaluation.

Core and GStreamer Clippy checks pass with warnings denied; the QML diagnostic
build passes. The broader core suite has a pre-existing failure in
`catalog_playlist_identity_membership_reload_and_duplicates`: it expects an empty
artist credit but receives `Artist A feat. Artist B`. This reproduces on unchanged
HEAD and is outside this source-lifecycle slice.

Final core run: **330 passed, 15 opt-in tests ignored**, with the one confirmed
pre-existing playlist-credit failure excluded. The focused lifecycle/storage/playback
run passed **65 tests**. Real fixture resolution and actual GStreamer/Spotify/GStreamer
playback were run separately and passed. No commit was created.
