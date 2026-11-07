# P36c explicit canvas graphic-creation candidate

Delivery follow-up: P36c #207 merged as `afe6410`; [post-merge main run42329036170](https://github.com/EunhakChoi/chemifree/actions/runs/42329036170) later completed with **failure** in the browser job (checked with `gh run list`); #209 followed. See the [P36 status table](../HANDOFF.ko.md). Earlier implementation-session pending notes below are historical. [P36d](p36d-graphic-line-width.md) separately adds candidate native v26 graphic widths and has its own pending web/full/review/CI gates; P36 remains in_progress.

Source/test inspection covers P36c implementation `fbec0a0` and review fixes
`a6c2f10`, `b630a3b` and `5864f84`, based on predecessor `f36550d`. This
candidate adds Graphic tool creation using the existing `add_graphic` command;
it adds no backend operation, native schema, provider format or scientific rule.
P36 remains `in_progress` against its broader unchanged roadmap scope.

The Graphic tool (G) exposes Line, Arrow and Rectangle choices. Shape starts
unset, and canvas input without an explicit choice creates nothing. A choice
can be reused for later drags. Shape controls are disabled while busy or dragging.
Creation can start over empty canvas, atoms, bonds or graphics without editing
those targets. Existing Scene Add controls remain available.

Press captures document ID/revision, shape, observed service generation and first
point. The actual SVG inverse screen transform accounts for prior pan/zoom;
missing/singular/nonfinite transforms cannot locate an edit. Existing rounding
to0.01 pt and finite inclusive±100000 pt bounds apply. The stored point order is
press then release; arrow tip remains point2 and rectangles retain their ordered
opposite corners. No snapping, aspect lock, clamping or added minimum-area rule
is introduced. Existing domain support for reversed/coincident points remains,
while click/subthreshold gestures do not create an object.

At or above the existing 3-client-pixel drag threshold, movement paints a local
preview through the normal graphic renderer using the current document line
width. It writes no document state, history, revision or ID. On valid release,
the editor copies the frozen shape/points and queues exactly one `add_graphic`
without parent or index. The existing insertion rule creates a fresh page root;
selected groups never become an implicit parent. Existing objects, graph,
placements, stereo, settings, hierarchy and references remain intact apart from
the appended root/new graphic and ordinary transaction metadata.
After moving beyond the threshold and returning below it, the creation preview
is cleared immediately, while pointer capture remains active until release or
cancellation. A release one or two client pixels from press creates nothing;
crossing the threshold earlier is not a permanent commit latch.

The command shares existing atomic validation, fresh ID allocation, revision
checks, same-process replay, selection history and one-step Undo/Redo. The
selection at submission supplies the ordinary transaction-before snapshot; no
automatic graph/parent selection is inferred from the drawn shape. The callback
checks document/revision and service generation again at queue time, and owns
copied points so later caller edits cannot mutate its payload.

Space/middle-button pan takes priority at press. Escape, pointercancel, lost
capture, blur, Fit or Reset discard the preview and release pointer capture.
Changed document/revision, tool, shape or observed service generation invalidates
the press context. Selection alone is not the creation-context predicate and
does not choose a parent. If a service restart has not yet been observed, release
may send one request refused by the expired session; the creation is not
automatically replayed into the replacement process. Normal explicit reload
reconciliation applies. Cancelled/stale/invalid releases store no partial object.

Native document/draft stays v25, using existing graphic and history state plus
the existing save/reopen/recovery paths. Clipboard/interchange loss policies and
resource limits remain. There is no new graphic or external roundtrip guarantee.
No shapes beyond line/arrow/rectangle, curves, reaction/electron-arrow meaning,
images, clipping, layers, style controls, automatic layout or group-insertion
gesture are added.

Authored checks inspected, not successful execution evidence:

- `scripts/web-graphic-creation-client-check.mjs` covers shape and exact copied
  ordered points, no parent/index inference, stale document/revision, changed
  shape/service and existing point bounds/degenerate support.
- `scripts/web-graphic-creation-check.mjs` uses the production controls, canvas
  and editor against a real backend. It covers explicit initial choice, zoomed
  previews over existing atoms/bonds/graphics with zero writes, one release
  transaction/fresh page root, exact retained graph/objects, history/save/reopen,
  threshold and cancellation, Space/middle pan, tool/shape/revision/document
  changes, queue staleness/payload copying, restart no-replay and the actual
  workspace Graphic toolbar route.
- Return-path additions in the creation browser script cover all three shapes
  after a larger excursion and a return to one/two client pixels from press:
  preview gone before release, capture retained, exact document retained and no
  writes before or after release. The endpoint browser script separately covers
  both endpoint returns, ordinary body-move returns without changing the existing
  selection/commit policy, and handles following the painted whole-object move
  then returning on Escape. These are focused regression scopes, not proof that
  the entire general smoke has completed.
- The client joins `test:client`; `test:browser:graphic-creation` is registered in
  npm and CI. Existing endpoint/numeric-form/caption/Et and general browser
  regressions remain separate integration checks, not proof from registration.

## Review findings and reproduced return paths

The parent reports actual reproduction of all three defects below before their
fixes. This documentation pass inspected the committed changes and regressions;
it did not rerun those reproductions or infer successful post-fix/full execution
from the presence of a test. The independent review raised the creation-preview
finding; endpoint and body-move/handle behavior were related follow-up findings.

| Defect | Fix and exact added boundary |
| --- | --- |
| A creation preview remained at its last distant position after the pointer returned below the drag threshold, although release there creates nothing. | `a6c2f10` clears the `graphic-create` preview on a subthreshold return without cancelling pointer ownership. The creation browser regression exercises line/arrow/rectangle and one/two-client-pixel returns, with no document mutation or request. |
| Endpoint editing had the analogous stale distant preview on return. | `b630a3b` extends the clearing rule to `endpoint`. Both endpoint return paths restore stored geometry before release, retain capture and send no transaction. |
| Ordinary body movement also retained a distant preview; endpoint handles stayed at stored positions rather than following the moved stroke. | `5864f84` includes ordinary `move` in subthreshold clearing and shares `translatedGraphicPoints` between the painted graphic and eligible single-graphic handles. Added body-return checks retain ordinary selection/commit behavior; handle checks follow the preview delta and restore both coordinates on Escape, without writes. |

These fixes change transient canvas presentation, not domain geometry rules,
selection semantics, commit thresholds, native v25 or scientific interpretation.
The movement clearing rule also applies to the existing ordinary move gesture;
the newly added focused browser assertions above use graphic body movement.
Broader integration behavior remains subject to the pending general smoke.

Verification status: preceding P36b [#206](https://github.com/EunhakChoi/chemifree/pull/206)
merged as `76016170fe68fb5ad188d04ed26153cdcdc911ba` from final `f36550d`.
Its [PR CI42324595907](https://github.com/EunhakChoi/chemifree/actions/runs/42324595907)
passed check/browser; [post-merge main run42325796411](https://github.com/EunhakChoi/chemifree/actions/runs/42325796411)
was pending in the last parent handoff at that time (historical: it later succeeded, run 42325796411). P36c full-check results, remaining
Codex review/final disposition recording and PR CI were separate pending gates then; P36c has since merged as #207 (`afe6410`), whose main run 42329036170 failed in the browser job, with #209 following (see the [P36 status table](../HANDOFF.ko.md)).
The independent finding and related follow-up fixes are recorded above rather
than described as an unstarted review; no repeat independent round is implied. The full general smoke is currently running and
is not recorded as passed. This documentation work reran no product
tests, builds or browser checks and made no P36c merge claim at that time (historical: merged as #207, see the delivery follow-up above) and still claims neither complete P36 delivery nor
external-renderer equivalence.
