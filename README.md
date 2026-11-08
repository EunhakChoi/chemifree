# Chemifree

Bounded P35 is done against the roadmap editing/storage scope under the approved graph-preserving display decision: captions, typed atom labels and editable Et/fragment text/script runs have documented acceptance evidence. #203 merged84ef063; final-head PR CI42320918256 and post-merge main CI42322504482 passed check/browser. General scientific/rendering requirements and unsupported arbitrary atom rich text, dictionaries/parsers and external caption/display roundtrip remain incomplete. Later implementation-session pending notes are historical; see the [completion audit](docs/verification/p35-completion-audit.md) for exact source, CI and review dispositions. P36a #204 is merged26962b6 (its main CI run 42323974348 succeeded) and P36 remains in progress; P36a–k are merged through #216/#217, see the [P36 status in the handoff](docs/HANDOFF.ko.md); this completion documentation has its own integration/review/CI gates.


An independently authored, open-source chemical drawing workspace for humans and agents.

**Early stage.** The browser, HTTP and CLI support creating and editing drawings, undo/redo, local save and recovery. Atoms and bonds retain their drawn states. Named, bounded valence, ring and graph-identity inspectors report unsupported states, and supported aromatic/Kekulé conversions are explicit undoable edits that are refused for the whole document when any ring system is unsupported (for example a benzene fused to a saturated ring, which is reported unsupported rather than non-aromatic). Formula inspection uses the valence model; average and exact-profile mass and isotope distributions additionally need a checked external mass dataset. Results include their analyzed snapshot and calculation policy. A bounded V2000 and V3000 MOL/SDF subset can be previewed, imported and exported with explicit loss reports (V3000 is read and written by the operations since P28b1 and by the web panel since P28c). Rich CDXML and a bounded CDX subset (canonical base64 over JSON) can be read and written, with a loss-accounted editor projection; that projection reads the stereo the public sources confirm (CDXML bond wedge, hash and wavy values with their narrow end, configurations derived from them, enhanced stereo groups, and the CDX atom tags; the binary bond `Display` is read (public INT16 list), `Display2` and `DoublePosition` stay unread, double-bond E/Z is not derived, and a bounded drawing export now writes supported marks and groups with explicit loss consent), status P29e. A bounded OpenSMILES subset can be previewed and imported (positions are generated on a grid) and written as recorded-order or Chemifree-canonical text; the canonical form is Chemifree's own, not another toolkit's. SMILES reading takes the tetrahedral marks `@` and `@@` of bracket atoms as stored configurations (with no bond mark, since the atoms have no coordinates) and the CXSMILES fields `a:`, `o<n>:` and `&<n>:` as enhanced stereo groups, status P29c; the directional bonds `/` and `\` beside a supported double bond (C=C, C=N, N=N; rings of at least eight atoms) are read as stored double-bond configurations, status P29c2a, and the ones it cannot place are refused with their own codes; other mark classes, a lone pair as fourth position and every other CXSMILES field are refused, recorded output writes supported tetrahedral configurations, stored double-bond configurations as `/` and `\` where the reader reads them back (status P29c2b; the rest are consent-required losses) and enhanced stereo groups while canonical output writes no stereo, and query and reaction SMILES are not implemented. Standard InChI and InChIKey generation is an optional Linux native operation (`chem.inchi`), configured explicitly as described below; InChI import is not implemented. A drawing can hold stereo bond marks and stated configurations in its model and file: the commands `set_bond_mark`, `set_atom_configuration` and `set_bond_configuration` (HTTP, CLI and operation contract 1) set and clear them, a transaction reply lists in `notes` the configurations an edit cleared, and snapshots, the CLI and the web show them. `set_configuration_from_drawing` can read a configuration off the marks and one instance's positions when it is sent (a bounded, documented rule set, named `chemifree-stereo-1`); it replaces the stored record of every item it processes, a configuration stated by hand included, and writes nothing for an item it skips. `chem.stereo` is a separate read-only inspection that compares each stored configuration with what each scene instance's drawing shows (agrees, disagrees, only stored, only drawn, neither) and lists advisory findings; it stores and re-derives nothing, and it does not judge whether an atom is stereogenic or compute a CIP label. `chem.cip` is a separate read-only operation that computes CIP descriptors (R, S, E, Z) from the stored configurations on demand, by Rules 1a, 1b, 2 and 3 only: every item is `assigned`, `none`, `unspecified`, `unknown`, `ambiguous` or `unsupported` with a reason, a tie that Rules 4a to 5 might break is `unsupported` rather than `none`, nothing is stored, and no equivalence with another program is claimed (the web editor shows them only behind two switches that are off by default, below). Beyond that they are recorded statements only: nothing labels or imports them; the web editor draws wedge, hashed wedge and wavy bonds with the bond tool (and a crossed mark in the bond inspector) and stores the configuration the drawing then shows in the same undo step, and for a drawing that holds stereo it calls `chem.stereo` on its own (a switch turns that off, "Check now" asks once) and shows how each stored configuration compares with the drawing as a summary line, a ring on the canvas and a Stereo panel whose rows each offer a one-click edit that updates, stores or removes the stored configuration (one undo step); its indicator itself judges no atom stereogenic and computes no CIP label. Two switches in that panel, "Atom stereo labels" and "Bond stereo labels" (off by default, not saved), are the only thing that makes the page ask `chem.cip`: while one is on it asks with `scope: "stored"` (never `stereogenic`, because the operation has no time limit), draws the descriptor in parentheses near each assigned center (`(S)`) or double bond (`(E)`) as a canvas overlay that is never part of the document, and adds a CIP cell next to the agreement with the drawing to each Stereo row that has a stored configuration, under the switch of its kind (a refused or unsupported item says so and gets no overlay text; with a switch on and nothing of its kind stored the panel says that, and nothing is asked); a label describes the stored configuration, not the drawing, and is never stored. The overlay's text form for bonds and its placement are Chemifree choices, not legacy behavior. `chem.validate`, `chem.identity` and `chem.substructure` do not read them. Geometry-only reflections preserve stored stereo and marks; chemical mirror/flip operations that invert those statements are not implemented. Identity and substructure replies for a drawing that holds them are `partial` with a stereo loss, a SMILES export requires consent for stereo it does not retain, and a MOL export writes the marks and asks consent for the stereo (configurations the marks do not show, groups) the file cannot carry. Enhanced stereo groups (absolute, and, or) are separate statements over stored tetrahedral centers: eight commands (`create_stereo_group`, `delete_stereo_group`, `set_stereo_group_kind`, `set_stereo_group_number`, `add_stereo_group_members`, `remove_stereo_group_members`, `merge_stereo_groups`, `split_stereo_group`; HTTP, CLI and operation contract 1) create and change them, snapshots list them as `stereo.groups` with the number as stated, the interpreted number and a label, and a transaction reply reports in `notes` when an edit changed which centers are stated to belong together (splitting, merging, removing members, a member losing its configuration). They state no ratio, CIP, identity, substructure and validation ignore them, recorded SMILES writes supported groups, canonical SMILES needs consent to drop them, MOL V3000 import and export maps them and CDXML and CDX import reads them from the atoms' enhanced stereo attributes (V2000 carries none, a CXSMILES block is read and written for supported groups, bounded CDXML/CDX drawing export writes groups only when every member derives from its output drawing, and canonical SMILES or V2000 export asks consent to drop them), and the web Stereo panel shows and edits them (its recorded SMILES export writes supported groups, and its canonical SMILES and MOL V2000 exports ask consent to drop them; its MOL V3000 export writes them as collections; they are not compared with the drawing). A bounded substructure search (`chem.substructure`, also the web "Substructure search" panel) finds every atom and bond mapping of one drawing in another: query atoms and bonds, a stated charge or isotope, and aromatic and Kekulé spellings read alike through the aromaticity model; a drawing that model cannot read, or a search that reaches its limit, is refused rather than answered empty, and ring, degree and hydrogen-count conditions are not available because the drawing cannot record them. Other formats (apart from the export-only, partial bounded SVG drawing of P38a, which is listed in the `format_conversion` label only beside an attached codec, for example "MOL/SDF, CDXML, CDX, SMILES and SVG subsets"), stereochemistry beyond that (CIP Rules 4a to 5, pseudoasymmetric centers, heteroaromatic rings), layout, naming, prediction engines, authentication and Windows integration remain unimplemented. See the [valence](docs/CONTRACT.md#valence-inspection), [aromaticity](docs/CONTRACT.md#ring-and-aromaticity-interpretation), [identity](docs/CONTRACT.md#graph-identity-and-symmetry), [substructure matching](docs/CONTRACT.md#substructure-matching) and [property](docs/CONTRACT.md#formula-and-mass-properties) policies for scope and limitations.

Continuing in a new session? Start with [the handoff](docs/HANDOFF.ko.md) and [project instructions](AGENTS.md). They record the completed baseline, current limits, and the proposed next implementation slice.

P34a/P34b add the bounded **Ring** tool (R): choose 5 or 6 (initially 6), then click a
center to append an independent carbon single-bond regular ring at the current bond length,
in a document with one placement. HTTP and CLI share `insert_ring`; optional `size` is 5 or 6
and omission retains 6. One Undo removes the complete ring; Redo restores its IDs.
P34c adds explicit **Attach to atom** mode: drag from an existing atom toward a fresh 5/6-carbon
ring connected by one single bond. HTTP/CLI use `attach_ring` with an explicit atom and nonzero
direction. Ordinary bond-addition stereo clearing and group notes apply; Undo restores the statements.
Recorded atom states are preserved; valence remains an inspection rather than a new edit gate.
P34d adds **User graph templates**: save complete selected graph components to a named
native fragment JSON file, then choose that file and explicitly insert it independently.
The file preserves the chosen source placement's coordinates and recorded states; insertion
adds the specified point offsets, requires matching style and one destination placement,
and uses fresh IDs with one Undo/Redo step. File selection alone changes nothing.
Crossing-bond selections, scene content and enhanced stereo groups cannot be saved as templates.
This is file reuse, without a library, automatic attachment, rotation or layout.
See [the P34d record](docs/verification/p34d-user-template.md); checks are written and unrun.
P34e adds **Attach template file…**: validate one connected native file, explicitly choose
source and destination atoms and a finite nonzero dx/dy direction, preview, then connect by
one single bond. Geometry is translated without rotation. Source-anchor stored stereo loss
is refused; unrelated copied stereo is remapped, and destination stereo clearing/group notes
follow ordinary bond editing. The file is unchanged. Read-only preview reserves no IDs;
apply uses one atomic, replayable history step with frozen recovery. P34 remains in progress.
[P34e checks](docs/verification/p34e-template-attachment.md) are authored and unrun.


P34f adds **Replace terminal atom…**: explicitly choose a template anchor and a destination
atom with one single outside bond, preview, then replace. The anchor keeps the target's ID and
position; its complete atom state is replaced. The outside bond/mark, references and neighboring
stored stereo stay, while N−1 atoms and M internal bonds receive fresh IDs. Identical one-atom
replacement is a no-op. See [the P34f record](docs/verification/p34f-template-replacement.md);
checks are written and unrun in this implementation session.

The independent six-carbon ring additionally offers an explicit **Kekulé alternating bonds**
pattern (three double and three single bonds). The existing all-single default and five-carbon
recipe are unchanged. Attached six-carbon rings now offer the same explicit pattern, with
one additional single connector and ordinary bond-addition stereo clearing. Enter Attach to
atom, then choose Kekulé alternating bonds explicitly; the pressed size/pattern remain fixed
through the gesture and uncertain-outcome recovery. This states bond orders without
aromaticity inference. Pattern choice is retained through lost-reply recovery and is not
saved as a tool setting. Its checks are written and unrun; see the
[P34g record](docs/verification/p34g-kekule-ring.md) and
[P34h record](docs/verification/p34h-attached-kekule-ring.md).

Fusion, spiro, non-terminal replacement, aromatic-bond patterns, other sizes, abbreviation contraction/expansion and
molecular conformations remain unimplemented. P34 is in progress; the new checks are written
but have not run in this implementation session. See the
[ring contract](docs/CONTRACT.md#independent-six-carbon-ring-insertion-p34a),
[attachment contract](docs/CONTRACT.md#attached-carbon-ring-p34c), and
[P34c verification record](docs/verification/p34c-attached-ring.md).

## Run locally

Use Node **22.22.2**, npm **11.13.0**, and Rust **1.95.0**. The repository includes `.nvmrc`, `packageManager`, and `rust-toolchain.toml`.

`npm run check:rust` also needs **cargo-nextest 0.9.149** on `PATH`, the version set by `nextest-version` in `.config/nextest.toml`. Install it with `cargo install cargo-nextest --version 0.9.149 --locked` or from its prebuilt release binary.

```bash
npm ci
npm run dev
```

Open **http://127.0.0.1:5173**. This starts Vite and the Rust API together; Vite proxies `/api` to **127.0.0.1:3001**. Stop both with Ctrl+C. Choose **New document**, draw with the bond tool (click to start a chain, click an atom to grow it, drag to bond, click a bond to cycle its order), relabel atoms with the atom tool, inspect and edit charge or isotope with the select tool, then **Save** (Ctrl/⌘+S). **Duplicate placement** (under the inspector) draws the same graph again, copied from the last placement and shifted down and right; each placement moves on its own with its arrow buttons, dragging atoms moves them only in the placement where the drag starts, and chemistry edits apply to every placement. Undo/redo: Ctrl/⌘+Z and Ctrl/⌘+Shift+Z. Close and reopen from **Saved documents**, where you can search titles, sort by title or most recent save, rename, and move documents to the **Trash** (and restore them). A document with unsaved changes or a recovery draft is not moved to the trash. Under the title, **File** shows the saved state and **Recovery draft** shows the autosave (1 s after the last change, at least every 5 s while editing). The API also drafts unsaved work on its own, with or without the page (2 s after the last change, at most 10 s after the first), and at a normal stop. If the API stops before you save, reopening the document offers **Recover** (with undo history) or **Discard draft**; if it was saved elsewhere meanwhile, **Recover as copy**. **Save as copy** (the copy icon) commits what is on screen as a new document and opens it; the original keeps its last save. If the original's recovery draft cannot be removed, the original is left open with its unsaved changes instead, and the notice above the copy says so and offers **Open original**; the notice goes away once you leave the copy, or once the page learns that the local service restarted (the page finds out when its next write is refused, so an idle page keeps the notice until then).

Documents are stored as `documents/<id>.json` in the data directory (recovery drafts in `drafts/`, save locks in `locks/`; files from before graph and scene were separated are read and migrated, and the first save over one keeps its original bytes in `migrated/`): `--data-dir` or `CHEMIFREE_DATA_DIR`, otherwise the platform user-data directory (`$XDG_DATA_HOME/chemifree` or `~/.local/share/chemifree` on Linux, `~/Library/Application Support/chemifree` on macOS, `%APPDATA%\chemifree` on Windows). The API prints the path at startup.

The **MOL and SDF** panel reads UTF-8 V2000 and V3000 files (the dialect is detected; collections become the drawing's stereo groups, with renumbering, merging and chiral-flag losses shown like other import losses) and previews a selected record, including conversion losses, before appending it as one undoable edit. It can preview and download the current drawing as MOL or SDF; a version control (V2000 default, V3000, Auto) is sent as the `ctab` key and is a choice of this page, not saved in the drawing, and the result says which dialect was written. When the export drops something that needs consent (stereo groups in V2000, stored centers in no group in V3000, configurations the marks do not show) the losses are listed with plain sentences and nothing is offered for download until they are accepted; a changed drawing or digest asks again. Refusals (a group member the marks do not derive, query atoms, S-groups, R-groups, 3D, reactions) are worded plainly and leave the drawing unchanged. The subset, safety limits, field handling and operation requests are described in [MOL and SDF](docs/CONTRACT.md#mol-and-sdf-subset); unsupported chemistry is refused rather than silently changed. Through `document.import` and `document.export` (HTTP, CLI, operation contract 1) a V3000 connection table is read (core atoms, bonds 1 to 4 with aromatic, charge, isotope, radical, wedge/hash/either bond marks with the configurations they show) and written (`ctab` `v3000` or `auto`), and a V2000 export writes the bond stereo column, which the V2000 reader now also reads (wedge, hash, either); `accepted_losses` is the consent for stereo a file cannot carry. Since P28b2 the V3000 `COLLECTION` entries `STEABS`, `STERAC<n>` and `STEREL<n>` map to and from the drawing's enhanced stereo groups (any 32-bit number as stated, the chiral flag written as 1 only when there is an absolute group, a stored center in no group asks consent, a member that is not a stored stereocenter refuses the file), and `format` accepts the `mol-v2000` and `mol-v3000` ids. Not implemented: V3000 query atoms and bonds, dative bonds, stereo groups in V2000 (a V2000 export asks consent to drop groups), SGroups, R-groups, 3D and reactions, atom parity on read (status: P28a, P28b1 and P28b2; the web choice is P28c, see the contract).

Through HTTP/CLI `document.export`, `{ format: "cdxml" | "cdx", document_id, accepted_losses? }` exports a captured drawing as one page and fragment and returns its snapshot. It preserves supported wedge/hash/wavy/crossed marks; it invents no marks or CIP labels. Groups require every member to derive from the written drawing; omitted configurations/groups require digest-based consent. Group numbers outside 1–127 are renumbered with an informational loss. Written group members explicitly show enhanced-stereo indicators (`0445=true`), reported once as `enhanced_stereo_display` rendering normalization; the drawing stores no visibility setting, so roundtrip checks of configurations and groups exclude it. The existing rich-tree export and strict opaque-extension rules remain available. The web **CDXML and CDX export** panel now prepares either file, shows losses, and offers a download only after any required losses are explicitly accepted. Download rechecks the exact file and snapshot; changes to the drawing, format, consent or service invalidate the preview. CDX downloads contain binary bytes, not base64 text. P29 remains in progress pending P29g; this does not establish full stereo interchange. See [the bounded drawing profile](docs/CONTRACT.md#cdxml-interchange).


The **SMILES** panel keeps the line breaks of what is pasted, previews the atoms, bonds and losses of a string before appending it as one undoable edit (asking for acceptance only of losses that change the chemistry or drop text), and writes the drawing as recorded or canonical SMILES. Grammar, limits, error codes and what is refused are in [SMILES](docs/CONTRACT.md#smiles).

The shared HTTP/CLI operations offer read-only `document.selection_scope`, `document.arrangement_plan` and `document.transform_plan` previews, with atomic union movement, six geometry-bound alignments, two equal-gap distributions, rotation and horizontal/vertical axis reflection. Stored stereo, marks and groups remain unchanged; the transformed drawing can disagree with them. Rectangle rotations require exact quarter turns. See the [arrangement](docs/CONTRACT.md#scene-object-alignment-and-distribution-p32b) and [transform](docs/CONTRACT.md#selection-rotation-and-reflection-p32c) limits.

The web **Scene** controls expose whole-object/group alignment, distribution, ±90° rotation and axis reflection around the selected geometry center. On the canvas, Shift-click toggles atoms, bonds and graphics; dragging empty space selects atom centers in the active movement placement and fully contained graphic geometry, with Shift adding to the selection. Dragging selected geometry moves the captured mixed union once. The movement placement is shown explicitly: switching it clears atom/bond selection while retaining Scene choices, and a deleted placement is never silently replaced. These gestures use the existing server movement command and validation; they add no backend behavior.

The four quarter-turn and axis-reflection buttons also accept mixed scene/atom/bond selections, using the captured active placement for explicit graph targets. Queued actions revalidate the captured targets and compute the latest geometry-bounds center as the pivot; missing targets lapse the whole action. Stored stereo remains unchanged. See the [mixed transform contract](docs/CONTRACT.md#mixed-selection-quarter-turns-and-reflections-p32f).

The bounded P32 selection and geometry delivery is complete; [verification and review evidence](docs/verification/p32-selection-completion.md) records the merged implementation and successful main CI. This completion documentation PR still awaits its own CI. Center arrangements that would change again on repetition refuse atomically as `arrangement_center_unstable`. Alignment/distribution require whole scene objects; partial-atom arrangement, lasso or edge-intersection selection, cross-placement raw graph selection, arbitrary-angle/custom-pivot controls were outside that P32 delivery. P36f adds a separate finite-angle form candidate; custom pivots, snapping and cleanup remain unsupported.

Bounded **P33 is done** against the roadmap's explicit clipboard scope and acceptance criteria. [PR #172](https://github.com/EunhakChoi/chemifree/pull/172) passed isolated `npm run check`, the standard browser smoke, all eight clipboard browser commands, and [PR CI check/browser](https://github.com/EunhakChoi/chemifree/actions/runs/42148445439). [Post-merge main CI for `1082293`](https://github.com/EunhakChoi/chemifree/actions/runs/42149543576) also passed. This completion documentation PR still requires its own final review dispositions and current-head CI. See the [scope and evidence](docs/verification/p33-clipboard-completion.md).

Native graph Copy/Cut/Paste preserves supported recorded states, marks and self-contained stored stereo with fresh atom/bond IDs. The separate native graphic/group forest profile preserves ordered containment, internal references including self-links/cycles, and fresh object IDs. Both profiles use explicit plain-text browser actions, manual fallbacks, atomic edits, selection-aware Undo/Redo and same-request outcome checks with restart reconciliation. Cut requires a successful browser write before deletion under the exact prepared source/process; a refused deletion leaves a copy. See the [graph contract](docs/CONTRACT.md#native-graph-fragment-copy-and-paste-p33a), [graphic forest contract](docs/CONTRACT.md#native-graphic-subtree-clipboard-backend-p33e) and [graphic Cut](docs/CONTRACT.md#explicit-native-graphic-forest-cut-p33j).

MOL and SMILES clipboard text uses the existing preview/loss-consent flow to append to one existing placement. MOL supports the existing V2000/V3000/Auto profiles; SMILES retains its bounded grammar and deterministic import grid. CDXML provides explicit whole-drawing text copying after export consent and text import into a **new drawing**, with page/molecule selection, preview and loss acceptance. CDX remains binary download only. These are explicit formats without format guessing. See the [MOL](docs/CONTRACT.md#explicit-mol-text-clipboard-bridge-p33d), [SMILES](docs/CONTRACT.md#explicit-smiles-text-clipboard-bridge-p33g) and [CDXML import](docs/CONTRACT.md#explicit-cdxml-text-import-as-a-new-drawing-p33i) contracts.

Native graph version 1 and graphic-forest versions 2/3 are separate profiles and require exact destination settings. Graph paste requires one destination placement; graph Cut additionally requires one source placement, no crossing bonds and no incoming graph references. Graphics can paste into multiple-placement destinations but reject molecule instances, mixed graph targets, outgoing/graph references and groups with fewer than two children; Cut also rejects incoming references from outside the forest. Molecule-instance cloning, enhanced-stereo-group graph copying, global shortcuts, custom MIME, external-format Cut and Windows/OLE remain outside this bounded delivery. OS clipboard permission success, external-application compatibility and full editor canvas/navigation are not established by the clipboard harness. The real-backend P33k harness checks cross-document graph/forest copying and pasting; it replaces browser clipboard transport only. Permission-error and uncertainty behavior is covered separately with scripted HTTP/clipboard boundaries. Broader scientific and format requirements remain partial.

The catalog lists 40 operations, with 30 executable in the default profile.

The scene can also contain ordered groups and simple line, rectangle, arrow, and candidate axis-aligned ellipse, closed polygon and single open cubic spline graphics. Directed references from scene objects to objects, atoms, or bonds record relationships without computing chemistry. Grouping, movement, ordering, references, and selection participate in the same undo/redo history. Each document owns its style (bond length and width, label font and size, carbon labels, and display unit); style changes are undoable. Data-directory preferences separately hold the theme, defaults copied into newly created documents, and mass-result display decimal places. **Document style** and **Preferences** edit these settings in the web UI; general numeric precision and NMR preferences remain unavailable.

If preferences could not be read when the page started, open **Preferences** and choose **Retry preferences** after the local service is available; a successful read applies the saved theme.

P35a adds independent single-line Unicode captions with baseline/subscript/superscript runs;
P35b adds explicit normal/bold weight and normal/italic style on each run, composable with script,
ordinary Undo/Redo and native save/recovery (introduced in v8). P35c adds whole-caption
left/center/right alignment at a fixed anchor, including edge spaces in the natural advance. Captions preserve text verbatim and do not
change chemical graph or abbreviation meaning. Click or use Scene to select them;
move/group/delete are supported, while caption-containing arrangement/transform/graphic
clipboard requests refuse whole. Chemical exports require consent for caption loss.
P35d adds a generic font family per caption run: inherit the drawing font or override with sans-serif, serif or monospace. P35e adds a finite 4–72 pt size override per caption run; null/omission dynamically inherits the drawing size. Script scaling and shifts use each run’s effective size. P35f adds nullable per-run color: null/omission dynamically follows the theme foreground; canonical opaque sRGB #RRGGBB overrides stay fixed across themes. Atom/Et labels and arbitrary font names remain outside this slice.
P35 remains in progress. P35f checks are written and isolated execution is pending; see [scope and pending verification](docs/verification/p35f-caption-run-color.md).

P34i adds explicit terminal Et contraction/expansion over an existing two-carbon graph.
Atoms, bonds, IDs, coordinates and calculations remain complete. Only the canvas contracts;
copying compact members through native clipboard/templates refuses, while chemical exports
report a rendering loss and write the full graph. Local stereo and multiple placements are
outside this first contraction slice. New checks are written but unexecuted; see
[scope and pending verification](docs/verification/p34i-ethyl-display.md).

Version 14 documents and drafts require explicit nullable caption run color (null inherits theme foreground; canonical opaque sRGB #RRGGBB overrides it; v8–13 migrate as inherited), nullable caption run size overrides (finite 4–72 pt; null inherits the drawing size; v8–12 migrate as inherited), nullable caption run font overrides (null inherits the drawing font; sans-serif/serif/monospace override it; v8–11 migrate as inherited), whole-caption left/center/right alignment (v8–10 migrate as left) and explicit caption run weight/style; v8–9 captions migrate these to normal. Version 9 added and v10–14 retain per-placement Et display maps and their history. Earlier displays default empty. Documents and drafts retain the scene, selection, style, the storage for stereo marks and stated configurations (shown in every snapshot and set by the stereo commands), and enhanced-stereo groups (eight commands, shown in every snapshot as `stereo.groups` and edited in the web Stereo panel; only MOL V3000 import and export reads and writes them as `COLLECTION` entries); versions 1–4 are read in memory with the default style, version 5 keeps its stored style, versions 1–5 have no stereo, and versions 1–6 have no groups. Original document bytes are kept before the first overwrite, and original draft bytes before recovery; explicitly discarding a draft does not retain a copy. Preferences are stored separately in `preferences.json`. See [the document contract](docs/CONTRACT.md) for limits and migration behavior.

The P15 numeric library provides exact-rational Real48, binary32, binary64, and 80-bit extended storage conversions, finite arithmetic, selected ordered heading and deviation boundaries, independent length and concentration unit conversions, and explicit decimal display and F32 storage policies. The numeric library does not itself enable a chemistry engine. See [the numeric boundaries](docs/NUMERICS.ko.md).

Optional external DATA stays in a separately selected local directory and requires a trusted registration and review. Typed readers cover the selected active DATA roles, with public checks using independent fixtures. A separately authorized NIST snapshot passed a narrow local methane check; a distinct personal mass-profile registration was rejected and remains unverified ([record](https://github.com/EunhakChoi/chemifree/pull/81#issuecomment-6546579774)). The host selects individual registrations, and an aggregate registry hash alone does not prove a manifest belongs to it. See [the DATA handoff](docs/DATA.ko.md) for profiles and limits. No data or registry is needed for drawing or formula inspection; mass operations consume only the two explicitly supported checked profiles, and loading other data does not enable prediction.

The first API launch compiles Rust dependencies. If that takes longer than the UI's five-second request timeout, use **Retry connection** once the API is listening. Both development ports must be available.

```bash
# Run entry adapters separately.
npm run dev:web
npm run dev:api

# CLI invokes the shared application library directly; it does not contact HTTP.
cargo run --locked -p chemifree-cli -- status --json
cargo run --locked -p chemifree-cli -- capabilities --json
cargo run --locked -p chemifree-cli -- data status --json

# Documents through the same application and data directory. Each CLI edit saves.
cargo run --locked -p chemifree-cli -- documents create --title Ethanol
cargo run --locked -p chemifree-cli -- documents apply <id> --expected-revision 0 --commands \
  '[{"op":"add_atom","element":"C","position":{"x":0,"y":0}},
    {"op":"add_atom","element":"C","position":{"x":26,"y":15},"bond_to":{"atom":"$0","order":"single"}},
    {"op":"add_atom","element":"O","position":{"x":52,"y":0},"bond_to":{"atom":"$1","order":"single"}}]'
cargo run --locked -p chemifree-cli -- documents undo <id> --expected-revision 1
cargo run --locked -p chemifree-cli -- documents show <id> --json

# A document style edit is undoable; preferences affect new documents only.
cargo run --locked -p chemifree-cli -- documents apply <id> --expected-revision 2 --commands '[{"op":"update_settings","bond_length_pt":40}]'
cargo run --locked -p chemifree-cli -- preferences show
cargo run --locked -p chemifree-cli -- preferences set --expected-revision 0 --preferences '{"ui":{"theme":"dark"},"new_documents":{"label_font":"serif"}}'

# Operation contract 1: one JSON request per line in, one reply per line out.
echo '{"contract":1,"request_id":"r1","profile":"preserve-semantics","operation":"capabilities.inspect","parameters":{}}' \
  | cargo run --locked -p chemifree-cli -- operations
cargo run --locked -p chemifree-cli -- documents save-as <id> --expected-revision 3 --title "Ethanol copy"
# After an API crash: recover the autosaved draft (and save it), or save it as a copy, or drop it.
cargo run --locked -p chemifree-cli -- documents recover <id>
cargo run --locked -p chemifree-cli -- documents recover-as-copy <id>
cargo run --locked -p chemifree-cli -- documents discard-draft <id>

# Optional API port and data directory; the address remains 127.0.0.1.
cargo run --locked -p chemifree-api -- --port 3002 --data-dir ./tmp/chemifree-data
```

The development proxy targets port 3001. If you choose another API port, update `apps/web/vite.config.ts` accordingly. The standalone CLI's `status` confirms its local application can run; it does not report whether an HTTP server is running. The browser holds no document state of its own: each edit is a request with the revision it was based on, and the API's working copy is authoritative until saved. An open API session and the CLI can edit the same file; saves from every process take an OS lock on the document, so a later save from the other side is refused with `save_conflict` rather than overwriting, and a crashed process never leaves the lock held.

## Check and build

The real-backend native transfer check is `npm run test:browser:clipboard-integration`. It starts an API on an assigned port with temporary storage and Vite on port 5173 (which must be free), then drives production clipboard panels, editor hook and HTTP client. Browser clipboard transport alone is replaced. See the [P33k check scope](docs/verification/p33k-native-clipboard-integration.md) and [bounded P33 completion evidence](docs/verification/p33-clipboard-completion.md).

```bash
npm run lint             # ESLint: JS/TS + React Hooks checks
npm run typecheck        # TypeScript project references
npm run test:client      # Web client against a scripted service: unknown outcomes, restarts
npm run build:web        # Production web assets in apps/web/dist
npm run check:rust       # rustfmt + clippy (-D warnings) + Rust tests (cargo-nextest, plus doctests)
npm run test:contracts   # Real HTTP/CLI processes: parity, document flow, guards, conflicts, failures
npm run test:conformance # Conformance cases → target/conformance/report.json; exit 1 if any case fails
npm run test:traceability # Traceability files against the roadmap, the recorded run, and npm scripts
npm run test:checks      # node:test checks for traceability rules and smoke request-log lifecycle
npm run check            # All of the above
npm run conformance:record   # Rewrite the committed run record (review the change before committing)
npm run build            # Web assets and debug Rust binaries
npm run test:browser     # Browser smoke (needs Chrome or Playwright's Chromium; CI job `browser`)
npm run test:browser:routes  # The Playwright race behind #8 and the smoke's guard against it
npm run test:browser:clipboard-integration  # Native graph/graphic forests across documents, real API
```

The fixtures of the substructure search (`crates/application/tests/data/query-fixtures.json`) are also compared, by hand, with an RDKit 2022.03.2 Python (no equivalence is claimed beyond those fixtures), which needs that RDKit and a built CLI: `python -I scripts/verify-query-rdkit.py target/debug/chemifree`. It states an expected outcome for every fixture (the full mapping set RDKit's `GetSubstructMatches(uniquify=False)` finds, or a specific refusal with the number of mappings RDKit finds) and fails on any difference or error; it is not part of `npm run check` and no CI job runs it. Run it whenever a fixture, the matcher's rules or the aromaticity model change, from the RDKit 2022.03.2 environment after `cargo build`.

The contract smoke builds the workspace, starts an API on an ephemeral loopback port with a temporary data directory, and compares HTTP JSON with actual CLI JSON. It runs create → edit → undo/redo → save → close → reopen, checks request-ID replay, revision and save conflicts, write guards, a save that fails because the target path is unwritable and then succeeds on retry, and that a killed API restarts with the last committed document. It then races six CLI saves from one base (exactly one commits), kills a process holding the save lock, races an API save against a CLI save, and runs autosave → SIGKILL → restart → recover/discard/recover-as-copy, an unreadable draft, a failed autosave, and save as copy over HTTP and the CLI. Storage feedback: while the API's save of a document pauses inside its save lock, asking about that save again is answered `request_in_progress` at once, requests queued behind it and a CLI save of the same document are refused as `busy` within the time budget without changing anything, and asking again afterwards returns the committed reply without writing again; a read-only `documents/` is reported as `read_only`. GitHub Actions installs npm 11.13.0 into an isolated runner-temporary prefix, adds its binaries to `PATH`, and verifies the selected version before running `npm ci` and `npm run check` with pinned tools.

The conformance runner (`crates/conformance`, binary `chemifree-conformance [--out PATH] SUITE`) runs the cases in `conformance/cases/` against the shared application with the same request types and arguments as the HTTP handlers and a `request_id` on every write (the HTTP layer is not involved, and status codes are not compared), each case on a fresh temporary data directory; `restart` drops the application without a normal stop, as a crash would. Pending cases name the roadmap item expected to make them runnable and are recorded as `not_run`, never as passed. `docs/traceability/` traces the 33 public readiness requirements (by part, with status, implementation paths, evidence, and follow-ups) and counts the 2,914 public interface declarations per proposed route; a declaration count is not a feature count. The functional spec archive stays outside the repository and traceability references it by path and SHA-256; the loader embeds only the public personal-local context needed to verify its exact identity. `CHEMIFREE_SPEC_DIR=<dir holding spec/> npm run test:traceability` also compares the references against the spec. [docs/TRACEABILITY.ko.md](docs/TRACEABILITY.ko.md) states exactly what these checks establish and what they do not (for example, that a status judgement is right, or that the repository holds no private data).

`npm run test:browser` starts the API and Vite on a temporary data directory (ports 3001 and 5173 must be free) and drives the editor with `playwright-core`: drawing, editing, undo/redo with buttons and keys, moving, saving, reopening, the discard prompt, revision-conflict reload, save failure and retry, read-only storage and a document busy behind a CLI's save lock (each named as such), a save whose reply arrives after the page's deadline (confirmed, not failed) and one whose replies are lost (**Save not confirmed**, then **Check again**), a copy still in progress when the page asks again (same request ID, one copy), a save conflict with the CLI, autosave state, crash recovery after killing the API, discarding and recovering drafts as copies, a failed autosave with **Retry now** and a retry that edits do not postpone, draft states after a restart, save as copy (including an original left open because its draft could not be removed, and the notice ending when the page leaves the copy or when an edit reaches a restarted service), a late autosave reply after leaving a document, drafting before leaving a document, a session moved on by another client, a duplicated placement moved on its own (with an atom added to both placements, one dragged in one only, and a third placement copied from the moved second), 320/390 px layouts, and dark mode. Point `CHEMIFREE_BROWSER` at a Chrome/Chromium executable, or run `npx playwright-core install chromium` once. The latest local result is in [docs/verification/editor-browser-smoke.json](docs/verification/editor-browser-smoke.json). The API runs with `--log-requests` (one line per request on standard error; also `CHEMIFREE_LOG_REQUESTS=1`). Every run, passed or failed (including a failed build or server start), leaves its records in `CHEMIFREE_SMOKE_OUT`, or in the temporary directory, which only a passing run removes:

- `results.json`: the steps passed, the last step reached, and the error.
- `build.log`: cargo's diagnostics.
- `environment.json`: the browser, driver, Node, and platform.
- `requests.log`, `api.log`, `web.log`: the browser's requests, the API's request log, and Vite's output.
- The screenshots, plus `failure.png` when a run fails.

A failed run also prints the step and the last requests. Records of an earlier run in `CHEMIFREE_SMOKE_OUT` are removed first; other files there are left alone. Run as root, the steps that make `drafts/` read-only are skipped (permissions do not stop root) and listed in `results.skipped_steps`; `CHEMIFREE_SMOKE_NO_SKIPS=1` makes that a failure.

CI runs both browser checks in the `browser` job, set up as follows:

- **Environment.** Ubuntu 24.04, with the Chromium build that the locked `playwright-core` names (`npx playwright-core install --with-deps chromium`).
- **Runs.** Each check runs once, without retries, with `CHEMIFREE_SMOKE_NO_SKIPS=1`. The smoke also runs when the route check fails.
- **Artifact.** The records are uploaded as `browser-smoke-attempt-<n>` whether the smoke passes or fails, and are kept for 14 days.

[docs/verification/browser-ci.json](docs/verification/browser-ci.json) records the job's runs, including one where a deliberately broken build failed it. The smoke keeps Playwright's request interception on for the whole run: otherwise a request sent just as the last step route is removed can stall in the browser, which made the restart step flaky (#8); `npm run test:browser:routes` reproduces that against a plain server ([docs/verification/browser-route-race.json](docs/verification/browser-route-race.json)).

`npm run preview --workspace @chemifree/web` previews built assets on port 4173 and inherits the local API proxy; run the API separately. Vite's preview server is a local verification tool. For a production deployment, serve `/api/v1/*` to the Rust API through the same origin. Hosting, authentication, and a production deployment configuration are future work.

## Workspace

```text
apps/
  web/              React + TypeScript + Vite presentation
    src/editor/     Structure canvas (SVG rendering, hit testing, gestures), separate from UI controls
  api/              Axum HTTP entry adapter; loopback only; write guards
  cli/              clap entry adapter; shared application calls
crates/
  domain/           Elements, document graph, commands, transactions, change-set history; no dependencies
  application/      Use cases, editing sessions, v1 DTOs, CapabilityCatalog and DocumentRepository ports
  providers/        Capability catalog and the local JSON file repository
  conformance/      Conformance runner and the chemifree-conformance binary
conformance/
  suite.json        The suite: suite_version and case paths
  cases/            Cases written for this project (runnable or pending)
scripts/
  contract-smoke.mjs HTTP/CLI boundary and document-flow verification
  browser-smoke.mjs  Manual browser verification of the editor
  traceability-check.mjs  Traceability check (and its tests in traceability-check.test.mjs)
docs/
  TRACEABILITY.ko.md What the traceability files, runner, and check establish
  testing-practice.md How tests are shown to fail, kept deterministic, compared with oracles, and run isolated
  traceability/     Requirement parts and declaration counts
  roadmap/          Implementation roadmap (PR plan)
  verification/     Run records, including conformance-report.json, and review history
  CONTRACT.md       v1 shape, compatibility rules, and capability identifiers
  architecture.html Detailed architecture and staged design
  SHADCN_REVIEW.ko.md shadcn/ui and @shadcn/lint review
```

Dependencies point inward: `application → domain`; `providers → application + domain`; HTTP and CLI compose the application with concrete providers. Document rules (stable IDs, reference integrity, atomic transactions, undo/redo as change sets) live in `crates/domain`; sessions, revisions, request-ID replay, and save semantics live in `crates/application`; file I/O lives in `crates/providers` behind the application-owned `DocumentRepository` port. The web mirror of the v1 contract validates responses at runtime. There is no code generation, service locator, database, or general prediction engine; the scoped property calculations are described in the contract. The structure canvas in `apps/web/src/editor/` is separate from the shadcn/ui controls.

## API and implementation status

The [interactive architecture document](docs/architecture.html) maps these layers to the actual source files, current contracts, UI decisions, and next implementation steps. Download the HTML and open it in a browser; GitHub's file viewer displays its source. It is self-contained and works offline. See also the [shadcn review](docs/SHADCN_REVIEW.ko.md) and [contract reference](docs/CONTRACT.md).

| HTTP | CLI | Meaning |
| --- | --- | --- |
| `GET /api/v1/health` | `status --json` | Application liveness contract; not scientific readiness |
| `GET /api/v1/capabilities` | `capabilities --json` | Actual implementation status |
| `GET/POST /api/v1/documents` | `documents list` / `create` | List and create documents |
| `GET /api/v1/documents/{id}` | `documents show` | Working copy |
| `POST …/transactions`, `…/undo`, `…/redo` | `documents apply` / `undo` / `redo` | Edits with `expected_revision` |
| `POST …/settings/reset`, `…/settings/import` | `documents settings reset` / `import` | Reset or import document style as an edit |
| `GET/PUT /api/v1/preferences`, `POST …/reset`, `…/import` | `preferences show` / `set` / `reset` / `import` | Data-directory UI and new-document preferences |
| `POST …/save`, `…/close` | (CLI edits save) | Commit; end session |
| `POST …/autosave` | — | Write the session's recovery draft |
| `POST …/save-as`, `…/recover-as-copy` | `documents save-as` / `recover-as-copy` | Commit as a new document |
| `POST …/recover`, `…/discard-draft` | `documents recover` / `discard-draft` | Act on a draft left by an ended process |
| `GET /api/v1/documents?query=&sort=` | `documents list --query --sort` | Search titles; sort by title or last save |
| `POST …/rename` | `documents rename` | Commit a new title (no unsaved changes) |
| `POST …/delete`, `GET /api/v1/trash`, `POST /api/v1/trash/{id}/restore` | `documents delete` / `trash` / `restore` | Move to the trash and back; refused while unsaved work or a draft exists |
| `POST /api/v1/operations` | `operations` (stdin lines) | Operation contract 1: versioned per-operation requests and replies; unimplemented operations answer `unsupported` |

All JSON responses include `api_version: "v1"`. Shared Rust DTOs live in `crates/application`; the TypeScript mirrors live in `apps/web/src/lib/`. See [the contract](docs/CONTRACT.md) for commands, errors, limits, the file format, and version policy. `runtime_inspection`, `document_editing`, `structure_analysis` (the explicitly labeled bounded valence inspection), `format_conversion` (available only with at least one format codec attached; its label names the attached formats and the SVG drawing, for example "MOL/SDF, CDXML, CDX, SMILES and SVG subsets" with all three codecs or "MOL/SDF and SVG subsets" with MOL/SDF alone; the export-only SVG drawing is described below), and `persistence` are `available`. The operation catalog is authoritative for individual operations; model loading, authentication and Windows integration report `not_implemented`.

The bounded SVG drawing export (P38a, status partial) is `document.export` with `format: "svg"` on HTTP, CLI and operation contract 1: it draws atoms, bonds and labels only and reports outcome `partial` whenever atoms exist; it requires `accepted_losses` consent only for the losses marked `yes` in its loss table, which are the features it leaves out or draws differently (radical and attachment marks, crossed stereo marks on bonds that are not double and marks that do not fit their bond order, stereo configurations and groups, label typography, truncated query labels, abbreviation displays, captions, graphics and images), while the losses marked `no`, including `chemical_graph`, need no `accepted_losses` but still make the outcome `partial`; a fixed font advance table means viewer fonts may be wider than the reserved text box; it is export-only (SVG import is refused with `unsupported_format`), has no web panel, and does not provide `render.scene` or general SVG output ([contract](docs/CONTRACT.md#svg-drawing-export-p38a)); P38 and RENDER-01 are not complete.

The API binds IPv4 loopback and sets no CORS permissions. Writes require an allowed `Host`, a same-site or allowed `Origin`, a per-process session token from `GET /api/v1/session`, a JSON body, and at most 256 KiB. This blocks other websites and DNS rebinding; it is not user authentication and does not separate users of the same machine. A remote or multiuser service still requires a separately implemented access-control boundary.

## Canvas navigation (P31)

The web canvas has Zoom in/out, Fit drawing, Reset zoom (100%), and Pan view (`H`). Ctrl/⌘ + wheel zooms at the pointer; ordinary wheel scrolling remains available. With the canvas focused, `+`/`−` zoom, `F` fits, `0` resets scale, and arrow keys pan. Hold Space while dragging, middle-drag, or use Pan view with one mouse/touch pointer. Pinch and other multi-touch gestures are not implemented.

The viewport belongs only to the current browser tab/canvas. View changes send no document commands, create no undo entry, and do not change saved coordinates or dirty state. Opening another document or reloading starts fitted; edits and resizing keep the current center and scale. Fit uses the full geometry bounds, including the empty drawing frame; 100% means one CSS pixel per document point and keeps the current center. Scale is bounded to 1/4096–32 CSS pixels per point. Selection tolerances and the click/drag threshold use screen pixels. The bounded P31 scope is complete in [PR #150](https://github.com/EunhakChoi/chemifree/pull/150); [merged-main CI](https://github.com/EunhakChoi/chemifree/actions/runs/42084338264) passed both jobs. See [the viewport contract](docs/CONTRACT.md#browser-canvas-viewport-p31).

## UI components and lint

The starter uses official **@shadcn** registry components with **Base UI / Nova**, Tailwind CSS 4, semantic tokens, Lucide icons, and a locally bundled Geist font. Installed: Alert, Alert Dialog, Badge, Button, Card, Dialog, Empty, Field (with Label and Separator), Input, Kbd, Select, Skeleton, Spinner, Toggle Group (with Toggle), and Tooltip. `components.json` records aliases and the theme CSS path. Generated source has been reviewed; upstream notices are preserved in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

Initialization used **shadcn CLI 4.22.1**:

```bash
cd apps/web
npx shadcn@latest init --base base --preset nova --no-monorepo -y
npx shadcn@latest info --json
npx shadcn@latest docs button card badge empty alert skeleton
npx shadcn@latest add @shadcn/button @shadcn/card @shadcn/badge @shadcn/empty @shadcn/alert @shadcn/skeleton -y
# Editor controls (existing button.tsx kept; the registry copy differed only by our notice header)
npx shadcn add @shadcn/toggle-group @shadcn/input @shadcn/field @shadcn/separator @shadcn/tooltip @shadcn/kbd @shadcn/alert-dialog @shadcn/spinner
# Save-as form (answered "no" to overwriting button.tsx)
npx shadcn@4.22.1 add @shadcn/dialog
```

Use the CLI's current help when adding components; `components.json` resolves this configuration to `base-nova`. The installed CLI and dependencies are pinned by `package-lock.json`. Review generated changes before committing them.

**`@shadcn/lint` 0.2.3 is installed and registered as an ESLint plugin. This validates plugin integration only; no shadcn design rules are enabled or enforced in default lint or CI yet.** It is a separate ESLint/Oxlint plugin, not a `shadcn lint` CLI command. Read [the review](docs/SHADCN_REVIEW.ko.md) before selecting design rules for this project.

## License and contribution boundary

[MIT](LICENSE), Copyright (c) 2026 EunhakChoi. Third-party components and fonts retain [their own notices](THIRD_PARTY_NOTICES.md).

Read [AGENTS.md](AGENTS.md) and [the contribution guide](.github/CONTRIBUTING.md) before contributing. Do not use original application DLLs as the execution engine. Keep credentials, local data, generated outputs, and caches outside this public repository. Each PR receives one Codex review and one independent subagent review. Scaffolding verification does not establish chemical correctness or compatibility with any other application.


### Optional Standard InChI (bounded P30)

On Linux, both hosts accept `--inchi-library /absolute/path/libinchi.so --inchi-version 1.07.5 --inchi-sha256 <64 lowercase hex>`, together, and optional `--inchi-timeout-seconds 5` (1–60). Equivalent variables are `CHEMIFREE_INCHI_LIBRARY`, `CHEMIFREE_INCHI_VERSION`, `CHEMIFREE_INCHI_SHA256` and `CHEMIFREE_INCHI_TIMEOUT_SECONDS`. Select a trusted build from the [official public release](https://github.com/IUPAC-InChI/InChI/releases/tag/v1.07.5); nothing is downloaded, discovered or bundled. The version is registered, not detected; SHA-256 checks selected bytes against your expected digest and does not prove provenance. The native timeout is cooperative and cannot interrupt library loading or a hung C call.

Send `{"contract":1,"request_id":"inchi-1","profile":"preserve-semantics","operation":"chem.inchi","parameters":{"document_id":"<open document id>"}}` to CLI `operations` or HTTP `POST /api/v1/operations`. Without a working registration it returns `unsupported` / `inchi_unavailable` and the host remains usable. This bounded profile uses ordinary bonds and stored tetrahedral/double-bond configurations, refuses unsupported chemistry and AND/OR groups, and requires digest-bound consent for omitted marks, absolute group metadata and native warnings. Standard normalization may merge distinctions; identifiers are not Chemifree canonical SMILES or a graph-equality proof. There is no dedicated web panel or InChI import. See [the contract](docs/CONTRACT.md#standard-inchi-and-inchikey) for limits and consent.

Caption runs additionally support independent boolean underline (P35g, native v15), with explicit SVG decoration bounds. P35 remains in progress; external caption roundtrip is unsupported.

Caption runs also support independent hollow glyph outlines (P35h, native v16), with run-color strokes and stroke-aware bounds; underlines retain filled decoration. P35 remains in progress.


P35i: 독립 caption run `shadow` bool(생략 false), +1,+1 pt/opacity0.35/blur0 및 기존 outline·underline·효과색상 조합, actual viewport provenance를 따르는 ink bounds와 native v17 strict/v8–16 false migration을 구현한다. v16 admission은 유지하며 물리 metadata192-byte 예약의 전체 상한은61,421,978 bytes(<64MiB)다. P35는 in_progress이며 외부 caption 호환·legacy rendering equivalence는 주장하지 않는다. 구현 에이전트는 테스트·빌드·cargo check·browser·skill gate를 실행하지 않았고 부모 격리 검증과 리뷰는 대기다. [P35i 기록](docs/verification/p35i-caption-run-shadow.md).

P35j: document settings `label_weight` normal/bold를 atom/query/H/isotope/charge와 graph-preserving Et의 표시만 제어하도록 추가한다. normal400/500·bold700이며 radical·독립 caption 정책과 chemical graph/inspection/property 결과는 유지한다. 브라우저가 실제700 자연 폭·glyph 돌출·수직 ink를 측정하고 trim/fit/hit/selection이 한 레이아웃을 공유한다. native v18 strict와 v1–17 normal migration/exact-byte archive, 이전 normal template payload 호환 및 exact style 비교, 기존 v16/v17 admission·64MiB 물리 경계를 보존한다. P35는 in_progress다. 회귀는 작성했으나 이 구현 작업에서 테스트·빌드·cargo check·브라우저·skill gate는 실행하지 않았다. 부모 격리 검증과 두 리뷰는 대기다. [P35j 기록](docs/verification/p35j-chemical-label-weight.md).

P35k adds document chemical **Label style** (`normal`/`italic`, default normal),
combined with normal/bold label weight. Atom/query/H/Et/isotope/charge/count text
changes appearance; radical/attachment marks and independent captions keep their
own styling. The shared HTTP/CLI edit, preferences, undo/replay, copy and recovery
paths carry it. Native v19 migrates earlier settings/history to normal and archives
old bytes; bounded external exports report drawing-style loss. SVG measurements
for italic include effective family, weight, size and viewport. P35 remains in
progress. [P35k checks](docs/verification/p35k-label-style.md) are authored and
unrun in this implementation session.


P35l adds optional chemical label underlining (`label_underline`, default false),
with measured per-run SVG rectangles and shared trim/pick/selection/fit bounds.
Radical/attachment marks and caption styling stay independent; graph, stereo and
coordinates are preserved. Settings and preferences share HTTP/CLI/UI and native
v20 persistence; v1–19 migrate to false with exact old-byte archival. Existing
external `drawing_settings` losses remain. P35 is still in progress. The added
checks (`npm run test:browser:label-underline`, client and Rust suites) are authored
but were not executed in this implementation session.

Chemical label outline (P35m, in progress) adds a default-off **Outline labels**
document setting, shared by preferences, HTTP/CLI, undo, save and recovery. Only
chemical glyphs and Et become hollow strokes; underlines stay filled and
caption/state marks remain independent. Native v21 migrates v1–20 as false with
exact byte archives. Browser coverage is registered in CI but was not run in
this session. See the contract for bounded rendering and format rules.


P35n implements document `label_shadow` (default false): the P35i +1,+1 pt,
0.35 opacity/no blur policy for chemical glyphs and Et, in one composited shadow
group before foreground. Actual SVG ink bounds include translated outline and
underline; state marks/captions and graph/stereo remain independent. Native v22
strictly checks current/history and migrates v1–21 to false with exact archives.
P35 remains `in_progress`. New browser/client/native checks are authored and
registered; tests, builds, cargo check and browser execution were intentionally
not run in this implementation session. See [P35n record](docs/verification/p35n-label-shadow.md).

P35o adds a plain display label on an already collapsed ethyl graph: one nonempty
Unicode line, at most 128 UTF-8 bytes; `text: null` resets it to `Et`. **Text does
not define chemistry.** Arbitrary text is never parsed, and the retained graph,
identity and formula stay unchanged. It uses document label typography and SVG
bounds; chemical exports retain the full graph and report display loss. Native
v23 stores current/history metadata and preserves exact v1–22 bytes before
replacement/recovery. General multistyle atom/abbreviation labels and fragment
editing remain unsupported. P35 stays `in_progress`; execution and both required
PR reviews are pending. See [P35o record](docs/verification/p35o-display-label.md).

P35p adds an unverified bounded candidate for explicit display contraction of an
existing connected acyclic neutral/default element fragment with one unmarked
single boundary at an explicit anchor and exactly one placement. The user supplies
members and one plain single-line text (up to 128 UTF-8 bytes), edits that text or
expands the display. Text does not define chemistry: the original graph, IDs,
states, stereo and coordinates supply every scientific result. Unsupported local
stereo, query states, rings, multiple boundaries/placements and overlapping
contractions refuse. The canvas uses shared measured label bounds; chemical
exports keep the full graph with a rendering loss, and native clipboard/Cut/template
export require expansion before intersecting member selections. Native v24 strictly
stores current/history fragment maps; v1–23 migrate empty with exact-byte archives
before replacement, and v25 refuses untouched. General nickname dictionaries,
text-to-graph parsing and external label roundtrip remain unsupported. P35 stays
`in_progress`; tests/build/browser execution and both reviews are pending.
See [P35p record](docs/verification/p35p-fragment-display.md).

P35q is an unverified follow-up candidate for baseline/subscript/superscript runs
on retained fragment displays. `set_fragment_display_runs` replaces explicit
nonempty runs within the same total 128 UTF-8 byte, nonblank single-line limit.
Plain-text commands reset to one baseline run; scripts inherit document label
style and share measured SVG bounds. Text and scripts do not define chemistry;
the retained graph supplies scientific results. Native v25 strictly stores runs
in current/history, migrates v24 text to baseline with exact document/draft
archives before replacement/recovery, and refuses future v26 untouched. Existing
full-graph export losses and clipboard/Cut/template restrictions remain binding.
P35 remains `in_progress`; execution and both reviews are pending. See
[P35q record](docs/verification/p35q-fragment-display-runs.md).

P36a adds a bounded candidate for editing an existing line, arrow or rectangle's
two stored points through `update_graphic_points` and the **Graphic endpoints**
panel. Finite coordinates within inclusive±100000 pt may be reversed or coincident;
shape, object IDs, groups, references and chemical graph remain intact. The edit
shares atomic transactions, replay, Undo/Redo and native v25 history without a
schema change. The UI guards invalid/IME input and stale queued geometry edits.
Resize handles, new shapes, images, clipping, layers and graphic styles remain
outside this slice. P36 is `in_progress`; checks, both reviews and PR CI are
pending. Historical: that was the status when this slice was written; P36a has since merged as #204 (`26962b6`) with main CI run 42323974348 success (see the [P36 status table](docs/HANDOFF.ko.md)). See [P36a record](docs/verification/p36a-graphic-points.md).

P36b adds candidate canvas endpoint handles for exactly one explicitly selected,
noncoincident line, arrow or rectangle in Select mode. Actual SVG screen-to-document
coordinates respect pan/zoom and preserve the pointer's press offset; dragging
previews locally and valid release sends one existing endpoint-update command.
Cancellation, stale document/revision/selection/target and service restart do not
silently commit or retry the drag. Coincident points use the numeric form. No
schema change or general resize/group/rotation handles are added. P36 remains
`in_progress`; prior P36a PR #204 merge and P36b full checks/reviews/CI are separate
pending gates. Historical: that was the status when this slice was written; P36b has since merged as #206 (`7601617`) with main CI run 42325796411 success (see the [P36 status table](docs/HANDOFF.ko.md)). See [P36b record](docs/verification/p36b-graphic-endpoint-drag.md).

P36c adds a candidate **Graphic** tool (G) with an explicit Line/Arrow/Rectangle
choice. A drag previews without writes and valid release submits one existing
`add_graphic` transaction for a fresh page root, without implicit group parenting
or chemistry changes. Pan/zoom coordinates, cancellation and captured document,
revision, shape and service guards keep stale previews from silently committing
or replaying after restart. Native v25 and existing graphic/format limits remain;
no new shapes, styles, layers or scientific arrow semantics are added. P36 stays
`in_progress`. Preceding #206 is merged with passing PR CI42324595907 and
successful post-merge main run42325796411.
See [P36c record](docs/verification/p36c-graphic-creation.md).

P36d adds candidate per-graphic `line_width_pt`: an explicit finite0.1–10 pt
value or null for dynamic document inheritance. A number equal to the current
document width still counts as an override. The selected-graphic form exposes
width/reset and effective inheritance; rendering, selection underlay and hit
stroke use that width without changing geometry or chemistry. Native v26 strictly
stores it through history/drafts and migrates v1–25 to inheritance with exact-byte
archives. P36d initially refused explicit widths in v2; P36e adds a v3 graphics profile
that preserves them. Resetting to document width remains an explicit choice that
may change appearance, not a Copy/Cut prerequisite. P36 stays `in_progress`.
The parent reports27 targeted Rust cases passed at e6e3b45; web/full/reviews/CI
remain pending. Preceding #207 merged afe6410; its main run42329036170 failed in the browser job and #209 followed.
See [P36d record](docs/verification/p36d-graphic-line-width.md).

P36e adds a candidate styled-graphics clipboard profile: all-inherited forests
still copy as strict v2, while any explicit width selects v3 with required
nullable width on every graphic. Paste accepts either profile, including all-null
v3, preserving explicit values and inherited state through groups, references,
offsets and Undo/Cut/Paste. V2 still forbids width fields; v2-only readers are not
claimed to support v3. Settings must match and existing byte/command limits remain.
Native v26 and chemical graphs are unchanged. P36 stays `in_progress`;
Rust/browser/full/reviews/CI remain pending without confirmed execution evidence.
Historical: that was the status when this slice was written; P36e has since merged as #210 (`d1f257c`) with main CI run 42335557133 success (see the [P36 status table](docs/HANDOFF.ko.md)).
See [P36e record](docs/verification/p36e-styled-graphics-clipboard.md).

P36f adds a candidate finite arbitrary-angle input beside fixed Scene rotations.
It captures IDs, placement and angle without freezing revision: the queued action
recomputes the pivot from latest geometry. Captions block the UI; non-quarter-turn
rectangle rotations reach the existing backend's atomic refusal. Existing restart
reconciliation applies, without a special no-replay rule. API/native26 and
geometry-only graph/stereo/width/reference behavior remain unchanged. Custom
pivot and rotation-drag controls are not added. P36 stays `in_progress`;
tests/reviews/CI remain pending while browser verification is in progress.
Historical: that was the status when this slice was written; P36f has since merged as #211 (`d1920be`) with main CI run 42337029831 success (see the [P36 status table](docs/HANDOFF.ko.md)).
See [P36f record](docs/verification/p36f-selection-rotation.md).

P36g adds candidate Scene **First**/**Last** buttons for one selected scene
object, preserving accompanying atom/bond selections. A group moves with its
subtree within the same parent; these are sibling-order controls, without new
layers or reparenting. Captured document/object/parent remain fixed while the
queue computes the boundary from latest siblings; existing boundaries are no-ops
and stale/deleted/reparented targets lapse. Ordinary revision, selection history,
restart reconciliation, API and native26 rules remain. P36 stays `in_progress`;
browser/full/reviews/CI are pending. Historical: that was the status when this slice was written; P36g has since merged as #212 (`6545af6`) with main CI run 42339112423 success (see the [P36 status table](docs/HANDOFF.ko.md)). See
[P36g record](docs/verification/p36g-scene-order.md).


P36h adds a bounded **Ellipse** candidate with exactly two opposite bounding-box
corners and stroke-only axis-aligned rendering. Reversed/coincident/zero-axis
corners are retained; zero-axis objects have no visible stroke, so use Scene
selection and the numeric corner form. Non-quarter-turn rotations refuse the
whole transaction, including circles and degenerate ellipses. Native27 admits
ellipses only under that header; old current/history headers remain strict, with
width required from v26. Graphics clipboard output uses required-width v4 when
the copied forest includes an ellipse, otherwise canonical v2/v3; older readers
are not assumed compatible. Graph/scientific state and existing export rendering
losses remain. Earlier P36a–g shape/schema/version statements are historical slice
boundaries, superseded here for P36h. P36 stays `in_progress`. Existing execution
logs and [PR #213](https://github.com/EunhakChoi/chemifree/pull/213) record passed
Rust clippy/117 focused tests, web lint/typecheck/client/build, ellipse/creation/
clipboard/endpoint/width/order/rotation and general browser checks. The independent
review of `4e56f5e` reported no findings. Codex's stale-status P2 is addressed by
this documentation correction; full isolated local validation and PR CI remain
in progress/pending, with no merge/main-CI success claimed. Historical: that was the status when this slice was written; P36h has since merged as #213 (`698a364`) with main CI run 42341200117 success (see the [P36 status table](docs/HANDOFF.ko.md)). See the
[P36h record](docs/verification/p36h-ellipse.md) for logs and exact scope.


P36i adds a closed stroke-only **Polygon** candidate with 3–64 ordered bounded
vertices. The64 cap is a Chemifree resource policy; repeated, crossing and
degenerate vertices stay exact. Existing shapes retain exactly two points, while
`add_polygon`/`update_polygon_vertices` handle whole polygon lists. Graphic click
drafts write only on explicit Finish; numeric list edits replace all vertices,
with captured/stale/restart guards and no automatic unknown-edit replay into a
replacement process. Native28/clipboard5 admit polygons with strict old headers,
shape counts and canonical older-profile output when no polygon is copied.
Polygon/history-only content shares48MiB current/8MiB history budgets and older
undo retention; polygon-bearing overflow has its own reason while caption-only
errors keep their existing code. Graph/scientific state and export rendering
losses remain. Earlier shape/version notes describe their historical slices.
P36 stays `in_progress`. Existing logs and [PR #214](https://github.com/EunhakChoi/chemifree/pull/214)
record passed Rust clippy/55 focused tests and five mutations, web lint/typecheck/
client/build, polygon/ellipse/creation/endpoint browsers and three web mutations.
Independent review of `13cebd5` and Codex review of `f1a7978` reported no findings.
Full isolated local validation and CI remained in progress when this was written, with no merge/main-CI success claimed (historical: P36i merged as #214 (`d7e2b16`), main CI run 42343862092 success; see the [P36 status table](docs/HANDOFF.ko.md)). See [P36i record](docs/verification/p36i-polygon.md) for exact evidence
and limits.


P36j adds a bounded **Spline** candidate: exactly P0,C1,C2,P3 defining one open
stroke-only cubic Bézier. Controls are explicit, without fitting/interpolation;
repeated/degenerate definitions stay exact. Bounds/Fit/marquee/arrangement and
transform pivot use all four stored points, a conservative box rather than exact
curve extrema or stroke-ink bounds. Dedicated add/update commands retain old
shape cardinalities. Native29/clipboard6 strictly preserve the definition/history
and reject it under older profiles, with canonical older output when no spline
is copied. Shared byte/undo budgets, typed spline/polygon/caption errors, graph
preservation and export rendering losses remain. Four-click draft writes only
on explicit Finish, with existing stale/restart guards and no automatic unknown-
create replay into a replacement process; numeric edits replace all four points.
This single open cubic is a Chemifree choice within a broader public spline spec.
Earlier no-curve/version statements are historical slice descriptions. P36 stays
`in_progress`. The Rust executor’s final log records clippy/71 focused tests and
five detected mutations with restored-source passes. Final web lint/typecheck/
client/build, spline/polygon/ellipse/creation/endpoint and general browser checks
passed, including the actual workspace focus/underlay regression; three web
mutations were detected and restored checks passed. The parent reports independent
review `1d28c6d` with no findings. The separate formatter attempt hit `ENOTCACHED`
and did not pass. Full isolated validation, Codex review and CI remain pending;
this documentation ran no product checks. Historical: that was the status when this slice was written; P36j has since merged as #215 (`31e9328`) with main CI run 42345870358 success (see the [P36 status table](docs/HANDOFF.ko.md)). See
[P36j record](docs/verification/p36j-spline.md) for exact scope and evidence.

P36k adds a bounded **Embedded PNG image** candidate. Choose a local static,
noninterlaced 8-bit RGB/RGBA PNG, then enter its origin and positive width/height
in pt and explicitly add it. Source files are limited to 128 KiB, 2048 pixels per
edge and 1,048,576 pixels per image; current drawings allow 4,194,304 image pixels.
The server validates the complete PNG. Pixel density does not select physical
size, and the stored rectangle may stretch the aspect ratio. Transparent pixels
remain part of the selectable rectangle. Geometry edits, movement, grouping,
ordering, alignment, references and ordinary history retain the embedded bytes;
rotation/reflection and image clipboard copying are refused. There is no URL
fetching, OCR or image-to-chemistry conversion. Native30 stores image objects;
older headers forbid them, including history-only states. Scientific graphs stay
unchanged and external exports report omitted scene rendering. P36 remains
`in_progress`. Focused Rust clippy/tests and mutations, web lint/typecheck/client/
build and image/regression browsers passed in the recorded runs. Independent
review of `eb40cfa` found a P2: image pixel limits could be bypassed through
history replay. The shared replay-state guard and a regression now address that
finding; follow-up clippy and 52 focused tests passed. Codex review of
`5ea2107` in [PR #216](https://github.com/EunhakChoi/chemifree/pull/216) completed
without findings. #216 merged as `7bec5ea`; a later Codex review of `f69f8de` left
an unresolved P2 (an image ordered before a molecule instance can cover that
instance's label shadow), fixed in [#219](https://github.com/EunhakChoi/chemifree/issues/219). The merge commit's main
CI run 42350215647 failed in a spline cancellation browser test; follow-up #217
(`4198438`) fixed the test and main run 42353217355 succeeded. A caption-shadow
ink-fit check (`scripts/web-caption-check.mjs:513`) also failed once in #217's CI and passed on rerun; its cause is not
determined ([#220](https://github.com/EunhakChoi/chemifree/issues/220)). P36 stays `in_progress`; the user stopped P36 after the PNG image (2026-11-07 session) and the next scope is set by the user's next request. These focused passes do not establish general
image-format or renderer parity.
See [P36k record](docs/verification/p36k-embedded-image.md).
