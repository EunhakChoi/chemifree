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
fn a_fused_system_asks_about_every_bond_of_its_cover() {
    // Entry, one system and the cover of 11 bonds. If the cover lost its checkpoint, the count
    // would fall to the entry plus the systems.
    let fused = naphthalene([':'; 5]);
    let counter = StopAt::counting();
    aromaticity::conversion_commands_with(&fused, Target::Kekule, &counter).unwrap();
    assert!(
        counter.calls.get() >= 1 + 1 + 11,
        "a fused aromatic system must pass at least 13 checkpoints, passed {}",
        counter.calls.get()
    );
}

/// An aromatic system of 93 atoms and 121 bonds whose Kekulé search runs through the whole
/// 100,000-step budget (the same edges as `a_kekule_search_that_exhausts_the_step_budget...`
/// in `aromaticity_v2.rs`).
fn exhausting_search() -> MoleculeGraph {
    const EDGES: [(usize, usize); 121] = [(0,15),(0,31),(0,34),(1,39),(1,52),(2,53),(2,73),(2,75),(3,9),(3,44),(3,68),(4,33),(4,35),(4,73),(5,21),(5,42),(5,61),(6,28),(6,62),(6,92),(7,68),(7,71),(8,25),(8,31),(8,67),(9,18),(9,45),(10,29),(10,41),(10,43),(11,45),(11,81),(11,87),(12,43),(12,56),(13,30),(13,65),(13,84),(14,67),(14,78),(14,91),(15,21),(15,22),(16,55),(16,59),(16,67),(17,61),(17,86),(17,92),(18,71),(19,22),(19,79),(19,87),(20,24),(20,49),(20,89),(21,70),(22,48),(23,59),(23,63),(24,77),(24,90),(25,27),(26,48),(26,66),(26,75),(27,34),(28,29),(29,86),(30,35),(30,52),(31,80),(32,55),(32,63),(33,61),(33,84),(34,87),(35,46),(36,45),(36,79),(37,58),(37,65),(37,72),(38,47),(38,83),(39,65),(40,41),(40,50),(40,56),(41,58),(42,48),(42,73),(44,77),(44,81),(46,53),(47,77),(49,54),(50,72),(51,57),(51,60),(51,74),(54,85),(55,82),(57,66),(58,76),(60,64),(62,78),(64,88),(66,79),(69,74),(69,88),(70,80),(70,92),(74,75),(76,84),(76,86),(78,80),(81,89),(82,91),(83,90),(85,90)];
    let bonds: Vec<(usize, usize, char)> = EDGES.iter().map(|&(a, b)| (a, b, ':')).collect();
    mol(&["C"; 93], &bonds)
}

#[test]
fn the_kekule_search_asks_once_per_stride_not_at_every_step() {
    let graph = exhausting_search();
    let counter = StopAt::counting();
    assert_eq!(
        aromaticity::conversion_commands_with(&graph, Target::Kekule, &counter),
        Err(Halt::Reason("search_limit"))
    );
    let calls = counter.calls.get();
    // 100,000 steps at one question per 1,024 is about 98; the entry, the system and the 121
    // bonds of its cover add 123. Asking at every step would be over 100,000; never asking in
    // the search would leave 123.
    assert!(
        (200..=300).contains(&calls),
        "the search must ask about once per 1,024 steps, asked {calls} times"
    );
    // A stop inside the search ends it with that stop, and one after it changes nothing.
    for stop_at in [3, calls / 2, calls] {
        let probe = StopAt::new(Some(stop_at), Interrupted::TimeLimit);
        assert_eq!(
            aromaticity::conversion_commands_with(&graph, Target::Kekule, &probe),
            Err(Halt::Interrupted(Interrupted::TimeLimit)),
            "stopping at checkpoint {stop_at} of {calls}"
        );
    }
    let late = StopAt::new(Some(calls + 1), Interrupted::TimeLimit);
    assert_eq!(
        aromaticity::conversion_commands_with(&graph, Target::Kekule, &late),
        Err(Halt::Reason("search_limit"))
    );
}
