# P21 aromaticity model: changed outcomes against main

Model IDs are now `chemifree-aromaticity-2` and `chemifree-chemical-graph-2`. Compared with `crates/providers/examples/aromaticity_diff.rs` on 68 inputs (`crates/providers/tests/data/smiles/aromaticity-diff.smi`, run once with main's `aromaticity.rs` and once with the new one); every input not listed here has an identical `chem.aromaticity` result and chemical-identity outcome before and after the change.

| Input | main | now | Reason |
|---|---|---|---|
| `N1C=CC=C1` | non_aromatic/not_conjugated/5 identity:ok | aromatic/-/5 identity:ok | Pyrrole-type nitrogen (no ring double bond, inferred H, two neighbours) is a donor in Kekule form, matching the aromatic spelling. |
| `N1C=NC=C1` | non_aromatic/not_conjugated/5 identity:ok | aromatic/-/5 identity:ok | Pyrrole-type nitrogen (no ring double bond, inferred H, two neighbours) is a donor in Kekule form, matching the aromatic spelling. |
| `N1C=CN=C1` | non_aromatic/not_conjugated/5 identity:ok | aromatic/-/5 identity:ok | Pyrrole-type nitrogen (no ring double bond, inferred H, two neighbours) is a donor in Kekule form, matching the aromatic spelling. |
| `N1N=CC=C1` | non_aromatic/not_conjugated/5 identity:ok | aromatic/-/5 identity:ok | Pyrrole-type nitrogen (no ring double bond, inferred H, two neighbours) is a donor in Kekule form, matching the aromatic spelling. |
| `C1=CC=C2NC=CC2=C1` | non_aromatic/not_conjugated/9 identity:ok | unsupported/fused_system_outside_model/9 identity:fused_system_outside_model | Same refusal as the aromatic spelling (fused heteroaromatic outside the model); main called the Kekule form non-aromatic. |
| `[CH2]1[CH2][CH2]1` | unsupported/valence_not_in_model/3 identity:valence_not_in_model | non_aromatic/saturated_atom/3 identity:ok | Saturated ring atoms (stated H or four neighbours) no longer make the system unsupported; they cannot conjugate, so the ring is non-aromatic. |
| `CC1(C)CC1` | unsupported/valence_not_in_model/3 identity:valence_not_in_model | non_aromatic/saturated_atom/3 identity:ok | Saturated ring atoms (stated H or four neighbours) no longer make the system unsupported; they cannot conjugate, so the ring is non-aromatic. |
| `[CH2]1[CH2][CH2][CH2][CH2][CH2]1` | unsupported/valence_not_in_model/6 identity:valence_not_in_model | non_aromatic/saturated_atom/6 identity:ok | Saturated ring atoms (stated H or four neighbours) no longer make the system unsupported; they cannot conjugate, so the ring is non-aromatic. |
| `C1CCC2(CC1)CCCC2` | unsupported/valence_not_in_model/6 unsupported/valence_not_in_model/5 identity:valence_not_in_model | non_aromatic/saturated_atom/6 non_aromatic/saturated_atom/5 identity:ok | Saturated ring atoms (stated H or four neighbours) no longer make the system unsupported; they cannot conjugate, so the ring is non-aromatic. |
| `CC1(C)CCCCC1` | unsupported/valence_not_in_model/6 identity:valence_not_in_model | non_aromatic/saturated_atom/6 identity:ok | Saturated ring atoms (stated H or four neighbours) no longer make the system unsupported; they cannot conjugate, so the ring is non-aromatic. |
| `C1CC[NH2+]C1` | unsupported/valence_not_in_model/5 identity:valence_not_in_model | non_aromatic/saturated_atom/5 identity:ok | Saturated ring atoms (stated H or four neighbours) no longer make the system unsupported; they cannot conjugate, so the ring is non-aromatic. |
| `C1CC[N+](C)(C)C1` | unsupported/valence_not_in_model/5 identity:valence_not_in_model | non_aromatic/saturated_atom/5 identity:ok | Saturated ring atoms (stated H or four neighbours) no longer make the system unsupported; they cannot conjugate, so the ring is non-aromatic. |
| `CC1=CCC2CC1C2(C)C` | unsupported/valence_not_in_model/7 identity:valence_not_in_model | non_aromatic/saturated_atom/7 identity:ok | Saturated ring atoms (stated H or four neighbours) no longer make the system unsupported; they cannot conjugate, so the ring is non-aromatic. |
| `CC(C)CCCC(C)C1CCC2C1(CCC3C2CC=C4C3(CCC(C4)O)C)C` | unsupported/valence_not_in_model/17 identity:valence_not_in_model | non_aromatic/saturated_atom/17 identity:ok | Saturated ring atoms (stated H or four neighbours) no longer make the system unsupported; they cannot conjugate, so the ring is non-aromatic. |
| `[CH2]1C=CC=C1` | unsupported/valence_not_in_model/5 identity:valence_not_in_model | non_aromatic/saturated_atom/5 identity:ok | Saturated ring atoms (stated H or four neighbours) no longer make the system unsupported; they cannot conjugate, so the ring is non-aromatic. |
| `[CH2]1C=CC=CC=C1` | unsupported/valence_not_in_model/7 identity:valence_not_in_model | non_aromatic/saturated_atom/7 identity:ok | Saturated ring atoms (stated H or four neighbours) no longer make the system unsupported; they cannot conjugate, so the ring is non-aromatic. |
| `C1=CC=C2C(=C1)CCC2` | non_aromatic/not_conjugated/9 identity:ok | unsupported/fused_partially_aromatic/9 identity:fused_partially_aromatic | Fused system containing a possibly aromatic cycle is refused in every spelling instead of depending on the Kekule drawing. |
| `C1=CC=C2C(=C1)CCCC2` | non_aromatic/not_conjugated/10 identity:ok | unsupported/fused_partially_aromatic/10 identity:fused_partially_aromatic | Fused system containing a possibly aromatic cycle is refused in every spelling instead of depending on the Kekule drawing. |
| `C1=CC=C2C(=C1)CC1=CC=CC=C12` | non_aromatic/not_conjugated/13 identity:ok | unsupported/fused_partially_aromatic/13 identity:fused_partially_aromatic | Fused system containing a possibly aromatic cycle is refused in every spelling instead of depending on the Kekule drawing. |

Unchanged and still correct: benzene, naphthalene, anthracene, phenanthrene, pyridine/pyrrole/furan/thiophene/imidazole/pyrazole in aromatic spelling, norbornene, norbornadiene, octalin, decalin, bicyclo[2.2.2]octane, cycloheptatriene, exocyclic double bonds (`unsupported_external_bond`), `n1cccc1` (pyridine-type, not aromatic).

## Seeded random corpus

`crates/providers/tests/data/smiles/aromaticity-random.smi` holds 2,500 generated inputs (seed 20260501): 1,300 Kekulé monocycles of 3-8 atoms over C, N, O, S, charged and hydrogen-stated atoms, 300 aromatic-spelled monocycles, and 900 bicycles (fused, bridged or spiro, built by closing a path onto a ring atom). Together with the 68 inputs above, 2,561 inputs were read (the rest are refused by the SMILES reader). Each was run through `chem.aromaticity` and chemical identity with main's `aromaticity.rs` and with the new one (`aromaticity_diff` example). 2,442 are identical in classification, reason and identity outcome; 119 changed, in the 19 classes below (counts include the first corpus).

| Count | main (systems / identity) | now (systems / identity) | Example |
|---|---|---|---|
| 26 | unsupported/valence_not_in_model / identity:valence_not_in_model | non_aromatic/not_conjugated / identity:unusual_valence | `[CH-]1S[CH+]=C[CH2]O1` |
| 26 | unsupported/valence_not_in_model / identity:valence_not_in_model | unsupported/radical_state / identity:radical_state | `[CH2]1[NH]C[N-]=[NH+][NH](C)C1` |
| 19 | unsupported/valence_not_in_model / identity:valence_not_in_model | non_aromatic/saturated_atom / identity:ok | `[CH2]1[CH2][CH2]1` |
| 9 | non_aromatic/not_conjugated / identity:ok | aromatic/- / identity:ok | `N1C=CC=C1` |
| 6 | unsupported/valence_not_in_model / identity:valence_not_in_model | non_aromatic/not_conjugated / identity:radical_state | `[C](C)C1=S[CH2]C1` |
| 6 | unsupported/valence_not_in_model / identity:valence_not_in_model | non_aromatic/not_conjugated unsupported/valence_not_in_model / identity:valence_not_in_model | `C1=[N+]NC21C=C=[CH-][NH]2` |
| 5 | unsupported/valence_not_in_model / identity:valence_not_in_model | unsupported/radical_state unsupported/valence_not_in_model / identity:valence_not_in_model | `C1=[N+]CC21[NH]=[CH2]2` |
| 4 | unsupported/valence_not_in_model / identity:valence_not_in_model | non_aromatic/not_conjugated / identity:ok | `[CH2]1N=NNN=SN1` |
| 4 | unsupported/valence_not_in_model / identity:valence_not_in_model | non_aromatic/not_conjugated non_aromatic/saturated_atom / identity:unusual_valence | `C1=N=OC21[NH+]ONC2` |
| 3 | non_aromatic/not_conjugated / identity:ok | unsupported/fused_partially_aromatic / identity:fused_partially_aromatic | `C1=CC=C2C(=C1)CCC2` |
| 2 | non_aromatic/not_conjugated / identity:ok | non_aromatic/electron_count / identity:ok | `C1=CN1` |
| 2 | unsupported/valence_not_in_model / identity:valence_not_in_model | non_aromatic/saturated_atom unsupported/valence_not_in_model / identity:valence_not_in_model | `[CH2]1=N[CH+][N+][NH+]C21NC[CH2]2` |
| 1 | non_aromatic/not_conjugated / identity:ok | unsupported/fused_system_outside_model / identity:fused_system_outside_model | `C1=CC=C2NC=CC2=C1` |
| 1 | unsupported/valence_not_in_model / identity:valence_not_in_model | non_aromatic/saturated_atom / identity:unusual_valence | `N1[NH+][CH2][CH-]1` |
| 1 | non_aromatic/not_conjugated / identity:unusual_valence | unsupported/fused_partially_aromatic / identity:fused_partially_aromatic | `N12=[C](C)CC=N1=O=[NH+]=NN2` |
| 1 | unsupported/unsupported_external_bond unsupported/valence_not_in_model / identity:valence_not_in_model | unsupported/radical_state unsupported/unsupported_external_bond / identity:radical_state | `[CH2]1NC=N[CH+][NH]=C21C[NH]=N[N+]2` |
| 1 | unsupported/radical_state unsupported/valence_not_in_model / identity:radical_state | non_aromatic/not_conjugated unsupported/radical_state / identity:radical_state | `O1=[N-]=[NH][CH2]ONC21C=C=[CH+]2` |
| 1 | non_aromatic/not_conjugated / identity:radical_state | unsupported/fused_partially_aromatic / identity:fused_partially_aromatic | `[C](C)C1O=N2=N1=[CH-]2` |
| 1 | unsupported/valence_not_in_model / identity:valence_not_in_model | non_aromatic/not_conjugated unsupported/radical_state / identity:radical_state | `[C](C)C1[CH+]N[NH+]=N=C[N+]21C[C](C)C=[NH]=[CH-]2` |

Classes, in words:

- `valence_not_in_model` becomes `saturated_atom` (identity ok, or `unusual_valence` when another atom is invalid): saturated ring atoms no longer stop the whole system.
- `valence_not_in_model` becomes `not_conjugated`, `radical_state` or a later atom's reason: the first failing atom in a system used to be a saturated one; the system now reports the next real cause (reason-only changes, including `radical_state` for atoms the SMILES reader marks with an undetermined radical).
- `not_conjugated` becomes `electron_count` (for example `C1=CN1`): a ring that the Kekulé donor rule now lets be fully conjugated but not 4n+2.
- `non_aromatic` becomes `aromatic` for Kekulé rings of donors and conjugated atoms with inferred hydrogens (`N1C=CC=C1`, `N1C=NC=C1`). The donor rule applies to every ring whose atoms are all donors or conjugated, not only azoles: all-heteroatom 3-, 4- and 5-rings such as `N1[NH]N1` or `S1S[CH+]N1` are also read as aromatic when their electron count is 4n+2. This is a property of the bounded model (it counts electrons and does not judge geometry or ring strain).
- `non_aromatic` becomes `unsupported/fused_partially_aromatic` or `fused_system_outside_model` for Kekulé fused systems, as in the first corpus.

Aromatic conversion no longer changes these answers: it states the hydrogens of donor nitrogens in the same edit (`every_kekule_ring_the_model_calls_aromatic_survives_conversion_both_ways`).

## Kekulé benzene fused to saturated rings

main reported each of these as `non_aromatic/not_conjugated` (the benzene ring silently non-aromatic) with chemical identity ok. They are now `unsupported/fused_partially_aromatic` with identity refused for the same reason; the test `kekule_benzene_fused_to_saturated_rings_is_unsupported_never_silently_non_aromatic` asserts it.

| Input | main | now |
|---|---|---|
| tetralin `C1=CC=C2CCCCC2=C1` | non_aromatic/not_conjugated, identity ok | unsupported/fused_partially_aromatic, identity refused |
| indane `C1=CC=C2CCCC2=C1` | same | same as above |
| benzodioxole `C1=CC=C2OCOC2=C1` | same | same as above |
| indoline `C1=CC=C2NCCC2=C1` | same | same as above |

