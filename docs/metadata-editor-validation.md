# Metadata editor completion and validation

Implemented without committing. The design and operational details are in
[metadata-editor.md](metadata-editor.md).

1. **Storage:** Migration 30 adds sparse, typed Track and canonical Album override
   tables. Track titles retain the existing title-override table. Optional blanks
   are distinct from absence; numeric overrides use integer values and explicit-set
   flags. Original application observations and provider evidence remain intact.
2. **Precedence:** Specific Track override, applicable shared Album override, then
   the existing automatic selection. Clearing one override reveals current automatic
   evidence without freezing other fields. Artist identity relationships remain intact.
3. **Track UI:** Metadata… is available on canonical Song context menus in Songs,
   Artist, Genre, Album, and resolved Playlist rows. The clicked canonical ID is the
   target. Save, Cancel, per-field automatic reset, and shared Album navigation are
   provided. Editing runs independently of playback.
4. **Album UI:** Album rows and tiles open by canonical Album ID. Shared edits apply
   across known Track occurrences in all attached Releases. The dialog shows Track
   and file counts and explicit file selection.
5. **Track fields:** Title, artist display credit, Genre, year, disc number, and Track
   number. Duration, shared Album metadata, and trusted identities are read-only.
6. **Album fields:** Title, shared artist credit, Genre, year, and release type.
   Track titles, durations, numbering, and distinct featured credits are preserved.
   Shared credit corrections change display text without merging Artist entities.
7. **Provenance:** Editable Library values identify override/automatic provenance.
   Separate read-only local-file and provider cards retain differing values and exact
   Release evidence. Persisted Spotify candidate evidence is labelled as unconnected.
   Opening/saving performs no Spotify or MusicBrainz requests.
8. **Tag library/formats:** Existing Lofty 0.25.1 writes MP3, FLAC, M4A/MP4, Ogg
   Vorbis, and Opus. Supported mappings cover title/artist/Album/Album artist/Genre/
   year/disc/Track number. Release type and other formats report unsupported writes.
   Round-trip preservation tests cover MP3, FLAC, M4A, generated Ogg Vorbis, and Opus;
   real-file validation uses MP3. Ogg/Opus decoded PCM is identical before/after edits.
9. **Multiple sources:** Write-back defaults unchecked. No attached file is selected
   automatically. Only explicitly selected source IDs are written; unattached IDs are
   rejected before the Library transaction.
10. **Partial failures:** Library changes commit first. Files are handled individually
    outside that transaction; paths and errors are returned and displayed separately.
    Read-only, missing, unavailable, unsupported, and mixed-automatic-Genre cases
    preserve the Library correction. Successful files survive other files' failures.
11. **Rescan/identity:** Sibling temporary copies have a scanner-ignored `.tmp` suffix.
    Lofty writes/validates the copy, permissions are retained, and atomic replacement
    follows a concurrent-change check. Normal-reader evidence is stored against the
    existing source ID. Rescan preserves source/Track/Album IDs and overrides.
12. **Search/UI:** Affected effective Track metadata, Genre projection, Track/Album
    FTS, and ordering keys update transactionally. Bounded UI windows preserve identity
    and viewport anchors; active search and queue labels refresh. Queue order,
    resolver state, and transport commands are untouched. Artist navigation keeps its
    canonical entities; corrected credit text finds Tracks/Albums through bounded FTS.
13. **Reconciliation:** Matching-relevant edits stale affected unresolved review state
    and Album matching keys through the existing lifecycle. Trusted associations and
    exclusions remain. Genre-only edits do not stale identity matching. Existing
    local/cache reevaluation consumes the corrected evidence without provider calls.
14. **Real Track validation:** Automated offscreen QML menu/keyboard/save validation
    used disposable copies of Get Disowned. The target differed from current playback.
    Library-only save left file bytes unchanged; restart retained the correction.
    Selected-file write-back was read with the normal reader and rescanned with stable
    canonical IDs. This was automated validation, not a human manual-click session.
15. **Real Album validation:** A disposable 10-file Get Disowned copy received shared
    title/credit corrections, then explicit write-back to all 10 selected files.
    Normal-reader checks verified intended shared tags; Track titles and identities
    remained. GStreamer decoded audio before/after one edited file to identical PCM.
    Original ignored test-media files were never modified.
16. **Performance:** Debug-build measurements on a fresh disposable deterministic
    200,000-Track database, with a 10-Track target Album:

    | Operation | Median | p95 |
    |---|---:|---:|
    | Open Track | 1.667 ms | 1.816 ms |
    | Open Album | 1.570 ms | 1.794 ms |
    | Save Track title + FTS | 5.009 ms | 6.401 ms |
    | Save Album title + affected FTS | 16.203 ms | 20.555 ms |

    Opens were sampled 100 times, saves 30 times. Query plans use Album/Release,
    Track/source, and override indexes. FTS row count remained unchanged. These are
    local measurements, not newly imposed performance budgets.
17. **Tests/checks:** Full offline core suite: 397 passed. Spotify adapter: 50 passed;
    MusicBrainz adapter: 20 passed (local mock HTTP servers, no live provider calls).
    Latest serialized offscreen Qt suite: 28 passed and two import assertions failed
    before the final first-page refresh correction. Both import tests subsequently
    passed individually after that fix. The full suite was not rerun after the final
    correction. All 17 focused metadata
    regression tests passed, as did explicit ignored real-media, generated Ogg/Opus,
    and synthetic/real-QML metadata workflows. Core and Qt Clippy with warnings
    denied, formatting, and diff checks passed. Migration
    downgrade helpers cover the new schema. Three obsolete test expectations were
    reproduced on untouched HEAD and updated to existing credit fallback/duration
    tolerance behavior; those runtime policies were not changed by this feature.
    The Album comparison follow-up additionally verifies canonical per-Track source
    grouping, effective multi-disc/Track ordering, and the real 10-Track QML workflow.
    Later action checks cover Select all/Clear selection on 10 real copied files,
    clicking a persisted Spotify candidate for the canonical target, absent candidate
    rejection, protection of trusted associations, and repeated background refreshes
    retaining the Album viewport and selection.
    Refresh now preserves unchanged window boundaries and paging history, while
    ordering/removal changes fall back to the visible identity anchor. Look-ahead
    results are truncated to 200 displayed rows. Header and queue paging regressions
    verify that subsequent pages and the clicked playback position remain correct.
    First-page refreshes include newly inserted music that sorts ahead of the old
    first row. The Album viewport regression passes with this behavior as well.
18. **Deferred:** Automatic suggestions/consensus, automatic tag correction, fuzzy
    Artist merging, whole-Library cleanup, new providers, generic metadata refresh,
    history/undo, artwork editing, full-date editing, bulk numbering, and additional
    tag-writing formats. Provider identity reassignment remains in its existing UI.

Useful reproduction commands:

```sh
cargo test --offline
cargo test --offline --test metadata_editor
cargo test --offline --test metadata_editor -- --ignored
QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software cargo test --offline --manifest-path tools/qml-diagnostic/Cargo.toml --all-features -- --test-threads=1
QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software cargo test --offline --manifest-path tools/qml-diagnostic/Cargo.toml --all-features metadata_context_actions_use_clicked_canonical_ids_and_keep_queue -- --ignored --test-threads=1
METADATA_REAL_FIXTURE=1 QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software cargo test --offline --manifest-path tools/qml-diagnostic/Cargo.toml --all-features metadata_context_actions_use_clicked_canonical_ids_and_keep_queue -- --ignored --test-threads=1
```

The ignored media tests require local GStreamer tools; the real Album tests also
require the ignored Get Disowned fixture. Performance measurements require a
disposable copy passed to `cargo run --offline --example metadata_performance -- PATH`.

Follow-up: explicit Album regrouping on title collision

Album title saves now use an indexed local lookup and pause for Move Tracks,
Keep separate, or Cancel. Confirmation transactionally moves exact Releases into
the selected existing Album, retaining Track/Release IDs, Track overrides,
playlists and membership. Destination shared metadata applies; conflicting
trusted Album identities are rejected. No network requests or automatic merges.

Validation: metadata editor integration suite passed 18 active tests (2 optional
media tests ignored in that run). New regression covers ordinary rename keeping
Albums separate, explicit same-title resave finding matches, genre-only edits,
confirmed regrouping, stable IDs/membership/playlists/Track overrides, conflicting
identities and compatible identity transfer. Offscreen QML regression passed
Cancel/draft preservation, Keep separate and Move choices, including a second run
using disposable copies of the real Get Disowned Album and local tag write-back.
Core and desktop Clippy with warnings denied passed; desktop build and diff check
passed. Original media and the user's live database were not modified.


## Canonical Artist assignment follow-up

Focused regression coverage separates displayed-credit overrides from canonical
assignment, preserves comma names as one Artist, reuses exact existing IDs, rejects
ambiguous exact names, creates new Artists, preserves featured credits, limits Track
assignment to that Track, and rolls back a failed 10-Track Album update. Coverage
checks stable Track/Release/Album/PlaylistEntry/source identity, membership, ignored
Track and Artist-specific hidden preferences, immediate browse/search, trusted
provider identity preservation and unresolved staleness. Disposable MP3 tests cover
unchecked file bytes, checked Track/Album Artist tags and rescan source reuse.

The real Freshman Year case was tested on a SQLite backup of the real Library,
with 16 known Tracks and Hop Along as its original canonical Artist. The corrected
Artist was created/reused, both Artists remained browse-visible due to the rest of
the Library, and Freshman Year's Album ordering points to Hop Along, Queen Ansleis.
Track IDs, Album ID, attached provider IDs and original audio bytes were preserved.
Assignment took about 29 ms. Write-back was exercised on a copied attached MP3,
never original media. An offscreen Qt test also opens the real Album Metadata action,
types the complete name, accepts its structural confirmation, and checks saved
assignment, stable attachments and queue IDs on another disposable database backup.
The user's live database was not edited.

On the deterministic 200,000-Track fixture (10-Track target), canonical assignment
plus inspection/targeted refresh measured median 40.6 ms, p95 86.2 ms; bounded Artist
lookup measured median 0.104 ms, p95 0.243 ms in a debug build. Existing Album/Track
attachment indexes remain in use. No full-library refresh or provider requests are
introduced. Reproduction uses the updated `metadata_performance` example on a
**disposable** backup.

Validation commands:

```sh
cargo test --offline --test metadata_editor
ARTIST_ASSIGNMENT_REAL_COPY=/tmp/artist-assignment-real.sqlite cargo test --offline --test metadata_editor freshman_year_artist_assignment -- --ignored --nocapture
QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software cargo test --offline --manifest-path tools/qml-diagnostic/Cargo.toml --all-features metadata_context_actions_use_clicked_canonical_ids_and_keep_queue -- --ignored --test-threads=1
ARTIST_ASSIGNMENT_REAL_COPY=/tmp/artist-assignment-ui-real.sqlite QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software cargo test --offline --manifest-path tools/qml-diagnostic/Cargo.toml --all-features freshman_year_artist_assignment_ui -- --ignored --test-threads=1
cargo clippy --offline --all-targets -- -D warnings
cargo clippy --offline --manifest-path tools/qml-diagnostic/Cargo.toml --all-features --all-targets -- -D warnings
```

The broad `cargo test --offline` run stops at the pre-existing
`matching_upgrade_backfills_unicode_credit_display_without_changing_entities`
assertion in `album_matching_migration`: it expects schema version 31 while existing
Hidden Artists migration 32 is installed. This focused change adds no migration.


Follow-up: the user's real database contained only a displayed Album credit override,
with Freshman Year still assigned to Hop Along and no corrected Artist entity.
The assignment control now appears first, labelled **Artist (Library assignment)**.
A repeat-edit regression reproduced a second bug: autocomplete notifications reused
the preceding success message and cleared the next draft. Draft reset now depends
on a successful-save revision, and two consecutive assignments in the same modal
pass. Real-library Qt validation also asserts that the corrected Artist appears in
the refreshed Artists pane. The desktop executable was rebuilt for this follow-up.
