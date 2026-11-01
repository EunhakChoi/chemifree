//! The preview, consent and commit flow that a source-text import into an open document shares.
//!
//! A format supplies how its text is read, how the result becomes edits and losses, and what a
//! preview of it looks like; the flow supplies everything that makes an import safe: the
//! replay of a repeated request, the exact snapshot the preview was made from, the digest that
//! binds consent to that snapshot, graph and losses, and the all-or-nothing commit.

use super::*;

/// What one format contributes to an import.
pub(super) trait ImportFormat {
    /// The source as the format's reader returns it.
    type Parsed;

    /// Reads the source. Runs before the target document is read.
    fn parse(&self) -> Result<Self::Parsed, Diagnostic>;

    /// Edits and losses for `target`. `replayed` says the import already committed, so the
    /// edits are not needed and nothing that only the edits need may be computed.
    fn stage(
        &self,
        parsed: &Self::Parsed,
        target: &DocumentView,
        replayed: bool,
    ) -> Result<Staged, Diagnostic>;

    /// The digest a preview is bound to: the service generation, the format's parameters and
    /// source (hashed by reference), the exact snapshot, the prospective document and the
    /// losses. Consent given to one digest cannot be reused for anything else.
    fn digest(
        &self,
        generation: [u8; 16],
        parsed: &Self::Parsed,
        snapshot: &DocumentView,
        document: &DocumentView,
        losses: &[Loss],
    ) -> String;

    fn preview(
        &self,
        parsed: Self::Parsed,
        snapshot: DocumentResponse,
        document: DocumentView,
        preview_digest: String,
    ) -> OperationResult;

    /// Whether the import needs the caller's `accept_loss` for these losses.
    fn needs_accept(&self, losses: &[Loss]) -> bool;

    /// The refusal named `code`: a changed preview, missing consent, or no service identity.
    fn refusal(&self, code: &'static str, message: &str, losses: &[Loss]) -> Diagnostic;

    /// The reply for a document operation that failed.
    fn document_error(&self, known: &Known, error: &ApplicationError) -> OperationResponse;
}

/// What staging an import yields.
pub(super) struct Staged {
    pub commands: Vec<Command>,
    pub losses: Vec<Loss>,
    pub atoms: usize,
    pub bonds: usize,
}

/// The request-level fields every import carries.
pub(super) struct ImportRequest<'a> {
    pub document_id: &'a str,
    pub expected_revision: u64,
    pub preview: bool,
    pub preview_digest: Option<&'a str>,
    pub accept_loss: bool,
    /// A digest of the whole request, which decides whether a repeated request ID is a replay.
    pub fingerprint: String,
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn run_import<F: ImportFormat>(
        &self,
        known: &Known,
        format: &F,
        request: ImportRequest<'_>,
        generation: Option<[u8; 16]>,
    ) -> OperationResponse {
        let replay = if request.preview {
            None
        } else {
            match self.documents.replay_import(
                request.document_id,
                Some(&known.request_id),
                &request.fingerprint,
            ) {
                Ok(replay) => replay,
                Err(error) => return format.document_error(known, &error),
            }
        };
        let parsed = match format.parse() {
            Ok(parsed) => parsed,
            Err(diagnostic) => return known.invalid(diagnostic),
        };
        let target = match replay.as_ref() {
            Some(response) => response.clone(),
            None => match self.document(request.document_id) {
                Ok(response) => response,
                Err(error) => return format.document_error(known, &error),
            },
        };
        let Staged {
            commands,
            losses,
            atoms,
            bonds,
        } = match format.stage(&parsed, &target.document, replay.is_some()) {
            Ok(staged) => staged,
            Err(diagnostic) => return known.invalid(diagnostic),
        };
        if let Some(response) = replay {
            let mut result: DocumentResult = response.into();
            result.import_mapping = Some(ct_mapping(&result.document, atoms, bonds));
            return converted(
                known,
                Some(result.document.revision),
                OperationResult::Document(Box::new(result)),
                losses,
            );
        }
        let Some(generation) = generation else {
            return known.invalid(format.refusal(
                "service_identity_unavailable",
                "A service identity could not be created for the preview.",
                &losses,
            ));
        };
        let (before, candidate) = match self.documents.preview_import(
            request.document_id,
            request.expected_revision,
            &commands,
        ) {
            Ok(candidate) => candidate,
            Err(error) => return format.document_error(known, &error),
        };
        if before.document != target.document {
            return known.invalid(format.refusal(
                "preview_changed",
                "The drawing changed while the import preview was prepared.",
                &losses,
            ));
        }
        let bind = |snapshot: &DocumentView, document: &DocumentView| {
            format.digest(generation, &parsed, snapshot, document, &losses)
        };
        let preview_digest = bind(&before.document, &candidate.document);
        if request.preview {
            let revision = target.document.revision;
            return converted(
                known,
                Some(revision),
                format.preview(parsed, before, candidate.document, preview_digest),
                losses,
            );
        }
        if request.preview_digest != Some(preview_digest.as_str()) {
            return known.invalid(format.refusal(
                "preview_changed",
                "Preview is missing or does not match the current document, graph and losses.",
                &losses,
            ));
        }
        if !request.accept_loss && format.needs_accept(&losses) {
            return known.invalid(format.refusal(
                "loss_consent_required",
                "Review the reported losses and explicitly accept them before importing.",
                &losses,
            ));
        }
        let response = self.documents.import(
            request.document_id,
            Some(&known.request_id),
            request.expected_revision,
            request.fingerprint,
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
                result.import_mapping = Some(ct_mapping(&result.document, atoms, bonds));
                converted(
                    known,
                    Some(result.document.revision),
                    OperationResult::Document(Box::new(result)),
                    losses,
                )
            }
            Err(error) => format.document_error(known, &error),
        }
    }
}
