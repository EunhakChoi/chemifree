# P36c explicit canvas graphic-creation candidate

Source/test inspection covers the P36c working tree based on `f36550d`. This
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
- The client joins `test:client`; `test:browser:graphic-creation` is registered in
  npm and CI. Existing endpoint/numeric-form/caption/Et and general browser
  regressions remain separate integration checks, not proof from registration.

Verification status: preceding P36b [#206](https://github.com/EunhakChoi/chemifree/pull/206)
merged as `76016170fe68fb5ad188d04ed26153cdcdc911ba` from final `f36550d`.
Its [PR CI42324595907](https://github.com/EunhakChoi/chemifree/actions/runs/42324595907)
passed check/browser; [post-merge main run42325796411](https://github.com/EunhakChoi/chemifree/actions/runs/42325796411)
remains pending in the parent handoff. P36c full checks, Codex and independent
fresh-context review/finding dispositions, and PR CI are separate pending gates.
Existing browser regression is in
progress and is not recorded as passed. This documentation work reran no product
tests, builds or browser checks and claims no P36c merge, complete P36 delivery or
external-renderer equivalence.
