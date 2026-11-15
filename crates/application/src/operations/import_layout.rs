//! The format-neutral placement of the components of a coordinate-free import (CONTRACT, P48 S1
//! and S2): the grid, the components of a graph, the layout of one component on a scratch document
//! that is never stored, the shelf packing, the origin below the existing drawing and the
//! `layout_not_covered` losses.

use std::collections::BTreeSet;

use chemifree_domain::document::{AtomId, Command, Document, DocumentId};
use chemifree_domain::layout::{self, Scope};
use chemifree_domain::scene::Selection;
use chemifree_domain::settings::Settings;

use super::{Loss, LossConsent, LossDisposition, SemanticImpact};
use crate::contract::{DocumentView, PositionView, SceneObjectView};

/// The grid of the components the layout does not cover, and of a whole import whose layout could
/// not be computed: 10 columns, 30 pt apart, in input order.
const GRID_SPACING: f64 = 30.0;
const GRID_COLUMNS: usize = 10;

/// New atoms go below whatever the drawing already holds, from its leftmost point; an empty drawing
/// starts them at (40, 40).
const GAP_BELOW_DRAWING: f64 = 60.0;
const EMPTY_DRAWING_ORIGIN: f64 = 40.0;

/// Grid positions in input order: the k-th of `count` items (k from 0) sits in column
/// `k % GRID_COLUMNS` and row `k / GRID_COLUMNS`, each step `GRID_SPACING` pt apart.
pub(super) fn grid_shape(count: usize) -> Vec<(f64, f64)> {
    (0..count)
        .map(|k| {
            (
                (k % GRID_COLUMNS) as f64 * GRID_SPACING,
                (k / GRID_COLUMNS) as f64 * GRID_SPACING,
            )
        })
        .collect()
}

/// Why a component is not laid out.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Fallback {
    /// The layout refused the component with this `LayoutError::reason()` code; it stays on the
    /// grid.
    Grid(&'static str),
    /// The component is not given to the layout, for this code; its atoms keep their one shared
    /// source point.
    SourcePoint(&'static str),
}

/// How one read component is placed: its atoms, their local shape and why the layout does not
/// cover it, if it does not.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ComponentPlan {
    /// The 0-based input indices of its atoms, ascending.
    pub(super) atoms: Vec<usize>,
    /// The local position of each atom, in the order of `atoms`; only the shape is used.
    pub(super) shape: Vec<(f64, f64)>,
    /// Why the component is not laid out, as a `Fallback`; `None` when laid out.
    pub(super) fallback: Option<Fallback>,
}

/// The connected components of a graph of `atom_count` atoms joined by `edges`, each as its
/// ascending 0-based indices, ordered by their smallest index. Every edge endpoint is below
/// `atom_count`; the caller checks it.
pub(super) fn components(
    atom_count: usize,
    edges: impl Iterator<Item = (usize, usize)>,
) -> Vec<Vec<usize>> {
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); atom_count];
    for (begin, end) in edges {
        neighbors[begin].push(end);
        neighbors[end].push(begin);
    }
    let mut visited = vec![false; atom_count];
    let mut found: Vec<Vec<usize>> = Vec::new();
    for start in 0..atom_count {
        if visited[start] {
            continue;
        }
        // Every earlier component is explored in full, so `start` is the smallest atom of the
        // component it begins.
        visited[start] = true;
        let mut atoms = Vec::new();
        let mut pending = vec![start];
        while let Some(atom) = pending.pop() {
            atoms.push(atom);
            for &next in &neighbors[atom] {
                if !visited[next] {
                    visited[next] = true;
                    pending.push(next);
                }
            }
        }
        atoms.sort_unstable();
        found.push(atoms);
    }
    found
}

/// The placement of one component, laid out on a scratch document that `commands` build: its
/// atoms (the k-th `AddAtom` is its k-th atom, at its grid cell), then its bonds and any
/// configurations. The P42 `after` positions of its atoms when the layout covers it, its local
/// grid otherwise. `None` when the scratch document cannot be built or applied, or the layout does
/// not return one position per atom.
pub(super) fn plan(
    commands: Vec<Command>,
    settings: &Settings,
    atoms: Vec<usize>,
) -> Option<ComponentPlan> {
    // The scratch id is `import-layout`; the layout never reads the document id.
    let id = DocumentId::parse("import-layout").expect("import-layout is a valid document id");
    let mut scratch = Document::new_with_settings(id, "layout", settings.clone()).ok()?;
    scratch.apply(&commands).ok()?;
    // The k-th atom of the component is the k-th atom of its scratch document: identifiers are
    // fresh in command order, so they ascend with the input indices, as in the whole read.
    let ids: Vec<AtomId> = scratch.atoms().map(|(id, _)| id).collect();
    if ids.len() != atoms.len() {
        debug_assert!(
            false,
            "the scratch document holds one atom per component atom"
        );
        return None;
    }
    let instance = scratch.first_instance();
    let selection = Selection {
        atoms: BTreeSet::from([ids[0]]),
        ..Selection::default()
    };
    let prepared = layout::prepare(&scratch, instance, &selection, Scope::Components)
        .and_then(|prep| layout::place(&prep));
    match prepared {
        Ok(preview) => {
            // The layout set is this component alone, and its positions ascend by atom, which is
            // ascending input order.
            let shape: Vec<(f64, f64)> = preview
                .positions
                .iter()
                .map(|placement| (placement.after.x(), placement.after.y()))
                .collect();
            if shape.len() != atoms.len() {
                debug_assert!(false, "a laid-out component has one position per atom");
                return None;
            }
            Some(ComponentPlan {
                atoms,
                shape,
                fallback: None,
            })
        }
        Err(error) => {
            let shape = grid_shape(atoms.len());
            Some(ComponentPlan {
                atoms,
                shape,
                fallback: Some(Fallback::Grid(error.reason())),
            })
        }
    }
}

/// The position of every input atom, indexed by its input index, with the first box at `origin`:
/// the components of `plans` packed by [`pack`] or, when the layout could not be computed (`None`),
/// the whole import as one box on the grid.
pub(super) fn positions(
    plans: Option<&[ComponentPlan]>,
    atom_count: usize,
    origin: (f64, f64),
    bond_length_pt: f64,
) -> Vec<(f64, f64)> {
    match plans {
        Some(plans) => pack(plans, atom_count, origin, bond_length_pt),
        None => {
            // One box of every atom on the grid. Its top-left corner is the grid's first cell
            // (0, 0), so each atom lands at `origin` plus its grid cell, bit for bit.
            let whole = ComponentPlan {
                atoms: (0..atom_count).collect(),
                shape: grid_shape(atom_count),
                fallback: None,
            };
            pack(
                std::slice::from_ref(&whole),
                atom_count,
                origin,
                bond_length_pt,
            )
        }
    }
}

/// Packs the components in shelf rows, one bounding box each, and returns the position of every
/// input atom, indexed by its input index. A box's top-left corner is its position: boxes in a row
/// are `2 * bond_length_pt` apart, and a box goes into the current row only while it ends within
/// `20 * bond_length_pt` of `origin`'s x; the first box of a row is always placed. Overlaps between
/// boxes are not checked.
fn pack(
    plans: &[ComponentPlan],
    atom_count: usize,
    origin: (f64, f64),
    bond_length_pt: f64,
) -> Vec<(f64, f64)> {
    let gap = 2.0 * bond_length_pt;
    let row_width = 20.0 * bond_length_pt;
    let (x0, y0) = origin;
    let mut placed = vec![(x0, y0); atom_count];
    let (mut cursor, mut row_top, mut row_height) = (x0, y0, 0.0_f64);
    let mut row_has_box = false;
    for plan in plans {
        debug_assert_eq!(
            plan.atoms.len(),
            plan.shape.len(),
            "one local position per atom"
        );
        let Some((&(first_x, first_y), rest)) = plan.shape.split_first() else {
            continue;
        };
        let (min_x, min_y, max_x, max_y) = rest.iter().fold(
            (first_x, first_y, first_x, first_y),
            |(low_x, low_y, high_x, high_y), &(x, y)| {
                (low_x.min(x), low_y.min(y), high_x.max(x), high_y.max(y))
            },
        );
        let (w, h) = (max_x - min_x, max_y - min_y);
        if row_has_box && cursor + w > x0 + row_width {
            row_top += row_height + gap;
            cursor = x0;
            row_height = 0.0;
        }
        for (&atom, &(x, y)) in plan.atoms.iter().zip(&plan.shape) {
            placed[atom] = (x - min_x + cursor, y - min_y + row_top);
        }
        cursor += w + gap;
        row_height = row_height.max(h);
        row_has_box = true;
    }
    placed
}

/// The `layout_not_covered` losses of the components the layout does not cover, in layout order:
/// those kept on the grid and those kept at their source point. A component the layout covers has
/// none. `path_prefix` goes right before `atoms[...]`, so it ends with a dot when not empty (SMILES
/// passes `""`, MOL passes `records[N].`).
pub(super) fn not_covered_losses(plans: &[ComponentPlan], path_prefix: &str) -> Vec<Loss> {
    let mut losses = Vec::new();
    for plan in plans {
        let Some(fallback) = plan.fallback else {
            continue;
        };
        let atoms: Vec<String> = plan
            .atoms
            .iter()
            .map(|atom| (atom + 1).to_string())
            .collect();
        let reason = match fallback {
            Fallback::Grid(code) => format!(
                "The chain and simple ring layout ({}) does not cover this component ({code}): its atoms are placed on a grid in input order.",
                layout::MODEL_ID
            ),
            Fallback::SourcePoint(code) => format!(
                "The chain and simple ring layout ({}) does not cover this component ({code}): its atoms keep their source coordinates, one shared point, so its bond marks show no configuration the file does not state.",
                layout::MODEL_ID
            ),
        };
        losses.push(Loss {
            feature: "layout_not_covered".into(),
            source_path: format!("{path_prefix}atoms[{}]", atoms.join(", ")),
            disposition: LossDisposition::Normalized,
            reason,
            semantic_impact: SemanticImpact::Geometry,
            consent: LossConsent::NotRequired,
        });
    }
    losses
}

/// The top-left corner of the first box of an import into `target`: the leftmost x of the drawing,
/// and 60 pt below its largest y, over its atoms, graphic points, image corners and caption
/// anchors. An empty drawing starts at (40, 40).
pub(super) fn origin(target: &DocumentView) -> (f64, f64) {
    // Everything the drawing holds that has a position: atoms and the points of graphics.
    let graphic_points = target.scene.objects.iter().flat_map(|object| match object {
        SceneObjectView::Graphic { points, .. } => points.clone(),
        SceneObjectView::Image {
            origin, size_pt, ..
        } => vec![
            *origin,
            PositionView {
                x: origin.x + size_pt.x,
                y: origin.y + size_pt.y,
            },
        ],
        SceneObjectView::Caption { anchor, .. } => vec![*anchor],
        SceneObjectView::Instance { .. } | SceneObjectView::Group { .. } => Vec::new(),
    });
    target
        .atoms
        .iter()
        .map(|atom| atom.position)
        .chain(graphic_points)
        .fold(None, |extent: Option<(f64, f64)>, position| {
            Some(match extent {
                Some((left, bottom)) => (left.min(position.x), bottom.max(position.y)),
                None => (position.x, position.y),
            })
        })
        .map_or(
            (EMPTY_DRAWING_ORIGIN, EMPTY_DRAWING_ORIGIN),
            |(left, bottom)| (left, bottom + GAP_BELOW_DRAWING),
        )
}
