# P38a2 bounded SVG drawing export: verification record

Scope: `document.export` with `format: "svg"` over HTTP, CLI and operation contract 1, the 16-row loss table with digest consent, and the refused SVG import. The renderer core is P38a1 (`docs/verification/p38a1-svg-renderer.md`). P38 and RENDER-01 are not complete.

## Run after the last edit

- `npm run check` in the sandbox: exit 0. Cargo tests 2397 passed, 0 failed; conformance 117 cases, 117 passed (three new: `svg-export-drawing`, `svg-export-needs-consent`, `svg-import-refused`); `contract-smoke` includes the HTTP/CLI SVG export and import refusal.
- New test files: `crates/application/tests/svg_operations.rs` (7), `crates/application/tests/svg_loss_drift.rs` (one table test, 33 rows: 15 drawn, 18 not drawn), unit tests in `crates/application/src/svg/losses.rs`; updated `format_wording.rs` and `combined_interchange_operations.rs`.

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

## Not run or not covered

- Browser suites and any rendering in a viewer; the label advance table is not compared with viewer fonts.
- A crossed mark on a single bond (no command can state it).
- The catalog sentence "with accepted_losses consent for what it does not draw" reads as if every undrawn item needed consent; README and CONTRACT state that only consent-marked losses do. Rewording the catalog text is a follow-up.
