# P36b single-graphic canvas endpoint-drag candidate

Delivery follow-up: [#206](https://github.com/EunhakChoi/chemifree/pull/206) merged as `76016170fe68fb5ad188d04ed26153cdcdc911ba` from final `f36550d`, with successful [PR check/browser CI42324595907](https://github.com/EunhakChoi/chemifree/actions/runs/42324595907). [Post-merge main run42325796411](https://github.com/EunhakChoi/chemifree/actions/runs/42325796411) is pending in the parent handoff. The implementation-session pending notes below are historical; [P36c](p36c-graphic-creation.md) has separate pending full checks/reviews/CI. P36 remains in_progress.

Source inspected at `79434f2`. This follow-up uses P36a's existing
`update_graphic_points` transaction; it adds a canvas gesture, without a new
command, schema, migration or scientific operation. Native document/draft stays
v25 with existing graphic state/history. P36 remains `in_progress`.

Exactly one explicitly selected, noncoincident line, arrow or rectangle exposes
two pointer handles in Select mode. A directly selected graphic child within a
group qualifies. Selecting a group itself, an ancestor/descendant union, multiple
objects or mixed graph/scene targets exposes no handles. Busy/disabled canvases
cannot start the gesture. Handles are drawing overlays, not new scene objects.
Coincident endpoints remain valid geometry but show no overlapping handles;
the existing numeric endpoint form is the fallback and coordinate-entry path.

Press captures document ID/revision, the exact graphic snapshot, selected endpoint
and document-space pointer origin. Pointer coordinates use the actual SVG inverse
screen transform, accounting for prior canvas pan/zoom. Missing, singular or
nonfinite transforms cannot locate an edit. The existing canvas rounding applies.
Movement uses the pointer delta from press, retaining any offset within the
handle instead of jumping the stored point to the pointer. Only the selected
point changes; the opposite point remains exact. Existing finite inclusive
±100000 pt bounds apply, without snapping or clamping.

Space/middle-button pan takes precedence when pressed, and Pan mode exposes no
endpoint handles. Wheel zoom is ignored during an active pointer gesture; zoom
buttons are disabled, while Fit and Reset explicitly cancel it. View pan/zoom is
local navigation, not persisted graphic geometry.

Movement at or above the existing 3-client-pixel drag threshold paints a local
preview with the normal line/arrow/rectangle renderer and sends no transaction.
On valid release, one captured endpoint edit queues one existing command. The
P36a builder rechecks document/revision and exact target and copies the submitted
points. A click/subthreshold gesture, unchanged coordinates or invalid final
point set yields no effective update. Valid changes share one Undo step, normal
no-op, replay, atomicity and selection/history behavior. Point order, arrow tip,
rectangle corner interpretation, shape/ID/parent/group/order/references, chemical
graph, placements and stereo keep their existing contracts.

Escape on the canvas, pointercancel, lost pointer capture, blur, Fit and Reset
discard previews and release capture. Document/revision, explicit selection,
target geometry/kind/group membership or tool changes invalidate the press
context. Queue-time validation prevents a delayed release from replacing newer
geometry. An expired-service request may be refused, but an unknown graphic edit
is not automatically resent into a restarted process; normal explicit reload
reconciliation applies.

Authored regression scope, not a successful execution claim:

- `scripts/web-graphic-points-client-check.mjs` adds single/noncoincident graphic
  selection, group/mixed/multiple refusal, press-offset/opposite-point retention,
  coordinate bounds and stale-context cases.
- `scripts/web-graphic-points-check.mjs` extends the production canvas/form/editor
  harness against a real backend with actual SVG screen transforms and zoomed
  handles, preview without writes, one release transaction, all three shapes,
  exact opposite point, coincident numeric fallback, subthreshold/no-op,
  Escape/pointercancel/lost-capture disposal, Space/middle pan, revision changes,
  native save/reopen and service-restart no-retry behavior.
- Existing P36a domain/application and numeric-form checks remain applicable;
  no new Rust or persistence semantics are introduced by this slice. Existing
  `test:client` and `test:browser:graphic-points` registrations carry the new
  cases rather than implying a new command or independently completed P36 scope.

General bounding-box resize, aspect locking, multi-object/group handles, rotation
handles, new shapes, curves, image controls, clipping, layers, graphic styles,
automatic layout and reaction/electron-arrow semantics remain unsupported by
this slice. It does not claim complete P36 acceptance or external-renderer parity.

Verification status: this documentation pass inspected source/diffs and ran no
tests, builds or browser checks. P36a PR #204 merge is pending in the parent
handoff; it is distinct from P36b's pending full checks, Codex and independent
fresh-context reviews/finding dispositions, and PR CI. General smoke in progress
is not recorded as passed. No merge or successful full verification is claimed.
