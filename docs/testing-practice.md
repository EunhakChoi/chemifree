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
- Sleeps are not synchronization. Wait on an observable condition (a response, a file, a process exit), not on elapsed time.
- A timing flake is a finding. Find the race and fix it; do not retry it away. CI runs the browser checks once, without retries, and the Playwright race behind issue #8 has its own check (`npm run test:browser:routes`).

## 3. Hostile but legal inputs and boundaries

Test the edges of what the contract accepts: documented limits, empty input, the maximum, and input that is malformed but still parseable. The format readers refuse what they cannot read and report losses, so tests assert both the refusal code and that a refusal leaves the document unchanged (same revision, same content). A refusal that mutates state is a defect even when the error message is right.

## 4. Independent oracles

Where a result is chemistry, a test written from the same understanding as the code proves consistency, not correctness. Compare against an independent reference where one exists. The `scripts/verify-*-rdkit.py` scripts and `scripts/aromaticity_corpus.py` compare Chemifree with an RDKit Python, and each states an expected outcome per fixture so nothing passes silently. Record whether a comparison is run by hand or automated: these RDKit scripts are run by hand (they need that RDKit and a built binary), and no equivalence is claimed beyond the fixtures compared. See the README for the specific scripts and their usage.

## 5. Heavy commands run isolated

Full builds, the whole test suite, mutation runs and external-tool scripts run in an isolated, resource-capped environment:

- no network unless a step declares it needs one;
- a private temporary directory and home;
- work on a copy of the worktree, so the real tree is not changed;
- only declared files are exported out;
- the caps are never raised to make a run pass; a run that hits a cap is reported as such;
- peak memory is reported with the result;
- no scratch directories, logs or processes are left behind.

The concrete limits and the wrapper script are machine-local. They are kept out of this repository and are not documented here.

## 6. Gates

Each is a documented command; see the README and CI (`.github/workflows/ci.yml`) for what each covers.

| Gate | Command |
| --- | --- |
| Format | `cargo fmt --all --check` (inside `npm run check:rust`) |
| Lint, Rust | `cargo clippy --workspace --all-targets --locked -- -D warnings` (inside `npm run check:rust`) |
| Rust tests | `cargo test --workspace --locked` (inside `npm run check:rust`) |
| Lint, web | `npm run lint` |
| Types, web | `npm run typecheck` |
| Web client | `npm run test:client` |
| Web build | `npm run build:web` |
| HTTP/CLI contracts | `npm run test:contracts` |
| Conformance | `npm run test:conformance` |
| Traceability | `npm run test:traceability`, `npm run test:checks` |
| Everything above | `npm run check` |
| Browser smoke | `npm run test:browser`, `npm run test:browser:routes` |

Pointers: [CONTRACT.md](CONTRACT.md) for behavior, [TRACEABILITY.ko.md](TRACEABILITY.ko.md) for the traceability check, and [verification/](verification/) for run records.
