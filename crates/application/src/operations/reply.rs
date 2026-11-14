//! The operation reply envelope, diagnostics, losses and the per-operation result shapes.

use std::borrow::Cow;

use serde::Serialize;
use serde_json::{Map, Value};

use super::arrangement::ArrangementPlanResult;
use super::cip::CipResult;
use super::clipboard::{ClipboardCopyResult, ClipboardPasteResult};
use super::inchi::InchiResult;
use super::interchange::{
    InterchangeExportResult, InterchangeMapping, InterchangePreviewResult, InterchangeReadResult,
};
use super::layout::LayoutResult;
use super::scene_clipboard::{ClipboardSceneCopyResult, ClipboardScenePasteResult};
use super::selection_scope::SelectionScopeResult;
use super::smiles::{SmilesExportResult, SmilesPreviewResult};
use super::stereo::StereoResult;
use super::substructure::SubstructureResult;
use super::svg::SvgExportResult;
use super::template::{TemplateAttachment, TemplateInspection, TemplateReplacement};
use super::transform::TransformPlanResult;
use super::{diagnostic, protocol_error};

use crate::contract::{DocumentResponse, DocumentView, RecoveryView, SelectionDto, SessionView};
use crate::ctfile::CtRecord;
use crate::properties::{ComponentView, MassDataView};

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
    ClipboardCopy(Box<ClipboardCopyResult>),
    ClipboardPaste(Box<ClipboardPasteResult>),
    TemplateInspection(Box<TemplateInspection>),
    TemplateAttachment(Box<TemplateAttachment>),
    TemplateReplacement(Box<TemplateReplacement>),
    ClipboardSceneCopy(Box<ClipboardSceneCopyResult>),
    ClipboardScenePaste(Box<ClipboardScenePasteResult>),
    /// `document.close`.
    Closed(ClosedResult),
    InterchangeRead(Box<InterchangeReadResult>),
    InterchangePreview(Box<InterchangePreviewResult>),
    InterchangeExport(Box<InterchangeExportResult>),
    CtPreview(Box<CtPreviewResult>),
    CtExport(CtExportResult),
    SmilesPreview(Box<SmilesPreviewResult>),
    SmilesExport(Box<SmilesExportResult>),
    /// `document.export` with `format: "svg"`: a bounded SVG drawing and its document snapshot.
    SvgExport(Box<SvgExportResult>),
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
    /// `layout.structure`: a read-only chain layout preview at an exact snapshot.
    Layout(Box<LayoutResult>),
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
