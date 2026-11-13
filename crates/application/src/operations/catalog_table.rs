//! The support table: entry rows, the transaction and property part tables, and each
//! operation's entry.

use std::borrow::Cow;

use chemifree_domain::operation::{Execution, OperationId, Representation};

use super::reply::Support;

/// An operation's entry and the wording that replaces its note while codecs are connected.
pub(super) struct Row<'a> {
    pub(super) entry: Entry,
    pub(super) parts: &'static [Entry],
    pub(super) note: Option<&'a str>,
}

/// One row of the support table.
pub(super) struct Entry {
    id: &'static str,
    representation: Representation,
    pub(super) execution: Execution,
    pub(super) followup: Option<&'static str>,
    pub(super) note: &'static str,
    commands: Option<&'static [&'static str]>,
}

pub(super) const fn runs(id: &'static str, note: &'static str) -> Entry {
    Entry {
        id,
        representation: Representation::Supported,
        execution: Execution::Executable,
        followup: None,
        note,
        commands: None,
    }
}

const fn missing(id: &'static str, followup: &'static str, note: &'static str) -> Entry {
    Entry {
        id,
        representation: Representation::NotSupported,
        execution: Execution::NotImplemented,
        followup: Some(followup),
        note,
        commands: None,
    }
}

const fn needs_data(id: &'static str, note: &'static str) -> Entry {
    Entry {
        id,
        representation: Representation::Supported,
        execution: Execution::RequiresData,
        followup: None,
        note,
        commands: None,
    }
}

impl Entry {
    /// Marks a transaction part as a capability carried out by `commands`, not a command.
    const fn carried_out_by(self, commands: &'static [&'static str]) -> Self {
        Self {
            commands: Some(commands),
            ..self
        }
    }

    pub(super) fn view(&self, note: Option<&str>, sub_operations: &[Entry]) -> Support {
        Support {
            id: self.id,
            representation: self.representation.as_str(),
            execution: self.execution.as_str(),
            followup: self.followup,
            note: note.map_or(Cow::Borrowed(self.note), |note| Cow::Owned(note.to_owned())),
            commands: self.commands,
            sub_operations: sub_operations
                .iter()
                .map(|entry| entry.view(None, &[]))
                .collect(),
        }
    }
}

/// The parts of a document transaction: each command, and what a document cannot hold yet.
const TRANSACTION_PARTS: [Entry; 65] = [
    runs("attach_ring", "Connects a fresh regular carbon ring to an existing atom along an explicit direction in the only placement (size 5 or 6, default 6; pattern single by default, or explicit kekule for size 6). Connector stays single. Ordinary bond-addition stereo clearing and group notes apply. No fusion, aromaticity inference or layout."),

    runs(
        "insert_ring",
        "Appends an independent regular carbon ring (size 5 or 6, default 6; pattern single by default, or explicit kekule for size 6) at an explicit center in the only placement, using the current bond length. Kekule states alternating double/single orders. No attachment, fusion, aromaticity or conformation calculation.",
    ),
    runs(
        "add_atom",
        "An element (1–118) or query atom with a position in every scene instance and its \
         recorded hydrogen, radical, and attachment states, optionally bonded to an atom.",
    ),
    runs(
        "update_atom",
        "Element or query, formal charge (−8 to +8), isotope mass number, and the recorded \
         hydrogen, radical, and attachment states.",
    ),
    runs(
        "move_atoms",
        "Translates atoms by a delta in one scene instance.",
    ),
    runs(
        "move_selection",
        "Moves the union of selected object subtrees, explicit atoms and bond endpoints once; graph targets use the named instance or the only instance. Transaction selection snapshots remain independent.",
    ),
    runs("transform_selection", "Geometry-only rotation/reflection about an explicit pivot of the effective selected union. Stored stereo, marks and groups remain unchanged; selected axis-aligned rectangles and ellipses require exact quarter-turn rotations."),
    runs(
        "arrange_objects",
        "Rigid geometry-only alignment or equal-gap distribution of outermost selected scene objects, with shared preflight and atomic history. No partial atom/bond targets or rendered label bounds.",
    ),
    runs(
        "delete_atoms",
        "Deletes atoms, their bonds, and their positions in every scene instance.",
    ),
    runs(
        "add_bond",
        "A single, double, triple, aromatic, dative, query, or unknown bond between two atoms.",
    ),
    runs(
        "update_bond",
        "Changes a bond's kind among single, double, triple, aromatic, dative, query, and \
         unknown.",
    ),
    runs("delete_bonds", "Deletes bonds."),
    runs("rename", "Sets the document title."),
    runs(
        "add_instance",
        "Draws the molecule again on the page: a scene instance copying another's positions \
         with an offset.",
    ),
    runs(
        "move_instance",
        "Translates every position of one scene instance.",
    ),
    runs(
        "delete_instance",
        "Deletes a scene instance with its positions; the last one stays.",
    ),
    runs("update_abbreviation_label", "Changes or resets cosmetic display text on a collapsed retained ethyl graph; text never defines chemistry."),
    runs("collapse_ethyl", "Displays a checked existing terminal ethyl graph as Et; chemistry and positions are retained."),
    runs("expand_ethyl", "Reveals the original stored ethyl drawing."),
    runs("contract_fragment", "Contracts explicitly named supported fragment members behind bounded display text, retaining the complete chemical graph and coordinates. Text does not define chemistry."),
    runs("set_fragment_display_text", "Sets fragment display text as one baseline run, retaining the graph and member IDs. Text does not define chemistry."),
    runs("set_fragment_display_runs", "Replaces fragment display runs with explicit baseline, subscript, or superscript treatment under the shared 128 UTF-8 byte single-line bound. Runs do not define chemistry."),
    runs("expand_fragment", "Reveals retained fragment members at their stored coordinates without changing the chemical graph."),
    runs("add_caption", "Adds an independent single-line Unicode caption with explicit script, weight, style, generic font family and nullable size_pt (finite 4–72 pt; null/omission follows drawing size), and nullable color (canonical opaque sRGB #RRGGBB; null/omission follows theme foreground) runs."),
    runs("update_caption", "Replaces a caption anchor and its complete script, weight, style, generic font family and nullable size_pt (finite 4–72 pt; null/omission follows drawing size), and nullable color (canonical opaque sRGB #RRGGBB; null/omission follows theme foreground) runs."),
    runs(
        "add_graphic",
        "Adds a line, rectangle, ellipse, or arrow to a page or group.",
    ),
    runs(
        "update_graphic_points",
        "Replaces exactly two points of an existing line, rectangle, ellipse, or arrow, preserving its shape, identity, parent, and references. Coordinates use the existing finite point bounds; coincident and reversed points remain supported.",
    ),
    runs("add_image", "Adds a verified embedded immutable static RGB/RGBA PNG with explicit positive size; no URL, OCR or chemical interpretation."),
    runs("update_image_geometry", "Replaces only image origin and positive size, retaining the source PNG, identity, hierarchy and references."),
    runs("add_spline", "Adds one bounded open stroke-only cubic Bézier from exactly four points P0, C1, C2, P3. Stored-point bounds are conservative; no interpolation or chemical meaning."),
    runs("update_spline_points", "Replaces the complete P0, C1, C2, P3 state of an existing spline, preserving identity, width, hierarchy and references."),
    runs("add_polygon", "Adds a closed stroke-only polygon with 3–64 ordered bounded vertices; repeated, collinear, crossing and degenerate geometry is retained."),
    runs("update_polygon_vertices", "Replaces every vertex of an existing polygon, retaining its identity, width, parent and references."),
    runs("set_graphic_line_width", "Sets a graphic's explicit finite 0.1–10 pt line width, or restores dynamic document inheritance with null. Shape, geometry, identity, hierarchy, references and chemical graph are retained."),
    runs("group", "Groups sibling scene objects in drawing order."),
    runs("ungroup", "Removes a group and keeps its children."),
    runs("reorder", "Moves an object among its siblings."),
    runs(
        "delete_objects",
        "Deletes selected objects, with or without their descendants.",
    ),
    runs(
        "move_objects",
        "Translates selected scene objects and their descendants once.",
    ),
    runs(
        "add_reference",
        "Records a directed reference to a scene object, atom, or bond.",
    ),
    runs("remove_reference", "Removes a recorded directed reference."),
    runs(
        "update_settings",
        "Changes the document's style settings as an undoable edit.",
    ),
    runs(
        "set_bond_mark",
        "Sets or clears how a bond is drawn: wedge or hash (with the narrow end recorded in the \
         mark) or wavy on a single bond, crossed on a double bond. A drawing fact only.",
    ),
    runs(
        "set_atom_configuration",
        "States or clears a stereocenter's neighbor order and turn, or states it unknown. \
         Stated, not derived from the drawing.",
    ),
    runs(
        "set_bond_configuration",
        "States or clears how one neighbor on each end of a double bond lie across it, or \
         states it unknown.",
    ),
    runs(
        "set_configuration_from_drawing",
        "Stores the configuration that wedge, hash, wavy and crossed marks and one scene \
         instance's positions show, for the named centers and double bonds (default: every one \
         a mark bears on), or refuses with the reason. It replaces the stored record of every \
         item it processes, a configuration stated by hand included, and writes nothing for an \
         item it skips. Idempotent. Read off the drawing, not judged: no CIP label, no check \
         that an atom is stereogenic.",
    ),
    runs(
        "create_stereo_group",
        "Creates an absolute, and or or enhanced-stereo group of stored tetrahedral centers that \
         are in no group, with a stored number (any 32-bit integer, 0 and negatives kept as \
         stated, or none; an absolute group takes none). A statement of which centers belong \
         together, not a ratio.",
    ),
    runs(
        "delete_stereo_group",
        "Deletes a group; its members keep their stored configurations and become ungrouped.",
    ),
    runs(
        "set_stereo_group_kind",
        "Changes a group's kind among absolute, and and or. A document has at most one absolute \
         group, and one of the kind with no number or the same number is refused.",
    ),
    runs(
        "set_stereo_group_number",
        "Sets or clears a group's stored number; a number another group of the kind has (none \
         counts as 0) is refused, merge the groups instead.",
    ),
    runs(
        "add_stereo_group_members",
        "Adds stored tetrahedral centers that are in no group.",
    ),
    runs(
        "remove_stereo_group_members",
        "Removes members, which become ungrouped; it never empties the group (delete it \
         instead). Reports the relation that is no longer stated.",
    ),
    runs(
        "merge_stereo_groups",
        "Moves every member of one group into another of the same kind and deletes the first. \
         Reports that members now belong together.",
    ),
    runs(
        "split_stereo_group",
        "Moves some, not all, members of an and or or group into a new group of the same kind \
         with a given number. Reports the relation that is no longer stated.",
    ),
    runs(
        "hydrogen_state",
        "Records a stated non-vertex hydrogen count (0–16), unknown hydrogens, or inferred (the \
         default). Representation only: no hydrogen count is computed from valence.",
    )
    .carried_out_by(&["add_atom", "update_atom"]),
    runs(
        "radical",
        "Records none, singlet, doublet, triplet, or unknown. Representation only.",
    )
    .carried_out_by(&["add_atom", "update_atom"]),
    runs(
        "query_atom_or_bond",
        "Records query atoms (any, any heavy, heteroatom, halogen, or a possibly negated list of \
         1–10 elements) and query bonds (any, or a list of kinds). chem.substructure matches \
         them; recording is separate.",
    )
    .carried_out_by(&["add_atom", "update_atom", "add_bond", "update_bond"]),
    runs(
        "aromatic_or_dative_bond",
        "Records aromatic, dative (from its begin atom, the donor), and unknown bonds. \
         Recording is separate from the bounded aromaticity and conversion operations.",
    )
    .carried_out_by(&["add_atom", "add_bond", "update_bond"]),
    runs(
        "attachment_point",
        "Records an atom's attachment points 1–4. Representation only.",
    )
    .carried_out_by(&["add_atom", "update_atom"]),
    runs(
        "scene_placement",
        "One chemical graph per document, drawn by scene instances on one page, each with its \
         own atom positions.",
    )
    .carried_out_by(&["add_instance", "move_instance", "delete_instance"]),
    runs(
        "scene_tree",
        "Records ordered groups, simple graphics, and selection in history.",
    )
    .carried_out_by(&[
        "add_graphic",
        "update_graphic_points",
        "add_image",
        "update_image_geometry",
        "add_spline",
        "update_spline_points",
        "add_polygon",
        "update_polygon_vertices",
        "set_graphic_line_width",
        "add_caption",
        "update_caption",
        "contract_fragment",
        "set_fragment_display_text",
        "set_fragment_display_runs",
        "expand_fragment",
        "group",
        "ungroup",
        "reorder",
        "delete_objects",
        "move_objects",
        "move_selection",
        "arrange_objects",
        "transform_selection",
        "add_reference",
        "remove_reference",
    ]),
    runs(
        "document_settings",
        "Each document stores and undoes its own style; preferences set new-document defaults.",
    )
    .carried_out_by(&["update_settings"]),
    runs(
        "stereo_marks",
        "Records drawn stereo marks. Only set_configuration_from_drawing reads them to store a \
         configuration, on request, and chem.stereo reads them to compare a stored configuration \
         with the drawing without editing. The web editor draws wedge, hashed wedge, wavy and \
         crossed marks; its bond tool sends set_configuration_from_drawing in the same \
         transaction as a mark it draws.",
    )
    .carried_out_by(&["set_bond_mark"]),
    runs(
        "stereo_configuration",
        "Records tetrahedral centers (3 or 4 neighbors) and double bonds as stated, apart from \
         the marks: no mark blocks a stated configuration. Only set_configuration_from_drawing \
         derives one from the marks, on request, replacing the stored record of each item it \
         processes. chem.stereo compares a stored record with the drawing, read-only; it does not \
         compute labels or judge stereogenicity. chem.cip computes CIP labels from stored \
         configurations, read-only (its model and rules are named in the chem.cip entry). No \
         comparison in chem.identity or chem.substructure (they do not read stereo or groups), \
         no stereo check in chem.validate, no stereo in SMILES (CDXML and CDX import reads bond marks, configurations and groups and nothing writes them from a drawing), and a MOL or SDF \
         file carries only the bond marks (V2000 bond stereo column, V3000 CFG) and the \
         configurations they show; both dialects are read back.",
    )
    .carried_out_by(&[
        "set_atom_configuration",
        "set_bond_configuration",
        "set_configuration_from_drawing",
    ]),
    runs(
        "stereo_group",
        "Records absolute, and and or enhanced-stereo groups: sets of stored tetrahedral \
         centers, each center in at most one group, shown in the snapshot's stereo.groups with \
         the stored number, the interpreted number and a label. Limits: at most one group for \
         each stored center, so no more than the 2,000-atom cap; at most one absolute group; a \
         stored number is any 32-bit signed integer. Nothing states a ratio, a chiral flag or \
         double-bond members. chem.cip, chem.identity, chem.substructure and chem.stereo \
         ignore groups (chem.stereo says so); a SMILES or V2000 MOL export needs consent to \
         drop them, and a V3000 MOL export writes them as COLLECTION blocks.",
    )
    .carried_out_by(&[
        "create_stereo_group",
        "delete_stereo_group",
        "set_stereo_group_kind",
        "set_stereo_group_number",
        "add_stereo_group_members",
        "remove_stereo_group_members",
        "merge_stereo_groups",
        "split_stereo_group",
    ]),
];

const PROPERTY_PARTS: [Entry; 3] = [
    runs(
        "formula",
        "Hill formula and charge for a molecule, selection, or graph components under the bounded valence model.",
    ),
    needs_data(
        "mass",
        "A compatible checked mass registration is required; individual elements or isotopes may still be unavailable.",
    ),
    needs_data(
        "isotope_distribution",
        "A compatible checked isotope mass is required; natural distributions also need suitable abundances.",
    ),
];

pub(super) fn entry(operation: OperationId) -> (Entry, &'static [Entry]) {
    use OperationId as Op;
    let id = operation.as_str();
    match operation {
        Op::CapabilitiesInspect => (runs(id, "Reports this catalog."), &[]),
        Op::DocumentCreate => (
            runs(id, "Creates an empty drawing document and opens it."),
            &[],
        ),
        Op::DocumentSnapshot => (
            runs(id, "Reads an open or stored document and its session."),
            &[],
        ),
        Op::DocumentSelectionScope => (
            runs(
                id,
                "Read-only effective movement scope for an explicit selection and optional scene instance, with its exact document snapshot. Expands containment, never directed references, and changes no selection or history.",
            ),
            &[],
        ),
        Op::DocumentArrangementPlan => (
            runs(
                id,
                "Read-only checked plan for six geometry-only alignments and two equal-gap distributions of outermost selected scene objects. Returns ordered rigid units, original bounds, deltas, scopes and the exact snapshot; no model edit.",
            ),
            &[],
        ),
        Op::DocumentTransformPlan => (
            runs(
                id,
                "Read-only checked rotation/reflection geometry for an explicit selected union and pivot. Reports before/after positions/graphics, exact snapshot/scope and an advisory for changed graph geometry; never re-derives or alters stored stereo.",
            ),
            &[],
        ),
        Op::DocumentClipboardCopy => (
            runs(
                id,
                "Copies a bounded native induced graph fragment from an explicit placement, without editing. Scene objects, enhanced groups and stereo boundary loss are refused.",
            ),
            &[],
        ),
        Op::DocumentTemplateInspect => (
            runs(
                id,
                "Read-only native template validation and explicit anchor eligibility; no mutation or ID reservation.",
            ),
            &[],
        ),
        Op::DocumentTemplatePlan => (
            runs(
                id,
                "Read-only translation-only single-anchor attachment preview at an exact destination revision.",
            ),
            &[],
        ),
        Op::DocumentTemplateReplacePlan => (
            runs(
                id,
                "Read-only terminal atom replacement preview; retains the anchor and outside bond identities.",
            ),
            &[],
        ),
        Op::DocumentTemplateReplace => (
            runs(
                id,
                "Atomically replaces a terminal atom with a bound native anchor; outside identity, marks and references are retained.",
            ),
            &[],
        ),
        Op::DocumentTemplateAttach => (
            runs(
                id,
                "Atomically connects a fresh native template through one single bond; source-anchor stereo loss is refused.",
            ),
            &[],
        ),
        Op::DocumentClipboardPaste => (
            runs(
                id,
                "Atomically appends validated native graph data with fresh IDs into a single matching-style placement. No arbitrary command or external-format clipboard import.",
            ),
            &[],
        ),
        Op::DocumentClipboardSceneCopy => (
            runs(
                id,
                "Copies an ordered graphic-only scene forest and its internal object references, using v4 when an ellipse is included and otherwise preserving v2/v3 selection; refuses molecule instances, graph targets and outgoing references.",
            ),
            &[],
        ),
        Op::DocumentClipboardScenePaste => (
            runs(
                id,
                "Atomically appends graphic-only roots with fresh object IDs and remapped internal references under exact settings; chemistry and placements stay unchanged.",
            ),
            &[],
        ),
        Op::DocumentTransaction => (
            runs(
                id,
                "Applies commands all or nothing against an expected revision. A drawing only: nothing is checked chemically.",
            ),
            &TRANSACTION_PARTS,
        ),
        Op::HistoryUndo => (runs(id, "Undoes the last change set."), &[]),
        Op::HistoryRedo => (runs(id, "Redoes the last undone change set."), &[]),
        Op::DocumentSave => (
            runs(
                id,
                "Commits the working copy as a local Chemifree JSON file.",
            ),
            &[],
        ),
        Op::DocumentClose => (
            runs(
                id,
                "Ends the session; unsaved changes need explicit consent.",
            ),
            &[],
        ),
        Op::DocumentImport => (
            missing(id, "P20", "No chemical file reader is implemented."),
            &[],
        ),
        Op::DocumentExport => (
            missing(id, "P20", "No chemical file writer is implemented."),
            &[],
        ),
        Op::ChemValidate => (
            runs(
                id,
                "Read-only bounded valence and hydrogen inspection; unsupported states are diagnosed.",
            ),
            &[],
        ),
        Op::ChemAromaticity => (
            runs(
                id,
                "Read-only topological ring systems and a named bounded aromaticity interpretation, under a time limit (time_limit_ms).",
            ),
            &[],
        ),
        Op::ChemAromaticityConvert => (
            runs(
                id,
                "Converts supported aromatic or Kekule bonds as one atomic undoable edit under a named model. Computed outside the session lock under a time limit (time_limit_ms); a result for a revision that is no longer current is refused.",
            ),
            &[],
        ),
        Op::ChemIdentity => (
            runs(
                id,
                "Compares bounded recorded or model-normalized graphs and reports canonical numbering and symmetry, under a time limit (time_limit_ms).",
            ),
            &[],
        ),
        Op::ChemSubstructure => (
            runs(
                id,
                "Read-only substructure search of one drawing in another: every atom and bond mapping under a named model, or a refusal or limit, under a time limit (time_limit_ms).",
            ),
            &[],
        ),
        Op::ChemStereo => (
            runs(
                id,
                "Read-only comparison of the stored stereo configurations with what each scene instance's drawing shows, per center and double bond, with advisory findings, under a time limit (time_limit_ms). Nothing is derived into the document and nothing is judged: it computes no CIP label (chem.cip does), no stereogenicity, no ring or strain plausibility.",
            ),
            &[],
        ),
        Op::ChemCip => (
            runs(
                id, "", // the note is built from the active labeler: `cip::catalog_note`
            ),
            &[],
        ),
        Op::ChemInchi => (
            Entry {
                representation: Representation::Supported,
                ..missing(
                    id,
                    "P30",
                    "The optional native InChI provider is unavailable. This read-only operation requires an explicitly configured provider and returns no identifier without one. It runs under a time limit (time_limit_ms) that is asked before the native call and again once the native-call lock is held; the native call itself is not interrupted.",
                )
            },
            &[],
        ),
        Op::ChemProperties => (
            runs(
                id,
                "Computes bounded formula and, with available external data, mass or isotope distribution from an immutable graph snapshot, under a time limit (time_limit_ms).",
            ),
            &PROPERTY_PARTS,
        ),
        Op::ChemNameToStructure => (
            missing(id, "P56", "No chemical name parser is implemented."),
            &[],
        ),
        Op::ChemStructureToName => (missing(id, "P55", "No naming is implemented."), &[]),
        Op::LayoutStructure => (
            missing(id, "P42", "No structure layout is implemented."),
            &[],
        ),
        Op::LayoutReaction => (
            missing(id, "P51", "No reaction layout is implemented."),
            &[],
        ),
        Op::RenderScene => (
            missing(
                id,
                "P38",
                "No scene renderer or export is implemented; the web editor draws its own view.",
            ),
            &[],
        ),
        Op::BiopolymerExpand => (
            missing(id, "P69", "No biopolymer model is implemented."),
            &[],
        ),
        Op::PredictionEvaluate => (
            missing(
                id,
                "P59",
                "No prediction engine or model data is connected.",
            ),
            &[],
        ),
        Op::IntegrationInvoke => (
            missing(id, "W01", "No desktop or host integration is connected."),
            &[],
        ),
        Op::ScriptEvaluate => (missing(id, "P84", "No scripting is implemented."), &[]),
    }
}
