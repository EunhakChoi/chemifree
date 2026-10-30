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

use chemifree_domain::operation::{CompatibilityProfile, Execution, OperationId, Representation};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::contract::{
    CommandDto, DocumentResponse, DocumentView, RecoveryView, SelectionDto, SessionView,
    TransactionSelections,
};
use crate::documents::validate_request_id;
use crate::error::{ApplicationError, ErrorCode};
use crate::json;
use crate::{Application, CapabilityCatalog, DocumentRepository};

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
    /// It ran, and `loss_report` lists what it could not keep. No operation reports loss yet.
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
    /// The model or method that produced the result; null when it needs none, as every result
    /// so far does.
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
    /// `document.close`.
    Closed(ClosedResult),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DocumentResult {
    pub document: DocumentView,
    pub session: SessionView,
    pub recovery: Option<RecoveryView>,
}

impl From<DocumentResponse> for DocumentResult {
    fn from(response: DocumentResponse) -> Self {
        Self {
            document: response.document,
            session: response.session,
            recovery: response.recovery,
        }
    }
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
    pub note: &'static str,
    /// Present on a sub-operation that is a capability rather than a command: the commands
    /// that carry it out (none while it is not implemented). Added within operation contract 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commands: Option<&'static [&'static str]>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sub_operations: Vec<Support>,
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

impl Entry {
    /// Marks a transaction part as a capability carried out by `commands`, not a command.
    const fn carried_out_by(self, commands: &'static [&'static str]) -> Self {
        Self {
            commands: Some(commands),
            ..self
        }
    }

    fn view(&self, sub_operations: &[Entry]) -> Support {
        Support {
            id: self.id,
            representation: self.representation.as_str(),
            execution: self.execution.as_str(),
            followup: self.followup,
            note: self.note,
            commands: self.commands,
            sub_operations: sub_operations.iter().map(|entry| entry.view(&[])).collect(),
        }
    }
}

/// The parts of a document transaction: each command, and what a document cannot hold yet.
const TRANSACTION_PARTS: [Entry; 28] = [
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
         1–10 elements) and query bonds (any, or a list of kinds). Representation only: nothing \
         matches a query.",
    )
    .carried_out_by(&["add_atom", "update_atom", "add_bond", "update_bond"]),
    runs(
        "aromatic_or_dative_bond",
        "Records aromatic, dative (from its begin atom, the donor), and unknown bonds. \
         Representation only: aromaticity is not perceived and no Kekulé form is computed.",
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
        "add_reference",
        "remove_reference",
    ]),
    runs(
        "document_settings",
        "Each document stores and undoes its own style; preferences set new-document defaults.",
    )
    .carried_out_by(&["update_settings"]),
];

const PROPERTY_PARTS: [Entry; 2] = [
    missing("formula", "P19", "No molecular formula is computed."),
    missing("mass", "P19", "No average or exact mass is computed."),
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
            missing(
                id,
                "P16",
                "No valence or other chemical validation is implemented.",
            ),
            &[],
        ),
        Op::ChemProperties => (
            missing(id, "P19", "No chemical property is computed."),
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
            "Operations keep document meaning as stored; none drops or converts content.",
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
struct CloseParameters {
    document_id: String,
    #[serde(default)]
    discard_unsaved: bool,
    #[serde(default)]
    expected_revision: Option<u64>,
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

    fn invalid(&self, diagnostic: Diagnostic) -> OperationResponse {
        self.reply(Outcome::Error, None, None, vec![diagnostic])
    }

    fn document(&self, outcome: Result<DocumentResponse, ApplicationError>) -> OperationResponse {
        match outcome {
            Ok(response) => {
                let revision = response.document.revision;
                self.reply(
                    Outcome::Ok,
                    Some(revision),
                    Some(OperationResult::Document(Box::new(response.into()))),
                    Vec::new(),
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

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
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
                    let (entry, parts) = entry(operation);
                    entry.view(parts)
                })
                .collect(),
        }
    }

    /// Runs one request of the operation contract, given as UTF-8 JSON, and returns its
    /// reply. Every request gets a reply; nothing here panics on input.
    pub fn execute(&self, request: &[u8]) -> OperationResponse {
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
        self.dispatch(&known, parameters_value)
    }

    fn dispatch(&self, known: &Known, parameters_value: Value) -> OperationResponse {
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
        if entry(known.operation).0.execution != Execution::Executable {
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
            Op::DocumentTransaction => {
                match parameters::<TransactionParameters>(parameters_value) {
                    Ok(p) => known.document(self.apply_transaction_selecting(
                        &p.document_id,
                        request_id,
                        p.expected_revision,
                        p.commands,
                        TransactionSelections { before: p.selection_before, after: p.selection_after },
                    )),
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
            Op::DocumentImport
            | Op::DocumentExport
            | Op::ChemValidate
            | Op::ChemProperties
            | Op::ChemNameToStructure
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
