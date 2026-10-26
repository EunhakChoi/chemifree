# Chemifree contributor instructions

Chemifree is an independently authored, open-source chemical drawing workspace for humans and agents.

## Current scope

Build scaffolding only: a web application shell, a local Rust API, a CLI, explicit layer boundaries, development commands, CI, and architecture documentation. Do not implement chemical algorithms, import/export engines, model loaders, persistence, authentication, or Windows integration in this stage. Report those capabilities as not implemented.

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
