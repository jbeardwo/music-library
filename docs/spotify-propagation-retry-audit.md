# Spotify propagation and retry follow-up — 2026-10-09

No commit was made. This continues the [local/Spotify robustness pass](local-spotify-robustness-audit.md) without redesigning identity or weakening automatic Track-title acceptance.

## Sole Spotify edition as a playable source

User-directed policy update: a sole strongly corroborated Spotify remaster or
expanded/deluxe edition can supply the appropriate songs for the canonical
Release. The absence of an identical Spotify edition is not itself a reason to
leave those songs unconnected. Evaluator version is **10**; discovery strategy
remains **3**. This supersedes the historical SuperBass edition veto below.

The fallback requires an established Artist, a complete bounded candidate page,
no normally acceptable competing candidate, one compatible edition candidate,
and the same conservative Album base title after recognizing a terminal
remaster/deluxe/expanded/extended/anniversary annotation. Album-wide mapping
requires at least three unique title/identity correspondences covering at least
80% of the canonical program, with existing position, duration and trusted
identity guards. A normal compatible edition remains preferred. Competing
alternate editions stay reviewable. Live, Demo, remix, edit, Acoustic,
instrumental and arbitrary subtitles are not relaxed by this rule.

For expanded editions, unused provider bonus Tracks do not reduce canonical
program coverage. The existing shared alignment still represents provider-only
and local-only rows; ordinary positional guards remain active when two-sided
alignment is not strong. Only corresponding canonical Tracks receive provider
occurrences. No bonus Tracks are created/imported, no membership is added, and
unmatched local Tracks remain actionable. Canonical Release titles, dates,
MusicBrainz evidence, overrides and existing associations are retained. Review
shows the provider edition difference and the accepted-playable-edition reason;
no exact Release/Recording equivalence is asserted.

Fresh cached-data validation in disposable `/tmp/playable-edition-final.sqlite`
now selects Spotify **2KQ98XfQCiQ8TuZRjlgplN** for SuperBass: **seven anchors,
seven connected, zero unresolved**, without manual confirmation and with **zero
provider requests**. Its 2015 remaster remains visible as an edition warning,
while the canonical local release stays unchanged. The original DB was not
mutated by this validation. After rebuilding, Retry Album or Retry unresolved
can apply the rule using existing cached evidence.

Regression fixtures exercise sole remasters and anniversary/expanded/deluxe
editions, bonus Tracks, normal-edition preference and competing alternate
editions. Negative cases cover incomplete discovery, substantial duration
differences, duplicate/ambiguous titles, weak partial programs and different
performances. A cache-only adapter persistence test maps four of five local
Tracks to a deluxe program, leaves the fifth unresolved, imports no provider
bonus Track, preserves canonical metadata/membership and does no extra useful
work on a second reconciliation. Album review and rapid-target QML checks,
retry lifecycle, strict Clippy and formatting/whitespace checks are rerun.

Final checks for this policy follow-up: full core **453 passed, 18 ignored**;
the subsequent final candidate suite **37 passed, three ignored**, including
the new incomplete-page/duration/duplicate-title negatives. Focused alignment
(14), Album review (12) and lifecycle (22) suites passed. Spotify adapter
**67 passed, three ignored** (separate live integration remains ignored).
QML Album review/rapid targets and all three retry tests passed. Strict Clippy
passed for core and adapter all targets, plus QML all targets with GStreamer;
all three formatting checks and `git diff --check` passed.

Disposable replay changes trusted Spotify Track count **990→997**, exclusively
the seven SuperBass occurrences. Artists (159), Albums/Releases (250 each),
Tracks (1,255), membership (942) and playable sources (925) remain unchanged.
Membership, file observations, Track/Album overrides and existing Track external
identities compare unchanged. No live Spotify requests and no commit.

## Audiotree discovery and Connections controls follow-up

Both real cached CHON queries, `album:"audiotree live" artist:"Chon"` and
`album:"Chon on audiotree Live" artist:"Chon"`, returned empty pages. The alternate
now drops field restrictions and uses plain text, with unchanged candidate
identity checks and at most two total Album searches. Search strategy is **3**;
older observations remain inspectable without requests. Explicit Search Spotify
again is needed to discover new objects; opening an empty cached diagnostic does
not fetch. Live validation is unavailable here: no Spotify client credentials or
market environment are configured. Mocks validate successful discovery, cached
repeats, absence of field filters, and the two-search bound.

Connections now has a per-Track Mark as not on Spotify button beside Track
connection. The marked-list context action previously passed only the clicked
ID. It now uses the existing selection-aware action IDs and an atomic core bulk
reversal operation. QML regression checks inline marking/reversal, three selected
Tracks moving together, outside-selection behavior and preserved playback/table
state. Core lifecycle tests pass (22); Spotify adapter tests pass (66); QML
Connections and retry tests pass. Strict Clippy, formatting and whitespace checks
pass. No commit or live provider requests made.

## Homey follow-up: equivalent programs with different occurrence IDs

The user confirmed the Homey representations are the same release. The earlier
exact-occurrence-only requirement was unnecessarily restrictive for representation
selection. Evaluator **version 9** now permits different Spotify Track IDs when
both candidates independently pass canonical Album/program checks and share the
same Album title, Artist identity set, type and nonempty release date. Every Track
in both complete ordered programs must have the same nonempty unique normalized
title, valid matching unique position, established Artist identity set, and a
non-approximate duration within **100 milliseconds**. At least three Tracks are
required. Conflicting supplied Recording identities/ISRCs block equivalence.
No Artist/Track/Recording entities are merged and no exact Recording identity is
inferred. Existing trusted associations remain authoritative.

Homey's cached twelve-track programs have identical durations except Waterslide
(206253 versus 206247 ms) and Nayhoo (171080 versus 171071 ms). Every title,
position and Artist-ID set agrees; both releases are Albums dated 2017-06-16.
Credit order differs on Berry Streets and Nayhoo. The deterministic preferred
representation is **0AOM5mFujfMUhdOt7wgrPK**, independent of API result order.
Fresh disposable backups `/tmp/homey-equivalence-validation.sqlite` and
`/tmp/homey-equivalence-final.sqlite` change Homey
from twelve unresolved to **twelve connected, zero unresolved**, using cache-only
completion and **zero provider requests**. This supersedes the historical Homey
row and exact-ID limitation below; the original DB remains untouched.

Positive tests exercise both candidate orders and tiny duration differences.
Negative tests cover missing/approximate durations, a one-second discrepancy,
different Artist identities, Live qualifiers, duplicate titles, changed order,
missing occurrence identities and conflicting Recording/ISRC evidence.
Focused suites passed: 35 Album candidate tests, 14 alignment tests, 12 Album
review tests and 22 lifecycle tests. The Spotify adapter passed 66 tests and
the three Qt retry tests passed. Final extended negative cases also pass.
Strict Clippy passed for core/adapter all targets and QML all targets with
GStreamer enabled; formatting and `git diff --check` passed.

## Changes and causes

1. **Animals missing candidates.** Track diagnostics consumed independent Track-search snapshots only; retained Album program occurrences were not candidates for unresolved Tracks. Provider-only alignment rows must also not count as consumed mappings: that hides the remaining unmatched occurrences.
2. **Album propagation.** Bounded cache-only Album-context candidates use the same alignment as Album review. Mapped occurrences, or unconsumed occurrences from plausible Artist-established Albums, merge by Spotify Track identity and participate in normal evaluation/ranking. Diagnostics distinguish Album discovery, Track search and manual attachment. Accepted contextual programs complete through existing canonical Album/program snapshot checks. Existing associations and exclusions are preserved; contextual transformations do not become global Track rules.
3. **Audiotree discovery.** Evaluation understood established-Artist `Artist on Series Live` boilerplate, but could not evaluate absent objects. With no useful original candidate, a short plain series token permits one alternate `album:"Artist on Series Live" artist:"Artist"` query, including a token already followed by Live. No discography enumeration or combinatorial expansion occurs. This alternate and the older distinctive-token fallback are mutually exclusive. Album-search strategy is version 2; object, Artist-name and Track keys remain version 1. Offline inspection can reuse legacy Album search observations without renewing TTL.
4. **Provider equivalence.** Artist identity sets and complete ordered Spotify Track identities establish equivalent provider representations. An early primary-Artist veto previously prevented order-only equivalents reaching that comparison. Reconsideration requires another Artist-established candidate with the identical complete occurrence program and Artist-ID set. Credit order remains presentation metadata; changed occurrence IDs remain distinct. Equivalent representatives are chosen deterministically, independent of API order.
5. **Remaster annotations.** At least three/80% of local Tracks must share one `(YYYY Remaster)` annotation; the Album must corroborate that exact edition and derived titles must establish strong unique program correspondence. Raw titles remain intact. Review shows “Systematic edition annotation ignored for program comparison; raw titles retained.” This does not assert Recording identity. A bracket guard also incorrectly blocked harmless slash/bracket spacing in the long stylized title. Whitespace around identical bracket/slash symbols now compares equivalently; changed symbols and word boundaries remain conflicts.
6. **SuperBass.** Capitalization already compared equivalently. The actual cached blocker before and after is AlbumVersionMismatch: unqualified local `SUPERBASS!!!! (Black Death greatest hits vol. 1)` versus provider `Superbass​!​!​!​! (Black Death Greatest Hits Vol. 1) (2015 Remaster)`. Provider punctuation contains U+200B, but seven program anchors already corroborate despite it. The local 2005 edition cannot automatically become the 2015 remaster.
7. **Non-Latin prefixes.** Only an identical short non-ASCII alphabetic token repeated across at least three/80% of Tracks can be a proposed annotation. Removal must reconstruct a unique strongly aligned program within established Album context. Varying translations, meaningful suffixes and arbitrary subtitles are retained; there is no transliteration.
8. **Second-Retry root cause.** Batch completion advanced after an earlier Album decision despite newly persisted evidence usable by dependent Tracks. Fresh decisions could clear stale flags before Track replay, so stale-only processing missed dependencies. Independent Track replay also correctly rejected raw contextual titles even when Album-wide comparison was acceptable.
9. **Fixed point.** Cache-only completion precedes provider scheduling and follows each provider Album operation. Accepted Album programs complete canonically, then affected saved, unconnected, unexcluded Tracks replay locally. Up to three local sweeps continue only with new connections. Work is bounded to twenty Album contexts with paged Track enumeration; no full-library scan or provider-search loop drives convergence. Fresh negative discovery remains No candidates instead of becoming Needs retry. Failures expose retry messages; generations prevent cancelled callbacks resuming batches.
10. **Provider impact.** Real validation used zero provider requests. The bounded fallback adds at most one initial search, with none on cached repeats. Propagation/navigation stays cache-only. Thirty-day TTL, Retry-After cooldown, quota abort, explicit refresh, exclusions and exact-object lookup remain intact.
11. **Stale UI root cause.** A reusable diagnostic retained target-specific candidates, programs and manual state while changing targets. Some callbacks mutated counts/status before checking the target. Target changes now clear selection, proposal, candidates, program inspection, paste fields and advanced mapping before loading the new cached context. Review/Song generations guard results before visible mutation, including A→B→A.
12. **Rapid-target validation.** Offline Qt review switches between two Tracks and two Albums five times, checking immediate title/ID, candidate index/count and mapping reset. Late-result checks prove obsolete generations cannot overwrite the new selection/counts. Opening cached targets does not refresh discovery.
13. **Negative normalization.** Coverage retains meaningful Live, Demo, parenthetical subtitles, featured Artist text, varying Chinese prefixes, unrelated/long prefixes, duplicate-title ambiguity, weak overlap, wrong remaster years and mixed annotations. Changed brackets and word boundaries remain conflicts. Inherited candidates still pass normal identity/version/credit checks.

## Fresh real-library replay

Source: read-only `/tmp/music-library-ui-test.sqlite`. Mutation target: fresh SQLite backup `/tmp/spotify-propagation-final.sqlite`. Reproduce cache-only inspection/completion with `tools/qml-diagnostic/examples/spotify_propagation_audit.rs`. These results describe retained data, not assumed live Spotify contents.

| Case | Ranked evidence and current result | Manual work |
|---|---|---|
| TTNG — Animals | Rank 1 accepted, 13 anchors; Acoustic rank 2 rejected for edition. Both missing Tracks now inspect Animals occurrences through Album context. Provider has **Quetzel** and **If I Sit Still, Maybe I'll Get Out Of Here**; local has **Quetzal** and **Make It Out Of Here**. Normal title checks reject the actual wording differences. Two unresolved remain. | Confirm intended occurrences explicitly; no fuzzy rule added. |
| CHON — Audiotree | Snapshot title Audiotree Live, nine Tracks, no useful retained candidate. Mock discovery proves the Artist-on-series fallback returns its candidate and repeated lookup reuses cache. Query generation supports Audiotree and Audiotree Live. | Live discovery unvalidated; zero quota spent. |
| CHON — Homey | Both candidates have twelve anchors and remain competing NeedsReview, ranked deterministically. IDs `0AOM5mFujfMUhdOt7wgrPK` and `3FPWwokgiGkHGwRSdUpugG` have **different IDs for all twelve ordered provider Tracks**. Album Artist sets each contain CHON; Berry Streets and Nayhoo reorder guest Track credits. Twelve unresolved remain. Same-occurrence/order-only fixtures collapse correctly; this pair does not satisfy that premise. | Choose intended provider representation. |
| More Skin With Milk-Mouth (2015 Remaster) | Rank 1 accepted, five anchors. Before five unresolved; after **five connected, zero unresolved** in one local completion, including bracket/slash typography. | None for these five. |
| SuperBass!!!! | Rank 1 AlbumVersionMismatch despite seven anchors. Seven unresolved remain. Capitalization was never the blocker. | Edition confirmation. |
| Elephant Gym — Work | Provider Album `工作 Work` differs from local Work. Provider prefixes vary: `鏡子 Mirror`, `中途 Midway`, `春雨 Spring Rain`, `慶 Celebrate`. They are translations on the provider side, not one repeated local prefix. Rank 1 AlbumTitleMismatch, one anchor, five unresolved. | Review/manual mapping. |
| Elephant Gym — Angle / Balance | Local varying suffixes include `Intro - 序`, `Day time - 白日`, `Strong Ladder - 堅固耐用的梯子`; Balance has `Ocean in the Night - 夜洋風景`, `Dance Together - 一起跳舞`, `Galaxy - 銀河`. Meaningful varying text is retained. Existing trusted associations remain stable. | No broad transformation introduced. |
| Elephant Gym — Underwater | This snapshot's candidate remains TrackCountMismatch, twelve unresolved. Advanced partial/split mapping remains available. | Advanced mapping. |

Fresh replay changes trusted Spotify Track count **966→971**. Artists remain 159, Albums/Releases 250 each, Tracks 1,255, membership 942, playable sources/file observations 925 each. Membership, source observations, Track/Album overrides and existing external-identity rows compare unchanged; no prior rows removed. Original DB untouched.

## Verification and intentional limits

The batch regression persists a program during one provider discovery while returning an earlier empty decision. One invocation connects all three dependent Tracks, clears stale rows, and a second invocation does no provider work. Provider operation count is exactly one. Qt retry tests also cover cooldown and no Track-search fan-out. Final test/check results follow below.

Live CHON discovery is deferred to an explicit provider action. Genuine wording changes, translated titles, different occurrence IDs and edition conflicts remain manual review. Automatic split-release discovery, transliteration, fuzzy spelling identity and cross-process cache-miss coalescing are not introduced. Existing URL/URI attachment, advanced partial/split mapping, Album-level Not on Spotify and authoritative manual confirmation remain available.

### Executed checks

- Full core suite: **450 passed, 18 ignored, zero failures**. Subsequent focused final run: 14 library unit tests, 14 program-alignment tests and 10 Song-resolution tests passed, including the added query-generation and mixed-remaster negative cases.
- Spotify adapter: **66 passed, three ignored**; its separate live-authorized integration test remains ignored. Mock tests include Album propagation, bounded fallback, negative-cache reuse and no third search for an empty series-Live query.
- MusicBrainz adapter: **20 passed, three live tests ignored**. Initial sandbox binding failures were rerun outside the sandbox against local mocks; no live service was contacted.
- QML Album review/rapid targets: passed. QML retry lifecycle: all three passed. QML Connections update/context preservation: passed. Tests ran in separate processes to respect Qt application-thread ownership.
- Explicit 200k QML Connections fixture: final run passed in **12.77 seconds**, with bounded 200-Album groups, at most 600 materialized Track rows and zero fake-provider requests. Fresh deterministic fixture copy used; prior copies with already-connected Tracks do not satisfy its 200,000-unresolved premise. The test helper now distinguishes the small 450-Track fixture and waits for the asynchronous large total instead of asserting it immediately.
- Strict Clippy passed for core and Spotify adapter with all targets, and QML with all targets/GStreamer enabled. Default QML application Clippy also passes; default all-targets exposes the existing feature-dependent unused `test_handoffs` helper. Formatting checks for all three manifests and `git diff --check` passed.

Qt's environment emits existing EGL/software-driver and timer-thread cleanup warnings; the listed assertions pass. A combined full QML-suite run is not claimed. All real mutations used disposable backups; live Spotify requests: **zero**.
