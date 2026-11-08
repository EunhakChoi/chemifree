//! Bounded SVG drawing of a document's atoms and bonds (P38a1). This is a pure text renderer over a
//! `DocumentView`, with no font engine and no I/O. Nothing outside this module calls it yet: no
//! operation, catalog, CLI or HTTP route exports a drawing until P38a2.
//!
//! The geometry and the label layout follow `apps/web/src/editor/geometry.ts` and `label.ts`, and
//! the drawing order follows `structure-canvas.tsx`. `layout_drawing` exposes that layout as data,
//! which the parity fixture compares with the web code.
//!
//! Not drawn: radical dots, attachment points, abbreviation and fragment displays, captions,
//! graphics, images, groups, stereo perception (no descriptor is computed; the stored wedge, hash
//! and wavy marks are drawn), and bold, italic, underline, outline and shadow typography. The
//! status of each is in `docs/verification/p38a1-svg-renderer.md`.

mod font;
mod geometry;
mod label;
mod losses;
mod writer;

use std::collections::BTreeMap;

use serde::Serialize;

use crate::{DocumentView, SceneObjectView};

pub use font::ADVANCE_TABLE_ID;
pub use geometry::{LabelBox, Pattern, Point, Stroke};
pub use label::{AtomLabel, LabelRun};
pub(crate) use losses::svg_losses;

/// The version of the drawing's markup and geometry. A change to either changes it.
pub const SVG_MODEL: &str = "chemifree-svg-1";

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

/// The layout of a whole drawing: the label measurements it uses, and each instance in the order the
/// view lists them.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DrawingLayout {
    pub table: &'static str,
    pub instances: Vec<InstanceLayout>,
}

/// Where one instance's atoms are before the layout.
struct Placement {
    id: String,
    given: BTreeMap<String, Point>,
}

/// The placements of `view` in the order the web draws them (`structure-canvas.tsx`): the scene's
/// instances sorted by the position of their objects in `scene.objects`, which is the drawing order.
/// An instance without an object comes first, as `indexOf` gives it -1 in the web. Each placement
/// keeps its own positions. A document without an instance has no placement, so nothing is drawn.
fn placements(view: &DocumentView) -> Vec<Placement> {
    let order: Vec<&str> = view
        .scene
        .objects
        .iter()
        .filter_map(|object| match object {
            SceneObjectView::Instance { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
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
    let instances = placements(view)
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
    DrawingLayout {
        table: ADVANCE_TABLE_ID,
        instances,
    }
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
/// of a bond's ink, and of every label box grown by `LABEL_MARGIN`, each edge moved outward to the
/// hundredth. A drawing without atoms has the empty box at the origin.
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
}
