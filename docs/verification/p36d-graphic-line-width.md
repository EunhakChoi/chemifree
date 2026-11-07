# P36d per-graphic line-width candidate

Compatibility follow-up: [P36e](p36e-styled-graphics-clipboard.md) adds candidate v3 transport preserving explicit widths. The v2-only subtree refusal and mandatory pre-copy reset below describe the historical P36d policy, not the current P36e source. V2 stays strict/all-inherited output; v3 has its own pending verification and does not change native v26. Use document width is now an optional inheritance edit, not a Copy/Cut prerequisite.

Source inspection covers Rust production `e6e3b45` and the concurrently edited
web draft. The parent reports27 targeted Rust tests passed for that production
change. This documentation pass did not execute them; web/full checks, reviews
and PR CI remained pending when this was written (historical: P36d has since merged as #208, `53e7437`, with main CI run 42333431290 success; see the [P36 status table](../HANDOFF.ko.md)). P36 remains `in_progress` against its broader scope.

`set_graphic_line_width` requires a concrete existing line/arrow/rectangle object
and required nullable `line_width_pt`. Null dynamically inherits document width;
an explicit width must be finite within inclusive0.1–10 pt. Missing/wrong-type
fields refuse as `invalid_request`; out-of-range widths use `invalid_command`
with `line_width_out_of_range`, and absent/non-graphic targets retain existing
typed object errors. A refused edit rolls back the complete transaction,
including earlier edits/allocations. Nonfinite values are not an admitted width.

An explicit number equal to the current document width is still an override,
does not follow later document changes and still triggers clipboard refusal.
Resetting that number to null is an undoable metadata change even if today's
effective width is equal. Identical stored states are no-ops. `add_graphic`
continues creating inherited graphics and gains no new width input. Snapshots
expose the nullable field; older snapshots can omit it as inherited. This API
compatibility does not relax strict native version admission.

Only graphic width changes. Shape, points, object/graph IDs, root/child order,
parents/groups, references, selection, other objects, graph states/coordinates,
stereo and document/caption/label styles are preserved. Points edits, ordinary
movement and supported transforms retain overrides. Shared revision/replay,
selection-aware history, Undo/Redo, save/reopen/recovery paths carry the state.
This is graphic appearance, without chemical or reaction/electron semantics.

The single-explicit-graphic form shows inherited/override and effective width,
accepts decimal/scientific numeric input, interprets blank as null and offers
Use document width. Busy/invalid/IME/unchanged input cannot submit; exact captured
document/revision/graphic validation lapses stale queued edits. Restart handling
does not automatically replay a width edit into another process. The renderer
uses effective width for graphic and arrowhead-outline strokes, selection
underlay and transparent hit stroke. Fixed arrowhead dimensions and point-based
arrangement/fit geometry keep their existing policies; stroke-aware layout,
new hit geometry types or general graphic style controls are not claimed.

Native document/draft v26 requires nullable width on every current graphic and
non-null before/after historical graphic state. Versions1–25 forbid it even when
null, history-only or present only in a discarded old history record, and
migrate graphic states to inheritance. Current invalid/missing values and invalid
width history beyond retention limits refuse before it can be hidden by trimming.
Reads write nothing. Existing exact-byte document/draft archives precede first
replacement/recovery, now including v25. Future v27 refuses untouched. Existing
256-byte graphic reservations and64MiB provider cap remain; the provider fixture
exercises near-maximal graphic/history populations with long float lexemes for
document and draft encodings within the existing byte-admission/persistence
contracts.

Historical P36d-only clipboard policy (superseded by candidate P36e v3):
graphic forest v2 remains unchanged and cannot carry explicit widths.
Copy/Cut preparation traverses the whole selected forest and refuses if any
graphic, including a descendant, has an override with typed reason
`clipboard_scene_graphic_line_width_unsupported`. No subset is copied/deleted and
no width is silently stripped. Document-width-equal overrides also refuse.
The UI asks for an explicit Use document width reset before copying and warns
that appearance then follows document width. Unselected outside overrides stay
intact. Inherited missing/null snapshot normalization is limited to compatibility;
it does not admit a per-graphic width field to the strict v2 payload or silently
drop an override. Chemical exports keep existing `scene_object` rendering loss
for omitted graphics and do not claim styled-graphic roundtrip.

Authored/inspected verification scope:

- Domain `graphic_line_width.rs`: inherited/explicit semantics, equal-value
  override identity, null reset, no-op, invalid bounds/targets/atomicity, graph/
  scene/reference/history invariants, geometry preservation, clipboard descendants
  and invalid current/discarded width history.
- Application `graphic_line_width.rs`: public command errors, replay/reset,
  save/reopen/history, graph/scene retention and v2 refusal then explicit reset.
- Provider `graphic_line_width_format.rs`: current/history/draft strictness,
  v25 inheritance migration and exact archives, old-header field smuggling,
  pre-retention validation and physical reservation fixtures.
- `web-graphic-line-width-client-check.mjs`: parsing, additive snapshot validation,
  override identity, captured target/revision, strict clipboard behavior and
  normalization of inherited snapshots without changing outside overrides.
- Draft `web-graphic-line-width-check.mjs`: real-backend form/IME/no-op/reset,
  inheritance versus document edits, rendered stroke/selection/hit at zoom,
  endpoint/move/transform retention, queue/restart guards and native26 reopen.
  npm/CI registration and adapted existing graphic fixtures are source, not
  evidence that the browser/full matrices have passed.

Only the parent-reported27 targeted Rust cases are currently recorded as passed;
the list above is broader intended regression scope and is not a count of that
run. Web/full results, Codex/independent review findings/dispositions and current
PR CI remain pending. Preceding P36c #207 merged `afe6410`; its
[post-merge main run42329036170](https://github.com/EunhakChoi/chemifree/actions/runs/42329036170)
later completed with failure in the browser job (checked with `gh run list`); #209 followed. P36d itself merged as #208 (`53e7437`) with successful main run 42333431290. This documentation work reran no
product tests/builds/browser checks and does not claim full P36
completion (P36d itself merged as #208, see above). Colour/dash/fill/arrowhead controls, general graphic styles/images,
clipping/layers/layout and external equivalence remain outside this slice.
