//! The command `set_configuration_from_drawing`: which items it takes, what it skips and what
//! it refuses, the instance it reads, that it stores through the ordinary stereo table (so
//! it is idempotent and undoable as one step), and that nothing it does breaks a document
//! invariant. The expected configurations are the hand-computed ones of `stereo_drawing.rs`
//! (E1: F top, Cl lower right with a wedge at the center, Br lower left, the fourth position
//! a hydrogen, clockwise over `[F, Cl, Br, H]`; E3: Br moved to the upper right, anticlockwise).

use std::collections::BTreeMap;

use chemifree_domain::document::{
    Applied, Atom, AtomEdit, AtomId, AtomRef, Bond, BondId, BondOrder, BondRef, BondTo, Command,
    Document, DocumentId, EditError, NewAtomPositions, ObjectId, Outcome, Point, TransactionError,
    Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::TransactionSelection;
use chemifree_domain::states::{AtomKind, Hydrogens, QueryAtom};
use chemifree_domain::stereo::{
    AtomConfiguration, BondConfiguration, BondEnd, CenterState, DoubleBondState, Relation, Slot,
    SlotRef, StereoError, StereoKey, StereoMark, StereoState, Turn,
};
use chemifree_domain::stereo_drawing::{DeriveError, Drawing, NotDerivable};

fn aid(serial: u32) -> AtomId {
    AtomId::from_serial(serial).unwrap()
}

fn bid(serial: u32) -> BondId {
    BondId::from_serial(serial).unwrap()
}

fn slot(serial: u32) -> Slot {
    Slot::Atom(aid(serial))
}

fn wedge(narrow: BondEnd) -> Option<StereoMark> {
    Some(StereoMark::Wedge { narrow })
}

/// Builds a document by commands: atoms `a1...` and bonds `b1...` in the order they are added.
#[derive(Default)]
struct Build {
    commands: Vec<Command>,
    atoms: u32,
    bonds: u32,
}

impl Build {
    fn atom(&mut self, symbol: &str, x: f64, y: f64) -> u32 {
        self.atom_as(Atom::from(Element::from_symbol(symbol).unwrap()), x, y)
    }

    fn atom_as(&mut self, atom: Atom, x: f64, y: f64) -> u32 {
        self.commands.push(Command::AddAtom {
            atom,
            positions: NewAtomPositions::Single(Point::new(x, y).unwrap()),
            bond_to: None,
        });
        self.atoms += 1;
        self.atoms
    }

    fn bond(&mut self, begin: u32, end: u32, order: BondOrder) -> u32 {
        self.commands.push(Command::AddBond {
            begin: AtomRef::Id(aid(begin)),
            end: AtomRef::Id(aid(end)),
            order,
        });
        self.bonds += 1;
        self.bonds
    }

    fn mark(&mut self, bond: u32, mark: Option<StereoMark>) {
        self.commands.push(set_mark(bond, mark));
    }

    /// Carbon at (x, y) with F above, Cl lower right and Br lower left (E1 geometry), bonded by
    /// single bonds from the carbon; returns the carbon and the three bonds in that order.
    fn halo(&mut self, x: f64, y: f64) -> (u32, [u32; 3]) {
        let center = self.atom("C", x, y);
        let f = self.atom("F", x, y - 30.0);
        let cl = self.atom("Cl", x + 26.0, y + 15.0);
        let br = self.atom("Br", x - 26.0, y + 15.0);
        let bonds = [f, cl, br].map(|end| self.bond(center, end, BondOrder::Single));
        (center, bonds)
    }

    /// `ends` atoms around a carbon at (x, y) at the given offsets, all single bonds from it.
    fn star(&mut self, x: f64, y: f64, offsets: &[(f64, f64)]) -> (u32, Vec<u32>) {
        let center = self.atom("C", x, y);
        let bonds = offsets
            .iter()
            .map(|&(dx, dy)| {
                let end = self.atom("C", x + dx, y + dy);
                self.bond(center, end, BondOrder::Single)
            })
            .collect();
        (center, bonds)
    }

    fn document(&self) -> Document {
        let mut document =
            Document::new(DocumentId::parse("doc-derive").unwrap(), "Derive").unwrap();
        document.apply(&self.commands).unwrap();
        document
    }
}

fn set_mark(bond: u32, mark: Option<StereoMark>) -> Command {
    Command::SetBondMark {
        bond: BondRef::Id(bid(bond)),
        mark,
    }
}

fn derive(atoms: Option<&[u32]>, bonds: Option<&[u32]>) -> Command {
    derive_in(atoms, bonds, None)
}

fn derive_in(atoms: Option<&[u32]>, bonds: Option<&[u32]>, instance: Option<ObjectId>) -> Command {
    Command::SetConfigurationFromDrawing {
        atoms: atoms.map(|atoms| atoms.iter().map(|&n| AtomRef::Id(aid(n))).collect()),
        bonds: bonds.map(|bonds| bonds.iter().map(|&n| BondRef::Id(bid(n))).collect()),
        instance,
    }
}

fn run(document: &mut Document, commands: &[Command]) -> Applied {
    document
        .apply_reporting(commands, &TransactionSelection::default())
        .unwrap_or_else(|error| panic!("{error}"))
}

fn refuse(document: &mut Document, commands: &[Command]) -> TransactionError {
    let before = document.clone();
    let error = document
        .apply_reporting(commands, &TransactionSelection::default())
        .expect_err("the transaction is refused");
    assert_eq!(*document, before, "a refused transaction changes nothing");
    error
}

fn center(document: &Document, serial: u32) -> Option<CenterState> {
    match document.stereo().get(StereoKey::Center(aid(serial))) {
        Some(StereoState::Center(state)) => Some(*state),
        Some(other) => panic!("not a center: {other:?}"),
        None => None,
    }
}

fn double_bond(document: &Document, serial: u32) -> Option<DoubleBondState> {
    match document.stereo().get(StereoKey::DoubleBond(bid(serial))) {
        Some(StereoState::DoubleBond(state)) => Some(*state),
        Some(other) => panic!("not a double bond: {other:?}"),
        None => None,
    }
}

fn tetra(slots: [Slot; 4], turn: Turn) -> CenterState {
    CenterState::tetrahedral(slots, turn).unwrap()
}

fn e1_clockwise() -> CenterState {
    tetra(
        [slot(2), slot(3), slot(4), Slot::ImplicitHydrogen],
        Turn::Clockwise,
    )
}

/// The one halo, with the wedge on the Cl bond (`b2`), narrow end at the center.
fn wedged_halo() -> Document {
    let mut build = Build::default();
    let (_, bonds) = build.halo(100.0, 100.0);
    build.mark(bonds[1], wedge(BondEnd::Begin));
    build.document()
}

/// The reason of a refusal of a named center or double bond, and its command index.
fn cause_of(error: &TransactionError) -> (StereoKey, DeriveError, Option<usize>) {
    match &error.error {
        EditError::FromDrawing { item, cause } => (*item, *cause, error.index),
        other => panic!("not a drawing refusal: {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// What is stored
// ---------------------------------------------------------------------------------------------

#[test]
fn a_named_center_is_stored_as_the_hand_computed_e1_configuration() {
    let mut document = wedged_halo();
    let applied = run(&mut document, &[derive(Some(&[1]), None)]);
    assert_eq!(applied.outcome, Outcome::Changed);
    assert!(applied.cleared.is_empty());
    assert_eq!(center(&document, 1), Some(e1_clockwise()));
    assert_eq!(document.stereo().len(), 1);
    // Reading the document back from its parts shows the same table.
    assert_eq!(Document::restore(document.to_parts()).unwrap(), document);
}

/// The no-list scope, one item of each kind (the offsets are E1 around every carbon).
#[test]
fn without_a_list_exactly_the_items_a_mark_bears_on_are_stored() {
    let mut build = Build::default();
    // a1..a4: marked halo, in scope.
    let (_, marked) = build.halo(100.0, 100.0);
    build.mark(marked[1], wedge(BondEnd::Begin));
    // a5..a8: the same drawing without a mark, not in scope.
    build.halo(300.0, 100.0);
    // a9..a12: a wedge whose narrow end is the wide-end atom Cl: says nothing about the center.
    let (_, narrow_elsewhere) = build.halo(500.0, 100.0);
    build.mark(narrow_elsewhere[1], wedge(BondEnd::End));
    // a13..a16: wavy at the center, in scope as unknown.
    let (_, wavy) = build.halo(700.0, 100.0);
    build.mark(wavy[1], Some(StereoMark::Wavy));
    // a17, a18: a wedge whose narrow end is a terminal atom: it could never hold a record.
    let carbon = build.atom("C", 900.0, 100.0);
    let oxygen = build.atom("O", 930.0, 100.0);
    let terminal = build.bond(carbon, oxygen, BondOrder::Single);
    build.mark(terminal, wedge(BondEnd::End));
    // a19=a20 with a21 on a19 and a22 on a20, crossed: in scope as unknown.
    let (c1, c2) = (
        build.atom("C", 1100.0, 100.0),
        build.atom("C", 1130.0, 100.0),
    );
    let (r1, r2) = (
        build.atom("C", 1085.0, 80.0),
        build.atom("C", 1145.0, 120.0),
    );
    let crossed = build.bond(c1, c2, BondOrder::Double);
    build.bond(c1, r1, BondOrder::Single);
    build.bond(c2, r2, BondOrder::Single);
    build.mark(crossed, Some(StereoMark::Crossed));
    // The same without the crossed mark: not in scope.
    let (d1, d2) = (
        build.atom("C", 1300.0, 100.0),
        build.atom("C", 1330.0, 100.0),
    );
    let (s1, s2) = (
        build.atom("C", 1285.0, 80.0),
        build.atom("C", 1345.0, 120.0),
    );
    build.bond(d1, d2, BondOrder::Double);
    build.bond(d1, s1, BondOrder::Single);
    build.bond(d2, s2, BondOrder::Single);
    // A crossed mark on a cumulated double bond could never hold a record either.
    let (e1, e2, e3) = (
        build.atom("C", 1500.0, 100.0),
        build.atom("C", 1530.0, 100.0),
        build.atom("C", 1560.0, 100.0),
    );
    let cumulated = build.bond(e1, e2, BondOrder::Double);
    build.bond(e2, e3, BondOrder::Double);
    build.mark(cumulated, Some(StereoMark::Crossed));
    let mut document = build.document();

    let applied = run(&mut document, &[derive(None, None)]);
    assert_eq!(applied.outcome, Outcome::Changed);
    let stored: Vec<StereoKey> = document.stereo().iter().map(|(key, _)| key).collect();
    assert_eq!(
        stored,
        [
            StereoKey::Center(aid(1)),
            StereoKey::Center(aid(13)),
            StereoKey::DoubleBond(bid(crossed)),
        ]
    );
    assert_eq!(center(&document, 1), Some(e1_clockwise()));
    assert_eq!(
        center(&document, 13),
        Some(CenterState::unknown([slot(14), slot(15), slot(16), Slot::ImplicitHydrogen]).unwrap())
    );
    assert_eq!(
        double_bond(&document, crossed),
        Some(DoubleBondState::Unknown)
    );
    assert_eq!(Document::restore(document.to_parts()).unwrap(), document);
}

#[test]
fn a_list_restricts_the_command_to_the_named_kind() {
    let mut build = Build::default();
    let (_, bonds) = build.halo(100.0, 100.0);
    build.mark(bonds[1], wedge(BondEnd::Begin));
    let (c1, c2) = (build.atom("C", 300.0, 100.0), build.atom("C", 330.0, 100.0));
    let crossed = build.bond(c1, c2, BondOrder::Double);
    build.mark(crossed, Some(StereoMark::Crossed));
    let mut only_bonds = build.document();
    run(&mut only_bonds, &[derive(None, Some(&[crossed]))]);
    assert_eq!(
        only_bonds
            .stereo()
            .iter()
            .map(|(key, _)| key)
            .collect::<Vec<_>>(),
        [StereoKey::DoubleBond(bid(crossed))],
        "naming only a bond derives no center, even a marked one"
    );
    let mut only_atoms = build.document();
    run(&mut only_atoms, &[derive(Some(&[1]), None)]);
    assert_eq!(
        only_atoms
            .stereo()
            .iter()
            .map(|(key, _)| key)
            .collect::<Vec<_>>(),
        [StereoKey::Center(aid(1))],
        "naming only an atom derives no bond"
    );
}

#[test]
fn a_cleared_mark_leaves_the_center_out_of_the_scope() {
    let mut document = wedged_halo();
    run(&mut document, &[set_mark(2, None)]);
    let applied = run(&mut document, &[derive(None, None)]);
    assert_eq!(applied.outcome, Outcome::Unchanged);
    assert!(document.stereo().is_empty());
}

// ---------------------------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------------------------

#[test]
fn without_a_list_a_markable_item_the_drawing_does_not_determine_refuses_the_whole_command() {
    // a1 is fine; a5 is wedge-marked on both a wedge and an opposite hash, so it is flat.
    let mut build = Build::default();
    let (_, halo) = build.halo(100.0, 100.0);
    build.mark(halo[1], wedge(BondEnd::Begin));
    let (_, cross) = build.star(
        300.0,
        100.0,
        &[(0.0, -30.0), (30.0, 0.0), (0.0, 30.0), (-30.0, 0.0)],
    );
    build.mark(cross[0], wedge(BondEnd::Begin));
    build.mark(
        cross[2],
        Some(StereoMark::Hash {
            narrow: BondEnd::Begin,
        }),
    );
    let mut document = build.document();
    let error = refuse(&mut document, &[derive(None, None)]);
    assert_eq!(
        cause_of(&error),
        (
            StereoKey::Center(aid(5)),
            DeriveError::NotDerivable(NotDerivable::Degenerate),
            Some(0)
        ),
        "the first center was derived but is not stored: nothing is"
    );
    assert_eq!(
        error.error.reason(),
        Some("stereo_not_derivable_degenerate")
    );
    assert!(document.stereo().is_empty());
    assert_eq!(
        error.error.to_string(),
        "atom a5: the drawing is too flat to determine a configuration"
    );
}

#[test]
fn a_named_item_that_is_underivable_refuses_with_its_reason_and_stores_none_of_the_others() {
    let mut build = Build::default();
    let (_, halo) = build.halo(100.0, 100.0);
    build.mark(halo[1], wedge(BondEnd::Begin));
    // a5: a plain halo, so `no_marks`.
    build.halo(300.0, 100.0);
    let mut document = build.document();
    let error = refuse(
        &mut document,
        &[derive(Some(&[1]), None), derive(Some(&[1, 5]), None)],
    );
    assert_eq!(
        cause_of(&error),
        (
            StereoKey::Center(aid(5)),
            DeriveError::NotDerivable(NotDerivable::NoMarks),
            Some(1)
        )
    );
    assert_eq!(error.error.reason(), Some("stereo_not_derivable_no_marks"));
}

#[test]
fn each_reason_a_drawing_can_fail_to_determine_a_named_item_is_reported() {
    let reason = |document: &mut Document, command: Command| match refuse(document, &[command]) {
        TransactionError {
            error: EditError::FromDrawing { cause, .. },
            ..
        } => cause,
        other => panic!("{other:?}"),
    };

    // No mark bears on the center.
    let mut build = Build::default();
    build.halo(100.0, 100.0);
    assert_eq!(
        reason(&mut build.document(), derive(Some(&[1]), None)),
        DeriveError::NotDerivable(NotDerivable::NoMarks)
    );

    // A wavy bond to a terminal atom beside the center's own wedge.
    let mut build = Build::default();
    let (_, bonds) = build.halo(100.0, 100.0);
    build.mark(bonds[1], wedge(BondEnd::Begin));
    build.mark(bonds[2], Some(StereoMark::Wavy));
    assert_eq!(
        reason(&mut build.document(), derive(Some(&[1]), None)),
        DeriveError::NotDerivable(NotDerivable::ConflictingMarks)
    );

    // A neighbor on top of the center.
    let mut build = Build::default();
    let (_, bonds) = build.halo(100.0, 100.0);
    build.mark(bonds[1], wedge(BondEnd::Begin));
    let mut document = build.document();
    run(
        &mut document,
        &[Command::MoveAtoms {
            atoms: vec![aid(3)],
            delta: Vector {
                dx: -26.0,
                dy: -15.0,
            },
            instance: None,
        }],
    );
    assert_eq!(
        reason(&mut document, derive(Some(&[1]), None)),
        DeriveError::NotDerivable(NotDerivable::CoincidentAtoms)
    );

    // A flat arrangement: opposite wedge and hash.
    let mut build = Build::default();
    let (_, bonds) = build.star(
        100.0,
        100.0,
        &[(0.0, -30.0), (30.0, 0.0), (0.0, 30.0), (-30.0, 0.0)],
    );
    build.mark(bonds[0], wedge(BondEnd::Begin));
    build.mark(
        bonds[2],
        Some(StereoMark::Hash {
            narrow: BondEnd::Begin,
        }),
    );
    assert_eq!(
        reason(&mut build.document(), derive(Some(&[1]), None)),
        DeriveError::NotDerivable(NotDerivable::Degenerate)
    );

    // A carbonyl carbon: its fourth position cannot be named.
    let mut build = Build::default();
    let carbon = build.atom("C", 100.0, 100.0);
    let oxygen = build.atom("O", 100.0, 70.0);
    let fluorine = build.atom("F", 126.0, 115.0);
    let chlorine = build.atom("Cl", 74.0, 115.0);
    build.bond(carbon, oxygen, BondOrder::Double);
    let marked = build.bond(carbon, fluorine, BondOrder::Single);
    build.bond(carbon, chlorine, BondOrder::Single);
    build.mark(marked, wedge(BondEnd::Begin));
    assert_eq!(
        reason(&mut build.document(), derive(Some(&[1]), None)),
        DeriveError::NotDerivable(NotDerivable::FourthPosition)
    );

    // A double bond whose end has no other neighbor.
    let mut build = Build::default();
    let (c1, c2, c3) = (
        build.atom("C", 0.0, 0.0),
        build.atom("C", 30.0, 0.0),
        build.atom("C", 50.0, 20.0),
    );
    let double = build.bond(c1, c2, BondOrder::Double);
    build.bond(c2, c3, BondOrder::Single);
    assert_eq!(
        reason(&mut build.document(), derive(None, Some(&[double]))),
        DeriveError::NotDerivable(NotDerivable::NoReference)
    );
}

#[test]
fn a_named_item_the_model_cannot_hold_is_refused_with_the_existing_structural_reason() {
    let structural = |document: &mut Document, command: Command| {
        let error = refuse(document, &[command]);
        let (_, cause, _) = cause_of(&error);
        (cause, error.error.reason())
    };

    // A terminal atom is not a center.
    let mut document = wedged_halo();
    assert_eq!(
        structural(&mut document, derive(Some(&[2]), None)),
        (
            DeriveError::Structure(StereoError::CenterUnsupported),
            Some("stereo_center_unsupported")
        )
    );
    // A single bond is not a double bond.
    assert_eq!(
        structural(&mut document, derive(None, Some(&[1]))),
        (
            DeriveError::Structure(StereoError::DoubleBondRequired),
            Some("stereo_double_bond_required")
        )
    );
    // A cumulated double bond.
    let mut build = Build::default();
    let (a, b, c) = (
        build.atom("C", 0.0, 0.0),
        build.atom("C", 30.0, 0.0),
        build.atom("C", 60.0, 0.0),
    );
    let first = build.bond(a, b, BondOrder::Double);
    build.bond(b, c, BondOrder::Double);
    assert_eq!(
        structural(&mut build.document(), derive(None, Some(&[first]))),
        (
            DeriveError::Structure(StereoError::GeometryUnsupported),
            Some("stereo_geometry_unsupported")
        )
    );
    // An end with three other neighbors.
    let mut build = Build::default();
    let (a, b) = (build.atom("C", 0.0, 0.0), build.atom("C", 30.0, 0.0));
    let double = build.bond(a, b, BondOrder::Double);
    for (x, y) in [(-20.0, 20.0), (-20.0, -20.0), (0.0, 30.0)] {
        let other = build.atom("C", x, y);
        build.bond(a, other, BondOrder::Single);
    }
    assert_eq!(
        structural(&mut build.document(), derive(None, Some(&[double]))),
        (
            DeriveError::Structure(StereoError::ReferenceInvalid),
            Some("stereo_reference_invalid")
        )
    );
}

#[test]
fn lists_follow_the_usual_target_rules() {
    let mut document = wedged_halo();
    let empty_atoms = refuse(&mut document, &[derive(Some(&[]), None)]);
    assert_eq!(empty_atoms.error, EditError::EmptyTargetList);
    let empty_bonds = refuse(&mut document, &[derive(None, Some(&[]))]);
    assert_eq!(empty_bonds.error, EditError::EmptyTargetList);
    let unknown_atom = refuse(&mut document, &[derive(Some(&[9]), None)]);
    assert_eq!(unknown_atom.error, EditError::UnknownAtom(aid(9)));
    let unknown_bond = refuse(&mut document, &[derive(None, Some(&[9]))]);
    assert_eq!(unknown_bond.error, EditError::UnknownBond(bid(9)));
    // A repeated name derives once.
    let applied = run(&mut document, &[derive(Some(&[1, 1]), None)]);
    assert_eq!(applied.outcome, Outcome::Changed);
    assert_eq!(center(&document, 1), Some(e1_clockwise()));
}

/// A tetrahedral configuration of `center` over three atom neighbors and an implicit hydrogen.
fn state_center(center: u32, neighbors: [u32; 3], turn: Turn) -> Command {
    let [a, b, c] = neighbors.map(|n| SlotRef::Atom(AtomRef::Id(aid(n))));
    Command::SetAtomConfiguration {
        atom: AtomRef::Id(aid(center)),
        configuration: Some(AtomConfiguration::Tetrahedral {
            neighbors: [a, b, c, SlotRef::ImplicitHydrogen],
            turn,
        }),
    }
}

#[test]
fn the_command_replaces_a_stated_record_of_every_item_it_processes_and_writes_nothing_else() {
    let mut build = Build::default();
    // a1..a4: marked halo whose stated configuration is `unknown`, a statement the drawing
    // contradicts.
    let (_, marked) = build.halo(100.0, 100.0);
    build.mark(marked[1], wedge(BondEnd::Begin));
    // a5..a8: unmarked halo with a stated turn: no mark bears on it, so it is not processed.
    build.halo(300.0, 100.0);
    // a9=a10 with a11 on a9 and a12 on a10: crossed, with a stated relation.
    let (c1, c2) = (build.atom("C", 500.0, 100.0), build.atom("C", 530.0, 100.0));
    let (r1, r2) = (build.atom("C", 485.0, 80.0), build.atom("C", 545.0, 120.0));
    let crossed = build.bond(c1, c2, BondOrder::Double);
    build.bond(c1, r1, BondOrder::Single);
    build.bond(c2, r2, BondOrder::Single);
    build.mark(crossed, Some(StereoMark::Crossed));
    // a13, a14: a wedge whose narrow end is a terminal atom: skipped.
    let carbon = build.atom("C", 700.0, 100.0);
    let oxygen = build.atom("O", 730.0, 100.0);
    let terminal = build.bond(carbon, oxygen, BondOrder::Single);
    build.mark(terminal, wedge(BondEnd::End));
    let mut document = build.document();
    let unknown_center =
        CenterState::unknown([slot(2), slot(3), slot(4), Slot::ImplicitHydrogen]).unwrap();
    run(
        &mut document,
        &[
            Command::SetAtomConfiguration {
                atom: AtomRef::Id(aid(1)),
                configuration: Some(AtomConfiguration::Unknown {
                    neighbors: [
                        SlotRef::Atom(AtomRef::Id(aid(2))),
                        SlotRef::Atom(AtomRef::Id(aid(3))),
                        SlotRef::Atom(AtomRef::Id(aid(4))),
                        SlotRef::ImplicitHydrogen,
                    ],
                }),
            },
            state_center(5, [6, 7, 8], Turn::Anticlockwise),
            Command::SetBondConfiguration {
                bond: BondRef::Id(bid(crossed)),
                configuration: Some(BondConfiguration::Relative {
                    begin_neighbor: SlotRef::Atom(AtomRef::Id(aid(r1))),
                    end_neighbor: SlotRef::Atom(AtomRef::Id(aid(r2))),
                    relation: Relation::OppositeSides,
                }),
            },
        ],
    );
    let stated_relation = double_bond(&document, crossed).unwrap();
    assert_ne!(stated_relation, DoubleBondState::Unknown);
    let stated_turn = center(&document, 5);
    assert_eq!(center(&document, 1), Some(unknown_center));
    let before = document.clone();

    // A list restricts the overwriting to what it names: the other items keep their records.
    run(&mut document, &[derive(Some(&[1]), None)]);
    assert_eq!(
        center(&document, 1),
        Some(e1_clockwise()),
        "unknown is replaced"
    );
    assert_eq!(double_bond(&document, crossed), Some(stated_relation));

    // Without a list, every marked item is processed and replaces its stated record.
    let mut document = before;
    let applied = run(&mut document, &[derive(None, None)]);
    assert_eq!(applied.outcome, Outcome::Changed);
    assert!(applied.cleared.is_empty());
    assert_eq!(
        center(&document, 1),
        Some(e1_clockwise()),
        "unknown is replaced"
    );
    assert_eq!(
        double_bond(&document, crossed),
        Some(DoubleBondState::Unknown),
        "a stated relation is replaced by the crossed mark's unknown"
    );
    // Nothing else is written: the unmarked center keeps its statement and the skipped terminal
    // wedge stores nothing.
    assert_eq!(center(&document, 5), stated_turn);
    assert_eq!(document.stereo().len(), 3);
    assert_eq!(center(&document, carbon), None);
    // The mark does not block a stated configuration at write time, and the drawing is not
    // changed by the command.
    assert_eq!(document.bond(bid(2)).unwrap().mark, wedge(BondEnd::Begin));
    document.undo().unwrap();
    assert_eq!(center(&document, 1), Some(unknown_center));
    assert_eq!(double_bond(&document, crossed), Some(stated_relation));
}

/// A crossed double bond between two new carbons at `x` with `begin_others` other single-bonded
/// neighbors at its begin atom and `end_others` at its end atom, and, when `cumulated_at` is
/// given, a further double bond on that end. Returns the bond.
fn crossed_bond(
    build: &mut Build,
    x: f64,
    (begin_others, end_others): (u32, u32),
    cumulated_at: Option<BondEnd>,
) -> u32 {
    let begin = build.atom("C", x, 100.0);
    let end = build.atom("C", x + 30.0, 100.0);
    let bond = build.bond(begin, end, BondOrder::Double);
    for (atom, count, dx) in [(begin, begin_others, -20.0), (end, end_others, 50.0)] {
        for index in 0..count {
            let other = build.atom("C", x + dx, 60.0 + 20.0 * f64::from(index));
            build.bond(atom, other, BondOrder::Single);
        }
    }
    if let Some(side) = cumulated_at {
        let (atom, dx) = match side {
            BondEnd::Begin => (begin, -30.0),
            BondEnd::End => (end, 60.0),
        };
        let other = build.atom("C", x + dx, 100.0);
        build.bond(atom, other, BondOrder::Double);
    }
    build.mark(bond, Some(StereoMark::Crossed));
    bond
}

#[test]
fn without_a_list_a_crossed_bond_the_model_cannot_hold_is_skipped_for_every_structural_cause() {
    let mut build = Build::default();
    let (_, halo) = build.halo(100.0, 100.0);
    build.mark(halo[1], wedge(BondEnd::Begin));
    let held_back = [
        // Three other neighbors at one end, either end: ReferenceInvalid.
        (
            crossed_bond(&mut build, 300.0, (3, 1), None),
            StereoError::ReferenceInvalid,
        ),
        (
            crossed_bond(&mut build, 500.0, (1, 3), None),
            StereoError::ReferenceInvalid,
        ),
        // A cumulated double bond at one end, either end: GeometryUnsupported.
        (
            crossed_bond(&mut build, 700.0, (1, 1), Some(BondEnd::Begin)),
            StereoError::GeometryUnsupported,
        ),
        (
            crossed_bond(&mut build, 900.0, (1, 1), Some(BondEnd::End)),
            StereoError::GeometryUnsupported,
        ),
    ];
    let mut document = build.document();

    // Named, each is refused with the structural reason the skip hides.
    for (bond, expected) in held_back {
        let error = refuse(&mut document, &[derive(None, Some(&[bond]))]);
        assert_eq!(
            cause_of(&error).1,
            DeriveError::Structure(expected),
            "bond b{bond}"
        );
    }

    // Without a list they are skipped, and the center beside them is stored.
    let applied = run(&mut document, &[derive(None, None)]);
    assert_eq!(applied.outcome, Outcome::Changed);
    assert!(applied.cleared.is_empty());
    assert_eq!(
        document
            .stereo()
            .iter()
            .map(|(key, _)| key)
            .collect::<Vec<_>>(),
        [StereoKey::Center(aid(1))],
        "a bond the model cannot hold gets neither a record nor a refusal"
    );

    // With nothing else in the scope the command changes nothing.
    let mut only_skipped = Build::default();
    crossed_bond(&mut only_skipped, 300.0, (3, 1), None);
    let mut document = only_skipped.document();
    let applied = run(&mut document, &[derive(None, None)]);
    assert_eq!(applied.outcome, Outcome::Unchanged);
    assert!(document.stereo().is_empty());
}

#[test]
fn duplicates_merge_and_a_failing_item_is_named_by_lowest_id_not_by_list_position() {
    let mut build = Build::default();
    // a1..a4: derivable. a5..a9: flat (wedge opposite hash). a10..a13: no mark.
    let (_, halo) = build.halo(100.0, 100.0);
    build.mark(halo[1], wedge(BondEnd::Begin));
    let (_, cross) = build.star(
        300.0,
        100.0,
        &[(0.0, -30.0), (30.0, 0.0), (0.0, 30.0), (-30.0, 0.0)],
    );
    build.mark(cross[0], wedge(BondEnd::Begin));
    build.mark(
        cross[2],
        Some(StereoMark::Hash {
            narrow: BondEnd::Begin,
        }),
    );
    build.halo(500.0, 100.0);
    let mut document = build.document();

    // Both fail; the higher id comes first in the list, the lower id is named.
    let error = refuse(&mut document, &[derive(Some(&[10, 5]), None)]);
    assert_eq!(
        cause_of(&error),
        (
            StereoKey::Center(aid(5)),
            DeriveError::NotDerivable(NotDerivable::Degenerate),
            Some(0)
        ),
        "a5 is named although a10 was listed first"
    );
    let error = refuse(&mut document, &[derive(Some(&[10, 10, 5, 5]), None)]);
    assert_eq!(cause_of(&error).0, StereoKey::Center(aid(5)));

    // Centers are checked before double bonds, whatever the ids: b1 is a single bond (refused
    // with the structural reason) but the unmarked center a10 is named first.
    let error = refuse(&mut document, &[derive(Some(&[10]), Some(&[1]))]);
    assert_eq!(cause_of(&error).0, StereoKey::Center(aid(10)));

    // Repeats, also through `$N`, are one item: the result equals the single-item command.
    let mut once = wedged_halo();
    run(&mut once, &[derive(Some(&[1]), None)]);
    let mut repeated = wedged_halo();
    let applied = run(&mut repeated, &[derive(Some(&[1, 1, 1]), None)]);
    assert_eq!(applied.outcome, Outcome::Changed);
    assert_eq!(repeated, once);

    let mut created = Document::new(DocumentId::parse("doc-dup").unwrap(), "Dup").unwrap();
    run(
        &mut created,
        &[
            created_atom("C", 100.0, 100.0, None),
            created_atom("F", 100.0, 70.0, Some(0)),
            created_atom("Cl", 126.0, 115.0, Some(0)),
            created_atom("Br", 74.0, 115.0, Some(0)),
            Command::SetBondMark {
                bond: BondRef::Created(2),
                mark: wedge(BondEnd::Begin),
            },
            // `a1` and `$0` are the same atom.
            Command::SetConfigurationFromDrawing {
                atoms: Some(vec![AtomRef::Id(aid(1)), AtomRef::Created(0)]),
                bonds: None,
                instance: None,
            },
        ],
    );
    assert_eq!(center(&created, 1), Some(e1_clockwise()));
    assert_eq!(created.stereo().len(), 1);
}

// ---------------------------------------------------------------------------------------------
// $N, instances, history
// ---------------------------------------------------------------------------------------------

fn created_atom(symbol: &str, x: f64, y: f64, bond_to: Option<usize>) -> Command {
    Command::AddAtom {
        atom: Atom::from(Element::from_symbol(symbol).unwrap()),
        positions: NewAtomPositions::Single(Point::new(x, y).unwrap()),
        bond_to: bond_to.map(|index| BondTo {
            atom: AtomRef::Created(index),
            order: BondOrder::Single,
        }),
    }
}

#[test]
fn atoms_and_bonds_created_earlier_in_the_transaction_can_be_named() {
    let mut document = Document::new(DocumentId::parse("doc-new").unwrap(), "New").unwrap();
    // `$2` is the bond made with the third atom, as for `set_bond_mark`.
    run(
        &mut document,
        &[
            created_atom("C", 100.0, 100.0, None),
            created_atom("F", 100.0, 70.0, Some(0)),
            created_atom("Cl", 126.0, 115.0, Some(0)),
            created_atom("Br", 74.0, 115.0, Some(0)),
            Command::SetBondMark {
                bond: BondRef::Created(2),
                mark: wedge(BondEnd::Begin),
            },
            Command::SetConfigurationFromDrawing {
                atoms: Some(vec![AtomRef::Created(0)]),
                bonds: None,
                instance: None,
            },
        ],
    );
    assert_eq!(center(&document, 1), Some(e1_clockwise()));

    // A bond made by `add_bond`, named as `$N`.
    let mut document = Document::new(DocumentId::parse("doc-bond").unwrap(), "Bond").unwrap();
    run(
        &mut document,
        &[
            created_atom("C", 0.0, 0.0, None),
            created_atom("C", 30.0, 0.0, None),
            Command::AddBond {
                begin: AtomRef::Created(0),
                end: AtomRef::Created(1),
                order: BondOrder::Double,
            },
            Command::SetBondMark {
                bond: BondRef::Created(2),
                mark: Some(StereoMark::Crossed),
            },
            Command::SetConfigurationFromDrawing {
                atoms: None,
                bonds: Some(vec![BondRef::Created(2)]),
                instance: None,
            },
        ],
    );
    assert_eq!(double_bond(&document, 1), Some(DoubleBondState::Unknown));

    // A reference that names nothing keeps the usual refusals.
    let mut document = wedged_halo();
    let error = refuse(
        &mut document,
        &[Command::SetConfigurationFromDrawing {
            atoms: Some(vec![AtomRef::Created(7)]),
            bonds: None,
            instance: None,
        }],
    );
    assert_eq!(error.error, EditError::UnresolvedReference(7));
    let error = refuse(
        &mut document,
        &[Command::SetConfigurationFromDrawing {
            atoms: None,
            bonds: Some(vec![BondRef::Created(7)]),
            instance: None,
        }],
    );
    assert_eq!(error.error, EditError::UnresolvedBondReference(7));
}

/// The wedged halo drawn twice; in the second instance Br is moved across the gap to the upper
/// right (E3), which reverses the turn.
fn two_instances() -> (Document, ObjectId, ObjectId) {
    let mut document = wedged_halo();
    let first = document.first_instance();
    run(
        &mut document,
        &[Command::AddInstance {
            source: None,
            offset: Vector { dx: 200.0, dy: 0.0 },
        }],
    );
    let second = document.instances().find(|&id| id != first).unwrap();
    run(
        &mut document,
        &[Command::MoveAtoms {
            atoms: vec![aid(4)],
            delta: Vector {
                dx: 52.0,
                dy: -30.0,
            },
            instance: Some(second),
        }],
    );
    (document, first, second)
}

#[test]
fn with_several_instances_the_command_must_name_the_one_it_reads() {
    let (mut document, first, second) = two_instances();
    let error = refuse(&mut document, &[derive(Some(&[1]), None)]);
    assert_eq!(error.error, EditError::InstanceRequired { instances: 2 });
    assert_eq!(error.index, Some(0));
    assert_eq!(error.error.reason(), None);
    assert_eq!(
        error.error.to_string(),
        "the molecule has 2 scene instances; name the `instance` to use"
    );
    let error = refuse(
        &mut document,
        &[derive_in(Some(&[1]), None, ObjectId::parse("s99"))],
    );
    assert_eq!(error.error.reason(), Some("object_unknown"));

    // The other instance's positions are ignored: the same marks give opposite turns.
    run(&mut document, &[derive_in(Some(&[1]), None, Some(first))]);
    assert_eq!(center(&document, 1), Some(e1_clockwise()));
    let applied = run(&mut document, &[derive_in(Some(&[1]), None, Some(second))]);
    assert_eq!(applied.outcome, Outcome::Changed);
    assert!(applied.cleared.is_empty(), "an overwrite is not a clear");
    assert_eq!(
        center(&document, 1),
        Some(tetra(
            [slot(2), slot(3), slot(4), Slot::ImplicitHydrogen],
            Turn::Anticlockwise
        ))
    );
    // Neither instance's positions or marks changed.
    assert_eq!(document.bond(bid(2)).unwrap().mark, wedge(BondEnd::Begin));
}

#[test]
fn deriving_twice_changes_nothing_the_second_time() {
    let mut document = wedged_halo();
    run(&mut document, &[derive(Some(&[1]), None)]);
    let (revision, depth, snapshot) =
        (document.revision(), document.undo_depth(), document.clone());
    for command in [derive(Some(&[1]), None), derive(None, None)] {
        let applied = run(&mut document, &[command]);
        assert_eq!(applied.outcome, Outcome::Unchanged);
    }
    assert_eq!(
        (document.revision(), document.undo_depth()),
        (revision, depth)
    );
    assert_eq!(document, snapshot);
}

#[test]
fn a_different_stored_state_is_overwritten_and_undo_and_redo_restore_both() {
    let mut document = wedged_halo();
    run(
        &mut document,
        &[Command::SetAtomConfiguration {
            atom: AtomRef::Id(aid(1)),
            configuration: Some(AtomConfiguration::Tetrahedral {
                neighbors: [
                    SlotRef::Atom(AtomRef::Id(aid(2))),
                    SlotRef::Atom(AtomRef::Id(aid(3))),
                    SlotRef::Atom(AtomRef::Id(aid(4))),
                    SlotRef::LonePair,
                ],
                turn: Turn::Anticlockwise,
            }),
        }],
    );
    let stated = center(&document, 1);
    assert_eq!(
        stated,
        Some(tetra(
            [slot(2), slot(3), slot(4), Slot::LonePair],
            Turn::Anticlockwise
        ))
    );
    let applied = run(&mut document, &[derive(Some(&[1]), None)]);
    assert_eq!(applied.outcome, Outcome::Changed);
    assert!(applied.cleared.is_empty());
    assert_eq!(center(&document, 1), Some(e1_clockwise()));
    document.undo().unwrap();
    assert_eq!(center(&document, 1), stated);
    document.redo().unwrap();
    assert_eq!(center(&document, 1), Some(e1_clockwise()));
}

#[test]
fn a_mark_and_the_derivation_made_together_are_one_history_entry() {
    let mut build = Build::default();
    build.halo(100.0, 100.0);
    let mut document = build.document();
    let depth = document.undo_depth();
    run(
        &mut document,
        &[set_mark(2, wedge(BondEnd::Begin)), derive(Some(&[1]), None)],
    );
    assert_eq!(document.undo_depth(), depth + 1);
    assert_eq!(center(&document, 1), Some(e1_clockwise()));
    document.undo().unwrap();
    assert_eq!(document.bond(bid(2)).unwrap().mark, None);
    assert!(document.stereo().is_empty());
    document.redo().unwrap();
    assert_eq!(document.bond(bid(2)).unwrap().mark, wedge(BondEnd::Begin));
    assert_eq!(center(&document, 1), Some(e1_clockwise()));
}

#[test]
fn a_record_cleared_and_derived_again_in_one_transaction_is_not_reported_as_cleared() {
    let mut document = wedged_halo();
    run(&mut document, &[derive(Some(&[1]), None)]);
    // A fourth neighbor changes the center's neighbors, which clears the record.
    let new_neighbor = created_atom("I", 100.0, 130.0, None);
    let bond_to_center = Command::AddBond {
        begin: AtomRef::Id(aid(1)),
        end: AtomRef::Created(0),
        order: BondOrder::Single,
    };
    let mut control = document.clone();
    let cleared = run(
        &mut control,
        &[new_neighbor.clone(), bond_to_center.clone()],
    );
    assert_eq!(cleared.cleared, [StereoKey::Center(aid(1))]);

    let applied = run(
        &mut document,
        &[new_neighbor, bond_to_center, derive(Some(&[1]), None)],
    );
    assert!(applied.cleared.is_empty());
    // F (0, 1, 0), Cl (0.866, -0.5, +1), Br (-0.866, -0.5, 0), I (0, -1, 0): D = +1.732.
    assert_eq!(
        center(&document, 1),
        Some(tetra([slot(2), slot(3), slot(4), slot(5)], Turn::Clockwise))
    );
}

#[test]
fn hydrogens_the_atom_states_name_the_fourth_position_the_command_stores() {
    // A stated lone pair (zero hydrogens on a nitrogen with a wedge) is stored as such.
    let mut build = Build::default();
    let nitrogen = build.atom_as(
        Atom {
            hydrogens: Hydrogens::Specified(0),
            ..Atom::from(Element::from_symbol("N").unwrap())
        },
        100.0,
        100.0,
    );
    let ends = [
        build.atom("C", 100.0, 70.0),
        build.atom("C", 126.0, 115.0),
        build.atom("C", 74.0, 115.0),
    ];
    let bonds = ends.map(|end| build.bond(nitrogen, end, BondOrder::Single));
    build.mark(bonds[1], wedge(BondEnd::Begin));
    let mut document = build.document();
    run(&mut document, &[derive(Some(&[nitrogen]), None)]);
    assert_eq!(
        center(&document, nitrogen),
        Some(tetra(
            [slot(2), slot(3), slot(4), Slot::LonePair],
            Turn::Clockwise
        ))
    );
    // The same atom with its hydrogens made unknown cannot name it.
    run(
        &mut document,
        &[Command::UpdateAtom {
            atom: aid(nitrogen),
            edit: AtomEdit {
                hydrogens: Some(Hydrogens::Unknown),
                ..AtomEdit::default()
            },
        }],
    );
    let error = refuse(&mut document, &[derive(Some(&[nitrogen]), None)]);
    assert_eq!(
        cause_of(&error).1,
        DeriveError::NotDerivable(NotDerivable::FourthPosition)
    );
}

// ---------------------------------------------------------------------------------------------
// Invariants over random edit sequences
// ---------------------------------------------------------------------------------------------

/// A small deterministic generator (xorshift64*), so a failing seed can be replayed.
struct Random(u64);

impl Random {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(bound.max(1)).unwrap()).unwrap()
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> Option<T> {
        (!items.is_empty()).then(|| items[self.below(items.len())])
    }

    fn coordinate(&mut self) -> f64 {
        f64::from(u32::try_from(self.below(120)).unwrap()) - 60.0
    }
}

fn random_order(random: &mut Random, orders: &[BondOrder]) -> BondOrder {
    orders[random.below(orders.len())].clone()
}

fn random_mark(random: &mut Random) -> Option<StereoMark> {
    random
        .pick(&[
            None,
            wedge(BondEnd::Begin),
            wedge(BondEnd::End),
            Some(StereoMark::Hash {
                narrow: BondEnd::Begin,
            }),
            Some(StereoMark::Hash {
                narrow: BondEnd::End,
            }),
            Some(StereoMark::Wavy),
            Some(StereoMark::Crossed),
        ])
        .unwrap()
}

fn random_command(random: &mut Random, document: &Document) -> Command {
    let atoms: Vec<AtomId> = document.atoms().map(|(id, _)| id).collect();
    let bonds: Vec<BondId> = document.bonds().map(|(id, _)| id).collect();
    let instances: Vec<ObjectId> = document.instances().collect();
    let new_atom = |random: &mut Random| Command::AddAtom {
        atom: Atom::from(Element::CARBON),
        positions: NewAtomPositions::Single(
            Point::new(random.coordinate(), random.coordinate()).unwrap(),
        ),
        bond_to: random
            .pick(&atoms)
            .filter(|_| random.below(4) > 0)
            .map(|atom| BondTo {
                atom: AtomRef::Id(atom),
                order: random_order(
                    random,
                    &[BondOrder::Single, BondOrder::Single, BondOrder::Double],
                ),
            }),
    };
    match random.below(14) {
        0..=2 => new_atom(random),
        3 => match (random.pick(&atoms), random.pick(&atoms)) {
            (Some(begin), Some(end)) => Command::AddBond {
                begin: AtomRef::Id(begin),
                end: AtomRef::Id(end),
                order: random_order(random, &[BondOrder::Single, BondOrder::Double]),
            },
            _ => new_atom(random),
        },
        4..=5 => match random.pick(&bonds) {
            Some(bond) => Command::SetBondMark {
                bond: BondRef::Id(bond),
                mark: random_mark(random),
            },
            None => new_atom(random),
        },
        6 => match random.pick(&bonds) {
            Some(bond) => Command::UpdateBond {
                bond,
                order: random_order(
                    random,
                    &[BondOrder::Single, BondOrder::Double, BondOrder::Triple],
                ),
            },
            None => new_atom(random),
        },
        7 => match random.pick(&atoms) {
            Some(atom) => Command::MoveAtoms {
                atoms: vec![atom],
                delta: Vector {
                    dx: random.coordinate() / 4.0,
                    dy: random.coordinate() / 4.0,
                },
                instance: random.pick(&instances),
            },
            None => new_atom(random),
        },
        8 => match random.pick(&atoms) {
            Some(atom) => Command::DeleteAtoms { atoms: vec![atom] },
            None => new_atom(random),
        },
        9 => Command::AddInstance {
            source: None,
            offset: Vector { dx: 5.0, dy: 5.0 },
        },
        _ => {
            let instance = random.pick(&instances);
            match random.below(3) {
                0 => derive_in(None, None, instance),
                1 => Command::SetConfigurationFromDrawing {
                    atoms: Some(random.pick(&atoms).into_iter().map(AtomRef::Id).collect()),
                    bonds: None,
                    instance,
                },
                _ => Command::SetConfigurationFromDrawing {
                    atoms: None,
                    bonds: Some(random.pick(&bonds).into_iter().map(BondRef::Id).collect()),
                    instance,
                },
            }
        }
    }
}

#[test]
fn random_sequences_with_derivation_keep_every_invariant_and_replay_exactly() {
    let mut derived = 0_u32;
    let mut refused_derivations = 0_u32;
    for seed in 0..250 {
        let mut random = Random::new(seed);
        let mut document = Document::new(DocumentId::parse("doc-random").unwrap(), "R").unwrap();
        // Start with a hub so that marks and centers exist often enough.
        let mut hub = Build::default();
        hub.star(
            0.0,
            0.0,
            &[(0.0, -30.0), (26.0, 15.0), (-26.0, 15.0), (0.0, 30.0)],
        );
        document.apply(&hub.commands).unwrap();
        for _ in 0..40 {
            let before = document.clone();
            match random.below(10) {
                0 => {
                    let _ = document.undo();
                }
                1 => {
                    let _ = document.redo();
                }
                _ => {
                    let command = random_command(&mut random, &document);
                    let is_derive = matches!(command, Command::SetConfigurationFromDrawing { .. });
                    match document.apply_reporting(
                        std::slice::from_ref(&command),
                        &TransactionSelection::default(),
                    ) {
                        Ok(applied) if is_derive => {
                            if applied.outcome == Outcome::Changed {
                                derived += 1;
                            }
                            // Idempotence: the same command again changes nothing.
                            let again = document
                                .apply_reporting(
                                    std::slice::from_ref(&command),
                                    &TransactionSelection::default(),
                                )
                                .unwrap();
                            assert_eq!(again.outcome, Outcome::Unchanged, "seed {seed}");
                        }
                        Ok(_) => {}
                        Err(error) => {
                            if let EditError::FromDrawing { cause, .. } = &error.error {
                                // The existence checks come first, and a document holds a
                                // position of every instance for every atom.
                                assert!(
                                    !matches!(
                                        cause,
                                        DeriveError::UnknownAtom(_)
                                            | DeriveError::UnknownBond(_)
                                            | DeriveError::MissingPosition(_)
                                    ),
                                    "seed {seed}: unreachable cause {cause:?}"
                                );
                            }
                            refused_derivations += u32::from(is_derive);
                            assert_eq!(document, before, "seed {seed}: refused changes");
                        }
                    }
                }
            }
            let restored = Document::restore(document.to_parts())
                .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
            assert_eq!(restored, document, "seed {seed}");
        }
        let last = document.clone();
        let mut undone = 0;
        while document.undo().is_ok() {
            undone += 1;
        }
        for _ in 0..undone {
            document.redo().unwrap();
        }
        assert_eq!(document.stereo(), last.stereo(), "seed {seed}");
    }
    assert!(
        derived > 100,
        "too few derivations stored a record: {derived}"
    );
    assert!(
        refused_derivations > 100,
        "too few derivations were refused: {refused_derivations}"
    );
}

// ---------------------------------------------------------------------------------------------
// The scope lists and the derivations read the same marks
// ---------------------------------------------------------------------------------------------

struct Sketch {
    atoms: BTreeMap<AtomId, Atom>,
    bonds: BTreeMap<BondId, Bond>,
    positions: Vec<(AtomId, Point)>,
}

impl Sketch {
    fn drawing(&self) -> Drawing<'_> {
        Drawing::new(&self.atoms, &self.bonds, self.positions.iter().copied())
    }
}

/// A small random drawing straight from maps, so that marks the bond's order cannot carry (a
/// document never holds one) and query atoms occur too.
fn random_sketch(random: &mut Random) -> Sketch {
    let count = 3 + random.below(6);
    let mut sketch = Sketch {
        atoms: BTreeMap::new(),
        bonds: BTreeMap::new(),
        positions: Vec::new(),
    };
    for serial in 1..=u32::try_from(count).unwrap() {
        let kind = if random.below(6) == 0 {
            AtomKind::Query(QueryAtom::Any)
        } else {
            AtomKind::Element(Element::CARBON)
        };
        sketch.atoms.insert(aid(serial), Atom::new(kind));
        let position = Point::new(random.coordinate(), random.coordinate()).unwrap();
        sketch.positions.push((aid(serial), position));
    }
    for serial in 1..=u32::try_from(count + 3).unwrap() {
        let (begin, end) = (
            aid(1 + u32::try_from(random.below(count)).unwrap()),
            aid(1 + u32::try_from(random.below(count)).unwrap()),
        );
        let joined = sketch.bonds.values().any(|bond| {
            (bond.begin, bond.end) == (begin, end) || (bond.begin, bond.end) == (end, begin)
        });
        if begin == end || joined {
            continue;
        }
        let order = random_order(
            random,
            &[
                BondOrder::Single,
                BondOrder::Single,
                BondOrder::Single,
                BondOrder::Double,
                BondOrder::Double,
                BondOrder::Triple,
                BondOrder::Aromatic,
                BondOrder::Dative,
            ],
        );
        let mut bond = Bond::new(begin, end, order);
        bond.mark = random_mark(random);
        sketch.bonds.insert(bid(serial), bond);
    }
    sketch
}

/// `marked_centers` and `crossed_double_bonds` are the scope of the no-list command; the
/// derivations are what it then runs. They must agree on which marks bear on which items, or the
/// scope would store what no mark shows or leave out what one does.
#[test]
fn the_no_list_scope_is_exactly_the_items_the_derivations_find_a_mark_for() {
    let (mut listed_centers, mut structural_centers, mut unmarked_centers) = (0_u32, 0_u32, 0_u32);
    let (mut unfit_marks, mut listed_bonds, mut skipped_bonds, mut other_double_bonds) =
        (0_u32, 0_u32, 0_u32, 0_u32);
    for seed in 0..400 {
        let mut random = Random::new(seed);
        let sketch = random_sketch(&mut random);
        let drawing = sketch.drawing();
        let centers = drawing.marked_centers();
        let crossed = drawing.crossed_double_bonds();
        unfit_marks += u32::try_from(
            sketch
                .bonds
                .values()
                .filter(|bond| bond.mark.is_some_and(|mark| !mark.fits(&bond.order)))
                .count(),
        )
        .unwrap();

        for &atom in sketch.atoms.keys() {
            let answer = drawing.derive_center(atom);
            if centers.contains(&atom) {
                listed_centers += 1;
                assert!(
                    !matches!(
                        answer,
                        Err(DeriveError::NotDerivable(NotDerivable::NoMarks)
                            | DeriveError::Structure(_))
                    ),
                    "seed {seed}: {atom} is in the scope but the derivation finds no mark \
                     or no center: {answer:?}"
                );
            } else {
                match answer {
                    Err(DeriveError::NotDerivable(NotDerivable::NoMarks)) => unmarked_centers += 1,
                    Err(DeriveError::Structure(_)) => structural_centers += 1,
                    other => {
                        panic!("seed {seed}: {atom} is not in the scope but derives: {other:?}")
                    }
                }
            }
        }

        for (&id, bond) in &sketch.bonds {
            let answer = drawing.derive_double_bond(id);
            if crossed.contains(&id) {
                listed_bonds += 1;
                match answer {
                    Ok(DoubleBondState::Unknown) => {}
                    Err(DeriveError::Structure(
                        StereoError::ReferenceInvalid | StereoError::GeometryUnsupported,
                    )) => skipped_bonds += 1,
                    other => panic!("seed {seed}: {id} is in the scope but answers {other:?}"),
                }
            } else {
                assert!(
                    !matches!(answer, Ok(DoubleBondState::Unknown)),
                    "seed {seed}: {id} ({:?}, {:?}) is not in the scope but is derived unknown",
                    bond.order,
                    bond.mark
                );
                other_double_bonds += u32::from(answer.is_ok());
            }
        }
    }
    // The generator reaches every branch above, so none of the checks is vacuous.
    assert!(listed_centers > 100, "listed centers: {listed_centers}");
    assert!(
        structural_centers > 300,
        "terminal or query: {structural_centers}"
    );
    assert!(
        unmarked_centers > 100,
        "unmarked centers: {unmarked_centers}"
    );
    assert!(
        unfit_marks > 300,
        "marks a bond cannot carry: {unfit_marks}"
    );
    assert!(listed_bonds > 30, "listed double bonds: {listed_bonds}");
    assert!(
        skipped_bonds > 0,
        "structurally impossible listed bonds: {skipped_bonds}"
    );
    assert!(
        other_double_bonds > 30,
        "uncrossed derived bonds: {other_double_bonds}"
    );
}
