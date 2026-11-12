# Architecture

This page describes the structure of the code as it is, the structural decisions of the
2026-11-10 architecture review, and what that review planned or deferred. Numbers were
measured at main `5a0dc579` and will drift. Behavior is defined by
[CONTRACT.md](CONTRACT.md), per-row status by the
[roadmap](roadmap/implementation-roadmap.json), and session state by
[HANDOFF.ko.md](HANDOFF.ko.md). [architecture.html](architecture.html) is a snapshot of the
scaffold (commit `119f477`) and does not describe the current code.

Nothing on this page marks a feature as implemented. "Planned" items below have not started.

## Layers

| Crate or app | Depends on | Holds |
| --- | --- | --- |
| `crates/domain` | nothing | Document model, commands, transactions, undo/redo, chemistry rules (valence, aromaticity, identity, substructure, stereo, CIP), scene objects |
| `crates/numerics` | `num-bigint`, `num-integer` | Exact rationals and stored number formats |
| `crates/application` | domain, numerics | Sessions, revisions, request replay, the operation surface, ports, the SVG layout, DATA registry and typed loaders |
| `crates/inchi-native` | domain | Optional native InChI binding |
| `crates/providers` | application, domain, inchi-native | File store, codecs (MOL/SDF, CDXML, CDX, SMILES), PNG decoder, InChI provider, DATA directory source, capability catalog |
| `crates/host` | application, providers | Assembly of the local `Application` for entry adapters: the file stores of a data directory, the PNG decoder, the codecs, file preferences, the user-selected DATA registry and the optional InChI selection (`InchiArgs`) |
| `crates/conformance` | application, host | Conformance runner |
| `apps/api` | application, host, providers | HTTP adapter (axum) |
| `apps/cli` | application, host, providers | CLI adapter |
| `apps/web` | HTTP only | React/TypeScript editor |

Dependencies point inward: a crate depends only on crates listed above it in this table,
never on one below it. The one exception is test-only: `application`'s tests use
`providers` as a dev-dependency to run against the real file store. Entry adapters keep their transport, data-directory resolution and host-only settings and
assemble the `Application` only through `crates/host`; a new root (a product server, an MCP
process that embeds `Application`) does the same. `domain` and
`application` do no file or network access; from the operating system they use the clock,
threads and the random source (`getrandom`).

**Ports.** The inner layers own the interfaces and `providers` implements them:

| Port | Owner | Implementation |
| --- | --- | --- |
| `CapabilityCatalog` | application | `LocalCapabilities` |
| `DocumentRepository` | application | `FileDocumentRepository` |
| `PreferenceRepository` | application | `FilePreferenceStore` (`MemoryPreferences` in application) |
| `InterchangeCodec` | application | `LocalInterchangeCodec` (CDXML, CDX) |
| `CtCodec` | application | `V2000Codec` (MOL/SDF V2000 and V3000) |
| `SmilesCodec` | application | `OpenSmilesCodec` |
| `InchiProvider` | application | `RegisteredInchiProvider` |
| `DataSource` | application | `DirectoryDataSource` |
| `CipLabeler` | application | `BuiltinCip` (in application; the port exists so a host can replace it) |
| `PngDecoder` | domain | `EmbeddedPngDecoder` |
| `Interrupt` | domain | Deadline and cancel flags supplied by application |

## One operation surface

Every entry adapter calls the same `Application`:

- `Application::execute` (and `execute_with`, which adds a host stop signal) takes one
  operation request and returns one reply. Forty operation ids are registered; the ones with
  no implementation (for example `layout.structure`, `render.scene`,
  `chem.name_to_structure`, `prediction.evaluate`, `script.evaluate`) answer `unsupported`.
- HTTP exposes it as `POST /api/v1/operations`, next to the document REST routes.
- The CLI exposes it as `chemifree operations`, one JSON request per line in and one reply
  per line out, and calls the application library directly without HTTP.
- The web editor uses `/api/v1/operations` for chemistry, import and export, and the
  document routes for editing.

A new feature is a new operation or a new transaction command, not a new adapter path.

## Sessions and locks

- The working copy of each open document lives in the API process (`Documents`), behind one
  `sessions` mutex. Work that touches a document's committed file or draft is serialized by
  a per-document turn. The lock order is the document turn, then `sessions`.
- The rule for heavy computation is: outside the lock on a copy of the graph, with a request
  time limit, a host cancel signal and a cap on concurrent computations, and a result that
  is applied only if the revision is unchanged. Today `chem.aromaticity_convert` and
  `chem.cip` follow all of it, and `chem.aromaticity`, `chem.identity`,
  `chem.substructure` and `chem.properties` follow it except the cap on concurrent
  computations (CONTRACT.md, "Computation time limit and cancel"); those four commit
  nothing, so the revision rule does not apply to them. In the domain
  an interruptible computation is a `_with(…, interrupt)` function returning
  `Result<Result<T, ModelError>, Interrupted>`, and the existing function is its wrapper
  over `NEVER`; a stop is never a model error. SMILES export follows the same rule: the
  `SmilesCodec` port has a required `write_interruptible` method (no default, so an
  implementor cannot silently ignore the limit), the provider writer carries a private
  `Halt` like the domain modules, and the reader stays uninterrupted because import has no
  time limit; that includes the read-back parses the writer makes, so a write's stop can
  wait for them (CONTRACT.md lists them as unchecked). The remaining read-only operations (`chem.stereo`, `chem.inchi`) compute outside the lock and keep their work limits,
  but have no time limit or cancel, and `chem.validate` computes under the lock (its work
  is linear in the document). Separately, creates, closes, copies and the file checks most
  requests make do storage I/O while holding `sessions`; draft writes do not (CONTRACT.md,
  "Slow storage"). What moved out of the table in T0 is the draft write's recovery lookup, and the analysis and save-as copies no longer copy undo and redo history (`Document::without_history`); the file store records its own commit in the stored-revision cache, so the first clean-copy check after a save is a stat. Left under the table, deliberately: the session refresh read, the first-open load, closed-document graph reads, the per-response draft lookup, and create/copy commits. Moving them needs new state (a generation for the sessions table, an epoch per document in the turns, a slot reservation against the cap), so they are a separate design-first change, started on the user's request. `chem.validate` stays under the lock: its copy would be as linear as its computation. A new
  operation does not inherit the protection; it has to be wired in.
- This fits a local single-user server. Per-document locks instead of one table lock are
  needed only for a multi-user service (roadmap S rows, not scheduled).

## Document model and file format

- A document holds one chemical graph (`MOLECULE_ID`) and one page (`PAGE_ID`), with ordered
  scene instances and scene objects (caption, instance, group, graphic, image) on that page.
  The single graph and single page are constants, not assumptions spread through the code.
- **Decision:** multi-page documents (P40) and reaction roles (P49) each start with a
  model-widening PR that includes the file-format migration, before any feature work.
- The native file format is version 30. Older versions, including their undo history, are
  migrated in memory when read.

## Rendering

The roadmap row P38 asks for a common renderer. Today there are two implementations:

- the web editor's TypeScript geometry (`apps/web/src/editor/`, about 1,800 lines for
  bonds, labels, marks, captions and images), which draws the canvas, and
- its Rust port (`crates/application/src/svg*`, about 4,900 lines), which produces the
  bounded SVG export.

They share no code (P38a refused WASM and a JavaScript runtime in the service) and are tied
by a parity fixture of thirteen golden documents
(`crates/application/tests/fixtures/svg/parity.json`, `scripts/svg-parity-client-check.mjs`).
The Rust side does not draw radical dots, attachment points, abbreviation and fragment
displays or label typography, and it measures label widths from a bundled advance table
where the browser measures text. Each new kind of drawn object would have to be implemented
twice and compared.

**Planning basis (not started; can be changed before track T3 begins):** the Rust layout
becomes the single source.

1. `render.scene` returns the layout (`DrawingLayout`, already serializable) and the web
   paints it. The web still runs no Rust: it receives data.
2. New object kinds are laid out only in Rust.
3. The existing TypeScript atom, bond and label geometry is frozen at its current scope
   until the font engine (P38b3) lands; then it is moved to Rust and the parity fixture is
   removed.

Cost and limits: three to four PRs at the head of track T3; no change to what the canvas
shows today; a drag preview of a new object kind translates its last layout until the
service answers. Running the same Rust code in the browser through WASM remains possible
later and would build on step 1. Keeping two implementations is the alternative; under it
every new object kind is implemented twice and compared, and the estimate for track T8 is
50–80 PRs instead of 42–70.

## Findings of the 2026-11-10 review

**Kept as is:** the layering and ports, the single operation surface, the session and lock
model, the versioned file format with in-memory migration.

**Planned changes (track T0, no behavior change):**

| Finding | Evidence | Change |
| --- | --- | --- |
| Hub files are edited by almost every feature | In the last 150 non-merge commits: `apps/web/src/lib/document-contract.ts` (4,177 lines) 45 times, `crates/application/src/contract.rs` (3,067) 32, `crates/application/src/operations.rs` (3,225) 26; a single new shape (#213) also touched `crates/domain/src/document.rs` (4,049) and `crates/providers/src/file_format.rs` (3,736) | Split each file by concern, one PR per file. Split for `apps/web/src/lib/document-contract.ts` (T0, seventh PR; the figures in the Evidence cell are from before the split): it is now a named re-export barrel over `apps/web/src/lib/document-contract/{model,guards,responses,readers,inspection,editing,interchange,transport,client}.ts`; `DocumentClient` stays one class in `client.ts` (about 2,000 lines), so feature edits to its methods still land in one file; decomposing it is a follow-up. Split for `crates/application/src/contract.rs` (T0, eighth PR): it is now a 55-line module root over `crates/application/src/contract/{request,chemistry,stereo,settings,scene,parse,command,view,convert,response}.rs`; `to_command` stays one function in `convert.rs` (about 560 lines), so a new transaction command still edits it; decomposing it is a follow-up. `operations.rs`, `document.rs` and `file_format.rs` remain |
| The composition root exists three times | `apps/api/src/main.rs`, `apps/cli/src/main.rs` and `crates/conformance/src/runner.rs` repeat the same builder chain; `inchi_config.rs` (188 lines) is identical in api and cli; an MCP adapter would be the fourth | Done in T0 (sixth PR): `crates/host` holds `local_application`, `with_selected_providers` and the shared `InchiArgs`; API, CLI and the conformance runner assemble through it (what differs stays in the adapters: the API and the runner prune stale temporaries, the CLI does not; the runner sets the compute limits to the cap and never selects DATA or InChI) |
| Status text lives in five places and the roadmap page is synced by hand | Of the same 150 commits: HANDOFF 86, CONTRACT 85, README 74, roadmap JSON 59 and its HTML copy 52 | Done in T0 (fifth PR): `scripts/roadmap-html.mjs` writes the page's data from the JSON and `npm run test:roadmap` checks that they agree |

**Web contract mirror layout (done in T0, seventh PR).** `apps/web/src/lib/document-contract.ts` is the only import entry for the rest of the web code and the Node client checks; it holds explicit `export {}` / `export type {}` lists and no code. The files in `apps/web/src/lib/document-contract/` import one another only as `@/lib/document-contract/<file>` and never through the barrel, in the order model → guards → responses → readers → {inspection, editing} → interchange → transport → client. There is no `index.ts`: the Node alias hook (`scripts/lib/web-alias-hooks.mjs`) appends `.ts` and does no directory lookup. Known and left: `scene-clipboard-contract.ts` still imports the barrel (a cycle that is safe only while neither side uses the other at module top level), and `DocumentClient` is still one large class.

**Rust contract layout (done in T0, eighth PR).** `crates/application/src/contract.rs` holds the module doc, ten private child modules and the re-export lists (`pub use` for the public DTOs, `pub(crate) use` for the parse, convert, settings and stereo helpers the operations use); callers keep the `crate::contract::<Name>` and `chemifree_application::<Name>` paths. Children refer to each other only as `super::<child>::<item>`, and cross-child items are `pub(super)`. Leaf files are `request`, `chemistry` and `stereo`; `settings` uses `request` and `scene` uses `settings`; the wire shapes `command`, `view` and `response` and the wire-to-domain `parse` use those; `convert` (`to_commands`/`to_command`) uses `command`, `parse`, `scene` and `settings`. The split dropped three re-exports nothing used (`FragmentDisplayRunDto`, `FragmentDisplayView`, `GROUP_NUMBER_RANGE`); the items remain inside their child.

**Deferred, with the point at which each is taken up:**

| Item | Evidence | When |
| --- | --- | --- |
| DATA loaders for engines that do not exist yet sit in `application` | About 17,300 of its 50,800 source lines: `phase4_*`, `phase6_profiles.rs`, `naming_*`, `prediction_*`, `nmr_profile.rs`, `prepared_*`, `nickname_data.rs`, `layout_data_binding.rs`, `active_data.rs` and `active_data/`. The general DATA registry (`data.rs`, `data_formats.rs`, `data_profiles.rs`, `active_registry.rs`) and the `mass_*` files, about 3,300 lines, serve the implemented mass and isotope operation and stay | Move to a separate crate and rename when the first engine that reads them is built (planned at the start of track T7) |
| The contract is written twice by hand | 85 Rust DTO types and the TypeScript validators | At P82: generate a schema from the Rust types for the MCP tool list and use it to check the TypeScript validators for drift |
| About 190 integration test binaries | The shared build cache is about 41 GiB | P86 |
| How a local MCP adapter runs | A separate process embedding `Application`, like the CLI, would not show an agent's unsaved edits in the open web editor | P82 connects to the running local server, so P87 (product server) comes first |

## Order of work

The execution order after the first batch (through P15) is recorded as tracks T0–T9 in the roadmap's `tracks` field
and shown on the roadmap page. PR counts there are rough and are re-cut when a track starts.
A track starts on the user's request.
