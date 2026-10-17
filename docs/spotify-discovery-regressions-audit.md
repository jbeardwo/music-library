# Focused Spotify discovery and contextual reconciliation audit

All writes and live validation used disposable `/tmp/` backups of
`/tmp/music-library-fresh.sqlite`. The original database was read only. No commit
was created. Existing uncommitted work was preserved. This is a query/lifecycle
and contextual Track-comparison follow-up, not a matcher redesign.

[Structured evidence](spotify-discovery-regressions-evidence.json) records canonical
and normalized names, trusted aliases, actual cached request fingerprints, raw
provider search results, before/after ranking, complete Homeboy evidence, the six
Track comparisons, final saved-Track states and user-data preservation.
The checked-in `tools/qml-diagnostic/examples/discovery_regressions.rs` reproduces
cache-only inspection or explicit bounded discovery on a disposable database.

## Discovery root causes and exact queries

The fresh workflow's catalog-name preflight searched the first trusted alternate
instead of the canonical Artist. `ClownCore` resolved to Spotify **Clowncore**
(`6OJktwLDU5IXLzrihegpeC`), rather than **Clown Core**
(`6mtABo05yeYtPftYEVfGoV`). Hop Along acquired only its former provider name,
**Hop Along, Queen Ansleis** (`7hDLm8O99kRfpY3DjFHQAP`), while the three later
Releases belong to **Hop Along** (`3yYUV3hkJit05YIUEODqgp`). Search consequently
used the wrong provider name/identity context. These were discovery/Artist-context
regressions, not Track-program failures or an Album-title threshold problem.

The query builder compounded this by using title-only discovery when trusted
names differed, or spending its only alternate on a speculative `Artist on Title
Live` query for ordinary single-word titles. Wrong-Artist results were already
filtered by ID; the self-titled fallback returned **Welcome to Clown School** by
**3AM CLOWNCORE**, which was correctly rejected. That did not retrieve the intended
self-titled Release. No Artist/Album-name deduplication defect was found.

Canonical names now precede alternate-name discovery. Ambiguous/alternate provider
Artists require a unique complete corroborated program before catalog-name evidence
is added. Existing IDs are retained. Multiple trusted names keep an Artist field
in both primary and alternate Album queries. A relevant release-type suffix also
counts as useful discovery, avoiding an unnecessary speculative fallback.

| Release | Exact prior queries | Exact final primary query | Result |
| --- | --- | --- | --- |
| Clown Core — 1234 | `album:"1234" artist:"Clowncore"` | `album:"1234" artist:"Clown Core"` | Connected 4/4 |
| Clown Core — Clown Core | `album:"clown core" artist:"Clowncore"`; `album:"clown" artist:"Clowncore"` | `album:"clown core" artist:"Clown Core"` | Connected 13/13 |
| Clown Core — Toilet | `album:"toilet" artist:"Clowncore"`; `Clowncore on toilet Live` | `album:"toilet" artist:"Clown Core"` | Connected 9/9 |
| Clown Core — Van | `album:"van" artist:"Clowncore"` | `album:"van" artist:"Clown Core"` | Connected 12/12 |
| Hop Along — Get Disowned | `album:"get disowned" artist:"Hop Along, Queen Ansleis"`; `album:"disowned" artist:"Hop Along, Queen Ansleis"` | `album:"get disowned" artist:"Hop Along"` | Connected 10/10 |
| Hop Along — Painted Shut | `album:"painted shut" artist:"Hop Along, Queen Ansleis"`; `album:"painted" artist:"Hop Along, Queen Ansleis"` | `album:"painted shut" artist:"Hop Along"` | Connected 10/10 |
| Hop Along — Bark Your Head Off, Dog | `album:"bark your head off, dog" artist:"Hop Along, Queen Ansleis"`; `album:"head" artist:"Hop Along, Queen Ansleis"` | `album:"bark your head off, dog" artist:"Hop Along"` | Connected 9/9 |

Every final primary query above returned the correctly named Release by the exact
intended Artist; none required an alternate Album query. `1234` is the actual
canonical and Spotify title in this database; the generic `1234 EP` suffix scenario
is separately covered by tests. Search strategy **5** distinguishes the corrected
Album strategy from historical strategy-4 negative results. Evaluation **14**
reconsiders unresolved evidence independently. Cache-only inspection can still
inspect prior fingerprints without issuing HTTP.

## Contextual Track-title evidence

The shared policy distinguishes abbreviation punctuation, token boundaries, tiny
edits/adjacent transpositions and the structured weak Album Version annotation.
It runs after at least three independent program anchors with at least 75% coverage,
and requires established Spotify Artist IDs, a unique positioned counterpart,
compatible durations and no trusted Recording conflict. Existing shared-span and
apostrophe matches may supply those independent anchors; weak title rules cannot
bootstrap one another. Album title/Artist gates remain separate.

The old one-edit occurrence helper now delegates to the existing shared bounded
edit-distance implementation. Four-character typos can qualify in a corroborated
program; shorter differing titles remain unresolved. Adjacent transposition is
bounded to long titles. Every competing local/provider title is checked, including
other positions. Harmless punctuation, token-boundary and weak annotation evidence
use the existing 10-second Spotify playback tolerance; tiny typos retain 3 seconds.

| Actual title comparison | Before | After / program corroboration | Final decision |
| --- | --- | --- | --- |
| Guy Vs. D.C. Sniper ↔ Guy vs DC Sniper | Local only; 5/6 anchors | Contextual punctuation; 6/6; duration delta 3,093 ms | Track connected; Release fully connected |
| Fifzteen ↔ Fiftzeen | Local only; 9/10 anchors | Contextual adjacent typo; 10/10 | Track connected; Release fully connected |
| Soundtrack to Insecurity ↔ Sound Track to Insecurity | Local only; 10/11 anchors | Contextual token boundary; 11/11, with existing apostrophe/shared-span evidence | Track connected; Release fully connected |
| Dodr ↔ Dotr | Corroborated position, strict program not strong | Contextual one-character typo; 8/8 | Playback occurrence connected; existing canonical Release and other occurrences retained |
| Mini Rockers ↔ Minirockers | Corroborated position, strict program not strong | Contextual token boundary; 8/8 | Playback occurrence connected; existing canonical Release and other occurrences retained |
| Gaze at Blue (Album Version) ↔ Gaze at Blue | Local only; 11/12 anchors | Weak Album Version annotation; 12/12 | Dreams connected 12/12 |

The real Dreams database has the **reverse direction** from the supplied example:
its file tag is `Gaze at Blue (Album Version)`, and the current Spotify Dreams
program returns `Gaze at Blue` (`5IaD1VllrK2Z7vXxZWz235`). Both directions are
compatible; the requested plain-local/annotated-provider direction is tested.
Raw titles are retained. Album Version has weak contextual semantics; Live,
Acoustic, Demo and other meaningful qualifiers remain conflicts. Outside established
program context this pass does not blindly strip Album Version during Track search.

Adebisi Shank's six already-connected Tracks used another Spotify edition. Their
occurrence-ID differences suppressed strict program anchors, although their titles,
Artist and durations corroborated the alternate program. Playback inspection can
use that corroboration without replacing existing exact Spotify attachments;
Recording and Artist conflicts still veto association. Strict canonical edition
evaluation remains conservative.

## Homeboy

The old stored blocker on all four saved Tracks was `incomplete_program`, displayed
as **Insufficient evidence**, at evaluation 13 with `stale=0`. The retained current
program is complete: same Hella ID (`1n861RIk6CTAWncgHR9UHg`), four corresponding
positions/titles, provider Release **Homeboy EP** (`5TgI4E1qDUKJqc9zqcVfMV`),
provider type Single and date 2005-11-08. Local/provider duration pairs are
671922/671826, 142681/142600, 229329/229240 and 720980/720893 ms. Local MusicBrainz
Recording IDs have no contradictory provider Recording IDs. The release-type
suffix is compatible; there is no meaningful competing candidate.

Current evaluation already accepts that full cached evidence. Its old terminal
summary had outlived the missing-program condition and was not replayed by the
current-version automatic selector. The evaluator upgrade and complete coordinator
replay apply the accepted result. Homeboy connects 4/4; no threshold was lowered
for it. The full structured before evidence is retained in the JSON audit.

## WAO!!

Both the raw local file Album title and canonical title are literally **WAO!!**.
The trusted MusicBrainz release-group title is also WAO!!, with date 2007-05-23.
No title translation, transliteration or invented alternate is involved. The
canonical Artist is `オレスカバンド`; trusted catalog aliases include ORESKABAND,
Oreska Band and Ore Ska Band.

The prior `album:"wao!!" artist:"オレスカバンド"` returned zero. The bounded plain
alternate `"オレスカバンド" wao!!` retrieved relevant WAO!! objects, but their Artist
IDs differed. Trusted-name selection now favors a catalog alias actually observed
in Spotify. ORESKABAND Artist discovery returns two normalized-name peers; complete
program evidence supports `56OQWVhAWYw1yRxD0R2HSR` uniquely. The final scoped query
`album:"wao!!" artist:"ORESKABAND"` returns **WAO!!**
(`0dyEtIksUU6PYFBsttExjk`) and **Wao!!** (`2FY7NTcxo2WPZzxG4y9DEL`). The 13-Track
program is accepted; the other 13-Track peer's incompatible titles/program are rejected.
Final WAO!! result: 13/13 connected.

Mixed-script trusted titles can additionally produce one literal meaningful ASCII
span for discovery, such as `Japanese text / WAO!!` → `Artist + WAO!!`. The two
exclamation marks survive. Titles without such literal text produce no fabricated
WAO!! component. This helper is tested but was unnecessary for the actual pure
WAO!! title.

## Actual Connections lifecycle and retry reasons

The actual QML Bridge launches on a disposable fresh/current database, opens
Spotify Connections, executes one explicit reconciliation, pumps queued callbacks
until all coordinator/local work finishes, and compares every displayed row with
its current backend query. Successful provider observations from the named audit
are reused to avoid duplicate traffic; canonical identities/memberships/review
states start from the original fresh database. This is not a narrow storage fixture.

The final run used two internal chunks, connected **96** additional saved Tracks,
and finished with **219** unresolved. All **149 displayed rows** agreed with the
backend: **zero stale rows**. Repeat cache-backed fresh-copy runs converge to the
same result. Two new standalone search-scope observations were added; the per-run
allowance remains two across all internal chunks.

| Saved-Track state | Final count |
| --- | ---: |
| Connected | 691 |
| Needs Review | 207 |
| No Candidates | 12 |
| Waiting for Spotify | 0 |
| Needs Retry | 0 |
| Not on Spotify | 20 |
| Provider search deferred | 0 |

There are no remaining actionable Needs Retry entries. Four old `needs_retry`
records belonged to manually excluded Tracks and are omitted from the actionable
retry audit, matching the UI. Exclusions retain their user-state ownership.

Missing/rekeyed cache and unavailable Album evidence now carry explicit refresh
reasons. Local reevaluation failures include the error and pending action. Provider
unavailability remains Waiting for Spotify. When the standalone allowance leaves
pending work, **Provider search deferred** exposes that exact cause and is terminal
for the unchanged bulk window. A focused real-SQLite regression checks that state.
The coordinator refreshes Connections on completion, without resetting viewport,
filters or selection.

Review preserves the actual primary blocker, generated search texts and rejected
Artist evidence, including empty candidate pages. The top of the dialog no longer
hides discovery/filtering behind a generic No Candidates message. Existing Track/
position/duration/Recording evidence remains available in the structured inspection.

## Requests, validation and preservation

Album discovery remains one primary plus at most one alternate, ten result objects
and the existing three-program comparison bound. Alternate Artist ambiguity uses
that same Album/program bound. Alias recursion cannot cycle. Typo variants are not
sent to search. Album Version changes evaluation only. Normal cache freshness,
explicit refresh, provider cooldown/Retry-After and short HTTP-free write
transactions retain their boundaries. Named corrected Clown Core/Hop Along queries
all finish on their first scoped Album request. Cache-only final evidence capture
makes zero provider requests.

Validation: full core suite **482 passed, 18 ignored** before the last four
regressions; final affected suites include **25 program-alignment**, **31 lifecycle**
and **41 Album-candidate** tests passing (three opt-in Album tests ignored).
Spotify adapter **80 passed, three ignored**, plus one ignored live playlist test.
MusicBrainz **21 passed, three ignored**. Seventeen isolated Spotify QML checks pass;
six opt-in checks are skipped, and the full disposable real workflow passes
separately. Strict Clippy passes for core, Spotify adapter and QML app/examples;
formatting and diff whitespace checks pass. The existing unused test-only
`test_handoffs` warning remains when building QML tests.

All 930 canonical Tracks/Recordings, 96 Releases/Albums, 26 Artists, 930 memberships,
930 playable sources/source associations, ordered credits and sparse override
records compare equal before/after. Existing 595 connected Tracks remain connected.
Only intended provider reconciliation evidence/associations and derived status
bookkeeping change. No source tags, user exclusions, playlist contents or canonical
identifiers are rewritten.

Deferred: translation/transliteration, arbitrary fuzzy Album matching, unbounded
provider searches, correction/deletion of historical trusted identifiers, and
manual resolution of the remaining 207 review/12 negative cases. These are outside
this focused pass; no named validation case remains stranded.
