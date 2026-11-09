//! Bounded SVG drawing of a document's atoms, bonds, graphics, images and captions (P38a1, P38b1,
//! P38b2). This is a pure text renderer over a `DocumentView`, with no font engine and no I/O.
//! `document.export` with format `svg` (operations/svg.rs) serves it over HTTP and the CLI, so
//! its markup is public output: a change to it needs `SVG_MODEL`, the goldens, the conformance
//! cases and the loss table checked.
//!
//! The geometry and the label layout follow `apps/web/src/editor/geometry.ts` and `label.ts`, and
//! the drawing order follows `structure-canvas.tsx`. `layout_drawing` exposes that layout as data,
//! which the parity fixture compares with the web code.
//!
//! Not drawn: radical dots, attachment points, abbreviation and fragment displays, captions
//! outside the drawn character set, caption underlines (`caption.rs`), groups, stereo perception
//! (no descriptor is computed; the stored wedge, hash and wavy marks are drawn), and the bold,
//! italic, underline, outline and shadow typography of atom labels. The status of each is in
//! `docs/verification/p38a1-svg-renderer.md`, and that of captions in
//! `docs/verification/p38b2-svg-captions.md`.

mod caption;
mod font;
mod geometry;
mod label;
mod losses;
mod scene;
mod writer;

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::{DocumentView, SceneObjectView};

pub use font::ADVANCE_TABLE_ID;
pub use geometry::{LabelBox, Pattern, Point, Stroke};
pub use label::{AtomLabel, LabelRun};
pub(crate) use losses::svg_losses;
pub use scene::{GraphicMark, ObjectLayout};

/// The version of the drawing's markup and geometry. A change to either changes it.
pub const SVG_MODEL: &str = "chemifree-svg-3";

/// How far a label box reaches past its label on each side, in pt, in the view box (`LABEL_MARGIN`).
const LABEL_MARGIN: f64 = 4.0;

/// The rectangle a drawing occupies, in pt (its `viewBox`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ViewBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A drawing as SVG text, and the rectangle it occupies.
#[derive(Clone, Debug, PartialEq)]
pub struct SvgDrawing {
    pub content: String,
    pub view_box: ViewBox,
}

/// One instance of the scene, laid out in pt on the page.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InstanceLayout {
    /// The instance's id, as the view lists it.
    pub instance: String,
    /// Each atom's point in this instance.
    pub atoms: BTreeMap<String, Point>,
    /// Each bond's strokes, in the order they are drawn.
    pub bonds: BTreeMap<String, Vec<Stroke>>,
    /// The label of each labelled atom.
    pub labels: BTreeMap<String, AtomLabel>,
}

/// The layout of a whole drawing: the label measurements it uses, each instance in the order the
/// view lists them, and the paint list of its drawn objects.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DrawingLayout {
    pub table: &'static str,
    pub instances: Vec<InstanceLayout>,
    /// Every drawn object in paint order, as `layout_drawing` orders them. An `instance` entry
    /// names an entry of `instances` by its id.
    pub objects: Vec<ObjectLayout>,
}

/// Where one instance's atoms are before the layout.
struct Placement {
    id: String,
    given: BTreeMap<String, Point>,
}

/// The ids of the instance objects of `view`, in the order of `scene.objects`.
fn instance_object_ids(view: &DocumentView) -> Vec<&str> {
    view.scene
        .objects
        .iter()
        .filter_map(|object| match object {
            SceneObjectView::Instance { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect()
}

/// The placements of `view` in the order the web draws them (`structure-canvas.tsx`): the scene's
/// instances sorted by the position of their objects in `scene.objects`, which is the drawing order.
/// An instance without an object comes first, as `indexOf` gives it -1 in the web. Each placement
/// keeps its own positions. A document without an instance has no placement, so nothing is drawn.
fn placements(view: &DocumentView) -> Vec<Placement> {
    let order = instance_object_ids(view);
    let mut placements: Vec<Placement> = view
        .scene
        .instances
        .iter()
        .map(|instance| Placement {
            id: instance.id.clone(),
            given: instance
                .positions
                .iter()
                .map(|position| {
                    (
                        position.atom.clone(),
                        Point {
                            x: position.x,
                            y: position.y,
                        },
                    )
                })
                .collect(),
        })
        .collect();
    placements.sort_by_key(|placement| order.iter().position(|id| *id == placement.id));
    placements
}

/// Each atom's point in a placement: its given position, or else the atom's own (`placedView`).
fn placement_positions(
    view: &DocumentView,
    given: &BTreeMap<String, Point>,
) -> BTreeMap<String, Point> {
    view.atoms
        .iter()
        .map(|atom| {
            let at = given.get(&atom.id).copied().unwrap_or(Point {
                x: atom.position.x,
                y: atom.position.y,
            });
            (atom.id.clone(), at)
        })
        .collect()
}

/// The layout of `view`: what `render_drawing` draws, and what the parity fixture stores.
pub fn layout_drawing(view: &DocumentView) -> DrawingLayout {
    let line_width = view.settings.line_width_pt;
    let lines = geometry::Lines {
        spacing: geometry::bond_spacing(line_width),
        width: line_width,
    };
    let style = label::LabelStyle {
        font: view.settings.label_font,
        size: view.settings.label_size_pt,
        carbons: view.settings.carbon_labels,
    };
    let degrees = geometry::degrees(&view.bonds);
    let rings = geometry::bond_rings(&view.bonds);
    let instances: Vec<InstanceLayout> = placements(view)
        .into_iter()
        .map(|placement| {
            let atoms = placement_positions(view, &placement.given);
            let leaning = geometry::leaning_right(&view.bonds, &atoms);
            let labels: BTreeMap<String, AtomLabel> = view
                .atoms
                .iter()
                .filter_map(|atom| {
                    let degree = degrees.get(&atom.id).copied().unwrap_or(0);
                    let hydrogens_left = leaning.contains(&atom.id);
                    label::atom_label(atom, degree, hydrogens_left, style)
                        .map(|label| (atom.id.clone(), label))
                })
                .collect();
            let sides = geometry::bond_sides(&view.bonds, &atoms, &rings);
            let bonds = view
                .bonds
                .iter()
                .filter_map(|bond| {
                    let begin = *atoms.get(&bond.begin)?;
                    let end = *atoms.get(&bond.end)?;
                    let ends = geometry::Ends {
                        begin,
                        end,
                        begin_box: label_box(&labels, &bond.begin, begin),
                        end_box: label_box(&labels, &bond.end, end),
                    };
                    let side = sides.get(&bond.id).copied().unwrap_or(1.0);
                    Some((
                        bond.id.clone(),
                        geometry::bond_strokes(bond, ends, side, lines),
                    ))
                })
                .collect();
            InstanceLayout {
                instance: placement.id,
                atoms,
                bonds,
                labels,
            }
        })
        .collect();
    let objects = paint_list(view, &instances);
    DrawingLayout {
        table: ADVANCE_TABLE_ID,
        instances,
        objects,
    }
}

/// The paint list of `view`, in the order the web paints it (`structure-canvas.tsx:1056-1088`).
/// An instance without a scene object comes first, as `placements` orders it. Then come the
/// objects of `scene.objects` in order: each instance with a placement, each graphic, each image,
/// and each caption that `caption::layout_caption` lays out. A group has no entry, since it draws
/// nothing, and a caption outside the drawn character set has none either. A graphic whose point
/// count its shape does not admit draws nothing: the domain rejects it (`Graphic::validate`).
fn paint_list(view: &DocumentView, instances: &[InstanceLayout]) -> Vec<ObjectLayout> {
    let listed: BTreeSet<&str> = instance_object_ids(view).into_iter().collect();
    let placed: BTreeSet<&str> = instances
        .iter()
        .map(|instance| instance.instance.as_str())
        .collect();
    let mut objects: Vec<ObjectLayout> = instances
        .iter()
        .filter(|instance| !listed.contains(instance.instance.as_str()))
        .map(|instance| ObjectLayout::Instance {
            id: instance.instance.clone(),
        })
        .collect();
    for object in &view.scene.objects {
        match object {
            SceneObjectView::Instance { id, .. } => {
                if placed.contains(id.as_str()) {
                    objects.push(ObjectLayout::Instance { id: id.clone() });
                }
            }
            SceneObjectView::Graphic {
                id,
                line_width_pt,
                shape,
                points,
                ..
            } => {
                objects.push(ObjectLayout::Graphic {
                    id: id.clone(),
                    width: scene::effective_width(*line_width_pt, view.settings.line_width_pt),
                    marks: scene::graphic_marks(*shape, points),
                });
            }
            SceneObjectView::Image {
                id,
                origin,
                size_pt,
                ..
            } => {
                objects.push(ObjectLayout::Image {
                    id: id.clone(),
                    x: origin.x,
                    y: origin.y,
                    width: size_pt.x,
                    height: size_pt.y,
                });
            }
            SceneObjectView::Caption {
                id,
                anchor,
                alignment,
                runs,
                ..
            } => {
                if let Some(layout) =
                    caption::layout_caption(&view.settings, id, *anchor, *alignment, runs)
                {
                    objects.push(ObjectLayout::Caption(layout));
                }
            }
            SceneObjectView::Group { .. } => {}
        }
    }
    objects
}

/// The box a bond keeps clear of at one end: the label box of its atom, placed at the atom, when the
/// atom is labelled.
fn label_box(labels: &BTreeMap<String, AtomLabel>, id: &str, at: Point) -> Option<LabelBox> {
    labels
        .get(id)
        .map(|label| geometry::placed(label.bounds, at))
}

/// Draws `view` as SVG text, with no error path: every instance of the scene is drawn. It panics
/// only if `view` holds a non-finite number, which a view built from a document never does; see
/// `number` in `svg/writer.rs`.
pub fn render_drawing(view: &DocumentView) -> SvgDrawing {
    let layout = layout_drawing(view);
    let view_box = view_box_of(view, &layout);
    SvgDrawing {
        content: writer::document(view, &layout, view_box),
        view_box,
    }
}

/// The view box of a drawing (`viewBox`): the union of every drawn atom's point grown by the reach
/// of a bond's ink, of every label box grown by `LABEL_MARGIN`, of every graphic's ink and every
/// image's rectangle, each as the writer rounds it, and of every caption's reserved box. Each edge
/// is moved outward to the hundredth. A drawing with no atom, graphic, image or caption has the
/// empty box at the origin.
fn view_box_of(view: &DocumentView, layout: &DrawingLayout) -> ViewBox {
    let line_width = view.settings.line_width_pt;
    let reach = geometry::stroke_reach(line_width, geometry::bond_spacing(line_width));
    let mut edges: Option<(f64, f64, f64, f64)> = None;
    let mut include = |left: f64, top: f64, right: f64, bottom: f64| {
        edges = Some(match edges {
            None => (left, top, right, bottom),
            Some((l, t, r, b)) => (l.min(left), t.min(top), r.max(right), b.max(bottom)),
        });
    };
    for instance in &layout.instances {
        for at in instance.atoms.values() {
            include(at.x - reach, at.y - reach, at.x + reach, at.y + reach);
        }
        for (id, label) in &instance.labels {
            let Some(at) = instance.atoms.get(id) else {
                continue;
            };
            let bounds = geometry::placed(label.bounds, *at);
            include(
                bounds.x - LABEL_MARGIN,
                bounds.y - LABEL_MARGIN,
                bounds.x + bounds.width + LABEL_MARGIN,
                bounds.y + bounds.height + LABEL_MARGIN,
            );
        }
    }
    for object in &layout.objects {
        if let Some((left, top, right, bottom)) = scene::object_ink(object) {
            include(left, top, right, bottom);
        }
    }
    let Some((left, top, right, bottom)) = edges else {
        return ViewBox {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };
    };
    let x = floor_hundredth(left);
    let y = floor_hundredth(top);
    let right = ceil_hundredth(right);
    let bottom = ceil_hundredth(bottom);
    ViewBox {
        x,
        y,
        width: right - x,
        height: bottom - y,
    }
}

/// `value` moved down to the hundredth, so a low edge never moves inward.
fn floor_hundredth(value: f64) -> f64 {
    (value * 100.0).floor() / 100.0
}

/// `value` moved up to the hundredth, so a high edge never moves inward.
fn ceil_hundredth(value: f64) -> f64 {
    (value * 100.0).ceil() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CarbonLabelsDto, DisplayUnitDto, GraphicShapeDto, LabelFontDto, LabelStyleDto,
        LabelWeightDto, MoleculeView, PositionView, SceneView, SelectionDto, SettingsDto,
        StereoView,
    };

    #[test]
    fn view_box_edges_move_outward_to_the_hundredth() {
        assert_eq!(floor_hundredth(-3.456), -3.46, "a low edge moves down");
        assert_eq!(ceil_hundredth(10.001), 10.01, "a high edge moves up");
        assert_eq!(
            floor_hundredth(2.0),
            2.0,
            "an edge on the hundredth stays put"
        );
        assert_eq!(
            ceil_hundredth(2.0),
            2.0,
            "an edge on the hundredth stays put"
        );
    }

    /// A view whose scene holds `scene_objects` and nothing else, with plain label settings.
    fn view_with(scene_objects: Vec<SceneObjectView>) -> DocumentView {
        DocumentView {
            fragment_displays: Vec::new(),
            abbreviations: Vec::new(),
            id: "d1".to_owned(),
            title: "Untitled".to_owned(),
            revision: 0,
            atoms: Vec::new(),
            bonds: Vec::new(),
            molecule: MoleculeView { id: "m1" },
            scene: SceneView {
                pages: Vec::new(),
                instances: Vec::new(),
                roots: Vec::new(),
                objects: scene_objects,
                references: Vec::new(),
            },
            selection: SelectionDto::default(),
            settings: SettingsDto {
                bond_length_pt: 30.0,
                line_width_pt: 1.2,
                label_font: LabelFontDto::SansSerif,
                label_weight: LabelWeightDto::Normal,
                label_style: LabelStyleDto::Normal,
                label_underline: false,
                label_outline: false,
                label_shadow: false,
                label_size_pt: 12.0,
                carbon_labels: CarbonLabelsDto::Default,
                display_unit: DisplayUnitDto::Pt,
            },
            stereo: StereoView::default(),
        }
    }

    #[test]
    fn an_off_grid_image_edge_lies_inside_the_view_box() {
        const EPSILON: f64 = 1e-9;
        // 0.125 and 10.125 are exact in binary, so the unrounded right edge, 10.25, is exact and
        // the view box ends there. The writer draws the image from 0.13 to 10.26.
        let scene_image = SceneObjectView::Image {
            id: "img1".to_owned(),
            parent: None,
            mime_type: "image/png",
            data_base64: "iVBORw0KGgoA+/8=".to_owned(),
            origin: PositionView { x: 0.125, y: 0.125 },
            size_pt: PositionView {
                x: 10.125,
                y: 10.125,
            },
            pixel_width: 1,
            pixel_height: 1,
        };
        let drawing = render_drawing(&view_with(vec![scene_image]));
        let document = roxmltree::Document::parse(&drawing.content).expect("the drawing is XML");
        let view_numbers: Vec<f64> = document
            .root_element()
            .attribute("viewBox")
            .expect("the root element has a viewBox")
            .split_whitespace()
            .map(|token| token.parse().expect("a viewBox entry is a number"))
            .collect();
        let [view_x, view_y, view_width, view_height] =
            <[f64; 4]>::try_from(view_numbers).expect("a viewBox holds four numbers");
        let (view_right, view_bottom) = (view_x + view_width, view_y + view_height);
        let image = document
            .descendants()
            .find(|node| node.tag_name().name() == "image")
            .expect("the drawing holds the image");
        let attribute = |name: &str| -> f64 {
            image
                .attribute(name)
                .expect("the image names its geometry")
                .parse()
                .expect("the image geometry is a number")
        };
        let (x, y) = (attribute("x"), attribute("y"));
        let (right, bottom) = (x + attribute("width"), y + attribute("height"));
        assert!(
            x >= view_x - EPSILON && right <= view_right + EPSILON,
            "the image spans x {x}..{right}, outside the view box's x {view_x}..{view_right}"
        );
        assert!(
            y >= view_y - EPSILON && bottom <= view_bottom + EPSILON,
            "the image spans y {y}..{bottom}, outside the view box's y {view_y}..{view_bottom}"
        );
    }

    #[test]
    fn an_off_grid_graphic_edge_lies_inside_the_view_box() {
        const EPSILON: f64 = 1e-9;
        // The end at 1.006 is written as 1.01 and the width 1.206 as 1.21, so the round cap reaches
        // 1.01 + 0.605 = 1.615. Measured unrounded, the ink would end at 1.609 and the view box at
        // 1.61, short of the drawn edge. The bottom edge fails the same way, from y 1.006.
        let line = SceneObjectView::Graphic {
            id: "g1".to_owned(),
            parent: None,
            line_width_pt: Some(1.206),
            shape: GraphicShapeDto::Line,
            points: vec![
                PositionView { x: 0.0, y: 0.0 },
                PositionView { x: 1.006, y: 1.006 },
            ],
        };
        let drawing = render_drawing(&view_with(vec![line]));
        let document = roxmltree::Document::parse(&drawing.content).expect("the drawing is XML");
        let view_numbers: Vec<f64> = document
            .root_element()
            .attribute("viewBox")
            .expect("the root element has a viewBox")
            .split_whitespace()
            .map(|token| token.parse().expect("a viewBox entry is a number"))
            .collect();
        let [view_x, view_y, view_width, view_height] =
            <[f64; 4]>::try_from(view_numbers).expect("a viewBox holds four numbers");
        let (view_right, view_bottom) = (view_x + view_width, view_y + view_height);
        let line_element = document
            .descendants()
            .find(|node| node.tag_name().name() == "line")
            .expect("the drawing holds the line");
        let stroke_width: f64 = line_element
            .parent_element()
            .expect("the line sits in its graphic's group")
            .attribute("stroke-width")
            .expect("the group names its stroke width")
            .parse()
            .expect("the stroke width is a number");
        let coordinate = |name: &str| -> f64 {
            line_element
                .attribute(name)
                .expect("the line names each end")
                .parse()
                .expect("a line end is a number")
        };
        let (x1, y1) = (coordinate("x1"), coordinate("y1"));
        let (x2, y2) = (coordinate("x2"), coordinate("y2"));
        let half = stroke_width / 2.0;
        let (left, right) = (x1.min(x2) - half, x1.max(x2) + half);
        let (top, bottom) = (y1.min(y2) - half, y1.max(y2) + half);
        assert!(
            left >= view_x - EPSILON && right <= view_right + EPSILON,
            "the stroke spans x {left}..{right}, outside the view box's x {view_x}..{view_right}"
        );
        assert!(
            top >= view_y - EPSILON && bottom <= view_bottom + EPSILON,
            "the stroke spans y {top}..{bottom}, outside the view box's y {view_y}..{view_bottom}"
        );
    }
}
