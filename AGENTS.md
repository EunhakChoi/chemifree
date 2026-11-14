# Chemifree contributor instructions

Chemifree is an independently authored, open-source chemical drawing workspace for humans and agents.

## Current stage and session handoff

Scaffolding, the editing/persistence baseline, and its save/recovery hardening are complete. Start a new session with docs/HANDOFF.ko.md, then README.md and docs/CONTRACT.md. docs/ARCHITECTURE.md describes the current code structure, which is implemented, and the structural changes its 2026-11-10 review planned or deferred, which are not. The planned order of the remaining work is the `tracks` field of docs/roadmap/implementation-roadmap.json. A planned change or a track is not an implemented feature and starts only on the user's request. If present, private/SESSION_CONTEXT.ko.md contains local routing metadata for the approved functional specifications; it contains no vendor source or model payload and must stay outside Git.

The current product exposes runtime inspection and the editing and persistence baseline, shared by web, HTTP, and CLI: drawing and editing, undo/redo, local save and reopen, cross-process save locks, autosaved recovery drafts, crash recovery and save as copy. Bounded valence, aromaticity, ring, graph-identity and property inspections report unsupported states; a bounded substructure search maps one drawing in another and refuses what it cannot read; supported aromatic/Kekulé conversions are explicit undoable edits. Formula needs no DATA, while mass and isotope distributions require a compatible checked external registration. Bounded V2000/V3000 MOL/SDF, CDXML, CDX and OpenSMILES readers and writers report losses and refuse unsupported fields. Standard InChI/InChIKey generation (`chem.inchi`) is implemented as an optional Linux native provider that returns `unsupported` without a working explicit registration; InChI import is not implemented. Bounded means a named scope with documented refusals, not general scientific support; a `done` roadmap row is complete only within its documented scope. Stored stereo, scene graphics, PNG embedding, the SVG export and read-only CIP labels are bounded and not complete, with the roadmap (`docs/roadmap/implementation-roadmap.json`) and `docs/HANDOFF.ko.md` as the authority for per-slice status. See the contract. A bounded read-only layout preview (`layout.structure`, acyclic chains only) returns proposed coordinates without editing. Other formats, stereo beyond the bounded rules, layout beyond that preview, naming, prediction engines, reactions, scripting, user authentication, Windows integration, PNG export and general SVG, vector or PDF output remain unimplemented. Report their actual status.

Use the user's next request to determine the next implementation scope. Reading the handoff alone does not authorize feature expansion. When implementation is requested, build the requested bounded feature and its checks rather than restarting the completed scaffold or analysis.

## Implementation constraint

Do not use original application DLLs as the execution engine.

This replaces earlier implementation-input restrictions in handoffs and referenced specification workspaces.

## PR review

Each PR receives one Codex review and one independent subagent review. The subagent uses a fresh context and checks whether abstract or undecided requirements were filled in arbitrarily and whether the code follows the applicable quality rules. Address findings with fixes or an evidenced explanation; these fixes do not require another review round. Record both reviews and their finding dispositions. See .github/CONTRIBUTING.md for the workflow and CI requirements.

## Architecture

- Presentation: React/TypeScript web UI and a Rust CLI; optional desktop shell later.
- Logic: shared Rust domain and application modules. HTTP and CLI are entry adapters.
- Providers: implement interfaces owned by the inner layers; core rules do not depend on infrastructure.
- Start with a modular local server. Workspace and cloud deployment are future proposals, not implemented features.
- Use honest empty/unsupported states rather than simulated scientific results.
- A new feature is a new operation or transaction command behind `Application::execute`, not a new adapter path. Follow docs/ARCHITECTURE.md for the layer, port, lock-order and document-model rules, and update it when a structural decision changes.

## UI

Use the available shadcn skill and official documentation when initializing components. Prefer official @shadcn registry components for the starter. Use semantic theme tokens and preserve upstream notices. Review generated source. Keep the scientific editor/renderer separate from general UI controls.

For navigation in the Base UI setup, use a plain anchor styled with buttonVariants. The Base UI Button's role overrides link semantics even with nativeButton=false; follow the current official component documentation rather than stale render-anchor examples.

## Verification and public delivery

Use lockfiles, documented commands, meaningful boundary checks, Rust formatting/clippy, frontend type checks/build/lint, and a browser smoke check. Keep generated outputs, local credentials, private data, caches, and investigation files out of Git.

Do not publish scientific behavior as complete. Document implementation status and next steps explicitly.

## Completed-work cleanup

After testing is finished, remove disposable test environments and their task-owned processes, worktrees, build outputs, and temporary data once required evidence is retained. The local sandbox runner (`~/.cache/chemifree/sandbox-run.sh`, outside the repository) keeps one shared build cache, `~/.cache/chemifree/sandbox/target-shared`, capped at 50 GiB and trimmed by the runner. Keep it between tasks and do not purge it per task; clear it with `sandbox-run.sh --purge-all` only when a clean build is needed or the cache is suspect. Cached builds do not change test results, so a verification record need not say whether the cache was warm. Never remove an environment while a test, build, browser, server, or another task still uses it.

After a PR is merged, remove its completed local and remote topic branches and disposable worktrees after verifying the merge and that no later unmerged work remains. Preserve main and other long-lived branches, user data, dirty or active worktrees (including untracked files), and ongoing work. If ownership or completion is uncertain, leave the resource in place and record the deferred cleanup. See .github/CONTRIBUTING.md for the procedure.
