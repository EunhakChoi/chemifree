//! Operation contract 1: one versioned request and result envelope for every operation, shared
//! by every entry adapter (`POST /api/v1/operations`, `chemifree operations`, and in-process
//! callers).
//!
//! A request names the contract version, a request ID, a compatibility profile, an operation,
//! and that operation's parameters. A reply is either an `operation_result`, whose `outcome`
//! says whether the operation ran (`ok`, `partial`), failed (`error`), or is not supported
//! (`unsupported`), or a `protocol_error` for a request that cannot be read as one. Failures of
//! a known operation carry the same codes as the v1 error envelope. An unsupported operation,
//! profile, or model never returns an empty or made-up result.
//!
//! [`Application::operation_catalog`] reports, per operation and sub-operation, whether it is
//! representable and whether it runs; the dispatcher below runs exactly the operations it marks
//! executable.

mod arrangement;
mod cip;
mod import;
mod inchi;
mod interchange;
mod selection_scope;
mod smiles;
mod stereo;
mod substructure;
mod transform;

use chemifree_domain::budget::{Budget, Interrupt, NEVER};
use chemifree_domain::document::Command;
use chemifree_domain::operation::{CompatibilityProfile, Execution, OperationId, Representation};
use chemifree_domain::{graph::MoleculeGraph, identity};
pub use interchange::{
    InterchangeExportResult, InterchangeMapping, InterchangePreviewResult, InterchangeReadResult,
};
pub use smiles::{
    SmilesExportResult, SmilesPreviewAtom, SmilesPreviewBond, SmilesPreviewResult, SpanView,
};
use std::borrow::Cow;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::contract::{
    CommandDto, DocumentResponse, DocumentView, RecoveryView, SelectionDto, SessionView,
    TransactionSelections,
};
use crate::ct_projection;
use crate::ctfile::{CtCodec, CtError, CtField, CtFormat, CtRecord, CtResult, CtTab};
use crate::documents::{AromaticityConversion, Compute, validate_request_id};
use crate::error::{ApplicationError, ErrorCode};
use crate::json;
use crate::properties::{
    self, ComponentView, MassDataView, PropertiesError, PropertiesScope, PropertyKind,
};
use crate::{Application, CapabilityCatalog, DocumentRepository};
use arrangement::ArrangementParameters;
pub use arrangement::{ArrangementBounds, ArrangementPlanResult, ArrangementUnitView};
use cip::CipParameters;
pub use cip::{
    AtomLabel, BondLabel, CipResult, EndLabel, Priority, REASONS as CIP_REASONS, reason_sentence,
};
use import::{ImportFormat, ImportRequest, Staged};
use inchi::InchiParameters;
pub use inchi::InchiResult;
use selection_scope::SelectionScopeParameters;
pub use selection_scope::{EffectiveSelectionScope, ScopePosition, SelectionScopeResult};
use smiles::{SmilesExportParameters, SmilesImportParameters};
use stereo::StereoParameters;
pub use stereo::{
    AtomDrawnView, AtomInspection, BondDrawnView, BondInspection, MAX_ENTRIES, StereoResult,
};
use substructure::SubstructureParameters;
pub use substructure::{
    AtomPair, BondPair, MATCH_CODES, MappingView, MatchCodeRow, SubstructureLimits,
    SubstructureResult, SubstructureStereo,
};
use transform::TransformParameters;
pub use transform::{TransformPlanResult, TransformedGraphicView, TransformedPositionView};

/// The operation contract version this service speaks.
pub const OPERATION_CONTRACT: u32 = 1;

const ENVELOPE_FIELDS: [&str; 5] = [
    "contract",
    "request_id",
    "profile",
    "operation",
    "parameters",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Envelope {
    OperationResult,
    ProtocolError,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The operation ran to completion. This says nothing about chemical correctness.
    Ok,
    /// It ran, and `loss_report` lists what it could not keep or validate.
    Partial,
    Error,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Diagnostic {
    pub code: &'static str,
    pub severity: Severity,
    pub message: String,
    /// Where in the request it applies (`parameters.commands[2]`), or null.
    pub path: Option<String>,
    /// Code-specific facts, such as `current_revision`, `command_index`, `reason`, or `followup`.
    pub details: Map<String, Value>,
}

/// What happened to one feature of the input that the result does not keep as it was.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Loss {
    pub feature: String,
    pub source_path: String,
    pub disposition: LossDisposition,
    pub reason: String,
    pub semantic_impact: SemanticImpact,
    /// Whether the content may only be returned after the caller accepts this loss. The
    /// provider that causes a loss decides; it is not part of the wire report.
    #[serde(skip)]
    pub consent: LossConsent,
}

/// The consent a reported loss needs before an export returns its content.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LossConsent {
    /// Reported only; the content is returned.
    #[default]
    NotRequired,
    /// The caller must accept the loss (`accept_loss`) first.
    Required,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LossDisposition {
    Retained,
    Normalized,
    Dropped,
    Rejected,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticImpact {
    None,
    Chemistry,
    Document,
    Geometry,
    Stereo,
    Rendering,
    Unknown,
}

/// A reply of the operation contract.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OperationResponse {
    pub envelope: Envelope,
    pub contract: u32,
    /// Null only in a protocol error whose request ID could not be read.
    pub request_id: Option<String>,
    /// Null only in a protocol error whose operation could not be read.
    pub operation: Option<String>,
    /// Null in a protocol error.
    pub profile: Option<&'static str>,
    pub outcome: Outcome,
    /// The document's revision after the operation, or the current one with a revision
    /// conflict; null when the operation concerns no document.
    pub document_revision: Option<u64>,
    /// The operation's typed result; null unless the outcome is `ok` or `partial`.
    pub result: Option<OperationResult>,
    /// The model or method that produced the result; null when none was used.
    pub model_id: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
    pub loss_report: Vec<Loss>,
}

impl OperationResponse {
    /// The HTTP adapter answers a protocol error with 400 and an operation result with 200.
    pub fn is_protocol_error(&self) -> bool {
        self.envelope == Envelope::ProtocolError
    }

    /// The reply to a request an entry adapter refused to read, such as a body over its size
    /// limit (`resource_limit`) or of another media type (`unsupported_media_type`).
    pub fn unread(code: &'static str, message: impl Into<String>) -> Self {
        protocol_error(
            None,
            None,
            Outcome::Error,
            diagnostic(code, message, Map::new()),
        )
    }
}

/// Each operation's result type.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum OperationResult {
    /// `capabilities.inspect`.
    Catalog(OperationCatalog),
    /// `document.create`, `document.snapshot`, `document.transaction`, `history.undo`,
    /// `history.redo`, and `document.save`: the document and its session afterwards. Boxed:
    /// a document view is much larger than the other results.
    Document(Box<DocumentResult>),
    /// `document.selection_scope`: read-only movement targets at an exact snapshot.
    SelectionScope(Box<SelectionScopeResult>),
    /// `document.arrangement_plan`: checked rigid object translations at an exact snapshot.
    ArrangementPlan(Box<ArrangementPlanResult>),
    /// `document.transform_plan`: checked geometry-only rotation/reflection at an exact snapshot.
    TransformPlan(Box<TransformPlanResult>),
    /// `document.close`.
    Closed(ClosedResult),
    InterchangeRead(Box<InterchangeReadResult>),
    InterchangePreview(Box<InterchangePreviewResult>),
    InterchangeExport(Box<InterchangeExportResult>),
    CtPreview(Box<CtPreviewResult>),
    CtExport(CtExportResult),
    SmilesPreview(Box<SmilesPreviewResult>),
    SmilesExport(Box<SmilesExportResult>),
    /// `chem.validate`: inspection and the exact document snapshot it used.
    Valence(Box<ValenceResult>),
    /// `chem.properties` over one immutable document snapshot.
    Properties(Box<PropertiesResult>),
    /// `chem.aromaticity`: cyclic systems and their model-specific interpretations.
    Aromaticity(Box<AromaticityResult>),
    /// `chem.identity`: canonical graph identity and an optional second snapshot comparison.
    Identity(Box<IdentityResult>),
    /// `chem.substructure`: every mapping of a query drawing in a target drawing.
    Substructure(Box<SubstructureResult>),
    /// `chem.stereo`: stored against drawn stereo, per item and scene instance.
    Stereo(Box<StereoResult>),
    /// `chem.cip`: CIP descriptors of the stored configurations, per center and double bond.
    Cip(Box<CipResult>),
    /// `chem.inchi`: Standard identifiers from an explicitly configured optional provider.
    Inchi(Box<InchiResult>),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct IdentityResult {
    pub snapshot: DocumentResult,
    pub identity: IdentityView,
    pub comparison: Option<IdentityComparison>,
    /// What this model does with stereo. Added within v1.
    pub stereo: IdentityStereo,
}

/// Stereo and `chem.identity`. `compared` is a property of the model, not of the pair: it is
/// `false` in every reply because this model never reads stereo, whether or not the drawings
/// hold any. The `*_present` fields are facts about the compared documents.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct IdentityStereo {
    pub compared: bool,
    /// The source document holds a stereo mark or stated configuration.
    pub source_present: bool,
    /// The same for `other_document_id`; null when no other document was given.
    pub other_present: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct IdentityView {
    pub code: String,
    pub numbering: Vec<String>,
    pub symmetry: Vec<Vec<String>>,
    pub folded_hydrogens: Vec<FoldedHydrogen>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FoldedHydrogen {
    pub hydrogen: String,
    pub attached_to: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct IdentityComparison {
    pub snapshot: DocumentResult,
    pub identity: IdentityView,
    pub equivalent: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AromaticityResult {
    pub snapshot: DocumentResult,
    pub systems: Vec<AromaticitySystem>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AromaticitySystem {
    pub atoms: Vec<String>,
    pub bonds: Vec<String>,
    pub cycle_rank: usize,
    pub classification: &'static str,
    pub electrons: Option<u16>,
    pub reason: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ValenceResult {
    pub snapshot: DocumentResult,
    pub atoms: Vec<ValenceAtom>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ValenceAtom {
    pub atom_id: String,
    pub bond_order_sum: Option<u16>,
    pub hydrogens: Option<u16>,
    pub status: &'static str,
    pub reason: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PropertiesResult {
    pub snapshot: DocumentResult,
    pub scope: &'static str,
    pub selection: Option<SelectionDto>,
    pub property: &'static str,
    pub valence_model: &'static str,
    pub arithmetic: &'static str,
    pub mass_unit: Option<&'static str>,
    pub display_decimals: u8,
    pub data: Option<MassDataView>,
    pub valence_warnings: Vec<PropertyValenceWarning>,
    pub components: Vec<ComponentView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PropertyValenceWarning {
    pub atom: String,
    pub status: &'static str,
    pub reason: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DocumentResult {
    pub document: DocumentView,
    pub session: SessionView,
    pub recovery: Option<RecoveryView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interchange_mapping: Option<InterchangeMapping>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub import_mapping: Option<CtMapping>,
    /// What the request cleared of stereo (see `DocumentResponse::notes`). Added within v1;
    /// always present, and empty unless the operation is a transaction that cleared a record.
    pub notes: Vec<Loss>,
}

impl From<DocumentResponse> for DocumentResult {
    fn from(response: DocumentResponse) -> Self {
        Self {
            document: response.document,
            session: response.session,
            recovery: response.recovery,
            interchange_mapping: None,
            import_mapping: None,
            notes: response.notes,
        }
    }
}

/// One-based file indices and their corresponding native identifiers.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CtMapping {
    pub atoms: Vec<String>,
    pub bonds: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CtPreviewResult {
    pub selected_record: usize,
    pub records: Vec<CtRecord>,
    /// Exact working snapshot captured under the same lock as the candidate.
    pub snapshot: DocumentResult,
    pub document: DocumentView,
    pub preview_digest: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CtExportResult {
    pub content: String,
    /// The dialect every record was written in: `v2000` or `v3000`. A request writes one dialect.
    pub ctab: &'static str,
    pub records: Vec<CtExportMapping>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CtExportMapping {
    /// The exact working copy used for this record, including its revision.
    pub snapshot: DocumentResult,
    pub atoms: Vec<String>,
    pub bonds: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ClosedResult {
    pub document_id: String,
    /// True when the session had unsaved changes and they were discarded.
    pub discarded_changes: bool,
}

/// What this service supports of the operation contract.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OperationCatalog {
    pub implementation: &'static str,
    pub version: &'static str,
    /// Operation contract versions this service accepts.
    pub contracts: Vec<u32>,
    pub profiles: Vec<ProfileSupport>,
    pub operations: Vec<Support>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProfileSupport {
    pub id: &'static str,
    pub execution: &'static str,
    pub note: &'static str,
}

/// How far an operation, or one part of it, is supported. Representation and execution are
/// separate: a document can hold what an operation would compute without anything computing it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Support {
    pub id: &'static str,
    pub representation: &'static str,
    pub execution: &'static str,
    /// The plan entry expected to deliver what is missing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub followup: Option<&'static str>,
    pub note: Cow<'static, str>,
    /// Present on a sub-operation that is a capability rather than a command: the commands
    /// that carry it out (none while it is not implemented). Added within operation contract 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commands: Option<&'static [&'static str]>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sub_operations: Vec<Support>,
}

/// Which provider serves a `document.import` or `document.export` request.
enum Backend {
    Smiles,
    Interchange,
    Ct,
    /// The format names a provider that is not connected.
    NotConnected(&'static str),
}

fn not_connected(provider: &str) -> Diagnostic {
    diagnostic(
        "unsupported_format",
        format!("The {provider} provider is not connected."),
        Map::new(),
    )
}

/// An operation's entry and the wording that replaces its note while codecs are connected.
struct Row<'a> {
    entry: Entry,
    parts: &'static [Entry],
    note: Option<&'a str>,
}

/// One row of the support table.
struct Entry {
    id: &'static str,
    representation: Representation,
    execution: Execution,
    followup: Option<&'static str>,
    note: &'static str,
    commands: Option<&'static [&'static str]>,
}

const fn runs(id: &'static str, note: &'static str) -> Entry {
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

    fn view(&self, note: Option<&str>, sub_operations: &[Entry]) -> Support {
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
const TRANSACTION_PARTS: [Entry; 46] = [
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
    runs("transform_selection", "Geometry-only rotation/reflection about an explicit pivot of the effective selected union. Stored stereo, marks and groups remain unchanged; selected rectangles require quarter-turn rotations."),
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
    runs(
        "add_graphic",
        "Adds a line, rectangle, or arrow to a page or group.",
    ),
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

fn entry(operation: OperationId) -> (Entry, &'static [Entry]) {
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
                "Read-only topological ring systems and a named bounded aromaticity interpretation.",
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
                "Compares bounded recorded or model-normalized graphs and reports canonical numbering and symmetry.",
            ),
            &[],
        ),
        Op::ChemSubstructure => (
            runs(
                id,
                "Read-only substructure search of one drawing in another: every atom and bond mapping under a named model, or a refusal or limit.",
            ),
            &[],
        ),
        Op::ChemStereo => (
            runs(
                id,
                "Read-only comparison of the stored stereo configurations with what each scene instance's drawing shows, per center and double bond, with advisory findings. Nothing is derived into the document and nothing is judged: it computes no CIP label (chem.cip does), no stereogenicity, no ring or strain plausibility.",
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
                    "The optional native InChI provider is unavailable. This read-only operation requires an explicitly configured provider and returns no identifier without one.",
                )
            },
            &[],
        ),
        Op::ChemProperties => (
            runs(
                id,
                "Computes bounded formula and, with available external data, mass or isotope distribution from an immutable graph snapshot.",
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

fn profile_support(profile: CompatibilityProfile) -> ProfileSupport {
    let (execution, note) = match profile {
        CompatibilityProfile::PreserveSemantics => (
            Execution::Executable,
            "Operations report conversion losses and uncertainty; imports require consent for reported losses.",
        ),
        CompatibilityProfile::LegacyObserved => (
            Execution::NotImplemented,
            "No observed behavior of another program is reproduced. Only capabilities.inspect answers under it.",
        ),
    };
    ProfileSupport {
        id: profile.as_str(),
        execution: execution.as_str(),
        note,
    }
}

/// A known operation's parameters, or why they are invalid.
fn parameters<T: DeserializeOwned>(parameters: Value) -> Result<T, Diagnostic> {
    serde_json::from_value(parameters).map_err(|error| Diagnostic {
        code: ErrorCode::InvalidRequest.as_str(),
        severity: Severity::Error,
        message: format!("Invalid parameters: {error}"),
        path: Some("parameters".into()),
        details: Map::new(),
    })
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct NoParameters {}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateParameters {
    #[serde(default)]
    title: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotParameters {
    document_id: String,
}

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum IdentityMode {
    Recorded,
    Chemical,
}

impl IdentityMode {
    fn model_id(self) -> &'static str {
        match self {
            Self::Recorded => identity::MODEL_ID,
            Self::Chemical => identity::CHEMICAL_MODEL_ID,
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityParameters {
    document_id: String,
    mode: IdentityMode,
    #[serde(default)]
    other_document_id: Option<String>,
}

fn identity_view(
    graph: &MoleculeGraph,
    mode: IdentityMode,
) -> Result<IdentityView, identity::IdentityError> {
    let result = match mode {
        IdentityMode::Recorded => identity::canonicalize(graph),
        IdentityMode::Chemical => identity::canonicalize_chemical(graph),
    }?;
    Ok(IdentityView {
        code: result.code,
        numbering: result
            .numbering
            .into_iter()
            .map(|id| id.to_string())
            .collect(),
        symmetry: result
            .symmetry
            .into_iter()
            .map(|orbit| orbit.into_iter().map(|id| id.to_string()).collect())
            .collect(),
        folded_hydrogens: result
            .folded_hydrogens
            .into_iter()
            .map(|(hydrogen, attached_to)| FoldedHydrogen {
                hydrogen: hydrogen.to_string(),
                attached_to: attached_to.to_string(),
            })
            .collect(),
    })
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PropertiesParameters {
    document_id: String,
    scope: PropertiesScope,
    property: PropertyKind,
    #[serde(default)]
    selection: Option<SelectionDto>,
    #[serde(default)]
    data_registration_id: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TransactionParameters {
    document_id: String,
    expected_revision: u64,
    commands: Vec<CommandDto>,
    #[serde(default)]
    selection_before: Option<SelectionDto>,
    #[serde(default)]
    selection_after: Option<SelectionDto>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RevisionParameters {
    document_id: String,
    expected_revision: u64,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AromaticityConversionParameters {
    document_id: String,
    expected_revision: u64,
    to: AromaticityConversion,
    #[serde(default)]
    selection_before: Option<SelectionDto>,
    #[serde(default)]
    selection_after: Option<SelectionDto>,
    /// The time limit this request asks for, in milliseconds: any value up to the server's maximum
    /// (so it may be above the server's default); above the maximum it is refused.
    #[serde(default)]
    time_limit_ms: Option<u64>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CloseParameters {
    document_id: String,
    #[serde(default)]
    discard_unsaved: bool,
    #[serde(default)]
    expected_revision: Option<u64>,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct CtImportParameters {
    document_id: String,
    expected_revision: u64,
    format: CtFormat,
    source: String,
    /// One-based; every record is validated even when it is not selected.
    record: usize,
    #[serde(default)]
    preview: bool,
    #[serde(default)]
    preview_digest: Option<String>,
    #[serde(default)]
    accept_loss: bool,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CtExportRecord {
    document_id: String,
    #[serde(default)]
    fields: Vec<CtField>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CtExportParameters {
    format: CtFormat,
    records: Vec<CtExportRecord>,
    /// The connection-table dialect to write. Omitted means `v2000`, as before the V3000
    /// profile existed.
    #[serde(default)]
    ctab: CtabChoice,
    /// The `loss_digest` of an earlier refusal, accepting the losses that need consent.
    #[serde(default)]
    accepted_losses: Option<String>,
}

/// The `ctab` request key of a MOL or SDF export.
#[derive(Clone, Copy, Debug, Default, serde::Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum CtabChoice {
    #[default]
    V2000,
    V3000,
    /// V3000 when a drawing needs it (more than 999 atoms or bonds, an aromatic bond, an enhanced stereo group),
    /// otherwise V2000.
    Auto,
}

/// Resolves the `mol-v2000` and `mol-v3000` format ids to `mol`. For an export the id sets
/// `ctab`; an explicit `ctab` that names another dialect (or `auto`) is `format_ctab_conflict`.
/// For an import the dialect is returned, and the selected record must be in it. Any other
/// `format` value is left alone.
fn ct_format_alias(request: &mut Value, export: bool) -> Result<Option<CtTab>, Diagnostic> {
    let Some(map) = request.as_object_mut() else {
        return Ok(None);
    };
    let (tab, name) = match map.get("format").and_then(Value::as_str) {
        Some("mol-v2000") => (CtTab::V2000, "v2000"),
        Some("mol-v3000") => (CtTab::V3000, "v3000"),
        _ => return Ok(None),
    };
    map.insert("format".into(), "mol".into());
    if export {
        // A `ctab` that is not one of the three values is a parameter error, as without an
        // alias; only a valid, differing value is a conflict.
        let chosen = match map.get("ctab") {
            None => None,
            Some(value) => Some(parameters::<CtabChoice>(value.clone())?),
        };
        match chosen {
            None => {
                map.insert("ctab".into(), name.into());
            }
            Some(choice)
                if (choice == CtabChoice::V3000) == (tab == CtTab::V3000)
                    && choice != CtabChoice::Auto => {}
            Some(_) => {
                return Err(ct_failure(CtError {
                    code: "format_ctab_conflict",
                    message: format!(
                        "The format id names {name}; `ctab` must be {name} or left out."
                    ),
                    record: 1,
                    line: None,
                }));
            }
        }
    }
    Ok(Some(tab))
}

fn ct_failure(error: CtError) -> Diagnostic {
    let mut details = Map::new();
    details.insert("record".into(), error.record.into());
    if let Some(line) = error.line {
        details.insert("line".into(), line.into());
    }
    Diagnostic {
        code: error.code,
        severity: Severity::Error,
        message: error.message,
        path: None,
        details,
    }
}

fn indexed_failure(error: &ApplicationError, record: usize) -> Diagnostic {
    let mut diagnostic = failure(error);
    diagnostic.details.insert("record".into(), record.into());
    diagnostic
}

fn ct_mapping(document: &DocumentView, atoms: usize, bonds: usize) -> CtMapping {
    CtMapping {
        atoms: document
            .atoms
            .iter()
            .skip(document.atoms.len().saturating_sub(atoms))
            .map(|a| a.id.clone())
            .collect(),
        bonds: document
            .bonds
            .iter()
            .skip(document.bonds.len().saturating_sub(bonds))
            .map(|b| b.id.clone())
            .collect(),
    }
}

/// The same failure as the v1 error envelope reports, as a diagnostic.
fn failure(error: &ApplicationError) -> Diagnostic {
    let mut details = Map::new();
    if let Some(revision) = error.current_revision {
        details.insert("current_revision".into(), revision.into());
    }
    if let Some(index) = error.command_index {
        details.insert("command_index".into(), index.into());
    }
    if let Some(reason) = error.reason_code() {
        details.insert("reason".into(), reason.into());
    }
    if let Some(limit_ms) = error.limit_ms {
        details.insert("limit_ms".into(), limit_ms.into());
    }
    Diagnostic {
        code: error.code.as_str(),
        severity: Severity::Error,
        message: error.message.clone(),
        path: error
            .command_index
            .map(|index| format!("parameters.commands[{index}]")),
        details,
    }
}

fn diagnostic(
    code: &'static str,
    message: impl Into<String>,
    details: Map<String, Value>,
) -> Diagnostic {
    Diagnostic {
        code,
        severity: Severity::Error,
        message: message.into(),
        path: None,
        details,
    }
}

/// A request that has been read as one of a known operation.
struct Known {
    request_id: String,
    operation: OperationId,
    profile: CompatibilityProfile,
}

impl Known {
    fn reply(
        &self,
        outcome: Outcome,
        document_revision: Option<u64>,
        result: Option<OperationResult>,
        diagnostics: Vec<Diagnostic>,
    ) -> OperationResponse {
        OperationResponse {
            envelope: Envelope::OperationResult,
            contract: OPERATION_CONTRACT,
            request_id: Some(self.request_id.clone()),
            operation: Some(self.operation.as_str().into()),
            profile: Some(self.profile.as_str()),
            outcome,
            document_revision,
            result,
            model_id: None,
            diagnostics,
            loss_report: Vec::new(),
        }
    }

    fn unsupported(&self, diagnostic: Diagnostic) -> OperationResponse {
        self.reply(Outcome::Unsupported, None, None, vec![diagnostic])
    }

    /// The reply to an operation the catalog does not mark executable: unsupported, with its
    /// execution state and follow-up, and no result.
    fn not_running(&self) -> OperationResponse {
        let (entry, _) = entry(self.operation);
        let mut details = Map::new();
        details.insert("execution".into(), entry.execution.as_str().into());
        if let Some(followup) = entry.followup {
            details.insert("followup".into(), followup.into());
        }
        let code = if self.operation == OperationId::PredictionEvaluate {
            "model_unavailable"
        } else {
            "operation_not_implemented"
        };
        self.unsupported(diagnostic(
            code,
            format!(
                "{} is not implemented. {}",
                self.operation.as_str(),
                entry.note
            ),
            details,
        ))
    }

    fn indexed_error(&self, error: &ApplicationError, record: usize) -> OperationResponse {
        self.reply(
            Outcome::Error,
            error.current_revision,
            None,
            vec![indexed_failure(error, record)],
        )
    }

    fn invalid(&self, diagnostic: Diagnostic) -> OperationResponse {
        self.reply(Outcome::Error, None, None, vec![diagnostic])
    }

    fn document(&self, outcome: Result<DocumentResponse, ApplicationError>) -> OperationResponse {
        match outcome {
            Ok(response) => {
                let revision = response.document.revision;
                // Stereo the edit cleared is a loss: `partial`, with the same entries as
                // `result.notes`. Every other document reply has none and stays `ok`.
                let losses = response.notes.clone();
                converted(
                    self,
                    Some(revision),
                    OperationResult::Document(Box::new(response.into())),
                    losses,
                )
            }
            Err(error) => self.reply(
                Outcome::Error,
                error.current_revision,
                None,
                vec![failure(&error)],
            ),
        }
    }
}

fn identity_failure(
    known: &Known,
    revision: u64,
    mode: IdentityMode,
    side: &'static str,
    snapshot: DocumentResponse,
    error: identity::IdentityError,
) -> OperationResponse {
    let mut details = Map::new();
    details.insert("side".into(), side.into());
    details.insert("reason".into(), error.code().into());
    details.insert(
        "snapshot".into(),
        serde_json::to_value(DocumentResult::from(snapshot))
            .expect("validated document snapshots serialize to JSON"),
    );
    let (outcome, code) = match error {
        identity::IdentityError::Unsupported(_) => (Outcome::Unsupported, "identity_unsupported"),
        identity::IdentityError::AtomLimit | identity::IdentityError::SearchLimit => {
            (Outcome::Error, "identity_limit")
        }
    };
    let mut response = known.reply(
        outcome,
        Some(revision),
        None,
        vec![diagnostic(
            code,
            "Graph identity was not determined under this model.",
            details,
        )],
    );
    response.model_id = Some(mode.model_id().into());
    response
}

/// The loss a comparison reports for each side that holds stereo it did not read. `path` names
/// the side in the reply.
fn stereo_not_read(path: &str) -> Loss {
    Loss {
        consent: LossConsent::NotRequired,
        feature: "stereo".into(),
        source_path: path.into(),
        disposition: LossDisposition::Unknown,
        reason: "The drawing holds stereo marks or stated configurations that this comparison does not read: mirror images and cis/trans forms are not told apart.".into(),
        semantic_impact: SemanticImpact::Stereo,
    }
}

fn converted(
    known: &Known,
    revision: Option<u64>,
    result: OperationResult,
    losses: Vec<Loss>,
) -> OperationResponse {
    let outcome = if losses.is_empty() {
        Outcome::Ok
    } else {
        Outcome::Partial
    };
    let mut response = known.reply(outcome, revision, Some(result), Vec::new());
    response.loss_report = losses;
    response
}

fn protocol_error(
    request_id: Option<String>,
    operation: Option<String>,
    outcome: Outcome,
    diagnostic: Diagnostic,
) -> OperationResponse {
    OperationResponse {
        envelope: Envelope::ProtocolError,
        contract: OPERATION_CONTRACT,
        request_id,
        operation,
        profile: None,
        outcome,
        document_revision: None,
        result: None,
        model_id: None,
        diagnostics: vec![diagnostic],
        loss_report: Vec::new(),
    }
}

fn malformed(
    request_id: Option<String>,
    operation: Option<String>,
    message: impl Into<String>,
) -> OperationResponse {
    protocol_error(
        request_id,
        operation,
        Outcome::Error,
        diagnostic("malformed_request", message, Map::new()),
    )
}

fn property_failure(error: PropertiesError) -> (bool, Diagnostic) {
    use chemifree_domain::mass::{FormulaError, MassError};
    let mut details = Map::new();
    let (unsupported, code, message) = match error {
        PropertiesError::Formula(FormulaError::NoAtoms) => {
            (false, "empty_scope", "The chosen scope contains no atoms.")
        }
        PropertiesError::Formula(
            FormulaError::MissingSelection | FormulaError::InvalidSelection,
        ) => (
            false,
            "invalid_selection",
            "The selection is missing or contains unknown identifiers.",
        ),
        PropertiesError::Formula(FormulaError::UnknownAtom(atom)) => {
            details.insert("atom".into(), atom.to_string().into());
            (
                false,
                "invalid_selection",
                "The selection names an unknown atom.",
            )
        }
        PropertiesError::Formula(FormulaError::QueryAtom(atom)) => {
            details.insert("atom".into(), atom.to_string().into());
            (
                true,
                "query_atom",
                "A query atom does not determine an element for a formula.",
            )
        }
        PropertiesError::Formula(FormulaError::UnknownHydrogens(atom)) => {
            details.insert("atom".into(), atom.to_string().into());
            (
                true,
                "hydrogens_unavailable",
                "The hydrogen count is not determined by the selected valence model.",
            )
        }
        PropertiesError::Formula(FormulaError::UnknownAttachment(atom)) => {
            details.insert("atom".into(), atom.to_string().into());
            (
                true,
                "attachment_unknown",
                "An attachment point does not determine a complete formula.",
            )
        }
        PropertiesError::Formula(FormulaError::CountOverflow) => (
            false,
            "resource_limit",
            "Formula counts exceed the supported range.",
        ),
        PropertiesError::DataUnavailable => (
            true,
            "mass_data_unavailable",
            "No selected, compatible external mass dataset is available.",
        ),
        PropertiesError::DataAmbiguous => (
            false,
            "mass_data_ambiguous",
            "Select one compatible mass data registration.",
        ),
        PropertiesError::DataInvalid(reason) => {
            details.insert("reason".into(), reason.into());
            (
                true,
                "mass_data_invalid",
                "The selected data cannot supply this mass profile.",
            )
        }
        PropertiesError::Mass(reason) => {
            let (code, message) = match reason {
                MassError::MissingElement => (
                    "missing_element_data",
                    "The selected dataset lacks an element.",
                ),
                MassError::MissingIsotope => (
                    "isotope_selection_unavailable",
                    "The selected isotope or an unambiguous default is unavailable.",
                ),
                MassError::MissingAverage => (
                    "average_mass_unavailable",
                    "A defensible average mass is unavailable for an element.",
                ),
                MassError::MissingAbundance => (
                    "abundance_unavailable",
                    "The selected dataset cannot determine this isotope distribution.",
                ),
                MassError::InvalidData => (
                    "mass_data_invalid",
                    "The selected mass data contain an unsupported value.",
                ),
                MassError::ResourceLimit => (
                    "resource_limit",
                    "The isotope distribution exceeds its work or peak limit.",
                ),
                MassError::ProbabilityUnderflow => (
                    "probability_underflow",
                    "An isotope probability cannot be represented; no partial distribution is returned.",
                ),
            };
            (reason != MassError::ResourceLimit, code, message)
        }
        PropertiesError::Display => (
            false,
            "numeric_precision_invalid",
            "The mass display precision cannot be applied.",
        ),
    };
    (unsupported, diagnostic(code, message, details))
}

/// MOL/SDF through the shared import flow: one selected record of the source is appended.
struct CtImportFormat<'a> {
    codec: &'a dyn CtCodec,
    p: &'a CtImportParameters,
    /// The dialect a `mol-v2000` or `mol-v3000` format id asks for; the selected record must be
    /// in it.
    required: Option<CtTab>,
}

impl CtImportFormat<'_> {
    fn refusal_at(&self, code: &'static str, message: &str) -> Diagnostic {
        ct_failure(CtError {
            code,
            message: message.into(),
            record: self.p.record,
            line: None,
        })
    }
}

impl ImportFormat for CtImportFormat<'_> {
    type Parsed = (CtResult<Vec<CtRecord>>, usize);

    fn parse(&self) -> Result<Self::Parsed, Diagnostic> {
        let input = self
            .codec
            .read_ctab(self.p.format, self.p.source.as_bytes())
            .map_err(ct_failure)?;
        let selected = self
            .p
            .record
            .checked_sub(1)
            .filter(|index| *index < input.value.len())
            .ok_or_else(|| {
                ct_failure(CtError {
                    code: "invalid_record",
                    message: "Selected record does not exist; records are numbered from one."
                        .into(),
                    record: self.p.record.max(1),
                    line: None,
                })
            })?;
        if let Some(required) = self.required {
            let found = input.value[selected].ctab;
            if found != required {
                return Err(self.refusal_at(
                    "format_ctab_conflict",
                    &format!(
                        "The format id names {required:?} but the selected record is {found:?}; use format \"mol\" to read either."
                    ),
                ));
            }
        }
        Ok((input, selected))
    }

    fn stage(
        &self,
        (input, selected): &Self::Parsed,
        target: &DocumentView,
        _replayed: bool,
    ) -> Result<Staged, Diagnostic> {
        let record = &input.value[*selected];
        let mut losses = input.losses.clone();
        ct_projection::unselected_losses(&input.value, self.p.record, &mut losses);
        let commands = ct_projection::import_commands(record, self.p.record, target, &mut losses)
            .map_err(ct_failure)?;
        Ok(Staged {
            commands,
            losses,
            atoms: record.atoms.len(),
            bonds: record.bonds.len(),
        })
    }

    fn digest(
        &self,
        generation: [u8; 16],
        _parsed: &Self::Parsed,
        snapshot: &DocumentView,
        document: &DocumentView,
        losses: &[Loss],
    ) -> String {
        ct_projection::digest(&(
            generation,
            self.p.format,
            self.p.record,
            &self.p.source,
            snapshot,
            document,
            losses,
        ))
    }

    fn preview(
        &self,
        (input, _): Self::Parsed,
        snapshot: DocumentResponse,
        document: DocumentView,
        preview_digest: String,
    ) -> OperationResult {
        OperationResult::CtPreview(Box::new(CtPreviewResult {
            selected_record: self.p.record,
            records: input.value,
            snapshot: snapshot.into(),
            document,
            preview_digest,
        }))
    }

    /// Every reported loss of a MOL/SDF import needs the caller's acceptance.
    fn needs_accept(&self, losses: &[Loss]) -> bool {
        !losses.is_empty()
    }

    fn refusal(&self, code: &'static str, message: &str, _losses: &[Loss]) -> Diagnostic {
        self.refusal_at(code, message)
    }

    fn document_error(&self, known: &Known, error: &ApplicationError) -> OperationResponse {
        known.indexed_error(error, self.p.record)
    }
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    /// The provider for the request's `format`, decided once for import and export alike. A
    /// format that no provider names goes to MOL/SDF, which refuses it as a parameter error;
    /// with only the rich interchange provider connected, that provider refuses it.
    fn format_backend(&self, parameters: &Value) -> Backend {
        let (backend, provider, connected) = match parameters.get("format").and_then(Value::as_str)
        {
            Some("smiles") => (Backend::Smiles, "SMILES", self.smiles_codec.is_some()),
            Some("cdxml" | "cdx") => (
                Backend::Interchange,
                "rich interchange",
                self.interchange_codec.is_some(),
            ),
            Some("mol" | "sdf" | "mol-v2000" | "mol-v3000") => {
                (Backend::Ct, "MOL/SDF", self.ctfile_codec.is_some())
            }
            _ if self.ctfile_codec.is_none() && self.interchange_codec.is_some() => {
                return Backend::Interchange;
            }
            _ => return Backend::Ct,
        };
        if connected {
            backend
        } else {
            Backend::NotConnected(provider)
        }
    }

    /// The one row of the support table for an operation, with the wording of the format
    /// codecs connected to this application. The catalog reports it and the dispatcher runs
    /// exactly what it marks executable.
    fn row(&self, operation: OperationId) -> Row<'_> {
        if operation == OperationId::ChemInchi && self.inchi_provider.is_some() {
            return Row {
                entry: runs(
                    "chem.inchi",
                    "Read-only bounded Standard InChI/InChIKey generation with checked provider version, build, effective options, exact snapshot, explicit losses and consent. Unsupported chemistry and AND/OR groups are refused. Stored configurations are used; drawn marks are not substituted.",
                ),
                parts: &[],
                note: None,
            };
        }
        if let Some(text) = &self.format_text {
            let note = match operation {
                OperationId::DocumentImport => Some(text.import_note.as_str()),
                OperationId::DocumentExport => Some(text.export_note.as_str()),
                _ => None,
            };
            if let Some(note) = note {
                return Row {
                    entry: runs(operation.as_str(), ""),
                    parts: &[],
                    note: Some(note),
                };
            }
        }
        let (entry, parts) = entry(operation);
        Row {
            entry,
            parts,
            note: None,
        }
    }

    fn ct_import(
        &self,
        known: &Known,
        p: CtImportParameters,
        required: Option<CtTab>,
    ) -> OperationResponse {
        let Some(codec) = &self.ctfile_codec else {
            return known.not_running();
        };
        let request = ImportRequest {
            document_id: &p.document_id,
            expected_revision: p.expected_revision,
            preview: p.preview,
            preview_digest: p.preview_digest.as_deref(),
            accept_loss: p.accept_loss,
            // The format id's dialect is part of the request: `mol` and `mol-v3000` differ.
            fingerprint: ct_projection::digest(&(&p, required.map(|tab| tab == CtTab::V3000))),
        };
        let format = CtImportFormat {
            codec: codec.as_ref(),
            p: &p,
            required,
        };
        self.run_import(known, &format, request, self.preview_generation)
    }

    fn ct_export(&self, known: &Known, p: CtExportParameters) -> OperationResponse {
        let Some(codec) = &self.ctfile_codec else {
            return known.not_running();
        };
        if p.records.is_empty() {
            return known.invalid(ct_failure(CtError {
                code: "empty_input",
                message: "At least one source document is required.".into(),
                record: 1,
                line: None,
            }));
        }
        if p.records.len() > 128 || (p.format == CtFormat::Mol && p.records.len() != 1) {
            return known.invalid(ct_failure(CtError {
                code: "invalid_count",
                message: "MOL needs one source; SDF supports up to 128.".into(),
                record: if p.format == CtFormat::Mol { 2 } else { 129 },
                line: None,
            }));
        }
        let mut snapshots = Vec::with_capacity(p.records.len());
        for (i, source) in p.records.iter().enumerate() {
            match self.documents.analysis_snapshot(&source.document_id) {
                Ok(snapshot) => snapshots.push(snapshot),
                Err(error) => return known.indexed_error(&error, i + 1),
            }
        }
        let v3000 = match p.ctab {
            CtabChoice::V2000 => false,
            CtabChoice::V3000 => true,
            CtabChoice::Auto => snapshots
                .iter()
                .any(|(response, _)| ct_projection::needs_v3000(&response.document)),
        };
        let (target, tab, name) = if v3000 {
            (ct_projection::Target::V3000, CtTab::V3000, "v3000")
        } else {
            (ct_projection::Target::V2000, CtTab::V2000, "v2000")
        };
        let mut records = Vec::with_capacity(p.records.len());
        let mut mappings = Vec::with_capacity(p.records.len());
        let mut losses = Vec::new();
        for (i, (source, (snapshot, domain))) in p.records.into_iter().zip(snapshots).enumerate() {
            let index = i + 1;
            let record = match ct_projection::export_record(
                &snapshot.document,
                index,
                source.fields,
                target,
                &mut losses,
            ) {
                Ok(record) => record,
                Err(error) => return known.invalid(ct_failure(error)),
            };
            if target == ct_projection::Target::V3000 {
                let undrawn = ct_projection::undrawn_group_members(&domain, &snapshot.document);
                if !undrawn.is_empty() {
                    return known.invalid(ct_failure(CtError {
                        code: "v3000_group_member_not_drawn",
                        message: format!(
                            "Group members {} have no wedge or hash that derives their stored configuration, so the collection would name atoms this reader refuses; draw the marks or remove the members from the group. Nothing was written.",
                            undrawn.join(", ")
                        ),
                        record: index,
                        line: None,
                    }));
                }
            }
            losses.extend(ct_projection::molfile_stereo_losses(
                &domain,
                &snapshot.document,
                index,
                target,
            ));
            mappings.push(CtExportMapping {
                atoms: snapshot
                    .document
                    .atoms
                    .iter()
                    .map(|atom| atom.id.clone())
                    .collect(),
                bonds: snapshot
                    .document
                    .bonds
                    .iter()
                    .map(|bond| bond.id.clone())
                    .collect(),
                snapshot: snapshot.into(),
            });
            records.push(record);
        }
        match codec.write_ctab(p.format, tab, &records) {
            Ok(output) => {
                losses.extend(output.losses);
                if let Some(refusal) = interchange::export_consent_refusal(
                    &losses,
                    output.value.as_bytes(),
                    p.accepted_losses.as_deref(),
                ) {
                    return known.invalid(refusal);
                }
                converted(
                    known,
                    None,
                    OperationResult::CtExport(CtExportResult {
                        content: output.value,
                        ctab: name,
                        records: mappings,
                    }),
                    losses,
                )
            }
            Err(error) => known.invalid(ct_failure(error)),
        }
    }

    /// What this service supports of the operation contract, per operation and sub-operation.
    pub fn operation_catalog(&self) -> OperationCatalog {
        OperationCatalog {
            implementation: "chemifree",
            version: env!("CARGO_PKG_VERSION"),
            contracts: vec![OPERATION_CONTRACT],
            profiles: CompatibilityProfile::ALL
                .into_iter()
                .map(profile_support)
                .collect(),
            operations: OperationId::ALL
                .into_iter()
                .map(|operation| {
                    let row = self.row(operation);
                    let mut support = if operation == OperationId::ChemCip {
                        let note = cip::catalog_note(self.cip_labeler.as_ref());
                        row.entry.view(Some(&note), row.parts)
                    } else {
                        row.entry.view(row.note, row.parts)
                    };
                    if operation == OperationId::ChemInchi {
                        if let Some(provider) = &self.inchi_provider {
                            let identity = provider.identity();
                            support.note = format!("{} Registered provider: {}; version: {}; build: {}; effective options: {:?}. {}", support.note, identity.name, identity.version, identity.build, identity.options, crate::inchi::STANDARD_SEMANTICS).into();
                        } else if let Some(error) = &self.inchi_provider_error {
                            support.note = format!("{} Registration failure {}: {}", support.note, error.code, error.message).into();
                        }
                    }
                    if operation == OperationId::ChemProperties {
                        for part in &mut support.sub_operations {
                            let available = match part.id {
                                "mass" => self.property_data_readiness.mass,
                                "isotope_distribution" => self.property_data_readiness.distribution,
                                _ => continue,
                            };
                            if available {
                                part.execution = Execution::Executable.as_str();
                            }
                        }
                    }
                    support
                })
                .collect(),
        }
    }

    /// Runs one request of the operation contract, given as UTF-8 JSON, and returns its
    /// reply. Every request gets a reply; nothing here panics on input.
    pub fn execute(&self, request: &[u8]) -> OperationResponse {
        self.execute_with(request, &NEVER)
    }

    /// [`Self::execute`] whose computations also stop, with the `cancelled` diagnostic, when
    /// `cancel` says so. A stopped request changes nothing.
    pub fn execute_with(&self, request: &[u8], cancel: &dyn Interrupt) -> OperationResponse {
        // A repeated key is refused rather than resolved, so a request means the same thing to
        // every reader of it.
        let value = match json::parse_strict(request) {
            Ok(value) => value,
            Err(error) => {
                return malformed(
                    None,
                    None,
                    format!("The request is not UTF-8 JSON without repeated keys: {error}"),
                );
            }
        };
        let Value::Object(mut fields) = value else {
            return malformed(None, None, "The request is not a JSON object.");
        };
        let request_id = fields
            .get("request_id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let operation_name = fields
            .get("operation")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let echo = || (request_id.clone(), operation_name.clone());

        match fields.get("contract").and_then(Value::as_u64) {
            Some(version) if version == u64::from(OPERATION_CONTRACT) => {}
            Some(version) => {
                let (id, op) = echo();
                let mut details = Map::new();
                details.insert("supported".into(), Value::from(vec![OPERATION_CONTRACT]));
                return protocol_error(
                    id,
                    op,
                    Outcome::Unsupported,
                    diagnostic(
                        "unsupported_contract",
                        format!("Operation contract {version} is not supported."),
                        details,
                    ),
                );
            }
            None => {
                let (id, op) = echo();
                return malformed(
                    id,
                    op,
                    "contract must be the operation contract version, a non-negative integer.",
                );
            }
        }
        if let Some(unknown) = fields
            .keys()
            .find(|key| !ENVELOPE_FIELDS.contains(&key.as_str()))
        {
            let (id, op) = echo();
            return malformed(id, op, format!("Unknown request field `{unknown}`."));
        }
        let Some(request_id) = request_id.clone() else {
            return malformed(None, operation_name, "request_id must be a string.");
        };
        if let Err(error) = validate_request_id(Some(&request_id)) {
            return protocol_error(None, operation_name, Outcome::Error, failure(&error));
        }
        let Some(name) = operation_name.clone() else {
            return malformed(Some(request_id), None, "operation must be a string.");
        };
        let Some(operation) = OperationId::parse(&name) else {
            return protocol_error(
                Some(request_id),
                Some(name.clone()),
                Outcome::Unsupported,
                diagnostic(
                    "unknown_operation",
                    format!("There is no operation `{name}`."),
                    Map::new(),
                ),
            );
        };
        let profile = match fields.get("profile").and_then(Value::as_str) {
            Some(profile) => match CompatibilityProfile::parse(profile) {
                Some(profile) => profile,
                None => {
                    return protocol_error(
                        Some(request_id),
                        Some(name),
                        Outcome::Error,
                        diagnostic(
                            "unknown_profile",
                            format!("There is no compatibility profile `{profile}`."),
                            Map::new(),
                        ),
                    );
                }
            },
            None => {
                return malformed(
                    Some(request_id),
                    Some(name),
                    "profile must be a string naming a compatibility profile.",
                );
            }
        };
        let parameters_value = match fields.remove("parameters") {
            Some(value @ Value::Object(_)) => value,
            _ => {
                return malformed(
                    Some(request_id),
                    Some(name),
                    "parameters must be an object.",
                );
            }
        };
        let known = Known {
            request_id,
            operation,
            profile,
        };
        self.dispatch(&known, parameters_value, cancel)
    }

    fn chemical_properties(
        &self,
        known: &Known,
        parameters: PropertiesParameters,
    ) -> OperationResponse {
        if (parameters.scope == PropertiesScope::Selection) != parameters.selection.is_some() {
            return known.invalid(diagnostic(
                "invalid_selection",
                "Selection scope requires an explicit selection; other scopes do not accept one.",
                Map::new(),
            ));
        }
        if parameters.property == PropertyKind::Formula && parameters.data_registration_id.is_some()
        {
            return known.invalid(diagnostic(
                "invalid_request",
                "Formula-only requests do not select mass data.",
                Map::new(),
            ));
        }
        let selection = match crate::contract::to_selection(&TransactionSelections {
            before: parameters.selection.clone(),
            after: None,
        }) {
            Ok(value) => value.before,
            Err(error) => return known.invalid(failure(&error)),
        };
        let (snapshot, document) = match self.documents.analysis_snapshot(&parameters.document_id) {
            Ok(snapshot) => snapshot,
            Err(error) => return known.invalid(failure(&error)),
        };
        let precision = if parameters.property == PropertyKind::Formula {
            0
        } else {
            match self.preferences.current() {
                Ok(preferences) => preferences.mass_precision.decimals,
                Err(error) => return known.invalid(failure(&error)),
            }
        };
        let data = if parameters.property == PropertyKind::Formula {
            None
        } else {
            match properties::select_data(
                &self.data_registry,
                parameters.data_registration_id.as_deref(),
            ) {
                Ok(data) => Some(data),
                Err(error) => {
                    let (unsupported, diagnostic) = property_failure(error);
                    return if unsupported {
                        known.unsupported(diagnostic)
                    } else {
                        known.invalid(diagnostic)
                    };
                }
            }
        };
        let assessments = chemifree_domain::chemistry::inspect_valence(&document);
        let inferred = assessments
            .iter()
            .filter(|state| state.finding == chemifree_domain::chemistry::Finding::Valid)
            .filter_map(|state| state.hydrogen_count.map(|count| (state.atom, count)))
            .collect();
        let components = match properties::calculate(
            &document,
            parameters.scope.domain(),
            selection.as_ref(),
            &inferred,
            parameters.property,
            data.as_ref().map(|(_, table)| table),
            precision,
        ) {
            Ok(result) => result,
            Err(error) => {
                let (unsupported, diagnostic) = property_failure(error);
                return if unsupported {
                    known.unsupported(diagnostic)
                } else {
                    known.invalid(diagnostic)
                };
            }
        };
        let mut losses = Vec::new();
        let included: std::collections::BTreeSet<_> = components
            .iter()
            .flat_map(|component| component.atoms.iter().map(String::as_str))
            .collect();
        let valence_warnings = assessments
            .iter()
            .filter(|state| {
                state.finding != chemifree_domain::chemistry::Finding::Valid
                    && included.contains(state.atom.to_string().as_str())
            })
            .map(|state| PropertyValenceWarning {
                atom: state.atom.to_string(),
                status: state.finding.as_str(),
                reason: state.reason,
            })
            .collect();
        for (index, component) in components.iter().enumerate() {
            if component.mass.as_ref().is_some_and(|mass| {
                !mass.average_omitted_abundances.is_empty()
                    || !mass.exact_omitted_abundances.is_empty()
            }) || !component.omitted_abundances.is_empty()
            {
                losses.push(Loss { feature: "unreported_isotope_abundances".into(), source_path: format!("components[{index}]"), disposition: LossDisposition::Unknown, reason: "The calculation is conditional on reported isotope fractions; missing fractions are listed in the result.".into(), semantic_impact: SemanticImpact::Chemistry, consent: LossConsent::NotRequired });
            }
            if component
                .mass
                .as_ref()
                .is_some_and(|mass| !mass.exact_natural_weight_fallbacks.is_empty())
            {
                losses.push(Loss { feature: "exact_natural_weight_fallback".into(), source_path: format!("components[{index}].mass"), disposition: LossDisposition::Normalized, reason: "The profile used a natural weight because no isotope rows were available; affected elements are listed in the result.".into(), semantic_impact: SemanticImpact::Chemistry, consent: LossConsent::NotRequired });
            }
        }
        let revision = snapshot.document.revision;
        let result = PropertiesResult {
            snapshot: snapshot.into(),
            scope: parameters.scope.as_str(),
            selection: parameters.selection,
            property: parameters.property.as_str(),
            valence_model: chemifree_domain::chemistry::VALENCE_MODEL,
            arithmetic: if parameters.property == PropertyKind::Formula {
                "integer-composition"
            } else {
                "binary64-sequential-species-order; fixed display ties-to-even"
            },
            mass_unit: data.as_ref().map(|_| "u"),
            display_decimals: precision,
            data: data.map(|(view, _)| view),
            valence_warnings,
            components,
        };
        let mut response = known.reply(
            if losses.is_empty() {
                Outcome::Ok
            } else {
                Outcome::Partial
            },
            Some(revision),
            Some(OperationResult::Properties(Box::new(result))),
            Vec::new(),
        );
        response.model_id = Some("chemifree-properties-1".into());
        response.loss_report = losses;
        response
    }

    fn dispatch(
        &self,
        known: &Known,
        parameters_value: Value,
        cancel: &dyn Interrupt,
    ) -> OperationResponse {
        use OperationId as Op;
        if known.profile == CompatibilityProfile::LegacyObserved
            && known.operation != Op::CapabilitiesInspect
        {
            let mut details = Map::new();
            details.insert("profile".into(), known.profile.as_str().into());
            return known.unsupported(diagnostic(
                "profile_unavailable",
                "No operation runs under the legacy-observed profile; use preserve-semantics.",
                details,
            ));
        }
        if known.operation != Op::ChemInchi
            && self.row(known.operation).entry.execution != Execution::Executable
        {
            return known.not_running();
        }
        let request_id = Some(known.request_id.as_str());
        match known.operation {
            Op::CapabilitiesInspect => match parameters::<NoParameters>(parameters_value) {
                Ok(NoParameters {}) => known.reply(
                    Outcome::Ok,
                    None,
                    Some(OperationResult::Catalog(self.operation_catalog())),
                    Vec::new(),
                ),
                Err(invalid) => known.invalid(invalid),
            },
            Op::DocumentCreate => match parameters::<CreateParameters>(parameters_value) {
                Ok(p) => known.document(self.create_document(request_id, p.title.as_deref())),
                Err(invalid) => known.invalid(invalid),
            },
            Op::DocumentSnapshot => match parameters::<SnapshotParameters>(parameters_value) {
                Ok(p) => known.document(self.document(&p.document_id)),
                Err(invalid) => known.invalid(invalid),
            },
            Op::DocumentSelectionScope => match parameters::<SelectionScopeParameters>(parameters_value) {
                Ok(parameters) => self.resolve_selection_scope(known, parameters),
                Err(error) => known.invalid(error),
            },
            Op::DocumentArrangementPlan => match parameters::<ArrangementParameters>(parameters_value) {
                Ok(parameters) => self.resolve_arrangement_plan(known, parameters),
                Err(error) => known.invalid(error),
            },
            Op::DocumentTransformPlan => match parameters::<TransformParameters>(parameters_value) {
                Ok(parameters) => self.resolve_transform_plan(known, parameters),
                Err(error) => known.invalid(error),
            },
            Op::ChemValidate => match parameters::<SnapshotParameters>(parameters_value) {
                Ok(p) => match self.documents.inspect_valence(&p.document_id) {
                    Ok((snapshot, assessed)) => {
                        let revision = snapshot.document.revision;
                        let mut warnings = Vec::new();
                        let atoms = assessed.into_iter().map(|item| {
                            let atom_id = item.atom.to_string();
                            if let Some(reason) = item.reason {
                                let mut details = Map::new();
                                details.insert("atom_id".into(), atom_id.clone().into());
                                details.insert("reason".into(), reason.into());
                                warnings.push(Diagnostic { code: "valence_diagnostic", severity: Severity::Warning,
                                    message: "The atom is invalid or outside this valence model.".into(), path: None, details });
                            }
                            ValenceAtom { atom_id, bond_order_sum: item.bond_order_sum,
                                hydrogens: item.hydrogen_count, status: item.finding.as_str(), reason: item.reason }
                        }).collect();
                        let mut reply = known.reply(Outcome::Ok, Some(revision),
                            Some(OperationResult::Valence(Box::new(ValenceResult { snapshot: snapshot.into(), atoms }))), warnings);
                        reply.model_id = Some(chemifree_domain::chemistry::VALENCE_MODEL.into());
                        reply
                    }
                    Err(error) => known.reply(Outcome::Error, error.current_revision, None, vec![failure(&error)]),
                },
                Err(invalid) => known.invalid(invalid),
            },
            Op::ChemAromaticity => match parameters::<SnapshotParameters>(parameters_value) {
                Ok(p) => match self.documents.graph_snapshot(&p.document_id) {
                    Ok((snapshot, graph)) => {
                        let revision = snapshot.document.revision;
                        let interpretation = chemifree_domain::aromaticity::perceive(&graph);
                        let systems = interpretation.systems.into_iter().map(|system| {
                            let classification = match system.classification {
                                chemifree_domain::aromaticity::Classification::Aromatic => "aromatic",
                                chemifree_domain::aromaticity::Classification::NonAromatic => "non_aromatic",
                                chemifree_domain::aromaticity::Classification::Unsupported => "unsupported",
                            };
                            AromaticitySystem {
                                atoms: system.atoms.into_iter().map(|id| id.to_string()).collect(),
                                bonds: system.bonds.into_iter().map(|id| id.to_string()).collect(),
                                cycle_rank: system.cycle_rank,
                                classification,
                                electrons: system.electrons,
                                reason: system.reason,
                            }
                        }).collect();
                        let mut reply = known.reply(Outcome::Ok, Some(revision),
                            Some(OperationResult::Aromaticity(Box::new(AromaticityResult { snapshot: snapshot.into(), systems }))), Vec::new());
                        reply.model_id = Some(chemifree_domain::aromaticity::MODEL_ID.into());
                        reply
                    }
                    Err(error) => known.reply(Outcome::Error, error.current_revision, None, vec![failure(&error)]),
                },
                Err(invalid) => known.invalid(invalid),
            },
            Op::ChemAromaticityConvert => match parameters::<AromaticityConversionParameters>(parameters_value) {
                Ok(p) => {
                    let limit_ms = match self.compute_limits.resolve(p.time_limit_ms) {
                        Ok(limit_ms) => limit_ms,
                        Err(maximum) => {
                            let mut details = Map::new();
                            details.insert("maximum_ms".into(), maximum.into());
                            let mut reply = known.invalid(Diagnostic {
                                code: ErrorCode::InvalidRequest.as_str(),
                                severity: Severity::Error,
                                message: format!("time_limit_ms is above the maximum of {maximum} ms."),
                                path: Some("parameters.time_limit_ms".into()),
                                details,
                            });
                            reply.model_id = Some(chemifree_domain::aromaticity::MODEL_ID.into());
                            return reply;
                        }
                    };
                    // Limits are capped far below what an `Instant` can add; a deadline that could not be
                    // formed would still stop the computation at once rather than never.
                    let now = Instant::now();
                    let deadline = Some(now.checked_add(Duration::from_millis(limit_ms)).unwrap_or(now));
                    let budget = Budget::new(cancel, deadline);
                    let outcome = self.documents.convert_aromaticity(&p.document_id, request_id, p.expected_revision, p.to,
                        TransactionSelections { before: p.selection_before, after: p.selection_after },
                        Compute { interrupt: &budget, limit_ms, requested_ms: p.time_limit_ms });
                    let mut reply = known.document(outcome);
                    reply.model_id = Some(chemifree_domain::aromaticity::MODEL_ID.into());
                    reply
                }
                Err(invalid) => known.invalid(invalid),
            },
            Op::ChemIdentity => match parameters::<IdentityParameters>(parameters_value) {
                Ok(p) => {
                    let ((snapshot, graph), other) = match self.documents.graph_pair_snapshot(
                        &p.document_id,
                        p.other_document_id.as_deref(),
                    ) {
                        Ok(pair) => pair,
                        Err(error) => return known.reply(Outcome::Error, error.current_revision, None, vec![failure(&error)]),
                    };
                    let revision = snapshot.document.revision;
                    let identity = match identity_view(&graph, p.mode) {
                        Ok(value) => value,
                        Err(error) => return identity_failure(known, revision, p.mode, "source", snapshot, error),
                    };
                    let other_graph_stereo = other.as_ref().map(|(_, other_graph)| other_graph.has_stereo());
                    let comparison = if let Some((other_snapshot, other_graph)) = other {
                        let other_identity = match identity_view(&other_graph, p.mode) {
                            Ok(value) => value,
                            Err(error) => return identity_failure(known, revision, p.mode, "target", other_snapshot, error),
                        };
                        Some(IdentityComparison {
                            equivalent: identity.code == other_identity.code,
                            snapshot: other_snapshot.into(),
                            identity: other_identity,
                        })
                    } else {
                        None
                    };
                    let stereo = IdentityStereo {
                        compared: false,
                        source_present: graph.has_stereo(),
                        other_present: other_graph_stereo,
                    };
                    let mut losses = Vec::new();
                    if stereo.source_present {
                        losses.push(stereo_not_read("snapshot"));
                    }
                    if stereo.other_present == Some(true) {
                        losses.push(stereo_not_read("comparison.snapshot"));
                    }
                    let mut response = converted(
                        known,
                        Some(revision),
                        OperationResult::Identity(Box::new(IdentityResult {
                            snapshot: snapshot.into(),
                            identity,
                            comparison,
                            stereo,
                        })),
                        losses,
                    );
                    response.model_id = Some(p.mode.model_id().into());
                    response
                }
                Err(invalid) => known.invalid(invalid),
            },
            Op::ChemSubstructure => match parameters::<SubstructureParameters>(parameters_value) {
                Ok(p) => self.substructure(known, p),
                Err(invalid) => known.invalid(invalid),
            },
            Op::ChemStereo => match parameters::<StereoParameters>(parameters_value) {
                Ok(p) => self.stereo(known, p),
                Err(invalid) => known.invalid(invalid),
            },
            Op::ChemInchi => match parameters::<InchiParameters>(parameters_value) {
                Ok(parameters) => self.generate_inchi(known, parameters),
                Err(error) => known.invalid(error),
            },
            Op::ChemCip => match parameters::<CipParameters>(parameters_value) {
                Ok(p) => self.cip(known, p, cancel),
                Err(invalid) => {
                    // Malformed parameters never reach `cip()`; the reply still names the
                    // labeler the operation would have used.
                    let mut reply = known.invalid(invalid);
                    reply.model_id = Some(self.cip_labeler.model_id().into());
                    reply
                }
            },
            Op::DocumentTransaction => {
                match parameters::<TransactionParameters>(parameters_value) {
                    Ok(p) => {
                        let derives = p.commands.iter().any(|command| {
                            matches!(command, CommandDto::SetConfigurationFromDrawing { .. })
                        });
                        let mut reply = known.document(self.apply_transaction_selecting(
                            &p.document_id,
                            request_id,
                            p.expected_revision,
                            p.commands,
                            TransactionSelections { before: p.selection_before, after: p.selection_after },
                        ));
                        if derives {
                            reply.model_id = Some(chemifree_domain::stereo_drawing::MODEL_ID.into());
                        }
                        reply
                    }
                    Err(invalid) => known.invalid(invalid),
                }
            }
            Op::HistoryUndo => match parameters::<RevisionParameters>(parameters_value) {
                Ok(p) => known.document(self.undo(&p.document_id, request_id, p.expected_revision)),
                Err(invalid) => known.invalid(invalid),
            },
            Op::HistoryRedo => match parameters::<RevisionParameters>(parameters_value) {
                Ok(p) => known.document(self.redo(&p.document_id, request_id, p.expected_revision)),
                Err(invalid) => known.invalid(invalid),
            },
            Op::DocumentSave => match parameters::<RevisionParameters>(parameters_value) {
                Ok(p) => known.document(self.save(&p.document_id, request_id, p.expected_revision)),
                Err(invalid) => known.invalid(invalid),
            },
            Op::DocumentClose => match parameters::<CloseParameters>(parameters_value) {
                Ok(p) => match self.close(
                    &p.document_id,
                    request_id,
                    p.discard_unsaved,
                    p.expected_revision,
                ) {
                    Ok(closed) => known.reply(
                        Outcome::Ok,
                        None,
                        Some(OperationResult::Closed(ClosedResult {
                            document_id: closed.id,
                            discarded_changes: closed.discarded_changes,
                        })),
                        Vec::new(),
                    ),
                    Err(error) => known.reply(
                        Outcome::Error,
                        error.current_revision,
                        None,
                        vec![failure(&error)],
                    ),
                },
                Err(invalid) => known.invalid(invalid),
            },
            Op::DocumentImport => match self.format_backend(&parameters_value) {
                Backend::Smiles => match parameters::<SmilesImportParameters>(parameters_value) {
                    Ok(p) => self.smiles_import(known, p),
                    Err(invalid) => known.invalid(invalid),
                },
                Backend::Interchange => self.dispatch_interchange_import(known, parameters_value),
                Backend::Ct => {
                    let mut parameters_value = parameters_value;
                    match ct_format_alias(&mut parameters_value, false) {
                        Ok(required) => match parameters::<CtImportParameters>(parameters_value) {
                            Ok(p) => self.ct_import(known, p, required),
                            Err(invalid) => known.invalid(invalid),
                        },
                        Err(invalid) => known.invalid(invalid),
                    }
                }
                Backend::NotConnected(provider) => known.invalid(not_connected(provider)),
            },
            Op::DocumentExport => match self.format_backend(&parameters_value) {
                Backend::Smiles => match parameters::<SmilesExportParameters>(parameters_value) {
                    Ok(p) => self.smiles_export(known, p),
                    Err(invalid) => known.invalid(invalid),
                },
                Backend::Interchange => self.dispatch_interchange_export(known, parameters_value),
                Backend::Ct => {
                    let mut parameters_value = parameters_value;
                    match ct_format_alias(&mut parameters_value, true) {
                        Ok(_) => match parameters::<CtExportParameters>(parameters_value) {
                            Ok(p) => self.ct_export(known, p),
                            Err(invalid) => known.invalid(invalid),
                        },
                        Err(invalid) => known.invalid(invalid),
                    }
                }
                Backend::NotConnected(provider) => known.invalid(not_connected(provider)),
            },
            Op::ChemProperties => match parameters::<PropertiesParameters>(parameters_value) {
                Ok(p) => self.chemical_properties(known, p),
                Err(invalid) => known.invalid(invalid),
            },
            Op::ChemNameToStructure
            | Op::ChemStructureToName
            | Op::LayoutStructure
            | Op::LayoutReaction
            | Op::RenderScene
            | Op::BiopolymerExpand
            | Op::PredictionEvaluate
            // The catalog marks these not executable, so they were answered above; answering
            // the same here keeps a catalog that drifts from this match from panicking.
            | Op::IntegrationInvoke
            | Op::ScriptEvaluate => known.not_running(),
        }
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::{Draft, DraftRecoveryTarget, DraftStatus, StorageError, TrashEntry};
    use chemifree_domain::document::{Document, DocumentId};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    struct TestCapabilities;
    impl CapabilityCatalog for TestCapabilities {
        fn capabilities(&self) -> Vec<chemifree_domain::Capability> {
            Vec::new()
        }
    }
    #[derive(Default)]
    struct TestRepository(Mutex<BTreeMap<DocumentId, Document>>);
    impl DocumentRepository for TestRepository {
        fn allocate_id(&self) -> Result<DocumentId, StorageError> {
            Ok(DocumentId::parse("fixture-document").unwrap())
        }
        fn list_ids(&self) -> Result<Vec<DocumentId>, StorageError> {
            Ok(self.0.lock().unwrap().keys().cloned().collect())
        }
        fn load(&self, id: &DocumentId) -> Result<Option<Document>, StorageError> {
            Ok(self.0.lock().unwrap().get(id).cloned())
        }
        fn save(&self, document: &Document, _base: Option<u64>) -> Result<(), StorageError> {
            self.0
                .lock()
                .unwrap()
                .insert(document.id().clone(), document.clone());
            Ok(())
        }
        fn draft_status(&self, _id: &DocumentId) -> Result<DraftStatus, StorageError> {
            Ok(DraftStatus::None)
        }
        fn write_draft(&self, _draft: &Draft) -> Result<(), StorageError> {
            unreachable!()
        }
        fn claim_draft(
            &self,
            _id: &DocumentId,
        ) -> Result<Option<Result<Draft, String>>, StorageError> {
            unreachable!()
        }
        fn prepare_draft_for_recovery(
            &self,
            _id: &DocumentId,
            _target: DraftRecoveryTarget,
        ) -> Result<(), StorageError> {
            unreachable!()
        }
        fn release_draft(&self, _id: &DocumentId) {
            unreachable!()
        }
        fn remove_draft(&self, _id: &DocumentId) -> Result<(), StorageError> {
            unreachable!()
        }
        fn saved_at_ms(&self, _id: &DocumentId) -> Result<Option<u64>, StorageError> {
            Ok(None)
        }
        fn trash(&self, _id: &DocumentId, _base: u64) -> Result<(), StorageError> {
            unreachable!()
        }
        fn restore(&self, _id: &DocumentId) -> Result<(), StorageError> {
            unreachable!()
        }
        fn list_trash(&self) -> Result<Vec<TrashEntry>, StorageError> {
            unreachable!()
        }
        fn load_trashed(&self, _id: &DocumentId) -> Result<Option<Document>, StorageError> {
            unreachable!()
        }
    }

    #[test]
    fn catalog_distinguishes_formula_from_data_backed_properties() {
        let states = |app: &Application<TestCapabilities, TestRepository>| {
            let catalog = app.operation_catalog();
            let entry = catalog
                .operations
                .iter()
                .find(|entry| entry.id == "chem.properties")
                .unwrap();
            let execution = |id| {
                entry
                    .sub_operations
                    .iter()
                    .find(|part| part.id == id)
                    .unwrap()
                    .execution
            };
            (
                entry.execution,
                execution("formula"),
                execution("mass"),
                execution("isotope_distribution"),
            )
        };
        let empty = Application::new(TestCapabilities, TestRepository::default());
        assert_eq!(
            states(&empty),
            ("executable", "executable", "requires_data", "requires_data")
        );

        let snapshot = crate::mass_data::tests::decode(&crate::mass_data::tests::fixture());
        let dataset =
            crate::PersonalDataset::fixture(crate::ProfileRecords::Phase4Nist(Box::new(snapshot)));
        let ready = Application::new(TestCapabilities, TestRepository::default())
            .with_data_registry(crate::DataRegistry::fixture(vec![dataset]));
        assert_eq!(
            states(&ready),
            ("executable", "executable", "executable", "executable")
        );

        let mut input = crate::mass_data::tests::synthetic_mass_input();
        input.features.mass_unit = "unknown-unit".into();
        let dataset =
            crate::PersonalDataset::fixture(crate::ProfileRecords::MassInput(Box::new(input)));
        let incompatible = Application::new(TestCapabilities, TestRepository::default())
            .with_data_registry(crate::DataRegistry::fixture(vec![dataset]));
        assert_eq!(
            states(&incompatible),
            ("executable", "executable", "requires_data", "requires_data")
        );

        let mut sparse = crate::mass_data::tests::fixture();
        sparse["isotopes"] = json!([]);
        sparse["isotope_count"] = json!(0);
        let dataset = crate::PersonalDataset::fixture(crate::ProfileRecords::Phase4Nist(Box::new(
            crate::mass_data::tests::decode(&sparse),
        )));
        let unusable = Application::new(TestCapabilities, TestRepository::default())
            .with_data_registry(crate::DataRegistry::fixture(vec![dataset]));
        assert_eq!(
            states(&unusable),
            ("executable", "executable", "requires_data", "requires_data")
        );

        let mut natural_only = crate::mass_data::tests::synthetic_mass_input();
        natural_only.isotopes.clear();
        natural_only.selections.clear();
        let dataset = crate::PersonalDataset::fixture(crate::ProfileRecords::MassInput(Box::new(
            natural_only,
        )));
        let natural_only = Application::new(TestCapabilities, TestRepository::default())
            .with_data_registry(crate::DataRegistry::fixture(vec![dataset]));
        assert_eq!(
            states(&natural_only),
            ("executable", "executable", "executable", "requires_data")
        );
        // P14 permits an exactly positive decimal whose binary64 witness underflows to 0.
        // Such a table cannot supply a valid numeric mass merely because a row exists.
        let mut underflow = crate::mass_data::tests::synthetic_mass_input();
        underflow.isotopes.clear();
        underflow.selections.clear();
        for element in &mut underflow.elements {
            element.natural_atomic_weight.binary64_bits = 0.0_f64.to_bits();
        }
        let dataset =
            crate::PersonalDataset::fixture(crate::ProfileRecords::MassInput(Box::new(underflow)));
        let underflow = Application::new(TestCapabilities, TestRepository::default())
            .with_data_registry(crate::DataRegistry::fixture(vec![dataset]));
        assert_eq!(
            states(&underflow),
            ("executable", "executable", "requires_data", "requires_data")
        );

        let out_of_range = |abundance: f64| {
            let mut input = crate::mass_data::tests::synthetic_mass_input();
            input.elements.truncate(1);
            input.isotopes.truncate(1);
            input.isotopes[0].mass_number_u32 = 1000;
            input.isotopes[0].mass.binary64_bits = 1000.0_f64.to_bits();
            input.isotopes[0].abundance_percent.binary64_bits = abundance.to_bits();
            input.selections[0].mass_number_u32 = 1000;
            crate::PersonalDataset::fixture(crate::ProfileRecords::MassInput(Box::new(input)))
        };
        let no_natural_distribution = Application::new(TestCapabilities, TestRepository::default())
            .with_data_registry(crate::DataRegistry::fixture(vec![out_of_range(0.0)]));
        assert_eq!(
            states(&no_natural_distribution),
            ("executable", "executable", "executable", "requires_data")
        );
        let natural_distribution = Application::new(TestCapabilities, TestRepository::default())
            .with_data_registry(crate::DataRegistry::fixture(vec![out_of_range(100.0)]));
        assert_eq!(
            states(&natural_distribution),
            ("executable", "executable", "executable", "executable")
        );

        let mut zero = crate::mass_data::tests::fixture();
        zero["isotopes"][0]["isotopic_composition"]["value_decimal"] = json!("0");
        zero["isotopes"][1]["isotopic_composition"]["value_decimal"] = json!("0");
        let dataset = crate::PersonalDataset::fixture(crate::ProfileRecords::Phase4Nist(Box::new(
            crate::mass_data::tests::decode(&zero),
        )));
        let labeled = Application::new(TestCapabilities, TestRepository::default())
            .with_data_registry(crate::DataRegistry::fixture(vec![dataset]));
        assert_eq!(
            states(&labeled),
            ("executable", "executable", "executable", "executable")
        );
        let id = labeled
            .create_document(None, Some("Labeled isotope"))
            .unwrap()
            .document
            .id;
        let call = |request_id: &str, operation: &str, parameters: Value| -> Value {
            let request = json!({"contract":1,"request_id":request_id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
            serde_json::to_value(labeled.execute(request.to_string().as_bytes())).unwrap()
        };
        assert_eq!(
            call(
                "draw-labeled",
                "document.transaction",
                json!({
                    "document_id":id,"expected_revision":0,"commands":[{"op":"add_atom","element":"C","hydrogens":{"mode":"specified","count":0},"position":{"x":0,"y":0}}]
                })
            )["outcome"],
            "ok"
        );
        assert_eq!(
            call(
                "set-isotope",
                "document.transaction",
                json!({"document_id":id,"expected_revision":1,"commands":[{"op":"update_atom","atom":"a1","isotope":12}]})
            )["outcome"],
            "ok"
        );
        let fixed = call(
            "fixed-isotope",
            "chem.properties",
            json!({"document_id":id,"scope":"molecule","property":"distribution"}),
        );
        assert_eq!(fixed["outcome"], "ok", "{fixed}");
        assert_eq!(
            fixed["result"]["components"][0]["isotope_distribution"],
            json!([{"mass_u":12.0,"mass_display":"12.00","probability":1.0}])
        );
        assert_eq!(
            call(
                "change-isotope",
                "document.transaction",
                json!({
                    "document_id":id,"expected_revision":2,"commands":[{"op":"update_atom","atom":"a1","isotope":null}]
                })
            )["outcome"],
            "ok"
        );
        let natural = call(
            "natural-isotope",
            "chem.properties",
            json!({"document_id":id,"scope":"molecule","property":"distribution"}),
        );
        assert_eq!(natural["outcome"], "unsupported");
        assert!(natural["result"].is_null());

        let valid = crate::PersonalDataset::named_fixture(
            "valid",
            crate::ProfileRecords::Phase4Nist(Box::new(crate::mass_data::tests::decode(
                &crate::mass_data::tests::fixture(),
            ))),
        );
        let mut bad = crate::mass_data::tests::synthetic_mass_input();
        bad.features.mass_unit = "unknown-unit".into();
        let invalid = crate::PersonalDataset::named_fixture(
            "invalid",
            crate::ProfileRecords::MassInput(Box::new(bad)),
        );
        let multiple = crate::DataRegistry::fixture(vec![valid, invalid]);
        assert!(matches!(
            properties::select_data(&multiple, None),
            Err(properties::PropertiesError::DataAmbiguous)
        ));
        assert!(properties::select_data(&multiple, Some("valid")).is_ok());
        assert!(matches!(
            properties::select_data(&multiple, Some("invalid")),
            Err(properties::PropertiesError::DataInvalid(
                "unsupported_mass_profile"
            ))
        ));
        let mixed = Application::new(TestCapabilities, TestRepository::default())
            .with_data_registry(multiple);
        assert_eq!(
            states(&mixed),
            ("executable", "executable", "executable", "executable")
        );
        let make_valid = |id| {
            crate::PersonalDataset::named_fixture(
                id,
                crate::ProfileRecords::Phase4Nist(Box::new(crate::mass_data::tests::decode(
                    &crate::mass_data::tests::fixture(),
                ))),
            )
        };
        let both_valid =
            crate::DataRegistry::fixture(vec![make_valid("first"), make_valid("second")]);
        assert!(matches!(
            properties::select_data(&both_valid, None),
            Err(properties::PropertiesError::DataAmbiguous)
        ));
        assert!(properties::select_data(&both_valid, Some("first")).is_ok());
        assert!(properties::select_data(&both_valid, Some("second")).is_ok());
    }

    #[test]
    fn synthetic_external_profile_supplies_mass_with_honest_selection_and_charge_metadata() {
        let snapshot = crate::mass_data::tests::decode(&crate::mass_data::tests::fixture());
        let dataset =
            crate::PersonalDataset::fixture(crate::ProfileRecords::Phase4Nist(Box::new(snapshot)));
        let app = Application::new(TestCapabilities, TestRepository::default())
            .with_data_registry(crate::DataRegistry::fixture(vec![dataset]));
        let id = app
            .create_document(None, Some("Fixture"))
            .unwrap()
            .document
            .id;
        let call = |request_id: &str, operation: &str, parameters: Value| -> Value {
            let body = json!({"contract":1,"request_id":request_id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
            serde_json::to_value(app.execute(body.to_string().as_bytes())).unwrap()
        };
        let added = call(
            "add",
            "document.transaction",
            json!({"document_id":id,"expected_revision":0,"commands":[
                {"op":"add_atom","element":"C","hydrogens":{"mode":"specified","count":0},"position":{"x":0,"y":0}}
            ]}),
        );
        assert_eq!(added["outcome"], "ok");
        let before = serde_json::to_value(app.document(&id).unwrap()).unwrap();
        let preferences: crate::PreferencesChange = serde_json::from_value(json!({
            "ui":{"theme":"system"}, "new_documents":{},
            "computation":{"mass_precision":{"decimals":5}}
        }))
        .unwrap();
        app.set_preferences(None, 0, preferences).unwrap();
        let natural = call(
            "natural",
            "chem.properties",
            json!({"document_id":id,"scope":"molecule","property":"distribution"}),
        );
        assert_eq!(natural["outcome"], "partial", "{natural}");
        assert_eq!(natural["result"]["display_decimals"], 5);
        assert_eq!(
            natural["result"]["components"][0]["mass"]["average_display"],
            "12.01074"
        );
        assert_eq!(
            natural["result"]["data"]["dataset_id"],
            "synthetic-public-values"
        );
        assert_eq!(
            natural["result"]["data"]["source_schema_id"],
            serde_json::Value::Null
        );
        assert_eq!(
            natural["result"]["data"]["calculation_profile"],
            "chemifree-nist-representative-1"
        );
        assert_eq!(natural["result"]["components"][0]["mass"]["exact_u"], 12.0);
        assert_eq!(
            natural["result"]["components"][0]["mass"]["exact_selections"][0]["basis"],
            "maximum_reported_fraction"
        );
        assert_eq!(
            natural["result"]["components"][0]["omitted_abundances"][0]["isotope"],
            14
        );
        assert_eq!(
            natural["loss_report"][0]["feature"],
            "unreported_isotope_abundances"
        );
        assert_eq!(
            serde_json::to_value(app.document(&id).unwrap()).unwrap(),
            before
        );
        app.reset_preferences(None, 1).unwrap();
        let after_reset = call(
            "natural-after-reset",
            "chem.properties",
            json!({"document_id":id,"scope":"molecule","property":"distribution"}),
        );
        assert_eq!(after_reset["outcome"], "partial", "{after_reset}");
        assert_eq!(after_reset["result"]["display_decimals"], 2);
        assert_eq!(
            after_reset["result"]["components"][0]["mass"]["average_display"],
            "12.01"
        );
        assert_eq!(
            natural["result"]["display_decimals"], 5,
            "a previous mass result is unchanged"
        );

        let updated = call(
            "isotope",
            "document.transaction",
            json!({"document_id":id,"expected_revision":1,"commands":[{"op":"update_atom","atom":"a1","isotope":13,"charge":1}]}),
        );
        assert_eq!(updated["outcome"], "ok");
        let labeled = call(
            "labeled",
            "chem.properties",
            json!({"document_id":id,"scope":"molecule","property":"masses","data_registration_id":"synthetic-test"}),
        );
        assert_eq!(labeled["outcome"], "ok", "{labeled}");
        let exact = labeled["result"]["components"][0]["mass"]["exact_u"]
            .as_f64()
            .unwrap();
        assert!(
            (exact - (13.00335483507 - crate::mass_data::CODATA_2022_ELECTRON_MASS_U)).abs()
                < 1e-12
        );
        assert_eq!(
            labeled["result"]["components"][0]["mass"]["exact_selections"][0]["basis"],
            "explicit_isotope"
        );
    }
}
