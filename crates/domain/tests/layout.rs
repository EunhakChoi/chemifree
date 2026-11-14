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
fn ring_atoms_in_the_layout_set_are_refused_and_a_fixed_ring_is_only_counted() {
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
    assert_eq!(prepared.fixed_ring_systems, 1);
    assert_eq!(prepared.fixed_atom_count, 6);
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
fn an_abbreviation_keyed_at_a_layout_atom_is_refused_and_one_keyed_at_a_fixed_atom_is_not() {
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
    let error = prepare_of(&document, &[2, 3]).unwrap_err();
    assert_eq!(
        error,
        LayoutError::DisplayInLayoutSet {
            atoms: vec![atom(2)],
        }
    );
    assert_eq!(error.reason(), "display_in_layout_set");
    // With carbon 2 fixed, the display it keys does not block the oxygen.
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

#[test]
fn a_pair_closer_than_half_a_bond_is_reported_once_and_a_pair_at_exactly_half_is_not() {
    // Carbon 2 is laid out between the fixed carbons 1 and 3: 10 pt from carbon 1 and 15 pt from
    // carbon 3, which is half a bond and does not count.
    let document = carbons(&[(0.0, 0.0, None), (10.0, 0.0, None), (25.0, 0.0, None)]);
    let preview = preview_of(&document, &[2]).unwrap();
    assert_eq!(preview.overlaps, vec![(atom(1), atom(2))]);
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
