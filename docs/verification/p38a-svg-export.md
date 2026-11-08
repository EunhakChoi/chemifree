# P38a2 bounded SVG drawing export: verification record

Scope: `document.export` with `format: "svg"` over HTTP, CLI and operation contract 1, the 18-row loss table (16 rows since P38b1, see `p38b1-svg-scene-objects.md`) with digest consent, and the refused SVG import. The renderer core is P38a1 (`docs/verification/p38a1-svg-renderer.md`). P38 and RENDER-01 are not complete.

## Run after the last edit

- `npm run check` in the sandbox: exit 0. Cargo tests 2406 passed, 0 failed (after the PR #226 review fixes, on a purged build cache); conformance 117 cases, 117 passed (three new: `svg-export-drawing`, `svg-export-needs-consent`, `svg-import-refused`); `contract-smoke` includes the HTTP/CLI SVG export and import refusal.
- New test files: `crates/application/tests/svg_operations.rs` (7), `crates/application/tests/svg_loss_drift.rs` (one table test, 36 rows: 18 drawn, 18 not drawn), unit tests in `crates/application/src/svg/losses.rs`; updated `format_wording.rs` and `combined_interchange_operations.rs`.

## Mutations (each applied alone, without the build cache, then restored and compared byte for byte)

| Mutation | Caught by |
| --- | --- |
| `deny_unknown_fields` removed from the export parameters | `an_unknown_parameter_is_an_invalid_request` |
| accepted losses ignored by the consent check | three `svg_operations` tests; conformance `svg-export-needs-consent` |
| caption loss row never emitted | `svg_loss_drift` (caption row) |
| crossed mark on a double bond reported as a loss | `svg_loss_drift` (crossed-double row); `losses` unit test |
| SVG import answered `ok` | `importing_svg_is_unsupported_and_leaves_the_revision_unchanged`; conformance `svg-import-refused` |
| `chemical_graph` loss made consent-required | two `svg_operations` tests; conformance `svg-export-drawing` and `svg-export-needs-consent` |

## Decisions made during implementation

- A crossed mark on a double bond is drawn as two crossing lines (the P38a1 renderer), so it is not a loss; a crossed mark on another order cannot be stated through commands and is reported if it ever reaches the renderer.
- SVG is listed and executable only when at least one format codec is attached, like every other export format; the API, CLI and conformance runner attach all of them.
- The view-box margin applies to label boxes; atom points grow by the line-width reach (the P38a1 rule).

## Review fixes (PR #226)

- Codex P2 wording fix: the export note and the catalog description said "with accepted_losses consent for what it does not draw", which reads as if every undrawn item needed consent. They now say "with accepted_losses consent needed only for the losses that require it" (`crates/application/src/format_text.rs`).
- Shared renderer rules instead of copies: `crates/application/src/svg/losses.rs` calls `mark_is_drawn` and `is_zero_length` (`svg/geometry.rs`) and `SHOWN_QUERY_ELEMENTS` and `shows_label` (`svg/label.rs`), the functions the renderer itself uses.
- `label_typography` is reported only when a label style is set and at least one atom is labelled (`shows_label`). Bold on bare carbons, which draw no label, is no longer a loss.
- New `query_bond_expression` row (chemistry, no consent): the renderer draws every query bond as one dotted line and never reads `BondView::query`. The canvas draws the same dotted line and shows the expression only in the inspector.
- New `zero_length_bond` row (chemistry, no consent): neither the renderer nor the canvas strokes a bond whose two atoms share one point in a placement (`bondStrokes`, `length === 0` in `apps/web/src/editor/geometry.ts`).
- `title` is reported only for a non-empty title other than the default, as the SMILES `title` row in `ct_projection::drawing_losses` already does. The `selection` row already matched ct_projection; the per-format rows stay separate because each format's wording differs.
- The export reads the view through `documents.get` instead of `analysis_snapshot`, which also copied the document with its history. The result's snapshot is unchanged.
- The dead `Connected.svg` flag is removed (`lib.rs` always set it to `true`); the SVG listing is unchanged.

## Not run or not covered

- Browser suites and any rendering in a viewer; the label advance table is not compared with viewer fonts.
- A crossed mark on a single bond (no command can state it).
- The review-fix mutations (query-bond row, zero-length row, label_typography label guard) were each run against `svg_loss_drift` and failed it as expected; they are not in the Mutations table above.
