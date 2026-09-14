# Known-Album Track program audit

Measured against read-only local files and the public MusicBrainz service on
2026-09-13. Provider data and latency may change. Library writes in the probe use
an in-memory database. No exact edition is accepted and no source file is changed.

## Causes before refinement

Subsequent exact-title refinement: a unique exact normalized title in the retained
known-Album program now accepts Recording identity regardless of duration, unless
strong identity conflicts or competing mappings remain. The historical live results
below describe the earlier duration gate. The deterministic Rich Kid regression now
accepts its Recording with the 3,102 ms discrepancy retained diagnostically; no new
live measurement is implied. The three-second tolerance remains for weaker matches.

* **Bride And Groom:** punctuation folding removed `&`, comparing `bride and groom`
  to `bride groom`, outside the one-edit allowance. Position, Artist and duration
  were compatible. Standalone `and`/`&` now share comparison form `bride and groom`.
* **Rich Kid:** both programs have exactly `Rich Kid` at position 3 and the same
  Recording ID. Local length is 276,898 ms; both report 280,000 ms. The 3,102 ms
  difference exceeded the existing 3,000 ms limit. No title/sampling bug was found.
* **Super Fx:** local tags already establish Recording `f5d57454-…`. The LP's
  `Super Fxx` is within one edit but points to `9b36fa32-…`. The old per-Track union
  allowed that contradiction to veto the two programs confirming the local identity.

The Hella and Hop Along target Tracks had no accepted local Recording identity;
their acceptance therefore depended on metadata evidence rather than identity confirmation.

## Sampled programs and target Tracks

Positions below are local → provider. Missing local disc tags are not completeness
evidence; the existing positional matcher can align their Track numbers to disc 1.
All target entries were considered after the comparison refinement. Other entries
in these programs were rejected for title disagreement; the probe prints every pair.

| Program Release ID | Track / position | Local → provider milliseconds | Recording ID | Occurrence ID | Decision |
| --- | --- | --- | --- | --- | --- |
| `5645a188-a0b8-4cd3-ae39-87b92a139349` | Bride And Groom → Bride & Groom; ?/1 → 1/1 | 279406 → 278000 | `eee892bd-b74c-4bad-91c6-a446a42bff80` | `23db17d1-4de6-4664-b48f-9e1ee79c7094` | Track and Recording matched |
| `28579bdf-d63c-428b-80e3-6a9729b9da94` | Rich Kid → Rich Kid; ?/3 → 1/3 | 276898 → 280000 | `cebfd7ea-1c87-4d52-b184-ec9ffc975bcb` | `de753cc4-c959-46a0-b445-e9978ee42c29` | Track matched; Recording withheld for duration |
| `cf38d1b7-572c-4cd4-9265-41ee910bde1e` | Rich Kid → Rich Kid; ?/3 → 1/3 | 276898 → 280000 | `cebfd7ea-1c87-4d52-b184-ec9ffc975bcb` | `09f6caba-a02f-333e-aea6-0b29c81136db` | Same evidence; tie retained |
| `19d85dc0-3cdc-415c-b62e-7c29630d83c0` | Super Fx → Super Fxx; 1/12 → 1/12 | 191373 → absent | `9b36fa32-bd94-48cd-b922-c2243fcba8ce` | `932cf884-8691-4858-8ece-9dc585ca338f` | Program excluded: existing Recording contradiction |
| `830af860-6bc2-4e2c-b683-7635a25f30b8` | Super Fx → Super Fx; 1/12 → 1/12 | 191373 → 191373 | `f5d57454-cb8b-42bd-9c13-c32a38a1dd99` | `ebb8845f-469a-40ff-a302-7ff3e3363bd7` | Confirms local Recording; program retained |
| `af6cdcde-6c1e-46b9-af17-33f3f27ae5fb` | Super Fx → Super Fx; 1/12 → 1/12 | 191373 → 191000 | `f5d57454-cb8b-42bd-9c13-c32a38a1dd99` | `7426d76a-8f9d-4a34-902d-06b89d996e3e` | Same identity; tie retained |

`Rich Kid` comparison form is `rich kid` on both sides. `Super Fx` and `Super Fxx`
remain distinct comparison forms `super fx` and `super fxx`; no special equivalence
was added. The standard Tera Melos programs each agree on 12/12 present Tracks.
The LP agrees on 11/12, with a Recording contradiction at the last position.
Synthetic tests also prove preference from 12 exact positions versus 11 exact plus
one typo when embedded Recording identities are absent.

Hella's four local Tracks now have these outcomes in both sampled programs:

| Track | Local → provider milliseconds | Outcome |
| --- | --- | --- |
| Ho's in the House | 86857 → 87000 | Track + Recording identified |
| Bitches Ain't Shit but Good People | 432745 → 434000 | Track + Recording identified |
| Rich Kid | 276898 → 280000 | Track associated; Recording `DurationMismatch` |
| D. Elkan Sings Republic of R & R | 226168 → 227000 | Track + Recording identified |

The last provider title uses `R+R` in one program and `R & R` in the other. Existing
punctuation comparison remains supported alongside conjunction equivalence. Display
selection prefers an exact comparison title and then lexical title order, never a
provider score or candidate order. Local metadata remains unchanged.

## Policy and bounds

Only a supported noncontradicting program can exclude programs with existing strong
Recording contradictions. For metadata preference, require at least three distinct
exact positions, more exact agreements than the weaker program, no weaker evidence
for any present Track, and no additional duration mismatches. Ties and tradeoffs are
retained. Missing local Tracks are not counted against any program.

Unique exact normalized Track titles now establish Recording identity even with a
duration mismatch. The unchanged three-second tolerance applies to weaker or
duplicate-title inference; duration mismatches remain visible in probe diagnostics.
Genuine Recording disagreement also withholds identities while an otherwise clear
Track association remains useful. Different fuzzy titles without a common identity
or exact local-title anchor remain Track-ambiguous.

The normal request bound is unchanged: one browse, up to three program lookups,
and one fallback browse only if no Official candidates exist. No per-Track requests.
The measured Hella run used three logical requests. Tera Melos used four (six HTTP
attempts including two transient 503s); Wretches used two program requests (four
attempts), plus a diagnostic-only Album search (two attempts). The normal matcher
already knows the Album ID and does not need that search.

Opt-in `recording_probe FOLDER CONFIRMED_GROUP_ID --programs` prints all local/provider
positions, original and comparison titles, durations, Recording/occurrence IDs,
consideration reasons, and selected program indices. `query:...` in place of the
confirmed ID is diagnostic-only discovery and proceeds only for a single result.
Automated regressions use synthetic provider evidence, not the public service.

Local release-build comparison including template selection against three programs
took approximately 62 µs / 636 µs / 1.81 ms for 5/15/30 local Tracks. The 200k-Track /
20k-Album fixture retained indexed Album candidate lookup at 6.15 µs, targeted
Track corroboration at 14.42 µs, and a first library page at 140 µs. No storage
schema, query path, network bound or playback behavior changed in this refinement.
