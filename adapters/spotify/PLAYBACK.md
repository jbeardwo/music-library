# Spotify user playback diagnostic

Catalog authentication and playback authentication are independent. `Spotify`
uses Client Credentials for catalog matching; `playback::Playback` uses a separate
user token to control an official Spotify client through Spotify Connect. It never
decodes Spotify audio or sends Spotify URIs to GStreamer. No Web Playback SDK is
used. Premium/playback capability and Development Mode access restrictions still
apply; an accepted catalog song identity is not a guarantee of playable access.

## Setup and authorization

In the existing [Developer Dashboard](https://developer.spotify.com/dashboard) app,
register exactly:

```text
http://127.0.0.1:43821/callback
```

Keep `SPOTIFY_CLIENT_ID` configured. Playback does **not** read
`SPOTIFY_CLIENT_SECRET` or `SPOTIFY_MARKET`. Those remain catalog configuration.
The [PKCE S256 flow](https://developer.spotify.com/documentation/web-api/tutorials/code-pkce-flow)
requests only `user-read-playback-state user-modify-playback-state`. A cryptographic
random verifier and state exist only during the ten-minute authorization attempt.
The callback binds only IPv4 loopback, validates state and path, and exchanges the
code without a client secret. It does not log requests, codes, verifiers or tokens.

From the repository root (choose a disposable database, never reset your library):

```sh
export MUSIC_LIBRARY_DIAGNOSTIC_DATABASE=/tmp/music-library-spotify-playback.sqlite
export MUSIC_LIBRARY_SPOTIFY_PLAYBACK_CREDENTIALS=/tmp/music-library-spotify-playback-auth/session.json
cargo run --offline --manifest-path tools/qml-diagnostic/Cargo.toml --features gstreamer -- \
  --catalog-provider spotify --gstreamer '/mnt/f/music/Hop Along/Painted Shut'
```

Allow catalog matching once. Subsequent launches reuse the same database. Open
**Spotify Playback… → Connect Spotify Playback**. The UI attempts to open the
normal browser; **Open authorization in browser** retries that action. The URL is
also selectable for manual opening when WSL browser integration fails.
If the Windows browser cannot reach WSL's loopback-forwarded port, use a browser
that can reach the listener; do not change it to a public/wildcard bind or share
the callback URL containing the authorization code.

If an SSH shell inherited a stale `DISPLAY=localhost:10.0`, run the diagnostic
with `DISPLAY=:0 QT_QPA_PLATFORM=xcb QT_QUICK_BACKEND=software
PULSE_SERVER=unix:/mnt/wslg/PulseServer` on WSLg. These are launch-environment
overrides, not application defaults. See the [live validation audit](../../docs/spotify-playback-audit.md).

Open Spotify desktop on this computer. Refresh devices and explicitly choose its
name/type. No device is automatically selected, including after restart or stale
device errors. Spotify does not supply a trustworthy universal same-computer ID;
hostname matching is intentionally absent. Restricted devices cannot receive
commands. If no devices appear, open/make the desktop Spotify client available.

In the library results, **Spotify…** selects that application Track without
starting audio or searching the catalog. **Play / Resume** sends its persisted Spotify song URI to
the selected device. Missing/ambiguous associations produce **NoAssociation**,
without a catalog request. Manual song associations take precedence. Play observes
current state first: the same song on the selected device resumes, otherwise it
sends exactly one URI. Pause and seek explicitly target that device. No Album
context, queue entries, shuffle or repeat changes are sent.

## Explicit resolution of catalog-added Tracks

A Track added through MusicBrainz or another catalog need not have a local file
or a Spotify Album identity. Select it with **Spotify…**, then explicitly click
**Search Spotify for this Track**. This uses the existing Spotify catalog client,
Client Credentials token, market configuration, query escaping and error handling.
Playback OAuth is neither used nor required to search.

Discovery is one `GET /search?type=track&limit=10&offset=0` using quoted `track:` and
`artist:` terms from current effective Track metadata. Release Artist credit is
used if Track Artist credit is absent. Local Album text is retained for context;
it is not an exact-edition filter. No automatic pagination, per-candidate lookup,
or alternate-query retry is added. Extra results beyond the first page are reported
as a bound, not silently enumerated. Artist/title metadata is not rewritten.

Choose explicitly from titles, Artists, Album names, dates, durations, disc/Track
positions and diagnostic IDs; then click **Confirm Spotify association**.
Identical Spotify IDs are deduplicated, while distinct catalog song IDs remain
separate choices. No candidate is selected automatically. **Cancel selection**,
closing the panel or selecting another application Track invalidates the pending
choice; a late HTTP reply cannot attach anything. Playback polling does not reset
the selected candidate or trigger another search.

Confirmation attaches `spotify | track | <song ID>` to the existing application
Track through `track_external_identity`, as independently accepted evidence.
It does not establish a Spotify Album, Release or Recording identity. Existing
MusicBrainz identities remain intact. The operation rechecks local metadata and
existing associations in a short transaction. No network or file read occurs in
that transaction. No schema migration is needed.

The playback lookup gives existing manual Album-track choices precedence, then
independently accepted Track mappings, then automatic provider-program mappings.
An existing association disables new resolution; replacing/clearing this new
independent mapping is deliberately not supported by this chooser. Automatic
matching and rescans cannot overwrite it. After confirmation, **Play / Resume**
works immediately; after restart the persisted mapping is used with zero search.
Provider display metadata from the search is ephemeral; the accepted URI remains
visible offline.

The panel reports **Explicit catalog resolution: token / API requests** separately
from playback counters. First search normally needs one Client Credentials token
request plus one search. Further explicit searches reuse that worker's in-memory
token. Temporary failures honor a bounded cooldown before another explicit retry;
there is no background resolution loop. Polling always issues zero catalog requests.
Ordinary Play may search only under the bounded resolver policy below. Catalog
configuration failures are separate from playback auth.

## Storage and lifecycle

Default credential path:
`$XDG_STATE_HOME/music-library/spotify-playback.json`, or
`$HOME/.local/state/music-library/spotify-playback.json`. The optional
`MUSIC_LIBRARY_SPOTIFY_PLAYBACK_CREDENTIALS` overrides the **path**, not token values.
New credential directories use mode 0700 and files 0600 on Unix. Writes use a
random create-new temporary file, fsync and atomic rename. Existing Unix files
with group/other permissions are rejected. This is diagnostic plaintext secret
storage protected by filesystem permissions, **not** a production credential vault;
a production frontend should use the platform credential/keychain service.
Do not place the file in the repository, shared folders, normal diagnostic logs,
or library backups. Tokens never enter SQLite or external-identity tables.

The file stores client ID, access/refresh tokens, access expiry and original
authorization time. No PKCE verifier is persisted. Access expiry refreshes with a
small margin; omitted replacement refresh tokens preserve the old refresh token.
One 401 permits one refresh/retry, then reauthorization is required. Spotify's
[six-month refresh-token lifetime](https://developer.spotify.com/documentation/web-api/tutorials/refreshing-tokens)
starts at original authorization and is not renewed by access refresh. The server
is authoritative for expiration/revocation (`invalid_grant`); that response clears
the stored session and stops refresh attempts. Reconnect explicitly to authorize
again. Catalog credentials, catalog identities and local playback are unaffected.

Device IDs are ephemeral and rediscovered. A stale-device response clears the
selection and fetches devices once; it does not replay a command on another device.

## State, bounds and errors

Only the separate worker performs OAuth/HTTP. While the playback dialog is open
and connected, polling normally runs every five seconds, with a first follow-up
about one second after a command. Closing the dialog stops polling, not Spotify
audio. Reopening rediscover devices. No polling occurs for catalog-only work or
disconnected authorization. External playback changes are observed without trying
to restore our last requested Track.

429 honors Retry-After (30 seconds if absent); 5xx/transport failures defer at least
30 seconds. Requests time out after 15 seconds; there is no retry fan-out or tight
loop. Credential/scope/capability/API rejection stops automatic polling until
explicit interaction. Typed playback errors are separate from catalog match
outcomes. HTTP counters count playback token and API requests separately; no
payload/header logging is enabled. Polls and explicit controls are sequential.

`spotify_playback_probe` is an opt-in interactive backend diagnostic using the same
adapter and persisted associations. It lists at most 100 library Tracks, requests
user authorization if necessary, lists devices, then waits for explicit commands:

```sh
cargo run --offline --manifest-path tools/qml-diagnostic/Cargo.toml --example spotify_playback_probe
# device <returned device ID>
# play <displayed Track index>
# pause
# state
# seek 42000
# quit
```

Do not run two authorization listeners simultaneously. The probe never searches
the catalog or chooses a playback device automatically.

## Unified explicit Play

In GStreamer diagnostic mode, the normal library-row **Play** action now resolves:

```text
Play application Track
  → local source available? → GStreamer
  → otherwise Spotify association known? → Spotify user playback
  → otherwise bounded Spotify enrichment → persist association → Spotify playback
```

Local preference checks all currently available sources associated with this Track,
in stable source-ID order. A regular file must still exist and open for reading.
Missing paths are skipped without mutating source observations; unexpected access
or decoder errors remain local failures. No rescan, hashing or moved-file search
occurs. If local works, neither Spotify client is invoked for this Play.

Spotify requires connected playback authorization, an explicitly selected usable
device, and no blocking playback error. Open **Spotify…** to connect/rediscover and
select the desktop client once per launch; no device is silently selected. A known
song association bypasses catalog configuration and search entirely.

If no association exists and both capabilities are available, Play uses the same
catalog worker/search as the explicit chooser: one page, maximum ten results, no
follow-up queries. Automatic acceptance requires a unique eligible candidate on a
complete page, exact Unicode-lowercase/whitespace-folded Artist, title and Album,
and no supplied position/disc contradiction or duration difference over 3 seconds.
No fuzzy edits, punctuation removal, semantic qualifier stripping or search-order
selection is used. Missing optional duration/positions are not contradictions.
The short transaction rechecks metadata and existing associations before attaching
only the song ID. It does not establish Album, Recording or edition identity.

Ambiguous or insufficiently supported plausible results open the existing chooser,
reusing the fetched page without another search. No match reports unavailable.
Explicit confirmation remains available and stronger than automatic inference.
After a successful automatic acceptance, subsequent Play/restart uses the saved
association with zero catalog requests. Stop cancels a pending automatic lookup;
late/canceled replies cannot persist or start audio.

Backend ownership is None, Local or Remote(provider). Switching waits for local
Stop or application-controlled Spotify Pause to succeed before starting the other
backend. A local handoff first reads fresh Spotify state: already paused/idle
playback needs no Pause command, and playback on another device is not controlled.
A failed first remote Play with a definite rejection does not establish remote
ownership. Unknown remote output after a transport error retains remote ownership
so the next local switch must establish inactivity or pause successfully. The application does not pause Spotify merely
because local playback is chosen when it did not previously control Spotify.
Polling remains five seconds while the Spotify panel or application-owned remote
backend is active (first post-command observation after one second), with existing
backoff. It never invokes catalog lookup.

Mixed queue advancement, EOS/Next source switching, queue takeover, source
preferences, moved-file discovery and cross-provider identity reconciliation remain
deferred. The explicit Spotify panel remains available for debugging.
