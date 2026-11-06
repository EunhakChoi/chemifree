# P35e: caption run size

Status: implementation candidate; P35 remains `in_progress`. Base: `6bc2a15` (#189).
Branch/worktree: `feat/p35e-caption-run-size`, isolated `chemifree-p35e`.

Each independent caption run has `size_pt: number | null`. Null or request omission
inherits the document's current `label_size_pt` dynamically; explicit overrides use the
existing `LABEL_SIZE_RANGE`, finite 4–72 pt inclusive. Snapshots always emit the nullable
field. Whole-run replacement resets an omitted override to inheritance. Size-only edits
participate in the existing atomic transaction, replay comparison and Undo/Redo.
Wrong types are `invalid_request`; typed out-of-range values are `invalid_command` with
`caption_invalid` and `command_index`. Neither refusal partially commits a transaction.

The production shadcn Input/Field form keeps numeric text while it is being edited. Blank
means drawing size; invalid text disables submission and exposes invalid state accessibly.
The SVG uses each run's effective size: baseline at full size, sub/sup at two thirds, with
y offsets +one third and −five twelfths of that same effective size. Existing actual SVG
advance and ink measurement continue to drive whole-line alignment, hit bounds and fit.
Browser fallback, shaping and synthetic styles retain their existing limitations.
Atom/Et labels, color, wrapping, external caption mapping and clipboard are outside this slice.

Native document/draft v13 requires nullable size_pt on every current/history run, including
redo-only captions. A field under v1–12 headers is refused even when null or history-only.
V8–12 migrate all boundaries to inherited size in memory; reading writes nothing, and the
existing first-overwrite/recovery archive keeps exact source bytes, including v12 files.
Prior font, alignment, style and Et gates remain in force. The run metadata reservation
increases from 96 to 128 bytes: the previous conservative 86-byte maximum plus a 30-byte
upper bound for the size key, comma and bounded f64 spelling fits 116 bytes. The caption
256-byte overhead, content/history budgets (48/8 MiB) and native 64 MiB limit stay unchanged.

Written regression coverage (execution pending):

- Domain finite/boundary validation, complete rollback, size-only undo/redo, inherited and
  explicit metadata remaining distinct after document style changes.
- Application size-only replay/reused request IDs, omission reset, invalid number/type
  rollback and undo; HTTP/CLI shared smoke now combines sizes, scripts, styles and families.
- Native current/history document/draft size matrix for v8–13; exact-byte v8–12 migration
  and archive; mixed inherited/explicit sizes across save/reopen, crash recovery and undo.
- Existing maximal-state and repeated-update persistence checks use a long f64 override;
  native version assertions throughout the storage and browser checks now expect v13.
- Web reply validation accepts omitted/null/bounded sizes and rejects malformed sizes.
  Production browser harness uses mixed sizes/families/styles/scripts, rejects invalid input,
  checks dynamic inheritance and fixed overrides after drawing-size edits, reset/undo,
  actual script sizes/offsets, measured alignment/ink and native save/reload.
- CLI text prints `inherit` or an explicit `Npt` per run, without changing stored text.

Rust/TypeScript design/testing and shadcn skills guided the change. Official Input and Field
documentation was read after `npx shadcn@latest docs input field`:
[Input](https://ui.shadcn.com/docs/components/base/input),
[Field](https://ui.shadcn.com/docs/components/base/field). No dependencies were added.

Implementation-agent execution record:

- `cargo check --workspace --offline` ran once in the host worktree before the regression
  additions, without isolation or explicit CPU/memory/time limits, and exited 0. This was
  a compilation check only; it did not execute tests and does not validate the final tree.
- `cargo fmt --all` and `git diff --check` completed successfully.
- No tests, production builds, browser checks or mutation checks were executed by the
  implementation agent. Parent integration owns isolated, resource-bounded validation and
  both repository reviews; no passing outcome for those checks is claimed here.
