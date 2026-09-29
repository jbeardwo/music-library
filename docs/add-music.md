# Add Music / catalog search

The main Add Music button opens a temporary panel for finding external music and
saving it. Search library remains a separate local-only operation.

Typing waits 300 ms, then searches Artists, Albums and Songs. All groups results;
the other filters request only their category. Enter submits immediately, Down
focuses the result list, and Enter opens the selected result. Escape closes the
panel. Search and Back remain usable during network work.

- Artist opens a bounded Album page with Next Albums. Browsing never imports.
- Album opens artist/date context and its ordered multi-disc track list. Add Album
  saves missing Tracks; each row also has Add Song.
- Song opens its Album context and the matching occurrence. When a recording
  appears more than once in that program, separate normal Song rows let the user
  choose the position. No other Track becomes a library member.
- Successful adds refresh the main library and keep Add Music open. They never
  invoke playback or change the queue.

Ordinary rows contain human labels and membership, without provider IDs or edition
terminology. The old catalog diagnostic methods and edition dialog remain for
existing diagnostic tests; the main Add Music control opens the new panel.

## Catalog boundary

`CatalogProvider` adds `catalog_albums`, `catalog_songs`, `browse_artist` and
`catalog_album`. These discovery methods are separate from the existing matching
methods. `SongCandidate` retains a provider identity plus an explicit Album context;
MusicBrainz Songs also retain a reference for loading that context. It is not a
new local identity.

MusicBrainz uses release-group search, recording search and Artist-ID-scoped
release-group search. Recording search uses Album context already present in the
response, with at most two different Albums per recording; it performs no Song
lookup per row. Album preview uses the existing representative selection helper,
then the existing complete Release parser. Spotify uses catalog search, Artist
Albums and the existing bounded Album program loader. Its Album object supplies a
catalog reference, without claiming MusicBrainz-style exact-edition certainty.

The new QML adapter routes detail requests back to the result's provider. Available
configured providers search independently; one failure does not discard another
provider's results. Legacy Spotify-only mode also allows MusicBrainz discovery as
fallback. Missing Spotify credentials do not disable MusicBrainz. Existing
matching, provenance, provider circuits and enrichment rules are unchanged.

## Saving and local awareness

`Library::add_catalog_selection` extends the existing atomic catalog importer.
It creates or reuses the complete known Album/Release/Track program, but grants
membership only at the supplied disc/Track positions. Other Tracks remain known,
unsaved entities. Empty, missing or ambiguous selections roll back. Repeating an
add preserves IDs, user overrides and membership; adding the Album can then save
its remaining known Tracks. Selecting positions uses one indexed program read.

Before adding, the panel rereads membership and saves only missing positions.
It reuses an unambiguous established catalog reference when previewing that Album
again, including after restart. This keeps a Song-context add and a later Album
add on the same known representation. It does not pick among conflicting local
references or establish equivalence from similar titles.

`catalog_context` uses at most four local set queries for a bounded result set.
Strong, unambiguous local Artist/Album/Track associations permit consolidation;
names alone never do. Recording-based Song results require a unique local Track
within the established Album. Ambiguous occurrences remain separate. Detail
membership uses the established reference and Album-scoped Track identities.

Search Albums show a saved count. After a preview supplies a known total, the
panel can show `5 of 13 Tracks in library` or `In library`. Detail always compares
against its displayed program. Eight recent program sizes are retained, without
fetching metadata solely to determine membership.

## Bounds and responsiveness

There is one background worker per provider, with one active request and one
replaceable pending request. Generation checks run between category calls and
before applying replies. Typing, changing filters, Back and dismissal invalidate
old replies immediately. An in-flight HTTP call finishes under the adapter's
existing timeout/retry policy; it is not forcibly interrupted. Results publish
progressively by provider/category. An unavailable provider stops its remaining
category calls for that request.

Normal request shapes, excluding retries:

| Action | MusicBrainz | Spotify |
| --- | --- | --- |
| All search | 3 requests | 3, or 4 if existing Artist-query fallback applies |
| Type-filtered search | 1 | 1, or 2 for Artist-query fallback |
| Artist page | 1, ten Albums | 1, ten Albums |
| Album preview | 2; 3 if no Official candidate exists | 1–20 program pages, at most 1,000 Tracks |
| Song preview | 1 referenced track-list lookup | Same Album program loader |
| Established-reference preview | 1 | 1 metadata lookup plus program pages if uncached |
| Save / membership check | 0 | 0 |

Spotify may additionally need one Client Credentials token request. Existing
post-import enrichment can make its usual separate requests when enabled; it is
not part of the Save transaction. Search never crawls discographies or requests
one resource per displayed Song. Each provider/category is capped at twenty UI
rows (current adapters normally return ten; MusicBrainz may return twenty Song
contexts). QML therefore holds at most 120 search rows with both providers.
Artist pages replace the previous page; detail programs over 1,000 Tracks are
rejected. Network calls never run in database transactions or on the Qt thread.

Ranking is stable within groups: normalized exact title/name, prefix or phrase,
all title tokens, then provider order. There is no new fuzzy matching, transliteration,
embedding retrieval or cross-provider identity inference.

## Validation and limitations

Core SQLite tests cover selective import, completing partial membership, repeat
adds, rollback, metadata preservation, local awareness and reference reuse after
reopening. Catalog adapter fixtures verify search/browse/detail request shapes,
no per-Song lookup and Spotify selective import. QtTest exercises the shipped
panel with delayed and unavailable providers: grouping, filters, stale results,
Artist browsing, Back, Album/Song previews, both add actions, membership refresh,
Escape, main-library updates and unchanged playback. A worker test checks queue
coalescing; consolidation tests distinguish established IDs from identical names.

The live probe exercised tricot, T H E, Hatsumimi, toe, For Long Tomorrow, Gorillaz,
Demon Days, Feel Good Inc, Hop Along and Painted Shut. MusicBrainz returned useful
Artist/Album results for the named Artists and Albums. It returned no result for
romanized Hatsumimi; its first Feel Good Inc page favored covers and an a cappella
Gorillaz recording. Those are observed retrieval limitations, not matching changes.
Spotify credentials were unavailable in this environment, so its added endpoints
were checked with HTTP fixtures, not a live account.

Reproduce live catalog checks (read-only, no import):

```sh
cargo run --offline --manifest-path tools/qml-diagnostic/Cargo.toml --example catalog_search_probe
```

Deferred: transliteration and popularity-aware retrieval; broad search pagination;
Artist-level Song pages; reconciling unresolved/ambiguous existing identity claims;
new matching rules; artwork and multi-select. Artist Album pagination and individual
Song additions are implemented. Unknown cross-provider equivalents deliberately
remain separate.

Endpoint references: [MusicBrainz search](https://musicbrainz.org/doc/MusicBrainz_API/Search),
[Spotify search](https://developer.spotify.com/documentation/web-api/reference/search),
[Spotify Artist Albums](https://developer.spotify.com/documentation/web-api/reference/get-an-artists-albums).
