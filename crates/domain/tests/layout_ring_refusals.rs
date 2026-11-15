//! Simple ring refusals of the chain and ring layout preview, through the public API. A ring
//! system with a layout atom is laid out only when it is one cycle of 3 to 12 atoms that lies
//! wholly in the layout set and shares no atom with another ring system. Otherwise `prepare`
//! refuses with the first failing check, in this order: a member outside the layout set
//! (partly), more than one ring (fused or bridged), a shared atom (spiro), and more than 12 atoms
//! (large). A ring system with no layout atom is never refused.

use std::collections::BTreeSet;
use std::error::Error;
use std::f64::consts::{PI, TAU};
use std::ops::RangeInclusive;

use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondId, BondOrder, BondRef, Command, Document, DocumentId,
    NewAtomPositions, Point, RingPattern, RingSize,
};
use chemifree_domain::element::Element;
use chemifree_domain::layout::{self, Anchor, LayoutError, Preparation, Scope};
use chemifree_domain::scene::Selection;
use chemifree_domain::stereo::{BondEnd, StereoMark};

/// A test returns this, so that a setup failure is reported with its error and `?` can be used.
type TestResult = Result<(), Box<dyn Error>>;

/// The atom with the given serial. A document numbers its atoms from 1.
fn atom(serial: u32) -> Result<AtomId, Box<dyn Error>> {
    AtomId::from_serial(serial).ok_or_else(|| format!("serial {serial} is not an atom id").into())
}

/// The atoms with the serials in `serials`, ascending.
fn serial_atoms(serials: RangeInclusive<u32>) -> Result<Vec<AtomId>, Box<dyn Error>> {
    serials.map(atom).collect()
}

/// The atoms as a set, so that the order of a ring's walk does not matter.
fn set_of(atoms: &[AtomId]) -> BTreeSet<AtomId> {
    atoms.iter().copied().collect()
}

/// The bond that joins `first` and `second`, in either direction.
fn bond_between(
    document: &Document,
    first: AtomId,
    second: AtomId,
) -> Result<BondId, Box<dyn Error>> {
    document
        .bonds()
        .find(|(_, bond)| {
            let ends = (bond.begin, bond.end);
            ends == (first, second) || ends == (second, first)
        })
        .map(|(id, _)| id)
        .ok_or_else(|| format!("no bond joins {first:?} and {second:?}").into())
}

/// A new, empty document.
fn new_document() -> Result<Document, Box<dyn Error>> {
    let id = DocumentId::parse("ring-refusals").ok_or("the document id is not valid")?;
    Ok(Document::new(id, "Ring refusals")?)
}

/// Prepares the atoms `selected` of the first instance of `document` as one selection.
fn prepare_selection(
    document: &Document,
    selected: &[AtomId],
    scope: Scope,
) -> Result<Preparation, LayoutError> {
    let selection = Selection {
        atoms: selected.iter().copied().collect(),
        ..Selection::default()
    };
    layout::prepare(document, document.first_instance(), &selection, scope)
}

/// `count` points in a row, 40 pt apart. A refusal never reads these places.
fn row(count: usize) -> Result<Vec<Point>, Box<dyn Error>> {
    let mut points = Vec::with_capacity(count);
    for index in 0..count {
        points.push(Point::new(40.0 * f64::from(u32::try_from(index)?), 0.0)?);
    }
    Ok(points)
}

/// The corners of a regular polygon of `count` atoms with 30 pt sides, centred at the origin and
/// listed in cycle order.
fn polygon(count: usize) -> Result<Vec<Point>, Box<dyn Error>> {
    let sides = f64::from(u32::try_from(count)?);
    let radius = 30.0 / (2.0 * (PI / sides).sin());
    let mut points = Vec::with_capacity(count);
    for index in 0..count {
        let angle = TAU * f64::from(u32::try_from(index)?) / sides;
        points.push(Point::new(radius * angle.cos(), radius * angle.sin())?);
    }
    Ok(points)
}

/// Carbons at `points`, numbered from atom 1 in order, joined by one single bond for each pair of
/// zero-based indices in `bonds`.
fn carbon_graph(points: &[Point], bonds: &[(usize, usize)]) -> Result<Document, Box<dyn Error>> {
    let mut document = new_document()?;
    let mut commands: Vec<Command> = points
        .iter()
        .map(|&point| Command::AddAtom {
            atom: Atom::from(Element::CARBON),
            positions: NewAtomPositions::Single(point),
            bond_to: None,
        })
        .collect();
    commands.extend(bonds.iter().map(|&(begin, end)| Command::AddBond {
        begin: AtomRef::Created(begin),
        end: AtomRef::Created(end),
        order: BondOrder::Single,
    }));
    document.apply(&commands)?;
    Ok(document)
}

/// The bonds that close a cycle through `members` in order: each member is joined to the next,
/// and the last to the first. Members are zero-based indices.
fn cycle(members: &[usize]) -> Vec<(usize, usize)> {
    members
        .iter()
        .zip(members.iter().cycle().skip(1))
        .map(|(&from, &to)| (from, to))
        .collect()
}

/// The bonds of a ring of `count` atoms, numbered from zero.
fn ring_bonds(count: usize) -> Vec<(usize, usize)> {
    let members: Vec<usize> = (0..count).collect();
    cycle(&members)
}

/// The bonds of `bonds` with every index moved on by `offset`.
fn shifted(bonds: &[(usize, usize)], offset: usize) -> Vec<(usize, usize)> {
    bonds
        .iter()
        .map(|&(from, to)| (from + offset, to + offset))
        .collect()
}

/// The bonds of naphthalene on indices 0 to 9, which are atoms 1 to 10: the rings 0-1-2-3-4-5 and
/// 4-6-7-8-9-5 share the bond 4-5.
fn naphthalene_bonds() -> Vec<(usize, usize)> {
    vec![
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 4),
        (4, 5),
        (5, 0),
        (4, 6),
        (6, 7),
        (7, 8),
        (8, 9),
        (9, 5),
    ]
}

/// A regular hexagon of atoms 1 to 6, drawn by the ring command.
fn hexagon() -> Result<Document, Box<dyn Error>> {
    let mut document = new_document()?;
    let instance = document.first_instance();
    document.apply(&[Command::InsertRing {
        instance,
        center: Point::new(0.0, 0.0)?,
        size: RingSize::Six,
        pattern: RingPattern::Single,
    }])?;
    Ok(document)
}

/// Biphenyl: a hexagon on atoms 1 to 6 and a hexagon on atoms 7 to 12, joined by the bond 6-10.
/// Atom 10 is vertex 3 of the second ring, 30 pt from atom 6.
fn biphenyl() -> Result<Document, Box<dyn Error>> {
    let mut document = hexagon()?;
    let instance = document.first_instance();
    document.apply(&[Command::InsertRing {
        instance,
        center: Point::new(75.0, -15.0 * 3f64.sqrt())?,
        size: RingSize::Six,
        pattern: RingPattern::Single,
    }])?;
    document.apply(&[Command::AddBond {
        begin: AtomRef::Id(atom(6)?),
        end: AtomRef::Id(atom(10)?),
        order: BondOrder::Single,
    }])?;
    Ok(document)
}

/// Naphthalene on atoms 1 to 10.
fn naphthalene() -> Result<Document, Box<dyn Error>> {
    carbon_graph(&row(10)?, &naphthalene_bonds())
}

/// Naphthalene with a cyclopentane on atom 1, which is not a fusion atom, so the two share one
/// atom only.
fn naphthalene_with_spiro_cyclopentane() -> Result<Document, Box<dyn Error>> {
    let mut bonds = naphthalene_bonds();
    bonds.extend(cycle(&[0, 10, 11, 12, 13]));
    carbon_graph(&row(14)?, &bonds)
}

/// Spiro[4.5]decane: a hexagon on atoms 1 to 6 and a cyclopentane 1-7-8-9-10 that share atom 1.
fn spiro_decane() -> Result<Document, Box<dyn Error>> {
    let mut bonds = ring_bonds(6);
    bonds.extend(cycle(&[0, 6, 7, 8, 9]));
    carbon_graph(&row(10)?, &bonds)
}

/// Bicyclo[2.2.1]heptane on atoms 1 to 7, where atom 7 bridges atoms 1 and 4.
fn bicyclo_heptane() -> Result<Document, Box<dyn Error>> {
    let bonds: [(usize, usize); 8] = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 4),
        (4, 5),
        (5, 0),
        (0, 6),
        (6, 3),
    ];
    carbon_graph(&row(7)?, &bonds)
}

/// Two octagons on atoms 1 to 14 that share the bond 1-2.
fn two_octagons() -> Result<Document, Box<dyn Error>> {
    let mut bonds = ring_bonds(8);
    // The closing bond of the second octagon, 1-0, is the shared bond and is already listed.
    let second: Vec<usize> = vec![0, 8, 9, 10, 11, 12, 13, 1];
    bonds.extend(cycle(&second).into_iter().filter(|&bond| bond != (1, 0)));
    carbon_graph(&row(14)?, &bonds)
}

/// Two 13-membered rings on atoms 1 to 25 that share only atom 1.
fn two_thirteen_rings() -> Result<Document, Box<dyn Error>> {
    let first: Vec<usize> = (0..13).collect();
    let second: Vec<usize> = std::iter::once(0).chain(13..25).collect();
    let mut bonds = cycle(&first);
    bonds.extend(cycle(&second));
    carbon_graph(&row(25)?, &bonds)
}

/// A hexagon on atoms 1 to 6 and, apart from it, a naphthalene on atoms 7 to 16.
fn hexagon_and_naphthalene() -> Result<Document, Box<dyn Error>> {
    let mut bonds = ring_bonds(6);
    bonds.extend(shifted(&naphthalene_bonds(), 6));
    carbon_graph(&row(16)?, &bonds)
}

#[test]
fn a_single_hexagon_atom_with_scope_selection_is_ring_partly_and_with_scope_components_the_whole_ring_is_laid_out()
-> TestResult {
    let document = hexagon()?;
    let first = atom(1)?;
    assert_eq!(
        prepare_selection(&document, &[first], Scope::Selection).err(),
        Some(LayoutError::RingPartlyInLayoutSet { atoms: vec![first] }),
    );
    // With Scope::Components, atom 1's graph component is the whole ring, so no member is fixed.
    let prepared = prepare_selection(&document, &[first], Scope::Components)?;
    assert_eq!(prepared.layout_atoms, set_of(&serial_atoms(1..=6)?));
    let preview = layout::place(&prepared)?;
    assert_eq!(preview.positions.len(), 6, "every ring atom is laid out");
    Ok(())
}

#[test]
fn a_spiro_pair_with_one_ring_selected_is_ring_partly() -> TestResult {
    let document = spiro_decane()?;
    // The hexagon is selected whole. The cyclopentane keeps atoms 7 to 10 fixed, so its only layout
    // atom is the shared atom 1, and that atom is the one listed.
    assert_eq!(
        prepare_selection(&document, &serial_atoms(1..=6)?, Scope::Selection).err(),
        Some(LayoutError::RingPartlyInLayoutSet {
            atoms: vec![atom(1)?],
        }),
    );
    Ok(())
}

#[test]
fn a_fully_selected_spiro_pair_is_spiro_ring() -> TestResult {
    let document = spiro_decane()?;
    assert_eq!(
        prepare_selection(&document, &serial_atoms(1..=10)?, Scope::Selection).err(),
        Some(LayoutError::SpiroRingInLayoutSet {
            atoms: serial_atoms(1..=10)?,
        }),
    );
    Ok(())
}

#[test]
fn fused_and_bridged_systems_are_fused_ring() -> TestResult {
    let cases: [(&str, Document, u32); 2] = [
        ("naphthalene", naphthalene()?, 10),
        ("bicyclo[2.2.1]heptane", bicyclo_heptane()?, 7),
    ];
    for (name, document, count) in cases {
        let selected = serial_atoms(1..=count)?;
        assert_eq!(
            prepare_selection(&document, &selected, Scope::Selection).err(),
            Some(LayoutError::FusedRingInLayoutSet { atoms: selected }),
            "{name}: every atom of the system is listed",
        );
    }
    Ok(())
}

#[test]
fn a_13_ring_is_large_with_limit_12_and_a_12_ring_is_laid_out() -> TestResult {
    let thirteen = carbon_graph(&polygon(13)?, &ring_bonds(13))?;
    assert_eq!(
        prepare_selection(&thirteen, &serial_atoms(1..=13)?, Scope::Selection).err(),
        Some(LayoutError::LargeRingInLayoutSet {
            atoms: serial_atoms(1..=13)?,
            limit: 12,
        }),
    );
    let twelve = carbon_graph(&polygon(12)?, &ring_bonds(12))?;
    let prepared = prepare_selection(&twelve, &serial_atoms(1..=12)?, Scope::Selection)?;
    let preview = layout::place(&prepared)?;
    assert_eq!(preview.positions.len(), 12, "every ring atom is laid out");
    let [component] = preview.components.as_slice() else {
        return Err("the 12-ring must place as one component".into());
    };
    assert!(component.chains.is_empty(), "a lone ring has no chain");
    let [ring] = component.rings.as_slice() else {
        return Err("the 12-ring must place as one ring".into());
    };
    assert_eq!(ring.atoms.len(), 12);
    assert_eq!(set_of(&ring.atoms), set_of(&serial_atoms(1..=12)?));
    Ok(())
}

#[test]
fn the_first_failing_check_is_the_reply() -> TestResult {
    let mut hexagon_and_naphthalene_selection = serial_atoms(1..=3)?;
    hexagon_and_naphthalene_selection.extend(serial_atoms(7..=16)?);
    // Each document fails two checks, and the earlier check in the order is the reply.
    let cases: Vec<(&str, Document, Vec<AtomId>, LayoutError)> = vec![
        (
            "naphthalene with one ring selected: partly (5a) before fused (5b)",
            naphthalene()?,
            serial_atoms(1..=6)?,
            LayoutError::RingPartlyInLayoutSet {
                atoms: serial_atoms(1..=6)?,
            },
        ),
        (
            "naphthalene with a spiro cyclopentane, all selected: fused (5b) before spiro (5c)",
            naphthalene_with_spiro_cyclopentane()?,
            serial_atoms(1..=14)?,
            LayoutError::FusedRingInLayoutSet {
                atoms: serial_atoms(1..=10)?,
            },
        ),
        (
            "two 13-membered rings sharing atom 1, all selected: spiro (5c) before large (5d)",
            two_thirteen_rings()?,
            serial_atoms(1..=25)?,
            LayoutError::SpiroRingInLayoutSet {
                atoms: serial_atoms(1..=25)?,
            },
        ),
        (
            "two octagons sharing a bond, all selected: fused (5b) before large (5d)",
            two_octagons()?,
            serial_atoms(1..=14)?,
            LayoutError::FusedRingInLayoutSet {
                atoms: serial_atoms(1..=14)?,
            },
        ),
        (
            "a hexagon partly selected beside a naphthalene selected whole: only the hexagon's atoms",
            hexagon_and_naphthalene()?,
            hexagon_and_naphthalene_selection,
            LayoutError::RingPartlyInLayoutSet {
                atoms: serial_atoms(1..=3)?,
            },
        ),
    ];
    for (label, document, selected, expected) in cases {
        assert_eq!(
            prepare_selection(&document, &selected, Scope::Selection).err(),
            Some(expected),
            "{label}",
        );
    }
    Ok(())
}

#[test]
fn a_laid_out_ring_beside_a_fixed_ring_is_not_counted_and_the_fixed_one_is() -> TestResult {
    let document = biphenyl()?;
    let prepared = prepare_selection(&document, &serial_atoms(1..=6)?, Scope::Selection)?;
    // Ring 7 to 12 holds no layout atom, so it keeps its place and is counted once.
    assert_eq!(prepared.fixed_ring_systems, 1);
    assert_eq!(prepared.fixed_atom_count, 6);
    let joint = bond_between(&document, atom(6)?, atom(10)?)?;
    let [component] = prepared.components.as_slice() else {
        return Err("the selected ring must be one component".into());
    };
    // The component hangs from atom 10 of the fixed ring, through the bond that joins it to atom 6.
    assert_eq!(
        component.anchor,
        Some(Anchor {
            fixed_atom: atom(10)?,
            bond: joint,
        }),
    );
    let preview = layout::place(&prepared)?;
    let [placed] = preview.components.as_slice() else {
        return Err("the selected ring must place as one component".into());
    };
    // The boundary atom is the root, and the ring branches from the fixed atom.
    assert_eq!(placed.root, atom(6)?);
    let [ring] = placed.rings.as_slice() else {
        return Err("the selected ring must place as one ring".into());
    };
    assert_eq!(ring.branch_from, Some(atom(10)?));
    assert_eq!(set_of(&ring.atoms), set_of(&serial_atoms(1..=6)?));
    Ok(())
}

#[test]
fn a_wedge_on_a_ring_bond_of_a_selected_ring_is_still_stereo_in_layout() -> TestResult {
    let mut document = hexagon()?;
    let selected = serial_atoms(1..=6)?;
    // Without a mark, the whole selected ring is laid out.
    prepare_selection(&document, &selected, Scope::Selection)?;
    let ring_bond = bond_between(&document, atom(1)?, atom(2)?)?;
    document.apply(&[Command::SetBondMark {
        bond: BondRef::Id(ring_bond),
        mark: Some(StereoMark::Wedge {
            narrow: BondEnd::Begin,
        }),
    }])?;
    assert_eq!(
        prepare_selection(&document, &selected, Scope::Selection).err(),
        Some(LayoutError::StereoInLayout {
            atoms: vec![],
            bonds: vec![ring_bond],
        }),
    );
    Ok(())
}
