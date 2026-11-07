# P36i bounded polygon candidate

This record describes the P36i candidate in [PR #214](https://github.com/EunhakChoi/chemifree/pull/214),
reviewed at `f1a797848867668187ca30a192271ccbd35a9ded` before this correction.
Completed focused Rust/web execution and both reviews are recorded below from
existing logs and PR evidence. This documentation pass reads those records and
runs no product tests/builds/browser checks. Full isolated local validation and
CI remain in progress per the parent handoff; no full-local/CI pass, merge or
post-merge main success is claimed. P36 remains `in_progress`. Earlier
P36a–h version/no-new-shape statements are historical slice boundaries; P36i adds
the polygon profile and native28/graphics clipboard5 described here.

## Ordered geometry and command separation

A polygon is a closed, stroke-only scene graphic with 3–64 ordered vertices.
Each coordinate is finite and within inclusive ±100000 document pt. The maximum
64 is an internal Chemifree resource-budget choice, not a bound established by
public specification or a chemical constraint. Repeated vertices, a repeated
first vertex at the end, collinear, self-crossing, coincident and zero-area paths
are retained exactly. No minimum edge/area, orientation, simple-polygon test,
simplification, winding correction or deduplication is inferred. SVG `polygon`
closes the last-to-first edge without synthesizing another stored point; fill is
none. Degenerate geometry may have overlapping or invisible strokes; Scene and
the numeric vertex form remain the way to select/edit it.

The snapshot/native/clipboard graphic state still calls the list `points`.
Polygon commands instead use `vertices`:

- `add_polygon { vertices, parent?, index? }` creates a fresh polygon. Omitted
  parent/index creates a page root; the public command starts with inherited
  width and has no line-width input. Existing `set_graphic_line_width` can then
  set finite0.1–10 pt/null inheritance, retaining explicit values equal to the
  document width as overrides.
- `update_polygon_vertices { object, vertices }` replaces the whole ordered list
  of an existing polygon, preserving shape/ID/width/parent/order/references.
- Existing line/arrow/rectangle/ellipse remain exactly two points. `add_graphic`
  does not create polygon and `update_graphic_points` does not edit it; polygon
  updates do not change an old shape into a polygon. These command/shape mismatches
  use `graphic_command_mismatch`; invalid shape-specific cardinality uses
  `graphic_geometry_invalid`. Malformed wire fields retain `invalid_request`;
  domain refusals use the existing indexed invalid-command/atomic transaction
  path. Missing/wrong-kind objects and coordinate bounds retain their usual codes.

Shared atomicity, same-process replay, no-op, selection history, Undo/Redo and
fresh-ID rules remain. Move, arrangements, marquee full containment and geometry
bounds use all vertices, including the last. Arbitrary finite rotation/reflection
transforms every polygon vertex; rectangle/ellipse arbitrary-angle refusal remains
and can still refuse a mixed selection whole. Graph/stored stereo, placements,
other objects, refs and non-geometry styles remain unchanged. Drawing/stereo
agreement is not a stored-stereo recalculation.

## Local click draft and complete vertex-list editing

Graphic explicitly offers Polygon. A valid click press/release below the existing
3 client-pixel drag threshold appends the press point to a local ordered draft;
a drag does not append a vertex. Existing inverse SVG pan/zoom mapping, 0.01 pt
rounding and bounded coordinate checks apply. The draft may have fewer than three
vertices and sends no edit. Its preview uses an open polyline below three and the
real closed renderer from three onward, with vertex markers. It is not persistent
recovery content. Reaching64 stops append without auto-finishing. Remove last
and Cancel are explicit controls; only **Finish polygon** with3–64 vertices
queues one page-root `add_polygon`, irrespective of selected groups.

Draft context captures document/revision/observed service generation; document,
revision, tool/shape or generation changes discard it. Escape and window
cancellation clear it. Finish copies the vertex list and keeps the captured
revision. The queue rejects different document/revision, observed-generation
change or a reconciled restart; an unknown creation is not automatically replayed
into a replacement service. This is the existing bounded graphic-create policy,
not a guarantee to detect every restart before the first expired request. Finish
clears the local draft; a failed write does not turn it into saved draft content.

The selected polygon gets an ordered numeric x/y vertex-list form. Row add/remove
last are local, and Apply replaces all vertices in one command. Invalid/empty/
nonfinite/out-of-bounds/IME/busy/unchanged submissions do not edit. Submitted
document/revision/exact target are captured; changed target/geometry/parent or
revision lapses instead of editing a replacement. Vertex edits share the existing
no-auto-replay rule for unknown graphic edits after restart. Polygon is excluded
from the two-point endpoint handle path. Whole-object dragging translates every
vertex; there are no per-vertex drag/insert-on-edge/resize/rotation handles.

## Native28 and shared byte/undo retention

Native document and recovery-draft writers use28. Headers1–27 forbid polygon in
current and every before/after history state, including history-only content
before retention. Restore validates shape-specific cardinality before trimming
history: old shapes never gain arbitrary point counts. Ellipse admission still
starts at27, and required nullable width still starts at26; earlier headers
retain their strict field prohibitions and inherited migrations. Valid old
state/history migrates in memory without read writes. First replacement/recovery
archives exact old document/draft bytes, now including27; future29 refuses
untouched. This is not permission to change headers on a polygon-bearing file.

Polygons activate the existing variable-size admission policy even when present
only in undo/redo after deletion. Each polygon reserves256 +96 bytes per vertex
for admission/physical encoding; old two-point graphics retain their exact256
reservation. Shared current content limit is48MiB, whole-history limit8MiB, and
physical document/draft read limit64MiB. A latest edit must fit on its own; older
undo entries are trimmed oldest first to make room, in addition to existing
entry/entity-change limits. Restore trims oldest undo then farthest redo only
after validation. Subsequent fixed-size edits still account for retained
history-only polygons until those entries are removed.

An over-budget polygon-bearing change reports `polygon_persistence_limit` and
leaves the whole edit unchanged. Caption-only overflow preserves the existing
`caption_persistence_limit` reason/message even in a polygon document; polygon
presence does not rename every text/budget failure. Authored capacity tests do
not by themselves establish an executed maximal-file or full-local success.

## Clipboard5, graph and remaining scope

Canonical graphics Copy/Cut output chooses v5 iff its included forest contains
polygon; otherwise ellipse chooses v4, any explicit width v3, and all inheritance
v2. Outside/unselected shapes do not choose the profile. V5 requires nullable
width on every graphic and preserves all points/group order/internal refs. V5
input containing only older shapes is valid; recopy chooses canonical2/3/4 from
actual content rather than retaining the input header. V2–4 forbid polygon;
v2 still forbids width even null; v3–5 require valid width on every graphic.
All shapes retain their exact count rules and strict keys/tree/settings/ref/point
validation. Future clipboard6 is unsupported. No v2/v3/v4-only consumer
compatibility, implicit polygon conversion or silent downgrade is claimed.

Existing64KiB UTF-8 text/500-command and other forest bounds, settings-match,
source/Cut guards, mixed graph/placement/outgoing-ref and Cut incoming-ref
refusals remain. Fresh pasted IDs, offsets, nullable width and internal references
are preserved; no graph clipboard v1/settings schema change is introduced.
MOL/SDF/SMILES/bounded CDXML/CDX keep the chemical graph and report omitted scene
polygons as `scene_object` rendering loss; external polygon roundtrip is absent.
Polygon is geometry only, with no atom/bond/ring/aromaticity/reaction inference
and no change to scientific calculations.

The approved public schema lists polygon in a broader graphic points/closed/
properties model; rendering R10/R12/R15 are broader still. This candidate does
not implement open polylines, fills/fill rules, arbitrary colors/dashes, curves/
arcs/splines, regular-polygon constraints, snapping/aspect locks, clipping/layers,
images/scientific symbols or scale/skew/vertex/resize/rotation gesture controls.
The internal64 bound and mandatory closed stroke profile are explicit Chemifree
choices, not general specification or original-application equivalence.

## Source and authored verification scope

- `crates/domain/src/scene.rs`, `document.rs`, `document/persistence.rs` and
  scene clipboard/transform paths: shape/cardinality/commands, all-vertex geometry,
  shared byte budgets, selection/history and refusal contracts.
- `crates/application/src/contract.rs`, operation catalog/transform/clipboard
  adapters and `scene_clipboard.rs`: public commands/snapshots and strict v5.
- `crates/providers/src/file_format.rs`: native28 current/history/draft admission,
  prior profiles/migrations/archives and independent old-shape cardinality.
- Authored `crates/domain/tests/polygon.rs`, `crates/application/tests/polygon.rs`,
  `crates/providers/tests/polygon_format.rs`, HTTP/CLI `tests/polygon.rs` inspect
  cardinality/repetition/degeneracy, all-vertex transforms/bounds, mismatches,
  rollback, graph/formula/export loss, replay/Undo, budgets including history-only
  polygons and caption-code preservation, strict codecs and exact old archives.
- Web `editor/polygon.ts`, shared graphic/selection helpers, actual canvas/forms,
  `use-document-editor.ts` and runtime/clipboard validators; authored
  `scripts/web-polygon-client-check.mjs` and `web-polygon-check.mjs` cover draft/
  Finish, ordered forms, all-vertex paint/hit/selection/movement, strict/canonical
  profiles and stale/restart/history/native paths. Script content/registration is
  not execution evidence.

## Completed focused execution and reviews

The following existing agent logs under `/tmp/chemifree-work/` were read for this
correction, alongside the #214 body. They are local run records, not executions
by the documentation agent or completed full/CI delivery evidence. P36h results
are not reused as P36i passes.

| Record | Confirmed completed scope |
| --- | --- |
| `p36i-rust-final.log` | Isolated Rust clippy and 55 focused tests passed: HTTP polygon1, application polygon4, CLI polygon1, domain polygon6, provider polygon_format4, application operations11, domain caption_budget5 and scene23. All groups report zero failures; sandbox exit 0. Polygon and provider tests were repeated after mutation restoration; those repeats are not extra unique cases in 55. |
| `p36i-rust-final.log` mutation section | Five intentional private mutations were detected: history-only budgeting, cardinality, budget, clipboard and native27 admission. The record then reruns the restored polygon/provider groups successfully and ends `FINAL VERIFICATION AND FIVE PRIVATE MUTATIONS PASSED`. These expected mutant failures are sensitivity evidence, not unresolved production failures. |
| `p36i-web-gate-3.log` | Isolated lint (`eslint --max-warnings 0`), typecheck, complete client command and production web build passed; every client group has zero failures, including all 6 polygon client cases, final sandbox exit 0. |
| `p36i-web-browser-4.log` | Service-backed polygon, ellipse, Graphic creation and graphic endpoint browser checks passed, sandbox exit 0. Polygon covers explicit Finish/no-write draft, closed-stroke hit, 3–64 vertices, whole-list editing, move/rotation, styled v5 and native28 restart. Prior-shape browsers cover their own retained geometry/preview/cancel/stale/restart/native behavior. This log does not establish a general browser-smoke run. |
| `p36i-polygon-final.log` | A final polygon browser run passed the same stated polygon scope, sandbox exit 0. |
| `p36i-web-mutation-2.log` | Three mutations were detected: 65th vertex admission, moving only the first two points and accepting polygon under v4. Restored source then passed all 6 polygon client cases, sandbox exit 0. |
| `p36i-conformance-record.log` | Separate record generation completed 110 conformance cases: 110 passed, zero failed/not run, sandbox exit 0. That run does not establish completion of later full isolated checks. |

The #214 body records an independent fresh-context subagent review of
`13cebd51e50244461933debd23dab702b78afeda` on 2026-11-07 UTC with no findings.
This is the exact reviewed commit, not an additional review of this documentation
correction. The [Codex review summary](https://github.com/EunhakChoi/chemifree/pull/214#issuecomment-6702712381)
records completed review of `f1a797848867668187ca30a192271ccbd35a9ded` at
2026-11-07 05:16:55 UTC, with no findings and no unresolved findings. The absence
of a formal review submission is not treated as the evidence; the completed
summary explicitly identifies the reviewed commit and result.

Full isolated local validation and CI remain in progress in the parent handoff.
The PR is open; no merge or post-merge main-CI result is claimed. The focused
passes and review results do not complete broader P36 acceptance or remove the
unsupported behavior above. This correction changes documentation only and
reruns no product checks.
