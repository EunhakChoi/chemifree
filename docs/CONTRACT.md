# Chemifree local API v1

Rust DTOs are shared by HTTP and the CLI in `crates/application/src/` (`lib.rs`, `contract.rs`, `error.rs`). The browser keeps a hand-maintained mirror with runtime validation in `apps/web/src/lib/contract.ts` and `apps/web/src/lib/document-contract.ts`. There is no generated SDK.

Every JSON response includes `api_version: "v1"`; HTTP paths carry the same major version.

## Entry points

| HTTP | CLI | Response |
| --- | --- | --- |
| `GET /api/v1/health` | `chemifree status --json` | Health |
| `GET /api/v1/capabilities` | `chemifree capabilities --json` | Capabilities |
| `GET /api/v1/session` | — | Write token for this API process |
| `GET /api/v1/documents` | `chemifree documents list --json` | Document list |
| `POST /api/v1/documents` | `chemifree documents create [--title T] --json` | Document (HTTP 201) |
| `GET /api/v1/documents/{id}` | `chemifree documents show ID --json` | Document |
| `POST /api/v1/documents/{id}/transactions` | `chemifree documents apply ID --expected-revision N --commands JSON` | Document |
| `POST /api/v1/documents/{id}/undo` | `chemifree documents undo ID --expected-revision N` | Document |
| `POST /api/v1/documents/{id}/redo` | `chemifree documents redo ID --expected-revision N` | Document |
| `POST /api/v1/documents/{id}/save` | (every CLI edit saves) | Document |
| `POST /api/v1/documents/{id}/close` | — | Close |

The CLI calls the same `Application` in-process and does not contact HTTP. Both use `--data-dir` / `CHEMIFREE_DATA_DIR` (default: the platform user-data directory, e.g. `~/.local/share/chemifree`). `status`, `capabilities`, and `show`/`list` do not write. A CLI process is its own short session, so `apply`, `undo`, and `redo` commit with a save before reporting success; if the save fails, nothing is committed. With `--json`, failures are printed to stdout as the error envelope and the exit status is 1.

## Inspection

Health: `{ "api_version", "service": "chemifree", "version", "stage": "scaffold", "status": "ok" }`. `ok` is handler liveness only. `stage` remains `scaffold`; most product capabilities are still unimplemented.

Capabilities: `{ api_version, capabilities: [{ id, label, status, description }] }`, `status` ∈ `available | not_implemented`. Identifiers are unique; consumers tolerate new ones.

| ID | Status |
| --- | --- |
| `runtime_inspection` | `available` |
| `document_editing` | `available` — drawing model only |
| `structure_analysis` | `not_implemented` |
| `format_conversion` | `not_implemented` |
| `local_model_loading` | `not_implemented` |
| `persistence` | `available` — local Chemifree JSON files |
| `authentication` | `not_implemented` |
| `windows_bridge` | `not_implemented` |

## Documents

A document is a titled drawing: atoms and bonds with one set of scene positions. It records what was drawn. It does not perceive valence, implicit hydrogens, aromaticity, stereochemistry, or layout, and no field implies chemical validity.

```json
{
  "api_version": "v1",
  "document": {
    "id": "doc-1e4309eae7a8f67b",
    "title": "Ethanol",
    "revision": 1,
    "atoms": [
      { "id": "a1", "element": "C", "charge": 0, "isotope": null, "position": { "x": 0, "y": 0 } },
      { "id": "a2", "element": "O", "charge": -1, "isotope": 18, "position": { "x": 26, "y": 15 } }
    ],
    "bonds": [{ "id": "b1", "begin": "a1", "end": "a2", "order": "single" }]
  },
  "session": { "saved_revision": 0, "unsaved_changes": true, "undo_depth": 1, "redo_depth": 0 }
}
```

- `id`: 1–64 characters of `[a-z0-9-]`. Atom IDs are `a<n>`, bond IDs `b<n>`. IDs are stable and never reused within a document, including after delete or undo.
- `position`: points (pt), x right, y down, finite and within ±100000.
- `element`: case-sensitive IUPAC symbol, H through Og. `charge`: −8..8. `isotope`: mass number 1..999, or `null` for natural abundance.
- `order`: `single | double | triple`. At most one bond joins a pair of atoms; self-bonds are rejected.
- Limits: 2000 atoms, 4000 bonds, title 1–200 characters without control characters (surrounding whitespace is trimmed).
- `revision` increases with every effective transaction, undo, or redo: by one, or further when a working copy continues past numbers handed out for discarded states (see No reuse). It never moves backward. It stays at most 2^53 − 1 so JSON clients read it exactly; a document there accepts no further changes (`503 resource_limit`), and files beyond it are not opened.
- `session` describes the working copy held by this process. `saved_revision` is the session revision whose content is the stored commit.

## Edits

Transaction body: `{ "request_id": string, "expected_revision": number, "commands": [Command, ...] }`, 1–500 commands. Commands are tagged by `op`:

| `op` | Fields | Effect |
| --- | --- | --- |
| `add_atom` | `element`, `position`, optional `bond_to: { atom, order }` | New neutral, natural-abundance atom |
| `add_bond` | `begin`, `end`, `order` | New bond |
| `update_atom` | `atom`, optional `element`, `charge`, `isotope` | Omitted fields unchanged; `"isotope": null` restores natural abundance |
| `move_atoms` | `atoms`, `delta: { x, y }` | Each listed atom moves once |
| `update_bond` | `bond`, `order` | Changes order |
| `delete_atoms` | `atoms` | Deletes the atoms and every incident bond |
| `delete_bonds` | `bonds` | Deletes the bonds |
| `rename` | `title` | Changes the title |

Where a command names an atom for a new bond (`bond_to.atom`, `begin`, `end`), `"$N"` refers to the atom added by the `add_atom` command at index N earlier in the same transaction. This lets one transaction add two atoms and bond them.

A transaction applies all of its commands or none. Commands that leave the document identical (e.g. moving by zero) succeed without a new revision or undo entry. Unknown fields and unknown `op` values are rejected.

Undo, redo, and save bodies: `{ "request_id", "expected_revision" }`. Undo reverts the latest transaction as a new revision; redo reapplies it; a new transaction after undo discards the redo branch. History keeps the latest 100 entries and at most 100,000 atom/bond/title changes in total (undo and redo together), dropping the oldest, and is saved with the document, so it survives reopening. Selection is client state and is not part of history.

## Consistency, idempotency, and saving

- **Revisions.** Every mutation carries `expected_revision`. A mismatch returns `409 revision_conflict` with `current_revision` and changes nothing.
- **Request IDs.** `request_id` is 1–64 characters of `[A-Za-z0-9_-]`, required over HTTP. Repeating a completed request with the same ID and body returns the original response without applying it again. Reusing an ID for a different body returns `409 request_id_reused`. Replays are remembered for the latest 64 successful mutations per open document (and per process for creates and closes, so a retried close cannot end a session opened after it); failed requests are not remembered, so a retry runs again.
- **Sessions.** The first access opens a document from storage into a per-process working copy. Edits change only the working copy until `save`. A working copy without unsaved changes follows newer commits by other writers (e.g. the CLI) on its next access, so an edit based on the older view gets `revision_conflict`. `close` ends the session; the next access reopens the committed file. Closing with unsaved changes returns `409 unsaved_changes` unless the body has `"discard_unsaved": true` together with `expected_revision`, the revision whose changes the client agreed to lose; the session is shared by all clients of the process, so a stale revision is `409 revision_conflict` and nothing is discarded. Close body: `{ "request_id", "discard_unsaved"?, "expected_revision"? }`. If a process stops, unsaved edits are lost and the last committed save remains.
- **No reuse.** Within a process, a revision number or atom/bond ID that a client has seen never names a different state later. After a discard (or when a clean copy reloads another writer's commit) the working copy continues numbering above everything it handed out, so a stale `expected_revision` is a conflict. A restarted process cannot know what the previous one handed out. When its token is refused, the web client re-reads the document and resends an edit only if the restarted service holds exactly the drawing on screen (never an undo or redo); otherwise it asks for a reload.
- **Create** commits the empty document before replying. It is the one write without `expected_revision`, so a client that got no response retries with the same `request_id`; the web client does this.
- **Save** writes a temporary file in the same directory, flushes it, and renames it over the document; the rename is the commit. The directory is then synced where the platform supports it, and a failure of that sync does not turn the committed save into an error. The acknowledgement is sent only after the commit. A document that would encode beyond the 64 MiB read limit is not written. On failure the previous file is intact, the reply is `503 storage_failed`, and the session keeps its unsaved changes for a retry. Saving an unchanged session is a successful no-op.
- **Concurrent writers.** A save requires storage to still hold the commit this session last saw. If another writer (another session or the CLI) committed first, the reply is `409 save_conflict` and nothing is written; close with `discard_unsaved` and reopen. The check and the rename are not a single atomic step across processes; two processes saving in the same instant can still race.

## Write protection

The API binds `127.0.0.1` only and never sends CORS headers.

1. Every request must carry a `Host` of `127.0.0.1:<port>` / `localhost:<port>` or the host of an allowed web origin, otherwise `403 forbidden_host` (DNS rebinding defense).
2. `GET /api/v1/session` and all writes reject a disallowed `Origin` and `Sec-Fetch-Site` other than `same-origin`/`none` with `403 forbidden_origin`.
3. Writes require the header returned by `GET /api/v1/session` (`x-chemifree-session: <token>`), otherwise `401 unauthorized`. The token is random per API process; a restarted API issues a new one.
4. Writes require `Content-Type: application/json` (`415 unsupported_media_type`) and at most 256 KiB (`413 payload_too_large`).

Allowed web origins default to `http://127.0.0.1:5173`, `http://localhost:5173`, and the same hosts on port 4173; `--allow-origin` replaces the list and accepts loopback origins only. This keeps other websites from driving the API. It is not user authentication: any local process that can reach the port can obtain the token, so it does not separate local OS users. `authentication` stays `not_implemented`.

## Errors

`{ "api_version": "v1", "code": string, "message": string, "current_revision"?: number, "command_index"?: number }`

| HTTP | `code` |
| --- | --- |
| 400 | `invalid_json` |
| 401 | `unauthorized` |
| 403 | `forbidden_host`, `forbidden_origin` |
| 404 | `not_found` (route), `document_not_found` |
| 405 | `method_not_allowed` |
| 409 | `revision_conflict`, `request_id_reused`, `nothing_to_undo`, `nothing_to_redo`, `unsaved_changes`, `save_conflict` |
| 413 | `payload_too_large` |
| 415 | `unsupported_media_type` |
| 422 | `invalid_request` (malformed body or field), `invalid_command` (with `command_index`) |
| 500 | `document_unreadable`, `internal_error` |
| 503 | `storage_failed`, `resource_limit` |

The CLI uses the same codes, exit status 1. A process keeps at most 64 open sessions; clean sessions are evicted first, and `resource_limit` is returned only when all hold unsaved changes.

## Document file format

`<data-dir>/documents/<id>.json`, owner-only permissions on Unix:

```json
{
  "format": "chemifree.document",
  "format_version": 1,
  "document": { "id", "revision", "title", "atoms": [{ "id", "state": { "element", "charge", "isotope", "position" } }],
                "bonds": [{ "id", "state": { "begin", "end", "order" } }], "next_atom", "next_bond" },
  "history": { "undo": [ChangeSet], "redo": [ChangeSet] }
}
```

A change set is `{ "title": { "before", "after" } | null, "atoms": [{ "id", "before", "after" }], "bonds": [...] }`, with `null` where the entity did not exist. `undo` is oldest first; the last `redo` entry is redone next. Loading rejects unknown fields, other format versions, files over 64 MiB, dangling or duplicate references, counters that are zero or would reuse IDs, and history that does not replay cleanly (including change sets that are empty, repeat an entity, or contain no-op entries); history beyond the limits is trimmed oldest first, as editing trims it; such files are listed under `unreadable` instead of being opened. Floats round-trip bit-exactly. This is Chemifree's working format, not an interchange format; MOL/SDF/CDXML/SMILES remain `not_implemented`.

## Compatibility and verification

Additive fields, endpoints, commands, and capability IDs may be introduced within v1; consumers ignore unknown response fields. Removing or renaming fields, changing types, or adding status values requires a reviewed version change and synchronized Rust/TypeScript updates. The browser rejects unknown versions, malformed records, duplicate IDs, and bonds that reference missing atoms.

- `cargo test --workspace`: domain invariants, transactions, undo/redo, restore validation; application revisions, replays, save failure and conflict; file store atomicity and corruption; HTTP guards and error mapping.
- `npm run test:contracts`: real API and CLI processes on a temporary data directory — HTTP = CLI JSON, write guards, the full create/edit/undo/redo/save/close/reopen flow, conflicts, save failure and retry, and recovery after the API is killed.
- `npm run test:browser`: manual browser smoke (not in CI); results in `verification/editor-browser-smoke.json`.

None of these checks validate chemical behavior or compatibility with any other application.
