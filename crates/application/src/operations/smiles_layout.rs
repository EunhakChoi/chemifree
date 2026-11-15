//! Placement of the components of a coordinate-free SMILES read. A component takes the positions of
//! the chain and simple ring layout of `chemifree_domain::layout` when that layout covers it, and
//! stays on the grid otherwise. The layout runs on a scratch document that is never stored
//! (CONTRACT, P48 S1).

use std::collections::BTreeSet;

use chemifree_domain::document::{
    AtomId, AtomRef, Command, Document, DocumentId, NewAtomPositions, ObjectId, Point,
};
use chemifree_domain::layout::{self, Scope};
use chemifree_domain::scene::Selection;
use chemifree_domain::settings::Settings;

use super::smiles;
use super::{Loss, LossConsent, LossDisposition, SemanticImpact};
use crate::smiles::SmilesRead;

/// How one read component is placed: its atoms, their local shape and why the layout does not
/// cover it, if it does not.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ComponentPlan {
    /// The 0-based input indices of its atoms, ascending.
    pub(super) atoms: Vec<usize>,
    /// The local position of each atom, in the order of `atoms`; only the shape is used.
    pub(super) shape: Vec<(f64, f64)>,
    /// The `LayoutError::reason()` code when the component stays on the grid; `None` when laid
    /// out.
    pub(super) fallback: Option<&'static str>,
}

/// Grid positions in input order: the k-th of `count` items (k from 0) sits in column
/// `k % GRID_COLUMNS` and row `k / GRID_COLUMNS`, each step `GRID_SPACING` pt apart.
pub(super) fn grid_shape(count: usize) -> Vec<(f64, f64)> {
    (0..count)
        .map(|k| {
            (
                (k % smiles::GRID_COLUMNS) as f64 * smiles::GRID_SPACING,
                (k / smiles::GRID_COLUMNS) as f64 * smiles::GRID_SPACING,
            )
        })
        .collect()
}

/// The placement of each component of `read`, in the order of `components`, for a document
/// whose bonds are `bond_length_pt` long. `None` when the scratch document cannot be built or
/// applied; the whole import then stays on the grid.
pub(super) fn placements(read: &SmilesRead, bond_length_pt: f64) -> Option<Vec<ComponentPlan>> {
    let id = DocumentId::parse("smiles-layout").expect("smiles-layout is a valid document id");
    let settings = Settings {
        bond_length_pt,
        ..Settings::default()
    };
    let mut scratch = Document::new_with_settings(id, "layout", settings).ok()?;
    let mut commands = Vec::with_capacity(read.atoms.len() + read.bonds.len());
    for (source, (x, y)) in read.atoms.iter().zip(grid_shape(read.atoms.len())) {
        commands.push(Command::AddAtom {
            atom: source.atom.clone(),
            positions: NewAtomPositions::Single(Point::new(x, y).ok()?),
            bond_to: None,
        });
    }
    for bond in &read.bonds {
        commands.push(Command::AddBond {
            begin: AtomRef::Created(bond.begin),
            end: AtomRef::Created(bond.end),
            order: bond.order.clone(),
        });
    }
    commands.extend(smiles::configuration_commands(read, read.atoms.len()));
    scratch.apply(&commands).ok()?;
    // The k-th atom of the read is the k-th atom of the scratch document: identifiers are fresh
    // in command order.
    let ids: Vec<AtomId> = scratch.atoms().map(|(id, _)| id).collect();
    if ids.len() != read.atoms.len() {
        debug_assert!(false, "the scratch document holds one atom per read atom");
        return None;
    }
    let instance = scratch.first_instance();
    components(read)
        .into_iter()
        .map(|atoms| plan(&scratch, instance, &ids, atoms))
        .collect()
}

/// The connected components of the read graph, each as its ascending input indices, ordered by
/// their smallest index.
fn components(read: &SmilesRead) -> Vec<Vec<usize>> {
    // Bond endpoints index `read.atoms`, as the reader builds them.
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); read.atoms.len()];
    for bond in &read.bonds {
        neighbors[bond.begin].push(bond.end);
        neighbors[bond.end].push(bond.begin);
    }
    let mut visited = vec![false; read.atoms.len()];
    let mut found: Vec<Vec<usize>> = Vec::new();
    for start in 0..read.atoms.len() {
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

/// The placement of one component: the P42 `after` positions of its atoms when the layout covers
/// it, its local grid otherwise. `None` when the layout does not return one position per atom.
fn plan(
    scratch: &Document,
    instance: ObjectId,
    ids: &[AtomId],
    atoms: Vec<usize>,
) -> Option<ComponentPlan> {
    let selection = Selection {
        atoms: BTreeSet::from([ids[atoms[0]]]),
        ..Selection::default()
    };
    let prepared = layout::prepare(scratch, instance, &selection, Scope::Components)
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
                fallback: Some(error.reason()),
            })
        }
    }
}

/// Packs the components in shelf rows, one bounding box each, and returns the position of every
/// input atom, indexed by its input index. A box's top-left corner is its position: boxes in a row
/// are `2 * bond_length_pt` apart, and a box goes into the current row only while it ends within
/// `20 * bond_length_pt` of `origin`'s x; the first box of a row is always placed. Overlaps between
/// boxes are not checked.
pub(super) fn pack(
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

/// The `layout_not_covered` losses of the components that stay on the grid, in layout order. A
/// component the layout covers has none.
pub(super) fn not_covered_losses(plans: &[ComponentPlan]) -> Vec<Loss> {
    let mut losses = Vec::new();
    for plan in plans {
        let Some(code) = plan.fallback else {
            continue;
        };
        let atoms: Vec<String> = plan
            .atoms
            .iter()
            .map(|atom| (atom + 1).to_string())
            .collect();
        losses.push(Loss {
            feature: "layout_not_covered".into(),
            source_path: format!("atoms[{}]", atoms.join(", ")),
            disposition: LossDisposition::Normalized,
            reason: format!(
                "The chain and simple ring layout ({}) does not cover this component ({code}): its atoms are placed on a grid in input order.",
                layout::MODEL_ID
            ),
            semantic_impact: SemanticImpact::Geometry,
            consent: LossConsent::NotRequired,
        });
    }
    losses
}
