# Spotify sole-remaster playback audit

Validation used only retained Spotify JSON, cache-only APIs, fixtures and disposable
SQLite copies. Live Spotify requests: **0**. No commit was made. The original database
was neither reconciled nor modified.

## Critical Meat evidence and old blocker

| Field | Observed evidence |
| --- | --- |
| Artist | You Slut!; established Spotify Artist `57FhnNHVDTmK7wsrNAq84o` |
| Canonical Album | Critical Meat; `f1a594de-5ca0-4615-a024-adbe47203f43` |
| Canonical year | Unset; retained local file observations consistently say **2006** |
| Provider Album | Critical Meat (Remastered); `7zc9EkiGkY5HQQL17JjyE3` |
| Provider year/type | **2012**, Album |
| Scoped candidates | **1**, complete page; no competing Artist-compatible representation |
| Program | **8/8** unique exact normalized title correspondences, disc 1, positions 1–8 |
| Durations | Largest difference **4,520 ms**; all inside existing Spotify playback tolerance |
| Recording identity | Eight retained local MusicBrainz Recording IDs; Spotify supplies none; no conflicting trusted ID |
| Stored old blocker | `album_version_mismatch`: “Version qualifier conflict”, evaluation 14, current/not stale |
| New result | Automatic playable-source association; **8 connected**, **0 unresolved**, **0 retries** |

The stored blocker was the remaster annotation, not the year. The effective canonical
year was absent, so the evaluator had no conflicting canonical date to veto. Source
observations nonetheless reveal the release-level 2006 versus 2012 difference.

Old query generation first used:

```text
album:"critical meat" artist:"You Slut!"
```

That retained response contains the relevant remaster and no more results. Because
remaster title correspondence was not counted as useful discovery, the old strategy
also used:

```text
"You Slut!" critical meat
```

The alternate returned ten results, including unrelated Artists, and a next page.
Cache-only old reevaluation then withheld the candidate for
`incomplete_candidate_page` while its row retained `album_version_mismatch`.
The new query uses only the scoped primary above. No additional title variants,
Track searches, pages or quota consumption were introduced.

The fresh DB did not retain this Album's Track page. Validation copied exactly one
immutable provider discovery response from the earlier disposable full-lifecycle DB:

```text
spotify:1:https://api.spotify.com/v1/albums/7zc9EkiGkY5HQQL17JjyE3/tracks?market=US&limit=50&offset=0
```

Canonical entities came from `/tmp/music-library-fresh.sqlite`; the retained page
came from `/tmp/discovery-full-lifecycle.sqlite`; reconciliation ran on
`/tmp/remaster-validated.sqlite`. No local metadata, identity or program was fabricated.
The fresh DB alone cannot finish offline without that missing retained page.

## Policy correction and safety

The existing sole-edition title policy is shared by discovery and Album-program
playback; it now explicitly includes Reissue. Qualifiers remain raw provider evidence.
A corresponding remaster counts as a relevant primary search result, without
establishing canonical identity or suppressing candidate competition checks.

A complete bounded candidate page and one Artist-compatible representation permit
fallback playback with established Artist identity, at least three independent
program anchors, at least 80% canonical coverage, unique mapping, compatible
durations and no trusted Recording conflict on an attached Track. Uncached competitors
and incomplete candidate pages cannot disappear from the uniqueness check. Original
matching candidates win; competing materially different representations stay reviewable.
Live/Acoustic/Demo are not made harmless release annotations. A single discrepant
Track remains unresolved while the independently strong remainder may connect.

Both general Album completion and cached reconciliation use the shared program
playback method. The canonical Spotify Album identity is not attached for a sole
alternate playable edition. Provider Album title/year are not observed as canonical
Album metadata. Canonical Album/Release/Track rows, title/year overrides, external
Album/Release identities and membership were compared before/after and are identical.
Track occurrences, raw provider candidates and Spotify duration observations supply
playback separately. No independent Track discovery is performed.

Accepted Connections summaries now retain this explanation rather than being overwritten
with “Album needs review”:

```text
Connected from compatible Spotify program; canonical Release retained.
Provider release differs: Critical Meat ↔ Critical Meat (Remastered).
Warning: Spotify remaster/reissue or edition differs; established Artist and
unique strong program, no competing provider representation.
Warning: release year differs: 2006 ↔ 2012
```

Review evidence also includes the raw edition qualifier and the explicitly labeled
local source-year observation. Accepted diagnostics say why acceptance succeeded,
rather than labeling that reason a blocker. Evaluator version is **15**, Album search
strategy **6**; retained earlier cache responses remain inspectable without TTL renewal.
HTTP boundaries, short write transactions, cooldown and request bounds are unchanged.

## Validation and limits

Focused coverage exercises complete unique remasters/reissues, year differences,
canonical preservation, raw metadata/warnings, original preference, version competition,
Live/Acoustic/Demo distinctions, trusted Recording conflicts, partial safe Track mapping,
and cache-only propagation with no Track search. The cache-only example
`tools/qml-diagnostic/examples/remaster_playback.rs` constructs no live client.

Validation passed:

- `cargo test --offline --lib --tests`: 490 core tests; 18 intentionally ignored.
- Focused Album candidate/program/lifecycle run: 101 passed.
- Spotify adapter suite: 81 passed; live/ignored tests were not executed.
- Isolated QML reconciliation/review tests: 17 passed; six live/ignored tests skipped.
- Strict Clippy: core library, Spotify library and cache-only remaster example.
- All examples compile; `cargo test --offline --no-run` builds all targets.
- Rust formatting and `git diff --check`.

The standalone availability benchmark embeds the application sources. Its module
list was updated to include the now-shared program-playback implementation so the
completion-boundary change also compiles in that benchmark.

Localhost mock-server
tests are distinct from live Spotify access; live integration tests remain ignored.
No fresh provider availability claims are made during cooldown. No unrelated matcher
redesign, canonical merge, new fuzzy search, or live reconciliation was attempted.

The full retained program, old/current reports, exact positions/durations/Recording
IDs, preservation comparisons and request count are in
[the machine-readable evidence](spotify-remaster-playback-evidence.json).
