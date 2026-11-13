# P41: time limit and cancel for the read-only operations

Status: bounded. `chem.aromaticity`, `chem.identity`, `chem.substructure` and `chem.properties` take the `time_limit_ms` of [CONTRACT.md "Computation time limit and cancel"](../CONTRACT.md#computation-time-limit-and-cancel) and the host cancel. They take no compute-gate slot. SMILES export and (since #106, 2026-11) `chem.stereo` and `chem.inchi` (one question before its native call) have them too (sections below). This is the first pull request of track T0; the plan, slicing and the lead's decision are in HANDOFF.ko.md ("T0 진행").

## What changed

- Domain: `identity::canonicalize_with` and `canonicalize_chemical_with`, `substructure::find_with`, `Composition::distribution_with`. Each returns `Result<Result<T, ModelError>, Interrupted>` and the existing function is its wrapper over `NEVER`. A private `Halt` keeps a stop from being read as a refusal; no error type gained a variant. Checkpoints ask the interrupt and count nothing, so results below the limit are bit-identical (`steps` of `chem.substructure` included).
- Application: `compute_read_only` (one check, computation outside the lock) and `stopped_read_only` (the stop reply); each operation reads `time_limit_ms` first, then the snapshot, then computes. `properties::calculate_with` checks per component.
- Decision (lead): the four operations take no compute slot. The first plan (Opus) gave them one, like `chem.cip`; Codex and the code-review both objected that a quick inspection could be answered `time_limit` behind long conversions while the web shows no `time_limit`. Giving them a slot later is a tightening that needs its own decision; the web display has since been delivered (`p41-web-stop-display.md`). Open for the user's confirmation.

## Tests

- `crates/domain/tests/identity_interrupt.rs`, `substructure_interrupt.rs`, `mass_interrupt.rs`: the same result as the wrapper for a never-stopping interrupt (refusals included), a stop at every checkpoint, the question counts that pin the checkpoints, the stride.
- `crates/application/tests/read_only_time_limit.rs`: one table of five requests (aromaticity, identity recorded and chemical, substructure, properties): results unchanged below the limit and with a never-firing cancel; limit 0 and a cancel give the stop reply with the operation's `model_id` and the analyzed snapshot's revision; a limit above the maximum is refused with the `model_id` before the document is read.
- `crates/application/tests/read_only_without_slot.rs`: with the only slot held by a latched conversion, each request still completes with its normal result (so it takes no slot); a host cancel gives `cancelled` with the operation's `model_id` and the analyzed revision; after `cancel_computations` each gives `cancelled` while `chem.validate` still works.
- `scripts/contract-smoke.mjs`: the four operations accept `time_limit_ms` over HTTP and the CLI (`0` gives `time_limit`, `8001` gives `invalid_request`).

## Mutations (each in a private copy)

See the PR comment for the observed failing assertions. Killed: no stride check; the stop mapped to a refusal; the interrupt replaced by `NEVER` in the domain call; `compute_read_only` without the check, or with `gate.enter` (the without-slot case fails); `start_time_limit` after the snapshot; the arms of `interruption()` swapped; no shutdown flag. Survivors are named in the PR comment.

## Not covered

- Stretches without a checkpoint are not measured (CONTRACT lists them).
- A deadline test that depends on the wall clock is not used; expiry is forced with a limit of 0 or a latch.
- SMILES export (second PR), lock placement (third PR, `p41-lock-placement.md`) and the web display (fourth PR, `p41-web-stop-display.md`) were delivered later; `chem.validate` stays under the lock; small items remain in issue #106.

## SMILES export (T0, second pull request)

Status: bounded. `document.export` with `format: "smiles"` (recorded and canonical) takes `time_limit_ms` and the host cancel, with no compute slot. Other export formats have no limit.

- Domain: `identity::chemical_form_with` (the `ChemicalForm` the writer uses; `chemical_form` is its `NEVER` wrapper). Provider: `analysis::interpret_with`; the writer carries a private `Halt` and `write_with`/`interruptible`; five checkpoints at stage boundaries (each render pass and before each read-back parse, each stored stereo record, before directional-bond placement, before the final stereo read-back). Since #106 (2026-11) each read-back parse also asks through `reader::parse_with`: in the aromaticity interpretation of a text with aromatic bonds and once after it; `Halt`, `interruptible` and `uninterrupted` moved to `smiles/mod.rs`, shared by reader and writer. Port: `SmilesCodec::write_interruptible`, required, no default. Application: `time_limit_ms` on the SMILES export parameters, order `not_running`, limit refusal, snapshot, write, consent; a stop returns before the losses and the consent check.
- Tests (new files): `crates/domain/tests/chemical_form_interrupt.rs`, `crates/providers/tests/smiles_write_interrupt.rs` (the interruptible write equals `write_with_groups` below the limit, a stop at the counted checkpoints, stop versus refusal), `crates/application/tests/smiles_export_time_limit.rs` (limit order, `model_id`, stop reply, no losses or digest, consent order, both styles, cancel), SMILES rows in `read_only_without_slot.rs` (no slot), a loop in `scripts/contract-smoke.mjs` (HTTP and CLI).
- Mutations (private copies, observed failing assertions in the PR comment): the checkpoints removed, a stop returned as `CanonicalLimit`, the style ignored in `write_interruptible`, a stop returned as a result, no limit armed, the limit read after the document, the write given `NEVER`, losses or `model_id` on the stop reply, the consent refusal before or inside the stop.
- Not covered: the token pass, ring perception and stereo/CX reading of each read-back parse (up to 7 per export) and the other stretches listed in CONTRACT have no checkpoint; no latency is promised.

## chem.stereo (T0, #106)

- Domain: `stereo_inspection::inspect_with(document, plan, instances, interrupt)`; `inspect` is its `NEVER` wrapper, with a private `Halt` (no public error type gains a variant). Checkpoints: the start of each instance, each center entry, each double-bond entry, once before the closing findings (n×(1+a+b)+1 questions).
- Application: `time_limit_ms` on `StereoParameters`, the dispatcher passes the host cancel; order: limit, `instance` parse, snapshot, membership, plan, the 20,000-entry `resource_limit`, then `compute_read_only`; no compute slot; a stop is `stopped_read_only` with `chemifree-stereo-1`.
- Tests: `crates/domain/tests/stereo_inspection_interrupt.rs` (an answering interrupt gives `inspect`'s result; a stop at every question for both reasons; the exact question count for 0, 1 and 2 instances). A `chem.stereo` row in `crates/application/tests/read_only_time_limit.rs` and `read_only_without_slot.rs` (unchanged below the limit, limit 0, cancel over an expired limit, an unknown document refused by an over-maximum limit, a stop inside, no slot, host cancel and `cancel_computations`). Catalog wording and `scripts/contract-smoke.mjs` include it.
- Mutations, each caught: the per-instance, per-entry or final checkpoint removed; the application passing `NEVER`; a stop mapped to `document_unreadable`; the limit read after the snapshot; a slot taken with `compute_gate.enter`.
- Not covered: the stretches named in CONTRACT ("`chem.stereo`" bullet); no latency is promised.

## chem.inchi (T0, #106)

- Application only: `time_limit_ms` on `InchiParameters`, the dispatcher passes the host cancel. Order: limit (an over-maximum limit is refused before `inchi_unavailable`), `inchi_unavailable`, snapshot, group and provider-identity refusals, then `compute_read_only` asks once and runs `provider.generate(graph, groups, stop)`; the native binding asks `stop` again once it holds `NATIVE_CALL` (`platform::locked`), before any output exists. No check after the call; no slot; the consent digest is unchanged. The `InchiProvider` port gains the interrupt and a nested `Result` (a stop is never an `InchiError`). Option A of the Opus plan (HANDOFF); a per-request `-W` (B) and process isolation (D) are user decisions.
- Tests in `crates/application/tests/inchi_operation.rs` (fixture provider with a call counter): a zero limit and a host cancel stop before the provider is called; 8001 is refused before the document and the provider, also with no provider registered; the loss-consent digest is the same with and without a limit; a limit the computation meets leaves the reply unchanged. `scripts/contract-smoke.mjs` checks the 8001 refusal through both adapters (CI has no InChI library).
- Mutations, each caught: no ask before the call; the ask after the call (only the provider-not-called checks catch it); the limit read after `inchi_unavailable`; `time_limit_ms` in the consent bytes; a changed reply below the limit.
- Review fix (PR #266): `platform.rs` unit tests check that a stop seen with the mutex held returns before the call and that the check runs while the lock is held; `inchi_operation.rs` checks a cancel arriving while queued (the provider is never called), no compute slot, and `cancel_computations`.
- Not covered: the native call itself (CONTRACT "`chem.inchi`" bullet).
