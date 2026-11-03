//! Stereo marks and stated configurations: validation, canonical form, the cascade when a
//! change makes a record meaningless, history, and restore. Representation only; nothing here
//! computes a CIP label or derives a configuration from a drawing.

use std::time::{Duration, Instant};

use chemifree_domain::document::{
    Applied, Atom, AtomEdit, AtomId, AtomRef, Bond, BondId, BondOrder, BondRef, BondTo, Change,
    ChangeSet, Command, Document, DocumentId, DocumentParts, EditError, HISTORY_LIMIT, MAX_ATOMS,
    MAX_BONDS, NewAtomPositions, Outcome, Point, TransactionError,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::TransactionSelection;
use chemifree_domain::states::{AtomKind, BondQuery, QueryAtom};
use chemifree_domain::stereo::{
    AtomConfiguration, BondConfiguration, BondEnd, CenterState, DoubleBondState, Relation, Slot,
    SlotRef, StereoError, StereoKey, StereoMark, StereoState, Turn,
};

fn atom(serial: u32) -> AtomId {
    AtomId::from_serial(serial).unwrap()
}

fn bond(serial: u32) -> BondId {
    BondId::from_serial(serial).unwrap()
}

fn s(serial: u32) -> SlotRef {
    SlotRef::Atom(AtomRef::Id(atom(serial)))
}

fn new_atom() -> Command {
    Command::AddAtom {
        atom: Atom::from(Element::CARBON),
        positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
        bond_to: None,
    }
}

fn add_bond(begin: u32, end: u32, order: BondOrder) -> Command {
    Command::AddBond {
        begin: AtomRef::Id(atom(begin)),
        end: AtomRef::Id(atom(end)),
        order,
    }
}

fn tetra(center: u32, neighbors: [SlotRef; 4], turn: Turn) -> Command {
    Command::SetAtomConfiguration {
        atom: AtomRef::Id(atom(center)),
        configuration: Some(AtomConfiguration::Tetrahedral { neighbors, turn }),
    }
}

fn clear_center(center: u32) -> Command {
    Command::SetAtomConfiguration {
        atom: AtomRef::Id(atom(center)),
        configuration: None,
    }
}

fn mark(bond_serial: u32, mark: Option<StereoMark>) -> Command {
    Command::SetBondMark {
        bond: BondRef::Id(bond(bond_serial)),
        mark,
    }
}

fn wedge(narrow: BondEnd) -> StereoMark {
    StereoMark::Wedge { narrow }
}

fn relative(
    bond_serial: u32,
    begin_neighbor: SlotRef,
    end_neighbor: SlotRef,
    relation: Relation,
) -> Command {
    Command::SetBondConfiguration {
        bond: BondRef::Id(bond(bond_serial)),
        configuration: Some(BondConfiguration::Relative {
            begin_neighbor,
            end_neighbor,
            relation,
        }),
    }
}

fn empty() -> Document {
    Document::new(DocumentId::parse("doc-stereo").unwrap(), "Stereo").unwrap()
}

fn run(document: &mut Document, commands: &[Command]) -> Applied {
    document
        .apply_reporting(commands, &TransactionSelection::default())
        .unwrap_or_else(|error| panic!("{error}"))
}

fn refusal(document: &mut Document, commands: &[Command]) -> TransactionError {
    let before = document.clone();
    let error = document
        .apply_reporting(commands, &TransactionSelection::default())
        .expect_err("the transaction is refused");
    assert_eq!(*document, before, "a refused transaction changes nothing");
    error
}

fn stereo_refusal(error: &TransactionError) -> Option<StereoError> {
    match error.error {
        EditError::Stereo(reason) => Some(reason),
        _ => None,
    }
}

/// Carbon `a1` bonded by single bonds to `neighbors` further carbons `a2...`.
fn center_with(neighbors: u32) -> Document {
    let mut document = empty();
    let mut commands: Vec<Command> = (0..=neighbors).map(|_| new_atom()).collect();
    commands.extend((2..=neighbors + 1).map(|n| add_bond(1, n, BondOrder::Single)));
    run(&mut document, &commands);
    document
}

/// `a1=a2` (bond `b1`) with `a1` bonded to `a3`, `a4` and `a2` bonded to `a5`.
fn alkene() -> Document {
    let mut document = empty();
    let mut commands: Vec<Command> = (0..5).map(|_| new_atom()).collect();
    commands.extend([
        add_bond(1, 2, BondOrder::Double),
        add_bond(1, 3, BondOrder::Single),
        add_bond(1, 4, BondOrder::Single),
        add_bond(2, 5, BondOrder::Single),
    ]);
    run(&mut document, &commands);
    document
}

fn center_state(document: &Document, serial: u32) -> Option<CenterState> {
    match document.stereo().get(StereoKey::Center(atom(serial))) {
        Some(StereoState::Center(state)) => Some(*state),
        Some(other) => panic!("a center key holds {other:?}"),
        None => None,
    }
}

fn double_bond_state(document: &Document, serial: u32) -> Option<DoubleBondState> {
    match document.stereo().get(StereoKey::DoubleBond(bond(serial))) {
        Some(StereoState::DoubleBond(state)) => Some(*state),
        Some(other) => panic!("a double bond key holds {other:?}"),
        None => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Canonical form and permutation parity
// ---------------------------------------------------------------------------------------------

/// Every ordering of `0..4`.
fn permutations() -> Vec<[usize; 4]> {
    let mut all = Vec::new();
    for a in 0..4 {
        for b in (0..4).filter(|&b| b != a) {
            for c in (0..4).filter(|&c| c != a && c != b) {
                let d = 6 - a - b - c;
                all.push([a, b, c, d]);
            }
        }
    }
    assert_eq!(all.len(), 24);
    all
}

/// Parity of a permutation by its cycle count: odd exactly when `n - cycles` is odd. This is a
/// different method from the production inversion count.
fn is_odd_by_cycles(permutation: &[usize; 4]) -> bool {
    let mut seen = [false; 4];
    let mut cycles = 0;
    for start in 0..4 {
        if !seen[start] {
            cycles += 1;
            let mut at = start;
            while !seen[at] {
                seen[at] = true;
                at = permutation[at];
            }
        }
    }
    (4 - cycles) % 2 == 1
}

#[test]
fn every_ordering_of_four_slots_canonicalizes_with_the_right_parity() {
    let atoms = [
        Slot::Atom(atom(1)),
        Slot::Atom(atom(2)),
        Slot::Atom(atom(3)),
    ];
    let sorted_sets = [
        [atoms[0], atoms[1], atoms[2], Slot::Atom(atom(4))],
        [atoms[0], atoms[1], atoms[2], Slot::ImplicitHydrogen],
        [atoms[0], atoms[1], atoms[2], Slot::LonePair],
    ];
    for sorted in sorted_sets {
        for turn in [Turn::Anticlockwise, Turn::Clockwise] {
            for permutation in permutations() {
                let given = permutation.map(|index| sorted[index]);
                let expected_turn = if is_odd_by_cycles(&permutation) {
                    turn.flipped()
                } else {
                    turn
                };
                let state = CenterState::tetrahedral(given, turn).unwrap();
                assert_eq!(
                    state,
                    CenterState::Tetrahedral {
                        slots: sorted,
                        turn: expected_turn
                    },
                    "given {given:?} {turn:?}"
                );
                // Canonical form is a fixed point.
                let CenterState::Tetrahedral { slots, turn: again } = state else {
                    panic!("tetrahedral stays tetrahedral");
                };
                assert_eq!(CenterState::tetrahedral(slots, again).unwrap(), state);
            }
        }
    }
}

#[test]
fn swapping_two_neighbors_inverts_the_turn_and_rotating_three_does_not() {
    // OpenSMILES: `N[C@](Br)(O)C` is the mirror image of `N[C@@](Br)(O)C`, and listing the
    // first two neighbors the other way round (`Br[C@@](N)(O)C`) names the same molecule as
    // `N[C@](Br)(O)C`; rotating the last three neighbors changes nothing.
    let (n, br, o, c) = (
        Slot::Atom(atom(1)),
        Slot::Atom(atom(2)),
        Slot::Atom(atom(3)),
        Slot::Atom(atom(4)),
    );
    let at = CenterState::tetrahedral([n, br, o, c], Turn::Anticlockwise).unwrap();
    // Absolute expectations, so a parity that is wrong everywhere cannot pass by being
    // consistent with itself: the sorted order keeps its turn, one swap inverts it.
    assert_eq!(
        at,
        CenterState::Tetrahedral {
            slots: [n, br, o, c],
            turn: Turn::Anticlockwise
        }
    );
    assert_eq!(
        CenterState::tetrahedral([br, n, o, c], Turn::Anticlockwise).unwrap(),
        CenterState::Tetrahedral {
            slots: [n, br, o, c],
            turn: Turn::Clockwise
        }
    );
    let double_at = CenterState::tetrahedral([n, br, o, c], Turn::Clockwise).unwrap();
    assert_ne!(at, double_at, "the mirror images are different statements");
    assert_eq!(
        CenterState::tetrahedral([br, n, o, c], Turn::Clockwise).unwrap(),
        at
    );
    assert_eq!(
        CenterState::tetrahedral([n, o, c, br], Turn::Anticlockwise).unwrap(),
        at
    );
    assert_eq!(
        CenterState::tetrahedral([n, br, c, o], Turn::Clockwise).unwrap(),
        at
    );
}

#[test]
fn a_repeated_slot_is_not_a_neighbor_list() {
    let a1 = Slot::Atom(atom(1));
    for repeated in [
        [a1, a1, Slot::Atom(atom(2)), Slot::Atom(atom(3))],
        [a1, Slot::LonePair, Slot::LonePair, Slot::Atom(atom(3))],
    ] {
        assert_eq!(
            CenterState::tetrahedral(repeated, Turn::Clockwise),
            Err(StereoError::CenterNeighbors)
        );
        assert_eq!(
            CenterState::unknown(repeated),
            Err(StereoError::CenterNeighbors)
        );
    }
}

#[test]
fn a_command_stores_the_canonical_form_whatever_order_it_names() {
    let sorted = [s(2), s(3), s(4), s(5)];
    let mut expected = center_with(4);
    run(&mut expected, &[tetra(1, sorted, Turn::Anticlockwise)]);
    let canonical = center_state(&expected, 1).unwrap();
    for permutation in permutations() {
        let mut document = center_with(4);
        let given = permutation.map(|index| sorted[index]);
        let turn = if is_odd_by_cycles(&permutation) {
            Turn::Clockwise
        } else {
            Turn::Anticlockwise
        };
        run(&mut document, &[tetra(1, given, turn)]);
        assert_eq!(
            center_state(&document, 1),
            Some(canonical),
            "{permutation:?} {turn:?}"
        );
    }
}

#[test]
fn a_three_bond_center_holds_a_hydrogen_or_a_lone_pair_slot_as_stated() {
    for (slot, expected) in [
        (SlotRef::ImplicitHydrogen, Slot::ImplicitHydrogen),
        (SlotRef::LonePair, Slot::LonePair),
    ] {
        let mut document = center_with(3);
        run(
            &mut document,
            &[tetra(1, [slot, s(4), s(2), s(3)], Turn::Clockwise)],
        );
        let Some(CenterState::Tetrahedral { slots, turn }) = center_state(&document, 1) else {
            panic!("a tetrahedral configuration is stored");
        };
        assert_eq!(
            slots,
            [
                Slot::Atom(atom(2)),
                Slot::Atom(atom(3)),
                Slot::Atom(atom(4)),
                expected
            ]
        );
        // [slot, a4, a2, a3] -> [a2, a3, a4, slot] is a 4-cycle: odd.
        assert_eq!(turn, Turn::Anticlockwise);
    }
}

// ---------------------------------------------------------------------------------------------
// Marks
// ---------------------------------------------------------------------------------------------

fn bond_with_order(order: BondOrder) -> Document {
    let mut document = empty();
    run(
        &mut document,
        &[new_atom(), new_atom(), add_bond(1, 2, order)],
    );
    document
}

#[test]
fn each_mark_fits_only_its_bond_orders() {
    let marks = [
        wedge(BondEnd::Begin),
        wedge(BondEnd::End),
        StereoMark::Hash {
            narrow: BondEnd::Begin,
        },
        StereoMark::Hash {
            narrow: BondEnd::End,
        },
        StereoMark::Wavy,
        StereoMark::Crossed,
    ];
    let orders = [
        BondOrder::Single,
        BondOrder::Double,
        BondOrder::Triple,
        BondOrder::Aromatic,
        BondOrder::Dative,
        BondOrder::Query(BondQuery::Any),
        BondOrder::Unknown,
    ];
    for order in &orders {
        for stereo_mark in marks {
            let allowed = match stereo_mark {
                StereoMark::Crossed => *order == BondOrder::Double,
                _ => *order == BondOrder::Single,
            };
            let mut document = bond_with_order(order.clone());
            let result = document.apply_reporting(
                &[mark(1, Some(stereo_mark))],
                &TransactionSelection::default(),
            );
            if allowed {
                assert_eq!(
                    result.unwrap().outcome,
                    Outcome::Changed,
                    "{stereo_mark:?} on {order:?}"
                );
                assert_eq!(document.bonds().next().unwrap().1.mark, Some(stereo_mark));
            } else {
                let error = result.expect_err("the mark is refused");
                assert_eq!(
                    (error.index, stereo_refusal(&error)),
                    (Some(0), Some(StereoError::MarkBondOrder)),
                    "{stereo_mark:?} on {order:?}"
                );
            }
        }
    }
}

#[test]
fn a_mark_records_its_narrow_end_and_never_swaps_the_bond_ends() {
    let mut document = bond_with_order(BondOrder::Single);
    for narrow in [BondEnd::End, BondEnd::Begin] {
        run(&mut document, &[mark(1, Some(wedge(narrow)))]);
        let (_, stored) = document.bonds().next().unwrap();
        assert_eq!((stored.begin, stored.end), (atom(1), atom(2)));
        assert_eq!(stored.mark, Some(wedge(narrow)));
    }
    run(&mut document, &[mark(1, None)]);
    let (_, stored) = document.bonds().next().unwrap();
    assert_eq!(
        (stored.begin, stored.end, stored.mark),
        (atom(1), atom(2), None)
    );
    // The history entry changes the mark only.
    document.undo().unwrap();
    let (_, stored) = document.bonds().next().unwrap();
    assert_eq!((stored.begin, stored.end), (atom(1), atom(2)));
}

#[test]
fn changing_the_order_of_a_marked_bond_is_refused_not_erased() {
    let mut document = bond_with_order(BondOrder::Single);
    run(&mut document, &[mark(1, Some(wedge(BondEnd::Begin)))]);
    let error = refusal(
        &mut document,
        &[Command::UpdateBond {
            bond: bond(1),
            order: BondOrder::Double,
        }],
    );
    assert_eq!(
        (error.index, stereo_refusal(&error)),
        (Some(0), Some(StereoError::MarkBondOrder))
    );
    // Clearing the mark first, in the same transaction, makes the change.
    run(
        &mut document,
        &[
            mark(1, None),
            Command::UpdateBond {
                bond: bond(1),
                order: BondOrder::Double,
            },
        ],
    );
    let (_, stored) = document.bonds().next().unwrap();
    assert_eq!(
        (stored.order.clone(), stored.mark),
        (BondOrder::Double, None)
    );
}

#[test]
fn setting_the_mark_a_bond_already_has_changes_nothing() {
    let mut document = bond_with_order(BondOrder::Single);
    run(&mut document, &[mark(1, Some(StereoMark::Wavy))]);
    let revision = document.revision();
    let applied = run(&mut document, &[mark(1, Some(StereoMark::Wavy))]);
    assert_eq!(applied.outcome, Outcome::Unchanged);
    assert_eq!(document.revision(), revision);
}

// ---------------------------------------------------------------------------------------------
// Validation of configurations
// ---------------------------------------------------------------------------------------------

fn assert_refused(document: &mut Document, command: Command, reason: StereoError, why: &str) {
    let error = refusal(document, &[command]);
    assert_eq!(
        (error.index, stereo_refusal(&error)),
        (Some(0), Some(reason)),
        "{why}"
    );
}

#[test]
fn center_configurations_are_checked_against_the_bonded_neighbors() {
    let mut four = center_with(4);
    assert_refused(
        &mut four,
        tetra(
            1,
            [s(2), s(3), s(4), SlotRef::ImplicitHydrogen],
            Turn::Clockwise,
        ),
        StereoError::CenterNeighbors,
        "an implicit slot on a center with four bonds",
    );
    assert_refused(
        &mut four,
        tetra(1, [s(2), s(3), s(4), s(1)], Turn::Clockwise),
        StereoError::CenterNeighbors,
        "the center named as its own neighbor",
    );
    assert_refused(
        &mut four,
        tetra(1, [s(2), s(3), s(3), s(5)], Turn::Clockwise),
        StereoError::CenterNeighbors,
        "a repeated neighbor",
    );
    assert_refused(
        &mut four,
        Command::SetAtomConfiguration {
            atom: AtomRef::Id(atom(1)),
            configuration: Some(AtomConfiguration::Unknown {
                neighbors: [s(2), s(3), s(4), SlotRef::LonePair],
            }),
        },
        StereoError::CenterNeighbors,
        "unknown over a wrong neighbor set",
    );
    let error = refusal(
        &mut four,
        &[tetra(1, [s(2), s(3), s(4), s(9)], Turn::Clockwise)],
    );
    assert_eq!(error.error, EditError::UnknownAtom(atom(9)));

    let mut three = center_with(3);
    assert_refused(
        &mut three,
        tetra(1, [s(2), s(3), s(4), s(1)], Turn::Clockwise),
        StereoError::CenterNeighbors,
        "a three-bond center without a hydrogen or lone-pair slot",
    );
    let mut two = center_with(2);
    assert_refused(
        &mut two,
        tetra(
            1,
            [s(2), s(3), SlotRef::ImplicitHydrogen, SlotRef::LonePair],
            Turn::Clockwise,
        ),
        StereoError::CenterUnsupported,
        "fewer than three neighbors",
    );
    let mut five = center_with(5);
    assert_refused(
        &mut five,
        tetra(1, [s(2), s(3), s(4), s(5)], Turn::Clockwise),
        StereoError::CenterUnsupported,
        "more than four neighbors",
    );
    let mut query = center_with(4);
    run(
        &mut query,
        &[Command::UpdateAtom {
            atom: atom(1),
            edit: AtomEdit {
                kind: Some(AtomKind::Query(QueryAtom::Any)),
                ..AtomEdit::default()
            },
        }],
    );
    assert_refused(
        &mut query,
        tetra(1, [s(2), s(3), s(4), s(5)], Turn::Clockwise),
        StereoError::CenterUnsupported,
        "a query atom",
    );
}

#[test]
fn turning_a_configured_center_into_a_query_atom_is_refused() {
    let mut document = center_with(4);
    run(
        &mut document,
        &[tetra(1, [s(2), s(3), s(4), s(5)], Turn::Clockwise)],
    );
    let error = refusal(
        &mut document,
        &[Command::UpdateAtom {
            atom: atom(1),
            edit: AtomEdit {
                kind: Some(AtomKind::Query(QueryAtom::Any)),
                ..AtomEdit::default()
            },
        }],
    );
    assert_eq!(stereo_refusal(&error), Some(StereoError::CenterUnsupported));
}

#[test]
fn double_bond_configurations_are_checked_against_the_ends() {
    // Not a double bond.
    let mut single = alkene();
    assert_refused(
        &mut single,
        relative(2, s(1), s(5), Relation::SameSide),
        StereoError::DoubleBondRequired,
        "a single bond",
    );
    let mut document = alkene();
    assert_refused(
        &mut document,
        relative(1, s(2), s(5), Relation::SameSide),
        StereoError::ReferenceInvalid,
        "the partner as a reference",
    );
    assert_refused(
        &mut document,
        relative(1, s(5), s(5), Relation::SameSide),
        StereoError::ReferenceInvalid,
        "a reference not bonded to its end",
    );
    assert_refused(
        &mut document,
        relative(1, SlotRef::ImplicitHydrogen, s(5), Relation::SameSide),
        StereoError::ReferenceInvalid,
        "an implicit slot where the end has explicit neighbors",
    );
    assert_refused(
        &mut document,
        relative(1, s(3), SlotRef::LonePair, Relation::SameSide),
        StereoError::ReferenceInvalid,
        "an implicit slot where the end has an explicit neighbor",
    );
    // An end with three other neighbors.
    let mut crowded = alkene();
    run(
        &mut crowded,
        &[
            new_atom(),
            new_atom(),
            add_bond(2, 6, BondOrder::Single),
            add_bond(2, 7, BondOrder::Single),
        ],
    );
    assert_refused(
        &mut crowded,
        relative(1, s(3), s(5), Relation::SameSide),
        StereoError::ReferenceInvalid,
        "an end with three other neighbors",
    );
    // A cumulated end: a2 also carries a double bond to a6.
    let mut allene = alkene();
    run(
        &mut allene,
        &[new_atom(), add_bond(2, 6, BondOrder::Double)],
    );
    assert_refused(
        &mut allene,
        relative(1, s(3), s(5), Relation::SameSide),
        StereoError::GeometryUnsupported,
        "a cumulated double bond",
    );
}

#[test]
fn a_double_bond_reference_is_stored_as_the_lowest_neighbor_with_the_relation_adjusted() {
    // a1 has neighbors a3, a4; a2 has a5. Naming a4 instead of a3 flips the relation.
    let cases = [
        (s(3), Relation::SameSide, Relation::SameSide),
        (s(4), Relation::SameSide, Relation::OppositeSides),
        (s(3), Relation::OppositeSides, Relation::OppositeSides),
        (s(4), Relation::OppositeSides, Relation::SameSide),
    ];
    for (begin_neighbor, relation, stored) in cases {
        let mut document = alkene();
        run(
            &mut document,
            &[relative(1, begin_neighbor, s(5), relation)],
        );
        assert_eq!(
            double_bond_state(&document, 1),
            Some(DoubleBondState::Relative {
                begin_ref: Slot::Atom(atom(3)),
                end_ref: Slot::Atom(atom(5)),
                relation: stored,
            }),
            "{begin_neighbor:?} {relation:?}"
        );
    }
}

#[test]
fn an_end_without_other_neighbors_takes_an_implicit_hydrogen_or_a_lone_pair() {
    // a1=a2, a1-a3, a1-a4; a2 has no other neighbor (an imine nitrogen, say).
    let mut document = empty();
    run(
        &mut document,
        &[
            new_atom(),
            new_atom(),
            new_atom(),
            new_atom(),
            add_bond(1, 2, BondOrder::Double),
            add_bond(1, 3, BondOrder::Single),
            add_bond(1, 4, BondOrder::Single),
        ],
    );
    run(
        &mut document,
        &[relative(1, s(4), SlotRef::LonePair, Relation::SameSide)],
    );
    assert_eq!(
        double_bond_state(&document, 1),
        Some(DoubleBondState::Relative {
            begin_ref: Slot::Atom(atom(3)),
            end_ref: Slot::LonePair,
            relation: Relation::OppositeSides,
        })
    );
    assert_refused(
        &mut document,
        relative(1, s(3), s(2), Relation::SameSide),
        StereoError::ReferenceInvalid,
        "an atom reference where the end has no other neighbor",
    );
}

#[test]
fn unknown_double_bond_and_center_states_are_explicit_records() {
    let mut document = alkene();
    run(
        &mut document,
        &[Command::SetBondConfiguration {
            bond: BondRef::Id(bond(1)),
            configuration: Some(BondConfiguration::Unknown),
        }],
    );
    assert_eq!(
        double_bond_state(&document, 1),
        Some(DoubleBondState::Unknown)
    );
    let mut center = center_with(3);
    run(
        &mut center,
        &[Command::SetAtomConfiguration {
            atom: AtomRef::Id(atom(1)),
            configuration: Some(AtomConfiguration::Unknown {
                neighbors: [SlotRef::ImplicitHydrogen, s(4), s(2), s(3)],
            }),
        }],
    );
    assert_eq!(
        center_state(&center, 1),
        Some(CenterState::Unknown {
            slots: [
                Slot::Atom(atom(2)),
                Slot::Atom(atom(3)),
                Slot::Atom(atom(4)),
                Slot::ImplicitHydrogen
            ]
        })
    );
}

// ---------------------------------------------------------------------------------------------
// The cascade: a change of neighbors clears what it made meaningless
// ---------------------------------------------------------------------------------------------

fn configured_center() -> Document {
    let mut document = center_with(4);
    run(
        &mut document,
        &[tetra(1, [s(2), s(3), s(4), s(5)], Turn::Anticlockwise)],
    );
    document
}

#[test]
fn deleting_a_neighbor_clears_the_center_and_reports_it_and_undo_restores_everything() {
    let mut document = configured_center();
    let before = document.clone();
    let applied = run(
        &mut document,
        &[Command::DeleteAtoms {
            atoms: vec![atom(5)],
        }],
    );
    assert_eq!(applied.outcome, Outcome::Changed);
    assert_eq!(applied.cleared, [StereoKey::Center(atom(1))]);
    assert!(document.stereo().is_empty());
    document.undo().unwrap();
    assert_eq!(document.stereo(), before.stereo());
    assert_eq!(
        document.atoms().collect::<Vec<_>>(),
        before.atoms().collect::<Vec<_>>()
    );
    assert_eq!(
        document.bonds().collect::<Vec<_>>(),
        before.bonds().collect::<Vec<_>>()
    );
    document.redo().unwrap();
    assert!(document.stereo().is_empty());
}

#[test]
fn deleting_a_bond_clears_the_centers_at_both_ends() {
    // a1 and a2 are both four-bond centers joined by bond b1.
    let mut document = configured_center();
    run(
        &mut document,
        &[
            new_atom(),
            new_atom(),
            new_atom(),
            add_bond(2, 6, BondOrder::Single),
            add_bond(2, 7, BondOrder::Single),
            add_bond(2, 8, BondOrder::Single),
            tetra(2, [s(1), s(6), s(7), s(8)], Turn::Clockwise),
        ],
    );
    assert_eq!(document.stereo().len(), 2);
    let applied = run(
        &mut document,
        &[Command::DeleteBonds {
            bonds: vec![bond(1)],
        }],
    );
    assert_eq!(
        applied.cleared,
        [StereoKey::Center(atom(1)), StereoKey::Center(atom(2))]
    );
    assert!(document.stereo().is_empty());
    document.undo().unwrap();
    assert_eq!(document.stereo().len(), 2);
}

#[test]
fn deleting_the_center_itself_clears_its_record() {
    let mut document = configured_center();
    let applied = run(
        &mut document,
        &[Command::DeleteAtoms {
            atoms: vec![atom(1)],
        }],
    );
    assert_eq!(applied.cleared, [StereoKey::Center(atom(1))]);
    assert!(document.stereo().is_empty());
}

#[test]
fn adding_a_bond_to_a_center_clears_its_configuration_even_in_place_of_an_implicit_slot() {
    let mut document = center_with(3);
    run(
        &mut document,
        &[tetra(
            1,
            [SlotRef::ImplicitHydrogen, s(2), s(3), s(4)],
            Turn::Clockwise,
        )],
    );
    let applied = run(
        &mut document,
        &[Command::AddAtom {
            atom: Atom::from(Element::CARBON),
            positions: NewAtomPositions::Single(Point::new(1.0, 1.0).unwrap()),
            bond_to: Some(BondTo {
                atom: AtomRef::Id(atom(1)),
                order: BondOrder::Single,
            }),
        }],
    );
    assert_eq!(applied.cleared, [StereoKey::Center(atom(1))]);
    assert!(document.stereo().is_empty());
}

#[test]
fn a_change_that_leaves_the_neighbors_alone_keeps_the_record() {
    let mut document = configured_center();
    let applied = run(
        &mut document,
        &[
            Command::UpdateAtom {
                atom: atom(1),
                edit: AtomEdit {
                    charge: Some(1),
                    ..AtomEdit::default()
                },
            },
            Command::UpdateBond {
                bond: bond(1),
                order: BondOrder::Double,
            },
            new_atom(),
        ],
    );
    assert!(applied.cleared.is_empty());
    assert_eq!(document.stereo().len(), 1);
}

#[test]
fn a_double_bond_configuration_is_cleared_when_either_end_gains_or_loses_a_neighbor() {
    let reference_deleted = |end: u32, extra: Command| {
        let mut document = alkene();
        run(
            &mut document,
            &[relative(1, s(3), s(5), Relation::SameSide)],
        );
        let applied = run(&mut document, &[extra]);
        assert_eq!(
            applied.cleared,
            [StereoKey::DoubleBond(bond(1))],
            "end a{end}"
        );
        assert!(document.stereo().is_empty());
    };
    reference_deleted(
        1,
        Command::DeleteAtoms {
            atoms: vec![atom(3)],
        },
    );
    reference_deleted(
        1,
        Command::DeleteAtoms {
            atoms: vec![atom(4)],
        },
    );
    reference_deleted(
        2,
        Command::DeleteAtoms {
            atoms: vec![atom(5)],
        },
    );
    reference_deleted(
        2,
        Command::DeleteBonds {
            bonds: vec![bond(4)],
        },
    );
    reference_deleted(
        2,
        Command::DeleteAtoms {
            atoms: vec![atom(2)],
        },
    );
    // A new bond at either end, even to an atom with a higher identifier.
    let mut document = alkene();
    run(
        &mut document,
        &[relative(1, s(3), s(5), Relation::SameSide), new_atom()],
    );
    let applied = run(&mut document, &[add_bond(2, 6, BondOrder::Single)]);
    assert_eq!(applied.cleared, [StereoKey::DoubleBond(bond(1))]);
}

#[test]
fn explicit_clears_and_records_made_in_the_same_transaction_are_not_reported() {
    let mut document = configured_center();
    let applied = run(&mut document, &[clear_center(1)]);
    assert!(applied.cleared.is_empty());
    assert!(document.stereo().is_empty());

    // Created and invalidated within one transaction.
    let mut document = center_with(4);
    let applied = run(
        &mut document,
        &[
            tetra(1, [s(2), s(3), s(4), s(5)], Turn::Clockwise),
            Command::DeleteAtoms {
                atoms: vec![atom(5)],
            },
        ],
    );
    assert!(applied.cleared.is_empty());
    assert!(document.stereo().is_empty());
    assert_eq!(document.undo_depth(), 2, "the transaction is a change");
}

#[test]
fn a_record_cleared_and_set_again_in_one_transaction_is_not_reported_cleared() {
    let mut document = center_with(3);
    run(
        &mut document,
        &[tetra(
            1,
            [SlotRef::ImplicitHydrogen, s(2), s(3), s(4)],
            Turn::Clockwise,
        )],
    );
    let applied = run(
        &mut document,
        &[
            new_atom(),
            add_bond(1, 5, BondOrder::Single),
            tetra(1, [s(2), s(3), s(4), s(5)], Turn::Clockwise),
        ],
    );
    assert!(applied.cleared.is_empty());
    assert_eq!(document.stereo().len(), 1);
}

#[test]
fn the_order_of_commands_inside_a_transaction_decides_the_outcome_deterministically() {
    // Setting before the topology change: the change clears it.
    let mut document = center_with(3);
    run(&mut document, &[new_atom()]); // a5, not yet bonded
    let applied = run(
        &mut document,
        &[
            tetra(
                1,
                [SlotRef::ImplicitHydrogen, s(2), s(3), s(4)],
                Turn::Clockwise,
            ),
            add_bond(1, 5, BondOrder::Single),
        ],
    );
    assert!(applied.cleared.is_empty(), "created within the transaction");
    assert!(document.stereo().is_empty());

    // Setting after: the four-neighbor statement is checked against the new bonds.
    let mut document = center_with(3);
    run(&mut document, &[new_atom()]);
    run(
        &mut document,
        &[
            add_bond(1, 5, BondOrder::Single),
            tetra(1, [s(2), s(3), s(4), s(5)], Turn::Clockwise),
        ],
    );
    assert_eq!(document.stereo().len(), 1);
    // Before the bond exists, the same statement is refused.
    let mut document = center_with(3);
    run(&mut document, &[new_atom()]);
    let error = refusal(
        &mut document,
        &[
            tetra(1, [s(2), s(3), s(4), s(5)], Turn::Clockwise),
            add_bond(1, 5, BondOrder::Single),
        ],
    );
    assert_eq!(
        (error.index, stereo_refusal(&error)),
        (Some(0), Some(StereoError::CenterNeighbors))
    );
}

#[test]
fn a_bond_added_in_a_transaction_can_be_configured_by_its_command_index() {
    let mut document = empty();
    run(
        &mut document,
        &[
            new_atom(),
            new_atom(),
            new_atom(),
            new_atom(),
            add_bond(1, 3, BondOrder::Single),
            add_bond(2, 4, BondOrder::Single),
            Command::AddBond {
                begin: AtomRef::Id(atom(1)),
                end: AtomRef::Id(atom(2)),
                order: BondOrder::Double,
            },
            Command::SetBondConfiguration {
                bond: BondRef::Created(6),
                configuration: Some(BondConfiguration::Relative {
                    begin_neighbor: s(3),
                    end_neighbor: s(4),
                    relation: Relation::OppositeSides,
                }),
            },
            Command::SetBondMark {
                bond: BondRef::Created(4),
                mark: Some(StereoMark::Hash {
                    narrow: BondEnd::End,
                }),
            },
        ],
    );
    assert_eq!(
        double_bond_state(&document, 3),
        Some(DoubleBondState::Relative {
            begin_ref: Slot::Atom(atom(3)),
            end_ref: Slot::Atom(atom(4)),
            relation: Relation::OppositeSides
        })
    );
    let marked: Vec<_> = document
        .bonds()
        .filter_map(|(id, bond)| bond.mark.map(|mark| (id, mark)))
        .collect();
    assert_eq!(
        marked,
        [(
            bond(1),
            StereoMark::Hash {
                narrow: BondEnd::End
            }
        )]
    );

    // A reference to a command that created no bond, or to a later command, is refused.
    let error = refusal(
        &mut document,
        &[
            new_atom(),
            Command::SetBondMark {
                bond: BondRef::Created(0),
                mark: None,
            },
        ],
    );
    assert_eq!(
        (error.index, &error.error),
        (Some(1), &EditError::UnresolvedBondReference(0))
    );
}

// ---------------------------------------------------------------------------------------------
// Identity, no-op detection, history, restore
// ---------------------------------------------------------------------------------------------

#[test]
fn a_stereo_only_edit_is_a_change_and_the_same_state_again_is_not() {
    let mut document = center_with(4);
    let revision = document.revision();
    let depth = document.undo_depth();
    let applied = run(
        &mut document,
        &[tetra(1, [s(2), s(3), s(4), s(5)], Turn::Clockwise)],
    );
    assert_eq!(
        applied.outcome,
        Outcome::Changed,
        "a stereo-only edit must not vanish"
    );
    assert_eq!(document.revision(), revision + 1);
    assert_eq!(document.undo_depth(), depth + 1);
    let applied = run(
        &mut document,
        &[tetra(1, [s(3), s(2), s(4), s(5)], Turn::Anticlockwise)],
    );
    assert_eq!(
        applied.outcome,
        Outcome::Unchanged,
        "an equivalent statement in another order is the same state"
    );
    assert_eq!(document.revision(), revision + 1);
    // And the change set itself is not empty.
    let mut parts = document.to_parts();
    let set: ChangeSet = parts.undo.pop().unwrap();
    assert!(!set.is_empty());
    assert_eq!(set.stereo.len(), 1);
}

#[test]
fn undo_and_redo_restore_a_configuration_exactly() {
    let mut document = center_with(4);
    let plain = document.clone();
    run(
        &mut document,
        &[tetra(1, [s(2), s(3), s(4), s(5)], Turn::Clockwise)],
    );
    let configured = document.clone();
    document.undo().unwrap();
    assert_eq!(document.stereo(), plain.stereo());
    document.redo().unwrap();
    assert_eq!(document.stereo(), configured.stereo());
    assert_eq!(
        document.bonds().collect::<Vec<_>>(),
        configured.bonds().collect::<Vec<_>>()
    );
    assert_eq!(
        document.revision(),
        configured.revision() + 2,
        "each is a revision"
    );
}

#[test]
fn a_new_atom_never_inherits_a_deleted_atoms_record() {
    let mut document = configured_center();
    run(
        &mut document,
        &[Command::DeleteAtoms {
            atoms: vec![atom(1)],
        }],
    );
    document.undo().unwrap();
    assert!(document.stereo().get(StereoKey::Center(atom(1))).is_some());
    run(
        &mut document,
        &[Command::DeleteAtoms {
            atoms: vec![atom(1)],
        }],
    );
    run(&mut document, &[new_atom()]);
    let newest = document.atoms().last().unwrap().0;
    assert_eq!(newest, atom(6), "identifiers are never reused");
    assert!(document.stereo().is_empty());
    // A record cannot be set on a deleted atom.
    let error = refusal(
        &mut document,
        &[tetra(1, [s(2), s(3), s(4), s(5)], Turn::Clockwise)],
    );
    assert_eq!(error.error, EditError::UnknownAtom(atom(1)));
}

#[test]
fn save_as_copy_keeps_marks_and_configurations() {
    let mut document = configured_center();
    run(&mut document, &[mark(1, Some(wedge(BondEnd::End)))]);
    let copy = document
        .copy_as(DocumentId::parse("doc-copy").unwrap(), "Copy")
        .unwrap();
    assert_eq!(copy.stereo(), document.stereo());
    assert_eq!(
        copy.bonds().next().unwrap().1.mark,
        Some(wedge(BondEnd::End))
    );
}

#[test]
fn restoring_parts_reproduces_the_document_and_checks_the_records() {
    let mut document = configured_center();
    run(&mut document, &[mark(1, Some(wedge(BondEnd::Begin)))]);
    run(
        &mut document,
        &[Command::DeleteAtoms {
            atoms: vec![atom(5)],
        }],
    );
    document.undo().unwrap();
    let parts = document.to_parts();
    assert_eq!(Document::restore(parts.clone()).unwrap(), document);

    let rejected = |change: &dyn Fn(&mut DocumentParts), why: &str| {
        let mut broken = parts.clone();
        change(&mut broken);
        assert!(Document::restore(broken).is_err(), "{why}");
    };
    let center = |state: StereoState| (StereoKey::Center(atom(1)), state);
    rejected(
        &|parts| {
            parts.stereo = vec![(
                StereoKey::Center(atom(9)),
                StereoState::Center(CenterState::Unknown {
                    slots: [Slot::Atom(atom(2)); 4],
                }),
            )]
        },
        "a record for an atom that does not exist",
    );
    rejected(
        &|parts| {
            parts.stereo = vec![center(StereoState::Center(CenterState::Tetrahedral {
                slots: [
                    Slot::Atom(atom(3)),
                    Slot::Atom(atom(2)),
                    Slot::Atom(atom(4)),
                    Slot::Atom(atom(5)),
                ],
                turn: Turn::Clockwise,
            }))]
        },
        "slots not in canonical order",
    );
    rejected(
        &|parts| {
            parts.stereo = vec![center(StereoState::Center(CenterState::Tetrahedral {
                slots: [
                    Slot::Atom(atom(2)),
                    Slot::Atom(atom(3)),
                    Slot::Atom(atom(4)),
                    Slot::ImplicitHydrogen,
                ],
                turn: Turn::Clockwise,
            }))]
        },
        "a neighbor set that is not the bonded one",
    );
    rejected(
        &|parts| parts.stereo = vec![center(StereoState::DoubleBond(DoubleBondState::Unknown))],
        "a record whose kind does not match its key",
    );
    rejected(
        &|parts| {
            let record = parts.stereo[0];
            parts.stereo.push(record);
        },
        "a repeated record",
    );
    rejected(
        &|parts| parts.bonds[0].1.mark = Some(StereoMark::Crossed),
        "a mark on a bond it does not fit",
    );
    rejected(
        &|parts| {
            // History that adds a record for a missing bond.
            let set = parts.undo.last_mut().unwrap();
            set.stereo.push((
                StereoKey::DoubleBond(bond(77)),
                Change {
                    before: None,
                    after: Some(StereoState::DoubleBond(DoubleBondState::Unknown)),
                },
            ));
        },
        "history naming a record that cannot exist",
    );
    let unknown_bond = || {
        (
            StereoKey::DoubleBond(bond(1)),
            Change {
                before: None,
                after: Some(StereoState::DoubleBond(DoubleBondState::Unknown)),
            },
        )
    };
    rejected(
        &|parts| {
            // A stereo change that changes nothing, for a record that exists at that point.
            let state = parts.stereo[0].1;
            let set = parts.undo.last_mut().unwrap();
            set.stereo.push((
                StereoKey::Center(atom(1)),
                Change {
                    before: Some(state),
                    after: Some(state),
                },
            ));
        },
        "history with a stereo no-op",
    );
    rejected(
        &|parts| {
            // The same record changed twice in one entry.
            let set = parts.undo.last_mut().unwrap();
            set.stereo.push(unknown_bond());
            set.stereo.push(unknown_bond());
        },
        "history repeating a record in one entry",
    );
}

// ---------------------------------------------------------------------------------------------
// Cost: validation stays linear at the document limits
// ---------------------------------------------------------------------------------------------

/// 2,000 atoms and about 4,000 bonds in which every interior atom has four neighbors, so each
/// can be a stereocenter.
fn at_the_limits() -> Document {
    let mut document = empty();
    for batch in (1..=MAX_ATOMS).collect::<Vec<_>>().chunks(500) {
        let commands: Vec<Command> = batch.iter().map(|_| new_atom()).collect();
        run(&mut document, &commands);
    }
    let pairs: Vec<(u32, u32)> = (1..MAX_ATOMS as u32)
        .map(|n| (n, n + 1))
        .chain((1..MAX_ATOMS as u32 - 1).map(|n| (n, n + 2)))
        .take(MAX_BONDS)
        .collect();
    for batch in pairs.chunks(500) {
        let commands: Vec<Command> = batch
            .iter()
            .map(|&(a, b)| add_bond(a, b, BondOrder::Single))
            .collect();
        run(&mut document, &commands);
    }
    document
}

fn neighbors_of(n: u32) -> [SlotRef; 4] {
    [s(n - 2), s(n - 1), s(n + 1), s(n + 2)]
}

#[test]
fn validation_cost_stays_linear_at_two_thousand_atoms_and_a_full_history() {
    let mut document = at_the_limits();
    let interior: Vec<u32> = (3..=MAX_ATOMS as u32 - 2).collect();
    for batch in interior.chunks(499) {
        let commands: Vec<Command> = batch
            .iter()
            .map(|&n| tetra(n, neighbors_of(n), Turn::Anticlockwise))
            .collect();
        run(&mut document, &commands);
    }
    // A full history of stereo-only entries.
    for round in 0..HISTORY_LIMIT {
        let turn = if round % 2 == 0 {
            Turn::Clockwise
        } else {
            Turn::Anticlockwise
        };
        let commands: Vec<Command> = interior
            .iter()
            .skip(round * 7)
            .take(300)
            .map(|&n| tetra(n, neighbors_of(n), turn))
            .collect();
        run(&mut document, &commands);
    }
    assert_eq!(document.undo_depth(), HISTORY_LIMIT);
    assert_eq!(document.stereo().len(), interior.len());

    // One history step validates the whole document once. Checking it first fails a quadratic
    // validation in seconds instead of after minutes of the full walk below.
    let mut stepped = document.clone();
    let started = Instant::now();
    stepped.undo().unwrap();
    let one_step = started.elapsed();
    assert!(
        one_step < Duration::from_secs(1),
        "one undo step took {one_step:?} with {} records",
        document.stereo().len()
    );

    let started = Instant::now();
    let restored = Document::restore(document.to_parts()).unwrap();
    let restore_time = started.elapsed();
    assert_eq!(restored, document);

    let mut walked = document.clone();
    let started = Instant::now();
    for _ in 0..HISTORY_LIMIT {
        walked.undo().unwrap();
    }
    for _ in 0..HISTORY_LIMIT {
        walked.redo().unwrap();
    }
    let walk_time = started.elapsed();
    assert_eq!(walked.stereo(), document.stereo());

    // Measured in a debug build: about 1 s for the restore and 2 s for the 200 steps. Rebuilding
    // the neighbor map per record instead (quadratic) is many times over this.
    let budget = Duration::from_secs(20);
    assert!(
        restore_time < budget && walk_time < budget,
        "restore took {restore_time:?} and 200 history steps {walk_time:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// Randomized sequences: the invariants hold after every step
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

    fn pick<T: Clone>(&mut self, items: &[T]) -> Option<T> {
        (!items.is_empty()).then(|| items[self.below(items.len())].clone())
    }
}

fn neighbors(document: &Document, center: AtomId) -> Vec<AtomId> {
    let mut found: Vec<AtomId> = document
        .bonds()
        .filter_map(|(_, bond)| {
            if bond.begin == center {
                Some(bond.end)
            } else if bond.end == center {
                Some(bond.begin)
            } else {
                None
            }
        })
        .collect();
    found.sort();
    found
}

/// A statement of the neighbors a center has now, in a random order, or `None`.
fn random_center_command(random: &mut Random, document: &Document) -> Option<Command> {
    let atoms: Vec<AtomId> = document.atoms().map(|(id, _)| id).collect();
    let center = random.pick(&atoms)?;
    let found = neighbors(document, center);
    let mut slots: Vec<SlotRef> = found
        .iter()
        .map(|id| SlotRef::Atom(AtomRef::Id(*id)))
        .collect();
    match found.len() {
        3 => slots.push(if random.below(2) == 0 {
            SlotRef::ImplicitHydrogen
        } else {
            SlotRef::LonePair
        }),
        4 => {}
        _ => return None,
    }
    for index in (1..4).rev() {
        slots.swap(index, random.below(index + 1));
    }
    let neighbors: [SlotRef; 4] = slots.try_into().ok()?;
    Some(Command::SetAtomConfiguration {
        atom: AtomRef::Id(center),
        configuration: match random.below(4) {
            0 => None,
            1 => Some(AtomConfiguration::Unknown { neighbors }),
            2 => Some(AtomConfiguration::Tetrahedral {
                neighbors,
                turn: Turn::Clockwise,
            }),
            _ => Some(AtomConfiguration::Tetrahedral {
                neighbors,
                turn: Turn::Anticlockwise,
            }),
        },
    })
}

fn random_command(random: &mut Random, document: &Document) -> Command {
    let atoms: Vec<AtomId> = document.atoms().map(|(id, _)| id).collect();
    let bonds: Vec<(BondId, Bond)> = document.bonds().map(|(id, b)| (id, b.clone())).collect();
    let orders = [
        BondOrder::Single,
        BondOrder::Single,
        BondOrder::Double,
        BondOrder::Aromatic,
    ];
    match random.below(10) {
        0 | 1 => Command::AddAtom {
            atom: Atom::from(Element::CARBON),
            positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
            bond_to: random.pick(&atoms).map(|atom| BondTo {
                atom: AtomRef::Id(atom),
                order: BondOrder::Single,
            }),
        },
        2 | 3 => match (random.pick(&atoms), random.pick(&atoms)) {
            (Some(begin), Some(end)) => Command::AddBond {
                begin: AtomRef::Id(begin),
                end: AtomRef::Id(end),
                order: random.pick(&orders).unwrap(),
            },
            _ => new_atom(),
        },
        4 => Command::DeleteAtoms {
            atoms: random.pick(&atoms).into_iter().collect(),
        },
        5 => Command::DeleteBonds {
            bonds: random.pick(&bonds).map(|(id, _)| id).into_iter().collect(),
        },
        6 => match random.pick(&bonds) {
            Some((id, _)) => Command::SetBondMark {
                bond: BondRef::Id(id),
                mark: random
                    .pick(&[
                        None,
                        Some(wedge(BondEnd::Begin)),
                        Some(wedge(BondEnd::End)),
                        Some(StereoMark::Wavy),
                        Some(StereoMark::Crossed),
                    ])
                    .unwrap(),
            },
            None => new_atom(),
        },
        7 => match random.pick(&bonds) {
            Some((id, _)) => Command::UpdateBond {
                bond: id,
                order: random.pick(&orders).unwrap(),
            },
            None => new_atom(),
        },
        8 => match random.pick(&bonds) {
            Some((id, b)) => {
                let side = |atom: AtomId, partner: AtomId| {
                    neighbors(document, atom)
                        .into_iter()
                        .find(|n| *n != partner)
                        .map_or(SlotRef::LonePair, |n| SlotRef::Atom(AtomRef::Id(n)))
                };
                Command::SetBondConfiguration {
                    bond: BondRef::Id(id),
                    configuration: random
                        .pick(&[
                            None,
                            Some(BondConfiguration::Unknown),
                            Some(BondConfiguration::Relative {
                                begin_neighbor: side(b.begin, b.end),
                                end_neighbor: side(b.end, b.begin),
                                relation: Relation::SameSide,
                            }),
                        ])
                        .unwrap(),
                }
            }
            None => new_atom(),
        },
        _ => random_center_command(random, document).unwrap_or_else(new_atom),
    }
}

#[test]
fn random_stereo_edits_keep_every_invariant_and_replay_exactly() {
    let mut changed = 0_u32;
    let mut with_stereo = 0_u32;
    let mut cleared_total = 0_usize;
    for seed in 0..200 {
        let mut random = Random::new(seed);
        let mut document = empty();
        let mut initial: Vec<Command> = (0..6).map(|_| new_atom()).collect();
        initial.extend([
            add_bond(1, 2, BondOrder::Single),
            add_bond(1, 3, BondOrder::Single),
            add_bond(1, 4, BondOrder::Single),
            add_bond(1, 5, BondOrder::Single),
            add_bond(5, 6, BondOrder::Double),
        ]);
        run(&mut document, &initial);
        for _ in 0..40 {
            let before = document.clone();
            match random.below(8) {
                0 => {
                    let _ = document.undo();
                }
                1 => {
                    let _ = document.redo();
                }
                _ => {
                    let commands: Vec<Command> = (0..=random.below(2))
                        .map(|_| random_command(&mut random, &document))
                        .collect();
                    match document.apply_reporting(&commands, &TransactionSelection::default()) {
                        Ok(applied) => {
                            if applied.outcome == Outcome::Changed {
                                changed += 1;
                                // Every reported key existed before and is gone now.
                                for key in &applied.cleared {
                                    assert!(before.stereo().get(*key).is_some(), "seed {seed}");
                                    assert!(document.stereo().get(*key).is_none(), "seed {seed}");
                                }
                                cleared_total += applied.cleared.len();
                            } else {
                                assert_eq!(document, before, "seed {seed}: no-op changed state");
                            }
                        }
                        Err(_) => assert_eq!(document, before, "seed {seed}: refused changes"),
                    }
                }
            }
            if !document.stereo().is_empty() {
                with_stereo += 1;
            }
            // Restore identity and cascade closure: every record satisfies the invariants.
            let restored = Document::restore(document.to_parts())
                .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
            assert_eq!(restored, document, "seed {seed}");
            // Canonical idempotence.
            for (_, state) in document.stereo().iter() {
                if let StereoState::Center(CenterState::Tetrahedral { slots, turn }) = state {
                    assert_eq!(
                        CenterState::tetrahedral(*slots, *turn).unwrap(),
                        CenterState::Tetrahedral {
                            slots: *slots,
                            turn: *turn
                        }
                    );
                }
            }
            // Identifiers are never reused.
            assert!(document.numbering().next_atom >= before.numbering().next_atom);
            assert!(document.numbering().next_bond >= before.numbering().next_bond);
        }
        // Undo everything, then redo everything: the final state returns.
        let last = document.clone();
        let mut undone = 0;
        while document.undo().is_ok() {
            undone += 1;
        }
        for _ in 0..undone {
            document.redo().unwrap();
        }
        assert_eq!(document.stereo(), last.stereo(), "seed {seed}");
        assert_eq!(
            document.bonds().collect::<Vec<_>>(),
            last.bonds().collect::<Vec<_>>(),
            "seed {seed}"
        );
    }
    assert!(
        changed > 1000,
        "too few edits succeeded to mean much: {changed}"
    );
    assert!(
        with_stereo > 500,
        "too few states held stereo: {with_stereo}"
    );
    assert!(
        cleared_total > 20,
        "the cascade ran too rarely: {cleared_total}"
    );
}
