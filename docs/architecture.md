# Music Library — Architecture

## Architectural Goals

The architecture should prioritize:

* Correct product semantics.
* Preservation of user intent.
* Fast startup.
* Responsive interaction.
* Efficient operation at large-library scale.
* Source independence.
* Frontend independence.
* Incremental work.
* Explicit durable-state boundaries.
* Simple components with clear responsibilities.

Prefer a modular monolith over speculative services, frameworks, or abstraction layers.

## Architectural Principle

Preserve facts and user intent separately.

Derive efficient effective views where needed.

Defer uncertain identity matches rather than silently merging data.

## Source-Agnostic Model

The application is not fundamentally a filesystem music scanner.

The library may be built from:

* External catalog discovery.
* Local files.
* Other future discovery sources.

A user may have a complete library without any locally stored music.

Filesystem scanning is therefore an adapter/workflow around one source type, not the origin of the domain model.

## Domain Model

### Release

A Release represents a specific edition/version.

It is the current durable grouping above Track.

Different editions may coexist even when much of their metadata is identical.

Release identity uses application-generated opaque IDs.

External-provider identifiers may be associated with Releases but do not define internal identity.

An abstract cross-Release Album concept is intentionally not modeled yet.

### Track

A Track is release-specific.

A Track belongs to exactly one Release under the current model.

Track identity uses an application-generated opaque ID.

A Track is not:

* A filesystem path.
* A file hash.
* An external catalog ID.
* Necessarily an abstract recording.

Abstract recording identity is intentionally deferred.

### Artist

Artists are durable domain entities identified internally by opaque IDs.

Tracks and Releases relate to Artists through ordered credits.

Track-level and Release-level credits are distinct.

The model should support roles without committing prematurely to a comprehensive contributor taxonomy.

### PlayableSource

PlayableSource represents a potential way to play a Track.

A source is a separate entity from Track.

Sources may be:

* Associated with a Track.
* Discovered but unassociated.
* Available.
* Unavailable.
* Reassociated later.

A Track may have zero-to-many associated sources.

Local filesystem sources are only one source kind.

The durable source model should remain capable of representing future non-filesystem source kinds.

### Source Association

Association between Track and PlayableSource should be modeled separately from the source itself.

This preserves the ability to:

* Keep unassociated discovery candidates.
* Associate multiple sources with one Track.
* Detach or reassociate sources.
* Avoid making source identity equivalent to Track identity.

### Library Membership

Track existence and library membership are separate.

Membership should be represented independently, such as a membership record keyed by Track.

Adding a membership record is an explicit user/application operation.

Removing the membership record is explicit removal from the user's library.

Scanning has no permission to insert or delete membership records.

Release library visibility is derived from member Tracks.

Do not put an independent `in_library` state on Release.

## Known Versus Library Entities

The system may know about Tracks or Releases that are not currently represented in the user's library.

Distinguish:

* Known Release.
* Release represented in the library.
* Release shown in a discovery/import context.

Likewise, a known or discovered PlayableSource may exist without an imported Track.

## Discovery and Import

Discovery and import are separate application concepts.

### Discovery

Discovery may:

* Enumerate possible sources.
* Record source observations.
* Parse source metadata.
* Persist discovery candidates.
* Mark observations about availability.

Discovery must not automatically establish:

* Durable Track identity.
* Durable Release identity.
* Library membership.
* Ambiguous duplicate identity.

### Import

Import creates or selects durable domain entities and explicitly associates discovered information with them.

For the first backend slice, explicit import is the preferred conservative workflow.

A first implementation may:

1. Discover sources.
2. Inspect candidates.
3. Explicitly import selected candidates.
4. Create or select a Release.
5. Create release-specific Tracks.
6. Associate sources with Tracks.
7. Explicitly add Tracks to the library.

This slice-level workflow does not permanently decide the final import UX.

External catalog workflows must eventually be able to create known/library Tracks and Releases without local PlayableSources.

## Identity Creation

Initial durable identities should use application-generated opaque identifiers for:

* Release.
* Track.
* Artist.
* PlayableSource.

Do not use as primary keys:

* Metadata combinations.
* Paths.
* File hashes.
* External-provider identifiers.

Do not automatically merge new entities based on similarity during the first implementation.

Identity matching can evolve independently.

## Metadata Architecture

Metadata must preserve provenance where it affects behavior.

Potential sources include:

* File observations.
* External-provider observations.
* Application-derived values.
* User overrides.

### Source Observations

Source-specific observations should be stored without destructively overwriting unrelated sources.

For filesystem sources, observations may include:

* File tag metadata.
* Duration.
* Format/codec facts.
* File attributes useful for change detection.
* Last observed path.
* Last successful observation time.
* Availability state.
* Scan bookkeeping.

### User Overrides

User edits are sparse overrides.

No override should be distinguishable from an explicit override.

Where blank displayed values are allowed, an intentional blank should also be distinguishable from absence of an override.

Deleting/clearing an override reveals the underlying effective value.

### Effective Metadata

Effective metadata is the materialized/derived view used for:

* Display.
* Sorting.
* Grouping.
* Search.

Effective values should be updated when relevant observations or overrides change.

FTS rows should be updated transactionally with effective searchable metadata.

Until external providers exist, a replaceable provisional resolver may use:

1. User override.
2. File observation.
3. Application-derived fallback.

External-source precedence remains deliberately undecided.

### Initial Normalized Fields

The first schema should normalize a small useful vocabulary, likely including:

* Track title.
* Release title.
* Ordered Track artist credits.
* Ordered Release artist credits.
* Disc number.
* Track number.
* Year/date where available.
* Duration.
* Relevant format/codec facts.

Avoid attempting a universal metadata schema in the first migration.

Avoid a generic entity-attribute-value design for frequently queried product metadata.

## Storage

### Database

SQLite is the current durable database direction.

Use:

* Explicit SQL.
* `rusqlite`.
* Checked-in ordered migrations.
* Appropriate indexes.
* Foreign-key enforcement.
* Deliberate transaction boundaries.

A full ORM is not currently justified.

### Connection Ownership

Begin with a simple connection model.

* Serialize writes through controlled ownership.
* Keep write transactions short.
* Do not hold a write transaction while parsing files or performing network work.
* Add read concurrency/pooling only when actual requirements justify it.

### Query Design

Prefer:

* Purpose-specific queries.
* Bounded result sets.
* Projection of only required columns.
* Keyset/cursor pagination for deep browsing where appropriate.
* Explicit indexes supporting hot paths.

Avoid:

* N+1 query patterns.
* Loading the entire library.
* Generic repository abstractions that obscure important SQL behavior.

## Search

SQLite FTS5 is the current baseline.

FTS should index effective searchable metadata rather than every competing source observation.

Initial searchable fields should include:

* Effective Track title.
* Effective Artist names.
* Effective Release title.

Structured filters should use ordinary indexed data.

Expose search as a bounded application operation containing concepts such as:

* Free-text query.
* Structured filters.
* Stable sort.
* Limit.
* Cursor/keyset state.

Exact fuzzy/prefix/substring behavior remains deferred.

## Filesystem Discovery and Reconciliation

Filesystem support is one adapter.

### Discovery

A filesystem scan should:

1. Enumerate candidate files.
2. Record PlayableSource observations.
3. Parse only new or plausibly changed files.
4. Preserve file-tag observations.
5. Batch database work appropriately.
6. Never modify library membership.
7. Never silently merge Tracks or Releases.

### Incremental Change Detection

Initial unchanged detection may use inexpensive observations such as:

* Known path.
* File size.
* High-resolution modification time.

These observations are optimization hints, not durable Track identity.

### Reconciliation

Availability is an observation.

The database is authoritative for what the application knows and for user intent, but not for whether a physical file exists at every instant.

Use scan-run/generation bookkeeping or an equivalent mechanism so that unavailable status is reconciled only after a relevant scan completes successfully.

An interrupted scan must not mark unvisited sources unavailable.

### Watching

Filesystem watching is advisory.

Watcher events may enqueue targeted checks.

Do not treat watchers as authoritative event logs because events may be:

* Dropped.
* Coalesced.
* Reordered.
* Missed while the application is closed.

Periodic or explicit reconciliation can repair watcher gaps later if needed.

## Duplicate and Move Handling

Do not make automatic duplicate merging part of the initial architecture.

Potential evidence may include:

* Paths.
* Tags.
* Durations.
* Sizes.
* Hashes.
* External identifiers.

Ambiguous evidence should not silently merge domain entities.

The first implementation may recognize a previously known source at the same path without attempting sophisticated move detection.

Move matching, hashing policy, and duplicate reconciliation are deferred.

## Application Operations

The backend boundary should expose user-meaningful operations rather than frontend-specific calls or database rows.

Likely operations include:

* Discover sources.
* Inspect discovery candidates.
* Import selected candidates.
* Create/select Releases.
* Add Tracks to the library.
* Remove Tracks from the library.
* Set metadata overrides.
* Clear metadata overrides.
* Search the library.
* Reconcile known sources.
* Later, control queue and playback.

These operations should accept and return bounded plain data structures.

They should not expose:

* SQL rows.
* QML objects.
* React state.
* Tauri-specific messages.
* Window concepts.

## Playback Architecture

Playback should remain behind a small application-owned boundary.

The application should own:

* Queue contents.
* Queue ordering.
* Current Track.
* Play/pause/stop state.
* Seeking intent.
* Next/previous behavior.
* Source resolution.
* Error handling.

The engine should own:

* Decoding.
* Audio-device interaction.
* Format-specific playback work.

Do not commit to GStreamer or another engine yet.

Do not build a generalized playback plugin framework before requirements justify it.

A small engine adapter should be enough to prototype competing playback implementations later.

## Frontend Architecture

Frontend technology is intentionally undecided.

The backend must not import or understand:

* React.
* Tauri.
* Qt.
* QML.
* Webviews.
* Windows.
* Skin implementation details.

Conceptually:

Replaceable frontend

↓

Thin frontend adapter

↓

Application operations

↓

* Library/membership
* Discovery/import
* Metadata resolution
* Search/query
* Playback control

↓

* SQLite storage
* Source adapters
* Metadata adapters
* Playback-engine adapter

A future frontend may link to or host the backend and translate application operations into its toolkit's native mechanism.

Do not introduce a daemon, local HTTP service, IPC layer, or event bus merely to preserve frontend replaceability.

## Skinning Strategy

Winamp-class skinning is a product requirement.

The frontend implementation should eventually be selected using prototypes that exercise the hardest requirements.

Prototype tests should include:

* Frameless windows.
* Transparency.
* Non-rectangular geometry.
* Custom hit regions.
* Custom drag regions.
* Arbitrary control placement.
* Multiple coordinated windows.
* Windows behavior.
* Linux/Wayland behavior.

The public skin format should preferably be frontend-independent.

The frontend renderer may use QML, web technologies, or another toolkit internally without exposing that implementation as the skin specification.

## Process Architecture

Start as a single-process modular monolith.

Do not initially build:

* A daemon.
* A local web server.
* Microservices.
* A plugin host.
* A generic event bus.
* Multiple repositories for internal modules.

Likely modules include:

### Domain

Stable concepts and product rules:

* Track.
* Release.
* Artist.
* Artist credits.
* PlayableSource.
* Source association.
* Library membership.
* Metadata values.
* Overrides.
* Opaque identifiers.

### Application

Coordinates meaningful product operations.

### Storage

Owns:

* SQLite.
* Migrations.
* SQL.
* Transactions.
* Pagination.
* Effective metadata.
* FTS maintenance.
* Scan bookkeeping.

### Source Adapters

Initially may include:

* Filesystem discovery.
* File tag extraction.

Later may include:

* External catalogs.
* Non-local playable sources.

### Metadata Resolution

Combines source observations and user overrides into effective metadata.

### Search

Provides bounded free-text and structured querying.

### Playback

Owns application playback behavior and adapts to a selected engine.

These should initially be modules, not services or elaborate framework layers.

## First Backend Vertical Slice

The smallest useful first backend slice should validate durable boundaries without choosing a frontend.

A useful first slice is:

**discover → import → search → reconcile**

### Initial Behavior

1. Create and migrate a fresh SQLite database.
2. Register one local discovery root.
3. Discover supported test audio files beneath that root.
4. Store PlayableSource observations and extracted file metadata.
5. Avoid reparsing an unchanged source on a second scan.
6. List unassociated discovery candidates through a bounded query.
7. Explicitly import selected candidates into a new Release.
8. Create one Track per selected candidate for this deliberately conservative first slice.
9. Associate each source with its Track.
10. Create ordered credits and disc/track positions.
11. Explicitly add those Tracks to the library.
12. Materialize effective metadata.
13. Search member Tracks by effective title, Artist, and Release.
14. Set and clear a Track-title override and verify search updates.
15. Remove a Track from membership and verify rescanning does not restore it.
16. Make a source disappear and reconcile it as unavailable without deleting the Track.
17. Restore the source at the same known location and restore availability.
18. Interrupt a scan and verify unvisited sources are not incorrectly marked unavailable.

### Deliberate Limitations

The first slice should not include:

* Automatic duplicate merging.
* Sophisticated move matching.
* External metadata providers.
* Artwork pipeline.
* Filesystem watching.
* Real playback engine.
* Tag write-back.
* Automatic library membership from discovery.
* Frontend.
* Public skin API.
* Background service.

The filesystem-based slice is a validation tool for difficult durable boundaries.

It must not cause the architecture to assume local files are required for the final product.

A later external-catalog slice must be able to create Tracks/Releases/library membership with no local PlayableSource.

## Performance

Operations over the library should avoid unnecessary full-library work.

Frequently used queries require appropriate indexes.

Large result sets should use bounded/incremental access.

Maintain a deterministic synthetic database of approximately 200,000 Tracks for selected performance tests and benchmarks.

Eventually establish measurable budgets for:

* Cold startup.
* Warm startup.
* Search latency.
* First-page navigation.
* Memory usage.
* Incremental scanning.
* No-change scanning.

Do not invent arbitrary budgets before an early prototype provides meaningful measurements.

## Durable Versus Reconstructible State

Distinguish:

### Portable Durable State

* Track/Release/Artist entities.
* Library membership.
* User overrides.
* Durable internal identifiers.
* Relevant external identifiers.
* User-created organization/state.

### Source/Machine Observations

* Filesystem paths.
* Availability.
* Last-seen times.
* File attributes.
* Machine-specific source information.

### Reconstructible Caches

* Artwork thumbnails.
* Derived temporary data.
* Other safely rebuildable artifacts.

Backup and restore must preserve irreplaceable state even if machine-specific source paths cannot immediately be reused.

## Deliberately Unmodeled or Deferred

The first durable schema should not accidentally commit to:

* Abstract recording identity.
* Abstract Album grouping.
* Automatic duplicate identity.
* Automatic move identity.
* External-provider authority.
* A specific playback engine.
* A specific frontend.
* A specific public skin format.
* A comprehensive artist-role taxonomy.
* A mandatory local-file workflow.

If implementation begins to depend on one of these decisions, surface the assumption before encoding it.
