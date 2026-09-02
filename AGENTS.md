# AGENTS.md

## Purpose

This repository is a source-agnostic personal music library application.

Users must be able to build and organize a library without having any music stored locally.

Local files are one possible discovery, metadata, and playback source. They are not the foundation of Track identity, Release identity, or library membership.

When making changes, optimize for:

1. Correct product behavior.
2. Performance and responsiveness.
3. Preservation of user intent and data.
4. Simplicity.
5. Maintainability.
6. Clear data ownership and boundaries.

Do not add complexity unless current requirements justify it.

## Core Product Model

### Tracks

A Track is a release-specific domain entity.

A Track is:

* Not merely a filesystem file.
* Not necessarily an abstract musical recording.
* A member of exactly one Release under the current model.
* Independently eligible for library membership.

Abstract recording/work identity may be introduced later if needed.

Do not derive durable Track identity from filenames, paths, hashes, tags, or external-provider identifiers alone.

Use application-generated opaque identifiers for durable internal identity.

### Releases

A Release represents a specific edition or release.

Examples that may be separate Releases include:

* Original releases.
* Remasters.
* Reissues.
* Deluxe editions.
* Regional editions.
* Other materially distinct editions.

Do not merge Releases solely because title, artist, year, artwork, or similar metadata matches.

A higher-level abstract Album concept is not currently required.

Use "Release" when referring to the durable release-specific entity.

### Playable Sources

Playable sources are separate from Tracks.

A Track may have:

* Zero playable sources.
* One playable source.
* Multiple playable sources.

A playable source may exist without yet being associated with a Track.

Local files are one type of playable source. The durable model must not assume every source is a filesystem path.

Future source types may be non-local.

Do not introduce a `track.file_path`-style durable shortcut that requires exactly one file per Track.

### Library Membership

Library membership exists at the Track level.

* Releases and Artists do not have independent saved-library state.
* Adding a Release is a bulk Track-membership operation.
* Removing individual Tracks from a Release is allowed.
* A Release appears in the user's library while at least one of its Tracks is a library member.

Library membership is independent from playable-source availability.

A Track may remain in the user's library with no currently available playable source.

Do not introduce an "owned" concept unless a future product requirement specifically needs one.

## Discovery and Import

Discovery and library membership are separate concepts.

Discovering a file or other source must not inherently add a Track to the user's library.

Discovery may produce an unassociated source candidate.

Import is responsible for creating or selecting the relevant Track and Release and associating a source with them.

Scanning must never silently create library membership.

If the user explicitly removes a Track from the library, later scans must not restore membership merely because a source still exists.

External catalog discovery must eventually be able to create or expose Tracks and Releases without requiring local files.

## Availability and Missing Sources

Missing sources are expected conditions.

If a source disappears:

* Preserve the source record.
* Mark its availability observation appropriately.
* Preserve its Track association.
* Preserve Track library membership.
* Preserve user metadata.

If the source later reappears or is confidently relinked, restore availability without recreating the Track or losing user state.

Scanning must not delete Tracks because a source is temporarily unavailable.

## Identity and Duplicate Handling

Be conservative with automatic identity matching.

Similar:

* Filenames.
* Tags.
* Durations.
* File sizes.
* Hashes.
* Artwork.
* External metadata.

may provide evidence but must not silently establish identity when ambiguity exists.

Prefer preserving distinct entities over incorrectly merging them.

Ambiguous duplicate or move candidates should remain separate or be deferred for reconciliation.

Do not use metadata-derived values, file hashes, or external-provider IDs as primary keys.

## Metadata

Metadata may originate from:

* File tags.
* External metadata providers.
* Application-derived values.
* Explicit user edits.

Preserve relevant source observations rather than destructively replacing one source with another.

User edits are sparse overrides and have highest precedence.

Rescanning files updates file-derived observations only.

Refreshing an external provider updates that provider's observations only.

Neither operation may silently destroy user overrides.

Clearing an override should reveal the appropriate underlying value.

Effective metadata used for display, sorting, grouping, and search may be materialized and indexed for performance.

Avoid a universal entity-attribute-value metadata design unless a demonstrated need justifies it.

Prefer typed normalized fields for metadata the product actively uses.

## Artist Credits and Track Ordering

Do not assume one Artist per Track.

The model should support:

* Ordered Track-level artist credits.
* Ordered Release-level artist credits.
* Distinct Track and Release artist relationships.
* Representable credit roles without prematurely defining a comprehensive contributor taxonomy.

Track ordering must support multi-disc Releases.

Disc number and Track number are ordering metadata, not identity.

## Filesystem Scanning

Filesystem scanning is one source adapter, not the canonical origin of the library.

Prefer incremental scanning.

A scan should:

* Enumerate relevant sources.
* Avoid reparsing sources reliably determined to be unchanged.
* Persist source observations.
* Preserve source-specific metadata.
* Reconcile availability only after a relevant scan completes successfully.
* Never modify Track membership.
* Never silently merge Tracks or Releases.

Interrupted scans must not incorrectly mark unvisited sources unavailable.

Filesystem watching is advisory.

Watcher events may trigger targeted reconciliation, but watcher events are not an authoritative event log.

## Database

Use the database for durable application state and efficient querying.

SQLite with explicit SQL is the current backend direction unless deliberately changed.

When adding or changing persistent behavior:

* Use checked-in migrations.
* Consider required indexes.
* Avoid accidental N+1 queries.
* Avoid loading more rows or columns than needed.
* Prefer bounded results.
* Prefer keyset/cursor pagination where deep browsing makes offset pagination inefficient.
* Keep transactions intentional and short.
* Avoid holding write transactions while performing slow filesystem or network work.

Keep source association separate from the source record so unassociated discovered sources remain possible.

Keep Track membership separate from Track existence.

## Search

Search must remain efficient around 200,000 Tracks.

Search should operate on effective metadata and use bounded result sets.

SQLite FTS5 is the current baseline direction for free-text search.

Structured filters should remain structured rather than being forced into free-text search.

Do not expose arbitrary SQL-like query behavior directly to the frontend.

## Playback

Playback must remain behind a UI-independent application boundary.

The application owns:

* Queue state.
* Current Track state.
* Play/pause/stop behavior.
* Seeking behavior.
* Track advancement.
* Source selection.
* Failure handling.

The playback engine owns decoding and audio output.

Do not let a playback library become the application's queue/state model.

Do not build a generic audio-plugin framework before requirements justify one.

## Frontend and Skinning

Frontend technology is intentionally undecided.

Current candidates may include:

* Qt Quick/QML.
* Tauri with a web frontend such as React.
* Other approaches if later evidence justifies them.

Do not introduce React, Tauri, Qt, QML, webview concepts, or another frontend framework into the core architecture until a frontend decision is explicitly made.

Core application operations must remain UI-independent.

Skinning is a core product requirement, not merely theming.

The eventual system should support, where the operating system permits:

* Arbitrary control placement.
* Custom layouts.
* Custom graphics.
* Custom fonts and typography.
* Transparent and frameless windows.
* Non-rectangular window shapes.
* Custom hit regions.
* Custom drag regions.
* Multiple coordinated windows or panels.
* Winamp-class visual customization.

The public skin format should preferably remain independent of the frontend implementation technology.

Simple skins should not require application programming.

Do not make arbitrary QML, React components, or another implementation framework itself the public skin format.

## Performance

Performance is a first-class product requirement.

Design for approximately 200,000 Tracks as an important stress case.

Prefer designs that:

* Start quickly.
* Keep interaction responsive.
* Avoid unnecessary filesystem access.
* Avoid unnecessary database queries.
* Avoid full-library work when targeted work is possible.
* Avoid loading large datasets into memory unnecessarily.
* Avoid recomputing unchanged data.
* Support indexed and incremental access.
* Keep expensive work away from interactive paths.

Maintain a deterministic synthetic large-library dataset for performance testing.

Establish explicit performance budgets from an early working prototype rather than inventing arbitrary numbers before measurements exist.

Treat performance regressions as product regressions.

## User Data and Portability

Distinguish:

* Irreplaceable user state.
* Reconstructible caches.
* Machine-specific source locations and observations.

User edits, membership, durable entities, and identifiers must be backupable.

Machine-specific paths should not define portable identity.

A restored library may legitimately contain unavailable sources until they are relinked or rediscovered.

## Testing

Add tests for behavior that is important, subtle, or likely to regress.

Prioritize:

* Track-level membership.
* Discovery versus import.
* Missing-source preservation.
* Source association.
* Metadata provenance.
* Sparse overrides.
* Effective metadata.
* Duplicate conservatism.
* Scanner reconciliation.
* Interrupted scans.
* Database migrations.
* Search behavior.
* Large-library performance-sensitive paths.

Use real temporary SQLite databases for storage tests where practical.

Bug fixes should generally include regression tests when the bug can reasonably be reproduced.

## Working Style

Before making a significant change:

1. Read the relevant code and documentation.
2. Understand current behavior.
3. Identify assumptions the change depends on.
4. Determine whether those assumptions are decided or intentionally deferred.
5. Make the smallest coherent change that solves the problem.
6. Run relevant tests and checks.
7. Update documentation when behavior or architecture changes.

Do not rewrite unrelated code while completing a focused task.

Do not silently encode a deferred product decision into the schema or architecture.

If a decision would be expensive to reverse, surface the assumption before committing to it.

## Documentation

`docs/requirements.md` describes product requirements.

`docs/architecture.md` describes current architectural direction.

When implementation and documentation disagree, determine which side is stale rather than silently choosing one.

Keep documentation focused on decisions, constraints, and deliberately deferred questions future contributors need to understand.
