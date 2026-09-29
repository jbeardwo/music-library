# Global local-library search

The upper-right Search control opens a temporary in-application panel. Typing
updates grouped Artists, Albums and Songs without submitting. Filters select All,
Artists, Albums, Songs or the deferred Playlists category. Escape or an outside
click dismisses the panel. Up/Down and Enter operate the result list.

Search always covers saved library Tracks. Current browser selections never enter
the request. Partial Albums, unavailable files, Spotify-associated and source-less
Tracks participate equally. Search performs no filesystem availability checks,
provider calls, matching, playback operations or identity writes.

## Retrieval and ranking

Library::local_search(text, Kind) and the read-only local_search_reader use
SQLite exclusively. Migration 16 adds external-content FTS5 indexes over canonical
Artist names and Album application titles, with insert/update/delete triggers and
an initial rebuild. Songs reuse the existing effective-metadata Track FTS index.

Up to twelve alphanumeric query tokens are safely quoted and ANDed as token
prefixes. The existing unicode61 tokenizer handles case, diacritics and punctuation.
There is no SQL/FTS operator syntax, edit-distance scan, fuzzy index or network fallback.

Within each section the order is:

1. Exact name/title (SQLite lowercase comparison).
2. Name/title beginning with the full typed phrase.
3. All query tokens matching words or word prefixes in the object's name/title.
4. Saved objects related through matching Artist or Album identities.

Ties use lowercase title and stable object ID. Unicode/punctuation variants still
match through FTS even if they do not receive the exact-phrase boost. Direct Song
matches always precede contextual Songs.

The top sixteen matched Artists expand through existing Album, Release and Track
credits to their saved Tracks and Albums. The top sixteen directly matched Albums
expand to their saved Tracks. SQL unions prevent duplicate results; unrelated
objects do not expand. Artist identities are never grouped by name or modified.

Presentation uses existing credited names and effective Track artist text. For
navigation, prefer the first established Album credit; when absent, use a saved
Release credit, then a saved Track credit. Multiple saved Track occurrences retain
their distinct Track IDs and established Album contexts. Missing credits leave the
Artist unselected rather than inventing an identity.

## Bounds and responsiveness

Each section returns at most forty results: **120 search rows maximum in QML**.
Broad queries show the best bounded subset; refine the query or navigate to an
Artist/Album to browse the complete library program. Playlist selection returns an
empty deferred state. No playlist storage or behavior was added.

The panel debounces typing by 150 ms. Database work runs off the Qt thread. There
is one active read and at most one pending request, replaced by the latest input.
Generation checks discard obsolete results and results arriving after dismissal.
An already running SQLite read finishes; cancellation does not interrupt it.
Presentation context is fetched in one bounded set query, not one query per result.
Explicit join order keeps contextual expansion anchored on indexed Artist matches.

Warm measurements on the deterministic 200k fixture, debug build, median of three
runs after warm-up:

| Query / purpose | Time | Returned rows |
| --- | ---: | ---: |
| Artist 0001 — exact Artist plus context | 21 ms | 81 |
| Artist 00 — Artist prefix | 102 ms | 120 |
| Release 00001 — Album | 12 ms | 11 |
| Song 000001 Track — Song | 32 ms | 1 |
| 000001 Track — multiple words | 16 ms | 1 |
| Artist 000 — contextual Artist expansion | 62 ms | 89 |
| a — broad query | 137 ms | 120 |

These are observations on this machine, not universal budgets. Reproduce with:

~~~sh
cargo run --offline --example library_search_performance
# Optional explicit database and queries:
cargo run --offline --example library_search_performance -- /tmp/library-copy.sqlite hop painted waitress
~~~

## Navigation

Result clicks close Search without changing playback or the queue:

- Artist: select that Artist, clear Album and Song.
- Album: select its established Artist and Album, clear Song.
- Song: select its established Artist, Album and exact Track ID.

The browse_from API seeks directly to an identity's ordering key. The browse_around
API also fetches at most 100 predecessors, then a bounded forward window. This preserves
complete small Albums and places distant targets inside a normal 200-row pane
without walking pages or loading the full result. Next continues with the usual
cursor. Previous from the initial anchored window returns to the beginning of
that scope. Selection is highlighted, scrolled into view and keyboard-focused.
If membership changed and the target is gone, the panel reports an error and
leaves the browser unchanged. On the same fixture, distant Artist/Album/Song
windows took approximately 23/31/2 ms respectively through the existing synchronous
browse boundary; interactive search queries themselves remain off the UI thread.

## Validation and deferred work

SQLite tests cover exact/prefix/case/multi-word/punctuation queries, contextual
expansion, direct-before-context ranking, filters, membership, missing sources,
effective metadata changes, index maintenance, duplicate titles, bounded broad
queries and direct seeks past 200 rows. QtTest exercises the shipped panel,
result clicks, selection/focus, Escape, rapid query replacement and unchanged
playback; provider workers remain unstarted.

The desktop audit on a disposable real-library copy exercised hop, hop along,
painted, waitress, hella, bygones and toe. The hop query returned Hop Along, all six
saved Albums and a bounded Song subset, alongside legitimate direct matches.
Clicks on Hop Along, Painted Shut and Waitress landed in the expected browser
states. Screenshots checked grouping and selected navigation targets.

~~~sh
MUSIC_LIBRARY_UI_AUDIT_DATABASE=/tmp/library-copy.sqlite MUSIC_LIBRARY_SEARCH_AUDIT=1 cargo test --offline --manifest-path tools/qml-diagnostic/Cargo.toml --all-features real_library_desktop_audit -- --ignored --nocapture
~~~

Search creates **zero provider HTTP requests**. The audit uses local database data
and no provider workers. Existing Qt test teardown/EGL diagnostics are unrelated
to search; no rendering overrides were added.

Deferred: playlists, typo correction, locale-specific ranking/collation, search
continuation pages, artwork and matching changes. [Add Music/catalog search](add-music.md)
is now a separate surface for finding and saving external music.
