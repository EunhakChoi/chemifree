# P35c: whole-caption horizontal alignment

Current evidence: this record preserves implementation-session observations. Its candidate/pending wording is historical; merged PRs, passing CI, current scope and remaining P35 work are reconciled in [the completion audit](p35-completion-audit.md). No checks were rerun for that documentation audit.

Status: implemented candidate; test/build/browser execution, mutation checks and reviews are
pending. P35 remains in progress. Base: `c685460` (latest origin/main when work started).
Branch/worktree: `feat/p35c-caption-alignment`, isolated `chemifree-p35c`.

The independent single-line caption records physical `left`, `center`, or `right` alignment
at a fixed baseline anchor. This composes with every existing script/weight/style run.
Alignment-only replacement is one undoable edit; identical state is a no-op. Text, run
boundaries, chemical graph, Et display metadata and existing export-loss behavior are kept.
No paragraph layout, per-run font/color, atom-label styling or external caption roundtrip is added.

The renderer measures the complete natural advance, including leading/trailing whitespace,
using the real SVG text after rendering and font loading. It shifts the entire text's x before
measuring its ink/hit/view-box bounds. Per-run y positions make SVG text chunks: using centered
or end `text-anchor` would anchor each chunk independently and overlap/separate the runs.
All chunks keep `text-anchor="start"`; their shared origin receives the whole-line offset.
Left/right are physical and paragraph direction is explicitly LTR regardless of surrounding
page direction. Browser-native bidi/shaping within runs remains, but full RTL paragraph and
cross-run bidi editing are unsupported. CONTRACT.md records this policy and typography limits.

Native v11 requires alignment in current/undo/redo captions. V8–10 migrate every boundary to
left without a read-time rewrite; old headers with alignment and v11 missing/null/unknown or
wrongly typed alignment are refused. Existing first-overwrite document and pre-recovery draft
archives now include v10 originals, byte for byte. V10 run-style requirements and v9 Et maps
continue unchanged. Existing conservative caption metadata reservations cover the bounded enum.

HTTP and CLI share application DTOs. Add/update request omission defaults to left; update is
a complete replacement, so an older client editing a centered/right caption resets it to left.
Snapshots always include alignment. Current web clients require it; older clients can ignore
it and render left aligned. This additive v1 compatibility limit is explicit in CONTRACT.md.
The production dialog exposes Left/Center/Right and captures alignment with runs and anchor
for the existing queued-edit stale-target and recovery rules. CLI text output includes alignment.
The controls reuse the existing shadcn Base UI ToggleGroup/Field components; official guidance:
https://ui.shadcn.com/docs/components/base/toggle-group.

Written regression paths (not executed in this implementation task):

- Domain whole-state replacement checks fixed anchor, exact whitespace/Unicode, script/bold/italic,
  no-op history, Undo/Redo, and unchanged empty graph/Et metadata.
- Application covers center add, right update, replay, save/reopen, abandoned draft recovery,
  Undo/Redo, omitted add/update defaults, malformed values, atomic refusal and changed replay.
  Existing caption-loss exports run with a centered caption and retain their consent boundary.
- Provider migration checks v8/v9/v10 document and draft bytes, old history boundaries and
  archives; strict cases cover current and redo-only captions in documents/drafts. Existing
  style/Et fixtures strip alignment when constructing real old schemas. Maximal caption
  persistence tests retain their byte/weight bounds with the new field.
- HTTP=CLI smoke covers right alignment through replay/move/history/save/reopen, rejects bad
  alignment, then center and omitted-update left. CLI text rendering checks explicit center.
- Client validators accept each alignment and refuse absent/null/unknown/wrong-type fields.
- Real browser harness edits all three alignments through the production dialog with baseline,
  subscript, superscript, bold and italic runs plus edge spaces. Every run's actual SVG start
  must shift by the same complete advance, preventing per-chunk alignment and ink-width errors.
  It checks fixed anchors/runs, one Undo/Redo step and right alignment in native save/reload.
  Existing cold-font, document-switch and view-box tests use aligned captions, including mixed
  Hebrew/Arabic/Latin text in the alternate drawing, and retain real font responses.

Rust/TypeScript design and testing skills and the shadcn skill guided implementation.
Only `cargo fmt --all` was executed here. Parent integration owns compilation, tests, lint,
real browser execution, mutation evidence, Codex review and the fresh independent review;
none is claimed as passing by this candidate document.
