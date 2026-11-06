//! Graphic-only native scene forests; chemistry and molecule placements are never copied.

use std::collections::{BTreeMap, BTreeSet};

use chemifree_domain::document::ObjectId;
use chemifree_domain::scene::{GraphicShape, SceneObject};
use chemifree_domain::scene_clipboard::{Fragment, ObjectMapping};
use serde::{Deserialize, Serialize};

use super::{
    ClipboardIdMapping, DocumentResult, Known, OperationResponse, OperationResult, Outcome, failure,
};
use crate::contract::{
    self, DocumentView, GraphicShapeDto, PointDto, SceneObjectView, SelectionDto,
};
use crate::scene_clipboard;
use crate::{Application, ApplicationError, CapabilityCatalog, DocumentRepository};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SceneCopyParameters {
    document_id: String,
    selection: SelectionDto,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ScenePasteParameters {
    document_id: String,
    expected_revision: u64,
    payload: String,
    offset: PointDto,
    #[serde(default)]
    selection_before: Option<SelectionDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ClipboardSceneCopyResult {
    pub snapshot: DocumentResult,
    /// Every copied object in source depth-first painting order.
    pub copied: SelectionDto,
    pub roots: Vec<String>,
    pub payload: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ClipboardSceneMapping {
    pub objects: Vec<ClipboardIdMapping>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ClipboardScenePasteResult {
    pub snapshot: DocumentResult,
    pub mapping: ClipboardSceneMapping,
}

fn refused(known: &Known, error: ApplicationError) -> OperationResponse {
    known.reply(
        Outcome::Error,
        error.current_revision,
        None,
        vec![failure(&error)],
    )
}

fn invalid_mapping() -> ApplicationError {
    crate::clipboard::refusal(
        "clipboard_scene_mapping_invalid",
        "The scene paste does not identify one complete fresh forest",
    )
}

/// Recover the checked mapping from the immutable commit/replay response, not current IDs.
pub(crate) fn mapping(
    fragment: &Fragment,
    snapshot: &DocumentView,
) -> Result<Vec<ObjectMapping>, ApplicationError> {
    let selected = &snapshot.selection;
    if !selected.atoms.is_empty() || !selected.bonds.is_empty() {
        return Err(invalid_mapping());
    }
    let selected_roots: BTreeSet<&str> = selected.objects.iter().map(String::as_str).collect();
    let roots: Vec<&str> = snapshot
        .scene
        .roots
        .iter()
        .map(String::as_str)
        .filter(|id| selected_roots.contains(id))
        .collect();
    if roots.len() != fragment.roots.len() || roots.len() != selected_roots.len() {
        return Err(invalid_mapping());
    }
    let source: BTreeMap<_, _> = fragment
        .objects
        .iter()
        .map(|item| (item.id, &item.object))
        .collect();
    let destination: BTreeMap<_, _> = snapshot
        .scene
        .objects
        .iter()
        .map(|object| {
            let id = match object {
                SceneObjectView::Instance { id, .. }
                | SceneObjectView::Group { id, .. }
                | SceneObjectView::Graphic { id, .. } => id.as_str(),
            };
            (id, object)
        })
        .collect();
    let mut pending: Vec<_> = fragment.roots.iter().copied().zip(roots).rev().collect();
    let mut seen = BTreeSet::new();
    let mut mapped = Vec::new();
    while let Some((source_id, target_id)) = pending.pop() {
        if !seen.insert(target_id) {
            return Err(invalid_mapping());
        }
        let original = source.get(&source_id).ok_or_else(invalid_mapping)?;
        let target = destination.get(target_id).ok_or_else(invalid_mapping)?;
        match (original, target) {
            (
                SceneObject::Group { children },
                SceneObjectView::Group {
                    children: copied, ..
                },
            ) if children.len() == copied.len() => {
                pending.extend(
                    children
                        .iter()
                        .copied()
                        .zip(copied.iter().map(String::as_str))
                        .rev(),
                );
            }
            (SceneObject::Graphic(graphic), SceneObjectView::Graphic { shape, .. }) => {
                let matches = matches!(
                    (graphic.shape, shape),
                    (GraphicShape::Line, GraphicShapeDto::Line)
                        | (GraphicShape::Rectangle, GraphicShapeDto::Rectangle)
                        | (GraphicShape::Arrow, GraphicShapeDto::Arrow)
                );
                if !matches {
                    return Err(invalid_mapping());
                }
            }
            _ => return Err(invalid_mapping()),
        }
        mapped.push(ObjectMapping {
            source: source_id,
            target: ObjectId::parse(target_id).ok_or_else(invalid_mapping)?,
        });
    }
    if mapped.len() != fragment.objects.len() {
        return Err(invalid_mapping());
    }
    Ok(mapped)
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn resolve_clipboard_scene_copy(
        &self,
        known: &Known,
        parameters: SceneCopyParameters,
    ) -> OperationResponse {
        let selection = match contract::exact_selection(&parameters.selection) {
            Ok(selection) => selection,
            Err(message) => {
                return known.invalid(super::diagnostic(
                    "invalid_request",
                    message,
                    serde_json::Map::new(),
                ));
            }
        };
        let (snapshot, fragment) = match self
            .documents
            .clipboard_scene_copy(&parameters.document_id, &selection)
        {
            Ok(result) => result,
            Err(error) => return refused(known, error),
        };
        let payload = match scene_clipboard::encode(&fragment) {
            Ok(payload) => payload,
            Err(error) => return refused(known, error),
        };
        let revision = snapshot.document.revision;
        let result = ClipboardSceneCopyResult {
            snapshot: snapshot.into(),
            copied: SelectionDto {
                objects: fragment
                    .objects
                    .iter()
                    .map(|item| item.id.to_string())
                    .collect(),
                atoms: Vec::new(),
                bonds: Vec::new(),
            },
            roots: fragment.roots.iter().map(ToString::to_string).collect(),
            payload,
        };
        known.reply(
            Outcome::Ok,
            Some(revision),
            Some(OperationResult::ClipboardSceneCopy(Box::new(result))),
            Vec::new(),
        )
    }

    pub(super) fn resolve_clipboard_scene_paste(
        &self,
        known: &Known,
        parameters: ScenePasteParameters,
    ) -> OperationResponse {
        let fragment = match scene_clipboard::decode(&parameters.payload) {
            Ok(fragment) => fragment,
            Err(error) => return refused(known, error),
        };
        let snapshot = match self.documents.clipboard_scene_paste(
            &parameters.document_id,
            Some(&known.request_id),
            scene_clipboard::Paste {
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
        let objects = match mapping(&fragment, &snapshot.document) {
            Ok(mapping) => mapping
                .into_iter()
                .map(|item| ClipboardIdMapping {
                    source: item.source.to_string(),
                    target: item.target.to_string(),
                })
                .collect(),
            Err(error) => return refused(known, error),
        };
        let revision = snapshot.document.revision;
        known.reply(
            Outcome::Ok,
            Some(revision),
            Some(OperationResult::ClipboardScenePaste(Box::new(
                ClipboardScenePasteResult {
                    snapshot: snapshot.into(),
                    mapping: ClipboardSceneMapping { objects },
                },
            ))),
            Vec::new(),
        )
    }
}
