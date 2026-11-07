# P36e styled graphics clipboard candidate

Original source inspection covered web `c9cdf4cf` and Rust v3 implementation/test
commit `30ebb34c`. This update inspects the duplicate-key fix `d1717d8b` (the
rebased equivalent of original `5bfcafa`) and settings fix `c51c957`. Known
independent/Codex findings and agent-reported focused results are recorded below.
This documentation pass ran no Rust, browser or full checks. Full checks and
CI remain pending; P36 stays `in_progress` against its broader unchanged scope.

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

The client parses JSON grammar and rejects duplicate decoded keys before using
the payload for a style list or successful Copy-reply check. Escaped spellings
that decode to the same key are duplicates, including nested coordinate/settings
keys and identical repeated values. The scan uses an iterative scope stack under
the existing caller byte limit; it does not relax scene-tree depth limits.

For both v2/v3, missing `label_weight`/`label_style` default to normal and missing
`label_underline`/`label_outline`/`label_shadow` to false, matching legacy serde
defaults. Then the shared complete `isDocumentSettings` validation checks all
supported fields, enums, numeric ranges and types. Missing base fields, unknown
settings and explicit invalid/null values refuse rather than gaining defaults.
Even an equally malformed expected-settings object cannot authorize a successful
Copy reply. These preview corrections align with an already strict service;
they add no new wire, native26 or graph/settings semantics.

Inspected verification scope; execution results are distinguished below:

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
  graph/outside-object/ref tamper detection; duplicate decoded keys at different
  scopes and v2/v3 invalid settings, legitimate boundaries and legacy omissions.
- Existing real-backend clipboard integration and scene-Cut browser scripts are
  extended for styled copying/pasting/cutting and invalid style display/refusal;
  width browser guidance is updated. Registration and adapted older fixtures
  do not establish successful browser/full execution.

## PR #210 review findings and reported execution

| Finding | Disposition and inspected regression scope |
| --- | --- |
| Independent P3: duplicate decoded JSON keys could show a last-value-wins width preview although the service refuses the payload. | Original `5bfcafa`, now `d1717d8b` in this branch, introduces `parseStrictJson` and uses it before summaries/reply checks. Their stable patch IDs match. Tests cover conflicting/identical/escaped width keys, nested coordinates/settings, valid escaping/punctuation and independent nested scopes; the real-backend clipboard script includes duplicate-width rejection without an ambiguous style list. |
| Codex P2: a payload with invalid document settings could still display a graphic style preview. | `c51c957` applies only the five legacy label omission defaults, then the shared complete settings validator. Tests exercise both v2/v3 missing base fields, unknown fields, malformed enums/types/nulls/ranges, valid boundary values and legitimate legacy omissions. Summary and Copy-reply validation share that gate. |

For [PR #210](https://github.com/EunhakChoi/chemifree/pull/210), the parent relays
the assigned agent's execution report: `baseline50` failed,
`fix81` passed, and sandbox lint/typecheck/client/build passed. These are reported
focused reproduction/fix and sandbox results, not runs by this documentation
agent and not successful full-project or GitHub CI evidence. The source and
regressions above were inspected here; the documentation pass did not reproduce
the failures or rerun the passing gates. Full checks and PR/main CI remain
pending, and no merged P36e delivery or repeat review round is asserted.

Colour/dash/fill/arrowhead settings, captions/instances in forests, external
graphics transports and broader P36 completion remain outside this candidate.
P36d's separately reported targeted Rust result is not evidence that these new
v3 cases passed. Final full execution/CI and delivery status require the parent
workflow's confirmed evidence; the two known findings have the fixes above.
