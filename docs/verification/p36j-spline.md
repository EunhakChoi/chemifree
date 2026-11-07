# P36j single open cubic spline candidate

This record describes current P36j code and authored checks. The Rust executor's
completed clippy/71 focused tests and mutation evidence are recorded below from
its existing log. Web/full checks, both reviews and PR/main CI results remain
pending until evidenced. This documentation pass reads source/logs and runs no
product tests/builds/browser checks. P36 stays `in_progress`.
Earlier P36a–i no-curve/version descriptions record their own historical slices;
this candidate adds the bounded spline/native29/graphics clipboard6 profiles.
Prior polygon/ellipse delivery evidence is not a P36j execution result.

## Explicit four-point curve and conservative bounds

`shape: "spline"` is exactly one open cubic Bézier in stored order P0,C1,C2,P3.
P0 is start and P3 end; C1/C2 are explicit controls, not points the curve must
interpolate. Each coordinate is finite within inclusive±100000 document pt.
Repeated/coincident/collinear/degenerate definitions and exact stored order are
retained, without fitting, deduplication, inferred controls or normalization.
SVG is `M P0 C C1 C2 P3`, with no close command, fill or arrowhead. Even equal
endpoints do not switch the object into a closed/fill profile. Effective inherited/
explicit width supplies foreground, selection underlay and zoom-adjusted hit
stroke on the actual SVG path. Degenerate geometry may overlap or be invisible;
Scene selection and the numeric form remain available.

Bounds, Fit, marquee containment, arrangement and the latest-geometry transform
pivot use the axis-aligned box of all four stored defining points. This control-
point AABB is conservative for the cubic centerline and may be much wider than
the visible curve because controls can lie outside it. It is not an exact curve-
extrema box, stroke-ink bounds, curve intersection test or a derivative-root
solver. Pointer hit uses the actual path stroke, not a filled control-point box.
The UI explicitly describes this difference. Move, arrangement, arbitrary finite
rotation and horizontal/vertical reflection visit all four points and preserve
their roles/order. Existing rectangle/ellipse non-quarter-turn refusal can still
reject a mixed transform whole. Graph/stated stereo is not recalculated.

## Commands, local draft and complete edits

`add_spline { points: [P0,C1,C2,P3], parent?, index? }` starts with inherited
width; the public command has no width parameter. `update_spline_points
{ object, points: [P0,C1,C2,P3] }` replaces the complete definition of an existing
spline, retaining identity/shape/width/parent/order/references. Existing
`set_graphic_line_width` supplies finite0.1–10pt/null inheritance; an explicit
value equal to document width remains an override. Line/arrow/rectangle/ellipse
still require exactly2 points; polygon retains3–64 ordered vertices. Two-point,
polygon and spline commands are distinct and incompatible targets refuse as
`graphic_command_mismatch`. Stored/domain shape-count errors use
`graphic_geometry_invalid`; typed wire arrays with wrong length are malformed
`invalid_request`. Other missing/wrong-kind/bounds errors retain existing codes.
Whole transaction atomicity, no-op/revision, same-process request replay, fresh
IDs, selection history and Undo/Redo are shared.

Graphic Spline collects four valid non-drag click-release press points as a local
draft named P0,C1,C2,P3, using existing inverse pan/zoom mapping,0.01pt rounding,
finite bounds and3px drag threshold. Controls/markers show the stored definition;
the real cubic preview appears when all four exist. No write occurs before
explicit **Finish spline**, and a fifth click neither appends nor replaces.
Remove last/Cancel are explicit. Finish queues one page-root add_spline regardless
of selected group, copies all points and clears the local draft. This local
preview is not an autosaved recovery draft; failure does not persist it.

Document/revision/tool/shape/observed-service changes, Escape and window cancel
clear the draft. First-click document/revision/generation remain captured;
queued creation rejects changed context or a reconciled restart and does not
automatically replay an unknown create into a replacement process. This does
not guarantee observing every restart before the first expired request. The
numeric P0/C1/C2/P3 form applies all eight coordinates together with invalid/IME/
busy/no-op and exact-target/document/revision stale guards. Updates follow the
existing no-automatic-unknown-graphic-edit replay policy. Whole-object drag
translates all four; no endpoint/control-point/curve-tangent drag handles exist.

## Native29, strict cardinality and typed byte budgets

Native document/recovery-draft writers use29. Headers1–28 forbid spline in every
current/history side, including history-only entries checked before retention.
Restoration validates exact4/old exact2/polygon3–64 before malformed history could
be trimmed. Polygon28+, ellipse27+ and nullable width26+ admission remain separate
strict boundaries. Valid older states/history migrate in memory without read
writes; exact old document/draft bytes, now including28, are archived before first
replacement/recovery. Future30 refuses untouched. Relabelling a spline file with
an older header is not migration and does not silently discard its points.

Spline reserves640 bytes (256+96*4) for admission/physical encoding, using the
shared48MiB current/8MiB whole-history budgets and physical64MiB limit. Current or
history-only spline activates byte admission and later fixed-size edits must
continue accounting for it. Old two-point reservations remain256, polygon weights
and caption weights stay unchanged. Latest edit must fit alone; older undo trims
oldest first under byte/count limits, while restore trims oldest undo then farthest
redo after validating incoming records. The code's over-budget reason follows
changed object states: polygon-bearing change first uses
`polygon_persistence_limit`; otherwise spline-bearing change uses
`spline_persistence_limit`; otherwise existing `caption_persistence_limit` remains.
Merely having a spline in content/history does not rename a caption-only failure.
No execution of authored capacity tests is implied by these source reservations.

## Clipboard6 and independent graph/export boundary

Copied forest containing spline chooses canonical graphics v6, including all-
inherited forests. Otherwise polygon5/ellipse4/any explicit width3/all-inherited2
remain canonical. V6 requires nullable width on every graphic; group records
still have no width. V6 older-shape input is valid, and recopy may choose2–5 from
actual content. V2–5 forbid spline, v2 forbids width even null, v3–6 require valid
width, and every shape retains exact cardinality/bounded coordinates. Strict
unknown/duplicate/header/tree/settings/ref checks remain; future7 is unsupported.
No old-consumer compatibility, cubic-to-polyline conversion or silent downgrade
is claimed. Graph clipboard1 is unchanged.

Existing64KiB text/500-command/forest limits, settings match, Copy/Cut source and
reference guards, fresh pasted IDs/offsets/group/internal refs and Undo remain.
Graph/placement/atom/bond/stored-stereo and non-geometry styles are independent.
MOL/SDF/SMILES/bounded CDXML/CDX retain the complete chemical graph and report
omitted spline as `scene_object` rendering loss; external spline roundtrip is
unsupported. No chemical ring/reaction/electron interpretation or calculation is
introduced by a curve or self-overlap.

The approved public schema lists spline within a broader points/closed/properties
model, and rendering R10 covers authored controls, cubic/quadratic/arc, closure,
multiple paths, fills and dash styles. R15 covers broader transforms. Exactly one
open four-point cubic and conservative stored-point bounds are explicit Chemifree
bounded choices, not requirements inferred from a name or proof of original
application parity. Public rendering observations also warn that syntactically
complete cubic paths alone do not establish viewport containment. No multi-
segment/closed/quadratic/arc/fit/interpolation spline, fill/arrowhead, tight extrema,
control/tangent handles, scale/skew/snapping, clip/layers/images/scientific symbols
or general rendering equivalence is supplied. Broader P36 remains incomplete.

## Inspected source and authored checks; execution pending

- Domain `scene.rs`, `document.rs`, `document/persistence.rs`, transform/
  arrangement/clipboard paths: exact shape counts, all-point affine geometry,
  command distinction, byte retention and error precedence.
- Application `contract.rs`, operation catalog/transform/clipboard adapters and
  `scene_clipboard.rs`: shared HTTP/CLI DTOs and canonical/strict graphics6.
- Provider `file_format.rs`: native29 current/history/draft, strict older profiles,
  geometry validation and exact archive protocol.
- Authored domain/application/provider `tests/spline.rs`/`spline_format.rs` plus
  HTTP/CLI `tests/spline.rs`: repeated/degenerate/no-op/commands, conservative
  control-point bounds, transforms/graph/ref retention, history-only/cardinality/
  budget behavior and caption-code compatibility, codecs/archives/Cut/replay/
  offsets/formula/export losses. These are authored scope, not passed results.
- Web `editor/spline.ts`, form/draft controls, canvas/helpers/editor mutation hook,
  runtime/clipboard validators and `scripts/web-spline-client-check.mjs`: explicit
  control definitions, draft/Finish/stale/restart, full state editing, bounds,
  canonical/strict profiles and graph retention. Registration/source is not
  evidence of successful client/browser/full runs.

## Completed focused Rust execution; remaining gates pending

The Rust executor reports final verification in
`/tmp/chemifree-work/p36j-rust-final.log`; the documentation agent read that
existing record without running checks. It shows
`cargo clippy --workspace --all-targets --locked -- -D warnings` completed,
followed by 71 focused tests with zero failures: HTTP polygon1/spline1,
application polygon4/spline4, CLI polygon1/spline2, domain polygon6/spline5,
provider polygon_format4/spline_format4, application operations11 and domain
caption_budget5/scene23. These group counts total 71; the later restored-source
spline/provider repeats are not counted as extra unique cases.

The same log records five intentional private mutations detected: cardinality,
clipboard profile admission, native28 admission, reservation weight and transform.
Restored source then passes workspace spline tests and provider spline_format,
ending `P36J FINAL VERIFICATION AND FIVE PRIVATE MUTATIONS PASSED` and sandbox
exit 0. These mutant failures are sensitivity evidence, not unresolved production
failures. This is focused Rust execution, not a full workspace/full-local pass.

Web/client/browser, full isolated validation, both reviews and PR/main CI remain
pending in this handoff. Existing web log filenames alone are not evidence of
completed checks; no web result is inferred here. No merge or broader P36
completion follows from the focused Rust result.
