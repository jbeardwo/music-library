# Music Library — Product Requirements

## Core Model

* Library membership exists at the **track level**.
* Tracks are the fundamental items that are added to or removed from the user's library.
* Albums and artists are not independently "saved" library entities.
* An album appears in the library because one or more library tracks belong to that album.
* An artist appears in the library because one or more library tracks belong to that artist.
* Adding an album means adding **all tracks belonging to that album**.
* Removing individual tracks from an album is allowed; the album remains visible as long as at least one of its tracks remains in the library.

## Performance

Performance is a first-class product requirement.

The application should:

* Start quickly.
* Remain responsive during normal navigation and search.
* Avoid unnecessary background work.
* Avoid loading or recomputing data that is not needed.
* Use memory efficiently.
* Scale well to very large music libraries.
* Treat approximately **200,000 tracks** as an important large-library stress case.
* Prefer simple, efficient implementations over architectural complexity without a demonstrated need.

Performance regressions should be treated as product regressions, not merely implementation details.

## Data and Metadata

The system must be able to represent music independently from the physical organization of files on disk.

The design must account for:

* Local music files.
* Metadata obtained from external sources.
* User-corrected metadata.
* Albums or tracks that may look identical but represent different releases or editions.
* External metadata that may later change or disappear.
* Rescanning local files without unintentionally destroying user corrections.

## Design Principle

Important domain assumptions should be explicit.

In particular, implementation choices should not silently determine product semantics for concepts such as:

* Album identity.
* Release/edition identity.
* Track identity.
* Metadata authority.
* Ownership.
* Library membership.

These concepts should be decided deliberately because changing them after a substantial library has been created may require difficult data migrations.

## Open Product Decisions

The following still require explicit decisions before they should become architectural assumptions:

* Exact definition of album identity.
* How different releases, editions, remasters, and reissues are represented.
* Whether an item can remain in the library when its local file is unavailable.
* Exact meaning of "owned."
* Which source wins when filesystem metadata, external metadata, database state, and user edits disagree.
* How rescanning files interacts with manually corrected metadata.
* How duplicate or apparently identical releases are distinguished.
