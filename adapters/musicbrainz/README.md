# Explicit MusicBrainz catalog adapter

This isolated package implements the application-owned `CatalogProvider` boundary
with blocking `ureq` HTTP on the diagnostic's catalog worker. Private serde types
are converted before leaving the adapter; the backend has no HTTP dependency.

The [MusicBrainz v2 API](https://musicbrainz.org/doc/MusicBrainz_API) supplies JSON:
search `release-group` with an escaped human query; browse `release` by
`release-group` with `status=official&inc=media` for Add Album, or
`artist-credits+labels+media` for explicit Editions; then look up the
selected Release with `release-groups+recordings+artist-credits+isrcs+media`.
Human queries use the [Release Group search syntax](https://musicbrainz.org/doc/MusicBrainz_API/Search#Release_Group).
Album search uses 10-item pages; edition browse requests up to 100 candidates with
explicit offsets based on the actual returned count.
Browse avoids the limited embedded Release list on a Release Group lookup.

A process-wide request gate enforces at least one second between requests across
client instances. The User-Agent is `music-library/<package-version>
(https://github.com/jbeardwo/music-library)`, following MusicBrainz's
[identification and rate rules](https://musicbrainz.org/doc/MusicBrainz_API/Rate_Limiting).
Other applications sharing an IP can still cause throttling. HTTP errors including
429/503 are recoverable; redirects remain disabled. Each HTTP attempt
has a 30-second timeout and an 8 MiB response limit. Nothing runs on startup,
local search, scanning, or playback. There is no persistent catalog cache.

Release lookup validates required IDs and track counts, orders media and tracks
by MusicBrainz position, and retains distinct Track/Recording IDs and every ISRC.
The [MusicBrainz medium serializer](https://github.com/metabrainz/musicbrainz-server/blob/master/lib/MusicBrainz/Server/WebService/Serializer/JSON/2/Medium.pm)
returns pregap and data tracks separately from regular tracks; conversion includes
both rather than dropping them. Credited names and join phrases are preserved, preferring Track credit, then
Recording credit, then Release credit. Optional discovery fields may be absent.
Titles are not normalized. Edition details are discovery data; the current durable
metadata model stores title, year, ordered credits and numeric disc/track positions,
not complete dates, barcodes, label information or printed track-number strings.
Release Group identity belongs to Album; Release identity belongs to its edition.
Album search metadata supplies the friendly title/credits/year independently of
the representative edition.
No artist identity reconciliation or provider metadata refresh is attempted.

## Checks and optional live probe

Normal tests use fixture JSON and a local HTTP server, never MusicBrainz:

```sh
cargo fmt --manifest-path adapters/musicbrainz/Cargo.toml --all -- --check
cargo test --manifest-path adapters/musicbrainz/Cargo.toml
cargo clippy --manifest-path adapters/musicbrainz/Cargo.toml --all-targets --all-features -- -D warnings
```

The tests cover escaped URLs/includes, identification, request spacing, conversion,
optional fields, ordering, identities, source-less import, and recoverable network,
HTTP and malformed/incomplete-response failures. The opt-in probe does not import:

```sh
cargo run --manifest-path adapters/musicbrainz/Cargo.toml --example probe -- 'releasegroup:Nevermind AND artist:Nirvana'
cargo run --manifest-path adapters/musicbrainz/Cargo.toml --example probe -- --release 006cb56d-6eff-4b7d-853f-ecd2db97f3b2
```

Both were exercised successfully during implementation: search, a page of 25
editions, and the selected Release's one medium and 12 Tracks. Public responses
can change; this observation is not a test fixture. See the
[diagnostic UI guide](../../tools/qml-diagnostic/README.md) for interactive import.


## Representative edition and request budget

`CatalogSession` in the application layer preserves the existing provider boundary.
Search does not browse editions. Add Album ranks the first edition page, excluding
entries without usable media/track counts; it prefers Official status, original-era
year, no obvious expanded-edition wording, fewer discs, then stable date/ID ties.
This is provisional selection within a bounded set, not a canonical edition claim.
An advanced Editions action requests rich, unfiltered summaries without choosing
for the user. Normal Add requests only Official media summaries and falls back to
one unfiltered media-only request if none exist. Errors do not trigger fallback.

A cold normal search-and-add needs three requests: search, browse, lookup. Up to
100 edition summaries fit in one requested page; MusicBrainz may return fewer under
its [paging limits](https://musicbrainz.org/doc/MusicBrainz_API#Paging). Albums with
about 20 ordinary editions can therefore fit in one browse call. Selection does
not crawl every page; explicit browsing can reach further editions. A session
caches four rich pages, one default candidate page and one detail. A complete
all-Official rich page can serve Add; lightweight candidates cannot serve rich
Editions. Re-add reuses candidates/detail. Rate limiting is unchanged; request
counts here exclude transient retries and the no-Official fallback. See the
[request-shape comparison](../../docs/catalog-latency-audit.md#request-shape-follow-up)
for measured payload reductions and remaining HTTP latency outliers.


The read-only `--album` probe exercises normal Add without preloading Editions
(use `--editions` for a rich page instead):

```sh
cargo run --manifest-path adapters/musicbrainz/Cargo.toml --example probe -- --album 'releasegroup:"Demon Days" AND artist:Gorillaz'
```

Before the request-shape change, the probe returned 31 unfiltered editions; the
Official candidate page now returns 26. Both selected the Official 2005
single-CD Release `14190090-8b00-4c4a-a861-50d7734c16fd`, with 15 Tracks.
This probe selects the first Album-type search match explicitly; a same-name
promotional Release Group also appeared and was initially mistaken for the Album
by the old probe's first-result shortcut. The UI does not select an Album for the
user. One live request timed out and surfaced an ordinary error; a subsequent
manual retry succeeded. These observations are not automated test expectations.


## Bounded 503 retry

MusicBrainz uses [503 for both throttling and service overload](https://musicbrainz.org/doc/MusicBrainz_API/Rate_Limiting).
The adapter makes at most three attempts for a logical request, retrying only 503.
It uses 2s then 4s backoff unless a valid
[Retry-After](https://www.rfc-editor.org/rfc/rfc9110.html#section-10.2.3) supplies delay-seconds
or an HTTP date (`httpdate` parses the latter). Invalid headers use the fallback;
past dates still pass through the one-second rate gate. If the requested delay
exceeds one minute, automatic retry stops with the recoverable 503 error rather
than shortening the server's delay. Ordinary 4xx, other 5xx, transport failures,
and malformed successful responses are not retried.

All attempts and retry waits hold the existing process-wide request gate, so other
clients cannot bypass the cooldown. The 30-second timeout remains per HTTP attempt;
the overall operation can take longer through bounded attempts/waits. These waits
stay on the existing catalog worker. Only its final success/error is delivered to
Qt, leaving the operation visibly pending throughout retries. Closing during a
request can therefore wait longer for the owned worker to finish.

Local HTTP tests exercise eventual success, exhausted retries, unchanged requests,
and spacing across attempts and client instances. Pure fixed-time tests cover
Retry-After seconds/dates and delay limits without sleeping. No live-service test
is needed to induce throttling.


The [final search/detail audit](../../docs/catalog-endpoint-audit.md) reduced only
search pages from 25 to 10; the existing next-page action remains. Detailed lookup
still includes eager ISRCs and Release Group association verification. Minimal
lookup variants did not demonstrate a consistent improvement worth dropping those
fields. Smaller payloads do not guarantee low public-service latency.
