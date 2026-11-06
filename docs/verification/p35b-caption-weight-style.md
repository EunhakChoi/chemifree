# P35b: explicit caption run weight and style

Current evidence: this record preserves implementation-session observations. Its candidate/pending wording is historical; merged PRs, passing CI, current scope and remaining P35 work are reconciled in [the completion audit](p35-completion-audit.md). No checks were rerun for that documentation audit.

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

## Independent review disposition

Independent fresh-context review of `d22ac17` identified P3: four malformed-script
client fixtures omitted the newly required weight/style fields, allowing style validation
to hide a broken script validator. Fixed by giving each array/object/unknown/missing-script
fixture valid normal weight and style, so script is its sole malformed field. Execution
and mutation verification remain assigned to the parent session; this fix ran only
`git diff --check` and does not claim passing tests.

The parent's first full check reached Clippy and failed on `collapsible_if` in the native
caption style-field scan. Fixed with the Rust 1.95 let-chain form, preserving both predicates
and the validation body. The implementation worker inspected other added conditional code,
ran formatting and `git diff --check`; the parent owns the next build/test run.

The parent's next full run passed Clippy and reached the v9 draft migration regression,
which failed because the fixture called `write_draft` directly after `claim_draft` and
expected a recovery archive. That expectation skipped the repository lifecycle contract:
`claim_draft` reads and captures bytes, while `prepare_draft_for_recovery` preserves those
bytes and verifies the saved base before recovered edits may overwrite the draft. The test
now calls preparation with the actual original base revision, asserts no archive on claim,
asserts the exact archive and unchanged draft immediately after preparation, then retains
the first rewrite/reclaim assertions. Production behavior was not changed. The worker ran
formatting and `git diff --check`; rerunning tests remains assigned to the parent.
