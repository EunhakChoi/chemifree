//! Bounded native graph copy/paste; browser clipboard transport is a separate adapter.

use serde::{Deserialize, Serialize};

use super::{
    Diagnostic, DocumentResult, Known, OperationResponse, OperationResult, Outcome, failure,
};
use crate::clipboard;
use crate::contract::{self, PointDto, SelectionDto};
use crate::{Application, CapabilityCatalog, DocumentRepository};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CopyParameters {
    document_id: String,
    selection: SelectionDto,
    instance: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PasteParameters {
    document_id: String,
    expected_revision: u64,
    payload: String,
    offset: PointDto,
    #[serde(default)]
    selection_before: Option<SelectionDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ClipboardCopyResult {
    pub snapshot: DocumentResult,
    pub instance: String,
    pub copied: SelectionDto,
    pub boundary_bonds: Vec<String>,
    pub payload: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ClipboardIdMapping {
    pub source: String,
    pub target: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ClipboardMapping {
    pub atoms: Vec<ClipboardIdMapping>,
    pub bonds: Vec<ClipboardIdMapping>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ClipboardPasteResult {
    pub snapshot: DocumentResult,
    pub mapping: ClipboardMapping,
}

fn refused(known: &Known, error: crate::ApplicationError) -> OperationResponse {
    known.reply(
        Outcome::Error,
        error.current_revision,
        None,
        vec![failure(&error)],
    )
}

fn invalid(known: &Known, message: String) -> OperationResponse {
    let diagnostic: Diagnostic =
        super::diagnostic("invalid_request", message, serde_json::Map::new());
    known.invalid(diagnostic)
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn resolve_clipboard_copy(
        &self,
        known: &Known,
        parameters: CopyParameters,
    ) -> OperationResponse {
        let selection = match contract::exact_selection(&parameters.selection) {
            Ok(selection) => selection,
            Err(message) => return invalid(known, message),
        };
        let instance = match contract::instance_id(&parameters.instance) {
            Ok(instance) => instance,
            Err(message) => return invalid(known, message),
        };
        let (snapshot, copied) =
            match self
                .documents
                .clipboard_copy(&parameters.document_id, &selection, instance)
            {
                Ok(copied) => copied,
                Err(error) => return refused(known, error),
            };
        let payload = match clipboard::encode(&copied.fragment) {
            Ok(payload) => payload,
            Err(error) => return refused(known, error),
        };
        let revision = snapshot.document.revision;
        let result = ClipboardCopyResult {
            snapshot: snapshot.into(),
            instance: instance.to_string(),
            payload,
            copied: SelectionDto {
                objects: Vec::new(),
                atoms: copied
                    .fragment
                    .atoms
                    .iter()
                    .map(|atom| atom.id.to_string())
                    .collect(),
                bonds: copied
                    .fragment
                    .bonds
                    .iter()
                    .map(|bond| bond.id.to_string())
                    .collect(),
            },
            boundary_bonds: copied
                .boundary_bonds
                .iter()
                .map(ToString::to_string)
                .collect(),
        };
        known.reply(
            Outcome::Ok,
            Some(revision),
            Some(OperationResult::ClipboardCopy(Box::new(result))),
            Vec::new(),
        )
    }

    pub(super) fn resolve_clipboard_paste(
        &self,
        known: &Known,
        parameters: PasteParameters,
    ) -> OperationResponse {
        let fragment = match clipboard::decode(&parameters.payload) {
            Ok(fragment) => fragment,
            Err(error) => return refused(known, error),
        };
        let snapshot = match self.documents.clipboard_paste(
            &parameters.document_id,
            Some(&known.request_id),
            clipboard::Paste {
                expected_revision: parameters.expected_revision,
                payload: &parameters.payload,
                offset: parameters.offset,
                selection_before: parameters.selection_before,
                fragment: &fragment,
            },
        ) {
            Ok(snapshot) => snapshot,
            Err(error) => return refused(known, error),
        };
        // The paste transaction selects only its new entities. Replay returns that same
        // immutable response, so mappings never depend on the destination's later numbering.
        let selected = &snapshot.document.selection;
        debug_assert_eq!(
            selected.atoms.len(),
            fragment.atoms.len(),
            "the checked paste plan selects every fresh atom"
        );
        debug_assert_eq!(
            selected.bonds.len(),
            fragment.bonds.len(),
            "the checked paste plan selects every fresh bond"
        );
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
        let revision = snapshot.document.revision;
        let losses = snapshot.notes.clone();
        let mut response = known.reply(
            if losses.is_empty() {
                Outcome::Ok
            } else {
                Outcome::Partial
            },
            Some(revision),
            Some(OperationResult::ClipboardPaste(Box::new(
                ClipboardPasteResult {
                    snapshot: snapshot.into(),
                    mapping,
                },
            ))),
            Vec::new(),
        );
        response.loss_report = losses;
        response
    }
}
