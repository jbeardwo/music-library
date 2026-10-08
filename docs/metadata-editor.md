# Metadata inspection and editing

`Library::inspect_metadata`, `save_metadata`, and `metadata_track_labels` in
`src/metadata.rs` form a UI-independent, offline application boundary. Inspection
reads persisted SQLite observations; neither inspection nor saving calls a provider.
The diagnostic desktop exposes **Metadata…** on canonical Song rows in Songs,
Artist Songs, Genre Songs, Album Songs, and resolved Playlist entries, and on Album
rows/tiles. PlaylistEntry IDs are resolved through the row's canonical Track link;
playback position and selection ordinal never identify the editor target.

## Storage and effective values

Migration 30 adds normalized per-Track and per-Album override records. Track titles
continue to use the existing `track_title_override` table. Other text fields use
nullable columns: NULL is automatic, while an empty optional text value deliberately
blanks the field. Numeric overrides use checked integer columns plus `*_set` flags;
a set flag and NULL value mean explicitly unknown. Unset fields keep following
underlying metadata. Clearing an override removes just that field's manual choice.

Override records have update timestamps, but no history or undo log. Ordinary credit-text editing never
changes Track/Album/Release/Artist IDs, credit relationships, provider identity
associations, membership, PlaylistEntries, queue order, or source ownership.

Effective precedence is a specific Track override, then a shared Album override
for shared fields, then the established automatic resolver. Automatic title/year/
duration/source selection otherwise retains its existing behavior. A shared Album
credit overrides a Track's automatic display credit only when that credit is absent
or equals the original Album credit. Distinct featured credits and explicit Track
credit overrides remain intact. Artist entity names and structured identities are
not renamed or inferred from entered credit text.

The `effective_album_metadata` view overlays display values without changing the
original application metadata. Effective Track metadata remains materialized.
Album title FTS, Album ordering keys, Track FTS, Track ordering metadata, and indexed
Genre membership update transactionally for the affected objects. A metadata save
does not rebuild the entire Library index. Credit text is searchable through bounded
FTS hits, including corrected credits that do not rename an Artist entity.

## Editor fields and provenance

| Target | Editable fields | Read-only context |
|---|---|---|
| Track | Title, artist credit, Genre, year, disc number, Track number | Album title/credit, release type, duration, attached identities |
| Album | Title, shared Album artist credit, Genre, year, release type | Track count, affected files, exact Release and Track evidence, identities |

Track Metadata links to shared Album Metadata instead of silently applying a Track
edit to every Album occurrence. Album editing covers all known Tracks across its
Releases, including currently unsaved occurrences. It never bulk-edits Track titles,
durations, disc numbers, or Track numbers. Dates remain year edits because the
current application metadata schema stores years; persisted provider dates retain
full available precision in evidence cards.

Library fields identify explicit/inherited user overrides or their automatic
origin. Missing values use a dash placeholder. Titles and explicit artist credits
must be nonblank; optional Genre/year/type/number values can be cleared. Input is
trimmed and validated without punctuation, capitalization, or Unicode normalization.
Numbers must be positive and within checked ranges (year 1–9999; numbers 1–99999).

Each local file has its own read-only card, including retained unavailable-source
observations. Album inspection groups these cards and provider Track observations
under each canonical Track, ordered by effective disc and Track number. Each group
starts with its effective Library metadata so source values can be compared for
that Track. Tracks without source evidence still appear. Shared Album/provider and
exact Release evidence follows separately. Provider objects retain their provider,
object kind, ID, and values.
The editor also exposes persisted manual/automatic association snapshots, last
Spotify candidate pages clearly labelled **not a connection**, original application
metadata, and exact Release snapshots. It neither treats missing provider fields
as observations nor presents mixed local values as a single agreed value. Release
type uses the explicit Album override or agreed attached trusted provider evidence;
conflicting provider types remain separate evidence. Artwork is read-only.

Unconnected persisted Spotify candidates have an explicit **Connect this Spotify
candidate** action for their canonical Track. It uses the existing song-resolution
manual confirmation path, performs no provider request, and cannot replace an existing
provider association. Metadata edits must be saved or cancelled before connecting.
This action adds an identity association; source values remain read-only.

**Save**, **Cancel**, and per-field **Use automatic value** operate within a modal
dialog. Opening/closing preserves the underlying view. Saving refreshes bounded
windows around retained identity/viewport anchors and invalidates inactive windows;
sorts, columns, selected Album/playlist context, and the main window remain in place.
Queue labels refresh through one query of affected IDs, without transport/resolver
commands. File writes run away from the Qt thread.

General Library refreshes also retain the Album viewport, selection, and unchanged
page boundaries instead of treating background enrichment as navigation. Removed
or reordered boundaries fall back to the visible canonical identity anchor; paging
look-ahead rows remain outside the 200 displayed rows.

## Local tag write-back

Write-back is unchecked by default. Enabling it reveals every attached file and
requires explicit selection; no file is preselected, including when only one exists.
**Select all** checks the attached files; **Clear selection** unchecks them.
Album Metadata shows Track/file counts and the selected file count. Selection is
validated against canonical attachments before the Library transaction begins.

The existing **Lofty 0.25** dependency writes MP3, FLAC, M4A/MP4, Ogg Vorbis, and Opus.
Other discovery formats are conservatively unsupported for writing. Supported fields
map to title, Track artist, Album title, Album artist, Genre, recording year, disc,
and Track-number tags. Album writes map title/credit to Album tags and never Track
title/number tags. Explicit assignment additionally writes Track Artist for Tracks
assigned to the new Artist; distinct featured Track artists retain their tags. Release type has no supported round-trip tag mapping;
requesting it reports a per-file failure. Clearing a mixed/missing automatic Album
Genre cannot silently erase individual Genres through bulk write-back.

The operation order is validation, one Library/FTS transaction, independent selected
file writes, and short transactions to record normal-reader observations against
existing source IDs. No SQLite write transaction spans a file write. Each write uses
a scanner-ignored `.tmp` sibling copy, Lofty's tag writer, permission preservation, file sync,
normal-reader validation, a concurrent-change check, and atomic replacement. Only
regular writable files are supported. Unsupported/unrelated tags and artwork are
retained by Lofty; the audio is not re-encoded. A failed file leaves the Library edit
saved, with the path and reason shown separately. Other selected files still proceed.

A subsequent scan reads new local observations using the same path/source attachment.
It does not recreate Tracks, re-add membership, or clear overrides. Immediate
read-back stores updated local tag evidence; the scanner may reparse once because
its existing size/mtime observation has changed.

## Reconciliation and limits

Matching-relevant edits mark affected unresolved Spotify review state stale and
update Album matching keys. The existing title-override lifecycle also invalidates
its Album program's unresolved state. Trusted associations and manual exclusions
are preserved. Genre-only edits do not invalidate identity matching. Existing
bounded local/cache reevaluation can read corrected title/Album/credit/year evidence;
credit text is not parsed into new Artist identities. Saving does not initiate a
Spotify or MusicBrainz search.

Deferred: automatic corrections/consensus suggestions, automatic rewriting, fuzzy
Artist merging, Library-wide cleanup, new providers, generic refresh, edit history,
artwork editing, full-date edits, bulk numbering, and more tag-write formats.

See [validation and completion report](metadata-editor-validation.md).

When an explicit Album-title edit matches another known Album's effective title,
Save pauses before database or file changes and asks whether to move the affected
Tracks, keep the Albums separate, or cancel. Matches use the indexed Album title
order, display credit/year/Track and Release counts, and require a chosen
destination. Re-entering an already corrected title also offers this choice.
Genre-only edits never prompt. No identity is inferred from a matching name.

A confirmed move transactionally reassigns the source Album's exact Releases to
the chosen Album. Release and Track IDs, source attachments, membership,
playlists, individual overrides and ordered artist relationships remain intact.
The destination's shared metadata applies; the source Album retains its original
metadata and overrides as an empty entity. Compatible Album provider identities
and evidence remain available at the destination; conflicting trusted Album
identities block the move. Filesystem writes follow the committed database change
and only affect the originally selected source files. Selection and Album context
follow the destination without changing playback. No provider requests run.


## Explicit canonical Artist assignment

**Artist (Library assignment)** appears first in the editor and is separate from
**Displayed Artist credit**. Display credit
continues to be a sparse override. Assignment chooses an existing canonical ID or
creates/reuses one exact trimmed name with a generated UUID. Commas, ampersands,
slashes, semicolons and `and` are never parsed. Lookup uses the existing indexed
name order with at most 20 results, including IDs to distinguish duplicate names.
Multiple exact-name entities require explicit ID selection; no arbitrary owner,
fuzzy match, equivalence, provider Artist reassociation or merge is inferred.
A lightweight confirmation describes the grouping change before Save. Successful
saves carry a revision counter; only a new save resets the draft. Autocomplete
notifications cannot clear another assignment entered in the still-open dialog.

Track assignment replaces its first ordered Artist relationship, leaving siblings,
Album and Release relationships intact. A Track inheriting its Artist from its
Release/Album receives its own primary relationship. Album assignment replaces
its first relationship and the old Artist's relationships within its Releases and
Tracks. Legacy imports may have separate IDs with the same exact primary name;
those primary relationships follow this explicit bulk correction too. Other
primary names and secondary Artists remain intact. Missing relationships that
inherit the corrected Album/Release receive explicit primary rows. Existing custom
credited names, roles, ordering and sparse overrides survive; default credited
names follow the new Artist. Original local and provider observations stay visible.

Assignment, targeted effective metadata/FTS refresh, Album ordering/matching keys
and unresolved Spotify staleness commit together. Existing lifecycle triggers
invalidate affected Album context; trusted connections and exclusions remain.
MusicBrainz preparation reads the corrected Artist ID and matching keys through
its existing snapshot machinery. Saving performs no provider calls.
Track, Release, Album, source and PlaylistEntry IDs, membership, queue and ignored
Track preferences remain stable. Hidden Artist preferences stay on their original
Artist ID; new Artists begin with no hidden preference. Empty Artist visibility
uses existing membership-derived browsing.

Write-back remains opt-in with explicit file selection and per-file failure results.
Assignment writes Album Artist and eligible Track Artist tags using the existing
atomic-copy writer; plain displayed-credit edits retain their original tag mappings.
Rescanning updates observations against existing source and Track IDs.
