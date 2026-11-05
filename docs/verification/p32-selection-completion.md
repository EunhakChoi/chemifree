# P32 bounded selection and geometry delivery: pending completion

P32 remains `in_progress`. Local verification of implementation commit `a542abc` passed as recorded below. The late P32b one-ULP and P32-center midpoint-tie findings were fixed and verified through ab26cc1. A still later Codex finding on ab26cc1 concerns post-translation union-center drift and remains open; see the review record below. The feature scope is covered, while this correctness finding and final delivery gates remain open.

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

## Recorded evidence and pending gates

The existing checked-in `conformance-report.json` and handoff record clean isolated suite28 with 105/105 passed (77 accept, 28 reject, zero failed/not_run), including the six P32a/b/c cases. In addition, the coordinating maintainer confirmed fresh isolated local verification of implementation commit `a542abc`: `npm run check` passed with 373 client tests, 105/105 conformance and traceability; strict route and no-skips browser checks also passed. This is local execution evidence, not PR/main CI evidence.

| Gate | Current record |
| --- | --- |
| Latest locally verified fix | `ab26cc1f1a05d4fa890fb052ac001149bef9d371`; documentation finalization is separate |
| `npm run check` | Passed at `a542abc`: 373 client tests,105/105 conformance,traceability; full isolated check passed again at `ab26cc1` |
| `npm run test:browser:routes` | Passed again after `ab26cc1`: 0/1000 stalled |
| `CHEMIFREE_SMOKE_NO_SKIPS=1 npm run test:browser` | Passed again after `ab26cc1`: `skipped_steps=[]`, `mixed_marquee_placement_and_group_drag=true`, `mixed_transform_scope_history_and_queue=true` |
| Execution isolation | Shared sandbox lock and private copy, MemoryMax=6G, swap=0; latest reported peak anonymous memory1702 MiB and page cache96 MiB (earlier a542abc run:1760/74 MiB) |
| Codex review and finding dispositions | Consolidated below; original P32b and midpoint-tie findings fixed; latest post-union drift finding remains open |
| Independent fresh-context subagent review and dispositions | Per-PR records consolidated below; local reviews are reported in PR bodies |
| Final P32f PR CI | [Run 42107718782](https://github.com/EunhakChoi/chemifree/actions/runs/42107718782) at `a542abcc738765b2c2bea48c0fe73ab6d2fd9923`: completed, success |
| P32f merge and main CI | PR #158 merged as `316295d441521f119c88c637ba78df1b3e0e3510`; [main run 42109700751](https://github.com/EunhakChoi/chemifree/actions/runs/42109700751) awaits coordinating maintainer confirmation; P32 is not `done` |

Retain the earlier P32e browser failures and their dispositions: placement controls changed fitted scale, exposing Chromium text bounding-box quantization; fitted text measurements now have a one-CSS-pixel allowance through the actual SVG CTM, while model/geometry checks stay strict and a separate 100% text probe keeps the original containment tolerance. Its reset readiness accepts binary64 roundoff near scale one. The successful no-skips local browser run above includes these fixes; Main CI confirmation remains pending.

The coordinating maintainer also confirmed an isolated mutation restored the midpoint oscillation and caused its regression to fail. PR #159 passed PR CI and merged as `545c09ddecc667b67d19049031428b70b1daa805`; [its main CI42113776746](https://github.com/EunhakChoi/chemifree/actions/runs/42113776746) remains pending confirmation. Passing checks do not dispose of the newly discovered post-union-center finding.

## Review and merge record

The records below combine GitHub review timelines with independent local-review reports in the linked PR bodies. A bot summary that still says unresolved is not rewritten as a clean review merely because fixes were committed.

| Slice and merge commit | Review findings and disposition |
| --- | --- |
| [P32a #152](https://github.com/EunhakChoi/chemifree/pull/152), `facc5dad5e4d6e4b5a832ff0fc56b95bf56864c3` | Initial Codex review of a255864 and independent review reported no findings. A later [handoff-staleness P2](https://github.com/EunhakChoi/chemifree/pull/152#discussion_r4484597848) is addressed by the current handoff separating executed local/server evidence from pending delivery. The latest bot summary retained an unresolved flag; this is an evidenced documentation disposition, not a claim the bot flag cleared. |
| [P32b #154](https://github.com/EunhakChoi/chemifree/pull/154), `33447372f6845e70f5e89c1d80c3f026b8b2620a` | Independent review found decimal no-op drift and false negative-gap refusal; both were fixed with regressions. Codex [edge-cancellation P2](https://github.com/EunhakChoi/chemifree/pull/154#discussion_r4484806033) was fixed by anchored translation. The late [one-ULP center suppression P2](https://github.com/EunhakChoi/chemifree/pull/154#discussion_r4485009471) remained in the merge, was rediscovered in the completion audit, and was fixed by #159 with both-axis point/midpoint/repeated-no-op regressions. |
| [P32c #155](https://github.com/EunhakChoi/chemifree/pull/155), `34bdb6b98a8793771fbd446a9e6cc9aacc820218` | Independent and Codex reviews found [affine-tail rounding](https://github.com/EunhakChoi/chemifree/pull/155#discussion_r4485056495); shared nearest-exact summation and both-axis/sign regression fixed it. Codex [handoff finding](https://github.com/EunhakChoi/chemifree/pull/155#discussion_r4485056513) was also corrected. Latest summary at b38eb03 reports no unresolved findings. |
| [P32d #156](https://github.com/EunhakChoi/chemifree/pull/156), `8162ee65bdef54b72829d461082f9adec456e3b6` | Independent review of9bdb947 reported no actionable findings. Codex [mixed-selection Group regression](https://github.com/EunhakChoi/chemifree/pull/156#discussion_r4485221730) was fixed by retaining scene-only grouping and adding regression/mutation coverage. Latest summary at a3610de reports no unresolved findings. |
| [P32e #157](https://github.com/EunhakChoi/chemifree/pull/157), `c0969627bb74b64875edd418e951a85077224367` | Independent review reported no actionable issues. Codex [stale public status P1](https://github.com/EunhakChoi/chemifree/pull/157#discussion_r4485429816) was corrected in README/handoff/traceability. Latest summary at c4a8533 reports no unresolved findings. Browser failures and their measurement/selector fixes are retained above and in the PR record. |
| [P32f #158](https://github.com/EunhakChoi/chemifree/pull/158), `316295d441521f119c88c637ba78df1b3e0e3510` | Independent review and Codex [status-source P2](https://github.com/EunhakChoi/chemifree/pull/158#discussion_r4485610752) found stale mixed-transform restrictions; current docs distinguish historical d/e scope from f. Latest summary at24dd39e reports no unresolved findings. |
| [P32-center #159](https://github.com/EunhakChoi/chemifree/pull/159), `545c09ddecc667b67d19049031428b70b1daa805` | e0108e2 fixed point-sized one-ULP suppression. Codex [midpoint oscillation P2](https://github.com/EunhakChoi/chemifree/pull/159#discussion_r4486205477) was fixed in ab26cc1: compare exact midpoint error of actually rounded candidate bounds, retaining equal/worse poses; both-axis stable-repeat/Undo/Redo regression and isolated mutation passed. The subsequent [post-union-center drift P2](https://github.com/EunhakChoi/chemifree/pull/159#discussion_r4486304783), reviewed on ab26cc1, remains unresolved and requires a further fix and regression. Independent review details for this followup remain to be consolidated. |

## Finalization after confirmation

Replace remaining pending rows with actual commit-bound results and links, preserving failures and their fixes. The local client/browser gates now pass; promotion and structured evidence for `OPS-01.scene-controls`, `OPS-01.canvas-selection` and `OPS-01.mixed-selection-transform` remain for coordinated finalization. The present totals remain 104 portions: 54 implemented, 12 partial, 5 unverified and 33 not implemented. Promoting exactly those three would yield 57 implemented and 2 unverified; recalculate from the final traceability file rather than treating that projection as the current status.

Update README, HANDOFF, TRACEABILITY, requirements evidence and roadmap JSON plus embedded HTML together. Keep overall OPS-01/OPS-02 partial. Once required delivery evidence is confirmed, record bounded P32 completion and remove P32 from catch-all followups for work explicitly outside its scope; retain appropriate later followups. Mark the roadmap row `done` only with the required merge/main-CI record.
