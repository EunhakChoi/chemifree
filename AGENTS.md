# Chemifree contributor instructions

Chemifree is an independently authored, open-source chemical drawing workspace for humans and agents.

## Current stage and session handoff

Scaffolding, the first editing/persistence slice, and its save/recovery hardening are complete. Start a new session with docs/HANDOFF.ko.md, then README.md and docs/CONTRACT.md. If present, private/SESSION_CONTEXT.ko.md contains local routing metadata for the approved functional specifications; it contains no vendor source or model payload and must stay outside Git.

The current product exposes runtime inspection and the first vertical slice: document creation, atom/bond drawing and editing, undo/redo, local save, and reopen, shared by web, HTTP, and CLI, with cross-process save locks, autosaved recovery drafts, crash recovery, and save as copy. MOL V2000/SDF import and export are available through the shared operation contract with explicit limitations and loss reports. Documents are drawings only. Chemical algorithms (valence, aromaticity, stereo, layout, naming, properties), other interchange formats, model engines, user authentication, and Windows integration remain not implemented. Report their actual status.

Use the user's next request to determine the next implementation scope. Reading the handoff alone does not authorize feature expansion. When implementation is requested, build the requested bounded feature and its checks rather than restarting the completed scaffold or analysis.

## Independent implementation

Product code must be independently written in a fresh context from public functional specifications and independent sources. Do not read original application binaries, source/decompilations, investigation evidence, private vendor payloads, or local analysis directories while writing product code. Existing public functional specifications may be consulted through the authorized handoff, without copying its entire archive into this repository.

Keep executable vendor engines and proprietary model data out of this public repository. Future local-only model providers and any separately authorized service datasets must remain distinct. Do not claim legacy equivalence, implemented chemistry, or working integrations from scaffolding checks.

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
