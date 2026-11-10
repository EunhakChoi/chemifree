# P41 lock placement (T0, third pull request)

Status: bounded. Replies are unchanged; only where work runs changes. Issue #106.

## What changed

- `Document::without_history` (domain): the document at the same identifier and revision with its content, selection and counters, without undo and redo. `analysis_snapshot` (chem.properties, chem.stereo, MOL/SDF export) and the `save_as` source copy use it, so the session table no longer copies up to 100 undo and 100 redo entries. The view still comes from `respond(session)` in the same hold.
- `write_draft`: the recovery lookup runs with the document's turn held and without the session table; the result and `writing()` are published together in the later table hold.
- `FileDocumentRepository::save` records the committed file's stamp and revision in the stored-revision cache while it holds the save lock, so the first clean-copy check after a save is a stat. A wrong entry can only delay a refresh; a save checks its base from the file.

## Left under the table (documented in CONTRACT "Slow storage" and ARCHITECTURE)

Session refresh read, first-open load, closed-document graph reads, the per-response draft lookup, creates/closes/copies. Moving them needs a generation, an epoch per document or a slot reservation: a design-first change. `chem.validate` stays under the lock (its copy is as linear as its computation). The document list's `saved_at_ms` already runs after the table block.

## Tests (new files)

- `crates/domain/tests/history_free_copy.rs`: identity, revision, content, selection and counters kept, history dropped, the source untouched, a history-free document equals its copy.
- `crates/application/tests/analysis_history_free.rs`: with both stacks non-empty, the properties, stereo and MOL export replies state the session's revision and depths; the save-as copy starts at depth 0.
- `crates/application/tests/draft_lookup_outside_table.rs`: a one-shot gate stalls the draft status lookup; other documents, edits, creates, lists and this document's own reads are answered while it is stalled; an edit made during the lookup is the one drafted; a response during the write reports the lookup's result. Latches, no wall-clock windows (30 s receive timeouts are only hang guards).
- `crates/providers/tests/stored_revision_own_commit.rs`: an own commit is answered from the recorded stamp (file rewritten in place with its length and mtime kept), a store that never saved reads the corrupt file (control), another writer's commit is read.

## Mutations (private copies)

Killed: `without_history` as `clone` or via `copy_as`, revision reset, selection or each counter dropped; the analysis view built from the copy; the lookup back under the table (hang guard); the snapshot taken before the lookup; `recovery_at_write` None; the recovery stored in a separate later hold; the cache insert removed; the stamp without the inode.
Survivors: an empty document in place of the copy survives the stereo and MOL export cases for a drawing without stereo (the instance list and the MOL blocks come from the session view; the copy feeds only the stereo losses and the group check) and is killed by the properties case; a recovery published in a separate hold before the write has no repository call in the gap, so no gate can stall it; a stamp taken before the write is killed by the own-commit case only (a pre-commit stamp can never equal the committed file).

## Not covered

- Time of the remaining under-table I/O is not measured; no latency is promised.
- The own-commit test for another writer's same-length, same-mtime commit is unix-only (inode stamps).
