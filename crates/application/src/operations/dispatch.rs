//! The dispatcher: reads each known operation's parameters and runs it, with the parameters
//! and identity helpers only its inline arms use.

use chemifree_domain::budget::{Interrupt, Interrupted};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::identity;
use chemifree_domain::operation::{CompatibilityProfile, Execution, OperationId};
use serde_json::{Map, Value};

use crate::contract::{CommandDto, DocumentResponse, SelectionDto, TransactionSelections};
use crate::documents::{AromaticityConversion, Compute};
use crate::{Application, CapabilityCatalog, DocumentRepository};

use super::arrangement::ArrangementParameters;
use super::backend::{Backend, not_connected};
use super::chem_aromaticity::AromaticityParameters;
use super::chem_properties::PropertiesParameters;
use super::cip::CipParameters;
use super::clipboard::{CopyParameters, PasteParameters};
use super::inchi::InchiParameters;
use super::molfile::{CtExportParameters, CtImportParameters, ct_format_alias};
use super::reply::{
    ClosedResult, Diagnostic, DocumentResult, FoldedHydrogen, IdentityComparison, IdentityResult,
    IdentityStereo, IdentityView, OperationResponse, OperationResult, Outcome, Severity,
    ValenceAtom, ValenceResult,
};
use super::scene_clipboard::{SceneCopyParameters, ScenePasteParameters};
use super::selection_scope::SelectionScopeParameters;
use super::smiles::{SmilesExportParameters, SmilesImportParameters};
use super::stereo::StereoParameters;
use super::substructure::SubstructureParameters;
use super::svg::SvgExportParameters;
use super::template;
use super::transform::TransformParameters;
use super::{
    Known, converted, diagnostic, failure, parameters, stereo_not_read, stopped_read_only,
};

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
    /// The time limit this request asks for, in milliseconds: any value up to the server's maximum
    /// (so it may be above the server's default); above the maximum it is refused.
    #[serde(default)]
    time_limit_ms: Option<u64>,
}

/// The identity of `graph` under `mode`, computed with `interrupt`. The outer `Result` carries an
/// interruption; the inner one carries the model's refusal of this graph.
fn identity_view(
    graph: &MoleculeGraph,
    mode: IdentityMode,
    interrupt: &dyn Interrupt,
) -> Result<Result<IdentityView, identity::IdentityError>, Interrupted> {
    let outcome = match mode {
        IdentityMode::Recorded => identity::canonicalize_with(graph, interrupt),
        IdentityMode::Chemical => identity::canonicalize_chemical_with(graph, interrupt),
    }?;
    let result = match outcome {
        Ok(result) => result,
        Err(error) => return Ok(Err(error)),
    };
    Ok(Ok(IdentityView {
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
    }))
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

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn dispatch(
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
            Op::DocumentClipboardCopy => match parameters::<CopyParameters>(parameters_value) {
                Ok(parameters) => self.resolve_clipboard_copy(known, parameters),
                Err(error) => known.invalid(error),
            },
            Op::DocumentTemplateInspect => match parameters::<template::InspectParameters>(parameters_value) {
                Ok(parameters)=>self.resolve_template_inspect(known,parameters),Err(error)=>known.invalid(error),
            },
            Op::DocumentTemplateReplacePlan | Op::DocumentTemplateReplace => match parameters::<template::ReplaceParameters>(parameters_value) {
                Ok(parameters) => self.resolve_template_replacement(known, parameters, known.operation == Op::DocumentTemplateReplacePlan), Err(error) => known.invalid(error),
            },
            Op::DocumentTemplatePlan | Op::DocumentTemplateAttach => match parameters::<template::AttachParameters>(parameters_value) {
                Ok(parameters)=>self.resolve_template_attachment(known,parameters,known.operation==Op::DocumentTemplatePlan),Err(error)=>known.invalid(error),
            },
            Op::DocumentClipboardPaste => match parameters::<PasteParameters>(parameters_value) {
                Ok(parameters) => self.resolve_clipboard_paste(known, parameters),
                Err(error) => known.invalid(error),
            },
            Op::DocumentClipboardSceneCopy => match parameters::<SceneCopyParameters>(parameters_value) {
                Ok(parameters) => self.resolve_clipboard_scene_copy(known, parameters),
                Err(error) => known.invalid(error),
            },
            Op::DocumentClipboardScenePaste => match parameters::<ScenePasteParameters>(parameters_value) {
                Ok(parameters) => self.resolve_clipboard_scene_paste(known, parameters),
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
            Op::ChemAromaticity => match parameters::<AromaticityParameters>(parameters_value) {
                Ok(p) => self.aromaticity(known, p, cancel),
                Err(invalid) => known.invalid(invalid),
            },
            Op::ChemAromaticityConvert => match parameters::<AromaticityConversionParameters>(parameters_value) {
                Ok(p) => {
                    let (budget, limit_ms) = match self.start_time_limit(p.time_limit_ms, cancel) {
                        Ok(started) => started,
                        Err(refusal) => {
                            let mut reply = known.invalid(refusal);
                            reply.model_id = Some(chemifree_domain::aromaticity::MODEL_ID.into());
                            return reply;
                        }
                    };
                    let outcome = self.documents.convert_aromaticity(&p.document_id, request_id, p.expected_revision, p.to,
                        TransactionSelections { before: p.selection_before, after: p.selection_after },
                        Compute {
                            interrupt: &budget,
                            limit_ms,
                            requested_ms: p.time_limit_ms,
                            gate: &self.compute_gate,
                        });
                    let mut reply = known.document(outcome);
                    reply.model_id = Some(chemifree_domain::aromaticity::MODEL_ID.into());
                    reply
                }
                Err(invalid) => known.invalid(invalid),
            },
            Op::ChemIdentity => match parameters::<IdentityParameters>(parameters_value) {
                Ok(p) => {
                    let (budget, limit_ms) = match self.start_time_limit_or_refuse(
                        known,
                        p.time_limit_ms,
                        cancel,
                        p.mode.model_id(),
                    ) {
                        Ok(started) => started,
                        Err(reply) => return *reply,
                    };
                    let ((snapshot, graph), other) = match self.documents.graph_pair_snapshot(
                        &p.document_id,
                        p.other_document_id.as_deref(),
                    ) {
                        Ok(pair) => pair,
                        Err(error) => return known.reply(Outcome::Error, error.current_revision, None, vec![failure(&error)]),
                    };
                    let revision = snapshot.document.revision;
                    // Read before the target graph moves into the computation below.
                    let other_graph_stereo = other
                        .as_ref()
                        .map(|(_, other_graph)| other_graph.has_stereo());
                    // One computation for the request. The target is computed only when the source
                    // has an identity: a source refusal is reported without it.
                    let (identity, target) = match self.compute_read_only(&budget, |interrupt| {
                        let identity = identity_view(&graph, p.mode, interrupt)?;
                        // A source refusal also gives `None`; it returns before `target` is read.
                        let target = match other {
                            Some((other_snapshot, other_graph)) if identity.is_ok() => {
                                let view = identity_view(&other_graph, p.mode, interrupt)?;
                                Some((other_snapshot, view))
                            }
                            _ => None,
                        };
                        Ok((identity, target))
                    }) {
                        Ok(views) => views,
                        Err(interrupted) => {
                            return stopped_read_only(
                                known,
                                revision,
                                interrupted,
                                limit_ms,
                                p.mode.model_id(),
                            );
                        }
                    };
                    let identity = match identity {
                        Ok(value) => value,
                        Err(error) => return identity_failure(known, revision, p.mode, "source", snapshot, error),
                    };
                    let comparison = match target {
                        Some((other_snapshot, Ok(other_identity))) => Some(IdentityComparison {
                            equivalent: identity.code == other_identity.code,
                            snapshot: other_snapshot.into(),
                            identity: other_identity,
                        }),
                        Some((other_snapshot, Err(error))) => {
                            return identity_failure(
                                known,
                                revision,
                                p.mode,
                                "target",
                                other_snapshot,
                                error,
                            );
                        }
                        None => None,
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
                Ok(p) => self.substructure(known, p, cancel),
                Err(invalid) => known.invalid(invalid),
            },
            Op::ChemStereo => match parameters::<StereoParameters>(parameters_value) {
                Ok(p) => self.stereo(known, p, cancel),
                Err(invalid) => known.invalid(invalid),
            },
            Op::ChemInchi => match parameters::<InchiParameters>(parameters_value) {
                Ok(parameters) => self.generate_inchi(known, parameters, cancel),
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
                Backend::Svg => known.invalid(diagnostic(
                    "unsupported_format",
                    "SVG is written by document.export only; it cannot be imported.",
                    Map::new(),
                )),
                Backend::NotConnected(provider) => known.invalid(not_connected(provider)),
            },
            Op::DocumentExport => match self.format_backend(&parameters_value) {
                Backend::Smiles => match parameters::<SmilesExportParameters>(parameters_value) {
                    Ok(p) => self.smiles_export(known, p, cancel),
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
                Backend::Svg => match parameters::<SvgExportParameters>(parameters_value) {
                    Ok(p) => self.svg_export(known, p),
                    Err(invalid) => known.invalid(invalid),
                },
                Backend::NotConnected(provider) => known.invalid(not_connected(provider)),
            },
            Op::ChemProperties => match parameters::<PropertiesParameters>(parameters_value) {
                Ok(p) => self.chemical_properties(known, p, cancel),
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
