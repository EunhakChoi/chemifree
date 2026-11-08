# P38a1 SVG renderer core

Status: P38a1 renderer core only; no export path yet. P38 is `in_progress` and is not complete.

## Scope

- `crates/application/src/svg.rs` with its submodules `svg/geometry.rs`, `svg/label.rs`, `svg/font.rs` and `svg/writer.rs`, exported as `pub mod svg` from `crates/application/src/lib.rs`.
- `render_drawing(&DocumentView)` returns the SVG text and its view box. `layout_drawing(&DocumentView)` returns the layout data that the parity fixture stores.
- No operation, catalog entry, CLI command, HTTP route or CONTRACT entry exports a drawing. Export is P38a2.

## Output

- One `<svg>` element with `width`, `height` and `viewBox` in pt. The view box covers the drawn atoms, the reach of bond ink and the label boxes with a 4 pt margin, with each edge moved outward to the hundredth.
- Bond strokes in one group and text in a second group, one element per line, `\n` line endings and a trailing newline.
- No ids, classes or inline styles. The same document gives the same bytes on every call and after save and reopen.

## Drawn

- Bonds: single, double and triple as parallel lines; crossed double bonds as two lines that cross at the midpoint; aromatic bonds as a solid line with an inner dashed line; query bonds as dotted lines; dative bonds as a line with an arrowhead; unknown bonds as a zigzag polyline.
- Stored stereo marks on single bonds: wedge (filled polygon), hash (short strokes) and wavy (quadratic wave). A wedge stops where it meets a label box.
- Labels: element symbols with hydrogen counts, charges (the sign, and the magnitude when above one), isotope mass numbers, unknown hydrogens as `?`, and query atoms as their canvas text such as `[N,O,S,…]` and `![N,O]`. Hydrogens of a leaning-left atom come before the symbol.
- Bonds stop at the label box of each labelled end, plus the label gap and half the line width.
- Instances are drawn in the order of `scene.objects`, the order `structure-canvas.tsx` draws them.
- A document without an instance draws nothing. A document without atoms has the empty view box at the origin.

## Not drawn

- Radical dots, attachment points, captions, graphics, images and groups.
- Abbreviation and fragment displays. The web derives them from the document at display time (`compactDisplaysIn`). The SVG draws the stored atoms and bonds, and no golden case contains a compact display.
- Stereo perception. No descriptor is computed; only the stored marks are drawn.
- Bold, italic, underline, outline and shadow label styles.
- Label widths measured by a browser (see Decisions).

## Parity with the web

- `crates/application/tests/fixtures/svg/parity.json` holds the Rust layout of the nine golden documents. The Rust test `the_parity_fixture_is_the_rust_layout_of_the_golden_documents` checks it against `layout_drawing`.
- `scripts/svg-parity-client-check.mjs`, appended to `test:client`, lays out the same documents with the web's `geometry.ts`, `label.ts` and `placement.ts` in canvas drawing order, without browser metrics, and compares the result with the fixture. It only reads the fixture.
- Tolerances: texts, font weights, shapes, patterns, `halfWaves`, counts and ids exactly; atom positions and label numbers within 1e-6; bond stroke coordinates within 0.011. Both sides round stroke coordinates to the hundredth, and a tie can round one step apart, so 0.011 is one rounding step.
- Not compared: the view box, browser-measured label widths, and abbreviation and fragment displays (the check asserts that no parity case has one).

## Verification

Pre-mutation pass (sandbox, `--cache`). The later edit to the `svg.rs` header is a comment only, and the full gate covers the final tree:

| Check | Command | Result |
| --- | --- | --- |
| Unit tests | `cargo test -p chemifree-application --locked --lib svg::` | 28 passed |
| Golden and structure tests | `cargo test -p chemifree-application --locked --test svg_render` | 8 passed |
| Parity | `node scripts/svg-parity-client-check.mjs` | 10 passed |
| Lint | `cargo clippy --workspace --all-targets --locked -- -D warnings` | clean |

Unit tests by module: `svg.rs` 1, `svg/geometry.rs` 9, `svg/label.rs` 9, `svg/font.rs` 5, `svg/writer.rs` 4.

Golden and structure tests (`tests/svg_render.rs`):

- `goldens_match_the_drawing_byte_for_byte`
- `the_parity_fixture_is_the_rust_layout_of_the_golden_documents`
- `every_golden_is_well_formed_svg_with_the_documented_structure` (parsed with roxmltree)
- `a_drawing_is_the_same_bytes_on_every_call_and_whatever_the_document_identity`
- `a_saved_and_reopened_document_draws_the_same_bytes`
- `instances_are_drawn_in_the_order_of_the_scene_objects`
- `a_document_without_an_instance_draws_nothing`
- `a_document_without_atoms_has_an_empty_view_box`

The full `npm run check` gate ran from a clean sandbox after the last edit. Its counts are in the handoff report for this slice, not repeated here.

## Mutation checks

Each mutation was one exact text replacement in the worktree, checked to match once. Each Rust mutation ran in its own sandbox without `--cache`, and each Node mutation ran in its own sandbox. The file was then restored from a backup, and `cmp` confirmed the restore. The table lists the tests that failed in that run.

All 14 mutations were detected: 11 Rust and 3 Node. None caused a compile error. The Rust runner ran `cargo test --lib svg::` and `cargo test --test svg_render` in one sandbox command, so its logged exit status is only the second command's. The rows below come from the `test ... FAILED` lines in each log.

Rust (`lib` is `cargo test -p chemifree-application --locked --lib svg::`; `svg_render` is `--test svg_render`):

| Mutation | Change | Failing tests |
| --- | --- | --- |
| M1 | `LABEL_GAP` 1.4 to 1.5 | lib: `a_bond_starts_past_a_label_box_by_the_label_gap_and_half_the_line_width`, `a_crossed_line_that_starts_beside_a_label_box_is_cut_where_it_leaves_it`; svg_render: `goldens_match_the_drawing_byte_for_byte`, `the_parity_fixture_is_the_rust_layout_of_the_golden_documents` |
| M2 | `GOQ` advance 0.8 to 0.9 in the sans-serif table | svg_render: `goldens_match_the_drawing_byte_for_byte`, `the_parity_fixture_is_the_rust_layout_of_the_golden_documents`. No unit test failed. |
| M3 | Hydrogen side inverted at the call site in `svg.rs` | svg_render: the same two tests. No unit test failed. |
| M4 | `DASHED` "2.5 2" to "2.5 3" | lib: `dashed_and_dotted_lines_carry_the_canvas_dash_lengths`; svg_render: `goldens_match_the_drawing_byte_for_byte` |
| M5 | `ceil_hundredth` uses `round` | lib: `view_box_edges_move_outward_to_the_hundredth`. No golden test failed. |
| M6 | `SHOWN_QUERY_ELEMENTS` 3 to 4 | lib: `canvas_query_label_names_three_elements_then_an_ellipsis`, `a_list_query_is_labelled_with_its_shortened_canvas_text_not_its_element`; svg_render: the goldens and parity tests |
| M7 | Bond normal sign flipped in `bond_sides` | lib: `a_ring_bond_takes_the_side_of_the_ring_centre`; svg_render: the goldens and parity tests |
| M8 | Instances reversed after the sort | svg_render: `goldens_match_the_drawing_byte_for_byte`, `instances_are_drawn_in_the_order_of_the_scene_objects`, `the_parity_fixture_is_the_rust_layout_of_the_golden_documents`. No unit test failed. |
| M8b | Sort key removed (scene instance order) | svg_render: the same three tests. No unit test failed. |
| M9a | Crossed double bond clipped with `clipped` instead of `clipped_across` | lib: `a_crossed_line_that_starts_beside_a_label_box_is_cut_where_it_leaves_it`. No golden test failed. |
| M9b | Crossed branch removed from `bond_strokes` | lib: `a_crossed_double_bond_draws_two_lines_that_cross_midway`, `a_crossed_line_that_starts_beside_a_label_box_is_cut_where_it_leaves_it`; svg_render: the goldens and parity tests |

Node (`node scripts/svg-parity-client-check.mjs`, 10 tests; each run had 9 passing and 1 failing):

| Mutation | Change | Failing test and message |
| --- | --- | --- |
| T1 | `bond-orders` b1[0].x1 plus 0.5 | `bond-orders: the web layout is the Rust layout`: `instances[0].bonds.b1[0].x1: web 0 differs from Rust 0.5 by more than 0.011` |
| T2 | `chain-bare-vertices` a3 box.x plus 0.01 | `chain-bare-vertices: the web layout is the Rust layout`: `instances[0].labels.a3.box.x: web -6.280000000000001 differs from Rust -6.270000000000001 by more than 0.000001` |
| T3 | `chain-bare-vertices` a3 runs[0].text "O" to "Q" | `chain-bare-vertices: the web layout is the Rust layout`: `instances[0].labels.a3.runs[0].text: web "O" differs from Rust "Q"` |

Each mutated file was restored from its backup, and a final `cmp` pass found every file identical to its backup.

## Decisions

1. Label widths come from the bundled advance table (`chemifree-label-advance-1`). Characters the table does not list take one fixed advance. The browser's measurement is not used, so SVG text widths can differ from the canvas when the browser measured them.
2. The view box is computed in Rust. The web's `viewBox` and `placementFrame` belong to the editor frame, so they are not compared.
3. Drawing order follows the sort in `structure-canvas.tsx`, by position in `scene.objects`. The scene's instance list order is not used for drawing. The golden `scene-object-order` pins this.
4. Coordinates are rounded to the hundredth with JavaScript's `Math.round` tie rule, so Rust and web values agree at ties.
5. A crossed mark is drawn only on a double bond, as two crossing lines. A crossed mark on any other bond is drawn as the plain bond.
6. Golden files are regenerated only when `CHEMIFREE_BLESS_SVG` is set, so an ordinary run compares and never rewrites them.
7. Abbreviation and fragment displays are not drawn as their compact text. The SVG shows the stored atoms and bonds.
8. Parity cases avoid radicals and attachment points, because those are not drawn. The Node check asserts this.

## Not run

- Browser smoke and browser label tests: no web behaviour changed.
- Visual inspection: no rasterizer or viewer was used. The goldens are compared as bytes and parsed structurally.
- Browser label metrics: the parity run has none, so SVG widths are not compared with the canvas measurement.
- Export surfaces (operation, CLI, HTTP, CONTRACT, conformance case): not part of P38a1.
- Mutation testing: the mutations were applied by hand, not with a mutation-testing tool.

## Status

P38a1 renderer core only; no export path yet. P38 remains `in_progress`. P38a2 adds the export surface and has not started.

## Known limits from review

These are limits of the current code. None is fixed in P38a1.

- Labels are centred with `dominant-baseline="central"`, which browsers honour. Some non-browser consumers (for example Inkscape, librsvg, Batik or converters) may ignore it, so labels may not be centred on their atoms there; that is an open follow-up, as no such consumer has been checked.
- A document without atoms yields a 0x0 SVG (`width="0pt" height="0pt"`, `viewBox="0 0 0 0"`). Whether the export operation refuses such a document or reports it is decided in P38a2.
- The cost of the per-instance layout in `layout_drawing` and of the per-bond ring search (`geometry::bond_rings`) has not been measured.
- The bond-stroke tolerance of 0.011 in `scripts/svg-parity-client-check.mjs` is one rounding step at the hundredth. It comes from tie handling, since a tie can round one step apart on the two sides, and not from `js_round` being inexact; its tie cases are unit-tested in `svg/geometry.rs`.
- The stroke width is read from `view.settings.line_width_pt` in three places (`layout_drawing`, `view_box_of` and the stroke-width attribute in `svg/writer.rs`), which agree today because they all read the same field of one immutable view. Computing it once in `render_drawing` is a possible later cleanup, but `layout_drawing` is `pub`, so passing the value down would change a public signature.
