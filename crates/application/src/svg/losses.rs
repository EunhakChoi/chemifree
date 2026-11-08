//! The losses of a drawing as SVG: what the picture leaves out or shows only in part. The export
//! reports them so that the caller can accept the ones that need consent. There is one loss per
//! feature, in the fixed order below, and each one is `dropped`. A feature found at several IDs
//! lists them in ascending ID order.
//!
//! The rules must agree with `render_drawing`. Two of them repeat private renderer rules that this
//! module cannot call: the mark rule (`drawn_mark` in `geometry.rs`) and the number of query
//! elements a label names (`SHOWN_QUERY_ELEMENTS` in `label.rs`).

use crate::{
    BondOrderDto, BondView, DEFAULT_TITLE, DocumentView, LabelStyleDto, LabelWeightDto, Loss,
    LossConsent, LossDisposition, MarkDto, QueryDto, RadicalDto, SceneObjectView, SemanticImpact,
};

/// The number of elements a list query names before its ellipsis (`SHOWN_QUERY_ELEMENTS`).
const SHOWN_QUERY_ELEMENTS: usize = 3;

/// The losses of drawing `view` as SVG, in the fixed feature order. A document that states none of
/// the features has no losses.
#[must_use]
pub(crate) fn svg_losses(view: &DocumentView) -> Vec<Loss> {
    [
        chemical_graph(view),
        title(view),
        selection(view),
        scene_references(view),
        scene_groups(view),
        label_typography(view),
        radical_marks(view),
        attachment_point_marks(view),
        query_labels_truncated(view),
        stereo_marks(view),
        stereo_configuration(view),
        stereo_groups(view),
        abbreviation_displays(view),
        captions(view),
        graphics(view),
        images(view),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn chemical_graph(view: &DocumentView) -> Option<Loss> {
    (!view.atoms.is_empty()).then(|| {
        loss(
            "chemical_graph",
            "document".into(),
            SemanticImpact::Chemistry,
            LossConsent::NotRequired,
            "The atoms and bonds are drawn as a picture only. Atom and bond IDs and the graph as data are not in the file, so the structure cannot be read back from it.",
        )
    })
}

fn title(view: &DocumentView) -> Option<Loss> {
    (view.title != DEFAULT_TITLE).then(|| {
        loss(
            "title",
            "document.title".into(),
            SemanticImpact::Document,
            LossConsent::NotRequired,
            "The document title is not drawn.",
        )
    })
}

fn selection(view: &DocumentView) -> Option<Loss> {
    let chosen = &view.selection;
    let selected =
        !chosen.objects.is_empty() || !chosen.atoms.is_empty() || !chosen.bonds.is_empty();
    selected.then(|| {
        loss(
            "selection",
            "document.selection".into(),
            SemanticImpact::Document,
            LossConsent::NotRequired,
            "The selection is not drawn.",
        )
    })
}

fn scene_references(view: &DocumentView) -> Option<Loss> {
    (!view.scene.references.is_empty()).then(|| {
        loss(
            "scene_references",
            "document.scene.references".into(),
            SemanticImpact::Document,
            LossConsent::NotRequired,
            "Links from a scene object to an object, atom or bond are not drawn or kept.",
        )
    })
}

fn scene_groups(view: &DocumentView) -> Option<Loss> {
    object_list(view, |object| matches!(object, SceneObjectView::Group { .. })).map(|list| {
        loss(
            "scene_group",
            format!("document.scene.objects{list}"),
            SemanticImpact::Document,
            LossConsent::NotRequired,
            "Scene groups are not kept: a group has no drawing of its own, and its members are drawn without the grouping.",
        )
    })
}

fn label_typography(view: &DocumentView) -> Option<Loss> {
    let settings = &view.settings;
    let styles: Vec<&str> = [
        (settings.label_weight == LabelWeightDto::Bold).then_some("bold"),
        (settings.label_style == LabelStyleDto::Italic).then_some("italic"),
        settings.label_underline.then_some("underline"),
        settings.label_outline.then_some("outline"),
        settings.label_shadow.then_some("shadow"),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!styles.is_empty()).then(|| {
        loss(
            "label_typography",
            "document.settings".into(),
            SemanticImpact::Rendering,
            LossConsent::Required,
            &format!(
                "The atom label styles ({}) are not drawn; labels are drawn in regular type.",
                styles.join(", ")
            ),
        )
    })
}

fn radical_marks(view: &DocumentView) -> Option<Loss> {
    let atoms: Vec<&str> = view
        .atoms
        .iter()
        .filter(|atom| atom.radical != RadicalDto::None)
        .map(|atom| atom.id.as_str())
        .collect();
    id_list(atoms).map(|list| {
        loss(
            "radical_mark",
            format!("document.atoms{list}"),
            SemanticImpact::Chemistry,
            LossConsent::Required,
            "Radical dots are not drawn.",
        )
    })
}

fn attachment_point_marks(view: &DocumentView) -> Option<Loss> {
    let atoms: Vec<&str> = view
        .atoms
        .iter()
        .filter(|atom| !atom.attachment_points.is_empty())
        .map(|atom| atom.id.as_str())
        .collect();
    id_list(atoms).map(|list| {
        loss(
            "attachment_point_mark",
            format!("document.atoms{list}"),
            SemanticImpact::Chemistry,
            LossConsent::Required,
            "Attachment-point numbers are not drawn.",
        )
    })
}

fn query_labels_truncated(view: &DocumentView) -> Option<Loss> {
    let atoms: Vec<&str> = view
        .atoms
        .iter()
        .filter(|atom| {
            matches!(
                atom.query.as_ref(),
                Some(QueryDto::List { elements, .. }) if elements.len() > SHOWN_QUERY_ELEMENTS
            )
        })
        .map(|atom| atom.id.as_str())
        .collect();
    id_list(atoms).map(|list| {
        loss(
            "query_label_truncated",
            format!("document.atoms{list}"),
            SemanticImpact::Chemistry,
            LossConsent::Required,
            "A list query label with more than three elements is drawn with its first three and an ellipsis; the other elements are not drawn.",
        )
    })
}

fn stereo_marks(view: &DocumentView) -> Option<Loss> {
    let bonds: Vec<&str> = view
        .bonds
        .iter()
        .filter(|bond| stereo_mark_reported(bond))
        .map(|bond| bond.id.as_str())
        .collect();
    id_list(bonds).map(|list| {
        loss(
            "stereo_mark",
            format!("document.bonds{list}"),
            SemanticImpact::Stereo,
            LossConsent::Required,
            "A crossed mark on a bond that is not double, and a wedge, hash or wavy mark on a bond that is not single, are not kept as stereo marks. Such a bond is drawn as a plain bond of its order.",
        )
    })
}

/// Whether a bond's mark is reported: a crossed mark on a bond that is not double, or a wedge, hash
/// or wavy mark on a bond that is not single. The renderer draws the wedge, hash and wavy marks
/// only on a single bond (`drawn_mark`). A crossed double bond is drawn with two crossing lines
/// (`crossed_strokes`), so its mark is not reported.
fn stereo_mark_reported(bond: &BondView) -> bool {
    match bond.mark.as_ref() {
        None => false,
        Some(MarkDto::Crossed { .. }) => bond.order != BondOrderDto::Double,
        Some(MarkDto::Wedge { .. } | MarkDto::Hash { .. } | MarkDto::Wavy { .. }) => {
            bond.order != BondOrderDto::Single
        }
    }
}

fn stereo_configuration(view: &DocumentView) -> Option<Loss> {
    let stated = !view.stereo.atoms.is_empty() || !view.stereo.bonds.is_empty();
    stated.then(|| {
        loss(
            "stereo_configuration",
            "document.stereo".into(),
            SemanticImpact::Stereo,
            LossConsent::Required,
            "Stated atom and double-bond configurations are not kept. The file holds only the drawing, so a configuration that the drawn marks do not show is absent from it.",
        )
    })
}

fn stereo_groups(view: &DocumentView) -> Option<Loss> {
    let groups: Vec<&str> = view
        .stereo
        .groups
        .iter()
        .map(|group| group.id.as_str())
        .collect();
    id_list(groups).map(|list| {
        loss(
            "stereo_group",
            format!("document.stereo.groups{list}"),
            SemanticImpact::Stereo,
            LossConsent::Required,
            "Enhanced stereo group labels (absolute, AND and OR groups) are not drawn.",
        )
    })
}

fn abbreviation_displays(view: &DocumentView) -> Option<Loss> {
    let abbreviations: Vec<(&str, &str)> = view
        .abbreviations
        .iter()
        .map(|abbreviation| (abbreviation.instance.as_str(), abbreviation.anchor.as_str()))
        .collect();
    let fragments: Vec<(&str, &str)> = view
        .fragment_displays
        .iter()
        .map(|display| (display.instance.as_str(), display.anchor.as_str()))
        .collect();
    let paths: Vec<String> = [
        pair_list(abbreviations).map(|list| format!("document.abbreviations{list}")),
        pair_list(fragments).map(|list| format!("document.fragment_displays{list}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!paths.is_empty()).then(|| {
        loss(
            "abbreviation_display",
            paths.join(" / "),
            SemanticImpact::Rendering,
            LossConsent::Required,
            "Abbreviation labels and fragment displays are not drawn; the full graph is drawn in their place.",
        )
    })
}

fn captions(view: &DocumentView) -> Option<Loss> {
    object_list(view, |object| {
        matches!(object, SceneObjectView::Caption { .. })
    })
    .map(|list| {
        loss(
            "caption",
            format!("document.scene.objects{list}"),
            SemanticImpact::Rendering,
            LossConsent::Required,
            "Captions are not drawn.",
        )
    })
}

fn graphics(view: &DocumentView) -> Option<Loss> {
    object_list(view, |object| matches!(object, SceneObjectView::Graphic { .. })).map(|list| {
        loss(
            "graphic",
            format!("document.scene.objects{list}"),
            SemanticImpact::Rendering,
            LossConsent::Required,
            "Graphic objects (lines, rectangles, ellipses, arrows, polygons and splines) are not drawn.",
        )
    })
}

fn images(view: &DocumentView) -> Option<Loss> {
    object_list(view, |object| {
        matches!(object, SceneObjectView::Image { .. })
    })
    .map(|list| {
        loss(
            "image",
            format!("document.scene.objects{list}"),
            SemanticImpact::Rendering,
            LossConsent::Required,
            "Images are not drawn.",
        )
    })
}

/// A dropped loss of `feature` at `source_path`.
fn loss(
    feature: &str,
    source_path: String,
    impact: SemanticImpact,
    consent: LossConsent,
    reason: &str,
) -> Loss {
    Loss {
        consent,
        feature: feature.into(),
        source_path,
        disposition: LossDisposition::Dropped,
        reason: reason.into(),
        semantic_impact: impact,
    }
}

/// The bracketed IDs of the scene objects that `keep` selects, in ascending ID order, or `None`
/// when no object is selected.
fn object_list(view: &DocumentView, keep: fn(&SceneObjectView) -> bool) -> Option<String> {
    let ids: Vec<&str> = view
        .scene
        .objects
        .iter()
        .filter(|object| keep(object))
        .map(object_id)
        .collect();
    id_list(ids)
}

fn object_id(object: &SceneObjectView) -> &str {
    match object {
        SceneObjectView::Caption { id, .. }
        | SceneObjectView::Instance { id, .. }
        | SceneObjectView::Group { id, .. }
        | SceneObjectView::Image { id, .. }
        | SceneObjectView::Graphic { id, .. } => id.as_str(),
    }
}

/// The IDs bracketed in ascending ID order (`[a1, a4]`), or `None` when there are none. An ID is a
/// kind's prefix and a serial with no leading zeros, so of two IDs of one kind the shorter one has
/// the smaller serial; the text only breaks ties between equal lengths.
fn id_list(mut ids: Vec<&str>) -> Option<String> {
    if ids.is_empty() {
        return None;
    }
    ids.sort_unstable_by_key(|id| (id.len(), *id));
    Some(format!("[{}]", ids.join(", ")))
}

/// The `instance:anchor` pairs bracketed in ascending order, by instance and then by anchor, or
/// `None` when there are none.
fn pair_list(mut pairs: Vec<(&str, &str)>) -> Option<String> {
    if pairs.is_empty() {
        return None;
    }
    pairs.sort_unstable_by_key(|(instance, anchor)| {
        ((instance.len(), *instance), (anchor.len(), *anchor))
    });
    let shown: Vec<String> = pairs
        .iter()
        .map(|(instance, anchor)| format!("{instance}:{anchor}"))
        .collect();
    Some(format!("[{}]", shown.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BondEndDto;

    #[test]
    fn an_id_list_is_bracketed_in_serial_order_or_absent() {
        assert_eq!(id_list(vec!["a4", "a1"]).as_deref(), Some("[a1, a4]"));
        assert_eq!(
            id_list(vec!["a10", "a2", "a1"]).as_deref(),
            Some("[a1, a2, a10]"),
            "a10 follows a2"
        );
        assert_eq!(id_list(Vec::new()), None, "no IDs, no source path");
    }

    #[test]
    fn instance_anchor_pairs_sort_by_instance_then_anchor() {
        let pairs = vec![("s10", "a1"), ("s2", "a3"), ("s2", "a1")];
        assert_eq!(pair_list(pairs).as_deref(), Some("[s2:a1, s2:a3, s10:a1]"));
    }

    #[test]
    fn a_crossed_mark_on_a_single_bond_is_reported() {
        let bond = bond_with_mark(BondOrderDto::Single, MarkDto::Crossed {});
        assert!(
            stereo_mark_reported(&bond),
            "a crossed single bond is drawn as a plain bond, so its mark is reported"
        );
    }

    #[test]
    fn a_crossed_mark_on_a_double_bond_is_not_reported() {
        let bond = bond_with_mark(BondOrderDto::Double, MarkDto::Crossed {});
        assert!(
            !stereo_mark_reported(&bond),
            "a crossed double bond is drawn with crossing lines, so its mark is not reported"
        );
    }

    #[test]
    fn a_wedge_hash_or_wavy_mark_is_reported_only_on_a_bond_that_is_not_single() {
        let marks = [
            MarkDto::Wedge {
                narrow: BondEndDto::Begin,
            },
            MarkDto::Hash {
                narrow: BondEndDto::End,
            },
            MarkDto::Wavy {},
        ];
        for mark in marks {
            let on_single = bond_with_mark(BondOrderDto::Single, mark.clone());
            let on_double = bond_with_mark(BondOrderDto::Double, mark.clone());
            assert!(
                !stereo_mark_reported(&on_single),
                "{mark:?} on a single bond is drawn as the mark, so it is not reported"
            );
            assert!(
                stereo_mark_reported(&on_double),
                "{mark:?} on a double bond does not fit the bond, so it is reported"
            );
        }
    }

    fn bond_with_mark(order: BondOrderDto, mark: MarkDto) -> BondView {
        BondView {
            id: "b1".to_owned(),
            begin: "a1".to_owned(),
            end: "a2".to_owned(),
            order,
            query: None,
            mark: Some(mark),
        }
    }
}
