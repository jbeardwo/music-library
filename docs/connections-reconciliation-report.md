# Connections program loading and reconciliation correction

No commit. This supersedes the explicit program-loading limitation in the earlier
inline review report. Original library databases were opened read-only for SQLite
backup; real confirmations changed disposable `/tmp` copies only.

1. **Missing candidate programs:** scoped Album discovery and Track-program cache
   entries have separate lifetimes. The review projected whatever optional program
   data happened to be present when it opened and did not rebuild its inspection
   after program completion. It also used strict canonical-edition comparisons
   where the automatic source matcher uses playable-program comparisons.
2. **Removed extra action:** the normal `Load candidate track mapping` button is
   gone. Selecting an Album candidate automatically checks retained/cached programs
   and, only when genuinely absent, starts a candidate-scoped worker. The window
   shows loading, cooldown, configuration/access failure, or provider failure
   explicitly while keeping candidate metadata visible.
3. **Authoritative mapping:** `inspect_playable_program` supplies the same safe
   decisions used by automatic best-available source reconciliation. The shared
   confirmation builder constructs exact occurrences from those rows, then uses
   existing Track-mapping validation and Album confirmation. QML never rematches
   positions/titles. Advanced explicit mapping remains distinct and is not
   overwritten by background inspection.
4. **Freshman Year:** UI-profile copy, canonical display Artist
   `hop along, queen ansleis`, trusted Spotify Artist `7hDLm8O99kRfpY3DjFHQAP`.
   Ranked candidates 1 and 2 both credit `Hop Along, Queen Ansleis`; candidate 1
   has a cached 15-Track program, candidate 2 has no retained program. Canonical
   program has 16 Tracks. `_` ↔ `-` is an exact contextual anchor; Organ Song
   (canonical position 9) remains unmapped; later Tracks align across the insertion.
   Both direct confirmation and the actual QML Connect action persist **15/16**,
   leave Organ Song visible, close review, and preserve scroll/expansion.
5. **Artist identity audit:** no substring/prefix/token-subset identity rule was
   found. Name comparison uses full normalized equality (case is harmless),
   conflicting trusted IDs veto spelling, and explicit equivalence remains valid.
   The separate fresh-library snapshot trusts Spotify Artist
   `3yYUV3hkJit05YIUEODqgp` (`Hop Along`) for this canonical Artist. Its rank 1 is
   therefore Hop Along (10-Track candidate); rank 2 is the full-name Artist
   (15-Track candidate), rejected for conflicting trusted identity. That is the
   measured ranking cause, not substring evidence. This pass deliberately does
   not silently replace that durable trusted identity. In the correctly identified
   profile the full Artist has the stronger trusted evidence and ranks first.
6. **Underwater:** canonical 12 Tracks, cached provider edition 8. Strict review
   treated shifted positions as mismatches. A backend fallback also accidentally
   restored existing other-edition occurrence IDs after removing them from the
   playable comparison view. Correcting that fallback, and corroborating partial
   playable programs with at least three trusted Artist anchors, exposes six safe
   shifted correspondences: Underwater, Satellite, Bad Dream, Half, Shell, Walk.
   Bad Dream/Shell have unparsed featured credits without Track-level trusted IDs;
   these are neutral only inside that independently corroborated program.
   Shower, Half Asleep, Lake, Speechless have no counterparts; Quilt/Moonset have
   meaningful international-collaboration-version differences and stay unresolved.
   The 75% smaller-program coverage gate applies only to playable evidence;
   canonical identity's stricter gate is unchanged. No Artist or Album-specific
   exception was added. Actual QML Connect confirms **6/12**, closes review,
   and leaves the six unresolved Tracks. Synthetic reordered, multiscript,
   contextual-title, and conflict regressions exercise the same shared evaluator.
7. **First-layer candidates:** a manual title/Artist or exact-object lookup may
   have a different discovery fingerprint from the canonical scoped query.
   The old review only reopened that scoped response, losing already inspected
   manual objects. Canonical-Album/market-scoped retained observations now merge
   into the same ranked cached review (bounded to ten). Exact retained objects
   are also reusable without reissuing discovery. Tests close/reopen review and
   recover a manually found candidate without credentials or provider requests.
   Empty states distinguish uncompleted search, active matching, completed empty
   discovery, and provider deferral; an active run is polled for new evidence.
8. **Auto-loading:** program cache lookup is cache-only, permits retained successful
   observations after discovery TTL, and does not renew them merely for display.
   Missing programs use existing paginated Album-program fetching and provider
   protections. No list-wide prefetch occurs. Loading and late program replies
   rebuild the current comparison automatically.
9. **Arrows:** preserve matcher order and selected identity across report refresh;
   each candidate follows the same automatic cache/load path. A late response can
   add that exact candidate's program but cannot replace the currently inspected
   candidate or a new review target. Opening/cached navigation makes zero provider
   requests. A genuinely uncached selected program is the deliberate exception.
10. **Partial Connect:** enabled whenever the displayed plan has safe new mappings.
    Version/identity conflicts and ambiguous rows remain unresolved. Partial
    playable evidence does not assert exact canonical Release equivalence.
    Existing canonical IDs, membership, other-provider preferences, manual tools,
    final-action close behavior, and user-selected review order remain intact.
11. **Repeated Retry root cause:** the nominal cached-preparation stage could make
    catalog-credit provider calls. It flattened typed transient failures into a
    generic storage error and stopped the coordinator before subsequent batches.
    Provider classification now survives this boundary. Temporary failures defer
    HTTP work while the same run drains cached/local work. Opening an Album also
    no longer overwrites provider-unavailable state as cache-refresh-required.
12. **503:** normal clients make at most two short retries after the original
    service request (250ms/500ms minimum), honor short Retry-After, and defer waits
    above five seconds rather than hammering or holding a writer. Authentication
    refresh remains independent and bounded; 429 uses existing durable cooldown.
    Exhaustion leaves an explicit temporary-provider result. Successful discovery
    is cached once and reused; failed attempts do not replace successful cache.
13. **One action:** tests drain 65 Albums/four batches and 101 Albums/six batches.
    A controlled exhausted-provider test processes one successful item, blocks
    further provider work, visits all 65 contexts across four batches locally,
    preserves the successful result, and leaves the rest waiting for Spotify.
    HTTP mock tests separately exercise first request 503 → retry success,
    three 503s → exhaustion, and long Retry-After → immediate deferral.
14. **Automatic lifecycle:** existing startup/import scheduling is retained.
    Tests automatically drain 65 Albums after startup and after import readiness,
    deduplicate overlapping run requests, and avoid rediscovering stable negative
    results. Internal batch boundaries never require a new user action.
15. **Provider/performance safety:** no new provider requests from list browsing,
    sorting, expansion, or scrolling. Cached candidate programs use zero requests;
    missing programs fetch only the selected Album on a worker outside writers.
    Artist-scoped catalog probes, pagination, cooldowns, and request bounds remain.
    The 200k-Track QML scrolling/context regression passed in 3.54 seconds, with
    the existing 300-Album bounded model. This is a test runtime, not a claim that
    every operation has that latency. No eager per-candidate list fetch was added.
16. **Validation/tests:** 500 core tests passed (18 opt-in tests ignored),
    84 Spotify adapter tests passed, and 18 targeted bridge/QML/coordinator/real
    checks passed in isolated processes. These include four multi-batch/startup/
    import checks, the exhausted-provider check, six shared backend checks, four
    UI/manual/context checks, and three disposable real/200k checks. The final
    lifecycle/connections recheck passed 42 tests. Formatting, `cargo check`, and
    `git diff --check` pass. Real QML checks confirm
    automatic cached mapping display, exact partial persistence, automatic close,
    and scroll/expansion preservation for Freshman Year and Underwater. Logs:
    `/tmp/reconciliation-real-ui.log`, `/tmp/alignment-connect2.log`,
    `/tmp/transient-coordinator-final.log`, `/tmp/autoload-test2.log`,
    `/tmp/review-spotify-adapter-unrestricted.log`, `/tmp/reconciliation-200k.log`,
    `/tmp/reconciliation-core-all-final.log`, `/tmp/review-backend-results.log`.
    Real partial-confirmation setup cleared existing Spotify routes and relevant
    unavailable marks only in the copies, so both complete proposed programs and
    residual unresolved rows could be exercised; original choices were untouched.
17. **Limits/deferred:** no live Spotify quota spent to manufacture a transient
    failure or uncached program. HTTP fetching/cache/retry behavior is controlled
    by mock servers; real UI checks use actual retained programs. Automatic
    uncached-program fetching was not exercised against the live Spotify API. No bulk
    MusicBrainz repair, Local discovery redesign, Artist identity overwrite,
    automatic next-item workflow, or new plugin framework was introduced.
