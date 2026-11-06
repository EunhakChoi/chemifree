# Chemifree contributor instructions

Chemifree is an independently authored, open-source chemical drawing workspace for humans and agents.

## Current stage and session handoff

Scaffolding, the first editing/persistence slice, and its save/recovery hardening are complete. Start a new session with docs/HANDOFF.ko.md, then README.md and docs/CONTRACT.md. If present, private/SESSION_CONTEXT.ko.md contains local routing metadata for the approved functional specifications; it contains no vendor source or model payload and must stay outside Git.

The current product exposes runtime inspection and the first vertical slice: document creation, atom/bond drawing and editing, undo/redo, local save, and reopen, shared by web, HTTP, and CLI, with cross-process save locks, autosaved recovery drafts, crash recovery, and save as copy. Bounded valence, aromaticity, graph-identity and property inspections diagnose unsupported states; a bounded substructure search maps one drawing in another and refuses what it cannot read; supported aromatic/Kekulé conversions are explicit undoable edits. Formula needs no DATA, while mass and isotope distributions require a compatible checked external registration. Bounded V2000 MOL/SDF, CDXML, CDX and OpenSMILES readers and writers report losses and refuse unsupported fields. See the contract. Other formats, stereo, layout, naming, prediction engines, user authentication, and Windows integration remain unimplemented. Report their actual status.

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

## UI

Use the available shadcn skill and official documentation when initializing components. Prefer official @shadcn registry components for the starter. Use semantic theme tokens and preserve upstream notices. Review generated source. Keep the scientific editor/renderer separate from general UI controls.

For navigation in the Base UI setup, use a plain anchor styled with buttonVariants. The Base UI Button's role overrides link semantics even with nativeButton=false; follow the current official component documentation rather than stale render-anchor examples.

## Verification and public delivery

Use lockfiles, documented commands, meaningful boundary checks, Rust formatting/clippy, frontend type checks/build/lint, and a browser smoke check. Keep generated outputs, local credentials, private data, caches, and investigation files out of Git.

Do not publish scientific behavior as complete. Document implementation status and next steps explicitly.

## Completed-work cleanup

After testing is finished, remove disposable test environments and their task-owned processes, worktrees, build outputs, and temporary data once required evidence is retained. Never remove an environment while a test, build, browser, server, or another task still uses it.

After a PR is merged, remove its completed local and remote topic branches and disposable worktrees after verifying the merge and that no later unmerged work remains. Preserve main and other long-lived branches, user data, dirty or active worktrees (including untracked files), and ongoing work. If ownership or completion is uncertain, leave the resource in place and record the deferred cleanup. See .github/CONTRIBUTING.md for the procedure.
