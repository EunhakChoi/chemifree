# P38b2 SVG export of captions: verification record

Scope: `document.export` with `format: "svg"` draws a caption as SVG text when every one of its characters is in a bounded drawn set (printable ASCII, Latin-1 without U+00AD, Greek capitals and smalls, and U+2013, 2014, 2026, 2032, 2033, 2190, 2192, 21CC, 2212). Each caption is one `<text>` with one `<tspan>` per run (generic font family, size, weight, style, fill, relative `dy` for scripts, outline stroke of size/16, a hidden-run shadow copy at +1 pt and opacity 0.35). The caption reserves a box in the view box from the existing advance table `chemifree-label-advance-1`. A caption with a character outside the set is not drawn and stays the consent loss `caption`; a drawn caption with an underlined run reports the new consent loss `caption_underline` (loss table 16 to 17 rows). Model id `chemifree-svg-2` to `chemifree-svg-3`. Not claimed: shaping, embedded fonts, glyph outlines (P38b3, an open unscheduled proposal), clipping, layers, a web download UI, `render.scene`.

## Run after the last edit

- `npm run check` in the sandbox: the Rust formatting, clippy (`-D warnings`), nextest and doc-test stage, the contract smoke and the nextest-override guard (33 exact names) passed; conformance 119 cases, 119 passed (one new: `svg-export-caption`; `suite_version` 35 to 36). A first run stopped at clippy `explicit_auto_deref` in `tests/svg_caption.rs` (fixed), and the next run stopped at the two `현재 집계` lines (updated to the pasted counts). The conformance, traceability, checks, client, caption and label browser checks and web build were run again after those two fixes (log `gate-p38b2c.log` in the session scratchpad; results in the PR comment).
- New and changed tests: `tests/svg_caption.rs` (13 tests, public layout and render), `tests/svg_render.rs` (caption golden `captions.svg`, parity), `fixtures/svg/parity.json`, `scripts/svg-parity-client-check.mjs` (Rust layout against the web run rules), `tests/svg_loss_drift.rs` (rows `caption`, `caption-underline`, `group`; two new tests), `tests/svg_operations.rs` (consent round trip, a drawable caption), `tests/format_wording.rs` (seven wording blocks), `scripts/web-client-check.mjs` (ten `caption-style` cases).
- Conformance: `svg-export-caption` is new; its expected content was read from the first run and checked by hand (top edge 60 - 12 - 2 = 46, bottom edge 60 + 7.2 + 2 = 69.2 from the 8 pt subscript shifted 4 pt). `svg-export-scene-objects` only names the new model id.

## Mutations (each applied alone in a private copy, then restored and compared by hash)

`svg_caption.rs`, 14 mutations, every one caught by at least one named test:

| Mutation | Caught by |
| --- | --- |
| subscript shift size/4; no two-thirds scale on a subscript | script size and shift, view box, outline tests |
| colour default `null` instead of `#000000` | seven tests |
| centre offset the full advance | alignment test |
| outline width size/8; outline growth summed instead of the widest | outline stroke and box-growth tests |
| shadow copy without `visibility="hidden"`; without the +1 pt offset; no +1 pt in the box | shadow test |
| a `y` on each tspan | seven tests |
| U+00AD and U+0301 made drawable | the outside-the-set test |
| caption ink missing from `object_ink`; captions moved to the end of the paint list | view-box test; paint-order test |

Other groups: the 13 `svg_render` mutations (colour default, bond stroke colour, `visibility` in the attribute set, superscript factor, the other new checks), the `format_wording` per-combination substitutions (sentence and export phrase), and the web parity mutations were run by their test groups. Not mutated separately: the family, size, weight and style fields of the parity comparison, which share one assertion with the mutated fields.

## Decisions made during implementation

- The reserved box is a model, not the ink: a viewer's generic face may be wider than the table, as for labels. The right edge is `x - offset + advance + overhang(last)`, so the box reaches past the advance at both ends (the plan's literal formula gave no right overhang for a single run).
- An empty run list has no layout (`None`); a validated document cannot reach it.
- Stored caption colours are drawn: an exception to the earlier rule that all ink is black (CONTRACT, What is drawn). A run without a stored colour is `#000000`.
- The web run rules moved unchanged to `apps/web/src/editor/caption-style.ts`; `scene-caption.tsx` reads them. Rust and TypeScript share no code; `svg-parity-client-check.mjs` compares them.
- `caption-style` cases are `node:test` cases in `scripts/web-client-check.mjs`; the repository has no Vitest.

## Process notes

- The runner's private copy keeps source mtimes (`cp -a`), so cargo can reuse a shared-cache artifact built from other source. Workers and the gate script ran `find crates -type f -exec touch {} +` inside the sandbox before building. The cache itself was not purged.
- One test-group agent ended without a structured result (the loss-drift and operations group); its edits were checked by reading the diff and by the gate run.
