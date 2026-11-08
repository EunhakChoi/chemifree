# P38b1 SVG export of scene graphics and PNG images: verification record

Scope: `document.export` with `format: "svg"` now draws scene graphics (line, rectangle, ellipse, arrow, polygon, cubic spline, with the per-graphic line width) and PNG images (the stored bytes) in scene order, adds their ink to the view box, no longer reports them as losses (loss table 18 to 16 rows), and changes the model id to `chemifree-svg-2`. The web `SceneGraphic` geometry moved into `apps/web/src/editor/graphic-marks.ts` as a behaviour-preserving refactor so Rust and the web can be compared. Captions stay a consent loss. P38 stays in progress and RENDER-01 partial; this is a bounded drawing export, not general vector output.

## Run after the last edit

- `npm run check` in the sandbox (one run, one build cache): exit 0. Cargo tests 2429 passed, 0 failed (2406 before, 23 new), 2 skipped; client tests 762 passed, 0 failed; conformance 118 cases, 118 passed (one new: `svg-export-scene-objects`); `npm run test:traceability`: ok.
- Browser (Chrome, sandbox, each exit 0): `graphic-line-width`, `graphic-ellipse`, `graphic-spline`, `graphic-polygon`, `graphic-creation`, `graphic-points`, `embedded-image`, run after the full check. They exercise `SceneGraphic` after the extraction. The label-shadow and caption browser checks were not part of this run.
- New and changed tests: `svg/scene.rs` unit tests (9), `svg/writer.rs` unit tests (5), `svg.rs` unit tests (2), `tests/svg_render.rs` (6 new tests, 3 new goldens `scene-graphics.svg`, `scene-image.svg`, `graphics-only.svg`; the 9 older goldens are byte-identical and only `parity.json` gained the `objects` field), `tests/svg_loss_drift.rs` (42 rows: 26 drawn, 16 not drawn), `tests/svg_operations.rs` (`a_graphic_and_image_drawing_exports_without_consent`), `tests/format_wording.rs` (7 expectations), `scripts/svg-parity-client-check.mjs` (25 tests: three new cases, the paint list and the graphic and image numbers compared with the web at 1e-6), conformance case `svg-export-scene-objects`.

## Review fixes after the independent review

Applied before the final run: `geometry::distance` reused for the arrow head (bit-identical), `graphic_marks` total with test `a_graphic_with_the_wrong_point_count_draws_nothing`, image ink from rounded values with test `an_off_grid_image_edge_lies_inside_the_view_box`, one base64 copy and indexed lookups in the writer with test `two_images_keep_their_own_base64`, React keys in `SceneGraphic`; and, from the Codex review, a graphic's ink measured from the marks and stroke width as the writer rounds them (`scene::rounded`, test `an_off_grid_graphic_edge_lies_inside_the_view_box`). The goldens did not change. The graphic browser checks ran again after the fixes and passed. Each new test was shown to fail under its mutation (polygon guard widened; image arm back to unrounded values), then the file was restored and compared byte for byte.

## Mutations (each applied alone, then restored and compared byte for byte)

| Mutation | Caught by |
| --- | --- |
| arrow head perpendicular sign flipped, head not guarded for zero length, head length changed | `svg/scene.rs` arrow tests (two directions and two shaft lengths) |
| rectangle `min`/`abs` removed, spline points reordered, polygon closing vertex added | `svg/scene.rs` shape tests |
| `width/2` growth dropped, head left out of the ink, explicit width ignored | `svg/scene.rs` ink and width tests |
| group fill, polygon fill, arrow head fill, `Q` for `C` in the spline path | `writer` markup test |
| base64 re-encoded or its padding dropped, `href` for `xlink:href`, `preserveAspectRatio` dropped | `writer` image test |
| `xmlns:xlink` unconditional or never written, stroke width not rounded | `writer` root and width tests |
| instances sorted before graphics, graphics with a parent skipped, document width for all graphics | `svg_render` order, grouping and width tests |
| view box without graphic ink, without `w/2`, empty for an atom-less document | `svg_render` view-box tests |
| one base64 character changed | `svg_render` `an_image_carries_the_stored_png_bytes` |
| a `graphic` loss row re-added (consent mismatch), an ellipse removed from the layout, a group adding ink | `svg_loss_drift` |
| `SVG_MODEL` back to `chemifree-svg-1` | `svg_operations`; conformance `svg-export-scene-objects` |
| arrow heads dropped from the writer | conformance `svg-export-scene-objects` |
| web head length 6 to 5, ellipse centre changed, paint order reversed, image origin shifted | `svg-parity-client-check` |
| old sentence restored in `format_text.rs` | all seven `format_wording` variants |

## Decisions made during implementation

- Port, not share: the exporter is Rust and the canvas TypeScript. The geometry is ported to `svg/scene.rs`; the canvas rules are extracted to `graphic-marks.ts` (the canvas renders from it, with byte-identical markup checked by server-rendering twelve graphics and six selection states against the previous file), and the parity script compares both against the Rust layout dump.
- An atom-less document with a graphic or image now has a view box covering that ink. Its outcome is `ok` only when the loss report is empty; the earlier plan text "ok whenever no consent loss" was wrong, because the outcome is `partial` for any loss, including title, selection, references and groups.
- The drift table's `group` row stays "not drawn": a group has no ink. Its members are now captions, because graphics already change the content and would hide a group that draws.
- The conformance runner now registers the PNG decoder like `apps/api` and `apps/cli` already do, so a case can add an image. No earlier case uses `add_image`; the 117 earlier results are unchanged.
- `xlink:href` is written because the root declares SVG 1.1; the namespace is declared only when an image is drawn, so drawings without images are unchanged.

## Not run or not covered

- Rendering in a viewer: the output is compared as text, geometry and structure, not as pixels. Visual correctness of arrows, ellipses and images in viewers and the handling of `xlink:href` by SVG-2-only tools are not checked.
- A graphic's drawn edge can lie less than 0.01 pt outside the view box for a stroke width or point off the 0.01 grid, because the view box uses the unrounded width and the markup the rounded one; no fixture hits it. An image rectangle and a graphic's marks and stroke width are measured as the writer rounds them, so they lie inside (`an_off_grid_image_edge_lies_inside_the_view_box`, `an_off_grid_graphic_edge_lies_inside_the_view_box`). A size below 0.005 pt rounds to 0 in the output.
- An instance missing from `scene.objects` (painted first, as in P38a) is not in any fixture of the parity script, and the web never draws such an instance.
- Very large replies (images near the document admission limit) were not exercised over HTTP or CLI; the exporter adds no limit and no refusal.
- The label-shadow, caption and other browser suites did not run locally for this change; CI runs them.
- A graphic whose point count its shape does not admit draws nothing (`graphic_marks` is total); the domain rejects such a graphic, so no document reaches it. The web `graphicMarks` has no such length check, as the canvas before the extraction.
