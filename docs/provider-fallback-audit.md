# Ordered catalog fallback

`ProviderChain` accepts ordered `ProviderSlot`s containing an adapter-declared
matching scope and its worker, or an actionable configuration failure. It never
branches on provider names. Diagnostic CLI wiring constructs the concrete adapters.
Single-provider runs retain the existing worker behavior.

One application owner-thread coordinator grants work to one provider at a time.
Each provider keeps its client, memory caches, request limits and circuit. No
concurrent fan-out, per-Track lookup, new schema or cross-provider identity table
is introduced. Album remains the same application object throughout an attempt.

## Selection and fallback

Manual Track associations pin their configured provider. Otherwise, before any
search, the first configured provider with an accepted Album identity wins over
rediscovery. Multiple accepted provider identities coexist. Without a known Album,
configured order controls attempts. No-match, Artist/Album ambiguity, equivalent
but unidentified catalog objects, request errors and temporary unavailability may
fall through. Configuration errors remain explicitly classified and disable further
attempts for that provider instance. Independently valid Artist identity is retained
even if that provider's Album search fails.

Accepted exact/close Album or known Album identity ends fallback. That provider
supplies Track enrichment, even if it has no Recording entity, some Tracks remain
unresolved, or its Track request fails. Track-phase outages preserve the continuation
with that provider and permit other Album jobs to proceed. There is no switch from
Spotify Album identity to MusicBrainz Track enrichment.

Failed provider work is removed from the child worker queue and owned by the
coordinator. Open circuits are skipped until their existing cooldown permits a
probe; another provider may finish the Album in the meantime. Timer callbacks
cannot start requests themselves. One pending job gets the serial probe grant;
normal recovery resumes preserved jobs, while repeated outages retain work and
the existing backoff. A provider recovering after another provider already won
does not enrich that successful Album again. Manual retry uses the same grant.

Per-Album history retains one latest result per configured provider in original
attempt order. It is ephemeral. The QML row shows the winning provider, pending
fallback history and normal expanded local-to-provider Track mappings. Manual
selection and clearing remain scoped to that row's provider. Artist selection is
a local operation whose continuation queues behind active work. No global picker
or provider-ID equivalence is implied.

## Live validation, September 14–15, 2026

All databases below were isolated under `/tmp`; no user library database or local
music file was modified. Spotify used market US. HTTP counts include token
acquisition and bounded retries; elapsed time includes rate waits. Local completion
time measures comparison/persistence and coordinator completion, excluding HTTP.

| Scenario | Database state | Spotify HTTP | MusicBrainz HTTP | Result | Elapsed | Local completion |
|---|---|---:|---:|---|---:|---:|
| Painted Shut, Spotify→MB | fresh | 4 | 0 | Spotify, 10/10 | 0.810 s | 3.185 ms |
| Trash Generator, Spotify→MB | fresh normal import | 0 | 4 | Known MB from embedded provenance, 12/12 | 3.228 s | 7.513 ms |
| Trash Generator discovery control, Spotify→MB | fresh, accepted Album mappings suppressed only in disposable DB | 4 | 0 | Spotify, 12/12 | 0.746 s | 3.924 ms |
| Hella / Bitches Ain't Shit But Good People, Spotify→MB | fresh | 5 | 0 | Spotify, 4/4 | 1.445 s | 1.586 ms |
| Wretches, Spotify→MB | fresh | 3 | 7 | Spotify no-match → MB, 3/3 Recording identities | 7.364 s | 1.370 ms |
| Wretches, MB→Spotify | fresh | 0 | 7 | MB, 3/3 | 6.601 s | 1.393 ms |
| Wretches, Spotify→MB | reopened successful fallback DB | 0 | 2 | Known MB, 3/3 preserved | 1.177 s | 1.451 ms |

Normal Spotify success uses token, Artist search, Album search and one Tracks page.
Hella additionally uses its bounded empty-title discovery fallback. Wretches used
Spotify token/Artist/Album search, then MusicBrainz Artist search, Release Group
search, representative browse and one representative program lookup. Three MB
operations returned 503 then succeeded on their bounded retry, giving seven actual
HTTP requests. No acceptance rule was changed. The reverse-order control made no
Spotify request, including no token request. Reopening Wretches used only MB browse
and program lookup, with neither provider's Artist/Album search.

Trash Generator illustrates why a fresh database is not sufficient to guarantee
discovery: its tags automatically establish an accepted MB Album identity during
import. Reuse is required by the product policy. The separate opt-in
`--discovery-control` probe suppresses only accepted Album mappings after import in
a **new `/tmp` database**, retaining raw observations and leaving files unchanged.
This artificial control verifies Spotify-first discovery without weakening normal
provenance acceptance or existing-identity precedence.

Every run closed/reopened the database and compared Album identities, local
Track/Recording evidence and Spotify song associations without file or network
access during reconstruction. The separate Wretches process restart also exercised
normal unchanged-source scanning. No duplicate Albums/Tracks or exact Release IDs
were introduced.

## Reproduction

With Spotify credentials/market configured in the environment:

```sh
MUSIC_LIBRARY_DIAGNOSTIC_DATABASE=/tmp/music-library-provider-fallback.sqlite \
  cargo run --offline --manifest-path tools/qml-diagnostic/Cargo.toml --features gstreamer -- \
  --catalog-providers spotify,musicbrainz --gstreamer /mnt/f/music
```

For a bounded live probe, choose a new database path for each fresh scenario:

```sh
MUSIC_LIBRARY_CATALOG_TIMING=1 MUSIC_LIBRARY_DIAGNOSTIC_DATABASE=/tmp/fallback-wretches-test.sqlite \
  cargo run --offline --manifest-path tools/qml-diagnostic/Cargo.toml --example provider_chain_probe -- \
  '/mnt/f/music/Hop Along/Wretches' spotify,musicbrainz
```

Reverse the comma-separated order for the reverse test; reuse the same database
only for an intentional restart/known-identity test. The probe stops once only
deferred work remains rather than retrying a live outage indefinitely. No token or
secret is logged. Deterministic core tests exercise automatic cooldown and retry.

Deferred: querying every provider after success, cross-provider Track/Recording
reconciliation, Spotify-song-to-MusicBrainz-Recording equivalence inference, and
metadata-only entity merging. Future providers can participate by supplying the
same Album/program capability without manufacturing an exact-edition hierarchy.

## Local cost and validation

Selection adds bounded accepted-identity/manual-association reads for the affected
Album, using existing indexed storage APIs. There is no global reconciliation or
per-Track discovery. The 200k fixture retained indexed Album lookup (median
0.012 ms), 10-Track verification (0.037 ms) and first library page (0.323 ms).
These host-dependent timings remain small compared with provider latency; storage
SQL and indexes were not changed by fallback.

Validation passed 209 core tests (including parameterized chain tests), nine
Spotify HTTP mocks, 19 MusicBrainz tests, five GStreamer tests, and five QML tests.
Chain tests cover early success, bounded misses/ambiguities, unavailable/configured
failures, independent recovery, ordering, known identities, manual provider pins,
Recording persistence, partial Albums, unresolved Tracks, restart and FIFO Album
completion. QML tests exercise clearing a manual match on a non-first provider,
retained scrolling/expansion and asynchronous updates. Both chain orders and both
single-provider modes pass smoke checks. Formatting, strict Clippy, QML lint and
diff checks pass. The pre-existing Qt timer teardown warning remains in the test
output; no new runtime test failure remains.
