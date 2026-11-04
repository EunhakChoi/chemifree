//! Enhanced-stereo groups: the model, the commands and their refusals, the cascade when a member
//! stops being a stored tetrahedral center, the notes a transaction reports, history, and
//! restore. Representation only: nothing here judges that members are stereogenic, and no
//! ratio or composition is stated for any kind.

use std::collections::BTreeSet;

use chemifree_domain::document::{
    Applied, Atom, AtomId, AtomRef, BondOrder, Change, ChangeSet, Command, Document, DocumentId,
    DocumentParts, EditError, GroupId, GroupRef, NewAtomPositions, Outcome, Point,
    TransactionError,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::TransactionSelection;
use chemifree_domain::stereo::{AtomConfiguration, SlotRef, Turn};
use chemifree_domain::stereo_group::{
    GroupKind, GroupNote, LeaveCause, StereoGroup, StereoGroupError,
};

fn atom(serial: u32) -> AtomId {
    AtomId::from_serial(serial).unwrap()
}

fn group(serial: u32) -> GroupId {
    GroupId::from_serial(serial).unwrap()
}

fn atom_refs(serials: &[u32]) -> Vec<AtomRef> {
    serials.iter().map(|n| AtomRef::Id(atom(*n))).collect()
}

fn gref(serial: u32) -> GroupRef {
    GroupRef::Id(group(serial))
}

fn members(serials: &[u32]) -> BTreeSet<AtomId> {
    serials.iter().map(|n| atom(*n)).collect()
}

fn new_atom() -> Command {
    Command::AddAtom {
        atom: Atom::from(Element::CARBON),
        positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
        bond_to: None,
    }
}

/// The `j`th (0..4) leaf of center `k` (1-based) in a [`fixture`] of `centers` centers.
fn leaf(centers: u32, k: u32, j: u32) -> u32 {
    centers + 1 + 4 * (k - 1) + j
}

fn configure(centers: u32, k: u32) -> Command {
    let neighbors = [0, 1, 2, 3].map(|j| SlotRef::Atom(AtomRef::Id(atom(leaf(centers, k, j)))));
    Command::SetAtomConfiguration {
        atom: AtomRef::Id(atom(k)),
        configuration: Some(AtomConfiguration::Tetrahedral {
            neighbors,
            turn: Turn::Clockwise,
        }),
    }
}

fn clear_configuration(k: u32) -> Command {
    Command::SetAtomConfiguration {
        atom: AtomRef::Id(atom(k)),
        configuration: None,
    }
}

fn unknown_configuration(centers: u32, k: u32) -> Command {
    let neighbors = [0, 1, 2, 3].map(|j| SlotRef::Atom(AtomRef::Id(atom(leaf(centers, k, j)))));
    Command::SetAtomConfiguration {
        atom: AtomRef::Id(atom(k)),
        configuration: Some(AtomConfiguration::Unknown { neighbors }),
    }
}

/// Atoms `a1..=a{centers}` are carbons with four leaf neighbors each and a stored tetrahedral
/// configuration; no group exists.
fn fixture(centers: u32) -> Document {
    let mut document = Document::new(DocumentId::parse("doc-groups").unwrap(), "Groups").unwrap();
    let mut commands: Vec<Command> = (0..centers * 5).map(|_| new_atom()).collect();
    for k in 1..=centers {
        for j in 0..4 {
            commands.push(Command::AddBond {
                begin: AtomRef::Id(atom(k)),
                end: AtomRef::Id(atom(leaf(centers, k, j))),
                order: BondOrder::Single,
            });
        }
    }
    commands.extend((1..=centers).map(|k| configure(centers, k)));
    run(&mut document, &commands);
    document
}

fn create(kind: GroupKind, number: Option<i32>, serials: &[u32]) -> Command {
    Command::CreateStereoGroup {
        kind,
        number,
        atoms: atom_refs(serials),
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

/// The group refusal of a transaction, with the index of the command that caused it.
fn group_refusal(
    document: &mut Document,
    commands: &[Command],
) -> (Option<usize>, StereoGroupError) {
    let error = refuse(document, commands);
    assert!(
        error.index.is_some(),
        "a refused group command names its command: {error}"
    );
    match error.error {
        EditError::StereoGroup(reason) => (error.index, reason),
        other => panic!("expected a group refusal, got {other:?}"),
    }
}

fn stored(document: &Document, serial: u32) -> (GroupKind, Option<i32>, BTreeSet<AtomId>) {
    let found = document
        .stereo_groups()
        .get(group(serial))
        .unwrap_or_else(|| panic!("group e{serial} exists"));
    (found.kind(), found.stored_number(), found.members().clone())
}

fn assert_group(
    document: &Document,
    serial: u32,
    kind: GroupKind,
    number: Option<i32>,
    who: &[u32],
) {
    assert_eq!(
        stored(document, serial),
        (kind, number, members(who)),
        "group e{serial}"
    );
}

// ---------------------------------------------------------------------------------------------
// Numbers: stored as stated, interpreted and labelled by computation
// ---------------------------------------------------------------------------------------------

#[test]
fn numbers_are_stored_as_stated_and_interpreted_by_computation() {
    // (kind, stated, interpreted, label)
    let cases = [
        (GroupKind::Or, None, Some(0), "or0"),
        (GroupKind::Or, Some(0), Some(0), "or0"),
        (GroupKind::Or, Some(-3), Some(-3), "or-3"),
        (GroupKind::And, Some(2), Some(2), "and2"),
        (
            GroupKind::And,
            Some(i32::MAX),
            Some(i32::MAX),
            "and2147483647",
        ),
        (
            GroupKind::And,
            Some(i32::MIN),
            Some(i32::MIN),
            "and-2147483648",
        ),
        (GroupKind::Absolute, None, None, "abs"),
    ];
    for (kind, stated, interpreted, label) in cases {
        let mut document = fixture(2);
        run(&mut document, &[create(kind, stated, &[1])]);
        let found = document.stereo_groups().get(group(1)).unwrap();
        assert_eq!(
            found.stored_number(),
            stated,
            "{kind:?} {stated:?}: stored as stated"
        );
        assert_eq!(
            found.interpreted_number(),
            interpreted,
            "{kind:?} {stated:?}: interpreted"
        );
        assert_eq!(found.label(), label, "{kind:?} {stated:?}: label");
    }
}

#[test]
fn an_absent_number_and_an_explicit_zero_differ_as_stored_values_but_are_one_group_on_the_wire() {
    let mut absent = fixture(2);
    run(&mut absent, &[create(GroupKind::Or, None, &[1])]);
    let mut zero = fixture(2);
    run(&mut zero, &[create(GroupKind::Or, Some(0), &[1])]);
    let (a, z) = (
        absent.stereo_groups().get(group(1)).unwrap(),
        zero.stereo_groups().get(group(1)).unwrap(),
    );
    assert_ne!(a.stored_number(), z.stored_number(), "stored values differ");
    assert_eq!(a.interpreted_number(), z.interpreted_number());
    assert_eq!(a.label(), z.label());

    // The wire collapses them, so one document cannot hold both.
    let both = [
        create(GroupKind::Or, None, &[1]),
        create(GroupKind::Or, Some(0), &[2]),
    ];
    let (index, reason) = group_refusal(&mut fixture(2), &both);
    assert_eq!((index, reason), (Some(1), StereoGroupError::NumberTaken));
    let both = [
        create(GroupKind::Or, Some(0), &[1]),
        create(GroupKind::Or, None, &[2]),
    ];
    let (index, reason) = group_refusal(&mut fixture(2), &both);
    assert_eq!((index, reason), (Some(1), StereoGroupError::NumberTaken));
}

#[test]
fn a_second_group_of_a_kind_cannot_take_a_number_and_the_refusal_points_to_merge() {
    let mut document = fixture(4);
    run(&mut document, &[create(GroupKind::Or, Some(1), &[1])]);
    // create with the number, set_number to it, and set_kind into a kind that holds it
    let (_, reason) = group_refusal(&mut document, &[create(GroupKind::Or, Some(1), &[2])]);
    assert_eq!(reason, StereoGroupError::NumberTaken);
    assert!(reason.to_string().contains("merge"), "{reason}");
    run(&mut document, &[create(GroupKind::Or, Some(2), &[2])]);
    let renumber = Command::SetStereoGroupNumber {
        group: gref(2),
        number: Some(1),
    };
    assert_eq!(
        group_refusal(&mut document, &[renumber]).1,
        StereoGroupError::NumberTaken
    );
    run(&mut document, &[create(GroupKind::And, Some(1), &[3])]);
    let into_or = Command::SetStereoGroupKind {
        group: gref(3),
        kind: GroupKind::Or,
    };
    assert_eq!(
        group_refusal(&mut document, &[into_or]).1,
        StereoGroupError::NumberTaken
    );
}

#[test]
fn or_1_and_and_1_are_different_groups_and_their_partition_keeps_them_apart() {
    let mut document = fixture(3);
    run(
        &mut document,
        &[
            create(GroupKind::Or, Some(1), &[1, 2]),
            create(GroupKind::And, Some(1), &[3]),
        ],
    );
    assert_group(&document, 1, GroupKind::Or, Some(1), &[1, 2]);
    assert_group(&document, 2, GroupKind::And, Some(1), &[3]);

    // The same members and the same number under the other kind are another meaning.
    let mut swapped = fixture(3);
    run(
        &mut swapped,
        &[
            create(GroupKind::And, Some(1), &[1, 2]),
            create(GroupKind::Or, Some(1), &[3]),
        ],
    );
    assert_ne!(
        document.stereo_groups().partition(),
        swapped.stereo_groups().partition()
    );
}

#[test]
fn the_partition_compares_membership_apart_from_ids_and_numbers() {
    let mut first = fixture(4);
    run(
        &mut first,
        &[
            create(GroupKind::Or, Some(1), &[1, 2]),
            create(GroupKind::And, Some(7), &[3, 4]),
        ],
    );
    // Created in the other order, so the identifiers swap, with other numbers.
    let mut second = fixture(4);
    run(
        &mut second,
        &[
            create(GroupKind::And, None, &[3, 4]),
            create(GroupKind::Or, Some(-9), &[1, 2]),
        ],
    );
    assert_ne!(
        first.stereo_groups().get(group(1)),
        second.stereo_groups().get(group(1))
    );
    assert_eq!(
        first.stereo_groups().partition(),
        second.stereo_groups().partition()
    );

    // A change of membership moves the partition; a change of number alone does not.
    let before = first.stereo_groups().partition();
    run(
        &mut first,
        &[Command::SetStereoGroupNumber {
            group: gref(1),
            number: Some(5),
        }],
    );
    assert_eq!(first.stereo_groups().partition(), before);
    run(
        &mut first,
        &[Command::RemoveStereoGroupMembers {
            group: gref(1),
            atoms: atom_refs(&[2]),
        }],
    );
    assert_ne!(first.stereo_groups().partition(), before);
}

#[test]
fn canonical_numbers_follow_membership_only_and_are_dense_per_kind() {
    let mut document = fixture(5);
    // Gaps and odd stored numbers, created out of member order.
    run(
        &mut document,
        &[
            create(GroupKind::Or, Some(9), &[4, 5]),
            create(GroupKind::Or, Some(-2), &[1]),
            create(GroupKind::And, None, &[3]),
            create(GroupKind::Absolute, None, &[2]),
        ],
    );
    let numbers = document.stereo_groups().canonical_numbers();
    let by_member = |serial: u32| {
        let (id, _) = document
            .stereo_groups()
            .iter()
            .find(|(_, group)| group.members().contains(&atom(serial)))
            .unwrap();
        numbers.get(&id)
    };
    assert_eq!(
        by_member(1),
        Some(&1),
        "lowest member first among the `or` groups"
    );
    assert_eq!(by_member(4), Some(&2));
    assert_eq!(by_member(3), Some(&1), "numbering restarts for each kind");
    assert_eq!(by_member(2), None, "an absolute group has no number");

    // Relabelling the stored numbers changes nothing.
    run(
        &mut document,
        &[
            Command::SetStereoGroupNumber {
                group: gref(1),
                number: Some(1),
            },
            Command::SetStereoGroupNumber {
                group: gref(2),
                number: Some(8),
            },
        ],
    );
    assert_eq!(document.stereo_groups().canonical_numbers(), numbers);
}

// ---------------------------------------------------------------------------------------------
// Kinds, membership, and the refusals of every command
// ---------------------------------------------------------------------------------------------

#[test]
fn an_absolute_group_coexists_with_an_and_group_and_singleton_groups_are_legal() {
    let mut document = fixture(3);
    let applied = run(
        &mut document,
        &[
            create(GroupKind::Absolute, None, &[1]),
            create(GroupKind::And, Some(1), &[2]),
            create(GroupKind::Or, None, &[3]),
        ],
    );
    assert_eq!(applied.outcome, Outcome::Changed);
    assert!(applied.group_notes.is_empty());
    assert_group(&document, 1, GroupKind::Absolute, None, &[1]);
    assert_group(&document, 2, GroupKind::And, Some(1), &[2]);
    assert_group(&document, 3, GroupKind::Or, None, &[3]);
}

#[test]
fn creating_a_group_is_refused_with_a_stable_code_for_each_precondition() {
    let mut document = fixture(5);
    // Atom a6 is a leaf (no configuration); center a3 is then cleared; a2 is made unknown.
    run(
        &mut document,
        &[clear_configuration(3), unknown_configuration(5, 2)],
    );
    run(&mut document, &[create(GroupKind::Absolute, None, &[1])]);
    let mut check = |commands: Vec<Command>, expected: StereoGroupError| {
        let (index, reason) = group_refusal(&mut document, &commands);
        assert_eq!(
            (index, reason),
            (Some(commands.len() - 1), expected),
            "{expected:?}"
        );
    };
    check(
        vec![create(GroupKind::Or, None, &[])],
        StereoGroupError::Empty,
    );
    check(
        vec![create(GroupKind::Or, None, &[6])],
        StereoGroupError::MemberNotCenter,
    );
    check(
        vec![create(GroupKind::Or, None, &[3])],
        StereoGroupError::MemberNotCenter,
    );
    check(
        vec![create(GroupKind::Or, None, &[2])],
        StereoGroupError::MemberNotCenter,
    );
    check(
        vec![create(GroupKind::Or, None, &[99])],
        StereoGroupError::MemberNotCenter,
    );
    check(
        vec![create(GroupKind::Or, None, &[1])],
        StereoGroupError::MemberTaken,
    );
    check(
        vec![create(GroupKind::Absolute, Some(1), &[5])],
        StereoGroupError::NumberNotAllowed,
    );
    assert_eq!(StereoGroupError::ALL.len(), 11, "every code is listed");
    let codes: BTreeSet<&str> = StereoGroupError::ALL.iter().map(|e| e.code()).collect();
    assert_eq!(codes.len(), 11, "codes are distinct");
    assert!(codes.iter().all(|code| code.starts_with("stereo_group_")));
}

#[test]
fn a_second_absolute_group_is_refused_however_it_is_made() {
    let mut document = fixture(4);
    run(
        &mut document,
        &[
            create(GroupKind::Absolute, None, &[1]),
            create(GroupKind::Or, None, &[2]),
            create(GroupKind::And, Some(1), &[3]),
        ],
    );
    assert_eq!(
        group_refusal(&mut document, &[create(GroupKind::Absolute, None, &[4])]).1,
        StereoGroupError::AbsoluteExists
    );
    let kind_to_absolute = |g: u32| Command::SetStereoGroupKind {
        group: gref(g),
        kind: GroupKind::Absolute,
    };
    // `or` with no stored number: only the existing absolute group refuses it.
    assert_eq!(
        group_refusal(&mut document, &[kind_to_absolute(2)]).1,
        StereoGroupError::AbsoluteExists
    );
    // A numbered group cannot become absolute at all, even once the absolute group is gone.
    run(
        &mut document,
        &[Command::DeleteStereoGroup { group: gref(1) }],
    );
    assert_eq!(
        group_refusal(&mut document, &[kind_to_absolute(3)]).1,
        StereoGroupError::NumberNotAllowed
    );
    run(&mut document, &[kind_to_absolute(2)]);
    assert_group(&document, 2, GroupKind::Absolute, None, &[2]);
    let number_on_absolute = Command::SetStereoGroupNumber {
        group: gref(2),
        number: Some(4),
    };
    assert_eq!(
        group_refusal(&mut document, &[number_on_absolute]),
        (Some(0), StereoGroupError::NumberNotAllowed)
    );
}

#[test]
fn members_are_added_and_removed_and_the_atoms_keep_their_configurations() {
    let mut document = fixture(4);
    run(&mut document, &[create(GroupKind::Or, Some(1), &[1])]);
    let add = |serials: &[u32]| Command::AddStereoGroupMembers {
        group: gref(1),
        atoms: atom_refs(serials),
    };
    run(&mut document, &[add(&[2, 3])]);
    assert_group(&document, 1, GroupKind::Or, Some(1), &[1, 2, 3]);
    // Adding a current member is not a change.
    let unchanged = run(&mut document, &[add(&[2])]);
    assert_eq!(unchanged.outcome, Outcome::Unchanged);

    let remove = |serials: &[u32]| Command::RemoveStereoGroupMembers {
        group: gref(1),
        atoms: atom_refs(serials),
    };
    assert_eq!(
        group_refusal(&mut document, &[remove(&[4])]).1,
        StereoGroupError::MemberAbsent
    );
    assert_eq!(
        group_refusal(&mut document, &[remove(&[1, 2, 3])]).1,
        StereoGroupError::Empty
    );
    assert_eq!(
        group_refusal(&mut document, &[remove(&[])]).1,
        StereoGroupError::Empty
    );
    assert_eq!(
        group_refusal(&mut document, &[add(&[])]).1,
        StereoGroupError::Empty
    );
    run(&mut document, &[create(GroupKind::And, None, &[4])]);
    assert_eq!(
        group_refusal(&mut document, &[add(&[4])]).1,
        StereoGroupError::MemberTaken
    );

    let applied = run(&mut document, &[remove(&[3])]);
    assert_group(&document, 1, GroupKind::Or, Some(1), &[1, 2]);
    assert_eq!(
        applied.group_notes,
        vec![GroupNote::RelationLost {
            group: group(1),
            together: vec![vec![atom(1), atom(2)]],
            ungrouped: vec![atom(3)],
        }],
        "the relation to the remaining members is no longer stated"
    );
    assert!(
        document
            .stereo()
            .get(chemifree_domain::stereo::StereoKey::Center(atom(3)))
            .is_some(),
        "the removed atom keeps its configuration"
    );
    assert!(
        document
            .stereo_groups()
            .iter()
            .all(|(_, group)| !group.members().contains(&atom(3)))
    );
}

#[test]
fn removing_a_member_of_the_absolute_group_states_no_relation_and_reports_nothing() {
    let mut document = fixture(2);
    run(&mut document, &[create(GroupKind::Absolute, None, &[1, 2])]);
    let applied = run(
        &mut document,
        &[Command::RemoveStereoGroupMembers {
            group: gref(1),
            atoms: atom_refs(&[2]),
        }],
    );
    assert!(applied.group_notes.is_empty(), "{:?}", applied.group_notes);
}

#[test]
fn deleting_a_group_ungroups_its_members_and_reports_only_a_lost_relation() {
    let mut document = fixture(3);
    run(
        &mut document,
        &[
            create(GroupKind::Or, Some(1), &[1, 2]),
            create(GroupKind::And, Some(1), &[3]),
        ],
    );
    let applied = run(
        &mut document,
        &[Command::DeleteStereoGroup { group: gref(1) }],
    );
    assert_eq!(
        applied.group_notes,
        vec![GroupNote::RelationLost {
            group: group(1),
            together: Vec::new(),
            ungrouped: vec![atom(1), atom(2)],
        }]
    );
    // One member states no relation to lose.
    let applied = run(
        &mut document,
        &[Command::DeleteStereoGroup { group: gref(2) }],
    );
    assert!(applied.group_notes.is_empty());
    assert!(document.stereo_groups().is_empty());
    let (index, reason) = group_refusal(
        &mut document,
        &[Command::DeleteStereoGroup { group: gref(1) }],
    );
    assert_eq!((index, reason), (Some(0), StereoGroupError::Unknown));
}

#[test]
fn renumbering_changes_only_the_label_and_is_not_a_loss() {
    let mut document = fixture(2);
    run(&mut document, &[create(GroupKind::Or, Some(1), &[1, 2])]);
    for number in [Some(7), Some(0), Some(-4), None] {
        let applied = run(
            &mut document,
            &[Command::SetStereoGroupNumber {
                group: gref(1),
                number,
            }],
        );
        assert!(
            applied.group_notes.is_empty(),
            "renumbering to {number:?} is no loss"
        );
        assert_group(&document, 1, GroupKind::Or, number, &[1, 2]);
    }
    // A change of kind between members that were together changes what is stated.
    let applied = run(
        &mut document,
        &[Command::SetStereoGroupKind {
            group: gref(1),
            kind: GroupKind::And,
        }],
    );
    assert_eq!(
        applied.group_notes,
        vec![GroupNote::KindChanged {
            group: group(1),
            atoms: vec![atom(1), atom(2)],
            from: GroupKind::Or,
            to: GroupKind::And,
        }]
    );
    assert_group(&document, 1, GroupKind::And, None, &[1, 2]);
}

#[test]
fn merging_reports_the_new_relation_and_keeps_the_target_kind_and_number() {
    let mut document = fixture(4);
    run(
        &mut document,
        &[
            create(GroupKind::Or, Some(1), &[1, 2]),
            create(GroupKind::Or, Some(2), &[3]),
            create(GroupKind::And, Some(1), &[4]),
        ],
    );
    let merge = |into: u32, from: u32| Command::MergeStereoGroups {
        into: gref(into),
        from: gref(from),
    };
    assert_eq!(
        group_refusal(&mut document, &[merge(1, 1)]).1,
        StereoGroupError::Same
    );
    assert_eq!(
        group_refusal(&mut document, &[merge(1, 3)]).1,
        StereoGroupError::KindMismatch
    );
    assert_eq!(
        group_refusal(&mut document, &[merge(1, 9)]).1,
        StereoGroupError::Unknown
    );
    let applied = run(&mut document, &[merge(1, 2)]);
    assert_group(&document, 1, GroupKind::Or, Some(1), &[1, 2, 3]);
    assert!(document.stereo_groups().get(group(2)).is_none());
    assert_eq!(
        applied.group_notes,
        vec![GroupNote::Merged {
            group: group(1),
            number: Some(1),
            kind: GroupKind::Or,
            sources: vec![
                (group(1), GroupKind::Or, Some(1)),
                (group(2), GroupKind::Or, Some(2)),
            ],
            intact: true,
        }]
    );
}

#[test]
fn splitting_reports_the_lost_relation_and_creates_a_group_of_the_same_kind() {
    let mut document = fixture(4);
    run(
        &mut document,
        &[
            create(GroupKind::Or, Some(1), &[1, 2, 3]),
            create(GroupKind::Absolute, None, &[4]),
        ],
    );
    let split = |g: u32, serials: &[u32], number: Option<i32>| Command::SplitStereoGroup {
        group: gref(g),
        atoms: atom_refs(serials),
        number,
    };
    assert_eq!(
        group_refusal(&mut document, &[split(2, &[4], None)]).1,
        StereoGroupError::SplitAbsolute
    );
    assert_eq!(
        group_refusal(&mut document, &[split(1, &[1, 2, 3], Some(2))]).1,
        StereoGroupError::Empty
    );
    assert_eq!(
        group_refusal(&mut document, &[split(1, &[], Some(2))]).1,
        StereoGroupError::Empty
    );
    assert_eq!(
        group_refusal(&mut document, &[split(1, &[4], Some(2))]).1,
        StereoGroupError::MemberAbsent
    );
    assert_eq!(
        group_refusal(&mut document, &[split(1, &[3], Some(1))]).1,
        StereoGroupError::NumberTaken
    );
    assert_eq!(
        group_refusal(&mut document, &[split(9, &[3], Some(2))]).1,
        StereoGroupError::Unknown
    );

    let applied = run(&mut document, &[split(1, &[3], Some(2))]);
    assert_group(&document, 1, GroupKind::Or, Some(1), &[1, 2]);
    assert_group(&document, 3, GroupKind::Or, Some(2), &[3]);
    assert_eq!(
        applied.group_notes,
        vec![GroupNote::RelationLost {
            group: group(1),
            together: vec![vec![atom(1), atom(2)], vec![atom(3)]],
            ungrouped: Vec::new(),
        }]
    );
}

#[test]
fn a_group_created_in_a_transaction_can_be_named_by_later_commands() {
    let mut document = fixture(3);
    let applied = run(
        &mut document,
        &[
            create(GroupKind::And, Some(1), &[1, 2]),
            Command::SetStereoGroupNumber {
                group: GroupRef::Created(0),
                number: None,
            },
            Command::SplitStereoGroup {
                group: GroupRef::Created(0),
                atoms: atom_refs(&[2]),
                number: Some(3),
            },
            Command::AddStereoGroupMembers {
                group: GroupRef::Created(2),
                atoms: atom_refs(&[3]),
            },
        ],
    );
    assert_group(&document, 1, GroupKind::And, None, &[1]);
    assert_group(&document, 2, GroupKind::And, Some(3), &[2, 3]);
    assert!(
        applied.group_notes.is_empty(),
        "groups built within the transaction are not reported"
    );

    // `$1` is the number-setting command, which created nothing.
    let error = refuse(
        &mut document,
        &[Command::DeleteStereoGroup {
            group: GroupRef::Created(1),
        }],
    );
    assert_eq!(error.error, EditError::UnresolvedGroupReference(1));
}

// ---------------------------------------------------------------------------------------------
// The cascade: a member that stops being a stored tetrahedral center leaves its group
// ---------------------------------------------------------------------------------------------

fn grouped_pair() -> Document {
    let mut document = fixture(3);
    run(
        &mut document,
        &[
            create(GroupKind::Or, Some(1), &[1, 2]),
            create(GroupKind::And, Some(1), &[3]),
        ],
    );
    document
}

#[test]
fn clearing_a_configuration_removes_the_member_and_reports_the_lost_relation() {
    let mut document = grouped_pair();
    let applied = run(&mut document, &[clear_configuration(2)]);
    assert_group(&document, 1, GroupKind::Or, Some(1), &[1]);
    assert_eq!(
        applied.group_notes,
        vec![GroupNote::MemberLeft {
            group: group(1),
            atom: atom(2),
            cause: LeaveCause::ConfigurationCleared,
            relation_lost: true,
        }]
    );
}

#[test]
fn stating_a_member_unknown_removes_it() {
    let mut document = grouped_pair();
    let applied = run(&mut document, &[unknown_configuration(3, 1)]);
    assert_group(&document, 1, GroupKind::Or, Some(1), &[2]);
    assert_eq!(
        applied.group_notes,
        vec![GroupNote::MemberLeft {
            group: group(1),
            atom: atom(1),
            cause: LeaveCause::ConfigurationUnknown,
            relation_lost: true,
        }]
    );
}

#[test]
fn a_neighbor_change_or_a_deleted_atom_removes_the_member_and_an_emptied_group() {
    // Deleting a member's neighbor clears its configuration.
    let mut document = grouped_pair();
    let applied = run(
        &mut document,
        &[Command::DeleteAtoms {
            atoms: vec![atom(leaf(3, 3, 0))],
        }],
    );
    assert!(
        document.stereo_groups().get(group(2)).is_none(),
        "the emptied group is gone"
    );
    assert_eq!(
        applied.group_notes,
        vec![
            GroupNote::MemberLeft {
                group: group(2),
                atom: atom(3),
                cause: LeaveCause::ConfigurationCleared,
                relation_lost: false,
            },
            GroupNote::GroupRemoved { group: group(2) },
        ],
        "a one-member group has no relation to lose, but the group disappears"
    );

    // Deleting the member atom itself.
    let mut document = grouped_pair();
    let applied = run(
        &mut document,
        &[Command::DeleteAtoms {
            atoms: vec![atom(1)],
        }],
    );
    assert_group(&document, 1, GroupKind::Or, Some(1), &[2]);
    assert_eq!(applied.group_notes.len(), 1);
    assert!(matches!(
        applied.group_notes[0],
        GroupNote::MemberLeft { atom: a, relation_lost: true, .. } if a == atom(1)
    ));
}

#[test]
fn a_member_of_the_absolute_group_that_leaves_loses_no_relation() {
    let mut document = fixture(2);
    run(&mut document, &[create(GroupKind::Absolute, None, &[1, 2])]);
    let applied = run(&mut document, &[clear_configuration(1)]);
    assert!(matches!(
        applied.group_notes[0],
        GroupNote::MemberLeft {
            relation_lost: false,
            ..
        }
    ));
}

#[test]
fn what_a_transaction_builds_and_tears_down_itself_is_not_reported() {
    let mut document = fixture(2);
    let applied = run(
        &mut document,
        &[
            create(GroupKind::Or, None, &[1, 2]),
            clear_configuration(1),
            clear_configuration(2),
        ],
    );
    assert!(document.stereo_groups().is_empty());
    assert!(applied.group_notes.is_empty(), "{:?}", applied.group_notes);
}

#[test]
fn a_group_cannot_be_formed_on_a_center_cleared_earlier_in_the_transaction() {
    let mut document = fixture(2);
    let (index, reason) = group_refusal(
        &mut document,
        &[clear_configuration(1), create(GroupKind::Or, None, &[1])],
    );
    assert_eq!(
        (index, reason),
        (Some(1), StereoGroupError::MemberNotCenter)
    );
}

// ---------------------------------------------------------------------------------------------
// History, identity, copy
// ---------------------------------------------------------------------------------------------

#[test]
fn undo_and_redo_restore_groups_and_their_cascade_with_the_same_ids() {
    let mut document = grouped_pair();
    let grouped = document.clone();
    run(&mut document, &[clear_configuration(2)]);
    let cascaded = document.clone();
    document.undo().unwrap();
    assert_eq!(
        document.stereo_groups(),
        grouped.stereo_groups(),
        "undo brings the member back"
    );
    assert_group(&document, 1, GroupKind::Or, Some(1), &[1, 2]);
    document.redo().unwrap();
    assert_eq!(document.stereo_groups(), cascaded.stereo_groups());

    // Back to before the groups existed, and forward again: ids are the ones first handed out.
    document.undo().unwrap();
    document.undo().unwrap();
    assert!(document.stereo_groups().is_empty());
    document.redo().unwrap();
    document.redo().unwrap();
    assert_eq!(document.stereo_groups(), cascaded.stereo_groups());
    assert!(document.stereo_groups().get(group(2)).is_some());
}

#[test]
fn identifiers_are_never_reused_after_undo_or_deletion() {
    let mut document = fixture(2);
    run(&mut document, &[create(GroupKind::Or, None, &[1])]);
    document.undo().unwrap();
    run(&mut document, &[create(GroupKind::Or, None, &[1])]);
    assert!(document.stereo_groups().get(group(1)).is_none());
    assert_group(&document, 2, GroupKind::Or, None, &[1]);
    run(
        &mut document,
        &[Command::DeleteStereoGroup { group: gref(2) }],
    );
    run(&mut document, &[create(GroupKind::Or, None, &[1])]);
    assert!(document.stereo_groups().get(group(3)).is_some());
    assert_eq!(document.numbering().next_group, 4);
}

#[test]
fn copy_as_keeps_the_groups_and_the_counter() {
    let mut document = grouped_pair();
    run(
        &mut document,
        &[Command::DeleteStereoGroup { group: gref(2) }],
    );
    let copy = document
        .copy_as(DocumentId::parse("copy").unwrap(), "Copy")
        .unwrap();
    assert_eq!(copy.stereo_groups(), document.stereo_groups());
    assert_eq!(copy.numbering().next_group, document.numbering().next_group);
}

// ---------------------------------------------------------------------------------------------
// Restore: every invariant is checked on load
// ---------------------------------------------------------------------------------------------

fn parts_with(groups: Vec<(u32, StereoGroup)>, next_group: u32) -> DocumentParts {
    let mut parts = fixture(4).to_parts();
    parts.stereo_groups = groups.into_iter().map(|(n, g)| (group(n), g)).collect();
    // Groups placed directly are not the product of the history, which would undo them.
    parts.undo.clear();
    parts.redo.clear();
    parts.next_group = next_group;
    parts
}

fn stated(kind: GroupKind, number: Option<i32>, serials: &[u32]) -> StereoGroup {
    StereoGroup::new(kind, number, members(serials))
}

fn restore_message(parts: DocumentParts) -> String {
    Document::restore(parts)
        .expect_err("restore refuses")
        .to_string()
}

fn content_error(reason: StereoGroupError) -> String {
    format!("invalid document content: {reason}")
}

#[test]
fn restore_accepts_valid_groups_as_the_positive_control() {
    let parts = parts_with(
        vec![
            (1, stated(GroupKind::Absolute, None, &[1])),
            (2, stated(GroupKind::Or, Some(0), &[2])),
            (3, stated(GroupKind::And, Some(0), &[3])),
            (4, stated(GroupKind::Or, Some(-1), &[4])),
        ],
        5,
    );
    let document = Document::restore(parts.clone()).expect("valid groups restore");
    assert_eq!(document.to_parts().stereo_groups, parts.stereo_groups);
}

#[test]
fn restore_refuses_a_group_that_breaks_an_invariant() {
    // A member that has no configuration (a leaf), that is unknown, and one that is missing.
    let leaf_member = parts_with(vec![(1, stated(GroupKind::Or, None, &[5]))], 2);
    assert_eq!(
        restore_message(leaf_member),
        content_error(StereoGroupError::MemberNotCenter)
    );
    let missing = parts_with(vec![(1, stated(GroupKind::Or, None, &[99]))], 2);
    assert_eq!(
        restore_message(missing),
        content_error(StereoGroupError::MemberNotCenter)
    );
    let mut unknown = parts_with(vec![(1, stated(GroupKind::Or, None, &[1]))], 2);
    let mut scratch = fixture(4);
    run(&mut scratch, &[unknown_configuration(4, 1)]);
    unknown.stereo = scratch.to_parts().stereo;
    assert_eq!(
        restore_message(unknown),
        content_error(StereoGroupError::MemberNotCenter)
    );

    let empty = parts_with(vec![(1, stated(GroupKind::Or, None, &[]))], 2);
    assert_eq!(
        restore_message(empty),
        content_error(StereoGroupError::Empty)
    );
    let shared = parts_with(
        vec![
            (1, stated(GroupKind::Or, Some(1), &[1])),
            (2, stated(GroupKind::And, Some(1), &[1])),
        ],
        3,
    );
    assert_eq!(
        restore_message(shared),
        content_error(StereoGroupError::MemberTaken)
    );
    let numbered_absolute = parts_with(vec![(1, stated(GroupKind::Absolute, Some(1), &[1]))], 2);
    assert_eq!(
        restore_message(numbered_absolute),
        content_error(StereoGroupError::NumberNotAllowed)
    );
    let two_absolute = parts_with(
        vec![
            (1, stated(GroupKind::Absolute, None, &[1])),
            (2, stated(GroupKind::Absolute, None, &[2])),
        ],
        3,
    );
    assert_eq!(
        restore_message(two_absolute),
        content_error(StereoGroupError::AbsoluteExists)
    );
    let same_number = parts_with(
        vec![
            (1, stated(GroupKind::Or, Some(3), &[1])),
            (2, stated(GroupKind::Or, Some(3), &[2])),
        ],
        3,
    );
    assert_eq!(
        restore_message(same_number),
        content_error(StereoGroupError::NumberTaken)
    );
    // No number and number 0 are one number on the wire.
    let none_and_zero = parts_with(
        vec![
            (1, stated(GroupKind::And, None, &[1])),
            (2, stated(GroupKind::And, Some(0), &[2])),
        ],
        3,
    );
    assert_eq!(
        restore_message(none_and_zero),
        content_error(StereoGroupError::NumberTaken)
    );
}

#[test]
fn restore_refuses_a_counter_that_would_reuse_a_group_identifier() {
    for next_group in [0, 1, 2] {
        let parts = parts_with(
            vec![
                (1, stated(GroupKind::Or, None, &[1])),
                (2, stated(GroupKind::And, None, &[2])),
            ],
            next_group,
        );
        assert_eq!(
            restore_message(parts),
            "identifier counter would reuse an existing identifier",
            "next_group {next_group}"
        );
    }
    let mut parts = parts_with(vec![], 1);
    parts.stereo_groups = vec![
        (group(1), stated(GroupKind::Or, None, &[1])),
        (group(1), stated(GroupKind::And, None, &[2])),
    ];
    parts.next_group = 2;
    assert!(restore_message(parts).contains("duplicate"));
}

#[test]
fn restore_refuses_history_that_does_not_match_or_reuses_a_group_id() {
    let mut document = fixture(2);
    run(&mut document, &[create(GroupKind::Or, None, &[1])]);
    run(
        &mut document,
        &[Command::DeleteStereoGroup { group: gref(1) }],
    );
    // Entry 0 builds the fixture, 1 creates e1, 2 deletes it.
    let positive = document.to_parts();
    assert!(Document::restore(positive.clone()).is_ok());

    // An entry that changes nothing.
    let mut no_op = positive.clone();
    let entry = &mut no_op.undo[1].stereo_groups[0].1;
    entry.before = entry.after.clone();
    assert!(restore_message(no_op).contains("no-op"));

    // A history that does not replay from the current state.
    let mut mismatch = positive.clone();
    mismatch.undo[1].stereo_groups[0].1.after = Some(stated(GroupKind::Or, Some(5), &[1]));
    assert_eq!(
        restore_message(mismatch),
        "undo history does not match the document"
    );

    // The identifier created a second time: every step replays, only the reuse is wrong.
    let mut reused = positive;
    let again = stated(GroupKind::Or, Some(2), &[1]);
    reused.stereo_groups = vec![(group(1), again.clone())];
    reused.undo.push(ChangeSet {
        stereo_groups: vec![(
            group(1),
            Change {
                before: None,
                after: Some(again),
            },
        )],
        ..Default::default()
    });
    assert_eq!(
        restore_message(reused),
        "history reuses a deleted identifier"
    );
}

// ---------------------------------------------------------------------------------------------
// Review round 1: one rule for notes (a relation among at least two members of a group that
// existed before the transaction no longer holds, or means something else, at its end) and one
// pruning point (the end of the transaction)
// ---------------------------------------------------------------------------------------------

fn add_to(g: u32, serials: &[u32]) -> Command {
    Command::AddStereoGroupMembers {
        group: gref(g),
        atoms: atom_refs(serials),
    }
}

fn split_off(g: u32, serials: &[u32], number: Option<i32>) -> Command {
    Command::SplitStereoGroup {
        group: gref(g),
        atoms: atom_refs(serials),
        number,
    }
}

#[test]
fn y1_a_member_added_and_split_off_again_within_one_transaction_loses_no_relation() {
    let mut document = fixture(3);
    run(&mut document, &[create(GroupKind::Or, Some(1), &[1])]);
    let applied = run(
        &mut document,
        &[add_to(1, &[2]), split_off(1, &[2], Some(2))],
    );
    assert_group(&document, 2, GroupKind::Or, Some(2), &[2]);
    assert!(applied.group_notes.is_empty(), "{:?}", applied.group_notes);
}

#[test]
fn z1_clearing_and_restating_a_center_in_one_transaction_keeps_its_group() {
    let mut document = fixture(2);
    run(&mut document, &[create(GroupKind::Or, Some(1), &[1, 2])]);
    let applied = run(&mut document, &[clear_configuration(1), configure(2, 1)]);
    assert_eq!(
        applied.outcome,
        Outcome::Unchanged,
        "the table shows no net change"
    );
    assert_group(&document, 1, GroupKind::Or, Some(1), &[1, 2]);
    assert!(applied.group_notes.is_empty());
}

#[test]
fn z2_a_member_that_left_after_a_split_or_a_merge_is_still_reported_for_its_original_group() {
    let mut document = fixture(4);
    run(
        &mut document,
        &[
            create(GroupKind::Or, Some(1), &[1, 2, 3]),
            create(GroupKind::Or, Some(2), &[4]),
        ],
    );
    let left = |group_serial: u32, atom_serial: u32| GroupNote::MemberLeft {
        group: group(group_serial),
        atom: atom(atom_serial),
        cause: LeaveCause::ConfigurationCleared,
        relation_lost: true,
    };
    let mut after_split = document.clone();
    let applied = run(
        &mut after_split,
        &[split_off(1, &[3], Some(3)), clear_configuration(3)],
    );
    assert!(
        applied.group_notes.contains(&left(1, 3)),
        "{:?}",
        applied.group_notes
    );

    // e2 = {a4} merges into e1 = {a1, a2, a3}; then a3 is cleared.
    let applied = run(
        &mut document,
        &[
            Command::MergeStereoGroups {
                into: gref(2),
                from: gref(1),
            },
            clear_configuration(3),
        ],
    );
    assert_eq!(
        applied.group_notes,
        vec![
            left(1, 3),
            GroupNote::Merged {
                group: group(2),
                number: Some(2),
                kind: GroupKind::Or,
                sources: vec![
                    (group(1), GroupKind::Or, Some(1)),
                    (group(2), GroupKind::Or, Some(2)),
                ],
                intact: true,
            },
        ],
        "the note lists the former groups and no atom that has since left"
    );
}

#[test]
fn a_merge_that_does_not_keep_every_member_together_says_so() {
    let mut document = fixture(3);
    run(
        &mut document,
        &[
            create(GroupKind::Or, Some(1), &[1, 2]),
            create(GroupKind::Or, Some(2), &[3]),
        ],
    );
    let applied = run(
        &mut document,
        &[
            Command::MergeStereoGroups {
                into: gref(1),
                from: gref(2),
            },
            Command::RemoveStereoGroupMembers {
                group: gref(1),
                atoms: atom_refs(&[2]),
            },
        ],
    );
    assert!(applied.group_notes.contains(&GroupNote::Merged {
        group: group(1),
        number: Some(1),
        kind: GroupKind::Or,
        sources: vec![
            (group(1), GroupKind::Or, Some(1)),
            (group(2), GroupKind::Or, Some(2)),
        ],
        intact: false,
    }));
    assert!(applied.group_notes.contains(&GroupNote::RelationLost {
        group: group(1),
        together: vec![vec![atom(1)]],
        ungrouped: vec![atom(2)],
    }));
}

fn merge(into: u32, from: u32) -> Command {
    Command::MergeStereoGroups {
        into: gref(into),
        from: gref(from),
    }
}

fn set_kind(g: u32, kind: GroupKind) -> Command {
    Command::SetStereoGroupKind {
        group: gref(g),
        kind,
    }
}

fn merged(
    group_serial: u32,
    kind: GroupKind,
    sources: &[(u32, GroupKind, i32)],
    intact: bool,
) -> GroupNote {
    GroupNote::Merged {
        group: group(group_serial),
        number: (kind != GroupKind::Absolute).then_some(1),
        kind,
        sources: sources
            .iter()
            .map(|(id, kind, number)| (group(*id), *kind, Some(*number)))
            .collect(),
        intact,
    }
}

#[test]
fn two_single_groups_merged_and_made_another_kind_in_one_transaction_are_one_merge_note() {
    // The sources are found by the groups the atoms were in before the transaction, whatever
    // kind the result has, and the kind change is part of that note.
    let mut document = fixture(2);
    run(
        &mut document,
        &[
            create(GroupKind::Or, Some(1), &[1]),
            create(GroupKind::Or, Some(2), &[2]),
        ],
    );
    let applied = run(&mut document, &[merge(1, 2), set_kind(1, GroupKind::And)]);
    assert_group(&document, 1, GroupKind::And, Some(1), &[1, 2]);
    assert_eq!(
        applied.group_notes,
        vec![merged(
            1,
            GroupKind::And,
            &[(1, GroupKind::Or, 1), (2, GroupKind::Or, 2)],
            true
        )]
    );
}

#[test]
fn an_or_group_and_an_and_group_made_one_or_group_are_a_merge_not_a_separate_kind_change() {
    let mut document = fixture(4);
    run(
        &mut document,
        &[
            create(GroupKind::Or, Some(1), &[1, 2]),
            create(GroupKind::And, Some(1), &[3, 4]),
        ],
    );
    let applied = run(
        &mut document,
        &[
            Command::DeleteStereoGroup { group: gref(2) },
            add_to(1, &[3, 4]),
        ],
    );
    assert_eq!(
        applied.group_notes,
        vec![merged(
            1,
            GroupKind::Or,
            &[(1, GroupKind::Or, 1), (2, GroupKind::And, 1)],
            true
        )]
    );
}

#[test]
fn and_groups_merged_into_an_absolute_group_state_no_new_relation_and_lose_the_old_ones() {
    let mut document = fixture(4);
    run(
        &mut document,
        &[
            create(GroupKind::And, None, &[1, 2]),
            create(GroupKind::And, Some(1), &[3, 4]),
        ],
    );
    let applied = run(
        &mut document,
        &[merge(1, 2), set_kind(1, GroupKind::Absolute)],
    );
    let kind_change = |g, atoms: &[u32]| GroupNote::KindChanged {
        group: group(g),
        atoms: members(atoms).into_iter().collect(),
        from: GroupKind::And,
        to: GroupKind::Absolute,
    };
    assert_eq!(
        applied.group_notes,
        vec![kind_change(1, &[1, 2]), kind_change(2, &[3, 4])]
    );
}

#[test]
fn a_merge_followed_by_a_split_reports_what_the_final_grouping_says() {
    let base = || {
        let mut document = fixture(4);
        run(
            &mut document,
            &[
                create(GroupKind::Or, Some(1), &[1, 2]),
                create(GroupKind::Or, Some(2), &[3, 4]),
            ],
        );
        document
    };
    // Split along the old lines: nothing was related that was not before.
    let applied = run(&mut base(), &[merge(1, 2), split_off(1, &[3, 4], Some(2))]);
    assert_eq!(applied.group_notes, vec![]);
    // Split across them: the final groups mix the old ones, and the old relations are gone.
    let applied = run(&mut base(), &[merge(1, 2), split_off(1, &[2, 3], Some(2))]);
    let homes: Vec<_> = applied
        .group_notes
        .iter()
        .filter_map(|note| match note {
            GroupNote::Merged { group, intact, .. } => Some((*group, *intact)),
            _ => None,
        })
        .collect();
    assert_eq!(homes, vec![(group(1), false), (group(3), false)]);
    assert_eq!(
        applied
            .group_notes
            .iter()
            .filter(|note| matches!(note, GroupNote::RelationLost { .. }))
            .count(),
        2
    );
}

#[test]
fn a_merge_then_a_kind_change_then_a_cleared_member_keeps_one_note_for_what_remains() {
    let base = |second: &[u32]| {
        let mut document = fixture(4);
        run(
            &mut document,
            &[
                create(GroupKind::Or, Some(1), &[1, 2]),
                create(GroupKind::Or, Some(2), second),
            ],
        );
        document
    };
    let steps = |clear: u32| {
        [
            merge(1, 2),
            set_kind(1, GroupKind::And),
            clear_configuration(clear),
        ]
    };
    let applied = run(&mut base(&[3, 4]), &steps(4));
    assert_eq!(
        applied.group_notes,
        vec![
            GroupNote::MemberLeft {
                group: group(2),
                atom: atom(4),
                cause: LeaveCause::ConfigurationCleared,
                relation_lost: true,
            },
            merged(
                1,
                GroupKind::And,
                &[(1, GroupKind::Or, 1), (2, GroupKind::Or, 2)],
                true
            ),
        ]
    );
    // When the second group's only member is the one cleared, no merge is left to report: the
    // first group simply changed kind.
    let applied = run(&mut base(&[3]), &steps(3));
    assert_eq!(
        applied.group_notes,
        vec![
            GroupNote::MemberLeft {
                group: group(2),
                atom: atom(3),
                cause: LeaveCause::ConfigurationCleared,
                relation_lost: false,
            },
            GroupNote::GroupRemoved { group: group(2) },
            GroupNote::KindChanged {
                group: group(1),
                atoms: vec![atom(1), atom(2)],
                from: GroupKind::Or,
                to: GroupKind::And,
            },
        ]
    );
}

#[test]
fn leaving_an_and_or_group_for_absolute_loses_the_relation_but_the_reverse_states_a_new_one() {
    let mut document = fixture(2);
    run(&mut document, &[create(GroupKind::And, None, &[1, 2])]);
    let kind = |kind| Command::SetStereoGroupKind {
        group: gref(1),
        kind,
    };
    let applied = run(&mut document, &[kind(GroupKind::Absolute)]);
    assert_eq!(
        applied.group_notes,
        vec![GroupNote::KindChanged {
            group: group(1),
            atoms: vec![atom(1), atom(2)],
            from: GroupKind::And,
            to: GroupKind::Absolute,
        }]
    );
    // Absolute members are independent, so stating a relation between them loses nothing.
    assert!(
        run(&mut document, &[kind(GroupKind::Or)])
            .group_notes
            .is_empty()
    );
}

#[test]
fn a_group_command_sees_members_as_stated_until_the_transaction_ends() {
    // The only member of the absolute group is cleared, but the group exists until the end of
    // the transaction, so a second absolute group is still refused, and a member can be
    // restated without losing its group.
    let mut document = fixture(2);
    run(&mut document, &[create(GroupKind::Absolute, None, &[1])]);
    let (index, reason) = group_refusal(
        &mut document,
        &[
            clear_configuration(1),
            create(GroupKind::Absolute, None, &[2]),
        ],
    );
    assert_eq!((index, reason), (Some(1), StereoGroupError::AbsoluteExists));
    let applied = run(&mut document, &[clear_configuration(1), configure(2, 1)]);
    assert_eq!(applied.outcome, Outcome::Unchanged);
    assert_group(&document, 1, GroupKind::Absolute, None, &[1]);
}

#[test]
fn z3_deleting_or_changing_a_group_is_a_note_and_stating_something_new_is_not() {
    let base = || {
        let mut document = fixture(3);
        run(&mut document, &[create(GroupKind::Or, Some(1), &[1, 2])]);
        document
    };
    let delete = Command::DeleteStereoGroup { group: gref(1) };
    assert!(
        !run(&mut base(), &[delete]).group_notes.is_empty(),
        "deleting a pair"
    );
    let to_and = Command::SetStereoGroupKind {
        group: gref(1),
        kind: GroupKind::And,
    };
    assert!(
        !run(&mut base(), &[to_and]).group_notes.is_empty(),
        "or becomes and"
    );
    assert!(
        run(&mut base(), &[add_to(1, &[3])]).group_notes.is_empty(),
        "adding states more"
    );
    let create_delete = [
        create(GroupKind::And, None, &[3]),
        Command::DeleteStereoGroup {
            group: GroupRef::Created(0),
        },
    ];
    assert!(run(&mut base(), &create_delete).group_notes.is_empty());
    let single = |kind| {
        let mut document = fixture(2);
        run(&mut document, &[create(GroupKind::Or, None, &[1])]);
        run(
            &mut document,
            &[Command::SetStereoGroupKind {
                group: gref(1),
                kind,
            }],
        )
        .group_notes
    };
    assert!(
        single(GroupKind::And).is_empty(),
        "no relation among one member"
    );
}
