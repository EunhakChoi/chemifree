# P36h bounded ellipse candidate

This record describes the current, still-changing P36h source and authored
checks. Documentation inspection ran no product tests, builds or browser checks.
Targeted Rust/web execution, full isolated checks, both reviews and PR/main CI
remain pending until their actual results are supplied. P36 stays `in_progress`;
this is not merged delivery or general rendering/specification equivalence.
Earlier P36a–g no-new-shape/no-schema/native25/native26 statements describe their
own implementation slices. This candidate changes the shape admission and native/
clipboard versions as stated here; their historical test notes are not P36h passes.

## Geometry, editing and visible behavior

`add_graphic` accepts `shape: "ellipse"` with exactly two finite document points,
within inclusive ±100000 pt. They are opposite corners of an axis-aligned bounding
box, not center/radius, arc endpoints or control points. Stored point order stays
exact, including reversed, zero-axis and coincident points. Rendering uses
`cx=(x1+x2)/2`, `cy=(y1+y2)/2`, `rx=abs(x2-x1)/2`, `ry=abs(y2-y1)/2`.
SVG foreground, selection underlay and hit stroke use `fill="none"` and effective
graphic width: null inherits the document width dynamically; an explicit finite
0.1–10 pt value remains an override even when equal to the document value.

Zero rx or ry has no visible ellipse stroke, including its transparent hit stroke.
The stored object is retained: select it in Scene and use the numeric corner form.
Coincident corners have no distinct canvas endpoint handles. Distinct corners use
the existing endpoint gesture; it edits the stored corners, not points on the
ellipse circumference. Marquee selection uses the existing full two-corner box
containment policy, not a new curved-edge intersection/lasso algorithm.

The explicit Graphic shape choice and Scene Add controls include Ellipse. Canvas
creation retains the existing pan/zoom inverse mapping, 0.01 pt rounding, finite
bounds and 3 client-pixel drag threshold. Move paints only local preview; valid
release queues one `add_graphic` without parent/index at the page root. Existing
cancel/document/revision/shape/observed-service checks and no automatic replay of
unknown creation requests into a replacement process still apply. Numeric corner
and width edits retain their existing capture/stale/atomicity policies.

Move, group/subtree order, references, corner edits and width edits preserve the
ellipse shape, identity and unrelated scene/graph state. Exact quarter turns,
full-turn identity and horizontal/vertical reflections reuse existing transforms.
A normalized rotation other than exact 0/90/180/270 degrees refuses as
`transform_ellipse_unsupported`, in both transform-plan and transaction paths.
This also applies to circles and degenerate ellipses, including selected group
descendants. There is no near-quarter tolerance or circle exception. A mixed
selection/transaction refuses whole, without first moving other objects/atoms or
committing an earlier command. No stored chemical stereo is recalculated.

## Native27 and clipboard compatibility

Native document and recovery-draft writers use version27. Versions1–26 forbid
ellipse graphics in current and every history side, including history-only
states checked before history retention. A relabelled old header is not migration.
Width admission remains independent: v26 and v27 require nullable graphic
`line_width_pt` in current/history; v1–25 forbid it and migrate to inheritance.
Existing valid older shapes/state migrate in memory with their history; reading
writes nothing. First overwrite/recovery preserves exact old document/draft bytes
under the existing versioned archive protocol. Future v28 is refused untouched.
Shape-changing corrupt history is refused. No new persistence reservation is
introduced for this shorter shape tag; the existing document/history/file limits
remain binding, rather than evidence of a newly measured maximal-file test.

Graphics clipboard output chooses v4 exactly when its copied forest contains an
ellipse, including a descendant and even when every width is inherited. Every
graphic in v4 has required nullable `line_width_pt`; group entries gain no width.
Without an included ellipse, output stays canonical v2 for all inheritance or v3
for any explicit width. An unselected ellipse does not change that decision.
V4 input may contain only older shapes/all-null widths; recopy then chooses v2/v3
from actual content. It is not required to retain the input header version.

V2/v3 reject ellipse; v2 forbids width even if null; v3/v4 require width on every
graphic and validate finite0.1–10/null. Unknown/duplicate keys, malformed trees,
settings, references or points keep the existing strict refusal; future v5 is
unsupported. No v2/v3-only consumer compatibility or silent ellipse downgrade is
claimed. Copy/Cut/Paste preserve explicit/inherited width, ordered subtrees,
internal object references and point order with fresh pasted IDs/offsets. Existing
settings-match, byte/command limits, graph/mixed-selection/placement/outgoing-ref
and Cut incoming-ref refusals remain. Graph clipboard v1 is unchanged.

## Graph, exports and specification boundary

Ellipse is geometry only, without chemical meaning. Chemical graph, placements,
atom/bond states, stored stereo and all scientific calculations remain independent.
MOL/SDF, SMILES and bounded CDXML/CDX exports retain the complete chemical graph
and report dropped ellipse scene objects as `scene_object` rendering loss; this
is not external ellipse roundtrip support. Other consent rules still apply.

The approved public `phase3-design-v3/document.schema.json` lists ellipse in a
broader graphic model with arbitrary points, closed and properties fields.
`deep-rendering-contracts.md` R12 also covers fills/shading/symbols, and R15 covers
broader transforms. Two axis-aligned corners, stroke-only presentation and the
arbitrary-angle refusal are this bounded Chemifree profile. Polygon/spline/arc/
bracket/orbital/scientific symbols, fill/color/dash/shadow/aspect locks, arbitrary
ellipse orientation, scaling/skew/resize/rotation handles, images, clip/layers and
reaction/electron semantics are not supplied. The broader P36 acceptance remains
open; no original-application rendering or Automation parity is established.

## Inspected source and authored checks; execution pending

- Domain: `crates/domain/src/scene.rs`, `document.rs`, `scene/transform.rs`;
  `crates/domain/tests/ellipse.rs` authors reversed/zero/coincident cases,
  geometry/style/graph/group/reference retention, atomic arbitrary-angle refusal
  including circles/degenerates, and clipboard/Cut/Undo.
- Application: `contract.rs`, `operations.rs`, `scene_clipboard.rs` and operation
  adapters; `crates/application/tests/ellipse.rs` authors v4/canonical v2-v3,
  strict headers/fields, offsets/replay/history, transform refusal and rendering
  losses with unchanged formula/graph.
- Provider: `crates/providers/src/file_format.rs`;
  `crates/providers/tests/ellipse_format.rs` authors native27 current/history/draft,
  old-header/history-only rejection, v26 exact-byte document/draft archives,
  future28 and corrupt history refusal. Existing migration checks are updated.
- HTTP/CLI: `apps/api/tests/ellipse.rs`, `apps/cli/tests/ellipse.rs` author shared
  operation/replay/quarter-turn/copy/Undo and arbitrary-rotation boundaries.
- Web: contract/runtime and clipboard validators, actual SVG renderer, Graphic
  controls/corner form/rotation guidance and clipboard panel;
  `scripts/web-ellipse-client-check.mjs` authors strict/canonical transport,
  snapshot/corner/offset preservation and unrelated-state checks. The authored
  `scripts/web-ellipse-check.mjs` uses real components with a service-backed browser
  harness for preview-only creation, SVG radii/stroke/hit behavior at zoom,
  width inheritance/override, corner editing/handles, transforms, clipboard and
  native persistence. `test:browser:graphic-ellipse` registration or script content
  does not establish a successful P36h browser run.

No pass count or review disposition follows from reading these files. Execution
and delivery results will be appended only with their actual source/log/CI head.
