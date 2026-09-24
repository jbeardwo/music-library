# Established Spotify Album occurrence association

Validated 2026-09-23 using fresh disposable databases; no commit.
This supersedes the 14/15 result in [the Album-context audit](spotify-album-context-audit.md).

## Cause and rule

DARE previously passed musical-title and position comparison, but structured
credit comparison returned `DifferentAdditional` (Rosie Wilson versus Roses Gabor).
`inspect_candidate` consequently set `occurrence_supported = false`, and `compare`
withheld its provider Track ID. Its exact diagnostic was:
“exact title/position and primary Artist; additional Artists unresolved”.

After `complete_album_program` revalidates the accepted Spotify Album identity and
unchanged application Track snapshot, an otherwise unresolved Track may receive
an occurrence association when its known Track number and disc match explicitly,
and its musical title matches under the existing safe comparison. This fallback
requires exactly one common Spotify Track ID across every complete supplied program.
It does not use flattened list position to excuse a conflicting provider position.
Title differences and semantic variants remain disqualifying. Featured annotations
are removed only by the existing provider-credit-verified title comparison.

This rule is restricted to established Spotify Album objects. It cannot establish
an Album preference, alter multi-Artist Track search, or treat a MusicBrainz release
group as an established edition. Manual choices retain precedence. Existing
successful comparisons retain their behavior. Persistence uses the existing
`provider_track_association` row, with no new schema, Recording, contributor/alias,
or exact-edition identity. Contributor disagreement is not an occurrence veto.

## DARE evidence and live result

| Evidence | Value |
|---|---|
| Application Album | Gorillaz — Demon Days, 2005 |
| Preferred Spotify Album | `0bUTHlWbkSQysoM3VsWldT`, 2005-05-23 |
| Nonpreferred representation | `45IcwOkFv3YqTpU5Bal8fl`, 2014-04-11 |
| Application/provider position | Disc 1, Track 12 |
| Application title | DARE |
| Provider title | DARE (feat. Shaun Ryder & Roses Gabor) |
| Safe comparison title | DARE |
| Application credit | Gorillaz feat. Shaun Ryder & Rosie Wilson |
| Provider credit | Gorillaz, Shaun Ryder, Roses Gabor |
| Persisted Spotify Track ID | `4Hff1IjRbLGeLgFgxvHflk` |

The established Album, explicit matching position, and matching musical title
establish the Track occurrence despite the unresolved contributor relationship.
The production worker automatically persisted **15/15** Tracks. A separate fresh
forensic probe independently verified both Spotify programs and the credits above,
and also persisted **15/15**. Neither run performed individual Track searches.

Closing/reopening the worker database recovered all 15 IDs. Every playback route
was `Remote` using the persisted ID, with **zero catalog HTTP requests** during
reconstruction/routing. This validates playback resolution, not live audio output.
DARE's persisted association has `recording_status: NotProvided`, empty Recording
identities/ISRCs, and an explanation stating contributor identity is unresolved.
Rosie Wilson retains only MusicBrainz Artist `8020c8b6-5363-4b4e-b552-c8a2bd7cd5a8`.
The database contains zero Spotify Recording identities and zero Spotify edition
identities. Existing MusicBrainz identities remain intact.

## Regressions and checks

- Focused SQLite tests cover credit disagreement, unchanged contributor evidence,
  different/unknown positions, different discs, different titles, Remix/Live,
  unresolved/wrong Album, manual precedence, restart, catalog-free playback,
  and absence of Recording/exact-edition inference.
- Recorded Demon Days fixture verifies the original `occurrence_supported = false`
  condition and all 15 persisted occurrences after establishing the representation.
- Existing unresolved two-representation fixture still selects no arbitrary IDs.
- Core suite: 236 passed, including structured multi-Artist search, source resolver,
  mixed queue/controller, manual precedence, and persistence regressions. Focused
  candidate tests reran successfully after final ID-kind restriction and unknown
  application-position coverage.
- Spotify adapter: 33 passed. Sandbox mock-socket binding was denied initially;
  the required rerun outside the sandbox passed.
- QML all-feature tests: 5 passed, 3 opt-in live tests ignored. QML lint and integration
  smoke passed. Qt's existing cross-thread timer teardown warning remains.
- Formatting, strict Clippy (core and Spotify all targets; QML all targets/all
  features), and `git diff --check` passed.

| Live control | Result after reopen |
|---|---|
| Painted Shut | 10/10 Spotify associations |
| Trash Generator | 12/12 Spotify associations |
| Hella — Bitches Ain't Shit But Good People | 4/4 Spotify associations |
| Wretches | No confident Spotify match; MusicBrainz fallback succeeds and reconstruction is identical |

No remaining automatic Spotify failure was observed in Demon Days or the three
positive controls. Wretches remains the expected Spotify-unmatched control.

Session artifacts: `/tmp/demon-live-U55kHQ/` (worker and controls),
`/tmp/demon-occurrence-evidence.log`, `/tmp/demon-occurrence-evidence-20260923.sqlite`,
and `/tmp/occurrence-*.log` (checks). Normal user-library databases were untouched.
