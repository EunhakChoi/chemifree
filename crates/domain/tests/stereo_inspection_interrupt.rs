//! Interrupting the stereo inspection either stops it with `Interrupted` or changes nothing about
//! its result: a stop is never a partial inspection, the questions are asked at the checkpoints
//! that `inspect_with` documents, and a stop after the last question changes nothing.

use std::cell::Cell;

use chemifree_domain::budget::{CancelFlag, Interrupt, Interrupted, NEVER};
use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondId, BondOrder, BondRef, Command, Document, DocumentId,
    NewAtomPositions, ObjectId, Point, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::stereo::{BondEnd, StereoMark};
use chemifree_domain::stereo_inspection::{InspectError, Inspection, Plan, inspect, inspect_with};

/// What `inspect_with` returns: the inspection or its refusal, or the stop.
type Outcome = Result<Result<Inspection, InspectError>, Interrupted>;

fn add_atom(symbol: &str, x: f64, y: f64) -> Command {
    Command::AddAtom {
        atom: Atom::from(Element::from_symbol(symbol).unwrap()),
        positions: NewAtomPositions::Single(Point::new(x, y).unwrap()),
        bond_to: None,
    }
}

fn add_bond(begin: u32, end: u32, order: BondOrder) -> Command {
    Command::AddBond {
        begin: AtomRef::Id(AtomId::from_serial(begin).unwrap()),
        end: AtomRef::Id(AtomId::from_serial(end).unwrap()),
        order,
    }
}

fn set_mark(bond: u32, mark: StereoMark) -> Command {
    Command::SetBondMark {
        bond: BondRef::Id(BondId::from_serial(bond).unwrap()),
        mark: Some(mark),
    }
}

fn wedge_begin() -> StereoMark {
    StereoMark::Wedge {
        narrow: BondEnd::Begin,
    }
}

/// Two centers, `a1` and `a7`, each bonded to three atoms, with a wedge on one of those bonds
/// whose narrow end is the center; and two crossed double bonds, `b4` (`a5` to `a6`) and `b8`
/// (`a11` to `a12`). The items are two centers and two double bonds. The document has two scene
/// instances: the first, and a second offset 200 points to the right.
fn two_centers_and_two_bonds() -> Document {
    let commands = [
        add_atom("C", 100.0, 100.0),
        add_atom("F", 100.0, 70.0),
        add_atom("Cl", 126.0, 115.0),
        add_atom("Br", 74.0, 115.0),
        add_atom("C", 200.0, 100.0),
        add_atom("C", 230.0, 100.0),
        add_atom("C", 400.0, 100.0),
        add_atom("F", 400.0, 70.0),
        add_atom("Cl", 426.0, 115.0),
        add_atom("Br", 374.0, 115.0),
        add_atom("C", 550.0, 100.0),
        add_atom("C", 580.0, 100.0),
        add_bond(1, 2, BondOrder::Single),
        add_bond(1, 3, BondOrder::Single),
        add_bond(1, 4, BondOrder::Single),
        add_bond(5, 6, BondOrder::Double),
        add_bond(7, 8, BondOrder::Single),
        add_bond(7, 9, BondOrder::Single),
        add_bond(7, 10, BondOrder::Single),
        add_bond(11, 12, BondOrder::Double),
        set_mark(2, wedge_begin()),
        set_mark(4, StereoMark::Crossed),
        set_mark(6, wedge_begin()),
        set_mark(8, StereoMark::Crossed),
        Command::AddInstance {
            source: None,
            offset: Vector { dx: 200.0, dy: 0.0 },
        },
    ];
    let mut document =
        Document::new(DocumentId::parse("stereo-interrupt").unwrap(), "Interrupt").unwrap();
    document.apply(&commands).unwrap();
    document
}

/// Counts its questions from zero, and stops the question numbered `stop_at` with `reason`. With
/// no `stop_at` it never stops.
struct Probe {
    stop_at: Option<usize>,
    reason: Interrupted,
    calls: Cell<usize>,
}

impl Probe {
    fn counting() -> Self {
        Self {
            stop_at: None,
            reason: Interrupted::Cancelled,
            calls: Cell::new(0),
        }
    }

    fn stopping_at(stop_at: usize, reason: Interrupted) -> Self {
        Self {
            stop_at: Some(stop_at),
            reason,
            calls: Cell::new(0),
        }
    }
}

impl Interrupt for Probe {
    fn check(&self) -> Result<(), Interrupted> {
        let question = self.calls.get();
        self.calls.set(question + 1);
        if self.stop_at == Some(question) {
            Err(self.reason)
        } else {
            Ok(())
        }
    }
}

#[test]
fn an_interrupt_that_never_stops_gives_the_inspection_of_inspect() {
    let document = two_centers_and_two_bonds();
    let plan = Plan::of(&document).unwrap();
    let instances: Vec<ObjectId> = document.instances().collect();
    for count in 0..=instances.len() {
        let inspected = &instances[..count];
        let expected: Outcome = Ok(Ok(inspect(&document, &plan, inspected).unwrap()));
        assert_eq!(
            inspect_with(&document, &plan, inspected, &Probe::counting()),
            expected,
            "{count} instances: an interrupt that never stops must give the inspection of inspect"
        );
        assert_eq!(
            inspect_with(&document, &plan, inspected, &CancelFlag::new()),
            expected,
            "{count} instances: an unset cancel flag must give the inspection of inspect"
        );
        assert_eq!(
            inspect_with(&document, &plan, inspected, &NEVER),
            expected,
            "{count} instances: NEVER must give the inspection of inspect"
        );
    }
}

#[test]
fn the_questions_are_one_per_instance_one_per_item_and_one_before_the_findings() {
    let document = two_centers_and_two_bonds();
    let plan = Plan::of(&document).unwrap();
    let instances: Vec<ObjectId> = document.instances().collect();
    // Each instance asks once before it and once before each of its items, which are the centers
    // and the double bonds of the plan; the findings ask once more.
    let atoms = plan.atoms().len();
    let bonds = plan.bonds().len();
    assert_eq!(
        (atoms, bonds),
        (2, 2),
        "the fixture has two center items and two double-bond items"
    );
    for count in 0..=instances.len() {
        let probe = Probe::counting();
        inspect_with(&document, &plan, &instances[..count], &probe)
            .unwrap()
            .unwrap();
        assert_eq!(
            probe.calls.get(),
            count * (1 + atoms + bonds) + 1,
            "{count} instances with {atoms} centers and {bonds} double bonds: one question per \
             instance, one per item of each instance, and one before the findings"
        );
    }
}

#[test]
fn a_stop_at_any_question_stops_the_inspection_and_a_later_stop_changes_nothing() {
    let document = two_centers_and_two_bonds();
    let plan = Plan::of(&document).unwrap();
    let instances: Vec<ObjectId> = document.instances().collect();
    for count in 0..=instances.len() {
        let inspected = &instances[..count];
        let expected: Outcome = Ok(Ok(inspect(&document, &plan, inspected).unwrap()));
        let counter = Probe::counting();
        assert_eq!(
            inspect_with(&document, &plan, inspected, &counter),
            expected,
            "{count} instances: the counting run gives the inspection of inspect"
        );
        let total = counter.calls.get();
        assert!(
            total > 0,
            "{count} instances: the findings always ask, but the run asked {total} times"
        );
        for question in 0..total {
            for reason in [Interrupted::Cancelled, Interrupted::TimeLimit] {
                assert_eq!(
                    inspect_with(
                        &document,
                        &plan,
                        inspected,
                        &Probe::stopping_at(question, reason),
                    ),
                    Err(reason),
                    "{count} instances: {reason:?} at question {question} of {total} must stop \
                     the inspection, with no partial result"
                );
            }
        }
        // The run asks no question after the last one, so a stop there never comes.
        assert_eq!(
            inspect_with(
                &document,
                &plan,
                inspected,
                &Probe::stopping_at(total, Interrupted::TimeLimit),
            ),
            expected,
            "{count} instances: a stop after the last question changes nothing"
        );
    }
}
