//! Scene objects: graphics and groups in the page's containment tree, the commands that edit
//! it, the selection history records, and the checks a restored document passes.

use std::collections::BTreeSet;

use chemifree_domain::document::{
    Atom, AtomId, Command, Document, DocumentId, EditError, NewAtomPositions, ObjectId, ObjectRef,
    Outcome, Point, SelectionAfter, SelectionUpdate, TransactionError, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{
    DeleteMode, Graphic, MAX_DEPTH, MAX_OBJECTS, ObjectKind, Parent, SceneError, SceneObject,
    Selection, Shape,
};

fn s(serial: u32) -> ObjectId {
    ObjectId::from_serial(serial).unwrap()
}

fn atom(serial: u32) -> AtomId {
    AtomId::from_serial(serial).unwrap()
}

fn point(x: f64, y: f64) -> Point {
    Point::new(x, y).unwrap()
}

fn offset(dx: f64, dy: f64) -> Vector {
    Vector { dx, dy }
}

fn rectangle(x: f64, y: f64) -> Graphic {
    Graphic {
        shape: Shape::Rectangle,
        points: [point(x, y), point(x + 40.0, y + 20.0)],
    }
}

fn add_graphic(graphic: Graphic, parent: Option<u32>) -> Command {
    Command::AddGraphic {
        graphic,
        parent: parent.map(s),
    }
}

fn group(objects: &[u32]) -> Command {
    Command::Group {
        objects: objects.iter().map(|serial| s(*serial)).collect(),
    }
}

fn move_objects(objects: &[u32], dx: f64, dy: f64) -> Command {
    Command::MoveObjects {
        objects: objects.iter().map(|serial| s(*serial)).collect(),
        offset: offset(dx, dy),
    }
}

fn delete_objects(objects: &[u32], mode: DeleteMode) -> Command {
    Command::DeleteObjects {
        objects: objects.iter().map(|serial| s(*serial)).collect(),
        mode,
    }
}

fn selection(objects: &[u32], atoms: &[u32]) -> Selection {
    Selection {
        objects: objects.iter().map(|serial| s(*serial)).collect(),
        atoms: atoms.iter().map(|serial| atom(*serial)).collect(),
        bonds: BTreeSet::new(),
    }
}

/// A selection after an edit naming the objects created by the commands at `created`, besides
/// `objects` and `atoms`.
fn selection_after(created: &[usize], objects: &[u32], atoms: &[u32]) -> SelectionAfter {
    let mut after = SelectionAfter::from(selection(objects, atoms));
    after
        .objects
        .extend(created.iter().map(|index| ObjectRef::Created(*index)));
    after
}

/// One carbon at (0, 0) drawn twice: `s1` and `s2` (moved by 100, 0), then a rectangle `s3`
/// at (0, 50) on the page.
fn drawing() -> Document {
    let mut document = Document::new(DocumentId::parse("doc-scene").unwrap(), "Scene").unwrap();
    document
        .apply(&[Command::AddAtom {
            atom: Atom::from(Element::CARBON),
            positions: NewAtomPositions::Single(point(0.0, 0.0)),
            bond_to: None,
        }])
        .unwrap();
    document
        .apply(&[
            Command::AddInstance {
                source: None,
                offset: offset(100.0, 0.0),
            },
            add_graphic(rectangle(0.0, 50.0), None),
        ])
        .unwrap();
    document
}

/// Applies `commands`, expects the refusal, and checks that nothing changed.
fn refused(document: &mut Document, commands: &[Command]) -> TransactionError {
    refused_with(document, commands, None)
}

fn refused_with(
    document: &mut Document,
    commands: &[Command],
    selection: Option<Selection>,
) -> TransactionError {
    let before = document.clone();
    let error = document
        .apply_with_selection(commands, selection)
        .expect_err("the transaction is refused");
    assert_eq!(*document, before, "a refused transaction changes nothing");
    error
}

fn graphic_points(document: &Document, id: u32) -> [Point; 2] {
    match document.object(s(id)) {
        Some(SceneObject::Graphic(graphic)) => graphic.points,
        other => panic!("s{id} is not a graphic: {other:?}"),
    }
}

fn children(document: &Document, id: u32) -> Vec<ObjectId> {
    match document.object(s(id)) {
        Some(SceneObject::Group(children)) => children.clone(),
        other => panic!("s{id} is not a group: {other:?}"),
    }
}

#[test]
fn a_new_document_has_its_instance_as_the_only_root() {
    let document = Document::new(DocumentId::parse("doc-new").unwrap(), "New").unwrap();
    assert_eq!(document.roots(), [s(1)]);
    assert_eq!(document.object(s(1)), Some(&SceneObject::Molecule));
    assert!(document.selection().is_empty());
}

#[test]
fn graphics_and_instances_join_the_page_in_order_with_one_counter() {
    let document = drawing();
    assert_eq!(document.roots(), [s(1), s(2), s(3)]);
    assert_eq!(
        document.object(s(3)),
        Some(&SceneObject::Graphic(rectangle(0.0, 50.0)))
    );
    assert_eq!(document.numbering().next_object, 4);
}

#[test]
fn a_graphic_can_be_added_to_a_group_and_nowhere_else() {
    let mut document = drawing();
    document.apply(&[group(&[3])]).unwrap();
    let line = Graphic {
        shape: Shape::Line,
        points: [point(0.0, 0.0), point(10.0, 10.0)],
    };
    document.apply(&[add_graphic(line, Some(4))]).unwrap();
    assert_eq!(children(&document, 4), [s(3), s(5)]);

    let error = refused(&mut document, &[add_graphic(line, Some(1))]);
    assert_eq!(
        error,
        TransactionError {
            index: Some(0),
            error: EditError::Scene(SceneError::KindMismatch {
                object: s(1),
                expected: ObjectKind::Group,
            }),
        }
    );
    assert_eq!(error.error.reason(), Some("object_kind_mismatch"));
    let error = refused(
        &mut document,
        &[add_graphic(line, None), add_graphic(line, Some(9))],
    );
    assert_eq!(error.index, Some(1));
    assert_eq!(error.error.reason(), Some("object_unknown"));
}

#[test]
fn grouping_puts_the_group_where_the_first_object_was_with_children_in_order() {
    let mut document = drawing();
    // Listed out of order and with a repeat: the children keep their order in the parent.
    document.apply(&[group(&[3, 2, 3])]).unwrap();
    assert_eq!(document.roots(), [s(1), s(4)]);
    assert_eq!(children(&document, 4), [s(2), s(3)]);

    document.apply(&[group(&[1])]).unwrap();
    assert_eq!(document.roots(), [s(5), s(4)]);
}

#[test]
fn grouping_objects_with_different_parents_is_refused() {
    let mut document = drawing();
    document.apply(&[group(&[2, 3])]).unwrap();
    let error = refused(&mut document, &[group(&[1, 3])]);
    assert_eq!(
        error,
        TransactionError {
            index: Some(0),
            error: EditError::Scene(SceneError::MixedParents),
        }
    );
    assert_eq!(error.error.reason(), Some("group_mixed_parents"));
    assert_eq!(
        refused(&mut document, &[group(&[])]).error,
        EditError::EmptyTargetList
    );
    assert_eq!(
        refused(&mut document, &[group(&[1, 7])]).error.reason(),
        Some("object_unknown")
    );
}

#[test]
fn ungrouping_puts_the_children_in_the_group_s_place_and_round_trips() {
    let mut document = drawing();
    let before = document.roots().to_vec();
    document.apply(&[group(&[2, 3])]).unwrap();
    document.apply(&[Command::Ungroup { group: s(4) }]).unwrap();
    assert_eq!(document.roots(), before, "the page's order is back");
    assert_eq!(document.object(s(4)), None, "the group is gone");

    let error = refused(&mut document, &[Command::Ungroup { group: s(3) }]);
    assert_eq!(error.error.reason(), Some("object_kind_mismatch"));
    let error = refused(&mut document, &[Command::Ungroup { group: s(4) }]);
    assert_eq!(
        error.error,
        EditError::Scene(SceneError::UnknownObject(s(4)))
    );
}

#[test]
fn moving_a_parent_and_its_child_moves_each_leaf_once() {
    let mut document = drawing();
    document.apply(&[group(&[2, 3])]).unwrap();
    document.apply(&[group(&[4])]).unwrap();
    // s5 ⊃ s4 ⊃ {s2, s3}; every level is listed, s2 twice.
    document
        .apply(&[move_objects(&[5, 4, 2, 3, 2], 10.0, -5.0)])
        .unwrap();
    assert_eq!(
        document.position(s(2), atom(1)),
        Some(point(110.0, -5.0)),
        "the instance moved exactly once"
    );
    assert_eq!(
        graphic_points(&document, 3),
        [point(10.0, 45.0), point(50.0, 65.0)],
        "the graphic moved exactly once"
    );
    assert_eq!(
        document.position(s(1), atom(1)),
        Some(point(0.0, 0.0)),
        "an instance outside the moved subtrees stays"
    );
}

#[test]
fn moving_nothing_or_by_zero_is_not_an_edit() {
    let mut document = drawing();
    let before = document.clone();
    assert_eq!(
        document.apply(&[move_objects(&[], 10.0, 0.0)]).unwrap(),
        Outcome::Unchanged
    );
    assert_eq!(
        document
            .apply_with_selection(
                &[move_objects(&[1, 3], 0.0, 0.0)],
                Some(selection(&[3], &[1]))
            )
            .unwrap(),
        Outcome::Unchanged
    );
    assert_eq!(
        document, before,
        "no revision, history, or selection recorded"
    );
    assert_eq!(
        refused(&mut document, &[move_objects(&[8], 1.0, 0.0)])
            .error
            .reason(),
        Some("object_unknown")
    );
}

#[test]
fn moving_an_instance_inside_a_group_moves_only_that_instance() {
    let mut document = drawing();
    document.apply(&[group(&[1, 2])]).unwrap();
    document
        .apply(&[Command::MoveInstance {
            instance: s(2),
            offset: offset(0.0, 7.0),
        }])
        .unwrap();
    assert_eq!(document.position(s(2), atom(1)), Some(point(100.0, 7.0)));
    assert_eq!(document.position(s(1), atom(1)), Some(point(0.0, 0.0)));

    let error = refused(
        &mut document,
        &[Command::MoveInstance {
            instance: s(4),
            offset: offset(1.0, 0.0),
        }],
    );
    assert_eq!(
        error.error,
        EditError::Scene(SceneError::KindMismatch {
            object: s(4),
            expected: ObjectKind::Molecule,
        })
    );
}

#[test]
fn deleting_a_subtree_removes_everything_in_it() {
    let mut document = drawing();
    document.apply(&[group(&[2, 3])]).unwrap();
    document
        .apply(&[delete_objects(&[4], DeleteMode::Subtree)])
        .unwrap();
    assert_eq!(document.roots(), [s(1)]);
    assert_eq!(document.objects().len(), 1);
    assert_eq!(
        document.positions(s(2)).count(),
        0,
        "s2's positions went too"
    );
}

#[test]
fn deleting_a_group_alone_puts_its_children_in_its_place() {
    let mut document = drawing();
    document.apply(&[group(&[2, 3])]).unwrap();
    document.apply(&[group(&[1, 4])]).unwrap();
    // s5 ⊃ {s1, s4 ⊃ {s2, s3}}; deleting s5 and s4 alone leaves s1, s2, s3 on the page.
    document
        .apply(&[delete_objects(&[5, 4], DeleteMode::ObjectOnly)])
        .unwrap();
    assert_eq!(document.roots(), [s(1), s(2), s(3)]);
    assert_eq!(document.position(s(2), atom(1)), Some(point(100.0, 0.0)));
}

#[test]
fn deleting_an_instance_leaves_its_group_in_place_even_when_empty() {
    let mut document = drawing();
    document.apply(&[group(&[2])]).unwrap();
    document
        .apply(&[Command::DeleteInstance { instance: s(2) }])
        .unwrap();
    assert_eq!(children(&document, 4), Vec::<ObjectId>::new());
    assert_eq!(document.roots(), [s(1), s(4), s(3)]);

    document.apply(&[group(&[3])]).unwrap();
    document
        .apply(&[delete_objects(&[3], DeleteMode::ObjectOnly)])
        .unwrap();
    assert_eq!(children(&document, 5), Vec::<ObjectId>::new());
}

#[test]
fn the_last_instance_cannot_be_deleted_by_any_command() {
    let mut document = drawing();
    document.apply(&[group(&[1, 2])]).unwrap();
    for command in [
        delete_objects(&[4], DeleteMode::Subtree),
        delete_objects(&[1, 2], DeleteMode::ObjectOnly),
    ] {
        let error = refused(&mut document, &[command]);
        assert_eq!(error.error, EditError::LastInstance);
        assert_eq!(error.error.reason(), Some("last_instance"));
    }
    document
        .apply(&[delete_objects(&[2], DeleteMode::Subtree)])
        .unwrap();
    let error = refused(&mut document, &[Command::DeleteInstance { instance: s(1) }]);
    assert_eq!(error.error.reason(), Some("last_instance"));
}

#[test]
fn reordering_moves_an_object_among_its_siblings() {
    let mut document = drawing();
    document
        .apply(&[Command::Reorder {
            object: s(3),
            index: 0,
        }])
        .unwrap();
    assert_eq!(document.roots(), [s(3), s(1), s(2)]);
    let error = refused(
        &mut document,
        &[Command::Reorder {
            object: s(3),
            index: 3,
        }],
    );
    assert_eq!(
        error.error,
        EditError::Scene(SceneError::IndexOutOfRange { index: 3, len: 3 })
    );
    assert_eq!(error.error.reason(), Some("index_out_of_range"));
}

#[test]
fn containment_is_at_most_sixteen_deep() {
    let mut document = drawing();
    // An empty group s4, then wrapped until it is MAX_DEPTH deep: s(3 + MAX_DEPTH) ⊃ … ⊃ s5 ⊃ s4.
    document.apply(&[group(&[3])]).unwrap();
    document
        .apply(&[delete_objects(&[3], DeleteMode::ObjectOnly)])
        .unwrap();
    let mut outer = 4;
    for _ in 1..MAX_DEPTH {
        document.apply(&[group(&[outer])]).unwrap();
        outer = document.numbering().next_object - 1;
    }
    assert_eq!(outer, 3 + MAX_DEPTH as u32);
    let error = refused(&mut document, &[add_graphic(rectangle(0.0, 0.0), Some(4))]);
    assert_eq!(error.error, EditError::Scene(SceneError::DepthLimit));
    assert_eq!(error.error.reason(), Some("scene_depth_limit"));
    let error = refused(&mut document, &[group(&[outer])]);
    assert_eq!(error.error.reason(), Some("scene_depth_limit"));
    // One level up there is still room.
    document
        .apply(&[add_graphic(rectangle(0.0, 0.0), Some(5))])
        .unwrap();
}

#[test]
fn a_document_holds_at_most_a_thousand_scene_objects() {
    let mut document = drawing();
    let room = MAX_OBJECTS - document.objects().len();
    let graphics: Vec<Command> = (0..room)
        .map(|index| add_graphic(rectangle(index as f64, 0.0), None))
        .collect();
    for batch in graphics.chunks(500) {
        document.apply(batch).unwrap();
    }
    assert_eq!(document.objects().len(), MAX_OBJECTS);
    for command in [
        add_graphic(rectangle(0.0, 0.0), None),
        group(&[3]),
        Command::AddInstance {
            source: None,
            offset: offset(1.0, 1.0),
        },
    ] {
        let error = refused(&mut document, &[command]);
        assert_eq!(error.error.reason(), Some("scene_object_limit"));
    }
}

#[test]
fn undo_and_redo_restore_graph_scene_and_selection_as_one_step() {
    let mut document = drawing();
    document
        .apply_with_selection(&[move_objects(&[3], 1.0, 0.0)], Some(selection(&[3], &[])))
        .unwrap();
    let before = document.clone();
    document
        .apply_with_selection(
            &[
                Command::AddAtom {
                    atom: Atom::from(Element::from_symbol("O").unwrap()),
                    positions: NewAtomPositions::PerInstance(
                        [(s(1), point(30.0, 0.0)), (s(2), point(130.0, 0.0))].into(),
                    ),
                    bond_to: None,
                },
                group(&[2, 3]),
                move_objects(&[4], 0.0, 9.0),
            ],
            Some(selection(&[4], &[2])),
        )
        .unwrap();
    let after = document.clone();
    assert_eq!(*document.selection(), selection(&[4], &[2]));

    document.undo().unwrap();
    assert_eq!(document.atoms().len(), before.atoms().len());
    assert_eq!(document.roots(), before.roots());
    assert_eq!(
        document.position(s(2), atom(1)),
        before.position(s(2), atom(1))
    );
    assert_eq!(graphic_points(&document, 3), graphic_points(&before, 3));
    assert_eq!(
        *document.selection(),
        selection(&[3], &[]),
        "the selection before"
    );

    document.redo().unwrap();
    assert_eq!(document.roots(), after.roots());
    assert_eq!(document.position(s(2), atom(2)), Some(point(130.0, 9.0)));
    assert_eq!(
        *document.selection(),
        selection(&[4], &[2]),
        "the selection after"
    );
}

#[test]
fn a_selection_naming_what_the_edit_leaves_out_is_refused() {
    let mut document = drawing();
    let error = refused_with(
        &mut document,
        &[delete_objects(&[3], DeleteMode::Subtree)],
        Some(selection(&[3], &[])),
    );
    assert_eq!(
        error,
        TransactionError {
            index: None,
            error: EditError::Scene(SceneError::UnknownSelection("s3".into())),
        }
    );
    assert_eq!(error.error.reason(), Some("selection_unknown"));
    let error = refused_with(
        &mut document,
        &[move_objects(&[3], 1.0, 1.0)],
        Some(selection(&[], &[9])),
    );
    assert_eq!(error.error.reason(), Some("selection_unknown"));
}

#[test]
fn the_selection_after_names_what_its_transaction_creates_by_command_index() {
    let mut document = drawing();
    document
        .apply_with_selections(
            &[
                move_objects(&[3], 1.0, 0.0),
                add_graphic(rectangle(0.0, 90.0), None),
                Command::AddInstance {
                    source: None,
                    offset: offset(0.0, 200.0),
                },
                group(&[2, 3]),
            ],
            SelectionUpdate {
                before: Some(selection(&[3], &[])),
                after: Some(selection_after(&[1, 2, 3], &[1], &[1])),
            },
        )
        .unwrap();
    // The rectangle is s4, the instance s5, and the group s6.
    assert_eq!(document.roots(), [s(1), s(6), s(4), s(5)]);
    let created = selection(&[1, 4, 5, 6], &[1]);
    assert_eq!(*document.selection(), created, "recorded with identifiers");

    document.undo().unwrap();
    assert_eq!(*document.selection(), selection(&[3], &[]));
    document.redo().unwrap();
    assert_eq!(*document.selection(), created);
    let restored = Document::restore(document.to_parts()).unwrap();
    assert_eq!(*restored.selection(), created);
}

#[test]
fn redo_selects_the_group_its_transaction_created() {
    let mut document = drawing();
    document
        .apply_with_selections(
            &[group(&[2, 3])],
            SelectionUpdate {
                before: Some(selection(&[2, 3], &[1])),
                after: Some(selection_after(&[0], &[], &[1])),
            },
        )
        .unwrap();
    assert_eq!(*document.selection(), selection(&[4], &[1]));
    document.undo().unwrap();
    assert_eq!(*document.selection(), selection(&[2, 3], &[1]));
    document.redo().unwrap();
    assert_eq!(*document.selection(), selection(&[4], &[1]), "the group");
}

#[test]
fn a_selection_naming_a_command_that_creates_no_object_is_refused() {
    let mut document = drawing();
    let commands = [
        Command::AddAtom {
            atom: Atom::from(Element::CARBON),
            positions: NewAtomPositions::PerInstance(
                [(s(1), point(0.0, 30.0)), (s(2), point(100.0, 30.0))].into(),
            ),
            bond_to: None,
        },
        move_objects(&[3], 1.0, 0.0),
        add_graphic(rectangle(0.0, 90.0), None),
    ];
    // An atom, a move, and an index past the last command.
    for index in [0, 1, 3] {
        let before = document.clone();
        let error = document
            .apply_with_selections(
                &commands,
                SelectionUpdate {
                    before: None,
                    after: Some(selection_after(&[2, index], &[], &[])),
                },
            )
            .expect_err("refused");
        assert_eq!(document, before, "a refused transaction changes nothing");
        assert_eq!(
            error,
            TransactionError {
                index: None,
                error: EditError::Scene(SceneError::UnresolvedSelection(index)),
            }
        );
        assert_eq!(error.error.reason(), Some("selection_unknown"));
        assert!(
            error.error.to_string().contains(&format!("${index}")),
            "{}",
            error.error
        );
    }
}

#[test]
fn the_selection_before_an_edit_is_what_undo_restores() {
    // The rectangle is added with nothing selected, then selected (not an edit) and moved.
    let mut document = drawing();
    assert!(document.selection().is_empty());
    document
        .apply_with_selections(
            &[move_objects(&[3], 5.0, 0.0)],
            SelectionUpdate {
                before: Some(selection(&[3], &[])),
                after: Some(selection(&[3], &[]).into()),
            },
        )
        .unwrap();
    document.undo().unwrap();
    assert_eq!(
        *document.selection(),
        selection(&[3], &[]),
        "the rectangle the user moved stays selected"
    );
    document.redo().unwrap();
    assert_eq!(*document.selection(), selection(&[3], &[]));

    // Without it, the entry's before is the selection the previous edit recorded.
    document
        .apply_with_selection(&[move_objects(&[3], 5.0, 0.0)], Some(selection(&[2], &[])))
        .unwrap();
    document.undo().unwrap();
    assert_eq!(*document.selection(), selection(&[3], &[]));
}

#[test]
fn a_selection_before_naming_what_the_document_does_not_hold_yet_is_refused() {
    let mut document = drawing();
    let before = document.clone();
    // s4 is the group this edit creates: it is not in the document before the edit.
    let error = document
        .apply_with_selections(
            &[group(&[2, 3])],
            SelectionUpdate {
                before: Some(selection(&[4], &[])),
                after: Some(selection(&[4], &[]).into()),
            },
        )
        .expect_err("refused");
    assert_eq!(document, before, "a refused transaction changes nothing");
    assert_eq!(error.index, None);
    assert_eq!(error.error.reason(), Some("selection_unknown"));
    assert!(
        error.error.to_string().contains("before"),
        "{}",
        error.error
    );
}

#[test]
fn a_selection_before_alone_records_nothing() {
    let mut document = drawing();
    let before = document.clone();
    let outcome = document
        .apply_with_selections(
            &[move_objects(&[3], 0.0, 0.0)],
            SelectionUpdate {
                before: Some(selection(&[3], &[1])),
                after: Some(selection(&[2], &[]).into()),
            },
        )
        .unwrap();
    assert_eq!(outcome, Outcome::Unchanged);
    assert_eq!(document, before);
}

#[test]
fn without_a_selection_an_edit_keeps_what_still_exists() {
    let mut document = drawing();
    document
        .apply_with_selection(
            &[move_objects(&[3], 1.0, 0.0)],
            Some(selection(&[2, 3], &[1])),
        )
        .unwrap();
    document
        .apply(&[delete_objects(&[3], DeleteMode::Subtree)])
        .unwrap();
    assert_eq!(*document.selection(), selection(&[2], &[1]));
}

#[test]
fn scene_object_identifiers_are_never_reused() {
    let mut document = drawing();
    document.apply(&[group(&[2, 3])]).unwrap();
    document.undo().unwrap();
    document.apply(&[group(&[2, 3])]).unwrap();
    assert_eq!(document.roots(), [s(1), s(5)], "s4 is not handed out again");
    document.apply(&[Command::Ungroup { group: s(5) }]).unwrap();
    let copy = document
        .copy_as(DocumentId::parse("doc-copy").unwrap(), "Copy")
        .unwrap();
    let mut restored = Document::restore(copy.to_parts()).unwrap();
    restored.apply(&[group(&[1])]).unwrap();
    assert_eq!(restored.roots(), [s(6), s(2), s(3)]);
}

#[test]
fn the_tree_is_listed_depth_first_with_parents() {
    let mut document = drawing();
    document.apply(&[group(&[2, 3])]).unwrap();
    assert_eq!(
        document.objects_in_tree_order(),
        [
            (s(1), Parent::Page),
            (s(4), Parent::Page),
            (s(2), Parent::Group(s(4))),
            (s(3), Parent::Group(s(4))),
        ]
    );
}

#[test]
fn restore_accepts_what_commands_make_and_rejects_broken_containment() {
    let mut document = drawing();
    document
        .apply_with_selection(&[group(&[2, 3])], Some(selection(&[4], &[1])))
        .unwrap();
    document.apply(&[group(&[4])]).unwrap();
    document.undo().unwrap();
    assert_eq!(Document::restore(document.to_parts()).unwrap(), document);

    let rejected = |change: &dyn Fn(&mut chemifree_domain::document::DocumentParts), why: &str| {
        let mut parts = document.to_parts();
        change(&mut parts);
        assert!(Document::restore(parts).is_err(), "{why}");
    };
    rejected(
        &|parts| parts.roots.push(s(2)),
        "an object with two parents",
    );
    rejected(
        &|parts| parts.roots.retain(|id| *id != s(4)),
        "a detached group",
    );
    rejected(
        &|parts| {
            // s4 and a new s5 contain each other, apart from the page.
            parts.roots.retain(|id| *id != s(4));
            parts.objects.push((s(5), SceneObject::Group(vec![s(4)])));
            for (id, object) in &mut parts.objects {
                if *id == s(4) {
                    *object = SceneObject::Group(vec![s(2), s(3), s(5)]);
                }
            }
            parts.next_object = 6;
        },
        "a cycle of groups",
    );
    rejected(
        &|parts| {
            for (id, object) in &mut parts.objects {
                if *id == s(4) {
                    *object = SceneObject::Group(vec![s(2), s(3), s(9)]);
                }
            }
        },
        "a child that does not exist",
    );
    rejected(
        &|parts| parts.selection.objects.insert(s(9)).then_some(()).unwrap(),
        "a selection naming an unknown object",
    );
    rejected(
        &|parts| {
            let last = parts.redo.len() - 1;
            parts.redo[last].selection.after.atoms.insert(atom(5));
        },
        "a redo entry selecting an atom that does not exist",
    );
    rejected(
        &|parts| {
            let last = parts.undo.len() - 1;
            parts.undo[last].selection.before.objects.insert(s(4));
        },
        "an undo entry selecting the group before it existed",
    );
    rejected(
        &|parts| {
            parts.positions.push((
                chemifree_domain::document::PositionKey {
                    instance: s(4),
                    atom: atom(1),
                },
                point(0.0, 0.0),
            ));
        },
        "a position in a group",
    );
}

#[test]
fn restore_rejects_containment_deeper_than_the_limit() {
    let mut parts = drawing().to_parts();
    parts.undo.clear();
    // s4 ⊃ s5 ⊃ … ⊃ s(3 + MAX_DEPTH) ⊃ s3: s3 ends up MAX_DEPTH + 1 deep.
    let first = 4;
    let last = first + MAX_DEPTH as u32 - 1;
    parts.roots = vec![s(1), s(2), s(first)];
    for serial in first..=last {
        let child = if serial == last { s(3) } else { s(serial + 1) };
        parts
            .objects
            .push((s(serial), SceneObject::Group(vec![child])));
    }
    parts.next_object = last + 1;
    let error = Document::restore(parts.clone()).unwrap_err();
    assert!(
        error.to_string().contains("deep"),
        "the depth is named: {error}"
    );
    // One level less is within the limit.
    parts.roots = vec![s(1), s(2), s(first + 1)];
    parts.objects.retain(|(id, _)| *id != s(first));
    Document::restore(parts).unwrap();
}

/// The edit `move_objects(s3)` sent with `before` and `after` as the client's selections.
fn nudged_with(document: &mut Document, before: Selection, after: Selection) {
    document
        .apply_with_selections(
            &[move_objects(&[3], 1.0, 0.0)],
            SelectionUpdate {
                before: Some(before),
                after: Some(after.into()),
            },
        )
        .unwrap();
}

#[test]
fn restore_refuses_a_selection_that_is_not_where_its_history_ends() {
    let mut document = drawing();
    nudged_with(&mut document, selection(&[], &[]), selection(&[3], &[]));
    nudged_with(
        &mut document,
        selection(&[2], &[]),
        selection(&[2, 3], &[1]),
    );
    // After an edit: the newest undo entry's selection after, and nothing else that exists.
    let mut parts = document.to_parts();
    parts.selection = selection(&[3], &[]);
    assert!(
        Document::restore(parts).is_err(),
        "an edit's selection after"
    );
    Document::restore(document.to_parts()).unwrap();

    // After an undo: the nearest redo entry's selection before, or the newest undo entry's
    // selection after (which a redo leaves); anything else is refused.
    document.undo().unwrap();
    assert_eq!(*document.selection(), selection(&[2], &[]));
    for (chosen, accepted) in [
        (selection(&[2], &[]), true),
        (selection(&[3], &[]), true),
        (selection(&[2, 3], &[1]), false),
        (selection(&[], &[]), false),
    ] {
        let mut parts = document.to_parts();
        parts.selection = chosen.clone();
        assert_eq!(
            Document::restore(parts).is_ok(),
            accepted,
            "{chosen:?} after an undo"
        );
    }
}

#[test]
fn a_redo_that_leaves_another_redo_entry_with_a_different_selection_before_restores() {
    // The client changed the selection between the edits, so the second edit's selection
    // before differs from what the first left. Redoing the first leaves the second in redo.
    let mut document = drawing();
    nudged_with(&mut document, selection(&[], &[]), selection(&[3], &[]));
    nudged_with(&mut document, selection(&[2], &[]), selection(&[2], &[]));
    document.undo().unwrap();
    document.undo().unwrap();
    document.redo().unwrap();
    assert_eq!(*document.selection(), selection(&[3], &[]));
    assert_eq!(Document::restore(document.to_parts()).unwrap(), document);
}

#[test]
fn trimming_every_undo_entry_on_restore_leaves_the_selection_at_the_nearest_redo_entry() {
    // A stored history one entry over the limit (as looser limits could have written): the
    // undo entry the selection matches is trimmed, so the selection becomes the one the
    // remaining history ends at, and the result restores again.
    let mut first = Document::new(DocumentId::parse("doc-trim").unwrap(), "Trim").unwrap();
    first
        .apply_with_selection(
            &[add_graphic(rectangle(0.0, 0.0), None)],
            Some(selection(&[2], &[])),
        )
        .unwrap();
    let added = first.to_parts().undo;
    let mut document = first.clone();
    for _ in 0..chemifree_domain::document::HISTORY_LIMIT {
        document
            .apply_with_selections(
                &[Command::MoveObjects {
                    objects: vec![s(2)],
                    offset: offset(1.0, 0.0),
                }],
                SelectionUpdate {
                    before: Some(selection(&[], &[])),
                    after: Some(selection(&[2], &[]).into()),
                },
            )
            .unwrap();
    }
    while document.undo_depth() > 0 {
        document.undo().unwrap();
    }
    let mut parts = document.to_parts();
    parts.undo = added;
    parts.selection = selection(&[2], &[]);
    let restored = Document::restore(parts).unwrap();
    assert_eq!(
        (restored.undo_depth(), restored.redo_depth()),
        (0, chemifree_domain::document::HISTORY_LIMIT)
    );
    assert!(
        restored.selection().is_empty(),
        "the nearest redo entry's before"
    );
    assert_eq!(Document::restore(restored.to_parts()).unwrap(), restored);
}
