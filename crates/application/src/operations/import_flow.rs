//! The preview, consent and commit pipeline shared by every text import that appends to a
//! drawing (MOL/SDF, SMILES). A caller parses its own source and builds the edits; this module
//! owns replay, the revision-bound preview token, loss consent and the atomic commit, so the
//! formats cannot drift apart.

use super::*;

/// What identifies one import request, independent of its format.
pub(super) struct ImportRequest<'a> {
    pub document_id: &'a str,
    pub expected_revision: u64,
    pub preview: bool,
    pub preview_digest: Option<&'a str>,
    pub accept_loss: bool,
    /// Identifies the request for replay after a retry.
    pub fingerprint: &'a str,
    /// The format-specific inputs the preview token must bind (source text, selection).
    pub binding: Value,
    /// One-based record number reported in failures.
    pub record: usize,
}

/// A parsed source turned into edits.
pub(super) struct ImportPlan {
    pub replay: Option<DocumentResponse>,
    pub target: DocumentResponse,
    pub commands: Vec<chemifree_domain::document::Command>,
    pub losses: Vec<Loss>,
    /// How many atoms and bonds the commands create.
    pub created: (usize, usize),
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn import_failure(code: &'static str, message: &str, record: usize) -> Diagnostic {
        ct_failure(CtError {
            code,
            message: message.into(),
            record,
            line: None,
        })
    }

    /// A completed request with the same ID and content is answered again without reading the
    /// document's current revision.
    pub(super) fn import_replay(
        &self,
        known: &Known,
        request: &ImportRequest<'_>,
    ) -> Result<Option<DocumentResponse>, OperationResponse> {
        if request.preview {
            return Ok(None);
        }
        self.documents
            .replay_import(
                request.document_id,
                Some(&known.request_id),
                request.fingerprint,
            )
            .map_err(|error| known.indexed_error(&error, request.record))
    }

    /// The document the edits are built against: the replayed result, or the current one.
    pub(super) fn import_target(
        &self,
        known: &Known,
        request: &ImportRequest<'_>,
        replay: Option<&DocumentResponse>,
    ) -> Result<DocumentResponse, OperationResponse> {
        match replay {
            Some(response) => Ok(response.clone()),
            None => self
                .document(request.document_id)
                .map_err(|error| known.indexed_error(&error, request.record)),
        }
    }

    pub(super) fn import_finish(
        &self,
        known: &Known,
        request: &ImportRequest<'_>,
        plan: ImportPlan,
        preview: impl FnOnce(DocumentResponse, DocumentView, String) -> OperationResult,
    ) -> OperationResponse {
        let ImportPlan {
            replay,
            target,
            commands,
            losses,
            created: (atoms, bonds),
        } = plan;
        let record = request.record;
        let mapped = |response: DocumentResponse| {
            let mut result: DocumentResult = response.into();
            result.import_mapping = Some(ct_mapping(&result.document, atoms, bonds));
            result
        };
        if let Some(response) = replay {
            let result = mapped(response);
            return converted(
                known,
                Some(result.document.revision),
                OperationResult::Document(Box::new(result)),
                losses,
            );
        }
        let Some(generation) = self.ctfile_generation else {
            return known.invalid(Self::import_failure(
                "service_identity_unavailable",
                "A service identity could not be created for the preview.",
                record,
            ));
        };
        let (before, candidate) = match self.documents.preview_import(
            request.document_id,
            request.expected_revision,
            &commands,
        ) {
            Ok(candidate) => candidate,
            Err(error) => return known.indexed_error(&error, record),
        };
        if before.document != target.document {
            return known.invalid(Self::import_failure(
                "preview_changed",
                "The drawing changed while the import preview was prepared.",
                record,
            ));
        }
        let bind = |snapshot: &DocumentView, document: &DocumentView| {
            ct_projection::digest(&(generation, &request.binding, snapshot, document, &losses))
        };
        let preview_digest = bind(&before.document, &candidate.document);
        if request.preview {
            let revision = target.document.revision;
            return converted(
                known,
                Some(revision),
                preview(before, candidate.document, preview_digest),
                losses,
            );
        }
        if request.preview_digest != Some(preview_digest.as_str()) {
            return known.invalid(Self::import_failure(
                "preview_changed",
                "Preview is missing or does not match the current document, graph and losses.",
                record,
            ));
        }
        if !request.accept_loss && !losses.is_empty() {
            return known.invalid(Self::import_failure(
                "loss_consent_required",
                "Review the reported losses and explicitly accept them before importing.",
                record,
            ));
        }
        let response = self.documents.import(
            request.document_id,
            Some(&known.request_id),
            request.expected_revision,
            request.fingerprint.to_owned(),
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
                let result = mapped(response);
                converted(
                    known,
                    Some(result.document.revision),
                    OperationResult::Document(Box::new(result)),
                    losses,
                )
            }
            Err(error) => known.indexed_error(&error, record),
        }
    }
}
