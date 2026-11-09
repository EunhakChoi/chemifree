//! The losses of a drawing as SVG: what the picture leaves out or shows only in part. The export
//! reports them so that the caller can accept the ones that need consent. There is one loss per
//! feature, in the fixed order below, and each one is `dropped`. A feature found at several IDs
//! lists them in ascending ID order.
//!
//! The table has 17 features. Scene graphics and images are drawn, so they are not reported. A
//! caption outside the drawn character set is not drawn and needs consent, and the underline of a
//! drawn caption is not drawn and needs consent. A scene group has no ink of its own, and scene
//! references are not drawn or kept; neither needs consent.
//!
//! The rules must agree with `render_drawing`, so this module calls the renderer's own rules and
//! does not copy them: the mark rule (`mark_is_drawn` in `geometry.rs`), the bond counts
//! (`degrees` in `geometry.rs`), the zero-length test (`is_zero_length` in `geometry.rs`) on each
//! placement's points (`placements` and `placement_positions` in `svg.rs`), whether an atom is
//! labelled (`shows_label` in `label.rs`), the number of query elements a label names
//! (`SHOWN_QUERY_ELEMENTS` in `label.rs`), and whether a caption is drawn (`caption_drawable` in
//! `caption.rs`).

use std::collections::BTreeMap;

use super::caption;
use super::geometry;
use super::label::{self, SHOWN_QUERY_ELEMENTS};
use super::{placement_positions, placements};
use crate::{
    BondView, DEFAULT_TITLE, DocumentView, LabelStyleDto, LabelWeightDto, Loss, LossConsent,
    LossDisposition, QueryDto, RadicalDto, SceneObjectView, SemanticImpact,
};

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
        query_bond_expressions(view),
        zero_length_bonds(view),
        stereo_marks(view),
        stereo_configuration(view),
        stereo_groups(view),
        abbreviation_displays(view),
        captions(view),
        caption_underline(view),
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

/// Same condition as the SMILES `title` row (`ct_projection::drawing_losses`): an empty
/// or default title holds nothing to lose.
fn title(view: &DocumentView) -> Option<Loss> {
    (!view.title.is_empty() && view.title != DEFAULT_TITLE).then(|| {
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

/// The label styles change only drawn labels, so the loss is reported only when an atom is
/// labelled.
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
    (!styles.is_empty() && any_atom_is_labelled(view)).then(|| {
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

/// Whether at least one atom is labelled. It asks `shows_label` of each atom, with the bond count
/// and carbon-label setting that `layout_drawing` gives `atom_label`.
fn any_atom_is_labelled(view: &DocumentView) -> bool {
    let degrees = geometry::degrees(&view.bonds);
    view.atoms.iter().any(|atom| {
        let degree = degrees.get(&atom.id).copied().unwrap_or(0);
        label::shows_label(atom, degree, view.settings.carbon_labels)
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

/// The query bonds, each of which is reported. `render_drawing` draws every query bond as one
/// dotted line (`BondOrderDto::Query` in `bond_strokes`) and never reads `BondView::query`, so no
/// query bond shows what it stands for. The renderer has no rule that tells the expressions apart,
/// so whether the bond states one decides. The canvas draws the same dotted line and shows the
/// expression only in the inspector, so the row is reported without needing consent (nothing visible
/// on the canvas is missing from the SVG).
fn query_bond_expressions(view: &DocumentView) -> Option<Loss> {
    let bonds: Vec<&str> = view
        .bonds
        .iter()
        .filter(|bond| bond.query.is_some())
        .map(|bond| bond.id.as_str())
        .collect();
    id_list(bonds).map(|list| {
        loss(
            "query_bond_expression",
            format!("document.bonds{list}"),
            SemanticImpact::Chemistry,
            LossConsent::NotRequired,
            "A query bond is drawn as a dotted bond; what it stands for (any bond, or one of a list of bond kinds) is not drawn.",
        )
    })
}

/// The bonds that a placement does not draw because its two atoms are at one point. The renderer
/// gives such a bond no stroke: `bond_strokes` asks `geometry::is_zero_length`, which is the
/// `length === 0` early return of `bondStrokes` (apps/web/src/editor/geometry.ts:664). The canvas
/// draws nothing for it at rest either, since its bond layer draws only `strokesOf(bond)`, which is
/// empty for such a bond (apps/web/src/editor/structure-canvas.tsx:1224-1251). The one mark the
/// canvas adds is a round-capped underlay while the bond or its atoms are selected, or the bond is
/// hovered (apps/web/src/editor/structure-canvas.tsx:923-928). That underlay is a selection or hover
/// highlight, the state the `selection` row reports without consent, so nothing the canvas shows in
/// the drawing itself is missing from the SVG. Hence no consent is required.
fn zero_length_bonds(view: &DocumentView) -> Option<Loss> {
    let points: Vec<BTreeMap<String, geometry::Point>> = placements(view)
        .iter()
        .map(|placement| placement_positions(view, &placement.given))
        .collect();
    let bonds: Vec<&str> = view
        .bonds
        .iter()
        .filter(|bond| points.iter().any(|atoms| zero_length_in(bond, atoms)))
        .map(|bond| bond.id.as_str())
        .collect();
    id_list(bonds).map(|list| {
        loss(
            "zero_length_bond",
            format!("document.bonds{list}"),
            SemanticImpact::Chemistry,
            LossConsent::NotRequired,
            "A bond whose two atoms lie at the same point in a placement has no direction there, so it is not drawn in that placement.",
        )
    })
}

/// Whether `bond` has no length in the placement whose atom points are `atoms`. A bond with an atom
/// that has no point in the placement is not laid out there, so it is not counted.
fn zero_length_in(bond: &BondView, atoms: &BTreeMap<String, geometry::Point>) -> bool {
    match (atoms.get(&bond.begin), atoms.get(&bond.end)) {
        (Some(&begin), Some(&end)) => geometry::is_zero_length(begin, end),
        _ => false,
    }
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

/// Whether a bond's mark is reported: the renderer does not draw it on the bond's order
/// (`mark_is_drawn`), so the bond is drawn as its plain order.
fn stereo_mark_reported(bond: &BondView) -> bool {
    bond.mark
        .as_ref()
        .is_some_and(|mark| !geometry::mark_is_drawn(mark, bond.order))
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

/// The captions with a character outside the drawn set. `caption::caption_drawable` refuses such a
/// caption whole, so none of its text is drawn. A caption inside the set is drawn and not listed.
fn captions(view: &DocumentView) -> Option<Loss> {
    object_list(view, |object| {
        matches!(object, SceneObjectView::Caption { runs, .. } if !caption::caption_drawable(runs))
    })
    .map(|list| {
        loss(
            "caption",
            format!("document.scene.objects{list}"),
            SemanticImpact::Rendering,
            LossConsent::Required,
            "Captions with characters outside the drawn set are not drawn.",
        )
    })
}

/// The drawn captions with an underlined run. The underline is not drawn, but the rest of the
/// caption is, so only the underline is reported. A caption that `captions` lists is not drawn at
/// all, so its underline is not listed here.
fn caption_underline(view: &DocumentView) -> Option<Loss> {
    object_list(view, |object| {
        matches!(
            object,
            SceneObjectView::Caption { runs, .. }
                if caption::caption_drawable(runs) && runs.iter().any(|run| run.underline)
        )
    })
    .map(|list| {
        loss(
            "caption_underline",
            format!("document.scene.objects{list}"),
            SemanticImpact::Rendering,
            LossConsent::Required,
            "Caption underlines are not drawn.",
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
    use crate::{
        AtomKindView, AtomPositionView, AtomView, BondEndDto, BondOrderDto, BondQueryDto,
        CarbonLabelsDto, DisplayUnitDto, HydrogensDto, InstanceView, LabelFontDto, MarkDto,
        MoleculeView, PageView, PositionView, QueryBondOrderDto, SceneView, SelectionDto,
        SettingsDto, StereoView,
    };

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

    fn atom(id: &str, element: &str) -> AtomView {
        AtomView {
            id: id.to_owned(),
            element: element.to_owned(),
            charge: 0,
            isotope: None,
            position: PositionView { x: 0.0, y: 0.0 },
            kind: AtomKindView::Element,
            query: None,
            hydrogens: HydrogensDto::Inferred {},
            radical: RadicalDto::None,
            attachment_points: Vec::new(),
        }
    }

    fn single_bond(id: &str, begin: &str, end: &str) -> BondView {
        BondView {
            id: id.to_owned(),
            begin: begin.to_owned(),
            end: end.to_owned(),
            order: BondOrderDto::Single,
            query: None,
            mark: None,
        }
    }

    /// A drawing of `atoms` and `bonds` with bold labels, so that only the labelled atoms decide
    /// whether the label-style loss is reported.
    fn bold_drawing(
        atoms: Vec<AtomView>,
        bonds: Vec<BondView>,
        carbon_labels: CarbonLabelsDto,
    ) -> DocumentView {
        DocumentView {
            fragment_displays: Vec::new(),
            abbreviations: Vec::new(),
            id: "d1".to_owned(),
            title: DEFAULT_TITLE.to_owned(),
            revision: 0,
            atoms,
            bonds,
            molecule: MoleculeView { id: "m1" },
            scene: SceneView {
                pages: Vec::new(),
                instances: Vec::new(),
                roots: Vec::new(),
                objects: Vec::new(),
                references: Vec::new(),
            },
            selection: SelectionDto::default(),
            settings: SettingsDto {
                bond_length_pt: 30.0,
                line_width_pt: 1.2,
                label_font: LabelFontDto::SansSerif,
                label_weight: LabelWeightDto::Bold,
                label_style: LabelStyleDto::Normal,
                label_underline: false,
                label_outline: false,
                label_shadow: false,
                label_size_pt: 12.0,
                carbon_labels,
                display_unit: DisplayUnitDto::Pt,
            },
            stereo: StereoView::default(),
        }
    }

    /// The `label_typography` row among the losses of `view`, if there is one.
    fn label_typography_loss(view: &DocumentView) -> Option<Loss> {
        svg_losses(view)
            .into_iter()
            .find(|loss| loss.feature == "label_typography")
    }

    #[test]
    fn a_bare_carbon_chain_has_no_label_typography_loss() {
        let view = bold_drawing(
            vec![atom("a1", "C"), atom("a2", "C"), atom("a3", "C")],
            vec![single_bond("b1", "a1", "a2"), single_bond("b2", "a2", "a3")],
            CarbonLabelsDto::Default,
        );
        assert_eq!(
            label_typography_loss(&view),
            None,
            "bonded bare carbons draw no label, so bold reaches no drawn text"
        );
    }

    #[test]
    fn a_labelled_atom_reports_the_label_typography_loss() {
        let view = bold_drawing(
            vec![atom("a1", "C"), atom("a2", "O")],
            vec![single_bond("b1", "a1", "a2")],
            CarbonLabelsDto::Default,
        );
        let row = label_typography_loss(&view).expect("the oxygen is labelled, so bold reaches it");
        assert_eq!(
            row.consent,
            LossConsent::Required,
            "the label style needs consent"
        );
    }

    #[test]
    fn labelling_every_carbon_reports_the_label_typography_loss() {
        let view = bold_drawing(
            vec![atom("a1", "C"), atom("a2", "C"), atom("a3", "C")],
            vec![single_bond("b1", "a1", "a2"), single_bond("b2", "a2", "a3")],
            CarbonLabelsDto::All,
        );
        assert_eq!(
            label_typography_loss(&view).map(|row| row.consent),
            Some(LossConsent::Required),
            "with every carbon labelled, bold reaches the drawn carbon labels"
        );
    }

    /// A query bond from `begin` to `end` that states `expression`.
    fn query_bond(id: &str, begin: &str, end: &str, expression: BondQueryDto) -> BondView {
        BondView {
            id: id.to_owned(),
            begin: begin.to_owned(),
            end: end.to_owned(),
            order: BondOrderDto::Query,
            query: Some(expression),
            mark: None,
        }
    }

    /// The `query_bond_expression` row among the losses of `view`, if there is one.
    fn query_bond_expression_loss(view: &DocumentView) -> Option<Loss> {
        svg_losses(view)
            .into_iter()
            .find(|loss| loss.feature == "query_bond_expression")
    }

    #[test]
    fn a_query_bond_reports_its_expression_without_needing_consent() {
        let expressions = [
            BondQueryDto::Any {},
            BondQueryDto::List {
                orders: vec![QueryBondOrderDto::Single, QueryBondOrderDto::Double],
            },
        ];
        for expression in expressions {
            let view = bold_drawing(
                vec![atom("a1", "C"), atom("a2", "C")],
                vec![query_bond("b1", "a1", "a2", expression.clone())],
                CarbonLabelsDto::Default,
            );
            let Some(row) = query_bond_expression_loss(&view) else {
                panic!("{expression:?} is not drawn, so its bond must be reported");
            };
            assert_eq!(
                row.consent,
                LossConsent::NotRequired,
                "{expression:?}: the canvas does not show the expression either"
            );
            assert_eq!(
                row.source_path, "document.bonds[b1]",
                "{expression:?}: the row names the query bond"
            );
        }
    }

    #[test]
    fn a_plain_bond_reports_no_query_bond_expression_loss() {
        let view = bold_drawing(
            vec![atom("a1", "C"), atom("a2", "C")],
            vec![single_bond("b1", "a1", "a2")],
            CarbonLabelsDto::Default,
        );
        assert_eq!(
            query_bond_expression_loss(&view),
            None,
            "a plain bond states no query expression"
        );
    }

    /// An atom drawn at (`x`, `y`) in pt.
    fn atom_at(id: &str, x: f64, y: f64) -> AtomView {
        AtomView {
            position: PositionView { x, y },
            ..atom(id, "C")
        }
    }

    /// `bold_drawing` with one placement, `i1`, which gives no atom a position of its own, so each
    /// atom is drawn at its own position.
    fn drawing_of(atoms: Vec<AtomView>, bonds: Vec<BondView>) -> DocumentView {
        let mut view = bold_drawing(atoms, bonds, CarbonLabelsDto::Default);
        view.scene.pages.push(PageView { id: "p1" });
        view.scene.instances.push(InstanceView {
            id: "i1".to_owned(),
            page: "p1",
            molecule: "m1",
            positions: Vec::new(),
        });
        view
    }

    /// The `zero_length_bond` row among the losses of `view`, if there is one.
    fn zero_length_bond_loss(view: &DocumentView) -> Option<Loss> {
        svg_losses(view)
            .into_iter()
            .find(|loss| loss.feature == "zero_length_bond")
    }

    #[test]
    fn a_bond_between_coincident_atoms_reports_a_zero_length_bond_without_consent() {
        let view = drawing_of(
            vec![atom_at("a1", 5.0, 5.0), atom_at("a2", 5.0, 5.0)],
            vec![single_bond("b1", "a1", "a2")],
        );
        let layout = crate::svg::layout_drawing(&view);
        assert!(
            layout.instances[0].bonds["b1"].is_empty(),
            "the renderer draws no stroke for a bond of no length"
        );
        let Some(row) = zero_length_bond_loss(&view) else {
            panic!("the bond is not drawn, so it must be reported");
        };
        assert_eq!(
            row.consent,
            LossConsent::NotRequired,
            "the canvas draws no stroke for it either, so nothing it shows at rest is missing"
        );
        assert_eq!(
            row.source_path, "document.bonds[b1]",
            "the row names the bond"
        );
    }

    #[test]
    fn a_bond_of_length_reports_no_zero_length_bond_loss() {
        let view = drawing_of(
            vec![atom_at("a1", 0.0, 0.0), atom_at("a2", 30.0, 0.0)],
            vec![single_bond("b1", "a1", "a2")],
        );
        assert_eq!(
            zero_length_bond_loss(&view),
            None,
            "a bond 30 pt long is drawn"
        );
    }

    #[test]
    fn a_bond_that_collapses_in_one_placement_is_reported_for_that_placement() {
        let mut view = drawing_of(
            vec![atom_at("a1", 0.0, 0.0), atom_at("a2", 30.0, 0.0)],
            vec![single_bond("b1", "a1", "a2")],
        );
        view.scene.instances.push(InstanceView {
            id: "i2".to_owned(),
            page: "p1",
            molecule: "m1",
            positions: vec![AtomPositionView {
                atom: "a2".to_owned(),
                x: 0.0,
                y: 0.0,
            }],
        });
        let layout = crate::svg::layout_drawing(&view);
        assert!(
            !layout.instances[0].bonds["b1"].is_empty(),
            "the first placement draws the 30 pt bond"
        );
        assert!(
            layout.instances[1].bonds["b1"].is_empty(),
            "the second placement puts both atoms at one point"
        );
        let Some(row) = zero_length_bond_loss(&view) else {
            panic!("the second placement does not draw the bond, so it must be reported");
        };
        assert_eq!(
            row.source_path, "document.bonds[b1]",
            "the row names the bond"
        );
    }

    #[test]
    fn only_a_named_title_is_reported() {
        let titled = |title: &str| {
            let mut view = bold_drawing(Vec::new(), Vec::new(), CarbonLabelsDto::Default);
            view.title = title.to_owned();
            svg_losses(&view).iter().any(|loss| loss.feature == "title")
        };
        assert!(!titled(""), "an empty title is not a loss");
        assert!(!titled(DEFAULT_TITLE), "the default title is not a loss");
        assert!(titled("Aspirin"), "a named title is not drawn");
    }
}
