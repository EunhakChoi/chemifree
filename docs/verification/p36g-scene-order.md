# P36g same-parent scene order boundary candidate

Source inspection covers the current scene-order web draft based on `ea9b2f1f`.
P36f PR #211 remains open in the parent handoff; its presence in branch ancestry
does not establish a merge. Browser verification is in progress, while browser/
full/review/CI results remain pending. This documentation pass ran no product
tests/builds or browser checks. P36 stays `in_progress` against its broader scope.

Scene First/Last moves a selected scene object directly to the beginning/end of
its existing parent's sibling list, reducing repeated Earlier/Later presses.
First is earlier in ordinary scene painting order and Last later within that
parent. This is existing scene-tree order, without a separate layer, global
z-index, visibility/lock/blend system or reparenting operation. Existing editor
overlays retain their own renderer policy.

Eligibility counts selected scene objects, not total selected IDs. Exactly one
scene object can coexist with valid atom/bond selections; those graph IDs remain
ordinary selection/history, not order targets. No/graph-only/multiple scene-object
selection disables boundary actions. Selecting ancestor and descendant together
is not reduced to an outermost unit. Existing scene kinds keep their ordinary
reorder eligibility; this is not limited to a graphic pair of points.

Page roots are siblings under null parent; group children are siblings under that
group. A group is one subtree target and moves without flattening/reversing its
internal child order or changing descendant geometry/style/IDs/references. Other
sibling relative order stays intact. Only the owning sibling list and canonical
depth-first traversal sequence change. Containment membership, object state,
graph/placements/stereo, per-graphic width and document/label/caption settings
are not changed by reorder.

Press captures document ID, object ID, parent and First/Last intent, plus the
normal gesture selection-before snapshot. No revision, geometry or sibling list
is frozen. At queue time, the helper rechecks that document/object/parent and
membership, then derives index 0 or latest sibling count minus one. Prior
same-parent additions/removals/reorders can change the boundary. Later live
selection does not redirect the target or overwrite the ordinary live-selection
adoption policy; valid accompanying graph selection is not reordered or cleared
by this order command.

The visible boundary button is disabled when already satisfied; queue-time
identity also skips an action when an earlier edit reached the boundary. A
singleton list needs no edit. Different document, deleted target, changed parent,
missing/non-group parent or missing sibling membership lapses without substituting
another target/index. A root grouped while waiting does not follow into that
group; an ungrouped child does not follow into its new parent. Geometry/style
changes alone are not a revision-freeze refusal.

One effective action sends existing `reorder { object, index }` under the latest
revision. The public API continues validating existing targets and an index
within its sibling list; transaction atomicity, replay, selection-aware Undo/Redo
and save/reopen are unchanged. The ordinary mutation/restart reconciliation path
applies without a service-generation capture or special no-replay exception.
No new in-flight restart guarantee follows from this UI.

API/catalog/native document/draft v26/provider formats and clipboard/external
loss policies remain unchanged. This adds no batch/multi-object boundary command,
new grouping/reparent control, layer model, geometry edit or scientific meaning.

Authored/inspected checks, not successful execution evidence:

- `web-scene-order-client-check.mjs`: one scene object with accompanying graph
  IDs, root/nested group boundaries, subtree target, no-op, latest siblings after
  addition/prior reorder, changed document/deletion/reparent/parent membership.
- `web-scene-order-check.mjs`: real-backend production buttons/Scene panel,
  roots/nested subtrees and DOM painting sequence, exact order-only changes,
  normal selection history/Undo/Redo, no-op/no writes, latest queued siblings,
  live-selection preservation, earlier boundary fulfillment, grouping/ungrouping/
  deletion/document staleness and native26 save/restart/reopen. Its restart step
  tests persisted order, not a new unknown-request replay policy.
- Client registration joins `test:client`; `test:browser:scene-order` is
  registered in npm/CI. Written assertions or registration do not establish the
  currently pending browser/full/review/CI outcome.

Existing bounded P32/P33 delivery is not reclassified by this candidate. P36f
#211 integration remains separate and open; this record claims neither that
merge nor successful P36g delivery or completion of broader P36.
