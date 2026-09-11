# Recording matching audit

Measured on Linux x86_64 with SQLite 3.53.2, 2026-09-11. Normal tests use
deterministic responses; live probes are opt-in and read local files without
changing them.

## Local cost

Release-build medians over 20 fresh temporary-library samples, excluding network:

| Local Tracks | Prepare eligibility/input | Compare, revalidate and commit identities |
| --- | ---: | ---: |
| 1 | 0.076 ms | 0.114 ms |
| 3 | 0.085 ms | 0.181 ms |
| 15 | 0.131 ms | 0.513 ms |
| 30 | 0.188 ms | 1.035 ms |

Run `cargo test --release --test local_matching recording_local_timing -- --ignored --nocapture`.
Reverse identity resolution and entity-local listing use their indexes; Track
reassignment uses `track_recording`. Eligibility targets Releases under one Album
and their Tracks, effective metadata and credits, rather than scanning the library.

The existing 20k-Album/200k-Track fixture completed its checks after migration:
Album candidate lookup 6.16 µs, ten-Track corroboration 14.58 µs, first library
page 0.137 ms, deep keyset page 6.48 ms. The first open included the one-time
Recording backfill (4.77 s); subsequent opens had a 0.196 ms median. These are
environment-specific observations, not new performance budgets.

## Request shape and live result

For Hella's *Bitches Ain't Shit but Good People*, three alternating samples compared
`rgid:<MBID>` with the same scope plus an OR of all four local Track titles.
Both returned four Recordings. Serialized payloads were 9,264 versus 9,261 bytes.
Successful HTTP attempt times were 609/585/301 ms for the broad scope and
309/174/179 ms for the title-constrained scope. One broad operation first received
503 and retried. These small samples do not establish a reliable service-latency
advantage. The selected broad scope avoids title-filter omissions and adds almost
no payload for this complete Album. A partial-title query returned only one candidate;
the implementation instead obtains the Album context once and compares locally.

The selected request is `recording?query=rgid:<MBID>&limit=100&offset=0`.
No per-Track requests or automatic paging occur. An incomplete page prevents new
automatic associations. Search semantics are defined by the official
[Recording search documentation](https://musicbrainz.org/doc/MusicBrainz_API/Search/RecordingSearch).

A read-only `/mnt/f/music/Hella/...` probe matched three of four local Tracks.
“Rich Kid” remained `NoConfidentMatch`; the diagnostic does not currently record
the individual comparison veto. No ISRCs were returned for these candidates, so
live ISRC persistence was not exercised (deterministic fixtures cover multiple ISRCs).
The successful probe used one logical request with two HTTP attempts after a 503;
an earlier probe exhausted all three attempts. Local metadata, source availability
and Track placements remained unchanged, with no Release/Track identities inferred.
Public-service outages remain independent of local matching/persistence cost.
