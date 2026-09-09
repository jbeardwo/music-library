# Final MusicBrainz search/detail audit (2026-09-09)

The only production change is **Album search limit 25 → 10**. Existing next-page
requests remain available. Candidate browse, representative ranking, detailed
lookup, eager ISRC import, rate/retry rules and all persistence are unchanged.

## Method

The ignored `live_search_detail_shapes` probe uses the production HTTP client,
User-Agent, 30-second timeout, bounded 503 retries and one-second process-wide
gate. One live client runs sequentially: three rounds with rotating limits/variants
and reversed case order in alternate rounds. No application cache, Qt, import or
SQLite work is inside these measurements. JSON bodies are fully consumed and saved
for field comparison. Builds are excluded; server/network caches are uncontrolled.

57 logical requests made 95 HTTP attempts: 57 HTTP 200 and 38 HTTP 503. Every
logical request eventually succeeded; none exhausted retries or timed out in this
batch. Search accounted for 25 of the 503s, lookup for 13. Every Retry-After was
zero, so retries still waited for the unchanged rate gate. Reported HTTP time sums
all attempts, excluding gate waits; total wall time includes initial gate wait,
retry cooldown and parsing. This prevents presenting retry time as JSON cost.
Bytes are decoded response bytes, not compressed wire bytes.

## Search limits and discovery

The measured queries were:

* `releasegroup:"Demon Days" AND artist:Gorillaz`
* `releasegroup:Animals AND artist:"Pink Floyd"`
* `Animals` (ambiguous; 1,700 reported matches)

These use the existing [Release Group search syntax](https://musicbrainz.org/doc/MusicBrainz_API/Search#Release_Group).
The adapter passes the user's query unchanged; this change does not introduce
query rewriting, a popularity ranking or a provider-side Album-type filter.

| Query | Limit | Returned / total | Bytes | HTTP median s | Total wall s, three rounds | 503 attempts |
|---|---:|---:|---:|---:|---|---:|
| demon-days | 5 | 5 / 6 | 12,889 | 0.932 | 1.559 / 0.191 / 1.865 | 2 |
| demon-days | 10 | 6 / 6 | 14,942 | 1.108 | 2.114 / 2.095 / 3.925 | 3 |
| demon-days | 25 | 6 / 6 | 14,942 | 1.064 | 1.980 / 2.239 / 1.891 | 3 |
| animals | 5 | 5 / 9 | 23,760 | 0.839 | 2.064 / 1.608 / 2.505 | 2 |
| animals | 10 | 9 / 9 | 31,616 | 1.009 | 1.891 / 2.228 / 3.641 | 3 |
| animals | 25 | 9 / 9 | 31,616 | 0.785 | 2.053 / 3.419 / 0.180 | 2 |
| ambiguous-animals | 5 | 5 / 1700 | 4,286 | 0.972 | 2.223 / 3.361 / 1.704 | 4 |
| ambiguous-animals | 10 | 10 / 1700 | 9,025 | 0.742 | 1.769 / 1.986 / 3.592 | 4 |
| ambiguous-animals | 25 | 25 / 1700 | 19,944 | 0.834 | 1.908 / 2.011 / 2.925 | 2 |

For all rounds and limits, the intended Demon Days Album was result **2** (a
same-name promotional single was first), and Pink Floyd's Animals was result **1**
in the artist-qualified search. Limit 10 retained all six/nine matches from these
queries; limit 5 omitted one/four secondary matches. No target-discovery loss was
observed for these qualified queries.

For unqualified `Animals`, Pink Floyd was absent from the first 5, 10 **and 25** in
every round. There are many tied title matches and the order also varies between
limits; a 10-result page is not guaranteed to be an exact prefix of the 25-result
page. Ten results show fewer alternatives, but the broad-query relevance problem
already exists at 25. Artist refinement or the existing **Next Album page** action
remains necessary; this is not a claim that ten results cover every user's intent.

Limit 10 reduces the ambiguous-query payload from 19,944 to 9,025 bytes (54.7%)
and bounds initial display/conversion to ten results, while preserving the full
qualified result sets. **A consistent HTTP latency improvement was not demonstrated.**
No server-side CPU/database work was measured, so smaller payloads must not be
reported as a proven reduction in MusicBrainz's internal query cost. Ten is a
small discovery/payload improvement, not a fix for service latency. Existing
actual-returned-count pagination is unchanged and has a ten-plus-one mock test.

## Detailed lookup fields and variants

Current includes remain:

`release-groups+recordings+artist-credits+isrcs+media`

| Import requirement | Response/application input |
|---|---|
| Album association | Returned Release Group MBID, checked against selected Album identity |
| Friendly Album metadata | Search candidate replaces lookup's group title/date/credits in `CatalogSession` |
| Concrete Release | Release MBID, title/date and ordered artist credit |
| Ordered release-specific Tracks | Medium positions/counts, Track positions/titles, separate Track MBIDs |
| Recording identity | Each Track's Recording MBID; no application Recording entity |
| Artist credits | Track-specific credit, Recording credit fallback, then Release credit; credited names/join phrases retained |
| ISRC external identities | Recording `isrcs` arrays (valuable enrichment, not the core identity) |
| Membership | Application Add action over imported Track IDs; no extra HTTP field/request |

Variants used the same selected Releases: Animals
`0a567b61-f09d-4549-9d43-1c3e81c21b26` (5 Tracks) and Demon Days
`14190090-8b00-4c4a-a861-50d7734c16fd` (15 Tracks).

* `no-isrc`: omit only `isrcs`.
* `no-group`: omit only `release-groups`.
* `no-media`: omit only `media`.
* `minimal`: only `recordings+artist-credits`.

| Album | Variant | Bytes | HTTP s, three rounds | Total wall median s | 503 attempts |
|---|---|---:|---|---:|---:|
| animals | current | 6,025 | 0.685 / 0.699 / 0.180 | 1.524 | 2 |
| animals | no-isrc | 5,720 | 0.956 / 1.384 / 0.827 | 1.860 | 1 |
| animals | no-group | 5,511 | 0.963 / 0.222 / 1.131 | 1.681 | 2 |
| animals | no-media | 6,025 | 0.256 / 2.883 / 0.716 | 2.010 | 1 |
| animals | minimal | 5,206 | 0.220 / 2.263 / 0.685 | 1.799 | 3 |
| demon-days | current | 18,314 | 0.173 / 0.173 / 0.672 | 0.954 | 1 |
| demon-days | no-isrc | 17,938 | 3.060 / 0.456 / 0.974 | 1.476 | 0 |
| demon-days | no-group | 17,791 | 2.646 / 0.275 / 0.250 | 0.818 | 0 |
| demon-days | no-media | 18,314 | 0.231 / 0.215 / 0.669 | 1.025 | 1 |
| demon-days | minimal | 17,415 | 1.281 / 0.218 / 3.363 | 3.356 | 2 |

All variants preserved the Release ID/title/date, ordered Tracks, Track and Recording
MBIDs and artist credits. Removing `isrcs` removed 17 ISRC associations from Animals
and 15 from Demon Days, saving just **305 bytes (5.1%) / 376 bytes (2.1%)**.
No repeatable latency or reliability benefit justified losing eager enrichment:
no-ISRC HTTP medians were 0.956s / 0.974s versus current 0.685s / 0.173s. These
small samples do not establish that ISRC expansion is intrinsically faster or
slower; they establish no useful measured case for deferring it. **Keep eager ISRCs.**

Removing `release-groups` saves **514 / 523 bytes**. Friendly group metadata is
redundant in the selected-Album flow, but the returned group ID is not: it verifies
that the fetched edition belongs to the Album. The standalone Release lookup also
returns a self-contained application Release. Dropping the include would require
changing that boundary/check for a small payload saving without a consistent
cross-case improvement. It remains included.

`recordings` already produced complete media in these lookups: `no-media` was
semantically identical JSON at exactly the same response size as current in all
rounds. Removing that redundant spelling offers no measured payload benefit.
The [MusicBrainz lookup include documentation](https://musicbrainz.org/doc/MusicBrainz_API#Lookups)
is the authority for these expansions. No lookup parameters or required identity/
credit validation were changed based on incidental per-URL timing differences.

## Normal latency, retries and upstream stalls

Successful HTTP-200 attempts in this batch took:

* search: 0.180–2.698s, median 0.673s;
* lookup: 0.173–3.363s, median 0.505s.

Body reading and JSON parsing each stayed below 1ms; the delays were predominantly
awaiting response headers. 23/27 search operations and 11/30 lookup operations
retried. The extra rate/retry gate time is separate from successful-response time.
This batch had **no 10–30-second successful outlier**, even though the previous
[audit](catalog-latency-audit.md#fresh-application-cache-qml-search-and-add) recorded
17-second lookups with 6KB/18KB bodies and a 27-second search. Earlier minimal
candidate responses also stalled despite tiny payloads. Do not keep probing until
an outlier appears or treat this faster interval as proof those stalls are fixed.

For application diagnosis, these are **upstream HTTP/public-service latency**,
not persistence, FTS, ranking or per-Track work. Client timings cannot distinguish
a MusicBrainz internal queue/cache/load delay from a transport-path stall. The
remaining dominant costs are remote response variability and the required rate/
retry waits; no unrelated application optimization is justified by these results.

## Later persistent-cache recommendation

A small bounded persistent cache would be worthwhile for repeated successful
searches and already-fetched Release details across application sessions. Search
keys should preserve the exact query, limit and offset; detail keys need the
Release MBID plus response/conversion version. Expiry/explicit refresh is needed
because provider data can change. Do not persist transient 503/network failures as
catalog results, or treat cached provider observations as user metadata overrides.

Cache hits could avoid both network uncertainty and rate waits. **A cache cannot
speed the first-ever uncached Album lookup.** It also does not solve remote service
availability or imply background refresh. No persistent cache/enrichment system
was implemented; the existing session cache remains.

## Reproduction and validation

```sh
MUSIC_LIBRARY_CATALOG_TIMING=1 MUSIC_LIBRARY_ENDPOINT_OUTPUT=/tmp/mb-endpoints \
  cargo test --manifest-path adapters/musicbrainz/Cargo.toml \
  live_search_detail_shapes -- --ignored --nocapture
```

The probe is excluded from normal tests. Leave at least one second between separate
live probe processes. The checked-in deterministic search test verifies limit 10,
unchanged query text, offset 10, no repeated identities and the final page. Existing
full-lookup/ISRC/artist-credit, candidate-browse, storage, playback and QML behavior
remain covered by the full validation suite. No database, ranking or optimized
candidate-browse changes were made.
