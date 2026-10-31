//! Application operations for rich CDXML and its loss-accounted editor projection.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::*;
use crate::interchange_codec::{InterchangeCodec, InterchangeError, InterchangeFormat};
use crate::interchange_contract::InterchangeDocumentDto;
use crate::interchange_projection::{self, Projection, SourceObject};

#[derive(Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum ImportParameters {
    /// Read into the rich interchange tree without creating a drawing.
    Read { format: String, source: String },
    /// Inspect a proposed single-page/fragment projection.
    Preview {
        format: String,
        source: String,
        title: String,
        page_id: u32,
        fragment_id: u32,
    },
    /// Create exactly the projection the caller accepted.
    Create {
        format: String,
        source: String,
        title: String,
        page_id: u32,
        fragment_id: u32,
        preview_digest: String,
        accept_loss: bool,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportParameters {
    format: String,
    document: InterchangeDocumentDto,
    #[serde(default)]
    original: Option<String>,
    #[serde(default)]
    discard_opaque: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InterchangeReadResult {
    document: InterchangeDocumentDto,
    choices: Vec<interchange_projection::Choice>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InterchangePreviewResult {
    document: DocumentView,
    mapping: InterchangeMapping,
    preview_digest: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InterchangeExportResult {
    content: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct IdMapping {
    source_path: String,
    source_id: Option<u32>,
    target_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InterchangeMapping {
    document: IdMapping,
    page: IdMapping,
    fragment: IdMapping,
    atoms: Vec<IdMapping>,
    bonds: Vec<IdMapping>,
}

fn format(name: &str) -> Result<InterchangeFormat, InterchangeError> {
    match name {
        "cdxml" => Ok(InterchangeFormat::Cdxml),
        "cdx" => Ok(InterchangeFormat::Cdx),
        _ => Err(InterchangeError {
            code: "unsupported_format",
            message: "Only CDXML is available in this profile.".into(),
            line: None,
            column: None,
        }),
    }
}

fn digest<T: Serialize>(value: &T) -> String {
    let bytes =
        serde_json::to_vec(value).expect("validated projection has finite serializable values");
    format!("{:x}", Sha256::digest(bytes))
}

fn issue(error: InterchangeError) -> Diagnostic {
    let mut details = Map::new();
    if let Some(line) = error.line {
        details.insert("line".into(), line.into());
    }
    if let Some(column) = error.column {
        details.insert("column".into(), column.into());
    }
    Diagnostic {
        code: error.code,
        message: error.message,
        severity: Severity::Error,
        path: None,
        details,
    }
}

fn refused(code: &'static str, message: &str) -> Diagnostic {
    diagnostic(code, message, Map::new())
}

fn response(
    known: &Known,
    result: OperationResult,
    revision: Option<u64>,
    losses: Vec<Loss>,
) -> OperationResponse {
    let outcome = if losses
        .iter()
        .any(|loss| loss.disposition != LossDisposition::Retained)
    {
        Outcome::Partial
    } else {
        Outcome::Ok
    };
    let mut response = known.reply(outcome, revision, Some(result), Vec::new());
    response.loss_report = losses;
    response
}

fn link(source: &SourceObject, target: impl Into<String>) -> IdMapping {
    IdMapping {
        source_path: source.path.clone(),
        source_id: source.id,
        target_id: target.into(),
    }
}

fn mapping(
    projection: &Projection,
    document: &DocumentView,
) -> Result<InterchangeMapping, InterchangeError> {
    if document.atoms.len() != projection.atoms.len()
        || document.bonds.len() != projection.bonds.len()
    {
        return Err(InterchangeError {
            code: "projection_mismatch",
            message: "The created graph does not match its projection.".into(),
            line: None,
            column: None,
        });
    }
    Ok(InterchangeMapping {
        document: link(&projection.document, document.id.clone()),
        page: link(&projection.page, "p1"),
        fragment: link(&projection.fragment, document.molecule.id),
        atoms: projection
            .atoms
            .iter()
            .zip(&document.atoms)
            .map(|(source, atom)| link(&source.source, atom.id.clone()))
            .collect(),
        bonds: projection
            .bonds
            .iter()
            .zip(&document.bonds)
            .map(|(source, bond)| link(&source.source, bond.id.clone()))
            .collect(),
    })
}

fn outside_root_loss() -> Loss {
    Loss {
        feature: "outside_root".into(),
        source_path: "source".into(),
        disposition: LossDisposition::Dropped,
        reason: "Outside-root XML comments or instructions are not stored by the editor.".into(),
        semantic_impact: SemanticImpact::Unknown,
    }
}

fn read(
    codec: &dyn InterchangeCodec,
    name: &str,
    source: &str,
) -> Result<crate::interchange_codec::InterchangeRead, InterchangeError> {
    codec.read(format(name)?, source.as_bytes())
}

fn normalized_document(document: &DocumentView) -> DocumentView {
    let mut document = document.clone();
    document.id = "preview".into();
    document
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn dispatch_interchange_import(
        &self,
        known: &Known,
        value: Value,
    ) -> OperationResponse {
        let parameters: ImportParameters = match parameters(value) {
            Ok(value) => value,
            Err(error) => return known.invalid(error),
        };
        let Some(codec) = &self.interchange_codec else {
            return known.not_running();
        };
        match &parameters {
            ImportParameters::Read { format, source } => match read(codec.as_ref(), format, source)
            {
                Ok(read) => response(
                    known,
                    OperationResult::InterchangeRead(Box::new(InterchangeReadResult {
                        document: InterchangeDocumentDto::from(&read.document),
                        choices: interchange_projection::choices(&read.document),
                    })),
                    None,
                    read.loss_report,
                ),
                Err(error) => known.invalid(issue(error)),
            },
            ImportParameters::Preview {
                format,
                source,
                title,
                page_id,
                fragment_id,
            } => self.interchange_editor(
                known,
                codec.as_ref(),
                format,
                source,
                title,
                *page_id,
                *fragment_id,
                None,
                None,
            ),
            ImportParameters::Create {
                format,
                source,
                title,
                page_id,
                fragment_id,
                preview_digest,
                accept_loss,
            } => {
                let fingerprint = digest(&parameters);
                match self
                    .documents
                    .replay_interchange_create(Some(&known.request_id), &fingerprint)
                {
                    Ok(Some(document)) => {
                        let read = match read(codec.as_ref(), format, source) {
                            Ok(read) => read,
                            Err(error) => return known.invalid(issue(error)),
                        };
                        let mut projection = match interchange_projection::project(
                            &read.document,
                            *page_id,
                            *fragment_id,
                        ) {
                            Ok(projected) => projected,
                            Err(error) => return known.invalid(issue(error)),
                        };
                        if read.opaque_outside_root {
                            projection.losses.push(outside_root_loss());
                        }
                        let mapped = match mapping(&projection, &document.document) {
                            Ok(mapped) => mapped,
                            Err(error) => return known.invalid(issue(error)),
                        };
                        let revision = document.document.revision;
                        let mut result: DocumentResult = document.into();
                        result.interchange_mapping = Some(mapped);
                        return response(
                            known,
                            OperationResult::Document(Box::new(result)),
                            Some(revision),
                            projection.losses,
                        );
                    }
                    Ok(None) => {}
                    Err(error) => {
                        return known.reply(
                            Outcome::Error,
                            error.current_revision,
                            None,
                            vec![failure(&error)],
                        );
                    }
                }
                self.interchange_editor(
                    known,
                    codec.as_ref(),
                    format,
                    source,
                    title,
                    *page_id,
                    *fragment_id,
                    Some((preview_digest, *accept_loss)),
                    Some(fingerprint),
                )
            }
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "The operation's explicit source and consent fields belong together at this boundary."
    )]
    fn interchange_editor(
        &self,
        known: &Known,
        codec: &dyn InterchangeCodec,
        format: &str,
        source: &str,
        title: &str,
        page_id: u32,
        fragment_id: u32,
        consent: Option<(&str, bool)>,
        fingerprint: Option<String>,
    ) -> OperationResponse {
        let Some(generation) = self.interchange_generation else {
            return known.invalid(refused(
                "service_identity_unavailable",
                "A service identity could not be created for this preview.",
            ));
        };
        let input = match read(codec, format, source) {
            Ok(input) => input,
            Err(error) => return known.invalid(issue(error)),
        };
        let mut projection =
            match interchange_projection::project(&input.document, page_id, fragment_id) {
                Ok(projected) => projected,
                Err(error) => return known.invalid(issue(error)),
            };
        if input.opaque_outside_root {
            projection.losses.push(outside_root_loss());
        }
        let commands = match projection.commands() {
            Ok(commands) => commands,
            Err(error) => return known.invalid(issue(error)),
        };
        let settings = match self
            .preferences
            .current()
            .and_then(|p| p.new_document_settings())
        {
            Ok(settings) => settings,
            Err(error) => {
                return known.reply(
                    Outcome::Error,
                    error.current_revision,
                    None,
                    vec![failure(&error)],
                );
            }
        };
        let preview = match crate::documents::Documents::<R>::preview_interchange_create(
            title, settings, &commands,
        ) {
            Ok(preview) => preview,
            Err(error) => {
                return known.reply(
                    Outcome::Error,
                    error.current_revision,
                    None,
                    vec![failure(&error)],
                );
            }
        };
        let bind = |document: &DocumentView| {
            digest(&(
                generation,
                format,
                source,
                title,
                page_id,
                fragment_id,
                &projection,
                normalized_document(document),
            ))
        };
        let expected = bind(&preview.document);
        if consent.is_none() {
            let mapped = match mapping(&projection, &preview.document) {
                Ok(mapped) => mapped,
                Err(error) => return known.invalid(issue(error)),
            };
            return response(
                known,
                OperationResult::InterchangePreview(Box::new(InterchangePreviewResult {
                    document: preview.document,
                    mapping: mapped,
                    preview_digest: expected,
                })),
                None,
                projection.losses,
            );
        }
        let Some((given, accepted)) = consent else {
            unreachable!("preview returned above")
        };
        if given != expected {
            return known.invalid(refused(
                "preview_changed",
                "The service, source, projection, settings or loss report changed; preview again.",
            ));
        }
        if !accepted && !projection.losses.is_empty() {
            return known.invalid(refused(
                "loss_consent_required",
                "Review and accept the reported losses before creating a drawing.",
            ));
        }
        let Some(fingerprint) = fingerprint else {
            return known.invalid(refused(
                "invalid_request",
                "Creation fingerprint is missing.",
            ));
        };
        let result = self.documents.create_interchange(
            Some(&known.request_id),
            fingerprint,
            title,
            &commands,
            || self.preferences.current()?.new_document_settings(),
            |document| {
                // Validate the source-to-target mapping while the candidate is still staged.
                mapping(&projection, document).map_err(|error| {
                    ApplicationError::new(ErrorCode::InvalidRequest, error.message)
                })?;
                if bind(document) == expected {
                    Ok(())
                } else {
                    Err(ApplicationError::new(
                        ErrorCode::InvalidRequest,
                        "The preview changed before creation.",
                    ))
                }
            },
        );
        match result {
            Ok(document) => {
                let mapped = match mapping(&projection, &document.document) {
                    Ok(mapped) => mapped,
                    Err(error) => return known.invalid(issue(error)),
                };
                let revision = document.document.revision;
                let mut result: DocumentResult = document.into();
                result.interchange_mapping = Some(mapped);
                response(
                    known,
                    OperationResult::Document(Box::new(result)),
                    Some(revision),
                    projection.losses,
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

    pub(super) fn dispatch_interchange_export(
        &self,
        known: &Known,
        value: Value,
    ) -> OperationResponse {
        let parameters: ExportParameters = match parameters(value) {
            Ok(value) => value,
            Err(error) => return known.invalid(error),
        };
        let Some(codec) = &self.interchange_codec else {
            return known.not_running();
        };
        let format = match format(&parameters.format) {
            Ok(format) => format,
            Err(error) => return known.invalid(issue(error)),
        };
        let document = match chemifree_domain::interchange::Document::try_from(parameters.document)
        {
            Ok(document) => document,
            Err(error) => return known.invalid(refused("invalid_document", &error.to_string())),
        };
        let original = parameters.original.as_deref().map(str::as_bytes);
        let output = match codec.write(format, &document, original, parameters.discard_opaque) {
            Ok(output) => output,
            Err(error) => return known.invalid(issue(error)),
        };
        let mut losses = output.loss_report;
        if original.is_some_and(|bytes| bytes != output.bytes.as_slice()) {
            losses.push(Loss {
                feature: "xml_serialization".into(),
                source_path: "source".into(),
                disposition: LossDisposition::Normalized,
                reason: "The edited document was reserialized; lexical XML formatting may differ."
                    .into(),
                semantic_impact: SemanticImpact::None,
            });
        }
        let content = match String::from_utf8(output.bytes) {
            Ok(content) => content,
            Err(_) => {
                return known.invalid(refused(
                    "invalid_output",
                    "The provider returned non-UTF-8 CDXML.",
                ));
            }
        };
        response(
            known,
            OperationResult::InterchangeExport(Box::new(InterchangeExportResult { content })),
            None,
            losses,
        )
    }
}
