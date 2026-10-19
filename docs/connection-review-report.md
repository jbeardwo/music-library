# Connections Album / Track review refinement

The subsequent [inline workflow correction](connections-inline-review-report.md)
supersedes this version's manual handoff and diagnostic mapping projection.

Implemented without committing. This refines the existing Music / Connections
page; its navigation, provider headers, lists, and unavailable model remain in
place. No review queue or automatic next-item behavior was added.

1. **Shared presentation:** one QML review dialog and Rust presentation state
   serve both Album and Track scopes for all three providers. Provider code owns
   candidate discovery, evaluation, confirmation, and manual attachment.
2. **Library side:** effective canonical title, Artist, Album, known date/type,
   duration/position, and ordered program. Labels say Known Library Album/Track.
   No file is required; unknown fields remain blank rather than inferred.
3. **Best first:** Spotify uses the existing ranked Album report and ranked Track
   Selection, including Album-derived occurrences. MusicBrainz uses the existing
   Recording comparator and program correspondence; unrelated Recordings from
   an inspected Release are not presented as Track guesses. A retained inspected
   MusicBrainz Release is one Album candidate. Local has no invented automatic
   candidate finder: its normal zero state leads to explicit file selection.
4. **Navigation:** previous/next inspect cached candidates only; opening starts at
   index zero. Arrows are hidden for zero/one candidate and disabled at boundaries.
   Navigation changes no associations, rankings, or unavailable choices.
5. **Album comparison:** Artist, title, known date/type, and aligned programs.
   Existing program decisions explain exact/normalized titles, reordering,
   differences, and Library/provider-only positions. Programs are bounded to 200
   displayed rows. Cached artwork is not fetched for review.
6. **Track comparison:** title, Artist credit, Album, duration, position, and
   available provider date/type fields. Local preview shows the selected file's
   observed tags against the selected canonical Track.
7. **Evidence:** existing evaluator decision, identity/title evidence,
   corresponding Track counts, blockers, and warnings. The concise summary is
   above the program. Detailed acceptance rules remain in Spotify's advanced
   tools; the shared dialog does not invent scores.
8. **Album Connect:** Spotify reuses the existing confirmation plan/mapping and
   difference confirmation. MusicBrainz confirms supported unresolved Recording
   correspondences in one atomic transaction. Local confirms the explicitly
   selected Track/file through canonical ingestion.
9. **Partial Albums:** successful partial confirmation closes the window. Matched
   counts update; only remaining unresolved Tracks stay relevant. Unsupported
   correspondences are left for the user's later manual/Track review.
10. **Track Connect:** existing Spotify confirmation, trusted MusicBrainz Recording
    association, or canonical Local source attachment. Canonical identity and
    library membership are retained.
11. **Manual matching:** Spotify Track search/exact URL/URI/ID and Album advanced
    mapping remain accessible. MusicBrainz retains explicit Album search, Release
    inspection, and Recording choice. Local selects a file, previews tags, then
    attaches through existing ingestion. Album manual tools can select the
    relevant canonical Track; this chooser is not the normal Match entry screen.
12. **Unavailable:** Track actions remain provider-specific; Album actions snapshot
    current unresolved saved Tracks only. Successful connections clear that
    provider's preference. No inherited Album rule is created.
13. **Lifecycle:** Connect, successful manual confirmation, and unavailable actions
    close review. Errors retain the current target/candidate and explain failure.
    Generation checks discard stale Spotify/MusicBrainz/file-preview results;
    an older file attachment cannot close a newer review.
14. **Place preservation:** provider, lower tab, bounded window/cursor, expansion
    dictionary, and nearby row/pixel anchors survive review. Nearby surviving
    Albums anchor the viewport if the reviewed group disappears. No whole-library
    reload occurs.
15. **Clicks:** Match opens the comparison directly. Persisted candidates require
    no Fetch, Review, or first-candidate selection click. Explicit difference
    warnings and advanced/manual tools remain available when appropriate.
16. **Requests:** opening, arrows, and cached confirmation made **zero provider
    requests** in fake-worker/cache tests and real Spotify validation. Local file
    preview is explicit filesystem work. Only explicit MusicBrainz Search/Inspect
    used the network in real validation; its HTTP request count was not separately
    instrumented. Existing Spotify caches/cooldowns/Retry-After policy is unchanged.
17. **Real Spotify:** disposable copies of both existing libraries were used.
    Animals opened directly on ranked candidate 1 of 2; next/previous were offline.
    Its two remaining title discrepancies have no trusted program mapping, so
    Connect correctly stays disabled rather than forcing a match. A separate
    correct Stab the New Cherry EP candidate connected its two saved Tracks:
    missed count 26 → 24, group removed, all **222** unavailable choices retained.
    Underwater's Shell Track connected with existing best-available semantics:
    Album count **7/12 → 8/12**, four unresolved Tracks remained. Windows closed
    and surrounding location/expansion was retained.
18. **Real MusicBrainz:** the profile originally has 942 saved Tracks, 838 trusted
    Recording identities, and 104 gaps across 13 Albums. The 100 Onces Album and
    its OCDC Track were inspected with the Track's local route temporarily marked
    unavailable in the disposable copy. Explicit MusicBrainz inspection showed
    the real OC/DC Recording. The spelling gap remains conservative; manual
    Recording selection succeeded and closed review. Coverage became 839/942 in
    the copy. The full Album program was inspectable; the matcher was not relaxed
    to bulk-confirm that unsupported spelling. No attempt was made to fix all gaps.
19. **Real Local:** profile baseline 918/942 usable, 24 missing. Album and Track
    review used Three Trapped Tigers / Numbers 1–13 / 8 and its actual FLAC file.
    Only in the disposable copy, its availability observation was temporarily
    disabled. Explicit preview/Connect restored the source through ingestion in
    both scopes, closed review, and created no duplicate entities or memberships.
    The original observation was restored between/after checks. Original databases
    and audio files were never modified.
20. **Tests:** focused presentation tests cover ranking, offline navigation,
    zero/one candidate, failure retention, partial confirmation, unavailable scope,
    and stale preview isolation. QML checks cover direct entry, common lifecycle,
    errors, provider manual tools, legacy Spotify advanced mapping, and immediate
    list updates. The 200k fixture traverses forward/reverse pages, drags the
    scrollbar, expands multiple Albums, and marks an Album from deep in the list:
    surrounding position/expansion survives and the model stays ≤300 Albums.
    Atomic MusicBrainz rollback and source-free Library projection have storage
    regression coverage. The adapter cache test proves evicted discovery programs
    can be inspected from retained reconciliation evidence without HTTP or TTL
    refresh. Exact final test totals are recorded below.
21. **Deliberately deferred:** new Local candidate discovery, MusicBrainz fuzzy
    matching/edition correction, and fixing existing canonical spelling/program
    gaps. Real multi-candidate Spotify navigation and valid Album confirmation
    were tested on different Albums because Animals' remaining Tracks cannot
    honestly be accepted by the current program mapping. No matcher redesign,
    automation, new schema migration, or credentials system was introduced.

Opt-in real checks require a disposable `/tmp` database and
`MUSIC_LIBRARY_DIAGNOSTIC_DATABASE`, with `QT_QPA_PLATFORM=offscreen`:

* `shared_connection_review_real_spotify_candidates_and_track`
* `shared_connection_review_real_spotify_album_connect`
* `shared_connection_review_real_musicbrainz_manual` (explicit network actions)
* `shared_connection_review_real_local_album_and_track`
* `connections_200k_bounded_continuous_scrolling`

Run these individually with `-- --ignored --nocapture`. They intentionally change
the disposable database. Qt integration checks run in separate processes because
Qt application/thread ownership is process-global.

Final results:

* Full core suite: **497 passed**, 18 opt-in tests ignored, zero failures.
* Full Spotify adapter suite: **82 passed**, four live/opt-in tests ignored,
  zero failures. Mock HTTP tests require local server access outside the sandbox.
* Shared presentation module: **5 passed**. The stale-result test covers both
  obsolete file previews and Spotify replies after switching review targets.
* Five isolated regular QML regressions passed: shared ranked review/lifecycle
  (including stale-confirmation error retention), Music/Connections navigation,
  existing Album review/advanced mapping, existing Spotify list updates/context,
  and marked-Track manual search.
* Five isolated opt-in QML checks passed: real Spotify candidate navigation/Track
  Connect, real Spotify Album Connect, real MusicBrainz manual association, real
  Local Album/Track ingestion, and deep 200k-list review/context/scrollbar behavior.
* Rendered Album and Track comparisons were inspected. Formatting checks for core,
  Spotify adapter, and QML bridge plus `git diff --check` passed.

Validation logs are in `/tmp/review-*.log` and
`/tmp/connection-review-mb-ui.log`; representative renders are
`/tmp/connection-review-album.png` and `/tmp/connection-review-track.png`.
