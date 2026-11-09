# Testing and isolation practice

This describes how Chemifree is tested, as a method. It lists no results; a run's counts and outcomes belong to its run record (see [docs/verification/](verification/)). The commands are the ones in the [README](../README.md#check-and-build) and `package.json`; this page does not repeat what they establish.

## 1. A test must be shown to fail

Passing proves little until the test has failed for the right reason. For every new predicate or guard:

1. Write the test and see it pass.
2. Break the guarded code (invert the condition, drop the check, widen the limit) and see the test fail.
3. Restore the code and see it pass again.

These mutation checks are run by hand. They do not use build caches: a stale cached artifact can hide the break, so the mutated tree is built from scratch. The same idea appears in the repository's own records, for example the browser CI record that includes a deliberately broken build that failed the job ([docs/verification/browser-ci.json](verification/browser-ci.json)).

## 2. Determinism

- Tests supply their own boundaries. The contract smoke starts the API on an ephemeral loopback port with a temporary data directory; the web client check runs against a scripted service rather than a live one (`npm run test:contracts`, `npm run test:client`).
- The target practice is that sleeps are not synchronization: wait on an observable condition (a response, a file, a process exit), not on elapsed time. The suite does not yet meet this. In `scripts/contract-smoke.mjs` the lock waits now poll with a deadline (`waitUntil`). The other `sleep` calls are poll ticks (`waitUntil`, 20 ms; `untilSession`, 100 ms). Two fixed-time sleeps remain:
  - `sleep(1_000)` checks that a second save is still running after a second while the holder keeps the lock. It remains because a save blocked on the document lock waits in `wait_for_lock`, which emits nothing observable; replacing it needs a production signal from `wait_for_lock` that a save is waiting.
  - `sleep(400)` paces the edits in the continuous-edit loop. It assumes no gap between edits reaches the 2 s quiet period, so only the 10 s maximum wait starts the draft; the loop then requires the draft between 9.5 s and 13 s after the edits begin.

  The remaining fixed-time sleeps are not a model to copy and are candidates for replacement by a condition wait.
- A timing flake is a finding. Find the race and fix it; do not retry it away. CI runs the browser checks once, without retries, and the Playwright race behind issue #8 has its own check (`npm run test:browser:routes`).

## 3. Hostile but legal inputs and boundaries

Test the edges of what the contract accepts: documented limits, empty input, the maximum, and input that is malformed but still parseable. The format readers refuse what they cannot read and report losses, so tests should assert both the refusal code and that a refusal leaves the document unchanged (same revision, same content). A refusal that mutates state is a defect even when the error message is right.

## 4. Independent oracles

Where a result is chemistry, a test written from the same understanding as the code proves consistency, not correctness. Compare against an independent reference where one exists, and record whether the comparison is hand-run or automated. Each script's own header states its usage and what it compares; read that, not this page.

- (a) Run Chemifree (a built CLI or example, named in the header) and compare its output with RDKit 2022.03.2: `verify-query-rdkit.py`, `verify-smiles-rdkit.py`, `verify-v2000-rdkit.py`, `verify-v3000-rdkit.py`. Each fixture states its expected outcome and a mismatch fails; `verify-v3000-rdkit.py` checks one CIP label against a hand derivation, not against Chemifree. The `verify-v2000-rdkit.py` header is short; it says only that it compares RDKit and Chemifree V2000 interpretations.
- (a), newer RDKit: `verify-smiles-stereo-rdkit.py` compares Chemifree's SMILES stereo read with RDKit's CIP labels and, for CXSMILES, the enhanced stereo groups. Its header says the group check needs an RDKit release that reads the `a:`, `o<n>:` and `&<n>:` fields, so it does not run on 2022.03.2; it prints the version used, and `docs/CONTRACT.md` records that the run for that change used RDKit 2026.09. It compares some fixtures on RDKit's side alone (`rdkit_only`).
- (b) Validate only the RDKit or independent half of a fixture that a Rust test then checks; they run no Chemifree code. `verify-stereo-rdkit.py` builds its own molblock and SMILES from the stereo fixtures and compares them with each fixture's hand-stated configuration; `crates/application/tests/stereo_fixtures.rs` proves Chemifree derives that configuration. `verify-cdxml-rdkit.py` does the same for a CDXML spelling written and read by the script itself (RDKit has no CDXML reader in that version); the Rust importer is checked by `crates/application/tests/cdxml_stereo_import.rs`. `verify-cip-rdkit.py` runs RDKit's CIP labeler on the SMILES of `crates/domain/tests/cip-fixtures.json` and checks it against the labels the Rust domain test proves, plus mirror-image and atom-order checks on RDKit alone.
- Hand-run status: the headers of `verify-cdxml-rdkit.py`, `verify-cip-rdkit.py`, `verify-smiles-stereo-rdkit.py`, `verify-stereo-rdkit.py` and `verify-v3000-rdkit.py` say hand-run and not part of `npm run check`; the README says so for the query comparison. The `verify-smiles-rdkit.py` and `verify-v2000-rdkit.py` headers do not say; none of these scripts is run by CI.
- Independent readers without RDKit: `verify-cdx-semantics.py` (a separate CDX parser) and `verify-cdxml-xml.py` (Python's standard-library XML reader), each over one bounded fixture.
- `aromaticity_corpus.py` only generates an RDKit-derived aromaticity corpus (needs RDKit); it does not run Chemifree. The Rust side consumes the generated corpus (`crates/providers/tests/aromaticity_corpus_example.rs`), and `aromaticity_corpus_diff.py` compares two runs of that example. See `docs/verification/p21a-aromaticity-changes.md`.

A comparison supports only the fixtures it covers; no wider equivalence is claimed.

## 5. Heavy commands run isolated (local practice)

This is the practice for local development and review runs. CI is different: `.github/workflows/ci.yml` runs `npm run check`, `npm run test:browser:routes` and `npm run test:browser` directly on the hosted runner, under that runner's own limits, with network for installing dependencies.

Locally, full builds, the whole test suite, mutation runs and external-tool scripts run in an isolated, resource-capped environment:

- no network unless a step declares it needs one;
- a private temporary directory and home;
- work on a copy of the worktree, so the real tree is not changed;
- only declared files are exported out;
- the caps are never raised to make a run pass; a run that hits a cap is reported as such;
- peak memory is reported with the result;
- no scratch directories, logs or processes are left behind.

The concrete limits and the wrapper script are machine-local. They are kept out of this repository and are not documented here; nothing in the repository enforces them.

## 6. Gates

Each is a documented command; see the README and CI (`.github/workflows/ci.yml`) for what each covers.

| Gate | Command |
| --- | --- |
| Format | `cargo fmt --all --check` (inside `npm run check:rust`) |
| Lint, Rust | `cargo clippy --workspace --all-targets --locked -- -D warnings` (inside `npm run check:rust`) |
| Rust tests | `cargo nextest run --workspace --locked --no-fail-fast`, then `cargo test --workspace --doc --locked` for doctests (inside `npm run check:rust`) |
| Lint, web | `npm run lint` |
| Types, web | `npm run typecheck` |
| Nextest overrides | `npm run test:nextest-overrides` (every exact `test(=...)` name in `.config/nextest.toml` must match a listed test; its unit tests run in `npm run test:checks`) |
| Web client | `npm run test:client` |
| Web build | `npm run build:web` |
| HTTP/CLI contracts | `npm run test:contracts` |
| Conformance | `npm run test:conformance` |
| Traceability | `npm run test:traceability`, `npm run test:checks` |
| Everything above | `npm run check` |
| Browser smoke | `npm run test:browser`, `npm run test:browser:routes` |

Pointers: [CONTRACT.md](CONTRACT.md) for behavior, [TRACEABILITY.ko.md](TRACEABILITY.ko.md) for the traceability check, and [verification/](verification/) for run records.
