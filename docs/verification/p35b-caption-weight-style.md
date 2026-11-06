# P35b: explicit caption run weight and style

Status: implemented candidate; execution verification and review pending. P35 remains in progress.
Base: `ba95baa`; branch: `feat/p35b-caption-font` in isolated `chemifree-p35b`.

Each independent caption run records normal/bold weight and normal/italic style, independently
of baseline/subscript/superscript. No font family/size/color editor, atom label styling,
abbreviation interpretation, chemical graph change, wrapping or caption clipboard is added.
Existing explicit whole-run add/update operations, IME gates and stale-target rules apply.
SVG tspans set both font attributes; the existing actual-ink/font-loading measurement follows
each document snapshot, including style-only edits. Font fallback/synthetic styling is not
portable visual roundtrip.

Native v10 writes both fields on current and historical caption states. V8–9 migrate to normal
without writes. V1–9 refuse either field even in history-only captions; v10 refuses missing,
null, wrongly typed or unknown style values. Existing exact-byte migration archives now also
cover v9 documents and drafts. API request omission remains compatible (normal defaults),
while responses include both fields. The existing 96-byte per-run metadata reservation covers
the new bounded enums; no persistence limits were relaxed.

Written regression paths:

- Domain caption tests exercise every script with bold, italic and combined styling, one
  undoable replacement, redo, exact text, and unchanged graph/abbreviation state.
- Application caption lifecycle now carries mixed styled scripts through replay, save/reopen,
  abandoned-draft recovery, Undo/Redo and every consent-required chemical export path.
  Malformed style and old request omission tests pin strict refusal and normal defaults.
- Native format tests exercise v9 current/undo/redo migration, exact source archives, old
  draft read/first rewrite, and forbidden/missing/null/history-only style fields. Existing
  maximal-caption save/draft/history reservation tests also serialize the new fields.
- CLI text tests show explicit script/weight/style while preserving terminal escaping.
  `scripts/contract-smoke.mjs` sends styled runs through both HTTP and CLI, then replay,
  malformed-field rejection, movement, Undo/Redo and save/reopen.
- Client response checks reject malformed/newly missing style fields. The real-backend
  caption browser harness selects weight/style in production controls, asserts native run
  states and SVG attributes, exercises a style-only edit with updated ink bounds and Undo,
  then uses existing actual glyph-bound, save and recovery checks.

Only `cargo fmt --all` and static diff review were performed by the implementation worker.
No build, tests, browser smoke, mutation proof, push or PR was executed by that worker.
The parent session must append actual checks, review dispositions and CI evidence.
