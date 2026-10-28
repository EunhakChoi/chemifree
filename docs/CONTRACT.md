# Chemifree local API v1

Rust DTOs are shared by HTTP and the CLI in `crates/application/src/` (`lib.rs`, `contract.rs`, `error.rs`). The browser keeps a hand-maintained mirror with runtime validation in `apps/web/src/lib/contract.ts` and `apps/web/src/lib/document-contract.ts`. There is no generated SDK.

Every JSON response includes `api_version: "v1"`; HTTP paths carry the same major version.

## Entry points

| HTTP | CLI | Response |
| --- | --- | --- |
| `GET /api/v1/health` | `chemifree status --json` | Health |
| `GET /api/v1/capabilities` | `chemifree capabilities --json` | Capabilities |
| `GET /api/v1/session` | — | Write token for this API process |
| `GET /api/v1/documents?query=Q&sort=title\|recent` | `chemifree documents list [--query Q] [--sort title\|recent] --json` | Document list |
| `POST /api/v1/documents` | `chemifree documents create [--title T] --json` | Document (HTTP 201) |
| `GET /api/v1/documents/{id}` | `chemifree documents show ID --json` | Document |
| `POST /api/v1/documents/{id}/transactions` | `chemifree documents apply ID --expected-revision N --commands JSON` | Document |
| `POST /api/v1/documents/{id}/undo` | `chemifree documents undo ID --expected-revision N` | Document |
| `POST /api/v1/documents/{id}/redo` | `chemifree documents redo ID --expected-revision N` | Document |
| `POST /api/v1/documents/{id}/save` | (every CLI edit saves) | Document |
| `POST /api/v1/documents/{id}/close` | — | Close |
| `POST /api/v1/documents/{id}/autosave` | — (CLI edits save) | Document |
| `POST /api/v1/documents/{id}/save-as` | `chemifree documents save-as ID --expected-revision N [--title T]` | Copy (HTTP 201) |
| `POST /api/v1/documents/{id}/recover` | `chemifree documents recover ID` (recovers, then saves) | Document |
| `POST /api/v1/documents/{id}/recover-as-copy` | `chemifree documents recover-as-copy ID [--title T]` | Copy (HTTP 201) |
| `POST /api/v1/documents/{id}/discard-draft` | `chemifree documents discard-draft ID` | Draft discard |
| `POST /api/v1/documents/{id}/rename` | `chemifree documents rename ID --expected-revision N --title T` | Document |
| `POST /api/v1/documents/{id}/delete` | `chemifree documents delete ID --expected-revision N` | Delete |
| `GET /api/v1/trash` | `chemifree documents trash --json` | Trash list |
| `POST /api/v1/trash/{id}/restore` | `chemifree documents restore ID` | Restore |

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
  "session": { "saved_revision": 0, "unsaved_changes": true, "undo_depth": 1, "redo_depth": 0, "draft_revision": 1, "draft_failure": null },
  "recovery": null
}
```

- `id`: 1–64 characters of `[a-z0-9-]`. Atom IDs are `a<n>`, bond IDs `b<n>`. IDs are stable and never reused within a document, including after delete or undo.
- `position`: points (pt), x right, y down, finite and within ±100000.
- `element`: case-sensitive IUPAC symbol, H through Og. `charge`: −8..8. `isotope`: mass number 1..999, or `null` for natural abundance.
- `order`: `single | double | triple`. At most one bond joins a pair of atoms; self-bonds are rejected.
- Limits: 2000 atoms, 4000 bonds, title 1–200 characters without control characters (surrounding whitespace is trimmed).
- `revision` increases with every effective transaction, undo, or redo: by one, or further when a working copy continues past numbers handed out for discarded states (see No reuse). It never moves backward. It stays at most 2^53 − 1 so JSON clients read it exactly, and files beyond it are not opened. Edits, undo, and redo stop at 2^53 − 2 (`503 resource_limit`); the last number is kept so a copy reopened after a discard still gets a fresh revision. When no fresh revision remains (e.g. to follow another writer's commit after that), the document returns `resource_limit` in this process and is listed under `unreadable`.
- `session` describes the working copy held by this process. `saved_revision` is the session revision whose content is the stored commit. `draft_revision` is the revision this session's recovery draft holds, or `null` when it has none. `draft_failure` is `{ attempts, retry_in_ms, message }` while the service's own drafting of this session keeps failing (see Autosave and recovery), and `null` otherwise; it was added within v1.
- `recovery` describes a recovery draft left by an earlier session of this document (see Autosave and recovery), or is `null`. The document list carries the same field per document.

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
- **Request IDs.** `request_id` is 1–64 characters of `[A-Za-z0-9_-]`, required over HTTP. Repeating a completed request with the same ID and body returns the original response without applying it again. Reusing an ID for a different body returns `409 request_id_reused`. Replays are remembered for the latest 64 successful mutations per open document (and per process for creates, closes, deletes, and restores, so a retried close cannot end a session opened after it); failed requests are not remembered, so a retry runs again. A create, or a write that takes a document's turn (see Time budget), whose `request_id` is still running is answered `409 request_in_progress` at once, whatever the body (see Outcome of a lost reply). A `request_id` is matched where its reply is remembered: per document for saves, autosaves, and renames, per process for creates, closes, recoveries, draft discards, copies, deletes, and restores.
- **Sessions.** The first access opens a document from storage into a per-process working copy. Edits change only the working copy until `save`. A working copy without unsaved changes follows newer commits by other writers (e.g. the CLI) on its next access, so an edit based on the older view gets `revision_conflict`. `close` ends the session; the next access reopens the committed file. Closing with unsaved changes returns `409 unsaved_changes` unless the body has `"discard_unsaved": true` together with `expected_revision`, the revision whose changes the client agreed to lose; the session is shared by all clients of the process, so a stale revision is `409 revision_conflict` and nothing is discarded. Close body: `{ "request_id", "discard_unsaved"?, "expected_revision"? }`. If a process stops, the last committed save remains, and unsaved edits survive only in the session's recovery draft (see Autosave and recovery).
- **No reuse.** Within a process, a revision number or atom/bond ID that a client has seen never names a different state later. After a discard (or when a clean copy reloads another writer's commit) the working copy continues numbering above everything it handed out, so a stale `expected_revision` is a conflict. A restarted process cannot know what the previous one handed out. When its token is refused, the web client re-reads the document and resends an edit only if the restarted service holds exactly the drawing on screen (never an undo or redo); otherwise it asks for a reload.
- **Create** commits the empty document before replying. Like the other writes that make a new document (save as copy, recover as copy), it is not guarded by a stored revision, so a client that got no response retries with the same `request_id`; the web client does this. A restarted service has forgotten that `request_id`, so when the retry meets a new token the web client does not resend: the document may already exist, and the page says so and refreshes the list.
- **Save** writes a temporary file in the same directory, flushes it, and renames it over the document; the rename is the commit. The directory is then synced where the platform supports it, and a failure of that sync does not turn the committed save into an error. The acknowledgement is sent only after the commit. A document that would encode beyond the 64 MiB read limit is not written. On failure the previous file is intact, the reply is `503 storage_failed` with a `reason` (see Storage failures), and the session keeps its unsaved changes for a retry. Saving an unchanged session is a successful no-op.
- **Concurrent writers.** A save requires storage to still hold the commit this session last saw. If another writer (another session, another API process, or the CLI) committed first, the reply is `409 save_conflict` and nothing is written; close with `discard_unsaved` and reopen, or save as a copy. Every save, from any process, holds an OS file lock on `<data-dir>/locks/<id>.lock` from the base check through the commit, so of two saves from the same base exactly one commits and the other is a conflict. The operating system releases the lock when its holder exits, including a crash. A save waits at most 5 seconds for another process's save of the same document, then fails with `503 storage_failed`, `reason: "busy"`, and can be retried. While a save waits, requests for other documents and edits to the same document proceed; the save commits the revision it was asked for (`expected_revision`), and edits made meanwhile stay unsaved.
- **Time budget per document.** Within one API process, saves, autosaves, closes, recoveries, draft discards, copies, renames, deletes, and restores of one document take turns. A request waits at most 3 seconds for its document's turn; if it does not get it, it changes nothing and fails with `503 storage_failed`, `reason: "busy"`. With the 5-second lock wait above, such a request is answered within about 8 seconds plus its own storage I/O, before the web client's 10-second deadline, however many are queued for that document (for example from two tabs). There is no cancel: a request that has its turn runs to the end.
- **Storage failures.** `storage_failed` carries `reason`: `read_only` (storage refused the write: missing permission or a read-only file system; retrying helps only after that is fixed), `busy` (the document stayed busy for longer than a request waits; nothing was changed, and retrying later can succeed), or `io` (any other read or write failure, including a file that cannot be read for lack of permission). In every case the previous commit and any recovery draft are intact, and the session keeps its unsaved changes.
- **Outcome of a lost reply.** A request whose reply never arrived (a timeout, a dropped connection) may still have taken effect. Sending it again with the same `request_id` and body is safe. While a request with that `request_id` still runs, the reply is `409 request_in_progress` at once, without waiting for the document's turn or for other requests' storage I/O. Once it completed, the original reply is returned without waiting for the document's turn, as long as it is still remembered (the replay window under Request IDs). If it failed, or is no longer remembered, it runs again. A committed save asked about within that window is therefore never reported as failed because its reply was lost. The web client does this for saves: when no reply arrives it asks again at once, then every second while the save is in progress (at most 12 times); if the service stays silent it reports the save as unconfirmed (`save_unconfirmed`), and **Check again** saves the revision on screen: if nothing was edited since, that is the same revision, asked about with the same `request_id`; otherwise it is a new save, which also commits the earlier changes. For creates and copies it keeps the `request_id` while the service reports them in progress, and asks again every second in the same way. Renames, deletes, and restores are kept from acting twice by their own preconditions (the expected revision; the document being stored or trashed), so the client asks about a lost reply once at once and then every second while it is in progress, with the same `request_id` and only in the service process it was sent to (`service_restarted` once the page meets a different one), and otherwise reports the failure as it is: a later attempt is a new request, which the precondition refuses if the first one took effect (`revision_conflict` or `document_not_found`). An unknown outcome belongs to the service process it was sent to (each process has its own session token): once any request of the page meets a different token, the client does not ask the new process with that `request_id`, which it has never seen, but reports the outcome as unknown (`create_unconfirmed`, `copy_unconfirmed`, or `service_restarted` for a save). A gateway error without a v1 body (502, 503, or 504 from a proxy in front of the service, such as the development server's) counts as no reply. When a create or copy is answered after being asked about again, the answer may be the record of the first attempt, so the client reads the document again and shows it as it is now.

## Autosave and recovery

A working copy with unsaved changes can be autosaved to a recovery draft, `<data-dir>/drafts/<id>.json`, kept apart from the saved document. The draft holds the whole working copy: content, undo/redo history, identifier counters, its revision, and the stored revision it started from (`base_revision`). Autosave body: `{ "request_id" }`. It writes the draft when the session has unsaved changes not yet in it, and removes the session's draft when nothing is unsaved; `session.draft_revision` reports what the draft holds.

- **When, without a client.** The API process drafts unsaved work on its own, whether or not any client calls autosave: 2 seconds after the last change, and at most 10 seconds after the first change the draft does not hold, so continuous editing cannot postpone it. A failed attempt is retried after 2, 5, 15, then every 30 seconds; edits do not push a retry back, and `session.draft_failure` reports the failure until a draft succeeds. A normal stop (SIGTERM, Ctrl+C) drafts every session's unsaved work after the last request is answered and before the process exits, and logs any it could not draft. The service's drafts and client autosaves are the same operation and take the same per-document turn, so they never run at once and one revision is drafted once: whichever comes second finds nothing to do. Drafting follows the session: a save or a discarding close leaves nothing to draft, a recovered draft already holds its work, and a session whose document has an earlier session's draft or whose draft another process owns is not drafted over (each attempt fails with `recovery_pending` or `draft_in_use` and is retried until that is resolved). The CLI commits every edit and has no sessions to draft.
- **When, in the web editor.** The web editor autosaves 1 second after the last change, and at least every 5 seconds while changes keep coming. A failed autosave is shown and retried after 2, 5, 15, then every 30 seconds, or at once with **Retry now**; the session keeps its changes meanwhile, and further edits do not postpone the retry. Before the page shows another document it drafts the one it leaves; if that fails, it says so. The page's draft state is what the service reports (`session.draft_revision` and `recovery`), so after a restart nothing counts as drafted until the new service has a draft, and while an earlier session's draft exists autosave shows as paused. A reply for a document the page has left is ignored. If a reply shows the service holding an older or different state than the page (another client closed the session, or a restart the page has not seen yet), the page re-reads the document instead of drafting again. Other HTTP clients may call the endpoint to draft sooner than the service would.
- **What a crash can lose.** After a crash (SIGKILL, power loss), the changes since the last completed draft: with the web editor open, normally up to the last 1–5 seconds of editing; with any other client, up to the last 2–10 seconds; longer while drafting fails or is blocked by an earlier draft.
- **Ownership.** The process whose session wrote a draft holds an OS lock on `drafts/<id>.owner` while it runs. Another process sees that draft as `in_use` and cannot recover, overwrite, or discard it. When the owner exits (normally or not) the draft is abandoned, and any process can act on it. Within one process, requests take owner locks one at a time, so a request never mistakes another request's brief look at an abandoned draft for a different running process.
- **Recovery.** A document with an abandoned draft reports `recovery: { state, revision, base_revision, saved_at_ms, message }`:

  | `state` | Meaning | Actions |
  | --- | --- | --- |
  | `recoverable` | Storage still holds `base_revision`. | `recover`, `discard-draft` |
  | `outdated` | Another writer saved since `base_revision`. | `recover-as-copy`, `discard-draft` |
  | `unreadable` | The draft file is corrupt or unsupported, or its contents do not restore to a valid document of this ID; `message` says why. | `discard-draft` |
  | `in_use` | A running process owns it. | none here |

  `recover` makes the draft the working copy: content, undo/redo history, and identifier counters as autosaved, unsaved, with the draft still protecting it. Its revision continues above every revision this process has handed out, so no revision a client has seen is reused; recovering an untouched document after a restart keeps the draft's revision. The web editor offers `recover` and `recover-as-copy` only while the open session has no unsaved changes. `recover` requires the open session (if any) to have no unsaved changes (`409 unsaved_changes`). If storage moved past `base_revision`, or this process already handed out atom/bond identifiers beyond the stored counters, it returns `409 draft_outdated` (with `current_revision`) and the draft stays; `recover-as-copy` then saves the draft's content as a new document (as for save as copy) and removes the draft. Its reply's `source.closed` is `false` (the source's session, if open, stays as it is) unless all 64 session slots are in use and the source's clean session is the only one that can make room for the copy; then that session closes, `closed` is `true`, and the source reopens from storage on its next access. `discarded_changes` is always `false`: only a clean session is closed this way. An unreadable draft returns `500 document_unreadable` on recovery.
- **Draft lifecycle.** A save removes the session's draft with its commit. Before the commit the draft is renamed to `drafts/<id>.committing`; if the process ends before removing it, the next save-lock holder removes it when storage no longer holds its base (the commit happened) and restores it otherwise. So a saved draft never comes back, and an interrupted save never loses one. Closing with `discard_unsaved` removes the draft before the session ends (if removal fails, the close fails and nothing is discarded). `discard-draft` removes an abandoned draft. New work is not autosaved over an abandoned draft: autosave returns `409 recovery_pending` until that draft is recovered or discarded.

## Save as copy

Body: `{ "request_id", "expected_revision", "title"? }` (default: the source title followed by ` (copy)`). The working copy's atoms, bonds, and identifier counters are committed as a new document with a new ID, at revision 0, without undo history; IDs are kept, so `a3` names the same atom in both. The reply is the copy's document response plus `source: { id, closed, discarded_changes }`. The source session then closes (`source.closed: true`): its working copy lives on in the copy, and the source reopens from its last save, which the copy never changes; `discarded_changes` says whether it had unsaved changes that now live only in the copy. If the source's recovery draft cannot be removed (a storage error), the source session stays open with its unsaved changes and its draft instead, so no draft is left behind to be offered for recovery later; the copy is still committed, and the reply says `closed: false` (with `discarded_changes: false`). The web editor names that outcome in its copy notice and offers **Open original**. The notice describes the moment of the copy, so the page drops it once it shows another document or learns that the service restarted, which it does when a write's session token is refused; a page that sends no writes keeps the notice until then. Like create, save as copy is not guarded by a stored revision, so `request_id` replay (per process) makes a retried request return the same copy; the web client reuses the ID for an unanswered attempt and does not resend it after a restart (`copy_unconfirmed`). A failed commit writes nothing and leaves the source session as it was.

## Document list, rename, and trash

- **List.** `query` keeps documents whose title contains it, ignoring case (surrounding spaces are ignored; at most 200 characters, else `422 invalid_request`); an empty query lists everything. `sort` is `title` (default: title, then ID) or `recent` (last commit first, then title). The commit time is the stored file's modification time, a wall-clock time: a clock adjustment (such as a time sync stepping the clock back) can reorder documents saved within it, and a program that touches the file changes it. Each summary has `saved_at_ms`, the time storage last committed the document (`null` if unknown). Unreadable files are always listed under `unreadable`. Unknown parameters are refused with `422 invalid_request`. The web client and the CLI pass these to the same service; neither filters or sorts on its own. A list reads at most 1000 documents (open ones first, then stored ones in identifier order), and the query and sort apply to those: in a larger library, a search can miss documents beyond them.
- **Rename.** Body: `{ "request_id", "expected_revision", "title" }`. Renaming commits the new title at once, as one new revision, all or nothing; the reply is the document with no unsaved changes. The open copy must have no unsaved changes (`409 unsaved_changes`: rename it in the editor and save instead), and no abandoned recovery draft may exist (`409 recovery_pending`), since the commit would leave that draft behind its base. An unchanged title commits nothing. Another process's unsaved copy meets the rename as a `save_conflict` on its next save, as with any other writer.
- **Delete.** Body: `{ "request_id", "expected_revision" }`, the revision the user confirmed. The document moves to `<data-dir>/trash/<id>.json` in one rename, so it is either stored or trashed, never both or neither, even if the process ends meanwhile. It is refused, changing nothing, when the open copy has unsaved changes (`409 unsaved_changes`), when an abandoned recovery draft exists (`409 recovery_pending`), when another running process owns its draft (`409 draft_in_use`; if that process takes the draft over while the delete runs, `503 storage_failed` with `reason: "busy"`, and a retry names the draft), or when storage holds a different revision (`409 save_conflict`). The check and the move happen under the document's save lock and its draft owner lock, so no save or draft from another process can slip in between. While a delete or rename is being committed, edits to that document are refused as `503 storage_failed`, `reason: "busy"`, and change nothing. Reply: `{ "api_version", "id", "title" }`. The session ends.
- **After a delete.** A clean copy of the document in another process closes on its next access (`404 document_not_found`) instead of showing it as stored. A copy with unsaved changes keeps them in memory: its save is a `save_conflict` and its autosave `404 document_not_found`, so it can never re-create the document or leave a draft without it; saving it as a copy keeps the work. The CLI cannot see another process's unsaved work that has no draft yet, so such a delete succeeds and that work is kept only through save as copy.
- **Trash and restore.** `GET /api/v1/trash` lists trashed documents (at most the 1000 most recently deleted), most recently deleted first, as `{ id, title, revision, atom_count, bond_count, deleted_at_ms }` plus `unreadable`. Restore body: `{ "request_id" }`; it moves the document back as it was trashed, with its identifier, and replies `{ "api_version", "id", "title" }`. A document not in the trash is `404 document_not_found`. Identifiers of trashed documents are never given to new documents. Nothing is ever removed from the trash: there is no permanent delete or automatic emptying yet.

## Guarantees and known limits

Reviews and changes are checked against these lists. Breaking a guarantee is a defect. A known limit is current scope, not a defect; lifting one is a separate change.

Guarantees:

- **All or nothing.** A transaction, undo, redo, save, or close either takes full effect or changes nothing.
- **No lost updates.** A save never replaces a commit its session did not start from; it is refused as `save_conflict`. This holds across processes: saves of one document are serialized by an OS lock from the base check through the commit.
- **No lock-out.** A process that ends while saving, however it ends, does not block later saves.
- **Intact on failure.** A failed save leaves the previous commit intact and the unsaved changes in the session. Success is reported only after the commit.
- **Reopenable.** A committed document can be opened again. A document that could not be read back is refused, never written. The same holds for recovery drafts.
- **Recoverable work.** Unsaved work autosaved to a draft survives the end of its process and can be recovered with its undo history, or saved as a copy if the document was saved elsewhere since. A draft is never overwritten by other work before it is recovered or discarded.
- **Drafted without a client.** Within one running API process, unsaved work in a session is in its recovery draft about 10 seconds (plus a quarter-second check interval and the write) after the first change the draft does not hold, while storage accepts the write and no earlier draft blocks it, and at a normal stop. The service's periodic check does not wait for a document whose turn is taken (by a save or a client's autosave, say): it tries that document again on the next check, a quarter-second later, without counting a failed attempt, so a busy document never delays the drafts of others. Its own draft waits until the turn is free; a save that is running leaves nothing to draft once it commits. At a normal stop, no request is running, and each draft waits up to 3 seconds for its turn. A session closed before the service drafts it is not opened again to be drafted.
- **No resurrection.** After a save or an explicit discard, that draft does not come back, even if the process ends in between.
- **Nothing deleted silently.** A delete never takes unsaved work with it: it is refused while any copy in the same process or any recovery draft holds such work, and a trashed document can be restored as it was. No stale session or draft re-creates a deleted document.
- **Independent copies.** A copy has its own ID and file; saving or editing either document never changes the other, and the source's saved file is untouched by the copy.
- **At most once per process.** Within one API process, a repeated `request_id` with the same body takes effect at most once (within the replay window above).
- **No reuse per process.** Within one API process, a revision or atom/bond ID that a client has seen, including in the document list, never names different content. Recovery keeps to this: it continues numbering, or refers the draft to a copy.
- **Honest outcomes.** Clients report success only when the service confirmed it. An outcome that cannot be known is reported as unknown, not resent where a resend could act twice. A save whose reply was lost is not reported as failed: its outcome is asked for with the same `request_id` (answered from the replay window; see Outcome of a lost reply).
- **Bounded waits.** Within one API process, a request that commits a document or touches its draft waits at most 3 seconds for its document's turn and at most 5 seconds for another process's save lock, and a repeated `request_id` that is still running is answered at once. A request refused for waiting too long changes nothing.

Known limits:

- **Restarts.** A process keeps working copies, replay logs, and the numbers it handed out in memory only; unsaved work survives only as far as its last recovery draft. After a restart, a retried request may run again and a revision may name different content than before. The web client therefore resends an edit only when the restarted service holds the same drawing, never an undo, redo, or an unconfirmed create or copy. An unconfirmed save is not asked about again there either; the page saves anew only if the restarted service already holds the drawing on screen.
- **Draft window.** Changes made after the last completed draft are lost in a crash (see Autosave and recovery): up to 10 seconds of editing without the web editor, longer while drafting fails.
- **Other writers are detected, not merged.** Saves are serialized and conflicts refused, and a clean copy picks up a newer commit on its next access. There is no live update or merge. Two API processes on one data directory can each keep a working copy of the same document; only one owns its draft, and the other's autosave returns `draft_in_use`.
- **Slow storage.** The waits above are bounded; storage I/O itself is not. Creates, autosaves, closes, recoveries, draft discards, and copies do their storage I/O while holding the process's session table, so a stalled disk (for example a hung network file system) delays other requests of the process as long as it stalls, and a client deadline passed meanwhile leaves that outcome unknown. Only the in-progress answer for a repeated `request_id` does not wait for it.
- **Replay window.** A lost reply is answered from the replay log (the latest 64 successful mutations of an open document; per process for creates, closes, deletes, and restores). Asked about after that, a request runs again; so does a save or autosave asked about after its document's session closed, since those are remembered with the session. A save whose revision has moved on then gets `revision_conflict`.
- **Blocked drafts.** While an earlier session's draft exists, new work on that document is not autosaved; the page shows this and offers the earlier draft's actions. Work left this way in a document the page moves away from stays only in the service's session until the page returns to it.
- **Advisory locks on local disks.** Locks are OS advisory locks honored by Chemifree processes; other programs that write the files are not stopped. File systems that do not implement them (some network shares) do not provide the cross-process guarantees. Lock files under `locks/` and `drafts/*.owner` are left in place by design.
- **Durability.** The rename is the commit and the directory sync is best effort, so a power loss right after a save (or a draft write) may bring back the previous file. A file is never left half-written.
- **Abandoned drafts are kept until someone decides.** Drafts never expire and are never cleaned up automatically. An abandoned draft is listed with its document (`recovery` with `state`, `revision`, `base_revision`, `saved_at_ms`) for as long as it exists, and is removed only by recovering it (it becomes the new session's draft, removed by that session's save or discarding close), recovering it as a copy, or `discard-draft`. A draft whose document file no longer exists is left in place and not listed.
- **Not implemented.** A read-only mode (opening documents read-only, or saving elsewhere when storage refuses writes; read-only storage is only reported, as `reason: "read_only"`), file locking against non-Chemifree programs, and multiple drafts per document.
- **Sizes.** The limits above (atoms, bonds, title, commands, history, 64 MiB files, revisions, 64 open sessions) are refused with an error, never truncated.
- **Write token.** It keeps other websites from writing; it is not user authentication (see below).

## Write protection

The API binds `127.0.0.1` only and never sends CORS headers.

1. Every request must carry a `Host` of `127.0.0.1:<port>` / `localhost:<port>` or the host of an allowed web origin, otherwise `403 forbidden_host` (DNS rebinding defense).
2. `GET /api/v1/session` and all writes reject a disallowed `Origin` and `Sec-Fetch-Site` other than `same-origin`/`none` with `403 forbidden_origin`.
3. Writes require the header returned by `GET /api/v1/session` (`x-chemifree-session: <token>`), otherwise `401 unauthorized`. The token is random per API process; a restarted API issues a new one.
4. Writes require `Content-Type: application/json` (`415 unsupported_media_type`) and at most 256 KiB (`413 payload_too_large`).

Allowed web origins default to `http://127.0.0.1:5173`, `http://localhost:5173`, and the same hosts on port 4173; `--allow-origin` replaces the list and accepts loopback origins only. This keeps other websites from driving the API. It is not user authentication: any local process that can reach the port can obtain the token, so it does not separate local OS users. `authentication` stays `not_implemented`.

## Errors

`{ "api_version": "v1", "code": string, "message": string, "current_revision"?: number, "command_index"?: number, "reason"?: "read_only" | "busy" | "io" }` (`reason` comes with `storage_failed`)

| HTTP | `code` |
| --- | --- |
| 400 | `invalid_json` |
| 401 | `unauthorized` |
| 403 | `forbidden_host`, `forbidden_origin` |
| 404 | `not_found` (route), `document_not_found`, `draft_not_found` |
| 405 | `method_not_allowed` |
| 409 | `revision_conflict`, `request_id_reused`, `request_in_progress`, `nothing_to_undo`, `nothing_to_redo`, `unsaved_changes`, `save_conflict`, `recovery_pending`, `draft_in_use`, `draft_outdated` |
| 413 | `payload_too_large` |
| 415 | `unsupported_media_type` |
| 422 | `invalid_request` (malformed body or field), `invalid_command` (with `command_index`) |
| 500 | `document_unreadable`, `internal_error` |
| 503 | `storage_failed`, `resource_limit` |

The CLI uses the same codes, exit status 1. `recovery_pending`, `draft_in_use`, `draft_outdated`, `draft_not_found`, the list query, rename, delete, trash, and restore (and `saved_at_ms`), `request_in_progress`, and the `reason` and `session.draft_failure` fields were added within v1; clients should treat unknown codes by their HTTP status. `request_in_progress` is an exception to read carefully: the outcome is not known yet, so it is not a failure. Ask again later with the same `request_id`. Before it existed, a retry of a running request waited for it and then got its reply. A process keeps at most 64 open sessions. When a new one is needed, a clean session is evicted (the lowest document ID; `recover-as-copy` evicts its own source only when no other session is clean), and `resource_limit` is returned only when all hold unsaved changes; nothing is changed then.

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

A change set is `{ "title": { "before", "after" } | null, "atoms": [{ "id", "before", "after" }], "bonds": [...] }`, with `null` where the entity did not exist. `undo` is oldest first; the last `redo` entry is redone next. Loading rejects unknown fields, other format versions, files over 64 MiB, dangling or duplicate references, counters that are zero or would reuse IDs, and history that does not replay cleanly (including change sets that are empty, repeat an entity, or contain no-op entries); history beyond the limits is trimmed oldest first, as editing trims it; such files are listed under `unreadable` instead of being opened. Floats round-trip bit-exactly. This is Chemifree's working format, not an interchange format; MOL/SDF/CDXML/SMILES remain `not_implemented`. Document files written before drafts existed load unchanged; the document format did not change.

A recovery draft, `<data-dir>/drafts/<id>.json`, uses the same `document` and `history` shapes:

```json
{ "format": "chemifree.draft", "format_version": 1, "base_revision": 4, "saved_at_ms": 1767225600000,
  "document": { ... }, "history": { "undo": [...], "redo": [...] } }
```

It is validated like a document file when recovered. Other files in the data directory: `trash/<id>.json` (a trashed document, same format) and `trash/<id>.deleted` (when it was trashed; ignored without the document), `locks/<id>.lock` (save lock), `drafts/<id>.owner` (draft owner lock), `drafts/<id>.committing` (a draft set aside during a save), and `.*.tmp` temporaries (in `documents/`, `drafts/`, and `trash/`) pruned after an hour.

## Compatibility and verification

Additive fields, endpoints, commands, and capability IDs may be introduced within v1; consumers ignore unknown response fields. Removing or renaming fields, changing types, or adding status values requires a reviewed version change and synchronized Rust/TypeScript updates. The browser rejects unknown versions, malformed records, duplicate IDs, and bonds that reference missing atoms.

- `cargo test --workspace`: domain invariants, transactions, undo/redo, restore validation, copies; application revisions, replays, save failure and conflict, autosave, recovery, save as copy, the service's drafting schedule (quiet period, maximum wait, retries that edits do not postpone) and drafts written without a client, at a stop, and after a failure, list search and sort, rename, delete to the trash and restore (refused for unsaved work and drafts anywhere, never undone by another process's copy), storage failure reasons, bounded waits for a document's turn, and a repeated `request_id` answered `request_in_progress` while it runs; the turn gate itself; file store atomicity, corruption, save locks between stores, draft ownership (including concurrent requests in one process), permission errors reported as denied, trash moves and restores (refused while a draft or another writer holds the document), and interrupted saves; HTTP guards and error mapping.
- `npm run test:contracts`: real API and CLI processes on a temporary data directory — HTTP = CLI JSON, write guards, the full create/edit/undo/redo/save/close/reopen flow, conflicts, save failure and retry, recovery after the API is killed; six CLI processes saving from one base at once (exactly one commits), a process killed while holding the save lock, an API save racing a CLI save; autosave, crash, and recover (content, history, IDs, revision); discard; an outdated draft recovered as a copy; an unreadable draft; a failed autosave and its retry; save as copy over HTTP and CLI, its independence, a failed copy, and a copy whose source draft cannot be removed (a permission error on `drafts/`: `closed: false`, the source keeps its changes and draft); a save lock held by another process and saves queued behind it answered `busy` within the time budget, a lost save reply asked about again (`request_in_progress`, then the committed result), and a read-only `documents/` reported as `read_only`; drafts the service writes with no client asking (edit over HTTP, no autosave call, wait for the draft, SIGKILL, restart, recover), drafting at a normal stop, a draft written within the maximum wait while edits continue, and a failed service draft that is reported and retried once storage recovers; list search and sort (HTTP = CLI), rename, delete over HTTP and the CLI, a clean API copy closing and an unsaved one unable to re-create the document after a CLI delete, deletes refused for drafts in use and abandoned, a process killed in the middle of a delete, and restore. Debug builds honor `CHEMIFREE_TEST_COMMIT_DELAY_MS`, a pause inside the save lock used to force these races.
- `npm run test:browser`: browser smoke, also run in CI (job `browser`: Playwright's Chromium on Ubuntu 24.04, no skipped steps, records uploaded whether it passes or fails); results in `verification/editor-browser-smoke.json` and `verification/browser-ci.json`.
- `npm run test:browser:routes`: reproduces, against a plain HTTP server, the Playwright route-expiry race that made the smoke's restart step flaky, and checks the smoke's guard (`--unguarded` shows the race); results in `verification/browser-route-race.json`.

None of these checks validate chemical behavior or compatibility with any other application.
