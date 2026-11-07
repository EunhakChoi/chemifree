# P36e styled graphics clipboard candidate

Source inspection covers web `c9cdf4cf` and Rust v3 implementation/test commit
`30ebb34c`. This pass ran no Rust, browser or full checks. No
confirmed execution/review/CI result for P36e is supplied here; those gates
remain pending. P36 stays `in_progress` against its broader unchanged scope.

This candidate extends existing graphic forest Copy/Cut/Paste with literal
nullable line-width preservation. It supersedes P36d's v2-only policy that refused
any selected explicit-width graphic and asked for reset before copying. That
earlier behavior remains historical in the P36d record, not a current prerequisite
for eligible v3 transport. The historical refusal diagnostic remains in the
domain enum for compatibility; ordinary eligible overrides no longer trigger it.

Copy chooses its version from included graphics only:

- All inherited: emit v2 with the existing graphic fields and no width field.
- Any explicit override: emit v3 with required `line_width_pt` on every graphic,
  including null for inherited siblings. An explicit value equal to document
  width still counts as an override. Groups gain no width field, and widths
  outside the selected forest do not select its version.

V2 and v3 readers are strict. V2 rejects a width field even when null. V3 requires
one on every graphic, accepting null or a finite number within0.1–10 pt and
refusing omission, wrong types, nonfinite/out-of-range values, unknown/duplicate
fields and invalid trees/references. Valid unsupported format/version/kind headers
retain `clipboard_format_unsupported`; malformed known-profile data retains
`clipboard_fragment_invalid` and the existing resource/semantic refusals.
Graph clipboard v1 remains a separate profile.

An all-null v3 forest is valid input to Paste. Copying the resulting inherited
forest emits v2. This is distinct from validating a current Copy reply: the
client expects that writer to choose canonical v2 for all inheritance and v3
for any override. No silent override removal or downgrade makes styled v3
readable by a v2-only consumer. Fully inherited forests retain the v2 output
shape; this establishes no external consumer compatibility.

Paste preserves each explicit numeric value and inherited null through ordered
geometry, translated offsets, fresh IDs, nested groups, internal references and
selected-root history. Exact destination document-settings equality remains
required, including document line width. An inherited pasted graphic continues
following the document; a pasted override remains independent of later document
width changes. Other objects/overrides and chemical graph/placements/stereo are
not modified. Native document/draft stays v26 with its existing migrations and
strictness; this slice adds no native format or document setting.

Cut keeps the existing successful-write-before-delete rule, exact prepared
source/selection/process checks, outside incoming-reference refusal, immutable
uncertainty gate and selection-aware Undo/Redo. Copy alone never deletes. Failed
or stale requests retain the source/destination under ordinary transaction
atomicity, revision/replay and ID-allocation rules. Width travels within the
graphic state, without an extra set-width command per graphic. The64KiB payload,
500-command, coordinate and scene-resource limits remain; extra v3 metadata can
reach the same byte boundary sooner than v2.

The dialogs show each strictly decoded graphic's inherited/override width and
state that Copy/Cut preserves explicit values. Invalid text does not produce a
valid style preview. The width form's Use document width button remains an
optional explicit inheritance edit, not a transport requirement. Reset may
change appearance and never occurs implicitly during Copy/Cut/Paste. Clipboard
activation/permission/manual fallback and restart reconciliation rules are
unchanged; no OS/external-application compatibility claim follows from a harness.

Authored/inspected verification scope, not passed execution evidence:

- Domain `scene_clipboard_v3.rs`: mixed inherited/explicit forests, immutable copy,
  offsets/refs/IDs/graph, bounds/settings failure, Cut rollback and style Undo.
- Application `scene_clipboard_v3.rs`: v3 preservation/replay/history/native26,
  canonical inherited v2, all-null v3 input and recopy, field/version/bounds/byte
  strictness and no deletion before the Cut transaction.
- API/CLI `scene_clipboard_v3.rs`: shared wire paste/copy/replay/Undo/Redo and
  v2 field-smuggling refusal. Provider `scene_clipboard_v3.rs`: native26 save and
  recovery/history without rewriting the source.
- `web-styled-clipboard-client-check.mjs`: strict parser/style lists, writer
  version choice, all-null input versus Copy reply, exact nullable mapping and
  graph/outside-object/ref tamper detection.
- Existing real-backend clipboard integration and scene-Cut browser scripts are
  extended for styled copying/pasting/cutting and invalid style display/refusal;
  width browser guidance is updated. Registration and adapted older fixtures
  do not establish successful browser/full execution.

Colour/dash/fill/arrowhead settings, captions/instances in forests, external
graphics transports and broader P36 completion remain outside this candidate.
P36d's separately reported targeted Rust result is not evidence that these new
v3 cases passed. Current Rust/browser/full results, both review dispositions,
PR/main CI and delivery status require the parent workflow's confirmed evidence.
