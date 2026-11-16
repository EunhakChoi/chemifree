//! Whether a MOL/SDF record states no coordinates, and the placement of its components when it does
//! not (CONTRACT, P48 S2).

use chemifree_domain::document::{Atom, AtomRef, BondOrder, Command, NewAtomPositions, Point};
use chemifree_domain::settings::Settings;

use super::import_layout::{self, ComponentPlan, Fallback};
use super::{Loss, LossConsent, LossDisposition, SemanticImpact};
use crate::ct_projection;
use crate::ctfile::{CtBondStereo, CtRecord};

/// Whether `record` states no coordinates (CONTRACT, P48 S2, M1-M5). The record needs a 2D header,
/// at least one atom, and every atom at the origin. The dimensional code is bytes 21-22 of header
/// line 2 (the CTfile field after the initials, program and date/time), matched exactly and
/// case-sensitively. A V3000 record has the same header line. `-0.0` counts as zero, and a single
/// atom at the origin counts.
pub(super) fn coordinate_free(record: &CtRecord) -> bool {
    record.program.as_bytes().get(20..22) == Some(b"2D".as_slice())
        && !record.atoms.is_empty()
        && record
            .atoms
            .iter()
            .all(|atom| atom.x == 0.0 && atom.y == 0.0 && atom.z == 0.0)
}

/// The placements of the components of a coordinate-free record, in the order of
/// `import_layout::components`, for a document whose bonds are `bond_length_pt` long (CONTRACT,
/// P48 S2, M10-M12). A component with a bond mark keeps its source shape, one point, and is never
/// given to the engine. Any other component is planned on its own scratch document, with its atoms
/// at their grid cells in the whole record. `None` when a bond endpoint is not below the
/// atom count, an atom or bond order does not convert, or a component has no plan. The record
/// then keeps its source coordinates, and the import reports any refusal itself.
pub(super) fn placements(record: &CtRecord, bond_length_pt: f64) -> Option<Vec<ComponentPlan>> {
    let count = record.atoms.len();
    if record
        .bonds
        .iter()
        .any(|bond| bond.begin >= count || bond.end >= count)
    {
        return None;
    }
    // The projection refuses an atom or an order that does not convert, naming the record. Here
    // such a record is only left unlaid-out, so the index of the discarded error is not used.
    let atoms: Vec<Atom> = record
        .atoms
        .iter()
        .map(|source| ct_projection::ct_atom(source, 0).ok())
        .collect::<Option<_>>()?;
    let orders: Vec<BondOrder> = record
        .bonds
        .iter()
        .map(|bond| ct_projection::ct_bond_order(bond.order))
        .collect::<Option<_>>()?;
    let settings = Settings {
        bond_length_pt,
        ..Settings::default()
    };
    let grid = import_layout::grid_shape(count);
    let components = import_layout::components(
        count,
        record.bonds.iter().map(|bond| (bond.begin, bond.end)),
    );
    // The component of each atom, and the atom's index within that component.
    let mut owner = vec![0; count];
    let mut local = vec![0; count];
    for (component, members) in components.iter().enumerate() {
        for (k, &atom) in members.iter().enumerate() {
            owner[atom] = component;
            local[atom] = k;
        }
    }
    // The record bonds of each component, in record order.
    let mut component_bonds: Vec<Vec<usize>> = vec![Vec::new(); components.len()];
    for (index, bond) in record.bonds.iter().enumerate() {
        component_bonds[owner[bond.begin]].push(index);
    }
    components
        .into_iter()
        .zip(component_bonds)
        .map(|(members, bonds)| {
            if bonds
                .iter()
                .any(|&index| record.bonds[index].stereo != CtBondStereo::None)
            {
                // A component with a bond mark is never given to the engine: grid or zigzag
                // geometry would let the marks derive a configuration the file does not state.
                // Every atom keeps the one source point. It is not checked for overlaps: its atoms
                // share one point on purpose (CONTRACT, P48 S4).
                return Some(ComponentPlan {
                    shape: vec![(0.0, 0.0); members.len()],
                    atoms: members,
                    fallback: Some(Fallback::SourcePoint("stereo_in_layout")),
                    overlaps: Vec::new(),
                });
            }
            let mut commands = Vec::with_capacity(members.len() + bonds.len());
            for &atom in &members {
                let (x, y) = grid[atom];
                commands.push(Command::AddAtom {
                    atom: atoms[atom].clone(),
                    positions: NewAtomPositions::Single(Point::new(x, y).ok()?),
                    bond_to: None,
                });
            }
            for &index in &bonds {
                let bond = &record.bonds[index];
                commands.push(Command::AddBond {
                    begin: AtomRef::Created(local[bond.begin]),
                    end: AtomRef::Created(local[bond.end]),
                    order: orders[index].clone(),
                });
            }
            import_layout::plan(commands, &settings, members)
        })
        .collect()
}

/// The loss that reports the generated layout of the coordinate-free record `record_index`, which
/// is 1-based (CONTRACT, P48 S2, M15).
pub(super) fn generated_layout(record_index: usize) -> Loss {
    Loss {
        consent: LossConsent::NotRequired,
        feature: "generated_layout".into(),
        source_path: format!("records[{record_index}].atoms"),
        disposition: LossDisposition::Normalized,
        reason: "The record's header declares 2D and every coordinate is 0, so it states no layout: atoms are placed by a generated layout below the existing drawing, not taken from the source.".into(),
        semantic_impact: SemanticImpact::Geometry,
    }
}
