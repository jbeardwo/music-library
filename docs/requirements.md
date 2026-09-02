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

A higher-level abstract Album concept may be introduced later if cross-Release grouping becomes useful.

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

## Add Release Semantics

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
* Higher-level abstract Album grouping.
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
