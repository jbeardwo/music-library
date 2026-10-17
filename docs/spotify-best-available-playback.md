# Best-available Spotify Track playback

Canonical Release identity and Spotify playable Track identity are separate. A
Track occurrence may use Spotify playback from a single, remaster or another
release without moving the canonical Track, changing Release metadata, or
asserting Album/Recording equivalence. Evaluation version is 15; Album discovery
strategy is 6. See the [focused robustness audit](spotify-focused-robustness-audit.md)
for provider-family, shared-title and fresh-database evidence.

## Decision and persistence

The previous Track feasibility gate rejected provider Album title/type, program
size and position differences before considering independent playback. The
strict Release-aware assessment remains. A separate `assess_playable` decision
is used only when that assessment supplies no unique winner.

Playback eligibility requires a shared established Artist identity, exact musical
title or a narrowly recognized terminal version annotation, compatible supplied
durations (Spotify's existing 10-second tolerance), and no contradictory trusted
Recording identity/ISRC. A single established Release Artist can corroborate an
otherwise identity-free matching Track credit. Trusted conflicting Track identities
and existing associations still prevent automatic replacement. Provider Release
positions, size and type remain visible warnings for independent playable-source
selection; they do not alone veto a uniquely sensible song. Canonical Album identity
retains its separate structural gates.

Additional provider credits and their ordering do not invalidate the match when
all known credits are present. Replacement collaborators remain conflicts. Raw
credits and titles are retained. Version annotations include Live, Acoustic, Demo,
Remaster, Remix and Radio Edit, with narrowly permitted year/version words. Other
subtitles and arbitrary prefixes are not removed. This comparison never establishes
Recording identity.

The full bounded candidate set is evaluated. Exact Track title precedes a version
alternative; equivalent credits precede extra credits. Comparable distinct versions
require review. Known same Recording/ISRC, matching version/title and credits, and
near-identical duration can collapse duplicate occurrences deterministically by
Spotify Track ID. Album-program wrappers already proven equivalent by the existing
complete-program matcher contribute only their preferred representation.

Acceptance inserts the Spotify Track occurrence and duration observation. It does
not insert an Album/Release equivalence. The retained candidate page includes the
provider Album identity and raw version metadata. Diagnostics say **Accepted as best
available Spotify Track**, show canonical/provider Album values, and label contextual
release differences as warnings. Existing trusted manual decisions remain authoritative.

## Bounded retry and quota

Retry runs existing cached Album/program and Track reconciliation first. Retained
program occurrences remain available even when the Album identity decision is rejected,
provided the established Artist passes. Candidate navigation stays cache-only.

After the whole Album batch finishes, the playback fallback considers at most two
unresolved Track searches across the batch, not two per Album and not one per Track
on every Album. The existing Spotify query is Artist plus Track, with limit 10 and
no automatic pagination. An incomplete returned page cannot establish uniqueness.
Already connected, excluded, ambiguous cached, and identity-free Artist contexts
are skipped. An error aborts the provider phase immediately. Existing request cache,
30-day TTL and durable Retry-After handling remain in the adapter.

A reconstructible `playable-track-search:1` discovery marker records completed fallback
scopes (Track, normalized title, established Artist evidence). Its indexed key and
30-day lifetime prevent repeated unsuccessful local retries consuming the budget.
Changing the search scope permits new work. This marker is not an identity or user
preference. Retry automatically advances through bounded Album batches and newly enabled local
evidence until its queue is drained. Work outside the two-search global fallback
bound retains individual review/retry actions. Ordinary navigation
never invokes this provider phase.

## Manual correction

The Track connection dialog supports **Replace Spotify association** and **Clear Spotify
connection**. Replacement is an explicit atomic operation guarded by the reviewed
canonical snapshot. It clears only previous Spotify playable claims and persists the
selected Track; no Album identity is inferred. Significant candidate differences remain
visible in the comparison. Advanced partial/split Album mapping and exact-object paste
remain available. Clearing and replacement preserve non-Spotify identities and
canonical Release/Track IDs.

## Real Giraffes? Giraffes! validation

Validated with a disposable backup of `/tmp/music-library-ui-test.sqlite`, retained
exact Spotify object responses, and only the two relevant Spotify associations cleared.
The actual persisted canonical Release is **Spazz Master**. The longer second title
includes **(Masters of the Fucking Universe)**.

| Canonical Track | Canonical Track ID, before and after | Spotify Track | Spotify release |
| --- | --- | --- | --- |
| SugaaaaaSpazzazazz | `009a6eee-3d33-455a-a229-d7cd514c679d` | `0mtQpTLkUcSmh4OcHkgJns` | `5Vn17UWeJ8b1Y7D2cBQF22`, SugaaaaaSpazzazazz |
| Swallowing Light and Sweating Diamonds (Masters of the Fucking Universe) | `ba485b50-fe2f-4fc3-acc2-04502179dc27` | `3Tz6oqzL8RzNMsPV2oDfJX` | `6Jn8usj4TBZdIHxDucX0xj`, Swallowing Light and Sweating Diamonds (Masters of the Fucking Universe) |

Canonical Album ID stays `6ce02256-e91c-4b23-8354-5ee5f6a51baa`; canonical
Release ID stays `164ac943-fee4-46d6-9b25-f818ee388fcf`. Both candidates accept
independently: exact titles, shared established Spotify Artist identity
`5wGMwXlBLMpuJzRpPxNFjq`, compatible durations and no trusted Recording conflict.
The second Track's canonical position 2 versus Spotify single position 1 and each
single's one-Track size are contextual differences, not playback vetoes. No Album
association is inferred. Provider requests: **0**.

Reproduction: run `examples/spotify_playable_split_audit.rs` against a disposable backup
with the two Track IDs above. It replays retained exact objects as complete synthetic
bounded discovery responses. This proves independent automatic acceptance and
persistence, **not** fresh Spotify search coverage/uniqueness. No live quota was spent.
The user's existing manually confirmed associations were not altered.

## Validation and deliberate limits

Focused tests cover split-source persistence/reload, canonical identity preservation,
unique cross-release acceptance, sole remaster/live/acoustic/demo playback, version
warnings and competing versions, evidence ranking independent of order, trusted
Recording/ISRC conflicts, unrelated Artists, extra/reordered credits, replacement
collaborators, duplicate wrappers, cached retry reuse, bounded discovery/error abort,
and explicit correction/clear. Negative titles include arbitrary subtitles, non-Latin
prefixes, featured text, unrelated titles and incomplete candidate pages. Existing
sequence alignment, exact attachment, advanced mapping and quota tests remain.

Live discovery for the Giraffes objects was deliberately deferred: the current cached
objects suffice for replay, but no Track-search page is retained for these manually
attached Tracks. No global transliteration, broad semantic title rewriting, Recording
inference, canonical Album merging, or full-Album Track request fan-out was added.

Final checks: core **462 passed / 18 opt-in ignored**; Spotify adapter **69 passed /
3 opt-in ignored**, plus its live integration test ignored; offline QML Album review
(including rapid target changes) passed; Qt retry tests **3 passed**; strict Clippy
passed for core, Spotify adapter and Qt all-targets with the GStreamer feature;
formatting for all three manifests and `git diff --check` passed. Existing headless
Qt graphics/timer warnings did not fail assertions. No live provider requests and
no commits were made. A new 200k timing run was not performed; new discovery work
is bounded to two Track contexts and the marker query uses the existing primary-key
index rather than scanning the library.

## Sole remaster/reissue Album programs

A remaster/reissue title remains meaningful provider evidence. Its literal base may
make the primary Artist-scoped search useful without invoking a broader alternate.
This is discovery correspondence, not automatic release identity.

The existing sole-edition fallback now also supplies playback through the shared
program-to-Track source policy. It requires an established Artist, a bounded complete
candidate page with one Artist-compatible representation, an explicit corresponding
release title, a complete provider program with at least three independent anchors
and at least 80% canonical coverage, unique correspondence, compatible durations,
and no trusted Recording conflict on any attached Track. Live/Acoustic/Demo qualifiers
are not release-edition equivalences. Comparable editions stay reviewable; an available
matching original is preferred. One meaningful unmatched Track can stay unresolved
while the strongly corroborated remainder connects.

Both cached reconciliation and the general Album completion boundary preserve canonical
Album/Release identities and metadata. Only corresponding Spotify Track occurrences
and provider observations are persisted. Original title/year, Track IDs, and membership
remain unchanged. Raw provider title/year remain in retained candidate evidence, with
edition and differing year warnings. A local file year is explicitly a source observation
when the canonical year is unset. Accepted program explanations survive subsequent
Album review persistence instead of becoming the misleading “Album needs review”.

See [the cache-only Critical Meat audit](spotify-remaster-playback-audit.md). No live
provider request is needed when retained discovery and Track-program pages exist.
