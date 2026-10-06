# Spotify reconciliation quality audit

Validated on 2026-10-05 against a SQLite backup of the real library, using the
existing Spotify Client Credentials configuration. Live requests and accepted
associations were confined to the five selected Albums on the disposable copy.
No library entities were deleted and existing confirmed associations were retained.

## Real evidence and outcomes

| Local Album | Previous failure | Current evidence and outcome |
| --- | --- | --- |
| Floral — The Second Floral EP | The title variant check strips EP only when provider type is EP; Spotify returns Single. Hyphen/spacing prevents the remaining literal comparison. | One candidate, established Artist `1FVOt1XlpnaCueBolWF92k`, Album `56WclCoxH3fyAe1Z8QmbKu`, Single, 2015-11-01, 6/6 positioned title anchors. Release-type presentation normalized; 6 persisted Track associations. |
| Hop Along — Freshman Year | Trusted Artist filter removes both results. Even without that filter, neither program accommodates all local positions. | Albums `1PtvYmXJV1FJBSDEsDbTs3` (2020-09-03) and `3ctenyhN6B5SQhRXy9Q5ux` (2020-09-04), both Album, credit Hop Along, Queen Ansleis (`7hDLm8O99kRfpY3DjFHQAP`), differ from established Hop Along (`3yYUV3hkJit05YIUEODqgp`). Both have 15 Tracks versus 16 local, omit Organ Song at position 9 and shift the rest. Their titles/positions agree with each other, but they share **0/15 Spotify Track IDs**. Not equivalent under the new safe rule; unresolved. |
| Bygones — Spiritual Bankruptcy | Artist discovery has Bygones/BYGONES homonyms; existing joint Artist/Album corroboration resolves the correct Artist. Album selection succeeds, but all five local disc numbers are `0`; established occurrence matching searches disc 0 against Spotify disc 1 and returns no occurrences. Baseline: 5 exact titles, 0 exact positions. | Artist `2QzoDw6X9beHUD8XVvfkDn`, Album `5MU76HzwG5pukQBZwIeYlw`, Single, 2010-02-16. Treating unspecified disc 0 as disc 1 **for comparison only** yields 5/5 anchors and 5 persisted Track associations. The real local program is five Tracks, rather than the illustrative eight. |
| Tabar — Hugs EP | Local EP stripping requires explicit provider EP, but Spotify categorizes the candidate as Single. | One candidate, Artist `3TzXqe05P5HxuZLDAMpaQU`, Album `1gP2qfXC2JIhx0Q4iVAIOQ`, Single, 2010-02-28, 3/3 anchors. Single is compatible with an EP hint, not proof that Spotify explicitly calls it an EP. Strong complete program permits suffix normalization; 3 persisted Track associations. |
| Piglet — Songs | Artist homonyms are unresolved; joint Album corroboration fails on the meaningful live qualifier. | Candidate `4G6dwnwGKwstVqZzERctAD`, Songs EP (Live in Chicago), Artist `7eMopflZIkcPF4refj6MNU`, Single, six Tracks versus nine local. Diagnostic independently records unestablished Artist, Album version mismatch and track-count mismatch. No automatic association. Manual connection remains available. |

The current live Floral response spells **Floral**, not the previously observed
**Floal**. The live pass therefore establishes release-title formatting support;
the reported one-character typo is covered by generic deterministic fixtures,
not claimed as a current live observation. No names above are hard-coded into rules.

Before/after copy comparison preserved all 762 Track records and IDs, 190 Release
IDs, 473 library membership records, 473 source relationships and local availability
observations, and all 291 PlaylistEntry identities, positions and canonical Track
relationships. Only intended provider associations/Artist resolution were added.
The source library was not modified by this validation.

## Comparison rules and safeguards

Album representation comparison uses Unicode NFC, case, spacing and punctuation
normalization. EP/LP are removable only as trailing release-type presentation with
compatible provider metadata. Spotify Single is the compatible Singles/EP bucket;
unknown/conflicting types cannot support suffix equivalence. Qualifier words remain
in normalized titles and existing semantic version checks remain protective.

Nonliteral title acceptance requires at least three independent positioned title
anchors and all but at most one required local position. Missing, duplicated or
shifted positions, trusted identity conflicts and version changes block selection.
One-edit title tolerance uses the existing bounded distance mechanism, minimum five
characters, established Artist, strong program support, no duration contradiction,
and no comparably supported competitor. It is never a catalog-wide fuzzy query.
One candidate still passes all evidence checks; incomplete competitors veto
uniqueness rather than disappearing from the decision.

Equivalent provider objects must share a complete ordered program with identical
Spotify Track IDs at every position, matching Artist identities, Album title and
release type, and compatible year/month date context. Preferred representation
uses existing local date evidence and then stable provider ID order. This identifies
a usable provider representation, not an exact edition or a Recording identity.
Different Track IDs are not silently equated. Existing safe agreed-occurrence
handling remains available without preferring an ambiguous Album object.

## Diagnostics and retry

The actual matcher emits structured candidate evidence and reason codes: Artist
unresolved/mismatch, title/version/type/count/position mismatch, insufficient
anchors, trusted identity conflict, incomplete program/page, comparison bound,
duration conflict, competing candidates, exact/normalized/suffix/typo acceptance,
equivalent provider collapse, and safe equivalent occurrences. Raw titles, IDs,
release type/date, counts, positions, trusted identities and selected-Track program
evidence remain separate from the decision.

The existing Song diagnostic renders this evidence and provides explicit bounded
Album re-evaluation for unresolved Tracks. Both row and Now Playing entry points
use the same stable Track-ID API; PlaylistEntry uses its canonical Track relation.
Opening the dialog does not request matching or change playback. Retry uses the
existing matcher, preserves trusted associations, and never queues or plays a Track.
Reports represent the last session evaluation; they are bounded to sixteen Albums,
not durable new matching state.

Search remains Artist-context scoped. Case-equivalent search names can share a
query but returned Artist IDs are still enforced. Rejected raw Album candidates
remain visible as evidence without becoming eligible. A ten-object page, at most
three candidate program requests, and a four-program worker cache bound work.
Selected programs are reused for occurrence enrichment; no per-Track network
requests, transactions containing network work, full-library retry or matching on
Play were introduced.

## Deferred

Tera Melos — Drugs / Complex remains unchanged and excluded from targeted retry.
Compilation decomposition and one Spotify playable Track serving multiple local
Track occurrences require future occurrence/Recording/source cardinality decisions.
No compilation exception, Artist alias inference, Recording merge, exact-edition
inference, source priority, membership, playlist or Track identity redesign is part
of this pass. Freshman Year remains unresolved for the real conflicts above.

## Reproduction

`adapters/spotify/examples/reconciliation_evidence.rs DATABASE ALBUM_ID...` is a
bounded read-only probe by default. `--apply` explicitly applies accepted results
through existing completion APIs; use a backup for validation. The probe prints
structured and readable evidence and audits raw provider programs separately from
eligible candidates. Ordinary UI retry is the existing connection diagnostic.

Detailed run logs were retained in `/tmp/reconciliation-baseline-complete.log` and
`/tmp/reconciliation-after-live.log`; credentials are not logged.

## Validation results

- Core suite with `--no-fail-fast`: 338 passed, 15 opt-in ignored; one existing
  `catalog_playlist_identity_membership_reload_and_duplicates` assertion fails
  because it expects an empty Artist credit instead of the preserved credited
  string. Reproduced on unchanged HEAD in a separate baseline checkout.
- Focused Album candidate, established occurrence and local matching suites:
  70 passed, four ignored, including eight new reconciliation regression tests.
- Spotify adapter suite: 47 passed, three live opt-in ignored. Local mock listeners
  required execution outside the filesystem sandbox.
- QML suite, each test in its own process: 26 passed, 13 opt-in ignored; the existing
  `interactive_add_music_qml` assertion about missing metadata/source presentation
  fails, also reproduced on unchanged HEAD. One-process execution is unreliable
  because this Qt build retains application/thread state across tests.
- Explicit live QML diagnostic test: all five real Albums produced the expected
  association/rejection and rendered specific evidence without changing the
  current Track, queue or playback state. The selected-Track deterministic test
  verifies one Artist search, one Album search and one reused program request,
  no request on opening, and no churn/request when retrying a trusted association.
- Local-first playback, missing-file Spotify fallback, canonical PlaylistEntry,
  sorted/scrolled Song targeting and manual selection isolation passed their
  desktop regressions. The prior real Hop Allong Playlist audit remains valid;
  this pass adds no playback or playlist policy change.
- Strict Clippy for core, Spotify adapter and all-feature desktop targets,
  formatting checks for all three manifests, and `git diff --check` passed.

Physical interactive listening was not repeated for this matching-only pass;
real-provider validation ran through the actual QML dialog offscreen on copies.
No commit was made.
