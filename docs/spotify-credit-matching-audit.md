# Demon Days structured-credit matching audit

Live fresh MusicBrainz catalog imports, 2026-09-23, Spotify US market. This follow-up
supersedes the credit/title failures in [the first audit](spotify-demon-days-audit.md).
Application: **Gorillaz — Demon Days (2005), 15 Tracks**, no local sources, imported
durations unknown. Display titles and credits remain unchanged. Normal library and
music files were untouched. No commit.

## Part A: bounded Track search

The exact path is QML `Search Spotify for this Track` → `spotify_resolution` worker
→ `Library::song_resolution_input` / `song_resolution::load_input` →
`Spotify::search_songs`. Previously `Input.artist` was the complete effective display
credit, passed literally into Spotify's `artist:` query field.

| Track | Old exact query | Old results | New exact query | New results |
|---|---|---:|---|---:|
| El mañana | `track:"El mañana" artist:"Gorillaz"` | 8 | `track:"El mañana" artist:"Gorillaz"` | 8 |
| Feel Good Inc. | `track:"Feel Good Inc." artist:"Gorillaz feat. De La Soul"` | 0 | `track:"Feel Good Inc." artist:"Gorillaz"` | 9 |
| Dirty Harry | `track:"Dirty Harry" artist:"Gorillaz feat. Bootie Brown & San Fernandez Youth Chorus"` | 0 | `track:"Dirty Harry" artist:"Gorillaz"` | 8 |
| DARE | `track:"DARE" artist:"Gorillaz feat. Shaun Ryder & Rosie Wilson"` | 0 | `track:"DARE" artist:"Gorillaz"` | 7 |

For every row Album Artist and canonical primary Artist are Gorillaz. The primary
comes from the **first ordered Track Artist-credit row's canonical Artist**, with
existing external identities; no punctuation parsing. The database already had
separate contributor rows before the fix. If Track credits are absent, a single
structured Album Artist supplies query identity. Without reliable structure, the
original display query remains. Stored Artist names, credited names and joins are
never rewritten.

Expected candidates now visible include Feel Good Inc. `0d28khcov6AiegSCpG5TuT`
with `[Gorillaz, De La Soul]`, and `72YCeOZX7NoyRPJGyGbTHs` with `[Gorillaz,
Dave Jolicoeur, Kelvin Mercer, Vincent Mason]`; Dirty Harry's two Album occurrences
both use `[Gorillaz, Bootie Brown]`; DARE's two use `[Gorillaz, Shaun Ryder,
Roses Gabor]`. El Mañana still exposes both original Album objects.

All four complete result pages assess as **NeedsSelection**, not unique. Candidate
validation requires primary identity, musical title, Album context, supplied
position/duration compatibility, and equivalent structured credits. A plausible
alternative with unresolved contributors prevents automatic acceptance of another
candidate. Unknown group/member or alias relationships are not asserted. Manual
selection remains explicit and authoritative; provider rank is not acceptance.

## Part B: Album-program correlation

Previously `album_program::inspect_candidate` compared flattened Artist-credit
strings and literal musical-title representations. Both programs failed because
at least two present Tracks were unexplained. Six positions passed; nine failed.
Title failures were specifically Dirty Harry's `(feat. Bootie Brown)` on A and
DARE's `(feat. Shaun Ryder & Roses Gabor)` on both. All other failures were credits.
Capitalization is already comparison-safe; no general fuzzy title rule was added.

`artist_credit::compare` now distinguishes equivalent structured Artists, omitted
contributors, unresolved additional contributors, and contradictory primary/known
identities. Join punctuation is not identity. `A & B` as a single Artist name is
never split. Existing hyphen typography comparison handles Martina Topley‐Bird.
Same-primary but unresolved contributors can support a human program only with
exact musical title and position; they cannot independently supply occurrence IDs.
Different primary performers or conflicting known Artist IDs still reject.

A trailing `(feat. …)` / `(featuring …)` is a comparison-only annotation **only
when its full contents match that provider's ordered additional Artist names**.
Unverified suffixes and live/remix/version distinctions remain. Duration unknown
is not a mismatch; existing supplied-duration rules are unchanged.

The four Spotify search objects are unchanged:

| Object | Title/type/count/date | Before → after |
|---|---|---|
| A: `0bUTHlWbkSQysoM3VsWldT` | Demon Days / album / 15 / 2005-05-23 | program rejected → supported |
| B: `45IcwOkFv3YqTpU5Bal8fl` | Demon Days / album / 15 / 2014-04-11 | program rejected → supported |
| `2FHwmbCq5LVystFUzjgx78` | Demon Days (Gorillaz 20 Mix) / single / 1 / 2021-05-23 | title excluded; no program fetched |
| `0J3u0IeHzuymSz7v934vrd` | Demon Days Live at the Manchester Opera House / compilation / 17 / 2011-11-25 | live title excluded; no program fetched |

Both surviving programs now have 15 exact musical titles/positions, zero unexplained
Tracks, and no supplied local duration contradictions. The later catalog date does
not exclude B as a representation of this human Album. A has stronger contributor
spelling at Feel Good Inc., but B is not contradicted: unknown group membership is
not evidence to reject it. Neither is uniquely preferable under current whole-program
rules. No new ranking or date-based tie-break is introduced.

## Position-by-position evidence

All local durations are unknown; Spotify durations below are milliseconds. All
positions are disc 1. Full provider Artist IDs are retained in the checked-in
[recorded fixture](../tests/fixtures/spotify/demon-days-credits.json).

### Program A — `0bUTHlWbkSQysoM3VsWldT`

| Position | Application title / display credit | Spotify title / Artist list | Duration | Spotify Track ID | Old reason | New reason |
|---:|---|---|---:|---|---|---|
| 1 | Intro / Gorillaz | Intro / [Gorillaz] | 67133 | `2zavoMfVVPJMikH57fd8yK` | exact normalized title | exact normalized title and compatible Artist evidence |
| 2 | Last Living Souls / Gorillaz | Last Living Souls / [Gorillaz] | 195400 | `7JzmCjvB6bk48JghLyrg8N` | exact normalized title | exact normalized title and compatible Artist evidence |
| 3 | Kids With Guns / Gorillaz feat. Neneh Cherry | Kids with Guns / [Gorillaz, Neneh Cherry] | 225773 | `0eEgMbSzOHmkOeVuNC3E0k` | Artist disagreement | exact normalized title and compatible Artist evidence |
| 4 | O Green World / Gorillaz | O Green World / [Gorillaz] | 275000 | `4hNPMfFHauPIbOKvdYqFt7` | exact normalized title | exact normalized title and compatible Artist evidence |
| 5 | Dirty Harry / Gorillaz feat. Bootie Brown & San Fernandez Youth Chorus | Dirty Harry (feat. Bootie Brown) / [Gorillaz, Bootie Brown] | 230426 | `2bfGNzdiRa1jXZRdfssSzR` | title/semantic qualifier disagreement | exact musical title; featured suffix verified against structured credits |
| 6 | Feel Good Inc. / Gorillaz feat. De La Soul | Feel Good Inc. / [Gorillaz, De La Soul] | 222640 | `0d28khcov6AiegSCpG5TuT` | Artist disagreement | exact normalized title and compatible Artist evidence |
| 7 | El mañana / Gorillaz | El Mañana / [Gorillaz] | 235360 | `0dcMqjeDpwqB2xhzMsld0p` | exact normalized title | exact normalized title and compatible Artist evidence |
| 8 | Every Planet We Reach Is Dead / Gorillaz | Every Planet We Reach Is Dead / [Gorillaz] | 295266 | `2pw9RZWZibttZzoFhwjuy6` | exact normalized title | exact normalized title and compatible Artist evidence |
| 9 | November Has Come / Gorillaz feat. MF DOOM | November Has Come / [Gorillaz, MF DOOM] | 165093 | `6lrDckuosGpwEHtm1hHBcf` | Artist disagreement | exact normalized title and compatible Artist evidence |
| 10 | All Alone / Gorillaz feat. Roots Manuva & Martina Topley‐Bird | All Alone / [Gorillaz, Martina Topley-Bird, Roots Manuva] | 213333 | `76Ug1q4l6rGwJ2ubV5wh3X` | Artist disagreement | exact normalized title and compatible Artist evidence |
| 11 | White Light / Gorillaz | White Light / [Gorillaz] | 133066 | `7hnW9af7GuGH5lyUUTa8UH` | exact normalized title | exact normalized title and compatible Artist evidence |
| 12 | DARE / Gorillaz feat. Shaun Ryder & Rosie Wilson | DARE (feat. Shaun Ryder & Roses Gabor) / [Gorillaz, Shaun Ryder, Roses Gabor] | 244999 | `4Hff1IjRbLGeLgFgxvHflk` | title/semantic qualifier disagreement | exact title/position and primary Artist; additional Artists unresolved |
| 13 | Fire Coming Out of the Monkey's Head / Gorillaz feat. Dennis Hopper | Fire Coming out of the Monkey's Head / [Gorillaz] | 199000 | `1S9tfxdFr4TqoqA14gnKj3` | Artist disagreement | exact normalized title and compatible Artist evidence |
| 14 | Don't Get Lost in Heaven / Gorillaz feat. London Community Gospel Choir | Don't Get Lost in Heaven / [Gorillaz] | 131866 | `4hjGZN8poACfjWCK3Og6vY` | Artist disagreement | exact normalized title and compatible Artist evidence |
| 15 | Demon Days / Gorillaz feat. London Community Gospel Choir | Demon Days / [Gorillaz] | 268400 | `2k6hpKTyubRVOmQR11ViY3` | Artist disagreement | exact normalized title and compatible Artist evidence |

### Program B — `45IcwOkFv3YqTpU5Bal8fl`

| Position | Application title / display credit | Spotify title / Artist list | Duration | Spotify Track ID | Old reason | New reason |
|---:|---|---|---:|---|---|---|
| 1 | Intro / Gorillaz | Intro / [Gorillaz] | 63160 | `6kYEzcByL2fsOIBy3hjEeS` | exact normalized title | exact normalized title and compatible Artist evidence |
| 2 | Last Living Souls / Gorillaz | Last Living Souls / [Gorillaz] | 190400 | `6B1CTvwp5UHmsC7C5dUFyH` | exact normalized title | exact normalized title and compatible Artist evidence |
| 3 | Kids With Guns / Gorillaz feat. Neneh Cherry | Kids with Guns / [Gorillaz, Neneh Cherry] | 225840 | `3p6CzPIM1oThkSAVwYMNL2` | Artist disagreement | exact normalized title and compatible Artist evidence |
| 4 | O Green World / Gorillaz | O Green World / [Gorillaz] | 271960 | `0JY0InSgMgbL4ndXwMeVMO` | exact normalized title | exact normalized title and compatible Artist evidence |
| 5 | Dirty Harry / Gorillaz feat. Bootie Brown & San Fernandez Youth Chorus | Dirty Harry / [Gorillaz, Bootie Brown] | 223800 | `5LMfXbPn6VF5IusbdW1dmr` | Artist disagreement | exact normalized title and compatible Artist evidence |
| 6 | Feel Good Inc. / Gorillaz feat. De La Soul | Feel Good Inc. / [Gorillaz, De La Soul, Dave Jolicoeur, Kelvin Mercer, Vincent Mason] | 221173 | `72YCeOZX7NoyRPJGyGbTHs` | Artist disagreement | exact normalized title and compatible Artist evidence |
| 7 | El mañana / Gorillaz | El Mañana / [Gorillaz] | 230026 | `6Q2B0I3t6oXSVBKfJJYiFZ` | exact normalized title | exact normalized title and compatible Artist evidence |
| 8 | Every Planet We Reach Is Dead / Gorillaz | Every Planet We Reach Is Dead / [Gorillaz] | 293266 | `0XpcgA4TlJAH3EpiyZhCeQ` | exact normalized title | exact normalized title and compatible Artist evidence |
| 9 | November Has Come / Gorillaz feat. MF DOOM | November Has Come / [Gorillaz, MF DOOM] | 161106 | `6VAIvE8McCUpZEqba1pSWj` | Artist disagreement | exact normalized title and compatible Artist evidence |
| 10 | All Alone / Gorillaz feat. Roots Manuva & Martina Topley‐Bird | All Alone / [Gorillaz, Martina Topley-Bird, Roots Manuva] | 210066 | `25VvZQmr3OwrBrjZDB2KP0` | Artist disagreement | exact normalized title and compatible Artist evidence |
| 11 | White Light / Gorillaz | White Light / [Gorillaz] | 128426 | `7m6fzGmgWlROIR4RDPShMS` | exact normalized title | exact normalized title and compatible Artist evidence |
| 12 | DARE / Gorillaz feat. Shaun Ryder & Rosie Wilson | DARE (feat. Shaun Ryder & Roses Gabor) / [Gorillaz, Shaun Ryder, Roses Gabor] | 244306 | `2WEe9qVLhqZHOFoykgVlNu` | title/semantic qualifier disagreement | exact title/position and primary Artist; additional Artists unresolved |
| 13 | Fire Coming Out of the Monkey's Head / Gorillaz feat. Dennis Hopper | Fire Coming out of the Monkey's Head / [Gorillaz] | 196426 | `1PSmZhEQks1E4OOBqSjdjh` | Artist disagreement | exact normalized title and compatible Artist evidence |
| 14 | Don't Get Lost in Heaven / Gorillaz feat. London Community Gospel Choir | Don't Get Lost in Heaven / [Gorillaz] | 120373 | `1sIBFDvOJacc9n4GPxYAK6` | Artist disagreement | exact normalized title and compatible Artist evidence |
| 15 | Demon Days / Gorillaz feat. London Community Gospel Choir | Demon Days / [Gorillaz] | 268893 | `3cQtzv5263FpoTaDmGudRm` | Artist disagreement | exact normalized title and compatible Artist evidence |

## Result, request bounds, persistence and playback

Fresh production AlbumMatcher result: **2 plausible human-Album representations,
no unique Spotify Album ID, 0/15 automatic Track associations**. Every Track in the
tables remains unresolved because A and B disagree on its Spotify Track ID. DARE
also has unresolved Rosie Wilson/Roses Gabor evidence; B's Feel Good Inc. has
unresolved group/member evidence. The matcher does not manufacture those identities.

Album processing uses **4 catalog HTTP requests + 1 token request**: Artist search,
Album search, two Track-program pages. MusicBrainz import uses 3 separate requests.
This is unchanged from the baseline diagnostic. No new pagination or catalog scan.

Four explicit diagnostic Track searches use **4 catalog requests + 1 token request**
with one bounded page of at most 10 results each. All are ambiguous. Production
**automatic per-Track fallback bound remains zero**: no useful automatic acceptance
was demonstrated, so no 15-request Album sweep was added. Resolving every remaining
Track through the chooser could require up to 15 individual user-initiated searches;
only four were investigated here, through the existing shared search implementation.

Closing and reopening the fresh worker database preserves unresolved state and all
15 routes return NeedsEnrichment, with **zero catalog HTTP during routing**. No
successful Demon Days audio playback is claimed, since no automatic ID is safe.
Positive SQLite tests establish a unique structured-credit association, reopen it,
and route to Spotify without a catalog client. Existing independent Album-ambiguous
Track persistence, manual priority, Waitress route, mixed queue and no-polling-search
regressions remain covered. No Recording or exact edition is inferred.

Raw disposable before evidence: `/tmp/demon-credit-before-x5woIm`; after forensic,
explicit searches and fresh real worker: `/tmp/demon-credit-after-Qrb9iz`.

## Performance

Optimized local comparison of three candidate programs: 5 Tracks 29.9 µs,
15 Tracks 262.3 µs, 30 Tracks 956.6 µs. Independent persistence benchmark (Artist +
3 agreed Track IDs, revalidation and commit): median 307.8 µs; corresponding
three-program comparison median 155.9 µs. Network counts are unchanged.

One fresh 200k-Track check: selected song evidence median 27.38 µs (the preceding
slice measured about 9 µs before loading structured credits), persisted playback
route 23.04 µs, first library page 143.23 µs, Album Track verification 14.48 µs.
The added indexed, selected-Track credit query costs microseconds, not a library
scan. Existing slow diagnostic filter shapes remain separate known limitations.

## Validation results

- Core: 228 passed, 9 opt-in ignored; Spotify: 33 passed; MusicBrainz: 19 passed,
  3 live opt-in ignored; GStreamer: 5 passed, 1 live ignored; QML: 5 passed,
  2 live opt-in ignored. QML's integrated test exercises source resolution,
  persisted-association search bypass, ambiguous explicit-Play selection, manual
  authority, mixed queue controls and polling without catalog search.
- Structured-credit tests cover representation-only joins, identity conflicts,
  unresolved contributors, verified/unverified suffixes, unchanged display metadata,
  canonical-primary query construction, unique versus ambiguous acceptance and
  real-SQLite persistence/reopen. The recorded 15-Track fixture checks both programs
  correlate without choosing their disagreeing IDs. Existing program tests preserve
  unique supported-representation selection, Super Fx/Fxx and standalone and/&.
- Formatting, strict Clippy (core, Spotify, MusicBrainz, QML all targets; QML all
  features), qmllint, offscreen QML integration smoke and `git diff --check` pass.
- Fresh live controls in `/tmp/demon-live-9lyRZE`: Painted Shut **10/10** (including
  Waitress `6aYsKS9XcItrOUWNMLn3Ba`), Trash Generator **12/12** (one-track Single
  excluded, Super Fx retained separately), Hella **4/4**. All stored associations
  reload identically without provider requests. Wretches still follows Spotify
  NoConfidentMatch → MusicBrainz Matched, including Bride And Groom / Bride & Groom.
- A redundant combined live run stopped at MusicBrainz HTTP 503 before controls;
  the separate controls above and earlier complete fresh Demon Days runs succeeded.

The remaining major coverage obstacles are conflicting provider occurrence IDs,
missing imported duration observations, and unestablished contributor aliases/group
membership. This change does not invent those facts or relax manual selection.
