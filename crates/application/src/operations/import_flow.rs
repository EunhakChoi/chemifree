//! The part of a drawing import that does not depend on the format: preview the candidate
//! against the live snapshot, bind a digest to everything the caller was shown, require the
//! caller's consent where a loss needs it, and commit atomically against the same snapshot.
//! MOL/SDF and SMILES imports share it; each supplies its own wording and result.

use chemifree_domain::document::Command;

use super::*;

/// What differs between formats in an import's final steps.
pub(super) trait ImportAdapter {
    /// A refusal in the format's own diagnostic shape.
    fn refusal(&self, code: &'static str, message: &str) -> Diagnostic;
    /// A failure of the document layer (stale revision, rejected edit) in the format's shape.
    fn failure(&self, known: &Known, error: &ApplicationError) -> OperationResponse;
    /// The digest binding the service, the source, the snapshot, the candidate and the losses.
    fn bind(
        &self,
        generation: [u8; 16],
        snapshot: &DocumentView,
        candidate: &DocumentView,
        losses: &[Loss],
    ) -> String;
    /// The preview a caller is shown, given the exact snapshot it was computed against.
    fn preview(
        self,
        snapshot: DocumentResponse,
        candidate: DocumentView,
        digest: String,
    ) -> OperationResult;
    /// The ids the import created in `result`.
    fn mapping(&self, document: &DocumentView) -> CtMapping;
    /// The refusal for losses the caller has not accepted, or `None` when none needs consent.
    fn consent(&self, known: &Known, losses: &[Loss]) -> Option<OperationResponse>;
}

/// The request's fixed fields and what the format computed from its source.
pub(super) struct ImportRun<'a> {
    pub document_id: &'a str,
    pub expected_revision: u64,
    pub preview: bool,
    pub preview_digest: Option<&'a str>,
    pub accept_loss: bool,
    pub fingerprint: String,
    pub generation: Option<[u8; 16]>,
    pub target: &'a DocumentResponse,
    pub commands: &'a [Command],
    pub losses: Vec<Loss>,
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn run_import<A: ImportAdapter>(
        &self,
        known: &Known,
        run: ImportRun<'_>,
        adapter: A,
    ) -> OperationResponse {
        let ImportRun {
            document_id,
            expected_revision,
            preview,
            preview_digest,
            accept_loss,
            fingerprint,
            generation,
            target,
            commands,
            losses,
        } = run;
        let Some(generation) = generation else {
            return known.invalid(adapter.refusal(
                "service_identity_unavailable",
                "A service identity could not be created for the preview.",
            ));
        };
        let (before, candidate) =
            match self
                .documents
                .preview_import(document_id, expected_revision, commands)
            {
                Ok(candidate) => candidate,
                Err(error) => return adapter.failure(known, &error),
            };
        if before.document != target.document {
            return known.invalid(adapter.refusal(
                "preview_changed",
                "The drawing changed while the import preview was prepared.",
            ));
        }
        let digest = adapter.bind(generation, &before.document, &candidate.document, &losses);
        if preview {
            let revision = target.document.revision;
            return converted(
                known,
                Some(revision),
                adapter.preview(before, candidate.document, digest),
                losses,
            );
        }
        if preview_digest != Some(digest.as_str()) {
            return known.invalid(adapter.refusal(
                "preview_changed",
                "Preview is missing or does not match the current document, graph and losses.",
            ));
        }
        if !accept_loss
            && let Some(refusal) = adapter.consent(known, &losses)
        {
            return refusal;
        }
        let response = self.documents.import(
            document_id,
            Some(&known.request_id),
            expected_revision,
            fingerprint,
            commands,
            |current, candidate| {
                if adapter.bind(generation, current, candidate, &losses) == digest {
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
                result.import_mapping = Some(adapter.mapping(&result.document));
                converted(
                    known,
                    Some(result.document.revision),
                    OperationResult::Document(Box::new(result)),
                    losses,
                )
            }
            Err(error) => adapter.failure(known, &error),
        }
    }
}
