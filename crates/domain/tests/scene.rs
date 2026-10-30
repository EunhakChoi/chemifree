//! The scene containment tree (groups, graphics, instances), its commands, and the selection
//! recorded with history.

use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondId, BondOrder, BondTo, Change, Command, Document, DocumentId,
    EditError, HISTORY_LIMIT, NewAtomPositions, ObjectId, Outcome, Point, TransactionError, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{
    DeleteMode, Graphic, GraphicShape, MAX_DEPTH, MAX_OBJECTS, ObjectKind, ObjectRef, SceneError,
    SceneObject, Selection, SelectionRequest, TransactionSelection,
};

fn point(x: f64, y: f64) -> Point {
    Point::new(x, y).unwrap()
}

fn object(serial: u32) -> ObjectId {
    ObjectId::from_serial(serial).unwrap()
}

fn atom(serial: u32) -> AtomId {
    AtomId::from_serial(serial).unwrap()
}

fn bond(serial: u32) -> BondId {
    BondId::from_serial(serial).unwrap()
}

fn empty() -> Document {
    Document::new(DocumentId::parse("doc-1").unwrap(), "Scene").unwrap()
}

fn line(x: f64) -> Graphic {
    Graphic {
        shape: GraphicShape::Line,
        points: [point(x, 0.0), point(x + 10.0, 0.0)],
    }
}

fn add_graphic(graphic: Graphic, parent: Option<u32>, index: Option<usize>) -> Command {
    Command::AddGraphic {
        graphic,
        parent: parent.map(object),
        index,
    }
}

fn group(serials: &[u32]) -> Command {
    Command::Group {
        objects: serials.iter().copied().map(object).collect(),
    }
}

fn delete(serials: &[u32], mode: DeleteMode) -> Command {
    Command::DeleteObjects {
        objects: serials.iter().copied().map(object).collect(),
        mode,
    }
}

fn move_objects(serials: &[u32], dx: f64, dy: f64) -> Command {
    Command::MoveObjects {
        objects: serials.iter().copied().map(object).collect(),
        delta: Vector { dx, dy },
    }
}

/// The tree as `id(parent)` in drawing order, e.g. `["s1(-)", "s4(-)", "s2(s4)"]`.
fn tree(document: &Document) -> Vec<String> {
    document
        .scene_objects()
        .into_iter()
        .map(|(id, parent, _)| match parent {
            Some(parent) => format!("{id}({parent})"),
            None => format!("{id}(-)"),
        })
        .collect()
}

fn graphic_points(document: &Document, serial: u32) -> [(f64, f64); 2] {
    match document.object(object(serial)) {
        Some(SceneObject::Graphic(graphic)) => graphic.points.map(|p| (p.x(), p.y())),
        other => panic!("s{serial} is not a graphic: {other:?}"),
    }
}

/// `s1` holds a carbon at the origin; `s2` and `s3` are lines on the page.
fn drawing() -> Document {
    let mut document = empty();
    document
        .apply(&[
            Command::AddAtom {
                atom: Atom::from(Element::CARBON),
                positions: NewAtomPositions::Single(point(0.0, 0.0)),
                bond_to: None,
            },
            add_graphic(line(0.0), None, None),
            add_graphic(line(20.0), None, None),
        ])
        .unwrap();
    document
}

fn refused(document: &mut Document, commands: &[Command]) -> TransactionError {
    refused_selecting(document, commands, &TransactionSelection::default())
}

fn refused_selecting(
    document: &mut Document,
    commands: &[Command],
    selection: &TransactionSelection,
) -> TransactionError {
    let before = document.clone();
    let error = document.apply_selecting(commands, selection).unwrap_err();
    assert_eq!(*document, before, "a refused transaction changes nothing");
    error
}

fn reason(error: &TransactionError) -> Option<&'static str> {
    error.error.reason()
}

fn selection(objects: &[u32], atoms: &[u32], bonds: &[u32]) -> Selection {
    Selection {
        objects: objects.iter().copied().map(object).collect(),
        atoms: atoms.iter().copied().map(atom).collect(),
        bonds: bonds.iter().copied().map(bond).collect(),
    }
}

fn select(objects: &[ObjectRef]) -> SelectionRequest {
    SelectionRequest {
        objects: objects.to_vec(),
        ..SelectionRequest::default()
    }
}

#[test]
fn graphics_are_added_to_the_page_or_into_a_group_at_an_index() {
    let mut document = drawing();
    assert_eq!(tree(&document), ["s1(-)", "s2(-)", "s3(-)"]);
    document.apply(&[group(&[2, 3])]).unwrap();
    document
        .apply(&[
            add_graphic(line(40.0), Some(4), Some(1)),
            add_graphic(
                Graphic {
                    shape: GraphicShape::Arrow,
                    points: [point(0.0, 0.0), point(5.0, 5.0)],
                },
                None,
                Some(0),
            ),
        ])
        .unwrap();
    assert_eq!(
        tree(&document),
        ["s6(-)", "s1(-)", "s4(-)", "s2(s4)", "s5(s4)", "s3(s4)"]
    );
    assert_eq!(document.roots(), [object(6), object(1), object(4)]);
}

#[test]
fn adding_into_what_is_not_a_group_or_past_the_end_is_refused() {
    let mut document = drawing();
    let cases = [
        (add_graphic(line(0.0), Some(9), None), "object_unknown"),
        (
            add_graphic(line(0.0), Some(1), None),
            "object_kind_mismatch",
        ),
        (add_graphic(line(0.0), None, Some(4)), "index_out_of_range"),
    ];
    for (command, code) in cases {
        let error = refused(&mut document, &[command]);
        assert_eq!((error.index, reason(&error)), (Some(0), Some(code)));
    }
    let error = refused(
        &mut document,
        &[
            add_graphic(line(0.0), None, None),
            Command::MoveInstance {
                instance: object(2),
                offset: Vector { dx: 1.0, dy: 0.0 },
            },
        ],
    );
    assert_eq!(
        (error.index, &error.error),
        (
            Some(1),
            &EditError::ObjectKindMismatch {
                object: object(2),
                expected: ObjectKind::Instance
            }
        )
    );
}

#[test]
fn a_group_takes_the_place_of_its_earliest_member_and_keeps_their_order() {
    let mut document = drawing();
    document
        .apply(&[add_graphic(line(40.0), None, None)])
        .unwrap();
    // Listed out of order: the order on the page wins.
    document.apply(&[group(&[4, 2])]).unwrap();
    assert_eq!(
        tree(&document),
        ["s1(-)", "s5(-)", "s2(s5)", "s4(s5)", "s3(-)"]
    );
    document
        .apply(&[Command::Ungroup { group: object(5) }])
        .unwrap();
    assert_eq!(tree(&document), ["s1(-)", "s2(-)", "s4(-)", "s3(-)"]);
    document.undo().unwrap();
    assert_eq!(
        tree(&document),
        ["s1(-)", "s5(-)", "s2(s5)", "s4(s5)", "s3(-)"]
    );
}

#[test]
fn grouping_needs_two_objects_that_share_a_parent() {
    let mut document = drawing();
    document.apply(&[group(&[2, 3])]).unwrap();
    let error = refused(&mut document, &[group(&[1, 2])]);
    assert_eq!(reason(&error), Some("group_mixed_parents"));
    let error = refused(&mut document, &[group(&[1, 1])]);
    assert_eq!(error.error, EditError::GroupTooSmall);
    let error = refused(&mut document, &[group(&[1, 7])]);
    assert_eq!(reason(&error), Some("object_unknown"));
    let error = refused(&mut document, &[Command::Ungroup { group: object(2) }]);
    assert_eq!(reason(&error), Some("object_kind_mismatch"));
}

#[test]
fn reordering_moves_an_object_among_its_siblings() {
    let mut document = drawing();
    document
        .apply(&[Command::Reorder {
            object: object(1),
            index: 2,
        }])
        .unwrap();
    assert_eq!(tree(&document), ["s2(-)", "s3(-)", "s1(-)"]);
    let error = refused(
        &mut document,
        &[Command::Reorder {
            object: object(1),
            index: 3,
        }],
    );
    assert_eq!(reason(&error), Some("index_out_of_range"));
    let unchanged = document.clone();
    assert_eq!(
        document.apply(&[Command::Reorder {
            object: object(1),
            index: 2,
        }]),
        Ok(Outcome::Unchanged)
    );
    assert_eq!(document, unchanged);
}

#[test]
fn deleting_an_object_only_lifts_its_children_into_its_place() {
    let mut document = drawing();
    document
        .apply(&[
            group(&[2, 3]),
            Command::Reorder {
                object: object(4),
                index: 0,
            },
        ])
        .unwrap();
    let grouped = document.clone();
    document
        .apply(&[delete(&[4], DeleteMode::ObjectOnly)])
        .unwrap();
    assert_eq!(tree(&document), ["s2(-)", "s3(-)", "s1(-)"]);
    document.undo().unwrap();
    assert_eq!(document.to_parts().roots, grouped.to_parts().roots);
    document
        .apply(&[delete(&[4], DeleteMode::Subtree)])
        .unwrap();
    assert_eq!(tree(&document), ["s1(-)"]);
    document.undo().unwrap();
    assert_eq!(tree(&document), tree(&grouped));
}

#[test]
fn deleting_a_parent_and_its_child_object_only_keeps_the_grandchildren() {
    let mut document = drawing();
    document
        .apply(&[
            add_graphic(line(40.0), None, None),
            group(&[2, 3]),
            group(&[5, 4]),
        ])
        .unwrap();
    assert_eq!(
        tree(&document),
        ["s1(-)", "s6(-)", "s5(s6)", "s2(s5)", "s3(s5)", "s4(s6)"]
    );
    document
        .apply(&[delete(&[6, 5], DeleteMode::ObjectOnly)])
        .unwrap();
    assert_eq!(tree(&document), ["s1(-)", "s2(-)", "s3(-)", "s4(-)"]);
}

#[test]
fn the_last_instance_is_never_deleted() {
    let mut document = drawing();
    document.apply(&[group(&[1, 2])]).unwrap();
    for mode in [DeleteMode::Subtree, DeleteMode::ObjectOnly] {
        let error = refused(&mut document, &[delete(&[1], mode)]);
        assert_eq!(
            (error.index, reason(&error)),
            (Some(0), Some("last_instance"))
        );
    }
    let error = refused(&mut document, &[delete(&[4], DeleteMode::Subtree)]);
    assert_eq!(reason(&error), Some("last_instance"));
    let error = refused(
        &mut document,
        &[Command::DeleteInstance {
            instance: object(1),
        }],
    );
    assert_eq!(reason(&error), Some("last_instance"));
    // Another instance makes the deletion possible, positions and all.
    document
        .apply(&[
            Command::AddInstance {
                source: None,
                offset: Vector { dx: 0.0, dy: 50.0 },
            },
            delete(&[4], DeleteMode::Subtree),
        ])
        .unwrap();
    assert_eq!(tree(&document), ["s3(-)", "s5(-)"]);
    assert_eq!(document.positions(object(1)).count(), 0);
    assert_eq!(
        document.position(object(5), atom(1)),
        Some(point(0.0, 50.0))
    );
}

#[test]
fn moving_a_group_and_its_member_moves_every_leaf_once() {
    let mut document = drawing();
    document.apply(&[group(&[1, 2])]).unwrap();
    document
        .apply(&[move_objects(&[4, 1, 2, 2], 5.0, -1.0)])
        .unwrap();
    assert_eq!(
        document.position(object(1), atom(1)),
        Some(point(5.0, -1.0))
    );
    assert_eq!(graphic_points(&document, 2), [(5.0, -1.0), (15.0, -1.0)]);
    assert_eq!(
        graphic_points(&document, 3),
        [(20.0, 0.0), (30.0, 0.0)],
        "what is not listed stays"
    );
    let error = refused(&mut document, &[move_objects(&[3], 100_000.0, 0.0)]);
    assert_eq!(error.error, EditError::CoordinateOutOfRange);
}

#[test]
fn nesting_is_bounded_in_depth() {
    let mut document = drawing();
    // Each round groups the previous group with a new line: one level deeper.
    let mut inner = vec![2, 3];
    let mut next = 4;
    for _ in 1..MAX_DEPTH {
        document.apply(&[group(&inner)]).unwrap();
        let grouped = next;
        document
            .apply(&[add_graphic(line(0.0), None, None)])
            .unwrap();
        inner = vec![grouped, grouped + 1];
        next += 2;
    }
    // The first lines are now at depth MAX_DEPTH; one more level is refused.
    let error = refused(&mut document, &[group(&inner)]);
    assert_eq!(reason(&error), Some("depth_limit"));
    assert_eq!(Document::restore(document.to_parts()).unwrap(), document);
    // Emptied, their group s4 can go one level deeper, but nothing fits inside it there.
    document
        .apply(&[delete(&[2, 3], DeleteMode::Subtree), group(&[4, 5])])
        .unwrap();
    let error = refused(&mut document, &[add_graphic(line(0.0), Some(4), None)]);
    assert_eq!(reason(&error), Some("depth_limit"));
}

#[test]
fn a_document_holds_a_bounded_number_of_objects() {
    let mut document = empty();
    for _ in 0..2 {
        let batch: Vec<_> = (0..(MAX_OBJECTS - 1) / 2)
            .map(|_| add_graphic(line(0.0), None, None))
            .collect();
        document.apply(&batch).unwrap();
    }
    document
        .apply(&[add_graphic(line(0.0), None, None)])
        .unwrap();
    assert_eq!(document.scene_objects().len(), MAX_OBJECTS);
    let error = refused(&mut document, &[add_graphic(line(0.0), None, None)]);
    assert_eq!(reason(&error), Some("object_limit"));
    let error = refused(&mut document, &[group(&[1, 2])]);
    assert_eq!(reason(&error), Some("object_limit"));
}

#[test]
fn identifiers_are_never_reused_across_kinds_or_undo() {
    let mut document = drawing();
    document.apply(&[group(&[2, 3])]).unwrap();
    document.undo().unwrap();
    document.apply(&[group(&[2, 3])]).unwrap();
    assert_eq!(tree(&document), ["s1(-)", "s5(-)", "s2(s5)", "s3(s5)"]);
    document
        .apply(&[Command::AddInstance {
            source: None,
            offset: Vector { dx: 1.0, dy: 1.0 },
        }])
        .unwrap();
    assert_eq!(
        document.instances().collect::<Vec<_>>(),
        [object(1), object(6)]
    );
}

#[test]
fn every_scene_reason_code_is_distinct_and_stable() {
    let codes: Vec<_> = SceneError::ALL.iter().map(|error| error.code()).collect();
    assert_eq!(
        codes,
        [
            "object_unknown",
            "object_kind_mismatch",
            "group_mixed_parents",
            "depth_limit",
            "object_limit",
            "index_out_of_range",
            "last_instance",
            "delete_mode_missing",
            "selection_unknown",
            "reference_unknown",
            "reference_in_use",
            "reference_limit",
        ]
    );
}

#[test]
fn history_records_the_selection_before_and_after_each_change() {
    let mut document = drawing();
    document
        .apply_selecting(
            &[group(&[2, 3])],
            &TransactionSelection {
                before: Some(selection(&[2, 3], &[], &[])),
                after: Some(select(&[ObjectRef::Created(0)])),
            },
        )
        .unwrap();
    assert_eq!(*document.selection(), selection(&[4], &[], &[]));
    document.undo().unwrap();
    assert_eq!(*document.selection(), selection(&[2, 3], &[], &[]));
    document.redo().unwrap();
    assert_eq!(*document.selection(), selection(&[4], &[], &[]));
}

#[test]
fn without_a_selection_the_previous_one_stays_less_what_was_deleted() {
    let mut document = drawing();
    document
        .apply(&[Command::AddAtom {
            atom: Atom::from(Element::CARBON),
            positions: NewAtomPositions::Single(point(20.0, 0.0)),
            bond_to: Some(BondTo {
                atom: AtomRef::Id(atom(1)),
                order: BondOrder::Single,
            }),
        }])
        .unwrap();
    document
        .apply_selecting(
            &[Command::Rename {
                title: "Selected".into(),
            }],
            &TransactionSelection {
                before: None,
                after: Some(SelectionRequest {
                    objects: vec![ObjectRef::Id(object(2))],
                    atoms: vec![atom(2)],
                    bonds: vec![bond(1)],
                }),
            },
        )
        .unwrap();
    document
        .apply(&[Command::DeleteAtoms {
            atoms: vec![atom(2)],
        }])
        .unwrap();
    assert_eq!(*document.selection(), selection(&[2], &[], &[]));
    document.undo().unwrap();
    assert_eq!(
        *document.selection(),
        selection(&[2], &[2], &[1]),
        "undo restores what the deletion took from the selection"
    );
}

#[test]
fn a_selection_alone_changes_nothing_and_records_nothing() {
    let mut document = drawing();
    let before = document.clone();
    let outcome = document
        .apply_selecting(
            &[move_objects(&[2], 0.0, 0.0)],
            &TransactionSelection {
                before: Some(selection(&[3], &[], &[])),
                after: Some(select(&[ObjectRef::Id(object(2))])),
            },
        )
        .unwrap();
    assert_eq!(outcome, Outcome::Unchanged);
    assert_eq!(document, before);
}

#[test]
fn refused_selections_fail_the_whole_transaction() {
    let mut document = drawing();
    let unknown_before = TransactionSelection {
        before: Some(selection(&[9], &[], &[])),
        after: None,
    };
    let after = |objects: &[ObjectRef]| TransactionSelection {
        before: None,
        after: Some(select(objects)),
    };
    let commands = [
        Command::AddAtom {
            atom: Atom::from(Element::CARBON),
            positions: NewAtomPositions::Single(point(5.0, 5.0)),
            bond_to: None,
        },
        add_graphic(line(0.0), None, None),
        delete(&[3], DeleteMode::Subtree),
    ];
    for request in [
        unknown_before,
        // `$0` added an atom, not a scene object.
        after(&[ObjectRef::Created(0)]),
        // Out of range.
        after(&[ObjectRef::Created(3)]),
        // Deleted by the transaction itself.
        after(&[ObjectRef::Id(object(3))]),
    ] {
        let error = refused_selecting(&mut document, &commands, &request);
        assert_eq!(
            (error.index, reason(&error)),
            (None, Some("selection_unknown")),
            "{request:?}"
        );
    }
    let bonds = TransactionSelection {
        before: None,
        after: Some(SelectionRequest {
            bonds: vec![bond(1)],
            ..SelectionRequest::default()
        }),
    };
    let error = refused_selecting(&mut document, &commands, &bonds);
    assert_eq!(reason(&error), Some("selection_unknown"));
    document
        .apply_selecting(&commands, &after(&[ObjectRef::Created(1)]))
        .unwrap();
    assert_eq!(*document.selection(), selection(&[4], &[], &[]));
}

#[test]
fn restore_accepts_where_an_edit_or_an_undo_leaves_the_selection() {
    let mut document = drawing();
    let with = |before: &[u32], after: &[u32]| TransactionSelection {
        before: Some(selection(before, &[], &[])),
        after: Some(select(
            &after
                .iter()
                .map(|serial| ObjectRef::Id(object(*serial)))
                .collect::<Vec<_>>(),
        )),
    };
    document
        .apply_selecting(&[move_objects(&[2], 1.0, 0.0)], &with(&[], &[2]))
        .unwrap();
    // The next edit was made with a selection other than the one the last edit left.
    document
        .apply_selecting(&[move_objects(&[3], 1.0, 0.0)], &with(&[3], &[3]))
        .unwrap();
    document.undo().unwrap();
    assert_eq!(*document.selection(), selection(&[3], &[], &[]));
    assert_eq!(Document::restore(document.to_parts()).unwrap(), document);
    document.undo().unwrap();
    assert_eq!(Document::restore(document.to_parts()).unwrap(), document);
    document.redo().unwrap();
    assert_eq!(*document.selection(), selection(&[2], &[], &[]));
    assert_eq!(Document::restore(document.to_parts()).unwrap(), document);
}

#[test]
fn restore_refuses_a_selection_that_is_not_where_history_ends() {
    let mut document = drawing();
    document
        .apply_selecting(
            &[move_objects(&[2], 1.0, 0.0)],
            &TransactionSelection {
                before: Some(selection(&[3], &[], &[])),
                after: Some(select(&[ObjectRef::Id(object(2))])),
            },
        )
        .unwrap();
    let mut elsewhere = document.to_parts();
    elsewhere.selection = selection(&[3], &[], &[]);
    assert!(
        Document::restore(elsewhere).is_err(),
        "the undo entry's selection before is not where an edit leaves it"
    );
    let mut missing = document.to_parts();
    missing.selection = selection(&[2], &[1], &[]);
    assert!(Document::restore(missing).is_err());
    let mut unknown = document.to_parts();
    unknown.selection = selection(&[8], &[], &[]);
    assert!(Document::restore(unknown).is_err());

    // A stored entry whose selection names what its side does not hold.
    let mut stale = document.to_parts();
    stale.undo[1].selection.before = selection(&[7], &[], &[]);
    assert!(Document::restore(stale).is_err());

    // Without history any selection of what exists is accepted.
    let fresh = drawing();
    let copy = fresh
        .copy_as(DocumentId::parse("doc-2").unwrap(), "Copy")
        .unwrap();
    let mut parts = copy.to_parts();
    parts.selection = selection(&[2], &[1], &[]);
    let restored = Document::restore(parts).unwrap();
    assert_eq!(*restored.selection(), selection(&[2], &[1], &[]));
}

#[test]
fn restore_refuses_trees_commands_could_not_produce() {
    let mut document = drawing();
    document.apply(&[group(&[2, 3])]).unwrap();
    assert_eq!(Document::restore(document.to_parts()).unwrap(), document);

    let mut twice = document.to_parts();
    twice.roots.push(object(2));
    assert!(Document::restore(twice).is_err(), "s2 contained twice");

    let mut loose = document.to_parts();
    loose.roots.retain(|id| *id != object(4));
    assert!(Document::restore(loose).is_err(), "s4 on no page");

    let mut dangling = document.to_parts();
    dangling.roots.push(object(9));
    assert!(Document::restore(dangling).is_err(), "s9 does not exist");

    let mut cycle = document.to_parts();
    for (id, object_state) in &mut cycle.objects {
        if *id == object(4) {
            *object_state = SceneObject::Group {
                children: vec![object(2), object(3), object(4)],
            };
        }
    }
    assert!(Document::restore(cycle).is_err(), "s4 contains itself");
}

#[test]
fn restore_refuses_history_that_changes_an_object_kind_or_shape() {
    let mut document = drawing();
    document.apply(&[move_objects(&[2], 1.0, 0.0)]).unwrap();
    let mut shape = document.to_parts();
    let entry = shape.undo.last_mut().unwrap();
    let (_, change) = &mut entry.objects[0];
    if let Some(SceneObject::Graphic(graphic)) = &mut change.before {
        graphic.shape = GraphicShape::Rectangle;
    }
    assert!(
        Document::restore(shape).is_err(),
        "a rectangle became a line"
    );

    let mut kind = document.to_parts();
    let entry = kind.undo.last_mut().unwrap();
    entry.objects[0].1 = Change {
        before: Some(SceneObject::Instance),
        after: entry.objects[0].1.after.clone(),
    };
    assert!(
        Document::restore(kind).is_err(),
        "an instance became a graphic"
    );
}

#[test]
fn restore_refuses_reusing_an_object_id_after_deletion() {
    let mut document = empty();
    document
        .apply(&[add_graphic(line(0.0), None, None)])
        .unwrap();
    document
        .apply(&[delete(&[2], DeleteMode::Subtree)])
        .unwrap();
    let rectangle = Graphic {
        shape: GraphicShape::Rectangle,
        ..line(10.0)
    };
    document
        .apply(&[add_graphic(rectangle, None, None)])
        .unwrap();
    assert_eq!(Document::restore(document.to_parts()).unwrap(), document);

    let mut reused = document.to_parts();
    // Rewrite the third edit and its resulting content to reuse the deleted s2 in place of
    // the legitimately allocated s3. Each change still replays cleanly in both directions.
    reused
        .roots
        .iter_mut()
        .filter(|id| **id == object(3))
        .for_each(|id| *id = object(2));
    reused
        .objects
        .iter_mut()
        .filter(|(id, _)| *id == object(3))
        .for_each(|(id, _)| *id = object(2));
    let last = reused.undo.last_mut().unwrap();
    last.roots
        .as_mut()
        .unwrap()
        .after
        .iter_mut()
        .filter(|id| **id == object(3))
        .for_each(|id| *id = object(2));
    last.objects
        .iter_mut()
        .filter(|(id, _)| *id == object(3))
        .for_each(|(id, _)| *id = object(2));
    let mut oldest_trimmed = reused.clone();
    oldest_trimmed.undo.remove(0);
    assert!(
        Document::restore(reused).is_err(),
        "a deleted line's ID cannot identify a new rectangle"
    );
    assert!(
        Document::restore(oldest_trimmed).is_err(),
        "a trimmed creation does not make its deleted ID reusable"
    );

    document.undo().unwrap();
    let mut across_redo = document.to_parts();
    let next = across_redo.redo.last_mut().unwrap();
    next.roots
        .as_mut()
        .unwrap()
        .after
        .iter_mut()
        .filter(|id| **id == object(3))
        .for_each(|id| *id = object(2));
    next.objects
        .iter_mut()
        .filter(|(id, _)| *id == object(3))
        .for_each(|(id, _)| *id = object(2));
    assert!(
        Document::restore(across_redo).is_err(),
        "redo cannot create an object using an ID deleted by undo history"
    );
}

#[test]
fn trimming_every_undo_entry_leaves_the_selection_of_the_next_redo() {
    let mut document = drawing();
    let drawn = document.to_parts().undo;
    // Each edit was made with s3 selected and selects s2.
    for _ in 0..HISTORY_LIMIT {
        document
            .apply_selecting(
                &[move_objects(&[2], 1.0, 0.0)],
                &TransactionSelection {
                    before: Some(selection(&[3], &[], &[])),
                    after: Some(select(&[ObjectRef::Id(object(2))])),
                },
            )
            .unwrap();
    }
    while document.undo().is_ok() {}
    // Editing dropped the entry that drew the document; a looser writer would have kept it.
    let mut parts = document.to_parts();
    assert_eq!((parts.undo.len(), parts.redo.len()), (0, HISTORY_LIMIT));
    parts.undo = drawn;
    parts.selection = Selection::default();
    let restored = Document::restore(parts).unwrap();
    assert_eq!(
        (restored.undo_depth(), restored.redo_depth()),
        (0, HISTORY_LIMIT)
    );
    assert_eq!(
        *restored.selection(),
        selection(&[3], &[], &[]),
        "the state an undo of the next redo entry leaves"
    );
}
