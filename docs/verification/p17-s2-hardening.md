# P17 S2: public cases and small hardening

## Changes

| Item | Change | Check |
|---|---|---|
| Charged ring at the public API | `conformance/cases/aromaticity-charged-ring-round-trip.json`: Kekulé pyridinium (N+) inspected (aromatic, 6 electrons, `chemifree-aromaticity-2`), converted to aromatic and back to the original Kekulé bonds; charge kept | `npm run test:conformance` |
| Refusal leaves the document unchanged | `conformance/cases/aromaticity-adjacent-donors-refused-unchanged.json`: Kekulé 1,2-dihydro-1,2-diazete is `unsupported`/`adjacent_donors`; aromatic conversion is refused (`invalid_command`, `details.reason` `adjacent_donors`); revision, bonds and undo/redo depth unchanged | same |
| Checked lookups in `calculate` | `crates/domain/src/aromaticity.rs`: ring bonds, incidence lists, per-atom orders and atoms are read with `.get(..)` and a missing item halts with `invalid_graph` instead of panicking; CONTRACT "Limits" says so. Of the eight new checks, three sit behind earlier checks of the same items and cannot be reached (the second incidence and bond lookups, and the atom lookup of the fused-block test); the unit test `a_ring_naming_an_item_the_context_lacks_is_invalid_graph_not_a_panic` covers the five reachable ones, including a ring that lists one atom twice (the per-atom orders lookup) | `cargo test -p chemifree-domain` (817 passed) |
| Valence targets pinned | `crates/domain/tests/aromaticity_valence_targets.rs`: `restricted_target` gives C0 4, C+ 3, C- 3, N0 3, N+ 4, N- 2, O0 2, S0 2 (CONTRACT "Atom role"); P0 has a target (3) in the shared table, so the test instead asserts a ring with P is `unsupported`/`element_or_charge` | `cargo test -p chemifree-domain --test aromaticity_valence_targets` |

Conformance totals: 123 cases (accept 90, reject 33), report regenerated with `npm run conformance:record`.

## Mutations

| # | Mutation | Result |
|---|---|---|
| B1 | the ring-bond lookup reverted to `context.bonds[&id]` | the unit test fails with a panic at that line |
| B2 | the per-atom orders lookup reverted to `.expect(..)` | the duplicate-atom case of the unit test fails with a panic at that line |
| A1, A2 | the charged case's expected revision and the diazete case's reason changed (scratch copies) | each case fails |
| C1 | N's valence list swapped in `chemistry.rs` | not run: the automatic permission check refused the mutated test run, and it was not routed around. The test asserts N0 = 3 literally, so the swapped first entry would fail it, but this is unproved |
