# P19 S1: formula acceptance matrix

Tests only; no production change. New `crates/application/tests/properties_acceptance.rs` runs `chem.properties` (`property: "formula"`) through `Application::execute` on documents built with one `document.transaction`, and checks `model_id` `chemifree-properties-1` (on results), the display decimals and that the document is unchanged.

| Test | Acceptance term | Checked |
|---|---|---|
| `salt_formula_by_scope` | salt | sodium acetate (Na+ with inferred hydrogens, so the monatomic-ion valence row is used): molecule `C2H3NaO2` with formal charge 0; components `C2H3O2-` (−1) and `Na+` (+1) as a set |
| `radical_hydrogen_reduction` | radical | C doublet `CH3`, singlet `CH2`, triplet `CH2`; N doublet `H2N` (no carbon, so Hill order is alphabetical) |
| `charged_radical_is_refused_without_result` | radical, failure | C+ doublet: `unsupported`, `hydrogens_unavailable` naming atom `a1`, no result, document unchanged |
| `hydrogen_modes` | explicit / inferred H | specified C H2 `CH2`; methanol with an explicit H vertex `CH4O`; selection {O} `O`, {O, H} `HO` (an explicit vertex counted once) |
| `isotope_hill_order` | isotope | ethane with one 13C: `C[13C]H6` |
| `hill_order_puts_carbon_and_hydrogen_first` | formula order | chloromethane `CH3Cl`; a lone Cl `ClH` |

Charge and isotope are set with `update_atom` in the same transaction (`add_atom` has neither field). A refused reply carries no `model_id`, so the refusal test does not assert it.

## Mutations

| # | Mutation | Caught by |
|---|---|---|
| a | doublet hydrogen reduction 1 → 0 (`chemistry.rs`) | `radical_hydrogen_reduction`: `CH4` ≠ `CH3` |
| b | the composition's charge sum dropped (`mass.rs`) | `salt_formula_by_scope`: components without charge |
| c | `contains_carbon` always false (`mass.rs`) | `hill_order_puts_carbon_and_hydrogen_first`: `CClH3` ≠ `CH3Cl` |
| d | specified hydrogens counted as 0 (`mass.rs`) | `hydrogen_modes`: `C` ≠ `CH2` |

Commands: `cargo test -p chemifree-application --test properties_acceptance` (6 passed), fmt, clippy.
