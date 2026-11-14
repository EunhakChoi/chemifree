//! The chain layout preview through the public API: what `prepare` refuses and in which order,
//! where `place` puts each layout component, how ties are broken, and where an interrupt stops
//! placement. Each expected position is worked out by hand from the rules in the documentation of
//! `chemifree_domain::layout`. A bond is 30 pt long unless a test says otherwise, so a bond at 30
//! degrees moves by `(dx, -15)` where `dx` is `15 * sqrt(3)`, and a bond at 90 degrees by `(0, 30)`.

use std::cell::Cell;
use std::collections::BTreeSet;

use chemifree_domain::budget::{CancelFlag, Interrupt, Interrupted};
use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondId, BondOrder, BondRef, BondTo, Command, Document, DocumentId,
    MAX_COMMANDS, MAX_COORDINATE, NewAtomPositions, ObjectId, Point, RingPattern, RingSize,
};
use chemifree_domain::element::Element;
use chemifree_domain::layout::{
    self, Anchor, Boundary, Chain, LayoutError, Placement, Preview, Scope,
};
use chemifree_domain::scene::Selection;
use chemifree_domain::settings::Settings;
use chemifree_domain::states::{
    AtomKind, BondQuery, Hydrogens, QueryAtom, QueryBondOrder, Radical,
};
use chemifree_domain::stereo::{AtomConfiguration, BondEnd, SlotRef, StereoMark};

/// The tolerance, in pt, of every comparison of a computed position.
const TOLERANCE: f64 = 1e-6;

/// `15 * sqrt(3)`, the horizontal step of a 30 pt bond at 30 degrees.
fn dx() -> f64 {
    15.0 * 3f64.sqrt()
}

fn atom(serial: u32) -> AtomId {
    AtomId::from_serial(serial).unwrap()
}

fn bond(serial: u32) -> BondId {
    BondId::from_serial(serial).unwrap()
}

fn point(x: f64, y: f64) -> Point {
    Point::new(x, y).unwrap()
}

fn carbon() -> Atom {
    Atom::from(Element::CARBON)
}

/// A carbon at (x, y), bonded to the carbon that command `bonded_to` of the same transaction adds.
fn add_carbon(x: f64, y: f64, bonded_to: Option<usize>) -> Command {
    Command::AddAtom {
        atom: carbon(),
        positions: NewAtomPositions::Single(point(x, y)),
        bond_to: bonded_to.map(|index| BondTo {
            atom: AtomRef::Created(index),
            order: BondOrder::Single,
        }),
    }
}

/// Carbons at `places`, each bonded to the carbon at the index given with its place.
fn carbons(places: &[(f64, f64, Option<usize>)]) -> Document {
    let mut document = Document::new(DocumentId::parse("layout").unwrap(), "Layout").unwrap();
    let commands: Vec<Command> = places
        .iter()
        .map(|&(x, y, bonded_to)| add_carbon(x, y, bonded_to))
        .collect();
    document.apply(&commands).unwrap();
    document
}

/// The butane of atoms 1 to 4 in a row 30 pt apart, bonds 1 to 3.
fn butane_row() -> Document {
    carbons(&[
        (0.0, 0.0, None),
        (30.0, 0.0, Some(0)),
        (60.0, 0.0, Some(1)),
        (90.0, 0.0, Some(2)),
    ])
}

/// The selection of the atoms with the given serials.
fn selection(serials: &[u32]) -> Selection {
    Selection {
        atoms: serials.iter().map(|&serial| atom(serial)).collect(),
        ..Selection::default()
    }
}

/// Prepares atoms `serials` of `document` as one selection, or returns the refusal.
fn prepare_of(document: &Document, serials: &[u32]) -> Result<layout::Preparation, LayoutError> {
    layout::prepare(
        document,
        document.first_instance(),
        &selection(serials),
        Scope::Selection,
    )
}

/// Prepares atoms `serials` of `document` as one selection and places them.
fn preview_of(document: &Document, serials: &[u32]) -> Result<Preview, LayoutError> {
    layout::place(&prepare_of(document, serials)?)
}

/// The placement of atom `serial` in `preview`.
fn placement_of(preview: &Preview, serial: u32) -> Placement {
    *preview
        .positions
        .iter()
        .find(|placement| placement.atom == atom(serial))
        .unwrap_or_else(|| panic!("a{serial} is not laid out"))
}

/// The placement of the layout atom whose before position is (x, y).
fn placement_before(preview: &Preview, x: f64, y: f64) -> Placement {
    *preview
        .positions
        .iter()
        .find(|placement| placement.before == point(x, y))
        .unwrap_or_else(|| panic!("no layout atom starts at ({x}, {y})"))
}

/// The atom of `document` at (x, y) in its first instance.
fn atom_at(document: &Document, x: f64, y: f64) -> AtomId {
    document
        .positions(document.first_instance())
        .find(|(_, at)| (at.x() - x).abs() < TOLERANCE && (at.y() - y).abs() < TOLERANCE)
        .map(|(id, _)| id)
        .unwrap_or_else(|| panic!("no atom at ({x}, {y})"))
}

/// Asserts that `actual` lies within `TOLERANCE` of (`x`, `y`).
fn assert_at(actual: Point, x: f64, y: f64, label: &str) {
    assert!(
        (actual.x() - x).abs() < TOLERANCE && (actual.y() - y).abs() < TOLERANCE,
        "{label}: expected ({x}, {y}), got ({}, {})",
        actual.x(),
        actual.y()
    );
}

/// Asserts that each atom in `offsets` lies at its offset from the placed position of `root`.
fn assert_offsets(preview: &Preview, root: u32, offsets: &[(u32, f64, f64)]) {
    let origin = placement_of(preview, root).after;
    for &(serial, dx_pt, dy_pt) in offsets {
        assert_at(
            placement_of(preview, serial).after,
            origin.x() + dx_pt,
            origin.y() + dy_pt,
            &format!("a{serial} from a{root}"),
        );
    }
}

/// Asserts that the placed atoms of a one-component `preview` keep the centroid of their before
/// positions.
fn assert_centroid_kept(preview: &Preview) {
    let count = preview.positions.len() as f64;
    let mut before = (0.0, 0.0);
    let mut after = (0.0, 0.0);
    for placement in &preview.positions {
        before.0 += placement.before.x();
        before.1 += placement.before.y();
        after.0 += placement.after.x();
        after.1 += placement.after.y();
    }
    assert_at(
        point(after.0 / count, after.1 / count),
        before.0 / count,
        before.1 / count,
        "centroid",
    );
}

/// The cosine of the angle at `apex` between the directions to `first` and to `second`. A straight
/// angle has cosine -1, and an angle of 120 degrees has cosine -1/2.
fn cosine_at(apex: Point, first: Point, second: Point) -> f64 {
    let (ux, uy) = (first.x() - apex.x(), first.y() - apex.y());
    let (vx, vy) = (second.x() - apex.x(), second.y() - apex.y());
    (ux * vx + uy * vy) / (ux.hypot(uy) * vx.hypot(vy))
}

/// A document of three atoms in a row 30 pt apart, with atom 2 bonded to atoms 1 and 3 by `order`.
fn row_of_three(atoms: [Atom; 3], order: BondOrder) -> Document {
    let mut document =
        Document::new(DocumentId::parse("row-of-three").unwrap(), "Row of three").unwrap();
    let [first, middle, last] = atoms;
    document
        .apply(&[
            Command::AddAtom {
                atom: first,
                positions: NewAtomPositions::Single(point(0.0, 0.0)),
                bond_to: None,
            },
            Command::AddAtom {
                atom: middle,
                positions: NewAtomPositions::Single(point(30.0, 0.0)),
                bond_to: Some(BondTo {
                    atom: AtomRef::Created(0),
                    order: order.clone(),
                }),
            },
            Command::AddAtom {
                atom: last,
                positions: NewAtomPositions::Single(point(60.0, 0.0)),
                bond_to: Some(BondTo {
                    atom: AtomRef::Created(1),
                    order,
                }),
            },
        ])
        .unwrap();
    document
}

/// A regular hexagon of carbons 1 to 6 centred on the origin, and carbon 7 apart from it.
fn ring_and_free_carbon() -> Document {
    let mut document = Document::new(DocumentId::parse("ring").unwrap(), "Ring").unwrap();
    let instance = document.first_instance();
    document
        .apply(&[
            Command::InsertRing {
                instance,
                center: point(0.0, 0.0),
                size: RingSize::Six,
                pattern: RingPattern::Single,
            },
            add_carbon(300.0, 300.0, None),
        ])
        .unwrap();
    document
}

/// A ring of `count` carbons on the circle through them, one 30 pt bond apart, centred at (x, y).
/// Carbon `index` is command `index` of the transaction; the last command closes the ring.
fn large_ring(count: usize, x: f64, y: f64) -> Vec<Command> {
    let radius = 30.0 / (2.0 * (std::f64::consts::PI / count as f64).sin());
    let mut commands: Vec<Command> = (0..count)
        .map(|index| {
            let angle = std::f64::consts::TAU * index as f64 / count as f64;
            add_carbon(
                x + radius * angle.cos(),
                y + radius * angle.sin(),
                index.checked_sub(1),
            )
        })
        .collect();
    commands.push(Command::AddBond {
        begin: AtomRef::Created(0),
        end: AtomRef::Created(count - 1),
        order: BondOrder::Single,
    });
    commands
}

/// Carbons 1 to `count` in a row 30 pt apart, each bonded to the one before. Each transaction
/// holds at most 250 commands, below the limit of `MAX_COMMANDS`.
fn long_chain(count: usize) -> Document {
    const CHUNK: usize = 250;
    const _: () = assert!(CHUNK <= MAX_COMMANDS);
    let mut document = Document::new(DocumentId::parse("chain").unwrap(), "Chain").unwrap();
    for start in (0..count).step_by(CHUNK) {
        let end = (start + CHUNK).min(count);
        // The last carbon added before this transaction, which its first carbon bonds to. Atom
        // identifiers grow with each addition, so it is the largest one present.
        let previous = document.atoms().map(|(id, _)| id).max();
        let mut commands = Vec::with_capacity(end - start);
        for index in start..end {
            let bond_to = if index == start {
                previous.map(|previous| BondTo {
                    atom: AtomRef::Id(previous),
                    order: BondOrder::Single,
                })
            } else {
                Some(BondTo {
                    atom: AtomRef::Created(index - start - 1),
                    order: BondOrder::Single,
                })
            };
            commands.push(Command::AddAtom {
                atom: carbon(),
                positions: NewAtomPositions::Single(point(index as f64 * 30.0, 0.0)),
                bond_to,
            });
        }
        document.apply(&commands).unwrap();
    }
    document
}

/// An interrupt that counts the questions it is asked, and stops with `TimeLimit` at the question
/// numbered `stop_at`, if one is given.
struct Probe {
    asked: Cell<usize>,
    stop_at: Option<usize>,
}

impl Probe {
    fn new(stop_at: Option<usize>) -> Self {
        Self {
            asked: Cell::new(0),
            stop_at,
        }
    }
}

impl Interrupt for Probe {
    fn check(&self) -> Result<(), Interrupted> {
        let asked = self.asked.get() + 1;
        self.asked.set(asked);
        if self.stop_at == Some(asked) {
            Err(Interrupted::TimeLimit)
        } else {
            Ok(())
        }
    }
}

#[test]
fn an_empty_selection_is_refused_with_its_reason() {
    let error = prepare_of(&butane_row(), &[]).unwrap_err();
    assert_eq!(error, LayoutError::EmptySelection);
    assert_eq!(error.reason(), "layout_selection_empty");
}

#[test]
fn a_selection_that_names_scene_objects_is_refused_before_any_atom_is_read() {
    let document = butane_row();
    let named_object = Selection {
        objects: [document.first_instance()].into(),
        atoms: [atom(9)].into(),
        ..Selection::default()
    };
    let error = layout::prepare(
        &document,
        document.first_instance(),
        &named_object,
        Scope::Selection,
    )
    .unwrap_err();
    assert_eq!(error, LayoutError::ObjectSelection);
    assert_eq!(error.reason(), "object_selection");
}

#[test]
fn names_missing_from_the_document_are_refused() {
    let document = butane_row();
    let missing_bond = Selection {
        bonds: [bond(9)].into(),
        ..Selection::default()
    };
    for (label, named) in [("atom a9", selection(&[9])), ("bond b9", missing_bond)] {
        let error = layout::prepare(
            &document,
            document.first_instance(),
            &named,
            Scope::Selection,
        )
        .unwrap_err();
        assert_eq!(error, LayoutError::UnknownSelection, "{label}");
        assert_eq!(error.reason(), "selection_unknown", "{label}");
    }
}

#[test]
fn the_components_scope_takes_each_component_whole_and_the_selection_scope_only_its_seeds() {
    let document = butane_row();
    let instance = document.first_instance();
    let whole = layout::prepare(&document, instance, &selection(&[4]), Scope::Components).unwrap();
    assert_eq!(
        whole.layout_atoms,
        BTreeSet::from([atom(1), atom(2), atom(3), atom(4)])
    );
    assert_eq!(whole.fixed_atom_count, 0);
    let seed = layout::prepare(&document, instance, &selection(&[4]), Scope::Selection).unwrap();
    assert_eq!(seed.layout_atoms, BTreeSet::from([atom(4)]));
    assert_eq!(seed.fixed_atom_count, 3);
    assert_eq!(
        seed.boundary,
        vec![Boundary {
            atom: atom(4),
            fixed_atom: atom(3),
            bond: bond(3),
        }]
    );
}

#[test]
fn a_selected_bond_seeds_both_of_its_atoms() {
    let document = butane_row();
    let bond_selection = Selection {
        bonds: [bond(3)].into(),
        ..Selection::default()
    };
    let prepared = layout::prepare(
        &document,
        document.first_instance(),
        &bond_selection,
        Scope::Selection,
    )
    .unwrap();
    assert_eq!(prepared.layout_atoms, BTreeSet::from([atom(3), atom(4)]));
    assert_eq!(prepared.layout_bonds, BTreeSet::from([bond(3)]));
    assert_eq!(
        prepared.boundary,
        vec![Boundary {
            atom: atom(3),
            fixed_atom: atom(2),
            bond: bond(2),
        }]
    );
}

#[test]
fn free_atoms_without_bonds_keep_their_exact_positions() {
    let document = carbons(&[(123.5, -7.25, None), (200.0, 50.0, None)]);
    let alone = preview_of(&document, &[1]).unwrap();
    assert_eq!(
        alone.positions,
        vec![Placement {
            atom: atom(1),
            before: point(123.5, -7.25),
            after: point(123.5, -7.25),
        }]
    );
    let pair = preview_of(&document, &[1, 2]).unwrap();
    assert_eq!(pair.components.len(), 2);
    for placement in &pair.positions {
        assert_eq!(placement.after, placement.before, "{placement:?}");
    }
    assert!(
        pair.components
            .iter()
            .all(|component| component.bonds.is_empty() && component.anchor.is_none())
    );
    assert!(pair.overlaps.is_empty(), "overlaps: {:?}", pair.overlaps);
}

#[test]
fn a_component_bonded_to_fixed_atoms_twice_is_refused_and_lists_both_bonds() {
    let error = prepare_of(&butane_row(), &[2, 3]).unwrap_err();
    assert_eq!(
        error,
        LayoutError::MultipleAttachments {
            atoms: vec![atom(2), atom(3)],
            bonds: vec![bond(1), bond(3)],
        }
    );
    assert_eq!(error.reason(), "multiple_attachments");
}

#[test]
fn a_fixed_neighbour_of_an_anchor_at_the_anchors_place_is_refused() {
    // Carbons 1 and 2 share one place and are bonded; carbon 3 hangs from carbon 2 at 60 pt.
    let document = carbons(&[(0.0, 0.0, None), (0.0, 0.0, Some(0)), (60.0, 0.0, Some(1))]);
    let error = prepare_of(&document, &[3]).unwrap_err();
    assert_eq!(
        error,
        LayoutError::CoincidentFixedAtoms {
            atom: atom(2),
            other: atom(1),
        }
    );
    assert_eq!(error.reason(), "coincident_fixed_atoms");
}

#[test]
fn an_instance_without_positions_refuses_its_first_layout_atom() {
    let document = butane_row();
    let missing = ObjectId::from_serial(9).unwrap();
    let error =
        layout::prepare(&document, missing, &selection(&[1]), Scope::Selection).unwrap_err();
    assert_eq!(error, LayoutError::MissingPosition { atom: atom(1) });
    assert_eq!(error.reason(), "missing_position");
}

#[test]
fn a_component_of_501_atoms_is_refused_and_one_of_500_is_laid_out() {
    let document = long_chain(501);
    let error = layout::prepare(
        &document,
        document.first_instance(),
        &selection(&[1]),
        Scope::Components,
    )
    .unwrap_err();
    assert_eq!(
        error,
        LayoutError::TooLarge {
            atoms: 501,
            limit: 500
        }
    );
    assert_eq!(error.reason(), "too_large");

    let document = long_chain(500);
    let prepared = layout::prepare(
        &document,
        document.first_instance(),
        &selection(&[1]),
        Scope::Components,
    )
    .unwrap();
    assert_eq!(prepared.layout_atoms.len(), 500);
    assert_eq!(layout::place(&prepared).unwrap().positions.len(), 500);
}

#[test]
fn ring_atoms_in_the_layout_set_are_refused_and_a_ring_elsewhere_is_not_counted() {
    let document = ring_and_free_carbon();
    assert_eq!(
        prepare_of(&document, &[1]).unwrap_err(),
        LayoutError::RingInLayoutSet {
            atoms: vec![atom(1)],
        }
    );
    // The refused ring atoms are listed in ascending order.
    assert_eq!(
        prepare_of(&document, &[2, 1, 7]).unwrap_err(),
        LayoutError::RingInLayoutSet {
            atoms: vec![atom(1), atom(2)],
        }
    );
    let prepared = prepare_of(&document, &[7]).unwrap();
    assert_eq!(prepared.layout_atoms, BTreeSet::from([atom(7)]));
    // The hexagon lies in a component with no layout atom, so it is not counted.
    assert_eq!(prepared.fixed_ring_systems, 0);
    assert_eq!(prepared.fixed_atom_count, 6);
}

#[test]
fn a_large_ring_in_another_component_is_not_counted_and_leaves_the_layout_unchanged() {
    // The butane row is one component. A 40-carbon ring, atoms 5 to 44, lies 1000 pt away in
    // another component, so no atom of the row is near it.
    let alone = butane_row();
    let mut with_ring = butane_row();
    with_ring.apply(&large_ring(40, 1000.0, 1000.0)).unwrap();
    // Carbon 5 is refused when it is laid out, which shows that the ring is a ring system.
    assert_eq!(
        prepare_of(&with_ring, &[5]).unwrap_err(),
        LayoutError::RingInLayoutSet {
            atoms: vec![atom(5)],
        }
    );
    let prepared = prepare_of(&with_ring, &[1, 2, 3, 4]).unwrap();
    let plain = prepare_of(&alone, &[1, 2, 3, 4]).unwrap();
    assert_eq!(prepared.fixed_atom_count, 40);
    assert_eq!(prepared.fixed_ring_systems, 0);
    assert_eq!(prepared.layout_atoms, plain.layout_atoms);
    assert_eq!(prepared.boundary, plain.boundary);
    assert_eq!(
        layout::place(&prepared).unwrap(),
        layout::place(&plain).unwrap()
    );
}

#[test]
fn a_whole_hexagon_in_the_layout_set_is_refused_as_ring_layout() {
    let document = ring_and_free_carbon();
    let error = prepare_of(&document, &[1, 2, 3, 4, 5, 6]).unwrap_err();
    assert_eq!(
        error,
        LayoutError::RingInLayoutSet {
            atoms: (1..=6).map(atom).collect(),
        }
    );
    assert_eq!(error.reason(), "ring_in_layout_set");
}

#[test]
fn a_substituent_on_a_fixed_ring_atom_points_away_from_the_ring() {
    // The hexagon vertex at (30, 0) carries carbon 8, drawn at (90, 40) and moved by the layout.
    let mut document = ring_and_free_carbon();
    let vertex = atom_at(&document, 30.0, 0.0);
    document
        .apply(&[Command::AddAtom {
            atom: carbon(),
            positions: NewAtomPositions::Single(point(90.0, 40.0)),
            bond_to: Some(BondTo {
                atom: AtomRef::Id(vertex),
                order: BondOrder::Single,
            }),
        }])
        .unwrap();
    let prepared = prepare_of(&document, &[8]).unwrap();
    // The hexagon shares a component with carbon 8, so it is counted.
    assert_eq!(prepared.fixed_ring_systems, 1);
    let preview = layout::place(&prepared).unwrap();
    // The vertex's ring neighbours lie at 120 and 240 degrees from it, and the widest gap runs
    // through 0 degrees.
    assert_at(placement_of(&preview, 8).after, 60.0, 0.0, "a8");
}

#[test]
fn a_wedge_on_a_layout_bond_or_on_a_bond_at_the_boundary_is_refused_and_a_distant_one_is_not() {
    let mut document = butane_row();
    document
        .apply(&[Command::SetBondMark {
            bond: BondRef::Id(bond(1)),
            mark: Some(StereoMark::Wedge {
                narrow: BondEnd::Begin,
            }),
        }])
        .unwrap();
    let expected = LayoutError::StereoInLayout {
        atoms: vec![],
        bonds: vec![bond(1)],
    };
    // Carbons 1 and 2 are laid out, and bond 1 joins them.
    assert_eq!(prepare_of(&document, &[1, 2]).unwrap_err(), expected);
    // Carbons 3 and 4 hang from carbon 2, a boundary fixed atom, and bond 1 touches it.
    assert_eq!(prepare_of(&document, &[3, 4]).unwrap_err(), expected);
    // Carbon 4 hangs from carbon 3, and bond 1 touches neither carbon 3 nor carbon 4.
    let prepared = prepare_of(&document, &[4]).unwrap();
    assert_eq!(prepared.layout_atoms, BTreeSet::from([atom(4)]));
}

#[test]
fn a_stated_center_next_to_the_layout_set_is_refused() {
    // Carbon 1 is bonded to carbons 2 to 5, and its stated configuration names all four.
    let mut document = carbons(&[
        (0.0, 0.0, None),
        (30.0, 0.0, Some(0)),
        (-30.0, 0.0, Some(0)),
        (0.0, 30.0, Some(0)),
        (0.0, -30.0, Some(0)),
    ]);
    let neighbours = [2, 3, 4, 5].map(|serial| SlotRef::Atom(AtomRef::Id(atom(serial))));
    document
        .apply(&[Command::SetAtomConfiguration {
            atom: AtomRef::Id(atom(1)),
            configuration: Some(AtomConfiguration::Unknown {
                neighbors: neighbours,
            }),
        }])
        .unwrap();
    // Carbon 2 is laid out, and its fixed neighbour carbon 1 carries the stated center.
    assert_eq!(
        prepare_of(&document, &[2]).unwrap_err(),
        LayoutError::StereoInLayout {
            atoms: vec![atom(1)],
            bonds: vec![],
        }
    );
}

#[test]
fn an_abbreviation_covering_a_layout_atom_is_refused_and_one_covering_fixed_atoms_is_not() {
    // An oxygen (1) and an ethyl group (2 and 3), drawn as one display keyed at carbon 2.
    let mut document = Document::new(DocumentId::parse("ethyl").unwrap(), "Ethyl").unwrap();
    let instance = document.first_instance();
    document
        .apply(&[
            Command::AddAtom {
                atom: Atom::from(Element::from_symbol("O").unwrap()),
                positions: NewAtomPositions::Single(point(0.0, 0.0)),
                bond_to: None,
            },
            add_carbon(30.0, 0.0, Some(0)),
            add_carbon(45.0, 25.0, Some(1)),
        ])
        .unwrap();
    document
        .apply(&[Command::CollapseEthyl {
            instance,
            anchor: atom(2),
            terminal: atom(3),
        }])
        .unwrap();
    // Carbon 2 is the key and carbon 3 the hidden terminal, so both are covered.
    let error = prepare_of(&document, &[2, 3]).unwrap_err();
    assert_eq!(
        error,
        LayoutError::DisplayInLayoutSet {
            atoms: vec![atom(2), atom(3)],
        }
    );
    assert_eq!(error.reason(), "display_in_layout_set");
    // The hidden terminal alone is refused under the selection scope. The components scope takes
    // the key along with it.
    assert_eq!(
        prepare_of(&document, &[3]).unwrap_err(),
        LayoutError::DisplayInLayoutSet {
            atoms: vec![atom(3)],
        }
    );
    assert_eq!(
        layout::prepare(&document, instance, &selection(&[3]), Scope::Components).unwrap_err(),
        LayoutError::DisplayInLayoutSet {
            atoms: vec![atom(2), atom(3)],
        }
    );
    // The display covers carbons 2 and 3 only, so the oxygen is laid out.
    let prepared = prepare_of(&document, &[1]).unwrap();
    assert_eq!(prepared.layout_atoms, BTreeSet::from([atom(1)]));
}

#[test]
fn a_member_of_a_fragment_display_in_the_layout_set_is_refused_and_a_fixed_member_is_not() {
    // An oxygen (1) bonded to carbon 2, which carries carbons 3 and 4. The three carbons are
    // contracted to one fragment display keyed at carbon 2.
    let mut document = Document::new(DocumentId::parse("fragment").unwrap(), "Fragment").unwrap();
    let instance = document.first_instance();
    document
        .apply(&[
            Command::AddAtom {
                atom: Atom::from(Element::from_symbol("O").unwrap()),
                positions: NewAtomPositions::Single(point(0.0, 0.0)),
                bond_to: None,
            },
            add_carbon(30.0, 0.0, Some(0)),
            add_carbon(45.0, 25.0, Some(1)),
            add_carbon(45.0, -25.0, Some(1)),
        ])
        .unwrap();
    document
        .apply(&[Command::ContractFragment {
            instance,
            anchor: atom(2),
            members: vec![atom(2), atom(3), atom(4)],
            text: "iPr".into(),
        }])
        .unwrap();
    assert_eq!(
        prepare_of(&document, &[3]).unwrap_err(),
        LayoutError::DisplayInLayoutSet {
            atoms: vec![atom(3)],
        }
    );
    assert_eq!(
        prepare_of(&document, &[3, 4]).unwrap_err(),
        LayoutError::DisplayInLayoutSet {
            atoms: vec![atom(3), atom(4)],
        }
    );
    // The oxygen is not a member, so it is laid out although its neighbour is the display's key.
    let prepared = prepare_of(&document, &[1]).unwrap();
    assert_eq!(prepared.layout_atoms, BTreeSet::from([atom(1)]));
}

#[test]
fn a_free_chain_zigzags_at_120_degrees_from_its_smaller_end() {
    // The ends of butane have the same code, so the root is the smaller identifier, a1.
    let preview = preview_of(&butane_row(), &[1, 2, 3, 4]).unwrap();
    let component = &preview.components[0];
    assert_eq!(component.root, atom(1));
    assert_eq!(component.anchor, None);
    assert_eq!(
        component.chains,
        vec![Chain {
            atoms: vec![atom(1), atom(2), atom(3), atom(4)],
            branch_from: None,
        }]
    );
    assert_offsets(
        &preview,
        1,
        &[
            (1, 0.0, 0.0),
            (2, dx(), -15.0),
            (3, 2.0 * dx(), 0.0),
            (4, 3.0 * dx(), -15.0),
        ],
    );
    assert_centroid_kept(&preview);
    assert!(
        preview.overlaps.is_empty(),
        "overlaps: {:?}",
        preview.overlaps
    );
}

#[test]
fn a_branch_takes_the_second_slot_and_the_first_ranked_child_continues_the_chain() {
    // 2-methylbutane: carbon 2 carries carbons 1 and 5, and carbon 3 runs on to carbon 4.
    let document = carbons(&[
        (0.0, 0.0, None),
        (30.0, 0.0, Some(0)),
        (60.0, 0.0, Some(1)),
        (90.0, 0.0, Some(2)),
        (30.0, 30.0, Some(1)),
    ]);
    let preview = preview_of(&document, &[1, 2, 3, 4, 5]).unwrap();
    let component = &preview.components[0];
    // The end of a longest path with the smallest code is carbon 4.
    assert_eq!(component.root, atom(4));
    assert_eq!(
        component.chains,
        vec![
            Chain {
                atoms: vec![atom(4), atom(3), atom(2), atom(1)],
                branch_from: None,
            },
            Chain {
                atoms: vec![atom(5)],
                branch_from: Some(atom(2)),
            },
        ]
    );
    assert_offsets(
        &preview,
        4,
        &[
            (4, 0.0, 0.0),
            (3, dx(), -15.0),
            (2, 2.0 * dx(), 0.0),
            (1, 3.0 * dx(), -15.0),
            (5, 2.0 * dx(), 30.0),
        ],
    );
    assert_centroid_kept(&preview);
}

#[test]
fn isobutane_puts_its_first_methyl_on_the_first_slot_and_breaks_the_tie_by_identifier() {
    // Carbon 2 is the central carbon, and carbons 1, 3 and 4 are its methyls.
    let document = carbons(&[
        (0.0, 0.0, None),
        (30.0, 0.0, Some(0)),
        (60.0, 0.0, Some(1)),
        (30.0, 30.0, Some(1)),
    ]);
    let preview = preview_of(&document, &[1, 2, 3, 4]).unwrap();
    assert_eq!(preview.components[0].root, atom(1));
    assert_offsets(
        &preview,
        1,
        &[
            (1, 0.0, 0.0),
            (2, dx(), -15.0),
            (3, 2.0 * dx(), 0.0),
            (4, dx(), -45.0),
        ],
    );
}

#[test]
fn a_triple_bond_keeps_the_chain_straight() {
    // But-2-yne: bonds 1 and 3 are single, and bond 2 is triple.
    let mut yne = Document::new(DocumentId::parse("yne").unwrap(), "Yne").unwrap();
    yne.apply(&[
        add_carbon(0.0, 0.0, None),
        add_carbon(30.0, 0.0, Some(0)),
        Command::AddAtom {
            atom: carbon(),
            positions: NewAtomPositions::Single(point(60.0, 0.0)),
            bond_to: Some(BondTo {
                atom: AtomRef::Created(1),
                order: BondOrder::Triple,
            }),
        },
        add_carbon(90.0, 0.0, Some(2)),
    ])
    .unwrap();
    let preview = preview_of(&yne, &[1, 2, 3, 4]).unwrap();
    assert_eq!(preview.components[0].root, atom(1));
    assert_offsets(
        &preview,
        1,
        &[
            (1, 0.0, 0.0),
            (2, dx(), -15.0),
            (3, 2.0 * dx(), -30.0),
            (4, 3.0 * dx(), -45.0),
        ],
    );
}

#[test]
fn a_free_sulfur_with_two_double_bonds_is_bent_at_120_degrees() {
    // O=S=O: the sulfur is not a carbon, so its two double bonds do not make it linear. Its other
    // neighbour turns 60 degrees away from the bond to the root oxygen, which leaves 120 degrees.
    let oxygen = Atom::from(Element::from_symbol("O").unwrap());
    let sulfur = Atom::from(Element::from_symbol("S").unwrap());
    let document = row_of_three([oxygen.clone(), sulfur, oxygen], BondOrder::Double);
    let preview = preview_of(&document, &[1, 2, 3]).unwrap();
    let cosine = cosine_at(
        placement_of(&preview, 2).after,
        placement_of(&preview, 1).after,
        placement_of(&preview, 3).after,
    );
    assert!(
        (cosine + 0.5).abs() < TOLERANCE,
        "the angle at the sulfur is 120 degrees, with cosine -0.5, got cosine {cosine}"
    );
}

#[test]
fn a_free_allene_stays_straight() {
    // C=C=C: the central carbon has two double bonds and is linear, so the three carbons lie on one
    // line.
    let document = row_of_three([carbon(), carbon(), carbon()], BondOrder::Double);
    let preview = preview_of(&document, &[1, 2, 3]).unwrap();
    let cosine = cosine_at(
        placement_of(&preview, 2).after,
        placement_of(&preview, 1).after,
        placement_of(&preview, 3).after,
    );
    assert!(
        (cosine + 1.0).abs() < TOLERANCE,
        "the angle at the central carbon is straight, with cosine -1, got cosine {cosine}"
    );
}

#[test]
fn the_document_bond_length_sets_the_length_of_every_placed_bond() {
    let settings = Settings {
        bond_length_pt: 20.0,
        ..Settings::default()
    };
    let mut document =
        Document::new_with_settings(DocumentId::parse("short").unwrap(), "Short", settings)
            .unwrap();
    document
        .apply(&[
            add_carbon(0.0, 0.0, None),
            add_carbon(20.0, 0.0, Some(0)),
            add_carbon(40.0, 0.0, Some(1)),
            add_carbon(60.0, 0.0, Some(2)),
        ])
        .unwrap();
    let prepared = prepare_of(&document, &[1, 2, 3, 4]).unwrap();
    assert_eq!(prepared.bond_length_pt, 20.0);
    let preview = layout::place(&prepared).unwrap();
    // At 20 pt the zigzag steps are (10 * sqrt(3), -10) and then (10 * sqrt(3), 10).
    let step = 10.0 * 3f64.sqrt();
    assert_offsets(
        &preview,
        1,
        &[
            (1, 0.0, 0.0),
            (2, step, -10.0),
            (3, 2.0 * step, 0.0),
            (4, 3.0 * step, -10.0),
        ],
    );
}

#[test]
fn a_lone_anchored_atom_hangs_at_minus_thirty_degrees_from_its_fixed_neighbour() {
    // Carbon 1 is fixed at the origin, and carbon 2 is the only layout atom, bonded to it.
    let document = carbons(&[(0.0, 0.0, None), (30.0, 0.0, Some(0))]);
    let preview = preview_of(&document, &[2]).unwrap();
    let component = &preview.components[0];
    assert_eq!(
        component.anchor,
        Some(Anchor {
            fixed_atom: atom(1),
            bond: bond(1),
        })
    );
    assert_eq!(component.root, atom(2));
    assert_at(placement_of(&preview, 2).after, dx(), -15.0, "a2");
}

#[test]
fn an_anchored_atom_with_one_placed_neighbour_takes_beta_plus_120_on_a_tie() {
    // Carbon 4 hangs from its anchor, carbon 3. Carbon 3's other fixed neighbour, carbon 2, lies at
    // 180 degrees, and the candidates at 300 and 60 degrees both lie 120 degrees from it, so the
    // tie takes 300 degrees.
    let preview = preview_of(&butane_row(), &[4]).unwrap();
    assert_at(placement_of(&preview, 4).after, 75.0, -dx(), "a4");
}

#[test]
fn an_anchor_between_two_fixed_neighbours_takes_the_middle_of_the_widest_gap() {
    // Carbon 2 is fixed, and its fixed neighbours lie at 180 degrees (carbon 1) and 90 degrees
    // (carbon 3). The widest gap runs from 180 to 450 degrees, so carbon 4 goes to 315 degrees.
    let document = carbons(&[
        (0.0, 0.0, None),
        (30.0, 0.0, Some(0)),
        (30.0, 30.0, Some(1)),
        (60.0, 0.0, Some(1)),
    ]);
    let preview = preview_of(&document, &[4]).unwrap();
    assert_at(
        placement_of(&preview, 4).after,
        30.0 + 15.0 * 2f64.sqrt(),
        -15.0 * 2f64.sqrt(),
        "a4",
    );
}

#[test]
fn a_chain_on_a_boundary_bends_away_from_a_nearby_fixed_atom() {
    // Carbons 1 to 3 are fixed in a row, and carbon 4 lies apart from them at (75, -20). Carbon 5
    // hangs from carbon 3, and carbon 6 from carbon 5.
    let document = carbons(&[
        (0.0, 0.0, None),
        (30.0, 0.0, Some(0)),
        (60.0, 0.0, Some(1)),
        (75.0, -20.0, None),
        (90.0, 90.0, Some(2)),
        (90.0, 120.0, Some(4)),
    ]);
    let prepared = prepare_of(&document, &[5, 6]).unwrap();
    assert_eq!(
        prepared.boundary,
        vec![Boundary {
            atom: atom(5),
            fixed_atom: atom(3),
            bond: bond(3),
        }]
    );
    let preview = layout::place(&prepared).unwrap();
    // Carbon 4 is 46 pt from the side at 60 degrees and 6 pt from the side at 300 degrees, so
    // carbon 5 takes the side at 60 degrees.
    assert_at(placement_of(&preview, 5).after, 75.0, dx(), "a5");
    assert_at(placement_of(&preview, 6).after, 60.0, 2.0 * dx(), "a6");
    assert!(
        preview.overlaps.is_empty(),
        "overlaps: {:?}",
        preview.overlaps
    );
}

#[test]
fn renumbering_the_atoms_leaves_each_placement_unchanged() {
    // Carbon X is bonded to the fixed carbon F, and carries the chain P1-P2 and the single carbon
    // Q1. The second document creates Q1 before P1, so the identifiers of X's children are swapped.
    let first = carbons(&[
        (0.0, 0.0, None),
        (100.0, 100.0, Some(0)),
        (100.0, 130.0, Some(1)),
        (100.0, 160.0, Some(2)),
        (130.0, 100.0, Some(1)),
    ]);
    let second = carbons(&[
        (0.0, 0.0, None),
        (100.0, 100.0, Some(0)),
        (130.0, 100.0, Some(1)),
        (100.0, 130.0, Some(1)),
        (100.0, 160.0, Some(3)),
    ]);
    // The layout atoms by their before positions, with the placement the rules give each.
    let roles = [
        ("X", (100.0, 100.0), (dx(), -15.0)),
        ("P1", (100.0, 130.0), (2.0 * dx(), 0.0)),
        ("P2", (100.0, 160.0), (3.0 * dx(), -15.0)),
        ("Q1", (130.0, 100.0), (dx(), -45.0)),
    ];
    for document in [&first, &second] {
        let preview = preview_of(document, &[2, 3, 4, 5]).unwrap();
        for (role, (before_x, before_y), (x, y)) in roles {
            let placement = placement_before(&preview, before_x, before_y);
            assert_at(placement.after, x, y, role);
        }
    }
}

/// A fixed carbon (1) and carbon 2 bonded to it, with two siblings bonded to carbon 2: `first` at
/// (60, 30) and `second` at (30, 30), each joined to carbon 2 by the bond order given with it. With
/// `swapped`, `second` is created first, so the identifiers of the two siblings trade places while
/// each keeps its place.
fn siblings(first: (Atom, BondOrder), second: (Atom, BondOrder), swapped: bool) -> Document {
    let mut document = Document::new(DocumentId::parse("siblings").unwrap(), "Siblings").unwrap();
    let mut pair = [(first, (60.0, 30.0)), (second, (30.0, 30.0))];
    if swapped {
        pair.reverse();
    }
    let mut commands = vec![add_carbon(0.0, 0.0, None), add_carbon(30.0, 0.0, Some(0))];
    commands.extend(
        pair.into_iter()
            .map(|((sibling, order), (x, y))| Command::AddAtom {
                atom: sibling,
                positions: NewAtomPositions::Single(point(x, y)),
                bond_to: Some(BondTo {
                    atom: AtomRef::Created(1),
                    order,
                }),
            }),
    );
    document.apply(&commands).unwrap();
    document
}

#[test]
fn siblings_that_differ_in_one_recorded_state_keep_their_places_when_renumbered() {
    // Each pair of siblings differs only in the state named. A tie left to identifiers would give
    // the swapped document the other places, so the code must decide the order of the pair.
    let pairs = [
        (
            "radical",
            Atom {
                radical: Radical::None,
                ..carbon()
            },
            Atom {
                radical: Radical::Doublet,
                ..carbon()
            },
        ),
        (
            "explicit hydrogens",
            Atom {
                hydrogens: Hydrogens::Specified(1),
                ..carbon()
            },
            Atom {
                hydrogens: Hydrogens::Specified(2),
                ..carbon()
            },
        ),
        (
            "query kind",
            Atom::new(AtomKind::Query(QueryAtom::Any)),
            Atom::new(AtomKind::Query(QueryAtom::Halogen)),
        ),
    ];
    for (state, first, second) in pairs {
        let plain = siblings(
            (first.clone(), BondOrder::Single),
            (second.clone(), BondOrder::Single),
            /* swapped= */ false,
        );
        let swapped = siblings(
            (first, BondOrder::Single),
            (second, BondOrder::Single),
            /* swapped= */ true,
        );
        assert_same_places(&plain, &swapped, &[2, 3, 4], state);
    }
}

/// Asserts that `swapped` places each atom of `serials` where `plain` places it, matched by before
/// position. `swapped` lays out the same atoms with their identifiers exchanged, so a tie that is
/// left to identifiers would move some atom. `state` names the case in a failure.
fn assert_same_places(plain: &Document, swapped: &Document, serials: &[u32], state: &str) {
    let before = preview_of(plain, serials).unwrap();
    let renumbered = preview_of(swapped, serials).unwrap();
    assert_eq!(before.positions.len(), serials.len(), "{state}");
    for placement in &before.positions {
        let twin = placement_before(&renumbered, placement.before.x(), placement.before.y());
        assert_at(
            twin.after,
            placement.after.x(),
            placement.after.y(),
            &format!("{state}, atom at {:?}", placement.before),
        );
    }
}

/// A fixed carbon (1) with two single carbons bonded to it: `first` at (0, 30) and `second` at
/// (0, -30), each joined to carbon 1 by the bond order given with it. With `swapped`, `second` is
/// created first, so the identifiers of the two carbons trade places while each keeps its place.
fn hanging_pair(first: BondOrder, second: BondOrder, swapped: bool) -> Document {
    let mut document = Document::new(DocumentId::parse("hanging").unwrap(), "Hanging").unwrap();
    let mut pair = [(first, (0.0, 30.0)), (second, (0.0, -30.0))];
    if swapped {
        pair.reverse();
    }
    let mut commands = vec![add_carbon(0.0, 0.0, None)];
    commands.extend(pair.into_iter().map(|(order, (x, y))| Command::AddAtom {
        atom: carbon(),
        positions: NewAtomPositions::Single(point(x, y)),
        bond_to: Some(BondTo {
            atom: AtomRef::Created(0),
            order,
        }),
    }));
    document.apply(&commands).unwrap();
    document
}

#[test]
fn siblings_that_differ_only_in_their_query_bond_keep_their_places_when_renumbered() {
    // Each pair of siblings is joined to carbon 2 by a query bond, and the bonds differ only in
    // what the query names: any kind, or a list of kinds, which the code compares in stored order.
    let list = |orders: Vec<QueryBondOrder>| BondOrder::Query(BondQuery::OneOf(orders));
    let pairs = [
        (
            "any and a list",
            BondOrder::Query(BondQuery::Any),
            list(vec![QueryBondOrder::Single]),
        ),
        (
            "lists of different kinds",
            list(vec![QueryBondOrder::Single]),
            list(vec![QueryBondOrder::Double]),
        ),
        (
            "one list in two stored orders",
            list(vec![QueryBondOrder::Single, QueryBondOrder::Double]),
            list(vec![QueryBondOrder::Double, QueryBondOrder::Single]),
        ),
    ];
    for (state, first, second) in pairs {
        let plain = siblings(
            (carbon(), first.clone()),
            (carbon(), second.clone()),
            /* swapped= */ false,
        );
        let swapped = siblings(
            (carbon(), first),
            (carbon(), second),
            /* swapped= */ true,
        );
        assert_same_places(&plain, &swapped, &[2, 3, 4], state);
    }
}

#[test]
fn components_hung_from_one_fixed_atom_by_query_bonds_of_different_kinds_keep_their_places() {
    // A component's rank compares its height, size and code before its boundary bond, so two single
    // carbons that differ only in the query of their boundary bond are ordered by that query.
    let first = BondOrder::Query(BondQuery::Any);
    let second = BondOrder::Query(BondQuery::OneOf(vec![QueryBondOrder::Single]));
    let plain = hanging_pair(first.clone(), second.clone(), /* swapped= */ false);
    let swapped = hanging_pair(first, second, /* swapped= */ true);
    assert_same_places(&plain, &swapped, &[2, 3], "query boundary bonds");
}

/// A dative bond between two carbons of one transaction. It begins at the carbon at index `from`
/// when `forward`, and at the carbon at index `to` otherwise, so only its direction differs.
fn dative_bond(from: usize, to: usize, forward: bool) -> Command {
    let (begin, end) = if forward { (from, to) } else { (to, from) };
    Command::AddBond {
        begin: AtomRef::Created(begin),
        end: AtomRef::Created(end),
        order: BondOrder::Dative,
    }
}

/// A fixed carbon (1) and carbon 2 bonded to it, with two siblings bonded to carbon 2 by dative
/// bonds: `first` at (60, 30) and `second` at (30, 30). A set flag makes its bond begin at carbon 2
/// and a clear flag makes it end there. With `swapped`, `second` is created first, so the
/// identifiers of the two siblings trade places while each keeps its place and its bond.
fn dative_siblings(first: bool, second: bool, swapped: bool) -> Document {
    let mut document = Document::new(
        DocumentId::parse("dative-siblings").unwrap(),
        "Dative siblings",
    )
    .unwrap();
    let mut pair = [(first, (60.0, 30.0)), (second, (30.0, 30.0))];
    if swapped {
        pair.reverse();
    }
    let mut commands = vec![add_carbon(0.0, 0.0, None), add_carbon(30.0, 0.0, Some(0))];
    commands.extend(pair.iter().map(|&(_, (x, y))| add_carbon(x, y, None)));
    commands.extend(
        pair.iter()
            .enumerate()
            .map(|(slot, &(forward, _))| dative_bond(1, 2 + slot, forward)),
    );
    document.apply(&commands).unwrap();
    document
}

/// A fixed carbon (1) at the origin with two carbons bonded to it by dative bonds: `first` at
/// (0, 30) and `second` at (0, -30). A set flag makes its bond begin at the fixed carbon and a
/// clear flag makes it end there. With `swapped`, `second` is created first.
fn dative_hanging(first: bool, second: bool, swapped: bool) -> Document {
    let mut document = Document::new(
        DocumentId::parse("dative-hanging").unwrap(),
        "Dative hanging",
    )
    .unwrap();
    let mut pair = [(first, (0.0, 30.0)), (second, (0.0, -30.0))];
    if swapped {
        pair.reverse();
    }
    let mut commands = vec![add_carbon(0.0, 0.0, None)];
    commands.extend(pair.iter().map(|&(_, (x, y))| add_carbon(x, y, None)));
    commands.extend(
        pair.iter()
            .enumerate()
            .map(|(slot, &(forward, _))| dative_bond(0, 1 + slot, forward)),
    );
    document.apply(&commands).unwrap();
    document
}

#[test]
fn dative_siblings_differing_only_in_direction_keep_their_places_when_renumbered() {
    // One sibling's dative bond begins at carbon 2 and the other's ends there. Only the code
    // states the direction, so the code must decide the order of the pair.
    for (state, first, second) in [
        ("first begins at carbon 2", true, false),
        ("first ends at carbon 2", false, true),
    ] {
        let plain = dative_siblings(first, second, /* swapped= */ false);
        let swapped = dative_siblings(first, second, /* swapped= */ true);
        assert_same_places(&plain, &swapped, &[2, 3, 4], state);
    }
}

#[test]
fn components_hung_by_dative_bonds_of_opposite_direction_keep_their_places_when_renumbered() {
    // Two carbons hang from one fixed carbon, one dative bond beginning at it and one ending at
    // it. The components tie on height, size and code, so the boundary bond's code decides.
    for (state, first, second) in [
        ("first begins at the fixed carbon", true, false),
        ("first ends at the fixed carbon", false, true),
    ] {
        let plain = dative_hanging(first, second, /* swapped= */ false);
        let swapped = dative_hanging(first, second, /* swapped= */ true);
        assert_same_places(&plain, &swapped, &[2, 3], state);
    }
}

#[test]
fn placing_twice_gives_the_same_preview_and_a_laid_out_chain_stays_put() {
    let prepared = prepare_of(&butane_row(), &[1, 2, 3, 4]).unwrap();
    let first = layout::place(&prepared).unwrap();
    assert_eq!(layout::place(&prepared).unwrap(), first);
    // Build the same chain at its placed positions: laying it out again moves no atom.
    let places: Vec<(f64, f64, Option<usize>)> = first
        .positions
        .iter()
        .enumerate()
        .map(|(index, placement)| {
            (
                placement.after.x(),
                placement.after.y(),
                index.checked_sub(1),
            )
        })
        .collect();
    let again = preview_of(&carbons(&places), &[1, 2, 3, 4]).unwrap();
    for (index, placement) in first.positions.iter().enumerate() {
        let serial = index as u32 + 1;
        assert_at(
            placement_of(&again, serial).after,
            placement.after.x(),
            placement.after.y(),
            &format!("a{serial} laid out again"),
        );
    }
}

#[test]
fn anchored_components_are_placed_in_the_order_of_their_anchors() {
    // Carbons 1 and 2 are fixed at x = 100 and x = 0. Carbon 3 hangs from carbon 1, and carbon 4
    // from carbon 2.
    let document = carbons(&[
        (100.0, 0.0, None),
        (0.0, 0.0, None),
        (100.0, 50.0, Some(0)),
        (0.0, 50.0, Some(1)),
    ]);
    let preview = preview_of(&document, &[3, 4]).unwrap();
    assert_eq!(preview.components.len(), 2);
    assert_eq!(preview.components[0].atoms, vec![atom(4)]);
    assert_eq!(
        preview.components[0].anchor,
        Some(Anchor {
            fixed_atom: atom(2),
            bond: bond(2),
        })
    );
    assert_eq!(preview.components[1].atoms, vec![atom(3)]);
    assert_at(placement_of(&preview, 4).after, dx(), -15.0, "a4");
    assert_at(placement_of(&preview, 3).after, 100.0 + dx(), -15.0, "a3");
}

/// The anchors of the components of a preview of carbons 2 and 4, in placement order. Carbon 1 is
/// fixed at (`double_x`, 0) with a double bond to carbon 2, and carbon 3 is fixed at (`single_x`,
/// 0) with a single bond to carbon 4. Each layout carbon hangs from its own anchor, so the anchors
/// alone decide the order of the two components.
fn anchor_order(double_x: f64, single_x: f64) -> Vec<Option<AtomId>> {
    let mut document = Document::new(DocumentId::parse("anchors").unwrap(), "Anchors").unwrap();
    document
        .apply(&[
            add_carbon(double_x, 0.0, None),
            Command::AddAtom {
                atom: carbon(),
                positions: NewAtomPositions::Single(point(double_x, 30.0)),
                bond_to: Some(BondTo {
                    atom: AtomRef::Created(0),
                    order: BondOrder::Double,
                }),
            },
            add_carbon(single_x, 0.0, None),
            add_carbon(single_x, 30.0, Some(2)),
        ])
        .unwrap();
    let preview = preview_of(&document, &[2, 4]).unwrap();
    preview
        .components
        .iter()
        .map(|component| component.anchor.map(|anchor| anchor.fixed_atom))
        .collect()
}

#[test]
fn anchors_that_differ_only_in_the_sign_of_zero_are_ranked_as_one_place() {
    // Carbon 1 is at (-0, 0) and carbon 3 at (0, 0): one place, so the rank decides, and the
    // single bond of carbon 3 ranks before the double bond of carbon 1.
    assert_eq!(
        anchor_order(-0.0, 0.0),
        vec![Some(atom(3)), Some(atom(1))],
        "anchors at (-0, 0) and (0, 0) are one place"
    );
}

#[test]
fn anchors_less_than_a_coincidence_tolerance_apart_are_ranked_as_one_place() {
    // Carbon 3 is at (1e-12, 0), in the same grid cell as carbon 1 at (0, 0).
    assert_eq!(
        anchor_order(0.0, 1e-12),
        vec![Some(atom(3)), Some(atom(1))],
        "anchors 1e-12 pt apart are one place"
    );
}

#[test]
fn anchors_at_the_same_place_are_ordered_by_rank() {
    // The identifiers would put carbon 1 first, so this is the rank at work.
    assert_eq!(
        anchor_order(0.0, 0.0),
        vec![Some(atom(3)), Some(atom(1))],
        "anchors at one place are ordered by rank, not by identifier"
    );
}

#[test]
fn anchors_a_point_apart_are_ordered_by_position_before_rank() {
    // Carbon 3 is at (1, 0), beyond carbon 1 at (0, 0), so carbon 1 is placed first although the
    // single bond of carbon 3 ranks first.
    assert_eq!(
        anchor_order(0.0, 1.0),
        vec![Some(atom(1)), Some(atom(3))],
        "anchors 1 pt apart are ordered by x before rank"
    );
}

#[test]
fn a_pair_closer_than_half_a_bond_is_reported_once_and_a_pair_at_exactly_half_is_not() {
    // Carbon 2 is laid out between the fixed carbons 1 and 3: 10 pt from carbon 1 and 15 pt from
    // carbon 3, which is half a bond and does not count.
    let document = carbons(&[(0.0, 0.0, None), (10.0, 0.0, None), (25.0, 0.0, None)]);
    let preview = preview_of(&document, &[2]).unwrap();
    assert_eq!(preview.overlaps, vec![(atom(1), atom(2))]);
}

/// A document of `count` carbons that all stand at the origin and are not bonded.
fn coincident_carbons(count: usize) -> Document {
    let places: Vec<(f64, f64, Option<usize>)> = vec![(0.0, 0.0, None); count];
    carbons(&places)
}

/// The pairs `first < second` of carbons `1..=count` that have a carbon in `layout`, ascending.
/// When all the carbons stand at one place and none is bonded, these are the overlaps of a preview
/// that lays out `layout`.
fn expected_overlaps(count: u32, layout: &[u32]) -> Vec<(AtomId, AtomId)> {
    let mut pairs = Vec::new();
    for first in 1..=count {
        for second in first + 1..=count {
            if layout.contains(&first) || layout.contains(&second) {
                pairs.push((atom(first), atom(second)));
            }
        }
    }
    pairs
}

#[test]
fn overlaps_up_to_the_listing_limit_are_all_listed() {
    // Fourteen laid out carbons at one place make 91 overlapping pairs, and all are listed.
    let layout: Vec<u32> = (1..=14).collect();
    let preview = preview_of(&coincident_carbons(14), &layout).unwrap();
    let expected = expected_overlaps(14, &layout);
    assert_eq!(expected.len(), 91, "fourteen carbons make 91 pairs");
    assert_eq!(preview.overlap_count, 91, "every pair is counted");
    assert_eq!(preview.overlaps, expected, "every pair is listed");
}

#[test]
fn overlaps_past_the_listing_limit_are_counted_and_the_first_hundred_listed() {
    // Fifteen laid out carbons at one place make 105 overlapping pairs. The first 100 in ascending
    // order are listed.
    let layout: Vec<u32> = (1..=15).collect();
    let preview = preview_of(&coincident_carbons(15), &layout).unwrap();
    let expected = expected_overlaps(15, &layout);
    assert_eq!(expected.len(), 105, "fifteen carbons make 105 pairs");
    assert_eq!(preview.overlap_count, 105, "every pair is counted");
    assert_eq!(
        preview.overlaps,
        expected[..100].to_vec(),
        "the first 100 pairs in ascending order are listed"
    );
}

#[test]
fn the_listed_overlaps_are_the_smallest_pairs_when_some_carbons_stay_fixed() {
    // Carbons 1 to 10 stay fixed and carbons 11 to 30 are laid out, all at one place and none
    // bonded. Each pair with a laid out carbon overlaps: 390 pairs. The first 100 in ascending
    // order run from (1, 11) to (5, 30), the pairs of carbons 1 to 5 with carbons 11 to 30.
    let layout: Vec<u32> = (11..=30).collect();
    let preview = preview_of(&coincident_carbons(30), &layout).unwrap();
    let expected = expected_overlaps(30, &layout);
    assert_eq!(expected.len(), 390, "the pairs with a laid out carbon");
    assert_eq!(preview.overlap_count, 390, "every pair is counted");
    assert_eq!(preview.overlaps.len(), 100, "only 100 pairs are listed");
    assert_eq!(preview.overlaps[0], (atom(1), atom(11)));
    assert_eq!(preview.overlaps[99], (atom(5), atom(30)));
    assert_eq!(
        preview.overlaps,
        expected[..100].to_vec(),
        "the listed pairs are the first 100 in ascending order"
    );
}

#[test]
fn a_placed_position_beyond_the_coordinate_limit_is_refused_at_its_atom() {
    // Two bonded carbons at the limit. The layout keeps their centroid, so carbon 1 lands about 13 pt
    // inside the limit and carbon 2 about 13 pt beyond it.
    let document = carbons(&[(MAX_COORDINATE, 0.0, None), (MAX_COORDINATE, 0.0, Some(0))]);
    let prepared = prepare_of(&document, &[1, 2]).unwrap();
    assert_eq!(
        layout::place(&prepared),
        Err(LayoutError::CoordinateOutOfRange { atom: atom(2) })
    );
}

#[test]
fn a_cancelled_placement_stops_with_cancelled() {
    let prepared = prepare_of(&butane_row(), &[1, 2, 3, 4]).unwrap();
    let cancel = CancelFlag::new();
    cancel.cancel();
    assert_eq!(
        layout::place_with(&prepared, &cancel),
        Err(Interrupted::Cancelled)
    );
}

#[test]
fn the_interrupt_is_asked_before_each_component_atom_and_overlap_row() {
    // A butane and a separate carbon: two components and five layout atoms.
    let document = carbons(&[
        (0.0, 0.0, None),
        (30.0, 0.0, Some(0)),
        (60.0, 0.0, Some(1)),
        (90.0, 0.0, Some(2)),
        (300.0, 300.0, None),
    ]);
    let prepared = prepare_of(&document, &[1, 2, 3, 4, 5]).unwrap();
    let probe = Probe::new(None);
    let placed = layout::place_with(&prepared, &probe).unwrap().unwrap();
    assert_eq!(placed, layout::place(&prepared).unwrap());
    // One question per component, one before each placed atom, and one for each layout atom of the
    // overlap scan: 2 + 2 * 5.
    assert_eq!(probe.asked.get(), 2 + 2 * 5);
}

#[test]
fn a_stop_at_the_last_question_stops_placement_and_a_later_stop_does_not() {
    let document = carbons(&[
        (0.0, 0.0, None),
        (30.0, 0.0, Some(0)),
        (60.0, 0.0, Some(1)),
        (90.0, 0.0, Some(2)),
        (300.0, 300.0, None),
    ]);
    let prepared = prepare_of(&document, &[1, 2, 3, 4, 5]).unwrap();
    let last_question = Probe::new(Some(2 + 2 * 5));
    assert_eq!(
        layout::place_with(&prepared, &last_question),
        Err(Interrupted::TimeLimit)
    );
    assert_eq!(last_question.asked.get(), 2 + 2 * 5);
    let past_the_end = Probe::new(Some(2 + 2 * 5 + 1));
    assert_eq!(
        layout::place_with(&prepared, &past_the_end)
            .unwrap()
            .unwrap(),
        layout::place(&prepared).unwrap()
    );
    assert_eq!(past_the_end.asked.get(), 2 + 2 * 5);
}
