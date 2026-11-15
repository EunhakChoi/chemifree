//! The SMILES-specific parts of the placement of a read's components: the commands that make a
//! read's graph and the reads of its components. The placement shared with the other formats,
//! including the chain and simple ring layout of `chemifree_domain::layout` (CONTRACT, P48 S1), is
//! in `import_layout`.

use chemifree_domain::document::{AtomRef, Command, NewAtomPositions, Point};
use chemifree_domain::settings::Settings;

use super::import_layout::{self, ComponentPlan};
use super::smiles;
use crate::smiles::{SmilesBond, SmilesRead, SmilesSlot, SmilesStereo};

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
    let grid = import_layout::grid_shape(read.atoms.len());
    let components = import_layout::components(
        read.atoms.len(),
        read.bonds.iter().map(|bond| (bond.begin, bond.end)),
    );
    let parts = component_reads(read, &components);
    components
        .into_iter()
        .zip(parts)
        .map(|(atoms, part)| {
            let positions: Vec<(f64, f64)> = atoms.iter().map(|&atom| grid[atom]).collect();
            let PlacedCommands {
                mut commands,
                first_bond,
            } = graph_commands(&part, positions).ok()?;
            commands.extend(smiles::configuration_commands(&part, first_bond));
            import_layout::plan(commands, &settings, atoms)
        })
        .collect()
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
