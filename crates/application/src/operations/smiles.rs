//! Application operations for SMILES text: import (preview, consent, append) and export
//! (recorded or canonical order) of a drawing's connectivity.

use serde::{Deserialize, Serialize};

use super::*;
use crate::smiles::WriteMode;
use crate::smiles::{IMPLEMENTATION, SmilesError, SmilesErrorCode, VERSION};
use crate::smiles_projection::{self, WriteRefusal};

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum SmilesFormat {
    Smiles,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SmilesImportParameters {
    document_id: String,
    expected_revision: u64,
    format: SmilesFormat,
    source: String,
    #[serde(default)]
    preview: bool,
    #[serde(default)]
    preview_digest: Option<String>,
    #[serde(default)]
    accept_loss: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SmilesExportParameters {
    document_id: String,
    #[allow(
        dead_code,
        reason = "the format only selects this operation's parameters"
    )]
    format: SmilesFormat,
    mode: WriteMode,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SmilesPreviewResult {
    /// Exact working snapshot captured under the same lock as the candidate.
    pub snapshot: DocumentResult,
    pub document: DocumentView,
    pub preview_digest: String,
    pub atoms: usize,
    pub bonds: usize,
    pub implementation: &'static str,
    pub version: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SmilesExportResult {
    /// The exact working copy that was written; the document is not changed.
    pub snapshot: DocumentResult,
    pub mode: &'static str,
    pub smiles: String,
    /// Document atom IDs in the order the atoms appear in the text.
    pub atoms: Vec<String>,
    pub implementation: &'static str,
    pub version: &'static str,
    pub models: SmilesModels,
    /// Canonical mode: terminal hydrogen vertices folded into counts, which have no entry in
    /// `atoms`; empty otherwise.
    pub folded_hydrogens: Vec<FoldedHydrogen>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SmilesModels {
    pub valence: &'static str,
    pub aromaticity: &'static str,
    /// The graph identity model that orders atoms; canonical mode only.
    pub identity: Option<&'static str>,
}

fn unsupported_syntax(error: &SmilesError) -> bool {
    matches!(
        error.code,
        SmilesErrorCode::StereoUnsupported
            | SmilesErrorCode::QueryUnsupported
            | SmilesErrorCode::ReactionUnsupported
            | SmilesErrorCode::AtomMapUnsupported
            | SmilesErrorCode::BondKindUnsupported
    )
}

fn syntax_failure(error: SmilesError, source: &str) -> Diagnostic {
    let code = if unsupported_syntax(&error) {
        "smiles_unsupported"
    } else if matches!(
        error.code,
        SmilesErrorCode::InputTooLong
            | SmilesErrorCode::SizeLimit
            | SmilesErrorCode::BranchDepthLimit
    ) {
        "smiles_limit"
    } else {
        "invalid_smiles"
    };
    let mut details = Map::new();
    details.insert("reason".into(), error.code.code().into());
    details.insert("start".into(), error.start.into());
    details.insert("end".into(), error.end.into());
    let text: String = source
        .get(error.start..error.end)
        .unwrap_or_default()
        .chars()
        .take(40)
        .collect();
    details.insert("text".into(), text.into());
    diagnostic(code, error.code.message(), details)
}

fn write_failure(
    known: &Known,
    revision: u64,
    mode: WriteMode,
    snapshot: DocumentResponse,
    refusal: &WriteRefusal,
) -> OperationResponse {
    let mut details = Map::new();
    details.insert("reason".into(), refusal.reason.into());
    details.insert("mode".into(), mode.name().into());
    if let Some(atom) = refusal.atom {
        details.insert("atom_id".into(), atom.to_string().into());
    }
    details.insert(
        "snapshot".into(),
        serde_json::to_value(DocumentResult::from(snapshot))
            .expect("validated document snapshots serialize to JSON"),
    );
    let (outcome, code, message) = if refusal.limit {
        (
            Outcome::Error,
            "smiles_limit",
            "The drawing exceeds a limit of the SMILES text or of the model that orders it.",
        )
    } else {
        (
            Outcome::Unsupported,
            "smiles_write_unsupported",
            "The drawing cannot be written as SMILES under the shared chemistry models.",
        )
    };
    let mut response = known.reply(
        outcome,
        Some(revision),
        None,
        vec![diagnostic(code, message, details)],
    );
    response.model_id = Some(mode.model_id());
    response
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn smiles_import(
        &self,
        known: &Known,
        p: SmilesImportParameters,
    ) -> OperationResponse {
        let Some(codec) = &self.smiles_codec else {
            return known.not_running();
        };
        let fingerprint = ct_projection::digest(&("smiles", &p));
        let request = ImportRequest {
            document_id: &p.document_id,
            expected_revision: p.expected_revision,
            preview: p.preview,
            preview_digest: p.preview_digest.as_deref(),
            accept_loss: p.accept_loss,
            fingerprint: &fingerprint,
            binding: serde_json::json!(["smiles", &p.source]),
            record: None,
        };
        let model = WriteMode::Recorded.model_id();
        let mut response = self.smiles_import_inner(known, &request, &p, codec.as_ref());
        response.model_id = Some(model);
        response
    }

    fn smiles_import_inner(
        &self,
        known: &Known,
        request: &ImportRequest<'_>,
        p: &SmilesImportParameters,
        codec: &dyn crate::smiles::SmilesCodec,
    ) -> OperationResponse {
        let replay = match self.import_replay(known, request) {
            Ok(replay) => replay,
            Err(response) => return *response,
        };
        let read = match codec.read(&p.source) {
            Ok(read) => read,
            Err(error) => {
                let diagnostic = syntax_failure(error, &p.source);
                return if unsupported_syntax(&error) {
                    known.unsupported(diagnostic)
                } else {
                    known.invalid(diagnostic)
                };
            }
        };
        let target = match self.import_target(known, request, replay.as_ref()) {
            Ok(target) => target,
            Err(response) => return *response,
        };
        let mut losses = Vec::new();
        let commands = match smiles_projection::import_commands(
            &read,
            // A replay's edits were committed already; its atoms are not placed again.
            replay.is_none().then_some(&target.document),
            &p.source,
            &mut losses,
        ) {
            Ok(commands) => commands,
            Err(refusal) => {
                return known.invalid(Self::import_failure(refusal.code, &refusal.message, None));
            }
        };
        let created = (read.molecule.atoms.len(), read.molecule.bonds.len());
        self.import_finish(
            known,
            request,
            ImportPlan {
                replay,
                target,
                commands,
                losses,
                created,
            },
            |snapshot, document, preview_digest| {
                OperationResult::SmilesPreview(Box::new(SmilesPreviewResult {
                    snapshot: snapshot.into(),
                    document,
                    preview_digest,
                    atoms: created.0,
                    bonds: created.1,
                    implementation: IMPLEMENTATION,
                    version: VERSION,
                }))
            },
        )
    }

    pub(super) fn smiles_export(
        &self,
        known: &Known,
        p: SmilesExportParameters,
    ) -> OperationResponse {
        let Some(codec) = &self.smiles_codec else {
            return known.not_running();
        };
        let (snapshot, graph) = match self.documents.graph_snapshot(&p.document_id) {
            Ok(pair) => pair,
            Err(error) => {
                return known.reply(
                    Outcome::Error,
                    error.current_revision,
                    None,
                    vec![failure(&error)],
                );
            }
        };
        let revision = snapshot.document.revision;
        match smiles_projection::write_graph(codec.as_ref(), &graph, p.mode) {
            Ok(written) => {
                let (implementation, version) = p.mode.implementation();
                let mut losses = written.losses;
                smiles_projection::export_losses(&snapshot.document, &mut losses);
                let mut response = converted(
                    known,
                    Some(revision),
                    OperationResult::SmilesExport(Box::new(SmilesExportResult {
                        snapshot: snapshot.into(),
                        mode: p.mode.name(),
                        smiles: written.text,
                        atoms: written.order.iter().map(ToString::to_string).collect(),
                        implementation,
                        version,
                        models: SmilesModels {
                            valence: chemifree_domain::chemistry::VALENCE_MODEL,
                            aromaticity: chemifree_domain::aromaticity::MODEL_ID,
                            identity: p.mode.identity_model(),
                        },
                        folded_hydrogens: written
                            .folded
                            .into_iter()
                            .map(|(hydrogen, attached_to)| FoldedHydrogen {
                                hydrogen: hydrogen.to_string(),
                                attached_to: attached_to.to_string(),
                            })
                            .collect(),
                    })),
                    losses,
                );
                response.model_id = Some(p.mode.model_id());
                response
            }
            Err(refusal) => write_failure(known, revision, p.mode, snapshot, &refusal),
        }
    }
}
