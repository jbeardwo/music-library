# Local identity and Spotify robustness audit

Validation date: 2026-10-09. No commit. The original real-library database was not
mutated. Migration, intentional import and connection validation used SQLite backups
under `/tmp`. Real validation used retained Spotify discovery and programs only:
**zero live Spotify requests**. Public musical metadata from eight retained programs
is checked in as regression fixtures; local paths and canonical identifiers are not.

## 1. Duplicate Artist root causes and consolidation

The supplied snapshot differs from the historical counts in the request. Counts below
are measured entities, not QML display grouping. All inspected names were visually
and textually equal; invisible whitespace/Unicode was not their cause.

| Artist | Available snapshot before → migrated copy after | Evidence and cause |
|---|---|---|
| Giraffes? Giraffes! | 3 → 1 (historical report: 4) | One Artist had Spotify `5wGMwXlBLMpuJzRpPxNFjq` and MusicBrainz `88a5ed53-1c52-474a-8ed5-49961ee2366e`; two local-only Artists owned separate programs. Their common Artist directory omitted punctuation. The old directory-name equality precondition prevented contextual consolidation. |
| Maps & Atlases | 3 → 1 | One Artist had Spotify `572aBw8LAuVMk47tfozZZr`; two local-only identities both credited Tree, Swallows, Houses. The old singleton-credit precondition skipped the redundant equal-name credits. |
| TTNG | 1 → 1 (historical report: 5) | Every available snapshot already had one canonical identity, with Spotify `5TUa95aB5Vu2CzwCnZd6t0` and MusicBrainz `544559db-c6c4-45d6-8435-d224e4f696e9`. The local directory says This Town Needs Guns. The repaired folder precondition covers that failure mechanism, but the original five entities are unavailable; their exact historical provenance cannot be established. |

Migration 33 rebuilds indexed local Artist context and runs conservative consolidation.
Same normalized name alone is insufficient: the operation requires shared local
Artist-directory/Album context and no conflicting trusted provider identity. Distinct
trusted same-name Artists stay distinct. Multiple same-name Album credits can now
register context. Canonical redundant unqualified credits are collapsed after merge;
roles, meaningful join phrases and source credit observations remain. Hidden Artist
preferences transfer to the surviving identity. No Albums/Releases are merged merely
because their metadata looks alike.

Inspected canonical IDs (the survivor is first):

* Giraffes? Giraffes!: `d55722a8-07a2-460d-aed7-52aae01b3e6b`,
  `b1bf1a5a-bdab-424e-9ebf-5a3295de7145`, `39b5991e-6bd3-4dd6-979a-b37df8951313`.
* Maps & Atlases: `1be989c7-66ee-495a-abb1-d18c5dd79828`,
  `0da99921-29d3-407d-88c7-317f873ebdb1`, `5aea88e1-9b3d-45f7-b78a-19b1f55e91d0`.
* TTNG: `b9088d1e-7ac8-4cf2-ace1-8eeff83d14e9`.

The original database remains schema 32 with the original 3/3/1 counts; the validation
copy is schema 33 with 1/1/1. Name keys were respectively `giraffes? giraffes!`,
`maps & atlases` and `ttng`; local-only identities had no conflicting external IDs.

Whole-copy integrity: Albums 250 → 250, Releases 250 → 250, Tracks 1,255 → 1,255,
playable sources 925 → 925. Membership 942 → 944 is exactly the two intentional
Polyphia restorations. All 863 existing trusted Spotify-connected Tracks remain;
the four new connections are the explicitly validated first Adebisi Shank EP.
No pre-existing external Track identity row was removed.

## 2. Polyphia missing Tracks

New Levels New Devils has **nine readable local files** in the available directory.
All nine were already discovered and attached to canonical Tracks. O.D. and Saucy
had attached to existing catalog Tracks without acquiring saved membership; seven
other files had created saved Tracks. This was an intentional-import membership bug,
not scanner omission, parsing failure or duplicate-file rejection.

| File/Track | Before saved | After explicit Folder import on copy |
|---|---|---|
| 1 Nasty | yes | yes |
| 2 O.D. | no | yes, existing Track ID retained |
| 3 Death Note | yes | yes |
| 4 Bad | yes | yes |
| 5 Drown | yes | yes |
| 6 Saucy | no | yes, existing Track ID retained |
| 7 Yas | yes | yes |
| 8 So Strange | yes | yes |
| 9 Rich Kids | yes | yes |

Copy result: scanned 9, unchanged 9, parsed 0, restored membership 2, unresolved 0,
unsupported 0, unreadable 0. Nine files → nine attached saved Tracks, without creating
replacement identities. G.O.A.T. is a source-less unsaved catalog Track, not a missing
local file. Two pre-existing canonical release contexts remain; equal release-group
or Spotify Album IDs do not prove the same physical edition and were not used to
merge them. Intentional Files/Folder admission now saves newly attached Tracks;
ordinary rescans preserve membership and cannot restore user removals.

Import reports also expose file-specific failures, including ambiguous local source
records, parsing/readability and admission errors. Reports retain up to 100 path/error
details plus the full failure count, and the import UI displays them.

## 3. Program-alignment design

`album_program::alignment` is UI-independent. Matcher, durable occurrence persistence
and diagnostic use the same result. It builds unique title/trusted-identity
correspondences, corroborates duration and checks conflicting identities, then tests
whether the mapped sequence is monotonic. It represents exact, title-normalized,
inserted local, missing local, reordered and unmatched rows. This is conservative
unique correspondence plus ordered-subsequence alignment, not an index-offset guess
or a fuzzy dynamic-programming match.

A strong complete program requires at least three unique anchors and at least 80%
coverage of both programs. Established Artist and compatible Album/version context
remain independent acceptance requirements. Ambiguous repeated titles stay unresolved;
conflicting identities and unsupported Live/studio changes remain blockers. Every
post-gap mapping is established by the Track evidence itself. Unreliable-position
fallback requires a conspicuously incompatible majority of local positions; normal
positions remain evidence. Raw tags and canonical ordering are unchanged.

## 4–11. Cached real Album outcomes

Before/after replay of the same retained programs:

| Case | Before this alignment pass | After |
|---|---|---|
| Animals | Rejected: missing/duplicate/shifted positions | Accepted, 13 actual correspondences |
| Audiotree Live (TTNG) | Rejected: version qualifier | Accepted, 5 contextual correspondences |
| Church Gone Wild / Chirpin Hard | Rejected: positions | Accepted, 24 correspondences |
| Dreams | Rejected: positions | Accepted, 11 correspondences |
| Freshman Year | Rejected: count/position accommodation | Needs review: 14 correspondences, unfetched competing object |
| Adebisi first EP | Rejected: title mismatch | Accepted, 4 correspondences |
| Critical Meat | Rejected: remaster qualifier | Still Rejected; numeric title correspondence now demonstrated |
| Songs / Underwater / split release / SuperBass | Rejected | Still Rejected with explicit evidence and manual controls |
| Animals Acoustic | Already trusted; not in unresolved replay | Trusted state preserved, 13/13 contextual fixture |
| WAO!! | No cached candidates | No cached candidates; exact-object manual path now available |

The available cached Piglet and Adebisi snapshots already list the plausible object
before the weak one; they do not reproduce the historical provider-order symptom.
Matcher fixtures deliberately reverse result order and verify the evidence ranking
and winner independently. Current real candidate ranks are reported below.

These are current evaluator decisions on retained data. An Accepted candidate is an
automatic eligibility result; offline inspection does not itself persist a connection
or renew lifecycle state. Remaining counts below mean unmatched program rows, not
claims that inspection saved every eligible association.

| Case | Our candidate rank / decision | Actual alignment and remaining action |
|---|---|---|
| Hop Along, Queen Ansleis — Freshman Year | 1 Needs review; competing rank 2 has no retained program | 14/16 unique correspondences against 15 provider Tracks. Final local 15 Workers → provider 14; local 16 A Drummer's Arm → provider 15. Local `_` and Organ Song remain unmatched. The actual snapshot gap is at local 9, not the reported local 14; local 14 maps to provider 13. Synthetic insertion tests prove the requested general post-gap behavior. Fetch/review the competing object or explicitly confirm the cached partial mapping. |
| TTNG — Animals | 1 Accepted; Acoustic alternative rank 2 Rejected | 13/15 mapped by actual titles across positions. Quetzal has a spelling mismatch; If I Sit Still Maybe I'll Make It Out Of Here has a meaningful wording difference. Those two stay unresolved for explicit review. No index pairing. |
| Hella — Church Gone Wild / Chirpin Hard | 1 Accepted | 24/24. Local 101–112 and 201–212 classified unreliable; unique titles and contextual Movement annotations establish correspondence. Raw positions retained. |
| TTNG — Animals Acoustic | Retained-program regression; already connected, absent from unresolved-group audit | 13/13. Systematic Acoustic suffix works only in acoustic Album context. Existing trusted associations retained; no reconnection required. |
| TTNG — Audiotree Live | 1 Accepted | 5/5 after contextual Audiotree Live Version annotation removal. Additional Audiotree credit retained as warning. Snapshot title is Audiotree Live; the shorter Audiotree form is covered by synthetic contextual tests. |
| You Slut! — Critical Meat | 1 Rejected | 8/8 title correspondence, including On the Count of 13 ↔ On the Count of Thirteen. Album candidate is explicitly Remastered; original/remaster conflict remains. Manual edition confirmation needed. |
| Piglet — Songs | Live in Chicago rank 1 Rejected; unrelated Cyrillic candidate rank 2 Rejected | Plausible established Artist candidate now outranks unrelated identities. Six provider Tracks have explicit Live qualifiers against nine local studio/unqualified Tracks. No automatic association; explicit review required. |
| Adebisi Shank — first EP | First EP rank 1 Accepted; second EP rank 2 Rejected | 4/4. Generic established-Artist “This Is” boilerplate corroborated by complete program. Four occurrences confirmed on disposable copy. |
| Elephant Gym — Dreams | 1 Accepted; unrelated documentary rank 2 Rejected | 11/12. Additional contributing Track Artists do not veto an otherwise established primary identity/program. Gaze at Blue (Album Version) retains a qualifier conflict and needs manual review. Provider credits remain observations. |
| Elephant Gym — Underwater | 1 Rejected | 6 unique suggestions among 12 local / 8 provider Tracks, below automatic coverage. Quilt/Moonset differ by International Collaboration Version; Shower, Half Asleep, Lake and Speechless lack counterparts in this object. Advanced mapping is required. |
| Giraffes? Giraffes! — SuperBass!!!! | 1 Rejected | 7/7 title anchors. **Actual blocker is provider 2015 Remaster versus unqualified local 2005 edition**, not punctuation, hidden zero-width characters or Artist fragmentation after consolidation. Explicit edition confirmation available. |
| Giraffes? Giraffes! — split local release | Single cached Swallowing Light object rank 1 Rejected | Snapshot canonical title is Spazz Master with two Tracks; provider contains one of them at a different position. Album title/count context cannot prove automatic identity. Paste both exact provider Albums and map individual Tracks without splitting the canonical Album. Second provider program unavailable in this cache. |
| ORESKABAND — WAO!! | No retained candidates | Discovery remains unresolved. Exact-object paste can bypass title discovery. No real object URL/ID was supplied or cached, so real WAO provider confirmation was not performed; mock Japanese/local versus different provider naming validates the bypass. |

Numeric normalization recognizes unambiguous common whole integer titles (0–99,
including Twenty One), and larger number-word phrases with unchanged surrounding
tokens. Ambiguous lexical phrases such as One More are not rewritten. Acoustic/Live
are never globally stripped. Systematic annotations require Album context,
program-wide consistency and strong correspondence. Movement annotations require
Album-wide corroboration even when only one disc uses them. Negative fixtures cover
ambiguous duplicates, weak overlap, trusted collaboration conflict, insufficient
annotation coverage and unestablished Live/studio equivalence.

## 8–10. Ranking and credits

Candidates sort Accepted → Needs review → Rejected, then by explainable established
Artist/title/program evidence, credit warnings and blockers, with stable object-ID
tie breaking. Spotify search order is retained separately when known. Track candidate
navigation also uses evaluated decision/evidence order. Exact credit candidates
dominate extra-credit alternatives only for equivalent complete provider programs;
distinct supported programs remain competing. Additional Artists are retained,
not converted into aliases. Trusted primary-identity contradictions still veto.

## 12–13. Lifecycle and actionable review

Album groups and Album diagnostics share the same current-state SQL, including
cooldown, manual-review contexts and no-unresolved state. Cached comparison is
explicitly separate from current lifecycle. Opening or navigating a cached candidate
does not overwrite Needs retry or renew discovery. Retained candidates appear
immediately, even after discovery TTL; an unfetched program gets a specific fetch
action rather than pretending completed evidence is current.

Completed evaluation clears Needs retry. Retry unresolved prioritizes eligible retry
Albums independently of the normal cursor, refreshes each once, and excludes unusable
manual contexts from the bounded priority batch so they cannot starve eligible work.
Manual review required always opens Review, including Track-level exact-object paste.

| State | Action |
|---|---|
| Needs retry | Retry Album / Retry unresolved |
| Needs review / Manual review required | Review candidates, paste exact object, advanced mapping |
| Waiting for Spotify | Display cooldown; retry after expiry |
| No candidates | Search Spotify again / paste exact object |
| Not searched | Search Spotify |
| Marked Not on Spotify | Check Spotify again in management view |

## 14–18. Explicit controls

Album review provides Connect this Album, Advanced Track Mapping, Paste Spotify
Album, Search Spotify again and Mark album as not on Spotify. Track review provides
Connect this Track, Paste Spotify Track, Search Spotify again and exclusion controls.
Album exclusion snapshots currently unresolved Tracks only, preserves trusted
connections, remains individually reversible, and affects no later-added Tracks.

Paste accepts strict Spotify Album/Track HTTPS URL, URI or 22-character ID. It fetches
that exact object through existing cache/TTL/cooldown infrastructure, without search
title thresholds. Returned identity is checked. Evidence and significant differences
are shown before authoritative user confirmation. Manual confirmation can override
automatic confidence and clears the relevant Track exclusion.

Advanced mapping loads one canonical Album and cached provider programs. Each Track
may choose a unique suggested occurrence, another occurrence or No match. Partial
mapping is valid; repeated provider occurrence assignment is rejected by default.
Title, Artist, duration and position differences require explicit confirmation.
Snapshot checks prevent stale plans from saving. Options use object IDs, not fragile
list indices. Pasting another Album adds its program to the same mapping context:
one canonical Album can acquire Track occurrences from multiple provider Albums.
Advanced mapping does not infer one Album-wide external identity or split user data.

Real split-release and WAO confirmation remain unvalidated because the required exact
objects were not cached/supplied. Partial/split mappings, Japanese discovery bypass,
duplicate prevention and exact fetch/cache reuse were validated with fixtures.

## 19–20. Requests and performance

Real audit, cached navigation, expansion and mapping: **0 Spotify requests**. Exact
Album/Track mock fetch: four HTTP calls (token, Album, program, Track); repeating the
same objects made zero additional calls and no search requests. Retry/cooldown mock
tests exercise one fresh discovery per Album and batch abort/protection behavior.
Explicit fetch retains bounded paging, TTL and Retry-After protections. Exclusions
remain outside automatic retry.

Disposable debug-build 200k-Track measurements:

| Operation | Time |
|---|---:|
| First 200 grouped Albums | 16.42 ms |
| Unresolved count (199,970) | 2.60 ms |
| Expand ten Tracks | 2.04 ms |
| Canonical program (ten Tracks) | 1.21 ms |
| Confirm ten mappings | 5.99 ms |
| Targeted Album row | 1.62 ms |
| Bounded group refresh | 15.33 ms |

These are measured observations, not performance guarantees. Artist context uses
indexes; Album alignment is bounded to an Album and compares its local/provider
programs, never the entire Library. Advanced mapping uses one Album context.

## 21. Verification

Final verification:

* Full core `cargo test --offline --no-fail-fast`: **444 passed, 0 failed, 18 ignored**.
* Subsequent Album/annotation suites after the final short Audiotree-title case:
  **33 Album-candidate tests and 11 alignment tests passed**, three opt-in tests ignored.
* Spotify adapter: **62 passed, 0 failed**; three opt-in unit tests and one live
  integration test ignored.
* Three isolated Qt tests passed: cached Album navigation/confirmation/advanced
  partial split mapping; retry/cooldown; targeted review updates preserving view.
* Strict core/Spotify/Qt Clippy, all three formatting checks and `git diff --check`
  passed. Qt tests emit existing test-helper/EGL/teardown warnings.
* Historical migration fixtures were updated to remove later preference tables when
  constructing older schemas, expect schema 33, and use 34 for future-schema rejection.
  No migration failures remain in the full run.

Regression coverage includes
real retained programs, local Artist conflict safety, source attachment membership,
per-file errors, sequence persistence/reopen, contextual normalization negatives,
candidate order independence, shared lifecycle, exclusions, strict paste parsing,
partial/split mapping, stale snapshot protection, cache-only inspection and Qt review.

## 22. Deliberately unresolved / deferred

* No automatic split-release search/reconciliation or automatic canonical Album split.
* No fuzzy spelling/translation identity rule for Quetzal, WAO or Underwater.
* No automatic acceptance of unestablished Live/studio or original/remaster equivalence.
* No speculative consolidation of Polyphia's two existing release contexts.
* Historical TTNG five-Artist provenance and original four-Artist Giraffes snapshot
  were unavailable; measured counts are disclosed rather than reconstructed.
* Freshman Year's available edition differs from the reported gap; the second
  provider object still requires explicit fetching/review.
* Further search pagination beyond the existing bounded discovery strategy is unchanged.
