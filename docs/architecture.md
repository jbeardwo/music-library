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

Each Release belongs to an application-owned Album, the normal user-facing grouping.

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
* Control the in-memory queue and playback.

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

GStreamer/GstPlay is the first empirical local-audio adapter, not a final engine choice.

Do not build a generalized playback plugin framework before requirements justify it.

A small engine adapter should be enough to prototype competing playback implementations later.

The initial backend slice implements `playback::Playback<E>`, which owns an engine and
an in-memory `PlaybackState` (Track-ID queue, position, status, and resolved source).
Callers observe state through an immutable snapshot reference and issue commands;
no frontend types or SQLite session persistence are involved. Queue entries may
repeat and need not be library members.

`Library::available_playback_source` resolves through Track/source associations.
It selects the available supported source with the smallest opaque source ID in
SQLite binary order. This is a deterministic initial rule, not a preference or
fallback policy. `PlayableSource` carries source identity and a `SourceLocation`;
the existing local-file adapter is the only implemented location variant. Tracks
remain independent of paths. No available supported source produces an explicit
error without changing queue contents, membership, or metadata. The attempted
entry remains selected; prior playback is stopped so it cannot be mistaken for
playback of that entry. Successful stop leaves status `Stopped` with the error
returned to the caller; an engine stop failure instead leaves status `Failed`.
Availability is a stored observation; opening a source can still fail.

`PlaybackEngine` accepts a resolved source for `start` (replacing the previous
input and starting from the beginning), plus `pause`, `resume`, and `stop`.
The default fake engine completes commands synchronously. An asynchronous engine
accepts commands on return and confirms them with plain `EngineEvent` values.
`PlaybackState.status` is confirmed state; `pending` is the latest requested target.
The application also owns media position/duration in milliseconds, separate from
the queue index. Neither events nor domain types contain GStreamer or Qt types.

* Setting a queue stops playback and selects its first entry without starting it.
* Enqueue appends one Track without disturbing playback; appending to an empty
  queue selects position zero. Duplicate IDs remain distinct queue entries.
* Clear stops playback, empties the queue, and clears position/source. A failed
  engine stop retains the queue and position and reports failure; retry is explicit.
* Play starts the selected entry, resumes an already loaded paused input, or is
  a no-op while playing. Resume does not resolve the source again.
* Pause affects playing input; stop clears the loaded source and retains the
  queue and position. Repeated pause/stop commands are harmless.
* Next/previous select and attempt the neighboring entry, including from `Failed`.
  Position records the attempted entry even when resolution, start, or recovery
  fails, so another navigation command can move past it. All earlier queue entries
  remain available to Previous; no listening-history subsystem is needed.
* Previous at the first entry does nothing; next at the last stops and retains
  the last position. Neither wraps nor silently skips an unavailable entry.
  `can_next`/`can_previous` depend on position and length, never playback status.
* An engine error preserves queue contents and the attempted selection, clears the
  resolved source, and marks status `Failed`: actual output may be unknown. Play
  and navigation perform a recovery stop internally before starting another input;
  callers do not need a separate Stop command. If that stop fails, return its error.
  Immediate stop rejection prevents queue replacement/clear. With an asynchronous
  engine, accepted commands commit queue changes without waiting; output remains
  explicitly pending until confirmed. A subsequent engine error is surfaced as
  `Failed`, not a rollback of already accepted queue edits.

Playback also owns an ephemeral normalized `Volume` (finite 0–1, default 1).
Invalid values are rejected. A volume command updates accepted session state
without changing transport status, pending transport commands, queue position,
or media generation; rejection leaves the previous state intact. Engines retain
volume across Stop and source replacement. This is not stored in SQLite.

The optional `adapters/gstreamer` package uses GstPlay, which provides application
playback over playbin3. This avoids building a custom decoder pipeline for this
slice ([GstPlay overview](https://gstreamer.freedesktop.org/documentation/play/gstplay.html)).
Its owned worker serializes commands and drains the GstPlay bus with a 20 ms
maximum idle wait. It sends state, EOS, errors, position, and duration as plain
application events. No GStreamer wait runs on the GUI thread. The diagnostic
frontend uses Qt queued callbacks to apply events and notify QML; it does not poll.
Its callback factory constructs the C++ QObject before capturing the weak pointer
([QPointer precondition](https://docs.rs/qmetaobject/0.2.10/qmetaobject/struct.QPointer.html));
otherwise engine events silently disappear before reaching orchestration.

Each Start creates a new GstPlay instance with an immutable media generation.
Stop, replacement, terminal errors, and consumed EOS retire the prior generation;
late messages from it are ignored, including for repeated copies of the same Track.
Pause/resume retain the input and position. GstPlay supplies no command IDs, so
state notifications may only confirm the latest requested target: an obsolete
Playing notification cannot undo a pending Pause. Errors/EOS still refer to the
same input across pause/resume. This is a media-lifetime protocol, not a general
operation/event framework; seeking will need its own completion semantics.

Accepted EOS invokes application Next once. At the last entry it stops and retains
queue/position. If the next entry is unplayable, it remains selected with an error;
there is no automatic skipping. Position updates are requested every 200 ms;
unknown duration stays optional. Stop resets the application clock and releases
the input; Play resolves and starts a fresh input. Tests with GstPlay 1.28.2 found
its stopped position getter could retain a cached nonzero value even though a
subsequent Play restarted at the beginning; the application does not use that
cache as its stopped clock.

Only native local paths are accepted. The adapter converts absolute paths through
[GLib filename_to_uri](https://docs.gtk.org/glib/func.filename_to_uri.html), preserving
Unix filename bytes and escaping reserved characters. WSL `/mnt/c/...` paths are
ordinary Linux paths here; no Windows-path string rewriting or network URI input
is exposed.

On replacement/Stop the worker stops the old player, flushes its bus, and releases
it before confirming Stop or starting another input. Bus flushing breaks the
player/message reference cycle required by the
[GstPlay cleanup contract](https://gstreamer.freedesktop.org/documentation/play/gstplay.html).
No bus watch or application GLib loop is installed. Shutdown joins the worker while
Qt and its callback target still exist; only exit may wait for cleanup. A wedged
plugin could still delay shutdown and needs investigation before product use.

Seeking, gapless/preloading, richer source policy, and queue/session persistence
remain deferred. See the [local-audio test guide](../adapters/gstreamer/README.md)
for measured behavior, platform limitations, and manual audio checks.

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

The explicit catalog import slice creates Tracks/Releases/library membership with no local PlayableSource.

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

## External catalog identity and matching

The application database is authoritative for the user's library. External catalogs provide metadata, identifiers, and evidence that can enrich library entities, but they do not define application identity.

Application entities use opaque internal IDs. External identifiers are stored separately and may be added, corrected, or associated with an existing entity without changing its internal identity.

MusicBrainz is the initial external catalog provider. The architecture must remain provider-neutral so additional catalogs and services such as Spotify and Apple Music can be associated with the same library entities later.

### External identity

External identity is distinct from playable-source availability.

A Track may have:

* no external identities and one or more playable sources;
* one or more external identities and no playable sources;
* both external identities and playable sources;
* neither, when only local or user-supplied metadata is known.

External identifiers may include:

* MusicBrainz Release Group MBID;
* MusicBrainz Release MBID;
* MusicBrainz Track MBID;
* MusicBrainz Recording MBID;
* ISRC;
* provider-specific identifiers such as Spotify Track IDs or Apple Music Song IDs.

External identifiers must not be used as application primary keys.

The initial MusicBrainz mapping is:

* a MusicBrainz Release corresponds to an external identity for an application Release;
* its MusicBrainz Release Group identifies the parent application Album;
* a MusicBrainz Track corresponds to an external identity for an application Track;
* the MusicBrainz Recording referenced by that track may also be retained as an external identity for the application Track;
* ISRCs associated with the recording may be retained as additional recording-level external identifiers for that Track.

The application does not initially require its own durable Recording entity. Recording identifiers may be associated with release-specific Tracks until product requirements demonstrate that a first-class Recording entity is necessary.

External identities should be represented generically rather than by provider-specific columns on Track or Release. Track and Release identities should use separate relational tables so that ordinary foreign-key integrity is preserved without polymorphic entity references.

Migration `0002_external_identities.sql` implements this as two STRICT tables:
`track_external_identity(track_id, provider, kind, external_id)` and
`release_external_identity(release_id, provider, kind, external_id)`. Entity
foreign keys cascade on durable deletion, consistent with other entity-owned
records. Removing library membership does not remove external identities.

Each table has an entity-first composite primary key across all four columns
and a non-unique `(provider, kind, external_id)` lookup index. The primary key
supports ordered entity-local listing and prevents only exact duplicate
associations. Multiple different IDs of the same provider/kind are allowed on
an entity, and the same external identity may be shared by multiple entities.
For example, Tracks on different Releases can share a Recording MBID or ISRC,
and multiple Albums can share a Release Group MBID. No provider-specific
cardinality rules are imposed by this generic storage layer.

`ExternalIdentity` contains three opaque strings, persisted without case folding
or normalization. `Library` and `Store` expose
`attach_track_external_identity`, `list_track_external_identities`,
`resolve_tracks_external_identity`, and corresponding Release methods
(`resolve_releases_external_identity` for reverse lookup). Attachment uses one
atomic insert with an exact-association conflict target: it returns true for an
insertion and false for an identical existing association. Sharing an identity
with another entity is valid. Listing is in binary provider/kind/ID order;
reverse lookup returns all associated IDs in unspecified order, including an
empty vector for no matches. Attachment requires an existing entity through
foreign-key enforcement. These identity operations do not perform matching,
metadata updates, membership changes, or source creation.

### Metadata and matching

Human-facing metadata and matching metadata are separate concerns.

Metadata received from local files, external catalogs, or other providers should be preserved as observations rather than rewritten into a single provider-independent title string.

Matching may derive normalized comparison values from those observations, but derived matching values must not replace the original metadata.

Matching should use the strongest available evidence and should prefer:

1. already-known external identifiers;
2. strong recording identifiers such as ISRC;
3. structured release context, including artist credit, release title, disc and track positions, track titles, track count, and duration;
4. conservative metadata comparison when stronger evidence is unavailable.

Normalization must not blindly discard terms that can distinguish different recordings or versions. Terms such as "live", "remix", "edit", "acoustic", or similar qualifiers may carry identity information rather than merely cosmetic formatting.

### Local-file matching

Local-file import does not require an external catalog match.

When imported music has sufficiently complete metadata, the application may attempt to associate it with an existing library Track or an external catalog entry.

Initial matching should favor release-level context. A consistently tagged album with matching artist, album title, track positions, track titles, track count, and compatible durations provides stronger evidence than independently matching each track by title.

If a source-less catalog Track already exists in the library and a newly imported local file can be confidently associated with it, the local file should become a PlayableSource for the existing Track rather than creating a duplicate Track.

Likewise, a local-first Track may later gain MusicBrainz, ISRC, Spotify, Apple Music, or other identities without being recreated.

Matching is best-effort enrichment. Failure to find a catalog match is a normal state and must not prevent local files from entering or functioning in the library.

The initial implementation deliberately does not attempt to recover severely incomplete or incorrect metadata. It does not require acoustic fingerprinting, aggressive fuzzy matching, filename inference, automatic tag repair, or automatic metadata rewriting. Users may manage poor source metadata externally, and more sophisticated recovery tools can be added later if demonstrated to be valuable.

### Local import and automatic matching workflow

Local-file import and catalog matching are separate operations.

Import must complete independently of catalog availability or matching success. Local files should become usable library entries immediately after their metadata and playable sources have been persisted.

After a successful local import, the application may automatically initiate catalog matching as a best-effort enrichment step when the imported metadata is sufficiently complete.

The intended flow is:

`local import -> usable library entities -> optional catalog matching`

Catalog latency, service outages, rate limiting, or ambiguous matches must not delay, roll back, or invalidate the completed local import.

Automatic matching is enabled by default for sufficiently well-tagged music, but it must be treated as application policy rather than a requirement of the filesystem scanner or importer. The architecture must permit automatic matching to be disabled later without changing local-import behavior.

A user must also be able to initiate or retry catalog matching manually after import. Manual matching remains available even when automatic matching is disabled.

No settings system or preferences UI is required initially. The first implementation may use the default automatic policy while keeping the importer and matcher sufficiently separated to support a future setting.

#### Partial Albums

A local Album does not need to contain every Track from the corresponding catalog Album or Release.

Import creates library membership only for Tracks actually represented by the user's local files. Catalog matching must not add missing Tracks to library membership merely because an external catalog indicates that they exist.

For example, three local Tracks from a fifteen-Track Album remain a valid partial Album containing only those three library Tracks.

Matching should therefore support local Track sets that are subsets of a catalog Release.

Album-level matching may still be strong when a partial set of Tracks consistently agrees with a catalog candidate by evidence such as:

* Album title;
* Album artist credit;
* Track positions;
* Track titles;
* durations;
* year or date where available;
* embedded external identifiers.

A complete Track count match is useful evidence when available but must not be required.

The application may confidently associate a partial local Album with a provider-neutral Album while leaving the exact Release unresolved when there is insufficient evidence to distinguish between multiple editions.

Likewise, individual Tracks may gain Recording-level external identities without requiring the application to claim certainty about the exact physical or digital Release from which the local files originated.

Catalog matching is enrichment only. It must not silently fill missing Album membership.

If the user later acquires additional Tracks from the same Album, those Tracks may be associated with the existing Album and matched independently or collectively without requiring a special transition from "partial" to "complete."

Explicitly adding an Album from a catalog remains a different operation: catalog Add Album means adding the selected/default Release's complete Track set to library membership.

### Matching policy and confidence boundaries

Catalog matching should distinguish between whether a match attempt is worth making and whether a candidate is safe to accept automatically.

These are separate decisions.

#### Matching order

The application should prefer matching against existing library entities before querying an external catalog.

The intended order is:

1. compare imported local Album/Track metadata against Albums and Tracks already present in the library;
2. if no sufficiently strong existing-library match is found, and the metadata is suitable for catalog lookup, query the configured external catalog;
3. if the catalog result is unambiguous and sufficiently supported by structured evidence, accept the association;
4. otherwise leave the music unmatched and allow later manual matching.

This ordering reduces unnecessary network traffic and helps source-less catalog Tracks acquire local PlayableSources without creating duplicate Tracks.

Matching should operate on grouped Album context rather than issuing one external search per file or Track.

#### Eligibility for automatic catalog lookup

The initial automatic matcher should be conservative about which imports justify a catalog request.

A local Album group should generally have:

* a usable Album title;
* usable Album artist information, or a sufficiently consistent artist credit across its Tracks;
* at least one usable Track title.

Track numbers, durations, dates, and multiple consistent Tracks strengthen the match but are not mandatory prerequisites for attempting a lookup.

Music with severely incomplete, placeholder, contradictory, or obviously unusable tags should be imported normally and skipped by automatic catalog lookup.

#### Automatic acceptance

The initial implementation should not rely on a single opaque numeric confidence threshold as the sole basis for accepting matches.

Candidate ranking may use transient scores internally, but automatic acceptance should be based on understandable structured evidence.

Useful evidence includes:

* Album title agreement;
* Album artist-credit agreement;
* original release year/date compatibility;
* local Track titles matching catalog Tracks;
* Track-position agreement;
* duration compatibility;
* multiple local Tracks consistently agreeing with the same candidate;
* embedded external identifiers where available.

A complete Album is not required.

Several partial Tracks that consistently align with one catalog Album may provide stronger evidence than a single isolated Track.

When multiple plausible Album candidates remain, the matcher must not silently choose one merely because it ranks slightly higher.

Ambiguous candidates should remain unmatched until the user explicitly chooses a match or better evidence becomes available.

#### Album, Release, and Track certainty

Matching certainty may exist at different levels.

The application may confidently establish:

`local Album -> external Album identity`

without knowing:

`local Release -> exact external Release identity`

This is expected for local files because ordinary tags often identify the friendly Album but do not identify a specific regional, physical, digital, or reissue edition.

The application must not invent Release-level certainty merely because one representative external Release is available.

Likewise, a release-specific external Track identifier must not be attached unless the corresponding external Release relationship is sufficiently established.

Recording-level identity may be established independently where appropriate evidence exists, but the matcher should remain conservative about distinctions such as remixes, live versions, edits, remasters, or alternate recordings.

The architecture must support partially enriched state, including:

* Album identity known, Release identity unknown;
* Recording identity known, exact release-specific Track identity unknown;
* no external identity at all.

These are normal states rather than errors.

#### Existing-library matching

Existing library entities should be preferred when imported metadata clearly corresponds to music already known to the application.

For example, if a source-less catalog Track already exists and a local file can be confidently associated with it, the local file should be attached as a PlayableSource to that existing Track rather than creating a duplicate Track.

Existing-library matching should use the same structured evidence principles as catalog matching, but can rely on already-persisted Album, Release, Track, and external-identity information.

The first local-only implementation runs once per `ImportReleaseRequest` group,
inside the import transaction, before creating entities. Eligibility requires a
usable Album title, at least one tagged Track title (not a filename fallback),
and usable Album credits or consistent Track artists across the group. Present
Album titles and Album-artist observations must agree. Empty and obvious
`Unknown Album`/`Unknown Artist`/`Unknown Track` placeholders decline matching.

Comparison folds whitespace and Unicode lowercase only; punctuation, accents,
join phrases and version qualifiers remain significant. Equal normalized Album
title and artist-credit display generate candidates; library uniqueness alone is
not acceptance evidence. For each candidate, compare tagged Track titles at known
positive disc and Track numbers against effective Track titles within each stored
Release. At least one exact normalized positional/title correspondence is required,
and every overlapping position must agree unambiguously within that same Release.
Absent positions provide no evidence; evidence is never assembled across editions.
Exactly one supported Album is accepted. No supported Album or multiple supported
Albums create a new local Album. Year, durations and total Track count are not used.
A one-Track partial import can qualify; non-overlapping partial imports deliberately
create another Album. Missing disc/Track numbers are not guessed.

Migration `0005_album_matching.sql` adds reconstructible `match_title` and
`match_artist_credit` columns to `album_application_metadata`, with covering
index `album_matching_lookup(match_title, match_artist_credit, album_id)`.
The bounded candidate query reads at most 65 IDs and declines groups larger than
64 rather than accepting from a truncated set. Verification uses `release_album`,
`track_release_order` and the effective Track metadata primary-key index, once per
candidate Album; it stops after finding two supported Albums. Rust backfills the same
Unicode normalization as new imports; schema changes, backfill and version bump
share one transaction. Original metadata and IDs are unchanged. Creation and
catalog credit installation maintain the keys; future Album/artist editing must
also refresh them.

An accepted Album still receives a **new local Release and new Tracks**: friendly
metadata does not prove edition identity, even if only one Release is stored.
No external identities are copied and source-less catalog Tracks/membership are
untouched. The tag extractor now reads AlbumArtist through Lofty's existing
accessor, but does not expose embedded MBIDs or ISRCs. Accordingly, cross-source
Track reuse is deferred; rescans retain the existing durable source association.
Repeated explicit import of an associated source still fails atomically, while
normal scans omit it from discovery candidates.

All source validation, matching, creation, source association and membership use
one immediate SQLite transaction with no filesystem reads or network calls.
The returned Release ID resolves its Album via `album_for_release` for the
post-import matcher described below. The filesystem importer does not dispatch
external work.

#### Matching throughput

Automatic matching must not serialize local import behind external catalog requests.

For large imports, the library should become usable before catalog enrichment completes.

Imported Tracks should first be grouped into local Album candidates and matching work should operate once per Album group where possible, rather than once per Track.

External catalog matching may proceed through a bounded application-owned queue governed by the provider's request policy.

The matching queue must not be embedded into filesystem scanning logic, and catalog failure must not invalidate already-imported local music.

### Match decisions and provenance

Candidate scoring and match evidence may exist transiently while a match is being evaluated. The initial architecture does not require a durable generic numeric match-confidence field.

Once an association has been accepted, the durable useful information is the external identity attached to the library entity.

The system may later distinguish associations established through direct catalog import, automatic matching, or explicit user confirmation if product behavior requires that provenance, but a generalized persistent match-evidence system is deferred.

### Catalog integration boundary

MusicBrainz access belongs behind a catalog/provider boundary rather than in the domain model or UI.

The domain and application layers must not depend on MusicBrainz-specific API types.

Catalog integration should convert provider responses into application-owned data before persistence. This keeps catalog access replaceable and allows later providers to participate in matching without redefining Track, Release, or PlayableSource.

Use of the public MusicBrainz service must account for its API identification and rate-limiting requirements. Catalog access should therefore be explicit, cacheable where appropriate, and avoid unnecessary repeated requests.

### Album and Release model

Album is the user-facing, provider-neutral grouping identity.

An Album represents the musical release concept a user normally thinks of and searches for, such as "Demon Days" by Gorillaz, without requiring the user to choose a particular physical, regional, or digital edition.

External provider concepts may be associated with an Album, including:

* MusicBrainz Release Group;
* Spotify Album;
* Apple Music Album;
* album-level metadata derived from local files.

A Release represents a specific edition or manifestation of an Album, such as a particular MusicBrainz Release with a country, date, format, barcode, or edition-specific tracklist.

Release specificity is normally an implementation detail and should not be required for ordinary library interaction. The application may select a suitable concrete Release internally when edition-specific information such as a tracklist is required.

Users may explicitly choose or inspect a specific Release when edition differences matter, such as deluxe editions, bonus tracks, regional variants, reissues, or alternate tracklists.

The expected relationship is:

`Album -> one or more Releases -> release-specific Tracks`

Album identity must remain application-owned and provider-neutral. A MusicBrainz Release Group ID, Spotify Album ID, Apple Music Album ID, or other external identifier may identify the corresponding concept in a particular provider but must not become the application's primary identity.

Album metadata used for ordinary display should likewise be application-owned. Provider and local-file metadata remain observations that may contribute to effective Album metadata rather than forcing provider-specific edition names into the normal user interface.

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
* Deliberate cross-source Album reconciliation.
* Automatic duplicate identity.
* Automatic move identity.
* External-provider authority.
* A specific playback engine.
* A specific frontend.
* A specific public skin format.
* A comprehensive artist-role taxonomy.
* A mandatory local-file workflow.

If implementation begins to depend on one of these decisions, surface the assumption before encoding it.

### Album persistence and catalog import

Migration `0004_albums.sql` adds `album`, `album_application_metadata`, ordered
`album_artist_credit` and `album_external_identity`. Album IDs are random opaque
application IDs. Release has a non-null Album foreign key and an `(album_id, id)`
index. Album deletion is restricted while Releases reference it; deleting a Release
does not delete its Album or sibling editions. Membership remains Track-level.

Backfill gives every existing Release its own Album, copying available friendly
Release metadata and credits without matching or merging. The parent-table rebuild
preserves Release IDs/timestamps and all child data, verifies foreign keys before
commit, and reenables enforcement afterward. Release Group identities from the
unshipped prototype move to their respective Albums. Shared legacy identities
remain shared rather than triggering an implicit merge.

Album identities follow the existing generic cardinality: composite primary key
`(album_id, provider, kind, external_id)` prevents duplicate associations; a non-unique
`(provider, kind, external_id)` index supports plural reverse resolution. Library/Store
expose attach/list/resolve Album identity operations and `album_for_release` for
friendly metadata. Generic identity storage imposes no provider-specific uniqueness.

`catalog::AlbumCandidate` represents provider-neutral discovery. `CatalogProvider`
searches Albums, browses concrete editions and looks up a complete Release carrying
its parent Album metadata. Private MusicBrainz JSON becomes these application values.
MusicBrainz Release Group maps to Album, Release to Release, and Track/Recording/ISRC
identities to release-specific Tracks. No durable Recording or provider-specific
Release Group entity is added.

`CatalogSession::add_album` lazily requests representative candidates and selects a provisional
representative for its tracklist. Among candidates with nonzero media/track counts,
the pure ranking prefers Official status, the smallest year distance from the
Album's first date, absence of obvious deluxe/expanded/anniversary/bonus wording,
fewer discs, then date and external ID. Unknown dates rank last within status.
It does not infer a canonical edition or automatically skip failed lookups.

Album search returns ten results per page, with existing explicit next-page access.
The [search/detail audit](catalog-endpoint-audit.md) supports bounding initial
payloads; it does not establish consistently lower public-service latency. Eager
ISRCs and the returned Release Group identity check remain in detailed lookup.

The normal MusicBrainz search-and-add path uses three requests: Album search,
one candidate browse (`status=official&inc=media&limit=100`), and the unchanged
full Release lookup. Media summaries preserve nonempty-track checks and disc-count
ranking; labels and release credits are only requested for explicit Editions.
An empty Official page triggers one unfiltered media-only browse; HTTP failures
are errors, not evidence that no Official edition exists. The request-shape
[measurements](catalog-latency-audit.md#request-shape-follow-up) support smaller
payloads but still show unpredictable service latency.

Only Add Album or Editions triggers browse; selecting a result does not. Ranking
remains bounded to the first page, not a guarantee about every edition. MusicBrainz
supports [status filtering and paging](https://musicbrainz.org/doc/MusicBrainz_API#Browse)
with a limit up to 100, but may shorten Release pages under its Track-count cap.
Next offsets use actual returned counts. Release Group lookup with `inc=releases`
is not a replacement: linked subqueries are limited to 25 entities.

Editions retains unfiltered rich metadata and explicit pagination. A session caches
four rich edition pages, one default candidate page and one complete Release.
Default candidates never masquerade as rich edition data. A complete all-Official
rich page can also serve Add Album; incomplete or mixed-status rich pages do not
replace the provider's filtered first page. There is no persistent cache or
background refresh.

`Library::add_catalog_release` atomically resolves/creates Album and the selected
Release, ordered Tracks, credits, identities and Track memberships. It creates no
sources. Another edition with the same Album identity shares the Album. Re-adding
an existing edition restores membership without replacing metadata or Tracks.
An exact existing Release can supply its parent Album when that Album has not yet
received the catalog identity. Ambiguous identity owners or conflicting Album/
Release associations produce an error instead of silent merging or reassignment.
All persistence, including Album creation and search materialization, rolls back
on failure. HTTP completes before the transaction starts.

Migration `0003_artist_credit_join_phrases.sql` preserves exact ordered join phrases;
NULL retains legacy comma-separated display. Initial Album metadata stores title,
year and credited names independently of edition metadata. Ordinary Track result
materialization uses the Album title (the existing `release_title` result/FTS column
name is retained for compatibility); the search execution strategy is unchanged.
Full dates, labels/barcodes and printed track-number strings remain discovery data.
Provider metadata refresh and sparse Album overrides remain deferred. Strong
Artist-identity consolidation is described below.

The MusicBrainz adapter retains its identifying User-Agent and process-wide
one-request-per-second gate under the [service rules](https://musicbrainz.org/doc/MusicBrainz_API/Rate_Limiting).
The diagnostic owns one catalog worker for HTTP/rate waits and uses a queued Qt
callback to apply results and notify properties. SQLite import remains synchronous
and short. Shutdown joins the worker and may wait for its bounded request timeout;
cancellation is deferred. See the [adapter guide](../adapters/musicbrainz/README.md).

Transient MusicBrainz 503 responses are handled inside the HTTP adapter: at most
two additional attempts, with 2s/4s backoff or a valid Retry-After. Every attempt
uses the same process-wide rate gate and retains its 30-second HTTP timeout.
Server delays over one minute stop automatic retry rather than being shortened.
Other failures are not retried. The catalog worker still emits only one final
result, so the diagnostic remains pending during retries; no new application or
Qt notification boundary is needed. See the [retry policy](../adapters/musicbrainz/README.md#bounded-503-retry).

### Implemented post-import Album identity enrichment

`AlbumMatcher::after_import` accepts committed import results, resolves/deduplicates
Album IDs and prepares local eligibility on the application owner thread. It is
separate from `Library::import_release` and filesystem scanning. One owned worker
per matcher serializes searches; session-local outcomes coalesce automatic attempts
and pending manual requests. `AutoMatchPolicy` defaults on; disabling dispatch does
not affect import or `match_album` manual retries. No match-state tables or jobs are
persisted. Dropping the matcher cancels queued work and joins the bounded in-flight
provider request; cancellation of an active HTTP call remains deferred.

Eligibility requires an unidentified Album with usable title/artist evidence and
at least one usable tagged local Track title. The first automatic Artist path
requires exactly one Album Artist credit with no trailing join phrase; complex
credits remain ArtistAmbiguous rather than being flattened. Local-first matching
and its Track corroboration rule are unchanged.

Migration 0006 adds `artist_external_identity(artist_id, provider, kind, external_id)`
with an entity-local primary key, cascading Artist foreign key and non-unique
reverse-lookup index, like the other external identity tables. Opaque Artist IDs
remain authoritative; attach/list/reverse-resolve APIs permit shared identities.

The matcher reuses one stored `musicbrainz | artist` identity or makes one Artist
search with `(artist:"name" OR alias:"name")`. Search aliases are converted into
application-owned names without per-candidate lookups. Automatic acceptance uses
conservative Unicode lowercase/whitespace equality on a complete page. Multiple
identities with exact primary or alias evidence remain ambiguous. A unique primary
match wins absent competing exact evidence. A unique alias match is vetoed by any
other returned Artist whose nonempty primary/alias name is within one Unicode
edit. Close names are never positive identity evidence;
scores cannot resolve ambiguity. Incomplete pages remain unaccepted.
See the [MusicBrainz Artist search fields](https://musicbrainz.org/doc/MusicBrainz_API/Search/ArtistSearch)
for primary/alias discovery; local comparison retains diacritics and punctuation
even where the search index folds them.

Before requesting manual resolution of an ambiguous Artist page, the matcher
makes one bounded Release Group search with `(arid:A OR arid:B ...)` and the
existing Album-title terms. The [MusicBrainz Release Group search fields](https://musicbrainz.org/doc/MusicBrainz_API/Search/ReleaseGroupSearch)
support this Artist scope. Only previously plausible Artists participate; a close-name
alias veto candidate may block acceptance but cannot win without exact primary/alias
evidence. Automatic resolution requires exactly one supported Artist and one
unambiguous Album under the normal comparison rules. A truncated Artist page may
still supply these bounded plausible candidates; it cannot independently establish
Artist uniqueness. An incomplete Album corroboration page or competing supported
Artists remain manual. Album evidence never introduces another Artist. Neither
result ordering nor search score establishes identity.
This pair-dependent result is not cached by Artist name. Unresolved picker candidates
are ordered by Album support, then MBID, without selecting one automatically.

For an individually resolved Artist, Album discovery uses `arid` plus the local title,
controlled trailing variants and bounded edit tokens in one request. Returned
Artist MBIDs are checked again. Acceptance tiers are exact full title, unique
structural variant, then unique close title. Literal EP/LP/CD may be part of the real
title: test complete titles first and only derive fallback variants if none match.
Variants strip one trailing EP, LP, CD,
CD1/CD 1, CD2/CD 2, Disc 1/2, 2CD or 2xCD, optionally enclosed in parentheses/brackets
or preceded by a spaced dash. EP requires candidate primary type EP; missing type
is not evidence. LP similarly requires Album primary type. Decorations inside words/titles and semantic qualifiers are not
removed. Recognized version words in multi-word titles must also agree before
edit comparison; a standalone title such as Acoustic can still match Acoustics.

Normal/alias-resolved Artists allow one Album edit (both titles >=5 Unicode
scalars). Explicit Use Artist additionally allows two edits (both >=8), only for
that Album and confirmed identity during this matcher session. This confirmation
survives deferred retries; no confidence state is persisted. All candidates within
the chosen tier count toward ambiguity, not just the closest/highest-scored one.
No fuzzy structural-variant chaining is performed. Local titles/credited names and
Release/Track identities remain unchanged. Existing ten-result bounds, serial
worker, process-wide rate gate, retries and cooldown recovery remain unchanged.

The completion transaction revalidates Artist name/identity and Album metadata.
It stores the independently resolved Artist MBID even if the later Album request
failed or was ambiguous, then attaches an accepted Release Group identity to Album.
Neither local names nor Release/Track identities, sources or membership change.
Successful replies also retain application-owned provider title/Artist presentation
in session memory for diagnostics. The MusicBrainz search credit's canonical Artist
name is used when supplied, otherwise its credited name. This does not add requests,
change acceptance evidence, or persist provider display metadata.
States distinguish Pending, Matched, MatchedClose, AlreadyMatched, Skipped,
ArtistAmbiguous, AlbumAmbiguous, NoConfidentMatch and Error. Album-specific errors do not stop later queued Albums. Exhausted provider
unavailability pauses dispatch as described below. Explicit `select_artist` chooses a retained candidate,
stores its identity and retries within that Artist, using the same canonical
identity resolution as automatic completion. Conflicting stored MusicBrainz
Artist identities are rejected. There is no manual Album selector.

### Canonical Artist identity and credited presentation

Artist names are not identity. Text-only creation still allocates distinct Artists;
no name lookup or fuzzy comparison merges them. An independently established exact
`musicbrainz | artist | MBID` permits canonical reuse. Catalog credits retain an
optional opaque external Artist identity; MusicBrainz credits use it before creating
Artists across Album, Release and Track credits. Other identity namespaces remain
opaque and do not trigger automatic consolidation.

An existing MBID owner is preferred. If multiple owners already exist, binary
internal-ID ordering chooses the canonical owner deterministically. Matcher
completion atomically consolidates its resolved Artist into that owner. Queued
replies revalidate the current Album credit and require the same established MBID
when an earlier completion has replaced their Artist ID. Newly created text-only
Artists still require independent resolution; a matching name is never a reuse key.

`Library::merge_artist(source, canonical)` is an explicit atomic reassignment
primitive whose caller must establish identity. It rewrites all Album, Release and
Track Artist-credit foreign keys, unions external identities idempotently, then
deletes the source Artist. Positions, roles, repeated Artist positions and join
phrases remain intact. Different MusicBrainz Artist MBIDs across the participants
produce `ArtistIdentityConflict` and roll back; other opaque identities are retained
without assuming their kinds are exclusive. Missing source is an idempotent no-op
provided the canonical Artist exists.

Migration 0007 backfills each credit's `credited_name` from its former Artist name.
New credits store their own presentation; legacy NULL falls back to Artist name
and is frozen before reassignment. Canonical names never replace credited display
text. Consequently consolidation needs no effective-metadata/FTS rebuild or Album
matching-key refresh. Reverse identity lookup and all three credit reassignments
use existing indexes; no migration changes their cardinality or adds indexes.

### Post-import provider outage circuit

The matcher owner keeps the pending FIFO and dispatches one item to its existing
worker at a time. Typed `CatalogError::ServiceUnavailable` (HTTP 503 after the
unchanged bounded retry policy), `Timeout` (any configured ureq timeout, including
body reads), or `TransportUnavailable` (I/O, DNS or connection failure) suspends
dispatch. Error text is diagnostic only; no string/header heuristic controls the
circuit. Retry-After is retained on the final 503 error. Ordinary 4xx, malformed
responses and semantic ambiguity/no-match do not open it.

The failed Album is deferred at the front; untouched and newly imported Albums
remain deduplicated and pending. Independently resolved Artist identities still
persist. `retry_catalog_matching` permits one logical probe using that front item
and the same worker/rate limiter/retries/timeouts. A responding operation resumes
the FIFO (including semantic no-match/ambiguity or a non-provider request error);
another provider failure preserves the queue and arms the next cooldown.
Per-Album Retry Match queues work while paused; Retry Matching probes immediately.
Use Artist persists the local selection and queues its scoped Album continuation
at the tail without bypassing the circuit. Repeated probe clicks coalesce.

Circuit and queue state are session-only, lost on application exit, and never
enter SQLite or the import transaction. Local music stays usable. This circuit
covers post-import/manual Album matching; explicit catalog browsing/Add Album is
still an independent user action, sharing the existing process-wide HTTP rate
limiter rather than being silently disabled by the matching circuit.

The application matcher now arms automatic cooldowns of 15, 30, 60, then 120
seconds (capped). Final 503 Retry-After accepts seconds or HTTP-date via `httpdate`:
use the larger of the normal cooldown and a positive requested delay clamped to
120 seconds. Zero, malformed, overflowed or past values cannot shorten cooldown.
Wall time is used only to convert a header date; the worker deadline is monotonic.
Any completed non-provider-failure response resets the progression, including
semantic ambiguity/no-match and ordinary request errors.

The existing owned worker waits interruptibly on its command channel with a timer
deadline, never sleeps for cooldown and never owns SQLite. Expiry emits a token
through the owner's callback; `cooldown_elapsed` reuses the same single-in-flight
dispatch gate. Manual retry invalidates that token before starting a probe, so an
already queued expiry cannot duplicate it. Failure arms one new timer. Shutdown
closes the channel and joins the worker without waiting for cooldown; an already
active HTTP operation retains its existing bounded shutdown behavior. Callers of
`AlbumMatcher::new` route both completion and retry callbacks to the application
owner; Qt is only the callback transport, not the timer owner.
