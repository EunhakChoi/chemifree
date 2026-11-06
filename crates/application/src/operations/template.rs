//! Source inspection, read-only attachment preview and one atomic native-template connection.

use chemifree_domain::clipboard::Fragment;
use chemifree_domain::document::{AtomId, ObjectId};
use chemifree_domain::states::AtomKind;
use chemifree_domain::template;
use serde::{Deserialize, Serialize};

use super::clipboard::{ClipboardIdMapping, ClipboardMapping};
use super::{DocumentResult, Known, OperationResponse, OperationResult, Outcome};
use crate::contract::{RingDirectionDto, SelectionDto};
use crate::{Application, ApplicationError, CapabilityCatalog, DocumentRepository};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InspectParameters {
    payload: String,
}

/// Complete immutable attachment input; no field is inferred from selection or attachment metadata.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttachParameters {
    pub document_id: String,
    pub expected_revision: u64,
    pub payload: String,
    pub instance: String,
    pub source_anchor: String,
    pub atom: String,
    pub direction: RingDirectionDto,
    #[serde(default)]
    pub selection_before: Option<SelectionDto>,
}

/// Replacement binds the chosen anchor at the target's existing position, without direction input.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReplaceParameters {
    pub document_id: String,
    pub expected_revision: u64,
    pub payload: String,
    pub instance: String,
    pub source_anchor: String,
    pub atom: String,
    #[serde(default)]
    pub selection_before: Option<SelectionDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TemplateReplacement {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<DocumentResult>,
    pub snapshot: DocumentResult,
    pub mapping: ClipboardMapping,
    pub boundary: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TemplateAtom {
    pub id: String,
    pub element: String,
    pub x: f64,
    pub y: f64,
    pub supported: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TemplateInspection {
    pub payload: String,
    pub atoms: Vec<TemplateAtom>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TemplateAttachment {
    /// Current destination only in a read-only plan, never a reserved write result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<DocumentResult>,
    pub snapshot: DocumentResult,
    pub mapping: ClipboardMapping,
    pub connection: String,
}

pub(crate) fn refusal(error: template::TemplateError) -> ApplicationError {
    crate::clipboard::refusal(error.reason(), error.to_string())
}

pub(crate) fn targets(
    parameters: &AttachParameters,
) -> Result<(ObjectId, AtomId, AtomId), ApplicationError> {
    target_ids(
        &parameters.instance,
        &parameters.source_anchor,
        &parameters.atom,
    )
}

pub(crate) fn replacement_targets(
    parameters: &ReplaceParameters,
) -> Result<(ObjectId, AtomId, AtomId), ApplicationError> {
    target_ids(
        &parameters.instance,
        &parameters.source_anchor,
        &parameters.atom,
    )
}

fn target_ids(
    instance: &str,
    source: &str,
    target: &str,
) -> Result<(ObjectId, AtomId, AtomId), ApplicationError> {
    Ok((
        ObjectId::parse(instance).ok_or_else(|| {
            crate::clipboard::refusal(
                "template_instance_invalid",
                "An explicit instance ID is required",
            )
        })?,
        AtomId::parse(source).ok_or_else(|| {
            crate::clipboard::refusal(
                "template_anchor_unknown",
                "An explicit source anchor ID is required",
            )
        })?,
        AtomId::parse(target).ok_or_else(|| {
            crate::clipboard::refusal(
                "template_target_invalid",
                "An explicit destination atom ID is required",
            )
        })?,
    ))
}

fn result(
    fragment: &Fragment,
    parameters: &AttachParameters,
    source: Option<crate::DocumentResponse>,
    snapshot: crate::DocumentResponse,
) -> Result<TemplateAttachment, ApplicationError> {
    let selected = &snapshot.document.selection;
    if selected.atoms.len() != fragment.atoms.len() || selected.bonds.len() != fragment.bonds.len()
    {
        return Err(crate::clipboard::refusal(
            "clipboard_mapping_invalid",
            "Attachment selection does not identify the complete fresh fragment",
        ));
    }
    let mapping = ClipboardMapping {
        atoms: fragment
            .atoms
            .iter()
            .zip(&selected.atoms)
            .map(|(source, target)| ClipboardIdMapping {
                source: source.id.to_string(),
                target: target.clone(),
            })
            .collect(),
        bonds: fragment
            .bonds
            .iter()
            .zip(&selected.bonds)
            .map(|(source, target)| ClipboardIdMapping {
                source: source.id.to_string(),
                target: target.clone(),
            })
            .collect(),
    };
    let anchor = mapping
        .atoms
        .iter()
        .find(|entry| entry.source == parameters.source_anchor)
        .ok_or_else(|| {
            crate::clipboard::refusal(
                "clipboard_mapping_invalid",
                "Source anchor mapping is missing",
            )
        })?;
    let connection = snapshot
        .document
        .bonds
        .iter()
        .find(|bond| bond.begin == parameters.atom && bond.end == anchor.target)
        .ok_or_else(|| {
            crate::clipboard::refusal("clipboard_mapping_invalid", "Connector is missing")
        })?
        .id
        .clone();
    Ok(TemplateAttachment {
        source: source.map(Into::into),
        snapshot: snapshot.into(),
        mapping,
        connection,
    })
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn resolve_template_replacement(
        &self,
        known: &Known,
        parameters: ReplaceParameters,
        preview: bool,
    ) -> OperationResponse {
        let resolved = (|| {
            let fragment = crate::clipboard::decode(&parameters.payload)?;
            replacement_targets(&parameters)?;
            let (source, snapshot) = if preview {
                let (source, after) = self
                    .documents
                    .template_replace_plan(&parameters, &fragment)?;
                (Some(source), after)
            } else {
                (
                    None,
                    self.documents
                        .template_replace(&known.request_id, &parameters, &fragment)?,
                )
            };
            // The bound anchor never comes from the fresh-ID selection order. On a no-op
            // the ordinary transaction retains the prior selection, so no new IDs are read.
            let fresh_count = fragment.atoms.len() - 1;
            let selected = &snapshot.document.selection;
            let fresh: Vec<_> = selected
                .atoms
                .iter()
                .filter(|id| **id != parameters.atom)
                .collect();
            if fresh_count > 0
                && (fresh.len() != fresh_count || selected.bonds.len() != fragment.bonds.len())
            {
                return Err(crate::clipboard::refusal(
                    "clipboard_mapping_invalid",
                    "Replacement selection does not identify its fresh content",
                ));
            }
            let mut fresh = fresh.into_iter();
            let mut atoms = Vec::new();
            for atom in &fragment.atoms {
                let target = if atom.id.to_string() == parameters.source_anchor {
                    parameters.atom.clone()
                } else {
                    fresh
                        .next()
                        .ok_or_else(|| {
                            crate::clipboard::refusal(
                                "clipboard_mapping_invalid",
                                "Fresh atom mapping is missing",
                            )
                        })?
                        .clone()
                };
                atoms.push(ClipboardIdMapping {
                    source: atom.id.to_string(),
                    target,
                });
            }
            let bonds: Vec<_> = fragment
                .bonds
                .iter()
                .zip(&selected.bonds)
                .map(|(source, target)| ClipboardIdMapping {
                    source: source.id.to_string(),
                    target: target.clone(),
                })
                .collect();
            let boundary = snapshot
                .document
                .bonds
                .iter()
                .find(|bond| {
                    (bond.begin == parameters.atom || bond.end == parameters.atom)
                        && !bonds.iter().any(|entry| entry.target == bond.id)
                })
                .ok_or_else(|| {
                    crate::clipboard::refusal(
                        "clipboard_mapping_invalid",
                        "Retained boundary is missing",
                    )
                })?
                .id
                .clone();
            Ok::<_, ApplicationError>(TemplateReplacement {
                source: source.map(Into::into),
                snapshot: snapshot.into(),
                mapping: ClipboardMapping { atoms, bonds },
                boundary,
            })
        })();
        match resolved {
            Ok(result) => {
                let revision = result
                    .source
                    .as_ref()
                    .map_or(result.snapshot.document.revision, |source| {
                        source.document.revision
                    });
                known.reply(
                    Outcome::Ok,
                    Some(revision),
                    Some(OperationResult::TemplateReplacement(Box::new(result))),
                    Vec::new(),
                )
            }
            Err(error) => known.reply(
                Outcome::Error,
                error.current_revision,
                None,
                vec![super::failure(&error)],
            ),
        }
    }
    pub(super) fn resolve_template_inspect(
        &self,
        known: &Known,
        parameters: InspectParameters,
    ) -> OperationResponse {
        let inspected = (|| {
            let fragment = crate::clipboard::decode(&parameters.payload)?;
            let anchors = template::inspect(&fragment).map_err(refusal)?;
            let atoms = fragment
                .atoms
                .iter()
                .zip(anchors)
                .map(|(atom, anchor)| TemplateAtom {
                    id: atom.id.to_string(),
                    element: match &atom.atom.kind {
                        AtomKind::Element(element) => element.symbol().to_owned(),
                        AtomKind::Query(query) => crate::contract::query_label(query),
                    },
                    x: atom.position.x(),
                    y: atom.position.y(),
                    supported: anchor.supported,
                })
                .collect();
            Ok::<_, ApplicationError>(TemplateInspection {
                payload: parameters.payload,
                atoms,
            })
        })();
        match inspected {
            Ok(result) => known.reply(
                Outcome::Ok,
                None,
                Some(OperationResult::TemplateInspection(Box::new(result))),
                Vec::new(),
            ),
            Err(error) => known.reply(
                Outcome::Error,
                error.current_revision,
                None,
                vec![super::failure(&error)],
            ),
        }
    }
    pub(super) fn resolve_template_attachment(
        &self,
        known: &Known,
        parameters: AttachParameters,
        preview: bool,
    ) -> OperationResponse {
        let resolved = (|| {
            let fragment = crate::clipboard::decode(&parameters.payload)?;
            targets(&parameters)?;
            let (source, snapshot) = if preview {
                let (source, after) = self.documents.template_plan(&parameters, &fragment)?;
                (Some(source), after)
            } else {
                (
                    None,
                    self.documents
                        .template_attach(&known.request_id, &parameters, &fragment)?,
                )
            };
            result(&fragment, &parameters, source, snapshot)
        })();
        match resolved {
            Ok(result) => {
                let revision = result
                    .source
                    .as_ref()
                    .map_or(result.snapshot.document.revision, |source| {
                        source.document.revision
                    });
                let notes = result.snapshot.notes.clone();
                let mut response = known.reply(
                    if notes.is_empty() {
                        Outcome::Ok
                    } else {
                        Outcome::Partial
                    },
                    Some(revision),
                    Some(OperationResult::TemplateAttachment(Box::new(result))),
                    Vec::new(),
                );
                response.loss_report = notes;
                response
            }
            Err(error) => known.reply(
                Outcome::Error,
                error.current_revision,
                None,
                vec![super::failure(&error)],
            ),
        }
    }
}
