# P35d: caption run font family

Status: implemented candidate. P35 remains in progress. Base: `5d4e8bb`.
Branch/worktree: `feat/p35d-caption-font-family`, isolated `chemifree-p35d`.
Tests, builds, browser checks, mutation evidence and reviews await parent integration;
none is claimed as passing in this implementation record.

A complete independent caption run stores `font: null | "sans-serif" | "serif" | "monospace"`.
Null inherits the current drawing setting dynamically. Request omission is also inheritance;
snapshots emit null explicitly, while the web validator accepts older omitted replies.
Updates replace complete runs: an older client can reset a new override to inheritance.
Font-only changes use existing atomic edits, history and replay comparison. No font names,
size/color, chemical atom/Et labels, external caption mapping or new typography engine is added.

The existing shadcn Base UI ToggleGroup offers Drawing font/Sans-serif/Serif/Monospace
per run; the script/weight/style inputs and queued target checks stay unchanged.
Official documentation was fetched after `npx shadcn@latest docs toggle-group`:
https://ui.shadcn.com/docs/components/base/toggle-group.
The SVG tspan selects a family per run, retaining the existing theme sans stack and browser
generic serif/monospace stacks. The complete actual SVG advance and ink still drive shared
whole-line alignment, view-box/hit bounds and initial fit after fonts load. Existing browser
checks now compose three font families with scripts, bold/italic, whitespace and alignment.
Fallback, synthetic styles and complex shaping remain browser dependent.

Native v12 requires font on every current/undo/redo caption run, including null inheritance.
A font under v1–11 headers is refused. V8–11 migrate every boundary to inheritance without
writing on reads; v11 joins the existing document first-overwrite/draft pre-recovery exact
byte archive. Existing strict weight/style/alignment and abbreviation gates remain.
The 96-byte run reservation covers the new bounded metadata (at most 86 bytes), keeping
caption content/history/file limits unchanged.

Written checks include:

- Domain family-only replacement through every family and reset, exact mixed runs/anchor,
  no-op/history, undo/redo and unchanged empty graph/Et state.
- Application mixed-family save/reopen/crash recovery and existing export-loss tests;
  family-only replay/reused request ID, invalid family/type rollback and omission reset.
- Native v8–11 document/draft migration and archive bytes, current/history-only strict
  font matrices for v8–12, old field prohibition, missing/null/valid/invalid font values.
- HTTP=CLI mixed-family snapshots, replay, move, undo/redo and save/reopen; CLI text
  includes an explicit `inherit` or generic family for every run.
- Web validators accept the closed vocabulary and older omitted fields; real browser
  harness creates all three families through production controls and checks actual computed
  family, dynamic inheritance after a drawing-font edit, fixed explicit families, Undo,
  mixed-script alignment/bounds and native save/reload.

Rust/TypeScript design/testing and shadcn skills guided this implementation. No dependencies
were added. `cargo fmt --all` and `git diff --check` ran successfully. Parent integration owns required checks and the two repository reviews.
