# P35p explicit retained-graph fragment display candidate

Current evidence: this is an implementation-session record. Its pending execution/review wording is historical: merged #201/#202 and passing final-head check/browser CI are reconciled in [the P35 completion audit draft](p35-completion-audit.md). The #203 script-run follow-up is now merged with passing final-head PR CI; post-merge main CI and the final P35 state change remain pending there. No checks were rerun for this documentation draft.

This candidate extends display contraction to explicitly selected existing fragment
members. It does not define a chemical nickname language. `contract_fragment`
requires instance, anchor, members and text; `set_fragment_display_text` requires
instance, anchor and string text; `expand_fragment` reveals the stored graph.
Text is one nonempty Unicode line, at most 128 UTF-8 bytes, with a non-whitespace
character and no control/line separator. Text is preserved without normalization
or chemical parsing; null is invalid and no dictionary/reset meaning is inferred.

The supported shape has exactly one explicitly named placement, 1–2000 unique
existing members including the anchor, subject to the document atom limit.
Members are neutral natural-abundance elements with inferred hydrogen, no radical
or attachment points. Their induced graph is a connected acyclic tree with only
unmarked single internal bonds and exactly one unmarked single external boundary
at the anchor. Nondefault/query states, rings, disconnected members, multiple
boundaries/placements and overlap with Et or another fragment display refuse.
Marks, stored centers/double configurations and stereo-group members touching a
member or its immediate outside neighbor refuse; unrelated stereo is retained.
Shape refusals use `fragment_display_invalid`, invalid strings use
`display_label_invalid`, and malformed wire input uses `invalid_request`.

Display text does not define chemistry. No pseudo atom or caption is introduced;
graph IDs, atom/bond states, coordinates and stereo remain original. Formula,
mass, identity, substructure and other scientific inspections use the full graph.
Atomic transactions, no-op, request replay, revision checks and Undo/Redo follow
the existing path. Whole-member rigid motion retains the display. Partial member
geometry or affected chemistry/local stereo/placement changes reveal it in the
same transaction. Expansion performs no layout and reveals stored coordinates.
Moving only the outside boundary neighbor's coordinates retains the display, as
for Et. Changes to that neighbor's atom state, the boundary bond or local stereo
still reveal it; this is separate from a partial move of retained members.

The actual panel asks for member selection, text and explicit anchor, displays
retained IDs and the chemistry warning, and provides text editing and expansion.
The canvas paints one plain label at the anchor, hides member/internal-bond ink
and hit targets, and selects/moves the retained member union. Document chemical
label typography and actual SVG bounds feed shared trim/fit/hit/selection. Domain
arrangement bounds continue to use the full stored geometry.

Native document/draft v24 requires `fragment_displays` in current and every
history change set, with complete nullable before/after states. Duplicate keys
or members, invalid text/membership/overlap and invalid replay are unreadable.
Versions 1–23 forbid the new field even when empty or history-only; migration
adds empty maps/diffs and retains all prior graph/caption/Et/label states. Reads
write nothing. First replacement/recovery archives exact old document/draft bytes
under the existing protocol. Future v25 refuses untouched. Member lists and
escaped text participate in existing current48MiB/history8MiB admission; physical
provider files remain limited to64MiB. Clipboard/template versions stay unchanged.

MOL/SDF, SMILES and bounded CDXML/CDX exports write the complete underlying graph
and report `abbreviation_display` rendering loss for each display. That display
loss alone needs no consent; existing unrelated loss-consent rules still apply.
Native graph clipboard, Cut preparation and template export refuse member
intersections with `clipboard_abbreviation_display_unsupported`; the UI tells the
user to expand first.

Authored/in-progress regression scope includes domain shape/text validation,
atomic refusal, chemistry invariance, no-op and reveal/Undo/Redo; application
commands/snapshot/replay, scientific graph use and export losses; native current,
legacy/future and history-only strictness, exact-byte document/draft archival;
client response validation and selection/rendering; HTTP/CLI parity and actual
production browser contraction, text edit, expansion, history, save/reopen,
scientific warnings and measured label bounds. These are intended checks, not
executed evidence. The parent workflow owns final test registration and results.

Execution status: this documentation pass read source and inspected diffs only.
Tests, builds, cargo check, browser and quality gates have not been run for this
candidate. Required Codex and fresh independent subagent PR reviews, findings and
dispositions are pending. Main P35a–n merged PR/CI evidence in the completion audit
does not establish this candidate's verification. P35 remains `in_progress`.
General nickname dictionaries, text-to-graph parsing, general multistyle chemical
labels, external label roundtrip and renderer equivalence remain unsupported.
