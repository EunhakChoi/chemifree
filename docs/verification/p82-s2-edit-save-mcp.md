# P82 S2: MCP edit and save tools

P82 stays `in_progress`. Planner decisions taken (the user asked to follow the planner's recommendations): an open `commands` schema validated by the server, with generated schemas and the drift check moved to S4; `partial` is a result, not a tool error; agent saves are allowed and marked destructive; no `discard_unsaved`.

## Scope

- Six tools on `apps/mcp`, named `chemifree_` and the operation id: `chemifree_document_create`, `chemifree_document_transaction`, `chemifree_history_undo`, `chemifree_history_redo`, `chemifree_document_save`, `chemifree_document_close`. Arguments map to the operation parameters of `crates/application/src/operations/dispatch.rs`; optional arguments are omitted, never null; selections and `discard_unsaved` are not sent.
- `expected_revision` is required for transaction, undo, redo and save.
- `commands` is an array of objects with an `op`, described from the catalog; the server validates it (`invalid_request`, `invalid_command` with `details.command_index`).
- Replay: the server replays edits for the same `request_id` and arguments. The adapter resolves the id before the call, and when an edit's call fails after the request may have reached the server, the error names the id so the agent can ask again. A `413` (256 KiB body limit) is reported as not applied, with advice to split the transaction.
- Results: `isError` for outcomes other than `ok` and `partial`, protocol errors and failed calls. S1's rule treated `partial` as an error; an applied edit with notes is now a result.
- Annotations: the S1 tools are read-only; save is destructive (it commits the shared session, including the web user's unsaved edits at that revision).

## Acceptance mapping (this slice)

| P82 acceptance | Evidence |
|---|---|
| Editing and saving verified first, with a real MCP client | `scripts/mcp-client-check.mjs` (`npm run test:mcp`, CI): 9 tools with per-tool read-only and destructive hints and the transaction's required arguments; create replayed by `request_id`; an agent transaction is seen by HTTP at the new revision; the same `request_id` replays the same reply; a web-style HTTP edit at a stale revision gets `409 revision_conflict`, and an agent edit at a stale revision gets `revision_conflict` with `current_revision`; save, close and snapshot read the saved file back; after the API restarts on the same data directory, the same MCP process finds it again (after one failed call) and reads revision 2 |
| Duplicate calls follow the HTTP/CLI policy | `apps/mcp/tests/edit_tools.rs`: replay of transaction, undo and save, `request_id_reused` for other arguments; the server enforces revisions |
| Unsupported models / errors | server validation passes through (`invalid_request`, `invalid_command`, empty commands, `unsaved_changes` on close) |
| Large requests | a 300 KB transaction gets `413`, reported as not applied, revision unchanged |

## Checks run

- `cargo test -p chemifree-mcp`: 59 passed (29 unit, 8 edit tools, 19 live server, 3 stdio).
- `npm run test:mcp -- --profile debug`: passed.
- Mutations caught (Rust): `partial` as an error, save not destructive, lost-reply text without the request id, close sending `discard_unsaved`.
- Mutations caught (client check): a tenth tool name, replayed revision, conflict code.

## Limits

- After the API restarts on another port, the adapter's first call goes to the cached address and fails as unreachable (nothing was sent); the next call reads the record again.
- No save as, rename, delete, recovery, `discard_unsaved`, selections, or other editing operations (paste, templates, aromatic conversion, layout apply, import).
- `commands` is not described by a generated schema yet (S4); an agent learns the op names from `chemifree_capabilities` and the fields from the contract.
- The tool error text for `413` names a 256 KiB limit as a constant of the adapter's text.
- Live display of agent edits in an open web editor is not part of P82 (the editor learns of them through `revision_conflict`).
