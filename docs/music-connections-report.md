# Music / Connections implementation and validation

Implemented without committing or modifying original validation databases.

The subsequent [Album/Track review refinement](connection-review-report.md)
supersedes the original Match-entry description below and records its validation.

1. **Top level:** persistent Music / Connections selector; default Music → Artists.
2. **Music:** existing Artists, Genres, Albums, Songs, Playlists pages and handlers
   remain in their existing hierarchy. Navigation remembers the prior Music view.
3. **Providers:** Rust descriptors expose Local, MusicBrainz, Spotify; one shared
   management page dispatches provider-specific controls and matching actions.
4. **Shared infrastructure:** core grouped/expanded queries, coverage, unavailable
   writes, keyset cursors, QML Album delegates, Track lists, scrolling, hover/focus,
   lower tabs, and navigation/expansion state.
5. **Local header:** authoritative configured roots, Add Folder, Remove Folder,
   Rescan. Adding registers the root and invokes existing root-scoped ingestion; Rescan
   explicitly covers all configured locations.
6. **MusicBrainz header:** describes the catalog identity provider; explicit coverage
   reports trusted Recording identity gaps. No account or fabricated availability.
7. **Spotify header:** existing authorization state, selected/device choices,
   reconnect, device refresh, authorization details, cooldown, and retry control.
   Account display names are unavailable in current persisted state.
8. **Missed:** Local requires a persisted available unsuppressed source;
   MusicBrainz requires trusted canonical Recording identity; Spotify reuses
   trusted associations and persisted authoritative review reasons.
9. **Unavailable:** durable independent `(Track, provider)` choices, scoped exactly
   like existing database-local Spotify choices.
10. **Migration:** 0036 preserves existing Spotify exclusion rows and timestamps
    in place. It adds connection views, sorting index, and Local/MusicBrainz
    successful-connection cleanup triggers. Membership/associations are unchanged.
11. **Albums:** canonical Artist A–Z, Album A–Z, ID tiebreaker; matched and total
    counts include all current saved Tracks, independent of the lower tab.
12. **Expansion:** only unresolved or marked Tracks; bounded 200-Track pages in
    disc/Track/ID order, virtualized delegates and explicit Track-page controls.
13. **Match:** Spotify reuses existing Track/Album/review/advanced mapping UI.
    Local Album inspection selects a Track for canonical file attachment.
    MusicBrainz Album inspection selects a Track, then searches candidates,
    inspects representative Release correspondence, and explicitly confirms its
    Recording identity. No matching algorithms were redesigned.
14. **Unavailable actions:** Track marking, confirmed Album snapshot marking,
    Check again, and automatic clearing after successful connections.
15. **Updates:** connection revision notifications plus existing library changes
    refresh bounded visible windows; stable IDs preserve delegates, expansion,
    scroll anchors, and provider state. Coverage/file parsing run off the UI thread.
16. **Old entry:** Spotify Connections menu navigates to the new Spotify provider.
    Compatibility tests still exercise the old internal browser API; it is not a
    second primary UI entry.
17. **MusicBrainz coverage:** fresh-library copy: 930 saved, 831 trusted Recording
    identities, 99 missing across 13 Albums. Existing UI-profile copy: 942 saved,
    838 connected, 104 missing across 13 Albums. These gaps were exposed, not fixed.
18. **Local coverage:** fresh-library copy: 930/930 usable. UI-profile copy:
    918/942 usable, 24 missing across two Albums. Root `/mnt/f/music` displayed.
    Example missing group: gumby's junk — Business & Pleasure, 0/11 matched.
19. **Spotify comparison:** fresh copy 641 connected, 289 missing, 0 marked;
    authoritative missed count 289. UI-profile copy 694 connected, 248 missing,
    222 marked; authoritative missed count 26, exactly `248 − 222`. All 222
    existing Not on Spotify choices survive migration. A real-copy UI test
    temporarily removed and manually restored a previously trusted route for
    The Bulletproof Tiger — You Wanna Kiss About It? — An Awkward Moment With a
    Grilled Cheese Sandwich. Missed count `26 → 27 → 26`; marked row cleared and
    fully resolved Album disappeared immediately. This reused the original trusted
    occurrence, rather than making an unverified new match.
20. **Network:** generic queries have no provider dependency. QML fake-worker
    assertions cover provider switching, expansion, dialogs and unavailable actions.
    Real-copy manual restoration and coverage audits made zero provider requests.
    MusicBrainz search/inspection and Spotify auth/device/search/retry are explicit.
21. **200k:** disposable deterministic fixture, 20,000 Albums. Across 20 pages,
    development-build p95 grouped query: Local 9.47 ms, MusicBrainz 9.67 ms,
    Spotify 20.26 ms. Empty marked lists ~0.63 ms; Spotify marked page 9.93 ms.
    Expansion examples 0.36–0.72 ms. Full coverage took 0.80–1.72 s and is therefore
    explicit/background work. QML forward/reverse traversal keeps ≤300 Albums and
    passes draggable-scrollbar validation. No full-library QML scan or eager Track
    instantiation.
22. **Tests:** focused storage/migration/connection tests, existing core regressions,
    isolated QML navigation/library/provider/dialog tests, opt-in 200k QML test,
    and opt-in disposable real manual-confirmation test. Exact final results below.
23. **Deferred:** optional alternate sorts; account display names absent from
    persisted auth state; arbitrary MusicBrainz edition replacement/correction;
    richer bulk MusicBrainz/local mapping. MusicBrainz manual Album matching is
    deliberately a Track-by-Track explicit Recording workflow, not a new matcher.

Reproduction uses `cargo run --offline --example connections_audit -- COPY.sqlite`.
The opt-in QML tests are `connections_200k_bounded_continuous_scrolling` and
`connections_real_manual_confirmation_updates_shared_page`; both require
`MUSIC_LIBRARY_DIAGNOSTIC_DATABASE=/tmp/COPY.sqlite`, `QT_QPA_PLATFORM=offscreen`,
and `--ignored`. Original snapshots were opened read-only and copied with SQLite
backup to `/tmp/connections-real.sqlite`, `/tmp/connections-profile.sqlite`, and
`/tmp/connections-200k.sqlite`.

Final validation results:

* `cargo test --offline`: 494 passed, 18 opt-in tests ignored, no failures (full
  suite before the last two added regression cases).
* Final focused core run: 61 passed across Connections, local ingestion, Spotify
  Connections and reconciliation lifecycle, including all six new connection tests.
  All six connection tests also passed after the final keyset query refinement.
* Nine isolated UI/bridge checks passed: new Music/Connections navigation;
  marked Spotify manual discovery/confirmation; existing Spotify review context;
  existing Album review/advanced mapping/bulk confirmation; Music navigation;
  preferences; native local chooser/import; playlist revision stability; removal.
* Disposable real manual-confirmation and 200k continuous-scroll/scrollbar tests
  passed. Generic model remained at most 300 Albums; expanded Track pages at most
  200. No provider requests were observed in browsing or real restoration checks.
* `git diff --check` and Rust formatting checks passed. No commit was created.

The combined Qt suite was stopped after Qt reported application/thread affinity
warnings and stalled in an existing scrolling test; the relevant UI checks were
then run in isolated processes. Isolated Qt tests can emit the existing nonfatal
QObject timer teardown warning. This report does not claim the combined Qt suite
completed.

Explicit Spotify manual discovery can inspect a marked Track while keeping its
unavailable preference. Its reply retains the original persisted snapshot, so
existing automatic acceptance remains excluded; only explicit confirmation clears
the preference. Playback/coordinator discovery retains its original guard. This is
a manual workflow routing change, not a candidate-ranking or matching-policy change.
