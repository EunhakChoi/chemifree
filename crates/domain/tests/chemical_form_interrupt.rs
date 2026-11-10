//! Interrupting a chemical form search either stops it with `Interrupted` or changes nothing
//! about its result: `chemical_form_with` gives exactly what `chemical_form` gives, or a stop.

use std::cell::Cell;

use chemifree_domain::budget::{Interrupt, Interrupted, NEVER};
use chemifree_domain::document::{
    Atom, AtomRef, BondOrder, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::identity::{self, ChemicalForm, IdentityError};

/// What a chemical form call returns when an interrupt may stop it: the form or the model's
/// refusal, or the stop itself.
type Outcome = Result<Result<ChemicalForm, IdentityError>, Interrupted>;

/// Atoms are element symbols; bonds are `-` single, `=` double and `:` aromatic.
fn mol(symbols: &[&str], bonds: &[(usize, usize, char)]) -> MoleculeGraph {
    let mut document =
        Document::new(DocumentId::parse("chemical-form-interrupt").unwrap(), "t").unwrap();
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

/// A chain of `atoms` carbon atoms joined in order by single bonds.
fn chain(atoms: usize) -> MoleculeGraph {
    let bonds: Vec<(usize, usize, char)> = (1..atoms).map(|i| (i - 1, i, '-')).collect();
    mol(&vec!["C"; atoms], &bonds)
}

/// A Kekulé six-membered ring fused through the bond 0-1 to a saturated five-membered ring. The
/// chemical model refuses it as `fused_partially_aromatic`, but only after its perception has
/// asked its question.
fn fused_to_saturated_ring() -> MoleculeGraph {
    let mut bonds = ring(0, KEKULE);
    bonds.extend([(0, 6, '-'), (6, 7, '-'), (7, 8, '-'), (8, 1, '-')]);
    mol(&["C"; 9], &bonds)
}

/// The graphs every case runs over: supported graphs of several shapes; iron, which the model
/// refuses before it asks any question; the fused ring, which it refuses after perception has
/// asked one; and a chain above the atom limit.
fn corpus() -> Vec<(&'static str, MoleculeGraph)> {
    vec![
        ("empty", mol(&[], &[])),
        ("single atom", mol(&["C"], &[])),
        ("chain of eight carbons", chain(8)),
        ("benzene kekule", mol(&["C"; 6], &ring(0, KEKULE))),
        ("benzene aromatic", mol(&["C"; 6], &ring(0, AROMATIC))),
        ("naphthalene aromatic", naphthalene([':'; 11])),
        (
            "naphthalene kekule",
            naphthalene(['=', '-', '=', '-', '=', '-', '-', '=', '-', '=', '-']),
        ),
        ("iron", mol(&["Fe"], &[])),
        ("fused saturated ring", fused_to_saturated_ring()),
        ("chain of 129 carbons", chain(129)),
    ]
}

/// Counts the questions a search asks, numbered from 1, and stops at the `question`-th one with
/// `reason` when there is one.
struct CancelAt {
    question: Option<usize>,
    reason: Interrupted,
    asked: Cell<usize>,
}

impl CancelAt {
    /// Counts the questions and never stops.
    fn counting() -> Self {
        Self {
            question: None,
            reason: Interrupted::Cancelled,
            asked: Cell::new(0),
        }
    }

    /// Stops at the `question`-th question with `reason`.
    fn at(question: usize, reason: Interrupted) -> Self {
        Self {
            question: Some(question),
            reason,
            asked: Cell::new(0),
        }
    }
}

impl Interrupt for CancelAt {
    fn check(&self) -> Result<(), Interrupted> {
        let asked = self.asked.get() + 1;
        self.asked.set(asked);
        if self.question == Some(asked) {
            Err(self.reason)
        } else {
            Ok(())
        }
    }
}

#[test]
fn an_interrupt_that_never_stops_gives_the_unbudgeted_form() {
    for (name, graph) in corpus() {
        let expected: Outcome = Ok(identity::chemical_form(&graph));
        let counter = CancelAt::counting();
        assert_eq!(
            identity::chemical_form_with(&graph, &counter),
            expected,
            "{name}: an interrupt that never stops must give the unbudgeted form"
        );
        assert_eq!(
            identity::chemical_form_with(&graph, &NEVER),
            expected,
            "{name}: NEVER must give the unbudgeted form"
        );
    }
}

#[test]
fn a_stop_at_any_question_is_a_stop_and_a_later_stop_changes_nothing() {
    for (name, graph) in corpus() {
        let counter = CancelAt::counting();
        let uninterrupted = identity::chemical_form_with(&graph, &counter);
        assert_eq!(
            uninterrupted,
            Ok(identity::chemical_form(&graph)),
            "{name}: the counting run must give the unbudgeted form"
        );
        let total = counter.asked.get();
        if matches!(uninterrupted, Ok(Ok(_))) {
            assert!(
                total >= 1,
                "{name}: a supported graph must ask at least one question, but asked {total}"
            );
        }
        for question in 1..=total {
            assert_eq!(
                identity::chemical_form_with(
                    &graph,
                    &CancelAt::at(question, Interrupted::TimeLimit)
                ),
                Err(Interrupted::TimeLimit),
                "{name}: a time limit at question {question} of {total} must stop the form"
            );
            assert_eq!(
                identity::chemical_form_with(
                    &graph,
                    &CancelAt::at(question, Interrupted::Cancelled)
                ),
                Err(Interrupted::Cancelled),
                "{name}: a cancel at question {question} of {total} must stop the form"
            );
        }
        assert_eq!(
            identity::chemical_form_with(&graph, &CancelAt::at(total + 1, Interrupted::TimeLimit)),
            uninterrupted,
            "{name}: a stop after the last question must change nothing"
        );
    }
}

#[test]
fn the_kekule_benzene_form_is_the_aromatic_ring_and_its_questions_can_stop_it() {
    let benzene = mol(&["C"; 6], &ring(0, KEKULE));
    let counter = CancelAt::counting();
    let Ok(Ok(form)) = identity::chemical_form_with(&benzene, &counter) else {
        panic!("benzene is a supported graph")
    };
    assert_eq!(form.atoms.len(), 6, "benzene has six ring atoms");
    assert_eq!(form.bonds.len(), 6, "benzene has six ring bonds");
    assert!(
        form.bonds
            .iter()
            .all(|(_, _, order)| *order == BondOrder::Aromatic),
        "the form numbers every bond of the aromatic ring as Aromatic, got {:?}",
        form.bonds
    );
    let total = counter.asked.get();
    assert!(
        total >= 1,
        "benzene's search asks at least one question, but asked {total} times"
    );
    assert_eq!(
        identity::chemical_form_with(&benzene, &CancelAt::at(1, Interrupted::TimeLimit)),
        Err(Interrupted::TimeLimit),
        "a time limit at the first question of benzene must stop the form"
    );
    assert_eq!(
        identity::chemical_form_with(&benzene, &CancelAt::at(total, Interrupted::Cancelled)),
        Err(Interrupted::Cancelled),
        "a cancel at the last question of benzene must stop the form"
    );
}

#[test]
fn the_fused_refusal_comes_after_a_question_and_a_stop_at_any_question_stays_a_stop() {
    let fused = fused_to_saturated_ring();
    let counter = CancelAt::counting();
    assert_eq!(
        identity::chemical_form_with(&fused, &counter),
        Ok(Err(IdentityError::Unsupported("fused_partially_aromatic"))),
        "a Kekulé ring fused to a saturated ring is refused by the chemical model"
    );
    let total = counter.asked.get();
    assert!(
        total >= 1,
        "the refusal comes after perception asks its question, but the search asked {total} times"
    );
    for question in 1..=total {
        assert_eq!(
            identity::chemical_form_with(&fused, &CancelAt::at(question, Interrupted::TimeLimit)),
            Err(Interrupted::TimeLimit),
            "a time limit at question {question} of {total} must stop, not become the refusal"
        );
    }
}

#[test]
fn the_atom_limit_is_refused_before_any_question_is_asked() {
    let oversized = chain(129);
    let counter = CancelAt::counting();
    assert_eq!(
        identity::chemical_form_with(&oversized, &counter),
        Ok(Err(IdentityError::AtomLimit)),
        "129 atoms exceed the limit of 128"
    );
    assert_eq!(
        counter.asked.get(),
        0,
        "the atom limit is decided before the first question"
    );
    assert_eq!(
        identity::chemical_form_with(&oversized, &CancelAt::at(1, Interrupted::TimeLimit)),
        Ok(Err(IdentityError::AtomLimit)),
        "with no question asked, a stop cannot replace the refusal"
    );
}
