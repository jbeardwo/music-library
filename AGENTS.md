# AGENTS.md

## Purpose

This repository is a local-first music library application.

When making changes, optimize for:

1. Correct product behavior.
2. Performance and responsiveness.
3. Simplicity.
4. Maintainability.
5. Clear data ownership and boundaries.

Do not add complexity unless the current requirements justify it.

## Product Model

Library membership is track-level.

* Tracks are the fundamental saved library entities.
* Albums and artists are derived from library tracks.
* Adding an album means adding all of its tracks.
* Albums and artists do not have an independent saved state.
* Removing individual tracks from an album is allowed.
* An album remains visible while at least one of its tracks remains in the library.

Do not introduce independent artist or album library-membership state unless the product requirements are explicitly changed.

## Performance

Performance is a first-class requirement.

Design and implementation decisions should account for libraries of approximately 200,000 tracks.

Prefer designs that:

* Start quickly.
* Keep interactive operations responsive.
* Avoid unnecessary filesystem access.
* Avoid unnecessary database queries.
* Avoid full-library scans when targeted or incremental work is possible.
* Avoid loading large datasets into memory unnecessarily.
* Avoid recomputing unchanged data.
* Support indexed and incremental access patterns.
* Keep expensive work away from the interactive UI path where practical.

Do not trade substantial performance or resource usage for architectural elegance without a concrete reason.

When introducing potentially expensive behavior, consider what happens at 200,000 tracks.

## Architecture

Keep responsibilities clear and separated.

Likely areas include:

* Domain model.
* Database/storage.
* Filesystem scanning.
* Metadata.
* Search/filtering.
* Playback.
* User interface.

These boundaries are guidelines, not a mandate to create layers or abstractions before they are needed.

Prefer direct, understandable implementations over speculative abstraction.

## Data Ownership

Do not assume that the filesystem is the application's source of truth.

The application may need to reconcile information from:

* Local files and file tags.
* The application database.
* External metadata providers.
* User edits.

Preserve the distinction between these sources when it affects behavior.

User corrections must not be silently destroyed by rescanning files or refreshing external metadata.

Do not make irreversible assumptions about:

* Track identity.
* Album identity.
* Release or edition identity.
* Ownership.
* Metadata authority.

If a change depends on one of these unresolved concepts, surface the assumption rather than silently encoding it.

## Filesystem Scanning

Prefer incremental scanning.

Do not repeatedly process files that can reliably be determined to be unchanged.

Scanning logic should tolerate:

* Added files.
* Removed files.
* Moved files.
* Changed tags.
* Missing files.
* Large libraries.

Avoid designs that require rebuilding the entire library for ordinary changes.

## Database

Use the database for durable application state and efficient querying.

When adding or changing queries:

* Consider required indexes.
* Avoid accidental N+1 query patterns.
* Avoid loading more rows or columns than needed.
* Prefer bounded or incremental result processing for large collections.
* Consider migration implications before changing persistent schemas.

Do not optimize blindly, but do not postpone obvious large-library problems.

## Testing

Add tests for behavior that is important, subtle, or likely to regress.

Prioritize tests around:

* Library membership semantics.
* Metadata precedence and preservation.
* Scanning and reconciliation.
* Database migrations.
* Identity-related behavior.
* Failure and recovery cases.
* Performance-sensitive logic where practical.

Bug fixes should generally include a regression test when one can reasonably reproduce the bug.

Do not create tests that merely duplicate implementation details without protecting useful behavior.

## Failure Cases

Consider failure and disagreement explicitly.

Examples include:

* A file disappears.
* A file moves.
* Tags change.
* Two releases appear nearly identical.
* External metadata conflicts with file tags.
* User-edited metadata conflicts with refreshed metadata.
* A scan is interrupted.
* The library contains 200,000 tracks.

Prefer behavior that preserves user data and allows recovery.

## Dependencies

Keep dependencies intentional.

Before adding a dependency, consider:

* Whether the functionality is substantial enough to justify it.
* Runtime and startup cost.
* Memory cost.
* Maintenance burden.
* Whether the same result can be achieved simply with existing tools.

Do not reimplement mature, difficult functionality solely to avoid a reasonable dependency.

## Working Style

Before making a significant change:

1. Read the relevant existing code and documentation.
2. Understand the current behavior.
3. Identify assumptions the change depends on.
4. Make the smallest coherent change that solves the problem.
5. Run relevant tests and checks.
6. Update documentation when behavior or architecture changes.

Do not rewrite unrelated code while completing a focused task.

Preserve existing behavior unless the task intentionally changes it.

## Documentation

`docs/requirements.md` describes product requirements.

`docs/architecture.md` describes current architectural direction.

When implementation and documentation disagree, do not silently choose one. Determine whether the code or documentation is stale and update the appropriate side.

Keep documentation focused on decisions and constraints that future contributors need to know.

## Decision Making

When several implementations are reasonable, prefer the one that is:

1. Correct.
2. Simpler.
3. More efficient.
4. Easier to change later.

Avoid premature generalization.

Avoid speculative infrastructure for requirements that do not yet exist.

If an architectural decision would be expensive to reverse later, make the assumption explicit before committing to it.
