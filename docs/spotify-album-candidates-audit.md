# Spotify Album candidate audit

Live Web API diagnostics, 2026-09-14, configured market US. All requests returned
200; no Development Mode access restriction was encountered. These are bounded
search observations, not proof of worldwide catalog presence/absence. Diagnostic
examples never print tokens or secrets. Local titles and membership remain unchanged.

## Candidates and decisions

| Local Album / resolved Artist | Spotify Album ID | Returned title | Type | Tracks | Date | Decision |
|---|---|---|---|---:|---|---|
| Trash Generator / Tera Melos | `410EeFNFvaM9YlJnZa00AD` | Trash Generator | album | 12 | 2017-08-25 | Exact Artist/title; accommodates 12 local Tracks; selected |
| Trash Generator / Tera Melos | `0vwojUeXlI9Gtxt1hZZJg9` | Trash Generator | single | 1 | 2017-08-16 | Exact Artist/title, but cannot contain 12 present Tracks; rejected before programs |
| Wretches / Hop Along | none returned | — | — | — | — | Targeted first page empty, no next page |
| Bitches Ain't Shit But Good People / Hella | `2ae0BqiRav1HsMfletBNSh` | Bitches Ain't Shit But Good People | single | 4 | 2003-03-31 | Full-title search empty; token discovery returns exact Artist/title; selected |
| Painted Shut / Hop Along (control) | `7bR9KYRb6jfhlle5Y9U4BD` | Painted Shut | album | 10 | 2015-05-04 | Still 10/10 local song associations |

Resolved Artists: Tera Melos `3K4vimkwmCyjD4g1hEMPjZ`, Hop Along
`3yYUV3hkJit05YIUEODqgp`, Hella `1n861RIk6CTAWncgHR9UHg`. All are unique exact
primary-name matches; no Artist tolerance changed. Trash Generator's original
ambiguity arose because both objects passed Artist/title matching and the core
ignored `total_tracks`. Type alone is insufficient: Hella's valid four-song object
is also a single.

Wretches: `album:"Wretches" artist:"Hop Along"` returned zero. The title-only first
page contained unrelated Artists and had another page; we did not enumerate it.
A separate `artist:"Hop Along, Queen Ansleis"` control also returned zero. Thus the
failure is candidate discovery, before title/Artist filtering or program retrieval;
market unavailability versus Spotify indexing cannot be conclusively distinguished.
`NoConfidentMatch` remains appropriate for the available evidence.

Hella: both the quoted full-title Artist-scoped query and a full-title-only control
returned zero. Separate `album:"Good People" artist:"Hella"` and
`album:"Bitches" artist:"Hella"` controls returned the same exact-title object.
This is an observable query/indexing failure, not censorship, title tolerance,
Artist identity, pagination, or market absence. The adapter now permits one
longest-token Artist-scoped fallback after an empty complete multiword search.
The core still compares the complete original title. No profanity-specific rule exists.

## Retrieved programs

The initial diagnostic fetched both Trash Generator programs. The production fix
fetches only the full Album program. All positions below are disc 1; durations are
provider milliseconds. IDs are Spotify song occurrences, never Recording IDs.

| Track | Full Trash Generator program | Duration | Song ID |
|---:|---|---:|---|
| 1 | System Preferences | 216533 | `3QnCm1WofA1nsWO3CWFu3t` |
| 2 | Your Friends | 202560 | `6z7kTcLAjHALSliaajeWQG` |
| 3 | Trash Generator | 201840 | `42A5Pf1NmT9ZXoJ3KqR0cH` |
| 4 | Warpless Run | 152480 | `1wfVq4nxgqz5KTO8QwN1TC` |
| 5 | Dyer Ln | 313946 | `2ACBm25gKbfDOLfNO6QyUu` |
| 6 | GR30A11 | 130119 | `2F4LP8UF77PXy8rWuZs5L8` |
| 7 | Men's Shirt | 251093 | `1Dq3jdWA0QQNpOPkOjp79n` |
| 8 | Don't Say I Know | 204693 | `7nqglga0KG4R3IBvx9I8uF` |
| 9 | A Universal Gonk | 371493 | `1n9C84a3rZ6G2meOHzGmtL` |
| 10 | Like a Dewclaw | 118573 | `7CUvmO4CxVDJd1URbeQM99` |
| 11 | Drawing | 161960 | `7FP7OGAzvCYQYP6dsUYr8m` |
| 12 | Super Fx | 191373 | `6eAGs4KAWuAVJ8Arhfscv6` |

The single contains only track 1, Trash Generator, 201840 ms,
`5uJPvgDuLbGZOImC5Lzosn`.

| Track | Hella program | Duration | Song ID |
|---:|---|---:|---|
| 1 | Ho's In The House | 86533 | `6Pw8zMsS2icQgQjp7WmIR0` |
| 2 | Bitches Ain't Shit But Good People | 433600 | `2E9rUgx60gsHbWzZjVEsN4` |
| 3 | Rich Kid | 279666 | `7ofuf3FeQSbl9QCpJurkTv` |
| 4 | D. Elkan Sings Republic Of R & R | 226733 | `4RFOsHEg1Cir6knf3FlmaT` |

## Rules, bounds and results

The generic candidate resolver uses cheap count impossibility first, then at most
three candidate programs. Extra provider Tracks are not penalized. Complete programs
are excluded for too few Tracks, trusted Recording conflicts, at least two unexplained
present Tracks or at least two incompatible durations. One weak disagreement or
incomplete provider evidence remains uncertain. A sole fully supported candidate
needs three exact position/title agreements to win program disambiguation. No
numeric confidence, provider score, order, completeness requirement or edition claim
is introduced. A fetched winning program is reused by enrichment and the manual picker.

Equivalent programs expose Album/Track association without selecting a catalog ID.
Only agreeing song identities are shown as known; different IDs remain unresolved.
This diagnostic state is ephemeral, with no new persistence model. MusicBrainz's
existing discovery stays unchanged because it does not enable the additive capability.

Live results: Trash Generator 12/12, Hella 4/4, Painted Shut 10/10. All persisted song
associations survived close/reopen with no metadata reread or provider request for
reconstruction. No exact Release or Recording identity was inferred.

Trash Generator used token + Artist search + Album search + one Tracks page:
115.6 / 331.0 / 303.7 / 118.9 ms; workflow 0.878 s. Zero extra program requests
for disambiguation were needed. Hella required one additional token-title search:
token 132.9 ms, persisted Artist lookup 340.0 ms, full search 251.6 ms, fallback
226.3 ms, Tracks 118.5 ms; workflow 1.075 s (endpoint timing includes initial token
acquisition). Its later restart used only token + Tracks. Painted Shut's restart
control took 0.298 s and still loaded 10 associations before network.
A final fresh-database Painted Shut control also resolved Artist and Album anew,
matched 10/10, and passed restart verification: token 125.0 ms, Artist search
723.3 ms, Album search 499.2 ms, one Tracks page 477.6 ms; workflow 1.835 s.

Local release-build comparison of three programs against 5/15/30 present Tracks
averaged 0.023/0.229/0.870 ms, excluding network. The 200k fixture retained indexed
Album lookup (6.45 µs), 10-Track verification (14.70 µs), and first library page
(140.17 µs). No schema/index changes or global reconstruction scans were added.

Validation: 202 core tests, nine Spotify HTTP mocks, 19 MusicBrainz tests, five
GStreamer tests and five QML tests passed. Opt-in live/performance tests remain
excluded from ordinary runs; the candidate timing and 200k checks were run explicitly.
Both provider QML smoke modes, QML lint, Rust formatting, strict Clippy and diff
checks passed. The existing Qt test teardown warning about cross-thread timer
destruction remains; no new test failure was observed.
