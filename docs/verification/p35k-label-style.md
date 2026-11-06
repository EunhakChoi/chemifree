# P35k chemical label style

Status: P35 remains in_progress. Based on eec877b (#195).

`label_style` is a document appearance enum (normal/italic), default normal.
Chemical 400/500 and bold 700 runs retain weight while acquiring italic. Marks
and independent captions retain their own styles. Actual mounted SVG metrics
include style, effective size/family/weight, separate left/right ink overhang and
vertical ink. One measured layout drives fit, trimming, picking and selection;
font loading, switched document identity and actual viewport remain provenance.

Native v19 requires style in every current/history settings state. v1–18 refuse
smuggled style and migrate to normal. v18 weight remains strict. v20 is refused
untouched. Exact migration archives and recovery, prior complete caption history
allowances and the physical 64 MiB bound remain unchanged. Clipboard/template
omissions mean normal and mismatch/invalid explicit values remain refused.
External exports report the existing drawing_settings rendering loss.

Authored checks:

- Domain settings: graph/scene/stereo invariance, full-settings overlays, undo,
  redo and repeated no-op.
- Application settings: preferences copied once, reset/replay, persistence and
  reopening; native clipboard omissions/mismatch; Et graph inspections and all
  supported export payloads and loss reports.
- Provider label_style_format: current/history document/draft missing/null/type/
  enum refusal, old headers, exact-byte archives and undo/redo recovery.
- HTTP settings transaction/reset serialization includes italic then normal.
- `node scripts/web-label-style-check.mjs`: actual SVG 400/500 and 700 italic
  across sans-serif/serif/monospace at 4/72pt, isotope/query/H/count/charge/Et,
  untouched marks/caption, glyph containment, natural widths, fit, trim, label
  edge picking/selection, style UI, undo/redo, save/reopen and delayed font
  completion after document switching while preserving viewport.

Validation executed: `cargo fmt --all`, `git diff --check` only.
Tests, builds, cargo check/clippy, frontend type/lint and browser runs were not
executed, as explicitly requested. Runtime correctness remains unverified.
Rust/TypeScript patterns, testing and existing shadcn composition guided changes.
No new component or dependency was introduced.

PR Codex and fresh-context independent reviews must be recorded by the delivery
workflow; no review or execution pass is claimed by this implementation record.

## Follow-up: shared client fixtures

The parent's first isolated `npm run check` log reported 69 client failures.
Static inspection traced 68 failures to the shared `sceneCutSource` complete
settings fixture missing `label_style`: caption, ring, abbreviation and template
fixtures derive from it, and the old-weight clipboard assertion compared its
incomplete source settings to a normalized payload. The remaining failure was
the expected `labelStyleOf` object missing `style: normal`. This evidence calls
for fixture updates; it does not establish an old clipboard decoder defect.

Current HTTP response fixtures and browser complete-settings fixtures now state
normal style explicitly. Complete API settings validation stays strict. A new
client suite explicitly deletes style (and both weight/style for older payloads),
checks normal inheritance, italic destination mismatch and invalid explicit style
refusal. It also checks style edits/preferences, enum validation and unchanged
400/500/700 weights and marks. The client runner imports this suite. No test or
build was rerun by the implementation agent; only `git diff --check` was run.
