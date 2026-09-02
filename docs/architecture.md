# Music Library — Architecture

## Goals

The architecture should prioritize:

* Fast startup.
* Responsive interaction.
* Efficient handling of large libraries.
* Minimal unnecessary background work.
* Clear ownership of data and responsibilities.
* Simple components with explicit boundaries.

## Core Domain Model

The library is fundamentally track-based.

### Track

A track is the primary unit of library membership.

A track may reference:

* One album.
* One or more artists.
* One local file.
* External metadata identifiers.
* User-edited metadata.

### Album

An album is derived from the tracks that belong to it.

An album does not need an independent "saved" state.

Adding an album adds all of its tracks.

### Artist

An artist is derived from tracks associated with that artist.

An artist does not need an independent "saved" state.

## Storage

The application should keep durable library state in a structured local database rather than relying on the filesystem as the database.

The filesystem is a source of files and metadata, but application state should not depend on directory layout.

The storage design should support:

* Large libraries.
* Indexed lookup and filtering.
* Incremental updates.
* User metadata overrides.
* External metadata identifiers.
* Missing or moved files.
* Future schema migrations.

## Metadata

Metadata from different sources should remain distinguishable where necessary.

Potential sources include:

* File tags.
* External metadata services.
* Application-derived metadata.
* User edits.

User edits should not be silently overwritten by rescanning files or refreshing external metadata.

## Scanning

Library scanning should be incremental where possible.

A scan should avoid reprocessing unchanged files unnecessarily.

Changes should be detected efficiently and applied to the database without rebuilding the entire library.

## Performance

Operations over the library should avoid unnecessary full-library scans.

Frequently used queries should be backed by appropriate database indexes.

Large result sets should be paginated, streamed, virtualized, or otherwise processed incrementally when appropriate.

Expensive work should not block the interactive UI unless unavoidable.

## Boundaries

The codebase should keep distinct responsibilities separated.

Likely areas include:

* Domain model.
* Database/storage.
* Filesystem scanning.
* Metadata providers.
* Search and filtering.
* Playback.
* User interface.

Exact module boundaries should emerge from implementation needs rather than being over-designed in advance.

## Unresolved Architecture Decisions

These should remain open until we have enough information to make a deliberate choice:

* Programming language and UI framework.
* Database engine.
* Exact album/release identity model.
* Metadata provider strategy.
* Search implementation.
* Playback backend.
* File watching strategy.
* Whether any background service is necessary.
