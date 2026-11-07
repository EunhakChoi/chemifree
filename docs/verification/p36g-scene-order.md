# P36g same-parent scene order boundary candidate

Delivery follow-up (checked with `gh pr view` and `gh run list`, 2026-11-07 UTC): P36g merged as #212 (`6545af6`) and its main CI [run 42339112423](https://github.com/EunhakChoi/chemifree/actions/runs/42339112423) succeeded. Statements below that call checks, reviews, CI or the PR pending or open describe the implementation session and are historical; see the P36 status table in [the handoff](../HANDOFF.ko.md). P36 remains `in_progress`.

The current [PR #212](https://github.com/EunhakChoi/chemifree/pull/212) candidate
head is `25d440c0`, based on main `d1920be8` (merged P36f #211). Its completed
frontend checks and independent review are recorded below; full isolated checks
and current PR CI remain pending. This documentation update read existing
execution logs and PR metadata and ran no product tests/builds or browser checks.
P36 stays `in_progress` against its broader scope; #212 was not recorded as merged when this was written (historical: merged as `6545af6`, see the delivery follow-up above).

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

Scope exercised by the frontend checks recorded below:

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
  registered in npm/CI. The local executions below establish their stated web
  scope, not successful full isolated checks or GitHub CI.

## Completed frontend execution and review evidence

The following existing frontend-agent logs under `/tmp/chemifree-work/` were
read for this documentation correction. They are local execution records, not
new runs by the documentation agent or committed CI artifacts. The PR body
independently records the same completed frontend scope.

| Local log | Confirmed result and scope |
| --- | --- |
| `p36g-web-gate.log` | Isolated web lint, typecheck, complete client command and web build passed; sandbox exit 0. Every recorded client group has zero failures, including all 9 scene-order client cases. |
| `p36g-order-browser.log` | Real-backend production Scene First/Last/browser scenario passed; sandbox exit 0. It covers root/group/subtree order and DOM painting sequence, queued siblings/selection, parent/document/deletion staleness, no-op/history and native persistence. |
| `p36g-web-regression.log` | Selection-rotation browser regression and general browser smoke passed; sandbox exit 0. General smoke records `skipped_steps: []` and no uncaught browser errors. This is not a full Rust/conformance/traceability execution. |
| `p36g-mutation-2.log` | Semantic mutations produced expected failures for parent/document/index/no-op checks and the browser's captured-target assertion (`s2` versus captured `s3`). Restored source then passed all 9 client cases and the production scene-order browser, ending with sandbox exit 0. The intentional mutant failures are distinct from the restored implementation's passing results. |

An independent fresh-context subagent reviewed
`3667f852b2f2efde5b305b107a5e413f57911a2f` on 2026-11-07 UTC and reported no
findings, as recorded in the #212 body. That reviewed head is identified
separately from the current rebased PR head; no new independent review of this
documentation correction is implied.

Codex P2 on #212 identified this record's stale claim that already completed
frontend gates/browser checks and review were still unexecuted or in progress.
This update reconciles that finding using the named logs and current PR body.
It changes only this verification document and reruns none of those checks.

Full isolated checks and current GitHub CI remain pending in the parent/PR
handoff. No success for those gates or merged #212 delivery followed from the
completed web logs at that time (historical; see the delivery follow-up above). Existing bounded P32/P33 delivery and merged predecessor
#211 are not reclassified, and broader P36 remains `in_progress`.
