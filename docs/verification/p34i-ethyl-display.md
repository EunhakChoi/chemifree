# P34i: retained ethyl graph, compact Et display

Status: implemented candidate; execution verification pending. P34 remains `in_progress`.
Branch: `feat/p34i-ethyl-display`, isolated at `chemifree-p34i`, based on P35a HEAD
`ed52406`, now integrated with P35a PR #184 squash main `cadec8e`.
This record claims neither merged P34i delivery nor successful integration CI.

## Authorized boundary

The user chose real graph retention with display-only contraction. This slice deliberately
supports one explicit terminal neutral/default ethyl pair and the fixed label Et. It adds no
parser, automatic recognition, custom nickname, inferred chemistry, external DATA/provider,
multiple-placement contraction, local stereo depiction, multi-boundary/nested abbreviation,
caption relationship, rich text or layout engine. It never executes original application DLLs.

The display map belongs to the document separately from graph/scene objects. It is keyed by
instance/anchor and records the real terminal atom. Stored atom/bond IDs, recorded states,
positions, marks, configurations and groups survive collapse/expand byte-for-value. The
public graph and chemical inspection inputs remain complete. v9 extends the P35a v8 schema
without reusing its version number; v8 captions/history remain intact on migration.

## Implementation and written checks

- Domain commands `collapse_ethyl`/`expand_ethyl`, exact eligibility and overlap checks,
  per-command invalidation within atomic transactions, no-op, display diff/history limits,
  checked restore/replay and ordinary copy-as content handling. Full rigid scopes retain the
  label; partial geometry and related chemistry/placement changes expand it.
- v9 documents/drafts require display lists and history diffs. v1–8 headers forbid those
  fields, including empty and history-only ones. Legacy migration fills empty maps/diffs;
  v8 caption preservation and byte archives reuse the existing save/recovery providers.
- Application strict wire commands, additive snapshot field, 52 transaction rows
  (41 command rows, 11 capability rows), typed refusal `abbreviation_invalid`.
- Canvas visible projection shared by ink, hit and underlay; natural Et label bounds and
  external-bond trimming; click/Shift-click/marquee whole-pair/internal-bond selection;
  movement uses both real coordinates. Erase on Et deletes both members in one graph edit.
- Fixed-label panel reuses existing shadcn Button, its variants and semantic theme tokens.
  It offers explicit anchor actions for the selected pair; domain eligibility is authoritative.
- MOL/SDF/SMILES/CDXML/CDX writers consume the full graph and report a dropped rendering
  `abbreviation_display` loss. That display loss alone needs no consent; existing caption,
  chemistry and stereo consent remains. Native graph copy, Cut preparation and template
  export refuse intersecting compact members instead of silently dropping display metadata.
- `crates/domain/tests/abbreviation.rs`: unchanged full graph/coordinates/numbering/stereo,
  no-op/Undo/Redo/restore, full/partial movement/transform, member/local mark edits and atomic
  rollback, malformed boundary/multiple-placement refusal, unrelated marked fragment,
  native-copy refusal and exact member-selection history.
- `crates/application/tests/abbreviation.rs`: operation replay/save/reopen/recovery/history,
  full graph snapshots, formula C2H6O, full-graph MOL/SMILES byte equality, rendering losses in
  all five writers, typed refusal/malformed wire and native-copy atomic refusal.
- `crates/providers/tests/abbreviation_format.rs`: current display/history/draft roundtrip,
  v8 caption/document/draft migration with unchanged original bytes, archive on save/recovery,
  required current fields, forbidden old-header history fields and forged history members.
- `scripts/web-abbreviation-client-check.mjs`, imported by `test:client`: runtime snapshot
  validation/refusal, hidden ink/hits, external clipping, whole-pair marquee/geometry and
  stale queued references.
- `scripts/contract-smoke.mjs`: HTTP=CLI parity for collapse/replay/refusal/copy/formula,
  rigid movement, member mutation/Undo/save/reopen. Existing writer-version/future-version
  assertions were advanced consistently; existing v8 caption test fixtures remove v9 fields
  when constructing an older schema.
- `npm run test:browser:abbreviation`, connected to CI: real panel, editor, HTTP backend and
  canvas; hidden hit/ink, boundary-only selection followed by Et click/drag, exact pair shape,
  natural glyph bounds, hidden member mutation/Undo, v9 save/page reopen/expand.

## Reviews and dispositions

Codex author review: re-read production/domain/provider/DTO/canvas changes and public tests.
Found and corrected stale current-version fixtures/catalog expectations, missing pre-save
setup in a new draft test, an incorrect browser SVG selector/box decoder, and an overbroad
covered-selection change that would have altered ordinary atom selection. The covered-path
normalization is now limited to compact targets. No outstanding known finding; execution
verification is still required, including compile/type/lint correctness.

Independent review: one fresh-context subagent (`p34i_review`), read-only, against the
uncommitted production implementation with the approved scope and local quality rules.
It reported one P2: selecting the visible boundary bond, then clicking Et, retained only the
boundary selection because the canvas considered the anchor geometrically covered. Dragging
would leave the terminal behind and expand/mutate the shape. **Fixed**: even when covered,
a nonadditive compact click unions the full pair/internal bond into the existing selection.
The production browser regression explicitly starts with the boundary-only selection. The
review identified no additional P1/P2 finding and no arbitrary requirement expansion. No
second review round was run; the contributor policy permits fixes after this review.

Applied skills: rust-patterns, rust-testing, typescript-patterns, typescript-testing, shadcn.
User instructions prohibit tests/build/browser/push, overriding the skills' execution gates.

## What actually ran

Only source/document reads, worktree creation, edits, `cargo fmt --all`, and `git diff --check`.
Formatting and whitespace inspection succeeded. No tests, compilation, clippy, type checks,
frontend lint/build, browser, mutation verification, push or CI ran. New test failure-sensitivity
is therefore not execution-proven. Do not interpret authored checks as passing evidence.

Required later, on the integrated P35a/P34i tree: Rust targeted abbreviation/provider/application
checks, lint/type/client gates, the complete existing check suite and browser suite including
caption and Et, then PR CI and required Codex review. Planned mutation checks should remove the
hidden-member filter, covered-target normalization, graph invalidation or required-v9 guard
and show their corresponding tests fail before restoring the code.


## P35a main integration

Integrated main `cadec8e` after PR #184. Kept its caption validation, measured canvas bounds,
viewport correction, wire checks, CLI text output, persistence budgets, compact serializer,
and recovery regressions. Applied only the original P34i feature diff over that final main
content to avoid restoring obsolete P35a candidates through the squash merge conflicts.

- v9 keeps v8 captions and the caption-safe 48 MiB current / 8 MiB combined history
  reservations. The reservations now include the current Et map and complete before/after
  display diffs. A bulk 100-display regression compares the added content and isolated
  undo/redo history reservations against actual native JSON bytes, so unrelated graph
  reservations cannot conceal missing display accounting.
- Updated old document/draft fixture construction to omit v9 fields and future-version
  refusals to use v10 while preserving current/history-only schema rejection.
- Kept the final P35a recovery rule: unchanged recovered old drafts stay byte-identical;
  only the next actual edit is written as v9. The v8-caption migration regression now checks
  unchanged autosave, an actual collapse edit, v9 rewrite, exact v8 archive, and recovery
  Undo/Redo through both the new display and original title history.

Read-only inspection and formatting/whitespace checks only in this integration pass.
Tests, builds, clippy, browser, mutations and push remain explicitly deferred to the parent
verification task; neither the new assertions nor the integrated full suite have been run.

### Integration compile correction

The parent's isolated `npm run check` on `15c63ec` reached Rust compilation and failed
with E0308 (`SceneError::ALL`, declared 21 entries versus 22 actual entries), recorded in
`/tmp/chemifree-work/p34i-full.log`. The merge retained both `caption_persistence_limit`
and `abbreviation_invalid` but missed their combined array length. Corrected the declaration
to 22 without changing or weakening the existing exact reason-list regression.

Statically counted related closed lists: scene reasons 22, top-level operations 40,
stereo-group reasons 11, transaction rows 52 (41 commands / 11 capabilities), and compared
those with their declarations and existing assertions. Corrected the caption contract's
stale current transaction totals from 39/50 to 41/52. Provider migration constructors retain
explicit empty abbreviation fields; test-only change sets use the default for new fields.
No further mismatch was found by this inspection. Only formatting and diff whitespace checks
ran for this correction; Rust compilation and all runtime verification remain with the parent.
