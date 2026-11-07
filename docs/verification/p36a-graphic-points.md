# P36a existing graphic point-editing candidate

Implementation-session follow-up (historical):

Follow-up: this P36a record describes the numeric-form implementation session. [P36b](p36b-graphic-endpoint-drag.md) adds single-graphic canvas endpoint dragging; its verification is separate. P36a PR #204 merge remains pending in the parent handoff, and this documentation update claims no merge or successful P36b full checks/reviews/CI.

Delivery follow-up: P36a PR #204 merged as26962b695ab2924370aecb63f0e231485e1cda04. Its post-merge main CI run [42323974348](https://github.com/EunhakChoi/chemifree/actions/runs/42323974348) later completed with success (checked with `gh run list`). The implementation-session verification notes below are historical. P36 remains in_progress, and no checks were rerun for this documentation rebase.

This bounded candidate adds `update_graphic_points` for an existing line, arrow
or rectangle. It requires a concrete graphic object ID and exactly two `{x,y}`
points in document pt (1/72 inch, x right and y down). Each coordinate must be
finite and within inclusive±100000 pt. Reversed or coincident points remain
allowed as in existing creation. Point order is preserved: an arrow's second
point remains its tip, and rectangles retain their stored opposite-corner order.
No new object, graphic shape or chemical interpretation is introduced.

The command changes only the target graphic's points. Shape, object ID, parent,
group membership, roots, sibling/child order, incoming/outgoing references and
other objects are unchanged, as are graph atoms/bonds, placement coordinates,
stereo, settings and ID counters. Graphic arrows do not automatically assert a
reaction role or electron flow. Group movement and existing object transforms
continue to use the edited geometry under their existing contracts.

Missing/malformed fields, null/wrong-type coordinates, point counts other than
two and extra fields such as shape are `invalid_request`. Out-of-range coordinates
are `invalid_command`; unknown objects or non-graphic targets are
`invalid_command` with `object_unknown` or `object_kind_mismatch`. Captions, groups
and molecule instances refuse. There is no `$N` target support. A failed command
rolls back the complete transaction, including earlier updates and allocated IDs.
Identical points are a no-op without revision/history growth. Effective edits
share revision gates, same-process replay, selection-aware atomic Undo/Redo and
the ordinary HTTP/CLI/operation contract 1 transaction path.

The web Graphic endpoints panel shows four pt fields for one selected scene
graphic, including a grouped child. Numeric parsing accepts trimmed decimal and
scientific notation but refuses blank, unit-suffixed, hex/binary, nonfinite and
out-of-range input. Invalid, unchanged, busy or IME-composing input cannot submit.
Submission captures document ID/revision, the exact graphic snapshot and copied
points. Changed revision/document, target geometry/group membership, removal or
kind replacement lapses a queued edit before posting. An unknown endpoint edit
is not automatically replayed into a restarted process. Rendering uses the same
existing line/arrow/rectangle canvas path.

Native document/draft remains v25 with existing scene-object and history shapes.
There is no new schema or migration; the usual save/reopen/recovery mechanisms
carry the updated existing object state. Existing clipboard and external-format
loss policies remain unchanged. This is no new graphic roundtrip or interchange
support claim.

Authored check scope, not execution evidence:

- `crates/domain/tests/graphic_points.rs` covers all three shapes, inclusive
  coordinate boundaries, reversed/coincident points, graph/tree/reference/ID
  retention, no-op, one-step selection-aware Undo/Redo, wrong/missing target and
  rollback including earlier ID allocation.
- `crates/application/tests/graphic_points.rs` covers public malformed fields,
  point count/type/bounds/target refusal, atomicity, replay/no-op, save/reopen and
  native history with retained grouping/references/graph.
- `scripts/web-graphic-points-client-check.mjs` covers numeric parsing, command
  capture and point copying, no-op, and queued document/revision/target changes.
- `scripts/web-graphic-points-check.mjs` drives the production form/editor against
  the real backend, checking invalid/IME submissions, actual line/arrow/rectangle
  coordinates, reversed/coincident geometry, stale queue refusal, group movement,
  history and native save/reopen. `npm run test:browser:graphic-points` is
  registered in npm and CI; the client script joins `npm run test:client`.

No resize or endpoint drag handles, new shape types, curves, reaction/electron
arrows, scientific glyphs, images, clipping, layers, graphic style controls or
automatic layout are added. P36 remains `in_progress` against its broader
unchanged roadmap scope and acceptance.

Status: source inspected at Rust `0650f7a` and web `66eac34`. This documentation
work ran no tests, builds, browser checks or quality gates. Execution verification,
Codex and independent fresh-context reviews with finding dispositions, and PR CI
remain pending the parent workflow. No scientific or external-renderer equivalence
is claimed.
