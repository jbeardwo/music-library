# MusicBrainz matching and identity audit

Measured 2026-10-18. **No commit; no matcher behavior changes; no original database writes.**

The application knows substantially more Recording identity than exact edition identity. Missing MBIDs do not invalidate its canonical objects. The main gaps are native-script Album discovery, differences between automatic and review program evaluation, and incomplete or differently scoped catalog programs. They are not one fuzzy-string-matching problem.

## Scope, definitions, and reproducibility

There are two real datasets in current project use. Reporting just the older UI-profile number would conceal a different unresolved set in the fresh-import library:

* **Fresh import:** `/tmp/music-library-fresh.sqlite`, schema 36 at snapshot, 930 saved Tracks.
* **UI profile:** `/tmp/music-library-ui-test.sqlite`, schema 33 at snapshot, 942 saved Tracks. Current `Library::open` migrates its disposable working copy to schema 36. Coverage was independently measured before migration.

SQLite's backup API copied each read-only source, including WAL state, to new `/tmp/mb-audit-*.sqlite` files. Integrity checks passed. The network probe used `/tmp/mb-audit-baseline.sqlite`, a separate UI-profile backup; all UI confirmations used further isolated copies. Original files were not opened through mutable application APIs. No audio files changed.

Library scope means saved Track membership, distinct parent Albums, and distinct Artists credited at Track, Release, or Album level for those saved objects. Artists are counted by canonical ID, not deduplicated by display name. Empty or unsaved catalog entities do not inflate denominators. In particular, Album-artist and Track-artist identities can differ; the UI profile has more unconnected credited Artist objects than its Track display alone suggests.

“Trusted” here means a nonempty attached identity in the appropriate external-identity table, matching the application's operational meaning. It does **not** mean this audit independently authenticated every historical association against MusicBrainz. The schema has no universal confidence/status or immutable origin log. Recording coverage uses `track.recording_id → recording_external_identity`, matching `connected_musicbrainz_track`, not historical Track-level IDs or provider display metadata.

Reproducible offline coverage tool: [`tools/audit/musicbrainz_coverage.py`](../tools/audit/musicbrainz_coverage.py). Retained measurements, all unresolved Track titles, candidate identifiers, program decisions, and query inputs are in [`musicbrainz-matching-audit-evidence.json`](musicbrainz-matching-audit-evidence.json). Live/retained probe: [`musicbrainz_matching_audit.rs`](../tools/qml-diagnostic/examples/musicbrainz_matching_audit.rs). It refuses non-audit database paths, does not complete matches, checkpoints results, and supports cache-only `--analyze`.

## 1. Current coverage

Percentages use the entity total in the same column. Partial categories are explicitly defined below rather than silently equating Album and Recording identity.

| Measurement | Fresh import | UI profile |
|---|---:|---:|
| Library Artists | 26 (100%) | 32 (100%) |
| Artists with Artist MBID | 23 (88.46%) | 20 (62.50%) |
| Artists without Artist MBID | 3 (11.54%) | 12 (37.50%) |
| Library Albums | 96 (100%) | 98 (100%) |
| Albums with Release Group MBID | 88 (91.67%) | 89 (90.82%) |
| Albums containing a saved Release with exact Release MBID | 0 (0%) | 1 (1.02%) |
| Albums with Release Group but incomplete saved Recording coverage | 5 (5.21%) | 4 (4.08%) |
| Albums with no trusted Album identity (Release Group) | 8 (8.33%) | 9 (9.18%) |
| Albums with complete saved Recording coverage | 83 (86.46%) | 85 (86.73%) |
| Albums containing at least one Recording gap | 13 (13.54%) | 13 (13.27%) |
| Library Tracks | 930 (100%) | 942 (100%) |
| Tracks with Recording MBID | 831 (89.35%) | 838 (88.96%) |
| Tracks without Recording MBID, but parent has Release Group | 21 (2.26%) | 15 (1.59%) |
| Tracks with neither Recording MBID nor parent Release Group | 78 (8.39%) | 89 (9.45%) |
| All Recording gaps | 99 (10.65%) | 104 (11.04%) |

Release coverage's denominator is all saved Albums, not a claim that every Album should have one exact edition selected. No exact Release is needed for a valid Album or useful Recording identity.

“Only partial evidence” has two useful views:

1. Album identity exists, but Track-level completion is partial: 5 fresh / 4 profile Albums, covering the 21 / 15 gaps above.
2. No Album identity, but the canonical Album Artist has an MBID: all 8 fresh Albums; 8 of the 9 profile Albums. These are Artist-context-only cases, with no embedded MusicBrainz identifiers in their source observations. Business & Pleasure is the ninth profile Album and lacks even that trusted Artist connection. These categories overlap the “no trusted Album identity” row, not the Release Group row.

The historical **838 / 942, 104 gaps across 13 Albums** happens to remain true for the freshly measured original UI profile. It is **not** the fresh-import result, and was not assumed.

## 2. Identity provenance: what is recoverable

Counts below are distinct attached identity associations within saved scope. The current datasets have one Recording MBID per connected saved Track, so these counts also equal connected Track counts. This need not hold for future shared Recordings.

| Evidence of origin | Fresh import | UI profile | Interpretation |
|---|---:|---:|---|
| Release Group associations | 88 | 89 | Total trusted Album IDs |
| Still marked managed embedded acceptance | 46 | 47 | Reliable current ownership from `album_provenance_identity` |
| Release Groups also observed in attached local tags | 46 | 47 | Same associations as managed set here |
| Other Release Groups, origin unreconstructible | 42 | 42 | Search/import/manual/legacy cannot be separated |
| Recording associations | 831 | 838 | Total trusted Recording IDs |
| Still marked managed embedded acceptance | 6 | 6 | Reliable current ownership from `recording_provenance_identity` |
| Recording IDs also present in attached embedded observations | 369 | 373 | Corroboration, not necessarily first-establishment provenance |
| Of those, unmarked/independently confirmed | 363 | 367 | Original tag ownership may have been superseded |
| Recording IDs without matching retained embedded observation | 462 | 465 | Origin unknown, **not proven search successes** |
| `recording_manual_identity` markers | 0 | 0 | Does not exclude manual actions through other APIs |

At least 46/88 (52.27%) fresh and 47/89 (52.81%) profile Album identities are currently managed embedded acceptance. Embedded observations corroborate 369/831 (44.40%) fresh and 373/838 (44.51%) profile Recording connections. This is materially different from claiming that approximately 89% of the library was successfully discovered by the matcher.

Migration 0010 intentionally treats unmarked associations as independent. The `*_identity_confirmation` triggers remove managed markers when an independent insert confirms an association, including an idempotent insert. Therefore **6 managed Recordings does not mean only 6 were originally tagged**. Conversely, a matching tag cannot prove the tag was the original source.

Practical provenance classification:

| Requested origin class | Reliable count available? | Finding |
|---|---|---|
| Embedded local identifier | Yes for current managed ownership | Counts above; broader retained-tag agreement reported separately |
| Normal MusicBrainz search/matching | No historical count | No append-only origin/event field on attached identities |
| Album/program matching | Some corroboration, no reliable original count | `provider_track_association.match_json` stores current program results, can be replaced/deleted; not a complete history |
| Artist alias resolution | No historical count | `trusted_artist_name` retains name provenance; Artist identity attachment does not retain which query/alias first resolved it |
| User/manual association | Partial marker only | Zero legacy manual markers; Connections confirmation does not create this marker, so zero is not proof of no manual work |
| Provider import | Corroborating structure only | Profile For Long Tomorrow has 13 MusicBrainz Track provider observations and the sole exact Release; structure is consistent with catalog ingestion, not a complete event ledger |
| Legacy/migrated | Migration behavior known, count not recoverable | Migration 0008 copied historical Track Recording IDs to application Recordings; no per-row migration-origin flag |
| Other | Unknown remainder | Preserve as unknown rather than allocating it to search |

Artist MBIDs (23 fresh / 20 profile) and exact Release MBIDs (0 / 1) have no comparable managed-origin marker. Retained provider evidence is evidence ownership, not proof of first establishment. No origin was invented.

## 3. All unresolved Albums, grouped by primary failure stage

The union is **14 Albums**, all inspected: 12 common, Business & Pleasure profile-only, New Levels New Devils fresh-only. Coverage below is `connected / saved` in the profile unless identified otherwise. Primary categories allocate each gap once; secondary issues explain why merely fixing discovery will not finish an Album.

| Primary category | Profile gaps | Fresh gaps | Albums |
|---|---:|---:|---|
| Album discovery: English/native-script title mismatch | 40 | 40 | Angle, Dreams, Underwater, Work |
| Album discovery: punctuation/full-title query mismatch | 29 | 29 | Church Gone Wild/Chirpin Hard (24), Hippy Jam Fest (5) |
| Album-title discovery/evaluation (mixed; detailed below) | 9 | 9 | Adebisi Shank EP (4), More Skin… remaster (5) |
| Artist discovered but rejected by name normalization | 11 | 0 | Business & Pleasure |
| One incomplete sampled program blocks complete alternatives | 11 | 11 | The Devil Isn't Red |
| Track title/program semantics differ between automatic and review | 2 | 2 | Freshman Year (1), 100 Onces (1) |
| Trusted Release Group is narrower than canonical saved program | 2 | 2 | Gaze at Blue |
| Featured-title/credit evaluation in automatic path | 0 | 6 | New Levels New Devils |
| **Total** | **104** | **99** | |

These are **current reproducible failure stages**, not reconstructed historical failure events. MusicBrainz does not retain the historical searches/rejections/cooldowns needed to prove exactly what happened at the time each object was imported.

No case established “no plausible MusicBrainz object exists.” No current live probe failed at the provider, and no conflicting attached MBID explains these gaps. Translation/title coverage and sampled Recording ambiguity remain real limits after discovery improves.

### Per-Album evidence and secondary blockers

1. **100 Onces — 100 Onces — 11 Tracks, 10/11 connected.** Trusted Release Group `e08a7838-434f-454b-8136-30c0d93afd63`; sampled Release `736852d6-39d4-4f2c-adca-c19a09b06414` has 11 Tracks. Artist and ten program anchors agree. Missing **OCDC** corresponds to **OC/DC**, approximately 260 seconds. Automatic program and legacy Recording comparison both reject the title. Current inspector also leaves it local-only: its contextual token-boundary branch requires a shared **Spotify** Artist ID, which a MusicBrainz program does not supply. Manual exact-group inspection exposes the Recording; actual QML manual confirmation succeeds. Discovery is not the problem.

2. **Adebisi Shank — The EP of a Band Called Adebisi Shank — 4 Tracks, 0/4.** Artist MBID `f58853a7-916d-4f0b-83fe-3e80b5cda47c` is established. Scoped automatic search already returns the plausible **This Is the EP of a Band Called Adebisi Shank**, Release Group `d00db664-430d-422f-ba41-ee092231dfa1`, alongside the distinct Second EP. `accepted_album` returns `NoConfidentMatch`: “This Is” is not an allowed legacy Album title relation. Manual full Artist/title search puts the correct EP first. Both inspected four-Track Releases support four identified Recordings; review confirmation persists all four without asserting Album/Release identity. **Found, then rejected**; not missing provider data. Spotify's established-Artist boilerplate evaluator has a relevant general concept, but it is not on this MusicBrainz decision path.

3. **Elephant Gym — Angle — 11 Tracks, 0/11.** Trusted Artist `9fdc68d1-98ac-463c-a309-181b58c74aed`; English scoped search and English manual Artist/title search return no candidate. Artist-scoped catalog browsing finds **角度**, group `e0ac3da3-5279-47de-9e42-8d06897962fb`, beyond its first ten broad results. Sampled Releases have 11/12/11 Tracks. Local `Intro - 序`, `Day time - 白日`, etc. meet native-only titles such as `序`, `白日`. Automatic comparison identifies zero. The review shared-span utility requires at least two alphanumeric characters; many native components here are one character, leaving too few independent anchors for its strong program gate. Inspector marks all rows insufficient. The catalog object is a plausible candidate, not proof that translation establishes identity. **Discovery plus weak whole-program bilingual correspondence**, not Latin being canonical. No automatic acceptance is proposed.

4. **Elephant Gym — Dreams — 12 Tracks, 0/12.** Same trusted Artist. English scoped search finds only **More Real Than Dreams (Documentary Original Soundtrack)**, which is correctly rejected; manual full English search finds none. Structured Artist browsing finds **夢境**, group `e5c00483-80f6-440f-8bdc-ac070010eebf`, on the third ten-result page. Two sampled 13-Track Releases carry mostly English titles and an inserted second Shadow; the 12-Track Release is mostly native-script. Current automatic comparison, once explicitly supplied this group, identifies **10 Recordings**, leaving **Dear Humans (JP Version)** and **Gaze at Blue (Album Version)** unresolved against `Japanese ver.` / `2022 ver.`. The program inspector also exposes shifted/insufficient-anchor decisions. Named version differences need evidence, not global suffix stripping.

5. **Elephant Gym — Underwater — 12 Tracks, 0/12.** English scoped/manual searches return none. Artist browsing finds **水底**, group `14165569-4114-43ca-b040-140898f5855d`, on the third page. Three sampled Releases have 13 Tracks, including a provider-only remix. Two have usable English programs, one mostly native-script. Existing automatic comparison supplied this group identifies **10 Recordings**, leaving **Bad Dream** and **Shell**. The first program inspector supports all 12 positions under Album context; automatic rejection for those two is in title/featured-credit correspondence, not exact edition selection. This is a different MusicBrainz program from the eight-Track Spotify edition discussed in earlier audits; Spotify's six-safe-mapping result must not be copied here.

6. **Elephant Gym — Work — 5 Tracks, 0/5.** English discovery fails. Artist browsing finds **工作**, group `8dd0b2dd-b46d-45ed-95e3-5e6ce7718dba`, in the first broad page. Three five-Track Releases expose `Mirror 鏡子`, `Midway 中途`, `D`, `Spring Rain 春雨`, `Celebrate 慶`. Automatic comparison identifies only **D**; current Album inspector supports **5/5** using bounded shared-title components and whole-program context. **Discovery, then evaluator divergence**. Neither global non-Latin stripping nor translation is needed for this observed correspondence.

7. **Elephant Gym — Gaze at Blue — 3 Tracks, 1/3.** Trusted group `1949e0a2-abe4-4147-af86-7bd1e8727300` has a **single-Track** Release, not the canonical three-Track program. Missing **Dawn** and **Gaze at Blue (Remix)** cannot be found on that program. Artist catalog browsing also exposes **凝視**, group `f2c15f47-8a93-4dac-853f-7910b5682140`, whose two sampled Releases have three Tracks. Supplied that alternative, automatic comparison identifies Dawn and preserves the existing title Track, leaving the remix unresolved against **Gaze at Blue (Charlie Lim remix)**. Inspector's first program has two anchors. This is **Release Group/program scope**, with a meaningful remix attribution difference; do not silently replace the trusted group or equate all remixes.

8. **Giraffes? Giraffes! — More Skin With Milk-Mouth (2015 Remaster) — 5 Tracks, 0/5.** Trusted Artist `88a5ed53-1c52-474a-8ed5-49961ee2366e`. Full remaster scoped/manual queries find no group. A bounded base-title query finds **More Skin With Milk-Mouth**, group `cd30bfa5-1c7c-3ae5-9bb6-e805b970e35d`, first. Existing legacy title qualification would still reject remaster versus unqualified title. Three five-Track sampled Releases exist. Automatic comparison identifies zero because every local Track has the systematic `(2015 Remaster)` annotation. Inspector supports four after contextual systematic comparison; first Track also differs **When The Catholic Girls…** versus **When Catholic Girls…**. This demonstrates existing annotation concepts, not proof that the remaster shares every Recording MBID with the original. Remaster Recording semantics must remain conservative; exact Release status is unknown.

9. **Gumby's Junk — Business & Pleasure — 11 Tracks, 0/11; profile only.** Artist search returns first-ranked **Gumby’s Junk**, Artist `06a012c5-7dac-4a4c-8929-fe3b5427814e`, but `resolve_artist` compares punctuation-preserving `normalize`, so straight/curly apostrophe prevents resolution. Provider ranking is fine. Full Album/Artist manual search returns a plausible group first; its single sampled Release is `45b8c751-aa3f-40ba-bbb5-439fdf814e4d`, 11 Tracks. Program comparison identifies 11; most provider durations are absent, which is not a conflict. Legacy Recording comparison rejects the Artist-name typography. Review backend confirms all 11 Recordings on a copy; Artist and Album IDs remain unassigned. **Artist evaluation failure**, not Album discovery failure or justified fuzzy Artist matching.

10. **Hella — Church Gone Wild/Chirpin Hard — 24 Tracks, 0/24.** Trusted Artist `1ecbc7e5-6e33-4062-997d-abc550dd63e6`. Full slash-joined automatic/manual queries return no candidate. Reduced raw search `releasegroup:"Church Gone Wild"` returns two unrelated Singles ahead of the correct **Church Gone Wild / Chirpin Hard**, group `385cbc6c-a9b5-3a7b-8146-f4a14bdcdfd2`, at provider rank **3**, score 73. Artist identity must rerank these; title similarity alone is unsafe. The shared title normalizer would accept the spacing relation once found, but search construction does not generate that alternative. Two 24-Track programs differ in order; supplied the group, automatic comparison identifies **23**, leaving canonical final **Chirpin Hard** (~17m40s) unresolved. Inspector shows reordering and local-only rows. Source-wide `(Movement N)` suffixes and reordered disc material are additional program evidence. Do not claim all 24 are fixable merely by query normalization.

11. **Hella — The Devil Isn't Red — 11 Tracks, 0/11.** Trusted group `9683e2b3-fef8-3c5f-b0cc-9dec6f938ce6`; Artist agrees. Sampled Releases `776b1c44-9785-438f-a769-a4e7b1b23bec` and `1cf4e272-9572-404e-a7b0-369042f0a478` are complete; `c3d3d127-8d68-44b2-9370-e695f05b746b` is marked **incomplete** by adapter conversion. `compare_album` rejects all Tracks as soon as **any** sampled program is incomplete, before choosing viable programs. Cache-only exclusion of that incomplete sample identifies **11/11**, including tolerant title correspondences. Legacy Recording search independently identifies eight exact titles, with three spelling/punctuation differences. Review backend's first-program plan confirms 11/11 on a copy. This is a reproducible **incomplete-alternative poisoning** problem. The converted response does not retain which raw media completeness predicate failed; do not invent the missing raw field or assume the provider Release is malformed. No exact Release is required.

12. **Hop Along, Queen Ansleis — Freshman Year — 16 Tracks, 15/16.** Group `f5b8ed06-6fcd-41e9-9612-c9e195571455`. Local `_` at position 1 (~24s) corresponds to `-` on the preferred 16-Track Release `dba2588b-ae43-4ae2-95e1-93f2586d40ec`; another sampled Release uses `:` and variant wording. Existing independent Recording IDs support 15 anchors. Automatic `compare_album` rejects the punctuation-only Track; `inspect_album_program` supports all 16 on the first program using the shared placeholder rule. Actual review-backend confirmation adds **one** Recording, preserving membership and Album IDs. The profile's canonical Artist object has no attached Artist MBID, although program context and existing Recording IDs support inspection. Case-insensitive full-name equality is harmless; neither `Hop Along` nor a Spotify identity should be substituted for the full canonical Artist.

13. **TTNG — Hippy Jam Fest — 5 Tracks, 0/5.** Trusted Artist `544559db-c6c4-45d6-8435-d224e4f696e9`; provider printed credit is **This Town Needs Guns**, a trusted-ID relationship rather than display similarity. Full Album plus Artist-name manual query returns no candidate. Title-only search puts **Hippy Jam Fest (The Likes of Which Has Never Been Seen)** first, group `680ff456-5e82-4d0c-ba1f-86479004d52d`. Automatic scoped search still does not expose it for the abbreviated input. Its five-Track program supports five identified Recordings; a three-Track alternative does not displace it. Legacy Album title acceptance would also need contextual full-title support after discovery. This is a discovery/title-and-alias ergonomics issue; general substring Album acceptance would be unsafe.

14. **Polyphia — New Levels New Devils — 10 Tracks, 4/10; fresh only.** Trusted group `db167d57-3779-42ca-9399-8df9b62fc3d9`. Three complete ten-Track programs are available. All ten rows are exact anchors in the contextual inspector. Six local titles carry explicit featured names: Nasty (Jason Richardson), Death Note (Ichika), Drown (Mateus Asato), Yas (Mario Camarena and Erick Hansel), So Strange (Cuco), Rich Kids (Yvette Young). Current automatic musical-title extraction cannot corroborate/remove these suffixes with the loaded structured credits, so those six remain `NoConfidentMatch`. Existing O.D., Bad, Saucy, G.O.A.T. Recordings remain intact. **Evaluation**, not candidate discovery, Release ambiguity, or missing provider Recordings. Profile saved membership does not contain this six-gap state.

## 4. Discovery is separate from acceptance

### Automatic MusicBrainz request path

`src/album_matching.rs` resolves a known Artist identity first, otherwise calls `search_artists`, then requests Artist-scoped Release Groups. Ambiguous Artist candidates can be jointly corroborated with an Album-scoped request. `adapters/musicbrainz/src/lib.rs` constructs:

```text
artist: (artist:"<canonical name>" OR alias:"<canonical name>")
release-group:
  arid:<trusted Artist MBID> AND
  (releasegroup:"<title>" OR
    (releasegroup:<word1>~1 AND releasegroup:<word2>~1 ...)
    OR <bounded format/type suffix alternatives>)
```

There are ten initial Artist/Album results at offset zero. Explicit manual paging exists; an incomplete page can withhold automatic confidence. Manual Artist confirmation allows edit budget two. Album searches use Release **Groups**, not Releases. MusicBrainz score is not an application identity decision; Artist/Album acceptance checks identities and full normalized relations. No per-Track discovery supplies alternate Album candidates in this path.

Important distinctions:

* Artist search combines primary-name and alias matching in **one OR query**. It does not first issue canonical-only search and then a separately bounded trusted-alias query. `resolve_artist` checks complete normalized names/aliases and ambiguity. It never uses substring or token-subset identity.
* A known Artist MBID scopes Album discovery regardless of its printed native-script name. That solves Artist text ambiguity but **does not translate the Album title**. The Elephant Gym result demonstrates this.
* Trailing EP/LP/CD/disc variants are shared between query generation and evaluation, with provider type checks. Native-language Album alternatives and punctuation-tokenization variants are not supplied automatically.
* `resolve_canonical` in `src/album_candidates.rs` sends MusicBrainz through `resolve_legacy`. The MusicBrainz adapter does not enable `album_candidate_programs()` (default false). Consequently the richer candidate-program ranking/disambiguation implemented for Spotify is not used to rescue a rejected/ambiguous MusicBrainz Album title.
* Successful Group association then queues sampled programs. When program matching is enabled, `enqueue_recordings` returns after scheduling program work; the legacy Recording-search evaluator is **not** an automatic fallback after a failed program.

### Manual-search findings

Bounded same-input manual full Artist/title queries found a plausible first result for **Adebisi Shank EP** and **Business & Pleasure**. The former was already present in automatic discovery and rejected; the latter was blocked earlier by Artist evaluation. Neither is proof of an automatic empty-result bug.

Different-input manual/structured searches found missing candidates for Hippy Jam Fest, Church…, the remaster's base title, and all four Elephant Gym Albums. These demonstrate discovery limitations, followed in several cases by additional evaluation limitations. A manual result being first does not bypass Artist identity, edition or Recording checks. Church… is a counterexample: correct result is third behind unrelated Artists.

Broad Artist-only search had **23** Elephant Gym Groups in three pages. Angle appeared on page two, Dreams/Underwater on page three, Work on page one. This proves that a first-page Artist catalog scan would miss some useful objects. It does **not** prove that the current title-filtered empty search had the correct object at rank 11: those are different query sets. No such claim can be made without the provider's complete original result page.

## 5. Release Group, Release, Track, Recording

The supplied root guidance still describes an earlier model without an Album abstraction, while the current schema, architecture documentation, and this task explicitly describe first-class application Albums, Releases, Tracks, and Recordings. This is a documentation inconsistency. This audit follows the task’s explicit request to inspect existing Album behavior; it does not introduce or redesign that model.

| MusicBrainz object | Application evidence owner | Current normal use |
|---|---|---|
| Artist | Artist | Trusted credit/discovery context |
| Release Group | Album | Normal automatic Album anchor |
| Release | Release | Exact edition association when independently selected/imported/established |
| Track MBID | Track occurrence evidence | Exact release-specific occurrence, not Recording identity |
| Recording MBID | Recording, reached from Track | Connections → MusicBrainz connected criterion |

`album_programs` browses up to 100 official Release summaries for a Group, falling back once without official filtering if empty, and samples **at most three** ranked Releases. It uses media/date ranking and fetches their complete program details. It does not expand all edition pages. Program identity is diagnostic provenance; it is not silently attached as the local exact Release.

Multiple Releases can provide evidence for one Album. The comparator selects viable templates, intersects agreed Recording identities, and retains ties rather than arbitrarily selecting a Recording. An exact edition can remain unknown while Recordings are established. Ordinary program persistence does not consolidate application Recordings; catalog import and the legacy Recording-binding path have separate conflict-aware reuse behavior.

Answers to the key questions:

* **Normal Album anchor:** Release Group.
* **Exact Release retained:** catalog import/exact edition selection and edition evidence; sampling does not select it. Only one saved profile Album has an exact Release association, despite 838 connected saved Tracks.
* **Several Releases, one Album:** supported as evidence; must not collapse incompatible editions into one Release.
* **Recording without exact Release:** supported, already common.
* **Recording without any Album identity:** normal automatic work requires a unique established Album provider identity; explicit Connections Recording confirmation can persist without establishing an Album or exact Release. Copy tests demonstrate this for Adebisi Shank and Gumby's Junk.
* **Does exact Release ambiguity block identities unnecessarily?** No blanket exact-Release requirement was found. The concrete blockers are unique Group preparation, incomplete sampled alternatives, and agreement/evaluation rules. Different Recordings across otherwise plausible editions can legitimately withhold Recording identity. “Require an exact Release first” would worsen this architecture.
* **Reissues/remasters:** qualify candidate evaluation and exact edition evidence separately. The remaster example shows incomplete discovery/contextual handling, but does not establish that all remaster Recordings should inherit original Recording MBIDs.
* **Correct Group obvious but exact edition ambiguous:** sampling can still identify Recordings shared by those programs. The Devil Isn't Red demonstrates a failure despite this intended design because one incomplete sample blocks the complete set.
* **Potential Group-level strictness:** `recording::prepare` requires exactly one Group and local source association. Normal program preparation also requires an associated provider Album identity. Therefore the nine/eight unanchored Albums do not receive normal Recording enrichment, even when manual structured discovery finds strong programs. That is an Album-context lifecycle gate, not exact Release certainty.

## 6. Semantic-capability matrix

Legend: **Shared/supported** = shared code is actually called on that path; **Partial** = supporting concept exists but some entry points/gates differ; **Spotify-only** = applicability/implementation is presently provider-specific; **Missing** = no such MusicBrainz behavior found; **Inappropriate** = use MusicBrainz relationships instead of copying a Spotify workaround.

| Capability | Shared authority/code | Spotify path | MusicBrainz path / status |
|---|---|---|---|
| Full case-insensitive Artist name | `matching::normalize`, `album_matching::resolve_artist` | Supported | Shared/supported |
| Artist NFC/punctuation typography | `artist_credit::same_artist` uses `normalize_album_title`; initial resolver uses weaker `normalize` | Partial | Partial; Gumby's straight/curly apostrophe fails initial resolution |
| Trusted alias identity evidence | `resolve_artist`, `trusted_artist_name`, `artist_equivalence` | Supported with bounded discovery and corroboration | Partial; provider alias OR search and exact returned aliases; no same ordered alternate lifecycle |
| Canonical name searched before aliases | Common desired discovery policy, no fully shared implementation | `search_artists_named`; `discover_catalog_credit_names` canonical then one alternate | Missing as separate search ordering; single primary/alias OR query |
| Ambiguous aliases require corroboration | `resolve_artist`, `corroborate_artist_album` | Supported | Shared/supported; truncated pages stay ambiguous |
| Substring/token-subset Artist identity | No authoritative rule permits it | Forbidden | Forbidden; no problematic rule found |
| Provider IDs override textual ambiguity; conflicting IDs veto | `artist_credit`, `canonical_evidence`, conflict helpers | Supported | Supported in program credits; initial name resolver intentionally still checks names when establishing a new ID |
| Full normalized Album title | `normalize_album_title`: NFC, case, spacing, apostrophes, narrow hyphens | Supported | Shared/supported in acceptance, raw query variants less complete |
| Contextual punctuation / `_` versus `-` | `contextual_title_class`, `punctuation_placeholder`, diagnostics | Supported under strong context | Partial: review placeholder works for Freshman Year; automatic comparator does not use it |
| Token boundary / tiny contextual typo | `matching::contextual_title_class`, diagnostics | Supported with trusted Artist/program gates | Partial/Missing: diagnostic contextual branch explicitly requires Spotify Artist IDs; simple positional one-edit legacy comparison is different |
| Weak Album annotations / Album Version | `album_candidates/evidence.rs`, diagnostics | Supported contextually | Partial: automatic Group title acceptance uses legacy qualifiers; reviewer has some shared annotation support |
| Remaster/reissue representation | `album_representation_title`, typed edition evidence | Provider Album/playback representation rules | Partial; MusicBrainz should use Group/Release relationships plus version-aware Recording evidence |
| EP/LP/CD/disc decorations | `album_title_variants` with provider type constraints | Supported | Shared/supported; Single wording not a universal strip rule |
| Provider boilerplate / “This Is” | Shared-file candidate evidence, invoked by Spotify resolver | Supported under established Artist evidence | Missing from automatic Group acceptance; Adebisi Shank example |
| Live/acoustic/demo/remix distinctions | `qualifiers`, typed edition evidence | Preserved | Preserved; separate qualifiers implementations can diverge |
| Track exact/title typography | `title_relation`, `comparison_title`, Artist-credit musical title | Supported | Shared/supported in program comparison; legacy Recording comparison only lowercase/spacing |
| Punctuation/token boundaries for Tracks | Same utilities plus contextual inspector | Strong contextual support | Partial; OCDC/OC/DC unresolved, review and automatic differ |
| Tiny Track typo under strong program | Inspector contextual rules; legacy positional one-edit rule | Supported | Partial; contextual branch Spotify-specific; legacy one-edit is not equivalent |
| Weak Track annotations | Inspector systematic/contextual comparison | Supported | Partial in review; automatic comparison bypasses inspector |
| Systematic Album-wide annotations | `inspect_album_program` | Supported | Review supports movement/remaster derivation; automatic `compare_album` does not consume it |
| Multiscript full exact equality | Unicode-preserving normalization | Supported | Shared/supported regardless of script |
| Multiscript shared component | `shared_title_span`, strong program/Artist/duration context | Supported, including playable partial-program policy | Partial in MusicBrainz review; minimum component length/coverage blocks Angle; automatic path does not use derived spans |
| Translation/transliteration | None | Missing/deferred | Missing/deferred; do not implement by stripping scripts |
| Inserted/missing/reordered Tracks | `align_program`, positioned diagnostics | Authoritative inspector drives established/playable mapping | Partial: review uses inspector, automatic MB uses `compare_album`/`mappings` |
| Unreliable positions, multi-disc | Shared aligner plus evidence loading | Supported | Partial: common representation exists; automatic positional preference differs; legacy program code has flattened-index fallback |
| Partial mapping / provider-only / local-only Tracks | Shared inspector decisions | Supported | Partial: review supports partial confirmation; automatic can associate a subset but has different gates |
| Candidate reranking | `album_candidates/evidence.rs` | Artist/title/program/date evidence; retained candidates | MusicBrainz legacy acceptance; full shared-file report contains Spotify `kind == "album"` assumptions |
| Program-derived/Track-derived Album search | Spotify discovery and `album_context` | Supported within request budgets | Missing; better MusicBrainz approach is structured Artist→Group→Release→Recording expansion |
| Release-family expansion | Provider-specific discovery | Necessary Spotify workaround for multiple catalog Albums | MusicBrainz Group→Releases is already structured; reuse relationship, not Spotify Album-family heuristics |
| Persistent discovery/program cache and lifecycle reasons | `spotify_discovery`, `spotify_program_cache`, review tables | Supported | Missing equivalent durable lifecycle; small in-memory program cache only |

The matrix intentionally does not label every function in a provider-neutral file “fully shared.” Actual dispatch and provider checks decide that. In particular `resolve_canonical` explicitly dispatches the richer resolver to Spotify, and `playable_alignment` plus contextual typo gates explicitly look for Spotify Artist identities.

### Search ergonomics comparison before provider evaluation

| Search preparation | Shared | Spotify | MusicBrainz | Recommended ownership |
|---|---|---|---|---|
| Canonical Artist query | Resolver input | First | First combined with provider aliases | Shared ordered policy |
| Trusted alias queries | Name observations | One bounded fallback, plus explicit corroborated discovery | Returned native aliases in same query; known MBID is stronger | Shared provenance/budget; provider-specific query |
| Unicode title normalization | Typography functions | Query alternatives and comparisons | Comparison supported, query coverage partial | Shared title evidence; avoid identity fuzziness |
| Punctuation/token-boundary alternatives | Contextual title classes | Search variants and program evaluation | Not equivalent automatic query preparation | Shared bounded variant descriptions, adapter renders syntax |
| Contextual typo variants | Bounded semantics | Contextual program rules plus provider search paths | Lucene per-token `~1`, full-title one-edit acceptance | Different mechanisms; do not mistake retrieval fuzziness for acceptance |
| EP/Single variants | Typed suffix concepts | Some provider wording handling | Shared EP/LP/CD variants; no blanket Single deletion | Shared type-aware policy |
| Weak annotations | Title evidence | Contextual evaluation/discovery | Legacy Group title gate misses several | Shared evaluation; Group relationships guide discovery |
| Program-derived search | Program evidence | Yes | No comparable candidate rescue | MusicBrainz structured approach |
| Track-derived Album discovery | Source discovery | Yes | Absent in normal Album worker | Bounded Recording→Release/Group approach if justified |
| Release-family expansion | Structural concepts | Catalog Album family heuristics | Native Group→Release traversal | Provider-specific adapter |
| Candidate reranking | Evidence model | Strong program-aware ranking | Legacy selection, sampled Release ranking only | Shared structural evidence, different provider relationships |
| Program corroboration | Multiple current entry points | Established/playable inspector | Automatic simpler comparator; review inspector | Consolidate authoritative correspondence decisions |

## 7. Artist aliases and multilingual behavior

`resolve_artist` accepts one complete normalized name or returned alias and rejects ambiguous exact alternatives. A close-name competitor can veto alias-only resolution; a truncated Artist page is not treated as proof of uniqueness. Album corroboration can resolve a bounded plausible Artist set. No substring-only identity or “all shorter tokens occur in longer name” rule was found.

`artist_credit::same_artist` uses trusted identity overlap or normalized full names with conflict vetoes. This is more typography-aware than `resolve_artist`. The rules should be reconciled deliberately: harmless apostrophes/NFC should not behave differently at initial Artist discovery and established program comparison. Do not replace it with broad punctuation deletion or accept Hop Along as Hop Along, Queen Ansleis.

Trusted aliases/names are durable in `trusted_artist_name`, exposed through methods currently housed in `src/spotify_lifecycle.rs`. The storage and provenance concepts are wider than that filename. `trusted_artist_search_names` returns at most two names; Spotify consumes at most one alternate. It also includes Spotify cache-specific ordering. Extracting the provenance/name policy is sensible; copying the whole helper unchanged into a universal utility would preserve provider-specific behavior.

For multilingual titles:

1. Full normalized equality remains strongest; title NFC and typography folding do not privilege Latin.
2. `shared_title_span` is Unicode-aware, uses text boundaries, and never translates. `水中漫步` equals itself; `水中漫步 Underwater` versus `Underwater` can contribute a shared textual component under contextual gates.
3. The ordinary MusicBrainz program comparator does not consume the inspector's derived shared-span program. This explains Work's 1/5 automatic versus 5/5 inspected result.
4. The span helper requires two alphanumeric characters. Most Angle native components are one character, and whole-program coverage remains below its required threshold. “Just take the Chinese portion” is neither implemented nor warranted.
5. Album discovery is a separate problem: English-only Album input does not retrieve Groups titled 工作, 水底, 夢境, 角度 merely because the Artist MBID is correct. Structured identity-scoped catalog evidence is the next useful investigation, not global translation or non-Latin removal.

## 8. Recording-level acceptance and persistence

There are three materially different decision paths:

1. **Legacy Recording search:** `src/recording.rs::compare` after one Group-scoped Recording search (`rgid`, limit 100). Requires a complete page, exact lowercase/whitespace title, Artist IDs when complete or full textual credit agreement, and known duration within 3 seconds. It does not use the newer title typography/program semantics. Results are not current normal-worker fallback when program mode is enabled.
2. **Normal MusicBrainz program enrichment:** `album_program::compare_album → selected_programs → compare → mappings → inspect_candidate`. Uses title/position/credit evidence and Recording agreement across sampled programs. Rejects everything if any sampled program is incomplete. Does not call the contextual Album inspector to generate final MB mappings.
3. **Connections review:** `prepare_connection_recordings`, `musicbrainz_review_mappings`, `inspect_album_program`, explicit `confirm_musicbrainz_connections`. Album review builds mappings from the **first** sampled program's safe decisions, rather than automatically intersecting all sampled Recording evidence. Track review considers contextual relevance and separately prefers legacy exact Recording comparison. Explicit manual Recording choice is another user-authorized route.

Consequences:

* **Not discovered:** normal automatic context missing for 8 fresh / 9 profile Albums; structured/manual probes locate candidates, but the lifecycle does not automatically use those alternatives.
* **Discovered but rejected:** Artist apostrophe; Adebisi Album boilerplate; featured/weak annotations; bounded title differences.
* **Present on Release, not aligned automatically:** Work's four bilingual titles, Freshman's placeholder, Polyphia's six featured titles, multiple contextual cases.
* **Blocked by incomplete provider program:** Devil's 11, despite strong complete alternatives. Removing the incomplete sample **in audit evaluation only** identifies all 11 with current comparator logic.
* **Blocked by Album context:** no trusted Group prevents normal program work. Explicit review confirmation proves persistence itself can accept Recording-only knowledge without an exact Release or newly established Group.
* **Aligned but not persisted:** no current storage bug reproduced. Review-backend confirmations persist exactly the displayed mapping count. Historical absent write/failure is unreconstructible without lifecycle logs.
* **Conflicting trusted Recording:** no attached conflict found; no evidence this is a current primary gap cause.

A user-confirmed first-program mapping is not equivalent to automatic cross-Release agreement. The successful review tests show persistence and observability work; they are not permission to make those same choices silently in an automatic matcher. Multiple sampled programs may disagree about a Recording even when titles align. Preserve that distinction in consolidation.

## 9. Conflicting trusted identity audit

Saved-scope SQL groups the correct owner tables by `(canonical ID, provider, kind)` and reports more than one distinct ID. Results in **both** datasets:

| Conflict | Count |
|---|---:|
| Artist → multiple Artist MBIDs | 0 |
| Album → multiple Release Group MBIDs | 0 |
| Recording (therefore its Tracks) → multiple Recording MBIDs | 0 |
| Release → multiple Release MBIDs | 0 |
| Attached Album/Recording IDs contradicted by retained embedded observations | 0 |

A separate profile-wide initial query, not just saved scope, also found no multiple attached IDs in these four namespaces. Invalid raw tag values do exist (for example a numeric value such as `1549642` in a MusicBrainz Artist tag); raw observation is not trusted attachment. It is preserved, and must not be counted as an identity or conflict between trusted IDs.

Safety findings:

* Embedded acceptance groups observations by namespace, validates format, and withholds conflicting observations or independent identities (`provenance_acceptance::assess`). Safe conservative boundary.
* `bind_musicbrainz_tx`/Recording merge checks different MBIDs and rolls back incompatible consolidation. Manual Connections confirmation rechecks Recording conflicts atomically before writing. Safe behavior; do not overwrite during cleanup.
* Generic external-identity schema is many-to-many and has no unique `(owner,provider,kind)` constraint. There are APIs that can attach evidence directly. Conflict safety is therefore API-dependent, not guaranteed by schema alone.
* Legacy unmarked evidence is protected as independent. That is intentionally conservative and limits reconstructible provenance. No measured current conflict justifies a cleanup phase now.
* A trusted Group can be too narrow for the canonical program without there being multiple trusted IDs: Gaze at Blue is a concrete example. Absence of duplicate IDs does not prove semantic correctness.

## 10. Connections → MusicBrainz observations

The current UI exposes **absence**, not detailed automatic lifecycle reasons. `connection_tracks` hard-codes “No MusicBrainz Recording.” Spotify has retained reason codes, evaluation state, retry/cooldown context, and discovery/program caches. Consequently MusicBrainz's new observability surface is useful but not equivalent in diagnostic depth.

Observed current behavior:

| Observation | Evidence |
|---|---|
| Best known candidate always appears on reopening | **No guarantee.** Open takes matcher `cached_programs` or retained current catalog review; matcher cache holds four Albums, adapter cache four Groups; restart/eviction loses programs. There is no MusicBrainz equivalent of durable Spotify candidate/program lifecycle. |
| Candidate program appears | Yes after explicit search/Release inspection; real QML 100 Onces test and four cached-real-program bridge tests. Cold absence is not proof no provider candidate exists. |
| Mapping uses backend authority | Review consumes `inspect_album_program` decisions; QML does not rematch strings. But this is not the same final decision path as automatic MB `compare_album`. |
| Candidate ranking reflects backend evidence | Track preference can use legacy Recording comparison; Album confirmation uses first inspected program. No equivalent durable, fully program-ranked MB first-layer candidate report was found. |
| Manual search finds better objects | Yes, bounded examples above. Some require changed query or structured Artist browse; not uniformly the first result. |
| Accept persists Artist/Release/Recording identities | Connections MusicBrainz Connect confirms **Recording IDs only**. It intentionally does not establish Artist, Release Group, exact Release, or exact Track occurrence. Separate catalog import/association workflows have other persistence semantics. |
| Partial confirmation preserves unresolved Tracks | Existing focused test passed; real confirmations change only new supported Recording mappings and preserve saved count. OCDC remains unresolved until explicitly chosen. |

Validation performed this audit:

* Actual QML Album and Track Match entrypoints used by `Connections → MusicBrainz` on isolated `/tmp/mb-audit-ui.sqlite`: open with usable local routes temporarily unavailable, manual exact-Group search and Release inspection, full program shown, OCDC/OC/DC left conservative, explicit Recording selection succeeds, window lifecycle succeeds, **838 → 839**. Existing Spotify marked choices remain **222**. No original observations changed.
* Four additional actual review-backend flows with retained real MusicBrainz programs and fresh isolated copies: Freshman Year **+1**, Devil **+11**, Business & Pleasure **+11**, Adebisi EP **+4**. They exercise the same bridge snapshot and Connect operations, preserve saved count, and leave existing Album IDs unchanged. These are backend/dialog-model tests, **not four additional pointer-driven QML navigation recordings**.
* Synthetic existing test exercises zero candidates, best Recording preference and partial Album confirmation. It passed.

The cold MusicBrainz candidate gap is analogous to part of Spotify's earlier lifecycle problem: current durable knowledge and a useful inspected candidate are not consistently projected into the initial dialog. However, this audit does **not** establish that “manual always finds the first correct result.” Several English full queries still found none, and automatic evaluation already saw the rejected Adebisi candidate.

## 11. Hidden failures and successful canonical objects

| Without Recording identity | Fresh import | UI profile |
|---|---:|---:|
| Missing MusicBrainz Recording | 99 | 104 |
| Has usable unsuppressed available local source | 99 (100% of gaps) | 93 (89.42% of gaps) |
| Lacks that usable local route | 0 | 11 |
| Canonical saved Track/effective metadata still exists | 99 | 104 |

Thus **99 fresh / 93 profile failures could remain invisible to ordinary local browsing/playback capability**, while all 99 / 104 still have valid canonical membership and display metadata. This is a measured capability/visibility count, not a claim to have decoded and listened to every source file. Availability observations are not a fresh filesystem/audio audit.

Playback resolution does not require MusicBrainz identity to choose a local source. Track display/grouping uses effective metadata and canonical ownership, not presence of a Recording MBID. Even the remaining 11 profile gaps are still structurally valid Library Tracks; other availability issues may make those noticeable independently. We cannot retrospectively count actual user awareness. Stronger claims such as “all 104 were previously invisible” would be unsupported.

This supports the hypothesis that MusicBrainz could fail more quietly than a provider used for playable-source discovery. It does not establish that Spotify's catalog matching quality is superior or inferior.

## 12. Known regression probes that actually have current evidence

These are controls for interpreting coverage, not hard-coded matching rules:

| Canonical Artist / Album | Saved Tracks | Recording coverage | Current evidence |
|---|---:|---:|---|
| Hop Along, Queen Ansleis / Freshman Year | 16 | 15/16 | Detailed live/retained failure above |
| Elephant Gym / Underwater | 12 | 0/12 | Detailed structured Group/program probe above |
| TTNG / Animals | 15 | 15/15 | Group `ec4d729c-2822-3d95-8ba6-10e30cff94bc` |
| TTNG / Animals Acoustic | 13 | 13/13 | Distinct Group `2357e721-ad81-49df-85c5-37977a87c27a`; no acoustic conflation inferred |
| TTNG / Audiotree Live | 5 | 5/5 | Group `8427d628-1c2b-49b6-b92c-f00cf528acc2` |
| CHON / Audiotree Live | 9 | 9/9 | Group `00f3ed62-1232-4454-8dc4-8c09a2898021` |
| Tera Melos / Audiotree Live | 6 | 6/6 | Group `ee9c8dbd-8f22-425c-8bdf-cbda300beb22` |
| Floral / The Second Floral EP | 6 | 6/6 | Saved current MB Recording evidence; no gap sampled |
| Tabar / Hugs EP | 3 | 3/3 | Saved current MB Recording evidence; no gap sampled |
| Piglet / Songs | 9 | 9/9 | Group `da90e4fd-2910-47a8-abd6-3a0bbdc69d54` |
| Giraffes? Giraffes! / More Skin With Milk-Mouth (2015 Remaster) | 5 | 0/5 | Detailed remaster probe above |
| オレスカバンド / WAO!! | 13 | 13/13 | Group `d386d19a-913e-3999-8c58-6abf8d4b04f0`; non-Latin Artist identity works when established |

Completed controls were inspected in persisted evidence, not re-fetched merely to make a known-case checklist. Full Recording coverage does not prove the current matcher would rediscover them after deleting all provider evidence; provenance limitations above apply.

## 13. Duplicated semantics and authoritative utility candidates

| Concept | Current implementations/owners | Consolidation candidate |
|---|---|---|
| Artist normalized identity | `matching::normalize`, `album_matching::resolve_artist`, `artist_credit::same_artist`, legacy `recording::compare`, `artist_identity` local-context code | One full-name/typography evidence policy, trusted-ID conflict priority, separately typed alias provenance. Keep local context consolidation distinct from provider resolution. |
| Title normalization | `normalize_album_title`, `album_representation_title`, `album_program::comparison_title/title_relation`, `song_resolution` title views, catalog search ranking | One explicit distinction between exact typography, contextual representation, version conflict, and discovery variants. Search ranking remains non-authoritative. |
| Qualifier/version handling | `album_matching::qualifiers`, `album_program` qualifiers, `album_candidates/evidence` annotation logic, `song_resolution` playback suffix handling | Shared typed version evidence; provider/operation-specific acceptance, not one string-stripping normalizer. |
| Program correspondence | `align_program`; `compare_album/mappings`; `inspect_album_program`; `established_occurrences`; `spotify_playable_program` | Shared correspondence/ambiguity result consumed by automatic and review paths, with separate occurrence/Recording certainty and canonical/playable purpose. |
| Artist aliases/name observations | `spotify_lifecycle` shared-looking storage APIs; Spotify adapter fallback; MusicBrainz alias search | Provider-neutral trusted-name storage and bounded query policy; MusicBrainz ID/alias adapter discovery stays structured. |
| Candidate evaluation/ranking | `album_candidates::resolve_legacy`, `evidence::resolve_spotify`, provider search ranks, representative Release ranking | Shared evidence vocabulary/report; appropriate provider-specific Group/Release topology and sampling policies. |
| Persistence and conflict handling | Program completion SQL; legacy Recording binding; explicit Connections confirmation; provenance acceptance | Keep operation-specific writes, but share typed validated Recording correspondence and consistent conflict checks. Do not make all routes merge Recordings. |

Priority shared abstractions are **Artist evidence**, **title relationship evidence**, and **program correspondence with explicit certainty**, not a generic provider plug-in framework. They should accept trusted evidence and return reasons, not opaque similarity scores or booleans that hide the acceptance purpose.

Desired boundary:

```text
shared canonical/evidence semantics
        ↓
provider-specific bounded discovery (MusicBrainz Artist/Group/Release relationships)
        ↓
shared program correspondence + operation-specific certainty
        ↓
provider/operation-specific persistence and conflict validation
```

Some concepts in shared files still contain provider assumptions: `initial_report` checks provider object kind `album`; MusicBrainz uses `release_group`. `playable_alignment` and contextual tiny-typo branches require Spotify Artist IDs. `established_occurrences` intentionally supplies Spotify Track occurrences without Recording identity. Those cannot become authoritative MusicBrainz Recording identity by simply changing a provider string.

## 14. Prioritized follow-up phases and expected impact

Estimates below are **opportunity bounds from retained evidence**, not guaranteed automatic fixes. Phases overlap; do not add upper bounds to predict 100% completion.

### Phase 1 — trustworthy MusicBrainz lifecycle and bounded incomplete-sample handling

Persist bounded candidate/program observations and evaluation reasons, distinguish unattempted/empty/deferred/rejected, and restore useful candidates on review reopen. Isolate an incomplete sampled alternative from independently complete alternatives while preserving ambiguity about information not actually observed. Keep raw completeness reason diagnostics so the adapter's false flag can be explained.

* Affects Devil's **11** gaps in both datasets. Cache-only complete-alternative replay identifies all 11 with existing matching logic; review persistence independently confirms them.
* Candidate visibility improves across all **13** unresolved groups per dataset; cache persistence itself fixes **0 identity gaps** without evaluation/confirmation.
* Risk: medium. Ignoring incompleteness indiscriminately could hide a competing Recording; require evidence that accepted IDs are supported independently and document sampling uncertainty.
* Ownership: provider-neutral lifecycle vocabulary and evidence storage shape; MusicBrainz-specific Group/sample policy. No need to clone Spotify's tables wholesale.
* Tests: incomplete alternative plus two complete programs; all-incomplete; conflicting complete Recording IDs; truncated release browse; restart/eviction; changed-input invalidation; review reopening; zero-request cached inspection.

### Phase 2 — consolidate Artist/title/program evidence without relaxing identity globally

Use one contextual correspondence result for automatic MusicBrainz enrichment and review, with explicit Recording certainty. Share harmless full-name typography in Artist resolution. Remove Spotify-only trusted-Artist assumptions from concepts that are genuinely provider-neutral, while preserving the distinct playable/canonical policies and first-program/manual versus cross-program/automatic certainty.

* Profile opportunities demonstrated independently: Gumby **11**, Freshman **1**, OCDC **1** = **13** gaps. Fresh has no Gumby Album, so **2** direct gaps there. OCDC still needs the contextual branch generalized; it is not already automatically supported.
* Fresh-only Polyphia adds **6** contextual featured-title opportunities, subject to Recording/credit corroboration.
* Once Phase 3 discovers appropriate groups, Work has **4** additional bilingual opportunities, Underwater **2**, remaster inspector **4** potential correspondences. These are gated opportunities, not a claim those original/remaster Recording IDs should automatically be equated.
* Risk: medium to high, because correspondence is weaker than Recording identity. Preserve semantic live/acoustic/remix distinctions and explicit trusted-ID vetoes.
* Ownership: shared Artist/title evidence and program result, provider-specific structural context and persistence.
* Tests: apostrophe/NFC Artist variants; Hop Along prefix nonidentity; ambiguous aliases; `_`/`-`; OCDC token boundary; bounded typo with independent anchors; featured credits; systematic remaster labels; bilingual shared span in either script; one-character components; conflicting Recordings; partial/reordered/multi-disc programs; manual confirmation cannot silently become automatic certainty.

### Phase 3 — structured MusicBrainz discovery and Group/edition scope

Use bounded Artist-ID catalog/relationship evidence, retained alternate titles and program corroboration to expose otherwise undiscovered Groups. Add typed discovery variants for slash spacing and safe established-Artist boilerplate; keep long titles/version differences as evidence rather than global substring acceptance. Inspect richer Group/Release relationships instead of copying Spotify release-family search heuristics.

* Discovery-only replay with current comparator identifies **4 Adebisi + 23 Church + 5 Hippy + 10 Dreams + 10 Underwater + 1 Work = 53** Recording opportunities in both datasets, after supplying the inspected groups. Adebisi additionally needs its title acceptance gate addressed; this is not 53 guaranteed outcomes from one query change.
* Appropriate Gaze at Blue Group/program investigation exposes **Dawn (+1)**, but must preserve the existing trusted Group relationship and remix uncertainty. Angle's **11** have candidates but currently insufficient contextual support; expected automatic gain **unknown**. Remaster's **5** require edition/version care; automatic gain **unknown**.
* Risk: medium to high. Native-title discovery is safe; identity inference from a translated title or an arbitrary first native-script result is not. Gaze at Blue may require multiple evidence relationships rather than replacement.
* Ownership: MusicBrainz-specific structured discovery, shared candidate/program evidence and bounded query-budget policy.
* Tests: English input/native-title Group; Artist pages beyond ten; canonical name before trusted alternate; broad search unrelated higher-ranked Artists; remaster versus original; Single versus EP program; alternate Groups do not overwrite established identity; provider-only Track and reordered disc material; no request per local Track.

No legacy-conflict-cleanup phase is justified by the measured **zero** conflicts. Add origin events for future decisions as part of lifecycle work; do not fabricate historical provenance or rewrite unmarked associations.

## 15. Queries, network work, tests, and limitations

### Queries and live work

Coverage SQL and partition assertions are checked in in `tools/audit/musicbrainz_coverage.py`. It independently checks SQLite integrity, saved membership scope, Recording connectivity, all gap-group counts, trusted-ID conflicts, managed markers, retained embedded agreement, and embedded/attached contradictions. It does not create membership or associate identities.

The probe's `calls` and follow-up `request` entries in the evidence file log exact high-level query inputs and inspected Group IDs. Adapter request syntax is described in section 4. Every successful sampled Release lookup is identified by its exact Release MBID in `programs.programs[].identity`. Request reasons are the unresolved-Album investigations above.

Network work was bounded to the unresolved union, not a catalog-wide refresh:

* Initial profile pass: **1 Artist search, 8 Artist-scoped Group searches, 9 explicit manual full-title/Artist searches, 6 Group program inspections, 6 legacy Group Recording searches**. Groups not found did not trigger Release expansion.
* Follow-up: **6** searches: Elephant Gym `arid:9fdc68d1-98ac-463c-a309-181b58c74aed` offsets 0/10/20; Giraffes `arid:88a5ed53-1c52-474a-8ed5-49961ee2366e AND releasegroup:"More Skin With Milk"`; `releasegroup:"Church Gone Wild"`; `releasegroup:"Hippy Jam Fest"`.
* **9** additional Group program inspections: the eight expanded candidates listed in section 3 plus fresh-only Polyphia. Total program inspections **15**, fetching **34 sampled Release details**, at most three per Group inspection. Gaze at Blue intentionally examines two different Groups, with different programs.
* Each program inspection uses a bounded official-release browse (limit 100, offset zero, `inc=media`), optional no-official fallback, and the listed sampled lookups (`inc=release-groups+recordings+artist-credits+isrcs+media`). Legacy Recording queries are `rgid:<Group MBID>`, limit 100, offset zero. No catalog-wide Track refresh or per-local-Track crawl occurred.
* The existing QML manual test separately performed exact-Group manual search and selected Release inspection for 100 Onces. Its exact Group/Release identities are retained in the probe evidence; it does not measure ordinary English-query search quality.
* Production transport's shared one-second gate, response size limit, retries and Retry-After handling remained in force. Calls were serial. Checkpointed successful probes and all subsequent analysis were reused offline.

Initial sandbox attempts failed DNS; they did not establish provider failure. Live work was rerun with approved network access. Adapter mock HTTP tests initially could not bind localhost in the sandbox and were rerun with the required permission. These environmental failures are not MusicBrainz match failures.

The high-level call log plus adapter code specifies query texts, IDs, offsets and bounds. **There is no wire-level HTTP audit log for official-fallback/retry attempt counts**, so this report does not invent an exact total HTTP request count or claim it captured raw response completeness fields. Raw converted probe artifacts remain under `/tmp/mb-audit-live`; the checked-in evidence preserves relevant converted programs, decisions, identifiers, and request inputs.

### Tests/checks executed

```sh
python3 tools/audit/musicbrainz_coverage.py /tmp/music-library-ui-test.sqlite /tmp/mb-audit-profile-measure.sqlite
python3 tools/audit/musicbrainz_coverage.py /tmp/music-library-fresh.sqlite /tmp/mb-audit-fresh-measure.sqlite
cargo test --offline --test connections --test program_alignment --test album_matching_migration --test durable_provenance
cargo test --offline --manifest-path adapters/musicbrainz/Cargo.toml
QT_QPA_PLATFORM=offscreen cargo test --offline --manifest-path tools/qml-diagnostic/Cargo.toml cached_musicbrainz_program_uses_recording_matcher_and_partial_album_confirmation -- --nocapture
QT_QPA_PLATFORM=offscreen MUSIC_LIBRARY_DIAGNOSTIC_DATABASE=/tmp/mb-audit-ui.sqlite cargo test --offline --manifest-path tools/qml-diagnostic/Cargo.toml shared_connection_review_real_musicbrainz_manual -- --ignored --nocapture
QT_QPA_PLATFORM=offscreen MUSIC_LIBRARY_DIAGNOSTIC_DATABASE=/tmp/mb-audit-baseline.sqlite MUSIC_LIBRARY_MB_AUDIT_EVIDENCE=/tmp/mb-audit-live cargo test --offline --manifest-path tools/qml-diagnostic/Cargo.toml musicbrainz_audit_retained_real_programs -- --ignored --nocapture
```

Results: **55 core integration tests passed** (2 explicitly ignored), **21 adapter tests passed** (3 live tests ignored), **1 focused UI test passed**, **1 real QML manual test passed**, and **1 four-Album retained review test passed**. SQLite integrity/partition assertions, Python compilation, Rust formatting, and `git diff --check` also passed. An initial test command named nonexistent test targets (`recordings`, `album_matching`); it ran no tests and was corrected to the checked-in integration target names above.

The narrowly added ignored audit test uses actual retained programs for four real Albums and validates snapshot, exact mapping-count persistence, membership preservation, and unchanged Album IDs. No matcher implementation was changed to make it pass. Offline analysis also replays complete-only Devil alternatives without persisting that modified evaluation.

### Limits

* Counts are snapshots of the two named sources, not a claim that all temporary historical databases represent the current library. Membership explains dataset differences; neither was silently chosen as universal truth.
* Historical discovery/search queries, rejection reasons and exact original identity provenance cannot be reconstructed from current MusicBrainz tables. Current probes classify reproducible stages, not every past event.
* Only the unresolved union received bounded live investigation. Completed regression controls use persisted evidence; high coverage is not proof of rediscovery quality.
* At most three sampled Releases per Group were inspected. Further editions could improve coverage or introduce Recording ambiguity. Native-script candidate identification is explicitly provisional unless strong program evidence corroborates it.
* Album/Track display and available local routes were measured; no whole-library playback or fresh file-availability scan was performed.
* Real QML navigation/manual confirmation covered 100 Onces. Four other real Albums exercised the actual review backend using retained data, not a full visual/pointer-driven session each.
* Neither successful manual confirmation nor a proposed program correspondence establishes an exact Release or authorizes overwriting an existing Group/Recording identity. No such overwrite occurred.
* The primary artifact is this report. Added code is isolated audit tooling and one ignored audit test; no broad matching refactor, schema change, or commit was made.

---

## Implementation follow-up: phases 1–2 (2026-10-18)

This section records the implementation after the historical audit above. The
original measurements and failure descriptions are preserved. No commit was
created. Validation writes only new disposable copies; it does not enrich the two
original databases. Full retained-program results and lifecycle summaries are in
[musicbrainz-matching-followup-evidence.json](musicbrainz-matching-followup-evidence.json).

### Previous divergence and the new boundary

The previous automatic path was `AlbumMatcher → complete_album_program →
album_program::compare_album → selected_programs → mappings`. It used its own
strict title/position correspondence and discarded all mappings if any sampled
Release was incomplete. Connections used `prepare_connection_recordings →
musicbrainz_review_mappings → inspect_album_program`, with contextual title
relationships and sequence alignment; Track review additionally had an independent
Recording comparator fallback. Thus identical provider evidence could produce
different structural answers. The inspector's broader correspondence did not
carry automatic cross-Release Recording certainty.

Both paths now call `program_correspondence::correspond → inspect_album_program`.
The typed result contains local decisions/evidence, ambiguity/conflict/version
warnings and provider-only rows. Insertions, missing rows, reordered editions,
unreliable and multi-disc positions, partial mappings and contextual title
relationships use the existing sequence/program aligner. Connections mapping and
confirmation consume this result; automatic enrichment consumes it through
`musicbrainz_recording::assess`. There is no separate automatic MusicBrainz Track
mapper. Historical `compare_album` remains for its other callers/tests but is no
longer the normal MusicBrainz enrichment path. Spotify selection is unchanged.

Structural correspondence and automatic identity certainty are deliberately
separate. Automatic policy requires trusted Artist evidence (ID overlap, or strong
existing Recording anchors plus complete-name Artist evidence), sufficiently
anchored contextual relationships, consistent supported Recording IDs across
complete samples, no observed contradictory IDs, compatible versions, no existing
trusted identity conflict, and a separate 3-second Recording-duration bound.
Incomplete samples are counted and retained, not treated as proof of agreement or
absence. Contradictions observed in incomplete samples still veto. All-incomplete
programs cannot establish automatic certainty. Explicit confirmation selects a
particular program through existing conflict-safe persistence; this never makes
that program globally certain for subsequent automatic work.

### Shared semantic changes

Artist resolution and program comparison now share Unicode-preserving NFC,
case-insensitive complete-name typography semantics, including straight/curly
apostrophes. Trusted IDs and explicit conflicts remain authoritative. Existing
trusted alias/equivalence evidence is preserved. No substring, prefix, token-subset
or broad fuzzy Artist identity was introduced.

Existing contextual title concepts now use provider-neutral trusted Artist overlap
instead of requiring Spotify Artist IDs. The token-boundary branch permits four
compact characters under its existing independent strong-context/uniqueness gates,
which covers `OCDC ↔ OC/DC`. Placeholder punctuation, bilingual shared components,
systematic annotations and sequence evidence remain contextual, never global title
stripping. Full non-Latin equality remains strongest. Structured additional Artist
credits can explain featured annotations under strong program context; arbitrary
parentheticals cannot. Live, acoustic, demo and remix differences remain meaningful.
Systematic remaster annotation can support structural correspondence while still
blocking automatic Recording identity.

The existing typed `EstablishedArtistBoilerplate` Album-title relationship is now
available to MusicBrainz evaluation, only for a unique sensible candidate under
trusted Artist IDs and a complete strongly matching program. This is not a literal
`This Is` removal rule and does not expand discovery.

### Real retained-evidence controls

| Control | Structural result / persistence result |
|---|---|
| Hop Along, Queen Ansleis — Freshman Year | `_ ↔ -` recognized; sampled structures 16 and 15 correspondences; complete evidence supports the missing Recording, +1 in each dataset. Existing 15 IDs unchanged. No substitution with Hop Along. |
| 100 Onces — OCDC | `OCDC ↔ OC/DC` supported with MusicBrainz Artist/program evidence; 11/11 structural, missing Recording automatically safe, +1 each. |
| Gumby's Junk — Business & Pleasure | Straight/curly apostrophe full-name Artist resolution succeeds; 11 Recording IDs established in the profile copy. Absent from the fresh gap set. |
| Elephant Gym — Work | Known supplied programs map 5/5 in all three samples in both paths. Native-title Group discovery/association remains deferred; no claim that discovery is fixed. |
| Elephant Gym — Underwater | Same supplied programs produce 12, 11 and 0 correspondences respectively in both paths; selected complete program supports all 12 structurally. Two positions still lack automatic Artist certainty and one has a Recording-duration conflict. Group discovery remains deferred. |
| Polyphia — New Levels New Devils | All three programs structurally map 10/10. Six featured-credit differences are explained by structured credits and become automatically safe in the fresh copy; no blind parenthetical stripping. |
| Hella — The Devil Isn't Red | Structures 11, 11, 10; third sample remains incomplete. Two complete alternatives support all 11 Recordings; +11 each without treating the incomplete alternative as complete. |
| Elephant Gym — Gaze at Blue | Alternate three-Track Group retained; existing trusted one-Track Group and existing Recording remain unchanged. Two gaps remain for review/discovery, not automatic Group replacement. |
| Adebisi Shank — The EP of a Band Called Adebisi Shank | Already discoverable unique candidate passes existing boilerplate relationship with trusted Artist and complete program evidence; +4 each. |
| Giraffes? Giraffes! — More Skin With Milk-Mouth (2015 Remaster) | Four of five missing rows structurally correspond in each supplied sample, retaining remaster uncertainty; no automatic original/remaster Recording equivalence. Remaining title difference is still unsupported. |

For every unresolved Album in the audit union with a supplied retained program,
the validation asserts byte-equivalent serialized structural results from automatic
assessment and the direct shared review call, for **every sampled Release**, not
only the selected presentation program. Automatic Recording certainty is reported
separately. Selection of one review program does not discard other samples.

### Durable lifecycle and Connections

Forward migration **37** adds `musicbrainz_match_state` and
`musicbrainz_candidate`. Unknown historical state remains unknown/unattempted.
Existing MusicBrainz identities, Spotify lifecycle, membership and manual
unavailable choices are preserved; migration tests cover both source versions
33 and 36.

Structured lifecycle reasons distinguish unattempted, matching, candidates found,
Album title rejection, Artist unresolved, completed-empty, provider deferred/error,
program incomplete, partial mapping, Recording ambiguity, identity conflict,
version uncertainty, duration conflict and changed evidence. Connections projects
compact per-Track reasons instead of universally reporting `No MusicBrainz
Recording`. Cold memory is no longer evidence of an empty provider search.

Normal enrichment and manual search retain the same bounded candidate observations:
Group identity, actual provider title/credits/types when observed, provider rank as
observation only, reason, timestamps and typed sampled programs including
completeness. Program-only retained evidence does not invent missing provider
metadata. Candidate count is capped at ten; automatic samples at three; serialized
program evidence is bounded at 8 MB. No catalog-wide raw payload store was added.

Automatic ensembles and manually inspected Releases are retained separately.
Manual inspection cannot replace a conflicting automatic ensemble with a convenient
single Release. Review can display that explicit inspection while automatic
certainty still considers its original alternatives. Incomplete manual provider
responses retain their program and completeness, rather than becoming an empty
catalog result. Inspection reuses one fetched detail response.

Reopening Library/bridge restores candidates, program and shared mappings without
provider calls. Fresh retained inspection also avoids a request on reselection.
The fake-provider restart test verifies zero requests; explicit refresh requests
fresh evidence. Opening review never renews observation age. Freshness is 30 days;
metadata/program/membership/trusted-Artist changes invalidate evaluation through
fingerprints and targeted triggers. Previous-process in-progress work becomes
changed evidence, not an eternal Matching state. Stale observations remain visible
for review with stale state, rather than disappearing at startup.

### Disposable-copy before/after coverage

These are **cache-only implementation replays**, not a live catalog-wide rerun.
They supply the audit's known programs and replay only already discoverable
Adebisi/Gumby candidate evaluation. Native-title or alternate Groups are retained
for review but not newly attached. All original identities are preserved.

| Dataset | Before Recordings | After Recordings | Gaps before → after | Albums with gaps before → after |
|---|---:|---:|---:|---:|
| Fresh, source schema 36 | 831/930 (89.35%) | 854/930 (91.83%) | 99 → 76 | 13 → 8 |
| UI profile, source schema 33 | 838/942 (88.96%) | 866/942 (91.93%) | 104 → 76 | 13 → 8 |

Fresh adds 23: Freshman 1, OCDC 1, Adebisi 4, Devil 11, Polyphia 6.
Profile adds 28: Freshman 1, OCDC 1, Adebisi 4, Devil 11, Gumby 11.
No exact Release certainty is required and no alternate trusted Group is overwritten.
These gains reflect specific newly supported evidence classes, not a relaxed
coverage target.

Both copies retain the following 76 gaps across eight Albums:

| Remaining Album | Missing | Meaningful classification |
|---|---:|---|
| Elephant Gym — Angle | 11 | Discovery/association still missing; retained supplied program is inspectable. |
| Elephant Gym — Dreams | 12 | Discovery/association still missing; supplied program does not authorize attaching a new Group. |
| Elephant Gym — Gaze at Blue | 2 | Trusted Group retained; alternate Group evidence needs review, no automatic replacement. |
| Elephant Gym — Underwater | 12 | Native-title Group discovery/association deferred; structural evidence retained, some Artist certainty still insufficient. |
| Elephant Gym — Work | 5 | Native-title Group discovery/association deferred despite complete structural correspondence. |
| Giraffes? Giraffes! — More Skin With Milk-Mouth (2015 Remaster) | 5 | Four structural candidates with version uncertainty; one unsupported title. |
| Hella — Church Gone Wild/Chirpin Hard | 24 | Existing discovery gap; supplied long/double-program evidence does not automatically establish a new Group. |
| TTNG — Hippy Jam Fest | 5 | Existing discovery/association gap; no newly authorized Group association. |

Lifecycle observations distinguish retained candidate/program knowledge from an
associated Group. Retaining an audit-supplied candidate is not evidence that the
normal bounded automatic search discovered it. Full per-Track reasons and sampled
completeness are in the follow-up evidence JSON.

### Verification and limits

Focused `tests/musicbrainz_followup.rs` covers shared structure; full-name Artist
normalization/conflicts; placeholders/token boundaries; inserted, missing,
provider-only, reordered and multi-disc programs; partial mapping; bilingual and
systematic title evidence; structured featured credits; meaningful version safety;
complete/incomplete/conflicting Recording alternatives; lifecycle states,
invalidation, migrations, restart/cache behavior, bounded queries and preservation
of the automatic ensemble across manual inspection.

The full core suite passed **515 tests** (18 ignored); MusicBrainz adapter mocks
passed **22** (3 live tests ignored); Spotify adapter tests passed **84** (3 ignored),
and backend review tests passed **7** (1 audit-only test ignored). Qt UI scenarios were run one per process
with the offscreen platform: navigation/offline management, shared ranked review,
inline Album/Track confirmation and Spotify review context. A combined Qt test
process aborts because its existing test setup shares QApplication state; isolated
scenarios pass. Live-provider tests remain intentionally ignored.

The deterministic 200k-Track Connections fixture passed bounded backend/UI checks:
MusicBrainz page p95 24.07 ms, Track expansion p95 1.09 ms in this environment;
backend page cap 200 and UI window cap 300 remain. These are observations, not
universal performance promises. Programs are fetched by selected Album, not N+1
per-Track queries or a QML full-library match.

No live MusicBrainz calls were made for this implementation validation. Adapter
requests used localhost mocks. Real coverage is a replay of retained provider
observations and cannot establish current remote catalog completeness. UI reopen
behavior is tested through recreated Library/bridge instances and isolated QML
scenarios, not a claim of a new interactive manual search session for every Album.

Deferred to the later structured-discovery phase: broad Artist catalog pagination,
native-script Release Group traversal, Recording→Group discovery and slash-title
search expansion. No broad identity redesign, global fuzzy normalization, exact
Release requirement, conflict cleanup/overwrite, per-Track provider searching or
Spotify playback-rule transplant was introduced.

Reproduction commands (all provider tests use mocks unless explicitly ignored):

```sh
cargo test --offline --no-fail-fast
cargo test --offline --test musicbrainz_followup --test connections
cargo test --offline --manifest-path adapters/musicbrainz/Cargo.toml
cargo test --offline --manifest-path adapters/spotify/Cargo.toml
QT_QPA_PLATFORM=offscreen cargo test --offline --manifest-path tools/qml-diagnostic/Cargo.toml connection_review::tests -- --nocapture
cargo run --offline --example musicbrainz_followup_validation -- /tmp/mb-phase12-NEW-NAME
```

The validation example refuses an existing output path, opens source databases
read-only, copies with `VACUUM INTO`, migrates the copies, asserts program equality,
checks preservation of trusted Groups and membership, applies only eligible
Recording decisions, and reopens the copies. Its provider stub panics on search
or Release requests. Coverage uses `Library::connection_coverage`; missing rows
use bounded `connection_groups` / `connection_tracks`. Targeted reason projection
tests were rerun after adding structured per-Track `{code,label}` state.
