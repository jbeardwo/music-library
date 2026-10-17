# Spotify exception inbox follow-up

This follow-up preserves the self-draining coordinator and canonical Release/Track
identity. No changes were committed. Validation uses disposable SQLite backups,
not the running application's database.

## Terminal decisions and bounded discovery

Local Track replay previously marked absent standalone Track pages `needs_retry`,
even after successful Album discovery had already produced reviewable candidates or
a successful negative result. Album-context replay now restores the current
structured Album report after local convergence. Opening Review also persists the
same current cached decision before exposing it. Provider-unavailable work receives
an explicit waiting reason rather than a vague retry instruction.

The standalone Track search allowance remains two calls per reconciliation run.
Previously an adapter regression explicitly expected a second immediate click to
search two more Tracks. Unchanged bulk fallback scopes now retain an exhausted
provider-search window in discovery storage. This is **not** a claim that every
Track was searched, and it is separate from successful negative discovery. Matching
metadata, catalog/Artist identity evidence, evaluation strategy changes, the normal
30-day discovery TTL, or an explicit Album Search again reopen the scope. Generic
Retry still reevaluates cached/local evidence; it does not silently reset a provider
safety allowance. Track fallback pages use a cursor so the internal 20-row bound
cannot hide a later eligible Track.

## Trusted catalog names

Normal MusicBrainz matching acquires Artist details with `inc=aliases` after
conservative catalog identity resolution. Existing catalog-associated Albums also
support missing Artist-name enrichment. Migrations 34/35 retain preferred names,
aliases and exact catalog-attested provider-name bindings with catalog identity
provenance. These are separate from user-confirmed Artist equivalence and raw
provider-observed credits. Canonical names and credits are preserved.

Trusted alternate names participate in bounded provider discovery and evaluation.
A cached provider credit can establish a binding only through an exact attested
catalog name, a unique provider Artist identity, a complete strong program, and
non-conflicting trusted occurrence/Recording evidence. Temporary comparison
context breaks the circular dependency in which a program could not corroborate
an alias because its Artist ID was not yet bound. No fuzzy alias is inferred.
Catalog requests are deduplicated per Artist within a run; new identity evidence
reenqueues that Artist's earlier contexts through the existing bounded cursor.

## Matcher and review

Review prioritizes the actual automatic decision, primary blocker and preferred
candidate. Connect/search/manual/exclusion actions precede the detailed program.
A preferred Album with an absent local Track now reports that missing occurrence
rather than an unrelated alternative's blocker or an acceptance reason.

Album shared-title-component comparison requires strong program context and no
semantic version qualifiers. Work's raw local title is `Work`; Spotify's is
`工作 Work`. Neither contains a version qualifier. The old displayed version
conflict was stale persisted evidence; the current rejection was Album title
mismatch. The safe contextual shared component fixes that rejection without
rewriting either title or associating canonical Release identity.

Punctuation-only titles are comparison-compatible only under independent strong,
unique Album/program/position and duration evidence. `_` versus `-` does not become
a global title equivalence. Unicode punctuation is supported while letters,
numbers, controls and combining marks are not silently discarded. Meaningful
Live/Acoustic/Demo qualifiers and trusted Recording conflicts remain blockers.

When equivalent complete ordered programs and Album Artist identity agree, the
candidate with fewer additional Track Artist credits dominates. A structurally
weaker/incomplete program, different primary Artist or conflicting Recording does
not gain this preference. Candidate choice remains independent of API order.

## Live catalog evidence

MusicBrainz Artist `26c077a4-046c-4b0d-870f-a6a2bdad7bfe` returned preferred name
`tsosis` and alias `The Speed of Sound in Seawater`. Fresh copied data acquired this
through normal ingestion with no manual equivalence insertion. Both the original
and validated copy contain zero explicit Artist-equivalence rows; the original
Seawater Artist had no external Artist IDs. Spotify Artist
`7sB62q3ZbUTVI9G5RVsWCZ` supplies First Contact (10 Tracks) and Blue Version (5).

MusicBrainz Artist `91640d9b-b077-4271-9fb7-84f3ea543f24` returned `Hop Along`
and alias `Hop Along, Queen Ansleis`. Freshman Year candidate
`1PtvYmXJV1FJBSDEsDbTs3` has the cleaner single-Artist Track credits; candidate
`3ctenyhN6B5SQhRXy9Q5ux` additionally credits Frances Quinlan. Both expose 15
Tracks against 16 local Tracks. Sequence alignment preserves the inserted local
`Organ Song`; `_` (24,216 ms) maps to `-` (24,182 ms). The intended result is
15 source connections, leaving Organ Song as an explicit missing occurrence.

Work candidate `0Qy0Srk2am6EZHHbPqCgBB` supplies all five Tracks. Mirror,
Midway, Spring Rain and Celebrate use contextual shared components with the
provider's additional Chinese title material. `D` uses ordinary full-title
correspondence. Raw metadata is retained.

Translation/transliteration, market-specific catalogs, unlimited provider searches
and unrelated release merging remain deferred. Provider HTTP is outside write
transactions; writes stay short and coordinated between internal batches.

## Final disposable-copy results

The current source snapshot has 930 saved Tracks, 609 connected and 321 unresolved.
Focused normal-catalog validation connects 35 additional Tracks (Work 5, Seawater
15, Freshman Year 15). The following complete coordinator run connects another 13,
processing two internal Album batches with one user start. An immediate second
complete run connects zero additional Tracks and performs zero Spotify or
MusicBrainz HTTP requests. The shrinking actual workload now spans two batches;
the 65/101-Album regressions continue proving four/six-batch lifetimes.

| Track state, excluding connected/excluded rows from exception counts | Count |
| --- | ---: |
| Connected | 657 |
| Needs Review | 53 |
| No Candidates | 220 |
| Waiting for Spotify | 0 |
| Needs Retry | 0 |
| Not on Spotify | 0 |

No remaining Needs Retry explanations are required because the final count is zero.
The broad title-discovery page for Freshman Year includes unrelated Artists and is
bounded/incomplete. Its cleaner candidate ranks above the extra-credit candidate,
and all 15 sensible occurrences connect automatically; exact Album representation
acceptance remains withheld by the incomplete-page safeguard. Organ Song remains
an exception rather than an invented correspondence. This limitation is not hidden.

The focused live check made 10 Spotify catalog requests, one token acquisition and
four MusicBrainz requests. The full coordinator made 13 Spotify catalog requests,
four token acquisitions and seven MusicBrainz requests; the immediate second run
made none. Search-window bookkeeping counts are not HTTP counts. Full-run start
returned in 6 ms, preparation appeared immediately, and the first completed local
transition was visible at 103 ms. Its first run completed in 43,616 ms. Both runs
completed in 75.13 seconds. There were no `database is locked` errors.

Structured current evaluator evidence, raw titles/programs and provenance are in
[spotify-exception-inbox-evidence.json](spotify-exception-inbox-evidence.json).

Verification: core suite 479 passed; Spotify suite 76 passed,
MusicBrainz suite 21 passed; focused Qt Spotify suite 17 passed (seven explicit
manual/live tests ignored), initial Library views regression passed. Coordinator
coverage includes simultaneous review/Album retry, 65/101-Album drain, startup/import
scheduling and cached fixed point. Adapter coverage acquires an independent SQLite
writer during HTTP. Formatting, diff checks and strict all-target Qt Clippy pass.
