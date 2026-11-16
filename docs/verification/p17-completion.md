# P17 completion (bounded): ring perception, aromaticity and Kekulé conversion

Roadmap acceptance: "재번호·동등 표현·융합계·전하·실패 사례를 검사한다. 변환 실패가 원래 구조를 바꾸지 않고 모델·정책이 결과에 포함된다." Closed within the scope documented in docs/CONTRACT.md "Ring and aromaticity interpretation" (`chemifree-aromaticity-2`), delivered by #82 (with model fixes #91), #284 (S1) and #285 (S2).

| Criterion | Evidence |
|---|---|
| Renumbering | `crates/providers/tests/aromaticity_corpus_invariance.rs` `corpus_results_do_not_depend_on_numbering`: 196 corpus records × 2 fixed renumberings (392 comparisons) give the same systems, conversion edits (bonds; atoms for the aromatic edit) and chemical identity; `crates/domain/tests/aromaticity.rs` (naphthalene), `crates/domain/tests/rings.rs`. The Kekulé alternation chosen follows the numbering; only the updated bond set is compared (docs/verification/p17-s1-corpus-invariance.md). |
| Equivalent representations | the same file, `kekule_and_aromatic_spellings_of_a_molecule_agree`: 74 of 80 Kekulé/aromatic pairs agree; the six whose refusals depend on the spelling are pinned and stated in CONTRACT; `crates/providers/tests/aromaticity_corpus_example.rs` (Kekulé → aromatic → Kekulé); `crates/domain/tests/aromaticity_v2.rs`. |
| Fused systems | `crates/domain/tests/aromaticity.rs` (naphthalene, pyrene, biphenylene), `aromaticity_v2.rs`; corpus classes benzenoid, fused heteroaromatic, fused partially saturated; conformance `aromaticity-partially-aromatic-fused-refused`. |
| Charges | conformance `aromaticity-charged-ring-round-trip` (pyridinium, #285); `crates/domain/tests/aromaticity.rs`, `aromaticity_v2.rs`, `crates/application/tests/aromaticity_operations.rs`; 15 charged corpus records; valence targets pinned in `crates/domain/tests/aromaticity_valence_targets.rs`. |
| Failure cases | `aromaticity_v2.rs` refusal tests; conformance `aromaticity-partially-aromatic-fused-refused`, `aromaticity-adjacent-donors-refused-unchanged`; inconsistent graphs halt with `invalid_graph` instead of panicking (#285). |
| Failure leaves the structure unchanged | `aromaticity_operations.rs` (revision and history kept); the two refusal conformance cases assert revision, bonds and undo/redo depth; `crates/domain/tests/aromaticity_interrupt.rs`. |
| Model/policy in the result | `model_id` asserted in `aromaticity_operations.rs` and every aromaticity conformance case. |

## Documented limits (not done)

- Adjacent donors in a 4n+2 ring or fused block stay `unsupported` (deciding them needs a lone-pair model).
- An aromatic → Kekulé conversion resets a stated nitrogen hydrogen count equal to the inferred one (issue #249); keeping it would need a per-atom "stated by" field, a file-format change, or would break the exact Kekulé → aromatic → Kekulé round trip. Left as a limit (Opus recommendation, option A).
- N(H0) with two single bonds is `non_aromatic` here and `unusual_valence` in chemical identity (documented difference).
- The `expect`/`unreachable!` on the `NEVER` interrupt stay: `Never::check` always returns `Ok`.
- The Kekulé-only branch does not apply the 4n+2 check to fused blocks by design (fused blocks are accepted only as neutral carbon systems covered by six-membered rings; pyrene pinned).
- Broader valence and aromaticity models and query matching remain under MODEL-02.rest, CHEM-02.rest (other rows).

## Traceability

MODEL-02.4, MODEL-02.5, CHEM-02.1 and CHEM-02.2 (bounded parts whose only follow-up was P17) are `implemented`; P17 is removed from MODEL-02.rest, CHEM-02.rest, LAYOUT-03.rest and the `chem_graph` declaration route, which keep their other follow-ups.
