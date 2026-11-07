# P36f finite arbitrary-angle rotation UI candidate

Delivery follow-up (checked with `gh pr view` and `gh run list`, 2026-11-07 UTC): P36f merged as #211 (`d1920be`) and its main CI [run 42337029831](https://github.com/EunhakChoi/chemifree/actions/runs/42337029831) succeeded. Statements below that call checks, reviews, CI or the PR pending or open describe the implementation session and are historical; see the P36 status table in [the handoff](../HANDOFF.ko.md). P36 remains `in_progress`.

Source inspection covers the current web draft based on `6cb71791`, especially
`selection-rotation.ts`, its form and existing selection/queue code. The browser
check is currently in progress. This documentation pass ran no tests/builds or
browser checks; tests, reviews and PR CI remain pending without claimed success.
P36 remains `in_progress` against its broader unchanged roadmap scope.

The Scene form adds a finite arbitrary-angle input beside the existing quarter
turn/reflection controls. It accepts trimmed decimal/scientific numbers without
a separate magnitude limit; blank, NaN/Infinity, overflow, hex, unit-suffixed or
comma-formatted input cannot submit. Positive degrees rotate clockwise in page
coordinates and negative counterclockwise. The angle draft is transient and
resets on document change; it is not a persisted setting.

The requested number remains unchanged in `transform_selection`. Existing domain
floating-point modulo360, exact quarter-turn and general trig behavior apply.
The UI sends no command for a modulo identity, including the domain-compatible
tiny-negative case rounding to360; it adds no epsilon or magnitude clamp.
Other tiny/large finite values remain eligible and an actually unchanged backend
result retains ordinary no-op/redo policy. IME composition blocks both the button
and direct form submission. The form can queue behind earlier edits rather than
freezing all input solely because a request is busy.

Submission captures raw selected object/atom/bond IDs, document ID, active
placement and numeric angle. It does not capture expected revision or pivot.
The callback resolves the captured IDs in the latest queued document, deduplicates
the effective geometry union and computes its current bounds center before
submitting under that snapshot's revision. Prior movement may change the pivot;
later live selection/placement/input changes cannot retarget the action. Normal
gesture selection-before history uses the captured IDs. The input angle itself
does not change while queued.

Bounds contain atom positions and graphic defining points, excluding labels,
stroke/arrowhead ink and reference traversal. Scene-only targets need no active
graph placement; explicit atoms/bond endpoints retain the captured placement.
Missing captured targets or required placement, different document, empty
geometry or caption-containing scope lapse the complete action without silently
shrinking the selection or choosing another placement. Repeated valid rotations
compose against latest queued geometry, as the fixed controls already do.

Caption scopes, including grouped descendants, block the UI without a request.
Rectangle scopes deliberately reach the existing backend: non-quarter-turn
rotation yields atomic `transform_rectangle_unsupported`, without transforming
the supported remainder or replacing a rectangle by a bounding box. Exact
quarter turns and supported axes retain their existing semantics. Final coordinate
bounds and API validation remain authoritative; identity/empty API requests do
not bypass target/angle/caption validation merely because the UI filters its own
identity submissions.

The form uses the existing shared mutation/restart reconciliation path. Unlike
graphic creation/endpoint/width edits, it introduces no special no-replay rule.
This is not a promise that a rotation is never retried after service restart;
ordinary shared reconciliation determines whether a rebuilt request is allowed.
No new restart guarantee is inferred from the focused browser script.

The existing read-only `document.transform_plan` remains available to API users;
this form directly queues a transaction and adds no preview call, canvas rotation
gesture or custom pivot. Public API/DTO/catalog, native document/draft v26 and
providers remain unchanged. Geometry-only graph/stored stereo/mark, hierarchy,
reference, shape/line-width and style invariants remain. Transformed drawing and
stored stereo can disagree; existing stereo inspection reports that boundary.

Authored/inspected scope, not executed success evidence:

- `web-selection-rotation-client-check.mjs`: finite parsing through large values,
  invalid input, raw mixed angle/placement, latest pivot after prior movement,
  missing target/placement/document lapse, identity and tiny-angle behavior,
  caption descendants and backend-owned rectangle refusal.
- `web-selection-rotation-check.mjs`: real-backend production form/canvas and
  workspace route, invalid/IME/identity, positive/negative/multi-turn/large angles,
  one-step history, geometry/stereo/style retention, caption no-write and atomic
  rectangle refusal, queued target/placement/pivot, deletion lapse, document-draft
  reset and native26 save/reopen. This is not a new restart matrix.
- Client registration joins `test:client`, and
  `test:browser:selection-rotation` is registered in npm/CI. Registration and
  authored assertions do not establish their current results or full CI success.

Custom-pivot/diagonal-reflection controls, snapping, caption rotation/reflection, arbitrary
rectangle rotation, layout/cleanup and chemical mirror interpretation remain
outside this slice. Existing bounded P32 completion is historical delivery;
this additional P36f UI candidate does not expand that old verification claim or
complete broader P36.
