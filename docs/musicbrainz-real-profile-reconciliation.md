# Real-profile MusicBrainz reconciliation / Connections correctness

Validated 2026-10-19. No commit and no writes to the source profile.

## Reproduction and root causes

The user confirmed `/tmp/music-library-fresh.sqlite`. No UI process was running
at inspection time. The SQLite read-only connection backup API included its WAL
state in `/tmp/mb-real-before.sqlite`: schema **38**, **930 saved Tracks**, **96
MusicBrainz lifecycle rows**. It contained **864 connected / 930, 66 gaps, eight
Albums with gaps**. OCDC was already connected in this snapshot; its reported
unresolved UI state could not be reproduced from persisted identity.

All writes used disposable backups: `/tmp/mb-real-normal.sqlite` for request
accounting and `/tmp/mb-real-ui-startup.sqlite` for actual Qt/QML launch, automatic
worker completion, first-layer Album Match, and Artist-scoped manual search.
The original was never opened through mutable application APIs. Its schema,
saved/connected counts and logical database contents were checked again afterward.
The original and frozen preflight backup produced the same logical-dump SHA-256:
`e0d456582c78fcf1f5ecd8b8f6fa3f7bc481662180ef84bba3586b7f718a7215`.

Persistent launch supplied zero imports, so `post_import` never started matching.
The old lifecycle evaluation version also described manual inspection, not proof
that current automatic discovery had completed. Raw observations were within the
30-day TTL; time freshness did not establish policy freshness. Therefore the main
failure was unreconciled automatic state (A), compounded by projection defects (B)
and structural-versus-connected presentation (C). The remaining title/Artist/version
limits are real evidence limitations (D), not fabricated empty discovery.

Before any reconciliation, the full inventory (all candidate IDs, reasons,
fingerprints, program/inspection presence and timestamps) was saved to
`/tmp/mb-real-before.json`. The preflight summary below retains the key facts.
All rows had evaluation version **2**; none had an explicit discovery generation
or an automatic-completion receipt.

| Album | Connected | Lifecycle reason | Candidates | Automatic snapshot | Program evidence |
|---|---:|---|---:|---|---|
| 100 Onces | 11/11 | program_retained | 2 | present | 1 ensembles; 0 inspections |
| The EP of a Band Called Adebisi Shank | 0/4 | program_retained | 10 | absent | 0 ensembles; 1 inspections |
| Angle | 0/11 | candidates_found | 10 | absent | 0 ensembles; 0 inspections |
| Dreams | 0/12 | candidates_found | 20 | absent | 0 ensembles; 0 inspections |
| Gaze at Blue | 1/3 | existing_group_conflict | 23 | present | 4 ensembles; 0 inspections |
| Underwater | 9/12 | duration_conflict | 10 | present | 2 ensembles; 0 inspections |
| More Skin With Milk-Mouth (2015 Remaster) | 0/5 | insufficient_evidence | 10 | absent | 0 ensembles; 1 inspections |
| Church Gone Wild/Chirpin Hard | 0/24 | insufficient_evidence | 10 | absent | 0 ensembles; 1 inspections |
| Hippy Jam Fest | 0/5 | program_retained | 10 | absent | 0 ensembles; 1 inspections |

| Album | Canonical fingerprint before retry | Last lifecycle attempt (UTC) |
|---|---|---|
| 100 Onces | `21717e6f-b51f-5974-890e-2cc4bd6515d2` | 2026-10-20T02:15:10+00:00 |
| The EP of a Band Called Adebisi Shank | `40453fc3-e480-5845-97a1-5f7bdcfea21d` | 2026-10-20T02:15:23+00:00 |
| Angle | `fe00ba2a-24e6-52df-b02b-42ba52da6c93` | 2026-10-20T02:16:12+00:00 |
| Dreams | `9b7850b1-ba72-5bf8-b55e-a2b8b44b1a36` | 2026-10-20T02:17:18+00:00 |
| Gaze at Blue | `113c68b6-a72e-5b48-bd47-5c2f897cea2b` | 2026-10-20T02:13:18+00:00 |
| Underwater | `1a5e131d-b8f8-50c8-8ed1-7aa4485ff2b9` | 2026-10-20T02:12:52+00:00 |
| More Skin With Milk-Mouth (2015 Remaster) | `90990b0b-38c6-5359-b71a-ea30d88df6c4` | 2026-10-20T02:22:20+00:00 |
| Church Gone Wild/Chirpin Hard | `d9630bdd-149e-542f-b21f-e90582bae94d` | 2026-10-20T02:13:34+00:00 |
| Hippy Jam Fest | `ac9dd03c-cb8e-5b64-a21a-48560f7ac367` | 2026-10-20T02:26:05+00:00 |

## Changes

Migration 39 adds automatic completion receipts with evaluation generation **3**
and discovery generation **2**. Manual metadata/search/inspection does not complete
the automatic receipt. Startup traverses eligible unresolved unmarked Albums in
128-item keyset batches; completion callbacks refill a bounded queue. Completed
unchanged generations are skipped, while transient failures stay distinguishable
and retain the existing worker's rate-limited retry behavior.

Raw candidate/Artist-page/program freshness remains separate. Retained full ensembles
are recomputed before discovery, preserving provider timestamps. Single manually
inspected Releases remain separate from automatic sampled-release agreement. A
previously attached Group with fresh ensemble evidence requires zero discovery
requests. Policy invalidation never deletes useful provider observations.

Topology ranking now bounds the report before its generic identifier sort. Candidate
metadata and programs are pruned after the complete batch, preventing a newly retained
native program from losing its title/Artist metadata. First-layer MusicBrainz status
uses its own durable lifecycle, instead of the old unconditional “No candidates found”
non-Spotify fallback. Review rows use saved membership and `connected_musicbrainz_track`
for Connected status, and shared correspondence for proposals. Recording certainty
comes from the automatic ensemble even when reviewing one selected Release.

Manual Album search uses trusted Artist MBID (or canonical Artist resolution) and
retained scoped candidates. “Search all Artists” explicitly broadens the query.
Async target resets still drop obsolete receivers. Manual search preserves completed
automatic evaluation reasons and cannot turn a proposal into a connection.

A typed trailing edition representation is available for bounded retrieval; original
remaster qualifiers remain intact during evaluation. A contextual internal `the`
addition/removal is permitted only for sufficiently long titles under four independent
sibling anchors, 80% coverage, matching position, measured duration within 3 seconds,
trusted Artist and unique counterpart. This does not alter global title normalization.

## Observed-case results

| Album / Track | Before | Root cause | Change / after | Provider requests, normal API run |
|---|---|---|---|---:|
| 100 Onces / OCDC | Already connected; Album 11/11 | Reported unresolved state absent from this confirmed snapshot | No additional identity rule; remains connected. Old-policy OCDC lifecycle replay has a zero-request regression test. | 0 |
| The EP of a Band Called Adebisi Shank | 0/4; manual inspection only | A: automatic reconciliation never completed | Current boilerplate/program policy; 4/4 connected | 7 |
| Angle | 0/11; unrelated global results | A/B: native discovery absent | 角度 retained, 0/11 structural; Needs Review | 10 |
| Dreams | 0/12; unrelated global results | A/B plus report/cache truncation | 夢境 metadata/program retained; 1/12 structural; Needs Review | 19 |
| Gaze at Blue | 1/3 connected; 2/3 structural | C plus existing trusted Group conflict | Still 1/3 connected; two structural correspondences; alternate retained | 0 |
| Underwater | 9/12 connected; additional structural mappings | C; valid duration and Artist blockers | 9/12 connected; 12/12 structural; provider-only remix separate | 0 |
| More Skin With Milk-Mouth (2015 Remaster) | 0/5; base Group manually inspected | A/B; typed retrieval and contextual article gap | Base Group retained; 5/5 structural; 0/5 Recording assertions, version uncertainty | 11 |
| Church Gone Wild/Chirpin Hard | 0/24; manually inspected program | A; inspected Release is not full automatic ensemble | Current slash/scoped discovery and sampled Releases; 24/24 connected | 4 |
| Hippy Jam Fest | 0/5; manual inspection only | A; no completed structured discovery | Existing extended-title/program semantics; 5/5 connected | 4 |

The normal API run and a separate fresh-copy QML startup run both reached
**897 / 930 connected, 33 gaps across five Albums**. The QML test did not manually
select Albums or inject Group IDs. After startup it opened each remaining Album's
first-layer review, verified nonempty retained candidates, and searched Angle/Dreams
in the default Artist scope, finding `角度`/`夢境` among the retained scoped results.
This corrects real-profile state; it does not replace the historical Phase 3 baselines.

### New trusted Groups

* adebisi shank — The EP of a Band Called Adebisi Shank: `d00db664-430d-422f-ba41-ee092231dfa1`, MusicBrainz **This Is the EP of a Band Called Adebisi Shank**. Artist-scoped title query, automatic candidate rank 1; sampled-program mapping counts [4, 4]. Trusted Artist MBID, strong unique coverage and no meaningful Group/version conflict established the Group. No exact Release was selected.
* hella — Church Gone Wild/Chirpin Hard: `385cbc6c-a9b5-3a7b-8146-f4a14bdcdfd2`, MusicBrainz **Church Gone Wild / Chirpin Hard**. Artist-scoped title query, automatic candidate rank 1; sampled-program mapping counts [23, 22]. Trusted Artist MBID, strong unique coverage and no meaningful Group/version conflict established the Group. No exact Release was selected.
* ttng — Hippy Jam Fest: `680ff456-5e82-4d0c-ba1f-86479004d52d`, MusicBrainz **Hippy Jam Fest (The Likes of Which Has Never Been Seen)**. Artist-scoped title query, automatic candidate rank 1; sampled-program mapping counts [5, 0]. Trusted Artist MBID, strong unique coverage and no meaningful Group/version conflict established the Group. No exact Release was selected.

### Gaze at Blue: three saved Library Tracks

Trusted Group `1949e0a2-abe4-4147-af86-7bd1e8727300` remains unchanged.
The review alternate is `f2c15f47-8a93-4dac-853f-7910b5682140` (`凝視`).

| Library title | Persisted Recording MBID | Alternate structural candidate | State / reason |
|---|---|---|---|
| Dawn | — | Dawn (corresponding) | Blocked — trusted Group conflict |
| Gaze at Blue | f19c9ff2-eb91-44d0-bc6e-4cb29839d6d1 | Gaze at Blue (corresponding) | Connected |
| Gaze at Blue (Remix) | — | Gaze at Blue (Charlie Lim remix) (not established) | Blocked — trusted Group conflict |

A single read-only structured Recording relationship inspection confirmed that
`ea674b4d-2b24-4bed-a454-a537d1e90fd7` has a MusicBrainz **remixer → Charlie Lim**
relationship, and a **remix → Gaze at Blue** relationship to the base Recording
`f19c9ff2-eb91-44d0-bc6e-4cb29839d6d1`. This explains the provider attribution;
it does not prove that the Library's generic `(Remix)` means that particular remix.
There is also an incompatible existing trusted Album Group. No generic remix stripping,
new automatic association or trusted Group replacement was introduced. Raw response:
`/tmp/mb-gaze-recording-relationships.json` (one extra investigation request).

### Underwater: twelve saved Library Tracks

Group `14165569-4114-43ca-b040-140898f5855d` (`水底`). All twelve structurally
correspond; nine have persisted authoritative Recording identity. The provider-only
`Midway【Rgry Remix】` is a separate thirteenth provider occurrence, not a saved Track.

| Library title | Connected | Structural | Candidate Recording MBID | Certainty / blocker |
|---|---|---|---|---|
| Shower | yes | yes | dedb1c58-956c-4b71-bd15-e30a7cbdf6f8 | already_known |
| Underwater | yes | yes | f441dd91-ed0b-4966-ba56-f1ce7e584a53 | already_known |
| Satellite | yes | yes | 0f5c1315-adb2-49cd-b043-6dddd9935d56 | already_known |
| Half Asleep | no | yes | 0f7aba2c-7c87-4c66-aec8-705a0625eccb | duration_conflict |
| Bad Dream | no | yes | eb984a59-e25a-4bd8-a817-0c40ec56275e | artist_untrusted |
| Half | yes | yes | 878712c1-9e9d-45b5-b944-448025452656 | already_known |
| Shell | no | yes | d9360a09-64e7-4516-b619-e38af52ce13e | artist_untrusted |
| Lake | yes | yes | af605d36-4283-4f4a-9cfa-7cb7cdddf3e8 | already_known |
| Quilt | yes | yes | 26b2b012-2adf-42a5-8139-8740386c5443, d8301a81-5605-4375-9f5e-95fa1c9a3685 | already_known |
| Walk | yes | yes | 908877a3-fb46-41b5-92ca-d136b3ffd857 | already_known |
| Speechless | yes | yes | 3bac9f36-d44a-47c4-9160-dea89e0669d8 | already_known |
| Moonset | yes | yes | 3aa1d1a6-acc2-4701-8314-162d9907472f, 2d6b64d4-94f3-499c-a66a-91955145bfdc | already_known |

`Half Asleep`: local **90,770 ms**, sampled provider **86,586 ms** (another sample
86,000 ms), exceeding the 3-second automatic-certainty gate. `Bad Dream` has one
local unsplit Artist name `Elephant Gym ft. Sowut` without trusted identity; MusicBrainz
has separate trusted primary and featured credits. `Shell` similarly has
`Elephant Gym ft. Cudjiy lja karivuwan` as one untrusted local credit. Durations for
those two are compatible; Artist certainty remains the blocker. No Artist substring
identity was added. These are current recomputed blockers, not stale evaluations.

### More Skin first Track and version safety

Library:
`When The Catholic Girls Go Camping, The Nicotine Vampires Rule Supreme. (2015 Remaster)`.
Provider:
`When Catholic Girls Go Camping, the Nicotine Vampires Rule Supreme.`

Case/typography are already normalized, and the systematic remaster annotation is
structural edition evidence. The remaining difference is one internal `the` after
`When`. The narrow contextual relation now provides the fifth structural correspondence
under four strong siblings, trusted Artist, position, duration and uniqueness.
Every Recording remains withheld for original/remaster version uncertainty. The
Album is not automatically associated with an incompatible original Group.

### Church: Wildlife, exact evidence

Library: `Wildlife Takes the Loser By Night (Movement 5)`, position **disc 1 / 105**,
**338,024 ms**, trusted Hella Artist MBID `1ecbc7e5-6e33-4062-997d-abc550dd63e6`.
Both sampled complete Releases identify Recording
`187d0bff-5054-48bf-aa1f-4b6d7f716606`, at **disc 1 / 5**, **337,973 ms**:

* Release `f27f4d20-df71-487e-bd14-b440ef628276`:
  `Wildlife Takes Loser by Night`.
* Release `79e57e1a-e04a-4584-bcfb-4ec0b63508c0`:
  `Wildlife Takes the Loser by Night: Movement 5`.

The first manually inspected Release omitted the article and Movement qualifier.
The normal automatic ensemble includes the second complete Release, which supplies
supported correspondence and the same Recording ID. Current automatic certainty
connects it; no Movement rule change was needed. One representative structural
sample can still show 23 correspondences, while the ensemble supports all 24
persisted Recordings. Connected state therefore remains 24/24 independently of
a single selected sample's proposal count.

## Retrieval / manual-search consistency and efficiency

Church's old state had no automatic snapshot despite a usable manual result. Normal
Artist-scoped slash-expanded retrieval now finds its Group, not a Church-specific
query. Hippy similarly uses existing scoped extended-title discovery. More Skin now
adds its recognized edition representation to the existing bounded query while
keeping remaster meaning for evaluation. Manual global title search was broader
than automatic scope and was not proof that automatic reconciliation had run.

Endpoint counts below are measured transport attempts in the final normal API run,
including existing retries. Artist pages can come from fresh shared cache; the
page count is inspected/retained pages, not necessarily new network pages. Programs
are Group ensembles, each with up to three sampled Release details.

| Album | HTTP attempts | Title queries | New Artist pages | Release browses | Release details | Pages inspected | Ensembles retained |
|---|---:|---:|---:|---:|---:|---:|---:|
| The EP of a Band Called Adebisi Shank | 7 | 1 | 0 | 2 | 4 | 0 | 2 |
| Angle | 10 | 2 | 0 | 2 | 6 | 3 | 2 |
| Dreams | 19 | 1 | 0 | 5 | 13 | 3 | 5 |
| Gaze at Blue | 0 | 0 | 0 | 0 | 0 | 3 | 4 |
| Underwater | 0 | 0 | 0 | 0 | 0 | 3 | 2 |
| More Skin With Milk-Mouth (2015 Remaster) | 11 | 1 | 2 | 3 | 5 | 2 | 3 |
| Church Gone Wild/Chirpin Hard | 4 | 1 | 0 | 1 | 2 | 0 | 1 |
| Hippy Jam Fest | 4 | 1 | 0 | 1 | 2 | 0 | 1 |

Total final normal-run transport attempts: **55**. The separate QML startup validation also performed two explicitly requested scoped manual-search checks. One additional Recording relationship request investigated the remix. Earlier failed sandbox DNS attempts were deferred, not completed-empty results.

No per-Track catalog search, increased Artist-page budget, increased ensemble budget,
parallel provider crawl or new aggressive retry was introduced. Current limits remain
three Artist pages, six Group ensembles, 34 logical endpoint calls plus existing bounded
transport retries. Gaze/Underwater replay required **zero** requests. Fixture tests also
prove old-policy full-ensemble replay with zero provider calls and no freshness renewal.

## Remaining Missed Connections

| Reason | Albums / gaps |
|---|---|
| Candidate retained; insufficient program anchors | Angle 11; Dreams 12 |
| Existing trusted Group conflict; generic remix attribution unresolved | Gaze at Blue 2 |
| Artist certainty | Underwater 2 |
| Duration conflict | Underwater 1 |
| Original/remaster version uncertainty | More Skin 5 |

These **33** gaps are review/evaluation outcomes, not discovery misses. No trusted
Group was replaced. No exact edition was asserted. No remix/remaster equivalence
was inferred merely from a structurally similar program.

## Verification and reproducibility

* `cargo test --offline`: **539 passed**, 18 opt-in tests ignored.
* `cargo test --offline --test musicbrainz_followup`: **39 passed**; policy generations, old OCDC /
  Adebisi replay, raw evidence retention, native candidate bounds, zero requests,
  unchanged-generation behavior, keyset batching, authoritative counts, provider-only
  rows, typed edition retrieval and guarded contextual article controls.
* MusicBrainz adapter suite: **25 passed**, three opt-in tests ignored.
* Unchanged Spotify adapter suite: **84 passed**, three opt-in unit tests and one opt-in documentation test ignored.
* Offscreen QML `connection_review` regressions: **10 passed**, six opt-in tests ignored.
* Opt-in `musicbrainz_real_profile_startup_and_review`: normal fresh-copy startup,
  async completion, actual first-layer retained candidates and scoped manual search.
* 200k disposable Connections audit: bounded startup selection **8.2 ms** for 256
  eligible IDs; MusicBrainz Album page p95 **13.5 ms**, Track expansion **0.68 ms**.
  The actual startup queue refills in batches of 128. No catalog requests occur in
  Connections list queries or mere retained review opening.

Full artifacts: `/tmp/mb-real-before.json`,
`/tmp/mb-real-normal.sqlite.json`, `/tmp/mb-real-ui-startup.sqlite.ui.json`,
`/tmp/mb-real-200k.log`, and `/tmp/mb-real-*-tests.log` / UI logs.
`adapters/musicbrainz/examples/real_profile_validation.rs` runs the normal API on
an explicitly named disposable `/tmp/mb-real-*` copy and records request classes,
programs, evaluation and authoritative review projections. The ignored QML test
uses `MUSIC_LIBRARY_DIAGNOSTIC_DATABASE=/tmp/mb-real-ui-startup.sqlite` and offscreen Qt.

Intentionally deferred: stronger native/bilingual correspondence, splitting ambiguous
featured Artist credits without trusted identity, attributing an unspecified local
remix to a particular remixer, and resolving original/remaster musical identity.
The user's original profile remains unchanged; it will receive normal reconciliation
when opened with the updated application and automatic matching enabled.
