# Spotify identity review and decision traces

Spotify Connections remains a management view under the Library `⋯` menu. Double-clicking a canonical Track or choosing `Spotify connection…` opens the same diagnostic used by Song context menus in Songs, Artist, Genre, Album and Playlist views.

The diagnostic compares accumulated known evidence and candidate values in one ordered table: Song, Artist credit, Album, Release type, Disc, Track, Track count, Duration, Release date, trusted external IDs, Artist IDs. Missing values are `—`. Identity equivalence, typography normalization, warnings and conflicts have distinct evidence/status markers. Conflict and warning colors are subdued. Selecting a candidate remains explicit; the first comparison can be inspected without selecting it for connection.

All returned candidates, including rejected candidates, are inspectable. Candidate count and bounded-page completeness are visible. The primary blocker is separate from secondary warnings and agreement evidence. The latest persisted reconciliation summary is shown too. A candidate qualifying for automatic **song** acceptance does not establish Album/Release or Recording identity; an explicit bounded diagnostic search now persists a final accepted Track result transactionally. Needs-review candidates still require manual confirmation.

## Matcher evidence

`song_resolution::evaluate` calls the same predicates and assessment used by explicit-Play acceptance. It exposes:

- Primary and complete ordered Artist compatibility, including trusted identity contradictions.
- Musical title/version compatibility and normalized established Album title.
- Established program count lower bound and supplied disc/Track position conflicts.
- The stricter **3-second** automatic duration tolerance, separately from the weaker presentation-feasibility check.
- Missing Album context, incomplete candidate page, plausible candidate competition and incomplete contributor evidence.
- Existing trusted Track identity conflicts before weaker metadata differences.
- Release date as presentation evidence rather than a song rejection rule. Unknown release dates/types are neutral. Known provider release type participates in compatibility checks; conflicting known positions remain blockers.

Album evidence retains the existing structured report. It exposes type/suffix handling, completeness/count bounds, duplicate/missing/shifted positions, trusted occurrence/Recording conflicts, independent anchors, competing representations and date-selection requirements. Relaxed Album-title acceptance requires at least `max(3, required_tracks - 1)` anchors. This is the same threshold the matcher consumes. There is no new scoring rule.

Expandable Program details render actual positioned-program evidence, including established-program cache evidence. Each row shows the local and provider positions/titles and the matcher's decision. Presentation is bounded to 200 established Tracks per program; full textual evidence remains available below. Programs rejected by earlier gates are not fetched just to fill the diagnostic.

QML renders structured data and statuses; it does not compare identities or implement acceptance rules. Search, connecting a Track and re-evaluation remain explicit. An optional Search Artist name is a bounded query override, useful when the provider no longer finds the historical name. It does not alter the canonical input used for evaluation, stored names or credits.

## Explicit Artist equivalence

Migration 0027 stores undirected, user-confirmed edges between application Artist IDs in `artist_equivalence`. IDs are ordered to make the edge unique; both traversal directions are indexed. Confirmation time is retained. If the candidate provider Artist already has a unique canonical owner, it is reused. Otherwise an opaque application Artist ID is created with the candidate's observed name and provider identity. Multiple owners are refused rather than guessed.

Relationships are not Artist merges. Direct `artist_external_identity` rows stay on their original Artist. Names, display credits, membership, playlist provenance, source files and local tags are not rewritten. Matching reads may derive identities reachable through confirmed equivalence; these derived IDs are never persisted on the other Artist. Batch credit reads traverse only their requested Artist IDs. Existing deliberate identity consolidation preserves explicit edges when replacing a duplicate Artist.

For an incompatible identified primary Artist, `Mark artists as same…` opens a modal confirmation showing both names and identities, explaining the future matching scope and retaining all other evidence requirements. Preparing or canceling changes nothing. Confirmation revalidates the proposal in a short write transaction, saves the edge and starts only the current Album's existing bounded re-evaluation. Further Albums remain explicit actions. Additional contributor conflicts are not silently equated by this primary-Artist action.

Saved links survive restart and are consulted in song, positioned-program and edition Artist evidence, and Spotify Album candidate Artist gates. Already trusted associations remain protected. Artist equivalence does not bypass Album title/type/version, complete-program, position, anchor, competition or trusted Track/Recording checks.

Review updates use the existing targeted Track-ID patches after reconciliation, retaining viewport, sort, filter and widths. There is no polling, whole-review diagnostic loading, offscreen provider lookup or automatic Artist-wide crawl.

## Real Library validation (2026-10-05)

The historical-name query returned no Track candidates for four representative Tracks by The Speed of Sound in Seawater. Searching explicitly for `tsosis` returned one candidate for **You Bite Down** from **Blue Version**. The pre-confirmation comparison showed the differing Artist names and the primary blocker `Artist identities are not established as equivalent`.

The actual QML confirmation was exercised, including cancel-before-confirm. The saved relationship links The Speed of Sound in Seawater to the canonical Spotify Artist `tsosis` (`7sB62q3ZbUTVI9G5RVsWCZ`). Blue Version re-evaluation associated all **5** saved Tracks. A separate explicit Red Version re-evaluation associated all **6** saved Tracks. Total: **2 Albums / 11 Tracks**, unresolved count **404 → 393**, membership unchanged at **484**. The original Artist still has its original name and no directly attached Spotify Artist ID; the ID is preserved on the linked Artist. Remote Track identities became available in source resolution and the rows disappeared immediately. A replay on the pre-change real-Library backup also verified unchanged playback/queue state. Audible playback was not exercised.

Regression controls remained conservative: Freshman Year has **16/16** saved Tracks unresolved; Piglet — Songs has **9/9** unresolved. The Second Floral EP (**6**), Spiritual Bankruptcy (**5**) and Hugs EP (**3**) have **zero** unresolved saved Tracks.

### Single-result audit

26 explicit bounded Track searches found these nine single-result cases. Their Album evidence was then inspected read-only using the existing bounded reconciliation machinery; no audit associations were applied. Duration/date differences below are secondary evidence, not invented primary rejection rules.

| Local Track | Sole candidate | Primary finding | Secondary differences |
| --- | --- | --- | --- |
| Hella — Ho's in the House | Ho's In The House | Song and read-only Album evaluation qualify; no prior retained attempt | Case, 0.3 s duration, date unknown/different |
| Hella — Trap Kit Whatever | Trap Kit Whatever | Album context mismatch: local Disc 2 grouping versus Spotify combined Church Gone Wild / Chirpin Hard | Position conflict, 0.1 s duration, date |
| Hella — World Series | World Series | Curly/straight apostrophe comparison blocked the song; fixed. Album still fails relaxed-title anchor threshold: **8/10 required** | Small duration difference, date; three program titles only position-corroborated |
| Hella — headless | Headless | Song and read-only Album evaluation qualify; no prior retained attempt | Case, small duration difference, date |
| Piglet — Bug Stomp | Bug Stomp | Song and read-only Album evaluation qualify; no prior retained attempt | Small duration difference, date |
| Piglet — Moon Bounce | Moon Bounce - Live | Track version conflict; Album version/program also conflict | Songs vs Songs EP (Live in Chicago), insufficient count, about 3 s duration, date; Album Artist establishment remains withheld |
| Shoes And Socks Off — And No One`s Seen Him Since | And No One's Seen Him Since | Song and read-only Album evaluation qualify; no prior retained attempt | Title typography, Album case, 0.1 s duration, date |
| Tera Melos — Ambassadors of All That Is Good | Ambassadors of All That is Good | Song and read-only Album evaluation qualify; no prior retained attempt | Case, 0.3 s duration, date |
| Tera Melos — System Preferences | System Preferences | Song and read-only Album evaluation qualify; no prior retained attempt | Date unknown/different; alternate Album candidate too short |

The safe general fix uses shared Album-title typography normalization for song context and normalizes curly apostrophes to the straight apostrophe. Meaningful punctuation, version qualifiers and conflicting provider identities remain distinct. The full Album matcher still requires structural corroboration for a normalized title; World Series demonstrates that this does not force its Album through the stricter gate.

## Validation and performance

Focused tests cover comparison field order/rendered values, agreement/conflict/normalized states, primary versus secondary evidence, one-candidate rejection, complete pages, competition, duration threshold, trusted conflicts, explicit confirmation/cancel, stale-proposal rejection, persistence/reopen, unchanged display/provider IDs, no inferred aliases, structural rejection after equivalence, qualifying multi-Track Album reconciliation, link preservation during existing identity consolidation and migration rollback. Existing review tests cover canonical targets, queue isolation, immediate multi-row removal, viewport/sort/filter/width preservation and bounded provider-free browsing.

On the existing 200k fixture, release-build targeted metadata/Artist-equivalence/identity context reads: **p50 0.247 ms / p95 0.329 ms**. Ten candidate decision traces: **p50 0.575 ms / p95 0.703 ms**. No provider requests occur in either path.

Warm review measurements: indexed count **3.85 ms**; first/next 201-row pages **1.9–10.5 ms**; p95 over 20 bounded pages: Song **2.86 ms**, Artist **11.02 ms**, Album **3.03 ms**, Reason **4.37 ms**; Album filters **0.66–0.78 ms**. Cold initial count/Artist measurements reached **123/243 ms**, respectively; report these separately from the warmed interactive path. The QML window remains bounded to 600 rows, with 200-row page increments.

Core suite: **350 passed / 15 ignored**, plus final focused migration/consolidation checks. One pre-existing unrelated playlist display assertion is excluded (`catalog_playlist_identity_membership_reload_and_duplicates`, reproduced on unchanged HEAD during the review implementation). Spotify adapter: 47 passed; MusicBrainz: 20 passed. Nine isolated QML regression tests, real confirmation/replay, formatting and strict Clippy checks pass. Generated validation artifacts are local under `/tmp`; nothing is committed.

## Deferred

Artist-wide retries, equivalence removal/editing, additional-contributor equivalence selection, automatic alias inference, generic provider alias/evidence management, metadata correction suggestions, consensus scoring, tag rewriting and file edits are deferred. Existing conservative matching/source priorities/provider architecture are retained.

Current canonical-evidence implementation and validation are documented in [Canonical Spotify reconciliation](canonical-spotify-reconciliation.md). The audit and timings above describe the preceding identity-review slice.
