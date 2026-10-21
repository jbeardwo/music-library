# Exact alternate title components — targeted multilingual correction

Implemented and validated 2026-10-20. No commit. Discovery, request budgets,
Recording-certainty policy and lifecycle architecture were not redesigned.
The existing evaluation generation advances **3 → 4**, so the already implemented
startup mechanism can reconsider retained evidence once under the corrected semantics.
There is no new migration or discovery generation.

## Failure and correction

Angle previously relied on `shared_title_span`. That requires two alphanumeric
characters and does not distinguish a complete representation from an arbitrary
substring. Consequently `序`, `燈`, and `頭` could not anchor their explicitly
separated bilingual titles. Longer native spans alone did not establish enough
unique whole-program correspondence. The first native sample reported **0/11**
correspondences, all `insufficient_anchors`.

The shared title layer now represents a *possible* alternate layout explicitly:
`AlternateTitleComponents` and `AlternateComponentMatch` retain the layout, the
complete normalized parts, the matched part/index and which side was formatted.
They confer no identity by themselves. Full title equality wins first.

Supported candidate layouts are two nonempty parts separated by a spaced hyphen,
en dash or em dash; or two internally unmixed alphabet blocks separated by actual
whitespace. Mixed-script words are never split. Whitespace plus an alphabet
boundary merely proposes a layout; it still requires program corroboration. No
translation, transliteration, language dictionary, Artist-specific rule or canonical
script is used. Native and Latin parts, and either formatted side, are symmetric.
Ordinary punctuation and parenthetical remix/live/acoustic/version/featured/remaster
annotations are not extracted as representation components.

A complete program must corroborate at least three unique pairs with a repeated
layout, exact trusted primary Artist identity, compatible meaningful qualifiers,
no trusted identity conflicts, strong sequence alignment, at least 80% coverage
in both programs and sufficient compatible duration evidence. Existing multi-character
whitespace/shared-span behavior can also use three independent program anchors;
one-character components require the repeated pattern. Component indexes and the
formatted side need not be uniform: neither script nor provider is privileged.

The positional fallback cannot promote an uncorroborated isolated component or
arbitrary substring merely because three other Tracks match. A tiny-typo derivation
also cannot remove an arbitrary script fragment from an otherwise whole mixed word.
`ABCの歌`, `TokyoラブSong`, `水Dance`, `Rock音楽Part2` and `A水B` remain whole titles.
A fragment such as `歌` inside `ABCの歌` is not a complete delimited representation.

The shared program layer derives comparison copies only. Raw local/provider titles,
observations, identity links and meaningful version annotations remain intact.
`PositionDecision::ExactAnchor` after a corroborated derivation is accompanied by
an explicit **exact alternate-title component** warning; this is distinct from a
**shared title span** warning. Automatic Recording certainty still evaluates raw
titles/credits, identities, duration and complete alternative samples separately.

## Safe copies and baselines

The same confirmed source path, `/tmp/music-library-fresh.sqlite`, now contains a
newer user profile than the prior pass: schema **39**, **930 saved Tracks**, **876
connected**, **54 gaps across 13 Albums**, and different canonical IDs. Its read-only,
WAL-inclusive SQLite backup is `/tmp/mb-components-before.sqlite`. All application
writes were restricted to disposable copies. The source was never opened through
mutable application APIs. The original and backup logical dumps have identical
SHA-256 `90bc63b50ee74c2856025182e9ab815908c02cb3fe6b6c86c3f419723d2f6cf6`.

Two validation cohorts remain separate:

| Cohort | Before | After | Remaining gaps |
|---|---|---|---|
| Current actual profile, normal reconciliation | 876/930; 13 Albums with gaps | **890/930; 11 Albums with gaps** | **54 → 40** |
| Prior reconciliation's retained-evidence copy | 897/930; 5 Albums with gaps | **908/930; 4 Albums with gaps** | **33 → 22** |

Both cohorts connect all eleven Angle Tracks. The current profile additionally
connects three TTNG `Juggling Moves (Beginner)` Tracks during normal policy
reevaluation; those three are not attributed to the multilingual correction.
The prior retained-evidence cohort isolates Angle's eleven connections, with
**zero provider requests**. Current-profile discovery uses the existing normal
path: ten transport requests for missing Angle ensemble evidence and three for
the TTNG case, **13 total**; no Group ID was manually injected. The three-page /
six-ensemble limits remain unchanged. A subsequent retained reevaluation is local.

The current profile initially retained Angle candidate metadata but not its correct
ensemble. The exact semantic before/after comparison below therefore uses the
actual native-only ensemble already retained by the prior real-profile validation,
not an invented fixture or translated title. The new normal current-profile run
independently retrieves the same candidate and reaches the same result.

## Angle — all eleven Track decisions

Release Group: `e0ac3da3-5279-47de-9e42-8d06897962fb`, **角度**.
Representative complete Release: `569f84f2-41e4-40af-b4bd-9757e0752608`.
All rows previously had `insufficient_anchors` / `no_correspondence` certainty.
All rows now have structural `exact_anchor` **from a corroborated exact alternate
component**, with automatic certainty `Supported` before persistence and
`AlreadyKnown` after normal reconciliation. All eleven Recording IDs are persisted
by the existing policy; no exact Release is selected.

For each row, the evidence is complete component equality, the same repeated spaced
hyphen layout, trusted Elephant Gym Artist MBID, a unique counterpart, coherent
position/sequence, compatible duration in the clean sample, and **11/11** coverage.
No row is accepted merely because it contains a shared character.

| Library title | Recognized complete components (normalized) | Provider title | Structural before → after | Automatic certainty before → after |
|---|---|---|---|---|
| Intro - 序 | intro / 序 | 序 | insufficient anchors → exact component anchor | no correspondence → Supported |
| Day time - 白日 | day time / 白日 | 白日 | insufficient anchors → exact component anchor | no correspondence → Supported |
| Games - 遊戲 | games / 遊戲 | 遊戲 | insufficient anchors → exact component anchor | no correspondence → Supported |
| Light - 燈 | light / 燈 | 燈 | insufficient anchors → exact component anchor | no correspondence → Supported |
| Strong Ladder - 堅固耐用的梯子 | strong ladder / 堅固耐用的梯子 | 堅固耐用的梯子 | insufficient anchors → exact component anchor | no correspondence → Supported |
| Ordinary Route - 日常的航道 | ordinary route / 日常的航道 | 日常的航道 | insufficient anchors → exact component anchor | no correspondence → Supported |
| Head - 頭 | head / 頭 | 頭 | insufficient anchors → exact component anchor | no correspondence → Supported |
| Body - 身體 | body / 身體 | 身體 | insufficient anchors → exact component anchor | no correspondence → Supported |
| Frog - 青蛙 | frog / 青蛙 | 青蛙 | insufficient anchors → exact component anchor | no correspondence → Supported |
| Whistle - 鳥鳴 | whistle / 鳥鳴 | 鳥鳴 | insufficient anchors → exact component anchor | no correspondence → Supported |
| Swan - 天鵝 | swan / 天鵝 | 天鵝 | insufficient anchors → exact component anchor | no correspondence → Supported |

The three sampled Releases have structural mappings **[11, 0, 10]** in this cohort.
The twelve-Track alternate sample has different material/representations and is
not forced into equivalence. A duration outlier in the third sample remains subject
to the existing alignment/certainty gates. The supported clean complete sample and
relevant corroborating evidence agree on the Recording IDs that are accepted.
The new rule does not make every sampled Release the canonical exact edition.

| Library Track | Automatically accepted Recording MBID |
|---|---|
| Intro - 序 | `2bce739d-7f00-4fae-bf45-922cf855f812` |
| Day time - 白日 | `d30480bb-e8dd-455c-b868-100c007d4417` |
| Games - 遊戲 | `60295730-9d56-4f04-beb9-42baad23b0fc` |
| Light - 燈 | `8ca100f6-3b97-4869-b91f-3f0c9c242796` |
| Strong Ladder - 堅固耐用的梯子 | `9da86c68-e4fb-4ae7-b9ed-8e159b5738f3` |
| Ordinary Route - 日常的航道 | `0159426f-34a2-4341-a64a-0aa0f532e3c3` |
| Head - 頭 | `b559d53e-d9c5-482f-9a64-33cd8c416ef3` |
| Body - 身體 | `df943cb3-24ca-4b1e-8674-b33c2d0a1037` |
| Frog - 青蛙 | `24356f8e-96ec-4448-8633-e681fe2a53be` |
| Whistle - 鳥鳴 | `ec1612c2-fa86-4c64-a3c1-cc069574aa14` |
| Swan - 天鵝 | `0ba7840c-e781-4483-8ff7-c5d0838aa9ad` |

## Dreams and regression controls

| Case | Structural result | Connected / certainty result |
|---|---|---|
| Dreams / 夢境 | **1/12 → 1/12** | **0/12 → 0/12** connected; Album identity remains insufficient |
| Work / 工作 | **5/5 unchanged** | Five existing connections preserved |
| Underwater / 水底 | **12/12 unchanged** | **9/12 unchanged**; Half Asleep duration and Bad Dream/Shell Artist blockers preserved |
| Freshman Year | **16/16 unchanged** | Placeholder `_ ↔ -` and existing identities preserved |
| 100 Onces / OCDC | **11/11 unchanged** | Token-boundary `OCDC ↔ OC/DC` remains supported |
| More Skin (2015 Remaster) | **5/5 unchanged** | **0/5** new Recording assertions; all remain version-uncertain |

Dreams' two thirteen-Track English samples have inserted/duplicate `Shadow` material;
the twelve-Track native sample shares only `Happy but Sad`. Most failures are
English-only versus native-only titles, not an explicitly combined title that
contains an exact alternate representation. No translation can be inferred.
`Dear Humans (JP Version)` versus `(Japanese ver.)`, and `Gaze at Blue (Album Version)`
versus `(2022 ver.)` remain under existing version semantics. The component rule does
not split or erase those annotations and does not force Dreams complete.

The following safety controls are explicit: full normalized equality wins; isolated
`Foo - Bar` does not create aliases; one-character substrings are rejected; multi-character
spans remain contextual; mixed words stay whole even with strong siblings; reversed
local/provider formatting works; component choices can alternate between scripts;
parenthetical semantics are preserved; competing component candidates, conflicting
identities, incomplete programs and untrusted Artists do not establish correspondence.
A duration-conflict fixture maps structurally while Recording certainty withholds it.
Existing featured-credit/remaster tests remain in the focused suite.

## Tests and artifacts

* Core `cargo test --offline`: **555 passed**, 18 opt-in tests ignored.
* Focused `musicbrainz_followup`: **55 passed**; `program_alignment`: **25 passed**.
* MusicBrainz adapter: **25 passed**, three opt-in tests ignored.
* Unchanged Spotify adapter: **84 passed**, three opt-in unit tests and one opt-in
  documentation test ignored.
* Offscreen Connections review: **10 passed**, six opt-in tests ignored.
* `cargo fmt --all --check` and `git diff --check`.

`tests/fixtures/alternate_title_programs.json` retains the real ensembles for all
seven required controls, including alternative Releases and version/duration evidence.
No provider calls are needed by these tests. The fixture README records sanitization
and provenance. No fixture title or Group ID is used in production matching.

Reproducible offline evidence: `examples/title_components_audit.rs`, which refuses
non-disposable paths. Before/after and normal-path artifacts:

* `/tmp/mb-components-current-before.json`
* `/tmp/mb-components-retained-before.json`
* `/tmp/mb-components-retained-after.json`
* `/tmp/mb-components-final-evidence.json`
* `/tmp/mb-components-normal-first-reconcile.json`
* `/tmp/mb-real-components-retained.sqlite.json`
* `/tmp/mb-real-components-normal.sqlite.json`
* `/tmp/components-*.log`

Intentionally unresolved: Dreams' unexpressed translations, inserted/ambiguous
material and meaningful version differences; Underwater's existing three blockers;
More Skin's original/remaster Recording semantics. No trusted Group replacement,
translation, transliteration or global title/Artist substring identity was added.
