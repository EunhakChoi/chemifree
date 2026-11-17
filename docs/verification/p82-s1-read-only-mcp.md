# P82 S1: read-only local MCP adapter

T2 started at the user's request (2026-11-17), following the planner's recommendation. P82 stays `in_progress`.

## Scope

- New entry adapter `apps/mcp` (`chemifree-mcp`): an MCP server over stdio built on `rmcp` 3.10 (Apache-2.0). It is an HTTP client of a running `chemifree-api` and never assembles an `Application`, takes the server lock or starts a server (P82 note: an agent's unsaved edits must be the open web editor's edits).
- Discovery: `--server-url` / `CHEMIFREE_SERVER_URL` (loopback form only) or the `url` of `<data-dir>/server.lock`, read through the new `ServerLock::recorded_holder` without taking the lock; looked up again after a connection failure.
- Session token from `GET /api/v1/session`; one refetch and resend on `401`/`403`.
- Three read-only tools: `chemifree_capabilities` (`capabilities.inspect`), `chemifree_documents_list` (`GET /api/v1/documents`), `chemifree_document_snapshot` (`document.snapshot`). Results carry the server's reply unchanged as `structuredContent`; a non-`ok` outcome, a protocol error, a reply over 8 MiB or a transport failure is `isError`.
- Tool argument schemas come from the adapter's own argument types (`schemars`); generating schemas from the application DTOs is left to the edit-tool slice.

## Acceptance mapping (this slice)

| P82 acceptance | Evidence |
|---|---|
| A real MCP client sees the registered operations | `scripts/mcp-client-check.mjs` (`npm run test:mcp`, CI browser job): `@modelcontextprotocol/sdk` 1.32.2 `StdioClientTransport` initializes `chemifree-mcp`, lists exactly the three tools (each `readOnlyHint`), and `chemifree_capabilities` returns the same catalog as HTTP `capabilities.inspect` |
| … and document handles and revisions | same check: the document list equals the document created over HTTP; the snapshot revision (1, with an unsaved edit) equals HTTP `document.snapshot`'s; an unknown document is a tool error |
| Same policy as HTTP/CLI | the adapter sends contract 1 requests to the server's `/api/v1/operations`, so replay, revision checks and `unsupported` answers are the server's; `apps/mcp/tests/live_server.rs` (15 tests against `chemifree_api::router` on a temp data dir) and `tests/stdio.rs` (3 tests over the built binary) |
| Editing and saving first; analysis and export per engine PR | not in S1 |

## Checks run

- `cargo test -p chemifree-mcp`: 33 passed (15 unit, 15 live server, 3 stdio); `crates/providers` `recorded_holder` tests: 2 passed.
- `npm run test:mcp -- --profile debug`: passed.
- Mutations caught (Rust): reply limit removed, 403 not treated as refusal, retry with the stale session, outcome predicate flipped, contract 2, record ignored, explicit address ignored, 5xx treated as refusal, digit limit 1–6, uppercase request id, session cache never dropped (after making the stopped-server test bind its replacement first). Removing the explicit `Host` header is an equivalent mutant (hyper sets it from the URI).
- Mutations caught (client check): wrong tool name, wrong revision, wrong catalog field, unknown id not an error, wrong document list.

## Limits

- Read-only; no edit, save or close tools; no MCP resources; no analysis or export tools.
- No live display of agent edits in an open web editor (it learns of a new revision through `revision_conflict`).
- No schema drift check between Rust DTOs and the web's TypeScript validators yet.
- No client-side request timeout; a hung server call waits.
- `scripts/mcp-client-check.mjs` is outside eslint/tsc coverage (as other root scripts); it is checked by running it.
