//! Placement of the components of a coordinate-free SMILES read. A component takes the positions of
//! the chain and simple ring layout of `chemifree_domain::layout` when that layout covers it, and
//! stays on the grid otherwise. The layout runs on one scratch document per component, which is
//! never stored (CONTRACT, P48 S1).

use std::collections::BTreeSet;

use chemifree_domain::document::{
    AtomId, AtomRef, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::layout::{self, Scope};
use chemifree_domain::scene::Selection;
use chemifree_domain::settings::Settings;

use super::smiles;
use super::{Loss, LossConsent, LossDisposition, SemanticImpact};
use crate::smiles::{SmilesBond, SmilesRead, SmilesSlot, SmilesStereo};

/// The grid of the components the layout does not cover, and of a whole import whose layout could
/// not be computed: 10 columns, 30 pt apart, in input order.
const GRID_SPACING: f64 = 30.0;
const GRID_COLUMNS: usize = 10;

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
fn grid_shape(count: usize) -> Vec<(f64, f64)> {
    (0..count)
        .map(|k| {
            (
                (k % GRID_COLUMNS) as f64 * GRID_SPACING,
                (k / GRID_COLUMNS) as f64 * GRID_SPACING,
            )
        })
        .collect()
}

/// The commands that make the graph of a read, and where its bonds start among them: a created
/// bond is named by the index of its command, so `first_bond + n` names the n-th bond of the read.
pub(super) struct PlacedCommands {
    pub(super) commands: Vec<Command>,
    pub(super) first_bond: usize,
}

/// The commands that add the atoms of `read`, the k-th at the k-th of `positions`, then its bonds,
/// both in input order. `Err(k)` names the 0-based input index of the first atom whose position
/// is out of range.
pub(super) fn graph_commands(
    read: &SmilesRead,
    positions: impl IntoIterator<Item = (f64, f64)>,
) -> Result<PlacedCommands, usize> {
    let mut commands = Vec::with_capacity(read.atoms.len() + read.bonds.len());
    for (index, (source, (x, y))) in read.atoms.iter().zip(positions).enumerate() {
        let point = Point::new(x, y).map_err(|_| index)?;
        commands.push(Command::AddAtom {
            atom: source.atom.clone(),
            positions: NewAtomPositions::Single(point),
            bond_to: None,
        });
    }
    let first_bond = commands.len();
    for bond in &read.bonds {
        commands.push(Command::AddBond {
            begin: AtomRef::Created(bond.begin),
            end: AtomRef::Created(bond.end),
            order: bond.order.clone(),
        });
    }
    Ok(PlacedCommands {
        commands,
        first_bond,
    })
}

/// The placement of each component of `read`, in the order of `components`, for a document
/// whose bonds are `bond_length_pt` long. Each component is laid out on a scratch document of its
/// own, which is never stored, with its atoms at the grid cells they have in the whole read: a
/// free component is laid out around the centroid of those cells, so they, and nothing outside
/// the component, decide its plan. `None` when a scratch document cannot be built or applied;
/// the whole import then stays on the grid.
pub(super) fn placements(read: &SmilesRead, bond_length_pt: f64) -> Option<Vec<ComponentPlan>> {
    let settings = Settings {
        bond_length_pt,
        ..Settings::default()
    };
    let grid = grid_shape(read.atoms.len());
    let components = components(read);
    let parts = component_reads(read, &components);
    components
        .into_iter()
        .zip(parts)
        .map(|(atoms, part)| {
            let positions: Vec<(f64, f64)> = atoms.iter().map(|&atom| grid[atom]).collect();
            plan(&part, positions, &settings, atoms)
        })
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

/// The read of each component on its own, in the order of `components`: its atoms, its bonds and
/// its configurations, in the read's order, with indices local to the component (its k-th atom is
/// its k-th smallest input index). One pass over the read, so the work is proportional to it.
fn component_reads(read: &SmilesRead, components: &[Vec<usize>]) -> Vec<SmilesRead> {
    let mut owner = vec![0; read.atoms.len()];
    let mut local = vec![0; read.atoms.len()];
    for (component, atoms) in components.iter().enumerate() {
        for (k, &atom) in atoms.iter().enumerate() {
            owner[atom] = component;
            local[atom] = k;
        }
    }
    let mut parts: Vec<SmilesRead> = components
        .iter()
        .map(|atoms| SmilesRead {
            atoms: atoms.iter().map(|&atom| read.atoms[atom].clone()).collect(),
            bonds: Vec::new(),
            ignored_text: None,
            losses: Vec::new(),
            stereo: SmilesStereo::default(),
        })
        .collect();
    let mut local_bond = vec![0; read.bonds.len()];
    for (index, bond) in read.bonds.iter().enumerate() {
        let part = &mut parts[owner[bond.begin]];
        local_bond[index] = part.bonds.len();
        part.bonds.push(SmilesBond {
            begin: local[bond.begin],
            end: local[bond.end],
            order: bond.order.clone(),
        });
    }
    for center in &read.stereo.centers {
        let mut moved = center.clone();
        moved.atom = local[center.atom];
        moved.slots = center.slots.map(|slot| match slot {
            SmilesSlot::Atom(atom) => SmilesSlot::Atom(local[atom]),
            SmilesSlot::ImplicitHydrogen => SmilesSlot::ImplicitHydrogen,
        });
        parts[owner[center.atom]].stereo.centers.push(moved);
    }
    for double_bond in &read.stereo.double_bonds {
        let mut moved = double_bond.clone();
        moved.bond = local_bond[double_bond.bond];
        moved.begin_neighbor = local[double_bond.begin_neighbor];
        moved.end_neighbor = local[double_bond.end_neighbor];
        parts[owner[read.bonds[double_bond.bond].begin]]
            .stereo
            .double_bonds
            .push(moved);
    }
    parts
}

/// The placement of one component, laid out on a scratch document that holds `part` (the
/// component's read, with local indices) with its k-th atom at the k-th of `positions`: the P42
/// `after` positions of its atoms when the layout covers it, its local grid otherwise. `None` when
/// the scratch document cannot be built or applied, or the layout does not return one position
/// per atom.
fn plan(
    part: &SmilesRead,
    positions: Vec<(f64, f64)>,
    settings: &Settings,
    atoms: Vec<usize>,
) -> Option<ComponentPlan> {
    let id = DocumentId::parse("smiles-layout").expect("smiles-layout is a valid document id");
    let mut scratch = Document::new_with_settings(id, "layout", settings.clone()).ok()?;
    let PlacedCommands {
        mut commands,
        first_bond,
    } = graph_commands(part, positions).ok()?;
    commands.extend(smiles::configuration_commands(part, first_bond));
    scratch.apply(&commands).ok()?;
    // The k-th atom of the part is the k-th atom of its scratch document: identifiers are fresh
    // in command order, so they ascend with the input indices, as in the whole read.
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
                fallback: Some(error.reason()),
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
