# Provider-neutral edition evidence audit

Audited 2026-09-11. Application Artist, Album, Release, Track and Recording IDs are
authoritative. This document and the read-only probe define an evidence boundary,
not permission to attach exact Release identities or merge Releases.

## Provider compatibility

“Available” below means the provider can expose evidence, not that every object
contains it. Missing fields must remain absent. Object names are not ontology.

| Evidence | MusicBrainz | Spotify | Apple Music | Discogs |
| --- | --- | --- | --- | --- |
| Artist identity | Artist MBID; ordered credits/join phrases | Artist IDs and names | Artist relationships and display name | Artist IDs, name variations and credits |
| Optional Album grouping | Release Group | No corresponding grouping resource in reviewed API | No corresponding grouping resource in reviewed API | Master, when present; grouping rules differ from Release Group |
| Concrete candidate identity | Release MBID | Catalog Album ID | Catalog Album ID, storefront context | Release ID |
| Title / version text | Title, disambiguation | Name; qualifiers may be embedded | Name; qualifiers may be embedded | Title, format descriptions/notes |
| Date / precision | Partial release date; release events | `release_date` and explicit year/month/day precision | `releaseDate`; retain supplied precision | Released date/year, potentially incomplete |
| Barcode | Barcode | UPC/EAN in full Album external IDs | Optional UPC | Typed identifiers; printed/scanned variants |
| Label / catalog number | Label-info pairs | Label; no catalog-number field reviewed | Optional recordLabel; no catalog-number field reviewed | Label/catalog-number pairs |
| Country | Release country/events | Availability markets are **not** edition country | Storefront is **not** manufacturing/release country | Release country |
| Media / discs / count | Ordered media, format, positions/counts | Track disc numbers, total tracks; not physical format | Song disc numbers, trackCount; not physical format | Format quantities/descriptions; side/index notation |
| Track occurrence identity | Release Track MBID | Track ID with Album association | Song ID with Album relationships | No independent stable track ID verified; do not fabricate one from position |
| Ordered track evidence | Medium/track position, title, credit, length | Disc/track number, name, Artists, duration | Disc/track number, name, artistName/relationships, duration | Tracklist position/title/Artists/duration; headings and subtracks require translation |
| Underlying Recording | Recording MBID plus optional ISRCs | ISRC from full Track; no separate Recording resource reviewed | Optional ISRC; no separate Recording resource reviewed | No comparable Recording resource or dependable per-track ISRC contract verified |

MusicBrainz's [API includes and browse rules](https://musicbrainz.org/doc/MusicBrainz_API)
provide rich linked edition/recording evidence. Release Group discovery is an
adapter strategy, not a prerequisite for generic acceptance.

Spotify's [Album response](https://developer.spotify.com/documentation/web-api/reference/get-an-album)
contains paginated simplified Tracks; its [full Track response](https://developer.spotify.com/documentation/web-api/reference/get-track)
contains ISRC and Album association. Album pages alone therefore do not guarantee
all recording evidence. Market availability, relinking and catalog changes must be
preserved as provenance in a future adapter. A Track ID is not assumed to identify
one underlying Recording globally, nor is an Album ID a Release Group equivalent.

Apple's [Album attributes](https://developer.apple.com/documentation/applemusicapi/albums/attributes-data.dictionary)
and [Song attributes](https://developer.apple.com/documentation/applemusicapi/songs/attributes-data.dictionary)
separate catalog names/positions from ISRC. Its [ISRC lookup](https://developer.apple.com/documentation/applemusicapi/get-multiple-catalog-songs-by-isrc)
explicitly permits multiple Songs for one ISRC. Relationships and storefront context
must be retained; localized display names are not identity. The public documentation's
JSON representation was inspected because its HTML shell omitted attribute details.

Discogs' [Master rules](https://support.discogs.com/hc/en-us/articles/360005055493-Database-Guidelines-16-Master-Release),
[tracklisting rules](https://support.discogs.com/hc/en-us/articles/360005055373-Database-Guidelines-12-Tracklisting)
and [identifier rules](https://support.discogs.com/hc/en-us/articles/360005054893-Database-Guidelines-5-Barcodes-Identifiers)
describe grouping, positions and physical-edition evidence. The current developer
page was blocked by its access challenge during this audit. Field availability was
cross-checked against the [archived official client model](https://github.com/discogs/discogs_client/blob/master/discogs_client/models.py);
this is not current API-contract verification. Recheck actual payloads before a
Discogs adapter. Do not assume its release-level identifiers assign ISRCs to tracks.

**Conclusion:** one comparison engine can consume all four sources. It must accept
optional groupings, unknown positions and incomplete evidence. Richer MusicBrainz
Recording links or Discogs pressing details can corroborate without becoming
mandatory. A shared ISRC is evidence, not an application ID or proof of edition.

## Current boundary audit

| Location | Current assumption | Assessment |
| --- | --- | --- |
| `domain.rs`, generic external-identity tables/APIs | Opaque IDs; many-to-many external mappings | Correct, provider-neutral; no schema change needed |
| `filesystem.rs`, local `import_release`, local Album corroboration | Local metadata/IDs only | Correct; no provider dependency in import |
| `adapters/musicbrainz` | MBID validation, `arid`/`rgid`, Release Group browse, includes, JSON, rate/retry | Legitimate adapter concerns |
| `catalog.rs` `CatalogProvider` | Required Album-group → Releases methods; `Release.album.identity` mandatory | Ontology leak: a concrete-only provider cannot use the existing import contract without inventing a group |
| `catalog.rs` representative ranking/session | Official status, primary/secondary types, group-first Add Album workflow | Useful initial policy shaped by MusicBrainz, not universal edition acceptance |
| `album_matching.rs` Artist/Album acceptance | Requires `musicbrainz/artist` and `musicbrainz/release_group`; scoped discovery/alias rules | Provider-aware application policy currently mixed into the matcher; future capability extraction needed |
| `storage.rs` prepare/complete Album match and known Artist lookup | Tests and attaches MB Artist/Release Group identities | Provider-specific policy in storage; leave working behavior intact now, move orchestration policy later |
| `storage.rs` canonical Artist and catalog credit insertion | MB Artist identity is the strong merge trigger | Intentional provider-specific safety rule, not safe to generalize to arbitrary IDs |
| `storage.rs` catalog import | Requires external Album identity; routes MB Recording/ISRC from Track identity list | Import-boundary leak; future import DTO should explicitly separate grouping, occurrence and recording evidence |
| `recording.rs` compare/prepare/bind | Requires MB Recording candidate, known MB group, MB Artist IDs; MBID merge conflict rule | Discovery/acceptance coupling; intentionally not reused by the edition comparator |
| Matching worker and diagnostic frontend | One configured MusicBrainz workflow/circuit; MB labels/IDs | Current integration limitation, not a provider fallback implementation |
| Migration 0008 | Copies historical MB Recording/ISRC evidence without merging | Legitimate compatibility migration; do not rewrite shipped history |

`CatalogProvider` currently combines Album/Artist discovery, edition browse/detail,
Recording discovery and import DTOs. Playback remains separate and should stay so.
Additive `EditionProvider::edition(identity)` supplies generic detailed evidence for
this probe. No universal query language or sweeping trait split was introduced.
Later, capability-specific discovery should accept application evidence and return
bounded candidates/completeness; each adapter chooses its own search strategy.

## Implemented evidence boundary

`edition::EditionCandidate` contains an external identity, optional grouping,
`EditionMetadata`, ordered `TrackEvidence` and explicit tracklist completeness.
Metadata retains title, ordered Artists, partial date, barcodes, paired label/catalog
numbers, country, format descriptions and edition text. Empty collections/Options
mean unavailable evidence, not negative evidence.

Artist evidence retains multiple external identities plus credited names/join phrases.
Track evidence separates occurrence identities, optional disc/number/title/Artists/
duration from `RecordingEvidence { identities, isrcs }`. Only identifiers verified
as recording-level belong there. No provider-specific fields or provider-name tests
appear in the comparator.

`LocalEditionEvidence` adds application Album/Release IDs, Album title, known external
grouping/Release identities and local Track/Recording IDs. A targeted read transaction
constructs it without requiring any external identity. Current storage does not
retain local barcode/catalog-number/physical-format evidence or proof that a local
tracklist is complete: those fields stay unknown. Codec format is not physical media.
The extractor preserves existing Recording mappings as evidence; future adapters
must not place catalog occurrence IDs on Recording indiscriminately.

## Identity certainty (read-only)

**Identical recordings do not prove identical pressing/edition.** Each candidate
and the overall report use these assessments; none authorizes persistence:

| Assessment | Meaning |
| --- | --- |
| `ExactEdition` | An independently trusted exact-edition identifier agrees, without strong contradictions. |
| `ContentEquivalent` | Both complete ordered musical programs agree; packaging/catalog origin remains unknown. |
| `AlbumOnly` | Known grouping or normalized Album title plus Artist supports the friendly Album, without establishing edition/content equivalence. This is support, not a replacement for existing Album acceptance rules. |
| `InsufficientEvidence` | Available evidence cannot establish those claims. |
| `Contradictory` | Trusted exact/Recording identifiers or trustworthy ordered-program evidence conflict. |
| `Ambiguous` | Multiple content-equivalent/exact candidates, or unresolved alternatives prevent one edition resolution. |

`exact_identities` is separate from generic external mappings and the candidate's
locator. Populate it only when identifier semantics and provenance independently
establish exact edition identity (for example a trusted embedded provider Release
identifier). MusicBrainz contributes its catalog Release ID as candidate-side exact
evidence; this does not identify a physical copy. No barcode, title, year, ordered
tracklist, ISRC or recording sequence is promoted to exact evidence. The current
database extractor leaves local `exact_identities` empty: generic stored mappings
are not silently assumed to have the required provenance. Embedded-ID extraction
and trusted manual confirmation remain future work.

Local `Completeness::TrustedComplete` asserts independent knowledge of the entire
musical program **and its order**. `Unknown` is the default and the only value
currently emitted by database extraction, even for contiguous Track numbers or
catalog-created Releases: the database does not retain this proof. Synthetic tests
explicitly supply completeness. Provider tracklist completeness comes from the
adapter's existing count/media validation. No count comparison establishes local
completeness; partial 3-of-15 or one-Track imports never prove whole-program equivalence.

Complete lists are compared by their supplied flattened musical order, retaining
disc/position metadata unchanged. Two CDs can equal one digital sequence. Every
element must agree: trusted Recording identities take precedence over spelling;
otherwise exact conservatively normalized title agreement supplies support. Known
durations use a 3,000 ms tolerance for metadata-only comparison. **Trusted Recording
identity outranks duration disagreement; duration is supporting evidence, not canonical
identity.** A shared trusted Recording identity retains compatibility despite a reported
`DurationConflict`; without that stronger identity, the mismatch remains a contradiction.
Artist-name disagreement or disjoint ISRCs
withholds title-only support, but is not a strong contradiction that overrides a
shared trusted Recording identity. ISRC alone never establishes content/exact identity.
Punctuation/version qualifiers are preserved: unsupported title differences lower
certainty rather than automatically proving different audio. Known sequence
permutations and extra/missing Tracks are contradictions. Conflicting trusted Recording
identities remain contradictory regardless of duration or partial identity overlap.

Partial lists compare only unambiguous supplied disc/track positions. They can
expose conflicting Recording evidence, but unknown alignment is not a contradiction.
Partial alignment across repackaged media remains deliberately unresolved.
Country, barcode, label, catalog number, year and physical/digital format differences
are packaging diagnostics, not content vetoes. No metadata is rewritten.

Trusted exact or Recording identifiers with different values in the same namespace
are contradictions, including internally inconsistent assertions. Different namespaces
are absent shared evidence. Generic many-to-many identity storage is unchanged:
only independently vetted strong identifiers belong in these evidence fields.
Metadata cannot score through a strong conflict. A contradictory positively identified
candidate blocks resolution to another weaker candidate.

Two content-equivalent represses produce overall `Ambiguous`. One valid exact
candidate can win over content-only alternatives. Incomplete discovery is reported
separately and blocks unique content resolution, while retaining per-candidate
assessments. An independently proven exact match need not wait for exhaustive discovery.
Unknown competitors also block unique content resolution. Provider order/score never
breaks ties. No exact provider hierarchy or numeric confidence is required.

## Read-only MusicBrainz probe

```sh
cargo run --release --manifest-path adapters/musicbrainz/Cargo.toml --example edition_probe -- \
  /path/to/library.sqlite APPLICATION_RELEASE_ID
```

The database opens with SQLite read-only flags: no migrations, imports or identity
writes. This particular discovery tool requires one known MB Release Group, browses
one page, then looks up at most three editions using the same client/rate limiter/
bounded retries. It requests labels in addition to existing full import details;
normal Add Album lookup is unchanged. It never makes a request per Track.
Truncation, extra browse pages, pregap/data-track complexity or incomplete details
prevent a complete-result claim. Output prints local/candidate evidence and reports.
Errors abort the probe, without altering the normal queue/circuit or application data.
This standalone opt-in tool does not add an automatic queue request stream.

## Validation and local cost

Synthetic-provider tests cover explicit exact IDs and conflicts, complete/partial
programs, packaging differences, indistinguishable represses, reordered/bonus/multi-disc
Tracks, Recording and ISRC evidence, duration boundaries, semantic qualifiers,
unknown fields and occurrence-only IDs. Storage tests verify indexed targeted
lookups and unchanged database bytes after both snapshot entry points. Deterministic
MusicBrainz HTTP fixtures verify one detailed request, field conversion, ordered
credits, identities, labels, durations and incomplete tracklists.

Current release-build medians over 1,000 iterations, excluding network and SQLite:

| Local evidence | 1 candidate | 10 candidates | 100 candidates |
| --- | ---: | ---: | ---: |
| Trusted complete, 10 Tracks | 1.96 µs | 21.98 µs | 225.73 µs |
| Partial, Tracks 2/5/10 of 10 | 0.89 µs | 10.19 µs | 101.91 µs |

These supersede the earlier comparator's timings; no production import/search path
was added. Run `cargo test --release --test edition_probe comparison_timing -- --ignored --nocapture`.
The existing 200k-Track performance harness also completed; no production search or
import query was changed. A live MusicBrainz edition probe was not run for this audit;
normal tests remain independent of the public service.

## Before persistent edition matching

Define what “same relevant catalog edition” means across physical pressings and digital
market variants. A barcode or identical audio sequence is not universally unique.
Define trusted completeness/provenance, barcode normalization, catalog-number namespaces,
relinking and contradictory identifiers before allowing writes. Do not infer missing
Tracks, merge Releases or rewrite credited/local metadata.

The comparator can support Spotify-first, Apple-first, Discogs-first or MB-first
evidence today. The existing catalog import and automatic matching orchestration
cannot yet switch providers unchanged: optional external grouping and separate
capability/policy boundaries must be implemented when a second adapter is added.
Fallback selection, cross-provider corroboration and per-provider circuits remain
future orchestration work. No provider hierarchy is required by the new boundary.
