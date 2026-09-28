# Established Spotify program occurrence audit — 2026-09-24

Fresh MusicBrainz catalog Add imports, Spotify market US, disposable `/tmp` databases.
No existing Spotify associations were used. No user library was modified; no commit.

## toe: discovery discrepancy, not an occurrence/title failure

Before: **0/13**, `MatchOutcome::NoConfidentMatch`, no accepted Spotify Album.
`AlbumMatcher::after_import` schedules the worker normally. Its
`Spotify::search_artists` request (`type=artist`, `q=artist:"toe"`, limit 10)
returned ten unrelated Artists, including Artist Joe Smith and RAC, but no toe.
`resolve_artist` correctly rejects them. Album candidate generation, program
comparison and occurrence persistence are therefore never reached.

The read-only control `q="toe"` returns toe `0rpKM0MniNkXM1SLSglYUZ`.
Using that ID diagnostically (without accepting it into the database), Artist-scoped
Album search returns one exact For Long Tomorrow representation. Its complete program
has all 13 positional titles corroborated by the existing comparator. There is no
kana-versus-English problem in this Album.

| Evidence | Application / MusicBrainz | Spotify |
|---|---|---|
| Artist / credited Artists | toe | toe |
| Album | For Long Tomorrow | For Long Tomorrow |
| Date | 2009 | 2009-12-09 |
| Program size | 13 Tracks | 13 Tracks |
| Position | disc 1, Track 6 | disc 1, Track 6 |
| Title | Two Moons | two moons |
| Duration | unknown | 250,253 ms |
| Album identity | MB group `3a2e4685-550f-4b00-a233-4a7ac2957072` | `6li0rkNGD98cO8Gxf7nnSk` |
| Track occurrence | MB Track `ba19ce1b-3214-3b73-af30-3320f28edaed` | `6qcUFIPZU2sJXiI1sqzwZG` |
| Recording | MB `def3af34-586c-4f17-9b99-fb121b7e0a57`, ISRC JPZ920911820 | no Recording claim |

MB Artist: `7ea8a523-b33d-4944-ad31-fad25a81d603`;
representative Release: `695060cc-1a85-4dc9-8e85-aa50b74964ef`.

Manual `SongSearch::search_songs` instead submits a bounded Track-title/primary-Artist
query directly. It does not depend on the failed Artist lookup. For Two Moons it
returns one candidate, `Preferred`, no infeasibility reasons, `Assessment::Unique(0)`.
After Image likewise returns one preferred feasible candidate; its explicit-Play
assessment requires selection because Spotify omits the additional Harada Ikuko
credit. The established program can identify that occurrence without identifying
the contributor. This is a discovery-path bug for toe, not evidence that Track
matching must be relaxed to get toe's matching titles accepted.

Fix: retain the existing Artist query, retry once with quoted name only if no exact
normalized name was returned. The unchanged core Artist/Album matcher validates the
result. An unconditional replacement query was tested and rejected: it made other
Artists' pages incomplete and interfered with their established disambiguation path.
No generic Artist alias matching or per-Track search was added.

## tricot: established representation, title withholding occurrence

Album: T H E, application year 2013, MB group
`1ac233d7-9dfe-4727-a2d6-bd1eaa3376bb`; representative Release
`3e2e612c-0289-4db3-9ffe-33f57f93fa73`.
The [label's release listing](https://www.topshelfrecords.com/products/609314-tricot-t-h-e)
also lists Hatsumimi on this Album.

The production Artist search has a partial page with tricot. The existing
Artist/Album corroboration path establishes Spotify Artist
`5IKKS7LhpdlmMwqIagqf3f` and the unique Album `7zNn3gNnkOp2XywYEbQBQN`, dated
2013-10-02, with 13 Tracks. This is provider representation acceptance, not exact edition.

| Evidence | Application | Spotify |
|---|---|---|
| Position | disc 1, Track 8 | disc 1, Track 8 |
| Title | Hatsumimi | 初耳 |
| Credits | tricot | tricot |
| Duration | unknown | 238,053 ms |
| Track ID | MB `a9ccd459-28e3-406b-b15e-e54d06a9932a` | `1jCS7yVL3BO6XqSIfXQ3MT` |
| Recording | MB `bce4b7ec-b630-4c5f-bd30-e60b822cc89e` | none |

Before: **7/13**. Six exact positional title anchors are pool side (1), POOL (2),
C&C (6), 99.974℃ (9), CGPP (11), Swimmer (12). art sick/artsick (5) previously
passed positional spelling tolerance. Positions 3, 4, 7, 8, 10 and 13 have differing
title renderings. Disc, position, program length/order, credits and available
identities supply no contradiction. There are no explicit version qualifiers.
The old `inspect_candidate` rejects these titles, and `established_occurrence`
also requires `exact_title`, so the accepted Album cannot supply their occurrence IDs.
The old whole-program `fit` calls the program rejected solely because of these
uncorroborated strings; it is not evidence of a reordered or different musical program.

## Rule and boundaries

`complete_album_program` still revalidates the accepted provider Album and input
snapshot in its transaction. Spotify now evaluates occurrences with
`established_occurrences`, once per program, instead of requiring each Track to
independently match through the generic Recording/template comparator.

- Explicit matching disc/positive Track number and one provider occurrence are required.
- All complete supplied programs must agree on that occurrence ID.
- Exact musical title corroborates; uncorroborated title needs at least three other
  exact positional anchors. This is the existing program-corroboration threshold.
- Missing/duplicate placements, too-short programs, and known titles at different
  positions block structural rescue. Different Album IDs and stale snapshots fail earlier.
- Explicit Live/Remix/Demo/Acoustic/Instrumental/Radio Edit/Extended and existing
  version qualifier differences block the occurrence. Trusted Recording, occurrence
  or primary-Artist identity contradictions also block it.
- No Recording, exact edition, Artist alias, transliteration or title equivalence is
  inferred. Original application/provider title and credit observations remain unchanged.
- Manual associations remain authoritative. Conflicting already-persisted occurrence
  identities are not replaced. Partial membership requires no new/missing Tracks.

`TitleRelation` distinguishes agreement, no corroboration and semantic contradiction.
The chooser shares its safe musical-title comparison (including punctuation), but a
global search candidate does not carry a revalidated Album ID or surrounding program;
it still requires title corroboration. A single visible candidate is not a substitute
for that context. Super Fx/Fxx are not normalized into equivalence; known distinct
positions and disagreeing programs remain protected.

## Fresh results, cost and regressions

| Fresh import | Automatic result | Reopen |
|---|---:|---|
| toe — For Long Tomorrow | 13/13 | 13/13, all remote routes, catalog HTTP=0 |
| tricot — T H E | 13/13, including Hatsumimi | 13/13, all remote routes, catalog HTTP=0 |
| Gorillaz — Demon Days | 15/15 | identical |
| Hop Along — Painted Shut | 10/10 | identical |
| Tera Melos — Trash Generator | 12/12 | identical |
| Hella — Bitches Ain't Shit But Good People | 4/4 | identical |
| Wretches | Spotify no confident match; MusicBrainz fallback | identical |

Neither target has unresolved Tracks or needs manual search after the fix.
Toe uses 4 catalog requests plus one token request: original Artist query, one
name-only retry, Album search, program fetch. The retry adds **one** request;
program/occurrence comparison adds **zero**. Tricot uses the unchanged 3 catalog
requests plus one token request. No polling, playback or per-Track HTTP is added.

Debug measurements on these 13-Track programs: local occurrence comparison 0.383 ms
(toe), 0.609 ms (tricot); whole program completion including snapshot reads and
persistence 2.043 ms / 2.391 ms. No SQL, schema, index or transaction-lifetime design
change; no full-library query. These are observations, not performance budgets.

Checked-in provider/application fixtures reproduce both real programs. Focused tests
cover established exact titles, contributor differences, uncorroborated titles,
partial programs, insufficient corroboration, unresolved representation, incorrect
positions/discs, semantic versions, conflicting identities, wrong/incomplete/reordered
programs, conflicting program IDs, Super Fx/Fxx ordering, unchanged metadata, manual
precedence, reopen and routing without a catalog provider. Spotify HTTP tests cover
bounded retry and rejection when retry still supplies no matching Artist.

Validation: focused matching/persistence, Spotify adapter, provider fallback,
playback resolver/mixed queue, QML integration and live chooser suites; formatting,
strict Clippy and diff whitespace checks. The live chooser still shows 2005 preferred,
2014 alternate; unrelated candidates remain hidden until Show all. Qt retains its
pre-existing timer teardown warning after successful tests.

Reproduction tools: `album_resolution_probe ... --worker` for a new `/tmp` database;
`established_program_probe ARTIST_ID ALBUM OUTPUT_JSON` for read-only program evidence;
`track_credit_probe TITLE...` for diagnostic manual search, never confirmation.
Session evidence: `/tmp/toe-worker-before.log`, `/tmp/toe-manual-before.log`,
`/tmp/toe-program.log`, `/tmp/tricot-worker-before.log`, `/tmp/tricot-program.log`,
`/tmp/toe-final.log`, `/tmp/tricot-final.log`, `/tmp/demon-live-xpRFkK/`, and
`/tmp/occurrence-live-chooser.log`.

## Resumed verification — 2026-09-27

The resumed environment retained source/build files but not the earlier `/tmp` logs
or Spotify environment credentials. Final local verification passed: 106 core tests
across the focused suites, 34 Spotify mock tests, five QML tests, formatting, strict
Clippy for core/Spotify/QML (all targets; QML all features), and `git diff --check`.

Fresh live reruns completed MusicBrainz import but then stopped with
`Set SPOTIFY_CLIENT_ID for Spotify catalog matching`; the live QML catalog-add check
also failed because enrichment could not be configured. The 13/13 target results,
control results and timing measurements above are the successful September 24
observations, not new September 27 live results. The checked-in real-program fixtures
still pass against the final code, including restart and zero-catalog playback.
The original temporary evidence paths above are historical; the recreated
`/tmp/toe-final.log` and `/tmp/tricot-final.log` now record the configuration failures.
