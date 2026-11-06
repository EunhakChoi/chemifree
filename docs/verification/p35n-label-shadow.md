# P35n chemical label shadow implementation record

Scope: document `label_shadow` default false; P35i's Chemifree +1,+1 pt, opacity
.35/no blur policy applies to chemical label glyphs and retained-graph Et only.
One canvas-wide group composites shadow overlap before foreground. Underline
rectangles and hollow strokes preserve their combinations; marks/captions and
chemical graph/stereo remain independent.

Actual mounted glyph/stroke/positive underline ink and translated copies feed
shared fit, trim, hit and selection boxes, including normal400/500. Existing
font/document/current-viewport readiness remains binding. Shadow duplicates have
no foreground measurement or hit/accessibility selectors.

Native v22 requires boolean shadow in every current/history settings state.
v1–21 false migration, exact byte archives, future23 refusal, old clipboard and
template omission, invalid explicit values, equality and existing export losses
are covered. Prior admission weights are unchanged; physical settings320 bytes
keep a conservative61,434,842-byte complete-file bound below64MiB.

Authored checks: client contract/queued preferences and clipboard equality;
application preferences/replay/reset/save/reopen/history; strict native current,
undo/redo, draft/archive, old/future headers; maximal v16–21 history fixture;
`npm run test:browser:label-shadow` for real fonts/style/size/outline/underline,
light/dark theme, overlapping shadow alpha/order, shared geometry, graph/Et,
UI/native/history and delayed font/document/viewport changes. npm and CI register
these checks.

Execution status: tests/build/cargo check/browser/skill gates intentionally NOT
RUN per user instruction. Formatting and diff inspection only. No scientific or
legacy rendering equivalence claim. P35 remains `in_progress`.

Required Codex and fresh independent subagent PR reviews and their dispositions:
pending parent workflow; no completed review is claimed here.
