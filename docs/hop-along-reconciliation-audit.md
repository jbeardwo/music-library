# Hop Along playlist reconciliation audit

Validated 2026-10-05 against `/tmp/music-library-ui-test.sqlite`, playlist
`Hop Allong` (`6b4aad53-fc47-4e2a-8b9a-77ddea11ea66`). The supplied session JSON
is user authorization state, not the library database. Read-only diagnosis was followed
by validation on a SQLite backup and repair of the original database.

## Before repair

| Track | PlaylistEntry ID | Playlist Track ID | Spotify Track ID | Local Track ID | Disc / Track |
| --- | --- | --- | --- | --- | --- |
| Some Grace | `5e3ec623-3e61-40a4-925b-b6b82ed3232e` | `59d86735-7b8d-463c-9161-d13e4244ee49` | `38CLjvzuqaIADFFZrThgn5` | `76727301-856b-4b63-a9dc-05ed13dbed0b` | 1 / 1 |
| Tibetan Pop Stars | `fbe82558-7412-4e48-832c-d48bda41ff46` | `89af332a-60e2-48bf-8b2f-d30fd02b66b6` | `3MVL733KX2m9G76qPnTttk` | `d5e94c59-1b4b-43f8-b437-a2db1c4f97b2` | 1 / 2 |
| Waitress | `aa933527-0ae3-438d-8bbd-de05164e8239` | `8f213ecc-dc39-446c-b0cf-94f3a3afc20a` | `4cVluxrIFQb32KWanHqYDF` | `7a38e528-e8b8-4509-973d-642511efc959` | 1 / 4 |
| Buddy in the Parade | `3223116b-cf79-4ba0-a686-f7601d08115f` | `652bcbef-f971-48d3-8c5a-e36535263ef4` | `3z4qUVE4YsiefmCc9K3R84` | `e4f67547-ba78-45c6-a5e9-3894800c5c47` | 1 / 2 |

All four local Tracks were Library members with readable files and no Spotify Track
identity. Their playlist-only counterparts had Spotify identity, no local source,
and no Library membership. Local MusicBrainz recording associations were preserved.

Local paths:

- `/mnt/f/music/Hop Along/Get Disowned/01 Some Grace.mp3`
- `/mnt/f/music/Hop Along/Get Disowned/02 Tibetan Pop Stars.mp3`
- `/mnt/f/music/Hop Along/Painted Shut/04 Waitress.mp3`
- `/mnt/f/music/Hop Along/Painted Shut/02 Buddy in the Parade.mp3`

## Cause and repair

Playback correctly resolved the playlist Track ID, but that Track had only Spotify.
The existing Spotify Album matcher then accepted alternate catalog Album objects:
their occurrence IDs differed from those in the playlist. Exact identity lookup
therefore still could not reuse the local Tracks.

The explicit **Resolve local Tracks** playlist action and automatic post-import
enrichment reuse the existing Spotify Album matcher for Artist/Album context, then
fetch complete programs for the actual playlist Album identities. The existing
`compare_album` rules decide occurrence matches, with an additional shared trusted
provider Artist gate. Partial programs, position/credit contradictions, ambiguous
local editions, and existing explicit conflicting choices remain separate.

Accepted incoming occurrences persist on local canonical Tracks; indexed trusted
identity lookup repoints PlaylistEntries in bounded transactional windows. Old
staging Track observations remain intact, preserving existing queue snapshots. No
Library membership, local source, user override, or playlist provenance is deleted.

All 29 Hop Allong entries were repaired. IDs and canonical positions, Library
membership rows, and playlist provenance were unchanged. Source resolution is local
first; playlist provenance does not select the backend.

Actual GStreamer playback on a disposable library backup using the real local files
passed double-click, Waitress direct Play, Play Playlist, Next/Previous, and Add to queue then Play. Volume
was zero; Playing state and Local dispatch were verified. PulseAudio required a
sandbox escalation. The non-local Spotify probe initially found no active Connect device. After the user
activated PUHI, persisted Spotify Track `3Qucz2EpOkNo1rx6tKUQlC` played successfully,
its current Track ID was confirmed, and the probe paused it. No catalog search
was required for that playback.

Hella files are mounted and readable here (for example `Ho's in the House`).
The earlier report meant the live database had not been located, not that Hella
files were absent. This task uses Hop Along as its primary real-library test.

## Canonical Playlist metadata follow-up

The local Hop Along Tracks have empty effective Track Artist strings but valid
ordered Album credits. Main Songs already uses those credits as its display-only
fallback. Playlist projections previously read only `effective_track_metadata.artist_names`,
so QML showed its `Unknown artist` fallback after a correct canonical Track repoint.
This was projection divergence, not an identity failure or retained staging metadata.

Both now share the same SQL display-credit expression: effective Track display
credit first, then ordered Album credit with credited names/join phrases. Playlist
canonical and sorted windows also carry effective canonical Genre rather than
an unconditional empty string. Title, Album and Length already followed the
current PlaylistEntry Track relationship. The table schema is unchanged.

The existing per-playlist revision detects Track repoints. Bounded replacement
rows patch QML properties by stable PlaylistEntry ID; no model/delegate replacement
is needed when the row set/order is unchanged. Regression coverage verifies Artist,
Album, Genre and Length alongside viewport, selection, sorting and retained widths.
All 29 real Hop Allong rows agree with the normal Songs projection; QML renders
Hop Along, and real-file GStreamer playback still succeeds. A fresh connection
rebuilds the temporary projection, so existing imports need no re-import or data repair.
