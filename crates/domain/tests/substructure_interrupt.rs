//! Interrupting a substructure search either stops it with `Interrupted` or changes nothing about
//! its result: a stop is never a partial list, a refusal or a different count of steps.

use std::cell::Cell;

use chemifree_domain::budget::{Interrupt, Interrupted, NEVER};
use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondOrder, Command, Document, DocumentId, MAX_COMMANDS,
    NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::substructure::{self, MAX_QUERY_ATOMS, MatchError, Matches};

/// The length of the chain in the long search.
const LONG_CHAIN: usize = 600;

/// How many stop positions the stop-at-every-question cases try; the last question is always
/// tried as well.
const SAMPLED_STOPS: usize = 50;

/// The atom with the zero-based creation index `index`: atoms are numbered from 1 in creation
/// order.
fn atom_ref(index: usize) -> AtomRef {
    AtomRef::Id(AtomId::from_serial(u32::try_from(index + 1).unwrap()).unwrap())
}

/// Atoms are element symbols; bonds are `-` single, `=` double and `:` aromatic, and name their
/// atoms by index. The atoms go first, in transactions of their own, so that every bond names
/// atoms that already exist.
fn mol(symbols: &[&str], bonds: &[(usize, usize, char)]) -> MoleculeGraph {
    let mut document =
        Document::new(DocumentId::parse("substructure-interrupt").unwrap(), "t").unwrap();
    let atoms: Vec<Command> = symbols
        .iter()
        .map(|symbol| Command::AddAtom {
            atom: Atom::from(Element::from_symbol(symbol).unwrap()),
            positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
            bond_to: None,
        })
        .collect();
    let joins: Vec<Command> = bonds
        .iter()
        .map(|&(begin, end, kind)| Command::AddBond {
            begin: atom_ref(begin),
            end: atom_ref(end),
            order: match kind {
                '-' => BondOrder::Single,
                '=' => BondOrder::Double,
                ':' => BondOrder::Aromatic,
                other => panic!("unknown bond kind {other}"),
            },
        })
        .collect();
    // A transaction holds at most MAX_COMMANDS commands.
    for chunk in atoms.chunks(MAX_COMMANDS).chain(joins.chunks(MAX_COMMANDS)) {
        document.apply(chunk).unwrap();
    }
    MoleculeGraph::from_document(&document)
}

/// The six bonds of a ring on atoms 0 to 5, all of kind `kind`.
fn ring(kind: char) -> Vec<(usize, usize, char)> {
    (0..6).map(|i| (i, (i + 1) % 6, kind)).collect()
}

/// A chain of `atoms` carbons joined by single bonds; `atoms` is at least 1.
fn chain(atoms: usize) -> MoleculeGraph {
    let bonds: Vec<(usize, usize, char)> = (0..atoms - 1).map(|i| (i, i + 1, '-')).collect();
    mol(&vec!["C"; atoms], &bonds)
}

/// Benzene with aromatic bonds.
fn benzene() -> MoleculeGraph {
    mol(&["C"; 6], &ring(':'))
}

/// Cyclohexane: the same ring with single bonds. A chain holds no ring.
fn cyclohexane() -> MoleculeGraph {
    mol(&["C"; 6], &ring('-'))
}

/// Naphthalene with aromatic bonds: the rings 0-1-2-3-4-5 and 4-6-7-8-9-5 share the bond 4-5.
/// Its bonds read as aromatic only after the target's ring system is perceived.
fn naphthalene() -> MoleculeGraph {
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
        .map(|&(begin, end)| (begin, end, ':'))
        .collect();
    mol(&["C"; 10], &bonds)
}

/// The cyclohexane query and the chain it is searched in: no mapping exists, and the search
/// takes thousands of steps before it finishes.
fn long_search() -> (MoleculeGraph, MoleculeGraph) {
    (cyclohexane(), chain(LONG_CHAIN))
}

/// A query and a target, with the name that a failure message gives the pair.
type Pair = (&'static str, MoleculeGraph, MoleculeGraph);

/// The pairs every case runs over: a benzene ring in naphthalene; C-C in a four-carbon chain; a
/// query element that the target does not have; an empty query; a query of 65 atoms; and the
/// long search.
fn corpus() -> Vec<Pair> {
    vec![
        ("benzene in naphthalene", benzene(), naphthalene()),
        ("C-C in butane", chain(2), chain(4)),
        ("absent element", mol(&["N"], &[]), benzene()),
        ("empty query", mol(&[], &[]), benzene()),
        ("65-atom query", chain(MAX_QUERY_ATOMS + 1), benzene()),
        (
            "cyclohexane in a long chain",
            cyclohexane(),
            chain(LONG_CHAIN),
        ),
    ]
}

/// Counts its questions, numbered from 1, and stops at the `stop_at`-th one when there is one.
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
    for (name, query, target) in corpus() {
        let expected = Ok(substructure::find(&query, &target));
        let counter = StopAt::counting();
        assert_eq!(
            substructure::find_with(&query, &target, &counter),
            expected,
            "{name}: an interrupt that never stops must give the unbudgeted result, steps included"
        );
        assert_eq!(
            substructure::find_with(&query, &target, &NEVER),
            expected,
            "{name}: NEVER must give the unbudgeted result"
        );
    }
}

#[test]
fn the_corpus_answers_are_the_known_matches() {
    // Benzene has twelve symmetry operations, and naphthalene holds two benzene rings.
    let benzenes = substructure::find(&benzene(), &naphthalene()).unwrap();
    assert_eq!(benzenes.mappings.len(), 24, "benzene in naphthalene");
    // A C-C bond lies on each of the three bonds of butane, read in both directions.
    let bonds = substructure::find(&chain(2), &chain(4)).unwrap();
    assert_eq!(bonds.mappings.len(), 6, "C-C in butane");
    // Each of the four carbons is tried for the first atom, then each of its neighbors for the
    // second: 4 + (1 + 2 + 2 + 1) assignments.
    assert_eq!(bonds.steps, 10, "C-C in butane takes ten steps");
    // No target atom is a candidate for the query element, so no step is taken.
    assert_eq!(
        substructure::find(&mol(&["N"], &[]), &benzene()),
        Ok(Matches {
            mappings: Vec::new(),
            steps: 0,
        }),
        "an absent element"
    );
    let (query, target) = long_search();
    let long = substructure::find(&query, &target).unwrap();
    assert!(
        long.mappings.is_empty(),
        "a six-membered ring does not occur in a chain"
    );
}

#[test]
fn a_stop_at_any_question_stops_the_search_and_a_later_stop_changes_nothing() {
    for (name, query, target) in corpus() {
        let expected = Ok(substructure::find(&query, &target));
        let counter = StopAt::counting();
        assert_eq!(
            substructure::find_with(&query, &target, &counter),
            expected,
            "{name}: the counting run gives the unbudgeted result"
        );
        let total = counter.calls.get();
        if matches!(expected, Ok(Ok(_))) {
            assert!(
                total >= 1,
                "{name}: a query that is searched asks its first question, but asked {total} times"
            );
        }
        let mut stops: Vec<usize> = (1..=total.min(SAMPLED_STOPS)).collect();
        if total > SAMPLED_STOPS {
            stops.push(total);
        }
        for stop_at in stops {
            for reason in [Interrupted::TimeLimit, Interrupted::Cancelled] {
                let probe = StopAt::new(Some(stop_at), reason);
                assert_eq!(
                    substructure::find_with(&query, &target, &probe),
                    Err(reason),
                    "{name}: {reason:?} at question {stop_at} of {total} must stop the search, \
                     with no partial list"
                );
            }
        }
        let late = StopAt::new(Some(total + 1), Interrupted::TimeLimit);
        assert_eq!(
            substructure::find_with(&query, &target, &late),
            expected,
            "{name}: a stop after the last question changes nothing"
        );
    }
}

#[test]
fn a_query_refused_for_its_size_is_refused_before_any_question() {
    let target = benzene();
    let refusals = [
        ("empty query", mol(&[], &[]), MatchError::EmptyQuery),
        (
            "65-atom query",
            chain(MAX_QUERY_ATOMS + 1),
            MatchError::QueryTooLarge {
                atoms: MAX_QUERY_ATOMS + 1,
            },
        ),
    ];
    for (name, query, refusal) in refusals {
        let counter = StopAt::counting();
        assert_eq!(
            substructure::find_with(&query, &target, &counter),
            Ok(Err(refusal)),
            "{name}: the size is refused"
        );
        assert_eq!(
            counter.calls.get(),
            0,
            "{name}: the size is read before the first question"
        );
        let probe = StopAt::new(Some(1), Interrupted::TimeLimit);
        assert_eq!(
            substructure::find_with(&query, &target, &probe),
            Ok(Err(refusal)),
            "{name}: with no question asked, a stop cannot replace the refusal"
        );
    }
}

#[test]
fn a_long_search_asks_once_per_stride_of_candidate_entries() {
    let (query, target) = long_search();
    let counter = StopAt::counting();
    let found = substructure::find_with(&query, &target, &counter)
        .unwrap()
        .unwrap();
    assert!(
        found.mappings.is_empty(),
        "a six-membered ring does not occur in a chain"
    );
    // The step bound is checked first: below 1,024 steps the question bound is trivially true.
    assert!(
        found.steps >= 3_072,
        "the long search must take at least 3,072 steps, took {}",
        found.steps
    );
    let questions = counter.calls.get();
    let needed = 1 + found.steps / 1_024;
    assert!(
        questions >= needed,
        "{} steps need the query question and one per 1,024 scanned entries, so at least \
         {needed} questions, but the search asked {questions}",
        found.steps
    );
}

#[test]
fn a_target_that_needs_perception_asks_while_its_aromaticity_is_read() {
    let counter = StopAt::counting();
    substructure::find_with(&benzene(), &naphthalene(), &counter)
        .unwrap()
        .unwrap();
    let asked = counter.calls.get();
    assert!(
        asked >= 3,
        "the query check and the target's ring system, with its entry and a bond check, are \
         questions, but the search asked {asked} times"
    );
}
