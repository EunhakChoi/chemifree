//! The loss vocabulary of exporting a drawing to a text or file format: what a drawing holds
//! that the format does not write. MOL/SDF and SMILES share it; only the wording a format
//! needs differs, so the wording is a parameter and never a reason to copy the logic.

use crate::contract::{DocumentView, SceneObjectView};
use crate::{DEFAULT_TITLE, Loss, LossConsent, LossDisposition, SemanticImpact};

/// Format-specific wording of the shared loss entries.
pub(crate) struct Wording {
    pub native_ids: &'static str,
    /// `None` when the format writes no placement at all (the coordinates entry covers it).
    pub placement_id: Option<&'static str>,
    pub selection: &'static str,
    pub extra_placement: &'static str,
    pub scene_object: &'static str,
    pub scene_references: &'static str,
    pub drawing_settings: &'static str,
    /// `None` when the format writes coordinates.
    pub coordinates: Option<&'static str>,
    /// `None` when the format writes the title.
    pub title: Option<&'static str>,
}

pub(crate) const V2000: Wording = Wording {
    native_ids: "Native atom and bond IDs are mapped to file indices and are not embedded in V2000.",
    placement_id: Some("The placement ID is not embedded in V2000."),
    selection: "The editor selection is not written.",
    extra_placement: "Only the first placement is written.",
    scene_object: "Groups and graphics are not represented in this V2000 profile.",
    scene_references: "Scene references are not written.",
    drawing_settings: "Drawing styles and settings are not represented in this V2000 profile.",
    coordinates: None,
    title: None,
};

pub(crate) const SMILES: Wording = Wording {
    native_ids: "Native atom and bond IDs are not written; SMILES identifies atoms by their position in the text.",
    placement_id: None,
    selection: "The editor selection is not written.",
    extra_placement: "SMILES writes no placement; this one is not written.",
    scene_object: "SMILES has no groups or graphics.",
    scene_references: "SMILES has no scene references.",
    drawing_settings: "SMILES carries no drawing styles or settings.",
    coordinates: Some("SMILES carries no coordinates; atom positions are not written."),
    title: Some("SMILES carries no title; the drawing's title is not written."),
};

fn drop(
    path_prefix: &str,
    path: &str,
    feature: &str,
    impact: SemanticImpact,
    reason: &str,
) -> Loss {
    Loss {
        consent: LossConsent::NotRequired,
        feature: feature.into(),
        source_path: format!("{path_prefix}{path}"),
        disposition: LossDisposition::Dropped,
        reason: reason.into(),
        semantic_impact: impact,
    }
}

/// Reports each part of `doc` the format does not write. A loss is reported only when the
/// content exists; the drawing settings always exist, so they are always reported.
pub(crate) fn drawing_losses(
    doc: &DocumentView,
    path_prefix: &str,
    wording: &Wording,
    losses: &mut Vec<Loss>,
) {
    if let Some(reason) = wording.coordinates
        && !doc.atoms.is_empty()
    {
        losses.push(drop(
            path_prefix,
            ".scene.instances",
            "coordinates",
            SemanticImpact::Geometry,
            reason,
        ));
    }
    if let Some(reason) = wording.title
        && doc.title != DEFAULT_TITLE
    {
        losses.push(drop(
            path_prefix,
            ".title",
            "title",
            SemanticImpact::Document,
            reason,
        ));
    }
    if !doc.atoms.is_empty() || !doc.bonds.is_empty() {
        losses.push(drop(
            path_prefix,
            ".ids",
            "native_ids",
            SemanticImpact::Document,
            wording.native_ids,
        ));
    }
    if let (Some(reason), Some(first)) = (wording.placement_id, doc.scene.instances.first()) {
        losses.push(drop(
            path_prefix,
            &format!(".scene.instances[{}]", first.id),
            "placement_id",
            SemanticImpact::Document,
            reason,
        ));
    }
    if !doc.selection.atoms.is_empty()
        || !doc.selection.bonds.is_empty()
        || !doc.selection.objects.is_empty()
    {
        losses.push(drop(
            path_prefix,
            ".selection",
            "selection",
            SemanticImpact::Document,
            wording.selection,
        ));
    }
    for instance in doc.scene.instances.iter().skip(1) {
        losses.push(drop(
            path_prefix,
            &format!(".scene.instances[{}]", instance.id),
            "extra_placement",
            SemanticImpact::Geometry,
            wording.extra_placement,
        ));
    }
    for object in &doc.scene.objects {
        match object {
            SceneObjectView::Group { id, .. } | SceneObjectView::Graphic { id, .. } => {
                losses.push(drop(
                    path_prefix,
                    &format!(".scene.objects[{id}]"),
                    "scene_object",
                    SemanticImpact::Rendering,
                    wording.scene_object,
                ));
            }
            SceneObjectView::Instance { .. } => {}
        }
    }
    if !doc.scene.references.is_empty() {
        losses.push(drop(
            path_prefix,
            ".scene.references",
            "scene_references",
            SemanticImpact::Document,
            wording.scene_references,
        ));
    }
    losses.push(drop(
        path_prefix,
        ".settings",
        "drawing_settings",
        SemanticImpact::Rendering,
        wording.drawing_settings,
    ));
}
