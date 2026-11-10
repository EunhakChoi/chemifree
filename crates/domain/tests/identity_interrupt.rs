//! Interrupting a graph identity search either stops it with `Interrupted` or changes nothing
//! about its result: a stop is never a partial answer, a refusal or a different canonical form.

use std::cell::Cell;

use chemifree_domain::budget::{Interrupt, Interrupted, NEVER};
use chemifree_domain::document::{
    Atom, AtomRef, BondOrder, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::identity::{self, CanonicalGraph, IdentityError};

/// What an identity call returns when an interrupt may stop it: the canonical form or the
/// model's refusal, or the stop itself.
type Outcome = Result<Result<CanonicalGraph, IdentityError>, Interrupted>;

/// A profile's name, its interruptible entry point, and the unbudgeted function that the entry
/// point must agree with exactly.
type Profile = (
    &'static str,
    fn(&MoleculeGraph, &dyn Interrupt) -> Outcome,
    fn(&MoleculeGraph) -> Result<CanonicalGraph, IdentityError>,
);

fn profiles() -> [Profile; 2] {
    [
        (
            "recorded",
            identity::canonicalize_with,
            identity::canonicalize,
        ),
        (
            "chemical",
            identity::canonicalize_chemical_with,
            identity::canonicalize_chemical,
        ),
    ]
}

/// Atoms are element symbols; bonds are `-` single, `=` double and `:` aromatic.
fn mol(symbols: &[&str], bonds: &[(usize, usize, char)]) -> MoleculeGraph {
    let mut document =
        Document::new(DocumentId::parse("identity-interrupt").unwrap(), "t").unwrap();
    let mut commands: Vec<Command> = symbols
        .iter()
        .map(|symbol| Command::AddAtom {
            atom: Atom::from(Element::from_symbol(symbol).unwrap()),
            positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
            bond_to: None,
        })
        .collect();
    commands.extend(bonds.iter().map(|&(begin, end, kind)| Command::AddBond {
        begin: AtomRef::Created(begin),
        end: AtomRef::Created(end),
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

/// The six bonds of a ring on atoms `offset..offset + 6`: bond `i` joins atoms `i` and `i + 1`
/// and has kind `kinds[i]`.
fn ring(offset: usize, kinds: [char; 6]) -> Vec<(usize, usize, char)> {
    (0..6)
        .map(|i| (offset + i, offset + (i + 1) % 6, kinds[i]))
        .collect()
}

const SINGLE: [char; 6] = ['-'; 6];
const KEKULE: [char; 6] = ['=', '-', '=', '-', '=', '-'];
const AROMATIC: [char; 6] = [':'; 6];

/// Naphthalene on atoms 0 to 9: the rings 0-1-2-3-4-5 and 4-6-7-8-9-5 share the bond 4-5. The
/// kinds follow the order of `PAIRS`.
fn naphthalene(kinds: [char; 11]) -> MoleculeGraph {
    const PAIRS: [(usize, usize); 11] = [
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
    ];
    let bonds: Vec<(usize, usize, char)> = PAIRS
        .iter()
        .zip(kinds)
        .map(|(&(begin, end), kind)| (begin, end, kind))
        .collect();
    mol(&["C"; 10], &bonds)
}

/// A Kekulé six-membered ring fused through the bond 0-1 to a saturated five-membered ring. The
/// chemical model refuses it as `fused_partially_aromatic`, but only after its perception has
/// asked its question.
fn fused_to_saturated_ring() -> MoleculeGraph {
    let mut bonds = ring(0, KEKULE);
    bonds.extend([(0, 6, '-'), (6, 7, '-'), (7, 8, '-'), (8, 1, '-')]);
    mol(&["C"; 9], &bonds)
}

/// The graphs every case runs over: supported graphs of several shapes; an iron atom that the
/// chemical model refuses before it asks any question; and the fused ring above.
fn corpus() -> Vec<(&'static str, MoleculeGraph)> {
    let mut two_rings = ring(0, AROMATIC);
    two_rings.extend(ring(6, AROMATIC));
    // Benzene and pyridine as two disconnected rings. Two benzenes would have 288 automorphisms
    // and make the stop-at-every-checkpoint loop too slow for a unit test.
    let benzene_and_pyridine = ["C", "C", "C", "C", "C", "C", "N", "C", "C", "C", "C", "C"];
    vec![
        ("empty", mol(&[], &[])),
        ("single atom", mol(&["C"], &[])),
        ("cyclohexane", mol(&["C"; 6], &ring(0, SINGLE))),
        ("benzene kekule", mol(&["C"; 6], &ring(0, KEKULE))),
        ("benzene aromatic", mol(&["C"; 6], &ring(0, AROMATIC))),
        ("naphthalene aromatic", naphthalene([':'; 11])),
        (
            "naphthalene kekule",
            naphthalene(['=', '-', '=', '-', '=', '-', '-', '=', '-', '=', '-']),
        ),
        (
            "benzene and pyridine",
            mol(&benzene_and_pyridine, &two_rings),
        ),
        ("iron", mol(&["Fe"], &[])),
        ("fused saturated ring", fused_to_saturated_ring()),
    ]
}

/// Counts its checkpoints, numbered from 1, and stops at the `stop_at`-th one when there is one.
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
fn an_interrupt_that_never_stops_gives_the_unbudgeted_result() {
    for (name, graph) in corpus() {
        for (profile, with_interrupt, unbudgeted) in profiles() {
            let expected: Outcome = Ok(unbudgeted(&graph));
            let counter = StopAt::counting();
            assert_eq!(
                with_interrupt(&graph, &counter),
                expected,
                "{name} ({profile}): an interrupt that never stops must give the unbudgeted result"
            );
            assert_eq!(
                with_interrupt(&graph, &NEVER),
                expected,
                "{name} ({profile}): NEVER must give the unbudgeted result"
            );
        }
    }
}

#[test]
fn a_stop_at_any_checkpoint_is_interrupted_and_a_later_stop_changes_nothing() {
    for (name, graph) in corpus() {
        for (profile, with_interrupt, unbudgeted) in profiles() {
            let counter = StopAt::counting();
            let uninterrupted = with_interrupt(&graph, &counter);
            assert_eq!(
                uninterrupted,
                Ok(unbudgeted(&graph)),
                "{name} ({profile}): the counting run must give the unbudgeted result"
            );
            let total = counter.calls.get();
            if matches!(uninterrupted, Ok(Ok(_))) {
                assert!(
                    total >= 1,
                    "{name} ({profile}): a graph that is searched asks at least its first \
                     question, but asked {total} times"
                );
            }
            for stop_at in 1..=total {
                let probe = StopAt::new(Some(stop_at), Interrupted::TimeLimit);
                assert_eq!(
                    with_interrupt(&graph, &probe),
                    Err(Interrupted::TimeLimit),
                    "{name} ({profile}): a time limit at checkpoint {stop_at} of {total} must stop"
                );
            }
            if total >= 1 {
                let probe = StopAt::new(Some(total), Interrupted::Cancelled);
                assert_eq!(
                    with_interrupt(&graph, &probe),
                    Err(Interrupted::Cancelled),
                    "{name} ({profile}): a cancel at checkpoint {total} of {total} must stop"
                );
            }
            let late = StopAt::new(Some(total + 1), Interrupted::TimeLimit);
            assert_eq!(
                with_interrupt(&graph, &late),
                uninterrupted,
                "{name} ({profile}): a stop after the last checkpoint must change nothing"
            );
        }
    }
}

#[test]
fn the_atom_limit_is_refused_before_any_question_is_asked() {
    let bonds: Vec<(usize, usize, char)> = (0..128).map(|i| (i, i + 1, '-')).collect();
    let chain = mol(&["C"; 129], &bonds);
    for (profile, with_interrupt, _) in profiles() {
        let counter = StopAt::counting();
        assert_eq!(
            with_interrupt(&chain, &counter),
            Ok(Err(IdentityError::AtomLimit)),
            "{profile}: 129 atoms exceed the limit of 128"
        );
        assert_eq!(
            counter.calls.get(),
            0,
            "{profile}: the atom limit is decided before the first question"
        );
        let probe = StopAt::new(Some(1), Interrupted::TimeLimit);
        assert_eq!(
            with_interrupt(&chain, &probe),
            Ok(Err(IdentityError::AtomLimit)),
            "{profile}: with no question asked, a stop cannot replace the refusal"
        );
    }
}

#[test]
fn the_recorded_search_asks_questions_for_cyclohexane() {
    let cyclohexane = mol(&["C"; 6], &ring(0, SINGLE));
    let counter = StopAt::counting();
    assert!(
        matches!(
            identity::canonicalize_with(&cyclohexane, &counter),
            Ok(Ok(_))
        ),
        "cyclohexane is a supported recorded graph"
    );
    let asked = counter.calls.get();
    assert!(
        asked >= 2,
        "the search entry and at least one refinement round are questions, but asked {asked}"
    );
}

#[test]
fn the_chemical_refusals_in_the_corpus_are_the_literal_reasons() {
    let iron = mol(&["Fe"], &[]);
    assert_eq!(
        identity::canonicalize_chemical_with(&iron, &NEVER),
        Ok(Err(IdentityError::Unsupported("element_or_charge"))),
        "iron is outside the chemical model's element table"
    );
    assert!(
        matches!(identity::canonicalize_with(&iron, &NEVER), Ok(Ok(_))),
        "the recorded profile reads the element and accepts iron"
    );
    let fused = fused_to_saturated_ring();
    assert_eq!(
        identity::canonicalize_chemical_with(&fused, &NEVER),
        Ok(Err(IdentityError::Unsupported("fused_partially_aromatic"))),
        "a Kekulé ring fused to a saturated ring is refused by the chemical model"
    );
    assert!(
        matches!(identity::canonicalize_with(&fused, &NEVER), Ok(Ok(_))),
        "the recorded profile does not perceive aromaticity and accepts the fused graph"
    );
}

#[test]
fn the_recorded_search_of_benzene_asks_at_every_checkpoint_it_passes() {
    // Pins the visit-entry checkpoint: benzene's search visits 19 states, and each visit asks at
    // entry. The 43 refinement questions are in the same total, so removing either kind of
    // checkpoint changes the count.
    let benzene = mol(&["C"; 6], &ring(0, AROMATIC));
    let counter = StopAt::counting();
    assert_eq!(
        identity::canonicalize_with(&benzene, &counter),
        Ok(identity::canonicalize(&benzene)),
        "benzene (recorded): the counting run must give the unbudgeted result"
    );
    assert_eq!(
        counter.calls.get(),
        62,
        "benzene (recorded): the search asks 19 entry questions plus 43 refinement questions"
    );
}

#[test]
fn the_recorded_search_of_a_chain_asks_at_every_checkpoint_it_passes() {
    // Pins the refinement checkpoint: the chain of eight carbons visits 3 states, and their
    // refinement takes 12 rounds in all, each asking at its top. Removing the refinement
    // question leaves only the 3 entry questions.
    let bonds: Vec<(usize, usize, char)> = (0..7).map(|i| (i, i + 1, '-')).collect();
    let chain = mol(&["C"; 8], &bonds);
    let counter = StopAt::counting();
    assert_eq!(
        identity::canonicalize_with(&chain, &counter),
        Ok(identity::canonicalize(&chain)),
        "chain (recorded): the counting run must give the unbudgeted result"
    );
    assert_eq!(
        counter.calls.get(),
        15,
        "chain (recorded): the search asks 3 entry questions plus 12 refinement questions"
    );
}
