# P41: time limit and cancel for four read-only operations

Status: bounded. `chem.aromaticity`, `chem.identity`, `chem.substructure` and `chem.properties` take the `time_limit_ms` of [CONTRACT.md "Computation time limit and cancel"](../CONTRACT.md#computation-time-limit-and-cancel), the host cancel and a compute-gate slot. SMILES export, `chem.stereo` and `chem.inchi` do not have them yet. This is the first pull request of track T0; the plan, slicing and the lead's decision are in HANDOFF.ko.md ("T0 진행").

## What changed

- Domain: `identity::canonicalize_with` and `canonicalize_chemical_with`, `substructure::find_with`, `Composition::distribution_with`. Each returns `Result<Result<T, ModelError>, Interrupted>` and the existing function is its wrapper over `NEVER`. A private `Halt` keeps a stop from being read as a refusal; no error type gained a variant. Checkpoints ask the interrupt and count nothing, so results below the limit are bit-identical (`steps` of `chem.substructure` included).
- Application: `compute_read_only` (compute slot, one check, computation outside the lock) and `stopped_read_only` (the stop reply); each operation reads `time_limit_ms` first, then the snapshot, then computes. `properties::calculate_with` checks per component.
- Decision (lead): the four operations take a compute slot, like `chem.cip`.

## Tests

- `crates/domain/tests/identity_interrupt.rs`, `substructure_interrupt.rs`, `mass_interrupt.rs`: the same result as the wrapper for a never-stopping interrupt (refusals included), a stop at every checkpoint, the question counts that pin the checkpoints, the stride.
- `crates/application/tests/read_only_time_limit.rs`: one table of five requests (aromaticity, identity recorded and chemical, substructure, properties): results unchanged below the limit and with a never-firing cancel; limit 0 and a cancel give the stop reply with the operation's `model_id` and the analyzed snapshot's revision; a limit above the maximum is refused with the `model_id` before the document is read.
- `crates/application/tests/read_only_compute_gate.rs`: with one slot held by a latched conversion, each request waits and is answered `time_limit` with its limit, a cancel while waiting takes no slot, and after `cancel_computations` each gives `cancelled` while `chem.validate` still works.
- `scripts/contract-smoke.mjs`: the four operations accept `time_limit_ms` over HTTP and the CLI (`0` gives `time_limit`, `8001` gives `invalid_request`).

## Mutations (each in a private copy)

See the PR comment for the observed failing assertions. Killed: no stride check; the stop mapped to a refusal; the interrupt replaced by `NEVER` in the domain call; `compute_read_only` without the check or without `gate.enter`; `start_time_limit` after the snapshot; the arms of `interruption()` swapped; no shutdown flag. Survivors are named in the PR comment.

## Not covered

- Stretches without a checkpoint are not measured (CONTRACT lists them).
- A deadline test that depends on the wall clock is not used; expiry is forced with a limit of 0 or a latch.
- SMILES export, `chem.validate` lock placement and the web display remain (issue #106).
