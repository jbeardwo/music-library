# Connections: one review and confirmation workflow

Candidate program loading and reconciliation behavior are superseded by
[the subsequent correction report](connections-reconciliation-report.md).

Implemented without committing. This supersedes the manual handoff and mapping
presentation described in [the earlier review report](connection-review-report.md).

1. **Previous failure:** the shared window projected optional diagnostic program
   rows separately from the confirmation plan. When that projection was absent,
   only candidate metadata/Library rows appeared. More seriously, its manual button
   opened the old provider diagnostic, moving selection and confirmation out of
   the new workflow. The new shell was therefore not the complete interaction.
2. **Actual Album mapping:** Spotify rows now expose canonical Track IDs and the
   exact provider occurrences from `AlbumSelection.matches()`. Metadata, arrows,
   and mapping all refer to the currently inspected candidate. Existing program
   evidence supplies warnings; rows distinguish proposed new connections,
   normalized titles, reordered positions, unmapped Tracks, provider-only entries,
   and context outside the current unresolved selection. MusicBrainz rows use the
   same Recording correspondences that its Connect action confirms. Local rows
   show the explicitly selected canonical Track → file attachment.
3. **Connect this candidate:** calls existing authoritative Spotify Album/Track
   confirmation, MusicBrainz Recording confirmation, or canonical Local ingestion.
   The small `PositionedMatch.candidate()` accessor exposes the existing plan;
   it does not introduce another matcher or persistence path. QML performs no
   provider-association writes or Track matching.
   `prepare_spotify_connections_confirmation` projects that plan onto existing
   supported program decisions, leaving version/identity conflicts and ambiguous
   correspondences unresolved. Legacy explicit positioned review and deliberate
   advanced mapping retain their existing confirmation semantics. Automatic
   matching/ranking is unchanged.
4. **Partial Albums:** safe proposed mappings connect; unsupported Tracks remain.
   Successful actions close review, update counts, and retain only unresolved
   Tracks. Fully resolved groups disappear. No inherited Album preference exists.
5. **Track Connect:** confirms the exact selected occurrence/Recording/file shown
   on the right. Choosing a manual search result changes presentation only; it
   cannot persist an association before Connect.
6. **Inline manual tools:** Find another manually switches the same dialog to a
   provider-specific search/selection panel. Spotify supports Album/Artist and
   Track/Artist searches and exact URL/URI/ID lookup. Results return to the same
   comparison. MusicBrainz searches catalog Albums, inspects a Release, and lets
   the user select a Recording before confirmation. Local chooses a canonical
   Track when necessary, then opens only the native file picker and returns to
   the same comparison. Advanced Spotify mapping is an inline editor; applying
   it prepares a plan, returns to comparison, and still requires Connect.
7. **Old Spotify UI retired from this path:** Connections Match and its manual
   actions never open the old Spotify Track, Album, or advanced diagnostic
   windows. They remain available to existing diagnostic callers elsewhere.
   Runtime regressions assert this for both manual scopes and all three providers.
8. **Real Spotify Album validation:** disposable fresh-library copy, TTNG / Animals,
   two persisted candidates. Only in the copy, existing Spotify routes were cleared
   to reopen reviewable connections. Candidate 1 opens immediately; next/previous
   change metadata and the actual mapping without requests or writes. Connect
   restores **13/15** safe routes, leaves the two unsupported title correspondences
   unresolved, closes review, and preserves the surrounding list context.
   On a separate disposable profile copy, Stab the New Cherry EP was selected
   manually using **ID, URI, and URL**, inspected with its actual two-Track mapping,
   and confirmed in the same window. Selection alone changed no associations;
   Connect removed the resolved group. All **222** Spotify unavailable choices
   remained intact. These real manual checks used retained candidates, not live
   Spotify HTTP searches; search-result handling is covered with injected provider
   replies and the existing adapter's HTTP regressions.
9. **Real Spotify Track validation:** Underwater / Shell used inline ID/URI/URL
   selection and comparison, then Connect. Matched count changed **7/12 → 8/12**;
   four unresolved Tracks remained. No old diagnostic appeared and the window
   closed at the same list location.
10. **Real MusicBrainz validation:** the profile's 104-gap baseline was copied.
    100 Onces / OCDC was reviewed without a usable local route. Inline explicit
    catalog search and Release inspection exposed the actual **OC/DC** Recording.
    Selecting it displayed that title on the right; Connect saved its Recording
    identity and closed review. Coverage changed **838 → 839** trusted saved
    Tracks. The Album program also remained inspectable. No spelling/matching
    policy was relaxed and no bulk gap repair was attempted.
11. **Real Local validation:** Album and Track scopes used Three Trapped Tigers /
    Numbers 1–13 / 8 and its actual FLAC. In the disposable copy its availability
    observation was temporarily disabled. Inline file choice returned to comparison;
    Connect restored the source through canonical ingestion without duplicate
    entities, membership changes, or changes to the audio file.
12. **Context:** provider/tab, bounded list window, scroll anchors, and unrelated
    expanded Albums are preserved. A separate 200k-Track fixture exercises forward/
    reverse browsing, a draggable scrollbar, and closing a deep Album review after
    marking unavailable. The model remains bounded to 300 Album groups. No next
    object opens automatically.
13. **Requests and async work:** opening and navigating persisted candidates,
    selecting already-retained exact objects, and confirmation made **0 provider
    requests** in Spotify real-copy checks. Manual search/fetch is explicit and
    uses existing provider cache/cooldown policy. MusicBrainz live validation made
    explicit Search/Inspect requests; its HTTP count was not separately instrumented.
    Pending results are scoped to the active review; cancellation/reopening drops
    obsolete receivers. Errors retain the current candidate and allow retry.
14. **Tests:** focused storage/confirmation regressions, shared presentation tests,
    inline QML result-selection/advanced-mapping/confirmation tests, existing Spotify
    diagnostics, real disposable checks for all providers, adapter regressions,
    and bounded 200k scrolling. Exact results are recorded below.
15. **Intentionally deferred:** matcher redesign, automatic Local candidate
    discovery, MusicBrainz edition correction, and repairing existing unsupported
    Track correspondences. Missing Spotify program data can be explicitly loaded
    inside this window; opening review never fetches it implicitly. No automatic
    review queue, next-item feeding, schema migration, or separate credentials
    system was added.

Reproduction uses the individually isolated QML tests in
`tools/qml-diagnostic/src/main.rs`. Real tests require a disposable `/tmp` database,
`MUSIC_LIBRARY_DIAGNOSTIC_DATABASE`, `QT_QPA_PLATFORM=offscreen`, and
`-- --ignored --nocapture`; they intentionally change the copy. Validation logs are
`/tmp/inline-*.log`. Original databases were opened read-only solely for SQLite
backup; validation did not write to them.

Final validation results:

* Core Connections, Spotify Album review, Spotify connection state, and Local
  ingestion suites: **40 passed**, two opt-in tests ignored. The new version-
  conflict regression also verifies that legacy explicit review remains unchanged.
* Spotify adapter regressions: **82 passed**, four live/opt-in checks ignored.
* Shared review/manual backend tests: **6 passed**, including ranked candidate
  mapping, exact confirmation-plan IDs, partial persistence, failed manual requests,
  and obsolete reply isolation.
* Six isolated regular QML tests passed: inline manual Album/Track selection and
  confirmation with advanced mapping; ranked review/lifecycle; Music/Connections
  navigation and provider controls; legacy Album review/advanced mapping; Spotify
  list updates/context; and marked-Track manual search. They assert that legacy
  windows stay closed during the Connections manual workflow.
* Five isolated opt-in checks passed: real Spotify multi-candidate Album plus
  inline manual Track; real inline manual Spotify Album; real MusicBrainz; real
  Local Album/Track; and deep 200k-list context/scrollbar behavior.
* Root/QML bridge formatting and `git diff --check` passed. The rendered Album
  comparison was inspected: proposed mappings and fixed Connect/manual/adjust/
  unavailable actions are visible and fit the existing window.

The inline manual QML regression is
`connections_inline_manual_album_and_track_confirm_displayed_mapping`; its
provider search replies are injected, while selection, preparation, and persistence
use the production backend. Real Spotify URL/URI/ID checks use retained provider
objects. Live Spotify discovery was not required for these checks.
