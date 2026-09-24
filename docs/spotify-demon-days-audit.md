# Demon Days automatic Spotify resolution audit

Historical first-slice results. The subsequent [structured-credit audit](spotify-credit-matching-audit.md)
documents corrected explicit searches and both programs now surviving correlation;
their different Track IDs still prevent automatic association.

Live diagnosis on 2026-09-22/23, Spotify market US. Source: normal MusicBrainz
catalog **Add Album**, Gorillaz — Demon Days, group
`f959a46a-a136-3134-9412-6572b23fad95`. The normal representative-release importer
created 15 application Tracks, disc 1, positions 1–15, without local sources.
Effective Album Artist is **Gorillaz**, title **Demon Days**. All application Track
durations are absent: this catalog import path currently does not carry durations.
No user database or music file was changed. No commit was made.

## Proven cause and scope of the fix

The user's launch mode was `--catalog-provider musicbrainz`.
Before this change, `Bridge::catalog_reply(Add)` imported membership and refreshed
search but did not schedule Spotify matching. Even a manually scheduled Spotify
Album job was stopped by `storage::prepare_album_match` requiring a usable
`file_metadata_observation.track_title`. `album_program::local_tracks` independently
filtered out every Track without a local source. The fresh reproduction returned
`Done(Skipped)` and an empty program input: **0/15**, zero production Spotify calls.

A separate read-only forensic comparison fetched Spotify's programs using the same
candidate/comparison rules and the application metadata. It disproved the proposed
same-ID-withheld explanation for Demon Days: the two objects have **different Spotify
Track IDs at all 15 positions**. Both also fail current whole-program structural checks
because nine present Tracks cannot be explained under current title/Artist rules.

There was a separate persistence defect: `album_candidates::resolve` could produce
`AlbumEquivalent` with unanimous Track IDs, but `Store::complete_album_match_for`
persisted only Artist/unique Album identities. `AlbumMatcher::complete` put the
Track results in memory; `prepare_program` required one accepted provider Album ID,
so the normal `complete_album_program` persistence path never ran. Its automatic
association reads additionally join against the accepted provider Album identity.

The fix keeps these boundaries independent:

- Catalog Add schedules a dedicated existing Spotify `AlbumMatcher` when configured,
  regardless of the metadata-provider selection; `--no-auto-match` disables it.
  MusicBrainz/fallback jobs, OAuth, polling and mixed-queue behavior are unchanged.
- Eligibility and program snapshots use effective application Track metadata,
  including catalog-only and partial Albums.
- Candidate objects are compared independently. Every surviving plausible object
  must support the same occurrence for a Track. Missing/disagreeing mappings veto
  only that Track; already structurally rejected objects do not vote. Cross-object
  template ranking cannot discard conflicting IDs to manufacture agreement.
- `complete_album_match_for` revalidates the Track snapshot in its short transaction
  and writes unanimous occurrences to existing `track_external_identity` rows.
  Manual and existing accepted choices are preserved. No schema migration, Recording
  identity, provider Album selection, exact edition, or absent Track is created.
- Existing `track_provider_occurrences` and `playback_route` already support these
  independent rows. Reopen and playback routing need no catalog client/search.

No normalization, Artist acceptance, structural rejection, or duration tolerance was
loosened. No automatic per-Track fallback was added. Ranked Track search cannot
resolve already-known provider-ID disagreement without discarding contrary evidence.

## Artist search

One bounded, complete page: 10 results; no next page. Only Gorillaz is an exact
primary-name match. All nine other names are rejected by unchanged Artist matching.

| Artist | Spotify Artist ID | Decision |
|---|---|---|
| Gorillaz | `3AA28KZvwAUcZuOKwyblJQ` | Accepted exact name |
| Gorillaz Day | `3cxdr5LcBOBJzYZ5AR5cWN` | Different name |
| The Garden State Gorillaz | `08x8OeT48yMpbgmnftfpqt` | Different name |
| 48 Gorillaz | `1ziiFFgvZ9o4mblGLP2KEK` | Different name |
| Iron Gorillaz | `3PPOQ47E4feRLgve2f0aZi` | Different name |
| Biznes Gorillaz | `7GWqbuHZ4QQzx8iyohL2iS` | Different name |
| Black Gorillaz | `5WvyOwICx2C8IR27oZr86S` | Different name |
| Silverback Gorillaz | `4czaLY1WbjfWbmdjpzIwUQ` | Different name |
| crimno x john doe x phinix blahk x mspotential of I.n.s gorillaz | `1a3cUjb82gRxVVI0QSDdbK` | Different name |
| Gorillazul Casa del Afrohouse | `66AVUBZdF0ikkWREg4SSic` | Different name |

## Album search and program filtering

One complete Artist-scoped page, four objects. All credit Spotify Artist
`3AA28KZvwAUcZuOKwyblJQ` (Gorillaz).

| Spotify Album ID | Title | Type | Tracks | Date | Decision |
|---|---|---|---:|---|---|
| `0bUTHlWbkSQysoM3VsWldT` (A) | Demon Days | album | 15 | 2005-05-23 | Exact Artist/title and count pass; program fetched; rejected: ≥2 unexplained present Tracks |
| `45IcwOkFv3YqTpU5Bal8fl` (B) | Demon Days | album | 15 | 2014-04-11 | Exact Artist/title and count pass; program fetched; rejected: ≥2 unexplained present Tracks |
| `2FHwmbCq5LVystFUzjgx78` | Demon Days (Gorillaz 20 Mix) | single | 1 | 2021-05-23 | Not the exact-title candidate set; no program fetched (also cannot accommodate 15 positions) |
| `0J3u0IeHzuymSz7v934vrd` | Demon Days Live at the Manchester Opera House | compilation | 17 | 2011-11-25 | Different/live title; no program fetched |

There are **two metadata-compatible objects, zero surviving programs under current
structural rules**. This is `NoConfidentMatch`, not a unique Spotify Album and not a
successful `AlbumEquivalent` result. Neither search order nor date breaks ties.

## Fetched programs

Both fetched programs are complete, one 15-Track page each, all disc 1. Durations
below are Spotify milliseconds. IDs are Track occurrences, never Recording IDs.

### `0bUTHlWbkSQysoM3VsWldT`

| Position | Spotify title | Duration ms | Spotify Track ID |
|---:|---|---:|---|
| 1 | Intro | 67133 | `2zavoMfVVPJMikH57fd8yK` |
| 2 | Last Living Souls | 195400 | `7JzmCjvB6bk48JghLyrg8N` |
| 3 | Kids with Guns | 225773 | `0eEgMbSzOHmkOeVuNC3E0k` |
| 4 | O Green World | 275000 | `4hNPMfFHauPIbOKvdYqFt7` |
| 5 | Dirty Harry (feat. Bootie Brown) | 230426 | `2bfGNzdiRa1jXZRdfssSzR` |
| 6 | Feel Good Inc. | 222640 | `0d28khcov6AiegSCpG5TuT` |
| 7 | El Mañana | 235360 | `0dcMqjeDpwqB2xhzMsld0p` |
| 8 | Every Planet We Reach Is Dead | 295266 | `2pw9RZWZibttZzoFhwjuy6` |
| 9 | November Has Come | 165093 | `6lrDckuosGpwEHtm1hHBcf` |
| 10 | All Alone | 213333 | `76Ug1q4l6rGwJ2ubV5wh3X` |
| 11 | White Light | 133066 | `7hnW9af7GuGH5lyUUTa8UH` |
| 12 | DARE (feat. Shaun Ryder & Roses Gabor) | 244999 | `4Hff1IjRbLGeLgFgxvHflk` |
| 13 | Fire Coming out of the Monkey's Head | 199000 | `1S9tfxdFr4TqoqA14gnKj3` |
| 14 | Don't Get Lost in Heaven | 131866 | `4hjGZN8poACfjWCK3Og6vY` |
| 15 | Demon Days | 268400 | `2k6hpKTyubRVOmQR11ViY3` |

### `45IcwOkFv3YqTpU5Bal8fl`

| Position | Spotify title | Duration ms | Spotify Track ID |
|---:|---|---:|---|
| 1 | Intro | 63160 | `6kYEzcByL2fsOIBy3hjEeS` |
| 2 | Last Living Souls | 190400 | `6B1CTvwp5UHmsC7C5dUFyH` |
| 3 | Kids with Guns | 225840 | `3p6CzPIM1oThkSAVwYMNL2` |
| 4 | O Green World | 271960 | `0JY0InSgMgbL4ndXwMeVMO` |
| 5 | Dirty Harry | 223800 | `5LMfXbPn6VF5IusbdW1dmr` |
| 6 | Feel Good Inc. | 221173 | `72YCeOZX7NoyRPJGyGbTHs` |
| 7 | El Mañana | 230026 | `6Q2B0I3t6oXSVBKfJJYiFZ` |
| 8 | Every Planet We Reach Is Dead | 293266 | `0XpcgA4TlJAH3EpiyZhCeQ` |
| 9 | November Has Come | 161106 | `6VAIvE8McCUpZEqba1pSWj` |
| 10 | All Alone | 210066 | `25VvZQmr3OwrBrjZDB2KP0` |
| 11 | White Light | 128426 | `7m6fzGmgWlROIR4RDPShMS` |
| 12 | DARE (feat. Shaun Ryder & Roses Gabor) | 244306 | `2WEe9qVLhqZHOFoykgVlNu` |
| 13 | Fire Coming out of the Monkey's Head | 196426 | `1PSmZhEQks1E4OOBqSjdjh` |
| 14 | Don't Get Lost in Heaven | 120373 | `1sIBFDvOJacc9n4GPxYAK6` |
| 15 | Demon Days | 268893 | `3cQtzv5263FpoTaDmGudRm` |

## Each application Track

**All 15 remain unresolved.** The immediate program-level reason for every row is
that both candidate objects were rejected. The following are exact per-position
comparison reasons; even the six individually supported rows disagree on Spotify ID
between A and B. The application titles/credits are retained unchanged.

| Position | Application title | Effective Track Artist | A comparison | B comparison |
|---:|---|---|---|---|
| 1 | Intro | Gorillaz | exact normalized title | exact normalized title |
| 2 | Last Living Souls | Gorillaz | exact normalized title | exact normalized title |
| 3 | Kids With Guns | Gorillaz feat. Neneh Cherry | Artist disagreement | Artist disagreement |
| 4 | O Green World | Gorillaz | exact normalized title | exact normalized title |
| 5 | Dirty Harry | Gorillaz feat. Bootie Brown & San Fernandez Youth Chorus | title/semantic qualifier disagreement | Artist disagreement |
| 6 | Feel Good Inc. | Gorillaz feat. De La Soul | Artist disagreement | Artist disagreement |
| 7 | El mañana | Gorillaz | exact normalized title | exact normalized title |
| 8 | Every Planet We Reach Is Dead | Gorillaz | exact normalized title | exact normalized title |
| 9 | November Has Come | Gorillaz feat. MF DOOM | Artist disagreement | Artist disagreement |
| 10 | All Alone | Gorillaz feat. Roots Manuva & Martina Topley‐Bird | Artist disagreement | Artist disagreement |
| 11 | White Light | Gorillaz | exact normalized title | exact normalized title |
| 12 | DARE | Gorillaz feat. Shaun Ryder & Rosie Wilson | title/semantic qualifier disagreement | title/semantic qualifier disagreement |
| 13 | Fire Coming Out of the Monkey's Head | Gorillaz feat. Dennis Hopper | Artist disagreement | Artist disagreement |
| 14 | Don't Get Lost in Heaven | Gorillaz feat. London Community Gospel Choir | Artist disagreement | Artist disagreement |
| 15 | Demon Days | Gorillaz feat. London Community Gospel Choir | Artist disagreement | Artist disagreement |

The six supported positions are 1, 2, 4, 7, 8, 11. The remaining nine fail credit
presentation/member comparisons or featured-artist title suffixes. For example,
MusicBrainz has `Gorillaz feat. De La Soul`; A has ordered credits `Gorillaz, De La Soul`,
and B credits the individual De La Soul members. Dirty Harry has a featured-artist
suffix only on A; DARE has one on both. These are concrete further matching issues,
but neither erases the separate provider Track-ID disagreement. Missing application
durations also prevent independent duration corroboration.

## Fresh worker result, restart and network bounds

The final fresh-database run uses `AlbumMatcher::after_import` and its actual worker,
completion and persistence functions, matching the new catalog-add job. It reports
15 input Tracks, 4 Album search results, 2 fetched candidate programs, 0 surviving
programs, **0 automatic Spotify Track associations** and no Spotify Album identity.
All 15 remain unresolved for the reasons above. No individual Track searches occur.

Spotify requests: **4 catalog HTTP requests + 1 token request**: Artist search,
Album search, A Track page, B Track page. The pre-change forensic and post-change
forensic runs independently used the same 4+1 requests. The original production
path used 0 because it never attempted Spotify. The MusicBrainz source import has
its separate three catalog requests (search, representative editions, release).

The existing bound is unchanged: ≤3 candidate objects, no Album-search pagination,
and ≤20×50 Tracks per program. No Artist catalog enumeration or automatic per-Track
fan-out was introduced. Candidate comparisons and persistence stay Album-scoped.

Closing/reopening the final disposable database reproduces all 15 empty Spotify
associations; all routes correctly return `NeedsEnrichment`, with zero catalog HTTP
calls during reconstruction/routing. **No successful persisted Demon Days Spotify
playback is claimed:** there is no safely accepted Spotify occurrence to play.
Creating a manual association just to make that validation pass would not test the
requested automatic behavior.

Deterministic real-SQLite tests instead demonstrate positive independent persistence:
two/three unresolved Album objects with agreed IDs, partial Albums, individual
agreement/disagreement, missing mapping veto, rejected candidate exclusion, unchanged
unique-Album behavior, manual/previous choice precedence, stale evidence rejection,
reopen and repeated remote-route lookup without any catalog provider. They assert
no Recording/edition identities are inferred. Existing Super Fx/Fxx, and/&,
ambiguous explicit-Play search and mixed-queue/polling tests remain in the suites.

## Live controls and validation

| Control | Result | Spotify catalog requests (token separate) |
|---|---|---:|
| Painted Shut | 10/10 persisted and identical after reopen, including Waitress `6aYsKS9XcItrOUWNMLn3Ba` | 3 + 1 token |
| Trash Generator | 12/12 persisted/reopened; one-track Single rejected before fetching its program; Super Fx retained separately | 3 + 1 token |
| Hella — Bitches Ain't Shit But Good People | 4/4 persisted/reopened on fresh retry | 4 + 1 token |
| Wretches | Spotify no confident match; MusicBrainz fallback succeeded; reconstruction identical after reopen | 2 + 1 token, then existing MusicBrainz workflow |

The first Hella live attempt stopped on Spotify HTTP 502 rather than retrying or
accepting partial evidence; the separate fresh retry above succeeded. The normal
library and local files were untouched. No live audio was started in these controls.

Validation passed: 224 core tests, 32 Spotify catalog/playback HTTP mocks,
19 MusicBrainz tests, 5 GStreamer tests, and 5 QML/session tests. Core coverage
includes source resolver, mixed queues, manual precedence, Super Fx/Fxx, and/& and
ambiguous search acceptance. QML tests verify catalog Add schedules the independent
Spotify worker in MusicBrainz mode and disabling automatic matching suppresses it.
MusicBrainz and Spotify QML smoke modes, QML lint, formatting, strict Clippy for
core/providers/QML (all targets; QML all features), and `git diff --check` passed.
Opt-in live tests are excluded from ordinary suites. Qt still emits the pre-existing
cross-thread timer teardown warning after its tests pass.

Initial provider mock runs were blocked by the sandbox's socket-binding restriction;
they passed when rerun with the required permission. QML fixture assumptions that
only local Albums have Track rows were corrected to explicitly select the intended
MusicBrainz fixture. The production matcher now legitimately exposes catalog-only rows.

## Performance

Release-build local measurements; these are observations, not new budgets:

| Work | Median/average |
|---|---:|
| Fit three programs against 5 present Tracks (average of 1,000 runs) | 22.906 µs |
| Fit three programs against 15 present Tracks | 204.497 µs |
| Fit three programs against 30 present Tracks | 791.479 µs |
| Full three-object agreement comparison, three present Tracks (20 samples) | 138.298 µs |
| Independent Artist + three Track IDs, snapshot validation and commit (20 samples) | 302.997 µs |
| 200k fixture: indexed Album lookup | 6.310 µs |
| 200k fixture: ten-Track verification | 14.400 µs |
| 200k fixture: first 50-row library page | 136.829 µs |
| 200k fixture: persisted association playback route including source check | 22.760 µs |

The 200k check ran once. Compared with the earlier candidate audit's approximately
6.45/14.70/140.17 µs lookup/verification/page observations, there is no significant
regression in these paths. New work is confined to the imported Album and its bounded
provider programs. Existing identities are batch-read before prepared inserts in one
short transaction; no whole-library scan, schema/index change or network transaction
was added.

Reproducible diagnostic: `tools/qml-diagnostic/examples/album_resolution_probe.rs`.
It requires a new `/tmp` database and never starts audio. `--worker` exercises the
production AlbumMatcher; omitting it prints all forensic programs and per-position
comparison reasons. Session artifacts: `/tmp/demon-days-before.log`,
`/tmp/demon-days-after.log`, `/tmp/demon-live-7XUET5/`,
`/tmp/demon-hella-retry.log`, `/tmp/demon-timing.log`, `/tmp/demon-200k.log`.
