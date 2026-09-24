# Spotify Album-context and date audit

Live Spotify US catalog validation, 2026-09-23. New disposable databases only;
normal library, music files, and playback credentials were not modified. No commit.
This supersedes the unresolved representation tie in the
[structured-credit audit](spotify-credit-matching-audit.md).

## Verified representations and baseline

Application: **Gorillaz — Demon Days — 2005**, ordered 15-Track catalog program,
no local files. Application durations are unknown. The current application schema
retains the established Album year, not an invented full release date.

| Spotify Album ID | Title | Album Artist | Date | Spotify precision | Type | Tracks |
|---|---|---|---|---|---|---:|
| `0bUTHlWbkSQysoM3VsWldT` | Demon Days | Gorillaz | 2005-05-23 | day | album | 15 |
| `45IcwOkFv3YqTpU5Bal8fl` | Demon Days | Gorillaz | 2014-04-11 | day | album | 15 |

**2005 versus 2014 is confirmed for these exact surviving objects**, not inferred
from another UI row. Both have Spotify Artist `3AA28KZvwAUcZuOKwyblJQ`, exact Album
title and 15 compatible ordered musical titles/positions, no unexplained Tracks,
and no supplied application duration contradictions. Structured credits support the
human program even where additional contributor aliases remain unresolved. Before
this slice neither was preferred: the program rules did not compare application
year. Their Spotify Track IDs differ at every position, so the baseline persisted
**0/15**, without arbitrarily selecting either object.

The two other search objects remain excluded: one-Track Single
`2FHwmbCq5LVystFUzjgx78` (“Demon Days (Gorillaz 20 Mix)”) and 17-Track Compilation
`0J3u0IeHzuymSz7v934vrd` (“Demon Days Live at the Manchester Opera House”).
Neither supplies a candidate program for this Album.

## Ordered programs

All positions are disc 1. A = 2005; B = 2014. Every occurrence ID below was verified
in the fresh live output against the existing recorded structured-credit fixture.
Titles shown are the application/musical titles; A's Dirty Harry includes the
verified `(feat. Bootie Brown)` annotation, and both DARE titles include
`(feat. Shaun Ryder & Roses Gabor)`. Other differences are capitalization.

| Position | Title | A Track ID | A ms | B Track ID | B ms |
|---:|---|---|---:|---|---:|
| 1 | Intro | `2zavoMfVVPJMikH57fd8yK` | 67133 | `6kYEzcByL2fsOIBy3hjEeS` | 63160 |
| 2 | Last Living Souls | `7JzmCjvB6bk48JghLyrg8N` | 195400 | `6B1CTvwp5UHmsC7C5dUFyH` | 190400 |
| 3 | Kids With Guns | `0eEgMbSzOHmkOeVuNC3E0k` | 225773 | `3p6CzPIM1oThkSAVwYMNL2` | 225840 |
| 4 | O Green World | `4hNPMfFHauPIbOKvdYqFt7` | 275000 | `0JY0InSgMgbL4ndXwMeVMO` | 271960 |
| 5 | Dirty Harry | `2bfGNzdiRa1jXZRdfssSzR` | 230426 | `5LMfXbPn6VF5IusbdW1dmr` | 223800 |
| 6 | Feel Good Inc. | `0d28khcov6AiegSCpG5TuT` | 222640 | `72YCeOZX7NoyRPJGyGbTHs` | 221173 |
| 7 | El mañana | `0dcMqjeDpwqB2xhzMsld0p` | 235360 | `6Q2B0I3t6oXSVBKfJJYiFZ` | 230026 |
| 8 | Every Planet We Reach Is Dead | `2pw9RZWZibttZzoFhwjuy6` | 295266 | `0XpcgA4TlJAH3EpiyZhCeQ` | 293266 |
| 9 | November Has Come | `6lrDckuosGpwEHtm1hHBcf` | 165093 | `6VAIvE8McCUpZEqba1pSWj` | 161106 |
| 10 | All Alone | `76Ug1q4l6rGwJ2ubV5wh3X` | 213333 | `25VvZQmr3OwrBrjZDB2KP0` | 210066 |
| 11 | White Light | `7hnW9af7GuGH5lyUUTa8UH` | 133066 | `7m6fzGmgWlROIR4RDPShMS` | 128426 |
| 12 | DARE | `4Hff1IjRbLGeLgFgxvHflk` | 244999 | `2WEe9qVLhqZHOFoykgVlNu` | 244306 |
| 13 | Fire Coming Out of the Monkey's Head | `1S9tfxdFr4TqoqA14gnKj3` | 199000 | `1PSmZhEQks1E4OOBqSjdjh` | 196426 |
| 14 | Don't Get Lost in Heaven | `4hjGZN8poACfjWCK3Og6vY` | 131866 | `1sIBFDvOJacc9n4GPxYAK6` | 120373 |
| 15 | Demon Days | `2k6hpKTyubRVOmQR11ViY3` | 268400 | `3cQtzv5263FpoTaDmGudRm` | 268893 |

## Date decision and persistence

`catalog_date` parses year/month/day with actual precision and calendar validation.
Agreement categories are unknown/different year, same year, same known month, and
same known full date. Year-only `2005` agrees equally with all valid 2005 dates;
full known agreement can distinguish two dates in that year. No nearest-year rule
or provider result order selects a winner.

Date discrimination runs only after Artist/title filtering and complete supported
program checks for every remaining object. An uncertain competitor prevents date
selection; rejected programs cannot be rescued by their dates. The preferred
program still needs at least three exact positions. Equal/unknown dates or neither
matching leave ambiguity unresolved. Completion revalidates the Album year, title,
Artist, and the existing Track snapshot protections.

Here A uniquely agrees with established **2005**; B remains compatible but has
weaker date evidence. A is now the preferred catalog representation. Its cached
program feeds the existing Track comparator and persistence path without requiring
agreement with B's different occurrence IDs. No additional program request is made.

**Human Album identity ≠ preferred provider catalog representation ≠ exact edition.**
This writes the usual provider Album and Track-occurrence associations, never an
exact Release/edition identity or a fabricated Spotify Recording identity. Existing
application metadata, sparse overrides and manual associations are preserved.

Fresh production worker result: **14/15 automatically associated**, identical after
closing/reopening. Each accepted occurrence routes remotely with zero catalog calls.
The sole unresolved Track is **12, DARE**: application additional credits are Shaun
Ryder and Rosie Wilson; Spotify supplies Shaun Ryder and Roses Gabor. That alias is
not established. Exact title/position supports human-Album compatibility and manual
feasibility, but existing automatic rules withhold its occurrence. No alias was
invented to force 15/15. No live audio was started.

## Manual chooser

The existing primary-Artist bounded query is unchanged. Spotify search responses
already include Album Artist, date, type and total Tracks; those fields now reach
the provider-neutral classifier without an Album lookup. It shares structured
credit/featured-title checks, lower-bound program feasibility and date comparison
with the matching paths. No opaque score is added.

- **Preferred feasible / Best match:** no contextual contradiction, strongest date
  agreement when available.
- **Alternate feasible / Other compatible Album representation:** compatible human
  Album/Track, weaker date evidence; e.g. the 2014 reissue. Unresolved contributor
  aliases do not alone exclude a manual choice.
- **Infeasible / Outside Album context (override):** contradictory primary/known
  Artist, Track title/version, Album title/Artist, insufficient program size, or
  conflicting known disc/Track position. Type explains Single/EP/compilation
  contradictions rather than universally banning those catalog types.

The program lower bound uses established Album Tracks across Releases, independent
of library membership and source availability. It does not equate partial local
ownership with a complete program. Track position constrains this established Album
context; it is not global identity across arbitrary reissues. Unusual sequencing can
still be chosen through the override.

A duration difference over 30 seconds corroborates other contradictions; it is never
a standalone feasibility veto. Small discrepancies and unknown durations stay
plausible. Automatic explicit-Play acceptance retains its stricter existing credit,
complete-page and supplied-duration requirements. No automatic acceptance runs merely
because a manual chooser has one visible item.

The QML chooser caches classifications and shows preferred then alternate results.
**Show all Spotify results** exposes the retained infeasible rows with explicit
outside-context labels. It adds no search/pagination, does not persist, and maps
visible selection indices back to the original bounded page before manual confirmation.
New searches start filtered. Explicit overrides remain authoritative.

### Live chooser results

The actual QML dialog, normal asynchronous search worker, ComboBox model and show-all
handler were exercised offscreen against live Spotify, using a temporary copy of the
fresh unassociated database. No choice was confirmed. Four searches used **one catalog
token request + four search HTTP requests**. Each default ComboBox had two choices;
show-all restored the full bounded page with unchanged request counters and empty
persisted Spotify associations.

| Track | Returned | Preferred | Alternate | Hidden | Initial debug classification |
|---|---:|---:|---:|---:|---:|
| Feel Good Inc. | 9 | 1 | 1 | 7 | 107 µs |
| Dirty Harry | 8 | 1 | 1 | 6 | 108 µs |
| DARE | 7 | 1 | 1 | 5 | 171 µs |
| El Mañana | 8 | 1 | 1 | 6 | 45 µs |

All pages were complete within the unchanged ten-result bound. In every row the
2005 occurrence is preferred and the 2014 occurrence remains a visible alternate.
Automatic explicit-Play assessment still reports NeedsSelection for these pages.

Concrete hidden results:

| Example | Contradictions |
|---|---|
| Feel Good Inc. EP, 4 Tracks, position 1 | Wrong Album, too short for established 15-Track program, wrong position (expected 6) |
| Feel Good Inc. on The Singles Collection 2001–2011, position 5 (two objects) | Wrong Album and position |
| Peda de Halloween y Día de Muertos, position 10; All Day Throwback 100 Hits, position 20 | Unrelated compilation, Album Artist and position differ |
| Feel Good Inc. on Rhythms Del Mundo Revival, 14 Tracks, position 3 | Wrong Album/Album Artist, insufficient count and wrong position |
| Feel Good Inc. — Stanton Warriors Remix, 444,906 ms | Explicit remix, D-Sides Album, position 2; over twice the preferred occurrence's 222,640 ms |
| Dirty Harry on The Singles Collection (two objects), position 7 | Wrong Album and position (expected 5) |
| Dirty Harry — Schtung Chinese New Year Remix (three objects) | Remix, D-Sides context, positions 8/21 |
| Dirty Harry — Live at Manchester Opera House | Explicit live version and different Album |
| DARE Single, one Track, position 1 | Wrong Album, insufficient program and position (expected 12) |
| DARE on The Singles Collection (two objects), position 6 | Wrong Album and position |
| DARE — DFA Remix, 734,488 ms, position 1 | Remix, D-Sides, wrong position; approximately three times the preferred occurrence's 244,999 ms |
| DARE — Junior Sanchez Remix, 326,373 ms, position 7 | Remix, D-Sides, wrong position |
| El Mañana — Metronomy Remix (three objects), 344,666 ms, positions 6/19 | Remix, D-Sides, wrong position (expected 7) |
| El Mañana on The Singles Collection (two objects), position 9 | Wrong Album and position |
| El Mañana — Live at Manchester Opera House | Explicit live version and different Album |

Application durations are absent in this import, so the live classifier does not
pretend to have a trusted duration difference. The gross differences above are
forensic comparisons to the preferred Spotify program; those results are already
excluded by context/version. Synthetic tests separately verify corroborating duration
evidence, small mismatches and no universal duration-only rejection.

## Bounds, cost and regressions

Automatic resolution remains **4 Spotify catalog HTTP requests + 1 token request**:
Artist search, Album search, and one Track page per plausible representation. The
selected program is reused. The MusicBrainz source import separately uses its normal
three catalog requests. No automatic per-Track search or global scan is introduced.

Release-build local date discrimination including comparison of two cached programs
(1,000 iterations): **34 µs / 244 µs / 970 µs** for 5/15/30 present Tracks. Manual
classification costs above are debug-build observations per bounded page, excluding
network. Classifications are cached for UI notifications and show-all.

Exactly one fresh 200k run: selected Track context median **63.41 µs**, persisted
playback route **22.89 µs**, indexed Album lookup **6.34 µs**, ten-Track verification
**14.83 µs**, first 50-row library page **137.87 µs**. The added Album-scoped metadata,
credit and position reads increase the preceding selected-Track observation (~27 µs)
by ~36 µs; playback/library browsing stay comparable. No full-library operation was
added. Existing slow diagnostic search-filter shapes remain outside this change.

Validation: broad core suite **233 passed**, plus the subsequently added stale-year
regression and focused rechecks; Spotify **33 passed**; MusicBrainz **19 passed**;
GStreamer **5 passed**; QML **5 passed**, plus the live four-Track chooser test.
Opt-in tests remain excluded from ordinary runs. Covered behaviors include date ties
and precision, wrong-program veto, no exact edition, normal occurrence persistence
and reopen, stale-date rejection, partial membership, chooser classifications, show-all
without HTTP, explicit override authority, Super Fx/Fxx, structured multi-Artist
queries, provider fallback, source resolver and mixed-source queues.

Fresh live controls: Painted Shut **10/10**, Trash Generator **12/12**, Hella **4/4**,
all identical after reopen; Wretches **Spotify NoConfidentMatch → MusicBrainz Matched**.
Strict Clippy (all targets, QML all features), formatting, qmllint, offscreen QML
integration smoke and diff checks pass. HTTP mock tests required sandbox permission
to bind local sockets; live provider tests required network permission. The existing
Qt cross-thread timer teardown warning remains after successful QML tests.

Remaining major automatic-coverage obstacles: unestablished contributor aliases/group
membership (DARE here), equal/unknown date evidence with conflicting occurrence IDs,
insufficient or uncertain program evidence, missing durations where existing stricter
rules need them, and genuinely absent/bounded-out provider results. This slice does
not broaden searches or infer exact editions to bypass those limits.

Artifacts: baseline `/tmp/demon-credit-before-G8TM3U/`; final forensic/worker
`/tmp/demon-credit-after-lYSKR9/`; live controls `/tmp/demon-live-8uQrSm/`;
actual QML `/tmp/album-context-live-qml.log`; timing `/tmp/album-context-timing.log`;
200k `/tmp/album-context-200k.log`. Reproducible probes are
`album_resolution_probe`, `track_credit_probe`, and the ignored
`live_spotify_album_context_chooser` QML test. For the latter, prepare an unassociated
disposable MusicBrainz import using `spotify_resolution_probe --add-musicbrainz`
with Artist/Album arguments and set `MUSIC_LIBRARY_DIAGNOSTIC_DATABASE` to that `/tmp`
database; the test copies it and opens the real chooser without confirming any choice.
