# Music Library — Product Requirements

## Product Goal

The application is a source-agnostic personal music library.

Users must be able to build, organize, search, and preserve a music library without having any music stored locally.

Local files are one possible discovery, metadata, and playback source. They are not a prerequisite for library membership.

## Supported Platforms

* Windows and Linux are first-class supported desktop platforms.
* Linux support must not assume a particular distribution.
* Modern Linux desktop environments, including Wayland, should be supported.
* Some advanced window positioning or skinning behavior may differ by compositor or operating system.
* macOS is not an initial requirement, but the architecture should avoid gratuitously preventing future support.
* Exact minimum operating-system versions may be selected later during packaging.

## Core Domain Model

### Track

A Track is a release-specific library/domain entity.

A Track is not:

* Merely a filesystem file.
* Necessarily an abstract musical recording.

A Track belongs to one Release under the current model.

Abstract recording/work identity may be introduced later if the product requires grouping multiple release-specific Tracks as the same underlying recording.

Durable Track identity should use application-generated opaque identifiers rather than paths, hashes, tags, or external-provider identifiers.

### Release

A Release represents a specific edition/version of an album or release.

Different:

* Remasters.
* Reissues.
* Deluxe editions.
* Regional editions.
* Other materially distinct releases.

may coexist as distinct Releases even when title and primary artist are identical.

Similar metadata must not automatically imply identical Release identity.

External catalog identifiers may be associated with Releases but must not be the sole durable identity mechanism.

Each Release belongs to an application-owned Album, the normal user-facing grouping.

### Playable Source

Playable sources are separate from Tracks.

A Track may have:

* Zero playable sources.
* One playable source.
* Multiple playable sources.

A source may also be known to the application without yet being associated with a Track.

Local files are one type of playable source.

The durable model must not assume every playable source is a filesystem path.

Future non-local source types must not be ruled out by the initial schema.

## Source-Independent Library

The user's library must remain independent of local file ownership and availability.

* A user may build an entire library without storing music locally.
* Tracks and Releases may originate from external catalog or metadata sources.
* Tracks may remain library members with zero playable sources.
* Adding a Track or Release must not require a local file.
* Filesystem scanning is one discovery workflow, not the canonical origin of library entities.
* Playable-source availability is independent of library membership.

Do not introduce an "owned" state.

The relevant durable concepts are:

* Whether a Track is in the user's library.
* Which playable sources are known.
* Which sources are currently available.

## Library Membership

Library membership exists at the Track level.

* Tracks are independently added to or removed from the library.
* Releases and Artists do not have independent saved-library state.
* Adding a Release adds all eligible Tracks belonging to that Release.
* Removing an individual Track from a Release is allowed.
* A Release appears in the library while at least one of its Tracks is a library member.

A Track may remain in the library while currently unplayable.

Removing a Track from the library is an explicit user action.

## Discovery Versus Membership

Discovery and library membership are separate.

Discovering a source does not inherently mean:

* A durable Track has been identified.
* A durable Release has been identified.
* The user wants the Track in their library.

A discovered source may remain unassociated until import or another explicit operation resolves it.

If a user removes a Track from the library, later discovery or scanning must not automatically restore membership merely because a source still exists.

The application may continue to know about a source associated with a Track that is not currently a library member.

The exact initial import workflow may be chosen later.

## Catalog-backed library additions and matching

The library must support music that has no currently playable source.

### Album interaction model

* Album is the normal user-facing grouping for album-oriented music.
* Users should be able to search for, view, and add an Album without selecting a specific physical, regional, or digital edition.
* Album identity must be provider-neutral and application-owned.
* MusicBrainz Release Groups, Spotify Albums, Apple Music Albums, and compatible local album metadata may later be associated with the same application Album.
* A specific Release represents an edition of an Album and should normally remain hidden from ordinary library interaction.
* The application may select a suitable concrete Release internally when edition-specific information such as a tracklist is required.
* Users must be able to inspect or choose a specific Release when edition differences are relevant, including alternate tracklists, deluxe editions, bonus tracks, reissues, or regional versions.
* Normal catalog search results should prioritize friendly Album metadata such as title, primary artist credit, and original release year rather than edition-specific details such as country, barcode, label, or physical format.
* Provider-specific edition metadata must not replace the Album metadata used for normal display.

### Catalog-backed additions

* The user must be able to discover music through an external catalog and add it to the library without possessing a local file or other playable source.
* MusicBrainz is the initial external catalog provider.
* Adding a catalog Album creates/reuses its application Album, a representative or explicitly selected Release, and release-specific Tracks.
* Library membership remains Track-level.
* A catalog-added Track may have zero PlayableSources.
* The absence of a PlayableSource must not make a Track incomplete, invalid, or unavailable for ordinary library organization.
* Catalog metadata and external identifiers must be retained so they can assist later matching to local files or playback providers.
* Application identity must remain independent of MusicBrainz or any other external provider.
* The architecture must permit additional external identities, including Spotify and Apple Music identifiers, without changing the identity of existing library entities.

### External metadata and identity

* Original provider metadata must be preserved.
* Metadata used for display must not be destructively rewritten solely to improve cross-provider matching.
* Matching may use derived normalized values without replacing original metadata observations.
* External identifiers such as MusicBrainz MBIDs, ISRCs, and provider-specific track IDs should be preferred over title-string matching when available.
* MusicBrainz Recording identifiers and ISRCs may be associated with application Tracks without requiring an application-level Recording entity.

### Local-file import and catalog matching

* Local files must always be importable without a successful external catalog match.
* An unmatched local Track is a normal supported library state.
* When local metadata is sufficiently complete, import may attempt to associate the music with an existing library Track or external catalog identity.
* Initial automatic matching should prefer structured release-level evidence over isolated fuzzy title matching.
* Useful evidence may include artist credit, release title, disc number, track number, track title, release track count, duration, embedded external identifiers, and ISRC.
* If imported files confidently correspond to source-less Tracks already in the library, their local files should be attached as PlayableSources to those existing Tracks rather than creating duplicate Tracks.
* Local-first Tracks may gain external catalog and playback-provider identities later without being recreated or losing their local metadata observations.
* Failure to find a match must not prevent import or local playback.
* Ambiguous matches must not be silently treated as certain matches.

### Automatic matching and partial Albums

* Local import must complete successfully without waiting for catalog matching.
* Imported local music must become usable immediately even when the external catalog is unavailable, slow, rate-limited, or unable to find a match.
* Catalog matching must operate as a separate best-effort enrichment step after local import.
* Automatic catalog matching should be enabled by default when imported metadata is sufficiently complete to justify an attempt.
* The local-file scanner and importer must not depend on automatic matching being enabled.
* The architecture must permit automatic matching to be disabled by user preference in the future without changing local-import semantics.
* A settings UI for this preference is not required initially.
* Users must be able to manually initiate or retry catalog matching after import, including when automatic matching is disabled or a previous attempt failed.

#### Partial Albums

* Local Albums may contain only a subset of the Tracks known to exist on an external catalog Album or Release.
* Import must create library membership only for Tracks actually represented by the user's local files.
* Catalog matching must not add missing Tracks to library membership as a side effect.
* A complete Track count match must not be required for Album matching.
* Partial Albums may be matched using available structured evidence such as Album title, Album artist, Track positions, Track titles, durations, dates, and embedded external identifiers.
* Several consistently matching Tracks may provide strong evidence for an Album match even when most Tracks from the catalog Release are absent locally.
* The application may associate a partial local Album with an external Album identity while leaving its exact Release identity unresolved.
* The application must not claim a specific Release when the available local metadata cannot reliably distinguish among editions.
* Additional local Tracks imported later may join an existing Album without requiring a special "partial Album" state transition.
* Explicit catalog Add Album remains distinct from local matching and may add the complete Track set of the selected or representative Release.

### Matching eligibility and acceptance

#### Matching order

* Automatic matching should first attempt to associate imported music with Albums and Tracks already present in the library.
* External catalog lookup should be attempted only when no sufficiently strong existing-library association is available.
* Matching should operate on grouped Album context where possible.
* The application must not issue one external catalog search per imported Track when a single Album-level lookup can provide the necessary evidence.
* Source-less existing Tracks that confidently match imported local files should receive those files as PlayableSources rather than being duplicated.

#### Eligibility for automatic lookup

* A local Album group should generally have a usable Album title, usable artist evidence, and at least one usable Track title before automatic catalog lookup is attempted.
* Track numbers, durations, dates, and additional Tracks may strengthen a match but are not required solely to justify a lookup.
* Music with severely incomplete, placeholder, contradictory, or unusable tags may be skipped by automatic catalog matching without affecting import.
* Skipped music must remain fully usable as local library content.

#### Automatic acceptance

* Automatic match acceptance must use multiple structured metadata signals rather than relying solely on a single fuzzy title comparison.
* Candidate ranking may use transient scores, but the initial implementation must not require a persistent numeric confidence field.
* Album title and artist agreement should form the primary Album-level evidence.
* Track titles, positions, durations, year/date compatibility, multiple agreeing Tracks, and embedded identifiers may strengthen the decision.
* Partial Albums must be eligible for automatic matching.
* A complete Track-count match must not be required.
* Multiple local Tracks consistently matching one Album candidate may justify automatic acceptance even when most Album Tracks are absent.
* When multiple plausible candidates remain, the application must not silently accept the highest-scoring candidate solely because it ranks first.
* Ambiguous results must remain unmatched until the user explicitly chooses a match or additional evidence becomes available.

#### Identity precision

* The application may attach an external Album identity without attaching an exact external Release identity.
* Local metadata must not be forced into a specific external Release when the available evidence cannot distinguish among editions.
* Release-specific external Track identifiers must not be attached unless the corresponding Release identity is sufficiently established.
* Recording-level identities may be associated independently when justified by available evidence.
* Album-known/Release-unknown and Recording-known/release-Track-unknown are valid supported states.
* The application must not infer identity by blindly discarding qualifiers such as live, remix, edit, acoustic, remaster, or similar version information.

#### Local existing-library matching boundary

Existing-library matching may run within local import because it uses only the
local database. Album title/artist agreement is candidate generation only;
metadata-only acceptance also requires corroborating positioned Track titles and
must decline ambiguity among supported Albums. A confident Album match must not imply a particular edition:
when Release identity is unknown, create a local Release under that Album rather
than attaching files to arbitrary catalog Tracks. Ambiguous or unusable metadata
falls through to normal local creation. External catalog matching remains a
separate post-import operation and must never delay this transaction.

Post-import matching enriches Artist and Album identities without rewriting local
metadata. Artist identity must be established first through a stored MBID, unique
exact normalized primary name, safe unique exact MusicBrainz alias, or explicit
manual selection. Exact primary/alias evidence on different Artists is ambiguous.
A close nonempty primary/alias name may veto alias acceptance (one Unicode edit); fuzzy names and scores never positively establish Artist identity.
Complex credits must not be flattened.

Album evidence may disambiguate already-plausible Artists before manual selection:
one bounded Artist-scoped search must establish exactly one supported Artist and
one unambiguous Album using normal automatic Album rules. Close Artist names alone
cannot win, and Album evidence cannot introduce unrelated Artists. Pair-dependent
resolution must not become a name-only identity shortcut for later Albums.

Only within the established Artist may Album comparison consider, in order, exact
titles, controlled trailing packaging/type decorations, then small edit distance.
The decoration list is EP, LP, CD, CD1/CD 1, CD2/CD 2, Disc 1/2, 2CD and 2xCD; EP removal
requires candidate EP type and LP removal requires Album type. Complete literal
titles always precede fallback: these designations may genuinely belong to the title.
Plain, bracketed, parenthesized and spaced-dash suffixes
are comparison-only. Semantic/version qualifiers are not stripped. Automatic
Artist resolution allows one edit with both Album titles at least five characters;
explicit manual Artist selection additionally allows two edits with both at least
eight. Multiple qualifying candidates stay ambiguous, regardless of score. Manual
confirmation is session-local for that Album; it is not a persisted confidence score.
A confidently resolved Artist survives later Album failures. Automatic dispatch
remains default-on and independently disableable; local-only matching is unchanged.

#### Matching execution

* Local import must not wait for automatic external matching to complete.
* Large imports must become usable before catalog enrichment finishes.
* Automatic matching work should be grouped by Album where possible and processed independently of filesystem scanning.
* External provider rate limits and transient failures must not delay or invalidate completed local imports.
* Manual Match or Retry Match behavior must remain available for skipped, failed, or ambiguous items.

### Initial matching scope

The initial matching implementation assumes reasonably tagged music.

It is not required to:

* identify severely mistagged or untagged music;
* infer identities primarily from filenames or directory names;
* perform acoustic fingerprinting;
* use AcoustID or similar fingerprint services;
* aggressively fuzzy-match weak metadata;
* automatically repair or rewrite file tags;
* guarantee a catalog identity for every imported Track.

Users may correct poor metadata outside the application. More sophisticated identification and tag-management functionality may be added later, but it is not required for the initial catalog-matching system.

Catalog matching is an enrichment feature, not a prerequisite for library membership or playback.

## Availability and Missing Sources

A Track remains a valid library entity when its sources are unavailable.

When a source disappears:

* Preserve the Track.
* Preserve library membership.
* Preserve the source record.
* Preserve the association where appropriate.
* Preserve user metadata.
* Mark source availability as an observation.

A scan must not silently remove Track membership because a source cannot be found.

If a source later reappears or is confidently relinked, the existing Track should regain that available source without losing library state or user edits.

## Album and Release Addition Semantics

Normal "Add Album" selects a representative concrete Release for its tracklist.
Advanced edition selection chooses that Release explicitly. Both use the same
Track-level membership operation below; neither creates Album-level saved state.

"Add Release" is a bulk Track-membership operation.

It is not an independent Release-level saved state.

The operation applies to a known Release with a known set of candidate Tracks.

Adding a Release:

* Adds all eligible Tracks belonging to that specific Release.
* Does not prevent later removal of individual Tracks.
* Does not create a contradictory Release-level membership flag.

Candidate Tracks may eventually originate from:

* Local discovery/import.
* External catalogs.
* Other explicit discovery workflows.

"Add Release" must not fabricate Tracks solely from incomplete metadata such as title alone.

## Duplicate and Identity Policy

Duplicate detection and durable identity are separate concerns.

Similar:

* Filenames.
* Paths.
* Tags.
* Durations.
* Sizes.
* Hashes.
* Artwork.
* External metadata.

may identify candidates but must not silently establish Track or Release identity when ambiguity exists.

Multiple physical sources may represent one Track.

Similar sources may also represent distinct Tracks, particularly across different Releases.

Prefer preserving distinct data over incorrectly merging entities.

Ambiguous matches should remain separate or be deferred for reconciliation.

## Metadata

Metadata may originate from:

* File tags.
* External metadata providers.
* Application-derived values.
* Explicit user edits.

Source observations should remain distinguishable where they affect behavior.

Metadata from one source must not destructively overwrite unrelated source observations.

### User Overrides

User edits are sparse overrides and have highest precedence.

Rescanning files:

* Updates file-derived observations.
* Must not destroy user overrides.
* Must not destroy unrelated external metadata.

Refreshing external metadata:

* Updates that provider's observations.
* Must not destroy user overrides.
* Must not destroy unrelated source observations.

Clearing a user override should reveal the appropriate underlying value.

The representation must distinguish:

* No override.
* An explicit override value.
* An intentionally blank override where the product permits blank values.

### Effective Metadata

The application exposes effective metadata values for:

* Display.
* Grouping.
* Sorting.
* Search.

Effective values may be materialized and indexed for performance.

The exact precedence between file-tag and external-provider metadata is intentionally deferred and may ultimately be field-specific.

Until external providers are introduced, a provisional effective-value policy may use:

1. User override.
2. File observation.
3. Application-derived fallback.

This policy must remain replaceable.

## Artist Credits

Tracks may have multiple ordered artist credits.

Release-level artist credits are distinct from Track-level credits.

The durable model must not assume exactly one Artist per Track.

Artist-credit roles should be representable without prematurely defining a comprehensive contributor taxonomy.

The exact roles surfaced by the initial UI are intentionally deferred.

## Multi-Disc Releases

Track ordering must support multi-disc Releases.

Disc number and Track number are ordering metadata.

They are not identity by themselves.

## Playback

Playback must be isolated behind a frontend-independent application boundary.

The application owns:

* Queue state.
* Current Track state.
* Play/pause/stop behavior.
* Seeking.
* Previous/next behavior.
* Source selection.
* Playback failure handling.

The underlying playback engine owns decoding and audio-device output.

Core requirements include:

* Common lossless local formats.
* Common lossy local formats.
* Reliable seeking.
* Reliable transitions.
* Gapless playback where appropriate.

The exact playback engine and advanced audio features are intentionally deferred.

## Search

Search should feel fast and incremental while the user types.

At minimum, search should cover effective:

* Track title.
* Artist name.
* Release title.

Structured filters such as:

* Availability.
* Year/date.
* Format.
* Release.
* Artist.

should remain distinct from free-text search.

Search must remain efficient around 200,000 Tracks and must not require loading the full library into memory.

SQLite FTS5 is an acceptable initial strategy if it satisfies the product requirements.

## Filesystem Scanning

Filesystem scanning is one source adapter/workflow.

It must not become the conceptual foundation of the library.

Scanning should be incremental where practical.

A scan should:

* Enumerate candidate local sources.
* Avoid reparsing sources reliably determined to be unchanged.
* Record source observations.
* Preserve file-derived metadata observations.
* Reconcile source availability.
* Never silently alter library membership.
* Never silently merge ambiguous Tracks or Releases.

Reconciliation of missing files should occur only after the relevant scan completes successfully.

An interrupted scan must not incorrectly mark unvisited sources unavailable.

Filesystem watchers are advisory accelerators, not authoritative event logs.

## Metadata Editing and File Tags

Editing metadata in the application changes durable application state.

It does not automatically rewrite source file tags.

File tags remain one metadata source.

Writing application metadata back to files, if supported, must be an explicit user operation.

Whether tag write-back ships in the initial release is intentionally deferred.

## Artwork

Artwork does not participate in Track or Release identity.

Artwork may originate from:

* Embedded file artwork.
* Local image files.
* External metadata providers.
* Explicit user selection.

User-selected artwork must not be silently overwritten by rescans or external metadata refreshes.

The application may maintain cached or resized artwork appropriate for UI use.

Large artwork should not be decoded or retained at full resolution for every library item during normal browsing.

## External Metadata

External metadata support is part of the intended architecture but is not required for the first backend vertical slice.

The application must remain usable for building and organizing a library without depending on any one external provider.

Tracks and Releases may eventually be discovered or created through external catalogs even when no local playable source exists.

Durable identity must not depend on the continued availability of an external provider.

Provider identifiers may be stored when available.

## Backup and Portability

Irreplaceable user state must be distinguishable from reconstructible caches and machine-specific source observations.

Portable durable state includes, at minimum:

* Track and Release entities.
* Library membership.
* User overrides.
* Durable internal identifiers.
* Relevant external identifiers.
* User-created organization/state.

Machine-specific source paths and availability observations need not define portable identity.

A restored library may initially contain unavailable sources until they are relinked or rediscovered.

## Frontend and Skinning

Frontend technology is intentionally undecided.

Likely candidates currently include:

* Qt Quick/QML.
* Tauri with a web frontend such as React.

Core domain, storage, discovery, metadata, search, and playback logic must not depend on the selected frontend technology.

Before committing to a frontend, disposable prototypes should test the hardest skinning and window-management requirements.

### Skinning

Skinning is a core product requirement, not merely theming.

The eventual skin system should support, where permitted by the operating system:

* Arbitrary control placement.
* Arbitrary layout.
* Custom graphics.
* Custom fonts and typography.
* Transparent and frameless windows.
* Non-rectangular window shapes.
* Custom input/hit regions.
* Custom drag regions.
* Multiple coordinated windows or panels.
* Winamp-class visual customization.

Simple skins should not require writing application code.

The public skin format should preferably remain independent of the frontend implementation technology.

Do not make arbitrary QML, React components, or another implementation framework itself the public skin format.

## Performance

Performance is a first-class product requirement.

The application should:

* Start quickly.
* Remain responsive during navigation and search.
* Avoid unnecessary background work.
* Avoid loading or recomputing data that is not needed.
* Use memory efficiently.
* Scale to large music libraries.

Approximately 200,000 Tracks is an important large-library stress case.

Startup must not require:

* Scanning music directories.
* Loading the entire library.
* Rebuilding large derived datasets.

Interactive queries should use indexed, bounded access patterns.

Large result sets should be paginated, streamed, virtualized, or otherwise processed incrementally.

Maintain a deterministic synthetic library of approximately 200,000 Tracks for performance testing.

Explicit budgets should eventually be established for:

* Cold startup.
* Warm startup.
* Search latency.
* First-page navigation.
* Memory use.
* Incremental scanning.
* No-change scanning.

These budgets should be established from an early working prototype rather than guessed before implementation.

## Intentionally Deferred Decisions

The following remain intentionally open unless implementation proves one must be decided earlier:

* Abstract recording/work identity.
* Cross-source Album matching and reconciliation.
* Exact file-tag versus external-provider precedence.
* Which discovery sources ship initially.
* Whether first-time filesystem discovery automatically imports/adds Tracks.
* Whether external-catalog Tracks with no playable source may immediately be added through the first UI.
* Exact duplicate and move-matching algorithms.
* Routine versus lazy content hashing.
* User-facing duplicate reconciliation workflow.
* Exact codec list.
* Playback engine.
* ReplayGain behavior.
* Exclusive/platform-specific audio output.
* DSP/equalizer/plugin architecture.
* Exact free-text matching semantics.
* Fuzzy search.
* Advanced search syntax.
* Exact filesystem-watcher implementation.
* Exact unchanged-file heuristic.
* Periodic versus user-initiated reconciliation.
* File-tag write-back in the initial release.
* Artwork source precedence and cache policy.
* Exact artist-credit roles surfaced initially.
* Compilation/classical-specific UI behavior.
* First external metadata provider.
* Backup/export UI.
* Exact OS-specific storage paths.
* Frontend toolkit.
* Public skin-package specification.

Deferred decisions must not be silently encoded as irreversible defaults.
