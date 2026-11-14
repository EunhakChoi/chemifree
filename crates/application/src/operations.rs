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
//! representable and whether it runs; the dispatcher (`operations/dispatch.rs`) runs exactly
//! the operations it marks executable.

mod arrangement;
mod backend;
mod catalog;
mod catalog_table;
mod chem_aromaticity;
mod chem_properties;
mod cip;
mod clipboard;
mod dispatch;
mod import;
mod inchi;
mod interchange;
mod layout;
mod molfile;
mod reply;
mod scene_clipboard;
mod selection_scope;
mod smiles;
mod stereo;
mod substructure;
mod svg;
pub(crate) mod template;
mod transform;
pub use template::{TemplateAtom, TemplateAttachment, TemplateInspection};

use chemifree_domain::budget::{Budget, Interrupt, Interrupted, NEVER};
use chemifree_domain::document::Command;
use chemifree_domain::operation::{CompatibilityProfile, OperationId};
pub use interchange::{
    InterchangeExportResult, InterchangeMapping, InterchangePreviewResult, InterchangeReadResult,
};
pub use smiles::{
    SmilesExportResult, SmilesPreviewAtom, SmilesPreviewBond, SmilesPreviewResult, SpanView,
};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::contract::{DocumentResponse, DocumentView, SelectionDto};
use crate::ct_projection;
use crate::documents::validate_request_id;
use crate::error::{ApplicationError, ErrorCode};
use crate::json;
use crate::{Application, CapabilityCatalog, DocumentRepository};
pub use arrangement::{
    ArrangementAxis, ArrangementBounds, ArrangementPlanResult, ArrangementTranslationView,
    ArrangementUnitView,
};
use catalog_table::entry;
pub use cip::{
    AtomLabel, BondLabel, CipResult, EndLabel, Priority, REASONS as CIP_REASONS, reason_sentence,
};
pub use clipboard::{
    ClipboardCopyResult, ClipboardIdMapping, ClipboardMapping, ClipboardPasteResult,
};
pub use inchi::InchiResult;
pub use reply::{
    AromaticityResult, AromaticitySystem, ClosedResult, CtMapping, Diagnostic, DocumentResult,
    Envelope, FoldedHydrogen, IdentityComparison, IdentityResult, IdentityStereo, IdentityView,
    Loss, LossConsent, LossDisposition, OperationCatalog, OperationResponse, OperationResult,
    Outcome, ProfileSupport, SemanticImpact, Severity, Support,
};
pub(crate) use scene_clipboard::mapping as scene_clipboard_mapping;
pub use scene_clipboard::{
    ClipboardSceneCopyResult, ClipboardSceneMapping, ClipboardScenePasteResult,
};
pub use selection_scope::{EffectiveSelectionScope, ScopePosition, SelectionScopeResult};
pub use stereo::{
    AtomDrawnView, AtomInspection, BondDrawnView, BondInspection, MAX_ENTRIES, StereoResult,
};
pub use substructure::{
    AtomPair, BondPair, MATCH_CODES, MappingView, MatchCodeRow, SubstructureLimits,
    SubstructureResult, SubstructureStereo,
};
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

/// The reply of a read-only computation stopped by its time limit or a cancel: no result, the
/// analyzed snapshot's revision, and the operation's model.
fn stopped_read_only(
    known: &Known,
    revision: u64,
    interrupted: Interrupted,
    limit_ms: u64,
    model_id: &'static str,
) -> OperationResponse {
    let error = crate::documents::interruption(interrupted, limit_ms);
    let mut reply = known.reply(Outcome::Error, Some(revision), None, vec![failure(&error)]);
    reply.model_id = Some(model_id.into());
    reply
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

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
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

    /// Resolves a request's `time_limit_ms` and starts the clock of the computation it bounds.
    /// A limit above the maximum is refused with the same diagnostic for every operation that
    /// takes a time limit. The clock starts before any wait for a computation slot, and the
    /// budget also stops once the host shuts down.
    fn start_time_limit<'a>(
        &'a self,
        requested: Option<u64>,
        cancel: &'a dyn Interrupt,
    ) -> Result<(Budget<'a>, u64), Diagnostic> {
        let limit_ms = match self.compute_limits.resolve(requested) {
            Ok(limit_ms) => limit_ms,
            Err(maximum) => {
                let mut details = Map::new();
                details.insert("maximum_ms".into(), maximum.into());
                return Err(Diagnostic {
                    code: ErrorCode::InvalidRequest.as_str(),
                    severity: Severity::Error,
                    message: format!("time_limit_ms is above the maximum of {maximum} ms."),
                    path: Some("parameters.time_limit_ms".into()),
                    details,
                });
            }
        };
        // Limits are capped far below what an `Instant` can add; a deadline that could not be
        // formed would still stop the computation at once rather than never.
        let now = Instant::now();
        let deadline = Some(
            now.checked_add(Duration::from_millis(limit_ms))
                .unwrap_or(now),
        );
        let budget = Budget::new(cancel, deadline).also(self.compute_gate.shutdown_flag());
        Ok((budget, limit_ms))
    }

    /// Starts the time limit of a computation, as `start_time_limit` does. A refused limit is the
    /// operation's reply instead: an error that carries the refusal, no document revision, and
    /// `model_id` naming the operation's model.
    fn start_time_limit_or_refuse<'a>(
        &'a self,
        known: &Known,
        requested: Option<u64>,
        cancel: &'a dyn Interrupt,
        model_id: &'static str,
    ) -> Result<(Budget<'a>, u64), Box<OperationResponse>> {
        self.start_time_limit(requested, cancel).map_err(|refusal| {
            let mut reply = known.invalid(refusal);
            reply.model_id = Some(model_id.into());
            // Boxed to keep the `Result` small: an `OperationResponse` is large, and a refusal is
            // the rare path.
            Box::new(reply)
        })
    }

    /// Runs a read-only computation under the request's budget. It asks the budget once, so that a
    /// limit of 0, a host cancel or a closed gate (`cancel_computations`) stops the request even
    /// when there is nothing to compute. It takes no compute slot: these inspections are not
    /// counted against `with_compute_concurrency` and never wait for it (CONTRACT.md, "Computation
    /// time limit and cancel"). A slot would let a quick inspection be answered `time_limit` while
    /// long conversions hold the slots.
    fn compute_read_only<T>(
        &self,
        budget: &Budget<'_>,
        compute: impl FnOnce(&dyn Interrupt) -> Result<T, Interrupted>,
    ) -> Result<T, Interrupted> {
        budget.check()?;
        compute(budget)
    }
}

#[cfg(test)]
mod property_tests;
