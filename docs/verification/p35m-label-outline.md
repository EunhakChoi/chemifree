# P35m chemical label outline

Status: implementation and checks authored; execution deferred by task instruction.
P35 remains in_progress. No scientific compatibility claim.

- Default-false document boolean flows through full edits, preferences, reset,
  HTTP/CLI/replay, undo/redo, save/copy and recovery.
- Only chemical symbol/query/H/count/isotope/charge/Et text gains hollow glyph
  strokes: effective size/16, round joins. Underline rectangles remain filled;
  captions and radical/attachment marks retain independent styles.
- Actual mounted SVG metrics are required even for normal 400/500 text. Nonzero
  glyph ink gains effective size/32 stroke radius. Source metric keys include
  outline and retain exact absolute shifts without dy reconstruction. Existing
  document/font/current viewport provenance and all-chunk readiness gates remain.
- Native v21 requires boolean outline in every current/before/after settings
  state. v1–20 omit it and migrate false with exact document/draft archival.
  Future v22 is refused untouched. Clipboard/template omission defaults false;
  explicit invalid values refuse and exact style matching includes outline.
- Existing history admission reservations and 64 MiB read boundary remain;
  maximum metadata caption-history migration now covers v16–20.

Authored checks: strict native outline format/migration/archive/recovery tests,
application preferences/reset/replay/persistence and clipboard/template tests,
HTTP/CLI contract smoke, client settings/old graphics payload tests, and
`npm run test:browser:label-outline` registered in CI. The browser matrix covers
3 font families × 4/72pt × normal/bold × normal/italic × underline off/on;
computed hollow strokes and actual stroke ink, shared fit/trim/hit/selection,
graph/Et invariance, captions and marks, Unicode/space/RTL/ligature probes,
production UI/native history, delayed real fonts and document-switch viewBox
provenance. Full settings fixtures and current/future native versions updated.

Executed: `cargo fmt --all` and `git diff --check` only. Builds, tests, cargo
check, clippy/typecheck and browsers were not run. No mutation proof is claimed.
PR reviews and dispositions remain with the parent delivery workflow.

## CI underline failure diagnostic follow-up

The supplied GitHub log reports the existing underline matrix failing at italic,
bold, monospace, 4 pt: zero rectangles for a one-run chemical label. The same
matrix passed the parent's isolated local run. The log does not identify the
label, source key, live settings, sampled readiness or SVG character positions;
it cannot establish a product cause or justify changing the geometry policy.

Static inspection identifies two distinguishable possibilities: the harness
previously synchronized only font/size rather than the complete requested style
and idle revision; and a found chunk metric counts as measured even when SVG
character positions gave no positive advance when sampled. The latter legitimately
produces no underline for empty/zero-advance input, so the missing label and
position evidence matters. Neither possibility is claimed as the reproduced cause.

Both underline and outline matrices now wait for the new revision, full requested
settings, idle editor and measured/current viewport agreement. Their single
geometry sample captures and asserts those preconditions atomically, including
font status. Strict rectangle counts, geometry tolerances and all other behavior
assertions remain. There is no wait for the expected rectangle count and no
assertion retry. Count failures now include the failing label, complete settings,
revision, font/readiness/viewBox provenance, exact source metric key, computed
fonts, per-character positions and fresh browser measurements. Fresh measurements
are diagnostic only; assertions still inspect the production rectangles/bounds.
The diagnostic module is loaded before the matrix, with no async gap inside a
sample. No production behavior was changed without evidence.

Validation of this follow-up: source review and `git diff --check` only. Tests,
builds and browser execution remain delegated to the parent by task instruction.
