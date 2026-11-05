# Chemifree

An independently authored, open-source chemical drawing workspace for humans and agents.

**Early stage.** The browser, HTTP and CLI support creating and editing drawings, undo/redo, local save and recovery. Atoms and bonds retain their drawn states. Named, bounded valence, ring and graph-identity inspectors report unsupported states, and supported aromatic/Kekulé conversions are explicit undoable edits that are refused for the whole document when any ring system is unsupported (for example a benzene fused to a saturated ring, which is reported unsupported rather than non-aromatic). Formula inspection uses the valence model; average and exact-profile mass and isotope distributions additionally need a checked external mass dataset. Results include their analyzed snapshot and calculation policy. A bounded V2000 and V3000 MOL/SDF subset can be previewed, imported and exported with explicit loss reports (V3000 is read and written by the operations since P28b1 and by the web panel since P28c). Rich CDXML and a bounded CDX subset (canonical base64 over JSON) can be read and written, with a loss-accounted editor projection; that projection reads the stereo the public sources confirm (CDXML bond wedge, hash and wavy values with their narrow end, configurations derived from them, enhanced stereo groups, and the CDX atom tags; the binary bond `Display` is read (public INT16 list), `Display2` and `DoublePosition` stay unread, double-bond E/Z is not derived, and a bounded drawing export now writes supported marks and groups with explicit loss consent), status P29e. A bounded OpenSMILES subset can be previewed and imported (positions are generated on a grid) and written as recorded-order or Chemifree-canonical text; the canonical form is Chemifree's own, not another toolkit's. SMILES reading takes the tetrahedral marks `@` and `@@` of bracket atoms as stored configurations (with no bond mark, since the atoms have no coordinates) and the CXSMILES fields `a:`, `o<n>:` and `&<n>:` as enhanced stereo groups, status P29c; directional bonds `/` and `\`, other mark classes, a lone pair as fourth position and every other CXSMILES field are refused, recorded output writes supported tetrahedral configurations and enhanced stereo groups while canonical output writes no stereo, and query and reaction SMILES and InChI are not implemented. A drawing can hold stereo bond marks and stated configurations in its model and file: the commands `set_bond_mark`, `set_atom_configuration` and `set_bond_configuration` (HTTP, CLI and operation contract 1) set and clear them, a transaction reply lists in `notes` the configurations an edit cleared, and snapshots, the CLI and the web show them. `set_configuration_from_drawing` can read a configuration off the marks and one instance's positions when it is sent (a bounded, documented rule set, named `chemifree-stereo-1`); it replaces the stored record of every item it processes, a configuration stated by hand included, and writes nothing for an item it skips. `chem.stereo` is a separate read-only inspection that compares each stored configuration with what each scene instance's drawing shows (agrees, disagrees, only stored, only drawn, neither) and lists advisory findings; it stores and re-derives nothing, and it does not judge whether an atom is stereogenic or compute a CIP label. `chem.cip` is a separate read-only operation that computes CIP descriptors (R, S, E, Z) from the stored configurations on demand, by Rules 1a, 1b, 2 and 3 only: every item is `assigned`, `none`, `unspecified`, `unknown`, `ambiguous` or `unsupported` with a reason, a tie that Rules 4a to 5 might break is `unsupported` rather than `none`, nothing is stored, and no equivalence with another program is claimed (the web editor shows them only behind two switches that are off by default, below). Beyond that they are recorded statements only: nothing labels or imports them; the web editor draws wedge, hashed wedge and wavy bonds with the bond tool (and a crossed mark in the bond inspector) and stores the configuration the drawing then shows in the same undo step, and for a drawing that holds stereo it calls `chem.stereo` on its own (a switch turns that off, "Check now" asks once) and shows how each stored configuration compares with the drawing as a summary line, a ring on the canvas and a Stereo panel whose rows each offer a one-click edit that updates, stores or removes the stored configuration (one undo step); its indicator itself judges no atom stereogenic and computes no CIP label. Two switches in that panel, "Atom stereo labels" and "Bond stereo labels" (off by default, not saved), are the only thing that makes the page ask `chem.cip`: while one is on it asks with `scope: "stored"` (never `stereogenic`, because the operation has no time limit), draws the descriptor in parentheses near each assigned center (`(S)`) or double bond (`(E)`) as a canvas overlay that is never part of the document, and adds a CIP cell next to the agreement with the drawing to each Stereo row that has a stored configuration, under the switch of its kind (a refused or unsupported item says so and gets no overlay text; with a switch on and nothing of its kind stored the panel says that, and nothing is asked); a label describes the stored configuration, not the drawing, and is never stored. The overlay's text form for bonds and its placement are Chemifree choices, not legacy behavior. `chem.validate`, `chem.identity` and `chem.substructure` do not read them, and mirror and flip commands do not exist yet. Identity and substructure replies for a drawing that holds them are `partial` with a stereo loss, a SMILES export requires consent for stereo it does not retain, and a MOL export writes the marks and asks consent for the stereo (configurations the marks do not show, groups) the file cannot carry. Enhanced stereo groups (absolute, and, or) are separate statements over stored tetrahedral centers: eight commands (`create_stereo_group`, `delete_stereo_group`, `set_stereo_group_kind`, `set_stereo_group_number`, `add_stereo_group_members`, `remove_stereo_group_members`, `merge_stereo_groups`, `split_stereo_group`; HTTP, CLI and operation contract 1) create and change them, snapshots list them as `stereo.groups` with the number as stated, the interpreted number and a label, and a transaction reply reports in `notes` when an edit changed which centers are stated to belong together (splitting, merging, removing members, a member losing its configuration). They state no ratio, CIP, identity, substructure and validation ignore them, recorded SMILES writes supported groups, canonical SMILES needs consent to drop them, MOL V3000 import and export maps them and CDXML and CDX import reads them from the atoms' enhanced stereo attributes (V2000 carries none, a CXSMILES block is read and written for supported groups, bounded CDXML/CDX drawing export writes groups only when every member derives from its output drawing, and canonical SMILES or V2000 export asks consent to drop them), and the web Stereo panel shows and edits them (its recorded SMILES export writes supported groups, and its canonical SMILES and MOL V2000 exports ask consent to drop them; its MOL V3000 export writes them as collections; they are not compared with the drawing). A bounded substructure search (`chem.substructure`, also the web "Substructure search" panel) finds every atom and bond mapping of one drawing in another: query atoms and bonds, a stated charge or isotope, and aromatic and Kekulé spellings read alike through the aromaticity model; a drawing that model cannot read, or a search that reaches its limit, is refused rather than answered empty, and ring, degree and hydrogen-count conditions are not available because the drawing cannot record them. Other formats, stereochemistry beyond that (CIP Rules 4a to 5, pseudoasymmetric centers, heteroaromatic rings), layout, naming, prediction engines, authentication and Windows integration remain unimplemented. See the [valence](docs/CONTRACT.md#valence-inspection), [aromaticity](docs/CONTRACT.md#ring-and-aromaticity-interpretation), [identity](docs/CONTRACT.md#graph-identity-and-symmetry), [substructure matching](docs/CONTRACT.md#substructure-matching) and [property](docs/CONTRACT.md#formula-and-mass-properties) policies for scope and limitations.

Continuing in a new session? Start with [the handoff](docs/HANDOFF.ko.md) and [project instructions](AGENTS.md). They record the completed baseline, current limits, and the proposed next implementation slice.

## Run locally

Use Node **22.22.2**, npm **11.13.0**, and Rust **1.95.0**. The repository includes `.nvmrc`, `packageManager`, and `rust-toolchain.toml`.

```bash
npm ci
npm run dev
```

Open **http://127.0.0.1:5173**. This starts Vite and the Rust API together; Vite proxies `/api` to **127.0.0.1:3001**. Stop both with Ctrl+C. Choose **New document**, draw with the bond tool (click to start a chain, click an atom to grow it, drag to bond, click a bond to cycle its order), relabel atoms with the atom tool, inspect and edit charge or isotope with the select tool, then **Save** (Ctrl/⌘+S). **Duplicate placement** (under the inspector) draws the same graph again, copied from the last placement and shifted down and right; each placement moves on its own with its arrow buttons, dragging atoms moves them only in the placement where the drag starts, and chemistry edits apply to every placement. Undo/redo: Ctrl/⌘+Z and Ctrl/⌘+Shift+Z. Close and reopen from **Saved documents**, where you can search titles, sort by title or most recent save, rename, and move documents to the **Trash** (and restore them). A document with unsaved changes or a recovery draft is not moved to the trash. Under the title, **File** shows the saved state and **Recovery draft** shows the autosave (1 s after the last change, at least every 5 s while editing). The API also drafts unsaved work on its own, with or without the page (2 s after the last change, at most 10 s after the first), and at a normal stop. If the API stops before you save, reopening the document offers **Recover** (with undo history) or **Discard draft**; if it was saved elsewhere meanwhile, **Recover as copy**. **Save as copy** (the copy icon) commits what is on screen as a new document and opens it; the original keeps its last save. If the original's recovery draft cannot be removed, the original is left open with its unsaved changes instead, and the notice above the copy says so and offers **Open original**; the notice goes away once you leave the copy, or once the page learns that the local service restarted (the page finds out when its next write is refused, so an idle page keeps the notice until then).

Documents are stored as `documents/<id>.json` in the data directory (recovery drafts in `drafts/`, save locks in `locks/`; files from before graph and scene were separated are read and migrated, and the first save over one keeps its original bytes in `migrated/`): `--data-dir` or `CHEMIFREE_DATA_DIR`, otherwise the platform user-data directory (`$XDG_DATA_HOME/chemifree` or `~/.local/share/chemifree` on Linux, `~/Library/Application Support/chemifree` on macOS, `%APPDATA%\chemifree` on Windows). The API prints the path at startup.

The **MOL and SDF** panel reads UTF-8 V2000 and V3000 files (the dialect is detected; collections become the drawing's stereo groups, with renumbering, merging and chiral-flag losses shown like other import losses) and previews a selected record, including conversion losses, before appending it as one undoable edit. It can preview and download the current drawing as MOL or SDF; a version control (V2000 default, V3000, Auto) is sent as the `ctab` key and is a choice of this page, not saved in the drawing, and the result says which dialect was written. When the export drops something that needs consent (stereo groups in V2000, stored centers in no group in V3000, configurations the marks do not show) the losses are listed with plain sentences and nothing is offered for download until they are accepted; a changed drawing or digest asks again. Refusals (a group member the marks do not derive, query atoms, S-groups, R-groups, 3D, reactions) are worded plainly and leave the drawing unchanged. The subset, safety limits, field handling and operation requests are described in [MOL and SDF](docs/CONTRACT.md#mol-and-sdf-subset); unsupported chemistry is refused rather than silently changed. Through `document.import` and `document.export` (HTTP, CLI, operation contract 1) a V3000 connection table is read (core atoms, bonds 1 to 4 with aromatic, charge, isotope, radical, wedge/hash/either bond marks with the configurations they show) and written (`ctab` `v3000` or `auto`), and a V2000 export writes the bond stereo column, which the V2000 reader now also reads (wedge, hash, either); `accepted_losses` is the consent for stereo a file cannot carry. Since P28b2 the V3000 `COLLECTION` entries `STEABS`, `STERAC<n>` and `STEREL<n>` map to and from the drawing's enhanced stereo groups (any 32-bit number as stated, the chiral flag written as 1 only when there is an absolute group, a stored center in no group asks consent, a member that is not a stored stereocenter refuses the file), and `format` accepts the `mol-v2000` and `mol-v3000` ids. Not implemented: V3000 query atoms and bonds, dative bonds, stereo groups in V2000 (a V2000 export asks consent to drop groups), SGroups, R-groups, 3D and reactions, atom parity on read (status: P28a, P28b1 and P28b2; the web choice is P28c, see the contract).

Through HTTP/CLI `document.export`, `{ format: "cdxml" | "cdx", document_id, accepted_losses? }` exports a captured drawing as one page and fragment and returns its snapshot. It preserves supported wedge/hash/wavy/crossed marks; it invents no marks or CIP labels. Groups require every member to derive from the written drawing; omitted configurations/groups require digest-based consent. Group numbers outside 1–127 are renumbered with an informational loss. Written group members explicitly show enhanced-stereo indicators (`0445=true`), reported once as `enhanced_stereo_display` rendering normalization; the drawing stores no visibility setting, so roundtrip checks of configurations and groups exclude it. The existing rich-tree export and strict opaque-extension rules remain available. The web **CDXML and CDX export** panel now prepares either file, shows losses, and offers a download only after any required losses are explicitly accepted. Download rechecks the exact file and snapshot; changes to the drawing, format, consent or service invalidate the preview. CDX downloads contain binary bytes, not base64 text. P29 remains in progress pending P29g; this does not establish full stereo interchange. See [the bounded drawing profile](docs/CONTRACT.md#cdxml-interchange).


The **SMILES** panel keeps the line breaks of what is pasted, previews the atoms, bonds and losses of a string before appending it as one undoable edit (asking for acceptance only of losses that change the chemistry or drop text), and writes the drawing as recorded or canonical SMILES. Grammar, limits, error codes and what is refused are in [SMILES](docs/CONTRACT.md#smiles).

The scene can also contain ordered groups and simple line, rectangle, and arrow graphics. Directed references from scene objects to objects, atoms, or bonds record relationships without computing chemistry. Grouping, movement, ordering, references, and selection participate in the same undo/redo history. Each document owns its style (bond length and width, label font and size, carbon labels, and display unit); style changes are undoable. Data-directory preferences separately hold the theme, defaults copied into newly created documents, and mass-result display decimal places. **Document style** and **Preferences** edit these settings in the web UI; general numeric precision and NMR preferences remain unavailable.

If preferences could not be read when the page started, open **Preferences** and choose **Retry preferences** after the local service is available; a successful read applies the saved theme.

Version 7 documents and drafts retain the scene, selection, style, the storage for stereo marks and stated configurations (shown in every snapshot and set by the stereo commands), and enhanced-stereo groups (eight commands, shown in every snapshot as `stereo.groups` and edited in the web Stereo panel; only MOL V3000 import and export reads and writes them as `COLLECTION` entries); versions 1–4 are read in memory with the default style, version 5 keeps its stored style, versions 1–5 have no stereo, and versions 1–6 have no groups. Original document bytes are kept before the first overwrite, and original draft bytes before recovery; explicitly discarding a draft does not retain a copy. Preferences are stored separately in `preferences.json`. See [the document contract](docs/CONTRACT.md) for limits and migration behavior.

The P15 numeric library provides exact-rational Real48, binary32, binary64, and 80-bit extended storage conversions, finite arithmetic, selected ordered heading and deviation boundaries, independent length and concentration unit conversions, and explicit decimal display and F32 storage policies. The numeric library does not itself enable a chemistry engine. See [the numeric boundaries](docs/NUMERICS.ko.md).

Optional external DATA stays in a separately selected local directory and requires a trusted registration and review. Typed readers cover the selected active DATA roles, with public checks using independent fixtures. A separately authorized NIST snapshot passed a narrow local methane check; a distinct personal mass-profile registration was rejected and remains unverified ([record](https://github.com/EunhakChoi/chemifree/pull/81#issuecomment-6546579774)). The host selects individual registrations, and an aggregate registry hash alone does not prove a manifest belongs to it. See [the DATA handoff](docs/DATA.ko.md) for profiles and limits. No data or registry is needed for drawing or formula inspection; mass operations consume only the two explicitly supported checked profiles, and loading other data does not enable prediction.

The first API launch compiles Rust dependencies. If that takes longer than the UI's five-second request timeout, use **Retry connection** once the API is listening. Both development ports must be available.

```bash
# Run entry adapters separately.
npm run dev:web
npm run dev:api

# CLI invokes the shared application library directly; it does not contact HTTP.
cargo run --locked -p chemifree-cli -- status --json
cargo run --locked -p chemifree-cli -- capabilities --json
cargo run --locked -p chemifree-cli -- data status --json

# Documents through the same application and data directory. Each CLI edit saves.
cargo run --locked -p chemifree-cli -- documents create --title Ethanol
cargo run --locked -p chemifree-cli -- documents apply <id> --expected-revision 0 --commands \
  '[{"op":"add_atom","element":"C","position":{"x":0,"y":0}},
    {"op":"add_atom","element":"C","position":{"x":26,"y":15},"bond_to":{"atom":"$0","order":"single"}},
    {"op":"add_atom","element":"O","position":{"x":52,"y":0},"bond_to":{"atom":"$1","order":"single"}}]'
cargo run --locked -p chemifree-cli -- documents undo <id> --expected-revision 1
cargo run --locked -p chemifree-cli -- documents show <id> --json

# A document style edit is undoable; preferences affect new documents only.
cargo run --locked -p chemifree-cli -- documents apply <id> --expected-revision 2 --commands '[{"op":"update_settings","bond_length_pt":40}]'
cargo run --locked -p chemifree-cli -- preferences show
cargo run --locked -p chemifree-cli -- preferences set --expected-revision 0 --preferences '{"ui":{"theme":"dark"},"new_documents":{"label_font":"serif"}}'

# Operation contract 1: one JSON request per line in, one reply per line out.
echo '{"contract":1,"request_id":"r1","profile":"preserve-semantics","operation":"capabilities.inspect","parameters":{}}' \
  | cargo run --locked -p chemifree-cli -- operations
cargo run --locked -p chemifree-cli -- documents save-as <id> --expected-revision 3 --title "Ethanol copy"
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

The fixtures of the substructure search (`crates/application/tests/data/query-fixtures.json`) are also compared, by hand, with an RDKit 2022.03.2 Python (no equivalence is claimed beyond those fixtures), which needs that RDKit and a built CLI: `python -I scripts/verify-query-rdkit.py target/debug/chemifree`. It states an expected outcome for every fixture (the full mapping set RDKit's `GetSubstructMatches(uniquify=False)` finds, or a specific refusal with the number of mappings RDKit finds) and fails on any difference or error; it is not part of `npm run check` and no CI job runs it. Run it whenever a fixture, the matcher's rules or the aromaticity model change, from the RDKit 2022.03.2 environment after `cargo build`.

The contract smoke builds the workspace, starts an API on an ephemeral loopback port with a temporary data directory, and compares HTTP JSON with actual CLI JSON. It runs create → edit → undo/redo → save → close → reopen, checks request-ID replay, revision and save conflicts, write guards, a save that fails because the target path is unwritable and then succeeds on retry, and that a killed API restarts with the last committed document. It then races six CLI saves from one base (exactly one commits), kills a process holding the save lock, races an API save against a CLI save, and runs autosave → SIGKILL → restart → recover/discard/recover-as-copy, an unreadable draft, a failed autosave, and save as copy over HTTP and the CLI. Storage feedback: while the API's save of a document pauses inside its save lock, asking about that save again is answered `request_in_progress` at once, requests queued behind it and a CLI save of the same document are refused as `busy` within the time budget without changing anything, and asking again afterwards returns the committed reply without writing again; a read-only `documents/` is reported as `read_only`. GitHub Actions installs npm 11.13.0 into an isolated runner-temporary prefix, adds its binaries to `PATH`, and verifies the selected version before running `npm ci` and `npm run check` with pinned tools.

The conformance runner (`crates/conformance`, binary `chemifree-conformance [--out PATH] SUITE`) runs the cases in `conformance/cases/` against the shared application with the same request types and arguments as the HTTP handlers and a `request_id` on every write (the HTTP layer is not involved, and status codes are not compared), each case on a fresh temporary data directory; `restart` drops the application without a normal stop, as a crash would. Pending cases name the roadmap item expected to make them runnable and are recorded as `not_run`, never as passed. `docs/traceability/` traces the 33 public readiness requirements (by part, with status, implementation paths, evidence, and follow-ups) and counts the 2,914 public interface declarations per proposed route; a declaration count is not a feature count. The functional spec archive stays outside the repository and traceability references it by path and SHA-256; the loader embeds only the public personal-local context needed to verify its exact identity. `CHEMIFREE_SPEC_DIR=<dir holding spec/> npm run test:traceability` also compares the references against the spec. [docs/TRACEABILITY.ko.md](docs/TRACEABILITY.ko.md) states exactly what these checks establish and what they do not (for example, that a status judgement is right, or that the repository holds no private data).

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
  testing-practice.md How tests are shown to fail, kept deterministic, compared with oracles, and run isolated
  traceability/     Requirement parts and declaration counts
  roadmap/          Implementation roadmap (PR plan)
  verification/     Run records, including conformance-report.json, and review history
  CONTRACT.md       v1 shape, compatibility rules, and capability identifiers
  architecture.html Detailed architecture and staged design
  SHADCN_REVIEW.ko.md shadcn/ui and @shadcn/lint review
```

Dependencies point inward: `application → domain`; `providers → application + domain`; HTTP and CLI compose the application with concrete providers. Document rules (stable IDs, reference integrity, atomic transactions, undo/redo as change sets) live in `crates/domain`; sessions, revisions, request-ID replay, and save semantics live in `crates/application`; file I/O lives in `crates/providers` behind the application-owned `DocumentRepository` port. The web mirror of the v1 contract validates responses at runtime. There is no code generation, service locator, database, or general prediction engine; the scoped property calculations are described in the contract. The structure canvas in `apps/web/src/editor/` is separate from the shadcn/ui controls.

## API and implementation status

The [interactive architecture document](docs/architecture.html) maps these layers to the actual source files, current contracts, UI decisions, and next implementation steps. Download the HTML and open it in a browser; GitHub's file viewer displays its source. It is self-contained and works offline. See also the [shadcn review](docs/SHADCN_REVIEW.ko.md) and [contract reference](docs/CONTRACT.md).

| HTTP | CLI | Meaning |
| --- | --- | --- |
| `GET /api/v1/health` | `status --json` | Application liveness contract; not scientific readiness |
| `GET /api/v1/capabilities` | `capabilities --json` | Actual implementation status |
| `GET/POST /api/v1/documents` | `documents list` / `create` | List and create documents |
| `GET /api/v1/documents/{id}` | `documents show` | Working copy |
| `POST …/transactions`, `…/undo`, `…/redo` | `documents apply` / `undo` / `redo` | Edits with `expected_revision` |
| `POST …/settings/reset`, `…/settings/import` | `documents settings reset` / `import` | Reset or import document style as an edit |
| `GET/PUT /api/v1/preferences`, `POST …/reset`, `…/import` | `preferences show` / `set` / `reset` / `import` | Data-directory UI and new-document preferences |
| `POST …/save`, `…/close` | (CLI edits save) | Commit; end session |
| `POST …/autosave` | — | Write the session's recovery draft |
| `POST …/save-as`, `…/recover-as-copy` | `documents save-as` / `recover-as-copy` | Commit as a new document |
| `POST …/recover`, `…/discard-draft` | `documents recover` / `discard-draft` | Act on a draft left by an ended process |
| `GET /api/v1/documents?query=&sort=` | `documents list --query --sort` | Search titles; sort by title or last save |
| `POST …/rename` | `documents rename` | Commit a new title (no unsaved changes) |
| `POST …/delete`, `GET /api/v1/trash`, `POST /api/v1/trash/{id}/restore` | `documents delete` / `trash` / `restore` | Move to the trash and back; refused while unsaved work or a draft exists |
| `POST /api/v1/operations` | `operations` (stdin lines) | Operation contract 1: versioned per-operation requests and replies; unimplemented operations answer `unsupported` |

All JSON responses include `api_version: "v1"`. Shared Rust DTOs live in `crates/application`; the TypeScript mirrors live in `apps/web/src/lib/`. See [the contract](docs/CONTRACT.md) for commands, errors, limits, the file format, and version policy. `runtime_inspection`, `document_editing`, `structure_analysis` (the explicitly labeled bounded valence inspection), `format_conversion` (the labeled V2000 MOL/SDF, CDXML and CDX subsets), and `persistence` are `available`. The operation catalog is authoritative for individual operations; model loading, authentication and Windows integration report `not_implemented`.

The API binds IPv4 loopback and sets no CORS permissions. Writes require an allowed `Host`, a same-site or allowed `Origin`, a per-process session token from `GET /api/v1/session`, a JSON body, and at most 256 KiB. This blocks other websites and DNS rebinding; it is not user authentication and does not separate users of the same machine. A remote or multiuser service still requires a separately implemented access-control boundary.

## UI components and lint

The starter uses official **@shadcn** registry components with **Base UI / Nova**, Tailwind CSS 4, semantic tokens, Lucide icons, and a locally bundled Geist font. Installed: Alert, Alert Dialog, Badge, Button, Card, Dialog, Empty, Field (with Label and Separator), Input, Kbd, Select, Skeleton, Spinner, Toggle Group (with Toggle), and Tooltip. `components.json` records aliases and the theme CSS path. Generated source has been reviewed; upstream notices are preserved in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

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

Read [AGENTS.md](AGENTS.md) and [the contribution guide](.github/CONTRIBUTING.md) before contributing. Do not use original application DLLs as the execution engine. Keep credentials, local data, generated outputs, and caches outside this public repository. Each PR receives one Codex review and one independent subagent review. Scaffolding verification does not establish chemical correctness or compatibility with any other application.
