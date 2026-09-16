# Spotify catalog matching

The catalog client uses Spotify's documented Web API. It does not provide
user-library access or Catalog **Add Album**. A separate optional
[user playback module](PLAYBACK.md) uses PKCE and Spotify Connect; its authorization
is independent from this catalog Client Credentials flow.
The diagnostic's existing MusicBrainz catalog-add button is disabled in Spotify mode.
Local import and GStreamer playback are unchanged.

Create an application in the [Spotify developer dashboard](https://developer.spotify.com/dashboard).
The adapter uses [Client Credentials](https://developer.spotify.com/documentation/web-api/tutorials/client-credentials-flow):
an in-memory bearer token with expiry margin (up to 30 seconds), and at most one
token refresh/retry after HTTP 401. Tokens and credentials are never stored in
SQLite or included in diagnostic formatting. Configure credentials in the environment;
avoid committing environment files or enabling shell tracing.

```sh
export SPOTIFY_CLIENT_ID='your-client-id'
export SPOTIFY_CLIENT_SECRET='your-client-secret'
export SPOTIFY_MARKET='US'
export MUSIC_LIBRARY_DIAGNOSTIC_DATABASE='/tmp/music-library-spotify.sqlite'
cargo run --offline --manifest-path tools/qml-diagnostic/Cargo.toml --features gstreamer -- --catalog-provider spotify --gstreamer /mnt/f/music
```

Run from the repository root. Market is mandatory and must be a two-letter country
code; the adapter never assumes global availability. No user market exists in this
authentication flow. Use the same database path when relaunching. To compare runs,
replace `spotify` with `musicbrainz`; accepted identities coexist. Use
`--catalog-providers spotify,musicbrainz` for ordered fallback. A successful Album
stays with its provider for Track enrichment; no secondary enrichment is performed.
An already accepted configured Album identity takes precedence over rediscovery.
See the [fallback audit](../../docs/provider-fallback-audit.md).

Open **Local Album matches…**, check the provider in its title, and expand an Album.
Both local and provider Track titles appear, including identical names. A matched
Spotify row says **provider song identified**; missing Recording identity is normal.
Use **Choose Match** for unresolved Tracks, explicitly select/confirm, restart and
verify the manual choice; **Clear** affects only the selected provider's choice.
Persistent automatic song associations also display offline. Matching never creates
missing local Tracks or writes exact-edition identities.

## Discovery and bounds

Cold unknown Artist/Album normally uses one token request, one Artist search, one
Album search, then one Album Tracks page for up to 50 entries. Token acquisition is
separate from catalog work. An ambiguous Artist set uses one combined Album search,
then filters returned Artist IDs; there is no per-candidate discography enumeration.
Spotify has no `arid` query: cached Artist names construct targeted searches and
returned IDs enforce scope. A persisted Artist with no cached name needs one Artist
lookup before Album search. A persisted Album skips both searches and directly
retrieves its program. The last four complete program sets and 64 Artist names are
session-cached. Cached manual selection requires zero additional requests.

Searches use only the first 10 results; truncation leaves automatic decisions
conservative. Exact local Album title comparison still precedes controlled variants
and existing small typo tolerance. Candidate discovery may use a controlled base
title to find decorated titles. Spotify's `single` category does not prove EP type;
EP-only fallbacks may therefore remain unresolved. Catalog duplicates can remain
unresolved rather than choosing a score/order winner. Returned `total_tracks` first
excludes structurally impossible objects; `single` alone is not a rejection. If
necessary, the generic matcher retrieves at most three candidate programs and
compares only present local Tracks. One clear program winner may supply the Album
identity, never exact edition. Programs are reused immediately for enrichment and
manual correction. Equivalent representations show diagnostic Album/Track support
without arbitrarily persisting an Album ID; these unresolved associations are ephemeral.

An empty multiword full-title search may make one additional Artist-scoped search
using its longest alphanumeric token (minimum four characters). This handles an
observed Spotify indexing/query failure; complete original titles and returned Artist
IDs still undergo unchanged acceptance. Truncated pages never trigger this fallback.
Single-word titles and successful normal searches cost no additional request.

[Album Tracks](https://developer.spotify.com/documentation/web-api/reference/get-an-albums-tracks)
uses 50-entry pages, capped at 20 pages/1,000 entries. Incomplete results cannot
auto-match. Pagination follows validated offsets on the configured endpoint, never
arbitrary response URLs. No full Track or per-local-Track requests are made. ISRC is
retained only if present in an already-required response; it is not required.

Artist and Album IDs are canonical provider mappings onto the corresponding
application entities. Track IDs are persisted in the generic Album-scoped Track
association snapshot, never on Recording or as an exact Release Track claim.
The application already creates provider-neutral Recordings for all Tracks; Spotify
adds no external Recording identity. Local metadata is preserved.

## Errors and access limitations

Spotify has no MusicBrainz-style mandatory one-second spacing. Calls are serialized
by the selected matching worker. HTTP 429 (including `QUOTA_EXCEEDED`), 5xx and
transport/timeouts pause that worker and retain its Album job. Existing cooldowns
are 15/30/60/120 seconds, capped at 120; positive Retry-After may increase the delay
up to that cap. No extra HTTP retry loop is added beyond one 401 token refresh.
HTTP status, safe message/reason and Retry-After remain diagnostic. Invalid credentials
and 403 access failures are configuration errors, cached for this adapter lifetime
to avoid repeated invalid requests; correct configuration and restart.

The [February 2026 migration guide](https://developer.spotify.com/documentation/web-api/tutorials/february-2026-migration-guide)
documents Development Mode's small search limit, removed bulk endpoints and Premium
app-owner requirement. This adapter uses single-object endpoints only. The newer
[July 2026 quota update](https://developer.spotify.com/blog/2026-07-23-web-api-quota-updates)
documents per-developer shared quota and structured quota errors. Access and catalog
availability can prevent a live match even when deterministic adapter tests pass.

## Validation and timing

```sh
cargo test --offline --manifest-path adapters/spotify/Cargo.toml
cargo test --offline --test selected_provider
```

Tests use localhost HTTP mocks and synthetic provider programs, never credentials
or live Spotify. Set `MUSIC_LIBRARY_CATALOG_TIMING=1` for safe endpoint timing; token
acquisition has its own timer. No token/secret is printed. Live timing depends on
network, Spotify quota and app access; it is not local import latency.

Local release-build measurements (2026-09-14, this development host): one
occurrence-only program compared against 5/15/30 Tracks averaged
0.024/0.204/0.818 ms. Loading, comparing and transactionally writing three local
associations averaged 0.393 ms; reconstructing those associations averaged 0.019 ms.
The 20k-Album/200k-Track fixture retained indexed matching (Album candidate lookup
median 0.006 ms, first 50 library rows 0.179 ms). These exclude network/token time.
Live validation in market US subsequently matched Hop Along's **Painted Shut**:
10/10 local Tracks gained Spotify song associations, with no Recording or exact
edition identity. Cold token acquisition took 149.6 ms; Artist search 471.7 ms;
Album search 348.6 ms; the single Album Tracks page 114.1 ms. The workflow took
1.092 s. A fresh process loaded all 10 associations before any network request;
it then used only a new token (108.2 ms) and one program page (253.4 ms), completing
in 0.367 s. All responses were HTTP 200; no Development Mode restriction appeared.

The subsequent [candidate audit](../../docs/spotify-album-candidates-audit.md)
resolved Trash Generator's 12-track Album versus one-track single using cheap counts
(12/12 Tracks), and Hella's empty full-title search using bounded token discovery
(4/4 Tracks). Wretches still has no usable returned candidate; this does not prove
global catalog absence. Painted Shut remains the 10/10 control. No acceptance
thresholds, Artist confidence rules, or exact-edition semantics were loosened.

The opt-in live probe reuses the diagnostic importer and application worker. It
stops on provider errors and checks database close/reopen without more network:

```sh
MUSIC_LIBRARY_CATALOG_TIMING=1 MUSIC_LIBRARY_DIAGNOSTIC_DATABASE=/tmp/spotify-live.sqlite \
  cargo run --offline --manifest-path adapters/spotify/Cargo.toml --example local_match_probe -- \
  '/mnt/f/music/Hop Along/Painted Shut'
```

Use a separate diagnostic database. Relaunch with the same path to inspect persisted
associations before provider requests. Credentials still come only from the environment.
