# Chemifree

An independently authored, open-source chemical drawing workspace for humans and agents.

**Scaffold stage.** The web shell, local read-only API, CLI, and shared application boundary work. Document editing, chemical calculations, import/export, model loading, persistence, authentication, and Windows integration are **not implemented**. There are no simulated chemical results, vendor engines, or proprietary datasets in this repository.

## Run locally

Use Node **22.22.2**, npm **11.13.0**, and Rust **1.95.0**. The repository includes `.nvmrc`, `packageManager`, and `rust-toolchain.toml`.

```bash
npm ci
npm run dev
```

Open **http://127.0.0.1:5173**. This starts Vite and the Rust API together; Vite proxies `/api` to **127.0.0.1:3001**. Stop both with Ctrl+C. The interface reads real service status, supports refresh/retry, follows your system's light/dark preference, and presents an honest empty canvas.

The first API launch compiles Rust dependencies. If that takes longer than the UI's five-second request timeout, use **Retry connection** once the API is listening. Both development ports must be available.

```bash
# Run entry adapters separately.
npm run dev:web
npm run dev:api

# CLI invokes the shared application library directly; it does not contact HTTP.
cargo run --locked -p chemifree-cli -- status --json
cargo run --locked -p chemifree-cli -- capabilities --json

# Optional API port; the address remains 127.0.0.1.
cargo run --locked -p chemifree-api -- --port 3002
```

The development proxy targets port 3001. If you choose another API port, update `apps/web/vite.config.ts` accordingly. The standalone CLI's `status` confirms its local application can run; it does not report whether an HTTP server is running.

## Check and build

```bash
npm run lint             # ESLint: JS/TS + React Hooks checks
npm run typecheck        # TypeScript project references
npm run build:web        # Production web assets in apps/web/dist
npm run check:rust       # rustfmt + clippy (-D warnings) + Rust tests
npm run test:contracts   # Real HTTP/CLI parity and read-only/version boundary smoke
npm run check            # All of the above
npm run build            # Web assets and debug Rust binaries
```

The contract smoke builds the workspace, starts an API on an ephemeral loopback port, compares HTTP JSON with actual CLI JSON, verifies capability status and error routes, and terminates its server. GitHub Actions installs npm 11.13.0 into an isolated runner-temporary prefix, adds its binaries to `PATH`, and verifies the selected version before running `npm ci` and `npm run check` with pinned tools. Browser smoke is a manual verification step at this stage; there is no bundled browser test runner.

`npm run preview --workspace @chemifree/web` previews built assets on port 4173 and inherits the local API proxy; run the API separately. Vite's preview server is a local verification tool. For a production deployment, serve `/api/v1/*` to the Rust API through the same origin. Hosting, authentication, and a production deployment configuration are future work.

## Workspace

```text
apps/
  web/              React + TypeScript + Vite presentation
  api/              Axum HTTP entry adapter; loopback only
  cli/              clap entry adapter; shared application calls
crates/
  domain/           Domain vocabulary; no dependencies
  application/      Use cases, v1 DTOs, and CapabilityCatalog port
  providers/        Scaffold provider implementing the application-owned port
scripts/
  contract-smoke.mjs HTTP/CLI boundary verification
docs/
  CONTRACT.md       v1 shape, compatibility rules, and capability identifiers
  architecture.html Detailed architecture and staged design
  SHADCN_REVIEW.ko.md shadcn/ui and @shadcn/lint review
```

Dependencies point inward: `application → domain`; `providers → application + domain`; HTTP and CLI compose the application with a provider. The web mirror of the v1 contract validates responses at runtime. There is no code generation, service locator, persistence layer, or scientific engine in the starter. Future chemistry domain rules belong in the inner crates; infrastructure providers implement ports owned by those inner layers. A scientific editor/renderer should be developed separately from general UI controls.

## API and implementation status

The [interactive architecture document](docs/architecture.html) maps these layers to the actual source files, current contracts, UI decisions, and next implementation steps. Download the HTML and open it in a browser; GitHub's file viewer displays its source. It is self-contained and works offline. See also the [shadcn review](docs/SHADCN_REVIEW.ko.md) and [contract reference](docs/CONTRACT.md).

| HTTP | CLI | Meaning |
| --- | --- | --- |
| `GET /api/v1/health` | `status --json` | Application liveness contract; not scientific readiness |
| `GET /api/v1/capabilities` | `capabilities --json` | Actual implementation status |

All JSON responses include `api_version: "v1"`. Shared Rust DTOs live in `crates/application`; the TypeScript mirror lives in `apps/web/src/lib/contract.ts`. See [the contract](docs/CONTRACT.md) for version policy. Runtime inspection is the only `available` capability. All seven product/integration capabilities report `not_implemented`.

The API binds IPv4 loopback and exposes GET endpoints only (HEAD is supplied by the router). It sets no CORS permissions. Authentication and authorization are planned; the scaffold has no accounts or user data. A remote, multiuser, or write-capable service requires a separately implemented access-control boundary.

## UI components and lint

The starter uses official **@shadcn** registry components with **Base UI / Nova**, Tailwind CSS 4, semantic tokens, Lucide icons, and a locally bundled Geist font. Only Button, Card, Badge, Empty, Alert, and Skeleton are installed. `components.json` records aliases and the theme CSS path. Generated source has been reviewed; upstream notices are preserved in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

Initialization used **shadcn CLI 4.22.1**:

```bash
cd apps/web
npx shadcn@latest init --base base --preset nova --no-monorepo -y
npx shadcn@latest info --json
npx shadcn@latest docs button card badge empty alert skeleton
npx shadcn@latest add @shadcn/button @shadcn/card @shadcn/badge @shadcn/empty @shadcn/alert @shadcn/skeleton -y
```

Use the CLI's current help when adding components; `components.json` resolves this configuration to `base-nova`. The installed CLI and dependencies are pinned by `package-lock.json`. Review generated changes before committing them.

**`@shadcn/lint` 0.2.3 is installed and registered as an ESLint plugin. This validates plugin integration only; no shadcn design rules are enabled or enforced in default lint or CI yet.** It is a separate ESLint/Oxlint plugin, not a `shadcn lint` CLI command. Read [the review](docs/SHADCN_REVIEW.ko.md) before selecting design rules for this project.

## License and contribution boundary

[MIT](LICENSE), Copyright (c) 2026 EunhakChoi. Third-party components and fonts retain [their own notices](THIRD_PARTY_NOTICES.md).

Read [AGENTS.md](AGENTS.md) before contributing. Product code is authored independently from public specifications and independent sources. Keep original binaries, decompilations, private datasets, local models, credentials, database files, caches, and investigation evidence outside this public repository. Scaffolding verification does not establish chemical correctness or compatibility with any other application.
