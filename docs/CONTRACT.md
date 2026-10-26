# Chemifree local API v1

This is an inspection contract for the scaffold. Rust response DTOs are shared by HTTP and the CLI in `crates/application/src/lib.rs`. The browser keeps a small, hand-maintained mirror with runtime validation in `apps/web/src/lib/contract.ts`. There is no generated SDK at this stage.

## Entry points

| HTTP | CLI | Response |
| --- | --- | --- |
| `GET /api/v1/health` | `chemifree status --json` | Health |
| `GET /api/v1/capabilities` | `chemifree capabilities --json` | Capabilities |

The CLI calls the application service in-process. It works without an API process; its health output is not a network connectivity check. HTTP `status: "ok"` confirms liveness of the API handler only. Neither output claims chemistry is implemented.

Health response:

```json
{
  "api_version": "v1",
  "service": "chemifree",
  "version": "0.1.0",
  "stage": "scaffold",
  "status": "ok"
}
```

Capabilities response has `api_version: "v1"` and a `capabilities` array. Each item contains four required strings:

| Field | Meaning |
| --- | --- |
| `id` | Stable identifier below; consumers must tolerate new identifiers |
| `label` | Human-readable display label |
| `status` | `available` or `not_implemented` |
| `description` | Honest explanation of the implementation status |

Identifiers must be unique. Human-readable labels and descriptions are not machine identifiers. The current list is:

| ID | Current status |
| --- | --- |
| `runtime_inspection` | `available` |
| `document_editing` | `not_implemented` |
| `structure_analysis` | `not_implemented` |
| `format_conversion` | `not_implemented` |
| `local_model_loading` | `not_implemented` |
| `persistence` | `not_implemented` |
| `authentication` | `not_implemented` |
| `windows_bridge` | `not_implemented` |

No operation endpoints are advertised for unimplemented capabilities. Unsupported routes return HTTP 404; unsupported methods on implemented routes return HTTP 405. Errors have `{ "api_version": "v1", "code": "not_found" | "method_not_allowed", "message": string }`. Axum supplies HEAD for the GET routes. The API currently binds only `127.0.0.1` and does not grant CORS access.

## Compatibility and verification

Every response includes `api_version: "v1"`; HTTP paths carry the same major version. Additive fields and new capability IDs may be introduced within v1. Consumers ignore unknown fields. Removing/renaming required fields, changing field types, or adding new status values requires a reviewed version change and synchronized Rust/TypeScript updates. The current browser deliberately rejects unknown contract versions, unsupported stage/status values, malformed records, an empty capability array, and duplicate IDs.

`version` is the application release version, independent of API major version. `stage: "scaffold"` is the only stage currently supported; changing that requires a deliberate contract and UI review.

`npm run test:contracts` compares parsed JSON from real HTTP and CLI processes and checks the read-only/error/version boundary. Rust route tests verify shared DTO serialization. Browser smoke checks cover connection failure, retry recovery, incompatible responses, mobile layout, and system dark mode. None of these checks validate chemical behavior.

When functionality is implemented, replace the relevant capability status only after connecting the real operation and its meaningful checks. Model provenance, storage, authentication/authorization, remote deployment, and Windows integration remain design work.
