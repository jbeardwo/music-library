# Spotify reconciliation lifecycle

## Versioned local reconciliation

`spotify_lifecycle::EVALUATION_VERSION` is currently **5**. Increment it when matcher/evidence semantics change. Migration 0029 adds the last evaluation version, an indexed stale flag and a separate `needs_retry` state to Spotify review bookkeeping. Existing trusted associations are preserved. Legacy unresolved rows start stale; successful associations receive only bookkeeping initialization, never identity replacement.

Version 3 treats ordinary apostrophe typography as equivalent title evidence under the standard exact-title program requirements, rather than imposing the additional title-relaxation anchor threshold. Straight, left/right typographic, modifier-letter and fullwidth apostrophes normalize to the straight apostrophe in title comparison/discovery. Version words and other structural gates remain meaningful. The real Library's `X’ed Out` contains U+2019, which normalizes to `X'ed Out`; this observation alone does not establish an Album association.

Version 5 permits up to **10,000 ms** duration difference for otherwise qualified Spotify Track acceptance. The diagnostic displays that tolerance. Differences over ten seconds, version qualifiers, position conflicts, identity conflicts and candidate competition retain their gates. The separate near-title/typo program gates retain their existing stricter duration rules; other providers are unchanged. Existing trusted associations are untouched.

Follow-up validation identified **X’ed Out and Tired**: known duration **278,986 ms**, Spotify **274,506 ms**, difference **4,480 ms**. On a disposable copy of the then-current Library, version 5's automatic cached pass accepted this Track with **zero provider requests** (273 unresolved → 272). The original Library/test case was not modified. Tests cover 4 seconds and exactly 10 seconds accepting, 10,001 ms rejecting, successful association persistence, and position/Live conflicts still rejecting within tolerance.

The title-order Album retry query uses the existing covering `album_order_title` index with a keyset lower bound. Read-only measurements on the 200k fixture returned 20 Albums in median **0.32 ms initially** and **0.04 ms at a deep cursor**. The worker regression verifies Album-by-Album fallback ordering; pagination tests verify alphabetical Album order with ID ties and ascending Track positions.

Version 4 fixes cached replay stopping after a rejected Track-search page, without consulting an available established Album program. The regression resolves four Tracks from that persisted program despite their rejected individual cached candidates, with no provider. The review completion message explicitly directs Needs retry cases to the Album-first batch action. In the subsequent live read-only audit, 321 unresolved Tracks lacked retained candidates/programs; those require explicit provider retry, not automatic search on page open.

Relevant writes invalidate affected unresolved evidence: Track titles/overrides/positions/relationships, source associations and matching metadata, typed catalog observations, trusted identities, ordered credits, Recording evidence and confirmed Artist-equivalence components. Program-dependent changes invalidate the affected Album; credit changes use indexed Artist relationships. Unrelated Albums/Artists remain current. Identical observations and availability-only changes do not invalidate matching. Already-stale rows are not rewritten on every import operation.

Bounded complete Song candidate pages retain the typed discovery input and all competing candidates (maximum ten). Established Spotify Album programs are also persisted. A changed evidence version can replay these through the existing current evaluators without network access. Missing/incomplete pages, changed search scope or unavailable established Album context become **Needs retry**. A reason string alone cannot reconstruct a safe candidate page. Rejected Album discovery without a sufficient stored program/page requires a fresh explicit retry.

Opening or refreshing Spotify Connections starts local-only work on a separate connection/thread in **32-Track batches**. Missing caches are classified with batched SQL; candidate caches and Album programs are loaded in batches. Cached programs are evaluated Album-by-Album. Independent cached Track acceptance uses the existing indexed canonical assembly and transactional current-evidence revalidation. All provider work remains outside write transactions. Current-version failures stop rerunning, including after restart. Successful identities and manual exclusions are skipped.

Accepted native Track decisions use the normal atomic acceptance path, including verified persistence and explicit persistence errors. Program replay preserves already trusted associations. Track-ID patches update visible review rows and counts; offscreen-only bookkeeping does not refresh the table. Progress notifications are throttled. Viewport, sorting, filters, widths and queue state are retained. There is no polling or provider work during opening/scrolling.

Refreshing a Spotify diagnostic also reconciles its cached review window with persisted association state. This covers identities written outside that window's active completion callback. The QML Album regression checks immediate multi-row removal after acceptance and a previously displayed row becoming connected outside the matcher callback.

## Explicit retry

**Retry unresolved** processes up to **20 distinct Albums** per click through the existing AlbumMatcher queue/cooldown machinery. Albums are traversed in **title A–Z order**, with stable ID ties. Each Album completes its program reconciliation and then its remaining Track fallback before the next Album starts. The fallback follows **disc/track position order**, with stable ID ties and **20-Track cursor pages**, using the existing Song worker. All targets are saved, unresolved and unmarked. Eligibility is checked again before dispatch and persistence; pending Album requests coalesce. A Track lookup failure stops the fallback and preserves the worker's provider cooldown. The Album cursor advances only after the entire batch finishes, so failures do not abandon the remaining Album work. Subsequent clicks advance the Album cursor; an exhausted pass resets it for another explicit pass.

This is deliberately bounded to the selected Album batch, rather than an automatic whole-Library provider crawl. A Track-page boundary never discards remaining work. No provider activity is launched merely by opening the review view or by a version change.

Cache-pass status and explicit provider-retry status are stored separately. A zero-work cache pass says "No stale cached evaluations" and cannot overwrite the currently checked Track, retry completion or provider error. Provider retry status takes precedence in the review header.

The review context menu's **Mark as not on Spotify** applies to the current selection when the clicked canonical Track belongs to it; right-clicking outside the selection targets only the clicked Track. The batch is one database transaction, deduplicates IDs and skips Tracks connected since selection. Rows/counts update together. The diagnostic button stays Track-specific. Storage and actual QML menu tests cover durable bulk marking, selected versus unselected targets, marked-view inclusion and table/playback-state preservation.

The previous fallback treated its 20-row page limit as a total batch limit, consuming the Album plan and leaving later Tracks unsearched. In the real unresolved queue, X’ed Out was sixth of twenty Albums with 37 preceding unresolved Tracks, so it was omitted by that limit despite belonging to the first Album batch. The worker regression now checks all 46 eligible Tracks across a forty-Track Album and a later eight-Track Album, including exclusions and a mark made while a request is in flight. The real X’ed Out test case was inspected without provider requests or association changes.

## Reversible negative user knowledge

`track_provider_exclusion` stores `(canonical Track ID, provider, declared_at)`, with a unique composite key and provider-list index. This is portable user state, independent of rejection reasons, source availability and Library membership. Spotify is the only implemented UI/provider for this decision.

**Mark as not on Spotify** is available on an unresolved review row's context menu and in the existing diagnostic. It is reversible and needs no extra confirmation. The diagnostic explicitly identifies it as a manual decision. Marked Tracks disappear from the actionable list, are excluded from both counts of actionable work and all automatic/batch reconciliation, and remain marked across version/evidence changes and restart. Album matching can retain their metadata as program context while skipping their association writes.

The shared details table has an **Unresolved / Marked Not on Spotify** selector under `⋯ → Spotify Connections`. Both lists/counts concern saved Library Tracks and use stable IDs and bounded keysets. The header shows separate unresolved and marked counts. Empty states distinguish reviewed eligible Tracks from an empty marked list.

**Check Spotify again**, in the marked context menu or diagnostic, deletes only the provider exclusion and marks the review state stale. Current cached evidence can then connect the Track, or it can return to Unresolved/Needs retry. Clearing a mark does not manufacture an identity. A successful explicit trusted connection clears an exclusion transactionally. Database triggers prevent marking a currently connected Track and clear exclusions when trusted direct/manual/current-Album associations arrive. Library membership, local sources, MusicBrainz identities, display credits and playlist references are unchanged.

## Real Library validation

All mutations used `/tmp/spotify-lifecycle-real.sqlite`, a disposable backup of the current Library. The original was not changed.

| Local-only upgrade pass | Result |
| --- | ---: |
| Starting unresolved | 350 |
| Starting marked | 0 |
| Locally checked | 350 |
| Newly connected | 0 |
| Ending unresolved | 350 |
| Needs retry | 350 |
| Automatic provider requests | **0** |
| Elapsed | about 29 ms |

These legacy rows contained reasons but **no durable candidate pages**. The zero-acceptance result is intentional: the app cannot safely invent the missing historical candidates or fetch them automatically. A second pass checked zero rows, demonstrating that current-version failures are not sticky work that runs repeatedly.

An explicit bounded capture to validate real historical candidate replay was attempted twice; Spotify returned HTTP **502**, then **504**, before returning a candidate page. Fresh real cached acceptance validation was therefore blocked by provider availability. No associations were forced. Synthetic integration tests and the 200k fixture exercise automatic cached acceptance, including eight accepted cached Track cases, multi-Track program replay and the actual QML automatic update path.

**Hop Along — Organ Song** was marked on the disposable copy: unresolved **350 → 349**, marked **0 → 1**. The mark survived restart, was excluded from stale work, and the canonical input disabled Spotify search. **Check Spotify again** restored unresolved **350**, marked **0**, and reconciliation eligibility without creating an association. Backend/worker tests verify batch and Album retry skipping. Before/after user-state counts stayed identical: **484 memberships, 473 sources/associations, 373 Track external identities, 207 Album identities, 150 Artist identities and 314 playlist entries**.

## Tests and performance

Sixteen focused lifecycle tests cover old versions, cached positive/negative/competing pages, changed discovery scope, current-version restart behavior, scoped catalog/Artist-equivalence invalidation, no-op observations, explicit persistence failures, durable/reversible exclusions, identity/source/membership/playlist preservation, multi-row program replay, trusted association preservation, bounded retry planning and actual one-Album search batching. The Spotify adapter exclusion test observes zero token/API requests. The dedicated fallback worker test checks cursor paging beyond twenty Tracks, stable targets, trusted/excluded skips and an exclusion added while a request is in flight.

Playlist-driven program reconciliation also checks provider exclusions before discovering program requests and again transactionally before attaching Track identities. Its regression verifies that marking prevents reconciliation and preserves playlist targets; clearing the mark restores eligibility. All ten playlist-import tests pass.

The enhanced real QML review test exercises automatic stale replay, immediate row/count removal, mark/secondary-view/check actions, disabled marked diagnostic search, and viewport/sort/filter/width/queue preservation. Nine isolated QML regression tests and the opt-in 200k bounded review test pass. The optimized 200k QML run took **13.98 s**, including its interaction/wait sequence, versus the preceding approximately 15 s baseline.

Core suite: **374 passed, 15 ignored**, with the previously documented unrelated `catalog_playlist_identity_membership_reload_and_duplicates` assertion excluded. Final focused tests were rerun after invalidation/cache optimizations. Spotify adapter: **50 passed, 2 ignored**, plus one ignored live fixture. MusicBrainz: **20 passed, 3 ignored**. Strict Clippy across core, both adapters and QML, formatting and `git diff --check` pass.

Release-build measurements on independent disposable copies of the existing 200k fixture, including 1,000 marked Tracks:

| Operation | Measurement |
| --- | --- |
| Unresolved 201-row initial/next pages | 3.22 / 1.86 / 1.86 ms |
| Marked 201-row initial/next pages | 4.01 / 3.49 / 3.05 ms |
| Indexed stale scan | p50 0.41 ms / p95 0.84 ms |
| Local missing-cache batch, 32 Tracks | p50 0.73 ms / p95 1.47 ms |
| Cached replay batch, 32 checked / 8 accepted | about 33 ms, off the UI thread |
| Open including schema upgrade | about 330 ms |

Browsing joins no candidate/provenance payloads. Lists remain bounded to 600 QML rows. All performance paths made zero provider requests. A large-import regression exposed redundant stale-row rewrites; the final invalidation skips those writes, and the import regression was rerun successfully.

## Deferred

Optional Album bulk marking, marked-date columns, a generic provider management UI, metadata correction/tag rewriting and unbounded/background provider retries remain deferred. There is no removal/re-import, fuzzy merging, source-priority change or playlist redesign. Historical absent candidate data cannot be retroactively recovered locally. No commit was created.
