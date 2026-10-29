# Chemifree

An independently authored, open-source chemical drawing workspace for humans and agents.

**Early stage.** The first vertical slice works end to end in the browser, over HTTP, and in the CLI: **create a document → add and edit atoms and bonds → undo/redo → save → reopen**, with saves serialized across processes, autosaved recovery drafts that survive a crash, and save as copy. Documents are drawings: Chemifree records one chemical graph per document, atoms (element or query atom, charge, isotope, and stated hydrogens, radical, and attachment points) and bonds (single, double, triple, aromatic, dative, query, unknown), apart from its scene, where one or more placements (scene instances) each hold their own atom positions, and does **not** compute valence, implicit hydrogens, aromaticity, stereochemistry, layout, names, or properties, or match queries: those states are recorded as drawn, and an atom whose hydrogens are left to be inferred shows no count. Chemical calculations, import/export (MOL, SMILES, CDXML…), model loading, user authentication, and Windows integration are **not implemented**. There are no simulated chemical results, vendor engines, or proprietary datasets in this repository.

Continuing in a new session? Start with [the handoff](docs/HANDOFF.ko.md) and [project instructions](AGENTS.md). They record the completed baseline, current limits, and the proposed next implementation slice.

## Run locally

Use Node **22.22.2**, npm **11.13.0**, and Rust **1.95.0**. The repository includes `.nvmrc`, `packageManager`, and `rust-toolchain.toml`.

```bash
npm ci
npm run dev
```

Open **http://127.0.0.1:5173**. This starts Vite and the Rust API together; Vite proxies `/api` to **127.0.0.1:3001**. Stop both with Ctrl+C. Choose **New document**, draw with the bond tool (click to start a chain, click an atom to grow it, drag to bond, click a bond to cycle its order), relabel atoms with the atom tool, inspect and edit charge, isotope, atom kind (element or query `*`, `A`, `Q`, `X`), stated hydrogens, radical, attachment points, and bond kind (including aromatic, dative, query, and unknown) with the select tool, then **Save** (Ctrl/⌘+S). **Duplicate placement** (under the inspector) draws the same graph again, copied from the last placement and shifted down and right; each placement moves on its own with its arrow buttons, dragging atoms moves them only in the placement where the drag starts, and chemistry edits apply to every placement. Undo/redo: Ctrl/⌘+Z and Ctrl/⌘+Shift+Z. Close and reopen from **Saved documents**, where you can search titles, sort by title or most recent save, rename, and move documents to the **Trash** (and restore them). A document with unsaved changes or a recovery draft is not moved to the trash. Under the title, **File** shows the saved state and **Recovery draft** shows the autosave (1 s after the last change, at least every 5 s while editing). The API also drafts unsaved work on its own, with or without the page (2 s after the last change, at most 10 s after the first), and at a normal stop. If the API stops before you save, reopening the document offers **Recover** (with undo history) or **Discard draft**; if it was saved elsewhere meanwhile, **Recover as copy**. **Save as copy** (the copy icon) commits what is on screen as a new document and opens it; the original keeps its last save. If the original's recovery draft cannot be removed, the original is left open with its unsaved changes instead, and the notice above the copy says so and offers **Open original**; the notice goes away once you leave the copy, or once the page learns that the local service restarted (the page finds out when its next write is refused, so an idle page keeps the notice until then).

Documents are stored as `documents/<id>.json` in the data directory (recovery drafts in `drafts/`, save locks in `locks/`; files of earlier format versions are read and migrated, and the first save over one keeps its original bytes in `migrated/`): `--data-dir` or `CHEMIFREE_DATA_DIR`, otherwise the platform user-data directory (`$XDG_DATA_HOME/chemifree` or `~/.local/share/chemifree` on Linux, `~/Library/Application Support/chemifree` on macOS, `%APPDATA%\chemifree` on Windows). The API prints the path at startup.

The first API launch compiles Rust dependencies. If that takes longer than the UI's five-second request timeout, use **Retry connection** once the API is listening. Both development ports must be available.

```bash
# Run entry adapters separately.
npm run dev:web
npm run dev:api

# CLI invokes the shared application library directly; it does not contact HTTP.
cargo run --locked -p chemifree-cli -- status --json
cargo run --locked -p chemifree-cli -- capabilities --json

# Documents through the same application and data directory. Each CLI edit saves.
cargo run --locked -p chemifree-cli -- documents create --title Ethanol
cargo run --locked -p chemifree-cli -- documents apply <id> --expected-revision 0 --commands \
  '[{"op":"add_atom","element":"C","position":{"x":0,"y":0}},
    {"op":"add_atom","element":"C","position":{"x":26,"y":15},"bond_to":{"atom":"$0","order":"single"}},
    {"op":"add_atom","element":"O","position":{"x":52,"y":0},"bond_to":{"atom":"$1","order":"single"}}]'
cargo run --locked -p chemifree-cli -- documents undo <id> --expected-revision 1
cargo run --locked -p chemifree-cli -- documents show <id> --json

# Operation contract 1: one JSON request per line in, one reply per line out.
echo '{"contract":1,"request_id":"r1","profile":"preserve-semantics","operation":"capabilities.inspect","parameters":{}}' \
  | cargo run --locked -p chemifree-cli -- operations
cargo run --locked -p chemifree-cli -- documents save-as <id> --expected-revision 2 --title "Ethanol copy"
# After an API crash: recover the autosaved draft (and save it), or save it as a copy, or drop it.
cargo run --locked -p chemifree-cli -- documents recover <id>
cargo run --locked -p chemifree-cli -- documents recover-as-copy <id>
cargo run --locked -p chemifree-cli -- documents discard-draft <id>

# Optional API port and data directory; the address remains 127.0.0.1.
cargo run --locked -p chemifree-api -- --port 3002 --data-dir ./tmp/chemifree-data
```

The development proxy targets port 3001. If you choose another API port, update `apps/web/vite.config.ts` accordingly. The standalone CLI's `status` confirms its local application can run; it does not report whether an HTTP server is running. The browser holds no document state of its own: each edit is a request with the revision it was based on, and the API's working copy is authoritative until saved. An open API session and the CLI can edit the same file; saves from every process take an OS lock on the document, so a later save from the other side is refused with `save_conflict` rather than overwriting, and a crashed process never leaves the lock held.

## Check and build

```bash
npm run lint             # ESLint: JS/TS + React Hooks checks
npm run typecheck        # TypeScript project references
npm run test:client      # Web client against a scripted service: unknown outcomes, restarts
npm run build:web        # Production web assets in apps/web/dist
npm run check:rust       # rustfmt + clippy (-D warnings) + Rust tests
npm run test:contracts   # Real HTTP/CLI processes: parity, document flow, guards, conflicts, failures
npm run test:conformance # Conformance cases → target/conformance/report.json; exit 1 if any case fails
npm run test:traceability # Traceability files against the roadmap, the recorded run, and npm scripts
npm run test:checks      # node:test tests for the main rules of the traceability check
npm run check            # All of the above
npm run conformance:record   # Rewrite the committed run record (review the change before committing)
npm run build            # Web assets and debug Rust binaries
npm run test:browser     # Browser smoke (needs Chrome or Playwright's Chromium; CI job `browser`)
npm run test:browser:routes  # The Playwright race behind #8 and the smoke's guard against it
```

The contract smoke builds the workspace, starts an API on an ephemeral loopback port with a temporary data directory, and compares HTTP JSON with actual CLI JSON. It runs create → edit → undo/redo → save → close → reopen, checks request-ID replay, revision and save conflicts, write guards, a save that fails because the target path is unwritable and then succeeds on retry, and that a killed API restarts with the last committed document. It then races six CLI saves from one base (exactly one commits), kills a process holding the save lock, races an API save against a CLI save, and runs autosave → SIGKILL → restart → recover/discard/recover-as-copy, an unreadable draft, a failed autosave, and save as copy over HTTP and the CLI. Storage feedback: while the API's save of a document pauses inside its save lock, asking about that save again is answered `request_in_progress` at once, requests queued behind it and a CLI save of the same document are refused as `busy` within the time budget without changing anything, and asking again afterwards returns the committed reply without writing again; a read-only `documents/` is reported as `read_only`. GitHub Actions installs npm 11.13.0 into an isolated runner-temporary prefix, adds its binaries to `PATH`, and verifies the selected version before running `npm ci` and `npm run check` with pinned tools.

The conformance runner (`crates/conformance`, binary `chemifree-conformance [--out PATH] SUITE`) runs the cases in `conformance/cases/` against the shared application with the same request types and arguments as the HTTP handlers and a `request_id` on every write (the HTTP layer is not involved, and status codes are not compared), each case on a fresh temporary data directory; `restart` drops the application without a normal stop, as a crash would. Pending cases name the roadmap item expected to make them runnable and are recorded as `not_run`, never as passed. `docs/traceability/` traces the 33 public readiness requirements (by part, with status, implementation paths, evidence, and follow-ups) and counts the 2,914 public interface declarations per proposed route; a declaration count is not a feature count. The spec files themselves stay outside the repository and are referenced by path and SHA-256 only; `CHEMIFREE_SPEC_DIR=<dir holding spec/> npm run test:traceability` also compares against them. [docs/TRACEABILITY.ko.md](docs/TRACEABILITY.ko.md) states exactly what these checks establish and what they do not (for example, that a status judgement is right, or that the repository holds no private data).

`npm run test:browser` starts the API and Vite on a temporary data directory (ports 3001 and 5173 must be free) and drives the editor with `playwright-core`: drawing, editing, undo/redo with buttons and keys, moving, saving, reopening, the discard prompt, revision-conflict reload, save failure and retry, read-only storage and a document busy behind a CLI's save lock (each named as such), a save whose reply arrives after the page's deadline (confirmed, not failed) and one whose replies are lost (**Save not confirmed**, then **Check again**), a copy still in progress when the page asks again (same request ID, one copy), a save conflict with the CLI, autosave state, crash recovery after killing the API, discarding and recovering drafts as copies, a failed autosave with **Retry now** and a retry that edits do not postpone, draft states after a restart, save as copy (including an original left open because its draft could not be removed, and the notice ending when the page leaves the copy or when an edit reaches a restarted service), a late autosave reply after leaving a document, drafting before leaving a document, a session moved on by another client, a duplicated placement moved on its own (with an atom added to both placements, one dragged in one only, and a third placement copied from the moved second), 320/390 px layouts, and dark mode. Point `CHEMIFREE_BROWSER` at a Chrome/Chromium executable, or run `npx playwright-core install chromium` once. The latest local result is in [docs/verification/editor-browser-smoke.json](docs/verification/editor-browser-smoke.json). The API runs with `--log-requests` (one line per request on standard error; also `CHEMIFREE_LOG_REQUESTS=1`). Every run, passed or failed (including a failed build or server start), leaves its records in `CHEMIFREE_SMOKE_OUT`, or in the temporary directory, which only a passing run removes:

- `results.json`: the steps passed, the last step reached, and the error.
- `build.log`: cargo's diagnostics.
- `environment.json`: the browser, driver, Node, and platform.
- `requests.log`, `api.log`, `web.log`: the browser's requests, the API's request log, and Vite's output.
- The screenshots, plus `failure.png` when a run fails.

A failed run also prints the step and the last requests. Records of an earlier run in `CHEMIFREE_SMOKE_OUT` are removed first; other files there are left alone. Run as root, the steps that make `drafts/` read-only are skipped (permissions do not stop root) and listed in `results.skipped_steps`; `CHEMIFREE_SMOKE_NO_SKIPS=1` makes that a failure.

CI runs both browser checks in the `browser` job, set up as follows:

- **Environment.** Ubuntu 24.04, with the Chromium build that the locked `playwright-core` names (`npx playwright-core install --with-deps chromium`).
- **Runs.** Each check runs once, without retries, with `CHEMIFREE_SMOKE_NO_SKIPS=1`. The smoke also runs when the route check fails.
- **Artifact.** The records are uploaded as `browser-smoke-attempt-<n>` whether the smoke passes or fails, and are kept for 14 days.

[docs/verification/browser-ci.json](docs/verification/browser-ci.json) records the job's runs, including one where a deliberately broken build failed it. The smoke keeps Playwright's request interception on for the whole run: otherwise a request sent just as the last step route is removed can stall in the browser, which made the restart step flaky (#8); `npm run test:browser:routes` reproduces that against a plain server ([docs/verification/browser-route-race.json](docs/verification/browser-route-race.json)).

`npm run preview --workspace @chemifree/web` previews built assets on port 4173 and inherits the local API proxy; run the API separately. Vite's preview server is a local verification tool. For a production deployment, serve `/api/v1/*` to the Rust API through the same origin. Hosting, authentication, and a production deployment configuration are future work.

## Workspace

```text
apps/
  web/              React + TypeScript + Vite presentation
    src/editor/     Structure canvas (SVG rendering, hit testing, gestures), separate from UI controls
  api/              Axum HTTP entry adapter; loopback only; write guards
  cli/              clap entry adapter; shared application calls
crates/
  domain/           Elements, document graph, commands, transactions, change-set history; no dependencies
  application/      Use cases, editing sessions, v1 DTOs, CapabilityCatalog and DocumentRepository ports
  providers/        Capability catalog and the local JSON file repository
  conformance/      Conformance runner and the chemifree-conformance binary
conformance/
  suite.json        The suite: suite_version and case paths
  cases/            Cases written for this project (runnable or pending)
scripts/
  contract-smoke.mjs HTTP/CLI boundary and document-flow verification
  browser-smoke.mjs  Manual browser verification of the editor
  traceability-check.mjs  Traceability check (and its tests in traceability-check.test.mjs)
docs/
  TRACEABILITY.ko.md What the traceability files, runner, and check establish
  traceability/     Requirement parts and declaration counts
  roadmap/          Implementation roadmap (PR plan)
  verification/     Run records, including conformance-report.json, and review history
  CONTRACT.md       v1 shape, compatibility rules, and capability identifiers
  architecture.html Detailed architecture and staged design
  SHADCN_REVIEW.ko.md shadcn/ui and @shadcn/lint review
```

Dependencies point inward: `application → domain`; `providers → application + domain`; HTTP and CLI compose the application with concrete providers. Document rules (stable IDs, reference integrity, atomic transactions, undo/redo as change sets) live in `crates/domain`; sessions, revisions, request-ID replay, and save semantics live in `crates/application`; file I/O lives in `crates/providers` behind the application-owned `DocumentRepository` port. The web mirror of the v1 contract validates responses at runtime. There is no code generation, service locator, database, or scientific engine. The structure canvas in `apps/web/src/editor/` is separate from the shadcn/ui controls.

## API and implementation status

The [interactive architecture document](docs/architecture.html) maps these layers to the actual source files, current contracts, UI decisions, and next implementation steps. Download the HTML and open it in a browser; GitHub's file viewer displays its source. It is self-contained and works offline. See also the [shadcn review](docs/SHADCN_REVIEW.ko.md) and [contract reference](docs/CONTRACT.md).

| HTTP | CLI | Meaning |
| --- | --- | --- |
| `GET /api/v1/health` | `status --json` | Application liveness contract; not scientific readiness |
| `GET /api/v1/capabilities` | `capabilities --json` | Actual implementation status |
| `GET/POST /api/v1/documents` | `documents list` / `create` | List and create documents |
| `GET /api/v1/documents/{id}` | `documents show` | Working copy |
| `POST …/transactions`, `…/undo`, `…/redo` | `documents apply` / `undo` / `redo` | Edits with `expected_revision` |
| `POST …/save`, `…/close` | (CLI edits save) | Commit; end session |
| `POST …/autosave` | — | Write the session's recovery draft |
| `POST …/save-as`, `…/recover-as-copy` | `documents save-as` / `recover-as-copy` | Commit as a new document |
| `POST …/recover`, `…/discard-draft` | `documents recover` / `discard-draft` | Act on a draft left by an ended process |
| `GET /api/v1/documents?query=&sort=` | `documents list --query --sort` | Search titles; sort by title or last save |
| `POST …/rename` | `documents rename` | Commit a new title (no unsaved changes) |
| `POST …/delete`, `GET /api/v1/trash`, `POST /api/v1/trash/{id}/restore` | `documents delete` / `trash` / `restore` | Move to the trash and back; refused while unsaved work or a draft exists |
| `POST /api/v1/operations` | `operations` (stdin lines) | Operation contract 1: versioned per-operation requests and replies; unimplemented operations answer `unsupported` |

All JSON responses include `api_version: "v1"`. Shared Rust DTOs live in `crates/application`; the TypeScript mirrors live in `apps/web/src/lib/`. See [the contract](docs/CONTRACT.md) for commands, errors, limits, the file format, and version policy. `runtime_inspection`, `document_editing` (drawing model only), and `persistence` (local files) are `available`; `structure_analysis`, `format_conversion`, `local_model_loading`, `authentication`, and `windows_bridge` report `not_implemented`.

The API binds IPv4 loopback and sets no CORS permissions. Writes require an allowed `Host`, a same-site or allowed `Origin`, a per-process session token from `GET /api/v1/session`, a JSON body, and at most 256 KiB. This blocks other websites and DNS rebinding; it is not user authentication and does not separate users of the same machine. A remote or multiuser service still requires a separately implemented access-control boundary.

## UI components and lint

The starter uses official **@shadcn** registry components with **Base UI / Nova**, Tailwind CSS 4, semantic tokens, Lucide icons, and a locally bundled Geist font. Installed: Alert, Alert Dialog, Badge, Button, Card, Dialog, Empty, Field (with Label and Separator), Input, Kbd, Skeleton, Spinner, Toggle Group (with Toggle), and Tooltip. `components.json` records aliases and the theme CSS path. Generated source has been reviewed; upstream notices are preserved in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

Initialization used **shadcn CLI 4.22.1**:

```bash
cd apps/web
npx shadcn@latest init --base base --preset nova --no-monorepo -y
npx shadcn@latest info --json
npx shadcn@latest docs button card badge empty alert skeleton
npx shadcn@latest add @shadcn/button @shadcn/card @shadcn/badge @shadcn/empty @shadcn/alert @shadcn/skeleton -y
# Editor controls (existing button.tsx kept; the registry copy differed only by our notice header)
npx shadcn add @shadcn/toggle-group @shadcn/input @shadcn/field @shadcn/separator @shadcn/tooltip @shadcn/kbd @shadcn/alert-dialog @shadcn/spinner
# Save-as form (answered "no" to overwriting button.tsx)
npx shadcn@4.22.1 add @shadcn/dialog
```

Use the CLI's current help when adding components; `components.json` resolves this configuration to `base-nova`. The installed CLI and dependencies are pinned by `package-lock.json`. Review generated changes before committing them.

**`@shadcn/lint` 0.2.3 is installed and registered as an ESLint plugin. This validates plugin integration only; no shadcn design rules are enabled or enforced in default lint or CI yet.** It is a separate ESLint/Oxlint plugin, not a `shadcn lint` CLI command. Read [the review](docs/SHADCN_REVIEW.ko.md) before selecting design rules for this project.

## License and contribution boundary

[MIT](LICENSE), Copyright (c) 2026 EunhakChoi. Third-party components and fonts retain [their own notices](THIRD_PARTY_NOTICES.md).

Read [AGENTS.md](AGENTS.md) before contributing. Product code is authored independently from public specifications and independent sources. Keep original binaries, decompilations, private datasets, local models, credentials, database files, caches, and investigation evidence outside this public repository. Scaffolding verification does not establish chemical correctness or compatibility with any other application.
