//! Interrupting an aromaticity conversion either stops it with `Interrupted` or changes
//! nothing about its result: there is no partial or different answer.

use std::cell::Cell;

use chemifree_domain::aromaticity::{self, Halt, Target};
use chemifree_domain::budget::{Interrupt, Interrupted, NEVER};
use chemifree_domain::document::{
    Atom, AtomRef, BondOrder, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;

/// Atoms are element symbols; bonds are `-` single, `=` double and `:` aromatic.
fn mol(atoms: &[&str], bonds: &[(usize, usize, char)]) -> MoleculeGraph {
    let mut document = Document::new(DocumentId::parse("interrupt-test").unwrap(), "t").unwrap();
    let mut commands: Vec<Command> = atoms
        .iter()
        .map(|symbol| Command::AddAtom {
            atom: Atom::from(Element::from_symbol(symbol).unwrap()),
            positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
            bond_to: None,
        })
        .collect();
    commands.extend(bonds.iter().map(|&(a, b, kind)| Command::AddBond {
        begin: AtomRef::Created(a),
        end: AtomRef::Created(b),
        order: match kind {
            '-' => BondOrder::Single,
            '=' => BondOrder::Double,
            ':' => BondOrder::Aromatic,
            other => panic!("unknown bond kind {other}"),
        },
    }));
    if !commands.is_empty() {
        document.apply(&commands).unwrap();
    }
    MoleculeGraph::from_document(&document)
}

fn ring(offset: usize, kinds: [char; 6]) -> Vec<(usize, usize, char)> {
    (0..6)
        .map(|i| (offset + i, offset + (i + 1) % 6, kinds[i]))
        .collect()
}

const KEKULE: [char; 6] = ['=', '-', '=', '-', '=', '-'];
const AROMATIC: [char; 6] = [':'; 6];

fn naphthalene(kinds: [char; 5]) -> MoleculeGraph {
    // Rings 0-1-2-3-4-5 and 4-5-9-8-7-6 share the bond 4-5.
    let mut bonds = ring(0, AROMATIC);
    for bond in &mut bonds {
        bond.2 = if bond.0 == 4 && bond.1 == 5 {
            ':'
        } else {
            kinds[0]
        };
    }
    bonds.extend([
        (4, 6, kinds[1]),
        (6, 7, kinds[2]),
        (7, 8, kinds[3]),
        (8, 9, kinds[4]),
        (9, 5, ':'),
    ]);
    mol(&["C"; 10], &bonds)
}

/// Graphs that reach every checkpoint: separate systems, a fused system (the six-membered
/// cover), Kekulé searches, rings that are not aromatic, and a refused system.
fn corpus() -> Vec<(&'static str, MoleculeGraph)> {
    let mut two_benzenes = ring(0, AROMATIC);
    two_benzenes.extend(ring(6, KEKULE));
    let mut cyclohexane = ring(0, ['-'; 6]);
    cyclohexane.push((0, 6, '-'));
    let mut cyclopentadiene_fused = ring(0, KEKULE);
    cyclopentadiene_fused.extend([(0, 6, '-'), (6, 7, '-'), (7, 8, '-'), (8, 1, '-')]);
    vec![
        ("empty", mol(&[], &[])),
        ("benzene aromatic", mol(&["C"; 6], &ring(0, AROMATIC))),
        ("benzene kekule", mol(&["C"; 6], &ring(0, KEKULE))),
        ("two benzenes", mol(&["C"; 12], &two_benzenes)),
        ("naphthalene aromatic", naphthalene([':'; 5])),
        (
            "pyridine aromatic",
            mol(&["N", "C", "C", "C", "C", "C"], &ring(0, AROMATIC)),
        ),
        ("cyclohexane", mol(&["C"; 7], &cyclohexane)),
        (
            "fused to a saturated ring (refused)",
            mol(&["C"; 9], &cyclopentadiene_fused),
        ),
    ]
}

/// Counts its checkpoints, and stops at the `stop_at`-th one when there is one.
struct StopAt {
    stop_at: Option<usize>,
    reason: Interrupted,
    calls: Cell<usize>,
}

impl StopAt {
    fn counting() -> Self {
        Self::new(None, Interrupted::Cancelled)
    }

    fn new(stop_at: Option<usize>, reason: Interrupted) -> Self {
        Self {
            stop_at,
            reason,
            calls: Cell::new(0),
        }
    }
}

impl Interrupt for StopAt {
    fn check(&self) -> Result<(), Interrupted> {
        let call = self.calls.get() + 1;
        self.calls.set(call);
        if self.stop_at == Some(call) {
            Err(self.reason)
        } else {
            Ok(())
        }
    }
}

#[test]
fn interrupting_at_any_checkpoint_stops_and_never_changes_the_result() {
    for (name, graph) in corpus() {
        for target in [Target::Aromatic, Target::Kekule] {
            let expected = aromaticity::conversion_commands(&graph, target);
            let counter = StopAt::counting();
            let uninterrupted = aromaticity::conversion_commands_with(&graph, target, &counter);
            assert_eq!(
                uninterrupted.clone().map_err(|halt| match halt {
                    Halt::Reason(reason) => reason,
                    Halt::Interrupted(_) => "interrupted",
                }),
                expected,
                "{name} {target:?}: an interrupt that never stops gives the unbudgeted result"
            );
            let total = counter.calls.get();
            assert!(total >= 1, "{name} {target:?}: the entry is a checkpoint");
            for stop_at in 1..=total {
                let reason = if stop_at % 2 == 0 {
                    Interrupted::TimeLimit
                } else {
                    Interrupted::Cancelled
                };
                let probe = StopAt::new(Some(stop_at), reason);
                assert_eq!(
                    aromaticity::conversion_commands_with(&graph, target, &probe),
                    Err(Halt::Interrupted(reason)),
                    "{name} {target:?}: stopping at checkpoint {stop_at} of {total} must stop"
                );
            }
            let late = StopAt::new(Some(total + 1), Interrupted::TimeLimit);
            assert_eq!(
                aromaticity::conversion_commands_with(&graph, target, &late),
                uninterrupted,
                "{name} {target:?}: a stop after the last checkpoint changes nothing"
            );
        }
    }
}

#[test]
fn perception_interrupted_at_any_checkpoint_stops_or_equals_the_plain_result() {
    for (name, graph) in corpus() {
        let expected = aromaticity::perceive(&graph);
        let counter = StopAt::counting();
        assert_eq!(
            aromaticity::perceive_with(&graph, &counter).as_ref(),
            Ok(&expected),
            "{name}"
        );
        let total = counter.calls.get();
        for stop_at in 1..=total {
            let probe = StopAt::new(Some(stop_at), Interrupted::Cancelled);
            assert_eq!(
                aromaticity::perceive_with(&graph, &probe),
                Err(Interrupted::Cancelled),
                "{name}: checkpoint {stop_at} of {total}"
            );
        }
        let late = StopAt::new(Some(total + 1), Interrupted::Cancelled);
        assert_eq!(
            aromaticity::perceive_with(&graph, &late).as_ref(),
            Ok(&expected),
            "{name}"
        );
        assert_eq!(
            aromaticity::perceive_with(&graph, &NEVER).as_ref(),
            Ok(&expected),
            "{name}"
        );
    }
}

#[test]
fn the_searches_are_checkpoints_not_only_the_systems() {
    // A fused system asks about every bond of its six-membered cover, and a Kekulé search asks
    // at every step. If either loop lost its checkpoint, the counts here would fall to the
    // number of systems plus the entry.
    let fused = naphthalene([':'; 5]);
    let counter = StopAt::counting();
    aromaticity::conversion_commands_with(&fused, Target::Kekule, &counter).unwrap();
    // Entry, one system, the cover of 11 bonds, and at least one search step per atom pair.
    assert!(
        counter.calls.get() >= 1 + 1 + 11 + 5,
        "a fused aromatic system must pass at least 18 checkpoints, passed {}",
        counter.calls.get()
    );
}
