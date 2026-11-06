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

Independent review disposition (P2, caption persistence boundary): fixed. Raising run
metadata from 96 to 128 bytes changes each 32-run maximal caption's complete before/after
update reservation to 74,304 bytes. The empty-selection entry overhead is 576 bytes, so
112 updates reserve 8,322,624 bytes (accepted) and 113 reserve 8,396,928 bytes (refused)
against the unchanged 8,388,608-byte history cap. The domain boundary regression now
checks those adjacent counts and retains its rollback, restore and Undo/Redo checks.

Static comparison found another affected accepted fixture: the maximal storage state has
7,000 selected entities in each selection, adding 224,576 bytes of entry overhead. Its
move now uses 109 captions (8,323,712 bytes total); 110 would reserve 8,398,016 bytes and
exceed the cap. Other budget assertions remain valid: 100-caption repeated updates reserve
7,430,976 bytes per entry (one fits, two do not), 500 maximal creations still exceed the
cap, the 117-caption application group move still exceeds the cap, and equal UTF-8 byte
lengths still receive equal text reservations. These are static calculations, not executed
test results. No tests or builds were run while addressing this finding; parent isolated
validation remains responsible for execution evidence.

Parent isolated validation found a stale future-version fixture in provider migration:
it still labeled a v4-shaped file as v13, which is now supported, so schema validation
correctly rejected its missing abbreviation fields before the expected future-version
diagnostic. The fixture now uses v14 and expects supported versions 1–13. A repository
search found the same stale fixture and diagnostic assertion in the HTTP/CLI contract
smoke; both were updated. The provider's separate version-99 refusal fixture remains
future and needs no change. No tests or builds were run for this fix; parent isolation
owns the rerun and its result.

The next parent clean run exposed an additional short HTTP diagnostic assertion in the
same future-version smoke block (`/version 13/`), missed by the previous targeted
replacement. It now expects `/version 14/`, matching the fixture and the adjacent full
diagnostic assertion. A static search across script version assertions and provider
future-version fixtures confirmed that remaining v13 assertions refer to current writes,
while unsupported fixtures use v14 or v99. A stale current-save explanation was also
updated from v12 to v13. No tests or builds were run for this correction.

Codex review P2 (#190, numeric text grammar): fixed. The point-size form previously
used Number directly, accepting hexadecimal/binary/octal strings such as 0x10, 0b100 and
0o10. A dedicated parser now accepts trimmed decimal/scientific notation only (optional
sign, decimal point and exponent), then enforces finite 4–72 pt bounds. Blank stays null
inheritance. The form submits the parsed value it validated instead of reinterpreting text.
Written parser cases cover radix rejection, fractional decimals, boundary-adjacent values,
both inclusive boundaries, signs/whitespace/exponents and malformed/nonfinite input. The
production browser form adds the three radix refusals and valid fractional/boundary inputs.
No tests or builds were run for this review fix; parent isolation owns their execution.
