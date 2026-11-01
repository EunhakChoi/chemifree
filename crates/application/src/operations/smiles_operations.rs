//! `document.import` and `document.export` with `format: "smiles"`.
//!
//! Import follows the MOL/SDF flow: a preview binds the source, the drawing's exact snapshot
//! and revision, the candidate and the losses to a digest; creating repeats the digest and the
//! consent. Export is read-only: it returns the text with the exact snapshot it was written
//! from.

use chemifree_domain::smiles::{
    self, CANONICAL_GRAPH_MODEL, CANONICAL_MODEL_ID, ErrorClass, RECORDED_MODEL_ID, SmilesError,
    Subject,
};
use serde::{Deserialize, Serialize};

use super::interchange::consent_refusal;
use super::*;
use crate::smiles_projection;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
enum Tag {
    #[serde(rename = "smiles")]
    Smiles,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SmilesImportParameters {
    document_id: String,
    expected_revision: u64,
    #[serde(rename = "format")]
    _format: Tag,
    source: String,
    #[serde(default)]
    preview: bool,
    #[serde(default)]
    preview_digest: Option<String>,
    #[serde(default)]
    accept_loss: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SmilesForm {
    /// Atoms in the recorded order, bond kinds and hydrogen states as recorded.
    Recorded,
    /// Chemifree's canonical form, independent of atom order and Kekulé/aromatic spelling.
    Canonical,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SmilesExportParameters {
    document_id: String,
    #[serde(rename = "format")]
    _format: Tag,
    form: SmilesForm,
    /// The `loss_digest` of the refusal that showed the losses needing consent.
    #[serde(default)]
    accepted_losses: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SmilesPreviewResult {
    /// The exact working snapshot the candidate was built from.
    pub snapshot: DocumentResult,
    /// The drawing as it would be after the import.
    pub document: DocumentView,
    /// Dot-disconnected parts, joined by ring closures where they are.
    pub components: usize,
    pub preview_digest: String,
}

/// The canonical form's implementation and the graph model it is computed from.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SmilesCanonicalIdentity {
    pub implementation: &'static str,
    pub graph_model: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SmilesExportResult {
    pub content: String,
    pub form: SmilesForm,
    /// Present for the canonical form only.
    pub canonical: Option<SmilesCanonicalIdentity>,
    /// The exact working snapshot written, with its revision.
    pub snapshot: DocumentResult,
    /// Atom IDs in the order the text names the atoms. Folded hydrogens are not in it.
    pub atoms: Vec<String>,
    /// Explicit hydrogen atoms folded into a heavy atom's count (canonical form).
    pub folded_hydrogens: Vec<FoldedHydrogen>,
}

/// The outcome and diagnostic of a refusal. A syntax error or a limit is an error of the
/// request; something valid that this implementation does not do is unsupported.
fn refusal(error: &SmilesError) -> (Outcome, Diagnostic) {
    let mut details = Map::new();
    details.insert(
        "class".into(),
        match error.class() {
            ErrorClass::Syntax => "syntax",
            ErrorClass::Unsupported => "unsupported",
            ErrorClass::Limit => "limit",
            ErrorClass::NotWritable => "not_writable",
        }
        .into(),
    );
    if let Some(span) = error.span {
        details.insert("offset".into(), span.start.into());
        details.insert("end".into(), span.end.into());
    }
    match error.subject {
        Some(Subject::Atom(id)) => {
            details.insert("atom_id".into(), id.to_string().into());
        }
        Some(Subject::Bond(id)) => {
            details.insert("bond_id".into(), id.to_string().into());
        }
        None => {}
    }
    if let Some(reason) = error.reason {
        details.insert("reason".into(), reason.into());
    }
    let outcome = match error.class() {
        ErrorClass::Syntax | ErrorClass::Limit => Outcome::Error,
        ErrorClass::Unsupported | ErrorClass::NotWritable => Outcome::Unsupported,
    };
    (
        outcome,
        diagnostic(error.code.code(), error.code.message(), details),
    )
}

fn projection_refusal(refused: smiles_projection::Refusal) -> Diagnostic {
    diagnostic(refused.code, refused.message, refused.details)
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn smiles_import(
        &self,
        known: &Known,
        p: SmilesImportParameters,
    ) -> OperationResponse {
        let fingerprint = ct_projection::digest(&p);
        let replay = if p.preview {
            None
        } else {
            match self.documents.replay_import(
                &p.document_id,
                Some(&known.request_id),
                &fingerprint,
            ) {
                Ok(replay) => replay,
                Err(error) => {
                    return known.reply(
                        Outcome::Error,
                        error.current_revision,
                        None,
                        vec![failure(&error)],
                    );
                }
            }
        };
        let read = match smiles::read(&p.source) {
            Ok(read) => read,
            Err(error) => {
                let (outcome, diagnostic) = refusal(&error);
                return known.reply(outcome, None, None, vec![diagnostic]);
            }
        };
        let losses = smiles_projection::read_losses(&read);
        if let Some(committed) = replay {
            // The reply the first request committed: its document, with a mapping and losses
            // that depend on the text alone. Nothing is placed or recomputed from the drawing,
            // which may have changed since.
            let mut result: DocumentResult = committed.into();
            result.import_mapping = Some(ct_mapping(
                &result.document,
                read.atoms.len(),
                read.bonds.len(),
            ));
            return converted(
                known,
                Some(result.document.revision),
                OperationResult::Document(Box::new(result)),
                losses,
            );
        }
        let target = match self.document(&p.document_id) {
            Ok(response) => response,
            Err(error) => {
                return known.reply(
                    Outcome::Error,
                    error.current_revision,
                    None,
                    vec![failure(&error)],
                );
            }
        };
        let commands = match smiles_projection::import_commands(&read, &target.document) {
            Ok(commands) => commands,
            Err(refused) => return known.invalid(projection_refusal(refused)),
        };
        let Some(generation) = self.ctfile_generation else {
            return known.invalid(refused_code(
                "service_identity_unavailable",
                "A service identity could not be created for the preview.",
            ));
        };
        let (before, candidate) =
            match self
                .documents
                .preview_import(&p.document_id, p.expected_revision, &commands)
            {
                Ok(candidate) => candidate,
                Err(error) => {
                    return known.reply(
                        Outcome::Error,
                        error.current_revision,
                        None,
                        vec![failure(&error)],
                    );
                }
            };
        if before.document != target.document {
            return known.invalid(refused_code(
                "preview_changed",
                "The drawing changed while the import preview was prepared.",
            ));
        }
        let bind = |snapshot: &DocumentView, document: &DocumentView| {
            ct_projection::digest(&(generation, "smiles", &p.source, snapshot, document, &losses))
        };
        let preview_digest = bind(&before.document, &candidate.document);
        if p.preview {
            let revision = target.document.revision;
            return converted(
                known,
                Some(revision),
                OperationResult::SmilesPreview(Box::new(SmilesPreviewResult {
                    snapshot: before.into(),
                    document: candidate.document,
                    components: read.components,
                    preview_digest,
                })),
                losses,
            );
        }
        if p.preview_digest.as_deref() != Some(preview_digest.as_str()) {
            return known.invalid(refused_code(
                "preview_changed",
                "Preview is missing or does not match the current document, structure and losses.",
            ));
        }
        if !p.accept_loss && !losses.is_empty() {
            return known.invalid(refused_code(
                "loss_consent_required",
                "Review the reported losses and explicitly accept them before importing.",
            ));
        }
        let response = self.documents.import(
            &p.document_id,
            Some(&known.request_id),
            p.expected_revision,
            fingerprint,
            &commands,
            |current, candidate| {
                if bind(current, candidate) == preview_digest {
                    Ok(())
                } else {
                    Err(ApplicationError::new(
                        ErrorCode::InvalidRequest,
                        "The import preview changed before commit.",
                    ))
                }
            },
        );
        match response {
            Ok(response) => {
                let mut result: DocumentResult = response.into();
                result.import_mapping = Some(ct_mapping(
                    &result.document,
                    read.atoms.len(),
                    read.bonds.len(),
                ));
                converted(
                    known,
                    Some(result.document.revision),
                    OperationResult::Document(Box::new(result)),
                    losses,
                )
            }
            Err(error) => known.reply(
                Outcome::Error,
                error.current_revision,
                None,
                vec![failure(&error)],
            ),
        }
    }

    pub(super) fn smiles_export(
        &self,
        known: &Known,
        p: SmilesExportParameters,
    ) -> OperationResponse {
        // A read: the snapshot and the graph come from one lock, and nothing is changed, so
        // the document, its history and its revision are exactly as they were.
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
        let canonical = p.form == SmilesForm::Canonical;
        let model_id = if canonical {
            CANONICAL_MODEL_ID
        } else {
            RECORDED_MODEL_ID
        };
        let written = if canonical {
            smiles::write_canonical(&graph)
        } else {
            smiles::write_recorded(&graph)
        };
        let written = match written {
            Ok(written) => written,
            Err(error) => {
                let (outcome, diagnostic) = refusal(&error);
                let mut response = known.reply(outcome, Some(revision), None, vec![diagnostic]);
                response.model_id = Some(model_id.into());
                return response;
            }
        };
        let losses = smiles_projection::export_losses(&snapshot.document, &written, canonical);
        if let Some(refused) = consent_refusal(
            &losses,
            written.text.as_bytes(),
            p.accepted_losses.as_deref(),
        ) {
            // The envelope of every format's consent refusal: an error with no revision and no
            // model, as for CDXML and CDX.
            return known.invalid(refused);
        }
        let mut response = converted(
            known,
            Some(revision),
            OperationResult::SmilesExport(Box::new(SmilesExportResult {
                content: written.text,
                form: p.form,
                canonical: canonical.then_some(SmilesCanonicalIdentity {
                    implementation: CANONICAL_MODEL_ID,
                    graph_model: CANONICAL_GRAPH_MODEL,
                }),
                atoms: written.order.iter().map(ToString::to_string).collect(),
                folded_hydrogens: written
                    .folded_hydrogens
                    .iter()
                    .map(|(hydrogen, heavy)| FoldedHydrogen {
                        hydrogen: hydrogen.to_string(),
                        attached_to: heavy.to_string(),
                    })
                    .collect(),
                snapshot: snapshot.into(),
            })),
            losses,
        );
        response.model_id = Some(model_id.into());
        response
    }
}

fn refused_code(code: &'static str, message: &str) -> Diagnostic {
    diagnostic(code, message, Map::new())
}
