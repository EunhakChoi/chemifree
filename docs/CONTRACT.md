# Chemifree local API v1

Bounded P35 is done against the roadmap editing/storage scope under the approved graph-preserving display decision. Caption/chemical typography, editable Et text (#201), explicit retained-fragment displays (#202) and script runs (#203) are merged with passing final-head PR check/browser CI. #203 merge `84ef063` also passed [post-merge main CI 42322504482](https://github.com/EunhakChoi/chemifree/actions/runs/42322504482). The [completion audit](verification/p35-completion-audit.md) records the acceptance evidence, actual Codex/independent reviews and finding fixes. Implementation-session notes marked historical below describe what was unrun then; they are superseded by that delivery evidence. General scientific/rendering requirements and the documented unsupported behavior remain incomplete. This documentation correction reran no product checks.


Rust DTOs are shared by HTTP and the CLI in `crates/application/src/` (`lib.rs`, `contract.rs`, `error.rs`). The browser keeps a hand-maintained mirror with runtime validation in `apps/web/src/lib/contract.ts` and `apps/web/src/lib/document-contract.ts`. There is no generated SDK.

Every JSON response includes `api_version: "v1"`; HTTP paths carry the same major version.

## Entry points

| HTTP | CLI | Response |
| --- | --- | --- |
| `GET /api/v1/health` | `chemifree status --json` | Health |
| `GET /api/v1/capabilities` | `chemifree capabilities --json` | Capabilities |
| `GET /api/v1/data` | `chemifree data status --json` | External data status |
| `GET /api/v1/session` | — | Write token for this API process |
| `GET /api/v1/documents?query=Q&sort=title\|recent` | `chemifree documents list [--query Q] [--sort title\|recent] --json` | Document list |
| `POST /api/v1/documents` | `chemifree documents create [--title T] --json` | Document (HTTP 201) |
| `GET /api/v1/documents/{id}` | `chemifree documents show ID --json` | Document |
| `POST /api/v1/documents/{id}/transactions` | `chemifree documents apply ID --expected-revision N --commands JSON` | Document |
| `POST /api/v1/documents/{id}/undo` | `chemifree documents undo ID --expected-revision N` | Document |
| `POST /api/v1/documents/{id}/redo` | `chemifree documents redo ID --expected-revision N` | Document |
| `POST /api/v1/documents/{id}/save` | (every CLI edit saves) | Document |
| `POST /api/v1/documents/{id}/close` | — | Close |
| `POST /api/v1/documents/{id}/autosave` | — (CLI edits save) | Document |
| `POST /api/v1/documents/{id}/save-as` | `chemifree documents save-as ID --expected-revision N [--title T]` | Copy (HTTP 201) |
| `POST /api/v1/documents/{id}/recover` | `chemifree documents recover ID` (recovers, then saves) | Document |
| `POST /api/v1/documents/{id}/recover-as-copy` | `chemifree documents recover-as-copy ID [--title T]` | Copy (HTTP 201) |
| `POST /api/v1/documents/{id}/discard-draft` | `chemifree documents discard-draft ID` | Draft discard |
| `POST /api/v1/documents/{id}/rename` | `chemifree documents rename ID --expected-revision N --title T` | Document |
| `POST /api/v1/documents/{id}/delete` | `chemifree documents delete ID --expected-revision N` | Delete |
| `GET /api/v1/trash` | `chemifree documents trash --json` | Trash list |
| `POST /api/v1/trash/{id}/restore` | `chemifree documents restore ID` | Restore |
| `POST /api/v1/operations` | `chemifree operations` (one request per line on stdin) | Operation reply (see Operation contract 1) |

The CLI calls the same `Application` in-process and does not contact HTTP. Both use `--data-dir` / `CHEMIFREE_DATA_DIR` (default: the platform user-data directory, e.g. `~/.local/share/chemifree`). `status`, `capabilities`, and `show`/`list` do not write. A CLI process is its own short session, so `apply`, `undo`, and `redo` commit with a save before reporting success; if the save fails, nothing is committed. With `--json`, failures are printed to stdout as the error envelope and the exit status is 1.

## Inspection

Health: `{ "api_version", "service": "chemifree", "version", "stage": "scaffold", "status": "ok" }`. `ok` is handler liveness only. `stage` remains `scaffold`; most product capabilities are still unimplemented.

Capabilities: `{ api_version, capabilities: [{ id, label, status, description }], operation_catalog }`, `status` ∈ `available | not_implemented`. Identifiers are unique; consumers tolerate new ones. `operation_catalog` (added within v1) is the catalog that `capabilities.inspect` returns (see Operation contract 1); a service from before it leaves it out, and the web client then says that per-operation support is not reported.

| ID | Status |
| --- | --- |
| `runtime_inspection` | `available` |
| `document_editing` | `available` — drawing model only |
| `structure_analysis` | `available` — the explicitly labeled bounded valence inspection only; operation catalog specifies other operations |
| `format_conversion` | `available` — only with at least one format codec attached; its label names the attached formats, for example "MOL/SDF, CDXML, CDX, SMILES and SVG subsets" with all three codecs or "MOL/SDF and SVG subsets" with MOL/SDF alone (bounded V2000 and V3000 MOL/SDF, rich CDXML, bounded CDX, bounded OpenSMILES, and the bounded SVG drawing export, P38a, partial, export only); `not_implemented` without a codec |
| `local_model_loading` | `not_implemented` |
| `persistence` | `available` — local Chemifree JSON files |
| `authentication` | `not_implemented` |
| `windows_bridge` | `not_implemented` |

## Operation contract 1

One request and reply shape for every operation, named and versioned apart from the HTTP routes above. HTTP and the CLI pass the request bytes to the same `Application::execute`, so both give the same reply, error codes included. Rust types: `crates/application/src/operations.rs`; vocabulary (operation names, profiles, representation, execution): `crates/domain/src/operation.rs`.

Request: `{ "contract": 1, "request_id", "profile", "operation", "parameters": {} }`. All five fields are required and no others are allowed. `profile` is `preserve-semantics` (keep document meaning; report what is lost) or `legacy-observed` (reproduce another program's observed behavior: nothing runs under it except `capabilities.inspect`; every other operation is answered `unsupported` with the diagnostic `profile_unavailable`). There is no default profile.

Reply: `{ envelope, contract, request_id, operation, profile, outcome, document_revision, result, model_id, diagnostics, loss_report }`.

- `envelope` is `operation_result`, or `protocol_error` when the request could not be read as an operation (`profile` is then null, and `request_id` and `operation` are null when they could not be read).
- `outcome`: `ok` (it ran; this says nothing about chemical correctness), `partial` (it ran and `loss_report` lists losses or unresolved interpretation), `error`, or `unsupported`. `result` is null unless the outcome is `ok` or `partial`. `ok` is never paired with a `dropped`, `rejected` or `unknown` loss entry, and `partial` is never paired with an empty `loss_report`; a test checks this on every reply path that reports stereo (`chem.identity`, `chem.substructure`, the MOL/SDF import and export, the SMILES export, and the reply of `document.transaction`, which is `partial` exactly when its `notes` are not empty).
- `document_revision`: the document's revision after a document operation that succeeded, or the current one with a revision conflict; null otherwise (no document involved, `document.close`, or a failure that reports no revision).
- `model_id`: the model or method that produced the result; null when no model was used.
- `diagnostics`: `[{ code, severity, message, path, details }]`, `severity` ∈ `info | warning | error`. `path` points into the request (`parameters.commands[2]`) or is null.
- `loss_report`: `[{ feature, source_path, disposition, reason, semantic_impact }]`, `disposition` ∈ `retained | normalized | dropped | rejected | unknown`, `semantic_impact` ∈ `none | chemistry | document | geometry | stereo | rendering | unknown`. Format conversions report losses and uncertainty by source path, including the record when applicable. Property calculations report conditional abundance omissions and exact-mass fallbacks here.

| Operation | Parameters | Result |
| --- | --- | --- |
| `capabilities.inspect` | `{}` | The catalog below |
| `document.create` | `{ title? }` | `{ document, session, recovery }`, as in a v1 document reply |
| `document.snapshot` | `{ document_id }` | same |
| `document.arrangement_plan` | `{ document_id, objects, mode }` | Read-only checked rigid alignment/distribution plan with exact snapshot; [rules](#scene-object-alignment-and-distribution-p32b) |
| `document.transform_plan` | `{ document_id, selection, pivot, transform, instance? }` | Read-only checked geometry rotation/reflection with exact snapshot; [rules](#selection-rotation-and-reflection-p32c) |
| `document.selection_scope` | `{ document_id, selection, instance? }` | Read-only effective movement targets with exact snapshot; [rules](#effective-selection-scope-and-movement-p32a) |
| `document.clipboard_copy` | `{ document_id, selection, instance }` | Read-only native graph fragment, copied IDs, boundary bonds and exact snapshot; [rules](#native-graph-fragment-copy-and-paste-p33a) |
| `document.clipboard_paste` | `{ document_id, expected_revision, payload, offset, selection_before? }` | One atomic native paste with exact resulting snapshot and fresh atom/bond ID mapping; [rules](#native-graph-fragment-copy-and-paste-p33a) |
| `document.template_inspect` | `{ payload }` | Validated local atoms and explicit anchor eligibility; [P34e](#user-template-anchor-connection-p34e) |
| `document.template_plan` | Attachment input | Read-only candidate and exact source snapshot; [P34e](#user-template-anchor-connection-p34e) |
| `document.template_attach` | Attachment input | Atomic native template connection; [P34e](#user-template-anchor-connection-p34e) |
| `document.template_replace_plan` | Replacement input | Read-only bound-anchor candidate; [P34f](#terminal-template-replacement-p34f) |
| `document.template_replace` | Replacement input | Atomic terminal atom replacement preserving its identity; [P34f](#terminal-template-replacement-p34f) |
| `document.clipboard_scene_copy` | `{ document_id, selection }` | Read-only graphic/group forest with internal object references and exact source snapshot; [rules](#native-graphic-subtree-clipboard-backend-p33e) |
| `document.clipboard_scene_paste` | `{ document_id, expected_revision, payload, offset, selection_before? }` | Atomic graphic forest paste with fresh object mapping, root selection and preserved containment; [rules](#native-graphic-subtree-clipboard-backend-p33e) |
| `document.transaction` | `{ document_id, expected_revision, commands }` (v1 commands) | same; its `result` also carries `notes` (see [Stereo marks and configurations](#stereo-marks-and-configurations)), and `loss_report` repeats them |
| `history.undo`, `history.redo`, `document.save` | `{ document_id, expected_revision }` | same |
| `document.close` | `{ document_id, discard_unsaved?, expected_revision? }` | `{ document_id, discarded_changes }` |
| `document.import` | `{ document_id, expected_revision, format, source, record, preview?, preview_digest?, accept_loss? }` (`format` `mol`, `sdf`, `mol-v2000` or `mol-v3000`) | MOL/SDF (V2000 or V3000, detected per record) preview or one atomic document edit; see below |
| `document.export` | `{ format, records: [{ document_id, fields? }], ctab?, accepted_losses? }` (MOL/SDF; `format` `mol`, `sdf`, `mol-v2000` or `mol-v3000`); `{ format: "svg", document_id, accepted_losses? }` (SVG); `{ format: "smiles", document_id, style, accepted_losses?, time_limit_ms? }` (SMILES, [rules](#smiles); [time limit and cancel](#computation-time-limit-and-cancel)) | Staged MOL/SDF text (`ctab` `v2000` by default, `v3000` or `auto`), source-index maps and losses, with consent for stereo the file cannot carry; see below. SVG: `{ content, media_type, view_box, font_table, snapshot }` with `loss_report` and consent for the losses marked yes; see [SVG drawing export (P38a)](#svg-drawing-export-p38a) |
| `document.import`, `document.export` | [Rich CDXML/CDX or editor projection](#cdxml-interchange) | Rich tree, preview, created document, or CDXML text / CDX base64 content |
| `chem.validate` | `{ document_id }` | `{ snapshot: { document, session, recovery }, atoms: [{ atom_id, bond_order_sum, hydrogens, status, reason }] }`; [policy and limits](#valence-inspection) |
| `chem.properties` | `{ document_id, scope, property, selection?, data_registration_id?, time_limit_ms? }` | Formula, selected mass or distribution, the analyzed snapshot and calculation metadata; [policy and limits](#formula-and-mass-properties); [time limit and cancel](#computation-time-limit-and-cancel) |
| `chem.aromaticity` | `{ document_id, time_limit_ms? }` | The analyzed snapshot and cyclic systems under the policy below; [time limit and cancel](#computation-time-limit-and-cancel) |
| `chem.aromaticity_convert` | `{ document_id, expected_revision, to: "aromatic" \| "kekule", selection_before?, selection_after?, time_limit_ms? }` | The document after one undoable conversion; [time limit and cancel](#computation-time-limit-and-cancel) |
| `chem.identity` | `{ document_id, mode: "recorded" \| "chemical", other_document_id?, time_limit_ms? }` | Snapshot-bound canonical identity, symmetry and an optional graph comparison; [time limit and cancel](#computation-time-limit-and-cancel) |
| `chem.substructure` | `{ query_document_id, target_document_id, time_limit_ms? }` | Every atom and bond mapping of the query drawing in the target drawing, with both snapshots, or a refusal or limit; [rules](#substructure-matching); [time limit and cancel](#computation-time-limit-and-cancel) |
| `chem.stereo` | `{ document_id, instance? }` | The stored stereo configurations against what each scene instance's drawing shows, per center and double bond, with the exact snapshot and advisory findings; read-only; [rules](#stereo-inspection) |
| `chem.inchi` | `{ document_id, accepted_losses? }` | Optional registered Standard InChI and InChIKey with snapshot, provider/options, warnings and explicit loss consent; [rules](#standard-inchi-and-inchikey) |
| `chem.cip` | `{ document_id, scope?, atoms?, bonds?, time_limit_ms? }` | The CIP descriptors (R, S, E, Z) of the stored configurations, per center and double bond, with an explicit state and reason for every item that is not labeled, the exact snapshot and the rules applied (1a, 1b, 2, 3); read-only; [rules](#cip-labels); [time limit and cancel](#computation-time-limit-and-cancel) |

Unknown parameters are `invalid_request` (`path: "parameters"`). Every other operation name the catalog lists (`chem.name_to_structure`, `chem.structure_to_name`, `layout.structure`, `layout.reaction`, `render.scene`, `biopolymer.expand`, `prediction.evaluate`, `integration.invoke`, `script.evaluate`) is answered `outcome: "unsupported"` with `details: { execution, followup }`, whatever its parameters. Prediction uses the diagnostic `model_unavailable`; the other operations use `operation_not_implemented`: nothing is computed and no placeholder result is returned. Mutations retain the v1 request replay, revision and saving rules; inspection reads the current working snapshot. An operation that fails reports the v1 error code as its diagnostic (`revision_conflict`, `document_not_found`, `storage_failed`, …), with `details.current_revision`, `details.command_index`, and `details.reason` where the v1 envelope has them.

## Independent six-carbon ring insertion (P34a)

The `insert_ring` transaction command is `{ op: "insert_ring", instance, center: { x, y }, size?: 5 | 6, pattern?: "single" | "kekule" }`.
P34b adds size 5; omission and explicit 6 retain the original six-ring recipe and arithmetic.
Explicit `null`, strings, non-integers and any number outside 5/6 are `invalid_request`
without changing the document.
It appends five or six neutral natural-abundance carbon atoms (inferred hydrogens, no radicals or
attachment points) and the same number of bonds as one independent cycle. The default pattern
is all single bonds. P34g adds explicit `pattern:"kekule"` for six vertices only: the bonds
from vertex 0 to 1, 2 to 3, and 4 to 5 are double; the other three are single. Omitted size
still means 6; explicit `pattern:"single"` keeps the old recipe for either size. Null, unknown
patterns, and non-string patterns are `invalid_request`; size 5 with `kekule` is
`invalid_command` with `details.reason:"ring_pattern_invalid"`. The stated orders do not
assign aromaticity, configurations, bond marks or CIP labels. P34h adds the same explicit
pattern field to `attach_ring` below; its default remains all single. It runs through the same
Application transaction via HTTP, CLI `documents apply`, or operation contract 1; it is not a
separate format, provider or external DATA consumer. The command requires an existing explicit
instance and exactly one molecule placement. The current document `bond_length_pt` is the side
length. For six vertices, starting at the rightmost vertex, vertices run clockwise on the y-down page at offsets
`(L,0), (L/2,H), (-L/2,H), (-L,0), (-L/2,-H), (L/2,-H)` where `H=L*(sqrt(3)/2)`.
For five vertices the regular pentagon has circumradius `R=L/(2 sin(π/5))`;
vertex `k=0..4` is centered at `(R cos(2πk/5), R sin(2πk/5))` on the y-down page.
The Domain computes this fixed recipe with golden-ratio radicals; vertices 2 and 3
have y offsets `L/2` and `-L/2`, respectively.
All resulting coordinates must fit the existing point bounds.

Fresh atom/bond IDs use the ordinary allocators and are not reused after Undo. The recipe
creates no `$N` atom, bond or object reference; subsequent commands cannot name one of its
vertices implicitly. Selection follows ordinary transaction semantics: `selection_before`
records the captured prior selection, and without `selection_after` the surviving selection
is preserved. Existing chemistry, stereo, graphics, references, containment and settings are
unchanged. Any failure, including a late coordinate/ID/entity-limit failure, rolls back the
entire transaction and preserves selection, numbering, revision, dirty state and redo history.
The existing 500-command budget counts this as one bounded command. Save/reopen and ordinary
Undo/Redo retain its IDs and coordinates without a storage schema change.

The web Ring tool (R) offers a 5/6 size selector (initially 6, not saved in the document)
and, for six-carbon rings in either mode, an explicit All single bonds / Kekulé alternating bonds
selector (initially all single, not saved). Choosing size 5 or entering Attach to atom resets the
pattern to all single; choosing Kekulé explicitly in Attach is supported since P34h. The pattern is captured with the click and stays frozen through
queueing and lost-reply recovery; its control is disabled while busy or dragging. The client
checks each returned bond order, including the chosen alternating phase. The tool
sends the pointer-down center on a completed click through the inverse
SVG transform; dragging does not insert. A click over existing content never attaches, replaces
or fuses it. The queued action captures size/pattern/document/process/instance/selection and resolves the
current bond length when the transaction executes. A missing target, another document/process
or unsupported placement count refuses the captured action. An uncertain response holds later
edits and shows **Check ring insertion outcome**, reusing the same request/body/process.
Restart or a terminal revision conflict requires explicit reload/reconciliation and never
resends the insertion to a new process. Size controls are disabled while busy; later tool
choices cannot retarget an already captured attempt. Checking and history never access the OS clipboard.
Close and its discard confirmation are disabled while the editor is busy, including an
unresolved ring or clipboard mutation. Hook callers still pass the queue-time unresolved
attempt guard; closing or opening cannot remove recovery controls while an attempt is pending.
A Close queued behind an Open never redirects to the newly shown document: it keeps the
document ID and revision captured when Close was requested and lapses if that ID is no longer
shown. Normal clean Close and the existing unsaved-discard confirmation resume after settlement.

This is an independently defined 2D regular-ring drawing recipe, not conformation prediction,
aromaticity assignment, chemical layout or collision avoidance. Multiple placements, other ring
sizes beyond 5/6 or other atom/bond patterns, fusion, other substituent patterns,
abbreviation contraction/expansion and prepared legacy template framing activation remain
unsupported. Unknown command fields are refused. P34 remains **in_progress**. The written
checks have not been executed in this implementation session; see
[the P34a verification plan](verification/p34a-independent-ring.md),
[the P34b verification plan](verification/p34b-five-ring.md), and
[the P34g verification plan](verification/p34g-kekule-ring.md).

## Existing ethyl display contraction and expansion (P34i)

This bounded candidate implements the approved **real graph retained, display contracted**
policy. A transaction accepts `{ "op":"collapse_ethyl", "instance":"s1", "anchor":"a2", "terminal":"a3" }`
or `{ "op":"expand_ethyl", "instance":"s1", "anchor":"a2" }`.
There must be exactly one placement, explicitly named. The existing, distinct member atoms
must be neutral natural-abundance C with inferred hydrogens, no radical or attachment points;
they have one internal unmarked single bond and exactly one unmarked single boundary bond
from the anchor. The terminal has no other bond. Overlapping member sets refuse. Marks,
stored centers, stored double configurations or stereo-group members touching either member
or its immediate outside neighbor refuse contraction. Unrelated stereo remains supported.
A refused shape has `invalid_command`, reason `abbreviation_invalid`, and its command index;
malformed wire fields are `invalid_request`. No eligibility or valence correction is inferred.

Snapshots always contain `abbreviations: [{ instance, anchor, terminal, display_label }]`, empty when expanded.
`display_label` is the effective plain string (default `Et`); P35o defines its cosmetic editing below.
Earlier services may omit it. This separate display map neither replaces graph atoms nor
creates caption/scene objects or graph IDs. Contracting and expanding change no atom/bond
state, ID, coordinate, mark, stored configuration or group. Each effective action is one
ordinary Undo step; an identical collapse or already-expanded existing anchor is a no-op.
Selection before/after, request replay, revision gates and uncertain transaction recovery use
the normal edit path. Formulas, masses, identity, substructure and every chemical export use
the full underlying graph; no pseudo atom is introduced.

The canvas shows Et at the anchor, hides the terminal and internal bond from ink, underlays
and hit tests, and trims the external bond at the label box. Click/Shift-click/marquee use the
whole member pair and internal bond; a marquee around only the hidden terminal selects no
part of that display. Whole-pair drag translates both retained coordinates. Whole-union rigid
moves/transforms retain the display. Partial geometry moves expand it. An edit changing member
or boundary bonds, member/outside atom states, local marks/stereo, or placement multiplicity
removes affected display entries within that same transaction. Deletion likewise removes
invalid entries. Undo restores the prior graph, geometry and display together. Expanding
reveals the stored geometry verbatim; it performs no layout or collision avoidance.
Canvas label/view/hit extents use visible ink; domain arrangement/transform bounds continue to
use all stored atom positions under the existing geometry-only contract.

MOL/SDF, SMILES and bounded CDXML/CDX exports write the complete graph and existing coordinates
where the format supports them. Each Et display has a dropped `abbreviation_display` rendering
loss; that display loss alone needs no consent and changes no chemistry. Existing unrelated
loss-consent rules still apply. Native graph clipboard v1, native Cut preparation and native
template export refuse selections intersecting a compact member with
`clipboard_abbreviation_display_unsupported`; expand explicitly before those actions. Other
native fragments and chemical text export paths retain their existing rules.

The UI provides explicit anchor buttons for a selected pair and explicit Expand buttons.
This slice has no text-to-graph parser, automatic abbreviation detection,
provider/legacy-template activation, multi-boundary or nested abbreviation, rich-text Et,
multiple-placement contraction or layout engine. P34 remains in progress. See
[authorship/review and pending checks](verification/p34i-ethyl-display.md).

### Plain display labels on retained ethyl graphs (P35o)

`{ "op":"update_abbreviation_label", "instance":"s1", "anchor":"a2", "text":"임의 표기" }`
edits only an existing collapsed ethyl display. `text` is required: a string sets
the label and explicit `null` resets it to `Et`. The string is one nonempty Unicode
line, at most 128 UTF-8 bytes, with at least one non-whitespace character; control
characters and line separators are refused. Invalid text reports
`display_label_invalid`; a missing collapsed display reports `abbreviation_invalid`.
No text is parsed as an element, formula, nickname or fragment. **Text does not
define chemistry.** Even a chemically suggestive label leaves the two retained
carbon atoms, bonds, IDs, coordinates, recorded states, stereo, graph identity and
formula unchanged. The default remains `Et`; reset, Undo/Redo and request replay
share the ordinary transaction path, and an unchanged effective label is a no-op.

The web form states this distinction beside Apply and Reset controls. The canvas
uses the document's chemical label font, size, weight, style, underline, outline
and shadow for this single plain string. Natural SVG measurements feed the shared
bond trimming, fit, picking and selection bounds. It does not introduce per-run
styles, chemical interpretation, new abbreviation membership or fragment editing.

Chemical exports retain the full graph and report the existing
`abbreviation_display` rendering loss, including the custom label. Native graph
clipboard/Cut/template restrictions remain unchanged. Native document/draft v23
stores cosmetic `display_labels` separately from retained-graph membership and
includes complete before/after history; see the file format below. This P35o slice
does not define new fragment membership; P35p's explicit bounded membership is
described next. General multistyle atom/abbreviation labels, text-to-graph
parsing and external display-label roundtrip remain unsupported. This slice shipped
in [#201](https://github.com/EunhakChoi/chemifree/pull/201), with successful final-head
[check/browser CI 42311153640](https://github.com/EunhakChoi/chemifree/actions/runs/42311153640).
Its Codex and independent review dispositions are recorded in the
[completion audit](verification/p35-completion-audit.md#merged-delivery-and-ci).
Bounded P35 is done; [the P35o record](verification/p35o-display-label.md) preserves
its historical implementation-session observations separately from current delivery.

### Explicit retained-graph fragment display (P35p)

`{ "op":"contract_fragment", "instance":"s1", "anchor":"a2", "members":["a2","a3","a4"], "text":"iPr" }`
contracts an explicitly selected existing fragment. `set_fragment_display_text`
accepts the same explicit instance/anchor and required string `text` to edit its
display; `expand_fragment` accepts instance/anchor to reveal the retained graph.
Text follows P35o's nonempty single-line, 128 UTF-8 byte limit, without parsing or
normalization. Null is invalid; there is no default nickname or reset dictionary.
**Display text does not define chemistry.** Formula, mass, identity, substructure
and all scientific inspections continue to use every original atom and bond.

There must be exactly one explicitly named placement. Members are 1–2000 unique
existing atom IDs including the anchor, bounded further by the existing document
atom limit. Each is a neutral natural-abundance element with inferred hydrogens,
no radical and no attachment points. The induced member graph must be a connected
acyclic tree of unmarked single bonds, with exactly one external unmarked single
bond at the anchor and no other boundary. Rings, disconnected selections, query
atoms/bonds, nondefault member states, multiple boundaries/placements and overlap
with another fragment display or Et display refuse. Marks, stored center/double
stereo or stereo-group members touching a member or its outside neighbor refuse.
Unrelated stereo is unchanged. Unsupported shapes report `invalid_command` with
reason `fragment_display_invalid`; invalid text uses `display_label_invalid`,
and malformed wire fields use `invalid_request`. These are eligibility checks,
without valence correction or a claim that the displayed text names the graph.

Snapshots carry `fragment_displays: [{ instance, anchor, members, text }]`, empty
when absent; P35q additionally exposes runs as described next. Older service
snapshots may omit the field. Member IDs are stored
in identifier order. The separate display map changes no graph IDs, atom/bond
states, stereo or coordinates. Transactions share atomic rollback, revision gates,
request replay and Undo/Redo. An unchanged display is a no-op; expanding an
existing atom without a display is a no-op. Whole-member rigid moves/transforms
retain the display. Partial geometry or affected member/boundary chemistry,
local stereo or placement multiplicity changes reveal it in the same Undo step.
Moving only the outside boundary neighbor's coordinates retains the display,
matching the existing Et policy; changing that neighbor's atom state or boundary
bond/stereo still reveals it. Expansion reveals original geometry without layout.

The web panel requires explicit selected members, text and anchor, shows retained
atom IDs and the chemistry warning, and provides Apply and Expand controls.
The canvas hides member ink and internal bonds, paints the plain text at the
anchor using document chemical-label typography, and uses actual mounted SVG
bounds for trimming, fit, hit and selection. Selection and dragging include the
retained members and internal bonds; hidden members are not separate hit targets.
Domain arrangement bounds still include the stored coordinates.

MOL/SDF, SMILES and bounded CDXML/CDX exports keep the complete chemical graph
and report dropped `abbreviation_display` rendering loss for each display.
The display loss alone needs no consent; other consent rules remain unchanged.
Native clipboard, Cut preparation and graph template export refuse selections
intersecting retained members with `clipboard_abbreviation_display_unsupported`;
the UI instructs expansion first. External label roundtrip, a nickname dictionary,
text-to-graph parsing, arbitrary graph replacement and general multistyle chemical
labels remain unsupported. P35p introduced native v24 plain-text persistence;
v25 introduced script-run persistence; current v30 carries the P36d
graphic-width field, P36h ellipse, P36i polygon, P36j spline and P36k image profiles described below. This slice shipped in
[#202](https://github.com/EunhakChoi/chemifree/pull/202), with successful final-head
[check/browser CI 42316586336](https://github.com/EunhakChoi/chemifree/actions/runs/42316586336).
The [completion audit](verification/p35-completion-audit.md#merged-delivery-and-ci)
records its Codex/independent review dispositions and bounded P35 completion.
[The P35p record](verification/p35p-fragment-display.md) retains historical session notes.

### Retained fragment display script runs (P35q)

`{ "op":"set_fragment_display_runs", "instance":"s1", "anchor":"a2", "runs":[{"text":"R","script":"baseline"},{"text":"2","script":"subscript"}] }`
replaces the complete run list of an existing fragment display. Each run requires
nonempty `text` and explicit `script: "baseline" | "subscript" | "superscript"`.
The concatenated text must contain a non-whitespace character, fit 128 UTF-8 bytes
in total and remain one Unicode line without control characters or line separators.
An empty list, empty run, all-blank concatenation or excess bytes refuses with
`display_label_invalid`; absent display uses `fragment_display_invalid`. Missing,
null, unknown script and wrong-type wire fields use `invalid_request`. There is
no independent 32-run limit: nonempty text and the byte budget bound the list to
at most 128 runs. Runs and text are not normalized or merged.

`contract_fragment` and `set_fragment_display_text` remain plain-text commands:
they create or replace the display with exactly one baseline run. Sending the
same concatenated text therefore resets prior scripts and boundaries in one Undo
step; an identical complete run list is a no-op. Ordinary transaction atomicity,
replay, selection and Undo/Redo apply. Snapshot entries include both `runs` and
`text`, where `text` is their exact concatenation. A client can read an older
entry without runs as one baseline run, but refuses invalid runs or a mismatch
between runs and text. This API compatibility is separate from strict native
file version admission below.

All runs inherit document chemical-label font, size, weight, style, underline,
outline and shadow. Baseline uses the document label size; scripts use the existing
chemical-label small size and vertical shift ratios. Actual SVG measurements
include each scripted glyph, underline, outline and shadow in shared trim, fit,
hit and selection bounds. The complete label is centered at the retained anchor.
Run boundaries are preserved; shaping across a boundary is not guaranteed, so the
UI asks users to keep combining marks and emoji sequences within a run. The run
editor provides native Unicode inputs, explicit script choices, add/remove and
Apply with an IME composition gate. Plain-text Apply explicitly warns that it
resets scripts to baseline.

Scripts change appearance only. Text does not define chemistry: original graph
membership, atom/bond IDs, states, coordinates, stereo and all scientific results
remain unchanged. P35p eligibility, overlap, automatic reveal and geometry rules
remain binding. Export retains the full graph and reports `abbreviation_display`
rendering loss; native clipboard/Cut/template member intersections still refuse
until expansion. No per-run font/color/weight/style override, dictionary, chemical
parser or external display-run roundtrip is introduced. This slice shipped in
[#203](https://github.com/EunhakChoi/chemifree/pull/203), with successful final-head
[check/browser PR CI 42320918256](https://github.com/EunhakChoi/chemifree/actions/runs/42320918256)
and [post-merge main CI 42322504482](https://github.com/EunhakChoi/chemifree/actions/runs/42322504482).
Codex and the independent reviewer identified the same post-reopen browser ink
assertion error; the fixed assertion and saved/reopened script-position regression
are recorded in the [audit](verification/p35-completion-audit.md#203-merged-evidence-and-post-merge-main-ci).
Bounded P35 is done. [The P35q record](verification/p35q-fragment-display-runs.md)
separates historical implementation-session pending statements from this verified delivery.

## User graph template files (P34d)

The web **User graph templates** panel saves complete selected graph components through
the existing `document.clipboard_copy` operation. Selected bond endpoints and every internal
bond are included, as in native copy. A non-empty `boundary_bonds` list refuses template
saving: cut graph boundaries are not guessed attachment instructions. Scene-object selection,
enhanced-stereo-group members and non-self-contained stereo retain native copy's refusals.
The selected source placement supplies coordinates; multiple source placements are permitted
when one is explicitly chosen. Source graph, selection, history and revision remain unchanged.

**Download template** saves the exact `chemifree.clipboard` version 1 payload as a JSON file
with a `.chemifree-fragment.json` filename suffix. The editable filename is not graph metadata;
characters other than Unicode letters/numbers, dot, underscore and hyphen become underscores,
and leading dots are removed. No new file schema, template database or server operation is
introduced. The offer is hidden after the complete source snapshot or service generation
changes. Download uses the already prepared bytes, without a network recheck at the click;
it cannot detect an otherwise unobserved service restart.

**Insert template file** reads one explicitly selected file, at most 64 KiB, with strict UTF-8
decoding. Empty files and decoding failures are local errors. The filename/extension does not
determine format. File selection alone sends no paste and edits nothing. A later file selection,
dialog close, document switch or observed process change invalidates an older asynchronous
read. The interface identifies the selected file; graph/version/style validation occurs through
the existing closed native decoder only on the explicit **Insert template** click.

Insertion is the existing `document.clipboard_paste`: exactly one destination placement,
exactly matching settings, finite explicit x/y point offsets added to saved coordinates,
fresh IDs, preserved recorded states/marks/self-contained stereo and selected new entities.
The normal 500-command and graph/coordinate/identifier limits apply. A refusal preserves graph,
selection, counters, history, revision and dirty state. A successful insertion is one undo step;
Undo/Redo and save/reopen retain the same graph, IDs and coordinates. Existing destination
entities, graphics, references and settings remain unchanged. Attachment-point metadata is
preserved and never consumed. Source files are not rewritten or executed.

The click captures file bytes, offsets, destination ID, selection and service generation;
the queue rejects a changed destination/process. The immutable native paste attempt owns
request ID, revision and body. An uncertain result blocks later mutations and exposes
**Check template insertion outcome** even after closing the dialog. It checks only the same
attempt. Restart or terminal conflict requires explicit reconciliation without inserting again
into another process. Browser clipboard APIs are never used.

Browser File/Download is the user-file adapter. Existing inert DATA template framing is not
activated. No graph normalization, scale/rotation, attachment, replacement, abbreviation
contraction/expansion, scene-template copying, template library management or layout search
is provided. P34 remains **in_progress**. The new real-backend browser checks are written,
not executed in this implementation session; see [P34d record](verification/p34d-user-template.md).

## User template anchor connection (P34e)

Three additive operation-contract-1 operations reuse the closed native graph v1 file codec.
They run through HTTP operations and the CLI operations process; no original template/DLL or
external DATA provider is activated. They do not add a transaction command or change storage
schema. Including P34f below, the catalog has 40 operations; 30 run in the default no-DATA/no-native-provider host.

| Operation | Parameters | Result |
| --- | --- | --- |
| `document.template_inspect` | `{ payload }` | Exact payload and local atoms `{id,element,x,y,supported}`; read-only validation and anchor eligibility |
| `document.template_plan` | Attachment input below | `{ source, snapshot, mapping, connection }`; source is the actual current document, snapshot is only a candidate preview |
| `document.template_attach` | Same attachment input | `{ snapshot, mapping, connection }`; committed immutable snapshot with the same native source→fresh ID mapping |

Attachment input is `{document_id, expected_revision, payload, instance, source_anchor, atom,
direction:{dx,dy}, selection_before?}`. All anchor, target, instance and direction fields are
required. IDs are explicit; `$N` references and attachment-point inference are unsupported.
Direction is a finite nonzero vector; magnitude is ignored and normalization scales by the
largest component before hypot. The selected source anchor is translated to destination
atom position plus current `bond_length_pt` along that direction. Every other source position
gets the identical offset. No rotation/reflection/scaling, automatic endpoint merge, collision
avoidance or layout is performed.

The existing 64KiB/strict native v1 decoder, exact-style rule and one destination placement
are retained. The template must contain exactly one connected component (one atom is allowed).
A source anchor with a stored center record or an incident stored double-bond record is
refused, including unknown configurations, because joining would discard that copied statement.
Other copied configurations/marks/states are retained under the existing fresh-ID mapping;
canonical tetrahedral slot/relation-reference adjustments preserve their statements. Numbered
attachment points are preserved data and never consumed.

The final connector is one ordinary `AddBond` after the native paste commands. Destination
center/incident double configurations are cleared and its groups pruned/reported with the
existing transaction notes. Marks and unrelated destination state are preserved. No new
valence edit gate, H correction, configurations, CIP labels or chemistry inference is added.
The operation uses N fresh atoms, M fresh internal bonds and one fresh connector, with the
connector included in the 500-command and identifier/entity limits. The new fragment atoms
and internal bonds are selected; connector/target are not automatically selected. Undo restores
the captured prior selection and destination statements/groups; Redo retains fresh IDs.
Any late failure rolls back the whole transaction, counters, revision, dirty state and redo.

Inspection and planning mutate no document/history/selection/counters and reserve no IDs.
The plan runs the same commands on a clone under the destination revision lock, returning
predicted notes in its candidate snapshot/loss report; this is not a committed edit. Its
`document_revision` envelope names the unchanged source revision, while the candidate body
shows the hypothetical next revision. Apply
revalidates under the ordinary mutation gate. Replay binds exact payload, anchors, instance,
direction (including signed zero), expected revision and captured selection. Reusing an apply
request with different input is refused. Its original response remains immutable after Undo.

Unknown/missing parameters and nonnumeric/null direction components are `invalid_request`.
Semantic refusals use `invalid_command` with existing native `details.reason` codes, or
`template_disconnected`, `template_anchor_unknown`, `template_anchor_stereo_unsupported`,
`template_instance_invalid`, `template_direction_invalid`, `template_target_invalid`.
A syntactically valid but absent destination atom also uses `template_target_invalid`;
malformed instance/source-anchor/destination ID strings use `template_instance_invalid`,
`template_anchor_unknown`, and `template_target_invalid` respectively. Missing or null ID
fields remain `invalid_request` rather than semantic ID failures.
Dangling references inside the native file retain `clipboard_fragment_invalid` and are not
reclassified as destination failures. Source-only inspection returns `document_revision:null`;
plan/apply keep their numeric source/committed revision semantics.
A stale destination revision is `revision_conflict`. Unknown source fields/versions, malformed
native records, style mismatch, unsupported groups, coordinate/ID/entity/command limits and
multiple placements are refused without editing; old independent file insertion stays available.

The web dialog first validates the chosen file, then exposes local source IDs/labels/coordinates
and disables unsupported anchors. Source and destination choices start empty. dx/dy must be
entered explicitly, followed by **Preview template connection** and **Attach template**.
Input/file changes invalidate the offer. Document/style/revision/process changes invalidate
adoption. A preview snapshot is never a live editor snapshot. The client verifies fresh counts,
properties/marks/translation, exact copied stereo remapping, retained destination state and
expected stereo/group notes before accepting the plan. Apply must match the frozen preview,
notes, mapping and connector; late/malformed success remains unconfirmed. The existing template
recovery UI checks the exact same body/request/process after response loss; closing the dialog
retains that recovery. Restart/conflict needs explicit reconciliation, with no new-process resend.
File operations do not access the OS clipboard.

The native file is not modified or upgraded to store anchor metadata. Fusion/spiro/non-terminal replacement,
multi-anchor/polymer rules, disconnected attachments, library management, abbreviation expansion
and automatic layout/conformation remain unsupported. P34 is **in_progress**. These checks have
not run in this implementation session; see [P34e record](verification/p34e-template-attachment.md).

## Terminal template replacement (P34f)

`document.template_replace_plan` and `document.template_replace` take the closed input
`{document_id, expected_revision, payload, instance, source_anchor, atom, selection_before?}`.
No direction/offset field is accepted. The native v1 template payload is unchanged; P35a separately upgrades the saved document to v8.
Plan returns `{source, snapshot, mapping, boundary}`; apply returns `{snapshot, mapping, boundary}`.
The plan's envelope revision names the unchanged source; the candidate is never a live snapshot.

The target must exist in the explicit only destination placement and have exactly one incident
single bond (query bonds are not single bonds). Other boundary kinds, isolated atoms and targets
with two or more neighbors refuse with `template_target_not_terminal_single`. The existing
64KiB native validation, one connected source component, exact style, explicit IDs and source
anchor stereo refusals apply. A source anchor carrying a center or incident double configuration,
including unknown, remains unsupported. Unknown/missing/null input fields retain the P34e
malformed/semantic distinctions. Disconnected sources and other native refusals retain their codes.

The source anchor is bound to the existing target ID. All target atom-state fields become the
source anchor's exact state, including query/element, charge, isotope, H, radical and attachment
points. Its existing coordinates are retained bit for bit. Every other source atom is translated
by target-position minus source-anchor-position, with fresh IDs for those N−1 atoms and all M
internal bonds. There is no new connector, deletion, rotation, reflection, scaling, bond-length
normalization, H correction, valence gate or attachment-point consumption. Marks on internal
source bonds are copied. Configurations on other source centers/double bonds are remapped and
canonicalized through the ordinary stored-stereo commands, including parity/reference changes.

The outside bond's ID, endpoints, order and mark are retained exactly; so are all existing
positions, graphics, references, settings and atoms other than the target state. The original
neighbor's center/double records and groups are preserved: its neighbor identities do not change.
In particular, incoming references to the bound atom and outside bond remain valid. Apply has
no stereo losses and returns `ok` with empty notes/loss report. No configuration, mark or CIP
label is inferred. Resulting chemistry may be diagnosed separately by the existing inspections.

Changed content selects all mapped template atoms, including the retained anchor, and internal
bonds; the outside bond is not automatically selected. It makes one revision/history step.
An identical single-atom replacement is an ordinary no-op: no IDs/history/revision/dirty-state
or selection changes. Mapping still explicitly binds that source ID to the target. Undo restores
the prior full target state and captured selection; Redo and save/reopen retain all mapped IDs,
positions and statements. Native 500-command/entity/coordinate/identifier limits apply, counting
the target state update as one command. All failures, including late ones, preserve the entire
document, selection, counters, dirty state, redo and revision. Planning reserves no IDs.

The web **Replace terminal atom…** dialog reads and validates the file, starts with empty source
and destination choices, and disables ineligible target/anchor choices. **Preview template
replacement** shows fresh counts, retained target/boundary and before/after target state.
Explicit **Replace atom with template** uses the frozen preview. File/choice/document/revision/
style/process changes invalidate it. The existing immutable template mutation gate holds the
exact body/request/process through lost replies; closed-dialog recovery remains available.
Restart or terminal revision conflict requires explicit reconciliation and never resends into
a new process. No OS clipboard calls occur. This is bounded terminal graph replacement, without
fusion/spiro, multi-boundary replacement, abbreviation contraction/expansion or new storage
objects. P34 remains **in_progress**; written checks are unrun in this implementation session.
See [P34f record](verification/p34f-template-replacement.md).

## Attached carbon ring (P34c)

`attach_ring` is `{ op: "attach_ring", instance, atom, direction: { dx, dy }, size?: 5 | 6, pattern?: "single" | "kekule" }`.
An invalid zero/non-finite direction reports `invalid_command` with
`reason:"ring_direction_invalid"` in the v1 envelope and `details.reason` in the operation
envelope. Ring recipe refusals preserve their failing `command_index` in both envelopes.
It connects the existing explicit atom to a fresh regular five/six-carbon cycle
by one additional single bond: N new atoms and N+1 new bonds, without sharing or replacing
any existing atom or bond. Size omission is 6; null, unsupported values and unknown fields
are malformed (`invalid_request`). `$N` is not an attachment target; a malformed identifier
is `invalid_command`, as with other explicit-ID commands. The target must exist and the
explicit instance must be the only placement. Direction components are finite and not both
zero; magnitude is ignored. A zero vector is `invalid_command` with its command index.
Non-numeric/JSON null components are malformed. Direct Domain non-finite vectors are refused.

P34h adds explicit `pattern:"kekule"` for size6 only (omitted size is still6). Internal cycle
edges0/2/4 are double and1/3/5 are single in the recipe's original vertex order. The connector
is always single, independent of the pattern. Pattern omission and explicit single retain
the original all-single size5/6 graph, geometry and notes. Unknown/non-string/null patterns
are `invalid_request`; size5+kekule is `invalid_command` with `ring_pattern_invalid` and its
command index. This states explicit bond orders and assigns no aromaticity or configurations.

For current bond length L, target position A, normalized direction u and the independent
recipe offsets (x,y) above, with R the first offset's x, each vertex is
`A + (L+(R-x))*u + y*(u_y,-u_x)`. The nearest vertex is A+L*u and the rest lie outward;
the original vertex order and fresh IDs are retained. Direction normalization scales by its
largest component before hypot so finite huge/subnormal inputs do not overflow/underflow.
Every resulting position must satisfy the existing bounds. No placement, collision or
conformation search is performed. The two recipes share the original six-ring arithmetic.

The connecting edge calls the same Domain `insert_bond` as `add_bond`. It clears the target's
stored center record and any stored double-bond record with that target as an end, reports
those removals in the ordinary `notes`, prunes the cleared member from its stereo group and
reports the existing group notes (removing a now-empty group). Marks and unrelated records
are preserved. The operation is `partial` exactly when the existing notes contract requires
it. Nothing derives new configurations, marks or CIP labels. Undo restores all original
statements and groups; Redo restores the exact graph, IDs and positions.

As with `add_bond`, no new valence admission rule is imposed. Existing query/charge/isotope/
hydrogen/radical/numbered attachment-point states remain unchanged. Numbered attachment
points are metadata, not instructions consumed by this command. The existing inspector can
diagnose resulting valence; attachment does not claim chemically valid substitution or
full template/monomer semantics. Existing scene/reference/selection/settings and coordinates
are preserved. The entire transaction, IDs, revision, dirty state, history and redo roll back
on any failure, including the final connection's identifier or bond limit.

Web R defaults to Independent; Attach to atom is explicit. In Attach mode only an atom drag
supplies a target and direction. Atom click and empty-space drag insert nothing. Release is
a direction hint, never an automatic merge with content under the pointer. Gesture size,
pattern, atom and instance are captured at press; mode/size/pattern are disabled during a gesture and while
busy. The queue captures the vector, selection and document/process, then revalidates the
target and uses its latest position/current bond length. The unmarked direction preview has
no snapping. The existing immutable ring attempt holds the complete body across lost replies;
restart/conflict require explicit reconciliation, without resending to another process.
The client checks the exact cycle/link, retained state, affected stereo/group removals and
note feature/path/disposition set before adopting a claimed success. The existing StereoNotes
shows the authoritative reasons. No clipboard access occurs.

The six-carbon pattern selector is also available in Attach mode. Entering Attach retains
the previous default reset to single, after which Kekulé can be explicitly chosen. The drag
retains its pressed size/pattern even if a caller rerenders the canvas with different choices
before release. The hook queues those captured scalar values; recovery reuses the same body,
request and process. The client checks exact alternating phase and the single connector as
well as ordinary stereo/group notes. No new command, storage schema or provider is introduced.

Fusion/spiro/non-terminal replacement, aromatic-bond patterns, other ring sizes, abbreviation
expansion and layout/collision/conformation remain unsupported. P34 is **in_progress**.
Tests/build/browser/mutation have not run for this candidate; see the
[P34c record](verification/p34c-attached-ring.md) and
[P34h record](verification/p34h-attached-kekule-ring.md).


## Embedded PNG image profile (P36k candidate)

An image is a separate scene object with immutable embedded PNG bytes and an
axis-aligned physical rectangle, without chemical interpretation. The shared
transaction command `add_image` requires `mime_type: "image/png"`, canonical
padded `data_base64`, `origin: {x,y}` and `size_pt: {x,y}`; optional `parent` and
`index` use existing group/page insertion rules. `update_image_geometry` requires
`object`, `origin` and `size_pt` and changes only the rectangle. It cannot replace
the PNG or its decoded metadata. API scene snapshots additionally return `id`,
`parent`, derived `pixel_width` and `pixel_height`; these pixel dimensions are
not trusted input fields. HTTP and CLI entry points configure the same decoder.

Both sizes must be finite and positive. Origin and the opposite corner must be
within inclusive ±100000 pt, so each size is at most 200000 pt. Pixel dimensions,
PNG density metadata and physical dimensions are independent. Aspect ratio is
not constrained; the web SVG image fills the complete rectangle with
`preserveAspectRatio="none"`. The transparent rectangle is the hit target and
supplies bounds, Fit, marquee containment and arrangement geometry, including
fully transparent pixels. No alpha-mask hit test or chemical interpretation is
performed.

The provider accepts only a complete static, noninterlaced 8-bit RGB or RGBA PNG.
It checks canonical padded base64, signature, chunk structure and every CRC,
IHDR/profile/dimensions, the exact zlib row-byte count/checksum/end position, a
complete PNG frame decode, final empty IEND and EOF. The bounded chunk whitelist
is IHDR/IDAT/IEND plus valid, unrepeated pre-IDAT sRGB/gAMA/pHYs scalar metadata.
When sRGB and gAMA both occur, gAMA must be 45455 regardless of chunk order.
Unknown chunks, palette/grayscale/16-bit/interlaced profiles, ICC/text metadata,
APNG controls anywhere in the file, truncation and trailing data are refused.
Original verified PNG bytes are retained; no re-encoding or color-equivalence
claim is made. The inner layer owns a decoder port; a weak asset cache shares
verified immutable bytes while current/history states retain them. Geometry edits
do not decode raster data again.

| Resource policy | Limit |
| --- | --- |
| Complete encoded PNG source | 128 KiB before base64 |
| Each decoded edge | 2048 pixels |
| One image | 1,048,576 pixels |
| Current scene, counted per image occurrence | 4,194,304 pixels |
| Distinct retained history assets | 67,108,864 pixels |
| Decoder allocation / caller-owned frame output | 8 MiB / 4 MiB |

These are Chemifree resource policies, not image-format maxima. Images share the
existing variable-content 48 MiB current / 8 MiB history byte admission and undo
retention limits. An oversized current edit refuses atomically; oldest retained
undo entries may be trimmed under history limits. Restore validates incoming
image states before retention and bounds unique decoded assets by the combined
current/history pixel allowance. Image failures use `image_invalid`,
`image_pixel_limit` or `image_persistence_limit` as applicable; retained history
is bounded by trimming. Malformed command fields retain ordinary request diagnostics.

Images support whole-object movement, group containment, sibling ordering,
alignment/distribution, references, deletion, undo/redo, local save/reopen,
recovery and save as copy through the existing document paths. Any selected
image or image descendant makes rotation/reflection refuse the complete
transaction as `image_unsupported`, including a zero-angle rotation. Scene
clipboard copying refuses an image-containing forest as
`image_clipboard_unsupported`; graph clipboard selections retain their existing
scene-selection refusal. Image Copy/Cut transport and image clipboard profiles
are not added. External MOL/SDF, SMILES and bounded CDXML/CDX exports retain the
graph and report omitted scene-object rendering under existing loss rules.

Native document/draft v30 stores image states with `kind: "image"`,
`mime_type`, `data_base64`, `origin` and `size_pt`. Pixel dimensions are derived
when read. Versions 1–29 forbid image objects anywhere in current/undo/redo
states, before retention; future v31 is refused untouched. Valid older files
keep their existing graph, scene and history behavior, with exact original
document/draft bytes archived before first replacement/recovery. The 64 MiB
provider file limit and earlier strict graphic/style profiles remain in force.

The web file chooser performs bounded container/header preflight; the service
performs authoritative complete PNG validation. Selecting a file alone writes
nothing. Add requires an explicit physical rectangle and is bound to the
captured document, revision and service generation. Geometry submission retains
the captured image identity/source and rejects stale/no-op edits. Existing
uncertain-outcome reconciliation applies without automatic replay into a new
process. URLs, JPEG/GIF/SVG, raster replacement, crop, rotation/reflection,
interactive resize handles, image clipboard transport, OCR and image-to-graph
conversion remain unsupported. Focused Rust/web execution and mutations are
recorded in the [P36k record](verification/p36k-embedded-image.md). Independent
review of `eb40cfa` found that history replay could bypass image pixel limits;
the shared content validator now checks the current pixel limit for every restored
and replayed state. A regression rejects both undo- and redo-only over-limit
states without rewriting the native bytes. Follow-up clippy and 52 focused tests
passed. Codex review of `5ea2107` in
[PR #216](https://github.com/EunhakChoi/chemifree/pull/216) completed without
findings. #216 merged as `7bec5ea`. A later Codex review of `f69f8de` left one
unresolved P2: an image ordered before a molecule instance was drawn after the
global chemical label-shadow group and could cover that instance's shadow. That
known limitation was tracked in [#219](https://github.com/EunhakChoi/chemifree/issues/219)
and fixed under #219 by painting chemical label shadows per run of consecutive
instances. The merge commit's main CI run 42350215647 failed
in a spline cancellation browser test; #217 (`4198438`) fixed the test and main
run 42353217355 succeeded. A caption-shadow ink-fit check (`scripts/web-caption-check.mjs:513`) failed once in #217's CI
and passed on rerun; its cause is not determined ([#220](https://github.com/EunhakChoi/chemifree/issues/220)). The closing record is in the
[P36k record](verification/p36k-embedded-image.md#merge-and-main-ci-closing-record).
P36a–k are all merged (#204, #206–#208, #210–#216) while P36 stays `in_progress`;
earlier per-slice "pending" notes in this section are historical, and the user stopped
P36 after the PNG image (2026-11-07 session); the next scope is set by the user's next request.

## Single open cubic spline profile (P36j candidate)

Spline is exactly four ordered points P0,C1,C2,P3 defining one open cubic Bézier.
P0/P3 are endpoints and C1/C2 explicit controls, not interpolation-through points.
Coordinates are finite inclusive±100000pt; repeats/degeneracy/order stay exact.
SVG uses M…C… with no Z, fill or arrowhead, including when endpoints coincide.
One open cubic is a bounded Chemifree choice within public spline/points/closed/
properties and R10/R15 contracts, not general spline or original renderer parity.

Bounds/Fit/marquee/arrangement and the latest-geometry transform pivot use all
four stored points' AABB. This is conservative for the centerline, potentially
wider than the visible curve, not tight curve extrema/stroke-ink bounds or a
curve-intersection solver. Hit uses the actual path stroke. Effective inherited/
explicit width supplies stroke/underlay/zoom-adjusted hit. All-point move,
arrangement, arbitrary finite rotation/reflection preserve roles/shape/width/ID/
refs/group/order/graph/stored stereo; existing rectangle/ellipse restrictions
still refuse mixed transforms whole.

`add_spline { points:[P0,C1,C2,P3], parent?, index? }` starts with inherited width;
`update_spline_points { object, points:[P0,C1,C2,P3] }` replaces the whole existing
spline definition. Public add has no width parameter; existing width command and
finite0.1–10/null rules apply. Old shapes stay exact2, polygon3–64, spline exact4.
Two-point/polygon/spline command mixing refuses graphic_command_mismatch; domain/
stored geometry count uses graphic_geometry_invalid, malformed typed array count
uses invalid_request. Shared atomicity/no-op/history/Undo/revision/replay applies.

Graphic Spline collects4 non-drag clicks with explicit P0/C1/C2/P3 controls,
existing inverse pan/zoom/bounds/0.01pt rounding/3px threshold, local control guides
and cubic preview. Only explicit Finish writes one page-root add_spline; fifth
click does not alter the draft. Remove last/Cancel exist. Document/revision/tool/
shape/observed generation or Escape/window cancel discards local draft. Finish
copies points/captured revision; queue stale/restart/generation checks prevent
retargeting or automatic unknown-create replay into a replacement process. This
is not autosaved recovery content or advance restart detection guarantee. Numeric
eight-coordinate Apply replaces all4 with invalid/IME/busy/no-op/exact-target
stale and existing no-auto-unknown-edit-replay guards. No control/tangent/endpoint
drag handles or curve-fitting interaction is added.

Native29 current/history/draft admits spline; old1–28 headers forbid it including
history-only before retention. Shape-specific cardinality is validated before
trimming malformed histories. Polygon28/ellipse27/width26 profiles remain strict;
valid old states/history migrate without read writes, exact old doc/draft bytes
are archived before replacement/recovery (including28), future30 refuses untouched.
Spline640=256+96*4 reserves shared admission/physical bytes and activates48MiB
current/8MiB whole-history budgeting even history-only. Old pair256 and other
weights/physical64MiB remain. Latest edit must fit alone; older undo byte/count
trim and restore oldest-undo/farthest-redo retention follow validation. Overflow
reason depends on changed shapes: polygon first polygon_persistence_limit, else
spline spline_persistence_limit, else existing caption_persistence_limit. A spline
elsewhere in content/history does not rename caption-only errors.

Clipboard canonical output is included spline6>polygon5>ellipse4>explicit3>
inherited2. V6 older-shape input is valid and can recopy canonical2–5; nullable
width required3–6/forbidden2, spline forbidden2–5, strict counts/keys/settings/ref/
trees/bounds and future7 refusal remain. No old-reader compatibility/silent
downgrade/curve approximation is promised. Existing limits/settings-match/refs/
Cut/fresh IDs/offsets/Undo and graph clipboard1 remain. External graph exports
report omitted spline as scene_object rendering loss; no external curve roundtrip
or chemical inference is added. Multi-segment/closed/quadratic/arc/fill/arrowheads,
tight extrema, control/tangent handles, scale/skew/snapping/images/clip/layers/
scientific symbols stay unsupported.

Earlier P36a–i shape/schema/version statements describe those slices. This current
profile supersedes their no-curve/native28/clipboard5 boundaries without reusing
their verification results. P36 stays in_progress; Rust executor final log records clippy/71 focused tests and five detected
mutations with restored-source passes; the documentation agent only read it.
Targeted web checks and the independent review are complete; full isolated
validation and CI remain pending. Historical: that was the status when this slice was written; P36j has since merged as #215 (`31e9328`) with main CI run 42345870358 success (see the [P36 status table](HANDOFF.ko.md)). See [P36j record](verification/p36j-spline.md).

## Bounded closed polygon profile (P36i candidate)

P36i adds a closed stroke-only scene polygon with3–64 ordered finite vertices in
inclusive±100000 document pt.64 is an internal Chemifree resource-budget choice,
not a limit supplied by the public schema. Repeated/collinear/crossing/coincident/
zero-area geometry and repeated closing vertices stay exact. SVG closes last to
first without adding a stored point; no fill, area/winding/chemistry or regular
polygon meaning is inferred. Degenerate strokes can overlap or be invisible;
Scene/numeric editing remains available. Width inheritance/override, zoom-adjusted
hit stroke and selection underlay use the existing effective width.

`add_polygon { vertices, parent?, index? }` starts with inherited width; public
wire has no width parameter. `update_polygon_vertices { object, vertices }`
replaces all vertices of an existing polygon without shape/ID/width/parent/order/
reference changes. Snapshot/native/clipboard geometry is still named `points`.
Line/arrow/rectangle/ellipse stay exact2; old `add_graphic`/`update_graphic_points`
reject polygon, and polygon update rejects old shapes, as `graphic_command_mismatch`.
Bad cardinality is `graphic_geometry_invalid`; malformed fields/bounded-coordinate/
missing-object codes retain existing wire/domain classifications. Atomic/no-op/
replay/Undo/Redo applies. Polygon move/bounds/arrangement/marquee and arbitrary-angle
rotation/reflection use every vertex; rectangle/ellipse rotation restrictions
remain. Graph/placement/stored stereo/style/ref state stays separate.

Graphic Polygon collects click-release press points into a local draft below the
existing3px drag threshold. Inverse pan/zoom mapping,0.01pt rounding/bounds remain.
Preview is open below3 and closed from3, with local vertex markers; no document
write occurs until explicit Finish with3–64 vertices. Finish queues one page-root
add_polygon;64 stops append without automatic finish. Remove last/Cancel exist.
Document/revision/tool/shape/observed-generation changes and Escape/window cancel
clear the draft. Finish freezes vertices/captured revision; queued document/
revision/generation/reconciled-restart checks prevent retargeting and automatic
unknown-create replay into a replacement process. It is not persisted recovery
content or guaranteed advance detection of every process restart. Numeric list
Apply replaces all vertices with invalid/IME/busy/no-op/exact-target stale guards
and existing graphic-edit no-replay policy. No polygon endpoint/per-vertex drag,
insert-on-edge/resize/rotation handles are added; body move translates all points.

Native28 admits polygons in current/history/drafts. Headers1–27 forbid them,
including history-only records checked before retention; all old shapes retain
exact2 even after the internal list representation expands. Ellipse starts27,
nullable width remains required from26/forbidden before26 with inherited migration.
Valid old state/history migrates in memory without read writes; exact old document/
draft bytes are archived before first replacement/recovery, now including27.
Future29 refuses untouched. Current/history-only polygons activate the shared
48MiB current and8MiB history admission budgets with256+96 bytes/vertex; old
pair weights stay256 and physical file limit64MiB remains. A latest edit must fit
alone; oldest undo trims by byte/count limits, restore trims oldest undo then
farthest redo after validation. Later fixed-size edits still account for retained
history-only polygon. Over-budget polygon-bearing changes report
`polygon_persistence_limit`; caption-only overflow keeps the existing
`caption_persistence_limit` reason/message even when polygons exist.

Clipboard output chooses5 iff the included forest has polygon, else ellipse4/
explicit-width3/all-inherited2. V5 requires nullable width on every graphic;
v5 older-shape input can recopy as canonical2/3/4. V2–4 polygon, v2 width even
null, v3–5 missing/invalid width, wrong shape cardinality and future6 refuse.
Strict keys/settings/tree/ref/points,64KiB/500command bounds, Cut source/ref guards,
fresh IDs/offsets/history and graph clipboard1 remain. Old-reader compatibility/
silent polygon conversion is not claimed. External MOL/SDF/SMILES/bounded CDXML/CDX
keep graph and report scene_object rendering loss; no polygon roundtrip exists.

Earlier P36a–h no-new-shape/native/clipboard statements record those slices and
are superseded here for current polygon admission. Public points/closed/properties
and rendering R10/R12/R15 contracts are broader: open paths/fills/colors/dashes,
curves/arcs/splines, snapping/regularity/aspect locks, general scale/skew, images,
clip/layers/scientific symbols/reaction semantics remain unsupported. P36 stays
`in_progress`. Source/authored-check inspection is not a passing execution;
Rust/web final/full/reviews/CI remain pending. Historical: that was the status when this slice was written; P36i has since merged as #214 (`d7e2b16`) with main CI run 42343862092 success (see the [P36 status table](HANDOFF.ko.md)). See [P36i record](verification/p36i-polygon.md).

## Axis-aligned ellipse profile (P36h candidate)

The historical P36h slice adds `shape: "ellipse"` to the existing graphic commands and
snapshots. Earlier P36a–g no-new-shape/no-schema/native25/native26 statements below
record those slices' introduction; this section supersedes their current shape
admission and storage/clipboard version boundary, not their historical execution
status. P36 remains `in_progress`; targeted/full/browser checks, both reviews and
CI are pending until evidenced. Historical: that was the status when this slice was written; P36h has since merged as #213 (`698a364`) with main CI run 42341200117 success (see the [P36 status table](HANDOFF.ko.md)). See [P36h record](verification/p36h-ellipse.md).

An ellipse stores exactly two finite opposite bounding-box corners within
inclusive ±100000 document pt, preserving their order even when reversed or
coincident. It is axis-aligned and stroke-only: center is the coordinate average,
radii are half absolute coordinate differences, with no saved angle/center/radius
or implicit fill. Effective inherited/explicit width supplies foreground,
selection underlay and transparent hit stroke. A zero radius has no visible
ellipse/hit stroke; retain the object and use Scene selection/numeric corner
editing. Existing distinct-corner endpoint handles edit bounding corners, not
circumference points; coincident corners use the form. Existing full-corner-box
marquee semantics apply without a curved-edge intersection/lasso algorithm.

Graphic choice and Scene Add include Ellipse. Existing preview-only pointer moves,
release-time single page-root `add_graphic`, inverse pan/zoom coordinates,
rounding/threshold, cancel/stale/service policies and no automatic unknown-create
replay into a replacement process remain. Numeric corner/width edits retain their
existing capture/atomicity policies. Group/order/reference/move/Undo/Redo paths
keep shape/width/IDs and unrelated graph/placement/stereo/style state.

Exact quarter turns/full turns and horizontal/vertical reflections remain
representable. Any other normalized angle refuses as
`transform_ellipse_unsupported`, in both plan and transaction, including circles,
zero-axis/coincident ellipses and group descendants. No circle special case or
near-quarter tolerance applies; mixed edits refuse atomically. Geometry edits do
not recalculate stored chemical stereo.

Native27 writes documents/drafts/current/history. Headers v1–26 forbid ellipse,
including history-only records before retention; they cannot smuggle it through
migration. Width remains required nullable from v26, forbidden in v1–25 with
inherited migration. Valid old states/history migrate in memory without a read
write, and exact old document/draft bytes are archived before first replacement/
recovery. Future v31 refuses untouched. Existing storage limits/reservations stay.

Clipboard output is v4 iff the copied forest contains an ellipse; every included
graphic then has required nullable width even for an all-inherited forest.
Otherwise canonical output remains v2 (all inherited) or v3 (any explicit width).
V4 older-shape/all-null input is valid; recopy may choose v2/v3. V2/v3 ellipse,
v2 width even null, and v3/v4 missing/invalid width refuse; future v5 is unsupported.
Strict keys/settings/tree/ref/point checks, width range0.1–10/null, settings-match,
limits, fresh IDs/offsets/Cut/history and graph clipboard v1 remain. No old-reader
compatibility or silent ellipse downgrade is claimed.

MOL/SDF/SMILES/bounded CDXML/CDX keep the chemical graph while dropping ellipse
with `scene_object` rendering loss; other consent rules remain. No external
ellipse roundtrip or scientific meaning is inferred. The public schema's ellipse
and R12/R15 rendering contracts are broader: fills/shading/symbols, arbitrary
oriented shapes/transforms, curves/arcs, images, clip/layers, scaling/skew/general
resize/rotation handles and reaction/electron semantics remain unsupported.

## Existing graphic point editing (P36a candidate)

`{ "op":"update_graphic_points", "object":"s2", "points":[{"x":15.5,"y":-20},{"x":80,"y":35}] }`
replaces exactly the two stored points of an existing line, arrow or rectangle.
Coordinates are document points (1/72 inch), with x increasing right and y down;
each coordinate must be finite and within inclusive −100000 to100000 pt.
Reversed and coincident points are allowed, matching existing graphic creation.
Point order is preserved; an arrow keeps its tip at its second point, and a
rectangle uses its two opposite corners without rewriting their stored order.
The command accepts a concrete existing graphic ID, not a `$N` creation reference.
It does not accept shape, parent, group, style or reference changes.

Malformed or missing fields, a list other than two points, null/wrong-type
coordinates and unknown fields such as `shape` use `invalid_request`. Coordinates
outside the domain limit use `invalid_command`. Missing object and wrong-kind
targets use `invalid_command` with `object_unknown` or `object_kind_mismatch`.
Instances, groups and captions are not graphics. A refusal rolls back the entire
transaction, including earlier edits and any allocated IDs. Effective coordinate
changes form one ordinary Undo step; identical points are a no-op without a new
revision/history entry. Shared revision checks, same-process request replay,
selection-aware Undo/Redo and persistence apply over HTTP, CLI and operation
contract 1.

Only the target's two points change. Its ID, shape, parent/group membership,
page roots, sibling/child order, outgoing and incoming references remain intact,
as do other objects, chemical atoms/bonds, placement coordinates, stereo and
settings. Graphic arrows remain drawings without inferred reaction roles or
electron-flow meaning. This point-edit command adds no graphic type or persistence schema; current
scene-object history now uses native v30 with the width/ellipse/polygon profiles described in their sections.

The web **Graphic endpoints** panel edits one selected scene graphic's four
coordinate fields in pt, including a child within a group. It accepts finite
decimal/scientific numeric input within the same range; invalid, unchanged, busy
or IME-composing input cannot submit. The queued edit captures the exact document,
revision, target and points; a changed document/revision, moved/regrouped/removed
or replaced target lapses the edit instead of overwriting newer geometry. An
unconfirmed endpoint edit is not automatically resent into a restarted process.
The canvas paints the new stored geometry using the existing renderer.

This P36a slice does not add endpoint drag handles; P36b adds the bounded canvas
gesture below. It does not add general resize controls, new shapes, curved or
reaction/electron arrows, scientific glyphs, images, clipping, layers, graphic
style controls or automatic layout. Existing object movement/group/transform and
format-loss policies remain binding. P36 remains `in_progress`; authored checks,
both required reviews and PR CI are pending. Historical: that was the status when this slice was written; P36a has since merged as #204 (`26962b6`) with main CI run 42323974348 success (see the [P36 status table](HANDOFF.ko.md)). See
[the P36a record](verification/p36a-graphic-points.md).

### Single-graphic canvas endpoint drag (P36b candidate)

In Select mode, exactly one explicitly selected, noncoincident line, arrow or
rectangle exposes two endpoint handles. A directly selected grouped child is
eligible; a group, ancestor/descendant union, multiple or mixed graph/scene
selection is not. Coincident points remain valid stored geometry but expose no
ambiguous overlapping handles; use the existing numeric endpoint form instead.
This is a pointer gesture; the numeric form retains the coordinate entry path.

Press captures the document ID/revision, exact graphic, chosen point and pointer
position. Screen coordinates use the actual SVG inverse screen transform, so
prior pan/zoom is accounted for; missing, singular or nonfinite transforms refuse
an edit. The pointer delta preserves the press offset within the handle. Only
the chosen endpoint moves; the opposite stored point stays exact. Coordinates
retain the canvas's existing rounding and finite inclusive±100000 pt bounds,
without snapping or clamping. Space/middle-button pan takes precedence at press.
View navigation remains local and is not saved as graphic geometry.

Pointer movement beyond the existing 3-pixel threshold paints a local preview
through the normal graphic renderer and sends no transaction. On valid release,
the parent queues one captured `update_graphic_points` command; the shared P36a
validation and ordinary Undo/Redo apply. A click/subthreshold movement, unchanged
coordinates, invalid final bounds or cancelled gesture produces no geometry
change. Point ordering, shape, ID, grouping, references and chemistry stay intact.
After a larger excursion, returning below the threshold restores the stored
endpoint preview before release, while pointer capture remains active. Ordinary
whole-object movement clears its transient preview on the same return; its
existing selection/commit policy is unchanged. For one eligible selected graphic,
endpoint handles follow the same translated whole-object preview as the stroke
and return with it on cancellation.

Escape on the canvas, pointercancel, lost capture, blur, Fit or Reset discards
the preview and releases capture. Document/revision, selection, target geometry
or kind, group membership or tool changes invalidate the press context. The
queued callback checks document/revision/exact target again before posting.
Unknown edits are not automatically resent into a restarted service; the normal
reconciliation/reload path applies. The gesture introduces no command, schema, migration or scientific semantics;
current scene/history uses native v30 with the P36d width, P36h ellipse and P36i polygon profiles.

No bounding-box resize, aspect lock, multi-object/group handles, rotation handles,
new shapes, image controls, clipping, layers, styles or reaction/electron arrow
meaning is added. P36 stays `in_progress`. P36a PR #204 has merged. P36b
[#206](https://github.com/EunhakChoi/chemifree/pull/206) merged as `7601617` from
`f36550d` with successful [PR check/browser CI 42324595907](https://github.com/EunhakChoi/chemifree/actions/runs/42324595907).
Its [post-merge main run 42325796411](https://github.com/EunhakChoi/chemifree/actions/runs/42325796411)
has its delivery evidence tracked separately from P36d verification. Historical notes remain; see
[the P36b record](verification/p36b-graphic-endpoint-drag.md).

### Explicit canvas graphic creation (P36c candidate)

The **Graphic** tool (G) exposes an explicit Line, Arrow or Rectangle choice.
Initially no shape is chosen; a canvas press without a choice creates nothing.
With a choice, a primary-pointer drag can start over empty canvas or existing
atoms, bonds and graphics. It creates independent graphic geometry without
editing the chemical graph or implicitly selecting a group as its parent.
Existing Scene graphic-add buttons remain available.

Press captures document ID/revision, chosen shape, service generation and the
first document point. Actual SVG inverse screen coordinates account for prior
pan/zoom and use the existing 0.01 pt rounding. Both points must be finite and
within inclusive±100000 pt. At the existing 3-client-pixel threshold, movement
paints a local preview through the normal graphic renderer and sends no write.
Valid release submits one existing `add_graphic` command with the ordered press
and release points. Omitting parent/index creates a fresh page-root graphic in
the ordinary root order, even if a group is selected. The arrow tip is point 2;
rectangle corners retain their stored order. Existing degenerate/reversed point
support is unchanged; a click/subthreshold gesture creates nothing. There is no
snapping, aspect lock or clamping.
If the pointer moves out and then returns below the threshold, the creation
preview disappears immediately while capture remains active. Release there
creates nothing and sends no transaction; threshold crossing is not a permanent
commit latch. This also holds after returning one or two client pixels from press.

Space/middle-button pan takes precedence at press. Escape, pointercancel, lost
capture, blur, Fit and Reset discard previews; view navigation remains local.
Changed document/revision, tool, shape or observed service generation invalidates
the press context. Selection does not choose a creation parent. At submission,
the editor copies the chosen shape/points and records the existing selection for
the normal transaction history; the queued callback rechecks document/revision
and service generation. A restart not yet observed can cause an expired-session
request to refuse, but no automatic creation replay is sent to the replacement
process. No partial object is stored from a cancelled or stale preview.

Creation shares `add_graphic` atomicity, fresh IDs, revision/replay and one-step
Undo/Redo. Existing objects, hierarchy, references, graph atoms/bonds, placements,
stereo and settings remain unchanged except for the new page root. P36c introduced no command/schema/provider format change; current native v30
adds polygon admission to the P36d width/P36h ellipse profiles. Existing external-format losses remain.
The historical P36c slice added no shapes beyond line/arrow/rectangle, curve/electron/reaction-arrow
semantics, images, clipping, layers, style controls, automatic layout or group
insertion gesture. P36 stays `in_progress`. P36c #207 merged as `afe6410`; its
post-merge main run42329036170 failed in the browser job and #209 followed.
Implementation-session execution notes remain historical.
Creation-preview, endpoint-preview and ordinary-movement return-path defects were
reproduced and fixed during review follow-up; their separate commits and added
regression scope are recorded below. The currently running full smoke is pending. Historical: P36c merged as #207 (`afe6410`); its main CI run 42329036170 failed in the browser job and #209 followed (see the [P36 status table](HANDOFF.ko.md)).
See [the P36c record](verification/p36c-graphic-creation.md).

### Per-graphic line-width override (P36d candidate)

`{ "op":"set_graphic_line_width", "object":"s2", "line_width_pt":2.75 }`
sets an existing line/arrow/rectangle's explicit finite width within inclusive
0.1–10 pt. `line_width_pt` is required; explicit `null` restores dynamic document
width inheritance. Missing/wrong-type fields are `invalid_request`;
out-of-range widths use `invalid_command` with `line_width_out_of_range`. Missing and
wrong-kind targets retain `object_unknown`/`object_kind_mismatch`. A refusal is
atomic, including earlier edits/ID allocation. Identical stored overrides are
no-ops; effective changes share revision checks, replay and one-step Undo/Redo.

An explicit width equal to today's document width remains an override: later
document-width changes do not affect it. Resetting to null is a real metadata
edit even when the effective width is currently equal. New graphics inherit by
default; `add_graphic` itself gains no width input. Snapshots expose nullable
graphic `line_width_pt`; older snapshots may omit it as inherited. This additive
API default is separate from strict native v26 admission below.

Only graphic stroke width changes. Shape, points, object/graph IDs, hierarchy,
root/child order, references, selection, other graphics, chemical states, graph
positions/stereo and document/caption/label styles remain intact. Point updates,
movement and supported transforms retain the override. The renderer uses effective
width for graphic strokes, including the arrowhead outline, selection underlay
and transparent hit stroke. Arrowhead dimensions and point-based arrangement/fit
geometry keep their existing policies; no new stroke-aware layout is claimed.

For one explicitly selected graphic, the width form shows inherited/override and
effective width, accepts a bounded decimal/scientific value, and treats blank as
null. **Use document width** resets explicitly. Busy/invalid/IME/unchanged inputs
cannot submit, and captured document/revision/exact-target checks reject stale
queued edits. Width edits are not automatically replayed into a restarted service.

Historical P36d-only clipboard policy: v2 could not carry overrides, so a selected
subtree containing any explicit width refused with
`clipboard_scene_graphic_line_width_unsupported` and required explicit reset
before copying. P36e below supersedes that policy with v3 preservation; reset is
now an optional change to dynamic inheritance, not a Copy/Cut prerequisite.
The v2 reader remains strict and never admits a per-graphic width field.

Native v26 adds the nullable field in current/history/drafts and migrates v1–25
to inheritance with exact-byte archives before replacement; see the file-format
section. External chemical exports still drop scene graphics with `scene_object`
loss, rather than claiming styled-graphic roundtrip. Colours, dashes, fills,
arrowhead controls, broader shapes/layout and scientific arrow meaning remain
outside this slice. P36 stays `in_progress`. The parent reports 27 targeted Rust
tests passed for `e6e3b45`; this documentation work reran none. Web/full checks,
reviews and PR CI remain pending. Preceding P36c #207 merged as `afe6410`; its
[post-merge main run42329036170](https://github.com/EunhakChoi/chemifree/actions/runs/42329036170)
failed in the browser job and #209 followed. Historical: that was the status when this slice was written; P36d has since merged as #208 (`53e7437`) with main CI run 42333431290 success (see the [P36 status table](HANDOFF.ko.md)). See [the P36d record](verification/p36d-graphic-line-width.md).

### Styled graphic forest clipboard (P36e candidate)

The existing graphics Copy/Cut/Paste operations preserve nullable line-width
metadata over the same graphic/group forest. Copy output is canonical v2 when
all included graphics inherit; any explicit override selects v3, even when equal
to the document width. In v3 every graphic has required `line_width_pt: null |
<finite 0.1–10>`; inherited siblings store null and group records gain no field.
The v2 output shape is unchanged and has no width field. Widths outside the
selected forest do not choose its payload version.

These v2/v3 profiles remain closed schemas; P36h separately adds v4 for
ellipse-containing output, as described above. V2 forbids the graphic field even when null;
v3 requires it on every graphic and rejects missing/wrong-type/out-of-range or
nonfinite values. V3 with all-null graphic widths is valid paste input, although
copying the resulting inherited forest emits v2. A v3-only field hidden behind
a v2 header is not a migration. Duplicate/unknown fields, bad trees/references,
unsupported kinds and future versions retain structured refusal. A v2-only
consumer cannot be assumed to read styled v3, and no silent downgrade strips
overrides for it.

Paste preserves explicit numbers and inherited null through copied ordered
geometry, fresh IDs, groups, internal references, offsets and selection history.
Exact destination document-settings matching remains required. Cut preserves
the payload before deleting the exact prepared source, retaining the existing
incoming-reference, source/process and uncertainty guards. Failed/stale operations
retain the source/destination under existing atomic/replay/Undo/Redo rules. Width
is part of the graphic state, without extra width commands; existing 64 KiB,
500-command and scene resource limits remain. V3's extra bytes can reach the
same byte limit sooner than a corresponding v2 payload.

The dialogs state that explicit widths are preserved and inherited widths follow
document width, and show per-graphic inherited/override values from strictly read
payloads. Before preview or Copy-reply validation, JSON grammar and duplicate
decoded object keys are checked: identical duplicates, escaped-equivalent keys
and nested coordinate/settings duplicates are all refused. Settings receive only
the backend's legacy omission defaults for `label_weight`, `label_style`,
`label_underline`, `label_outline` and `label_shadow`, then complete shared
document-settings validation. Required base fields, unknown keys, wrong types,
explicit nulls and out-of-range values are not repaired. Thus syntactically valid
JSON with invalid settings is not presented as a valid style list. **Use document
width** remains available in the width form to restore inheritance explicitly;
it is no longer required for Copy/Cut. Reset can change appearance, and never
happens implicitly during transport.

Graph clipboard v1, native document/draft v26 and its migrations, document settings,
chemical graph/placements/stereo and external-format losses are unchanged. This
does not add colour/dash/fill/arrowhead styles, captions/instances in forests,
external graphics formats or broader P36 completion. P36 stays `in_progress`.
The independent duplicate-key preview finding and Codex invalid-settings preview
finding were addressed with focused regressions. Their fixes and agent-reported
web validation are recorded separately from pending full checks and CI; this
documentation pass reran no product checks. Historical: that was the status when this slice was written; P36e has since merged as #210 (`d1f257c`) with main CI run 42335557133 success (see the [P36 status table](HANDOFF.ko.md)). See
[the P36e record](verification/p36e-styled-graphics-clipboard.md).

### Finite arbitrary-angle rotation input (P36f candidate)

The Scene **Rotate selected geometry** form adds a finite decimal/scientific
degree input beside the existing fixed rotations/reflections. There is no
separate magnitude cap. Positive angles rotate clockwise in page coordinates;
negative angles counterclockwise. The requested number is sent unchanged to
existing `transform_selection`; the domain retains its floating-point modulo360,
quarter-turn and trigonometric policy. UI full-turn identities submit no command.
Blank, malformed/nonfinite input, empty/invalid geometry and IME composition
cannot submit. Changing documents resets the angle draft.

Submission captures document ID, raw selected object/atom/bond IDs, the active
placement and numeric angle. It deliberately does not freeze a revision or pivot.
When the callback reaches the ordinary editor queue, it resolves those captured
targets in the latest document geometry and recomputes the union-bounds center,
then submits that explicit pivot under the latest revision. Prior geometry edits
can therefore move the pivot; later live selection/placement/input changes do
not redirect the captured rotation. A different document, missing captured target
or required placement, caption-containing scope or empty geometry lapses the
whole action. Scene-only targets need no active graph placement; raw atom/bond
targets retain their explicitly captured placement. Bounds exclude ink/strokes.

Captions, including grouped descendants, block the UI action without a request.
Rectangle-containing scopes are not silently filtered or UI-rewritten: an
arbitrary non-quarter-turn can be submitted and the backend refuses the entire
transaction with `transform_rectangle_unsupported`. Exact quarter turns remain
supported; rotated coordinate-limit failures likewise refuse atomically. Direct
API validation still applies even to identity/empty requests; UI no-command
filtering does not relax that contract.

This form uses the existing mutation/restart reconciliation policy. It adds no
special no-replay rule like the captured graphic creation/endpoint/width edits;
a blanket promise that rotations are never retried after restart would be false.
It directly queues a transaction, without a new `transform_plan` preview step,
canvas rotation gesture or custom-pivot input. Stored stereo/marks, graph state,
graphic shape/width, hierarchy/references and other styles retain existing
geometry-only invariants; the drawing can disagree with stored stereo.

Public API/catalog/native26/provider formats are unchanged. P36 remains
`in_progress`; tests, reviews and CI are pending. A browser check currently in
progress is not a pass. Historical: that was the status when this slice was written; P36f has since merged as #211 (`d1920be`) with main CI run 42337029831 success (see the [P36 status table](HANDOFF.ko.md)). See [the P36f record](verification/p36f-selection-rotation.md).

### Same-parent scene order boundaries (P36g candidate)

Scene **First**/**Last** buttons move one selected scene object directly to the
beginning/end of its current parent's sibling list, avoiding repeated
Earlier/Later presses. Index 0 is earlier in ordinary scene painting order and
the final index later, within that parent. This introduces no separate layer,
global z-index, reparenting or overlay-order system.

The condition is exactly one selected scene object, not exactly one selected ID.
Accompanying valid atom/bond IDs remain in the ordinary selection/history and
are not reorder targets. Graph-only or multiple scene-object selections have no
boundary action; selecting both ancestor and descendant is not silently reduced
to one. A selected group moves as one subtree with its internal child order,
geometry, styles and references intact. Root objects use page roots; a grouped
object uses only that group's children. Existing scene object kinds retain their
ordinary `reorder` eligibility.

Press captures document ID, object ID, parent (including null for page root) and
the requested boundary. It does not freeze revision, sibling count or geometry.
When queued, the callback checks the same document/object/parent and calculates
index 0 or `latest siblings.length - 1` from the latest same-parent list, under
the latest revision. Earlier additions/removals/reorders can change that index;
later live selection cannot redirect the target. At the shown boundary the
button is disabled, and reaching it in an earlier queued edit skips the command
as a no-op. Singleton sibling lists likewise produce no boundary edit.

A different document, deleted target, changed parent, missing/non-group parent
or missing sibling membership lapses the action. It does not follow a root into
a newly created group or substitute another object/index. The ordinary editor
mutation/restart reconciliation policy applies; no service-generation capture or
special no-replay rule is added.

Each effective action submits one existing `reorder` command. Normal atomic
validation, selection-aware Undo/Redo, request replay and save/reopen apply.
Only the sibling list and its canonical depth-first scene traversal order change;
IDs, geometry, graph/placements/stereo, style/width, containment and references
remain. Public API/catalog/native26/provider formats are unchanged. Multi-object
boundary moves, layer visibility/locking/blending, new grouping/reparent controls and
new scientific semantics are outside this slice. P36 stays `in_progress`.
Browser/full/review/CI results are pending; preceding P36f #211 is open, not a
confirmed merged dependency. Historical: that was the status when this slice was written; P36g has since merged as #212 (`6545af6`) with main CI run 42339112423 success (see the [P36 status table](HANDOFF.ko.md)). P36f #211 merged as `d1920be` (main CI run 42337029831 success). See [the P36g record](verification/p36g-scene-order.md).

## Effective selection scope and movement (P32a)

The bounded P32 delivery is complete; see the [completion evidence](verification/p32-selection-completion.md). `document.selection_scope` is a read-only operation taking `{ document_id, selection, instance? }` and returning `{ snapshot, effective: { objects, positions, graphics } }` with the exact document revision. A selection has `objects`, `atoms` and `bonds` arrays, each defaulting to empty; repeated IDs are deduplicated. IDs must already exist; these fields do not resolve transaction-local `$N` references. Results are ordered by domain identifier, with `positions` containing unique `{ instance, atom }` pairs in instance-then-atom order. It changes no geometry, stored selection, revision, dirty state or history; repeating a request inspects the current snapshot rather than replaying a previous result.

The scope is the union of selected scene subtrees, selected atoms and selected bond endpoints. `objects` includes the selected groups and their descendants; a placement contributes every atom position in that placement, and a graphic contributes its two points through `graphics`. Selected atom IDs and bond endpoints contribute positions in `instance`; omitting it is allowed only while there is exactly one placement when there are atom/bond targets. Scene-only and empty selections need no placement, but an explicitly supplied placement must exist and be an instance even then. Containment expands groups; references and chemical connectivity do not expand scope. A selected bond moves its endpoints rather than the connected component. The same atom in two placements is two position targets.

Transaction command `{ op: "move_selection", selection, delta: { x, y }, instance? }` resolves the same scope at that command's point in the transaction and translates each position/graphic once, including when parent/child, object/atom and atom/bond selections overlap. Geometry alone changes: graph connectivity, recorded atom/bond states, stored stereo, group membership and references are retained. Movement targets are independent of the transaction's `selection_before` / `selection_after`, which retain their existing history semantics. One command may move many targets within ordinary document bounds; it uses the normal transaction revision, replay and atomic Undo/Redo rules.

Unknown selected entities produce `invalid_command` with reason `selection_unknown`; an invalid explicit instance produces the existing object/instance refusal, and ambiguous placement requires an instance. Movement rejects non-finite deltas or any final coordinate outside ±100,000 pt. An invalid target or coordinate failure rejects the whole transaction, including preceding commands and earlier translated targets. Empty targets and a zero delta are no-ops after validation, with no new revision/history entry and no cleared redo branch. The scope inspection has no command index; a failing movement names its transaction command index when applicable.

P32a adds the shared HTTP/CLI primitives only. Rotation/reflection primitives are added by P32c below and bounded scene-object web controls by P32d. P32e adds bounded mixed canvas selection and movement below; snapping and structure cleanup remain outside these slices, and existing group and placement commands retain their earlier scope. The two new conformance cases cover scope union/reference exclusion, overlap movement/placement independence, history/no-op behavior and atomic refusals; their execution and final review evidence are recorded separately, not implied by this description.

## Scene object alignment and distribution (P32b)

The bounded P32 delivery is complete; see the [completion evidence](verification/p32-selection-completion.md). `document.arrangement_plan` takes `{ document_id, objects, mode }` and returns `{ snapshot, mode, units, scope }` without changing selection, geometry, revision, dirty state or history. Each unit contains `{ object, bounds: { left, top, right, bottom }, delta: { x, y }, translation, scope }`; bounds describe its original geometry. Unit and combined scopes have the same shape as `document.selection_scope`, including stationary targets. The plan preflights every translated point against coordinate bounds. It is snapshot evidence, not a commit token: callers use the snapshot revision as the later transaction's `expected_revision`, and the edit computes its plan against the geometry at that command.

`translation` is `null` for stationary units, or `{ axis: "horizontal" | "vertical", source, target }` for a moved unit. It defines the actual coordinate reconstruction: on the named axis, evaluate the exact three-term sum `coordinate + target - source` of the returned binary64 values, then round once to the nearest binary64 value with ties to even; preserve the other axis. An ordinary sequence of binary64 additions/subtractions can round intermediate values and produce a different result. `delta` is a nominal displacement for reporting and is not sufficient to reconstruct committed geometry. For example, moving a point from `99999.9` to `0.1` reports a nominal delta of `-99999.79999999999`; adding that delta produces `0.10000000000582077`, while the anchor translation commits exactly `0.1`.

The modes are `align_left`, `align_horizontal_center`, `align_right`, `align_top`, `align_vertical_center`, `align_bottom`, `distribute_horizontal` and `distribute_vertical`. Only existing scene objects are accepted, not partial atoms/bonds or transaction-local `$N` references. Duplicate objects are deduplicated; selected descendants of selected ancestors are absorbed into that ancestor's rigid unit. A unit contains all atom positions and graphic points in its subtree. References and chemical connectivity are not followed. Bounds exclude rendered labels, stroke width, arrowheads and other visual extents. An empty placement or group with no geometric points is refused rather than given invented bounds.

Alignment needs at least two outermost units and uses the union of their original bounds as reference: left/right/top/bottom align the corresponding edges; horizontal/vertical center align each unit's center on that axis to the union's center. Unit order is object identifier order, independently of input order. The other coordinate is unchanged. Distribution needs at least three units, ordered by original left/top edge then object identifier; the first and last stay fixed. Equal edge-to-edge gaps are computed as `(last trailing edge - first leading edge - sum of extents) / (count - 1)`; zero is allowed and a negative gap is refused. Units translate rigidly; widths/heights and internal geometry do not change.

Center alignment compares exact midpoint error before and after the actually rounded translation: a genuine improving move is retained, including a point-sized one-ULP move, while equally close unrepresentable midpoint poses remain unchanged. It also preflights the resulting bounds and their new union center. If applying the same center rule again would move any unit, the bounded single-materialization algorithm refuses the entire plan as `arrangement_center_unstable`; it does not apply hidden iterative translations or claim convergence. This refusal preserves all geometry, revision, selection and history, including the redo branch.

`{ op: "arrange_objects", objects, mode }` applies the same checked plan as one atomic transaction command, independently of history selection fields. It changes no chemistry, stored stereo, group membership, object order or references. Normal revision checks, request replay and Undo/Redo apply. Unknown objects, `arrangement_too_few_units`, `arrangement_empty_geometry`, `arrangement_negative_gap`, `arrangement_center_unstable` and coordinate failures refuse the whole transaction, including preceding commands. Already arranged geometry is a no-op after validation and preserves revision/history/redo. P32b itself adds no mixed atom selection or web controls. P32c adds rotation/reflection primitives, P32d adds bounded scene-object web controls and P32e adds bounded mixed canvas selection below. Snapping and cleanup remain outside the completed bounded P32 delivery.

## Selection rotation and reflection (P32c)

The bounded P32 delivery is complete. `document.transform_plan` takes `{ document_id, selection, pivot: { x, y }, transform, instance? }` and returns `{ snapshot, pivot, transform, scope, positions, graphics, drawing_stereo_may_change }`. `selection` and `instance` resolve exactly as in `document.selection_scope`: selected subtrees, explicit atoms and selected-bond endpoints form a deduplicated union; directed references and chemical connectivity add no targets. Targets must already exist; transaction-local `$N` references are not accepted here. The returned `transform` preserves the requested degree value. Positions are ordered by instance then atom and contain `{ instance, atom, before: { x, y }, after: { x, y } }`; graphics are ordered by ID and contain `{ object, shape, before: [point, point], after: [point, point] }`. Stationary targets remain in these arrays and in `scope`. The exact snapshot and checked coordinates are captured under one session lock. Preview changes no selection, geometry, revision, dirty state or history, and is not a commit token; repeat preview requests inspect the current state.

Transforms are `{ type: "rotate", degrees }`, `{ type: "reflect_horizontal" }` and `{ type: "reflect_vertical" }`, with no other fields. The explicit pivot must be finite and within the ordinary ±100,000 pt scene coordinate limits. Page x grows right and y grows down: positive rotation is visually clockwise. Finite degrees normalize modulo 360. Zero/full-turn identities return original points, and quarter turns use endpoint sums rounded once without sine/cosine approximations. General rotations use ordinary floating-point trigonometry. Horizontal reflection mirrors across the pivot's horizontal line (`x'=x`, `y'=2*pivot.y-y`); vertical reflection mirrors across its vertical line (`x'=2*pivot.x-x`, `y'=y`). Diagonal or angled reflection is outside this bounded profile.

A line or arrow retains its shape and endpoint order. A rectangle stores two opposite corners of an axis-aligned rectangle, so every selected rectangle—including group descendants and degenerate rectangles—requires an exact quarter-turn rotation. Any other angle refuses the entire command with `transform_rectangle_unsupported`; it is never replaced by a bounding box. Allowed quarter turns and axis reflections transform the existing opposite corners directly and preserve their identities. All final atom/graphic points are checked before applying any change.

`{ op: "transform_selection", selection, pivot, transform, instance? }` uses the same checked planner at that command's point in an ordinary atomic transaction. It changes geometry alone; graph connectivity, recorded atom states, bond marks, stored atom/bond configurations, enhanced stereo groups, hierarchy and directed references are preserved verbatim. This is not a chemical mirror operation: neither marks nor stored turns are inverted and no configuration is derived. Changed geometry, especially reflection or a partial-atom rotation, can disagree with stored stereo. `drawing_stereo_may_change` is a conservative preview advisory that is true exactly when at least one graph position changes; it is not a stereo assessment, transaction note or loss. It is false for graphics-only and exact no-op transforms. Use the existing `chem.stereo` inspection for actual drawing/stored agreement.

The command's geometry targets are independent of transaction `selection_before`/`selection_after`. Normal expected-revision checks, request replay and Undo/Redo apply. Unknown targets/instances, ambiguous placement, `transform_invalid_angle`, `transform_rectangle_unsupported` and coordinate-limit failures refuse the complete transaction, including preceding commands. Nonfinite JSON numbers and malformed transform/pivot fields fail request parsing; target and angle validation is not bypassed for empty selections or identity rotations. Empty or actually unchanged valid transforms preserve revision, selection, history and redo. The preview refusal has no command index; transaction refusals identify the failing command when applicable. P32c itself adds no web transform controls or implicit pivot choice; P32d supplies bounded web controls below. Diagonal reflection, snapping, structure cleanup and full P32 completion remain outside these slices.

## Web scene-object geometry controls (P32d)

The bounded P32 delivery is complete. The web Scene controls expose alignment, distribution, quarter-turn rotation and axis reflection for scene objects selected with the existing object toggles. Selected placements identify their own position sets, and a selected group contributes its subtree. Selected descendants of selected ancestors are absorbed into the outermost unit. The controls do not promote a partial atom/bond selection to a whole drawing. Alignment and distribution remain disabled with an explanation when explicit atom/bond targets are selected; [P32f](#mixed-selection-quarter-turns-and-reflections-p32f) extends the four transform buttons to mixed selection using the active placement. Existing scene grouping and ungrouping retain their scene-object scope.

The six alignment controls use the union of the selected units' geometry bounds and require at least two outermost units. Horizontal and vertical distribution require at least three and equalize edge-to-edge gaps while keeping the first and last units fixed, under the P32b rules above. Bounds use atom positions and the two graphic points, excluding labels, stroke width and arrowheads. Empty geometry is not assigned invented bounds; domain refusals, including a negative available distribution gap or coordinate-limit failure, leave the complete transaction unchanged.

Rotate 90° clockwise and Rotate 90° counterclockwise send quarter-turn transforms. Reflect across horizontal axis keeps x and mirrors y; Reflect across vertical axis keeps y and mirrors x. These four actions require nonempty selected geometry and use its union-bounds center as pivot. The client captures the selected object IDs at the press, then rebuilds the action and center against the latest document when its queued request runs, so repeated presses compose. It sends an explicit pivot to the existing `transform_selection` command; the service has no implicit pivot rule. Selected objects, including groups, retain their existing hierarchy and are transformed as the same effective geometric union.

The controls preserve graph data, bond marks, stored configurations and enhanced stereo groups. They explain that changed geometry can disagree with stored stereo; the existing stereo inspection assesses that agreement. Each successful action uses the ordinary atomic transaction, history selection, revision and Undo/Redo rules, and an unchanged result preserves the redo branch. P32d itself adds no mixed-object canvas marquee or mixed-selection dragging; P32e supplies those bounded interactions below. P32d introduced no custom angle/pivot controls. P36f adds the separate finite-angle form candidate; custom pivots, snapping and structure cleanup remain unsupported. View navigation retains its earlier scope.

## Mixed canvas selection and movement (P32e)

The bounded P32 delivery is complete. The web editor has an active placement for partial atom/bond selection, defaulting to the first placement and exposed by a placement selector. This context is transient client state, not saved document content. If the active placement is removed, partial graph movement and highlighting require choosing a remaining placement or clicking a graph target again; the client does not silently retarget the selection. Atom and bond selection underlays appear in the active placement only. Clicking an atom or bond in another placement changes that context and discards the previous atom/bond selection; with Shift, selected scene objects are retained before toggling the clicked graph target. Changing the context with the placement selector also clears atom/bond targets while retaining selected scene objects. A plain target click replaces selection unless the target is already selected for a move; Shift toggles targets. Whole placements and groups remain selectable through the Scene list. A partial graph selection is never promoted automatically to a placement or group.

Dragging a marquee on empty canvas selects atom centers from the active placement and graphics whose two stored points are both inside the box, with inclusive edges. It adds no bond IDs, does not select partial graph targets from other placements and does not promote contained geometry to whole placements or groups. Shift unions the enclosed targets with the selection captured at the press; without Shift the enclosed targets replace it. A blank click clears selection, while a Shift blank click retains it. These rules use atom centers and stored graphic points, not rendered labels, strokes or arrowheads.

Dragging a selected atom, bond or graphic moves the captured mixed selection with one `move_selection` command and an explicit active placement for partial graph targets. The effective union contains selected scene subtrees, selected atoms and selected-bond endpoints; each `{ instance, atom }` position and graphic moves once even when selected ancestors, descendants and graph targets overlap. Selected placements keep their own position sets, independently of the active placement. Directed references and chemical connectivity add no targets. The transient move preview follows that same geometric union. The edit changes geometry alone, retaining marks, stored configurations, enhanced stereo groups, hierarchy and references; normal revision, atomic refusal, history selection and Undo/Redo rules apply.

Scene Group/Ungroup retain their existing scene-object rules; mixed canvas selection does not turn atoms or bonds into scene-group children. P32d alignment and distribution controls still accept scene-object selection only and remain disabled when explicit atom/bond targets are selected. [P32f](#mixed-selection-quarter-turns-and-reflections-p32f) extends the quarter-turn and axis-reflection buttons to mixed selection. P31 view navigation, pointer capture, click/drag threshold and gesture cancellation retain their existing behavior: panning does not select or move document content, and cancelled gestures commit nothing. P32e itself adds no snapping, custom angle/pivot controls or structure cleanup. P36f separately adds a finite-angle form; custom pivots and broader scientific/layout functionality remain unsupported.

## Mixed-selection quarter turns and reflections (P32f)

The existing Rotate 90° clockwise, Rotate 90° counterclockwise, Reflect across horizontal axis and Reflect across vertical axis web buttons accept the same mixed selection as canvas movement. Selected scene subtrees, explicit atoms and selected-bond endpoints form one effective geometric union. A raw atom/bond target uses the shared transient active placement described in P32e, sent explicitly as `instance`; a scene-only action needs no graph placement context. Scene placements retain their own position sets. Positions and graphics are deduplicated, including overlaps between selected ancestors, descendants and graph targets. Directed references and chemical connectivity add no targets. Alignment and distribution retain the P32d scene-object-only restriction.

A button press captures the raw selected object/atom/bond IDs, document ID and the graph movement instance when graph targets are present. When the request reaches the editor queue, the client validates those captured targets against the latest snapshot of that document, resolves their effective geometry and computes its union-bounds center. Bounds use atom positions and the two graphic points, excluding rendered labels, strokes and arrowheads. The center is sent as an explicit pivot to one `transform_selection` command under that latest snapshot's expected revision. A missing target or required placement, an empty geometric union or a different current document makes the complete captured action lapse; the client does not silently shrink its selection or substitute another placement. Changing the live selection or active placement while the request waits does not redirect the captured action. Repeated valid presses compose against the queued geometry.

The four transforms retain the P32c angle and axis semantics: positive 90° is visually clockwise; horizontal reflection mirrors y across the pivot's horizontal line and vertical reflection mirrors x across its vertical line. The command changes geometry only, leaving the selection scope, graph data, bond marks, stored configurations, enhanced stereo groups, hierarchy and references intact. No mark is swapped or stored configuration inverted. Changed geometry can disagree with stored stereo; the existing stereo inspection assesses that agreement. Normal atomic refusal, revision, history selection and Undo/Redo behavior applies.

The bounded P32 delivery is complete; see the [completion evidence](verification/p32-selection-completion.md). P32f itself added no custom angle/pivot controls. [P36f](#finite-arbitrary-angle-rotation-input-p36f-candidate) adds the separate arbitrary-angle form candidate; custom pivot, snapping, structure cleanup and new transform semantics remain outside it.

## Native graph-fragment copy and paste (P33a)

Bounded P33 is `done` against the roadmap scope; [completion evidence and limitations](verification/p33-clipboard-completion.md) record successful PR and post-merge main CI separately. `document.clipboard_copy` takes `{ document_id, selection: { objects?, atoms?, bonds? }, instance }`, with `instance` a required existing placement ID even when there is only one placement. It returns `{ snapshot, instance, copied: { objects: [], atoms, bonds }, boundary_bonds, payload }`. `snapshot` is the exact ordinary document result (including `notes`) captured with the source graph; `copied` names the induced fragment in source-ID order, and `boundary_bonds` lists omitted crossing bond IDs. Copy changes no geometry, selection, revision, history or dirty state. Boundary bonds are informational result data, not losses, and copy has outcome `ok` with an empty `loss_report`.

`document.clipboard_paste` takes `{ document_id, expected_revision, payload, offset: { x, y }, selection_before? }` and returns `{ snapshot, mapping: { atoms: [{ source, target }], bonds: [{ source, target }] } }`. `payload` is the native JSON text string returned by copy, rather than an inline object. The resulting `snapshot` is the ordinary document result including `notes`; the operation is `partial` exactly when those notes are nonempty, with the same entries in `loss_report`, and otherwise `ok`. Replay binds the exact payload text, offset, expected revision and optional history selection to the request ID and returns the original result, including its mapping, rather than allocating again.

The bounded native clipboard profile copies and pastes graph fragments through shared application operations. It is a versioned data-only JSON representation with format `chemifree.clipboard` and version `1`, not an executable command list. These backend operations do not access the operating-system clipboard themselves. The P33b web adapter below adds explicit native text transport and P33c adds bounded explicit Cut, P33d adds the explicit MOL bridge, P33e adds the separate bounded graphic-forest backend, and P33f adds its explicit browser dialogs. Molecule-instance cloning and other external clipboard formats remain unsupported.

Copy accepts graph selection only; selecting scene objects is refused rather than interpreted as a request to copy their subtree. The source placement is explicit even when the document has only one placement. The copied atom set is the union of explicitly selected atoms and selected-bond endpoints. Every bond whose two endpoints belong to that set is included, independently of whether the bond was explicitly selected. Bonds crossing its boundary are omitted and reported. References and chemical connectivity do not expand the copied atom set. The report describes that boundary change; copying a fragment does not establish chemical equivalence with its source.

Recorded atom and bond states, bond marks and complete stored 0D stereo records are preserved. A stored configuration that requires an omitted neighbor is refused rather than dropped or reinterpreted. Selecting any member of an enhanced stereo group is refused in this slice, including when every group member would otherwise be copied. No stereo configuration is derived from the picture and no claim is made that the fragment's drawing agrees with its stored stereo.

Paste requires a destination with exactly one placement and settings equal to those captured in the payload. It applies an explicit coordinate offset, creates fresh atom/bond identifiers and returns the source-to-destination identifier mapping. It adds the complete fragment in one ordinary atomic transaction, with the pasted targets selected afterward. Normal expected-revision checks, request replay, Undo/Redo, graph and coordinate limits apply; a refusal leaves the document, selection and history unchanged. Pasted identifiers do not reuse deleted identifiers.

The closed payload is `{ format: "chemifree.clipboard", version: 1, settings, atoms, bonds, stereo }`. `settings` has the document settings shape. Each atom is `{ id, kind, charge, isotope, hydrogens, radical, attachment_points, position: { x, y } }`, where `kind` is `{ type: "element", symbol }` or `{ type: "query", query }`; the recorded state fields use the same meanings and limits as document atom states. Each bond is `{ id, begin, end, order, query, mark }`, using the document bond-order/query/mark shapes. `stereo` entries are `{ type: "atom", atom, configuration }` or `{ type: "bond", bond, configuration }`, with the stored configuration shapes documented below. The `isotope`, bond `query` and bond `mark` keys are required even when their value is `null`; no record defaults fill missing keys. A native atom query of kind `list` also requires an explicit `negated` boolean, including `false`; the public edit command’s default does not apply to native clipboard records. Fragment IDs are local references, never destination addresses. Unknown fields, duplicate JSON keys, duplicate entity/stereo IDs, dangling references and invalid states are refused rather than ignored.

Malformed payload data is `invalid_command` with reason `clipboard_fragment_invalid`. The typed format/version header is read before the closed version-1 body schema: a valid future-version header uses `clipboard_format_unsupported` even if its body has additional fields or omits version-1 fields. A malformed header or malformed version-1 body uses `clipboard_fragment_invalid`; an unsupported format/version uses `clipboard_format_unsupported` and excess UTF-8 bytes use `clipboard_byte_limit`. Other bounded refusals are `clipboard_scene_selection_unsupported`, `clipboard_empty_selection`, `clipboard_selection_unknown`, `clipboard_instance_invalid`, `clipboard_enhanced_stereo_unsupported`, `clipboard_stereo_boundary`, `clipboard_duplicate_atom`, `clipboard_duplicate_bond`, `clipboard_duplicate_stereo`, `clipboard_destination_instances`, `clipboard_style_mismatch`, `clipboard_command_limit` and `clipboard_identifier_limit`. An internal precommit mapping invariant failure uses `clipboard_mapping_invalid` and commits nothing. Malformed operation parameters, including a missing explicit copy instance, are `invalid_request`. The source/destination settings comparison is exact equality of all stored setting values; paste does not rescale or restyle the fragment.

The native payload string is limited to 64 KiB of UTF-8. The projected edit must fit the ordinary 500-command transaction limit as well as domain entity caps. The representation carries data for the bounded profile only; accepting its JSON never authorizes arbitrary document commands. This bounded profile is not a general interchange format; P33 completion covers only the profiles and limits recorded in the completion evidence.

## Explicit native clipboard dialogs (P33b)

The web editor offers **Copy fragment…** and **Paste fragment…** for the bounded native profile. Copy captures the document ID, graph selection and shared source placement when pressed. Its queued read runs after earlier edits and validates the complete captured selection and placement against that document's latest snapshot. It does not substitute a placement, silently omit missing targets or reinterpret scene objects. The prepared result states its source revision, copied atom/bond counts and omitted boundary bonds. Preparing it changes no document state, history or selection; its inspection snapshot never replaces the editor's drawing.

Preparation does not write the system clipboard. A second **Copy to clipboard** click writes the prepared JSON as plain text, directly within that user action. A read-only text field always provides manual copying; **Select native text** focuses it and selects the complete prepared payload, including when the field already had focus before preparation finished. This selection action never accesses the system clipboard. Clipboard availability or permission failure leaves that fallback available and never reports a successful write. Closing the dialog, switching documents or changing service generation invalidates obsolete asynchronous UI results.

Opening Paste does not read the clipboard. Users can paste native text into its ordinary textarea, or explicitly press **Read clipboard**. That button alone invokes the browser read API; reading does not send the text to the service. **Paste fragment** submits the entered text to the local service. The dialog exposes horizontal and vertical offsets in stored point units, initially one destination bond length on each axis. It preserves source coordinates plus these offsets, without viewport fitting, pointer placement or cascading repeated pastes. Repeating the same offsets may overlap earlier copies. Paste requires one destination placement and exactly matching settings; the backend remains authoritative for all native-profile refusals.

Submission freezes text, offsets, destination document ID and selection-before. At queue execution it uses that document's latest revision, refusing a different current document or missing captured selection targets. The atomic reply selects the fresh targets and participates in ordinary Undo/Redo. Failed reads, malformed text and refused pastes do not change the destination. The adapter does not guess external chemical formats, repair fragments, rescale settings or infer stereo.

A lost or still-running paste reply is checked using its original request ID and exact body. An unresolved paste blocks further queued document actions and offers **Check paste outcome**; it is never turned into a fresh paste automatically. A detected service restart does not resend that attempt to the new process. A terminal revision conflict after uncertainty can also mean that replay expired after an earlier commit, so it is not reported as proof that the original paste failed. Both cases offer **Reload document to reconcile** and read the authoritative drawing before releasing the block. Without a confirmed restart or terminal revision conflict, a read from the original process cannot abandon a request that may still commit there. Late dialog callbacks cannot replace another dialog's text or another document's state.

Clipboard transport uses plain text only, without HTML or custom MIME data. The page performs no background reads, clipboard polling, payload logging or clipboard persistence in localStorage/recovery drafts. Temporary dialog text is cleared on dismissal or document switch; only in-memory request tracking remains while a mutation is unresolved. Dismissal never erases the operating-system clipboard. Browser permission and activation policies can prevent programmatic access, so manual text copy/paste remains supported. No global Ctrl/Cmd+C/V/X override is added: text fields retain native editing behavior. P33c adds the bounded explicit Cut below; P33e/P33f provide the separate bounded graphic forest profile; other external clipboard formats remain follow-ups, and Bounded P33 completion with successful PR and post-merge main CI is recorded in the [completion evidence](verification/p33-clipboard-completion.md).

## Explicit native Cut of disconnected fragments (P33c)

**Cut fragment…** prepares the same native representation from the captured graph selection and explicit source placement. It adds three eligibility restrictions: exactly one source placement, no crossing bonds, and no scene references targeting copied atoms or bonds. Existing Copy refusals for scene-object selection, enhanced-stereo-group members and incomplete stored stereo remain. The fragment can contain several disconnected components, but cannot cut through a component boundary. These restrictions avoid discarding extra placement coordinates, uncopied boundary bonds or affected outside stereo. Copy retains its broader induced-fragment behavior.

The dialog states the copied atom/bond counts. **Cut to clipboard** removes the copied atoms, including selected-bond endpoints, rather than merely deleting the originally selected bonds. It invokes the browser's plain-text write directly in the user action, then queues deletion behind the write's completion. A failed or unavailable write sends no deletion request. **Select native text** and ordinary Copy remain manual alternatives; manually selecting or copying text never authorizes deletion.

The prepared source snapshot/revision, raw selection-before, expanded copied atom IDs and service generation are frozen. After a successful write, queue execution requires that exact source drawing and original service generation, rather than rebuilding against the latest revision. A changed drawing, document or service leaves a copy on the clipboard and sends no Cut deletion. Changes to live selection or active placement cannot redirect the prepared Cut. Eligible deletion sends one ordinary atomic `delete_atoms` transaction under the frozen expected revision, with the captured selection-before and empty selection-after. Its Undo/Redo follows the existing document history and restores the original IDs and selection; it does not restore or modify the system clipboard.

Clipboard writing and document deletion are separate operations, not a cross-system atomic transaction. A successful copy followed by a refused deletion leaves the source unaffected by Cut; the successful clipboard write is not rolled back. No earlier clipboard value is read or restored. Other applications can subsequently overwrite the clipboard. Cut does not promise that the operating-system clipboard remains unchanged after its write, derive chemistry or automatically paste data back on failure.

An uncertain deletion retains its exact request ID/body/process binding and blocks later document actions. **Check cut outcome** checks that deletion attempt only, without another clipboard write or fresh deletion. A detected restart never resends the attempt to the new process. As with Paste, a held old-process success cannot replace the drawing after the service generation changes, and explicit authoritative reload/reconciliation is required after restart or terminal revision conflict. Closing the dialog does not cancel an already submitted Cut or discard unresolved request tracking. No Cut payload is persisted in browser storage, no Ctrl/Cmd+X editor override is added, and there is no manual-copy confirmation that bypasses the successful-write requirement. Bounded P33 is complete within the [recorded scope](verification/p33-clipboard-completion.md); cuts through graph boundaries and cuts from multiple placements remain unsupported. P33e/P33f/P33j provide the separate graphic-forest profile; P33d/P33g/P33h/P33i provide explicit chemical-text bridges.

## Explicit MOL-text clipboard bridge (P33d)

The existing MOL/SDF panel adds a plain-text clipboard bridge for the already supported MOL profile. It does not auto-detect clipboard formats or reinterpret native fragment selection. MOL export still writes the entire drawing using the existing first-placement projection, irrespective of transient canvas selection. V2000, V3000 and Auto use the existing explicit dialect choice, bounded codec refusals and digest-bound export loss-consent flow. Scene hierarchy, extra placements, native IDs, settings, selection and unsupported scientific representation remain subject to the codec's existing losses and refusals; MOL clipboard text does not preserve a native scene subtree.

After export preparation and any required consent, **MOL clipboard text…** exposes the prepared content in a read-only text field. **Select MOL text** selects the complete text for manual copying. **Copy to clipboard** invokes the browser's plain-text write directly within that user action, without awaiting another backend request first. The prepared drawing snapshot, dialect-choice generation and service generation must still match; changing the choice invalidates earlier content even if the user switches back. No clipboard success is reported until the write succeeds. Denied or unavailable access leaves manual copying available and changes no document state.

**Import MOL text…** opens an empty text form without reading the clipboard. Users can enter/paste text normally or explicitly press **Read clipboard**. Reading alone submits nothing to the service. **Preview MOL text** sends the unchanged text as `format: "mol"`, `record: 1` through the existing `document.import` preview. This clipboard form is limited to 200 KiB of UTF-8; the server remains authoritative for codec and domain limits. It does not guess SMILES, SDF, CDXML/CDX, JSON, HTML or executable content. The ordinary file import/export controls retain their existing formats and behavior.

The preview displays imported counts and all reported losses. A separate **Import MOL text** confirmation accepts those losses under the exact preview digest, source bytes, destination snapshot/revision and service generation. Editing text, changing the destination or restarting the service invalidates the preparation. Missing or stale preview/consent and malformed, unsupported or over-limit input are refused atomically. No frontend chemistry repair, hidden rescaling or stereo inference is added.

Import appends fresh graph entities into exactly one destination placement using the existing MOL coordinate conversion and codec stereo/group rules. It may overlap existing content. This is the existing external import operation, not native fragment Paste: there is no native coordinate-offset control, matching-native-style requirement or automatic selection of imported targets. The web editor preserves its live selection of still-existing entities, and the transaction keeps the existing import history/selection behavior. Ordinary Undo/Redo applies. Native Copy/Paste/Cut retain their separate scope and safeguards; MOL clipboard export never authorizes Cut.

Clipboard MOL commits retain one immutable request ID/body and original service binding across lost replies, in-progress polling and explicit outcome checks. An unresolved import blocks later document actions and offers **Check MOL import outcome** in the MOL panel, including after the dialog closes. Checks do not reread or rewrite the operating-system clipboard, and never create a fresh import automatically. Service restart or terminal revision conflict requires explicit authoritative reload/reconciliation; a held old-process reply cannot replace the drawing after the service generation changes. Closing the dialog does not cancel an already submitted import or discard unresolved tracking.

Both clipboard dialogs are viewport-bounded with scrollable content, selectable bounded text fields and native input editing. Closing or switching documents clears temporary input and invalidates delayed UI callbacks. No background clipboard read, clipboard polling, payload logging, browser-storage persistence, custom MIME negotiation or global clipboard shortcut is added. Bounded P33 is complete within the [recorded scope](verification/p33-clipboard-completion.md); external clipboard support in this slice is MOL text only. Other chemical clipboard formats, molecule-instance cloning and boundary/multiplacement graph Cut remain follow-ups; P33j adds bounded graphic forest Cut below; the separate P33e/P33f profile supports only the bounded graphic/group forest below.

## Native graphic-subtree clipboard backend (P33e)

`document.clipboard_scene_copy` takes `{ document_id, selection: { objects?, atoms?, bonds? } }` and returns `{ snapshot, copied: { objects, atoms: [], bonds: [] }, roots, payload }`. It accepts nonempty object-only selection. Selected descendants of selected ancestors are deduplicated, and the outermost selected subtrees are copied in depth-first source painting order, with their group child order intact. `copied.objects` names every included object in that order; `roots` names the copied subtree roots in source order. Unselected parents are outside scope, so selected children become clipboard roots. Copy changes no source content, selection, revision, history or dirty state and needs no molecule-placement context.

The bounded forest contains only lines, arrows, rectangles and groups. A molecule instance anywhere inside the scope, any explicit atom/bond target, an unknown target or an empty selection refuses the entire copy. Groups with fewer than two children also refuse: although such groups can remain after other edits, the existing group-creation primitive cannot reconstruct them exactly. Copy does not flatten groups or manufacture temporary graphics. No graph connectivity or scene reference expands the selected scope.

Every reference owned by an included object must target another included object. Those internal links are preserved, including directed cycles and self-references. Outgoing references to excluded objects, atoms or bonds refuse rather than being dropped or followed. Incoming references owned by excluded objects remain solely in the unchanged source and are outside the copied scope. Reference links carry no chemical interpretation and remain separate from containment; the containment forest must be acyclic and contain every object exactly once.

The strict data-only graphics payload is `{ format: "chemifree.clipboard", version: 2 | 3, kind: "graphics", settings, roots, objects, references }`. V2 graphics are `{ kind: "graphic", id, shape, points: [{ x, y }, { x, y }] }` with no width field. V3 adds required nullable `line_width_pt` to every graphic, preserving explicit widths within0.1–10 pt and null inheritance; see [P36e](#styled-graphic-forest-clipboard-p36e-candidate). Shape is line/arrow/rectangle. Groups remain `{ kind: "group", id, children }`, references `{ source, target: { kind: "object", id } }`. Containment uses ordered roots and group children without a redundant parent field. Every version-specific field is required; v2 rejects even a null width, while v3 accepts all-null input. Copy output uses v2 for all inheritance and v3 for any explicit width.

Unknown fields/tags, duplicate JSON keys, duplicate object/reference records, dangling or repeated children, shared/cyclic containment, unreachable objects and invalid settings/points/widths refuse. Clip-local `sN` identifiers never address destination entities.

The common typed format/version header is classified before imposing the closed graphics schema, and the known version-2/version-3 header then requires a typed `kind`. Unsupported valid format/version/kind headers return `clipboard_format_unsupported`, even when their bodies differ; malformed headers and malformed known bodies return `clipboard_fragment_invalid`. UTF-8 payload text is limited to 64 KiB (`clipboard_byte_limit`). Graph version 1 remains unchanged and is not accepted by these scene operations; ordinary graph clipboard paste still refuses version 2. These are separate profiles and operations, not arbitrary transaction import.

`document.clipboard_scene_paste` takes `{ document_id, expected_revision, payload, offset: { x, y }, selection_before? }` and returns `{ snapshot, mapping: { objects: [{ source, target }] } }`. It requires exact equality of all stored settings, applies the explicit finite point offset to both graphic points and allocates fresh object IDs for every copied group and graphic. Internal reference endpoints are remapped only to those fresh IDs. Copied roots append to the destination page in source order without an implicit wrapper or a live-selection parent; existing root order is unchanged. Mapping entries follow source depth-first painting order, which need not be numeric destination-ID order.

Paste leaves all chemical graph states, stereo, molecule instances and their positions unchanged and accepts destinations with multiple placements. The result selects only the fresh roots, while ordinary selection-before/after history makes Undo restore the prior selection and Redo restore the pasted forest. Fresh IDs are not reused after Undo or deletion. The complete forest is preflighted against the same locked destination revision, including translated coordinate bounds, identifier exhaustion, the 1,000-object/16-level/4,096-reference limits and the actual 500-command transaction budget (one command per object plus one per reference, with widths carried in graphic state). Any refusal preserves content, selection, revision, dirty state and redo history.

The request ID binds exact payload text, offset, expected revision and optional selection-before. Each offset component binds its exact floating-point bits, so changing either signed zero is a different request. Replay returns the original immutable snapshot and reconstructs its complete ID mapping from that snapshot's selected roots and preserved child order, even after later edits, allocations or history changes. This is a lossless copy within the declared graphic-only scope: successful operations have outcome `ok` with an empty loss report; unsupported states refuse instead of asking to omit them. Domain refusals use `clipboard_scene_graph_selection_unsupported`, `clipboard_scene_empty_selection`, `clipboard_scene_selection_unknown`, `clipboard_scene_instance_unsupported`, `clipboard_scene_group_too_small`, `clipboard_scene_invalid_tree`, `clipboard_scene_depth_limit`, `clipboard_scene_object_limit`, `clipboard_scene_reference_unsupported`, `clipboard_scene_duplicate_reference`, `clipboard_scene_style_mismatch`, `clipboard_scene_command_limit`, `clipboard_scene_identifier_limit` or `clipboard_scene_fragment_invalid`. `clipboard_scene_graphic_line_width_unsupported` is retained for historical diagnostic compatibility but eligible explicit widths now use v3 instead of that refusal. A precommit mapping invariant failure is `clipboard_scene_mapping_invalid`. Malformed operation parameters use `invalid_request`.

P33e supplies shared HTTP/CLI primitives; P33f adds the explicit browser adapter below. P33j adds bounded graphic forest Cut below; molecule-instance cloning, chemical subtree cloning and external-format scene transport remain unsupported. Bounded P33 is complete within the [recorded scope](verification/p33-clipboard-completion.md); the existing native graph and MOL clipboard web flows retain their earlier boundaries.

## Explicit CDXML text clipboard copy (P33h)

The existing **CDXML and CDX export** panel offers **Copy CDXML to clipboard**, **Select CDXML text**, and a read-only **Prepared CDXML text** field only after the existing CDXML drawing export succeeds under its required loss consent. An offer to accept losses is not export content. Missing or changed consent digests, unsupported chemistry, and unrepresentable stereo retain the existing refusals and review flow. This is the whole-drawing, first-molecule-placement projection, independent of transient selection; it does not copy a native fragment or preserve arbitrary scene/group content. Existing export losses remain visible.

Opening the panel, preparing content and accepting losses do not access the browser clipboard. A separate direct click invokes `navigator.clipboard.writeText` with the prepared export string before any asynchronous service call. It writes plain text only; it does not promise that another application recognizes a chemical object. Browser permission denial or missing clipboard APIs leaves the manual field and select button available, and keyboard focus selects the prepared text. No read access, automatic permission request, custom MIME, global shortcut, payload logging or browser-storage persistence is added. Content is shown as text, never interpreted as HTML/XML in the browser.

Prepared content and completion messages are bound to the full drawing snapshot and detected service generation. A known drawing/style/format/process change hides the old copy controls and text; stale asynchronous clipboard completion cannot repopulate another preview or document. Clipboard copying uses the already prepared, reviewed snapshot bytes to preserve direct activation; unlike the unchanged **Download** flow, it does not re-export over the network at the copy click and cannot discover an otherwise unobserved service restart. A write already started may complete after the document changes; the operating-system clipboard is neither erased nor rolled back. Manual textarea copying follows browser text/line-ending behavior.

This read-only action sends no import/transaction, changes no document revision, selection, IDs or Undo/Redo history, and needs no mutation replay/reconciliation gate. CDX remains binary download only: its base64 transport is not exposed as clipboard text. P33i adds explicit CDXML clipboard reading and existing new-drawing import below; Cut remains a follow-up. Existing CDXML/CDX file import still selects a page and molecule and uses `document.import` with `mode: "create"` to create a new drawing; it is not MOL/SMILES append or native Paste. Bounded P33 completion with successful PR and post-merge main CI is recorded in the [completion evidence](verification/p33-clipboard-completion.md).

## Explicit native graphic forest dialogs (P33f)

The separate **Scene graphics clipboard** section provides **Copy graphics…** and **Paste graphics…** for the version-2/version-3 graphics profiles. It does not inspect text to choose between graph, graphics and MOL operations. Copy captures the complete document and selection at the press, queues behind prior edits and validates every captured ID against the latest source snapshot. Explicit graph targets and molecule placements, including those nested in a selected group, refuse the whole copy; they are never filtered from a mixed selection. The prepared inspection must match the queued source snapshot and service generation, and never replaces the live drawing or selection. No active molecule placement is required.

Preparation changes neither document nor browser clipboard. The result states root/object counts and source revision, with a read-only selectable text field, **Select native graphics text**, and a separate **Copy to clipboard** click. That click invokes browser plain-text writing directly, preserving user activation. Permission failure or unavailable browser APIs leave manual copying available. Opening Paste reads nothing. **Read clipboard** is an explicit optional action; reading alone does not submit text. Manual entry remains available. Native graphics text is bounded to 64 KiB of UTF-8, with no truncation or automatic repair.

**Paste graphics** freezes source text bytes, finite point offsets, destination ID, service generation and complete selection-before. At queue execution it requires the same document/process and every captured selection target, then uses the latest revision. Missing targets lapse the whole action instead of narrowing its history selection. Each offset defaults to one destination bond length in points; there is no viewport positioning, repeated-paste increment, rescaling or implicit selected-group parent. Multiple destination molecule placements are accepted. The atomic result selects new roots; Undo restores the captured mixed selection and Redo restores the forest and new roots.

Successful replies must have no losses. The client verifies copied containment and references against the requested source forest. Paste validation checks complete fresh object mappings, translated points, group child order, remapped references, appended root paint order and every unchanged destination graph, stereo, placement and existing scene object. Ordered roots and children determine painting order even if payload object records are shuffled; selected-root ID ordering need not be paint order. Unsupported states use the P33e refusals without a loss-consent shortcut.

Graphics paste shares the existing unresolved clipboard mutation gate with graph Paste/Cut and MOL import. A lost or malformed reply retains one immutable request/body/process, blocks further queued document changes, and exposes **Check graphics paste outcome**, including after dialog dismissal. Checking never rereads or writes the clipboard. A restart, held old-process success or terminal replay conflict requires explicit document reload reconciliation; no attempt is resent into another process. Temporary dialog text is discarded on dismissal/document switch and stale callbacks are ignored; operating-system clipboard content is not erased. No payload logging, browser-storage persistence, clipboard polling, custom MIME data, global shortcuts or external graphics transport is added by this Copy/Paste slice. P33j adds bounded graphic forest Cut below. Bounded P33 completion with successful PR and post-merge main CI is recorded in the [completion evidence](verification/p33-clipboard-completion.md).

## Explicit native graphic forest Cut (P33j)

**Cut graphics…** in **Scene graphics clipboard** prepares the existing version-2/version-3 graphics forest from the complete press-time selection. The queued inspection validates every captured target against the source and process; it does not narrow a mixed graph/placement selection. Existing Copy scope, selected-ancestor deduplication, child/root painting order, internal cycles/self-links and 64 KiB/500 projected-command limits remain unchanged. Cut additionally refuses any reference owned by an excluded object targeting an included object. Copy continues to allow such incoming links. Molecule instances, atoms/bonds, outgoing references and empty/singleton copied groups retain the existing refusals. Multiple molecule placements are allowed because this Cut changes no chemical graph or placement.

The prepared snapshot/revision, raw history selection, roots, complete copied object closure and service generation belong to one Cut. **Cut to clipboard** starts a plain-text browser write directly in its click and queues the result immediately. Only a successful write followed by an exact match of the prepared document and process can send one ordinary transaction: `delete_objects { objects: <frozen roots>, mode: "subtree" }`, under the prepared expected revision, with captured `selection_before` and empty `selection_after`. Later live selection never redirects deletion or history scope. A failed/unavailable write sends no deletion. **Select native graphics text**, manual copying and **Copy to clipboard** never authorize deletion.

Deleting the forest removes its owned internal references; all surviving outside objects, references, roots and sibling order remain, except the removed entries in unselected parents' child lists. Outside parents are retained even if they become singleton or empty groups. No parent pruning, graph/stereo edit or placement removal occurs. The client validates the full expected scene, empty selection, next revision, unchanged chemistry/settings/placements and surviving coordinate values including signed zero and nullable graphic widths before adopting success. The existing backend transaction independently refuses dangling references atomically. Undo restores original IDs, containment, references and captured selection; Redo removes the same forest. Neither changes the clipboard.

Clipboard writing and document deletion are separate, not one cross-system transaction. A write followed by source change or deletion refusal leaves a copy on the clipboard and the source uncut. There is no clipboard rollback or automatic paste-back. Lost/malformed replies retain the same deletion ID/body/process in the shared clipboard mutation gate, block later queued changes, and expose **Check graphics Cut outcome**, including after dialog dismissal. Checking never accesses the clipboard. A restart, held old-process success or terminal conflict after uncertainty requires explicit document reload reconciliation, never a fresh automatic Cut or retry into another process. Closing the dialog does not cancel a submitted deletion or release its uncertainty gate. No new backend operation, native format version, chemistry support, external graphic transport or global shortcut is added. Bounded P33 completion with successful PR and post-merge main CI is recorded in the [completion evidence](verification/p33-clipboard-completion.md).

## Explicit SMILES-text clipboard bridge (P33g)

The existing SMILES panel adds **Read clipboard**, **Select written SMILES** and **Copy to clipboard** around the same bounded OpenSMILES reader and writers. Opening the panel performs no clipboard access. **Read clipboard** invokes browser plain-text reading directly in that user action and puts the result into the ordinary source field; it does not submit text to the service. Manual typing/pasting remains available when browser access is unavailable or denied. A delayed read cannot overwrite a source edit or a different drawing/service. The form and immutable import attempt enforce the reader's 16 KiB UTF-8 limit without truncation, trimming, splitting records or format guessing.

Only **Preview SMILES import** submits that source text. It shows counts, models, every reported loss and any ignored-text character/byte span. The existing grammar, including its bounded tetrahedral/CXSMILES support and unsupported-feature refusals, is unchanged. Text outside the string and supported extension block is handled by the reader's existing ignored-source/loss rules, not discarded by the clipboard adapter. Generated grid geometry is reported without requiring consent; chemistry-changing and ignored-source losses still require a separate **Accept these losses and import SMILES** confirmation. A changed source, preview snapshot or service invalidates the corresponding offer. An explicit acceptance after a definite consent refusal is a new intentional request; an uncertain attempt cannot be replaced by one with a different acceptance flag.

SMILES import appends fresh graph entities to the required existing document; it does not create a document. Exactly one destination placement is required. Positions retain the existing deterministic grid below the current atom and graphic bounds, with `generated_layout` reported rather than a claim of chemical layout. The document title and destination settings remain unchanged. Typed and clipboard-fed sources preserve the same live selection of still-existing entities and existing import history/Undo/Redo behavior, without native Paste offsets or automatic selection of new targets. No layout cleanup, matching-native-style requirement or chemistry repair is added.

Written output is the entire chemical graph, irrespective of transient canvas selection. Recorded output remains the default and canonical output retains its existing model-specific behavior and stereo-loss consent; neither is advertised as another toolkit's canonical form. **Copy to clipboard** is available only for the current prepared drawing/style generation/service result. It writes the prepared plain text directly in that click, without another backend request or await before the browser call. Success is reported only after writing succeeds. **Select written SMILES** explicitly focuses and selects the complete read-only text for keyboard/manual copying; it never writes the clipboard. Changing output style invalidates earlier results even if the user switches back. Import source edits do not change independently prepared whole-graph output.

Both typed and clipboard SMILES commits use one immutable process-bound attempt containing document ID, expected revision, exact source, preview digest and acceptance flag. Lost, still-running or malformed replies keep the same request ID/body and block subsequent document actions. **Check SMILES import outcome** lives in the SMILES panel independently of the old preview or current source field; editing text cannot erase or redirect the unresolved attempt. Checking does not reread/write the clipboard or create another import. A detected restart, held old-process success or terminal replay conflict requires explicit authoritative reload/reconciliation, never automatic resend into a new process. Definite precommit refusals release the gate and leave the drawing unchanged.

No background read, clipboard polling, custom MIME data, HTML import, browser-storage payload persistence, clipboard shortcut override, external-format Cut, new backend operation or broader SMILES syntax is added. Existing native graph, graphic-forest and MOL clipboard flows retain their separate profiles. Bounded P33 is complete within the [recorded scope](verification/p33-clipboard-completion.md); this slice is explicit text transport and guarded import coordination, not general chemical interchange or layout completion.

## Explicit CDXML text import as a new drawing (P33i)

The existing **Create a drawing from CDXML or CDX** form also accepts explicitly labeled CDXML text. **Read CDXML clipboard** calls plain-text reading directly on the click and only fills the source field; opening the form never reads the clipboard. Manual typing/pasting remains available after denial or unavailable browser access. A delayed read cannot overwrite a manual edit, a newer source or a detected service change. The source is capped at 120 KiB of UTF-8 without truncation or format guessing. CDX remains the existing binary-file input, never a guessed base64 clipboard format.

**Read CDXML pages and molecules** sends the exact captured string through existing `document.import` mode `read`. The form shows the available page/top-level-fragment choices, then **Preview import** captures source, explicit format, title, selected page/fragment and the projection digest. Counts, source mappings and all existing losses are shown before **Accept losses and create drawing** sends mode `create` with that digest and acceptance. Text edits, source/choice/title changes and detected service changes invalidate the affected preview. Clipboard-origin strings retain their exact BOM/line endings in service requests; manual textarea editing follows browser text conventions. No XML/HTML is rendered, and no external resources are fetched.

This creates a separate drawing using the existing CDXML projection and creation lifecycle; it does not append to, transform or alter the currently displayed drawing's graph, IDs, selection or history. Existing autosave of the drawing being left retains its earlier behavior. The new drawing has the same initial document/session semantics as file import; this is not MOL/SMILES append or native Paste.

Reviewed creation disables automatic restart retry from the first POST: an authorization refusal after a restart stops before a new-process creation, even if no earlier creation response was lost. An uncertain creation keeps its captured input/digest and the existing `createOnce` request/process binding. **Check CDXML creation outcome** asks about that same creation, without rereading the clipboard or submitting another source. Source/file/choice/title/new-create controls remain locked while it is unresolved. The editor owns this attempt outside the conditional import picker: opening a Saved drawing retains its recovery notice, and returning to the picker retains the lock and exact request. Refreshing or opening Saved documents does not implicitly clear it; only confirmation or the explicit reconciliation acknowledgment does. Editing text invalidates and resets the native file input, so choosing that same file again fires a fresh read. A detected restart or held old-process success never replaces the displayed drawing: refresh and inspect **Saved documents** before explicitly clearing the attempt to prepare another creation. This acknowledgment is not a claim that the earlier creation failed; no automatic resend, clipboard rollback or automatic import into another drawing is performed. A confirmed creation followed by failure to autosave the previous drawing is reported as a draft issue, not exposed as a reason to create again.

Bounded P33 completion with successful PR and post-merge main CI is recorded in the [completion evidence](verification/p33-clipboard-completion.md). No new backend operation, broader CDXML support, append, external Cut, custom MIME, clipboard shortcut or chemistry/layout inference is added. `npm run test:browser:cdxml-import` covers the real form/editor/client with scripted HTTP/clipboard boundaries and passed with the other #172 browser checks; it does not establish real OS clipboard access or external-application compatibility.

## CDXML interchange

`document.export` supports both a caller-supplied rich tree and a bounded export of an editable drawing (P29e). A rich-tree request retains its existing preservation and opaque-content rules. A drawing request takes `{ format: "cdxml" | "cdx", document_id, accepted_losses? }` and returns `{ content, snapshot }`, with the captured document revision in the response. `snapshot` is the usual document/session result for the drawing actually exported. The two input forms are exclusive: a drawing request cannot also supply `document`, `original` or `discard_opaque`. Export captures the drawing under its session lock, builds the file outside that lock, and changes neither the drawing nor its history. CDXML content is UTF-8 text; CDX content is canonical base64.

The drawing profile writes one page and fragment from the first molecule placement: element atoms, 2D positions, charge, isotope, supported radical states and single/double/triple/aromatic bonds. Query atoms, dative/query/unknown bonds and unknown radicals are refused. Extra placements, native IDs, scene groups/graphics/references, settings and selection are reported as losses rather than silently presented as preserved. Stated or unknown hydrogen states and attachment points need loss consent because this profile does not write them. CDX positions must fit its fixed-point range; rounding is reported by the codec and requires consent. P29f connects this same operation to the web drawing-export panel; its controls and preview safeguards are described under Web editor below.

The rich tree now has four typed integer properties: bond `BondDisplay` (CDXML `Display`, CDX `0x0601`, INT16 0–14), and node `EnhancedStereoType` (`0x0446`, UINT8 0–4), `EnhancedStereoGroupNum` (`0x0447`, UINT16 on the binary wire; signed int32 in the common model and CDXML), and `ShowAtomEnhancedStereo` (`0x0445`, one-byte boolean; CDXML `yes`/`no`). Only confirmed names, tags, object locations, widths and values become typed. Unknown names, namespaces, locations, malformed widths or enumeration values retain their opaque representation and preservation rules; they are never guessed into these four properties. Known typed carriers can cross CDXML/CDX without `discard_opaque`; other extensions remain subject to the existing opaque policy. Atom `Geometry`, `BondOrdering`, `AS`, and bond `BS`, `Display2` and `DoublePosition` are not newly promoted to typed properties.

On import, the editor projection still reads the stereo statements the public sources confirm, as described under [CDXML and CDX stereo import](#cdxml-and-cdx-stereo-import-p29b), and refuses unsupported selected chemistry. Drawing export writes wedge/hash marks with their original narrow end (`WedgeBegin`/`WedgeEnd`, `WedgedHashBegin`/`WedgedHashEnd`); wavy single and crossed double bonds use `Wavy`. It invents no marks, atom/bond CIP labels, `Geometry` or `BondOrdering`. A stored center is compared with the configuration derived from the marks and the positions the output actually holds (including CDX quantization); a mismatch or stored-only configuration is a consent-required `stereo_configuration` loss. Stored double-bond configurations are not derived or encoded and require `double_bond_configuration` consent even when the drawn crossed mark is retained.

An enhanced stereo group is written only when every member's output drawing derives the stored tetrahedral configuration. Otherwise the whole group is omitted with a consent-required `stereo_group` loss; no surviving subset is made into a new group. Supported absolute/and/or groups write their type, and non-absolute groups their number. Written members also receive `ShowAtomEnhancedStereo=yes` (CDX `0x0445=true`), an explicit export display choice: the drawing stores no enhanced-stereo indicator visibility setting. When any group is written, exactly one informational `enhanced_stereo_display` loss (`normalized`, `rendering`, no consent required) reports: “The drawing stores no enhanced-stereo indicator visibility setting. This export explicitly shows indicators for written stereo groups.” No such loss or flag is emitted when no group is written. Configuration/group roundtrip checks exclude indicator visibility; this setting is not preserved through the drawing model. This drawing-export profile preserves numbers 1–127; other stored numbers (including missing/0, negatives and larger values) receive the lowest unused positive number per kind, reserving valid numbers before allocation, with a no-consent `stereo_group_number` normalization. This is a writer profile limit, not the binary reader's unsigned-16-bit range. Exhausted numbering is `stereo_group_limit`. Native group IDs are not encoded (`stereo_group_identity`, informational). Stored tetrahedral centers originally in no group need `stereo_ungrouped_centers` consent (`unknown`); a dropped group is reported once rather than duplicated as originally ungrouped centers. These rules make no claim about another reader's interpretation or mixture ratios.

Both drawing formats use the existing digest-based export consent: required losses appear in `diagnostics[0].details.losses` with `loss_digest`, no content is returned, and repeating with that digest as `accepted_losses` permits the export. The digest binds those losses and the exact output bytes; changed output is `loss_consent_changed`. `consent` is internal and is not serialized in a loss entry. Checks exercised in this slice: 7 drawing-export operation tests, 34 stereo-import operation tests, 5 typed-carrier provider tests, and 25 application unit tests. These checks establish the bounded cases, not full format or stereo compatibility.

With the CDXML provider connected, `document.import` accepts `{ mode: "read", format: "cdxml", source, compact_bytes? }` and returns an ordered rich `document` and selectable `choices` (page ID, fragment ID and source paths). The source is a UTF-8 string; unknown extensions and outside-root material are kept when an unchanged document is written with its `original` source. `document.export` takes `{ format: "cdxml", document, original?, discard_opaque? }` and returns `{ content }`. Edited opaque content or IDs cannot be claimed safe: a rewrite requiring the removal of opaque content must explicitly request `discard_opaque`, and reports the loss. Without `original`, the service cannot detect content the caller already omitted or verify unknown references in an earlier source. No external resources are fetched. See [interchange limits and preservation](INTERCHANGE.md).

For the single-graph editor, `document.import` with `mode: "preview"`, `format`, `source`, `title`, `page_id` and `fragment_id` returns `{ document, mapping, preview_digest }` and a loss report. It creates nothing. A `mode: "create"` request repeats those fields, supplies `preview_digest` and sets `accept_loss: true` when there are losses. The service reparses and binds consent to the exact source, projection, new document settings, loss report and current service generation. Changed input, settings or a restart invalidates the preview. It creates and saves a new document only after verification; an already committed request is replayed within the process replay window. The response includes `interchange_mapping` from source paths and IDs to new document, page, molecule, atom and bond IDs. Advanced chemical semantics are unsupported; a rich import can preserve information that an editor projection explicitly discards.

### CDX over the same operations

`format: "cdx"` selects the binary codec in `document.import` (`read`, `preview`, `create`) and `document.export`. The binary payload is the string field (`source`, `original`, and the exported `content`) carried as **canonical standard-alphabet base64**: padded to a multiple of four, no whitespace, URL-safe alphabet or non-zero trailing bits, so one byte string has one text and a preview digest binds exactly one file. Anything else is `invalid_base64`. The preview/consent/create protocol, snapshot binding, replay and read-only export are identical to CDXML.

Effective size limits, each tested: the codec accepts at most 1 MiB of CDX bytes (`resource_limit` at offset 1048576 for a longer file; base64 text longer than any 1 MiB file needs is refused before it is decoded, with `resource_limit` and no offset, because a text length is not a byte position); the HTTP `POST /api/v1/operations` body is at most 256 KiB, so about 190 KB of CDX fits once the JSON envelope and the one-third base64 growth are counted; one CLI `operations` request line is at most 1 MiB, so about 780 KB of CDX fits. The web form accepts files up to 120 KiB (CDXML or CDX). The tighter limit applies first.

Opaque binary property values in the rich tree keep contract 1's form, `{ "type": "bytes", "value": [0, 255, ...] }`, in every reply. A `mode: "read"` request may add `compact_bytes: true`, which makes the reply write them as `{ "type": "bytes_base64", "value": "<canonical base64>" }`, a quarter the size, so that a large unknown property can be sent back with its `original` inside the HTTP body limit. Both forms are accepted in an export request and mean the same bytes; the base64 form is decoded strictly (a non-canonical text is refused). Nothing about the existing form changes. The web client shows `details.offset` after the message of a refused file.

Errors from the binary codec carry `details.offset`, the byte position of the offending item (a font-table entry, a property, a text byte), and the message does not repeat it; zero is a real offset. A fault in the caller's `original` of an export, whether its base64 text or its bytes, is `invalid_original`; its offset, when present, is a position in the original, not in the submitted document. Codes include `invalid_signature`, `invalid_original`, `page_required`, `truncated`, `invalid_root`, `invalid_end_marker`, `trailing_data`, `resource_limit`, `invalid_property`, `out_of_range`, `duplicate_property`, `duplicate_id`, `dangling_reference`, `invalid_reference`, `invalid_containment`, `missing_property`, `unsupported_charset`, `unsupported_text_encoding`, `invalid_text`, `invalid_text_runs`, `invalid_font_table`, `invalid_color_table`, `duplicate_font`, `unknown_font` and `unknown_color`.

Model validation codes differ by format, on purpose. When the common model rejects a tree (an empty font table, a duplicate ID, a dangling reference), CDX reports the domain's specific code and a plain sentence (for example `invalid_font_table`, `duplicate_id`). CDXML keeps the code and wording it had before CDX existed: `invalid_document` with the model's internal name in the message. The CDXML behavior is retained only for compatibility with clients of contract 1; it is not a different judgment of the tree.

`document.export` consent for CDX is bound to what the caller was shown. The writer's report lists content it dropped or rounded. When some of it needs consent, the export is refused with `loss_consent_required`, no content, and `details` holding `losses` (the full entries: `feature`, `source_path`, `disposition`, `reason` with the largest rounding error and its bound, `semantic_impact`), `features` (each name once, in first-seen order) and `loss_digest`, a SHA-256 over those entries and the exact bytes that would be returned. The caller accepts them by repeating the request with `accepted_losses` set to that digest. A digest for other losses or other output (the document changed, or a different `original`) is refused with `loss_consent_changed` and the new losses and digest. The writer that causes a loss marks whether it needs consent; the application only reads the mark. Marked: `precision` (coordinates, colors and font sizes rounded to the nearest representable value), `xml_content` (comments, instructions and stray text, including inside font and color tables and Represents), `represents` and `object_ids`. Opaque content is gated by `discard_opaque` as for CDXML. Reported with the result but needing no consent: `xml_namespace` (dropping it changes no meaning), `font_platform`, `style_defaults`, `child_order`, `binary_serialization`. The reserved header bytes of an `original` are kept when the document is rewritten, so they are not a loss. Rich-tree CDXML exports ordinarily need no loss consent; drawing exports in either format require it for the chemistry and stereo omissions described above.

### CDXML and CDX stereo import (P29b)

The editor projection (`document.import`, modes `preview` and `create`) reads bond marks, configurations and enhanced stereo groups from a CDXML or CDX fragment. This section describes import; P29e drawing export is described above, while unchanged rich-document export retains its preservation rules. Everything below is what the public sources confirm; nothing is claimed beyond it.

*Bond display (CDXML and CDX).* Source: the public CDX SDK page [Bond_Display](https://iupac.github.io/IUPAC-FAIRSpec/cdx_sdk/properties/Bond_Display.htm). It lists the INT16 values 0 `Solid`, 1 `Dash`, 2 `Hash`, 3 `WedgedHashBegin`, 4 `WedgedHashEnd`, 5 `Bold`, 6 `WedgeBegin`, 7 `WedgeEnd`, 8 `Wavy`, 9 `HollowWedgeBegin`, 10 `HollowWedgeEnd`, 11 `WavyWedgeBegin`, 12 `WavyWedgeEnd`, 13 `Dot` and 14 `DashDot`; an absent display is `Solid`; and `Wavy` on a double bond is treated as a double-either (crossed) bond. The same table serves both formats (CDXML spells the name, binary CDX the number), so a binary `Display` of `n` reads exactly as the CDXML name for `n`. The page does not say which end of `...Begin` is narrow: the narrow end at the bond's `B` atom for `...Begin` and at its `E` atom for `...End` is read from the public fixtures, not from that page. `WedgeBegin` and `WedgedHashBegin` on a single bond are a wedge and a hash with the narrow end at `B`, `WedgeEnd` and `WedgedHashEnd` with the narrow end at `E`; `Wavy` on a single bond is a wavy mark and on a double bond the crossed mark. `Solid` (or no display) is a plain bond and reports nothing. `Dash`, `Bold`, `Dot` and `DashDot` are read as drawing styles: the bond is plain and the no-consent loss `bond_display` (`dropped`, `rendering`) says so; `Dash` and `Bold` are kept plain on purpose, because the page does not say they carry stereochemistry and they are not turned into a mark. `Hash`, `HollowWedgeBegin`, `HollowWedgeEnd`, `WavyWedgeBegin` and `WavyWedgeEnd` may state stereochemistry the bond cannot hold, and so does a wedge, hash or wavy value on a bond that is not of the order that mark needs, or in `Display2` (the second line of a double bond holds no mark): the bond is imported plain with the loss `stereo_mark_unplaceable` (`dropped`, `stereo`, internally consent-required, `source_path` the property). A value outside 0 to 14, a CDXML spelling not in the list (any other case or name) and a binary `Display` that is not exactly two bytes refuse the file (`unsupported_bond`, or `invalid_stereo_attribute` for the wrong width). `DoublePosition` and a bond `CIP` attribute have no public spelling here and stay refused (`unsupported_bond`, as every attribute the projection does not read).

*Atom attributes (CDXML).* `Geometry="Tetrahedral"` and `BondOrdering` (unsigned integers, a zero for a slot with no bond, at most 16 entries) are well-formedness checked and not stored: each gets the no-consent loss `stereo_geometry` or `bond_ordering` (`dropped`, `stereo`); any other `Geometry` value refuses the file (`unsupported_geometry`, `details.family` `unsupported_feature`) and a malformed `Geometry` or `BondOrdering` refuses it as `invalid_stereo_attribute`; both name the property's own path (for example `...children[0].properties[2]` in a binary file). The label `AS` (the atom label a program computed, for example `AS="S"`) is never interpreted or stored as a configuration: it gets the no-consent loss `cip_label_not_stored` and is not compared with anything. The configuration comes only from the marks and positions. `ShowAtomEnhancedStereo` is a document-level attribute: it is reported by the generic `unprojected_property` loss (not read, no consent), and on an atom it is refused like any unread attribute.

*Configurations.* After the marks, one `set_configuration_from_drawing` over the marked centers derives the configuration of each from the imported positions and marks, exactly as the MOL import does (the same shared derivation); a center the drawing does not determine keeps its mark, stores nothing and gets the loss `stereo_unreadable` (`dropped`, `stereo`) at the atom's source path. A double bond's cis/trans arrangement is **not** derived or stored (no E/Z is read from coordinates): the existing loss `double_bond_stereo` (`unknown`) says the editor did not examine it. CIP labels are never stored.

*Enhanced stereo groups.* `EnhancedStereoType` is `Unspecified`, `None`, `Absolute`, `Or` or `And` (anything else: `invalid_stereo_attribute`); `EnhancedStereoGroupNum` is any 32-bit integer in CDXML (the file attribute is `EnhancedStereoGroupNum`; `EnhancedStereoGroupNumber` is the name of the automation property, not of the attribute, and is refused as an unread attribute). Atoms with the same kind and number are one group (`Or` 1 and `And` 1 stay separate groups, and `Or` 2 and `Or` 258 are two groups); a group is keyed by its kind and number as a pair (no number for `Absolute`) and created with `create_stereo_group` in the import transaction, after the configurations, with the number stored as stated (an omitted `Or` or `And` number is read as `0`, as is a stated `0`, so they are one group; the omission is reported by the no-consent loss `stereo_group_number`, `normalized`). `Absolute` has no number: a stated non-zero number is dropped with `stereo_group_number` (`dropped`). `None` is reported with `enhanced_stereo_none` (`dropped`, no group); `Unspecified` and an absent type say nothing, and a number without a type names no group (`stereo_group_number`, `dropped`). Every member must be an atom whose marks derived a stored **tetrahedral** configuration in this import; one that is not (no mark, a center the drawing does not determine, a wavy center) refuses the whole fragment with `stereo_group_member_not_stereocenter`, and a fragment whose marks could not be read at all with `stereo_group_drawing_unreadable`. A member of a fragment that draws no stereo marks at all is told that no configuration can be derived for it, not that its marks give none. These refusals carry `details.family` (`malformed_input` for `stereo_group_member_not_stereocenter` and `invalid_stereo_attribute`, `unsupported_feature` for `stereo_group_drawing_unreadable` and `unsupported_geometry`); an import always creates a new drawing, so a refusal leaves nothing created and no revision changed (tested for `preview` and `create`). `consent` is not a field of the wire report; a `create` request needs `accept_loss` whenever any loss is reported.

*Binary CDX.* The atom properties are read by tag: `Geometry` `0x0430` (exactly one byte, `7` is tetrahedral; any other width is `invalid_stereo_attribute`, checked before the value), `BondOrdering` `0x0431` (a `CDXObjectIDArray`: 32-bit little-endian entries with no count, a multiple of four bytes; the page [CDXObjectID](https://iupac.github.io/IUPAC-FAIRSpec/cdx_sdk/DataType/CDXObjectID.htm) says "the length of the array is implicit from the size of the data" and its example `01 00 00 00 02 00 00 00 03 00 00 00 04 00 00 00` is 1, 2, 3, 4; a 16-bit count in front exists only in `CDXObjectIDArrayWithCounts`, so a length of 2 + 4n is not read and is `invalid_stereo_attribute`; the public fixtures show the same count-free layout), `EnhancedStereoType` `0x0446` (one byte: 0 `Unspecified`, 1 `None`, 2 `Absolute`, 3 `Or`, 4 `And`) and `EnhancedStereoGroupNum` `0x0447` (**two bytes, little-endian, read as an unsigned 16-bit integer**, so `0x0080` is 128 and `0x0102` is 258; a property of another length is `invalid_stereo_attribute`). Source for the types and the bond tag: the public pages [Node](https://iupac.github.io/IUPAC-FAIRSpec/cdx_sdk/Node.htm) (Geometry `0x0430` INT8, BondOrdering `0x0431` an object-ID array, EnhancedStereoType `0x0446` UINT8, ShowAtomEnhancedStereo `0x0445`, EnhancedStereoGroupNum `0x0447`; the page describes the position as vertical then horizontal but does not state the CDXML `p` order) and [Bond_Display](https://iupac.github.io/IUPAC-FAIRSpec/cdx_sdk/properties/Bond_Display.htm) (the value list above). The bond `Display` is read from `0x0601` as an INT16 through the table above, so binary wedge, hash and wavy bonds give marks and configurations like their CDXML twins. `Display2` and `DoublePosition` have no tag number in the two pages used, so a binary bond that carries them (`0x0602`, `0x0603`, as every other tag the projection does not read) is refused as `unsupported_bond`, as before. A binary point is stored vertical first (y, then x), the provider's reader and writer agree on it, and a CDXML `p` is `x y`; tests draw the same (S) center in both and read the same label back. Another program's reader is observed to read 258 as 2 and 128 as -128 from the same bytes; that is a measured behaviour of that reader, not a limit of the format, and it is not reproduced, so `Or` 2 and `Or` 258 stay two groups. An inherited/root `ShowAtomEnhancedStereo` (`0x0445`) and the zero-length marker `0x0448` remain uninterpreted (the generic `unprojected_property` loss). On a node, `ShowAtomEnhancedStereo` is now a typed display preference; projection reports it as not stored and it determines neither configuration nor group membership. The atom CIP tag `0x0437` (`Atom_CIPStereochemistry`, INT8, the CDXML `AS`; tag and type from [TableOfProperties](https://iupac.github.io/IUPAC-FAIRSpec/cdx_sdk/TableOfProperties.htm) and [Node](https://iupac.github.io/IUPAC-FAIRSpec/cdx_sdk/Node.htm), the values from [Atom_CIPStereochemistry](https://iupac.github.io/IUPAC-FAIRSpec/cdx_sdk/properties/Atom_CIPStereochemistry.htm): 0 `U`, 1 `N`, 2 `R`, 3 `S`, 4 `r`, 5 `s`, 6 `u`) is read like the CDXML `AS`: a hint, never stored, never compared with anything and never used for a configuration, reported by the no-consent loss `cip_label_not_stored`. The width is checked first (exactly one byte, else `invalid_stereo_attribute`), then the value (outside 0 to 6 is `invalid_stereo_attribute`); tests show the stored configuration is the same for every valid value and for no label.

*Verification.* `crates/application/tests/cdxml_stereo_import.rs` draws the 23 shared corpus drawings (`tests/data/stereo-fixtures.json`) as CDXML and checks each stored configuration against the hand-stated expectation, then the display values, the consent-loss shapes, the hints, the groups (`Or` 1 against `And` 1, omitted, 0, 127, 128, 258, negative and `i32::MAX` numbers) and the refusals; unit tests in `interchange_projection/stereo.rs` pin the CDX byte reads. `scripts/verify-cdxml-rdkit.py` (hand-run, not part of `npm run check`) reads the same CDXML with its own small parser, builds a molfile and compares the canonical isomeric SMILES with RDKit; the conformance cases `cdxml-stereo-import-marks` and `cdxml-stereo-import-refusals` run the operations. Not claimed: any CIP, validation or equivalence with another program's CDXML reader, E/Z from coordinates, `DoublePosition`, `Display2` in binary CDX, query atoms.

### Stereo interchange evidence (P29g)

This table describes the bounded profiles, not full-format compatibility. Drawing and
configuration retention are separate from group-indicator visibility. CDXML/CDX group
export deliberately writes node `ShowAtomEnhancedStereo=yes` / `0x0445=true` because
the drawing stores no visibility preference: one `enhanced_stereo_display` loss
(`normalized`, `rendering`, no consent) reports that choice whenever groups are emitted.
No flag or visibility-normalization loss is emitted when no group is written; the
configuration/group corpus comparisons exclude this display setting.

| Profile | Supported stereo | Normalization | Consent or refusal | Evidence |
| --- | --- | --- | --- | --- |
| CDXML drawing export/import | Wedge/hash narrow ends, wavy/crossed marks, bounded drawing-derived configurations, complete supported absolute/and/or groups | Numbers outside 1–127 to distinct free positive numbers per kind; visibility explicitly shown | Consent for mismatched/stored-only configurations, stored double-bond statements, omitted whole groups and originally ungrouped-center uncertainty; unsupported selected chemistry refused | [Operation checks](../crates/application/tests/cdxml_stereo_export.rs), [corpus](../crates/application/tests/stereo_codec_corpus.rs), [roundtrip case](../conformance/cases/cdxml-drawing-export-stereo-groups-round-trip.json) |
| CDX drawing export/import | Same bounded semantics, binary typed carriers | Same number/visibility policy; fixed-point coordinate rounding | Same stereo consent; precision consent; invalid/out-of-range or unsupported content refused; unknown extensions retain opaque policy | [Carrier checks](../crates/providers/tests/stereo_carrier_typing.rs), [corpus](../crates/application/tests/stereo_codec_corpus.rs), [roundtrip case](../conformance/cases/cdx-drawing-export-stereo-groups-round-trip.json) |
| Recorded SMILES / CXSMILES | Supported tetrahedral `@`/`@@` in actual emitted neighbor order, complete supported groups; readback verified | Hydrogen representation; unstated group number as 0 and negative numbers to free positive numbers | Unwritten marks/configurations/groups need consent. Directional `/` and `\` are read since P29c2a and written since P29c2b (see "SMILES double-bond direction write"): a stored double-bond configuration is written as marks, or left out as a consent-required `stereo_configuration` loss with a reason; lone-pair fourth-position marks are refused as `stereo_center_unsupported_here`; stored lone-pair configurations cannot be written and require loss consent | [Reader checks](../crates/providers/tests/smiles_stereo.rs), [writer checks](../crates/providers/tests/smiles_stereo_writer.rs), [double-bond writer checks](../crates/providers/tests/smiles_double_bond_write.rs) |
| Canonical SMILES | Stereo-insensitive bounded chemical graph | Existing canonical graph normalization | Consent for omitted stereo; no claim of stereo-canonical text or another toolkit's canonical form | [Writer checks](../crates/providers/tests/smiles_writer.rs), [application checks](../crates/application/tests/smiles_stereo_import.rs) |
| MOL/SDF V2000 | Supported marks and configurations derived from the written depiction; no enhanced groups | Narrow endpoint first, four-decimal coordinates | Consent for omitted groups and configurations not carried by the output; unsupported CTAB fields refused | [MOL stereo checks](../crates/application/tests/stereo_read.rs), [V3000 wire checks including V2000](../crates/application/tests/ctfile_v3000_wire.rs) |
| MOL/SDF V3000 | Supported marks/configurations and absolute/and/or collections | Signed int32 group numbers retained; explicit chiral-flag rule | Undrawn group member refuses export; originally ungrouped centers and uncarried stored configurations need consent; advanced CTAB fields refused | [Collection checks](../crates/application/tests/ctfile_v3000_groups.rs), [provider checks](../crates/providers/tests/ctfile_v3000.rs) |

The conformance suite is version 25 with **99/99 passed** (74 accept, 25 reject),
including [CDXML/CDX exact loss consent](../conformance/cases/cdxml-cdx-drawing-export-stereo-loss-consent.json).
The two [codec corpus tests](../crates/application/tests/stereo_codec_corpus.rs) passed:
23 existing independently checked drawings × 2 source codecs × 2 output codecs = 92
roundtrip paths, plus four hand-declared partitions on the public two-center drawing
× 4 codec paths = 16 group paths. Graph, configuration, group partition and drawn marks
are compared separately. The original 23-drawing corpus is unchanged; those four
partitions are explicit added test statements, not recovered legacy group data. These
counts do not establish arbitrary chemistry, external reader compatibility or visibility retention.

Separate no-network checks with RDKit 2022.03.2 passed: the [drawing comparison](../scripts/verify-stereo-rdkit.py)
checked 23 fixtures with 0 problems; the [SMILES stereo comparison](../scripts/verify-smiles-stereo-rdkit.py)
checked 24 fixtures with 0 failures; the [V3000 comparison](../scripts/verify-v3000-rdkit.py)
agreed for its supported core, marks and group partitions. RDKit skips the unrecognized
`STERAC-128` collection, so that negative number is **not** independently verified by
that comparison. RDKit did not independently parse the CDXML/CDX outputs. These manual
comparisons are separate from conformance and do not change the format profiles.
P29 is `done` for this documented bounded profile: P29f npm run check, browser smoke
and both required reviews completed. P29c2 directional bonds are read (P29c2a) and written (P29c2b) by the recorded SMILES writer for the
bounded profile below; SMILES lone-pair centers, other stereo classes/CX
fields, stereo canonical output and general stereo/format compatibility remain unsupported.

## Valence inspection

`chem.validate` uses the independent, bounded policy `chemifree-valence-3`. The snapshot and its atom assessments are captured under one session lock. Each assessment names the local atom ID, the sum of drawn integral covalent bond orders (or null), additional non-vertex hydrogens (or null), `valid`, `invalid`, or `unsupported`, and a nullable reason. Every non-valid atom also has a `valence_diagnostic` warning. An isotope does not alter valence; a drawn hydrogen is a vertex and its bond contributes to the sum. Stated non-vertex hydrogen counts are preserved. An unsupported state is reported, never converted to a guessed bond or hydrogen count. The operation neither edits the document nor changes selection, history, or revision. Its request ID does not freeze a read: a repeated request inspects the then-current working copy.

For neutral B, C, N, O, P, S, F, Cl, Br and I the policy uses the ordinary integral valences in the [OpenSMILES organic subset](https://opensmiles.org/opensmiles.html#_organic_subset): B 3, C 4, N/P 3 or 5, O 2, S 2/4/6, halogens 1. Neutral H has valence 1 and receives no inferred H. H± and halide anions use 0; so do the common monatomic ions Li+, Na+, K+, Mg2+ and Ca2+ (new in `chemifree-valence-3`; `chemifree-valence-2` listed none of them, so a drawn `Na+` was `unsupported` with reason `element_or_charge`, and is now `valid` with no hydrogens). A bond on one of these ions is `invalid` (`unusual_valence`). Other charges of these elements (such as Na−, Na2+, Mg+), the other alkali and alkaline-earth metals, transition metals and every other ion remain unlisted and `unsupported`. B−/B+ use 4/2, C± 3, N−/N+ 2/4, O−/O+ 1/3. The chosen charged states are a documented Lewis-model convention. Neutral H/C/N/O doublets reduce the target by one; neutral C/N/O singlets and triplets reduce it by two. Other radical/charge combinations are unsupported. Inferred H fills the smallest allowed target at least as large as the drawn bond sum. A stated H count plus the sum must equal an allowed target; `unusual_valence` means outside this policy, not proof a species is impossible.

Query atoms, attachment points, unknown hydrogens or radicals, unlisted element/charge combinations, and atoms with aromatic, dative, query or unknown bonds are unsupported. In those bond cases the sum is null. The web compares the actual returned atom and bond snapshot with the visible drawing in addition to its ID and revision; a service restart can reuse a revision for different content. A changed snapshot requires another inspection. `chem.validate` has no stereo check: it neither reads the marks and stated configurations a drawing may hold nor reports anything about them, and its assessments are the same with or without them (the snapshot it returns does carry them, see Documents). The comparison of stored and drawn stereo is the separate operation [`chem.stereo`](#stereo-inspection), which `chem.validate` may later delegate to with each finding keeping its own `model_id`. This model does not claim universal valence, stereochemistry, aromaticity, query matching, a mass engine, or compatibility with another product.

## MOL and SDF subset

The public [BIOVIA CTfile guide](https://discover.3ds.com/sites/default/files/2020-08/biovia_ctfileformats_2020.pdf) describes V2000 and SDfile syntax. This independently written profile reads and writes V2000 and V3000 element atoms, coordinates, charge, absolute isotopes, radicals, single/double/triple bonds (V3000 also aromatic, type 4), bond depictions (wedge, hash, wavy and crossed) and ordered SD fields. The dialect of a record read is decided by its counts line (`V2000` or `V3000`, case-insensitive); a record of either dialect imports through the same `document.import`, and one SDF may mix them. Not read or written in either dialect, each refused (never ignored) with its reason: query or pseudo atoms and query bond types (the model holds them, but this profile does not map them yet), dative bonds, atom parity/valence/mapping, Sgroups, R-groups, 3D blocks, reactions, aliases, other nondefault CTAB fields, and unknown chemical directives. Bond stereo, by dialect: **V3000 is read** (bond `CFG` 1 wedge, 3 hash, 2 either, with the first atom of the bond line as the narrow end; either is a wavy single bond or a crossed double bond; a `CFG` on another bond type is refused as `v3000_unsupported_bond_property`), and the configuration each wedge or hash shows is derived and stored by the same rule as `set_configuration_from_drawing` (a center whose marks do not determine one keeps its mark and stores nothing, reported as `stereo_unreadable`, `dropped`, `stereo`, `source_path` `records[N].atoms[i]`). **V2000 bond stereo is read the same way**: column 10-12 value 1 (wedge) and 6 (hash) on a single bond, 4 (either) on a single bond and 3 (either) on a double bond become the same marks and go through the same derivation of configurations (one shared projection); any other stereo value, or one on a bond order that cannot carry it, is refused as `unsupported_bond_field`, as is a nonzero unused, topology or reaction-center column (never ignored; tests pin each). V2000 atom parity stays refused on read. **Both dialects write** the marks: a wedge is `CFG=1` (V2000 column 1), a hash `CFG=3` (column 6), an either mark `CFG=2` (column 4 on a single bond, 3 on a double bond); a mark whose narrow end is the bond's `end` is written with the two atoms swapped (the bond stays the same bond). This replaces the earlier whole-drawing refusal `unsupported_stereo`, which no longer exists: V2000 files Chemifree writes with marks are read by RDKit 2022.03.2 (a hand-run script) and by Chemifree's own V2000 reader (a round-trip test and a conformance case). Stereo the file cannot carry needs the caller's consent, see "Export version and consent" below. It does not infer stereochemistry beyond the derivation above or prove full chemical or legacy compatibility. A double bond with stereo code 0 (V3000: no `CFG`) can carry E/Z meaning derived from coordinates: each is reported as `coordinate_stereo` with `unknown` stereo impact, including on export, except a double bond that carries an either (crossed) mark, which states the arrangement. This is a pre-existing asymmetry: the MOL/SDF export reports coordinate-implied E/Z as `coordinate_stereo`, while SMILES export, which does not derive double-bond stereo from coordinates, reports nothing for it. The separate RDKit 2022.03.2 comparison tests cis/trans 2-butene and both directions of a supported graph.

A `document.import` preview supplies `format: "mol" | "sdf"`, UTF-8 `source`, one-based `record`, `document_id`, `expected_revision`, and `preview: true`. Every record must parse, including unselected ones. The reply contains all parsed records, the exact pre-import snapshot captured under the same lock as the prospective document, `preview_digest`, and losses, without changing the document. To commit, send the same format, source, record, document and revision with `preview_digest` and `accept_loss: true` when losses are reported. A digest binds the service instance, pre-import snapshot, prospective document and complete loss report; it is checked again under the document lock before the one undoable edit. Restarting the service invalidates an earlier preview even if the same graph and revision return. A missing or changed preview is refused. Repeating a successful request ID replays its result; reusing it for different parameters is refused. The file is appended to the existing graph, never replacing its title or content. The drawing must have exactly one placement. An empty graph can be a no-op. Native atom/bond IDs are returned in `import_mapping` in file-index order. Preview, decoding and rejected commits do not modify the drawing; saving, undo and redo use the ordinary document contract.

`document.export` takes one source for MOL, or 1–128 for SDF, in `records`, each with a `document_id` and optional ordered `fields: [{name,value}]`. A field value uses `\n` between nonempty lines, or the empty string; duplicate names are retained. The reply's `content` contains the complete file, and `records` maps each source's atom and bond IDs in file-index order and includes the exact source snapshot and revision used. Each source is read as this process sees it; records are captured individually. A later missing source, unsupported graph, invalid field or cumulative output overflow fails the whole operation: no partial content is returned. Diagnostics carry one-based `details.record` and input errors also carry `details.line` when known. Export is a read; it does not save or modify documents.

**Export version and consent (P28b1).** `document.export` for `mol`/`sdf` takes two optional keys. `ctab` is `"v2000"` (the default when omitted, so every existing request keeps its meaning and bytes), `"v3000"` or `"auto"`; any other value is a parameter error. `"auto"` writes V3000 only when a drawing needs it (more than 999 atoms or bonds, an aromatic bond, which V2000 has no type for, or an enhanced stereo group, which only V3000 can carry), otherwise V2000, and lists its losses in both cases. One request writes one dialect for all of its records, and `auto` is decided once for the whole request (if any record needs V3000, every record is written as V3000); the reply's `result.ctab` (`"v2000"` or `"v3000"`, additive) says which. An explicit `"v2000"` for a drawing V2000 cannot hold is refused naming V3000: `resource_limit` for more than 999 atoms or bonds and `unsupported_bond` for an aromatic bond; V3000 holds at most 2,000 atoms and 4,000 bonds (`resource_limit`). The format ids `mol-v2000` and `mol-v3000` of the public functional specification are accepted as aliases of `mol` (P28b2): on export the id sets the dialect (`mol-v3000` is `format: "mol", ctab: "v3000"`), and an explicit `ctab` that is not `v2000`, `v3000` or `auto` is the usual parameter error (`invalid_request`), a valid `ctab` that names another dialect, or `"auto"`, is refused as `format_ctab_conflict` (a `ctab` equal to the id is accepted); on import the id requires the selected record to be in that dialect, otherwise it is refused as `format_ctab_conflict` with nothing imported (`mol` reads either). `sdf` has no versioned id, and `auto` stays a Chemifree-only value. Absent `accepted_losses` corresponds to the specification's `on_loss: reject` for consent-required losses; a digest corresponds to accepting the reported losses. `accepted_losses` has the shape of the SMILES and CDX flow: a loss that needs consent (`consent` required) refuses the export with `loss_consent_required`, no content, and `details.features`, `details.losses` and `details.loss_digest` (a SHA-256 over those entries and the exact file text, so it binds the dialect and the drawing); repeating the request with that digest as `accepted_losses` writes the file as `partial`, and a digest for other text is `loss_consent_changed`. Needing consent, per record (`source_path` `records[N]...` with snapshot IDs): `stereo_configuration_not_drawn` (`dropped`, `stereo`, `atoms[a1, ...]`: a stored center configuration that the drawing's own marks and positions do not derive to, judged on the positions as the file will hold them (rounded to four decimals, the same quantization the writers use), so a reader of the file gets another configuration or none), `stereo_bond_configuration` (the same for a stored double-bond configuration that neither a crossed mark nor the coordinates show) and, V2000 only, `stereo_group` (`dropped`, `stereo`, `groups[e1, e2]`, every group singletons included: V2000 has no collections); V3000 only, `stereo_ungrouped_centers` (`unknown`, `stereo`, `atoms[a1, ...]`, see "Enhanced stereo collections" below). Reported without consent: V2000 only, `chiral_flag` (`unknown`, `stereo`, `counts.chiral`, once per record whose drawing holds any mark or configuration: the counts flag is written as 0, no absolute stereochemistry is stated, and how a reader treats written marks with flag 0 is not established by the sources); V3000 only, `stereo_group_identity` (below); and the existing coordinate, hydrogen, attachment-point and drawing losses worded for the dialect written. Import of V3000 additionally reports `chiral_flag` (`dropped`, `stereo`) for a record whose counts flag is 1 and that holds no `STEABS` collection, without giving the flag a meaning (no group or absolute statement is made from it); with a `STEABS` collection the file's absolute statement is imported as a group and the flag adds nothing.

**Enhanced stereo collections (P28b2).** V3000 only for export and import of collections; V2000 has no group mapping; CXSMILES reads groups and recorded output writes supported groups, while CDXML and CDX also support bounded drawing-group export (see [CDXML and CDX stereo import](#cdxml-and-cdx-stereo-import-p29b)). *Export.* A V3000 export refuses (`v3000_group_member_not_drawn`, family `unsupported_feature`, listing the member atoms; consent cannot remove it, nothing is written and the document is unchanged) when a group member would not derive a tetrahedral configuration from the wedge/hash depiction as it will be written and quantized (a stored configuration with no mark, or a wavy mark); this is the same derivation the import checks members with, so every V3000 file this writes is one this reader accepts. V2000 is unchanged (groups are consent-dropped). Each P27 group is written as one `COLLECTION` entry: `absolute` as `MDLV30/STEABS`, `and` as `MDLV30/STERAC<n>`, `or` as `MDLV30/STEREL<n>`, each with `ATOMS=(count i1 i2 ...)` in file atom indexes, ascending. `n` is the group's interpreted number as stated: an omitted number is written `0`, and `0`, negative numbers and any 32-bit integer are written as they are (the domain keeps interpreted numbers unique per kind, so two entries never share a name). Order: the absolute group, then `and` groups by number, then `or` groups by number; lines longer than 80 columns are continued as for every V3000 line. The `COUNTS` chiral flag is `1` if and only if the drawing holds an absolute group, otherwise `0`; this is a conservative **product rule**, not a statement about any other program's reader or writer. A stored tetrahedral center that is in no group has no enhanced-stereo statement in the file, and Chemifree does not add one: that is the consent-required loss `stereo_ungrouped_centers` (`unknown`, `stereo`, `source_path` `records[N].atoms[a1, ...]`, one entry per record), also reported when groups exist for other centers. The public ChemDraw writer is observed to put such centers into `STEABS` and to set the flag; Chemifree deliberately does not, and this rule is not claimed to match how other readers interpret an unlisted center. A drawing with no stored configuration, or whose every stored tetrahedral center is in a group, has no such loss (so a drawing with a wedge and a stored configuration but no group needs `accepted_losses` for V3000). A center stored as `unknown` is never reported (a group cannot hold it). The group identifiers `e1..` are not in the file: one informational, no-consent loss `stereo_group_identity` (`dropped`, `stereo`, `records[N].groups[e1, e2]`) says so. Stored numbers equal the written interpreted ones, so no numbering loss exists. V2000 keeps the consent loss `stereo_group` (a V2000 file carries no group). *Import.* In the import transaction, after the configurations derived from the marks, each collection becomes a P27 group with `create_stereo_group` (`STEABS` has no number; `STERAC<n>`/`STEREL<n>` store `n` exactly as stated, any 32-bit integer including `0` and negatives; a collection number outside 32 bits is refused). Every member must be an atom whose marks derived a stored **tetrahedral** configuration in this import; one that is not (a plain atom, a center the marks do not determine, a wavy center, which derives `unknown`) refuses the whole record with `v3000_collection_member_not_stereocenter` and nothing is imported (when the marks of the whole record could not be read into configurations, for instance because the import has more commands than one transaction holds, the `stereo_unreadable` loss says so and a record with collections is refused as `v3000_collection_drawing_unreadable`, not by blaming its atoms), because dropping it would change which centers the file says belong together. Refused with closed reasons (nothing imported, the document and its revision unchanged): `v3000_empty_collection` (`ATOMS=(0)`), `v3000_duplicate_collection` (two `STEABS`, or two entries with the same type and index; a repeated index is a duplicate, not a merge), `v3000_collection_atom_shared` (an atom in two collections), `v3000_duplicate_index` (an atom twice in one collection), `v3000_invalid_collection` (including a number that does not fit 32 bits), `v3000_unknown_atom_index` (an atom the atom block does not define, forward references included, family `dangling_reference`). Families (`ct_reason_family`): the first five are `malformed_input`, `v3000_collection_not_supported_yet` (a collection name other than the three stereo kinds, a key other than `ATOMS`) is `unsupported_feature`. Appending to a document that already has groups never merges silently and never drops a member: the file's `STEABS` joins the document's absolute group (the domain allows one) and reports `stereo_group_merged` (`normalized`, `stereo`, `records[N].collections[k]`); an `and` or `or` number the document already uses for that kind is replaced by the smallest unused number from 1 up (counting the document's numbers and every number the file states or has been given, so it is deterministic and never reuses one; if every positive number is in use the import is refused as `resource_limit`) and reports `stereo_group_number` (`normalized`, `stereo`, same path) with both numbers in the reason, the group staying separate. As for every MOL import loss, these need `accept_loss`. A file with `STEABS` and chiral flag 0 imports the group as stated (no extra loss). *Verification.* Round trips export to import for absolute, `and` and `or` groups with omitted (written `0`), negative and 258 numbers, collision renumbering, split and merge after import, and V2000 consent, are tests and conformance cases; `scripts/verify-v3000-rdkit.py` (hand-run) compares only the **partition** (kind and member atoms) with RDKit 2022.03.2 in both directions, never the numbers (RDKit skips a negative `STERAC` number). Identity, CIP, validation and substructure results do not change with the groups. The numbers are written verbatim, negative and `0` ones included, but some other readers (RDKit 2022.03.2) skip a collection whose index is negative, so such a group may not be read there.

The editor stores only x/y positions. Import maps 1 Å to 20 points and flips the vertical axis, reporting each nonzero z coordinate dropped. Export uses the inverse mapping and writes four decimal places, reporting coordinate rounding. Source titles that differ from the target title, program/comment lines, SD fields and unselected records are reported as not stored; the full records are available in preview. Default V2000 hydrogen/valence fields do not establish a universal implicit-H count. Import records the editor's `inferred` state and both import and export report `default_hydrogen_interpretation` as a chemical uncertainty: Chemifree's separate bounded inference and other readers' policies can differ, including for charged or radical atoms. For a V3000 record or export the loss says only that the file states no hydrogen count and that Chemifree's inferred state and bounded valence inspection may differ from another reader's. Export also reports missing native IDs, extra placements, scene graphics/groups/references, settings, and nondefault hydrogen/attachment states; query or unknown chemical states that cannot be mapped are refused. The target document's source ID mapping is returned separately from losses. These are conversion reports, not chemical validation.

The parser treats the three header lines, fixed-width connection table and SD data as separate contexts. `M END`, `M CHG`, `M RAD`, `M ISO` and `V2000` literals are case-insensitive, while element symbols and user text retain their case. Header lines are limited to 80 Unicode characters, and the title cannot contain reserved CTfile tags (`$MDL`, `$$$$`, `$RXN`, `$RDFILE`); CTAB fields are ASCII and limited to 80 columns. Blank numeric fields mean zero. M CHG or M RAD overrides *all* compact atom charge/radical fields; unlisted states become zero. M ISO overrides all compact isotope differences; unlisted atoms are natural abundance. `M RAD 0` explicitly means no radical. Compact isotope differences are interpreted only for H–Ca using the documented shared baselines; other elements require M ISO. The guide does not identify the PTABLE.DAT version required to check absolute isotope bounds, so accepted positive masses 1–999 are reported with `isotope_validation: unknown`, rather than claiming that bound was verified.

**V3000 provider core (P28a; on the wire since P28b1).** The provider's `read_ctab` and `write_ctab` (a `ctab` selector, `CtTab::V2000` or `CtTab::V3000`, on the codec port) read and write a V3000 connection table; the codec's `read` and `write` keep their V2000-only behaviour, while `document.import` reads either dialect and `document.export` writes the dialect `ctab` names (see above). The record fields `ctab`, `chiral` and `collections` are not serialized in a preview; the bond field `stereo` (`none`, `wedge`, `hash`, `either`) is. The projection between records and drawings is described above (marks, derived configurations, losses) and under "Enhanced stereo collections" (groups); query atoms and bonds and dative bonds are not projected yet. Reading: the counts line must be 39 or more characters with `V3000` (case-insensitive) in columns 35 to 39 and only blank or integer fields before it; every line from `BEGIN CTAB` to `END CTAB` starts with exactly `M  V30 `, then `M  END`. A physical line that ends in `-` continues on the next line's text after its prefix (the hyphen is dropped and the text is joined without adding a space), a CRLF or LF line end is accepted, and a physical line is ASCII and at most 80 columns (a product limit, not stated by the public sources). `COUNTS na nb nsg n3d chiral [regno]` is cross-checked against the blocks; an optional sixth number must be the tagged `REGNO=<integer>` (anything else is `v3000_invalid_counts_line`) and is dropped with a `registry_number` loss. Atom lines are `index element x y z aamap` with `CHG` (-15 to 15, the CTfile guide's range and the V2000 writer's; anything else is `v3000_invalid_charge` on read and `invalid_charge` on write), `RAD` (0 to 3) and `MASS` (1 to 999, reported as `isotope_validation: unknown` like V2000); bond lines are `index type atom atom [CFG=n]` with type 1 to 4 (4 is aromatic: order 4 in the record, a V3000 bond of aromatic order in the drawing; the V2000 writer refuses it). Indices are positive, unique per kind and need not be contiguous; a bond or collection that names an atom the atom block has not defined (including one that comes before the atom block) is a dangling reference. The `COUNTS` chiral flag is read into the record and given no meaning by the reader: it is not turned into a group or a depiction. The V3000 writer states the record's flag as given (the export sets it as described under "Enhanced stereo collections"), the V2000 writer states `0` and reports a `chiral_flag` loss (`dropped`, `stereo`) for a record whose flag is set. `COLLECTION` lines `MDLV30/STEABS`, `STERAC<n>` and `STEREL<n>` with an `ATOMS=(n ...)` list are carried unmapped in the record, with the number exactly as written (any 32-bit integer, zero and negative included, no range is imposed); the application maps them to stereo groups (see "Enhanced stereo collections"), no meaning is stated for an atom in no collection, an atom in two collections, an empty collection, two `STEABS` and two entries of one type and index are refused by the reader and the writer alike (`v3000_collection_atom_shared`, `v3000_empty_collection`, `v3000_duplicate_collection`), and the V2000 writer refuses a record that holds any collection (`v3000_collection_not_supported_yet`) rather than drop it. Any other collection name and any key other than `ATOMS` are refused with `v3000_collection_not_supported_yet`. Writing: LF line ends (as the V2000 writer), the counts line `  0  0  0     0  0            999 V3000` (999 in the obsolete property count as the BIOVIA CTfile description gives it; the public ChemDraw files carry 0 there and the reader accepts both 0 and 999 and nothing else; RDKit 2022.03.2 reads the 999 form), `COUNTS na nb 0 0 chiral`, atoms with coordinates to four decimals (`coordinate_precision` as for V2000) and aamap `0`, bonds of order 1 to 4, properties in the order `CHG`, `RAD`, `MASS`, an optional `COLLECTION` block after the bonds (at most 2000 entries), empty blocks omitted, at most 2000 atoms and 4000 bonds (`resource_limit`), and a logical line longer than 80 columns continued with `-` between tokens (the line before the hyphen ends in a space), so the same record always gives the same bytes. Limits on read: the 4 MiB and 128-record input limits of V2000, 2000 atoms, 4000 bonds, 2000 collections, 16 KiB per logical line, 24 characters per number. Refused, never dropped, each with its own reason: SGroups (`COUNTS nsg > 0` or an `SGROUP` block), R-group definitions and atoms and `ATTCHPT`/`RGROUPS` properties, query atoms (`A`, `Q`, `X`, `*`, lists) and bond types 5 to 8, the query properties `HCOUNT`, `SUBST`, `UNSAT`, `RBCNT`, `TOPO`, `ENDPTS`, `ATTACH`, atom parity (`CFG`), `VAL`, any other atom or bond property, a nonzero atom-atom map, a bond `CFG` outside 0 to 3 or on a bond type that cannot carry it, bond types outside 1 to 4, 3D (`n3d > 0` or `OBJ3D`), reaction blocks, other unknown blocks and a second connection table in one record. Not claimed: that any of the refused constructs is understood, query matching, V3000 atom parity, what an unlisted stereo center means to another reader, equivalence with another program's V3000 output, and the public sources do not state the V3000 property catalog, continuation or line-length rules, so those parts follow the BIOVIA CTfile description and RDKit read-back only. The V3000 reasons (every `v3000_*` code, checked by a test that scans the V3000 source) and the shared framing codes listed below fall into the diagnostic families of the operation contract, kept in the codec as `ct_reason_family` (codes of the fixed-width V2000 columns and of the SD data section, such as `invalid_field`, `truncated_atom` and `unsupported_counts`, are not in that table and keep their own names) so that a later step can carry the fine reason in `details.reason` under the family `code` without a new table: `malformed_input` (`v3000_missing_prefix`, `v3000_unterminated_continuation`, `v3000_invalid_counts_line`, `v3000_count_mismatch`, `v3000_missing_block`, `v3000_duplicate_block`, `v3000_block_mismatch`, `v3000_block_not_closed`, `v3000_invalid_atom`, `v3000_invalid_bond`, `v3000_invalid_number`, `v3000_invalid_property`, `v3000_duplicate_property`, `v3000_invalid_index`, `v3000_duplicate_index`, `v3000_duplicate_bond`, `v3000_invalid_charge`, `v3000_invalid_radical`, `v3000_invalid_isotope`, `v3000_invalid_collection`, `v3000_empty_collection`, `v3000_duplicate_collection`, `v3000_collection_atom_shared`, `v3000_collection_member_not_stereocenter` and `v3000_group_member_not_drawn` (export only), `v3000_collection_drawing_unreadable` (raised by the application's projection, which a provider test therefore exempts from the source scan, and an application test scans), and the shared framing codes `empty_input`, `truncated`, `invalid_utf8`, `invalid_line`, `invalid_columns`, `invalid_header`, `reserved_title`, `truncated_counts`, `invalid_counts`, `missing_m_end`, `invalid_m_end`, `trailing_content`, `invalid_coordinate`, `invalid_bond`, `duplicate_bond`, `invalid_charge`, `invalid_isotope`, `invalid_count`), `dangling_reference` (`v3000_unknown_atom_index`), `unsupported_feature` (`v3000_unsupported_block`, `v3000_sgroup_not_supported`, `v3000_collection_not_supported_yet`, `v3000_collection_drawing_unreadable`, `v3000_unsupported_3d`, `v3000_unsupported_reaction`, `v3000_unsupported_rgroup`, `v3000_unsupported_query`, `v3000_unsupported_atom`, `v3000_unsupported_atom_property`, `v3000_unsupported_bond`, `v3000_unsupported_bond_property`, `v3000_multiple_structures`, `unsupported_atom`, `unsupported_bond`), `unsupported_format` (`unsupported_version`) and `resource_limit`.

For SDF, only after a complete mol block and completed data items does a line beginning `$$$$` terminate a record. Suffix text after that prefix is ignored with a loss report. Header lines and data lines can contain control-looking text in their proper context, but a value line beginning `$$$$` is rejected because it is ambiguous with a separator. Each data item ends with a blank line; a value line may have at most 200 Unicode characters. Field names begin with an ASCII letter and contain only ASCII letters, digits and underscore; DT-numbered fields are refused, and optional registry/header metadata around a named field is reported as lost. LF and CRLF are accepted and normalized to LF on output; isolated CR, NUL, malformed UTF-8, truncated sections and invalid indices are rejected. No silent truncation occurs.

The codec accepts at most 4 MiB input or output, 128 records, 999 atoms and 999 bonds per record, and 1,024 SD fields per record. An import is one transaction, so it must also fit the editor's 500-command and graph/coordinate limits. HTTP limits the JSON request to 256 KiB and CLI operations to 1 MiB; the web file picker limits source files to 200 KiB, and JSON escaping can reduce the effective transport limit. The web checks the source snapshot against the displayed drawing, invalidates previews on observed service restarts, and rechecks the export with the service at download time. It never overwrites a server-side destination.
## SMILES

`document.import` and `document.export` with `format: "smiles"` read and write one SMILES string through the same `Application::execute` as every other operation, so HTTP `POST /api/v1/operations`, `chemifree operations` and the web SMILES panel give the same replies. The port is `crates/application/src/smiles.rs`, the reader and writers are `crates/providers/src/smiles/`, and the operations are `crates/application/src/operations/smiles.rs`.

**Implemented:** a bounded OpenSMILES subset reader; a recorded-order writer; a canonical writer; preview, consent and import into an open document; read-only export. **Stereo read (P29c, see "Stereo read" below):** the tetrahedral marks `@` and `@@` of bracket atoms are stored as configurations, and a CXSMILES block of the fields `a:`, `o<n>:` and `&<n>:` as enhanced stereo groups. **Double-bond direction read (P29c2a, see "SMILES double-bond direction" below):** the directional bonds `/` and `\` beside a supported double bond are stored as double-bond configurations. **Stereo write (P29d):** recorded output writes supported tetrahedral `@`/`@@` configurations and CXSMILES enhanced stereo groups, and reports unsupported stereo as losses requiring consent. **Double-bond direction write (P29c2b, see "SMILES double-bond direction write" below):** recorded output writes a stored double-bond configuration as `/` and `\` where the reader reads the text back, and reports the others as consent-required losses. Canonical output writes no stereo and requires consent for dropped stereo. **Not implemented:** a directional bond in canonical output, directional bonds the reader cannot place (refused with one of the `directional_bond_*` codes, never ignored), the `@TH`, `@AL`, `@SP`, `@TB` and `@OH` classes (`stereo_class_unsupported`), every other CXSMILES field (`cxsmiles_field_unsupported`), query SMILES (`*` is refused with `wildcard_unsupported`), reaction SMILES, polymer or fragment-grouping SMILES, and InChI import or any other identifier reader. The canonical form is Chemifree's own: it is bound to `chemifree-chemical-graph-2` and is not the canonical SMILES of RDKit, Open Babel or any other toolkit, and no text of this form has been compared with another toolkit's. Models: the reader and the recorded writer are `chemifree-smiles-1`; the canonical writer is `chemifree-smiles-canonical-1`. Both use the valence model `chemifree-valence-3` and the aromaticity model `chemifree-aromaticity-2`, and report them as `valence_model` and `aromaticity_model`. A reply's `model_id` names the reader/recorded writer or the canonical writer that produced its result. A canonical export also reports `graph_model: "chemifree-chemical-graph-2"`.

### Reading

The grammar follows the [OpenSMILES specification](https://opensmiles.github.io/opensmiles/opensmiles.html), section "Formal Grammar" for the productions, "Rings" for ring labels, "Branches", "Bonds", "Disconnected Structures" for dots, "Charge", "Isotopes" and "Hydrogens" for bracket atoms, "Aromaticity" for aromatic atoms and "Parsing Termination" for the end of the string.

- **Atoms.** Bare `B C N O P S F Cl Br I` and aromatic `b c n o p s`; bracket atoms `[isotope? symbol hcount? charge? class?]` with any element symbol of the periodic table (aromatic `b c n o p s se as` in lower case). A bare atom has inferred hydrogens and no radical. A bracket atom states its hydrogens (`H`, `H1` to `H9`; none means 0); a second digit, as in `[CH10]`, is `invalid_hydrogen_count` (OpenSMILES "Hydrogens": `H` or `H` and one digit). An atom class (`:5`) is dropped with an `atom_class` loss.
- **Isotopes.** Digits with leading zeros allowed, so `[002H]` is `[2H]`. The drawing records mass numbers 1 to 999 only, so `[0C]` is refused as `isotope_zero` and a larger number as `isotope_out_of_range`.
- **Charges.** `+`, `-`, `++`, `--`, `+n` and `-n` with one or two digits; `+0` and `-0` are charge 0. The accepted range is the drawing's, `MIN_CHARGE` to `MAX_CHARGE` (−8 to +8); a charge written in the grammar but outside it, such as `[C+9]` or `[Fe-15]`, is refused as `charge_out_of_range`.
- **Bonds.** `-` `=` `#` `:` (aromatic) and the dot. `$` is refused as `quadruple_bond_unsupported`. A bond symbol directly before `(`, `)`, `.` or the end is a `dangling_bond` at the symbol.
- **Branches and rings.** Nesting up to 64 deep. Ring labels `0`-`9` and `%00`-`%99` follow their atom, or the bond symbol after that atom, and may be reused once closed, including at the atom that closes them. A label after `(`, `)` or `.` is refused with its own code. A dot may directly follow `(` (`C(.C)C`: the atom after it starts a new part and the branch returns to `C`), because the OpenSMILES "Formal Grammar" allows `'(' dot chain ')'`; it needs an atom after it (`C(.)C` is `dangling_dot`), and `(` cannot follow a dot (`C.(C)C` is `branch_without_atom`), because a chain is dot-separated branched atoms ("Disconnected Structures"). `%(nnn)` is valid OpenSMILES that the drawing cannot hold and is refused as `ring_label_extended_unsupported`. Two bonds between one pair of atoms and a bond from an atom to itself are refused. The earliest-opened ring that never closes is the one reported.
- **Ending.** The first space, tab, line feed or carriage return ends the string. Text after it is not read, except a CXSMILES block of stereo group fields (see "Stereo read"); if any of it is not whitespace it is reported as an `ignored_text` loss with its span, never dropped silently. Leading whitespace is `leading_whitespace`; empty text is `empty_input`.
- **Aromatic atoms.** An unspecified bond between two lower-case aromatic atoms is aromatic only if it lies on a ring; otherwise it is single, so biphenyl `c1ccccc1c1ccccc1` joins two aromatic rings by a single bond. An explicit `:` or a lower-case atom outside every ring is refused (`aromatic_bond_outside_ring`, `aromatic_atom_outside_ring`). Every ring system that holds an aromatic bond must be classified aromatic by `chemifree-aromaticity-2`; otherwise the string is refused as `aromatic_system_unsupported`, with the model's reason and the position of the system's first atom. Kekulé spellings are accepted as drawn.
- **Hydrogens and radical of a bracket atom** come from the shared valence model: the one radical state (none, else doublet, else a unique singlet or triplet) that makes `bond order sum + hydrogens` an allowed valence is recorded. When none does, or only singlet and triplet could, or the element and charge are outside the model (`[Fe]`, `[Na+2]`, `[CH2]`, `[C]`, `[CH]`, `[N]`), the radical is recorded as `unknown` and a `radical_state` loss is reported. The simple ions the model lists (`[Li+]`, `[Na+]`, `[K+]`, `[Mg+2]`, `[Ca+2]`, `[F-]`, `[Cl-]`, `[Br-]`, `[I-]`) read with radical none and no loss, so a salt such as `[Na+].[Cl-]` needs no consent.
- **Limits.** 16 KiB of input (`input_too_long` names the first character that does not fit, on a character boundary), and at most 500 atoms plus bonds, the size of one transaction (`command_limit`).
- **Positions.** Every refusal carries `details.offset` and `details.length` as UTF-8 byte offsets of a character boundary, `details.character_offset` and `details.character_length` in characters (counted from 0), `details.class`, and `details.reason` when the code names a family. A loss that names a part of the source writes it as `source[first..=last]`, the last byte included.

### Stereo read (P29c)

The reader stores what a text states about tetrahedral stereo and CXSMILES enhanced stereo groups, in the same transaction as the atoms and bonds. The sources are outside the functional specification, which has no SMILES or CXSMILES syntax: the [OpenSMILES specification](https://opensmiles.github.io/opensmiles/opensmiles.html), section "Tetrahedral Centers" (chirality: `@` anticlockwise, `@@` clockwise, a hydrogen written in the bracket is considered first after the preceding atom, ring-bond neighbours are ordered by their bonds) and the "Formal Grammar" production for the other classes, and, for the extension block, the "Support for Enhanced Stereochemistry" section of the [RDKit Book](https://www.rdkit.org/docs/RDKit_Book.html), whose examples (`C[C@H](F)C[C@H](O)Cl |a:4,&1:1|`, `|a:3,11,o1:14|`) are the whole evidence for the field syntax. Nothing else of CXSMILES is taken from it.

- **Tetrahedral marks.** `@` and `@@` after the element of a bracket atom (`[C@H]`, `[13C@@]`). The neighbours are taken in this order: the preceding atom; the bracket hydrogen (`H`, or `H1`) if there is one, so it is first when the atom starts a component (the text begins with it or follows a dot); the neighbours that ring labels written at this atom name, in the order the digits are written (for the label that opens here the partner is the atom that later closes it); then the branches and the chain in the order written. The marks `@` and `@@` are the turn of the last three seen from the first, which is the domain's `anticlockwise` and `clockwise`. The configuration is stored as `set_atom_configuration` would store it, with no bond mark (the atoms have no coordinates), so `chem.stereo` reports it as stored and not drawn, and a later MOL, CDXML or CDX export asks consent as for any such configuration. Four neighbours with no bracket hydrogen, or three neighbours and one bracket hydrogen, are read; the valence model must also give the atom a tetrahedral neighbour set (a neutral carbon with a hydrogen and three bonds does; a neutral nitrogen with a hydrogen and three bonds, or a charged carbon, does not). An unmarked atom states nothing.
- **Refused, never dropped.** `stereo_center_unsupported_here`: three bonds and no hydrogen (a lone pair as the fourth position, as in `[S@](=O)(C)CC`: the OpenSMILES text consulted does not define where the lone pair stands, so it is not guessed), an aromatic atom, or an atom the valence model gives no tetrahedral set. `stereo_neighbor_count`: any other count of bonds and hydrogens (`[C@H](F)Cl`, five neighbours). `stereo_class_unsupported`: the five named classes `@TH`, `@AL`, `@SP`, `@TB` and `@OH` with their digits, whole, also after `@@`; letters after `@` that name none of them (`[C@XX]`) are not a class and give `invalid_bracket_content` (`@TH1` and `@TH2` equal `@` and `@@` in OpenSMILES, and are not read as such yet). The directional bonds `/` and `\` are read, or refused with their own codes, as described in the next subsection. A `@` outside a bracket is `unexpected_character`.
- **CXSMILES block.** After the first whitespace, text that starts with `|` is a block that ends at the next `|`; an unterminated block is `cxsmiles_malformed`. Its fields are comma separated: `a:` (an absolute group), `o<n>:` (an or group) and `&<n>:` (an and group), each followed by comma separated 0-based atom indexes in the order the atoms are written (a ring label does not make an atom; the numbering goes on after a dot). The digits after `o` or `&` are the group number as stated (`o0:` is number 0); two entries of the same kind and number are one group, and all `a:` entries are one group. `o:` and `&:` without a number, and every other field (`r`, `$`, `c:`, `w:`, `^1:`, `Sg:`, `(...)` coordinates, and so on), are refused with `cxsmiles_field_unsupported` at the field. An index that is not an atom is `cxsmiles_atom_out_of_range`, an index whose atom has no `@` or `@@` is `cxsmiles_group_member_not_stereocenter`, and one that appears twice is `cxsmiles_member_repeated`. A tagged atom that is in no group has no statement and no loss. The block states no chiral flag and the document gets none. **Behaviour change:** before P29c every text after the first whitespace was an `ignored_text` loss, so `C |c:0|` or a block of coordinates or atom labels read as plain `C` with a consented loss; a block that starts with `|` is now read or refused, and only the three stereo group fields are read, so such a text is refused (`cxsmiles_field_unsupported`), never ignored. A block with no closing `|` is refused as `cxsmiles_malformed`. Text after the closing `|` is reported as `ignored_text` as before; text after whitespace that does not start with `|` is `ignored_text` unchanged.
- **Groups into the document.** The groups go through the same mapping as MOL V3000 collections and CDXML (`stereo_projection::group_commands`): importing into a document that already has groups never merges two numbered groups (a number in use is replaced by the smallest free one, reported as `stereo_group_number`) and an absolute group joins the document's absolute group (`stereo_group_merged`). A repeated request (same `request_id` and parameters) replays the committed reply with the same loss report: the two losses are found again from the document the commit produced. A group that cannot be mapped is refused with its own diagnostic: `cxsmiles_group_member_not_stereocenter` carries `class`, `family`, the field's span and the `group` and `atom` indexes, and an exhausted group number is `resource_limit`.
- **Bounds and atomicity.** Atoms, bonds, one configuration per mark and one command per group together stay within 500 commands (`command_limit`, with the span of the item that passes it); the import is one transaction, so a refusal anywhere leaves the document and its revision unchanged.
- **Diagnostic families.** Each refusal's `details.family` names the closed family of its class: `syntax` is `malformed_input`, `unsupported` is `unsupported_feature`, `chemistry` is `invalid_chemistry` and `limit` is `resource_limit`.
- **Verification.** `crates/providers/tests/smiles_stereo.rs` pins every neighbour order and refusal above by hand (hydrogen after the preceding atom, hydrogen first, explicit hydrogen atoms, ring labels at either end and out of numeric order, dot components, mirror pairs, respellings of one molecule giving one configuration, `F[C@H](Cl)[C@H](Br)I`, the 500-command bound); `crates/application/tests/smiles_stereo_import.rs` imports through `document.import`, checks the stored configurations, groups and renumbering, and that every refusal leaves the document unchanged; the conformance cases `smiles-stereo-read-configuration-and-groups` and `smiles-stereo-read-refusals-change-nothing` run the operations. `scripts/verify-smiles-stereo-rdkit.py` (hand-run, not part of `npm run check`) lets RDKit parse the same texts and labels each center with its own CIP code, compares them atom by atom with `chem.cip` on the imported configuration, and compares the groups as (kind, members), never by number; the run recorded for this change used RDKit 2026.09.1 (the CIP labeler and the CXSMILES group fields are not in 2022.03.2). Not claimed: equivalence with any other program's SMILES reader beyond those fixtures, stereo in written text, or that a written text reads back with the same configuration (the writers write none).

### SMILES double-bond direction (P29c2a)

The reader stores what the directional bonds `/` and `\` state about a double bond, in the same transaction as the atoms and bonds. This is the **read side**; the write side is the next subsection (P29c2b). Sources: the [OpenSMILES specification](https://github.com/opensmiles/OpenSMILES) (asciidoc source `opensmiles.asciidoc` of <https://opensmiles.github.io/opensmiles/opensmiles.html>), sections "Cis/Trans Configuration of Double Bonds" (the symbols "always come in pairs"; they "point" above or below the double bond relative to the carbon atom; `F/C=C/F` and `C(\F)=C/F` are trans, `F\C=C/F` and `C(/F)=C/F` are cis; a SMILES with conflicting up/down specifications such as `C/C(\F)=C/F` is invalid) and "Partial Stereochemistry" (some double bonds may be marked and others not: `F/C=C/C/C=C\C` is completely specified, `F/C=C/CC=CC` partially), and the [RDKit Book](https://rdkit.org/docs/RDKit_Book.html), section "Stereogenic bonds" (double-bond stereo "is not present in a ring with less than eight atoms"), which is the only source of the ring limit; OpenSMILES gives no number.

- **Reading rule.** A mark belongs to the bond between the atom written before it and the atom after it, and means that the second atom is above (`/`) or below (`\`) the first. So a substituent written after the double-bond atom with `/` is above it, and one written before it with `/` is below it: `F/C=C/F` is trans, `C(\F)=C/F` is trans, `C(/F)=C/F` is cis. Each double bond is read from its own marked neighbours: the bond between two double bonds serves both (`C/C=C/C=C/C`: each is trans). The two neighbours of one double-bond atom may both be marked when they lie on opposite sides (`F/C(/CC)=C/F`). The result is stored as `set_bond_configuration` would store it (`relative`, one neighbour per end, `same_side` or `opposite_sides`; the core canonicalizes the neighbour, so an unmarked lower-numbered neighbour is stored with the relation flipped), with no mark on any bond (the atoms have no coordinates) and no schema change. E/Z is not stored; `chem.cip` computes it. Each configuration is one command toward the 500-command bound (`command_limit`, at the span of the last configuration when it is the item that passes it).
- **Read.** At least one marked single bond beside each end of a double bond, where each end is a neutral carbon with at most two other neighbours or a neutral nitrogen with one other neighbour (C=C, C=N, N=N: oximes, azo); explicit `[H]` atoms and the hydrogens stated in a bracket (`[CH]`, `[NH]`) count as neighbours of the end, so `F/[CH]=[CH]/F` and `[H]/C(F)=C/F` are read and `Cl/[CH](F)=C/F`, `[CH2](/F)=C/F` and `F/[NH](C)=C/F` are `directional_bond_end_unsupported` (hydrogens left to the valence rule are not counted); marks on chain single bonds next to aromatic atoms (`c1ccccc1/C=C/F`); ring double bonds in rings of at least eight atoms (the ring size is the shortest cycle through the bond, found by a search that stops at seven bonds). Whether a stored configuration is stereogenic is not checked, as for P29c centres.
- **Shared-bond exemption.** A double bond with marks at one end only (`C/C=C/C=CC`, `OC(=O)/C=C/C(=O)O`, whose carbonyls also touch the marks) states nothing, and gets no record and no loss, **only when every mark it has is on a bond that a fully marked and accepted neighbouring double bond also reads**. This is a **Chemifree rule, not specification text**: the OpenSMILES partial-stereochemistry example (`F/C=C/CC=CC`) has a second double bond that no mark touches, and the specification says nothing about a one-sided double bond that shares a mark. The rule extends that example to any double bond whose every mark is served by an accepted configuration. The intent is that a C=O has no stereo, so `OC(=O)/C=C/C(=O)O` is accepted; the alternative would be to refuse those texts too. Any other one-sided double bond is refused (`directional_bond_incomplete`), including one with an own mark besides the shared one (`F/C=C/C(/F)=CC`, refused at the own mark).
- **Ring double bonds with a chain hung from the ring.** A ring double bond of fewer than eight atoms whose only marks are on chain bonds hung from the ring is not a candidate, as no double bond is in an aromatic ring: its marks are judged like those beside no double bond. So `C1=CC=CC=C1/C` and `c1ccccc1/C` are both `directional_bond_without_double_bond`, at the mark. A mark on a bond of that ring itself (`C1=C/CCCC1`), a ring double bond of eight atoms or more (`C1=C/CCCCCC1`) and a mark on a double bond's own end in a chain (`C/C=C`) stay `directional_bond_incomplete`. A chain double bond that is marked (`C1=CC=CC=C1/C=C/C`) is read, and the ring's stays silent.
- **Refused, never dropped.** Each refusal points at the span of a mark, and an error is a single one: when several things are wrong, the earliest mark in the text is reported (a documented policy, not the most serious fault). Within one double bond the faults are tried in this order, and the first one that applies is that bond's: cumulated, then an end that cannot be read (begin end first), then a conflict at an end, then a ring of fewer than eight atoms. So `C/C(\F)=P/F` is an unsupported end, not the conflict at the earlier mark, and a conflict inside a small ring is reported as the conflict (`directional_bond_conflict`), not as the ring. Every double bond and every mark is judged, and the fault with the earliest span among them is the one reported. The classes and `details.family` are those of the table below.

| Code | Class / family | Case |
| --- | --- | --- |
| `directional_bond_conflict` | chemistry / `invalid_chemistry` | Two marks at one end put their atoms on the same side: `C/C(\F)=C/C`. Reported at the second mark. |
| `directional_bond_incomplete` | syntax / `malformed_input` | Marks at one end of a double bond only, and not exempt: `C/C=C`, `C/C=CC`. |
| `directional_bond_without_double_bond` | syntax / `malformed_input` | A mark on a bond whose atoms are in no double bond: `F/CC`, `C/C/C`, `C/C#C`. |
| `directional_bond_small_ring` | unsupported / `unsupported_feature` | A marked double bond in a ring of fewer than eight atoms: `C1C/C=C\CCC1`. |
| `directional_bond_cumulated` | unsupported / `unsupported_feature` | A marked double bond whose end atom carries another double or a triple bond: `F/C=C=C/F`, `F/C=C=C=C/F`. |
| `directional_bond_end_unsupported` | unsupported / `unsupported_feature` | An end that is aromatic, charged, another element than C or N, a carbon with more than two other neighbours or a nitrogen with more than one: `F/P=C/F`. Judged before a conflict. |
| `directional_bond_unsupported` | unsupported / `unsupported_feature` | Kept with a narrower meaning: a mark on a ring-closure digit (`C/1=C/CCCCCCC1`; toolkits disagree on which atom such a mark is relative to) or on a ring bond between two aromatic (lower-case) atoms (`c1ccc/cc1`). A mark on a chain bond between aromatic atoms that is on no ring (`c1ccccc1/c1ccccc1`) is not this: the generic rules refuse it as `directional_bond_without_double_bond`. |

`/=` and `//` stay `repeated_bond_symbol`; a mark at the end or start of the text is `dangling_bond` or `bond_without_atom`.

- **Writing (this part did not change it; superseded by P29c2b).** The writers wrote no `/` or `\` in this part. P29c2b, below, writes them in recorded output; canonical output still writes none.
- **`chem.cip`.** A stored configuration in a ring of eight or more atoms is not "not applicable": `chem.cip` lists and labels every stored configuration in both scopes (`C1CC/C=C\CCC1` gives `Z`). Its `stereogenic` scan skips an unstored ring double bond, so the same ring written without marks lists nothing. CIP is not changed by this part.
- **Atomicity and replay.** One transaction: any refusal leaves the document and its revision unchanged. The reader adds no loss for a double bond (the exempt double bond is not a loss either), so a repeated request (same `request_id` and parameters) replays the committed reply with the same loss report and result.
- **Verification.** `crates/providers/tests/smiles_double_bonds.rs` pins every rule above by hand (the four spellings of `F/C=C/F`, branch marks, conjugated chains, the exemption on both sides, hydrogens, nitrogen ends, aromatic neighbours, the ring size 7 against 8 and a chord ring, components, nesting, each refusal code with its span, the command bound 500 against 501); `crates/application/tests/smiles_stereo_import.rs` imports through `document.import`, checks the stored configuration, the `chem.cip` label, atomicity, replay and the export behaviour above; the conformance cases `smiles-directional-bond-read-configuration` and `smiles-directional-bond-refusals-change-nothing` run the operations. `scripts/verify-smiles-stereo-rdkit.py` (hand-run, not part of `npm run check`, RDKit 2026.09.1) reads the same texts with RDKit, embeds each in 3D and compares the measured same/opposite side of the neighbours Chemifree stored, and compares RDKit's E/Z labels with `chem.cip` for the same double bonds (a bond that no one of ten fixed seeds embeds is a failure, never a note; the last hand run, with RDKit 2026.09.1 on this commit, printed `70 fixtures, 35 double-bond geometry comparisons, 0 failures`); refusal fixtures are listed separately, because RDKit reads many of them (it ignores most marks that Chemifree refuses, and reads `F/P=C/F` and the ring-closure-digit text). Not claimed: equivalence with any other program's reader beyond those fixtures, or that a stored configuration is stereogenic.

### SMILES double-bond direction write (P29c2b)

The **recorded** SMILES writer writes a stored double-bond configuration as the directional bonds `/` and `\`. Canonical SMILES is unchanged: it writes no stereo and a drawing with a stored configuration still needs consent to export it canonically (`stereo_configuration`, "SMILES stereo is not written"). No document schema, dependency or error code changes: the omissions below use the existing consent-required `stereo_configuration` loss.

Sources: the [OpenSMILES specification](https://github.com/opensmiles/OpenSMILES), "Cis/Trans Configuration of Double Bonds" (a mark states on which side of the double bond the atom after it lies, relative to the atom before it), and, for the ring limit shared with the reader, the [RDKit Book](https://rdkit.org/docs/RDKit_Book.html), "Stereogenic bonds". Everything else below is a **Chemifree rule, not specification text**.

- **Symbol (spec-derived).** The reader's table of sides is inverted: for each marked bond the writer knows on which side of the double bond the neighbour must lie as seen from the double-bond atom, and writes `/` or `\` from the order the text is actually written in (the same configuration is `F/C=C/F` or `C(\F)=C/F` depending on the walk), not from any order assumed earlier. A bond whose far atom is written first gets the other symbol. Where nothing else fixes the sides, the first mark in the text is a `/`, so `F\C=C\F` is written `F/C=C/F`.
- **Placement (Chemifree rule).** The fewest marks that state a configuration: one marked single bond at each end of the double bond; an end that already has a mark from a neighbouring double bond gets no second one. A bond that a double bond already placed marks on serves its neighbour as well, so a conjugated chain shares them (`C/C=C/C=C/C`). Otherwise the first neighbour in the order the text reaches them that is not written as a ring-closure digit is marked. A mark is never put on a ring-closure digit. If the marked neighbour is not the one the configuration names (it stores the lowest-numbered neighbour), the relation is flipped as the domain does.
- **Order and conflicts (Chemifree rule).** Configurations are placed greedily in bond-identifier order. A configuration's marks are added only if (a) they agree with marks already placed, and (b) every double bond they touch still states only what is stored: a double bond marked at **both** ends is read as configured, so it must be a stored configuration that is not yet left out and agrees with the marks. A double bond marked at one end only states nothing (the reader's shared-bond exemption), so it is allowed. Two marks at one end that would lie on the same side are refused (the reader refuses them). Otherwise the configuration is left out, whatever its position in a chain: with `CC=CC=CC=CC`, the outer double bonds stored and the middle one not, the first outer one is written (`C/C=C/C=CC=CC`) and the second is left out, because its marks would also state the unstored middle bond. Which one is left out depends on the bond identifiers, and the result is the same every time.
- **Left out, with a loss.** An unknown state; a ring double bond in a ring of fewer than eight atoms, a cumulated end, an aromatic or charged end, an end that is not C or N, a carbon end with more than two other neighbours or a nitrogen end with more than one, counting explicit hydrogen atoms **and the hydrogens a bracket states** (the reader's judgement, shared code: the writer asks the reader's own checks about the text as it will be written, so it never writes text the reader refuses); an end whose reference is an implicit hydrogen or lone pair (no bonded atom to mark) or whose only other neighbours are ring-closure digits; a stored configuration on a bond that the written text does not hold as a double bond (only reachable from a graph edited outside the document), and the contradictions above. The loss is the existing one: feature `stereo_configuration`, `source_path` `bonds[bN]`, `dropped`, `stereo`, consent required, and a `reason` that names the cause (for example "... the double bond is in a ring of fewer than eight atoms, so the reader would refuse it", "... would also state a configuration for double bond b4, which is not stored or is not written"). After consent the text has no mark for that bond only. The loss list is a function of the drawing, so an export repeated or replayed gives the same losses and the same digest. The classification does not depend on aromatic or Kekulé spelling of the rest of the molecule.
- **Read-back.** The text is read again with the same reader and the configurations it states must equal the retained ones exactly (none more, none fewer, each with the stored relation, centres and groups as before), else the export is refused as `readback_mismatch` ("a double-bond configuration differs from the one written"). An emitter given a mark on a ring-closure digit or on a bond that is not single also refuses (`readback_mismatch`); neither occurs with the placement above.
- **Counted.** Each written configuration is one command toward the 500-command bound the reader applies to the text (`command_limit` at export when it would pass it).
- **What stays unsupported.** Directional bonds in canonical output; lone-pair or implicit-hydrogen-only ends; rings of fewer than eight atoms; cumulated ends; stored configurations on aromatic atoms; whether a stored configuration is stereogenic is not checked; no claim of equivalence with another program beyond the checked fixtures.
- **Verification.** `crates/providers/tests/smiles_double_bond_write.rs` states every expected text by hand (the four spellings of `F/C=C/F`, branch ends, shared marks, H-bearing ends, 8- and 9-membered rings, multiple components, the 7-membered ring, a stated bracket hydrogen, unknown and unreadable states, the ring-closure digit, a conjugated ring, the greedy omission and the bond-id order, and a same-side conflict at one end); unit tests in `double_bonds.rs` pin each fault the writer asks the reader about, and in `writer.rs` three wrong placements (flipped, extra, missing) and a mark on a ring-closure digit are refused (`readback_mismatch`), a configuration on a non-double bond is left out with a loss, and the requests are ordered by bond identifier whatever order the records come in. `crates/application/tests/smiles_stereo_import.rs` checks `document.export` (no consent for a supported configuration, the consent loss and digest for an omitted one, replays); the conformance cases `smiles-directional-bond-write-recorded` and `smiles-directional-bond-write-omissions` run the operations. `scripts/verify-smiles-stereo-rdkit.py` (hand-run, not part of `npm run check`, RDKit 2026.09.1) parses every export with RDKit, measures cis/trans of the exported text in 3D and compares it with the stored relation, compares RDKit's isomeric SMILES of the export with the input's, and checks five write-only drawings against RDKit; a fixture that cannot be embedded or compared is a failure, never a note. The last hand run (RDKit 2026.09.1, summary line and the deliberately broken build that makes it fail) is recorded in the P29c2b entry of docs/HANDOFF.ko.md.

### Import

`document.import` with `format: "smiles"`, `document_id`, `expected_revision`, `source`, and `preview: true` returns `{ atoms, bonds, ignored_text, snapshot, document, preview_digest, valence_model, aromaticity_model }` and the loss report, without changing the document. The digest binds the service instance, the source, the exact pre-import snapshot, the prospective document and the complete loss report, and is rechecked under the document lock; a digest from before a restart, or made for other text or for another state of the drawing at the same revision, is refused as `preview_changed`; an `expected_revision` that is no longer current is `revision_conflict`. To commit, repeat the request with `preview_digest` and, when a loss needs it, `accept_loss: true`. Only losses that change the chemistry or drop text need acceptance: `radical_state`, `ignored_text` and `atom_class`. Without acceptance the commit is refused as `loss_consent_required` with `details.features` and the full `details.losses` that need it, and nothing changes. `generated_layout`, always reported, needs none: a plain `CCO` with its digest commits with `accept_loss` absent. The import appends to the drawing as one undoable transaction and replies with the document and `import_mapping`. Repeating a request ID returns the committed reply and does not place atoms again.

SMILES has no coordinates and no structure layout exists yet, so new atoms are placed on a grid of 10 columns, 30 points apart, in input order: starting at (40, 40) in a drawing with no atoms, otherwise at the x of the leftmost existing atom or graphic point and 60 points below the lowest one (atoms and the end points of lines, rectangles and arrows count; groups have no extent of their own). A position outside the drawing's coordinate range is `coordinate_out_of_range`. The document must have exactly one placement (`unsupported_scene`).

### Export

`document.export` with `format: "smiles"`, `document_id`, `style` (`recorded` or `canonical`, required) and optionally `accepted_losses` and `time_limit_ms` (the limit of [computation time limit and cancel](#computation-time-limit-and-cancel); only SMILES export takes it, and MOL/SDF, CDXML, CDX and SVG export keep no time limit and refuse the key as an unknown parameter) returns `{ content, style, snapshot, atom_order, folded_hydrogens, valence_model, aromaticity_model, graph_model }`. It is read-only: it changes neither the document nor its undo history, and `snapshot` is the exact working copy written. A reply's `model_id` is `chemifree-smiles-1` or `chemifree-smiles-canonical-1`. The order is: a parameter error, `not_running` when no SMILES codec is attached, the time-limit refusal (before any document is read), the document read, the write, a codec refusal, and last the consent refusal. A stop of the time limit or a cancel during the write is `outcome: "error"` with `result: null`, the snapshot's `document_revision`, the style's `model_id`, and a `time_limit` (`details: { limit_ms }`) or `cancelled` diagnostic; it carries no `loss_report` and no `loss_digest`, does not evaluate `accepted_losses`, and is never a loss or a consent. The time-limit refusal and a stop name the style's `model_id`; the other error replies of export keep `model_id` null.

Every string either writer returns is accepted by the same reader: the codec reads its own text back and compares every atom's element, charge, isotope, hydrogens and radical, and every bond, before returning it. The code of a read-back fault is decided by the lowest-numbered faulty atom of the drawing (of the canonical numbering for canonical text), whatever the order of the text, and the reason names that atom. A bond that does not join two different atoms of the graph is `bond_endpoints_invalid`. A graph for which no such string exists is refused with a `not_writable` or `limit` code and the real cause (for example the aromaticity model's reason is checked before any atom is formatted).

- **Recorded** keeps the drawing's atoms and bonds as recorded: Kekulé bonds stay Kekulé and aromatic bonds stay aromatic. The text is a depth-first walk from the first atom of the document, visiting neighbours in document order, starting each further component at its first unwritten atom; the last child of an atom continues the chain and earlier children are branches. If that order would nest branches deeper than the reader accepts (64), the order changes for the whole text: at every atom the child with the deepest branching continues the chain (the one latest in the document order among equally deep ones) and the others stay in document order as branches, which is the least depth any order gives that walk (a long backbone whose pendant atoms are numbered last is written `C(C)C(C)...`, not nested 69 deep). A text that fits keeps the order above unchanged. `atom_order` lists the drawing's atom for each written atom, in text order. Ring labels are the lowest free number from 1; a label that closes at an atom is free for the rings that open at that atom.
- **Canonical** writes one text for every drawing of the same chemical graph under `chemifree-chemical-graph-2`: it is the same walk (with the same depth rule) in the canonical numbering of that model, so atom order, Kekulé or aromatic spelling and explicit hydrogen vertices do not change it. It is refused (`canonical_unsupported`, with the model's reason in `details.reason`) wherever the graph model has no identity: atoms outside the valence model (metals such as `[Fe]`, ions the model does not list such as `[Na+2]`), a radical the model cannot state, unknown hydrogens, query atoms, dative or unknown bonds, or an aromatic system the aromaticity model cannot class. More than 128 atoms, or a search beyond its work limit, is `canonical_limit`. Terminal explicit hydrogens are folded into their neighbour's count and listed in `folded_hydrogens`, not in `atom_order`. Doublet radicals such as `[CH3]` and `[OH]` have a canonical form; `[CH2]`, `[C]` and `[CH]` have none, and drawn singlet and triplet radicals are `radical_not_writable`.
- **Bracket atoms.** An atom is written bare only if it is in the organic subset, neutral, natural isotope, without radical, and reads back with the same hydrogens; any other is written in brackets with the hydrogen count the shared valence model determines. A metal or ion outside the valence model (`Fe`, `Na+2`) is written as `[Fe]`, `[Na+2]` with the hydrogen and radical assumptions reported as the consent-required losses `hydrogen_assumption` and `radical_assumption`: the model has no valence for them and a reader records their radical as unknown. The simple ions the model lists (see Valence inspection) are written as `[Na+]`, `[Cl-]` like any other atom with a valence, and both writers give `[Na+].[Cl-]` for the salt.
- **Refusals.** At the codec, query atoms, unknown or dative bonds, unknown hydrogens, attachment points (`attachment_points_not_writable`; the operation removes them first, with consent, see Losses), a lower-case aromatic element without a symbol, aromatic bonds the model does not class aromatic, isotopes or charges the reader would refuse, radicals SMILES cannot state, more than 9 hydrogens on an atom, an empty drawing, more than 500 atoms plus bonds, more than 99 ring labels open at once, and text longer than 16 KiB are each refused with their own code (table below). `branch_depth_limit` is the reader's: a writer reorders the branches instead (see Recorded), and no graph of at most 500 atoms plus bonds needs deeper nesting than the reader accepts.
- **Losses.** The same vocabulary as MOL/SDF export, worded for SMILES, each only when the drawing holds the content: `coordinates`, `title` (a title other than the default), `native_ids`, `placement_id`, `extra_placement`, `scene_object`, `scene_references`, `selection` and `drawing_settings`. The writers add `hydrogen_state` (stated counts written as bare atoms; for the recorded writer also counts the valence model determines that a bracket states), and for the canonical writer `folded_hydrogens` and `aromatic_bonds` (Kekulé bonds written as aromatic atoms), all reported as normalized. A hydrogen count the valence model does not determine (an atom with an unusual valence whose hydrogens are inferred) is never written as a guess: when the text would need a bracket for it the write is refused as `hydrogens_not_writable`. Attachment points are not written: the export drops them, reports `attachment_points` and needs consent, in both styles, rather than being refused. Recorded output writes supported stored tetrahedral centers and complete enhanced stereo groups. Stored double-bond configurations are written as `/` and `\` where they can be (see "SMILES double-bond direction write"). Bond marks, unknown and lone-pair centers, double-bond configurations that cannot be placed, and groups with an unwritable member are omitted with consent-required `stereo_marks`, `stereo_configuration`, or `stereo_group` losses. Negative group numbers are assigned unused nonnegative numbers and omitted numbers are written as zero; changes are reported as `stereo_group_number` normalization losses. Emitted groups also report `stereo_group_identity` as an informational loss because the document group IDs are not stored. Canonical output writes no stereo, and requires consent for omitted marks, configurations, and groups. The recorded writer reads its output back and compares the configurations and groups as well as the graph.
- **Consent.** Losses that are marked as needing it (`hydrogen_assumption`, `radical_assumption`, `attachment_points`, `stereo_marks`, `stereo_configuration`, `stereo_group`) are returned in a `loss_consent_required` refusal with `details.features`, `details.losses` and `details.loss_digest`, the digest of those losses and of the exact text; repeating the request with `accepted_losses` set to it returns the content. A digest of other losses or other text is `loss_consent_changed`. This is the same refusal envelope as a CDX export.

### Error codes

Every refusal has one of five classes: `syntax` (the text is not SMILES), `unsupported` (valid or plausible SMILES the drawing or this implementation cannot hold), `chemistry` (the text contradicts itself, such as a stereo group member that is no stereocenter), `limit` and `not_writable` (the drawing has no string the reader accepts). A reading refusal also carries `details.family`, the closed diagnostic family of its class: `syntax` is `malformed_input`, `unsupported` is `unsupported_feature`, `chemistry` is `invalid_chemistry` and `limit` is `resource_limit`; a `not_writable` refusal has none. The table is the codec's own and is compared with it by a test.

| Code | Class | Meaning |
| --- | --- | --- |
| `empty_input` | syntax | The SMILES text is empty. |
| `leading_whitespace` | syntax | A SMILES string cannot start with whitespace; whitespace ends it. |
| `unexpected_character` | syntax | This character cannot appear here. |
| `bracket_required` | syntax | Only B, C, N, O, P, S, F, Cl, Br and I (and aromatic b, c, n, o, p, s) can be written without brackets. |
| `unclosed_bracket` | syntax | A bracket atom is not closed by ]. |
| `missing_element_symbol` | syntax | A bracket atom needs an element symbol. |
| `unknown_element` | syntax | This is not an element symbol. |
| `invalid_bracket_content` | syntax | A bracket atom is isotope, symbol, hydrogen count, charge and atom class, in that order. |
| `hydrogen_count_on_hydrogen` | syntax | A hydrogen atom cannot have a hydrogen count; write [H][H] for hydrogen gas. |
| `invalid_hydrogen_count` | syntax | A hydrogen count is H or H and one digit. |
| `invalid_ring_label` | syntax | A ring label is one digit, or % and two digits. |
| `ring_digit_at_start` | syntax | A ring label must follow the atom it belongs to; there is no atom yet. |
| `ring_digit_after_branch_open` | syntax | A ring label cannot follow ( ; it belongs right after an atom. |
| `ring_digit_after_branch_close` | syntax | A ring label cannot follow ) ; it belongs right after an atom. |
| `ring_digit_after_dot` | syntax | A ring label cannot follow a dot. |
| `ring_digit_after_detached_bond` | syntax | A ring bond symbol must directly follow the atom it belongs to. |
| `ring_bond_mismatch` | syntax | The two ends of a ring closure name different bond symbols. |
| `ring_self_bond` | syntax | An atom cannot be bonded to itself by a ring closure. |
| `duplicate_bond` | syntax | Two atoms cannot be joined by more than one bond. |
| `unclosed_ring` | syntax | A ring label is opened but never closed. |
| `branch_without_atom` | syntax | A branch needs an atom before its opening parenthesis. |
| `empty_branch` | syntax | A branch cannot be empty. |
| `unmatched_close_branch` | syntax | This ) has no matching (. |
| `unclosed_branch` | syntax | This ( is never closed. |
| `dangling_bond` | syntax | A bond symbol must be followed by an atom. |
| `dangling_dot` | syntax | A dot must be followed by an atom. |
| `bond_without_atom` | syntax | A bond symbol needs an atom before it. |
| `dot_without_atom` | syntax | A dot needs an atom before it. |
| `repeated_bond_symbol` | syntax | Two bond symbols cannot follow each other. |
| `stereo_class_unsupported` | unsupported | Only the tetrahedral marks @ and @@ are read; @TH, @AL, @SP, @TB and @OH classes are not supported. |
| `stereo_center_unsupported_here` | unsupported | The drawing cannot hold a tetrahedral configuration on this atom. |
| `stereo_neighbor_count` | unsupported | A tetrahedral mark needs four neighbours counting one bracket hydrogen: three or four bonds. |
| `directional_bond_unsupported` | unsupported | A directional bond / or \ on a ring-closure digit or on an aromatic bond is not read. |
| `directional_bond_conflict` | chemistry | Two directional bonds at one end of a double bond place their atoms on the same side. |
| `directional_bond_incomplete` | syntax | A double bond needs a directional bond / or \ at both ends; this one has them at one end only. |
| `directional_bond_without_double_bond` | syntax | A directional bond / or \ must be next to a double bond. |
| `directional_bond_small_ring` | unsupported | A double bond in a ring of fewer than eight atoms has no cis/trans configuration here. |
| `directional_bond_cumulated` | unsupported | A directional bond next to a cumulated double bond is not read. |
| `directional_bond_end_unsupported` | unsupported | A double-bond configuration is read only for C=C, C=N and N=N ends with at most two other neighbours. |
| `cxsmiles_malformed` | syntax | The CXSMILES block is not well formed: a bar, comma separated fields, a closing bar. |
| `cxsmiles_atom_out_of_range` | syntax | A CXSMILES atom index is beyond the atoms of the SMILES text. |
| `cxsmiles_field_unsupported` | unsupported | Only the stereo group fields a:, o<n>: and &<n>: of a CXSMILES block are read. |
| `cxsmiles_group_member_not_stereocenter` | chemistry | A CXSMILES stereo group lists an atom that carries no @ or @@ mark in the text. |
| `cxsmiles_member_repeated` | chemistry | A CXSMILES atom index appears twice among the stereo groups; a center is in one group. |
| `wildcard_unsupported` | unsupported | The wildcard atom * is not supported. |
| `quadruple_bond_unsupported` | unsupported | The quadruple bond $ is not supported. |
| `ring_label_extended_unsupported` | unsupported | Ring labels written %(nnn) are not supported; use 0 to 99. |
| `charge_out_of_range` | unsupported | The charge is outside what the drawing can record. |
| `isotope_zero` | unsupported | The drawing records isotope mass numbers 1 to 999 only; isotope 0 is not recorded. |
| `isotope_out_of_range` | unsupported | The drawing records isotope mass numbers 1 to 999 only. |
| `aromatic_bond_outside_ring` | unsupported | An aromatic bond must lie in a ring; this one is a bridge between rings or a chain bond. |
| `aromatic_atom_outside_ring` | unsupported | A lower-case aromatic atom must be joined to its ring by aromatic bonds. |
| `aromatic_system_unsupported` | unsupported | The aromatic ring system is outside the aromaticity model. |
| `input_too_long` | limit | The SMILES text is longer than the reader accepts. |
| `command_limit` | limit | The structure has more atoms and bonds than one import can add. |
| `branch_depth_limit` | limit | Branches are nested more deeply than the reader accepts. |
| `ring_label_limit` | limit | More rings are open at once than there are ring labels. |
| `output_too_long` | limit | The SMILES text would be longer than the reader accepts. |
| `canonical_limit` | limit | The canonical search exceeded its size or work limit. |
| `empty_graph` | not_writable | A drawing with no atoms has no SMILES. |
| `query_atom_not_writable` | not_writable | A query atom has no SMILES form here. |
| `bond_endpoints_invalid` | not_writable | A bond must join two different atoms of the drawing. |
| `bond_kind_not_writable` | not_writable | Only single, double, triple and aromatic bonds can be written. |
| `aromatic_not_writable` | not_writable | The aromatic bonds are not an aromatic ring system under the aromaticity model. |
| `aromatic_element_not_writable` | not_writable | This element has no lower-case aromatic symbol. |
| `charge_not_writable` | not_writable | The charge is outside what the reader accepts. |
| `isotope_not_writable` | not_writable | The isotope mass number is outside 1 to 999. |
| `hydrogens_not_writable` | not_writable | The hydrogen count of this atom cannot be determined or written. |
| `radical_not_writable` | not_writable | SMILES cannot state this radical so that it reads back the same. |
| `attachment_points_not_writable` | not_writable | Attachment points have no SMILES form. |
| `canonical_unsupported` | not_writable | The graph has no canonical form under the chemical graph model. |
| `readback_mismatch` | not_writable | The written text does not read back as the same atoms and bonds. |

### Examples: reading refusals

`crates/application/tests/smiles_doc_claims.rs` runs every row: the input, the code, and the byte offset of its span.

| Input | Code | Offset |
| --- | --- | --- |
| `C=(O)` | `dangling_bond` | 1 |
| `C1CC` | `unclosed_ring` | 1 |
| `C2CC1CC` | `unclosed_ring` | 1 |
| `C(` | `unclosed_branch` | 1 |
| `C(C)1` | `ring_digit_after_branch_close` | 4 |
| `C(1CC)` | `ring_digit_after_branch_open` | 2 |
| `C11` | `ring_self_bond` | 2 |
| `C1C1` | `duplicate_bond` | 3 |
| `C.(C)C` | `branch_without_atom` | 2 |
| `C(.)C` | `dangling_dot` | 2 |
| `.C` | `dot_without_atom` | 0 |
| `[CH10]` | `invalid_hydrogen_count` | 4 |
| `c` | `aromatic_atom_outside_ring` | 0 |
| `c:c` | `aromatic_bond_outside_ring` | 1 |
| `c1ccccc1:c1ccccc1` | `aromatic_bond_outside_ring` | 8 |
| `c1ccccc1.c1cccc1` | `aromatic_system_unsupported` | 9 |
| `c1ccc2c(c1)CCCC2` | `aromatic_system_unsupported` | 0 |
| `C/1=C/CCCCCCC1` | `directional_bond_unsupported` | 1 |
| `c1ccc/cc1` | `directional_bond_unsupported` | 5 |
| `C/C=C` | `directional_bond_incomplete` | 1 |
| `F/CC` | `directional_bond_without_double_bond` | 1 |
| `C/C(\F)=C/C` | `directional_bond_conflict` | 4 |
| `C1C/C=C\CCC1` | `directional_bond_small_ring` | 3 |
| `F/C=C=C/F` | `directional_bond_cumulated` | 1 |
| `F/P=C/F` | `directional_bond_end_unsupported` | 1 |
| `[C@H](F)Cl` | `stereo_neighbor_count` | 2 |
| `[C@TH1H](F)(Cl)Br` | `stereo_class_unsupported` | 2 |
| `[S@](=O)(C)CC` | `stereo_center_unsupported_here` | 2 |
| `C[C@H](F)Cl |a:0|` | `cxsmiles_group_member_not_stereocenter` | 15 |
| `C[C@H](F)Cl |r|` | `cxsmiles_field_unsupported` | 13 |
| `[0C]` | `isotope_zero` | 1 |
| `[C+9]` | `charge_out_of_range` | 2 |
| `C%(123)C` | `ring_label_extended_unsupported` | 1 |
| `Cé` | `unexpected_character` | 1 |

### Examples: writing

The input is read, then written, and every row is run by the same test: the recorded text and the canonical text, or `refused:` and the code.

| Input | Recorded | Canonical |
| --- | --- | --- |
| `CCO` | `CCO` | `C(C)O` |
| `OCC` | `OCC` | `C(C)O` |
| `CC(=O)O` | `CC(=O)O` | `C(C)(=O)O` |
| `C1CC11CC1` | `C1CC11CC1` | `C12(CC2)CC1` |
| `C1=CC=CC=C1` | `C1=CC=CC=C1` | `c1ccccc1` |
| `c1ccccc1c1ccccc1` | `c1ccccc1-c1ccccc1` | `c1(ccccc1)-c1ccccc1` |
| `C1=CNC=C1` | `C1=CNC=C1` | `c1c[nH]cc1` |
| `c1cc[nH]c1` | `c1cc[nH]c1` | `c1c[nH]cc1` |
| `[Na+].[Cl-]` | `[Na+].[Cl-]` | `[Na+].[Cl-]` |
| `[Fe]` | `[Fe]` | refused: `canonical_unsupported` |
| `[CH3]` | `[CH3]` | `[CH3]` |
| `[OH]` | `[OH]` | `[OH]` |
| `[CH2]` | `[CH2]` | refused: `canonical_unsupported` |
| `C[H]` | `C[H]` | `C` |
| `N#CC(=O)O` | `N#CC(=O)O` | `C(C#N)(=O)O` |
| `[NH4+]` | `[NH4+]` | `[NH4+]` |
| `C1=CC=C2CCCCC2=C1` | `C1=CC=C2CCCCC2=C1` | refused: `canonical_unsupported` |
| `CCO.CC(=O)O` | `CCO.CC(=O)O` | `C(C)(=O)O.C(C)O` |

Not implemented, and refused rather than approximated: see the first paragraph of this section. The web panel (`apps/web/src/components/smiles-panel.tsx`) keeps line breaks in its text area, shows error positions in characters with the bytes in parentheses, and offers a result only for the form, drawing and service it was made for. P33g adds explicit text clipboard transport and replaces preview-local uncertain retry with the shared immutable import attempt. **Check SMILES import outcome** remains available after source edits invalidate the old preview and checks only the original request; a restarted service requires explicit reconciliation. See [the clipboard bridge](#explicit-smiles-text-clipboard-bridge-p33g) for its limits and recovery rules.

## SVG drawing export (P38a)

`document.export` with `format` `svg` writes one SVG drawing of a document's atoms, bonds, scene graphics, PNG images and captions in a bounded character set. This is a bounded SVG drawing export (P38a), status partial: it is export only, it draws the items listed below, and it reports what it leaves out through the loss table. The web has one control for it, the **SVG drawing export** panel (see Web panel below); `render.scene` stays `outcome: "unsupported"` (see [Operation contract 1](#operation-contract-1)), and the wider P38 item is still open. The SVG format is listed and executable only in an application built with at least one format codec attached (the API, the CLI and the conformance runner attach all of them); a bare application without codecs refuses `document.export` as not running (outcome `unsupported`, diagnostic `operation_not_implemented`), like every other export format.

**P38b1: scene graphics and images.** The drawing also includes the document's scene graphics and PNG images, in scene order with the atoms and bonds (see What is drawn below). Graphics and images are drawn, so they are no longer losses and need no `accepted_losses`. The export adds no size limit for images and refuses none, since the image admission limits already bound them. The model id moves from `chemifree-svg-1` to `chemifree-svg-2`. This is a bounded drawing export with status partial, not general SVG output; visual correctness is judged separately.

**P38b2: captions.** A caption whose every character is in the drawn set below is drawn as SVG text, in scene order with the other objects, and is no longer a loss. A caption with any character outside the set is not drawn, in whole, and stays the consent loss `caption`. An underline is not drawn: a drawn caption with an underlined run reports the consent loss `caption_underline`. The model id moves from `chemifree-svg-2` to `chemifree-svg-3`; the advance table stays `chemifree-label-advance-1`. This is bounded: the text is set by the viewer's generic font, not shaped or converted to outlines, so the glyph shapes and widths are the viewer's (see Fonts). The font is not embedded; embedding it and drawing glyph outlines are an open, unscheduled proposal (P38b3), not part of this slice.

**Request and result.** The parameters are `{ format: "svg", document_id, accepted_losses? }`. `accepted_losses` is the `loss_digest` of an earlier refusal, as under Consent below. Unknown or malformed parameters are `invalid_request`. The result is `{ content, media_type, view_box, font_table, snapshot }`: `content` is a UTF-8 SVG document, `media_type` is `image/svg+xml`, `view_box` is `{ x, y, width, height }` in pt, `font_table` is `chemifree-label-advance-1` (see Fonts below), and `snapshot` is the document, session and recovery captured with the drawing, as in `document.snapshot`. The reply's `model_id` is `chemifree-svg-3`, the version of the drawing's markup and geometry; a change to either changes it. The export captures the drawing under its session lock and renders outside that lock. It changes neither the document, its undo history nor its dirty state.

**What is drawn.** Every molecule instance on the page is drawn, with no frame around each one.

- Bonds: all seven bond orders, with wedge, hash and wavy marks. Line width comes from the document settings. A query bond is a dotted line, as on the canvas; its expression is not drawn. A bond whose two atoms share one point in a placement is not drawn there, as on the canvas.
- Atom labels follow the label rules: a bonded carbon that records nothing is a bare vertex, every other atom is labelled, and with the carbon-label setting at `all`, every carbon is labelled too. A label shows the element or query symbol, the hydrogen count (an unknown count is `?`), the charge and the isotope, in the document's label font and size.
- Drawn in a simpler form: bold, italic, underline, outline and shadow label styling is drawn as plain text, and an abbreviation or fragment display is drawn as its full graph.
- A crossed mark on a double bond is drawn as two crossing lines. A crossed mark on any other bond order, or a stereo mark that does not fit its bond order, is drawn as the plain bond order (a consent loss).
- Scene graphics, in six shapes: a line; a rectangle or an ellipse, each between two opposite stored corners; an arrow; a polygon; and a cubic spline. A graphic uses its own line width if it sets one, else the document's line width, with round caps and joins. Graphics are unfilled except the arrow head, which is filled black: an arrow head at the tip, fixed at 6 pt long and 3 pt either side of the shaft, not scaled by the line width and omitted when the arrow has no length. A spline is written as its stored start, controls and end (`M p0 C c1 c2 p3`), with no curve evaluated. A polygon runs through its stored vertices with no closing vertex added, so the SVG polygon element closes it.
- Images: each PNG is drawn in the rectangle given by its origin and its size in pt (not its pixel size), axis-aligned and not rotated, stretched to fill it (`preserveAspectRatio="none"`). Its stored base64 is written unchanged into a `data:image/png;base64` URI on `xlink:href`; the root declares the `xlink` namespace only when the drawing has an image.
- Paint order: the scene's object order. Each instance, graphic and image is painted where its object sits in `scene.objects`, so a graphic can lie over or under atoms; a group paints nothing and its members follow it in depth-first order; an instance with no scene object is painted first.
- Captions: each drawable caption is one `<text>` element at its anchor with `text-anchor` `start`, `middle` or `end` for left, centre or right alignment, with `xml:space="preserve"` so spaces are kept, and one `<tspan>` per run. A run carries `font-family` (the run's font, else the document's label font, as a generic family), `font-size`, `font-weight`, `font-style` and the fill colour. A run follows the canvas rules: a subscript or superscript is two thirds of the run size and shifted down by a third or up by five twelfths of its own size, through a relative `dy` that the next run takes back; a run without a stored colour is `#000000`. An outlined run is unfilled with a stroke of one sixteenth of its size, round joins. A shadow run is drawn as a copy one point to the right and below, at opacity 0.35, painted before the caption, in a group whose other runs are `visibility="hidden"` so the copy keeps the caption's advances. The characters drawn are the printable ASCII range U+0020–U+007E, the Latin-1 range U+00A0–U+00FF except U+00AD, the Greek capitals U+0391–U+03A1 and U+03A3–U+03A9, the Greek smalls U+03B1–U+03C9, and U+2013, U+2014, U+2026, U+2032, U+2033, U+2190, U+2192, U+21CC and U+2212. Markup characters are escaped.
- Not drawn: radical dots and attachment marks (they are left out of label boxes), stereo configurations and enhanced stereo groups, underlines of captions, a caption with a character outside the drawn set, scene groups and references, the selection and the title.
- All ink is fixed black (`#000000`) on a transparent background, with one exception: a caption run draws its stored colour (`#RRGGBB`), and black only when it stores none; an image draws its own pixels, and there is no background rectangle.

**Units and view box.** Coordinates and sizes are in pt: the root `width` and `height` are in pt, and the `viewBox` uses the same user units, so one unit is one pt. Numbers are rounded to the hundredth of a pt, and the output carries no IDs or data attributes. The view box is tight. Each drawn atom point is grown by the reach of bond ink for the drawing's line width, each label box by a 4 pt margin on every side, each graphic's ink by half its own line width on every side, and each image's rectangle with no margin, and each drawn caption's reserved box; every edge is then moved outward to the hundredth. A caption's reserved box is a model, not the ink: it spans the advances of its runs from the table, grown by the first and last glyph overhang, 1.0 em above and 0.4 em below each run's baseline, half the widest outline, 1 pt right and below for a shadow, and 2 pt of padding on every side, so a viewer's wider font can draw text past it, as with labels. The advances come from the regular upright table whatever the run's weight or style, and a character the table does not list (the space and most punctuation, Latin-1 and Greek among them) takes the fixed fallback advance of 1.1 em, so a caption of such characters reserves more width than its text needs; a drawn caption is never clipped for that reason, and a wider viewer font can still overrun the box. A graphic's ink is the box of its geometry: each line end, arrow head point, polygon vertex, rectangle corner and ellipse bounds, and all four points of a spline, control points included, so its curve lies inside the box. The view box is empty (`0 0 0 0`) only when nothing is drawn.

**Decision: atom-less documents.** A document with nothing drawn has the empty box at the origin, so its SVG is 0 by 0 pt (`viewBox="0 0 0 0"`). A document with no atoms but a graphic, an image or a drawable caption is not empty: its box covers that ink, and it has no `chemical_graph` loss. Its outcome follows the loss rule like any other drawing: `ok` when it reports no losses (an empty document, or one holding only graphics, images or drawable captions without an underline, with the default title, nothing selected and no references), `partial` otherwise.

**Fonts.** Text uses a generic family from the document's label font: `sans-serif`, `serif` or `monospace`. Fonts are not embedded and glyphs are not converted to outlines, so the viewer's installed font chooses the glyph shapes. Label widths come from the static advance table `chemifree-label-advance-1` (`font_table`), not from the viewer; a character the table does not list uses a fixed fallback advance. A viewer whose font is wider than the table can draw text beyond its reserved box, where it may overlap a bond or, at the drawing's edge, be cut off by the view box.

**Baseline.** Labels are centred with `text-anchor="middle"` and `dominant-baseline="central"`. Some non-browser viewers ignore `dominant-baseline`; in those, labels may not be centred on their atoms.

**Losses.** The loss features are the 17 below, in this fixed order; a drawing reports the ones it contains. Each has disposition `dropped`, a `source_path` that starts with `document`, and ID lists in ascending ID order. The consent column says whether the feature needs `accepted_losses`; a `no` loss still appears in `loss_report` and makes the outcome `partial`. Every drawing with an atom reports `chemical_graph`, so a plain drawing exports without consent but with outcome `partial`: the SVG is a picture, and its graph is not machine-readable.

| feature | `source_path` | `semantic_impact` | consent | when |
| --- | --- | --- | --- | --- |
| `chemical_graph` | `document` | `chemistry` | no | any atom: the graph, IDs and states are a picture, not machine-readable |
| `title` | `document.title` | `document` | no | a non-empty title other than the default |
| `selection` | `document.selection` | `document` | no | a non-empty selection |
| `scene_references` | `document.scene.references` | `document` | no | any reference |
| `scene_group` | `document.scene.objects[sN]` | `document` | no | each group, which has no ink of its own |
| `label_typography` | `document.settings` | `rendering` | yes | a label style is set (bold, italic, underline, outline or shadow) and at least one atom is labelled |
| `radical_mark` | `document.atoms[a1, a4]` | `chemistry` | yes | an atom with a radical |
| `attachment_point_mark` | `document.atoms[..]` | `chemistry` | yes | an attachment point |
| `query_label_truncated` | `document.atoms[..]` | `chemistry` | yes | a list query with more than 3 elements; the label is drawn with `…` |
| `query_bond_expression` | `document.bonds[..]` | `chemistry` | no | a query bond that states an expression: drawn as a dotted bond, which does not show the expression |
| `zero_length_bond` | `document.bonds[..]` | `chemistry` | no | a bond whose two atoms share one point in a placement; that placement does not draw it |
| `stereo_mark` | `document.bonds[..]` | `stereo` | yes | a crossed mark on a bond that is not double, or a mark that does not fit its bond order |
| `stereo_configuration` | `document.stereo` | `stereo` | yes | any stated center or double-bond configuration |
| `stereo_group` | `document.stereo.groups[..]` | `stereo` | yes | any enhanced stereo group |
| `abbreviation_display` | `document.abbreviations[inst:anchor]`, `document.fragment_displays[..]` | `rendering` | yes | each one; the full graph is drawn |
| `caption` | `document.scene.objects[sN]` | `rendering` | yes | each caption with a character outside the drawn set; none of it is drawn |
| `caption_underline` | `document.scene.objects[sN]` | `rendering` | yes | each drawn caption with an underlined run; the underline is not drawn |

**Consent.** Only a loss marked `yes` must be accepted before content is returned: the `chemical_graph` loss and the other losses marked `no` do not need `accepted_losses`, although they still appear in `loss_report` and make the outcome `partial`. Without a matching `accepted_losses`, the export is refused with `loss_consent_required`: there is no `result`, and `diagnostics[0].details` holds `features` (each name once, in first-seen order), `losses` (the consent-needing entries) and `loss_digest`, a SHA-256 over those entries and the `content` bytes. Repeat the request with `accepted_losses` set to that `loss_digest` to receive the content. If an edit has changed the output or those losses, the old digest is refused with `loss_consent_changed`, and `details` hold the current losses and digest. When no loss needs consent, `accepted_losses` is ignored. This is the digest flow of the CDX and SMILES exports.

**Import.** `document.import` with `format` `svg` is refused rather than parsed: outcome `error`, no `result`, the diagnostic `unsupported_format`, and no change to the document. SVG is written by `document.export` only.

**Errors.** The section adds no error code. It uses `invalid_request` (unknown or malformed parameters), `document_not_found` (no document with that `document_id`), `loss_consent_required` and `loss_consent_changed` (consent), and `unsupported_format` (the import refusal). Each is an operation diagnostic with outcome `error`; see [Errors](#errors).

**Not included.** Text shaping or measured glyph bounds, font embedding or glyph outlines, clipping, layers, image rotation, `render.scene`, drawing of scene groups (a loss here), captions outside the drawn character set and caption underlines (both are losses), complex text layout, and any desktop control for SVG export.

**Web panel.** The web editor's **SVG drawing export** panel (`apps/web/src/components/svg-export-panel.tsx`, next to the CDXML and CDX panel) calls `document.export` with `format` `svg`: **Preview SVG drawing** prepares the file and lists its losses; when consent is needed it lists the losses and offers **Accept these losses and preview SVG**; **Download SVG** writes `chemifree.svg` (`image/svg+xml`). It shares the consent, stale-drawing and download-recheck logic of the CDXML and CDX panel (`useConsentedExport`): a drawing or service that changed since the preview refuses the download, and a download re-exports and compares the bytes and losses first. The panel states that the SVG is a picture, not an interchange file, that fonts are not embedded and that the viewer's font chooses the glyph shapes. It shows no rendering of the SVG and no copy to the clipboard. The reader is strict for the closed result shape (`content`, `media_type`, a finite non-negative `view_box`, `font_table` and `model_id` as non-empty strings, the snapshot) and ignores unknown reply fields; it does not pin the values of `model_id` or `font_table`. The operation is reached through `POST /api/v1/operations` and `chemifree operations`; there is no CLI convenience subcommand.

## Standard InChI and InChIKey

P30 is complete for the bounded Linux generation profile below ([PR #149](https://github.com/EunhakChoi/chemifree/pull/149), merge `853bb8c391b606028d70809d0f30ca76d68e0d62`). `chem.inchi` is a read-only optional provider operation (`model_id: "chemifree-standard-inchi-1"`), not an import/export format, graph identity policy or dedicated web panel. Parameters are `{ document_id, accepted_losses? }`; unknown fields are refused. It captures the open document's graph, groups and snapshot, computes outside the session lock, and returns `snapshot`, `inchi`, `inchikey`, `aux_info`, `warnings`, `native_code`, `native_message`, `native_log`, `standard_semantics`, `provider` and `standard: true`. The provider identity includes name, registered version, checked library SHA-256 build identity and effective options. Nothing changes in the document or its history.

Both hosts take `--inchi-library` (absolute shared-library path), `--inchi-version 1.07.5` and `--inchi-sha256` (64 lowercase hexadecimal characters) together. Optional `--inchi-timeout-seconds` is an integer 1–60, default 5. Corresponding variables are `CHEMIFREE_INCHI_LIBRARY`, `CHEMIFREE_INCHI_VERSION`, `CHEMIFREE_INCHI_SHA256`, `CHEMIFREE_INCHI_TIMEOUT_SECONDS`. There is no discovery, download, bundled engine or original-application DLL fallback. The Linux-only adapter verifies a regular library file of at most 128 MiB and its digest before loading required public symbols. Registration authorizes execution of trusted native code, including library constructors: a digest proves agreement with supplied bytes, not provenance. The public API provides no runtime version query; 1.07.5 is the administrator's registered release, not a detected version. Official source: [IUPAC InChI v1.07.5](https://github.com/IUPAC-InChI/InChI/releases/tag/v1.07.5), with its [public API header](https://github.com/IUPAC-InChI/InChI/blob/v1.07.5/INCHI-1-SRC/INCHI_BASE/src/inchi_api.h). Unsafe ABI/loader code is isolated in `crates/inchi-native/src/platform.rs`; other crates retain the workspace prohibition.

No registration or a failed registration leaves the host usable and returns `unsupported`, diagnostic `inchi_unavailable`, no identifier and an available reason when known. The catalog uses existing `execution: "not_implemented"` (does not run) with `representation: "supported"` and an explicit unavailable note; a loaded provider makes it executable. No new execution-enum value is introduced.

The bounded input has 1–1,000 element atoms, at most 10,000 ordinary single/double/triple bonds, at most 20 neighbors per atom and at most 1,000 stereo elements. Query atoms, aromatic/dative/unknown bonds, attachment points, unknown hydrogen/radical states, chemistry outside `chemifree-valence-3`, unknown stereo, lone-pair tetrahedral slots and unsupported double-bond stereo are refused rather than guessed. Inferred hydrogen counts come from that valence model; explicit hydrogen counts, charge, isotope and supported radical states are mapped explicitly. Stored tetrahedral and double-bond configurations become public 0D parity records with zero coordinates; a double bond whose endpoints share the same ring reference is refused as `inchi_double_stereo_unsupported`. Drawn geometry never substitutes for stored configurations. AND/OR groups are refused whole. Drawn marks and explicit absolute-group membership/IDs are omitted with consent-required `stereo_marks` / `stereo_group` losses; individual supported stored configurations are submitted.

The sole effective native option is a finite `-W<seconds>` timeout. Calls are serialized process-wide. This native deadline is cooperative, has no host cancellation and cannot interrupt loading, a hung C call or a wait for the native-call mutex. Each returned native text field is capped at 1 MiB after the trusted SDK returns it. Native errors return no partial identifier. Native warnings retain the raw return code/message/log and create an unknown-impact, consent-required `native_warning` loss: the adapter does not infer from English messages which chemistry or stereo survived. It does not claim that submitted stereo necessarily appears in the identifier.

Consent follows `loss_consent_required` → repeat with `accepted_losses: <loss_digest>`; changed context gives `loss_consent_changed`. Refusal details show the snapshot/provider, required losses, raw warning code/message/log and warnings before acceptance. The digest binds required losses, model/profile identity, provider/build/options, snapshot, InChI, InChIKey, AuxInfo, native code/message and warnings. The raw native log is diagnostic-only and excluded because timing may vary. Standard InChI normalization is informational and needs no consent: tautomer, protonation and representation distinctions may merge, without claiming that a particular input changed. No equivalence to canonical SMILES, Chemifree graph identity or complete stereo round-trip is implied. A reply with losses or warnings is `partial`.

Verification is bounded evidence. Native boundary tests passed 10/10; six direct native outputs and four application/provider outputs (two tetrahedral, two alkene) matched exact InChI/InChIKey from a separately executed official 1.07.5 executable. Both use the same official distribution: this checks adapter mapping, not agreement between independent chemical engines. Application consent tests passed 6/6, CLI configuration integration 7/7, and API/CLI configuration modules 4/4 each. Full local verification and browser smoke passed for the delivered head; the [PR #149 review record](https://github.com/EunhakChoi/chemifree/pull/149) records the Codex shared-ring-reference finding and independent subagent FIFO-startup finding, both fixed with regression checks. [Post-merge main CI](https://github.com/EunhakChoi/chemifree/actions/runs/42081222618) passed both check and browser jobs. Opt-in comparisons live in `crates/inchi-native/tests/official_reference.rs` and `crates/providers/tests/inchi_official_operations.rs`; run their ignored tests with `CHEMIFREE_TEST_INCHI_LIBRARY`, `CHEMIFREE_TEST_INCHI_SHA256` and `CHEMIFREE_TEST_INCHI_EXECUTABLE` all explicitly set. These official comparisons were explicitly run locally; ordinary CI does not execute the opt-in official comparisons. An ignored or unconfigured test is not executed evidence. This closes the bounded adapter milestone. InChI import, a dedicated web panel, other platforms, broader chemistry and native process isolation remain unimplemented; ordinary CI has no official-library comparison job. No general scientific conformance claim follows.

## Formula and mass properties

`chem.properties` takes `{ "document_id", "scope": "molecule" | "selection" | "components", "property": "formula" | "masses" | "distribution", "selection"?: Selection, "data_registration_id"?: string, "time_limit_ms"?: integer }` (`time_limit_ms` is the limit of [computation time limit and cancel](#computation-time-limit-and-cancel)). Selection is required only for selection scope and is the client's current atom/bond/object selection, not a stale history selection. Selected bonds add both endpoints; a selected molecule instance or its enclosing group adds the shared graph once; graphics and scene references contribute no atoms. Components follow graph connectivity. Non-vertex inferred hydrogen counts are evaluated on the entire graph before restricting to the selected atoms. Query atoms, undetermined hydrogens and attachment points are refused. Explicit non-vertex H and explicit H vertices are each counted once; valence warnings remain visible for recorded states whose count is known but whose valence is not established.

The reply includes the exact analyzed document snapshot, atom IDs per component, Hill formula, formal charge, `chemifree-valence-3`, `chemifree-properties-1` and the arithmetic policy. Formula uses integer composition and requires neither mass DATA nor the mass preference; its numeric `display_decimals: 0` is a compatibility sentinel and no display precision is applied. Mass and isotope distribution require a readable mass display preference and a checked compatible external registration. Mass results capture the display decimals and identify the registration/generation, dataset/version, profile, manifest digest, charge coefficient and calculation policy. Unsupported chemistry, unavailable required data, exceeded limits or underflow return no incomplete numeric result and never alter the document or history. Conditional calculations explicitly report their missing isotope abundance data.

The operation catalog always lists formula as executable. It lists mass or isotope distribution as executable only when at least one loaded generation can calculate that operation for a natural-abundance or document-representable explicit isotope composition. An explicitly labeled isotope (mass number 1–999) can produce a single-peak distribution without natural-abundance data; natural distributions need suitable abundance fractions. Otherwise those sub-operations say `requires_data`. These statuses describe whether at least one calculation is possible, not coverage for every element or molecule. If several recognized mass-input or NIST registrations exist, including unusable ones, callers must select one explicitly; an invalid selection or an unreadable preference remains a per-request error. Unrelated registrations do not cause ambiguity. Registry generations are fixed for an application instance, so readiness is evaluated when the checked registry is attached. Plain-text CLI capabilities also list the nested sub-operations and their status.

Masses and isotope peaks are in unified atomic mass units (`u`); positive charge changes mass by the selected signed coefficient per elementary charge. The independent mass-input profile uses its pinned natural weights, isotope selection and signed correction, including a visibly reported natural-weight fallback when there are no isotope rows. Its calculation binding accepts mass and natural-weight labels `u` or `Da`, abundance `%` or `percent`, and signed charge coefficient `u/e` or `Da/e`; other units are refused. Its source schema ID is reported as opaque provenance, while the catalog-bound structural profile supplies the field meanings. No actual personal DATA generation has been validated against these supported labels, and this profile does not claim universal or bit-for-bit legacy equivalence. The separate NIST representative profile uses [reported isotope fractions and masses](https://physics.nist.gov/PhysRefData/Compositions/notes.html), selects only a unique maximum among reported fractions whose sum is within 0.1% of one, and reports omitted unknown isotope abundances. Its distribution is conditional on the reported fractions; raw sums are returned before normalization. Its average uses those fractions only when their sum is within 0.1% of one. Standard-weight scalars and intervals are retained but not used as a fallback because the input's free-form kind vocabulary is not pinned by the public structural contract. Its positive-charge convention subtracts the [2022 CODATA electron mass](https://physics.nist.gov/cgi-bin/cuu/Value?meu); binding-energy differences and source uncertainties are not propagated. Display strings use the captured 0–8-place preference and the P15 fixed-format ties-to-even rule; the binary64 values are not rounded to the display setting.

## Ring and aromaticity interpretation

`chem.aromaticity` takes `{ "document_id", "time_limit_ms"? }` (`time_limit_ms` is the limit of [computation time limit and cancel](#computation-time-limit-and-cancel)). It captures the recorded graph and its document snapshot together, without editing, then returns cyclic biconnected edge blocks. A spiro junction alone does not join blocks; fused and bridged cycles share one. `cycle_rank` is the cycle-space dimension, not a unique count of chemical rings, and all recorded bond kinds participate in this topological step. No smallest ring basis is claimed.

The interpretation is `chemifree-aromaticity-2`, a bounded policy rather than a universal definition of aromaticity. It never reports `non_aromatic` for a block it cannot decide; those are `unsupported` with a reason. Its rules, stated once:

- **Atom role (one rule).** Only these ring atoms are supported, each with exactly one total valence (bond orders plus hydrogens), the same for Kekulé and aromatic spellings: neutral C (4), C+ and C- (3), neutral N (3), N+ (4), N- (2), and neutral O and S (2). Any other element or charge is `unsupported` (`element_or_charge`). A radical or carbene on any ring atom, saturated or not, is `unsupported` (`radical_state`). An atom with a different total, such as a hypervalent N (5) or S (4 or 6), an overvalent saturated carbon, or a stated hydrogen count that does not fit its bonds, is `unsupported` (`valence_not_in_model`); such an atom is never peeled away or accepted because the valence model alone would call its valence valid. Query atoms, attachment points and unknown hydrogens are likewise `unsupported`. The valence model supplies only the hydrogen count of an atom whose bonds are all integral. A ring atom is then either saturated (outside the conjugated system) or contributes π electrons. A neutral carbon contributes one electron and needs a double bond if it has a ring double or aromatic bond, and is saturated otherwise. A cation or anion carbon, an anionic nitrogen, and neutral O or S contribute 0, 2, 2 and 2 electrons. A charged nitrogen with a ring double or aromatic bond contributes one (saturated without one). A neutral nitrogen is pyrrole-type (two electrons, no double bond) when it carries hydrogen or has three sigma bonds, otherwise pyridine-type (one electron, needs a double bond). Its hydrogen count is the stated count, or, when the hydrogen state is `inferred` and every bond has an integral order, the count the valence model infers from its bonds; a Kekulé ring nitrogen with two single bonds is therefore pyrrole-type. An aromatic bond has no integral order, so an aromatic nitrogen without a stated hydrogen and with fewer than three sigma bonds is pyridine-type. This is deliberate and is the only asymmetry between spellings: a pyrrole with no stated N-H is aromatic when drawn Kekulé (the valence model supplies the hydrogen) but `non_aromatic` (`electron_count`) when drawn with aromatic bonds, and its chemical identity is then refused (`aromaticity_unsupported`). Aromatic bonds imported from a file therefore need an explicit hydrogen count on a pyrrole-type nitrogen (`[nH]`); V2000 records carry no such count, which is why the reader accepts no aromatic bond type. A double bond on a donor atom is `unsupported` (`valence_not_in_model`). One state is not refused for its valence: a neutral nitrogen with two single bonds (no double or aromatic bond) and a stated hydrogen count of 0 (total valence 2) takes no part in conjugation, so it counts as a saturated atom and its block follows "Block with saturated atoms" below: `non_aromatic` (`not_conjugated`) when no conjugated cycle remains, as in `chemifree-aromaticity-1`; its chemical identity is still refused (`unusual_valence`) because the valence model does not know the state.
- **Conjugated circuit.** A ring can be aromatic only if it has a circuit of contributing atoms through at least one double or aromatic ring bond. A ring made only of donors joined by single bonds (cyclotriazane, an O/S ring) is `non_aromatic` (`not_conjugated`). Two adjacent donors (for example N-N or N-S) are decided after the electron count and the double-bond pattern, because only a count that would make the ring aromatic depends on their lone-pair arrangement, which the model does not decide: a single circuit whose count (every donor counted as two electrons) is not 4n+2 is `non_aromatic` (`electron_count`, for example 1,2-dithiin with 8 electrons), a Kekulé double-bond pattern that cannot alternate is `non_aromatic` (`not_conjugated`), and a single circuit whose count is 4n+2, or a fused block, is `unsupported` (`adjacent_donors`).
- **Fully conjugated block.** When every ring atom contributes: a single circuit requires a conjugated assignment and a 4n+2 electron count (`non_aromatic`, `electron_count` otherwise; `not_conjugated` for an impossible double-bond pattern). A fused block is recognized only for neutral carbocyclic systems where each bond's shortest alternate path forms a six-membered ring; any other fused conjugated block is `unsupported` (`fused_system_outside_model`). Ring bonds that mix aromatic and non-aromatic orders are `unsupported` (`mixed_ring_bonds`). For a block in which every atom contributes, the checks run in this order and the first that applies decides: no π bond (`non_aromatic`, `not_conjugated`), mixed ring bonds (`mixed_ring_bonds`), a Kekulé double-bond pattern that cannot alternate (`non_aromatic`, `not_conjugated`), then for a fused block adjacent donors (`adjacent_donors`) before the fused-system rule (`fused_system_outside_model`), and for a single circuit the electron count (`non_aromatic`, `electron_count`) before adjacent donors (`unsupported`, `adjacent_donors`). Atom roles are assessed before any of these (see "Atom role"), so a ring that mixes bond orders and also holds an atom the model cannot read reports that atom's reason rather than `mixed_ring_bonds` (`chemifree-aromaticity-1` reported `mixed_ring_bonds` first). That order is kept because an aromatic spelling of a benzene fused to a saturated ring must report `fused_partially_aromatic`, like its Kekulé spelling, and that needs the roles first.
- **Block with saturated atoms.** If no cycle of contributing atoms passes through a double or aromatic ring bond, no cycle of the block could be aromatic and it is `non_aromatic` (`not_conjugated`), for example cyclohexene, norbornene, α-pinene, or a ring of amine N next to a CH2 ring (if an aromatic bond is recorded in such a block it is `unsupported`, `mixed_ring_bonds`, because the block mixes aromatic and saturated bonds). If such a cycle exists but the block also has saturated atoms (tetralin, indane, indene, 1,3-benzodioxole, indoline, fluorene), the model does not classify the aromatic sub-ring and reports `unsupported` (`fused_partially_aromatic`) rather than a non-aromatic fact. A neutral two-coordinate nitrogen with a stated hydrogen count of 0 and only single bonds counts as a saturated atom here, so an indoline-like block with such a nitrogen is `fused_partially_aromatic` too. A Kekulé and an aromatic spelling of the same molecule get the same classification, apart from the stated-hydrogen asymmetry above.
- **Limits.** One system has at most 128 atoms (`system_limit`). Peeling is linear and has no budget. Only a system with recorded aromatic bonds needs a Kekulé assignment, found by a matching search with a fixed 100,000-step budget; if that is exhausted the system is `unsupported` (`search_limit`), never `no_kekule_assignment`, which is reported only when the search finished without finding an assignment. The budget is reachable within the 128-atom cap: the step count depends on the order the atoms were drawn in, and a 93-atom, 121-bond system (29 independent rings) drawn in aromatic spelling takes 371,242 steps in the numbering the tests use; its odd atom count means it has no assignment, so a larger budget would end in `no_kekule_assignment`. A Kekulé-spelled system is never searched, so it cannot reach `search_limit`. Aromatic input bonds and the computed classification remain separate in the snapshot and result. `invalid_graph` is an internal-defect reason: it means perception named a bond or atom that the graph does not hold (or a ring bond with an end outside its ring). It cannot be reached through the public API with a consistent document; the conversion step and the ring-bond check named above report it instead of panicking (other lookups in perception are not covered by this), as `unsupported` in an inspection or identity and as an `invalid_command` refusal with this reason in a conversion (message: an internal inconsistency, nothing was changed), and it indicates a defect to report.

**Model ids and refusals.** Under one model id (`chemifree-aromaticity-2`, with `chemifree-chemical-graph-2` for chemical identity), every classification, identity and conversion that succeeds is identical in every build. A refusal (`unsupported`, or an identity or conversion refused for such a reason) may later become a result without a new id, so a stored refusal is valid only for the build that produced it. A change to anything that succeeded needs a new id. For example, 1,2-dithiin was refused (`adjacent_donors`) in the first builds of revision 2 and is now `non_aromatic` (`electron_count`) under the same ids.

The policy uses the [IUPAC Hückel rule](https://goldbook.iupac.org/terms/view/H02867) as a single-circuit reference and the [OpenSMILES discussion](https://opensmiles.org/opensmiles.html) for the need to check electron assignments, charges, hydrogens and external bonds together. A drawing does not establish planarity or an experimental aromaticity criterion; the classification is this implementation's graph model, not a physical measurement or a claim to reproduce another model.

An explicit `chem.aromaticity_convert` request converts supported systems to aromatic bonds or assigns every recorded aromatic bond single or double. Conversion is all or nothing for the document. Aromatic conversion refuses if any cyclic system anywhere in the document is unsupported (in `chemifree-aromaticity-1` a benzene next to a separate tetralin converted the benzene; in revision 2 the whole request is refused with `fused_partially_aromatic`). Kekulé conversion refuses any aromatic bond that cannot receive a complete assignment, including one outside a ring. A refusal reports `invalid_command` with the model reason and changes no atom, bond, selection, revision or history. The wire code stays `invalid_command` for these refusals (decided in #104; an `unsupported` code was not added). `chem.aromaticity_convert` reports it as an operation diagnostic whose `details.reason` names the refusal (for example `fused_partially_aromatic` and `aromatic_bond_outside_ring`, as pinned by `crates/application/tests/aromaticity_operations.rs`); other refused commands, including the stereo refusals above, also carry a `reason`, so a client reads the reason together with the operation that answered, not the code alone. A conversion that would change the order of a bond that holds a stereo mark or a stated configuration, or of a bond that would then break a stated configuration, is refused whole with the stereo `reason` (see [Stereo marks and configurations](#stereo-marks-and-configurations)) and no `command_index`: the conversion's commands are generated, so an index into them would name nothing the caller sent. Only the stereo refusals leave the index out; every other refusal of a conversion reports what it reported before the stereo commands existed. Remove the mark or configuration first, in an earlier transaction.

Conversion also keeps each nitrogen's meaning. Converting to aromatic states the hydrogen (`specified`, 1) of every Kekulé pyrrole-type nitrogen whose hydrogen state was `inferred`; otherwise the aromatic spelling would be read as pyridine-type. Converting to Kekulé returns a neutral ring nitrogen's stated hydrogen (count above zero) to `inferred` when it equals the count the valence model infers for the resulting Kekulé bonds, which is exactly the statement conversion to aromatic would restore. It keeps any other stated count, including a charged nitrogen's (`[nH+]`) and a stated zero. A Kekulé pyrrole converted to aromatic and back is therefore unchanged and keeps the same chemical identity. The document does not record who stated a count, so a count the user stated that equals the inferred count is reset like one the conversion stated; that is an atom edit the user did not ask for, and it does not change the chemical identity.

A successful conversion uses one ordinary edit and can be undone. It is subject to the document transaction bound: at most 500 commands, counting the changed bonds plus the nitrogen hydrogen updates (80 Kekulé pyrroles are 80 x (5 + 1) = 480 commands). A larger conversion is refused as a whole with `invalid_command` and the reason `conversion_command_limit` and a message that names the cause (the conversion needs more than 500 commands and a transaction accepts at most 500), not the generic transaction message, and changes nothing; the bound is not split. An already-converted graph is a no-op. The request's revision, conversion, selection and request ID participate in the usual conflict and replay rules. The web keeps inspection results tied to the returned graph and service generation, and requires inspection again after either changes.

### Computation time limit and cancel

`chem.aromaticity_convert` is the only operation that edits a document after a computation, and the revision and replay rules below are its own. `chem.cip`, `chem.aromaticity`, `chem.identity`, `chem.substructure` and `chem.properties` are read-only and share this time limit and cancel (see their bullets at the end of this section); only `chem.aromaticity_convert` and `chem.cip` take a concurrency slot (see Concurrency). They keep their work limits and compute outside the session lock. SMILES export (`document.export` with `format: "smiles"`, both styles) shares the limit and cancel too and takes no slot. The remaining read-only operations have no time limit or cancel yet: `chem.stereo` and `chem.inchi`. `chem.validate` computes under the session lock (its work is linear in the document).

- **Outside the lock.** The conversion is computed on a copy of the graph while no lock is held. Other requests, including edits to the same document, run meanwhile. The result is applied only if the document is still at `expected_revision` when the computation ends; otherwise nothing is applied and the reply is `revision_conflict` with `current_revision`, exactly as for any stale edit. The server never recomputes against the newer revision on its own; the client asks again with the new revision. A document closed and reopened in the meantime does not make the result current: a discarded revision number is never reused.
- **Time limit.** The computation is stopped when its time limit passes. The limit is the request's `time_limit_ms` (an integer; `0` is allowed and is always exceeded), or the server's default of 5 seconds when the request sets none. A request may set at most the server's maximum of 8 seconds, below the web client's 10-second deadline; more is `invalid_request` (`path: "parameters.time_limit_ms"`, `details.maximum_ms`), and that reply names `model_id: "chemifree-aromaticity-2"`, like the refusals that come after the parameters are read (parameters that cannot be read at all, such as an unknown field, name no model). A request may set any value up to the maximum, including one above the server's default. A stopped computation is `outcome: "error"` with the diagnostic `time_limit` and `details: { limit_ms }`; no elapsed time is reported. `chem.cip` has the same limit, default and maximum: its refusal above the maximum names `model_id: "chemifree-cip-1"`, and its limit is checked before its document is read. The four read-only inspections have the same limit, default and maximum, checked first as well, before any document is read or any other parameter is judged: the refusal above the maximum names `model_id: "chemifree-aromaticity-2"` (`chem.aromaticity`), `"chemifree-recorded-graph-1"` or `"chemifree-chemical-graph-2"` by the request's `mode` (`chem.identity`), `"chemifree-substructure-1"` (`chem.substructure`) and `"chemifree-properties-1"` (`chem.properties`). Both values are server settings (`ComputeLimits`). A host that configures them cannot lift the bound: a value above 1 hour (3,600,000 ms) is lowered to 1 hour (and a default above the maximum to the maximum) when the settings are applied, so no setting means "no deadline".
- **Concurrency.** At most N computations (`chem.aromaticity_convert` and `chem.cip`) run at the same time; N is the smaller of the machine's cores and 4 by default, and a host setting (`Application::with_compute_concurrency`) is clamped to 1 through 64. A request that finds every slot taken waits for one, and the wait counts against its time limit: if the limit passes first, the reply is `time_limit` with `details: { limit_ms }`, the same as a computation stopped by its limit (no new code or field; the reply does not say whether the wait or the computation used the time). Replays, `request_in_progress` and every refusal that is decided before the computation start do not wait. A waiting request that is cancelled leaves at once and takes no slot; waiting requests are not served first-come-first-served. Results below the limit do not change: a computation that finds a free slot is not asked anything more than before. Limits of the wait: the number of waiting requests is not bounded (each holds a worker thread and, for a conversion, its own copy of the graph until its limit passes); a conversion whose document was edited during the wait still computes once it has a slot and then replies `revision_conflict`; and `chem.cip` takes a slot even when it has nothing to label, as it checks its limit then too. `chem.aromaticity`, `chem.identity`, `chem.substructure`, `chem.properties` and SMILES export take no slot: they are not counted against N, never wait for one, and are never answered `time_limit` because the slots are busy (decided on 2026-11-11: a quick inspection must not wait behind long conversions; the web now shows a `time_limit`, so giving them a slot later is a possible tightening that needs its own decision). The work limits above (128 atoms per system, 100,000 search steps, 500 commands) are deterministic. Whether an input finishes inside a time limit depends on the machine, its load and how many computations run at the same time, so the same request can succeed on one machine and be answered `time_limit` on another. A `time_limit` reply says nothing about the model's support for the input; ask again with a larger limit, up to the maximum.
- **Cancel.** There is no operation that cancels a running request. A computation is cancelled when the host running the application stops it (`Application::execute_with`) and then ends with the diagnostic `cancelled`. Over HTTP, closing the connection of a running `POST /api/v1/operations` request does that: the computation stops at its next checkpoint (see Checkpoints below) and changes nothing; nobody is left to receive the `cancelled` reply. When the host stops (`Application::cancel_computations`, which the API calls when it receives its shutdown signal), every running and waiting computation ends with `cancelled` and changes nothing, and new computations are refused with `cancelled` while other operations keep working; over HTTP that reply is delivered, because its client is still connected. The CLI reads one request at a time from standard input and does not cancel: its only stop is the time limit, and ending the process ends the computation with it.
- **Checkpoints.** A stop (time limit or cancel) takes effect at the next checkpoint, not at once. The computation asks at its entry, at the start of the interpretation of every ring system, once per bond while a fused system's six-membered rings are chosen, once per 1,024 steps of the Kekulé search (a search of the full 100,000-step budget asks about 98 times; the stride affects when a stop is noticed, never the result), and once more under the session lock immediately before the result is applied. It has no checkpoint inside the copy of the graph, ring perception of the whole graph, the work of one system between two of the questions above, or the final turning of the interpretation into commands. These stretches are bounded by the document caps (2,000 atoms, 4,000 bonds) and the 128-atom system limit, so a stop waits for at most one of them. Measured once in a debug build on the largest allowed documents (333 separate benzenes, 40 x 50 and ring-and-chord graphs of 2,000 atoms, 15 fused systems of 126 atoms), the longest stretch between two questions was about 16 ms; that is a measurement on one machine, not a guarantee.
- **A stop is honoured until the result is committed.** The last question is asked under the session lock immediately before the conversion is applied. If it reports a stop, the reply is `time_limit` or `cancelled` and nothing changes. Once the edit is committed under the lock it stands, even if the client disconnects or the limit passes afterwards; a reply nobody receives is the ordinary lost-reply case (repeat the request with the same `request_id`).
- **Nothing partial.** A computation either finishes with exactly the result it would have without a limit, or is stopped and changes nothing: no atom, bond, selection, revision, history entry, dirty state or draft. A stopped request is not remembered for replay, so repeating its `request_id` computes it again, and it may ask for another `time_limit_ms` (a retry after a `time_limit` with a larger limit is the normal way on). A request that completed is different: the requested `time_limit_ms` (or its absence) is part of what the replay compares, so the same `request_id` with another `time_limit_ms` after a completed request is `request_id_reused`, like any other changed parameter. While a request with the same `request_id` computes, a second one is answered `request_in_progress`.
- **Mixed stops.** A model refusal (`invalid_command` with a reason) and a stop (`time_limit`, `cancelled`) are different: the refusal is the same on every machine, the stop is not. Which of the two a request meets first depends on where in the computation the stop falls.
- **`chem.cip`.** Its time limit is the one above, for the whole request: the scan, the centers and the bonds share one deadline. A host cancel wins over an expired deadline, because the host's interrupt is asked before the deadline. `chem.cip` commits nothing and its result is never replayed, so the stop rule for committed edits and the revision and replay rules do not apply to it. Its deterministic refusal is `resource_limit` (the node budget), which is the same on every machine.
- **Checkpoints of `chem.cip`.** The computation asks at the start of every sphere of every comparison that a ranking makes (the centers and double bonds it labels, and the scanned units that are ranked), and in the scan it asks once for every atom and every bond, skipped or not. The longest stretch between two of these questions has not been measured, so a stop can wait for an unknown stretch of work.
- **The four read-only inspections** (`chem.aromaticity`, `chem.identity`, `chem.substructure`, `chem.properties`). Each takes `time_limit_ms` and the order is: read the parameters and the limit, capture the snapshot under the session lock, release it, ask once, compute outside the lock. They take no compute slot (see Concurrency). One deadline covers the whole request (`chem.identity` with `other_document_id` and the two documents of `chem.substructure` share it). A stop is `outcome: "error"` with `result: null`, the model's `model_id`, the revision of the snapshot that was analyzed (`chem.identity`: the source document's; `chem.substructure`: the query's), and one diagnostic: `time_limit` with `details: { limit_ms }` or `cancelled`. It carries no snapshots and no partial result. A request that finishes below the limit returns exactly the reply it returned before the limit existed (the work limits, the `steps` of `chem.substructure` and every refusal are unchanged: the checkpoints count nothing). Like `chem.cip`, they commit nothing and are not replayed; a stop is the machine-dependent kind of outcome described under Mixed stops, and a model refusal (`unsupported`, `resource_limit`) is not.
- **Checkpoints of the four read-only inspections.** `chem.aromaticity` asks as `chem.aromaticity_convert` does for its interpretation (the entry, every ring system, every bond while a fused system's six-membered rings are chosen, every 1,024 Kekulé steps). `chem.identity` asks once per round of refinement, at the entry of every search state (at most 100,000) and during the chemical mode's aromaticity interpretation. `chem.substructure` asks once after the query is read, during the target's aromaticity interpretation and once per 1,024 scanned candidates. `chem.properties` asks for every component and once per 1,024 work units of an isotope distribution; its formula and mass paths have no checkpoint inside a component. Not measured and without a checkpoint: copying the graph, ring perception of a whole target, building the identity graph, the candidate prefilter of `chem.substructure`, valence inspection, and up to 1,024 scanned candidates or distribution work units between two questions. A stop can therefore wait for such a stretch; their bound is the document caps, no latency is promised.
- **SMILES export** (`document.export` with `format: "smiles"`, recorded and canonical). It follows the bullet above, with the time-limit refusal naming the style's model (`chemifree-smiles-1` or `chemifree-smiles-canonical-1`) and the order of the SMILES section; it takes no compute slot, and `Application::cancel_computations` and a host cancel stop it. It asks once before the write, and then: for recorded output, in the aromaticity perception of a drawing with aromatic bonds, at each render pass and before each read-back parse (at most 2 passes per render, up to 3 renders), at each stored stereo record, before directional-bond placement and before the final stereo read-back; for canonical output, in the identity search of the drawing (perception and search) and of the text read back, and at the render passes. A stop is never a refusal: `readback_mismatch`, `canonical_limit` and the other codes of the SMILES table are the same on every machine and are not produced by a stop. Not measured and without a checkpoint: the document snapshot, drawing-loss collection and attachment stripping, input validation and atom planning, the text emission of one pass, each read-back parse by the SMILES reader (its ring and aromaticity perception included: the reader is shared with import, which has no time limit, and an export parses up to 7 times), directional-bond placement, the read-back comparisons, CXSMILES group assembly and the reply assembly. They are bounded by the 500-command and 16 KiB limits and by the aromaticity step limit; a stop can wait for one of them and no latency is promised. The checkpoints count nothing, so below the limit the reply is exactly the reply it was.

## Graph identity and symmetry

`chem.identity` takes `{ "document_id", "mode", "other_document_id"?, "time_limit_ms"? }` (`time_limit_ms` is the limit of [computation time limit and cancel](#computation-time-limit-and-cancel)). It examines one graph and optionally compares a second document. Both graph snapshots and their document views are captured under one session lock; inspection does not edit either document. External writers are not locked across the two reads, so the pair names what was actually compared rather than a cross-process storage transaction. An open document uses its working copy; a closed one is read from storage without opening a session or evicting another. Its snapshot revision is reserved in the process's usual retired numbering, so a later different commit cannot reuse that number. An abandoned recovery draft is reported in its snapshot but is not included in the comparison. The result contains the exact source snapshot, canonical code, a canonical-number-to-local-atom-ID list, symmetry groups, folded hydrogen IDs, and, when requested, the second snapshot and its corresponding identity and equivalence result. Document and atom IDs, coordinates, scene objects and insertion order are not chemical labels. The web identifies the comparison snapshot and revision explicitly; a later edit to the other document requires another inspection.

When an interpretation is unsupported or exceeds a limit, its diagnostic names the failing side and carries that side's actual captured snapshot; the operation result remains null. The web labels the failure with that snapshot and clears it when the document list is refreshed, so an old comparison error is not presented as a finding about a later target.

The required `mode: "recorded"` selects `chemifree-recorded-graph-1`: labeled graph isomorphism preserves element and query expression, query-list order, isotope, formal charge, radical, recorded hydrogen mode/count, attachment points, bond order and dative direction. Recorded aromatic and Kekulé bonds, and H vertices versus non-vertex hydrogen counts, remain different representations. `mode: "chemical"` selects `chemifree-chemical-graph-2`: it uses the named P17 aromaticity and P16 valence policies, compares supported aromatic and Kekulé forms alike, and folds only neutral, nonisotopic, nonradical terminal H vertices attached by a single bond to a heavy atom. Isotopic or charged H remains distinct. Unknown or unsupported chemistry is refused instead of being classified as unequal; an aromaticity refusal carries the aromaticity model's own reason (for example `fused_partially_aromatic`) or `aromaticity_unsupported`, never a bond or valence reason. Revision 2 changed results: a Kekulé ring nitrogen with inferred hydrogen is now compared as the aromatic heterocycle, so a Kekulé pyrrole matches its aromatic spelling with a stated `[nH]`, and a Kekulé benzene fused to a saturated ring is refused instead of being compared as non-aromatic. This is a bounded model, not a universal statement of chemical equivalence; stereochemical distinctions will require their own representation before this model can compare them.

**Stereo.** Neither identity model reads stereo: `equivalent` means the graph axis only, and the canonical code, symmetry groups and mappings are stereo-blind, so mirror images and cis/trans forms of one graph are `equivalent` and have the same code. Enhanced stereo groups are not read either: two documents that differ only in their groups have the same canonical code and are `equivalent`. The result carries `stereo: { compared, source_present, other_present }`. `source_present` and `other_present` are facts about the compared documents: whether each holds any bond mark or stated configuration (`other_present` is null when no other document was given). `compared` is a property of the model, not of the pair, and is `false` in every reply, including for pairs that hold no stereo, because the model never reads it. When a compared document holds stereo the outcome is `partial` and the loss report has one entry per such side: feature `stereo`, `source_path` `snapshot` (the source) or `comparison.snapshot` (the other document), disposition `unknown`, `semantic_impact` `stereo`, consent not required. When neither holds stereo the outcome is `ok` with no loss, as before. A client that ignores `partial` would read a graph-only answer as a statement about configuration; the web identity panel shows the notice with the answer. `chem.stereo` compares a stored record with the drawing, but this operation still does not read stereo and does not use that comparison. The field was added within v1.

Canonical codes are stable within the named model, not SMILES or document IDs. A symmetric graph cannot assign a uniquely meaningful number to each atom inside an automorphism orbit: numbers may be exchanged within a reported symmetry group. In chemical mode, numbering excludes folded H vertices, which remain listed in `folded_hydrogens` and in the expanded symmetry groups. The search accepts at most 128 atoms and is bounded by 100,000 search states and 10,000,000 work units. Exceeding either bound returns `identity_limit`; unsupported chemistry returns `identity_unsupported`, with a source/target side and reason. Neither error returns a partial code or an equivalence conclusion.

## Substructure matching

`chem.substructure` takes `{ "query_document_id", "target_document_id", "time_limit_ms"? }`, the two documents required and no other parameter (`time_limit_ms` is the limit of [computation time limit and cancel](#computation-time-limit-and-cancel); a stop of that limit is `time_limit` or `cancelled`, shared stop codes that are not rows of the code table below), and finds every way the query drawing lies in the target drawing. The two may name one document, which is then searched in itself. It is read-only: it changes neither document, any revision, selection or history, and a repeated `request_id` is not replayed but searches the then-current drawings, as for the other inspections. A closed saved document is read from storage without opening a session. Nothing about the search is recorded in the drawing. HTTP `POST /api/v1/operations`, `chemifree operations` and the web "Substructure search" panel give the same replies from the same `Application::execute`.

The model is `chemifree-substructure-1`. It owns the matching rules below and uses `chemifree-aromaticity-2` to decide which bonds are aromatic. It is not RDKit's or any other toolkit's definition, and no equivalence is claimed beyond the fixtures of `crates/application/tests/data/query-fixtures.json`. Those fixtures are checked by the test suite against this service, and compared with an RDKit 2022.03.2 Python by `scripts/verify-query-rdkit.py`, which is run by hand and is not part of `npm run check`. Run it with the RDKit 2022.03.2 Python and a built CLI (`python -I scripts/verify-query-rdkit.py target/debug/chemifree`) whenever a fixture, the matcher's rules or the aromaticity model change; no CI job runs it, so a change to any of them is not compared with RDKit until someone does. Where the aromaticity model differs from RDKit's, the matcher follows the model or refuses.

### Reply

`result` is `{ query, target, aromaticity_model, limits, steps, mappings, stereo }`. `query` and `target` are the exact snapshots (`document`, `session`, `recovery`) of the two documents, captured under one lock before the search runs; they are one snapshot when a drawing is searched in itself. `model_id` is `chemifree-substructure-1`, `aromaticity_model` is `chemifree-aromaticity-2`, and `document_revision` is the query snapshot's revision, in a refusal or limit as in a result. A reply that has no snapshots because a document could not be read (`document_not_found` and the other document errors) has `document_revision: null`: it names no revision, in particular not that of whichever of the two documents failed. A restart can reuse a revision for a different graph, so a client compares the whole returned snapshot with the drawing it shows, as the web does, and treats a reply for a different snapshot as not about that drawing. `limits` is `{ query_atoms: 64, search_steps: 2000000, scanned_candidates: 20000000, mappings: 1000 }` and `steps` is how many assignments this search tried (a step is defined under Limits).

`mappings` is every mapping, ordered by the target atoms assigned in query-atom order. A mapping is `{ atoms: [{ query, target }], bonds: [{ query, target }] }`: `atoms` has one pair for every query atom, ordered by query atom ID, and `bonds` has one pair for every query bond, ordered by query bond ID. The IDs are each document's own (`a1` and `b1` exist in both), so a pair always names the query side and the target side by key. An empty list is a complete answer: no assignment of the query atoms to target atoms exists. It does not say the target could be read in full: when some query atom has no candidate at all (an element the target lacks), the search ends before the target's rings are read, so a target that is refused for one query gets a complete empty answer for another (`bromine-not-in-tetralin`, against `benzene-kekule-in-tetralin`). No mapping can exist in either case. When the search cannot finish or the drawings cannot be read, there is no `result` and no partial list: the reply is a refusal or a limit.

### Stereo

The matcher does not read stereo: a mapping says nothing about wedge or hash marks, stated configurations or double-bond geometry, so a query drawn with one configuration matches the mirror image and the other double-bond form of the same graph. `stereo` is `{ compared, query_present, target_present }`: the `*_present` fields are facts about the two documents (any bond mark or stated configuration), and `compared` is a property of the model, `false` in every reply because the model never reads stereo, including for pairs that hold none. When either document holds stereo the outcome is `partial` and the loss report has one entry for each such side: feature `stereo`, `source_path` `query` or `target`, disposition `unknown`, `semantic_impact` `stereo`, consent not required; the mappings are still returned, complete for the graph axis. When neither does the outcome is `ok` with no loss. A refusal or limit has no `stereo` object, as it has no result. `chem.stereo` compares a stored record with the drawing, but this operation still does not read stereo. The field was added within v1.

### What a mapping is

A mapping assigns every query atom a different target atom so that each query bond lies on a target bond between the assigned atoms. The target may have more bonds between those atoms (it is not an induced subgraph), and any other atoms. Distinct mappings are distinct atom assignments, all of them, not the distinct subgraphs: benzene in benzene has twelve (six rotations, two reflections) and propane in propane has two. The bond assignment follows from the atom assignment because a drawing has at most one bond between two atoms, so the two lists of a mapping always agree. A query of several parts places each part on distinct atoms anywhere in the target, so two unbonded carbons in ethane have two mappings.

### What is compared

- **Atoms.** An element atom matches an atom of the same element. A query atom matches the elements it stands for: `*` every atom including a drawn hydrogen, `A` every atom but hydrogen, `Q` every atom but carbon and hydrogen, `X` a halogen (group 17: F, Cl, Br, I, At, Ts), a list those elements, and a negated list every element but them, which includes hydrogen unless it is listed.
- **Charge and isotope.** A query element atom with a nonzero charge matches only an atom with that charge; one with charge 0 states no charge condition and matches any charge, because the drawing cannot record "neutral only". An isotope the query atom states must equal the target atom's isotope; with no isotope stated, any isotope matches. A query atom (`*`, `A`, `Q`, `X`, a list) carries neither, and the matcher does not read a query atom that does as if it did not: a drawing cannot record one (the editor clears both when a query is set), so only a graph built outside the drawing model can reach it, and it is refused as `match_unsupported` with `side: query`, `item: atom`, the atom's `id` and `reason` `query_atom_charge` or `query_atom_isotope` (the charge first). It is tested in the domain library, as `query_unsatisfiable` is.
- **Not compared.** Hydrogen counts and states (stated, unknown or inferred), radicals, attachment points, positions, the scene, settings, atom and bond IDs and the order atoms were drawn in never restrict a match and never widen one. A recorded count on a query atom is not a condition.
- **Hydrogen atoms.** A drawn hydrogen is an ordinary atom in both drawings and is never folded into its neighbor. Hydrogens that are not drawn are not atoms: a query C–H needs a drawn hydrogen in the target.
- **Bonds.** A query bond is a single, double, triple or aromatic bond, an `any` query bond, or a query list of those kinds. `any` matches every bond, including dative, unknown and query bonds. A single, double, triple or aromatic query bond, or a list, matches a target bond of one of those kinds as the matcher reads it; a dative target bond matches only `any`.
- **Aromatic bonds.** A bond is aromatic when `chemifree-aromaticity-2` classifies its ring system as aromatic, whether it was drawn Kekulé or with aromatic bonds, in the query and in the target alike. A Kekulé single bond of benzene is therefore aromatic and does not match a single query bond. The drawn kind never decides that alone: in a system the model calls aromatic, a drawn single, double or aromatic bond is read as aromatic only because the model has checked the drawn pattern (aromatic bonds throughout, or a complete Kekulé pattern in which every ring atom that needs a double bond has exactly one). A ring system that mixes aromatic bonds with other kinds is refused (`mixed_ring_bonds`), not normalised either way. In a ring system the model calls non-aromatic (cyclohexa-1,3-diene), bonds keep their drawn single and double kinds. A bond drawn aromatic must lie in an aromatic system, else it is refused with the model's reason. Every other bond keeps its drawn kind.
- **Rings and degree.** The drawing records no ring-membership, substituent-count or degree condition on an atom or bond, so none can be asked; the rings and neighbors of the query itself are what a mapping must reproduce. A condition that needs a new recorded state is not available.

### Refusals and limits

A query atom or bond that nothing can satisfy (an empty element list, a negated list that excludes every element, an empty bond list) is refused with `query_unsatisfiable`, never answered as no match. A drawing records no such predicate (lists hold 1 to 10 elements), so this code is reached only by a graph built outside the drawing model; it is tested in the domain library.

The query is read before the search. It is refused with `match_unsupported` and `side: query` when a bond is dative or unknown, or when the aromaticity model cannot read a ring system of the query that has a double or aromatic bond (a query atom or bond inside such a ring, a bond to a ring atom that is not single, a fused partially saturated system), or when a bond drawn aromatic is not in an aromatic system. A ring system with no double or aromatic bond cannot be aromatic in any spelling, so its bonds are read as drawn even if the model cannot classify the system (a ring that contains silicon, say).

A query bond (an `any` bond or a list of kinds) inside a ring system that has a double or aromatic bond is always refused, with `side: query` and the aromaticity model's reason `unsupported_ring_bond`: the shared aromaticity model cannot read a ring system that holds a query bond, and a ring system with a pi bond cannot be taken as drawn. This blocks the most common use of a wildcard ring bond, such as an `any` bond in a benzene ring. It is a limit of the aromaticity model, and the matcher has no workaround for it. The same bond in a ring of single bonds is read as drawn.

The target is refused with `side: target` only when a mapping would rest on something the matcher cannot decide: a bond of an unclassified ring system (the same rule), an unknown or query bond, or an atom that is itself a query, compared with a query item that does not accept everything. The search runs to its end, treating an undecidable comparison as a possible match. A mapping that rests on one is not returned, and if the search finds any the target is refused: the refusal names the undecided comparison with the lowest target atom ID (else, if no atom is undecided, the lowest target bond ID), and for that target item the lowest query ID, over every such mapping, with the reason of that item. When the search runs to its end, neither the order the atoms were drawn in nor the order the search visits them changes the refusal or its reason. If no mapping rests on an undecided comparison, the mappings returned are exactly the decided ones. A target is therefore not refused for an unclassified system that no mapping reaches, and no mapping is lost to one.

**A refusal that is known wins over a limit.** A mapping that rests on an undecided comparison counts toward the 1,000, and the assignment that would be the 1,001st is read before the limit is reported. If a limit ends the search and any complete assignment the search had reached rests on an undecided comparison, the reply is `match_unsupported` (never `match_limit`), naming the lowest undecided comparison among the assignments reached, by the rule above. A limit that ends the search before such an assignment was reached is `match_limit`, even if an assignment the search never reached would have made it a refusal. Because a limit stops the search early, which assignments were reached depends on the order atoms were drawn in (and so on how the search visits them): for a search that a limit ends, both whether the reply is the refusal or the limit and, for the refusal, which undecided comparison it names can differ between numberings of the same drawing. What it names is always an undecided comparison of a complete assignment that exists in the target, but it need not be the lowest over all of them; only a search that runs to its end names that one. Refusal is never an empty list: a Kekulé benzene query in tetralin is refused, although RDKit finds the benzene ring.

`details` of every refusal and limit holds `query_snapshot` and `target_snapshot`, the same snapshots a result carries. For `match_unsupported` it also holds `side` (`query` or `target`), `reason`, `item` and `id` (the atom or bond on that side that cannot be read) and, for a target, `query_item` and `query_id` (the query atom or bond compared with it). For `match_limit` it holds `limit_kind` (`search_steps`, `scanned_candidates` or `mappings`) and `limit`; for `query_too_large` the `atoms` it has and the `limit`; for `query_unsatisfiable` the `item` and `id`. An unknown document is the ordinary `document_not_found`.

**Limits.** The query has at most 64 atoms (`query_too_large`); a search tries at most 2,000,000 assignments and examines at most 20,000,000 candidate entries; a search returns at most 1,000 mappings, and finding the 1,001st is the limit. A step is one target atom that passed the element and bond-count prefilter, is not yet assigned, and is tried for a query atom against the bonds to atoms assigned before it. Looking at the entries of a candidate list or of an assigned atom's neighbor list to find such atoms is not a step; every entry looked at, skipped or tried, counts once toward the 20,000,000, which bounds the work steps leave out, however large the target. The step count can depend on the order atoms were drawn in, so a search close to the limit may finish under one numbering and not under another; the mapping count does not, and neither does the refusal of a search that finishes (what a limit does to it is described above). Reaching any of the last three is `match_limit` with no mappings, unless a refusal is already known (see above): a bounded search that stops is never reported as complete. Unbonded query atoms multiply: parts with n and m candidates have n x m mappings with distinct targets, so five unbonded nitrogens in a target holding five (120 mappings, 325 steps) are answered, while three unbonded carbons in 2,000 carbons reach the mapping limit. The target may be as large as a document (2,000 atoms). **Perceiving the drawings is not counted in these limits.** Before the search, `chemifree-aromaticity-2` reads the rings of the target (and of the query, which has at most 64 atoms) to decide which bonds are aromatic. That work counts toward neither `steps` nor the 20,000,000, and no limit of this operation bounds it; it is bounded by the model's own caps and by the size of a document. A drawing holds at most 2,000 atoms and 4,000 bonds. Ring systems are found in one depth-first pass over the atoms and bonds with no budget, and a ring system of more than 128 atoms is not read (`system_limit`: the system is unclassified, so a bond in it is refused when a mapping rests on it, or read as drawn if the system has no double or aromatic bond, as for any unclassified system). Reading a system is bounded by its size, and the only search in it is the Kekulé assignment of a system with recorded aromatic bonds, which has a fixed budget of 100,000 steps for each such system (`search_limit`, see Ring and aromaticity interpretation). So "a search is bounded" says that the search itself is bounded by the limits above and the reading of the drawings by those caps; no further limit is added. The target is read only once every query atom has a candidate (an empty answer can come before it), and a drawing searched in itself is read twice. The search is a depth-first extension from adjacency lists, restricted by the element and the number of bonds of each candidate; a target atom is tried for a query atom only through a bond of an atom already assigned, except for the first atom of each part of the query.

### Substructure codes

Every code is the diagnostic `code` of a reply with no `result`. The table is `MATCH_CODES` in `crates/application/src/operations/substructure.rs` and is compared with it by a test.

| Code | Outcome | Meaning |
| --- | --- | --- |
| `empty_query` | error | The query drawing has no atoms. |
| `query_too_large` | error | The query drawing has more atoms than one search accepts. |
| `query_unsatisfiable` | error | No atom or bond can satisfy this query atom or bond. |
| `match_unsupported` | unsupported | The matcher cannot read this atom or bond, and the answer depends on it. |
| `match_limit` | error | The search reached a limit before it could give a complete answer. |

### Substructure refusal reasons

`reason` of `match_unsupported` is one of these. The first rows are the matcher's own; the others are the reason codes of the aromaticity model (see Ring and aromaticity interpretation), passed on unchanged, of which the table shows those that have a fixture. Each row names a fixture of `crates/application/tests/data/query-fixtures.json` that is refused with exactly that code, side and reason.

| Reason | Side | Fixture |
| --- | --- | --- |
| `query_dative_bond` | query | `dative-bond-in-query` |
| `unknown_bond` | query | `unknown-bond-in-query` |
| `unknown_bond` | target | `unknown-bond-in-target` |
| `query_bond` | target | `query-bond-in-target` |
| `target_query_atom` | target | `query-in-target-with-query-atom` |
| `aromatic_bond_outside_ring` | query | `aromatic-bond-outside-ring-in-query` |
| `query_atom` | query | `query-atom-in-conjugated-ring` |
| `unsupported_ring_bond` | query | `query-bond-in-conjugated-ring-of-query` |
| `unsupported_ring_bond` | query | `wildcard-bond-in-aromatic-ring-query` |
| `unsupported_external_bond` | query | `query-bond-to-aromatic-substituent` |
| `electron_count` | query | `pyrrole-without-nh-in-query` |
| `fused_partially_aromatic` | query | `tetralin-query` |
| `fused_partially_aromatic` | target | `benzene-kekule-in-tetralin` |
| `fused_system_outside_model` | target | `benzene-in-indole` |
| `mixed_ring_bonds` | query | `partly-aromatic-ring-in-query` |
| `mixed_ring_bonds` | target | `partly-aromatic-ring-in-target` |

### Substructure examples

Each row is a fixture, checked against this service by the test suite; `scripts/verify-query-rdkit.py` compares the same fixtures with an RDKit 2022.03.2 Python by hand. Mappings are atom mappings, all of them.

| Fixture | Result |
| --- | --- |
| `benzene-kekule-in-benzene-aromatic` | 12 mappings |
| `benzene-aromatic-in-benzene-kekule` | 12 mappings |
| `propane-in-propane` | 2 mappings |
| `benzene-in-naphthalene` | 24 mappings |
| `benzene-in-anthracene` | 36 mappings |
| `pyrrole-kekule-in-pyrrole-aromatic-nh` | 2 mappings |
| `single-bond-does-not-match-kekule-benzene` | 0 mappings |
| `neutral-query-atom-matches-any-charge` | 3 mappings |
| `charged-query-atom` | 1 mapping |
| `explicit-hydrogen-not-in-implicit-carbon` | 0 mappings |
| `two-carbons-in-propane` | 6 mappings |
| `pyridine-not-in-indole` | 0 mappings |
| `cyclohexadiene-keeps-its-drawn-kinds` | 4 mappings |
| `bromine-not-in-tetralin` | 0 mappings |
| `benzene-kekule-in-tetralin` | refused |

## Stereo inspection

`chem.stereo` takes `{ "document_id", "instance"? }` (no other parameter) and compares, for every stereocenter and double bond that holds a stored configuration or that a mark bears on, what the document states with what the drawing of each scene instance shows. It is read-only: it stores nothing, derives nothing into the document, changes no revision, history or selection, and a repeated `request_id` is not replayed but inspects the then-current document, as for the other inspections. HTTP `POST /api/v1/operations` and `chemifree operations` give the same replies from the same `Application::execute`. The outcome is `ok` and never `partial`, and `loss_report` is empty: a disagreement is a finding about a valid document, not a lost feature. `model_id` is `chemifree-stereo-1`, the model of [Deriving a configuration from a drawing](#deriving-a-configuration-from-a-drawing), whose rules (frame, sign, marks that bear on a center, fourth position, constants) produce the drawn side; they read the valence model, so `valence_model` names it in the reply.

**Why a separate operation.** The public design lists a stereo check among the `checks` of a validation operation as an interface proposal. Chemifree keeps `chem.validate` valence-only (`chemifree-valence-3`, one assessment per atom, no `checks` parameter) because the unit differs (per center or double bond and per instance) and a green validation must not hide, or absorb, a stereo finding. This is a deliberate divergence, not an equivalence. A later `checks` parameter of `chem.validate` may delegate to this model; each finding keeps its own `model_id` and the codes below stay the contract. `chem.validate`, `chem.identity` and `chem.substructure` are unchanged and do not read stereo.

**Reply.** `result` is `{ snapshot, valence_model, stereogenicity_assessed, instances, atoms, bonds }`.

- `snapshot` is the exact `{ document, session, recovery, notes }` that was compared, captured under one lock before the comparison runs (`snapshot.document.stereo` is what is stored, and it is the same before and after the call). `document_revision` is its revision.
- `stereogenicity_assessed` is always `false`: it is a property of the model, not of the document. No atom or bond is judged stereogenic or not, and an atom or bond that is not listed is neither stereogenic nor non-stereogenic as far as this operation knows; "not listed" is never "nothing wrong".
- `instances` are the scene instances inspected, in identifier order.
- `atoms` has one entry per item: `{ atom, stored, drawn }`, ascending by atom ID. An atom is an item when a center record is stored for it, or when a mark bears on it (it is the narrow end of a wedge or hash, or an end of a wavy bond, and could hold a center record). `stored` is the stored configuration in the shape of a snapshot (so it can be sent back as `set_atom_configuration`) or `null`. `drawn` has one entry per inspected instance, in `instances` order: `{ instance, configuration, reason, agreement, label_differs }`. `configuration` is what the drawing shows, in the same shape, and is `null` exactly when `reason` is set; `reason` is a `stereo_not_derivable_*` code or an existing structural `stereo_*` code of the command (the codes under [Stereo marks and configurations](#stereo-marks-and-configurations)). `label_differs` is `true` when the stored and the drawn configuration both have a non-atom position and its label (`implicit_hydrogen` or `lone_pair`) differs; it says so apart from the agreement, which ignores the label (see below).
- `bonds` has one entry per double bond that is an item (a stored double-bond record, or a crossed mark): `{ bond, stored, drawn }`, with `drawn` entries `{ instance, configuration, reason, agreement }` (no label field). A double bond is read from its geometry whether or not it is marked, so its drawn side is a `same_side` or `opposite_sides` relation, `unknown` for a crossed bond, or a reason.

**Agreement** is decided per item and per instance, by this closed list:

| `agreement` | Stored | Drawn | Meaning |
| --- | --- | --- | --- |
| `agrees` | yes | derivable, same geometry | nothing to do |
| `disagrees` | yes | derivable, different geometry | a definite state against `unknown`, either way round, is a disagreement |
| `only_stored` | yes | not derivable (any reason; `stereo_not_derivable_no_marks` is the normal one after an import or a cleared mark) | the drawing says nothing usable |
| `only_drawn` | no | derivable | not stored; `set_configuration_from_drawing` would store it |
| `neither` | no | not derivable | marks are present but unusable, and the reason says why |

Same geometry means the same kind, the same neighbor set and, for a tetrahedral center, the same turn in canonical order. The single non-atom position of a center with three neighbors is one position whatever its label: a stored `implicit_hydrogen` against a drawn `lone_pair` with the same turn `agrees`, with `label_differs` true. `unknown` agrees with `unknown` and with nothing else. Double-bond references are canonical on both sides, so equal states agree. Different instances can show different configurations from the same marks, so the same stored record can agree in one instance and disagree in another; the command stores the configuration of the instance it is given.

**Diagnostics** are advisory; none refuses anything. In order: the findings of each atom entry and then of each bond entry (each by ID, and the instances in `instances` order within an entry), then `mark_narrow_end_not_a_center`, `center_has_aromatic_bond` and `wavy_between_possible_centers`, and last `stereo_not_checked`.

| `code` | Severity | When (`details`) |
| --- | --- | --- |
| `drawing_disagrees` | warning | an entry is `disagrees` (`item`, `id`, `instance`, `stored`, `drawn`) |
| `drawing_not_derivable` | warning | a center that a mark bears on (the same test that lists an unstored center as an item) but whose drawing gives no state in that instance, for any reason, or a double bond whose drawing gives none for any reason, including a crossed bond the model cannot hold (`stereo_geometry_unsupported`, `stereo_reference_invalid`) (`item`, `id`, `instance`, `reason`) |
| `marks_without_configuration` | info | an entry is `only_drawn` (`item`, `id`, `instance`) |
| `configuration_without_marks` | info | a center entry is `only_stored` and no mark bears on the atom, so the reason is normally `stereo_not_derivable_no_marks` (a structural `stereo_*` reason is checked before the marks, so it can only appear here when the atom is unmarked; the entry's `reason` shows it) (`item`, `id`, `instance`) |
| `mark_narrow_end_not_a_center` | warning | a wedge or hash whose narrow end cannot hold a center record states nothing; once per bond (`bond`, `narrow_atom`, `other_atom`, and `hint` when the other end could be a center, since a mark bears on the atom at its narrow end) |
| `center_has_aromatic_bond` | warning | an atom listed in `atoms` that is stored, or derivable in an *inspected* instance, has an aromatic bond; the drawing is read as drawn and nothing about it is judged; once per atom (`atom`). It is about the items of this reply, so naming an `instance` can remove it: a center derivable only in an instance that was left out, and not stored, is not reported |
| `double_bond_substituents_same_side` | warning | an end with two substituents drawn on the same side of the double bond, for a derived relation; the relation follows the lower-numbered one (`item`, `id`, `instance`, `ends`) |
| `wavy_between_possible_centers` | info | a wavy bond joins two atoms that could both hold a center record; an end with its own wedge or hash follows that mark and the other end is `unknown`; once per bond (`bond`, `atoms`) |
| `drawing_weak_evidence` | info | a tetrahedral state was derived from a nearly flat drawing, `\|D\|` below 0.1 (about 1.4 degrees for a nearly straight pair of plain bonds with a wedge); the state is still reported (`item`, `id`, `instance`, `strength`, `limit`) |
| `stereo_not_checked` | info | always, once: only stored against drawn is compared. This operation computes no CIP label (`chem.cip` does) and judges no atom stereogenic, ring and strain plausibility are not checked, and `chem.identity` and `chem.substructure` do not read stereo |

`drawing_weak_evidence` is reported when `|D|` is below the named constant `WEAK_EVIDENCE` (0.1) and not below the refusal limit (0.001); `set_configuration_from_drawing` is unchanged and still derives and stores at or above 0.001.

**Limits and parameters.** The work is the neighborhood of each item in each inspected instance, plus one neighbor index (linear in the atoms and bonds) per inspected instance, one for the findings that do not depend on an instance, and one for finding the items. An entry is one item in one instance, and a reply holds at most `MAX_ENTRIES` = 20,000 of them: more is `resource_limit` (outcome `error`, no result, `path: "parameters.instance"`, `details`: `entries`, `limit`, `items`, `instances`, `parameter: "instance"`), and naming one `instance` always fits (a document has at most 2,000 atoms and 4,000 bonds). Diagnostics are about as many as the entries. A malformed or unknown `instance` (a string that is not a scene instance of the document, including the identifier of a group or an atom, or any value that is not a string) is `invalid_request` with `path: "parameters.instance"`; `null` is the same as leaving it out, which inspects every instance. A document that cannot be read is answered as for the other inspections.

Example (the document holds C `a1` with F `a2` above, Cl `a3` lower right and Br `a4` lower left, a wedge on `b2` with its narrow end at `a1`, and the stored `clockwise` turn of `a1`; the picture is then edited so that Br is at the upper right):

```json
{ "document_revision": 2, "outcome": "ok", "model_id": "chemifree-stereo-1",
  "result": { "stereogenicity_assessed": false, "valence_model": "chemifree-valence-3", "instances": ["s1"],
    "atoms": [ { "atom": "a1",
      "stored": { "kind": "tetrahedral", "neighbors": ["a2", "a3", "a4", "implicit_hydrogen"], "turn": "clockwise" },
      "drawn": [ { "instance": "s1",
        "configuration": { "kind": "tetrahedral", "neighbors": ["a2", "a3", "a4", "implicit_hydrogen"], "turn": "anticlockwise" },
        "reason": null, "agreement": "disagrees", "label_differs": false } ] } ],
    "bonds": [] },
  "diagnostics": [ { "code": "drawing_disagrees", "severity": "warning" }, { "code": "stereo_not_checked", "severity": "info" } ] }
```

(`snapshot` and the full diagnostics are left out.) Nothing is re-derived: the stored turn stays `clockwise` until someone changes it, for example with `set_configuration_from_drawing`. The web editor calls `chem.stereo` automatically for a drawing that holds stereo (see "Web editor, stereo agreement" under the stereo marks and configurations notes).

## CIP labels

`chem.cip` takes `{ "document_id", "scope"?, "atoms"?, "bonds"?, "time_limit_ms"? }` (no other parameter; `time_limit_ms` is the limit of [computation time limit and cancel](#computation-time-limit-and-cancel)) and computes, for stereocenters and double bonds, the CIP descriptor that the stored configuration has in the current graph: `R` or `S` for a center, `E` or `Z` for a double bond. `model_id` is `chemifree-cip-1`. It is read-only in the same sense as [`chem.stereo`](#stereo-inspection): it stores nothing, changes no revision, history, selection or dirty state, and a repeated `request_id` is not replayed but computed again on the then-current document. HTTP `POST /api/v1/operations` and `chemifree operations` give the same replies from the same `Application::execute`. The algorithm is the domain's (`crates/domain/src/cip.rs`); the application reaches it through the `CipLabeler` port (`crates/application/src/cip.rs`), whose built-in implementation is installed by default and whose answers the application validates before it replies (each requested unit answered exactly once, a descriptor only with the state `assigned` and of the unit's own kind, reasons from the closed list below, a `cip_slot_unusual` warning exactly where the stored non-atom slot differs from the one the valence model names (never on a center with four atoms or an agreeing slot, and not left out where the built-in rules always raise it), a ranking that is exactly the unit's own four positions ranked 1 to 4 and, for a double bond, a higher substituent that is one of the end's own positions); the port has three methods, labeling centers, labeling double bonds and the scan for `scope: "stereogenic"` (`candidates`, whose default is the built-in scan and whose answer is validated like the others: units ascending without repeats, each an answer for its own unit, and a charge that is not less than what the reports it returns cost); a replacement labeler (`Application::with_cip_labeler`, Rust only) whose answer fails that check gets the diagnostic `cip_labeler_invalid` (outcome `error`, nothing labeled).

**What is computed, and what is not.** The sequence rules are 1a (higher atomic number first, with duplicate atoms for multiple bonds and ring closures, phantom atoms for missing substituents and lone pairs, and implicit hydrogens as hydrogen atoms), 1b (a duplicate nearer the root first), 2 (higher mass number first) and 3 (Z before E among ligand double bonds), each applied to the whole digraph before the next. Not applied: Rules 4a to 5, so no pseudoasymmetric `r` or `s` and no ligand-descriptor comparisons; axes, planes and helices (the model cannot store them); and heteroaromatic or charged aromatic rings (only neutral all-carbon aromatic systems are read, as mancude rings). A tie that a rule not applied might break is `unsupported`, never `none`, and a case the applied rules cannot settle is reported as such, never guessed. That refusal is a limit of this build; the public descriptor vocabulary (below) already has pseudo descriptors. Charges do not enter any rule (they matter only through the hydrogen count of the valence model). Enhanced stereo groups are ignored by CIP in this build. A label describes the stored configuration of this graph; it never reads the drawn marks or positions, is never stored or written back, and never changes what the document holds. An imported `AS` or `BS` annotation or a `(S)` text object in a drawing is never read as a label.

**Parameters.** Without parameters the operation lists every stored configuration (`scope: "stored"`). `scope` is `stored`, `stereogenic` or `named`; `named` is what a reply says when only the named items were listed, and sending it means exactly that (it needs `atoms` or `bonds`, otherwise `invalid_request` with `path: "parameters.scope"`). `scope: "stereogenic"` lists in addition every center or double bond that the scan finds stereogenic or undecidable and that has nothing stored (a double bond inside a ring system with no stored configuration is not scanned). `atoms` and `bonds` name items: a named item is always evaluated and listed, whatever the scope, with its true state; naming without a `scope` lists only the named items (`scope: "named"` in the reply), and naming together with a `scope` lists both, merged and ascending. A repeated identifier merges. An atom that is not a candidate (it needs three or four bonded neighbors) or a bond that is not a double bond is listed `unsupported` with `cip_not_a_candidate`. `null` is the same as leaving a parameter out. A malformed `scope` (anything but those three strings or `null`), a list that is not a list of strings or is empty, an identifier that does not name an atom (or a bond) of the document, and one of the wrong kind are `invalid_request` with the path of the first fault (`parameters.atoms[2]`, `parameters.scope`) and the offending value in `details.value`.

**Reply.** `result` is `{ snapshot, model, rules, scope, atoms, bonds }`. `snapshot` is the exact `{ document, session, recovery, notes }` the labels are about, captured under one lock before the labeler runs (`snapshot.document.stereo` is what is stored); `document_revision` is its revision. `rules` is `["1a", "1b", "2", "3"]`. `atoms` has one entry per listed center, ascending by atom ID: `{ atom, state, descriptor, reason, stored, priority, decided_by }`; `stored` is the stored configuration in the shape of a snapshot, or `null`; `priority` is the four positions (atom IDs, `implicit_hydrogen`, `lone_pair`) highest first with their rank from 1, and `decided_by` the last rule needed to separate them (`1a`, `1b`, `2` or `3`); both are present only for the states `assigned`, `unknown` and `unspecified`. `bonds` has one entry per listed double bond, ascending by bond ID: `{ bond, state, descriptor, reason, stored, ends }`, where `ends` (same three states) gives, for the `begin` end and then the `end` end, `{ atom, higher, decided_by }`, the position that ranks higher there. The descriptor is `R` or `S` for a center and `E` or `Z` for a bond exactly when the state is `assigned`; `reason` is set exactly when the state is `unsupported` or `ambiguous`.

| `state` | Meaning |
| --- | --- |
| `assigned` | The unit is stereogenic under the applied rules, a configuration is stored, and the ranking is complete: `descriptor` is the label. |
| `none` | The unit is not stereogenic under the applied rules and no rule not applied could separate the tied substituents (two equal ligands, a `=CH2` end). Listed when it is stored or named; a stored configuration on such a unit also gets `cip_stored_not_stereogenic`. |
| `unspecified` | Stereogenic, nothing stored. Listed with `scope: "stereogenic"` or when named. |
| `unknown` | Stereogenic and the stored record says "either"; the ranking is still given. This is a state, not a loss. |
| `ambiguous` | The label depends on stereo the document does not state: Rule 3 needs a ligand double bond whose geometry is unspecified or stored `unknown`. The reason is `cip_ligand_stereo_unspecified`. |
| `unsupported` | The model cannot decide; `reason` says why. |

Closed list of reasons (the model's own, not another program's): 

| `reason` | Meaning |
| --- | --- |
| `cip_not_a_candidate` | The named atom is not a stereocenter candidate (it needs three or four bonded neighbors) or the named bond is not a double bond. |
| `cip_center_structure` | The stored center does not fit the structure the model reads: an aromatic bond or a radical at the center, or a stored statement the valence model cannot place. |
| `cip_double_bond_structure` | The double bond does not fit the structure the model reads: a cumulated end, more than two other neighbors at an end, positions the valence model does not fit to two, or a stored reference that names no position of an end. |
| `cip_query_atom` | A query atom is met while expanding the digraph. |
| `cip_bond_kind` | A dative, unknown or query bond is met while expanding the digraph. |
| `cip_attachment_point` | An atom with an attachment point is met while expanding the digraph. |
| `cip_hydrogens_unknown` | The valence model cannot give the hydrogens of an atom that must be expanded: unknown hydrogens, a radical, or an element, charge or valence it does not list. |
| `cip_aromatic_system` | An aromatic system other than a neutral all-carbon one must be looked into. |
| `cip_mancude_rule_1b` | Rule 1b is needed on a digraph with a mancude duplicate, and the rule is not defined for those. |
| `cip_rule_1b_duplicate_distance` | Rule 1b compares duplicates that two readings of the distance to the corresponding atom order differently, which happens in small strained rings. |
| `cip_mass_number` | Rule 2 compares an unlabeled atom with a labeled one and the answer depends on how an unlabeled atom is read. |
| `cip_rule_4_or_5_needed` | The ligands tie under the rules applied (see `rules`); Rules 4 and 5, not among them, might separate them. |
| `cip_stereo_cycle` | The descriptor of a ligand double bond depends on itself. |
| `cip_rule3_nested_descriptor` | Rule 3 needs the descriptor of a ligand double bond that reads differently ranked in the whole molecule and ranked in the digraph rooted at the unit. |
| `cip_ligand_stereo_unspecified` | Rule 3 has to place a ligand double bond whose geometry the document does not state, so the answer depends on information it does not have. |
| `cip_ranking_inconsistent` | The pairwise comparisons do not form a consistent order; this is not expected and is reported, not guessed. |
| `cip_digraph_limit` | The comparison needs more digraph nodes or spheres than the limits allow. |

**Outcome.** `ok` when no listed item is `ambiguous` or `unsupported`; `partial` when any is, with one `loss_report` entry per such item (`feature` `cip_descriptor`, `source_path` `atoms[a7]` or `bonds[b9]`, `disposition` `unknown`, `semantic_impact` `stereo`, `reason` the code and its sentence). The other items of the same reply are still labeled. `none`, `unspecified` and `unknown` are `ok`. The per-item state `unsupported` is unrelated to the operation outcome `unsupported` (an operation that does not run).

**Diagnostics.** `cip_stored_not_stereogenic` (warning, `details: { item, id }`) for a stored configuration on a unit found `none`; `cip_slot_unusual` (warning) for a center whose stored non-atom position differs from the one the valence model names (the stored one was used); both come first, centers then bonds by ID. Always last: `cip_scope` (info, `details.scope`, saying what the scope listed and did not examine) and `cip_not_checked` (info, repeating that Rules 4a to 5, `r` and `s`, axes and planes and heteroaromatic rings are not applied and that a label is not stored).

**Limits.** The work is counted in digraph nodes: 20,000 per item and 2,000,000 per request (checked at stage boundaries, so a stage may run a little past a limit); a center's ranking reads at most 256 spheres, a double bond's end 32, and at most 4 ligand double-bond descriptors wait on one another. An item that reaches a limit is `unsupported` (`cip_digraph_limit`), never `none`. The 2,000,000 are one budget for the whole request: with `scope: "stereogenic"` the scan runs first (it labels every candidate and its reports are the answer for those units, so nothing it covered is labeled again), the stored and named units it did not cover are labeled with what it charged left over, and among those the double bonds get what the centers left. A request that needs more is refused whole: outcome `error`, no result, `resource_limit` whose `path` and `details.parameter` name the part that stopped (`parameters.scope` when the scan stopped, for which naming items does not help and `scope: "stored"` or a request that names items without a scope does; `parameters.atoms` for the centers; `parameters.bonds` for the double bonds) and whose `details` are `stage` (`scan`, `centers` or `bonds`), `limit` (the node limit that stage was given, which for the double bonds is what the centers left), `request_limit` (the whole budget), `items` (the units in play: the atoms and bonds of the document for the scan, otherwise the stored, named and scanned units), `item_limit`, `nodes_used` and `parameter`. `nodes_used` is what that stage had counted when it stopped, so it is greater than that stage's `limit` and may exceed it by the work of the last item (a stage checks at item and stage boundaries). It is not the request total; for the double bonds `limit` is what the centers left, and `nodes_used` is what the double bonds themselves counted. What the request had used in all when it stopped is `request_limit - limit + nodes_used`. A replacement labeler's own stop is reported as it gives it, with `limit` and `nodes_used` unchecked (the labeler is trusted, as described below). A labeler's stage with nothing to label is not run. A replacement labeler that reports more nodes than the budget its stage was given, whatever it answered, is treated as having hit the limit: the same `resource_limit` reply, never labels over the cap. The clock is a second bound, not a work limit: `time_limit_ms` (default 5 seconds, at most 8; see [computation time limit and cancel](#computation-time-limit-and-cancel)) gives one deadline for the whole request, and a stop by it is `outcome` `error` with the diagnostic `time_limit`, `details.limit_ms` and no result. A host stop (`Application::execute_with`, as when an HTTP client closes the connection) ends a running request with the diagnostic `cancelled` and no result, and wins over an expired deadline. No lower node cap for dense graphs has been added: that would be a model revision, so it is deferred (issue #250). A document whose stereo cannot be read is `document_unreadable`. Every error reply of this operation, an unknown document and parameters that do not read at all (no `document_id`, one of the wrong type, an unknown field) included, carries the labeler's `model_id`.

**What the application checks of a replacement labeler, and what it trusts.** A replacement labeler (`Application::with_cip_labeler`, Rust only) is a trusted, locally installed component, not an untrusted input. The application validates exactly what it can check against its own data (the document, the request and the limits it enforces): each answer names the requested units once, in order; the state, descriptor, reason, stored record, ranking and warnings fit the unit and the document (see above); a scan's units ascend, each is an answer for its own unit, and its charge is not less than what its reports cost (`cip_labeler_invalid`, problem `nodes`); the nodes a stage reports, and a scan's charge, do not exceed the budget that stage was given (else the `resource_limit` reply above). Its `model_id`, `rules` and `limits` are what the catalog note, the `cip_not_checked` text, the reply's `rules` and the node budget are built from; text that depends on the algorithm itself (the spheres limits, what the built-in does not apply) is said only of the built-in labeler. The sentences of a reply that speak of rules (`cip_not_checked`, the catalog note, `cip_stored_not_stereogenic`, the reason `cip_rule_4_or_5_needed`) name no fixed rule list except where they are built from the active labeler's `rules`; the rest of this section, which names Rules 1a, 1b, 2 and 3 and the figures of the limits, describes the built-in `chemifree-cip-1`. Anything else a labeler reports is trusted: that its rule list is true of what it does, and that its chemistry (the descriptors, rankings and which units are stereogenic) is correct. The application does not and cannot verify those.

**Public vocabulary.** The public functional specification lists numbered descriptor values for atoms (Undetermined 0, None 1, R 2, S 3, PseudoR 4, PseudoS 5, Unspecified 6) and for bonds (Undetermined 0, None 1, E 2, Z 3); this table says how Chemifree's states relate to them. Chemifree sends the strings, never these numbers, and does not reproduce a writable descriptor: its labels are computed on demand and cannot be set.

| Chemifree | Public value |
| --- | --- |
| atom `assigned` `R` / `S` | R (2) / S (3) |
| bond `assigned` `E` / `Z` | E (2) / Z (3) |
| `none` | None (1) |
| atom `unspecified` | Unspecified (6); a bond has no such value, so a bond's `unspecified` is a product extension |
| `unknown`, `ambiguous`, `unsupported` | no public value (the nearest, Undetermined (0), is kept distinct on purpose) |
| reserved `pseudo_r`, `pseudo_s` (atoms only; never emitted by `chemifree-cip-1`) | PseudoR (4), PseudoS (5) |

**Honest limits.** This is not claimed equivalent to any other program's labels. It has been compared by hand with one pinned version of RDKit's CIP labeler (2022.03.2) through `scripts/verify-cip-rdkit.py` and `crates/domain/tests/cip-fixtures.json`; that check is manual, covers those fixtures only and is evidence, not a guarantee. The two readings differ where this model refuses (Rules 4 and 5, heteroaromatic rings, the nested ligand-descriptor cases) and where RDKit orders an unspecified ligand double bond below E while this model says `ambiguous`. The web editor shows these labels only behind two switches that are off by default (see the "Web editor, CIP labels" entry in the web editor list below) and never asks for `scope: "stereogenic"`.

Example (L-alanine: N `a1`, the center `a2` with the stored turn `clockwise` over `[a1, a3, a4, implicit_hydrogen]`, methyl `a3`, carboxyl carbon `a4`; `snapshot` and the diagnostic messages are left out):

```json
{ "document_revision": 1, "outcome": "ok", "model_id": "chemifree-cip-1",
  "result": { "model": "chemifree-cip-1", "rules": ["1a", "1b", "2", "3"], "scope": "stored",
    "atoms": [ { "atom": "a2", "state": "assigned", "descriptor": "S", "reason": null,
      "stored": { "kind": "tetrahedral", "neighbors": ["a1", "a3", "a4", "implicit_hydrogen"], "turn": "clockwise" },
      "priority": [ { "neighbor": "a1", "rank": 1 }, { "neighbor": "a4", "rank": 2 }, { "neighbor": "a3", "rank": 3 }, { "neighbor": "implicit_hydrogen", "rank": 4 } ],
      "decided_by": "1a" } ],
    "bonds": [] },
  "diagnostics": [ { "code": "cip_scope", "severity": "info" }, { "code": "cip_not_checked", "severity": "info" } ],
  "loss_report": [] }
```

## Numeric foundation (P15)

The `chemifree-numerics` Rust library separates exact signed rational values from Real48, binary32, binary64 and 80-bit extended raw storage. Finite conversions and basic arithmetic use each documented rounding rule at explicit intermediate stores, and report nonfinite input, invalid encodings, division by zero, overflow or resource limits. The selected heading step uses explicit extended intermediate operations and a binary64 final store; the deviation boundary rounds nonnegative half-integers upward. Typed length conversion uses the computer point and exact physical factors; an independent Real48 mol/L–g/L conversion requires an explicit positive molar mass. Decimal display and F32 storage each use their stated, separate rounding rules. These types do not add an HTTP or CLI chemistry operation or claim legacy property-unit or locale equivalence. The exact supported boundary is described in [NUMERICS.ko.md](NUMERICS.ko.md).

## External DATA status

`GET /api/v1/data` and `chemifree data status --json` return `api_version`, `state` (`not_configured`, `refused`, or `checked`), nullable `reason`, registration IDs with `loaded` or `unavailable` state and nullable reason, and `engine: "not_implemented"` for the separate prediction model binding. A loaded registration may also include `retained_extra_columns`, a nonnegative integer count of source columns kept as opaque metadata; older responses omit it. The CLI and web display the count when it is positive. Neither response contains column names, data records or source paths. A checked registration means only that a supported profile was authorized and decoded; `chem.properties` independently recognizes the two mass input profiles described above. The host selects and trusts each registration separately; an aggregate registry hash alone does not establish manifest membership. Public checks use independent fixtures, not personal payloads. Missing or invalid data never prevents document editing. The explicitly selected local trust descriptor, identity checks, supported profiles, and current limits are specified in [DATA.ko.md](DATA.ko.md).

Catalog: `{ implementation, version, contracts: [1], profiles: [{ id, execution, note }], operations: [{ id, representation, execution, followup?, note, commands?, sub_operations? }] }`. Representation and execution are separate: `representation` ∈ `supported | not_supported` says whether a document and this contract can hold the operation's inputs and results; `execution` ∈ `executable | requires_data | not_implemented | rejected` says whether it runs, would need data that is not loaded, does not run, or never will. `document.transaction` lists its sub-operations, each either a command or a capability. A command row has no `commands` field and its `id` is an `op` of the transaction body. A capability row is not a command: its `commands` lists the command rows (by `id`) that carry it out, and is empty while it is not implemented. `commands` was added within contract 1; a reply without it predates that. The transaction lists the existing atom, bond, title, and placement commands, plus `add_graphic`, `update_graphic_points`, `set_graphic_line_width`, `add_caption`, `update_caption`, `contract_fragment`, `set_fragment_display_text`, `set_fragment_display_runs`, `expand_fragment`, `group`, `ungroup`, `reorder`, `delete_objects`, `move_objects`, `move_selection`, `arrange_objects`, `transform_selection`, `add_reference`, and `remove_reference`. The `scene_tree` capability names these nineteen commands; `scene_placement` names the three instance commands. Recorded-state capability rows name their implementing atom and bond commands. The four stereo commands are command rows (`set_bond_mark`, `set_atom_configuration`, `set_bond_configuration`, `set_configuration_from_drawing`); the `stereo_marks` capability row names the first and `stereo_configuration` the other three. The eight stereo group commands are command rows too (`create_stereo_group`, `delete_stereo_group`, `set_stereo_group_kind`, `set_stereo_group_number`, `add_stereo_group_members`, `remove_stereo_group_members`, `merge_stereo_groups`, `split_stereo_group`), carried out for the `stereo_group` capability row, which also states the group limits (at most one group for each stored center, so no more than the 2,000-atom cap; one absolute group; a stored number is any 32-bit signed integer); the catalog has no separate limits field. The transaction also lists the existing Et display commands `collapse_ethyl`, `update_abbreviation_label`, and `expand_ethyl`. The transaction lists 59 sub-operations: 48 command rows and 11 capability rows, including bounded `insert_ring` (P34a/P34b) and `attach_ring` (P34c) drawing recipes. Recorded states remain separate from the bounded `chem.validate` inspection described above. `chem.properties` lists formula and mass (P19). `followup` names the plan entry expected to change a row (docs/roadmap/).

Protocol errors (`envelope: "protocol_error"`):

| Code | Outcome | When |
| --- | --- | --- |
| `malformed_request` | `error` | Not JSON, not an object, a key repeated in any object, a missing or mistyped field (`contract` must be a non-negative integer), or an unknown field; over HTTP also a body that could not be read (400) |
| `unsupported_contract` | `unsupported` | `contract` is another non-negative integer; `details.supported` lists the accepted versions |
| `invalid_request` | `error` | `request_id` breaks the v1 rule (1–64 of `[A-Za-z0-9_-]`) |
| `unknown_operation` | `unsupported` | No such operation; the name is echoed |
| `unknown_profile` | `error` | No such profile |
| `resource_limit` | `error` | Larger than the entry point reads: 256 KiB over HTTP (413), 1 MiB per CLI request, not counting its line ending (the rest of the line is skipped and the request answered) |
| `unsupported_media_type` | `error` | HTTP without a JSON `Content-Type`, `application/json` or `application/*+json` as on the v1 routes (415) |

HTTP answers 200 with every operation result, whatever its outcome, and 400 with a protocol error. The route needs the session token like other writes. `chemifree operations` reads one JSON request per line (a line ending, `\n` or `\r\n`, is not part of it) and writes one compact reply per request, in order; blank lines are skipped without a reply. Its sessions last for the process and it writes no recovery drafts, so only a `document.save` keeps edits, and a session's draft fields can differ from the API's, which drafts open sessions in the background.

Version transitions. Within contract 1, new operations, new optional parameters, new reply and catalog fields, new diagnostic codes, and new sub-operations may be added; clients ignore unknown fields and treat an unknown diagnostic code by its outcome. The browser's readers of server replies follow this rule, except that a `stored` record must equal the snapshot's own record key for key; the readers of the closed native clipboard and template formats do not and refuse unknown fields, and the page builds what it sends from the fields it knows, because the service refuses unknown request fields. Removing or renaming anything, changing a type or a default, making a parameter required, or adding values to `outcome`, `envelope`, `representation`, `execution`, `severity`, `disposition`, or `semantic_impact` needs contract 2. A service that adds contract 2 keeps answering contract 1 requests as before and lists both in `contracts`; a request for a contract it does not speak gets `unsupported_contract` with the supported list. The HTTP `v1` routes are a separate contract and are not changed by this one.

## Documents

A document is a titled drawing: one chemical graph (`m1`) and one page (`p1`) with an ordered tree of molecule instances, groups, and simple line, rectangle, or arrow graphics. Each instance draws the whole graph with one position per atom. A directed reference owned by a scene object can name another scene object, an atom, or a bond, independently of containment; it records a relationship without assigning it chemical meaning. Drawing the graph twice keeps one set of atoms and bonds and two sets of positions. Atom and bond states remain as recorded until an explicit edit; the bounded valence, aromaticity and property inspections do not edit them. Bond marks and stated configurations are set and cleared by `set_bond_mark`, `set_atom_configuration`, `set_bond_configuration` and `set_configuration_from_drawing` and shown (see below), and stereochemistry (CIP labels, derivation of a drawing from a configuration, and any check that a mark and a configuration agree) and layout are not calculated; `set_configuration_from_drawing` reads a configuration off the marks and positions only when it is sent, and then replaces the stored record of every item it processes, whatever that record is (a configuration stated by hand, `unknown` and a different label included); it writes nothing for an item it skips or does not name. A drawn mark never blocks a stated configuration when that is written, but this command reads marks, so it replaces the statement for what it processes. Several graphs and pages, reactions, and specialized scene objects remain unimplemented. Independent captions are bounded by P35a–g below.

```json
{
  "api_version": "v1",
  "document": {
    "id": "doc-1e4309eae7a8f67b",
    "title": "Ethanol",
    "revision": 1,
    "atoms": [
      { "id": "a1", "element": "C", "charge": 0, "isotope": null, "position": { "x": 0, "y": 0 },
        "kind": "element", "query": null, "hydrogens": { "mode": "specified", "count": 3 }, "radical": "none", "attachment_points": [] },
      { "id": "a2", "element": "O", "charge": -1, "isotope": 18, "position": { "x": 26, "y": 15 },
        "kind": "element", "query": null, "hydrogens": { "mode": "inferred" }, "radical": "none", "attachment_points": [1] }
    ],
    "bonds": [{ "id": "b1", "begin": "a1", "end": "a2", "order": "single", "query": null, "mark": null }],
    "stereo": { "atoms": [], "bonds": [] },
    "molecule": { "id": "m1" },
    "scene": {
      "pages": [{ "id": "p1" }],
      "instances": [
        { "id": "s1", "page": "p1", "molecule": "m1", "positions": [{ "atom": "a1", "x": 0, "y": 0 }, { "atom": "a2", "x": 26, "y": 15 }] },
        { "id": "s2", "page": "p1", "molecule": "m1", "positions": [{ "atom": "a1", "x": 40, "y": 80 }, { "atom": "a2", "x": 66, "y": 95 }] }
      ],
      "roots": ["s1", "s2"],
      "objects": [
        { "kind": "instance", "id": "s1", "parent": null, "molecule": "m1" },
        { "kind": "instance", "id": "s2", "parent": null, "molecule": "m1" }
      ],
      "references": []
    },
    "selection": { "objects": [], "atoms": [], "bonds": [] },
    "settings": { "bond_length_pt": 30, "line_width_pt": 1.2, "label_font": "sans-serif",
                  "label_weight": "normal", "label_style": "normal", "label_underline": false, "label_outline": false, "label_shadow": false,
                  "label_size_pt": 12, "carbon_labels": "default", "display_unit": "pt" }
  },
  "session": { "saved_revision": 0, "unsaved_changes": true, "undo_depth": 1, "redo_depth": 0, "draft_revision": 1, "draft_failure": null },
  "recovery": null
}
```

- `id`: 1–64 characters of `[a-z0-9-]`. Atom IDs are `a<n>`, bond IDs `b<n>`, and all scene object IDs (instances, groups, graphics, captions) share `s<n>`. Documents, the molecule (`m1`), atoms, bonds, scene objects, and the page (`p1`) are separate namespaces. IDs are stable and never reused within a document, including after delete or undo.
- `atoms` and `bonds` are the graph of `molecule`. `scene.instances` lists the molecule scene instances in order (creation order); each is on page `scene.pages[0]`, names `molecule.id`, and has one `positions` entry per atom. A new document has instance `s1` with no positions.
- `atoms[].position` is the atom's position in the first scene instance (`scene.instances[0]`, whichever that is after deletions), kept so that v1 clients written before scene instances keep working. `molecule` and `scene` were added within v1: a client reading a reply without them treats it as one drawing at `atoms[].position`.
- `scene.roots` lists page children in drawing order. `scene.objects` is depth-first in that order, each with an `id`, `kind`, and `parent` (`null` on the page). A group has ordered `children`; a graphic has `shape: line | rectangle | arrow | ellipse | polygon | spline`, exactly two `points` for the first four shapes or3–64 for polygon or4 for spline, and nullable `line_width_pt`; an instance names `molecule`. Each object belongs exactly once to the page or a group. `scene.references` holds distinct `{ "source": "s2", "target": { "kind": "object | atom | bond", "id": "..." } }` links. Their source and target must exist; they carry no chemistry. `selection` has `objects`, `atoms`, and `bonds` arrays and is the selection at the current history boundary.
- `bonds[].mark` is how the bond is drawn: `null` for a plain bond, `{ "kind": "wedge" | "hash", "narrow": "begin" | "end" }` (single bonds), `{ "kind": "wavy" }` (single bonds) or `{ "kind": "crossed" }` (double bonds). `narrow` names the bond's own `begin` or `end`; the bond is never rewritten to move the narrow end, and `begin` and `end` do not swap. A mark is a drawing fact only: it states no configuration, and `null` means plain, not "non-stereogenic" (an unspecified bond, like an empty `stereo` list, says nothing either way). `wavy` and `crossed` mean unknown or either, which is a statement and not the absence of one. The public functional model's display values (`wedge_begin`, `hash_end`) name the same facts as `kind` with `narrow`; "narrow" is Chemifree's own word for the end the public model's names refer to, and which end each public name means is not specified there, so no import or export maps them yet. Styles this model cannot represent (bold, dashed, hollow, or any other kind) are not accepted by any command and a reply naming one is refused by the web client; there is no "unsupported" or "unknown" mark.
- `stereo` is `{ "atoms": [{ "atom", "configuration" }], "bonds": [{ "bond", "configuration" }], "groups": [...] }` (`groups` is described under [Stereo groups](#stereo-groups): always present, an empty list when the document has none), centers by atom ID then double bonds by bond ID, each ascending, at most one record per atom or bond. An atom configuration is `{ "kind": "tetrahedral", "neighbors": [four slots], "turn": "anticlockwise" | "clockwise" }` or `{ "kind": "unknown", "neighbors": [four slots] }`. A slot is an atom ID, `implicit_hydrogen` or `lone_pair`; the four are distinct and in canonical order (atom IDs ascending, then `implicit_hydrogen`, then `lone_pair`), and `turn` is seen looking from the first slot toward the center (`anticlockwise` is the OpenSMILES `@`, the public model's positive `parity`; `clockwise` is `@@`). A bond configuration is `{ "kind": "relative", "begin_neighbor", "end_neighbor", "relation": "same_side" | "opposite_sides" }` or `{ "kind": "unknown" }`. `opposite_sides` is the public model's `opposite_side`; `begin_neighbor` and `end_neighbor` are the lowest-ID other neighbor of each end, or `implicit_hydrogen` where the end has none (below). A configuration is stated by `set_atom_configuration` and `set_bond_configuration`, or stored from the drawing only when `set_configuration_from_drawing` is sent: no record means "unspecified", which says nothing about whether an atom or bond is stereogenic. No operation checks a record against a mark, and a drawn mark never blocks a stated configuration when it is written; but `set_configuration_from_drawing` reads marks, and it replaces the stored record of each item it processes (a configuration stated by hand, `unknown` and a different label included) with what the drawing shows, and writes nothing for an item it skips. `turn` is this document's word for the public model's `parity` (`anticlockwise` is its positive value). The file stores the same facts under other names, see Document file format.
- `mark` and `stereo` were added within v1, and `stereo.groups` was added to `stereo` later within v1; a client reads a `stereo` without `groups` as having none. A reply from an earlier service has neither, and a client reading such a reply treats every bond as plain and the stereo lists as empty. A client written before them ignores the fields, and then draws a marked bond as a plain line; the web client refuses a reply with a mark or stereo table whose kind it does not know, or whose fields are missing, mistyped or do not fit the graph, as incompatible rather than hide it; a field it does not know is ignored (the version-transition rule above). A document in which no stereo command was used has no marks and empty lists. The three stereo commands, `notes` on a transaction reply and the `invalid_command` reasons below were added within v1; a service from before them refuses their `op` values as unknown. `set_configuration_from_drawing`, its six `stereo_not_derivable_*` reasons and the `model_id` on a transaction reply that includes it were added within v1 after them.
- `settings` is the document's style, with lengths stored in pt (see Document settings and preferences). It was added within v1; a reply from an earlier service is drawn with built-in defaults.
- Positions: points (pt), x right, y down, finite and within ±100000.
- `element`: case-sensitive IUPAC symbol, H through Og. `charge`: −8..8. `isotope`: mass number 1..999, or `null` for natural abundance.
- `kind`: `element` or `query`. A query atom stands for a set of elements and has `query`: `{ "kind": "any" }` (`*`), `any_heavy` (`A`, any atom but hydrogen), `heteroatom` (`Q`, any atom but carbon and hydrogen), `halogen` (`X`), or `{ "kind": "list", "elements": [symbol, ...], "negated": bool }` with 1–10 distinct elements in the order given (`negated`: any atom except them). For a query atom `element` holds its label (`*`, `A`, `Q`, `X`, `[N,O]`, `![N,O]`), which is not an element symbol; read `kind` first. A query atom has charge 0 and no isotope. Recording is separate from matching: `chem.substructure` matches query atoms and bonds (see Substructure matching).
- `hydrogens`: `{ "mode": "inferred" }` (the default: no stored count; supported inspection derives one separately), `{ "mode": "unknown" }`, or `{ "mode": "specified", "count": 0..16 }` (non-vertex hydrogens as stated; separately drawn H atoms are ordinary atoms and not counted here).
- `radical`: `none` (default) `| singlet | doublet | triplet | unknown`. `attachment_points`: ascending, distinct values 1–4 (default `[]`).
- `order`: `single | double | triple | aromatic | dative | query | unknown`. A dative bond points from `begin` (the donor) to `end`. A bond of order `query` has `query`: `{ "kind": "any" }` or `{ "kind": "list", "orders": [...] }`, a non-empty list without repeats of `single`, `double`, `triple`, and `aromatic`; every other bond has `"query": null`. Recording an order does not automatically interpret or change other bonds; the separate bounded aromaticity operation is described above. At most one bond joins a pair of atoms; self-bonds are rejected.
- The recorded-state fields (`kind`, `query`, `hydrogens`, `radical`, `attachment_points` on atoms, `query` on bonds) and the orders after `triple` were added within v1. A reply from a service before them leaves them all out, and a client reading such a reply uses the defaults. A client written before them may refuse a reply holding one of the new orders as incompatible; a document drawn only with single, double, and triple bonds and default states reads the same to it apart from the added fields.
- Limits: 2000 atoms, 4000 bonds, 20 scene instances (at least one), 1000 scene objects, 16 containment levels, 4096 references, title 1–200 characters without control characters (surrounding whitespace is trimmed).
- `revision` increases with every effective transaction, undo, or redo: by one, or further when a working copy continues past numbers handed out for discarded states (see No reuse). It never moves backward. It stays at most 2^53 − 1 so JSON clients read it exactly, and files beyond it are not opened. Edits, undo, and redo stop at 2^53 − 2 (`503 resource_limit`); the last number is kept so a copy reopened after a discard still gets a fresh revision. When no fresh revision remains (e.g. to follow another writer's commit after that), the document returns `resource_limit` in this process and is listed under `unreadable`.
- `session` describes the working copy held by this process. `saved_revision` is the session revision whose content is the stored commit. `draft_revision` is the revision this session's recovery draft holds, or `null` when it has none. `draft_failure` is `{ attempts, retry_in_ms, code, reason?, message }` while drafting this session keeps failing (see Autosave and recovery), and `null` otherwise. `attempts` counts every failed attempt in a row, a client's autosave included; `code` and `reason` name the latest attempt's error as the v1 error envelope does (`reason` only with `storage_failed`). The field was added within v1, and `code` and `reason` after it.
- `recovery` describes a recovery draft left by an earlier session of this document (see Autosave and recovery), or is `null`. The document list carries the same field per document.

## Edits

Transaction body: `{ "request_id": string, "expected_revision": number, "commands": [Command, ...], "selection_before"?: Selection, "selection_after"?: Selection }`, 1–500 commands. The command count is not the only bound: the body must also fit what the entry point reads, 256 KiB over HTTP (`413 payload_too_large`) and 1 MiB from the CLI (`--commands-file`, or a line of `chemifree operations`). With many scene instances each `add_atom` carries a position per instance, so 500 of them in a document with 20 instances (over 300 KiB of compact JSON) fit the CLI but not HTTP: split such an edit into several transactions, or send it through the CLI. Commands are tagged by `op`:

| `op` | Fields | Effect |
| --- | --- | --- |
| `attach_ring` | `instance`, `atom`, `direction: { dx, dy }`, `size?: 5 \| 6` | Fresh carbon ring connected by one single bond; [P34c limits and ordinary stereo clearing](#attached-carbon-ring-p34c) |
| `insert_ring` | `instance`, `center: { x, y }`, `size?: 5 \| 6` | Independent five/six-carbon single-bond regular ring (omitted size is 6) at current bond length; exactly one placement; no `$N` target; [P34a limits](#independent-six-carbon-ring-insertion-p34a) |
| `add_atom` | `element` or `query`, `position` or `positions`, optional `hydrogens`, `radical`, `attachment_points`, `bond_to: { atom, order, query? }` | New neutral, natural-abundance atom, placed in every scene instance; omitted states take their defaults |
| `add_bond` | `begin`, `end`, `order`, `query` (with order `query` only) | New bond |
| `update_atom` | `atom`, optional `element` or `query`, `charge`, `isotope`, `hydrogens`, `radical`, `attachment_points` | Omitted fields unchanged; `"isotope": null` restores natural abundance; `element` makes a query atom an element atom and `query` the reverse |
| `move_atoms` | `atoms`, `delta: { x, y }`, `instance` (optional only while there is one instance) | Each listed atom moves once, in that scene instance only |
| `update_bond` | `bond`, `order`, `query` (with order `query` only) | Changes the bond's kind |
| `delete_atoms` | `atoms` | Deletes the atoms, every incident bond, and their positions in every scene instance |
| `delete_bonds` | `bonds` | Deletes the bonds |
| `rename` | `title` | Changes the title |
| `add_instance` | `offset: { x, y }`, optional `source` | New scene instance with the positions of `source` (default: the first instance) moved by `offset` |
| `move_instance` | `instance`, `offset: { x, y }` | Every position of the instance moves once; the graph and other instances stay |
| `delete_instance` | `instance` | Deletes the instance and its positions; deleting the last one is refused |
| `add_graphic` | `shape`, two `points`, optional `parent` and `index` | Adds a two-point line, rectangle, ellipse or arrow under a group or page; polygon uses its dedicated command |
| `add_image` | `mime_type`, `data_base64`, `origin`, positive `size_pt`, optional `parent`/`index` | Adds a verified embedded static PNG; [P36k image profile](#embedded-png-image-profile-p36k-candidate) |
| `update_image_geometry` | `object`, `origin`, positive `size_pt` | Replaces only the image rectangle, retaining immutable source bytes |
| `add_spline` | exactly four `points` P0,C1,C2,P3, optional `parent` and `index` | Adds one inherited-width open cubic; [P36j bounded profile](#single-open-cubic-spline-profile-p36j-candidate) |
| `update_spline_points` | `object`, exactly four `points` | Replaces the whole cubic definition retaining identity/width/containment/refs; other shapes refuse |
| `add_polygon` | `vertices` (3–64), optional `parent` and `index` | Adds a closed stroke-only polygon with inherited width; [P36i bounds and budget](#bounded-closed-polygon-profile-p36i-candidate) |
| `update_polygon_vertices` | `object`, `vertices` (3–64) | Replaces the whole polygon list while retaining identity/width/containment/references; old two-point shapes refuse |
| `set_graphic_line_width` | `object`, required `line_width_pt`: finite 0.1–10 or null | Sets only a graphic stroke override or dynamic document inheritance; [P36d semantics](#per-graphic-line-width-override-p36d-candidate) |
| `update_graphic_points` | `object`, exactly two `points` | Replaces an existing graphic's two stored points while retaining shape, containment and references; [P36a bounds and atomicity](#existing-graphic-point-editing-p36a-candidate) |
| `group` | `objects` | Groups at least two distinct siblings, preserving their order; the group occupies the first selected sibling's position |
| `ungroup` | `group` | Removes the group, promoting its children in order |
| `reorder` | `object`, `index` | Moves an object within its existing parent's siblings; [P36g First/Last queue semantics](#same-parent-scene-order-boundaries-p36g-candidate) |
| `delete_objects` | `objects`, `mode: subtree | object_only` | Deletes subtrees or only the named objects, promoting surviving children; at least one instance remains |
| `transform_selection` | `selection`, `pivot: { x, y }`, `transform`, optional `instance` | Geometry-only checked rotation/reflection of the effective union; [axes, angles and rectangle limits](#selection-rotation-and-reflection-p32c) |
| `arrange_objects` | `objects`, `mode` | Rigid alignment/distribution of outermost selected scene units; [modes and bounds](#scene-object-alignment-and-distribution-p32b) |
| `move_selection` | `selection`, `delta: { x, y }`, optional `instance` | Moves the effective union of selected subtrees, atoms and bond endpoints once; [scope and atomicity](#effective-selection-scope-and-movement-p32a) |
| `move_objects` | `objects`, `delta: { x, y }` | Moves graphics and instance positions in the selected subtrees once, even when an ancestor and child are both named |
| `add_reference`, `remove_reference` | `source`, `target: { kind, id }` | Add or remove a directed reference; no chemical semantics are inferred |
| `update_settings` | optional `bond_length_pt`, `line_width_pt`, `label_font`, `label_weight`, `label_style`, `label_underline`, `label_outline`, `label_shadow`, `label_size_pt`, `carbon_labels`, `display_unit` | Changes the named document settings; omitted fields remain unchanged |
| `set_bond_mark` | `bond` (an ID or `$N`), `mark`: `null` or `{ kind: "wedge" \| "hash", narrow: "begin" \| "end" }`, `{ kind: "wavy" }`, `{ kind: "crossed" }` | Sets or clears how the bond is drawn; `null` clears. Both keys are required |
| `set_atom_configuration` | `atom` (an ID or `$N`), `configuration`: `null`, `{ kind: "tetrahedral", neighbors: [four slots], turn }` or `{ kind: "unknown", neighbors: [four slots] }` | States or clears a stereocenter's configuration; neighbors may be listed in any order |
| `set_bond_configuration` | `bond` (an ID or `$N`), `configuration`: `null`, `{ kind: "relative", begin_neighbor, end_neighbor, relation: "same_side" \| "opposite_sides" }` or `{ kind: "unknown" }` | States or clears a double bond's configuration |
| `set_configuration_from_drawing` | optional `atoms` and `bonds` (each a non-empty list of IDs or `$N`), `instance` (optional only while there is one instance) | Stores the configuration the bond marks and that instance's positions show, or refuses with a reason; see [Deriving a configuration from a drawing](#deriving-a-configuration-from-a-drawing) |
| `create_stereo_group` | `kind` (`absolute`, `and` or `or`), `atoms` (non-empty; IDs or `$N`), `number` (an integer or `null`, required) | New enhanced stereo group `eN` over stored tetrahedral centers that are in no group; see [Stereo groups](#stereo-groups) |
| `delete_stereo_group` | `group` (`eN` or `$N`) | Removes the group; its members keep their configurations and become ungrouped |
| `set_stereo_group_kind` | `group`, `kind` | Changes the kind |
| `set_stereo_group_number` | `group`, `number` (an integer or `null`, required) | Sets or clears the stored number |
| `add_stereo_group_members` | `group`, `atoms` (non-empty) | Adds centers that are in no group; centers already in the group are left as they are |
| `remove_stereo_group_members` | `group`, `atoms` (non-empty) | Removes members, which become ungrouped; it never empties the group |
| `merge_stereo_groups` | `into`, `from` | Moves every member of `from` into `into` and deletes `from`; `into` keeps its kind and number |
| `split_stereo_group` | `group`, `atoms` (non-empty, some but not all members), `number` (an integer or `null`, required) | Moves the listed members of an `and` or `or` group into a new group of the same kind with that number |

Selection fields have arrays `objects`, `atoms`, and `bonds`. `selection_before` is the caller's snapshot at the edit; if omitted the document's stored selection is used. `selection_after` is validated against the result and may name a scene object created in this transaction as `$N` (the command's zero-based index); if omitted, the before selection is filtered to entities that still exist. Selections alone do not create a revision or history entry. Undo and redo restore each entry's selection along with graph, scene and references. The CLI `documents apply` accepts JSON through `--selection-before` and `--selection-after`; operation contract 1 accepts the same optional fields in its transaction parameters.

A deletion removes references owned by deleted objects. If a surviving source still refers to an object, atom, or incident bond being deleted, the command is rejected with `reference_in_use`; remove those links first, including in the same transaction. `ungroup` applies the same rule to the removed group. Unknown links or targets produce `reference_unknown`, and exceeding the link limit produces `reference_limit`. Other scene-specific reasons are `object_unknown`, `object_kind_mismatch`, `group_mixed_parents`, `depth_limit`, `object_limit`, `index_out_of_range`, `last_instance`, `delete_mode_missing`, and `selection_unknown`. A malformed selection is an `invalid_command` without a command index.

Creating an atom states its position in every existing scene instance. `position: { x, y }` is accepted while the document has exactly one instance; otherwise `positions` is required: an object keyed by instance ID (`{ "s1": { x, y }, "s2": { x, y } }`) naming exactly the current instances. A missing or unknown instance, both fields, or neither is `invalid_command` with the command's index, and the transaction changes nothing. A key given twice in `positions` makes the command body malformed: like any other malformed body it is `invalid_request` (the message names the key) without a command index, and nothing changes. How a client chooses positions for instances the user did not click is the client's policy. The web editor keeps each new atom where it belongs in every instance: an atom bonded to an existing atom (`bond_to`) is shifted by how far each instance has moved that atom; an atom bonded to an earlier new atom of the same transaction (`$N`) keeps its offset from the position chosen for that atom in each instance; only an unbonded first atom is shifted by how far each instance has moved the atom nearest the click. Bond lengths and directions are therefore the same in every instance. The scene instance commands, `positions`, and `move_atoms.instance` were added within v1.

Recorded states are validated on the atom or bond as the command leaves it, with the same rules for edits, loaded files, recovered drafts, and history. A refused combination is `invalid_command` with the command's index and a stable `reason`, and the transaction changes nothing:

| `reason` | Refused |
| --- | --- |
| `hydrogen_count_out_of_range` | a stated hydrogen count above 16 |
| `query_atom_charge` | a query atom with a non-zero charge (also when `update_atom` sets a charge on a query atom, or a query on a charged atom without `"charge": 0`) |
| `query_atom_isotope` | a query atom with an isotope |
| `query_list_invalid` | a query element list that is empty, longer than 10, or repeats an element |
| `bond_query_invalid` | a query bond list that is empty or repeats a kind |
| `bond_query_missing` | order `query` without `query` |
| `bond_query_unexpected` | `query` with any other order |
| `attachment_points_invalid` | attachment points outside 1–4, repeated, or not ascending |
| `atom_kind_conflict` | an atom named by both `element` and `query`, or an `add_atom` naming neither |

A value of the wrong type or outside its vocabulary (an unknown radical or query kind, `"mode": "inferred"` with a count, a negative count, a dative bond in a query bond list) makes the body malformed: `invalid_request` without a command index or `reason`, like any other malformed body. The state fields, the new orders, and `reason` with `invalid_command` were added within v1. Undo and redo cover every state.

Where a command names an atom for a new bond (`bond_to.atom`, `begin`, `end`), `"$N"` refers to the atom added by the `add_atom` command at index N earlier in the same transaction. This lets one transaction add two atoms and bond them. The same `"$N"` works in the stereo commands: where one names an atom (`set_atom_configuration.atom`, a neighbor slot, `begin_neighbor`, `end_neighbor`) it is the atom command N added; where it names a bond (`set_bond_mark.bond`, `set_bond_configuration.bond`) it is the bond command N created, which is the bond of an `add_bond` or the `bond_to` bond of an `add_atom`. A `$N` in a bond position whose command created no bond is `invalid_command` with the command's index and no `reason`. `set_configuration_from_drawing` takes `$N` in the same way: `atoms` accepts the atoms commands created and `bonds` the bonds commands created, and the command reads the document as it is at its place in the transaction. In the stereo group commands a group is `eN` or `"$N"`, the group the `create_stereo_group` or `split_stereo_group` command at index N made earlier in the same transaction, and an atom is `aN` or `"$N"` as above; a `$N` that names no earlier command of the right kind is `invalid_command` with the command's index and no `reason`.

### Stereo marks and configurations

Two kinds of statement are stored apart and set by separate commands: how a bond is drawn (`set_bond_mark`) and what is stated about a stereocenter or double bond (`set_atom_configuration`, `set_bond_configuration`). Neither of these three commands reads the other's data: a wedge may sit on a bond of a center whose stated turn disagrees with it, and setting or clearing one with its own command never touches the other. Two things read marks and positions. `set_configuration_from_drawing` does, only when it is sent (see [Deriving a configuration from a drawing](#deriving-a-configuration-from-a-drawing)); it does write the statement: for each item it processes it replaces the stored record, whatever it is (a stated turn or relation, `unknown`, a different label), with what the marks and positions show, so a stated configuration that disagrees with the drawing is overwritten, not reported; for an item it skips or does not name it writes nothing. The read-only operation `chem.stereo` compares what is stored with what each scene instance's drawing shows and reports the difference without editing or storing anything (see [Stereo inspection](#stereo-inspection)). Nothing derives a drawing from a configuration, labels anything (no CIP), or tells whether an atom or bond is stereogenic. Enhanced stereo groups are a third kind of statement, kept beside the configurations and set by their own commands (see [Stereo groups](#stereo-groups)).

- **`null` clears, and the key is required.** `mark` and `configuration` must be present: a command without the key is a malformed body (`invalid_request`, no command index), never a clear. Clearing something that is not set, or setting the value already stored, changes nothing (no new revision).
- **Mark.** `wedge` and `hash` need a single bond and carry `narrow`, `begin` or `end`: the end of the bond's own `begin`/`end` that is narrow. It is recorded in the mark and the bond's `begin` and `end` are never swapped. `wavy` needs a single bond and `crossed` a double bond, and neither has `narrow`. A bond of any other order (aromatic, dative, query, unknown, triple) carries no mark.
- **Center.** An element atom (not a query atom) with three or four bonded neighbors. `neighbors` names the bonded atoms exactly once each, plus, for a center with three bonds, one `implicit_hydrogen` or `lone_pair`. A slot is an atom ID, a `$N` for an atom created earlier in the transaction, `implicit_hydrogen` or `lone_pair`. The slots may be listed in any order; the snapshot always shows the canonical order (atom IDs ascending, then `implicit_hydrogen`, then `lone_pair`) and the `turn` that describes it, so listing the same slots in an odd permutation of the canonical order flips the stored `turn` and an even permutation keeps it. `turn` is seen looking from the first slot toward the center.
- **Double bond.** `begin_neighbor` is a position of the bond's `begin` atom and `end_neighbor` a position of its `end` atom. An end has two positions besides the double bond: with two other atoms, either may be named; with one other atom, that atom or `implicit_hydrogen` (or `lone_pair`), the position opposite it; with none, `implicit_hydrogen` or `lone_pair`. The stored form names the lowest-ID atom of each end, or `implicit_hydrogen` where it has none, and `relation` is flipped once for each end where the named position was the other one. `unknown` is a statement that the geometry is not known, which differs from having no record. A double bond with a cumulated double or triple bond at an end cannot hold either form.
- **Refusals.** A stereo command or edit that cannot hold is `invalid_command` with the command's index and one of these stable `reason`s, and the transaction changes nothing:

| `reason` | Refused |
| --- | --- |
| `stereo_mark_bond_order` | a wedge, hash or wavy mark on a bond that is not single, a crossed mark on a bond that is not double, including an `update_bond` that would leave a mark on a bond that cannot carry it |
| `stereo_center_unsupported` | a configuration on a query atom or on an atom with fewer than three or more than four bonded neighbors, including an `update_atom` that would make a configured center a query atom |
| `stereo_center_neighbors` | a neighbor list that is not exactly the center's bonded atoms plus the one non-atom slot, or that repeats a slot |
| `stereo_double_bond_required` | a bond configuration on a bond that is not double, including an `update_bond` that would make a configured double bond something else |
| `stereo_reference_invalid` | a double-bond reference that is the partner, not a neighbor of its end, an implicit hydrogen or lone pair where the end has two other atoms, or on an end with three or more other atoms |
| `stereo_geometry_unsupported` | a double bond with a cumulated double or triple bond at an end, including an `update_bond` that would make a neighbor bond of a configured double bond double or triple |
| `stereo_not_derivable_no_marks` | `set_configuration_from_drawing`: no wedge or hash has its narrow end at the named center and no wavy bond touches it, so the drawing states nothing |
| `stereo_not_derivable_conflicting_marks` | `set_configuration_from_drawing`: a wavy bond that cannot be read for this center together with a wedge or hash of the center's own |
| `stereo_not_derivable_degenerate` | `set_configuration_from_drawing`: the four directions of a center are flat (`\|D\|` below 0.001), or a double bond's reference lies on its axis (sine below 0.001) |
| `stereo_not_derivable_coincident_atoms` | `set_configuration_from_drawing`: a neighbor or reference is closer than 0.001 pt to its atom, or a double bond is that short |
| `stereo_not_derivable_fourth_position` | `set_configuration_from_drawing`: a center with three neighbors whose fourth position cannot be named, or a center with four neighbors whose stated hydrogens are above zero |
| `stereo_not_derivable_no_reference` | `set_configuration_from_drawing`: an end of the double bond has no other neighbor to read a direction from |
| `stereo_record_invalid` | a stored record that is not in canonical form or does not match its key. This is a problem of stored data found on load or restore, not of a command, so no command produces it; it is not a disagreement between a stored configuration and a drawn mark, which is a finding about a valid document that `chem.stereo` reports as `drawing_disagrees` |

- **What clears and what refuses.** An edit that changes the *neighbors* of an atom (adding, deleting or reconnecting a bond, deleting a neighbor atom) clears the stated configuration of that atom and of any double bond that ends at it, and deleting an atom or bond clears its records; the edit succeeds and the reply says what was cleared (below), and undo restores it. An edit that would change what a record *says about a bond that stays* is refused with the reason of the rule it breaks (`update_bond` on a marked or configured bond, a neighbor bond that would be cumulated, `update_atom` to a query atom): clear the record first, in the same transaction if you want. A mark goes with its bond.
- **`notes`.** Every `document.transaction` reply (the HTTP and CLI transaction replies and the operation contract's `result`) carries `notes`: a list, empty when nothing was cleared, of entries shaped like `loss_report` entries, one per stereo configuration the cascade cleared, in key order (centers by atom ID, then double bonds by bond ID): `feature` `stereo_configuration`, `source_path` `atoms[a1]` or `bonds[b4]`, `disposition` `dropped`, `semantic_impact` `stereo`, no consent, and a `reason` that says why. The records of a deleted atom or bond are listed. A record the transaction cleared itself with `null`, one created and cleared inside the transaction, and one cleared and set again in the same transaction are not listed. Undo and redo restore exactly the recorded content: they never produce `notes` (their replies carry `notes: []` and are `ok`), and a redo that removes a record again is the same removal the user already saw in the reply of the edit. `get` replies carry `notes: []` too. The `reason` of a note says why the record went and that undoing the edit brings it back while the edit is still in the history, which is bounded. In operation contract 1 the outcome is `partial` exactly when `notes` is not empty, and `loss_report` repeats the entries; the document is changed either way. A repeated `request_id` replays the same notes. The web editor shows them as text. A record that a stereo gesture of the editor replaces or removes itself is not in `notes`; the editor's status line tells it (see Web editor).

A transaction applies all of its commands or none. Commands that leave the document identical (e.g. moving by zero) succeed without a new revision or undo entry. Unknown fields and unknown `op` values are rejected.

**How the web editor draws and edits recorded states** (client policy, not a core rule). Each atom has one label layout: its symbol (or a query label, a list showing at most three elements and then "…"), stated or unknown hydrogens with their count as a subscript (`?` when unknown; inferred hydrogens add nothing), the isotope and charge, radical dots above (one for doublet, two for singlet, two rings for triplet, a dot and `?` for unknown), and attachment points below (`ap1,3`). Stated hydrogens go on the left of the symbol (`H3C`) when the atom's bonds lean right. Text is drawn at its natural width, never stretched or squeezed: the symbol and what precedes and follows it are one run of text, drawn against each other, placed so that the symbol's reserved width is centred on the atom (the symbol is then off centre by half the difference between the reserved and natural widths of what is beside it, a fraction of a point). The reserved widths are upper bounds of each character's advance in the bundled font (Geist) and the generic sans-serif fallback, measured in Chromium, so the drawn text lies inside the label's box; a font other than these could draw wider. One box encloses all of it; bond trimming, the canvas view box, and hit testing use that box. An atom is hit within 9 pt of its position or anywhere in its label box. A bond stops 2 pt outside the box; when two boxes leave less than 3 pt, a 3 pt stroke stays in the middle of the room there is (of the bond when there is none), so a bond is never drawn reversed or left out. A bonded carbon is a bare vertex only while it records nothing (no charge, isotope, stated or unknown hydrogens, radical, or attachment points). Bond kinds look different and have accessible names: aromatic is a line with a dashed line beside it, shortened at bare vertices: for a bond in a ring of at most 8 atoms, on the side of the centre of the smallest such ring through it (either ring where rings are fused), so it is inside a ring however the ring's bonds were drawn and whatever is bonded to them; for any other bond, on the side where more of the atoms bonded to its ends lie; dative a line with an arrowhead at its end; query dotted; unknown a zigzag. With several placements, each is framed with its ID, the view box holds every frame, and accessible names say which placement they are in (`Atom a1 in s2: O`). The bond tool draws every bond from where the drag starts, so a dative bond's donor is the atom or point the drag started from: from empty space onto an atom it adds the atom and then the bond from it, and over an existing bond drawn the other way it replaces that bond in the same transaction (the bond gets a new ID). The full query expression is in the inspector and the accessible name. Every inspector edit is built from the latest document when its request runs, after all queued before it, and is not sent when it no longer applies: setting a query clears the charge and isotope, a charge or isotope press on what has become a query atom sends nothing, and a typed query list and its negation each keep the other, so quick presses in any order are never refused.

Undo, redo, and save bodies: `{ "request_id", "expected_revision" }`. Undo reverts the latest transaction as a new revision; redo reapplies it; a new transaction after undo discards the redo branch. A history entry records the graph, scene, references, settings, title, and before/after selection changes together. History keeps the latest 100 entries and at most 100,000 changes and selected IDs in total (undo and redo together), dropping the oldest, and is saved with the document, so it survives reopening.

### Deriving a configuration from a drawing

`set_configuration_from_drawing` stores, for the items it takes, the configuration that the bond marks and the positions of one scene instance show. Everything in this section is a Chemifree choice, not a rule of the original application: the public functional specification states no frame and no derivation rule. The rules are anchored by tests: hand-computed drawings, the OpenSMILES examples, a seeded property test against the OpenSMILES definition, the public authored drawing of `F[C@H](Cl)[C@H](Br)I` (the narrow-end reading below is pinned by that drawing, not by a sentence of the specification), and the specification's positive and negative parity pair on `[F, Cl, Br, implicit_hydrogen]`. The rules are named `chemifree-stereo-1`. Only wedge, hash, wavy and crossed marks are read. The public specification declares a preference of the original, `DisablePerspectiveStereoPerception`, and states no meaning for it; the editor does not read it and nothing like it is implemented. Nothing is derived unless the command is sent, and the web editor sends it only inside a stereo-mark gesture it draws (see Web editor), never by itself and never for a mark that was not drawn there.

- **Items.** `atoms` and `bonds` name centers and double bonds (IDs, or `$N` for those created earlier in the transaction); a list that is present must not be empty (`invalid_command` with the index and no `reason`, as for `move_atoms`) and repeats are taken once. With neither list, the items are those a mark bears on: each atom that is the narrow end of a wedge or hash or an end of a wavy bond and could hold a center record (an element atom with three or four neighbors), and each double bond with a crossed mark. An unmarked center or double bond is never stored unless named, and a cleared mark takes its center out of the scope. With `atoms` and/or `bonds`, exactly the named items are derived and every one must derive; naming only one kind derives no item of the other. Every item the command processes has its stored record replaced (see Stored, not judged); an unmarked center or double bond that is not named keeps whatever is stored for it, a configuration stated by hand included. A list goes through the same de-duplication as `move_atoms`: repeats, also two references that name one atom (`"a1"` and `"$0"`), are one item and merge silently, and the items are processed centers first and then double bonds, each by ascending identifier, so when several would fail the refusal names the lowest identifier, not the first in the list.
- **Skip or refuse.** Without a list, an item the model could never hold a record for is skipped without a note, and the reply does not list what was skipped (`chem.stereo` reports a wedge or hash whose narrow end cannot be a center as `mark_narrow_end_not_a_center`, and a crossed double bond the model cannot hold as `drawing_not_derivable`). A mark whose narrow end is a terminal atom or another atom that cannot be a center (a query atom, five neighbors) is never in the scope. A crossed double bond is skipped whenever it is structurally impossible, which in this scope has two causes: a cumulated double or triple bond at an end (`stereo_geometry_unsupported`) and an end with more than two other neighbors (`stereo_reference_invalid`), at either end. `stereo_center_unsupported` and `stereo_double_bond_required` cannot occur without a list, because the scope holds only possible centers and double bonds. Any other item the drawing does not determine refuses the whole command with its reason (a conflict, a flat picture, an unnameable fourth position). With a list, an item the model cannot hold keeps the existing structural reason (`stereo_center_unsupported`, `stereo_double_bond_required`, `stereo_geometry_unsupported`, `stereo_reference_invalid`) and one the drawing does not determine gets the `stereo_not_derivable_*` reason. The message names the item (`atom a1: ...`, `bond b4: ...`) and the refusal carries the command's index; the transaction changes nothing, so an item derived before the failing one is not stored either.
- **Which instance.** The command reads the positions of one scene instance and no other. `instance` may be left out only while the document has exactly one (`invalid_command` with the index and no `reason` otherwise, as for `move_atoms`); an unknown instance is `object_unknown`. Marks belong to the graph, so different instances can show different configurations; the command stores the one of the instance it was given, over what was stored, and changes no position or mark.
- **Frame and sign.** Document y points down. The derivation negates y once and nowhere else: a neighbor at unit direction `(ux, uy)` from the center becomes `v = (ux, -uy, z)`, with `z = +1` for a wedge whose narrow end is the center, `-1` for a hash whose narrow end is the center and `0` for every other bond. With the slots `s0..s3` in canonical order, `D = det[v1 - v0, v2 - v0, v3 - v0]`: `D < 0` is `anticlockwise` (the OpenSMILES `@` and the public model's positive parity, whose determinant `det(p0 - p3, p1 - p3, p2 - p3)` is exactly `-D`) and `D > 0` is `clockwise` (`@@`). With three neighbors the fourth vector is minus the sum of the other three. Scaling every lift by a positive number scales `D` and never changes its sign. A wedge or hash bears on the atom at its narrow end only; at the wide end it says nothing about that atom. A picture with two or more lifted bonds whose four vectors do not surround the center is decided by the orientation of the lifted points, the public model's definition of parity, and other programs may read such a picture differently; that is a limit, not a guarantee.
- **Wavy and crossed.** A wavy bond at a center, with no wedge or hash narrow at that center, makes the center `unknown`. An end that has its own wedge or hash follows its own mark and does not read a wavy bond to another possible center. A wavy bond to an atom that could not hold a center record, beside a wedge or hash of the center's own, is `stereo_not_derivable_conflicting_marks`. A crossed double bond is `unknown` without reading any position. A wavy single bond beside a double bond (the usual "either E or Z" marker) is not read as an unknown double bond: that is a deliberate limit, and only a crossed mark is. A mark that does not fit its bond's order counts as no mark. The public display values `Hash`, `Dash`, `Bold` and `HollowWedge` have no narrow end and are not representable as a narrow-end hash; they are not read and are for the importers to refuse or report.
- **Fourth position.** A center with three neighbors needs its fourth position named, and the geometry does not depend on the name: `implicit_hydrogen` or `lone_pair`. The atom must be an element atom with no radical and known hydrogens, with every bond a single, double or triple bond, and the valence model (`chemifree-valence-3`) must accept the atom with the hydrogen count it states or infers. A count of 1 gives `implicit_hydrogen`, unless a bond is multiple or the bond sum plus the hydrogen exceeds the lowest valence the model lists for the element and charge. A count of 0 gives `lone_pair` for a neutral N or P with a bond sum of 3 and a neutral S with a bond sum of 4 (the sulfoxide), and is refused for every other element and every charged atom. A count of 2 or more, an atom unknown to the valence model, and any other case are `stereo_not_derivable_fourth_position`; state the configuration by hand instead. A center with four neighbors has no fourth position; stated hydrogens above zero would be a fifth position and are refused with the same reason. The rule reads the valence model, so a new valence model can change what is named and is a new `chemifree-stereo-N`. The CDX `BondOrdering` zero entry is the record's own non-atom slot, not a hydrogen: which label that slot has is decided by this rule or by the record, never by the zero.
- **Double bonds.** The references are the lowest-ID other neighbor of each end, the canonical form of a stored record. With the axis from the begin atom to the end atom and each reference vector taken from its own end, references on the same side of the axis are `same_side` and on opposite sides `opposite_sides`; handedness never matters, so there is no y flip. An end with no other neighbor is `stereo_not_derivable_no_reference` (unless the bond is crossed), a reference on the axis is `stereo_not_derivable_degenerate`, and a zero-length axis or reference is `stereo_not_derivable_coincident_atoms`. An end with two other neighbors uses the lower ID even when both lie on one side. A query atom as an end is not special.
- **Order of checks and constants.** For a center: the structure (a query atom or fewer than three or more than four neighbors is `stereo_center_unsupported`), then `no_marks`, `conflicting_marks`, `coincident_atoms` and `fourth_position` in that order, and `degenerate` last (an `unknown` center reads no geometry, so it cannot be coincident or degenerate). A neighbor closer than 0.001 pt is coincident, `|D|` below 0.001 is degenerate, and a double-bond reference whose bond makes a sine below 0.001 with the axis is degenerate. A nearly straight pair of plain bonds with one wedge gives `|D| = 4 * epsilon` for a tilt of `epsilon` radians, so a drawing flat to within about 0.014 degrees is refused as flat and one tilted a little more is answered with confidence. Only IEEE-754 basic operations and square roots are used, so a result cannot differ between platforms.
- **Stored, not judged.** The command stores through the ordinary stereo table, so storing what is already stored changes nothing (no revision, no history entry, no `notes`), a different stored state is replaced, and so is a configuration stated by hand: every item the command processes has its stored record replaced by what the drawing shows, including a stated `unknown`, a stated turn or relation, and another label, and a mark that disagrees with the old statement does not stop it. An item it skips or does not name is not written, and a refused command writes nothing. A record removed by the neighbor cascade earlier in the same transaction and stored again is not listed as cleared. `[set_bond_mark, set_configuration_from_drawing]` is one history entry, and undo restores the earlier record or its absence together with the mark. The command does not judge whether an atom or bond is stereogenic (a carbon with two equal substituents can get a record), computes no CIP label, and itself neither detects nor reports a disagreement between a stated configuration and a drawing: it overwrites the statement of each item it processes. `chem.stereo` is the operation that compares them, and it can be asked before or after. How the original application resolves a descriptor that conflicts with a depiction is not defined by the public specification.
- **Operation reply.** A `document.transaction` operation reply whose request includes the command carries `model_id` `chemifree-stereo-1`, also when the transaction changes nothing or is refused, but not when the parameters themselves are malformed; a request without the command carries none. The plain HTTP transaction reply has no model field.

Example (the document holds C `a1` with F `a2` above, Cl `a3` lower right and Br `a4` lower left and no mark; `b2` joins `a1` and `a3`):

```json
{ "request_id": "r1", "expected_revision": 1, "commands": [
  { "op": "set_bond_mark", "bond": "b2", "mark": { "kind": "wedge", "narrow": "begin" } },
  { "op": "set_configuration_from_drawing", "atoms": ["a1"] } ] }
```

The reply has `document.revision` 2 and `document.stereo.atoms` `[{ "atom": "a1", "configuration": { "kind": "tetrahedral", "neighbors": ["a2", "a3", "a4", "implicit_hydrogen"], "turn": "clockwise" } }]`. Sending only the second command again returns revision 2 unchanged. With no mark on the document, `{ "op": "set_configuration_from_drawing", "atoms": ["a1"] }` as the only command is refused:

```json
{ "api_version": "v1", "code": "invalid_command",
  "message": "command 0: atom a1: no wedge, hash, or wavy mark bears on this center",
  "command_index": 0, "reason": "stereo_not_derivable_no_marks" }
```

**Rules for a future mirror or flip command.** No such command exists, and nothing here moves a position or rewrites a mark. When one is added it must follow the public `Flip(vertical, preservingAbsoluteStereochemistry)` operation as these rules read it. With the flag false the picture is reflected and the compound becomes its mirror image: marks keep their kind and narrow end, every tetrahedral `turn` reverses (`StereoTable::mirrored`), and `unknown` centers and every double-bond record stay, because a cis or trans relation does not depend on handedness. With the flag true the same compound is seen from behind: wedge and hash swap kinds with their narrow ends unchanged (`StereoMark::flipped_over`), wavy and crossed stay, and the stored configurations stay. A horizontal and a vertical reflection obey the same rule. How the original rewrites its wedges for the true flag is not part of the public specification; that rule is derived here and pinned by tests (derive after reflecting, with the marks swapped, equals the derivation before). A command that reflects positions without applying one of these leaves a stored configuration and a drawing that disagree. A codec that reads a neighbor order (the CDX `BondOrdering` of bond IDs with a zero entry) maps each entry to a slot and asks `CenterState::turn_for_order` for the turn.

### Stereo groups

An enhanced stereo group states which stored tetrahedral centers belong together. It is a statement about membership, never a claim about composition, and nothing here judges that the members are stereogenic or related. Chemifree's eight group commands are its own product surface over this data; the public specifications declare no such commands.

**Kinds.** `absolute`: the members are as drawn. `and`: the author means a mixture of the drawn set of members and its all-inverted counterpart. `or`: the members invert together or not at all. No ratio is stated or implied for any kind (in particular `and` is not "1:1"), and no counts are kept. Not modelled: a chiral flag on a molecule, whole-molecule relative or racemic flags, reaction stereo, double bonds as members, and an explicit "no statement" state (a center in no group is simply ungrouped).

**Snapshot.** `document.stereo.groups` is a list in group ID order, always present, empty when the document has none: `{ "id": "e1", "kind": "absolute" | "and" | "or", "stored_number": integer | null, "number": integer | null, "label": string, "members": ["a1", ...] }`, members ascending, each an atom with a tetrahedral configuration in `stereo.atoms`. `holds_stereo` is unchanged (a group needs a stored configuration, so it is already true). The list is additive within contract 1.

**Number, stored and interpreted.** `stored_number` is the number as it was stated: any integer from -2147483648 to 2147483647, `null` when none. `0`, negative numbers and `null` are kept as stated and have no further meaning in Chemifree. `number` is the interpreted number: `null` for `absolute` (an absolute group has no number), otherwise the stored number, or `0` when none is stored. `label` is the display text built from kind and interpreted number: `abs`, `and2`, `or0`, `or-3`; it is never stored. A group is told from another of the same kind by its interpreted number alone, so a group with no number and a group numbered `0` stand for the same group and cannot both exist (`stereo_group_number_taken`); `or1` and `and1` are two groups; the same kind and number stated twice is refused and points to `merge_stereo_groups`. This choice of `0` for none is a product definition, not a claim about any file format. Identity is the group ID (`e1`, `e2`, ..., never reused, also not after a delete and undo); renumbering and relabelling change nothing else.

**Commands.** Every key listed in the Edits table is required, `null` included (`number` is `null` for none); an unknown key, a missing key, `kind` other than `absolute`, `and` or `or` (`abs` is not a kind), or a `number` that is not an integer is `invalid_request`, as is an integer that does not fit 64 bits. An integer that fits 64 bits but not 32 is `invalid_command` with `reason` `stereo_group_number_range` and the command's index. The commands run in order against the transaction's state; the transaction is all or nothing and the cap of 500 commands is unchanged. Setting what is already set, or adding a member the group has, changes nothing and makes no revision.

**Refusals.** An `invalid_command` refusal carries `command_index` and one `reason`:

| `reason` | Refused |
| --- | --- |
| `stereo_group_unknown` | no group has this ID |
| `stereo_group_empty` | a command naming no atoms, a group that would have no members (remove every member: delete the group instead), a split that moves none or all members |
| `stereo_group_member_not_center` | an atom that does not exist, has no configuration or is stated `unknown` |
| `stereo_group_member_taken` | an atom that is already in a group |
| `stereo_group_member_absent` | an atom that is not a member of the group (remove, split) |
| `stereo_group_number_not_allowed` | an `absolute` group with a number (also `set_stereo_group_kind` to `absolute` while a number is stored: clear it first) |
| `stereo_group_number_taken` | another group of the kind has this interpreted number |
| `stereo_group_abs_exists` | the document already has an absolute group (a product rule: one absolute group for a document; whether the unit should be the molecule is not measured) |
| `stereo_group_same` | `merge_stereo_groups` of a group into itself |
| `stereo_group_kind_mismatch` | `merge_stereo_groups` of groups of different kinds |
| `stereo_group_split_abs` | `split_stereo_group` of the absolute group (remove members instead) |
| `stereo_group_number_range` | a number outside the 32-bit signed range (decided by the wire; the domain never sees it) |

An ID of the wrong kind where an atom is expected (a bond or a group ID) is `invalid_command` with the command's index and no `reason`, like any malformed identifier of the other commands; it is not `stereo_group_member_not_center`.

**Invariants.** Every member is a stored tetrahedral center; a group has at least one member; an atom is in at most one group; an `absolute` group has no number and there is at most one; per kind (`and`, `or`) no two groups have the same interpreted number. A group may have one member. They are checked on every edit, load, recovery and history replay. Groups cannot outnumber the stored centers, which a document of at most 2,000 atoms bounds.

**Cascade.** A member that stops being a stored tetrahedral center (its configuration cleared with `null`, stated `unknown`, lost with a neighbor change, or lost with its atom) leaves its group, and a group left without members is removed. This happens once, at the end of the transaction, so a center cleared and stated again in one transaction keeps its group; until then a group command sees the members as they were stated. Undo and redo restore membership with the same group IDs.

**Notes.** A transaction that changes what a group states reports it in `notes` (repeated in `loss_report`), in the shape of the other stereo notes (`semantic_impact` `stereo`, no consent), after the `stereo_configuration` notes and in this order: for each group that existed before, in ID order, its members that left and then the group itself when it was removed; then relations lost or changed; then merges. The rule is applied by atom to the groups that existed before the transaction: a note is made when a relation among at least two members of one group no longer holds, or says something else, at the end of the transaction. Features: `stereo_group_member` (`atoms[a4]`, `dropped`: a member left because it stopped being a stored center; the reason says when the relation to the other members is no longer stated), `stereo_group` (`groups[e1]`, `dropped`: every member left), `stereo_group_relation` (`groups[e1]`: surviving members of an `and` or `or` group are no longer all in one group (`dropped`: split, members removed, group deleted or members moved elsewhere), are in a group of another kind (`normalized`, or `dropped` when they went into an `absolute` group), or members of two or more groups of one kind now belong together (`normalized` when all stayed together, otherwise `dropped`; the reason names a number that is not kept and a kind that changed)). Not reported: renumbering, adding members, a group made and changed in the same transaction, and the deletion or change of an `absolute` group or of a group with one member (nothing relates). A reply that carries a note is `partial`, as for the other stereo notes; undo and redo carry none. These are Chemifree's rules for when a meaning changed; the public specifications fix none.

**Exports and other operations.** Recorded SMILES writes each supported group as CXSMILES; an unwritable group is omitted whole with a consent-required `stereo_group` loss. Canonical SMILES requires consent to drop groups. A V2000 export also needs consent to drop groups (`stereo_group`); a V3000 export writes them as `COLLECTION` entries (see "Enhanced stereo collections"). CDXML and CDX read and write supported groups under the bounded drawing profile above. `chem.cip`, `chem.identity` (the canonical code, `equivalent` and mappings), `chem.substructure` and `chem.validate` do not read groups, so a document with and one without give the same results; `chem.stereo` adds the info diagnostic `stereo_groups_not_examined` only when there are groups. The operation count is 28.

**Example.** An `or` group over two stored centers, then its number cleared:

```json
{ "contract": 1, "request_id": "r1", "profile": "preserve-semantics", "operation": "document.transaction",
  "parameters": { "document_id": "d1", "expected_revision": 1, "commands": [
    { "op": "create_stereo_group", "kind": "or", "atoms": ["a1", "a5"], "number": 1 },
    { "op": "set_stereo_group_number", "group": "$0", "number": null }
] } }
```

The reply's `result.document.stereo.groups` and `result.notes` are:

```json
{ "groups": [ { "id": "e1", "kind": "or", "stored_number": null, "number": 0, "label": "or0", "members": ["a1", "a5"] } ],
  "notes": [] }
```

**Not claimed.** A ratio for `and`; a chiral flag in the document (the export derives the V3000 flag from the absolute group, a product rule); double-bond members; a numbered or repeatable absolute group; that members are related or stereogenic; CIP or relative (`rel`, `rac`) descriptors for groups; group interchange beyond the documented MOL V3000, recorded CXSMILES and bounded CDXML/CDX drawing profiles (V2000 has no group mapping); the group-editing controls are limited to "Web editor, stereo groups" above (no direct change of kind), while the separate conversion panels import and export only the documented bounded profiles.

## Document settings and preferences

Every document has its own **settings**, its style. They are saved with it, changed only by edits, and undone and redone like any other change:

| Field | Values | Built-in default |
| --- | --- | --- |
| `bond_length_pt` | finite, 5–200 | 30 |
| `line_width_pt` | finite, 0.1–10 | 1.2 |
| `label_font` | `sans-serif \| serif \| monospace` | `sans-serif` |
| `label_weight` | `normal \| bold` | `normal` |
| `label_style` | `normal \| italic` | `normal` |
| `label_underline` | boolean | `false` |
| `label_outline` | boolean | `false` |
| `label_shadow` | boolean | `false` |
| `label_size_pt` | finite, 4–72 | 12 |
| `carbon_labels` | `default` (a bonded carbon that records nothing is a bare vertex; every other carbon is labelled) `\| all` | `default` |
| `display_unit` | `pt \| mm \| cm \| in` | `pt` |

Lengths are always stored in pt (1 in = 72 pt = 25.4 mm); `display_unit` only says which unit a client shows and takes lengths in. The settings describe how a client draws the document (bond length for new bonds, stroke width, label font, size and weight, which carbons are labelled); nothing here renders, lays out, or computes chemistry.

- **Edits.** `update_settings` names any of the fields; it is an edit like the others: all or nothing with the rest of its transaction, one revision and one history entry, a no-op when every named field already has its value. A value out of range (or not finite) is `invalid_command` with the command's index and the `reason` `bond_length_out_of_range`, `line_width_out_of_range`, or `label_size_out_of_range`; a value of the wrong type or outside a closed set (`"label_font": "cursive"`) is a malformed body, `invalid_request` without a `reason`.
- **Reset.** `POST /api/v1/documents/{id}/settings/reset` with `{ "request_id", "expected_revision", "to": "defaults" | "preferences" }` replaces every setting with the built-in defaults or with the settings a new document would start from now (below), as one `update_settings` edit (undoable; a no-op when nothing changes).
- **Import.** `POST /api/v1/documents/{id}/settings/import` with `{ "request_id", "expected_revision", "source": document ID }` applies every setting of `source` as one `update_settings` edit. The source is read as this process holds it: an open session's working copy, otherwise its stored file (the CLI reads stored files). A missing source is `404 document_not_found` naming the source.
- Both accept optional `selection_before` and `selection_after` snapshots with the same rules as transactions, include them in request-ID replay matching, and reply with the document. They run in the application layer, so HTTP, the CLI, and in-process callers share their behavior.

**Preferences** belong to the data directory, never to a document: `<data-dir>/preferences.json`. `GET /api/v1/preferences` replies:

```json
{ "api_version": "v1", "revision": 2, "ui": { "theme": "dark" },
  "new_documents": { "label_font": "serif" },
  "new_document_settings": { "bond_length_pt": 30, "line_width_pt": 1.2, "label_font": "serif", "label_weight": "normal", "label_style": "normal", "label_underline": false, "label_outline": false, "label_shadow": false, "label_size_pt": 12, "carbon_labels": "default", "display_unit": "pt" },
  "computation": {
    "nmr": { "status": "not_implemented", "owner": "preferences", "followup": "P66", "note": "..." },
    "numeric_precision": { "status": "not_implemented", "owner": "preferences", "followup": "P60", "note": "..." },
    "mass_precision": { "status": "available", "owner": "preferences", "decimals": 2 } } }
```

- `ui.theme` (`system | light | dark`) is the web page's theme (`system` follows the operating system). `new_documents` holds the settings fields new documents start from, as stored; `new_document_settings` is the built-in defaults overlaid with them.
- **Precedence.** A new document (create, from any adapter) starts from the built-in defaults overlaid with `new_documents`, copied once at creation. From then on its settings are its own: changing or resetting preferences never changes an existing document, its file, its history, or its undo depth, whichever process changes them, and reopening a document gives the settings it was saved with. A copy (save as copy, recover as copy) keeps its source's settings.
- **Writes.** `PUT /api/v1/preferences` replaces the theme and `new_documents`, and may include `"computation": { "mass_precision": { "decimals": 0–8 } }`. Omitting this additive field, including from older v1 clients, preserves its current value. `POST /api/v1/preferences/reset` restores the built-in preferences including two mass display decimals. `POST /api/v1/preferences/import` sets `new_documents` to every setting of the named document, keeping the theme and mass precision. Each write that changes something stores revision + 1; an unchanged one keeps the revision. `expected_revision` must be the stored revision (0 while nothing is stored): otherwise `409 revision_conflict` with `current_revision`, whichever process wrote the newer one. A `new_documents` value out of range is `422 invalid_request` with the field's `reason` (the codes above). Writes are guarded like the other writes (token, Host, Origin), and `request_id` replay works per process as for documents.
- **Storage.** A write takes the OS lock `<data-dir>/locks/preferences.lock`, checks the stored revision, and replaces the file atomically (a temporary file flushed and renamed over it), so writers in every process take turns and a stale one is refused. Preference format 2 stores mass precision; format 1 reads with the default of two places and is kept byte-for-byte under `migrated/` before its first replacement. A file that cannot be read (malformed, a future format version, a value out of range) is `500 preferences_unreadable`: it is never replaced by defaults, and creating a document, reading, or writing preferences fails until it is fixed or removed. A create retried with the `request_id` of one already answered is answered from its record without reading the preferences.
- **Computation preferences.** `mass_precision.decimals` controls only the fixed decimal display of new mass results, not calculation or source precision; each result records the value used. The web offers 0–5 places and preserves values 6–8 set by other v1 clients. The legacy `numeric_precision` field remains `not_implemented` compatibility metadata for broader settings, as does `nmr`; naming either in a write (including `null`) is refused as `501 setting_not_implemented`. An empty `computation` object is accepted.
- **Initial read failure.** If the web page cannot read preferences at startup, its Preferences dialog reports the failure and offers an explicit retry. A successful retry applies the stored theme and enables the controls without a page reload.
- **CLI.** `chemifree preferences set --preferences JSON` takes the PUT body without `request_id` and `expected_revision`. The CLI and the API on one data directory share the file, and get the same codes, `reason`s, and messages.
- **Web editor** (client policy). The canvas draws with the document's settings: new bonds and snapping at its bond length, bonds and graphics at its line width (lines of double, triple, and aromatic bonds spread apart in proportion beyond 1.2 pt), labels in its font and size with every label length (text, marks, dots, and the label box that bond trimming, the view box, and hit testing use) scaled from the one layout, and carbons labelled as it says. If labels leave no room for a stroke and its round caps, the stroke is omitted while the bond remains selectable. Label boxes reserve upper bounds of each character's advance measured in Chromium for the bundled sans-serif font and for common serif and monospace fonts; a system whose generic family is wider may draw text past them. The **Document style** dialog edits every setting (lengths in the display unit; the label size is a font size and stays in pt), resets to the defaults or the preferences, and applies another saved document's style; the **Preferences** dialog sets the theme, new-document style and mass display decimals, takes the open document's style for new documents, and resets preferences. Unimplemented general computation settings remain disabled. Changes are built from the latest document or preferences when their request runs. A number being typed keeps its text and original unit while earlier requests finish; entering an equivalent displayed value makes no edit when the actual value has not changed. A value out of range is not sent.
- **Web editor, stereo marks** (client policy). The bond-kind group of the toolbar has three mark items after the bond kinds: Wedge bond, Hashed wedge bond and Wavy bond, variants of the bond tool (no new tool and no shortcut). The tapered mark the service calls `hash` is the public specification's hashed wedge (its `WedgedHash` displays); the public plain Hash, Dash, Bold and HollowWedge displays are untapered or have no narrow end, are not representable, and are not drawn, so the editor's labels and accessible names say "hashed wedge". `crossed` is the public "Either" double bond: the editor draws it and sets it only in the bond inspector (the "Drawn mark" row: for a single bond it offers Plain, Wedge, Hashed wedge and Wavy, for a double bond Plain and Either, the latter with the accessible name "Crossed double bond (either)", and for any other order the row says in words that a mark needs a single or a double bond); whether the original has a drawing tool for it is not decided by the public specification, so a canvas tool is a follow-up, not a decision. The row's "Narrow end" control names the two atoms. Gestures: a drag or click that makes a bond puts the mark on it and the narrow end of a wedge or hashed wedge at the atom or point where the gesture started; over an existing single bond a drag marks it with the narrow end at the drag start; a click on a bond cycles plain, narrow end at its begin, narrow end at its end, plain (the other of wedge and hashed wedge keeps the narrow end; wavy cycles plain, wavy, plain; a wedge or hashed wedge becomes wavy); a bond of any other order is refused in words and nothing is sent. With a kind of bond chosen, a click on a marked bond clears its mark and leaves the order; a click on an unmarked bond cycles the order. `update_bond` is refused on a marked or configured bond, so the click cycle, a drag over an existing bond and the inspector's change of kind send `set_bond_mark` null (when the new order cannot carry the mark) and `set_bond_configuration` null (when a stored double-bond configuration would outlive its bond being double) first, in the same transaction, and the status line says what they removed. A gesture is one transaction and one undo step: the bond commands, `set_bond_mark`, and `set_configuration_from_drawing` with `atoms` the narrow end (every end that could hold a center, for wavy; `bonds` for crossed) and `instance` the placement it was drawn in. The derive command is attached only when each named atom is an element atom that has 3 or 4 bonds once the gesture is done; otherwise the mark is sent alone and the status line says why. When the service refuses the derivation (the derive command's own index, with a reason that command gives: `stereo_not_derivable_*`, `stereo_center_unsupported`, `stereo_center_neighbors`, `stereo_double_bond_required`, `stereo_reference_invalid` or `stereo_geometry_unsupported`) the editor sends the mark alone at the same revision, shows the reason, and keeps the mark; any other refusal, and a failure of that second send (a revision conflict, say), is the page's error as for every edit, and no reason is shown as if the mark had been stored. A mark gesture is queued as its intent (the bond or atoms, the mark kind, the placement, drag or click) and its commands, its refusal and its derive command are built from the latest document when the request runs, so an edit that landed first (an Undo during a drag that changes a bond's order or an atom's bonds, say) changes what is sent and what a refusal says. Drawing the mark a bond already shows sends nothing and says so ("b3 is already drawn that way."), unless a center that mark bears on has no stored configuration, when the gesture is sent as it is (the service treats an unchanged mark as no change). The derive command replaces what was stored for the atom (a stated configuration included, as above): the status line names the replaced value and says undo restores it, and it says when a mark that moved or was cleared leaves a stored configuration of its old narrow end behind (not when the same gesture stored that atom's configuration again, nor while another mark still shows that atom: the narrow end of a wedge or hashed wedge, both atoms of a wavy bond; the same holds for a double bond's stored configuration and a crossed mark, and for a mark that went with its bond or was replaced by one that shows less). Deleting a bond or an atom that carries a mark says the mark went with the bond (the reply's `notes` tell the configurations the deletion cleared). That status line is the only place a gesture's own replacement is told, because the reply's `notes` list only the cascade. The inspector sets the mark the same way, with `instance` the single placement and, for a drawing with several placements, no derive command (the status line says to draw the mark on the canvas to store the placement's configuration). Moving atoms or changing bonds does not update a stored configuration: the notice says so and points to the Stereo panel (next item), where a disagreement shows, and it says that whether an atom is a stereocenter is not assessed (it never says there is none). A status text describes one revision and is shown only while the page shows it.
- **Web editor, stereo agreement** (client policy; the reply is `chem.stereo`, above). For a drawing that holds a mark or a stored configuration (and for no other: a drawing without either sends no request), the page asks `chem.stereo` with `{ document_id }` and no `instance`, 300 ms after the last committed change that alters what is drawn (an atom, a bond, a mark, a stored configuration or a position), with one request in flight and exactly one more scheduled for any changes made meanwhile, and none while a pointer gesture is in progress (the check resumes when it ends). A reply is used only when the snapshot it carries equals the drawing on screen, positions included, and when it comes from the service process that is current; otherwise the page shows "Checking the drawing…" (the Stereo panel has `aria-busy`), draws no ring and lists no row from an older answer. The reply must be `ok` with model `chemifree-stereo-1`; any other outcome shows the first diagnostic's message with a "Check again" button, and a reply that does not match the contract, or does not agree with its own snapshot, is refused as a whole. Advisory diagnostics are shown verbatim under "Details" (known codes first); the fixed sentence the panel states in place of `stereo_not_checked` is "Only stored configurations are compared with the drawing. CIP labels describe the stored configuration, not the drawing." A "Check automatically" switch in the panel (on by default, not saved) turns the automatic check off: then nothing is asked until "Check now", and turning it on asks again. The summary line (a status live region) says how many items agree, disagree, are stored only, are drawn only or are marked but unreadable ("not assessed" is never worded "no stereo"); the Stereo panel has one row per item and placement with its agreement in words and its reason; the canvas draws a ring on each item that does not agree, for the placement on screen only and never while the answer is not about the drawing on screen: a solid destructive ring for a disagreement, a dashed muted ring for a state with nothing to compare (stored only, drawn only, marked but unreadable), each an image named with the item and the state, and an overlay that takes no pointer events and is marked `data-editor-overlay` (it is not part of the drawing; the web editor has no export of the canvas yet). Nothing is stored or changed by the check. A row offers one click: "Update from drawing" (a disagreement) and "Store from drawing" (drawn only) send `set_configuration_from_drawing` for that item and placement, "Remove stored" (stored only) sends `set_atom_configuration` or `set_bond_configuration` with `null`, and "Update all from drawing" sends one transaction for every disagreement of a drawing with one placement when there are two or more; each is one undo step. The commands are built from the latest document when the request runs, and a click whose item has gone from the latest document sends nothing and says so ("The drawing changed, so nothing was changed. Check the comparison again."). The status line names what was replaced or removed, for example "a1: replaced stored clockwise with anticlockwise from the drawing in s1. Undo restores it." The comparison itself does not assess whether an atom is a stereocenter and computes no CIP label; the CIP labels are a separate reading of the stored configurations, shown only when the user switches them on (next entry).
- **Web editor, stopped computations** (client policy). The web sends `chem.aromaticity`, `chem.aromaticity_convert`, `chem.identity`, `chem.substructure`, `chem.properties`, `chem.cip` and SMILES export (the SMILES panel) without `time_limit_ms`, so they run under the server's default limit. A reply is a stop only when it is `outcome: "error"` with exactly one `error` diagnostic whose code is `time_limit` (with `details.limit_ms`, a non-negative integer) or `cancelled`, no `path`, `result` null, an empty `loss_report`, the operation's own `model_id` and an integer `document_revision` (`chem.aromaticity_convert` may name none: it is null there). A reply with such a code but another shape is refused as incompatible and shown as an incompatible reply, never as a stop; every other refusal (`resource_limit`, `invalid_request`, `revision_conflict` and the rest) keeps its own display, whatever its `model_id`. A stop is shown as a notice that is not styled as an error, titled "Stopped at the time limit (N ms)" or "Cancelled", saying that the computation was stopped before it finished, that no result is shown and that the drawing was not changed, with a retry: the properties, aromaticity inspection, identity and substructure panels offer "Try again" and the SMILES panel "Write SMILES again" (a retry asks again without any accepted loss, so a lossy form asks for consent again). The earlier result of that panel is cleared (a substructure search replaces its stored outcome), so no partial or stale result is shown. A stopped `chem.aromaticity_convert` reaches the page-level notice, titled the same way and not styled as an error, with Dismiss; the Convert buttons are the retry, since nothing changed and the inspection is still current. The MOL/SDF export has no time limit and is not affected. The client's own 10-second timeout is a connection failure (unreachable), not a stop.
- **Web editor, CIP labels** (client policy; the reply is `chem.cip`, see [CIP labels](#cip-labels)). Two switches in the Stereo panel, "Atom stereo labels" and "Bond stereo labels" (they mirror the public ShowAtomStereo and ShowBondStereo settings), are off by default and not saved. While both are off the page sends no `chem.cip` request. While one is on, and the drawing stores a configuration of that kind (with a switch on and nothing of its kind stored, the panel says "No stored atom configurations to label." or the bond or both equivalent, and nothing is asked), it asks `chem.cip` with `{ document_id, scope: "stored" }` (the scope is always sent explicitly) and never `stereogenic` (a dense graph's scan has no lower node cap yet, so the editor does not ask for one; issue #250), scheduled like the agreement check: 300 ms after the last committed change, one request in flight and one more for changes made meanwhile, none while a pointer gesture is in progress. A reply is used only when the snapshot it carries equals the drawing on screen and it comes from the current service process; otherwise the page shows "Computing CIP labels…" and no label, and a refused request (`resource_limit` included) shows "CIP labels unavailable: " and the service's message, no label and a "Try CIP labels again" button; a `time_limit` or `cancelled` stop of the request shows "CIP labels stopped at the time limit (N ms). No labels are shown." or "CIP labels cancelled. No labels are shown." instead, with the same button and no label. A reply that does not match the CIP contract (a missing or wrongly valued field, a state without its descriptor or reason, a ranking that is not the item's own four positions, a stored record the snapshot does not hold (compared key for key, it must equal the snapshot's own record), `E` or `Z` on a center or `R` or `S` on a bond, a stored center or double bond (or a named item) the reply does not list, or an item it lists that the scope does not, a loss entry or a known diagnostic that does not fit the items or comes out of order, another model or rules) is refused as a whole and shows no label; a field the page does not know, at any level outside a stored record, and a well-formed diagnostic with a code it does not know are ignored (kept, for the diagnostic), as the version-transition rule above ("clients ignore unknown fields") allows. Turning both switches off clears what was shown and discards a request still in flight, so turning labels on again never shows an earlier failure or answer. Only an `assigned` item gets overlay text on the canvas, `(S)` or `(R)` near the center and `(E)` or `(Z)` near the middle of the double bond, in every placement, beside the atom in the widest gap between its bonds and beside the middle of the bond on its upper side; the overlay takes no pointer events, is marked `data-editor-overlay`, is a named image per label, and is never part of the document or an export. The bond text form, the placement and the absence of any marker for an unspecified center are Chemifree choices, not legacy behavior, and nothing suggests an enhanced-stereo-group meaning (this build ignores such groups). `none`, `unspecified`, `unknown`, `ambiguous` and `unsupported` get no overlay text. Each Stereo row that has a stored configuration gets one more line, under the switch of its kind (an atom row under "Atom stereo labels", a bond row under "Bond stereo labels"; a row with nothing stored gets none): "CIP: S", "CIP: none, the ligands are equivalent" (with a sentence that a stored configuration makes no difference there, when the service warns of it), "CIP: not supported - " or "CIP: ambiguous - " and the sentence of the item's loss entry, "CIP: unknown (stored as either)", or "CIP: not examined" for a stored item the reply does not list (a valid reply lists every stored item, so this is a robustness case); each is followed by how the stored configuration relates to the drawing from `chem.stereo` (agrees, disagrees and so on), because a label describes the stored configuration and not the drawing. A label is never written to the document: no command carries one, and showing, hiding or failing to get one changes no revision, history, selection, mark or stored configuration. The request carries no `time_limit_ms`, so the server's default of 5 seconds applies and a slow request ends there with `time_limit`; the web client's own 10-second timeout is a further bound, and closing that connection makes the server cancel the request. The page applies Rules 1a, 1b, 2 and 3 as the service reports them and claims nothing beyond [CIP labels](#cip-labels).
- **Web editor, stereo groups** (client policy; the data is `stereo.groups`, see [Stereo groups](#stereo-groups)). Everything shown is read from the snapshot's `stereo.groups` (an omitted list reads as none). Each Stereo panel row of a stored center has a line `Group: ` and either `no group` or the group's id, snapshot label and kind word (`e1: or2 (OR)`; `absolute`, `AND`, `OR`). Under the rows a list shows, for each group, its id, label, kind word with the contract's meaning of the kind (absolute: the members are as drawn; AND: the author means a mixture of the drawn set of members and its all-inverted counterpart, with no ratio stated or implied; OR: the members invert together or not at all), the number as stated or `number omitted (reads as 0)` (an absolute group: no number), the member count, and members as buttons that select the atom. The section also says that groups are stored and edited here, can be written to a file from this page only as MOL V3000 (its MOL panel's V3000 or Auto choice writes them as collections; recorded SMILES writes supported groups, while canonical SMILES and MOL V2000 require consent to drop them), are created by reading a V3000 file with collections (and by reading CDXML, CDX or a CXSMILES block), are not compared with the drawing and do not change CIP labels or identity, that query features, S-groups and R-groups are refused, not read, and that double bonds cannot be grouped, centers stored `unknown` cannot be members, and group meaning is ignored by CIP labels, identity and substructure search. Seven actions apply to the selected atoms (bonds and drawn objects in the selection are ignored): Make absolute, Group as AND and Group as OR create a group from selected stored tetrahedral centers that are in no group, with the number typed in one shared field (empty for none; an integer from -2147483648 to 2147483647, `0` and negatives included; anything else is not sent); Add to group adds the ungrouped selected centers to the one group whose member is also selected; Remove from group removes the selected atoms, all of which must be members, and is unavailable if it would empty a group; Merge groups merges the groups of the selected atoms, all of which must be members, when there are exactly two groups of one kind, into the one with the lower id (which keeps its kind and number); Split selected out moves the selected atoms, all members of one `and` or `or` group and not every member of it, into a new group of the same kind with the typed number. Each group's entry has a number field with Set number (an `absolute` group has none) and Delete group. An action that cannot apply is disabled and says why in text: nothing selected, a selected atom with no stored configuration or stored `unknown`, an atom already in a group, an absolute group with a number typed, a second absolute group, an interpreted number the kind already has (a group with no number reads as 0), members of the wrong number of groups, groups of different kinds, or every member selected. Set number does not check for a number another group has: the service refuses it, and the refusal offers the merge. Each action is one transaction built, with `expected_revision`, from the latest document and the atoms selected at the click, so it is one undo step, a click that no longer applies sends nothing and says so, and undo and redo restore the same group ids. A refusal (`invalid_command` with a `stereo_group_*` reason) is shown as a plain sentence for each of the twelve reasons in the table above, followed by "Nothing was changed.", and the number field returns to the stored number; a reply's group notes are shown as the other stereo notes are. The Stereo panel's Group labels switch is off by default and not saved; while on, the canvas draws beside each member of every group, in every placement, `abs` or the kind word and interpreted number (`OR 2`, `AND 0`, `OR -3`), one step (12 pt) beyond the CIP label's place for that atom (above it when that place is above the atom, below otherwise), so the two stack and the position does not depend on whether CIP labels are shown. The overlay takes no pointer events, is marked `data-editor-overlay`, is a named image per label, uses no color to suggest meaning and is never part of the document or an export. No ratio is claimed for any kind. Changing a group's kind is not offered in the web editor.
- **Web editor, how marks are drawn** (client policy). Every number is in `MARK_STYLE` in `apps/web/src/editor/geometry.ts`, in points. The public specification gives no rendered geometry for a mark: it declares the wedge width and the hash spacing as document settings (`BoldWidth`, `HashSpacing`) and all its authored fixtures use 4 and 2.7 with a line width of 1 and a bond length of 30, so those two values are used as stand-ins for settings Chemifree documents do not have. They do not scale with the bond length or the line width; the wedge is never thinner than the line. Chemifree has no such settings and no format change is made here: the CDXML codec (P28, P29) must carry `BoldWidth` and `HashSpacing` (and the label margin) from a file or report them as loss. The rest is a Chemifree choice: a wedge is a filled polygon from a point at the narrow end to the full width at the other, straight-edged; a hashed wedge is stripes across the same taper, one every 2.7 pt centred half a spacing from the narrow end along the whole bond (so a label that trims the bond removes stripes without moving the others), each as wide as the line and as long as the taper there (at least the line width), and at least one stripe where a short piece is visible; a wavy bond is a smooth wave of quadratic arcs, 4 pt long and bulging 1.8 pt, on the axis; a crossed double bond is two lines crossing between the places of a double bond's two lines, each trimmed against the label boxes along its own path (a wide line can start beside a small box and cross it). Marks are trimmed at atom labels like every bond (a cut wedge keeps the slope it would have had), have no ink where labels leave no room, and are bounded (at most 400 stripes or arcs a bond). The ink stays within the reach the view box and placement frames already allow. Selection, hover and hit testing are those of a plain bond. None of this is claimed to match the original application's drawing.
- **Web editor, MOL and SDF versions** (client policy, P28c; no service change). The MOL and SDF panel has a version control, `V2000` (the default), `V3000` and `Auto`, sent as the `ctab` key of `document.export`; the choice is held by the page only and is not saved in the drawing, and the panel shows the dialect `result.ctab` says was written. A `loss_consent_required` refusal shows the listed losses with a plain sentence for each known feature id (an id the page does not know is shown as it is, never hidden) and an explicit accept button; accepting repeats the request with `accepted_losses` set to the reply's `loss_digest`, and a `loss_consent_changed` answer voids the accepted digest, asks again without it (at most twice, then an error) and shows the new losses marked as changed. A consent refusal without a `loss_digest` (an import, or a service reply that carries none) shows its losses and says plainly that this page cannot accept them for that request. Nothing is offered for download before consent, and a download rechecks the content, losses and dialect against a fresh export; if that recheck finds the consent stale (`loss_consent_changed`, or new losses needing consent) the preview is dropped and the losses are offered again. Refusals of V3000 import and export show one plain sentence per closed reason the page can reach (`v3000_collection_member_not_stereocenter`, `v3000_collection_drawing_unreadable`, `v3000_group_member_not_drawn`, the collection, S-group, R-group, query, 3D and reaction reasons, `resource_limit`, said without any number as a limit of the service that its message names, and `format_ctab_conflict`), ending `Nothing was changed.` once, worded for the direction (file read or drawing written) where the two differ, then the service's own message; every other reason of the table above is worded by one generic malformed-input sentence, and a code the page does not know shows the service message alone. A V3000 file is imported through the same file and preview steps as V2000, and its collections appear as groups in the Stereo panel from the snapshot. The page does not claim query, S-group or R-group support: those files are refused.

- **Web editor, CDXML and CDX drawing export** (client policy, P29f; the bounded service profile remains P29e). The **CDXML and CDX export** panel offers `Preview CDXML export` and `Preview CDX export` for the current drawing. A preview prepares a file and lists losses; it does not modify the drawing or add stereo marks. Required losses appear in a separate offer, with an explicit `Accept these losses and preview CDXML` or `CDX` action; until acceptance succeeds there is no download button. Informational losses need no acceptance. A changed digest clears the prepared file and asks for a fresh review; an offer with no valid digest cannot be accepted. Headings distinguish an omitted group from an omitted native group ID and from uncertain interpretation of originally ungrouped stored centers; they do not assert that every format loses every group, that an omitted group survived, or that a group has a mixture ratio.

  Preparation, loss offers, retries and download are bound to the complete displayed conversion snapshot and the current service generation. Matching document ID and revision alone is insufficient. An edit, opening another drawing, a new format preview, or a noticed restart invalidates the earlier result. The download action sends another export request with the same format and accepted digest and requires the same snapshot, content and losses before downloading; a delayed pre-edit reply cannot authorize a download. CDXML becomes a UTF-8 Blob, while canonical base64 CDX is decoded into binary bytes before the Blob is built; files are named `chemifree.cdxml` and `chemifree.cdx`. A service restart needs a fresh preview, without silently retrying prior acceptance against the new process. The panel retains the service's limits: first placement only, groups only when every written depiction derives its stored member, no invented wedges or CIP/geometry metadata, and explicit losses/refusals for unsupported content. Full scene export, double-bond E/Z derivation and general interoperability remain outside this UI. P29g records completion of the documented bounded profile with P29f checks and reviews complete; these controls do not establish full format compatibility or release completion.

## Consistency, idempotency, and saving

- **Revisions.** Every mutation carries `expected_revision`. A mismatch returns `409 revision_conflict` with `current_revision` and changes nothing. This includes an aromaticity conversion whose revision was current when it started but not when its computation ended.
- **Request IDs.** `request_id` is 1–64 characters of `[A-Za-z0-9_-]`, required over HTTP. Repeating a completed request with the same ID and body returns the original response without applying it again. Reusing an ID for a different body returns `409 request_id_reused`. Replays are remembered for the latest 64 successful mutations per open document (and per process for creates, closes, deletes, and restores, so a retried close cannot end a session opened after it); failed requests are not remembered, so a retry runs again. A create, or a write that takes a document's turn (see Time budget), whose `request_id` is still running is answered `409 request_in_progress` at once, whatever the body (see Outcome of a lost reply). A `request_id` is matched where its reply is remembered: per document for saves, autosaves, and renames, per process for creates, closes, recoveries, draft discards, copies, deletes, and restores.
- **Sessions.** The first access opens a document from storage into a per-process working copy. Edits change only the working copy until `save`. A working copy without unsaved changes follows newer commits by other writers (e.g. the CLI) on its next access, so an edit based on the older view gets `revision_conflict`. `close` ends the session; the next access reopens the committed file. Closing with unsaved changes returns `409 unsaved_changes` unless the body has `"discard_unsaved": true` together with `expected_revision`, the revision whose changes the client agreed to lose; the session is shared by all clients of the process, so a stale revision is `409 revision_conflict` and nothing is discarded. Close body: `{ "request_id", "discard_unsaved"?, "expected_revision"? }`. If a process stops, the last committed save remains, and unsaved edits survive only in the session's recovery draft (see Autosave and recovery).
- **No reuse.** Within a process, a revision number or atom/bond ID that a client has seen never names a different state later. After a discard (or when a clean copy reloads another writer's commit) the working copy continues numbering above everything it handed out, so a stale `expected_revision` is a conflict. A restarted process cannot know what the previous one handed out. When its token is refused, the web client re-reads the document and resends an edit only if the restarted service holds exactly the drawing on screen (never an undo or redo); otherwise it asks for a reload.
- **Create** commits the empty document before replying. Like the other writes that make a new document (save as copy, recover as copy), it is not guarded by a stored revision, so a client that got no response retries with the same `request_id`; the web client does this. A restarted service has forgotten that `request_id`, so when the retry meets a new token the web client does not resend: the document may already exist, and the page says so and refreshes the list.
- **Save** writes a temporary file in the same directory, flushes it, and renames it over the document; the rename is the commit. The directory is then synced where the platform supports it, and a failure of that sync does not turn the committed save into an error. The acknowledgement is sent only after the commit. A document that would encode beyond the 64 MiB read limit is not written. On failure the previous file is intact, the reply is `503 storage_failed` with a `reason` (see Storage failures), and the session keeps its unsaved changes for a retry. Saving an unchanged session is a successful no-op.
- **Concurrent writers.** A save requires storage to still hold the commit this session last saw. If another writer (another session, another API process, or the CLI) committed first, the reply is `409 save_conflict` and nothing is written; close with `discard_unsaved` and reopen, or save as a copy. Every save, from any process, holds an OS file lock on `<data-dir>/locks/<id>.lock` from the base check through the commit, so of two saves from the same base exactly one commits and the other is a conflict. The operating system releases the lock when its holder exits, including a crash. A save waits at most 5 seconds for another process's save of the same document, then fails with `503 storage_failed`, `reason: "busy"`, and can be retried. While a save waits, requests for other documents and edits to the same document proceed; the save commits the revision it was asked for (`expected_revision`), and edits made meanwhile stay unsaved.
- **Time budget per document.** Within one API process, saves, autosaves, closes, recoveries, draft discards, copies, renames, deletes, and restores of one document take turns. A request waits at most 3 seconds for its document's turn; if it does not get it, it changes nothing and fails with `503 storage_failed`, `reason: "busy"`. With the 5-second lock wait above, such a request is answered within about 8 seconds plus its own storage I/O, before the web client's 10-second deadline, however many are queued for that document (for example from two tabs). There is no cancel for these requests: a request that has its turn runs to the end. (An aromaticity conversion takes no turn; it has a time limit, see [Computation time limit and cancel](#computation-time-limit-and-cancel).)
- **Storage failures.** `storage_failed` carries `reason`: `read_only` (storage refused the write: missing permission or a read-only file system; retrying helps only after that is fixed), `busy` (the document stayed busy for longer than a request waits; nothing was changed, and retrying later can succeed), or `io` (any other read or write failure, including a file that cannot be read for lack of permission). In every case the previous commit and any recovery draft are intact, and the session keeps its unsaved changes.
- **Outcome of a lost reply.** A request whose reply never arrived (a timeout, a dropped connection) may still have taken effect. Sending it again with the same `request_id` and body is safe. While a request with that `request_id` still runs, the reply is `409 request_in_progress` at once, without waiting for the document's turn or for other requests' storage I/O. Once it completed, the original reply is returned without waiting for the document's turn, as long as it is still remembered (the replay window under Request IDs). If it failed, or is no longer remembered, it runs again. A committed save asked about within that window is therefore never reported as failed because its reply was lost. The web client does this for saves: when no reply arrives it asks again at once, then every second while the save is in progress (at most 12 times); if the service stays silent it reports the save as unconfirmed (`save_unconfirmed`), and **Check again** saves the revision on screen: if nothing was edited since, that is the same revision, asked about with the same `request_id`; otherwise it is a new save, which also commits the earlier changes. For creates and copies it keeps the `request_id` while the service reports them in progress, and asks again every second in the same way. Renames, deletes, and restores are kept from acting twice by their own preconditions (the expected revision; the document being stored or trashed), so the client asks about a lost reply once at once and then every second while it is in progress, with the same `request_id` and only in the service process it was sent to (`service_restarted` once the page meets a different one), and otherwise reports the failure as it is: a later attempt is a new request, which the precondition refuses if the first one took effect (`revision_conflict` or `document_not_found`). An unknown outcome belongs to the service process it was sent to (each process has its own session token): once any request of the page meets a different token, the client does not ask the new process with that `request_id`, which it has never seen, but reports the outcome as unknown (`create_unconfirmed`, `copy_unconfirmed`, or `service_restarted` for a save). A gateway error without a v1 body (502, 503, or 504 from a proxy in front of the service, such as the development server's) counts as no reply. When a create or copy is answered after being asked about again, the answer may be the record of the first attempt, so the client reads the document again and shows it as it is now.

## Autosave and recovery

A working copy with unsaved changes can be autosaved to a recovery draft, `<data-dir>/drafts/<id>.json`, kept apart from the saved document. The draft holds the whole working copy: content, undo/redo history, identifier counters, its revision, and the stored revision it started from (`base_revision`). Autosave body: `{ "request_id" }`. It writes the draft when the session has unsaved changes not yet in it, and removes the session's draft when nothing is unsaved; `session.draft_revision` reports what the draft holds.

- **When, without a client.** The API process drafts unsaved work on its own, whether or not any client calls autosave: 2 seconds after the last change, and at most 10 seconds after the first change the draft does not hold, so continuous editing cannot postpone it. A failed attempt is retried after 2, 5, 15, then every 30 seconds; edits do not push a retry back, and `session.draft_failure` reports the failure until a draft succeeds. A normal stop (SIGTERM, Ctrl+C) drafts every session's unsaved work after the last request is answered and before the process exits, and logs any it could not draft. The service's drafts and client autosaves are the same operation and take the same per-document turn, so they never run at once and one revision is drafted once: whichever comes second finds nothing to do. Drafting follows the session: a save or a discarding close leaves nothing to draft, a recovered draft already holds its work, and a session whose document has an earlier session's draft or whose draft another process owns is not drafted over (each attempt fails with `recovery_pending` or `draft_in_use` and is retried until that is resolved). The CLI commits every edit and has no sessions to draft.
- **When, in the web editor.** The web editor autosaves 1 second after the last change, and at least every 5 seconds while changes keep coming. A failed draft is shown with what to do (for `reason: "read_only"`, make the data directory writable) and can be retried at once with **Retry now**; the session keeps its changes meanwhile. A service that reports `session.draft_failure` retries on its own schedule, so the page shows that record (who retries, when, how many attempts) and does not draft beside it: it reads the document once the service's retry is due, and again after 2, 5, 15, then every 30 seconds while those reads fail. A read confirms that the service process is the one that served the page's view (the page learns the session token before it reads a document, and the token is unchanged after the read); if the service restarted, the page stops drafting and keeps the drawing on screen until the user reloads, as it does when a write finds a restart. With a service from before `draft_failure`, the page retries the draft itself after 2, 5, 15, then every 30 seconds. Further edits do not postpone either retry. Before the page shows another document it drafts the one it leaves; if that fails, it says so. The page's draft state is what the service reports (`session.draft_revision` and `recovery`), so after a restart nothing counts as drafted until the new service has a draft, and while an earlier session's draft exists autosave shows as paused. A reply for a document the page has left is ignored. If a reply shows the service holding an older or different state than the page (another client closed the session, or a restart the page has not seen yet), the page re-reads the document instead of drafting again. Other HTTP clients may call the endpoint to draft sooner than the service would.
- **Web background reads and process ownership.** A background refresh preserves an existing editor error whether its read succeeds or fails. The displayed snapshot remains bound to its service generation even if a failed edit learns a replacement process's token. A noticed restart stops automatic drafting and manual draft retries for that snapshot; late draft replies from an earlier generation cannot adopt content or trigger another background read. A queued refresh checks that generation before and after reading. Explicit Reload/reconcile or the existing same-content edit recovery must adopt the current process's snapshot before drafting resumes. Matching document ID and revision alone does not permit adoption. The autosave restart regression uses the real editor/autosave hooks and client with scripted HTTP and browser time; it is not evidence of a real API crash or OS storage behavior.
- **Leaving undrafted work after a noticed restart.** A flush of unsaved work not covered by a draft returns a restart reason while its displayed process generation is unresolved. Open/Create then refuses before requesting its destination, keeps the original drawing on screen and reports `service_restarted` with Reload available. The replacement process is not claimed to retain the page-only changes. Explicit Reload/reconcile remains the existing adoption action. Clean drawings and work already covered by a draft can still navigate; an ordinary current-process draft failure retains the existing switch-and-report-`draft_not_written` behavior.
- **Snapshot response provenance.** Each actual write attempt captures the client process generation before awaiting its session token. Explicit document reads fetch the actual session token before and after reading, refresh a cached replacement token and generation when necessary, and reject a snapshot if the process changes during the read. Document normalization retains that origin in memory, without adding JSON or file fields. Every editor snapshot adoption requires an origin from the same client and its current generation; a successful response held across a noticed restart cannot rebind the drawing. This covers ordinary edits, navigation/creation, copies, recovery, import and Reload as well as background reads. A safe internal retry in the new process carries that retry's generation. Close has no snapshot, so its request generation is checked before clearing the drawing. Further edit/copy/Close requests from an unresolved old snapshot require reconciliation first.
- **An edit after another inspection noticed a restart.** A new explicit edit first reads the replacement process's document and checks the same complete content used by ordinary restart recovery. Only a matching current-process snapshot permits rebuilding that edit through its existing `restartedFrom` rules. A differing drawing or a command that cannot be rebuilt sends no mutation and retains the displayed drawing with `service_restarted`. Learning a new token through inspection does not bypass this read, and autosave remains stopped until a current snapshot is actually adopted. A late old-process success alone never triggers this explicit-edit recovery.
- **Closing a view whose process ended.** After a noticed restart, an explicit clean Close or confirmed discard Close can clear that old drawing from this page without closing any replacement-process session or removing its recovery drafts. Undiscarded unsaved changes stay on screen. The displayed ID, revision and complete content must still match the captured Close, and the pending ring/clipboard gate still applies. A Close response already in flight when a restart is noticed still cannot clear the page; another explicit Close is required. Current-process Close keeps its existing server revision/discard checks.
- **What a crash can lose.** After a crash (SIGKILL, power loss), the changes since the last completed draft: with the web editor open, normally up to the last 1–5 seconds of editing; with any other client, up to the last 2–10 seconds; longer while drafting fails or is blocked by an earlier draft.
- **Ownership.** The process whose session wrote a draft holds an OS lock on `drafts/<id>.owner` while it runs. Another process sees that draft as `in_use` and cannot recover, overwrite, or discard it. When the owner exits (normally or not) the draft is abandoned, and any process can act on it. Within one process, requests take owner locks one at a time, so a request never mistakes another request's brief look at an abandoned draft for a different running process.
- **Recovery.** A document with an abandoned draft reports `recovery: { state, revision, base_revision, saved_at_ms, message }`:

  | `state` | Meaning | Actions |
  | --- | --- | --- |
  | `recoverable` | Storage still holds `base_revision`. | `recover`, `discard-draft` |
  | `outdated` | Another writer saved since `base_revision`. | `recover-as-copy`, `discard-draft` |
  | `unreadable` | The draft file is corrupt or unsupported, or its contents do not restore to a valid document of this ID; `message` says why. | `discard-draft` |
  | `in_use` | A running process owns it. | none here |

  `recover` makes the draft the working copy: content, undo/redo history, and identifier counters as autosaved, unsaved, with the draft still protecting it. Its revision continues above every revision this process has handed out, so no revision a client has seen is reused; recovering an untouched document after a restart keeps the draft's revision. The web editor offers `recover` and `recover-as-copy` only while the open session has no unsaved changes. `recover` requires the open session (if any) to have no unsaved changes (`409 unsaved_changes`). If storage moved past `base_revision`, or this process already handed out atom, bond, scene instance or stereo group identifiers beyond the stored counters, it returns `409 draft_outdated` (with `current_revision`) and the draft stays; `recover-as-copy` then saves the draft's content as a new document (as for save as copy) and removes the draft. Its reply's `source.closed` is true when the source was open when the copy was made and is no longer open when the reply is recorded. The copy may evict the source only when all 64 slots are in use and it is the only clean session that can make room; another request may also evict it while draft removal is in progress. A closed source reopens from storage on its next access. `discarded_changes` is always `false`: only a clean session is closed this way. An unreadable draft returns `500 document_unreadable` on recovery. Claiming and preserving a draft for recovery, and removing it after recovery-as-copy or explicit discard, do not hold the shared session table. During that local operation, session replies treat the draft as locally held rather than advertising it as abandoned; a later read reports any draft the operation did not consume. Autosave still takes the document turn and never overwrites an abandoned draft. If this document is edited while its draft is being claimed or preserved, recovery returns `409 unsaved_changes` and leaves both the edit and the abandoned draft intact.
- **Draft lifecycle.** A save removes the session's draft with its commit. Before the commit the draft is renamed to `drafts/<id>.committing`; if the process ends before removing it, the next save-lock holder removes it when storage no longer holds its base (the commit happened) and restores it otherwise. So a saved draft never comes back, and an interrupted save never loses one. Closing with `discard_unsaved` removes the draft before the session ends (if removal fails, the close fails and nothing is discarded). `discard-draft` removes an abandoned draft. New work is not autosaved over an abandoned draft: autosave returns `409 recovery_pending` until that draft is recovered or discarded.

## Save as copy

Body: `{ "request_id", "expected_revision", "title"? }` (default: the source title followed by ` (copy)`). The working copy's atoms, bonds, and identifier counters are committed as a new document with a new ID, at revision 0, without undo history; IDs are kept, so `a3` names the same atom in both. The reply is the copy's document response plus `source: { id, closed, discarded_changes }`. The source session then closes (`source.closed: true`): its working copy lives on in the copy, and the source reopens from its last save, which the copy never changes; `discarded_changes` says whether it had unsaved changes that now live only in the copy. If the source's recovery draft cannot be removed (a storage error), the source session stays open with its unsaved changes and its draft instead, so no draft is left behind to be offered for recovery later; the copy is still committed, and the reply says `closed: false` (with `discarded_changes: false`). The web editor names that outcome in its copy notice and offers **Open original**. The notice describes the moment of the copy, so the page drops it once it shows another document or learns that the service restarted, which it does when a write's session token is refused; a page that sends no writes keeps the notice until then. Like create, save as copy is not guarded by a stored revision, so `request_id` replay (per process) makes a retried request return the same copy; the web client reuses the ID for an unanswered attempt and does not resend it after a restart (`copy_unconfirmed`). A failed commit writes nothing and leaves the source session as it was.

## Document list, rename, and trash

- **List.** `query` keeps documents whose title contains it, ignoring case (surrounding spaces are ignored; at most 200 characters, else `422 invalid_request`); an empty query lists everything. `sort` is `title` (default: title, then ID) or `recent` (last commit first, then title). The commit time is the stored file's modification time, a wall-clock time: a clock adjustment (such as a time sync stepping the clock back) can reorder documents saved within it, and a program that touches the file changes it. Each summary has `saved_at_ms`, the time storage last committed the document (`null` if unknown). Unreadable files are always listed under `unreadable`. Unknown parameters are refused with `422 invalid_request`. The web client and the CLI pass these to the same service; neither filters or sorts on its own. A list reads at most 1000 documents (open ones first, then stored ones in identifier order), and the query and sort apply to those: in a larger library, a search can miss documents beyond them. The reply says so: `truncated` is `true` exactly when that cap left stored documents unread, and `false` otherwise (an additive field; a client that does not know it ignores it). The web client and the CLI show a notice when it is `true`.
- **Rename.** Body: `{ "request_id", "expected_revision", "title" }`. Renaming commits the new title at once, as one new revision, all or nothing; the reply is the document with no unsaved changes. The open copy must have no unsaved changes (`409 unsaved_changes`: rename it in the editor and save instead), and no abandoned recovery draft may exist (`409 recovery_pending`), since the commit would leave that draft behind its base. An unchanged title commits nothing. Another process's unsaved copy meets the rename as a `save_conflict` on its next save, as with any other writer.
- **Delete.** Body: `{ "request_id", "expected_revision" }`, the revision the user confirmed. The document moves to `<data-dir>/trash/<id>.json` in one rename, so it is either stored or trashed, never both or neither, even if the process ends meanwhile. It is refused, changing nothing, when the open copy has unsaved changes (`409 unsaved_changes`), when an abandoned recovery draft exists (`409 recovery_pending`), when another running process owns its draft (`409 draft_in_use`; if that process takes the draft over while the delete runs, `503 storage_failed` with `reason: "busy"`, and a retry names the draft), or when storage holds a different revision (`409 save_conflict`). The check and the move happen under the document's save lock and its draft owner lock, so no save or draft from another process can slip in between. While a delete or rename is being committed, edits to that document are refused as `503 storage_failed`, `reason: "busy"`, and change nothing. Reply: `{ "api_version", "id", "title" }`. The session ends.
- **After a delete.** A clean copy of the document in another process closes on its next access (`404 document_not_found`) instead of showing it as stored. A copy with unsaved changes keeps them in memory: its save is a `save_conflict` and its autosave `404 document_not_found`, so it can never re-create the document or leave a draft without it; saving it as a copy keeps the work. The CLI cannot see another process's unsaved work that has no draft yet, so such a delete succeeds and that work is kept only through save as copy.
- **Trash and restore.** `GET /api/v1/trash` lists trashed documents (at most the 1000 most recently deleted), most recently deleted first, as `{ id, title, revision, atom_count, bond_count, deleted_at_ms }` plus `unreadable` and `truncated` (`true` exactly when the cap left trashed documents unread). Restore body: `{ "request_id" }`; it moves the document back as it was trashed, with its identifier, and replies `{ "api_version", "id", "title" }`. A document not in the trash is `404 document_not_found`. Identifiers of trashed documents are never given to new documents. Nothing is ever removed from the trash: there is no permanent delete or automatic emptying yet.

## Browser canvas viewport (P31)

This browser-only presentation state stores a center in document points and a uniform scale in CSS pixels per point. It belongs to the mounted canvas in one browser tab: it is not a document setting, preference, HTTP/CLI operation, save/recovery field or undo entry. Navigation sends no model commands and changes no document revision, coordinates or dirty state. No viewport state is persisted across a reload or document switch. The bounded P31 scope is complete in [PR #150](https://github.com/EunhakChoi/chemifree/pull/150), merged as `7fb7ae99a5dff9c8e70f16f0aecdd3f6e1ab1258`; [merged-main CI](https://github.com/EunhakChoi/chemifree/actions/runs/42084338264) passed `check` and `browser`. Checks, review dispositions and verification limits are recorded in [the completion record](verification/p31-viewport-completion.md).

Initial/open fit and the explicit **Fit drawing** action use `geometry.viewBox`: its existing empty 400×240-point frame, atom positions and label boxes, placement frames, scene graphic points, and enhanced-group labels while shown. Fit adds no second margin and keeps the aspect ratio. Edits, imports, undo/redo and container resizing preserve the current center and scale, so new or moved content may need an explicit Fit. A zero-size container defers its first fit until it has positive dimensions; invalid or singular screen transforms produce no edit position. Scale is clamped to 1/4096–32 CSS pixels per document point. **Reset zoom (100%)** sets one CSS pixel per point while keeping the center; it is different from fitting the drawing. No physical monitor-size calibration is claimed.

Controls:

- Toolbar: **Zoom in**, **Zoom out**, **Fit drawing**, **Reset zoom (100%)**, and the zoom-level readout. The tool group has **Pan view** (`H`). Toolbar zoom acts at the canvas center.
- Mouse/trackpad: Ctrl or ⌘ plus wheel zooms about the pointer, including when a scale limit is reached. Ordinary wheel events retain normal page scrolling. Middle-button drag, Space plus primary-button drag, or dragging with Pan view moves the view.
- Keyboard with the canvas focused: `+`/`=` zooms by 1.25, `−` by 0.8, `F` fits, `0` resets scale, and arrow keys pan the view by 40 CSS pixels per key press. Text inputs and dialogs keep their normal editing behavior. Space temporarily enables pan; Escape, blur, pointer cancellation or lost capture ends an active canvas gesture without committing its preview.
- Touch: Pan view supports one-pointer dragging and the toolbar supplies zoom/fit/reset. Extra pointers do not start another active gesture. Pinch zoom, multi-touch navigation, rotation and momentum are not implemented.

Pointer edits use the inverse SVG screen transform to return to document points, retaining the existing 0.01-point rounding. Click-versus-drag uses 3 CSS pixels of pointer travel. Atom and bond hit testing uses nominal 9-pixel and 5-pixel tolerances at every scale, retaining label-box selection and the existing short-bond endpoint cap/tie rules; graphic selection underlays also adapt to screen scale. Navigation itself does not change selection or chemical geometry; the existing Escape selection-clearing shortcut remains available. Pending gesture previews are not saved as document content.

## Guarantees and known limits

Reviews and changes are checked against these lists. Breaking a guarantee is a defect. A known limit is current scope, not a defect; lifting one is a separate change.

Guarantees:

- **All or nothing.** A transaction, undo, redo, save, or close either takes full effect or changes nothing.
- **No lost updates.** A save never replaces a commit its session did not start from; it is refused as `save_conflict`. This holds across processes: saves of one document are serialized by an OS lock from the base check through the commit.
- **No lock-out.** A process that ends while saving, however it ends, does not block later saves.
- **Intact on failure.** A failed save leaves the previous commit intact and the unsaved changes in the session. Success is reported only after the commit.
- **Reopenable.** A committed document can be opened again. A document that could not be read back is refused, never written. The same holds for recovery drafts.
- **Recoverable work.** Unsaved work autosaved to a draft survives the end of its process and can be recovered with its undo history, or saved as a copy if the document was saved elsewhere since. A draft is never overwritten by other work before it is recovered or discarded.
- **Drafted without a client.** Within one running API process, unsaved work in a session is in its recovery draft about 10 seconds (plus a quarter-second check interval and the write) after the first change the draft does not hold, while storage accepts the write and no earlier draft blocks it, and at a normal stop. The service's periodic check does not wait for a document whose turn is taken (by a save or a client's autosave, say): it tries that document again on the next check, a quarter-second later, without counting a failed attempt, so a busy document never delays the drafts of others. Its own draft waits until the turn is free; a save that is running leaves nothing to draft once it commits. At a normal stop, no request is running, and each draft waits up to 3 seconds for its turn. A session closed before the service drafts it is not opened again to be drafted.
- **No resurrection.** After a save or an explicit discard, that draft does not come back, even if the process ends in between.
- **Nothing deleted silently.** A delete never takes unsaved work with it: it is refused while any copy in the same process or any recovery draft holds such work, and a trashed document can be restored as it was. No stale session or draft re-creates a deleted document.
- **Originals kept on upgrade.** Reading a document or draft of an earlier file version never writes. The first save that replaces a version 1–25 document, and recovery or interrupted-save cleanup of an older draft, first keep its exact bytes (see Document file format).
- **Independent copies.** A copy has its own ID and file; saving or editing either document never changes the other, and the source's saved file is untouched by the copy.
- **At most once per process.** Within one API process, a repeated `request_id` with the same body takes effect at most once (within the replay window above).
- **No reuse per process.** Within one API process, a revision or atom/bond ID that a client has seen, including in the document list, never names different content. Recovery keeps to this: it continues numbering, or refers the draft to a copy.
- **Honest outcomes.** Clients report success only when the service confirmed it. An outcome that cannot be known is reported as unknown, not resent where a resend could act twice. A save whose reply was lost is not reported as failed: its outcome is asked for with the same `request_id` (answered from the replay window; see Outcome of a lost reply).
- **Bounded waits.** Within one API process, a request that commits a document or touches its draft waits at most 3 seconds for its document's turn and at most 5 seconds for another process's save lock, and a repeated `request_id` that is still running is answered at once. A request refused for waiting too long changes nothing.

Known limits:

- **Restarts.** A process keeps working copies, replay logs, and the numbers it handed out in memory only; unsaved work survives only as far as its last recovery draft. After a restart, a retried request may run again and a revision may name different content than before. The web client therefore resends an edit only when the restarted service holds the same drawing, never an undo, redo, or an unconfirmed create or copy. An unconfirmed save is not asked about again there either; the page saves anew only if the restarted service already holds the drawing on screen.
- **Draft window.** Changes made after the last completed draft are lost in a crash (see Autosave and recovery): up to 10 seconds of editing without the web editor, longer while drafting fails.
- **Other writers are detected, not merged.** Saves are serialized and conflicts refused, and a clean copy picks up a newer commit on its next access. There is no live update or merge. Two API processes on one data directory can each keep a working copy of the same document; only one owns its draft, and the other's autosave returns `draft_in_use`.
- **Slow storage.** The waits above are bounded; storage I/O itself is not. Creates, closes, and copies commit while holding the process's session table. So do the file checks that nearly every request makes under it: a clean working copy compares its stored revision (a file stat, except the first check after another writer's commit, which reads the whole file; this process's own commits are recorded when they are made), a response looks up the document's recovery draft (file stats; an abandoned draft is read), a close removes its draft, a save as copy removes the source's draft, a list checks every open clean copy, and the first request for a closed document reads and restores the whole file. A stalled disk (for example a hung network file system) therefore delays other requests of the process, and a client deadline passed meanwhile leaves the outcome unknown. An in-progress answer for a repeated `request_id` does not wait for the table. Draft claim, preservation, recovery-copy cleanup, and discard do not hold it. In-place recovery checks the stored base under the same cross-process lock as a save; a save after that check can still conflict with a later save of the recovered session. An edit made in this process during recovery is kept and makes recovery refuse to replace it. Draft writes (autosave, the service's own drafting, and the flush before a stop) look up the recovery state and write the draft without holding the table (the lookup can read an abandoned draft or settle an interrupted save, so it is storage I/O too; a response for the same document waits for it with the table held), and take their snapshot of the working copy under the table: a stalled draft write delays only requests waiting for the same document's turn (up to the turn wait, then `busy`), and edits made meanwhile are left undrafted, reported by `session.draft_revision` naming the revision written. The service drafts each due document on its own thread, as it does before a stop, so one stalled write does not hold up other documents' drafts (a stop still waits for the stalled write to end).
- **Replay window.** A lost reply is answered from the replay log (the latest 64 successful mutations of an open document; per process for creates, closes, deletes, and restores). Asked about after that, a request runs again; so does a save or autosave asked about after its document's session closed, since those are remembered with the session. A save whose revision has moved on then gets `revision_conflict`.
- **Blocked drafts.** While an earlier session's draft exists, new work on that document is not autosaved; the page shows this and offers the earlier draft's actions. Work left this way in a document the page moves away from stays only in the service's session until the page returns to it.
- **Advisory locks on local disks.** Locks are OS advisory locks honored by Chemifree processes; other programs that write the files are not stopped. File systems that do not implement them (some network shares) do not provide the cross-process guarantees. Lock files under `locks/` and `drafts/*.owner` are left in place by design.
- **Durability.** The rename is the commit and the directory sync is best effort, so a power loss right after a save (or a draft write) may bring back the previous file. A file is never left half-written.
- **Abandoned drafts are kept until someone decides.** Drafts never expire and are never cleaned up automatically. An abandoned draft is listed with its document (`recovery` with `state`, `revision`, `base_revision`, `saved_at_ms`) for as long as it exists, and is removed only by recovering it (it becomes the new session's draft, removed by that session's save or discarding close), recovering it as a copy, or `discard-draft`. A draft whose document file no longer exists is left in place and not listed.
- **Not implemented.** A read-only mode (opening documents read-only, or saving elsewhere when storage refuses writes; read-only storage is only reported, as `reason: "read_only"`), file locking against non-Chemifree programs, and multiple drafts per document.
- **Sizes.** The limits above (atoms, bonds, scene instances, title, commands, history, 64 MiB files, revisions, 64 open sessions) are refused with an error, never truncated.
- **One graph, one page.** A document holds one chemical graph and one page. Groups, basic graphics, and generic references are represented; several graphs and pages, reactions, and specialized object references are not. Independent captions use the P35a–g profile.
- **Write token.** It keeps other websites from writing; it is not user authentication (see below).

## Write protection

The API binds `127.0.0.1` only and never sends CORS headers.

1. Every request must carry a `Host` of `127.0.0.1:<port>` / `localhost:<port>` or the host of an allowed web origin, otherwise `403 forbidden_host` (DNS rebinding defense).
2. `GET /api/v1/session` and all writes reject a disallowed `Origin` and `Sec-Fetch-Site` other than `same-origin`/`none` with `403 forbidden_origin`.
3. Writes require the header returned by `GET /api/v1/session` (`x-chemifree-session: <token>`), otherwise `401 unauthorized`. The token is random per API process; a restarted API issues a new one.
4. Writes require `Content-Type: application/json` (`415 unsupported_media_type`) and at most 256 KiB (`413 payload_too_large`).

Allowed web origins default to `http://127.0.0.1:5173`, `http://localhost:5173`, and the same hosts on port 4173; `--allow-origin` replaces the list and accepts loopback origins only. This keeps other websites from driving the API. It is not user authentication: any local process that can reach the port can obtain the token, so it does not separate local OS users. `authentication` stays `not_implemented`.

## Errors

`{ "api_version": "v1", "code": string, "message": string, "current_revision"?: number, "command_index"?: number, "reason"?: string }`. `reason` comes with `storage_failed` (`read_only | busy | io`), an `invalid_command` that refused a recorded-state combination, scene rule, or setting, and an out-of-range preference as `invalid_request`. The operation contract reports the same `reason` in the diagnostic's `details`.

| HTTP | `code` |
| --- | --- |
| 400 | `invalid_json` |
| 401 | `unauthorized` |
| 403 | `forbidden_host`, `forbidden_origin` |
| 404 | `not_found` (route), `document_not_found`, `draft_not_found` |
| 405 | `method_not_allowed` |
| 409 | `revision_conflict`, `request_id_reused`, `request_in_progress`, `nothing_to_undo`, `nothing_to_redo`, `unsaved_changes`, `save_conflict`, `recovery_pending`, `draft_in_use`, `draft_outdated` |
| 413 | `payload_too_large` |
| 415 | `unsupported_media_type` |
| 422 | `invalid_request` (malformed body or field), `invalid_command` (with `command_index`) |
| 500 | `document_unreadable`, `preferences_unreadable`, `internal_error` |
| 501 | `setting_not_implemented` |
| 503 | `storage_failed`, `resource_limit`; also mapped, but reached by no current route: `time_limit`, `cancelled` (see below) |
| operation reply, HTTP 200 | `unsupported_format` (`document.import` with `format` `svg`; see [SVG drawing export (P38a)](#svg-drawing-export-p38a)) |

`time_limit` and `cancelled` (computation stopped, nothing changed) are diagnostics of operation replies only, which HTTP answers with 200; no v1 route returns them as an HTTP error, and a `cancelled` reply is not delivered over HTTP when its client has gone (it is delivered, as 200, when the server stops and the client is still connected). The error-code mapping in `apps/api/src/lib.rs` does list them as 503, so a future route that returned them as an `ApiError` would answer 503; no current route does. The CLI uses the same codes, exit status 1. `recovery_pending`, `draft_in_use`, `draft_outdated`, `draft_not_found`, the list query, rename, delete, trash, and restore (and `saved_at_ms`), `request_in_progress`, and the `reason` and `session.draft_failure` fields were added within v1; clients should treat unknown codes by their HTTP status. `request_in_progress` is an exception to read carefully: the outcome is not known yet, so it is not a failure. Ask again later with the same `request_id`. Before it existed, a retry of a running request waited for it and then got its reply. A process keeps at most 64 open sessions. When a new one is needed, a clean session is evicted (the lowest document ID; `recover-as-copy` evicts its own source only when no other session is clean), and `resource_limit` is returned only when all hold unsaved changes; nothing is changed then.

## Document file format

Current native30 adds embedded PNG image states; v1–29 forbid images in all
current/history states. See [P36k image admission and limits](#embedded-png-image-profile-p36k-candidate).

P36j native29 introduced exact-four-point open cubic spline admission. V1–28
forbid it in current/history before retention; polygon28/ellipse27/width26 remain
independent strict profiles. See P36j for conservative bounds and shared budgets.

The P36i slice introduced native28 polygon admission; v1–27 forbid polygon in all current/
history graphic states before retention. Shape-specific cardinality remains
strict. The P36i profile above defines shared byte/undo budgeting and preserves
ellipse27+/width26+ admission and exact old archives independently.

P36h adds native27 ellipse admission to the v26 width profile. V1–26 current and
every history graphic state forbid ellipse; v26+ still require nullable width,
and v1–25 forbid width. Old valid states/history migrate without read writes,
with exact-byte archives before replacement/recovery. See the P36h section above.

`<data-dir>/documents/<id>.json` is owner-only on Unix. Version 30 is compact JSON with a final newline. It contains `format: "chemifree.document"`, `format_version: 30`, `document`, and `history`. The document contains its `id`, `revision`, `title`, molecule and one page, ordered `roots`, typed `objects`, directed `references`, instance/atom `positions`, `stereo`, `stereo_groups`, `selection`, `settings`, and `next_atom`, `next_bond`, `next_object`, and `next_group` counters. Instances name `m1`; groups contain ordered child IDs; graphics contain their shape, a shape-checked `points` list (exactly2 for old shapes,3–64 for polygon or4 for spline) and nullable per-graphic `line_width_pt`. Atom and bond states are as in the API, except that a bond state may also hold a stereo `mark` (below). `stereo_groups` is a required list of enhanced-stereo groups `{ "id": "e<n>", "state": { "kind": "absolute" | "and" | "or", "number": <integer> | null, "members": [atom IDs] } }` in ID order (written ascending, read in any order like every list in the file; a member listed twice is refused); `number` is always present and is stored as stated (the full 32-bit integer range, `0` and negatives included, `null` for none), `next_group` is at least one above every group ID in the document or its history, and each history entry holds a required `stereo_groups` list of `{ "id", "before", "after" }` with the whole group or `null`. A document that does not satisfy the stored-group rules is unreadable: every member is an atom with a stored tetrahedral configuration (not `unknown`), a group has at least one member and no member twice, an atom is in at most one group, an `absolute` group has no number and there is at most one, and per kind (`and`, `or`) no two groups have the same number, where no number counts as `0`. The groups are edited and shown as described under [Stereo groups](#stereo-groups).

Every v30 `settings` object, in the current document and both before/after history
states, requires boolean `label_shadow`, `label_outline` and `label_underline`. Versions 1–21 omit shadow and migrate it to false. Versions 1–20
omit outline and migrate it to false. Versions 1–19 omit underline and
migrate it to false; a field hidden behind an earlier header is refused. The
existing `label_weight` requirement starts at v18 and `label_style` at v19;
earlier versions migrate each to normal. Reading accepts versions 1–30 and
refuses future versions, including v31, without rewriting their bytes.

The v9 document also requires `abbreviations: [{ instance, anchor, terminal }]`, including
an empty list when no Et display exists. Every history entry requires `abbreviations` changes
`[{ instance, anchor, before: terminalID|null, after: terminalID|null }]`. Both nullable sides
are mandatory. Duplicate keys/member overlap, invalid current or history-only displays, and
unreplayable display history are unreadable. v1–8 files/drafts migrate with empty display maps
and empty display diffs; v8 captions and caption history remain unchanged. Abbreviation fields
are forbidden behind v1–8 headers even when empty or present only in history. Reading writes
nothing; v8 original bytes receive the same first-overwrite/recovery archive as earlier versions.

Version 23 additionally requires `display_labels: [{ instance, anchor, text }]` in
the current document and `display_labels: [{ instance, anchor, before, after }]`
in every history change set; both nullable history sides are mandatory. Only
nondefault strings are stored: absent metadata means `Et`, and the corresponding
retained-graph display must exist at each replayed boundary. Duplicate/orphan
keys, invalid text, redundant default records and invalid or unreplayable history
are refused. Versions 1–22 forbid this field, including empty or history-only
fields, and migrate with empty cosmetic maps while preserving membership and
history. Reads never write. First overwrite/recovery preserves exact old document
and draft bytes under the existing versioned archive protocol. Future v31 is
refused untouched; the provider retains its 64 MiB physical file limit.

Version 24 additionally requires `fragment_displays: [{ instance, anchor, members, text }]`
in the current document and `fragment_displays: [{ instance, anchor, before, after }]`
in every history change set. Each non-null history side contains `{ members, text }`;
both nullable sides are mandatory. Duplicate IDs/keys, invalid text or membership,
overlaps, invalid current/history-only entries and unreplayable histories refuse.
Versions 1–23 forbid the field even when empty or history-only and migrate with
empty fragment maps and diffs, preserving prior graph, caption, Et and label data.
Reads write nothing; first replacement/recovery archives exact original old
document/draft bytes under the existing versioned archive protocol. Current and
before/after member lists and escaped text count toward the existing 48 MiB
current/8 MiB history admission budgets; the 64 MiB physical provider limit remains.
Version 25 replaces each current fragment display's `text` with required
`runs: [{ text, script }]`; non-null before/after history states likewise contain
`{ members, runs }` without a top-level `text`. Every run requires valid text and
explicit baseline/subscript/superscript. v25 refuses the old top-level text even
when null; v24 requires text and forbids runs even when empty/null or history-only.
v24 current and historical text migrates to one baseline run without changing
membership or graph/history. Earlier v1–23 retain their empty-map migration.
One baseline run retains v24 admission weights, and each additional run reserves
64 bytes of metadata in current and both history sides. Existing 48 MiB current,
8 MiB history and 64 MiB physical bounds apply. Reading writes nothing; the
existing exact-byte archive protocol now includes v24 documents/drafts before
first overwrite/recovery. Future v31 refuses untouched. Clipboard/template
formats do not gain this map.

Version 26 additionally requires nullable `line_width_pt` on every current and
before/after historical graphic state in documents and drafts. Null means dynamic
document inheritance; numbers must be finite within0.1–10 pt. Missing/invalid
current or history-only fields and invalid records beyond retention limits refuse.
Versions1–25 forbid the field on graphic states even when null or history-only
and migrate every graphic boundary to inheritance. Read does not write; first
replacement/recovery archives exact old document/draft bytes, now including v25,
under the existing protocol. Future v31 refuses untouched. Existing 256-byte
graphic reservations and physical64MiB limits remain; current/history capacity
fixtures exercise long-float widths within those reservations. Clipboard v2 still forbids a per-graphic width field; P36e adds the separate v3
payload for preserving explicit widths; P36h separately adds native27 ellipse
admission and graphics clipboard v4 as described above.

Stereo is stored and shown, and the four stereo commands set, clear and (from the drawing, when sent) derive it over HTTP, the CLI and operation contract 1 (see Stereo marks and configurations); the web editor draws marks and sets them with its bond tool and bond inspector (see Web editor). One command, `set_configuration_from_drawing`, stores a configuration read off the marks and positions when it is sent (a stored record is an ordinary record, so the file format is unchanged), the read-only operation `chem.stereo` compares the stored configurations with the drawing, the read-only operation `chem.cip` computes CIP descriptors from the stored configurations (Rules 1a, 1b, 2 and 3 only, see [CIP labels](#cip-labels)), and chemical stereo is otherwise unimplemented (no stereogenicity judgement beyond those labels, SMILES `@` and `@@` and CXSMILES `a:`, `o<n>:`, `&<n>:` are read and supported configurations and groups are written in recorded SMILES (see the SMILES section); enhanced-stereo groups are stored, edited by eight commands and shown in the snapshot over HTTP, the CLI and operation contract 1 (see [Stereo groups](#stereo-groups)), and CIP, identity and substructure ignore them; MOL V3000 reads and writes the groups (as `COLLECTION` entries, see Enhanced stereo collections) and CDXML and CDX import reads them and bounded drawing export writes groups whose members derive from the output drawing (see CDXML interchange); V2000 carries none, recorded SMILES writes supported groups as CXSMILES while canonical SMILES and V2000 export require consent to drop them; MOL import reads V3000 marks and derives configurations, CDXML import reads marks and derives configurations too, MOL export writes marks, and export drops or asks consent for stereo and groups as stated under MOL and SMILES). A `mark` is `{"kind": "wedge" | "hash", "narrow": "begin" | "end"}`, `{"kind": "wavy"}`, or `{"kind": "crossed"}`; the narrow end is recorded in the mark and a bond's `begin` and `end` are never swapped. A wedge, hash, or wavy mark is allowed only on a single bond and a crossed mark only on a double bond. `stereo` is a required list of `{ "id", "state" }` records in key order (centers first, then double bonds, each ascending), at most one per stereo center (an atom ID) or double bond (a bond ID). A center state is `{"kind": "center", "state": {"mode": "tetrahedral", "neighbors": [four slots], "turn": "clockwise" | "anticlockwise"}}` or the same with `"mode": "unknown"` and no `turn`; a slot is an atom ID, `implicit_hydrogen`, or `lone_pair`, and the four slots are distinct, in canonical order (atom IDs ascending, then `implicit_hydrogen`, then `lone_pair`), with the turn seen from the first slot (`anticlockwise` is the OpenSMILES `@` handedness). A double-bond state is `{"kind": "double_bond", "state": {"mode": "relative", "begin_ref", "end_ref", "relation": "same_side" | "opposite_sides"}}` or `{"mode": "unknown"}`; each reference is the lowest-ID other neighbor of its end, or `implicit_hydrogen` where the end has none. The two positions of an sp2 end other than the double bond are complementary, not interchangeable labels: with one other atom `n`, the hydrogen (or lone pair) is the other position, so a state naming it is stored as `n` with the relation flipped; with none, a lone pair is stored as `implicit_hydrogen` with the relation flipped. One older form is accepted and normalized on load: the first version 6 build also stored a `lone_pair` reference at an end with no other atom, so a version 6 document, draft, or history entry (before or after side) that holds one is read as `implicit_hydrogen` with the relation flipped for each such end, the same rule as above; reading does not rewrite the file, and the format version does not change. Any other stored record that is not in this form is rejected on load, and the domain refuses to state a hydrogen or lone pair at an end with two other atoms. Whether a position is a hydrogen or a lone pair is a label derived when a configuration is derived, not stored. The file's names differ from the snapshot's: `mode` is `kind`, `begin_ref` and `end_ref` are `begin_neighbor` and `end_neighbor`, and the file's wrapper `kind` (`center` or `double_bond`) and `id` are the snapshot's `atoms[].atom` and `bonds[].bond` lists; the words `tetrahedral`, `unknown`, `relative`, `same_side`, `opposite_sides`, `clockwise`, `anticlockwise` and the slot names are the same. When a transaction changes the neighbors of an atom (adding, deleting, or reconnecting a bond, or deleting a neighbor), or deletes an atom or bond, the records of that atom as a center and of double bonds that end at it, and the record of a deleted bond, are removed in the same transaction, and undo restores them. A mark goes away with its bond, and changing the order of a bond so that its mark or its stored record no longer fits is refused with the rule it breaks. A mark and a stated configuration are separate statements and are never validated against each other when written: a crossed mark may sit on a double bond that has a stated relative configuration, and a wedge or hash on a bond at a stated center, whichever is set first, and `set_bond_mark`, `set_atom_configuration`, `set_bond_configuration` and any other edit never refuse or remove one because of the other. The one command that reads marks, `set_configuration_from_drawing`, replaces the stored record of each item it processes with what the drawing shows (a configuration stated by hand, `unknown` and a different label included) and writes nothing for an item it skips. Comparing the drawn mark with the stated configuration is not done when anything is written; it is the read-only operation `chem.stereo`, which edits and stores nothing.

Each history change set records the title, atoms, bonds, page roots, scene objects, references, positions, stereo records, settings, and before/after selections. Undo is oldest first; the last redo entry is redone next. References record their presence before and after; entity changes use `null` for an absent entity. A selection without a content change is not a history entry.

Loading rejects unknown fields or versions, files over 64 MiB, invalid atom and bond states, a mark on a bond order that cannot carry it, stereo records whose atoms, bonds, neighbors, or order do not fit the graph they are stored with, repeated or dangling entities and references, malformed trees, incorrect positions, counters that could reuse IDs, and history that cannot replay in both directions. It also rejects a stored selection that matches neither the newest undo entry after an edit or redo nor the nearest redo entry before an undo. Both boundaries can be valid after undo, undo, redo. History cannot change a scene object kind or graphic shape or reuse a deleted entity ID. History beyond the limits is trimmed oldest undo first and then farthest redo; when every undo entry is trimmed, selection follows the nearest redo boundary. A rejected file is listed as unreadable and left untouched. Finite floats round-trip bit-exactly. This is a working format, not MOL, SDF, CDXML, or SMILES.

Versions 1–25 documents and drafts are read and migrated in memory, including history. Version 7 has no captions. Captions in a v1–7 current or history state are refused, rather than reinterpreted as a newer file. Version 1 combined atoms and their single positions; migration separates graph and `s1` positions. Version 2 had only element atoms and single/double/triple bonds; migration fills recorded-state defaults. Version 3 already holds those recorded states and separate instances. Versions 2 and 3 reconstruct page roots at each historical boundary in instance ID order; all three start with empty selection and references. Version 4 already holds the scene tree, references and selection; migration preserves those and adds the built-in style. Version 5 already holds the settings; migration adds no marks and no stereo records, and a version 5 file that contains either is refused. Version 6 already holds those; migration adds an empty group list and `next_group` 1, and a version 6 file that contains `stereo_groups` or `next_group`, in the document or in a history entry, is refused (as is a version 7 file that omits them). History that exceeds the current change limit is trimmed as above. Reading never writes.

Before the first save over a version 1–25 document, under the save lock, the provider writes and flushes the exact old bytes to `migrated/<id>.v<version>.json` before replacing the document. A failed copy fails the save and preserves the old file. A different old file with the same ID gets the next free numbered name; identical bytes are not copied again. These backups are not listed or treated as documents. Unknown versions are refused and left untouched.

A recovery draft uses `format: "chemifree.draft"`, version 29, `base_revision`, `saved_at_ms`, and the same `document` and `history` shapes. Old drafts migrate in memory and inspection writes nothing. Recovery can retain an already complete draft in its earlier version; an autosave with no new undrafted edit does not rewrite it. The next actual draft write uses the current version. Before recovery or recovery as a copy can rewrite or remove an old draft, the provider keeps the exact bytes it claimed under `migrated/<id>.draft.v<version>.json`; different bytes get numbered copies and identical bytes are not duplicated. If preservation fails, recovery fails and the draft stays abandoned. Explicit discard removes the draft without creating a copy. An old draft left as `drafts/<id>.committing` by an interrupted successful save is preserved before cleanup. Other files include `trash/<id>.json` and `trash/<id>.deleted`, migrated copies, `locks/<id>.lock`, `drafts/<id>.owner`, `drafts/<id>.committing`, and abandoned temporary files. Draft ownership and save locking remain distinct.

Version 11 additionally requires caption `alignment` in every current and historical scene
state, including redo-only captions; valid values are `left`, `center`, `right`. Versions 8–10
migrate every caption boundary to left without rewriting source bytes. Alignment fields under
v1–10 headers are refused even in history. V11 refuses absent/null/wrong-type/unknown values.
The existing first-overwrite document and pre-recovery draft archives also cover v10. V10–12
continue to require run weight/style; v8–9 fill normal and forbid those fields under old headers.

Version 12 requires each caption run's `font` in current and every history state: `null`
means inherit the document label font; otherwise only `sans-serif`, `serif`, `monospace`
are valid. Versions 8–11 migrate every run to inheritance. A `font` field (including null)
under any v1–11 header is refused, including history-only captions. V12–14 refuse omission,
unknown families and wrong types. Exact first-overwrite document and pre-recovery draft
archives include v11. Reading never rewrites; weight/style and alignment requirements remain.

Version 13 requires `size_pt` on every caption run in current and all undo/redo states,
including redo-only captions, in both documents and drafts. Null inherits the document size;
a number must be finite and in the existing `LABEL_SIZE_RANGE` (4–72 pt inclusive).
Omission and wrong types are unreadable in v13–15. Versions 8–12 migrate current and all history
runs to null inheritance; a `size_pt` field under any v1–12 header is refused, even when null
or present only in history. Reading never rewrites bytes; the first actual overwrite and
recovery preserve the exact older bytes through the existing migrated archive protocol.
The 64 MiB file limit is unchanged. V13 increased the metadata reservation from 96 to
128 bytes; v14 supersedes that reservation with the recounted bound below.
The current-content and history budgets remain 48 and 8 MiB respectively.

Version 14 requires `color` on every caption run in current and all undo/redo states,
including redo-only captions, in documents and drafts. Null inherits theme foreground.
Overrides must be canonical opaque sRGB `#RRGGBB` strings with uppercase ASCII hex digits.
Omission, malformed strings, alpha and wrong types are unreadable. Versions 8–13 migrate
all runs to null inheritance; `color` under any v1–13 header is refused, even null or
history-only. Reading never rewrites; first overwrite and pre-recovery archives preserve
exact earlier bytes, including v13. The 64 MiB limit and 48/8 MiB budgets stay unchanged.
Run metadata now reserves 160 bytes. Its actual maximum is 136: 89 for the longest
text/script/weight/style/font metadata, 29 for the size field with an 18-character bounded
float, and 18 for `,"color":"#RRGGBB"`, exceeding the former 128-byte reservation.

Version 15 requires a boolean `underline` on every current and historical caption run,
including redo-only states, in both documents and drafts. Omission, null and all other
non-boolean values are unreadable. Versions 8–14 migrate every run to false and refuse
any underline field under their old header. Reading writes nothing; the existing exact-byte
first-overwrite document and pre-recovery draft archives include v14. The additional
`,"underline":false` is 18 bytes, so maximum metadata is 154 and the existing 160-byte
reservation, 48/8 MiB budgets and 64 MiB read limit remain unchanged.

Version 16 additionally requires a boolean `outline` on every current and historical
caption run in documents and drafts. Omission, null and non-booleans are unreadable.
Versions 8–15 migrate all outlines to false; any outline field behind an older header
is refused, including history-only states. Reads preserve original bytes; first-overwrite
and pre-recovery archives now also include v15. The additional `,"outline":false` is
16 bytes, giving 170 maximum compact JSON metadata bytes per run (154+16). The reservation
is split from admission accounting. Physical encoding reserves 192 bytes per run,
which bounds the v17 185-byte maximum. The 48 MiB content and 8 MiB history limits now
mean admission budgets: outline=false retains the v15 160-byte run weight, while
outline=true counts 192 bytes. Shadow=true counts 224 bytes; shadow=false keeps these v16 admission weights. All other weights, text escape reservations and fixed
content/history envelopes are unchanged. This preserves every previously admitted v15
content/history state, including 109 maximal before/after updates in undo or redo,
without silently dropping an entry during migration. A wholly outlined maximal 32-run
before/after update counts 78,400 admission bytes; with the 576-byte history envelope,
106 updates fit 8 MiB and 107 refuse atomically. Outline=false retains the old 109/110
boundary, even with the longest v15 font/style/size/color metadata.
History physical reservation is at most admission ×192/160=6/5. Current content has at most32,000 runs, so its physical overhead is at most1,024,000 bytes above admission. Thus the complete file reserves at most48MiB+1,024,000+8MiB×6/5=61,421,978 bytes, below64MiB with more than5MiB spare including fixed metadata envelopes. `Document::persistence_weight` returns this physical encoding
bound, which may exceed 56 MiB; it is never used as the history admission weight. The 1,000 scene-object
ceiling (including the initial instance) limits a pure-caption population to 999; its
content reservation is below 48 MiB, so a pure-caption exact 48 MiB boundary is unreachable.

## Independent Unicode captions (P35a–h)

A caption is a new independent scene object, not an atom, abbreviation, chemical formula,
CIP overlay or graph replacement. Its snapshot is
`{ kind: "caption", id, parent, anchor: { x, y }, alignment: "left" | "center" | "right", runs: [{ text, script, weight, style, font, size_pt, color, underline, outline }] }`.
It shares `sN` IDs, ordered containment, references and selection with other scene objects.
CLI text output lists the caption ID, parent, anchor, alignment and each ordered `script/weight/style/font/size/color/underline/outline("text")` run. Size is `inherit` or an explicit `Npt`; color is `inherit` or a canonical opaque sRGB `#RRGGBB` override, including explicit `#000000` black.
Run text uses Rust ASCII escapes (including Unicode scalar escapes), so quotes, backslashes
and directional/format characters cannot disguise terminal output. JSON output retains the
original Unicode strings and script/weight/style/font/size_pt/color/underline/outline fields without that presentation escaping.
The transaction catalog includes `add_caption` and `update_caption`; with the Et display
commands it contains 41 commands and 11 capabilities, 52 sub-operation rows and no new
top-level operation.

- `add_caption`: required `anchor`, optional `alignment` (default `left`), required `runs`, optional group `parent` and insertion
  `index`, with the same meaning as `add_graphic`. It can be selected as `$N` in
  `selection_after`.
- `update_caption`: required existing caption `object`, complete `anchor` and complete
  `runs`, optional `alignment` (default `left`). It replaces those values atomically. Other object kinds are refused. Setting an
  identical state is a no-op. Both commands use ordinary expected-revision, atomic rollback,
  request replay, selection-aware history, save, drafts and recovery.
- P35c `alignment` is physical `left` | `center` | `right` for the whole single line at
  its unchanged baseline anchor. Changing alignment does not move that anchor or change runs.
  Both add and update requests default omission to `left`; update is a complete replacement,
  so an older client editing a centered/right caption resets it to left. Snapshots always
  carry alignment. Null, unknown strings and wrong types are `invalid_request`. Current web
  clients require explicit valid alignment in replies; older web clients may ignore this added
  field and render captions as left aligned. Upgrade the web client together with the server
  for accurate display. HTTP/CLI JSON use the same application DTO and replay comparison.
- P35d `font` is a per-run optional generic family override using existing `label_font`
  vocabulary: `sans-serif`, `serif`, `monospace`. Null or request omission inherits the
  document setting dynamically; snapshots always emit `font` (null for inheritance).
  An inherited run follows later document font changes; an explicit family stays fixed.
  Add/update replace complete runs, so omitted font resets the run to inheritance.
  Unknown names (including `inherit`), arrays, objects and other wrong types are
  `invalid_request`, with no edit. Font-only changes are ordinary undoable/replayable edits.
  Web clients accept omitted font in older replies as inheritance. Older clients may ignore
  the new field and replace overrides with inheritance on edit; update client and service
  together for accurate display. The sans-serif override uses the same theme sans stack as
  document sans-serif labels; serif/monospace use browser generic families. No arbitrary
  installed-font names or atom/Et label overrides are introduced.
- P35e `size_pt` is a nullable per-run size override, finite 4–72 pt inclusive using the
  same `LABEL_SIZE_RANGE` as document labels. Null or omission in requests inherits the
  document's `label_size_pt` dynamically; snapshots emit explicit null. Add/update replace
  complete runs, so omission resets an override to inheritance. Number values outside the
  range are `invalid_command` with `caption_invalid` and `command_index`; wrong types are
  `invalid_request`. Nonfinite JSON numbers are malformed requests. Both refusals preserve
  the entire transaction. Size-only changes participate in replay, undo/redo and native
  save/recovery. Omitted size in older server replies is accepted as inheritance by the web
  client. Older clients may ignore overrides or reset them on edit; update client and service
  together for accurate display. Baseline text renders at the effective run size. Subscript
  and superscript render at two thirds of it, with vertical offsets of +1/3 and −5/12 of
  that effective run size. Alignment uses the measured total SVG advance across mixed sizes,
  styles and families, including edge spaces; hit regions and fit use measured ink.
  The form accepts a blank size as inheritance and decimal input (optional sign, decimal
  point and exponent) for overrides; hexadecimal/binary/octal prefixes, invalid numeric
  text and out-of-range sizes are rejected without silently substituting a size. Atom/Et labels, wrapping and clipboard are outside
  this slice.
- P35f `color` is nullable per run. Null or omission follows the current theme foreground
  dynamically; explicit canonical opaque sRGB `#RRGGBB` with uppercase hex digits stays
  fixed through theme changes (including explicit `#000000`, distinct from inheritance).
  Snapshots always emit color. Complete-run updates reset omitted color to inheritance.
  Malformed strings, lowercase/noncanonical hex, alpha, CSS names/expressions and palette
  references are `invalid_command` with `caption_invalid`; wrong types are `invalid_request`.
  The entire transaction rolls back on refusal. Color-only edits use ordinary replay,
  undo/redo, save/draft/recovery and loss reporting. Older replies may omit color and the
  web treats this as inheritance; older clients may ignore/reset overrides, so update
  client and service together. Color is applied to the caption tspan and underline fill; glyph
  geometry, alignment, script sizing/offsets, hit regions and fit are unchanged. No
  arbitrary palette registration, alpha, atom/Et label color or external caption roundtrip
  is provided. The form accepts blank inheritance or the exact canonical spelling.
- P35g `underline` is an independent boolean appearance flag, freely combined with all
  scripts, weights, styles, sizes, fonts, colors and alignments. Omission in a complete-run
  command resets it to false; snapshots always emit it. Null and non-booleans are
  `invalid_request` and preserve the entire transaction. Underline-only edits use normal
  revision checks, replay, undo/redo, native save/draft/recovery and export loss handling.
  Older replies may omit underline and render as false; malformed values are refused.
  Older clients may ignore/reset it, so update client and service together.
  The browser draws an explicit filled rectangle for each enabled run, in its color.
  Its horizontal interval is the minimum/maximum of actual SVG character start/end
  positions, including preserved whitespace and native RTL/cluster shaping. Its top is
  the run baseline plus rendered size / 12; thickness is rendered size / 16. These are
  Chemifree's explicit decoration dimensions, independent of font underline tables.
  Zero-advance runs produce no painted width. Glyph shaping, character advance and the
  fixed alignment anchor remain unchanged. The measured union of glyph and rectangle
  ink enters fit, selection and hit bounds; text getBBox is never assumed to include
  decoration. The form offers Off/On per run. Atom/Et labels and chemical interpretation
  are unchanged; external caption roundtrip is still unsupported and export losses apply
  to the whole caption, including underline.
- P35h `outline` is an independent boolean, default false on omitted requests and
  older replies; null/non-booleans are invalid requests. Snapshots emit it explicitly.
  Complete-run replacement resets omitted outline to false. Outline-only edits use the
  same replay comparison, rollback, undo/redo, save/draft/recovery and export loss rules.
  The UI offers Off/On per run. Enabled runs use hollow SVG glyphs (`fill: none`) with
  their resolved run color as stroke; inherited stroke follows theme foreground and
  explicit opaque colors stay fixed. Filled siblings have no stroke. Inline run paint
  prevents the inherited theme fill from replacing hollow fill. Underlines stay filled
  rectangles in their existing color and geometry. No atom/Et label behavior changes.
  Stroke width is rendered glyph size / 16 with round joins, a Chemifree policy chosen
  independently of any external font/application effects. Script scaling applies before
  computing stroke width. Text, character advances, fixed anchor, weight, style, font,
  size, script and underline positions are preserved; browser shaping remains native.
  Since SVG getBBox excludes strokes, each non-whitespace, nonzero-ink outlined run box
  expands by rendered size / 32 on all sides before union with the other glyphs and
  explicit underlines. Existing 2 pt hit padding is applied after this union; fit,
  selection and hit use the same result. Zero advance does not suppress combining-mark
  ink; empty/whitespace/zero-ink runs add no outline margin. RTL, ligatures and emoji use
  the browser-shaped run box. Document edits and font/viewport changes invalidate this
  measurement through the existing canvas mechanism. Older clients may ignore/reset
  outline; update client and server together. External caption roundtrip remains unsupported.
- A run stores a nonempty Unicode string and exactly one `script`: `baseline`, `subscript`
  or `superscript`. P35b adds independent `weight` (`normal` | `bold`) and `style`
  (`normal` | `italic`), freely combined with every script. Older API requests may omit either
  new field, defaulting it to `normal`; snapshots always include both. Null, numeric CSS
  weights, `oblique`, unknown values and wrong types are `invalid_request`. There are 1–32 runs and at most 16384 UTF-8 text bytes in total; at least
  one Unicode scalar must not have the Unicode White_Space property. No text is trimmed,
  normalized, parsed as chemistry, merged across runs or converted into Unicode subscript
  glyphs. Literal `H₂` and baseline `H` plus subscript `2` remain distinct. U+FEFF is not
  White_Space. Control characters (Unicode Cc), U+2028 and U+2029 are refused; this is a
  single-line profile. Rust strings and the browser reject unpaired UTF-16 surrogates.
- Malformed/missing fields, unknown script values and wrong types are `invalid_request`.
  Validly typed but empty/over-limit/control-bearing content or out-of-range run size is `invalid_command` with
  `reason: "caption_invalid"` and `command_index`. Unknown objects and kind mismatches use
  the existing scene reasons. No refused transaction changes IDs, history or content.

Captions can be selected in Scene or by a canvas click, grouped/ungrouped, reordered,
deleted, referenced and translated. `move_objects` and `move_selection` translate the
anchor exactly once across overlapping selected subtrees. Captions do not enter atom,
formula, stereo, identity or substructure interpretation. Selection-scope `objects` includes
captions; the existing `graphics` field continues to name two-point graphics only.
Arrangement and rotation/reflection previews and mutations refuse a selected caption,
including one nested in a group, with `caption_unsupported`; no surviving subset is edited.
The graphic clipboard profile refuses the whole forest with
`clipboard_scene_caption_unsupported`. Native graph/template payloads are unchanged.
Marquee still selects atom centers and complete two-point graphics only; use Scene or
click to select a caption. Automatic text layout, text clipping, wrapping, arbitrary
justification, paragraph-direction editing and caption clipboard remain unsupported.

The web **Add caption…** and **Edit selected caption…** forms edit whole runs through ordinary
inputs, with whole-caption alignment, script, weight, style, outline, underline, size, color and font family controls and explicit point coordinates (new caption initially at
0,0). IME composition must finish before submission. Inputs and target state are captured
at submission; a changed/missing target or document makes the queued edit lapse instead of
retargeting it. Document/service changes remount the panel. Ordinary editor mutation and
restart reconciliation rules apply. No OS clipboard access or HTML interpretation is added.

Display is a horizontal line using each run's effective size and font and a fixed baseline
anchor. Left starts the complete advance there, center places its midpoint there, and right
places its end there. Advance includes preserved leading/trailing spaces, every script run
and size/bold/italic/family metrics; it is not the glyph-ink box. The canvas measures the complete SVG
advance after rendering/fonts load and shifts all chunks together, without stretching text.
SVG `text-anchor` on y-separated chunks would incorrectly align runs independently and is
not used for center/right. Paragraph direction is explicitly LTR, independent of surrounding
page direction: left/right never mean bidi start/end. Browser-native bidi/shaping within
runs is retained, with runs flowing in stored order; full RTL paragraph editing, cross-run
bidi layout and portable complex-script typography are unsupported.
Script runs use two-thirds of their effective run size, subscript +one-third of that size in y
and superscript minus five-twelfths of that size. These are explicit Chemifree display choices, not vendor-derived metrics.
Each SVG run explicitly sets `font-weight` and `font-style`. Browser font fallback or synthetic
bold/italic may vary; these fields never change document label styling or atom labels.
The SVG keeps text at natural advance, preserves whitespace and uses text nodes. The canvas
measures actual SVG glyph ink after rendering and font loading for view-box inclusion,
selection indication and hit area. A new document's first fit waits for its current caption
measurements, font completion and positive canvas size. The opening fit records the SVG
viewBox of each measurement and confirms containment using a measurement in its resulting
viewport; if realized glyph metrics expanded, only this initial phase widens the union of
observed bounds and fits again. Document switches discard the old pending fit. Confirmation,
a new edit snapshot or a user choosing Fit/pan/zoom ends that initial correction authority;
later measurements and font completion then preserve the chosen viewport. Font fallback, glyph availability, complex shaping and
cross-run combining/emoji shaping are not portable visual-roundtrip guarantees. Avoid
splitting such sequences across runs. Native persistence preserves the exact scalar/run
sequence independently of visual accuracy in a given environment.

Caption-bearing documents (a caption in current content, undo or redo) additionally use a
conservative persistence reservation: current content at most 48 MiB and both history stacks
at most 8 MiB combined, below the native 64 MiB file limit even with document/draft envelopes.
The reservation accounts for all entity kinds, IDs, floats, roots, references, both selections
and complete before/after states, including current Et displays and their history diffs. Caption/title strings reserve twice their UTF-8 bytes for
JSON quote/backslash escaping; every run also reserves 192 physical encoding metadata bytes, covering v17 shadow and v16 outline, underline, color, size, font, weight/style and script (admission counts 160 for outline=false, 192 for true as above); the 256-byte caption overhead covers alignment. It is an upper bound, not
an exact file/memory size. The existing 100-entry/100000-change limits also apply.
Older undo entries are dropped first; restore then drops farthest redo entries when needed.
A latest transaction whose own history cannot fit is refused atomically as `invalid_command`
with `reason: "caption_persistence_limit"` and no command index; no accepted newest edit is
silently made non-undoable. Thus 500 commands is an independent ceiling, not permission for
500 maximal caption changes; even a small group-move request can exceed the byte reservation.
Use smaller edit scopes. Text is never truncated. Undo/Redo move the same budgeted entries
between stacks. Loading looser v8 history applies the same trimming in memory without writing;
caption-free v1–7 migrations and their existing history limits remain unchanged.

P35a introduced native v8 captions and v9 added Et display metadata. V10–12 additionally
require explicit caption weight/style on every current and history run. Versions 8–9
migrate missing weight/style to normal; fields present behind a v1–9 header are refused,
including history-only captions. A v10–12 missing/null field is refused. V11 adds the strict whole-caption alignment described above. Earlier versions remain readable without
rewriting; the first overwrite/recovery keeps original bytes under the existing migration
rules. Captions currently have no chemical-format mapping. MOL/SDF, SMILES and bounded
CDXML/CDX drawing export report each caption as a dropped `caption` rendering loss and
require digest-bound consent before returning content. Rich-tree CDXML/CDX interchange
continues its separate existing contract and does not become editor-caption roundtrip.
Historical implementation-session regression scope and then-unexecuted checks are recorded in
[the P35a verification record](verification/p35a-unicode-caption.md) and
[the P35b verification record](verification/p35b-caption-weight-style.md),
[the P35d verification record](verification/p35d-caption-font-family.md) and
[the P35c verification record](verification/p35c-caption-alignment.md). Current merged
PR/CI evidence and bounded P35 completion are recorded in the [audit](verification/p35-completion-audit.md).

## Compatibility and verification

Additive fields, endpoints, commands, and capability IDs may be introduced within v1; consumers ignore unknown response fields. Removing or renaming fields, changing types, or adding status values requires a reviewed version change and synchronized Rust/TypeScript updates. The browser rejects unknown versions, malformed records (a field the browser does not know is ignored, except inside a `stored` record, which must equal the snapshot's own record; the native clipboard format stays closed), duplicate IDs, bonds that reference missing atoms, recorded states that break the rules above or come for some atoms and bonds but not others (a reply has them all or none), and scenes that break the rules above (an instance without a position for every atom or with one for an unknown atom, a repeated instance, another page or molecule, no instance, `molecule` without `scene` or the reverse, or `atoms[].position` other than the first instance's).

- Rust tests (`npm run check:rust`: `cargo nextest run --workspace`, plus doctests): domain invariants, transactions, undo/redo, restore validation, copies, scene instances (independent moves that keep the graph and other instances, a position per instance for new atoms, deleted atoms' positions removed everywhere and restored by undo, the last instance kept, no-ops, identifiers never reused); recorded states (defaults, every state set on creation and by edits with undo and redo, element and query conversions, each refusal code at its command index with nothing changed, the same rules on restore); stereo marks and stated configurations at the domain level (all 24 slot orders of a center canonicalized with an independently computed parity, mark and bond-order rules, record validation and refusals, records removed with a neighbor change and restored by undo, an order change refused with the rule it breaks, a mark and a configuration on one bond in either order, a stereo-only edit counted as an edit, a document of 2000 atoms with a 100-entry history validated within a time budget, restore corruption cases); reading a configuration off a drawing (the frame and sign against hand-computed and OpenSMILES examples, the marks that bear on a center, the fourth position, and for `set_configuration_from_drawing` its scope, skipping versus refusing, instances, `$N`, idempotence, one undo step, and seeded random edit sequences mixed with it); version 1 and version 2 fixture files and drafts (`crates/providers/tests/data/`), and version 3 independent version 4 scene fixtures, and a version 5 scene fixture (whose stored settings are kept), migrated with their history (version 1 trimmed oldest first when it exceeds the limit), the exact bytes kept by the first save and never overwritten (other bytes under the same ID get the next numbered copy, identical bytes none, version 1 and 2 copies apart), a newer version refused and untouched, version 6 round trips of every state and of stereo marks and records, a version 6 scene fixture migrated to version 7 with no groups (exact bytes kept by the first save, draft likewise), version 7 round trips of enhanced-stereo groups with their history and a refusal for each damaged group record, stored invalid combinations unreadable, a document of 2000 atoms with a history of 50 000 widest-state changes (document and draft) within the read limit; application revisions, replays, save failure and conflict, autosave, recovery, save as copy, the service's drafting schedule (quiet period, maximum wait, retries that edits do not postpone) and drafts written without a client, at a stop, and after a failure, list search and sort, rename, delete to the trash and restore (refused for unsaved work and drafts anywhere, never undone by another process's copy), storage failure reasons, bounded waits for a document's turn, and a repeated `request_id` answered `request_in_progress` while it runs; the turn gate itself; file store atomicity, corruption, save locks between stores, draft ownership (including concurrent requests in one process), permission errors reported as denied, trash moves and restores (refused while a draft or another writer holds the document), and interrupted saves; HTTP guards and error mapping; operation contract 1 (a full flow through `execute`, the same error codes and details as the v1 calls, replay and reuse, invalid parameters, each protocol error, the legacy-observed profile, a catalog whose executable operations are exactly the ones dispatched, and sub-operations: command rows that parse exactly when executable, and capability rows that are not commands and name only executable command rows); the stereo write side (the three commands read back in canonical form and kept by undo, redo, save and reopen; a value read from a snapshot sent back as a command changes nothing; `null` clears and a missing key is malformed; every `reason` with the failing command's index and the transaction unchanged; `$N` naming the bond an `add_atom` brought; `notes` for deleted atoms and bonds, an added bond, an explicit clear and a clear then set; the `ok`/`partial` against `notes` invariant; an aromaticity conversion over a marked bond refused whole; every `reason` code documented here) and the stereo inspection `chem.stereo` (the reply shape and its closed field set, every agreement value, every diagnostic code with its count, the non-atom label field, weak evidence, instance selection and its invalid-parameter path, the 20,000-entry limit at and over its boundary, that a call changes nothing and `chem.validate` is byte-identical around it, and that this section lists every agreement and code emitted) and the stereo read side (document snapshots that show marks and stated configurations, the canonical double-bond reference forms, identity and substructure replies that are `partial` with a `stereo` loss when a side holds stereo and `ok` otherwise, SMILES consent for `stereo_marks`/`stereo_configuration`, the MOL export that writes the marks and asks consent only for a configuration its marks do not show, the V2000 bond-stereo column read and written and CDXML `Display` refusals, and the `ok`/`partial` against loss-list invariant over those replies). The V3000 provider core (`crates/providers/tests/ctfile_v3000.rs`, hand-authored fixtures): reading a core fixture, a CRLF fixture with collection numbers 0, negative and 258, and a continued fixture; the V2000 entry points still refusing V3000; a table of about 70 single-change refusals each pinned to its reason and line (counts mismatches and caps, duplicate and zero indices, forward references, huge and non-finite numbers, non-ASCII, 81-column lines, continuation at the end of the file, trailing data, every unsupported construct); the reason-to-family table; one aggregated `isotope_validation` loss per record on read and on write (`atoms[2, 5].isotope`), and a V3000 double-bond loss that does not mention V2000; truncation of a valid file at every byte; the 2000-atom and 4000-bond limits at and one over; the writer's exact bytes, determinism, round trip (also through CRLF) and continuation. The V3000 wire (`crates/application/tests/ctfile_v3000_wire.rs`, `stereo_read.rs`, `stereo_wire.rs`, `stereo_group_wire.rs` and three conformance cases): a V3000 wedge imported as a mark with the configuration it shows (equal to a hand-stated one), a hash, either and crossed mark in and out with the narrow end first, an unreadable center costing a loss and not the import, `COLLECTION` blocks and the chiral flag, refusals that leave the document at its revision, `ctab` omitted equal to `v2000` byte for byte, `auto` choosing V3000 for an aromatic bond and for more than 999 atoms, the consent flow (refusal, changed digest, accepted digest, a digest not valid for the other dialect) V2000 bond stereo columns written and read back (a V2000 round trip, plus the refused values), the hydrogen-uncertainty wording per dialect, an import refused for a document with two placements, and the consent comparison against the placement that is written. The V3000 collections (`crates/application/tests/ctfile_v3000_groups.rs`, `crates/providers/tests/ctfile_v3000.rs`): exact collection bytes, the chiral flag rule, the numbers `-128`, omitted (`0`), `258`, `-2147483648` and `2147483647` through export and import, the consent for a center in no group, every closed refusal with the document and revision unchanged, collision renumbering and the absolute join on append, a split and merge after import compared by partition, V2000 consent, unchanged `chem.cip`, `chem.validate` and `chem.identity`, and the `mol-v2000`/`mol-v3000` ids. `scripts/verify-v3000-rdkit.py` (hand-run) reads the service-written marks and group partitions in both dialects with RDKit 2022.03.2 and reads RDKit-written V3000 wedges and enhanced stereo groups with Chemifree.
- `npm run test:contracts`: real API and CLI processes on a temporary data directory — HTTP = CLI JSON (also for operation contract 1: a long-running `chemifree operations` process and `POST /api/v1/operations` give the same replies for the catalog, create, transaction and its replay, a stale undo, an unimplemented operation, an unknown operation, a malformed request, save, and close; the route needs the token and a JSON content type), write guards, the full create/edit/undo/redo/save/close/reopen flow, conflicts, save failure and retry, recovery after the API is killed; six CLI processes saving from one base at once (exactly one commits), a process killed while holding the save lock, an API save racing a CLI save; autosave, crash, and recover (content, history, IDs, revision); discard; an outdated draft recovered as a copy; an unreadable draft; a failed autosave and its retry; save as copy over HTTP and CLI, its independence, a failed copy, and a copy whose source draft cannot be removed (a permission error on `drafts/`: `closed: false`, the source keeps its changes and draft); a save lock held by another process and saves queued behind it answered `busy` within the time budget, a lost save reply asked about again (`request_in_progress`, then the committed result), and a read-only `documents/` reported as `read_only`; drafts the service writes with no client asking (edit over HTTP, no autosave call, wait for the draft, SIGKILL, restart, recover), drafting at a normal stop, a draft written within the maximum wait while edits continue, and a failed service draft that is reported and retried once storage recovers; list search and sort (HTTP = CLI), rename, delete over HTTP and the CLI, a clean API copy closing and an unsaved one unable to re-create the document after a CLI delete, deletes refused for drafts in use and abandoned, a process killed in the middle of a delete, and restore; version 1 and 2 files placed in the data directory (opened over HTTP and from the CLI with the same reply, nothing written by reading, undo of migrated history, the first save from either adapter keeping the exact bytes under `migrated/`, a newer version listed as unreadable and left untouched); recorded states drawn over HTTP shown the same by the CLI, and refused combinations answered with the same `reason` and message over HTTP and from the CLI, malformed values as `invalid_request` without one; the stereo commands over both operation routes (a mark, a configuration and a `$N` bond, a refusal with its `reason`, a cleared neighbor reported in `notes` and `partial`, undo restoring it, SMILES consent, the V2000 `unsupported_stereo` refusal, and the catalog's transaction parts). Debug builds honor `CHEMIFREE_TEST_COMMIT_DELAY_MS`, a pause inside the save lock used to force these races.
- `npm run test:client`: the web client without a browser, including the strokes of wedge, hashed wedge, wavy and crossed bonds (narrow end at the right atom, an endpoint-swapped twin giving the same picture, label cuts, no ink without room, bounds, reach), the commands of a mark gesture (when the configuration is asked for, the cycle, the clears an order change needs, the fallback that keeps a mark the service will not derive a configuration for and only for the derive command's reasons, a gesture rebuilt on the latest document, the status sentences) and the public two-center drawing, and which replies it accepts (with and without recorded states; mixed or partial ones refused), the `reason` of a refused combination, label layouts (inferred hydrogens add nothing, the box holds every mark, reserved symbol widths against measured natural widths, the text beside a symbol in one run with the symbol's reserved width centred on the atom, long query lists shortened, the side stated hydrogens take, alike atoms sharing one layout), bond trimming and gaps too narrow to draw, bond kinds and aromatic inner lines inside a ring drawn in mixed directions without crossing, inside the ring for a ring bond between two substituted atoms and for fused rings, each drawn either way, with the rings found once per graph and used for every placement, the view box with label boxes and placement frames, hit testing by radius and label box, accessible names per placement, the bond each drag draws (always from where it starts), and each inspector edit built from the latest document, marks and the stereo table (validators that accept exactly what the service sends and refuse a malformed or unknown mark or record, older replies filled in, the bond's accessible name with the narrow end, the stereo notice, the `notes` of a transaction reply (validated, and filled in as empty for an older reply), the new commands among the ones a stale selection check knows, and identity and substructure replies that are `partial` with a stereo loss), and the stereo agreement indicator: the `chem.stereo` reply validators (accepting the documented reply and the public two-centre drawing F[C@H](Cl)[C@H](Br)I, refusing each tamper with the snapshot, the stored side, the order or the reasons), the staleness rule, the check scheduler on a fake clock (debounce, one in flight, one more, no start while dragging), the summary sentences, rows, rings and button names, and the one-click commands and status text built from the latest document.
- `npm run test:browser`: browser smoke (including a wedge drawn by a drag with its configuration stored in the same undo step, hashed wedge, wavy and crossed marks, a refusal that keeps the mark, the inspector's keyboard path, the empty status line in the accessibility tree, the arrow keys through every bond kind, and the underlay of a hovered wedge; a drawing duplicated as a second placement, moved with its controls, an atom added in both, an atom dragged in one only, and a further copy made from the last placement; recorded states set in the inspector, drawn, and kept through save, a page reload, and reopening, including narrow and wide symbols (I, Cl, with a charge) and a long query list (with stated hydrogens), measured on the rendered SVG so no text is drawn at a forced length, the text before and after a symbol (H3C, Cl+, the query's H2) is drawn within 2 pt of it, every drawn text and mark lies inside the label box the canvas reserves, no bond ends inside a label box, and every label box, placement frame, and placement ID is inside the view box; with the first request held, charge-then-query, query-then-charge, and a typed query list then its negation, none refused; selection changes during grouping and a queued Delete with Undo, an immediate selection change followed by Delete, and deletion and Undo of a selected reference owner and target; the stereo agreement indicator on the public two-centre drawing: no request for a drawing without stereo, the summary line, the panel row and the canvas ring (a role and a name, no pointer events), no ring and "Checking the drawing…" with `aria-busy` while the answer is held and exactly one more request for the changes made meanwhile, Update from drawing in one transaction with its status and Undo, Update all, Store from drawing, Remove stored, the switch off and on and Check now, and a refused check with Check again), also run in CI (job `browser`: Playwright's Chromium on Ubuntu 24.04, no skipped steps, records uploaded whether it passes or fails); results in `verification/editor-browser-smoke.json` and `verification/browser-ci.json`.
- `npm run test:browser:routes`: reproduces, against a plain HTTP server, the Playwright route-expiry race that made the smoke's restart step flaky, and checks the smoke's guard (`--unguarded` shows the race); results in `verification/browser-route-race.json`.

None of these checks validate chemical behavior or compatibility with any other application.


P35i independent caption run shadow: `shadow` is a boolean; omission means false, explicit null and other types are malformed. It composes with every existing run field and never changes graph identity, text, advances or anchor. The shared application provides UI/API/CLI, replay and undo/redo. Chemifree renders a second copy at (+1,+1) pt with opacity 0.35 and zero blur, using the effective run color (theme foreground for null); hollow outlines stay hollow and underline rectangles receive the same shadow. This is an independent visual policy, without a claim of legacy rendering equivalence. Only visible shaped glyph/stroke ink and positive-width underlines enter the translated union; whitespace, invisible formatting and zero-advance shaping follow browser measurements. Fit/selection/hit bounds use the current actual SVG viewport measurement provenance.

Native document/draft v17 requires boolean shadow in every current/history run. v8–16 forbid that field and migrate to false, preserving all original bytes in versioned archives before replacement. Future v31 is refused untouched (v19 adds label style and v20 adds underline, below). Shadow=false keeps the exact v16 admission weights (160 without outline, 192 with it); shadow=true admits at224 bytes/run. Physical metadata reserves192 bytes for at most185 encoded metadata bytes. History physical weight is at most6/5 of its8MiB admission; current admission is48MiB and at most32,000 runs add at most1,024,000 physical bytes. The conservative complete-file bound is61,421,978 bytes, below64MiB with more than5MiB spare, including the existing4096-byte envelope. Existing maximal109-caption current/undo/redo states must not be trimmed or refused during migration. External caption roundtrip and scientific labels remain unsupported.


Shadow compositing policy: all shadow glyphs and underline rectangles paint together in one translated SVG group behind every foreground glyph and underline. The group receives opacity0.35 once, so overlapping descenders/underline shadows do not accumulate opacity and later-run shadows cannot cover foreground ink. A real-browser regression rasterizes the actual detached shadow tree with frozen computed styles onto a transparent canvas and asserts nonempty alpha≤90/255, including g/j descenders, alongside DOM paint order and the existing geometry/theme checks. Historical session note: this regression was initially written without execution.
Merged #194 and subsequent caption browser CI evidence in the
[completion audit](verification/p35-completion-audit.md) supersede that session status.


### Chemical label weight (P35j)

Document settings add `label_weight: "normal" | "bold"` (built-in normal). The same
`update_settings`, document style and preference controls, HTTP/CLI, reset/import,
request replay, undo/redo, save/copy and recovery paths carry it. New-document
preferences are copied once. Normal keeps the established 500 symbol/query/H/Et
and 400 isotope/charge/subscript typography; bold uses 700 for these chemical text
runs. Radical dots, unknown-radical and attachment marks keep their existing weight
and sizing policy. Independent caption runs keep their own normal/bold weight;
caption weight does not inherit `label_weight`. No chemical graph, valence,
aromaticity, identity or property calculation changes. Scientific result snapshots
still name the exact document/revision, including its settings.

The canvas measures actual mounted SVG 700 runs at the effective size and family,
including natural advance, horizontal glyph overhang and vertical ink. Unchanged
400 radical/attachment text inside bold labels is measured too, so small-font ink
reserves its actual bounds without changing mark weight, size or placement policy.
Those measurements rebuild the single chemical-label layout used by bond trimming,
fit, picking and selection highlighting; no glyph is stretched. Measurements are
replaced when the document, loaded font or actual SVG viewBox changes. Provisional
bounds before measurement are not evidence for 700 font metrics. Normal continues
to use the existing 400/500 metric tables. Et keeps both carbon atoms, the internal
bond, IDs and coordinates and changes only its visible label weight.

Native document/draft v18 requires normal/bold `label_weight` in current settings
and in every history settings before/after state. v1–17 forbid that field and
migrate all settings states to normal. Reading writes nothing; replacement or
recovery preserves exact old document/draft bytes in existing versioned archives.
v31 is refused untouched (v19 adds label style and v20 adds underline, below). The existing 256-byte settings reservation covers the
new field, so caption admission weights, the 61,421,978-byte conservative complete
file bound and the provider's physical 64 MiB bound remain unchanged. Existing
v16/v17 maximal 109-caption edits must retain complete undo/redo during migration.

Native graph templates remain v1; graphics use the v2/v3 profiles described under
P36e. Settings migration remains independent of that version choice: an older
payload with no settings `label_weight` reads as normal; explicit null, unknown
strings and other types are refused. Newly encoded payloads include the field.
Exact destination-style matching includes weight: an old normal template can be
inserted into a normal drawing and is refused in a bold drawing. This payload
default is separate from strict v18 document/draft admission. Bounded external
MOL/SDF, CDXML/CDX and SMILES exports retain the full chemical graph and report
existing `drawing_settings` rendering loss; they do not silently claim to carry
the new chemical label weight. General font registration,
external label-style roundtrip and legacy rendering equivalence remain unsupported.

Regression code is written for all three families at 4/72 pt, scripted H/count,
isotope/charge, list queries and Et, real SVG ink versus shared trim/fit/hit/selection,
caption independence, preferences/replay/history, native old-byte archives and
payload compatibility. Historical implementation-session note: that run used
formatting/diff inspection only and left execution/reviews pending. Merged #195
and its passing CI/review dispositions in the [audit](verification/p35-completion-audit.md)
supersede that historical verification status.

### Chemical label style (P35k)

Document settings add `label_style: "normal" | "italic"`, default normal. The
style selector, `update_settings`, HTTP/CLI, preferences copied once at creation,
reset/import, replay, undo/redo, save as copy and recovery share this field.
Normal preserves P35j typography. Italic applies only to chemical symbol/query,
stated H/count, isotope, charge and compact Et runs, combined with their existing
400/500 or explicitly bold 700 weights. Radical/attachment marks and independent
caption run styles remain independent. Graphs, chemistry and export payloads do
not change; bounded external exports report `drawing_settings` rendering loss.

Actual mounted SVG advance and ink are measured for italic 400/500 and 700,
including separate left/right overhang, all three label families and effective
sizes. Measurement keys include style, size, family and weight. Chemical bounds
use these measurements for fit, bond trimming, picking and selection. Provisional
bounds pending fonts are not evidence of font metrics; font completion, document
identity and actual viewport retain the existing measurement provenance checks.
No explicit font registration or equivalence with another renderer is claimed.

Native document/draft v19 requires `label_style` as normal/italic in current and
all history settings before/after states. v1–18 forbid this field and migrate
all such states to normal; v18 still requires normal/bold weight. Reading does
not write; overwrite/recovery archives exact prior bytes. Future v31 is refused
untouched. Existing settings reservations, caption history admission and the
physical 64 MiB bound are unchanged. Native clipboard/template versions remain
unchanged: omission of style means normal; explicit null/unknown/type errors are
refused, and exact destination matching includes style. Bounded P35 is done;
external style roundtrip and general font registration remain unimplemented.
Merged #196 verification is recorded in the [audit](verification/p35-completion-audit.md).


## Chemical label underline (P35l, bounded)

Document `label_underline` is a boolean, default false. It underlines only chemical
text: atom/query symbols, stated or unknown H, H counts, isotope, charge, and the
existing graph-preserving Et label. Radical/attachment marks and independent
caption styling stay independent. Atom coordinates, bonds, stereo, chemistry and
abbreviation membership do not change.

Each chemical text run paints an explicit SVG rectangle from the minimum to
maximum actual SVG character start/end x positions, including whitespace,
Unicode, RTL ordering and ligatures. Its top is the actual run baseline plus its
effective font size/12 and its thickness is that size/16. Rectangles use the
same natural 400/500 or 700, normal/italic font that shapes the text; no
`textLength` or text decoration substitutes for advance measurements. The union
of glyph ink and underline rectangles feeds the same label bounds used for bond
trimming, hit testing, selection and fit. Provisional bounds are not font evidence;
completed measurement retains document, loaded-font and viewBox provenance and
is invalidated when the document, viewport or fonts change.

Document edits, preferences for new documents, HTTP, CLI, reset/import, undo/redo
and replay carry the same field. Native document/draft v20 requires the boolean
in current settings and every before/after settings history state. Versions 1–19
forbid the new field and migrate to false in memory; first rewrite/recovery
preserves exact old bytes under the existing archive protocol. Weight remains
required from v18 and style from v19. Future v31 is refused. Existing history
admission and 64 MiB provider file limits stay unchanged. Older clipboard/template
payloads may omit the field (false); explicit null or a nonboolean is invalid.
Copy validation compares the complete migrated style. External exports keep the
existing `drawing_settings` loss rather than pretending to preserve this styling.

Historical P35l session note: checks were authored but not run in that session.
Merged #197 and passing browser/check CI in the [audit](verification/p35-completion-audit.md)
supersede that execution status. Bounded P35 is done without a general scientific
validation or renderer-equivalence claim.


## Chemical label outline (P35m)

`label_outline` is a document boolean, default false. Preferences copy it to new
documents; HTTP, CLI and queued edits, reset, replay, undo/redo, save, recovery and
save as copy share the setting. It hollows only element/query, stated H/count,
isotope, charge and graph-preserving Et glyphs: fill none, current-color stroke
at the effective run size / 16 with round joins. Explicit underline rectangles
remain filled. Radical and attachment marks and independent caption runs keep
their own appearance. Chemical graphs, recorded states and stereo are unchanged.

Outline always requires actual mounted SVG chunk metrics, including normal
400/500 runs. Each nonzero glyph-ink box gains its stroke radius (effective size
/ 32); whitespace and zero-ink characters gain no false stroke box. The exact
source metric key survives SVG without reconstructing floating-point shifts.
Document/font/current-viewport provenance and all chemical chunk readiness gate
fit and bounds; bond trimming, hit testing and selection share the measured box.

Native document and draft v21 require a boolean in every current and historical
settings state. Versions 1–20 omit it, migrate to false and retain exact source
bytes before their first replacement; v31 is refused untouched. Clipboard and
template payloads may omit it for false; explicit invalid values are refused and
style matching includes outline. Existing nondefault-settings interchange losses
cover this style. Existing history admission and 64 MiB read limits are retained.

Checks are registered as `npm run test:browser:label-outline` in CI and client,
application, clipboard and strict-native boundary coverage. Historical session
note: they were not executed in that implementation session, at the user's request.
Merged #198 verification in the [audit](verification/p35-completion-audit.md)
supersedes that status. Bounded P35 is done; shadow is described in P35n below.


## Chemical label shadow (P35n)

`label_shadow` is a document boolean, default false, copied from preferences only
when creating a document. It is shared by settings/reset/templates, HTTP/CLI,
queued edits/replay, undo/redo, save/reopen, recovery and save as copy. It changes
only the appearance of element/query, stated H/count, isotope, charge and the
retained-graph Et glyphs. Radical dots/text, attachment marks and independent
captions keep their own style. Graph, recorded states and stereo are unchanged.

Chemifree adopts its P35i caption shadow policy for chemical glyphs: a +1,+1 pt
copy in the current foreground color, opacity 0.35, no blur. Chemical shadows are
grouped per run of consecutive instances in scene order: an image, graphic or
caption ends a run, while a `group` object paints nothing and does not. The
shadow copies of one run are rendered together in one SVG opacity group painted
just before that run's first foreground, so overlap within a run stays at 0.35
and an object painted before the run cannot cover its shadow. A later run's
shadow paints above earlier runs' foreground where they overlap, as the instance
itself does, and the shadows of different runs composite separately. Without an
image, graphic or caption between instances there is one group before every
foreground. Filled glyphs,
hollow outline strokes and positive-width explicit underline rectangles keep
their appearance in the copy. Shadow duplicates carry no foreground measurement
selectors, atom labels, hit targets or accessible names.

Shadow requires actual mounted chunk metrics, including normal 400/500 glyphs.
The shared bounds union foreground glyph/stroke/underline ink and its translated
copy. Font readiness, document identity and the currently painted viewport gate
fit and readiness; trimming, hit and selection share these boxes. State marks
receive no shadow expansion. Disabling shadow restores the prior style path.

Native document/draft v22 requires a boolean in every current and historical
settings state; v1–21 forbid that field and migrate to false. Reading leaves
source bytes intact; first replacement/recovery preserves exact source archives.
Future v31 is refused untouched. Old template/clipboard payloads may omit shadow
for false; explicit invalid values are refused and style equality includes it.
Existing nondefault-settings interchange loss reporting covers shadow.

The v16–21 admission weights remain unchanged. Settings physically reserve 320
bytes instead of 256; at most 100 history entries plus current settings add
12,864 bytes. The conservative complete-file bound is 61,434,842 bytes, below
64 MiB, including envelopes. Maximal legacy current/undo/redo histories remain
covered by the existing migration fixture extended through v21.

`npm run test:browser:label-shadow` is registered in CI; authored checks cover
actual fonts, sizes, weight/style, fill/outline/underline, both themes, overlapping
shadow alpha, rendering order, translated ink/fit/trim/hit/selection, graph/Et,
settings/native history, switched documents and late font loading. Client,
application, clipboard and strict-native boundary checks accompany it. Historical
session note: these checks were not executed in that implementation session at
the user's request. Merged #199 and its passing PR CI in the
[audit](verification/p35-completion-audit.md) supersede that session status.
Bounded P35 is done; no legacy rendering equivalence is claimed.
