# Library selection and context actions

Every selectable pane supports normal click (one item), Ctrl-click (toggle), and
Shift-click (an inclusive range from its anchor in current display order). Keyboard
arrows also accept these modifiers. Selection uses stable canonical object IDs;
playlist Songs use stable entry IDs so duplicate occurrences remain distinct.
Each pane in each view retains its own selection, anchor and focus for the session.
Selecting an Album or Song does not replace higher-level selections. Escape and
Show all clear the applicable pane's selection.

Artists and Genres union the selected containers. Selected Albums intersect that
union in Songs. Removing an upstream filter preserves downstream selections that
still belong to the resulting scope and prunes those that no longer do. This is a
query against selected IDs, not a check against the visible page: valid off-page
selections survive. Multiple selected playlists display a concatenation in stable
Playlist ID order, with each playlist's persisted integer entry order intact.
Playlist Songs remain blank when no playlist is selected. Playlist order is never
changed by library sort controls.

Right-clicking an already selected item preserves that pane's complete selection.
Right-clicking an unselected item first makes it the pane's sole selection.
Context actions use that pane only; other pane selections do not narrow their
track expansion. An Artist or Genre resolves the saved Tracks represented by that
library container; an Album resolves its saved Track program; Songs resolve only
the selected Track IDs. Containers shared by multiple selected Artists/Genres
produce a union without duplicate Tracks. Playlist containers and playlist Songs
resolve persisted occurrences, including intentional duplicates and unsaved Tracks.
Library containers use the existing applicable Songs/Album sort mode and full
cursor tie-break keys. Playlist containers use stable Playlist ID order and each
persisted entry order. One Track with credits to several selected Artists remains
one library result; duplicate playlist entries remain separate occurrences.

All these item types expose Add to queue and the paged Add to Playlist chooser.
The chooser can create a playlist. Its source is captured when opened, so later
navigation cannot switch the batch. A dedicated background reader resolves the
batch through `track_container::Target`; QML receives neither complete container
metadata nor the complete selection. Library membership is never changed by either
operation. Queue append uses the existing application queue and metadata snapshot.
Playlist append uses one short SQLite transaction on a separate worker connection.

Before append, `playlist::AppendPlan` checks destination membership by canonical
Track ID and reports how many candidate occurrences already exist. One dialog
covers the entire operation: Yes appends the complete requested snapshot; No skips
all candidate identities already present in the destination, preserving the order
of the rest. If No skips everything, no rows or timestamp change. No rechecks the
destination in the write transaction. Duplicate occurrences internal to a source
playlist whose Track is absent from the destination remain intentional occurrences.
Equal titles on different canonical Tracks do not count as duplicates.

Single-Song context Play, double click, and Enter preserve the existing full
current-view program with an exact start. Multiple selected Songs use a selected
program for context Play. Container context Play uses that pane's complete selected
program. Selecting, sorting, paging or editing a playlist never starts playback or
changes an existing queue snapshot. Playlist entry activation preserves exact
occurrence identity. Library removal, playlist deletion and entry removal also
use the right-clicked pane's selection. Rename and adjacent Move up/down remain
single-item commands; they are disabled for multi-item selections. A move targets
the entry's actual source playlist when several playlists are selected.

## Bounds and validation

Only 200 rows and at most 200 visible selected IDs per pane go to QML, alongside a
total selection count. Selection changes do not rebuild unchanged row delegates
or schedule artwork again. Cross-page ranges resolve IDs on a background reader.
Pruning selections larger than 1,000 IDs also runs there; stale range/prune replies
cannot replace newer selections or another view's state. Track expansion, duplicate
checking and batch writes run off the Qt thread. Single-Track queue append retains
its small synchronous indexed path. No migrations or persistent UI state are added.

Focused real SQLite tests cover union/intersection filtering, direct ID pruning,
both alphabetical range directions and Album ranges crossing 200-row pages,
container expansion, empty selections, playlist occurrence order, canonical
duplicate counts, Yes/No, and saved-membership independence. The offscreen QML test
clicks shipped delegates with Ctrl/Shift, checks independent pane/view selections,
right-click targeting, every container's queue/playlist actions, one aggregate
confirmation and actual Yes/No buttons. The deterministic 200k QML audit includes
cross-page reverse Songs and playlist ranges, bounded selection projections,
retained Album delegates, and queue preservation during navigation/resize.

`selection_performance` runs against a disposable deterministic 200k SQLite copy.
Large union filters use bounded 1,001-credit/observation probes to select indexed
streaming; small unions gather indexed IDs. Existing single-filter paths are
retained. Indexed Artist credit, genre-name, source association, playlist order
and reverse Track membership lookups serve the new operations.

Local release-build measurements (first + second page, 201 rows each): Artist
union Songs 11–14ms, Artist union Albums 13–14ms, Genre union Songs 16ms, and Genre
plus selected Albums about 1.3ms. A 20,360-Track aggregate snapshot took 169ms;
ID pruning took 98ms off the UI thread; atomic batch append took 59ms; duplicate
preview plus skip-all took 17ms. Existing 200k browsing diagnostics and the bounded
QML resize/scroll audit also pass. These are measurements, not cross-machine
budgets or native-desktop/Windows runtime guarantees.


Final validation passed: full core suite; MusicBrainz (20), Spotify (37), and
GStreamer (6) adapter tests; all-feature offscreen QML suite (14), including the
actual multi-selection/action test; opt-in 200k Songs/playlist paging and resize
audit; formatting; strict Clippy for core, QML and all adapter crates; and
`git diff --check`. Live-provider and native-desktop audits remain opt-in.
No git commit was made.

Album selection reveals the interacted tile only when it is outside the viewport.
The ListView current index identifies a Flow section, so automatic highlight
scrolling and unconditional section positioning are disabled. Ctrl toggles of
visible tiles leave the viewport stable; keyboard navigation still reveals tiles.

Songs in Album ordering use native inline ListView sections keyed by canonical
Album ID from the existing browse cursor. Each contiguous group has an album-name
header, including a continuation header at the start of a new bounded page.
Same-title Albums remain separate. Headers are not song records or action targets;
selection ranges, playback and context actions continue to use logical song IDs.
The album name is omitted from individual song subtitles in this mode, while
multi-disc numbering remains unchanged. Alphabetical Songs and playlist order
remain flat. QML still receives at most 200 song rows and requires no extra query
or full-result materialization for grouping.

Follow-up validation covers visible Album Ctrl toggles, canonical group boundaries,
same-title Albums, page continuations, flat alphabetical sorting, and song selection,
queue, playback and playlist actions across headers. The full offscreen suite now
has 15 passing tests; the 200k bounded resize/scroll audit also passes.
