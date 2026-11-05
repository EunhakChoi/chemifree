# P32 bounded selection and geometry completion

Bounded P32 is complete through [PR #161](https://github.com/EunhakChoi/chemifree/pull/161), merged as `2c7a51847cd316b73a2324cb8a8152f95ee3ecc3`. Its [final PR CI](https://github.com/EunhakChoi/chemifree/actions/runs/42115749139) and [merge main CI](https://github.com/EunhakChoi/chemifree/actions/runs/42117006940) passed. This completion documentation PR still awaits its own review and CI; those future results are not claimed here. Overall OPS-01/OPS-02 remain partial.

## Scope and acceptance

The roadmap requires box/multiple selection, grouping/ungrouping, rotation/reflection, alignment/distribution and reporting the actual affected parent/child and atom/bond scope. Acceptance requires preventing duplicate movement, broken references and damage to unselected objects, with atomic Undo/Redo.

| Requirement | Implementation and executable checks |
| --- | --- |
| Box/multiple selection and placement context | P32e canvas Shift toggles, inclusive marquee and captured mixed drag; `scripts/web-client-check.mjs`; browser result `mixed_marquee_placement_and_group_drag` |
| Group/ungroup and references | Existing scene commands and Scene controls; `conformance/cases/scene-tree-history.json`, `scene-references.json`; browser `scene_group_and_queued_delete_selection`, `scene_reference_owner_and_target_delete` |
| Actual affected scope and movement once | `document.selection_scope` / `move_selection`; `crates/domain/tests/selection_scope.rs`, `crates/application/tests/selection_scope.rs`; `selection-scope-union-move-history` / `selection-scope-refusal-atomic` cases |
| Six alignments and two distributions | `document.arrangement_plan` / `arrange_objects`; domain/application `object_arrangement.rs`; `object-arrangement-plan-history` / `object-arrangement-refusal-atomic` cases; browser `scene_geometry_controls` |
| Rotation/reflection, mixed scope and stored state | `document.transform_plan` / `transform_selection`; domain/application `selection_transform.rs`; `selection-transform-plan-history` / `selection-transform-refusal-atomic` cases; browser `mixed_transform_scope_history_and_queue` |
| Atomic failure, references, unselected positions, no-op, revision and Undo/Redo | The above domain/application and conformance checks assert complete rollback, deduplication, independent placements, retained references and chemistry, history selection, and preserved redo on no-op |

The table maps requirements to implementation/check locations; execution evidence is recorded separately below. P32d/f web controls resolve captured targets against the latest queued document and send an explicit geometry-center pivot; the authoritative server validates and applies the command. Stored stereo, marks and enhanced groups remain unchanged; changed drawing geometry can disagree with stored stereo and can be inspected with `chem.stereo`.

## Documented bounds

Alignment/distribution operate on outermost whole scene objects. Marquee uses active-placement atom centers and complete two-point graphic geometry. Explicit graph targets use one active placement; selected scene placements retain their own positions. Web transforms provide quarter turns and horizontal/vertical axis reflection at the geometry center. The shared API supports explicit pivots and arbitrary atom/line/arrow rotations, while rectangles require exact quarter turns. Geometry bounds exclude font-dependent labels, strokes and arrowheads.

Partial-atom alignment, lasso/edge-intersection selection, cross-placement raw graph selection, custom-angle/pivot web controls, diagonal reflection, snapping and cleanup remain outside this delivery. These limits are not additional acceptance requirements for the P32 roadmap row. Broader OPS-01/OPS-02 requirements remain partial.

## Verification evidence

The recorded clean isolated suite28 contains 105/105 passed cases (77 accept, 28 reject, zero failed/not_run), including the six P32a/b/c cases. Earlier implementation a542abc passed 373 client checks, 105/105 conformance, strict routes and no-skips browser. Center followups ab26cc1 and final eeb6f7a were subsequently verified; the final run below includes the single-edit stability guard and documentation fix.

| Evidence | Confirmed result |
| --- | --- |
| Final implementation head | eeb6f7a11465f50de09c7897911a7af10b2500a1 |
| npm run check | Passed at eeb6f7a: 373 client tests, 105/105 conformance, traceability and the full configured check chain |
| npm run test:browser:routes | Passed: 0/1000 stalled |
| CHEMIFREE_SMOKE_NO_SKIPS=1 npm run test:browser | Passed: skipped_steps=[], mixed_marquee_placement_and_group_drag=true, mixed_transform_scope_history_and_queue=true |
| Isolation | Shared sandbox lock, private copy, MemoryMax=6G, swap=0; final peak anonymous memory 1462 MiB, page cache 123 MiB |
| Focused mutation evidence | Isolated midpoint-oscillation mutation and removal of the final stability guard each caused their focused regression to fail |
| Reviews | Per-PR findings and dispositions below; #161 independent review had no actionable findings and its Codex handoff finding was fixed |
| Implementation PR/main CI | #161 final PR run 42115749139 and merge main run 42117006940 passed |
| Completion documentation PR | Review and CI pending; no future documentation merge/main-CI result is claimed |

Retain the earlier P32e browser failures and their dispositions: placement controls changed fitted scale, exposing Chromium text bounding-box quantization; fitted text measurements now have a one-CSS-pixel allowance through the actual SVG CTM, while model/geometry checks stay strict and a separate 100% text probe keeps the original containment tolerance. Its reset readiness accepts binary64 roundoff near scale one. The successful no-skips browser run includes these fixes, and the final implementation PR/main CI also passed.

The original point-sized one-ULP and midpoint-tie fixes merged in #159 as 545c09ddecc667b67d19049031428b70b1daa805. The later union-center finding required #161: center plans materialize the proposed rounded bounds in preflight, recompute their union center and refuse as arrangement_center_unstable if any unit would move on repetition. The bounded algorithm applies no hidden iterations. Both-axis exact reproductions assert preview/command refusal, prior-command rollback and preserved geometry, revision, selection, history and redo. Genuine one-ULP improvements and stable midpoint ties retain their tests.

## Review and merge record

The records below combine GitHub review timelines with independent local-review reports in the linked PR bodies. A bot summary that still says unresolved is not rewritten as a clean review merely because fixes were committed.

| Slice and merge commit | Review findings and disposition |
| --- | --- |
| [P32a #152](https://github.com/EunhakChoi/chemifree/pull/152), `facc5dad5e4d6e4b5a832ff0fc56b95bf56864c3` | Initial Codex review of a255864 and independent review reported no findings. A later [handoff-staleness P2](https://github.com/EunhakChoi/chemifree/pull/152#discussion_r4484597848) is addressed by the current handoff separating executed local/server evidence from this documentation PR’s pending CI. The latest bot summary retained an unresolved flag; this is an evidenced documentation disposition, not a claim the bot flag cleared. |
| [P32b #154](https://github.com/EunhakChoi/chemifree/pull/154), `33447372f6845e70f5e89c1d80c3f026b8b2620a` | Independent review found decimal no-op drift and false negative-gap refusal; both were fixed with regressions. Codex [edge-cancellation P2](https://github.com/EunhakChoi/chemifree/pull/154#discussion_r4484806033) was fixed by anchored translation. The late [one-ULP center suppression P2](https://github.com/EunhakChoi/chemifree/pull/154#discussion_r4485009471) remained in the merge, was rediscovered in the completion audit, and was fixed by #159 with both-axis point/midpoint/repeated-no-op regressions. |
| [P32c #155](https://github.com/EunhakChoi/chemifree/pull/155), `34bdb6b98a8793771fbd446a9e6cc9aacc820218` | Independent and Codex reviews found [affine-tail rounding](https://github.com/EunhakChoi/chemifree/pull/155#discussion_r4485056495); shared nearest-exact summation and both-axis/sign regression fixed it. Codex [handoff finding](https://github.com/EunhakChoi/chemifree/pull/155#discussion_r4485056513) was also corrected. Latest summary at b38eb03 reports no unresolved findings. |
| [P32d #156](https://github.com/EunhakChoi/chemifree/pull/156), `8162ee65bdef54b72829d461082f9adec456e3b6` | Independent review of 9bdb947 reported no actionable findings. Codex [mixed-selection Group regression](https://github.com/EunhakChoi/chemifree/pull/156#discussion_r4485221730) was fixed by retaining scene-only grouping and adding regression/mutation coverage. Latest summary at a3610de reports no unresolved findings. |
| [P32e #157](https://github.com/EunhakChoi/chemifree/pull/157), `c0969627bb74b64875edd418e951a85077224367` | Independent review reported no actionable issues. Codex [stale public status P1](https://github.com/EunhakChoi/chemifree/pull/157#discussion_r4485429816) was corrected in README/handoff/traceability. Latest summary at c4a8533 reports no unresolved findings. Browser failures and their measurement/selector fixes are retained above and in the PR record. |
| [P32f #158](https://github.com/EunhakChoi/chemifree/pull/158), `316295d441521f119c88c637ba78df1b3e0e3510` | Independent review and Codex [status-source P2](https://github.com/EunhakChoi/chemifree/pull/158#discussion_r4485610752) found stale mixed-transform restrictions; current docs distinguish historical d/e scope from f. Latest summary at 24dd39e reports no unresolved findings. |
| [P32-center #159](https://github.com/EunhakChoi/chemifree/pull/159), `545c09ddecc667b67d19049031428b70b1daa805` | e0108e2 fixed point-sized one-ULP suppression. Codex [midpoint oscillation P2](https://github.com/EunhakChoi/chemifree/pull/159#discussion_r4486205477) was fixed in ab26cc1: compare exact midpoint error of actually rounded candidate bounds, retaining equal/worse poses; both-axis stable-repeat/Undo/Redo regression and isolated mutation passed. The subsequent [post-union-center drift P2](https://github.com/EunhakChoi/chemifree/pull/159#discussion_r4486304783), reviewed on ab26cc1, was addressed by #161 with post-state stability preflight and atomic refusal regression. The #161 independent review covers that followup. |
| [P32-settle #161](https://github.com/EunhakChoi/chemifree/pull/161), 2c7a51847cd316b73a2324cb8a8152f95ee3ecc3 | Independent subagent review found no actionable issues. Codex [handoff-refusal P2](https://github.com/EunhakChoi/chemifree/pull/161#discussion_r4486447786) was corrected in eeb6f7a, including related traceability/roadmap summaries. The [final Codex summary](https://github.com/EunhakChoi/chemifree/pull/161#issuecomment-6670310957) reports no unresolved findings at eeb6f7a. The #159 post-union-center finding is addressed by the bounded stability guard and verified refusal regression. |

## Traceability and delivery boundary

OPS-01.scene-controls, OPS-01.canvas-selection and OPS-01.mixed-selection-transform are implemented with valid check:test:client evidence, supported by the actual browser run recorded above. At P32 completion there were 104 portions: 57 implemented, 12 partial, 2 unverified and 33 not implemented; later feature slices update the current aggregate in HANDOFF and TRACEABILITY. Broader OPS-01/OPS-02 remain partial; catch-all followups no longer assign work outside the bounded scope to completed P32.

README, HANDOFF, TRACEABILITY, requirement evidence and roadmap JSON/embedded HTML record this bounded implementation completion. The implementation merge and main CI are confirmed; the completion documentation PR itself still needs review and CI. No scientific completeness, full rendering equivalence or support for the excluded controls is claimed.

The completion PR's [Codex status-consistency finding](https://github.com/EunhakChoi/chemifree/pull/162#discussion_r4486749422) identified old P32b/c summaries and a P31 paragraph that still described P32 as pending. Those current references now distinguish the historical server-only slices from the delivered bounded web behavior; HANDOFF's followup list and the roadmap's P31 note were reconciled as well. This correction does not claim completion of this documentation PR's remaining review/CI gates.
