# P17 S1: corpus invariance under renumbering and spelling

Tests only; no production change. New file `crates/providers/tests/aromaticity_corpus_invariance.rs` over `docs/verification/aromaticity-corpus.json` (196 records: 116 Kekulé, 80 aromatic spellings; 80 named Kekulé/aromatic pairs).

## What it checks

| Test | Rule |
|---|---|
| `corpus_results_do_not_depend_on_numbering` | Each record rebuilt under two fixed renumberings (atoms and bonds reversed with bond ends swapped; odd indices first) gives the same systems (original atom indices, classification, reason, electrons, cycle rank), `aromatize`/`kekulize` outcome, conversion outcome per target (command count, the original bonds each edit updates and, for the aromatic edit, each updated atom with its edit — the stated donor hydrogens, 35 such edits across the corpus numberings) and chemical identity code or refusal as the stated numbering; exactly 392 comparisons are asserted. Which updated bonds a Kekulé conversion makes double follows the numbering (the reversed numbering of the aromatic toluene record picks the other benzene alternation), so only the updated set is compared. |
| `kekule_and_aromatic_spellings_of_a_molecule_agree` | 74 pairs: equal systems, identity and `aromatize` outcome (`kekulize` is not compared: a Kekulé spelling has nothing to assign); the aromatic spelling converts to Kekulé exactly when the Kekulé spelling converts to aromatic, with equal refusal reasons otherwise, and classifies the same, with the same identity, after that conversion. The six pairs of `DOCUMENTED_SPELLING_DIFFERENCES` (azulene, biphenylene, heptalene, cyclotriazane, trioxadithiolane, triazane fused to a CH2 ring) have each spelling's systems, identity and conversion outcome pinned; CONTRACT "Ring and aromaticity interpretation" now states these differences (the earlier sentence claimed equal classification apart from stated hydrogens). |
| `no_corpus_record_reaches_the_search_limit` | No system under any numbering ends at `search_limit` (that reason may legitimately depend on numbering). |

## Commands

- `cargo test -p chemifree-providers --test aromaticity_corpus_invariance`: 3 passed.
- `cargo fmt --all -- --check`, `cargo clippy -p chemifree-providers --tests -- -D warnings`: pass.

## Mutations

| # | Mutation | Caught by |
|---|---|---|
| M1 | `calculate` refuses the first ring atom when it is N (`element_or_charge`) | numbering test: "morpholine (kekule) under the reversed numbering with ends swapped: differs in [systems, aromatize, to_aromatic, identity]" |
| M2 | the assignment success branch reports `electrons + 2` | spelling test on 1,2,3-triazole: "system keys differ between the spellings" |
| M3 | the test drops aromatic records | spelling test count: "0 pairs agree outside the documented table; expected 74" |
| M4 | the renumbering ignores `swap_ends` | survives (expected: perception does not depend on bond direction); recorded, not a missing check |
| M5 | one pinned reason changed (cyclotriazane aromatic `adjacent_donors` → `not_conjugated`) | spelling test: "cyclotriazane (aromatic): systems differ from the documented outcome" |

Mutation builds used the shared build cache (AGENTS.md: cached builds do not change test results); each mutation was restored and checked by hash.

## Review fixes

After the independent review the test also compares `aromatize` between spellings, the updated bond set of the Kekulé conversion across numberings, the identity after the aromatic spelling's Kekulé conversion, and the exact comparison count (392). CONTRACT's "Block with saturated atoms" sentence was narrowed to the six named molecules (an all-donor ring with a 4n count spelled aromatic is `non_aromatic`, `electron_count`, its identity refused as `aromaticity_unsupported`), and the older `mixed_ring_bonds` parenthetical now names only an aromatic bond that closes no cycle of contributing atoms, matching the code.
