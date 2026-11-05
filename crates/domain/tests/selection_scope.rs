//! Shared selection resolution and union movement: placements, containment, atomicity/history.

use std::collections::BTreeSet;

use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondId, BondOrder, Command, Document, DocumentId, EditError,
    MAX_COORDINATE, NewAtomPositions, ObjectId, Outcome, Point, PositionKey, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{
    Graphic, GraphicShape, ObjectRef, ReferenceTarget, SceneObject, SceneReference, Selection,
    SelectionRequest, SelectionScope, TransactionSelection,
};

fn atom(serial: u32) -> AtomId {
    AtomId::from_serial(serial).unwrap()
}
fn bond(serial: u32) -> BondId {
    BondId::from_serial(serial).unwrap()
}
fn object(serial: u32) -> ObjectId {
    ObjectId::from_serial(serial).unwrap()
}
fn point(x: f64, y: f64) -> Point {
    Point::new(x, y).unwrap()
}
fn selection(objects: &[u32], atoms: &[u32], bonds: &[u32]) -> Selection {
    Selection {
        objects: objects.iter().copied().map(object).collect(),
        atoms: atoms.iter().copied().map(atom).collect(),
        bonds: bonds.iter().copied().map(bond).collect(),
    }
}
fn movement(selection: Selection, instance: Option<u32>, dx: f64, dy: f64) -> Command {
    Command::MoveSelection {
        selection,
        instance: instance.map(object),
        delta: Vector { dx, dy },
    }
}
fn graphic(x: f64) -> Graphic {
    Graphic {
        shape: GraphicShape::Line,
        points: [point(x, 0.0), point(x + 5.0, 0.0)],
    }
}

/// s6 contains s5(s1 instance, s2 line) and s3 line. s4 is another placement on
/// the page. The graph is a1–a2–a3 plus unbonded a4; s2 references b2 without owning it.
fn drawing() -> Document {
    let mut document = Document::new(
        DocumentId::parse("selection-scope").unwrap(),
        "Union movement",
    )
    .unwrap();
    let mut commands = Vec::new();
    for x in [0.0, 10.0, 20.0, 30.0] {
        commands.push(Command::AddAtom {
            atom: Atom::from(Element::CARBON),
            positions: NewAtomPositions::Single(point(x, 0.0)),
            bond_to: None,
        });
    }
    for (begin, end) in [(0, 1), (1, 2)] {
        commands.push(Command::AddBond {
            begin: AtomRef::Created(begin),
            end: AtomRef::Created(end),
            order: BondOrder::Single,
        });
    }
    for x in [40.0, 80.0] {
        commands.push(Command::AddGraphic {
            graphic: graphic(x),
            parent: None,
            index: None,
        });
    }
    commands.extend([
        Command::AddInstance {
            source: Some(object(1)),
            offset: Vector { dx: 100.0, dy: 0.0 },
        },
        Command::Group {
            objects: vec![object(1), object(2)],
        },
        Command::Group {
            objects: vec![object(5), object(3)],
        },
        Command::AddReference {
            reference: SceneReference {
                source: object(2),
                target: ReferenceTarget::Bond(bond(2)),
            },
        },
    ]);
    document.apply(&commands).unwrap();
    document
}

type GeometrySnapshot = (Vec<(PositionKey, Point)>, Vec<(ObjectId, Graphic)>);

fn geometry(document: &Document) -> GeometrySnapshot {
    let mut positions = Vec::new();
    let mut graphics = Vec::new();
    for instance in document.instances() {
        positions.extend(
            document
                .positions(instance)
                .map(|(atom, position)| (PositionKey { instance, atom }, position)),
        );
    }
    for (id, _, value) in document.scene_objects() {
        if let SceneObject::Graphic(graphic) = value {
            graphics.push((id, graphic.clone()));
        }
    }
    (positions, graphics)
}

#[test]
fn scope_unions_nested_objects_atoms_and_bond_endpoints_in_one_placement() {
    let document = drawing();
    let before = document.clone();
    let scope = document
        .selection_scope(&selection(&[6, 5, 2], &[1], &[1]), Some(object(1)))
        .unwrap();
    assert_eq!(
        scope,
        SelectionScope {
            objects: [1, 2, 3, 5, 6].into_iter().map(object).collect(),
            positions: (1..=4)
                .map(|serial| PositionKey {
                    instance: object(1),
                    atom: atom(serial)
                })
                .collect(),
            graphics: [2, 3].into_iter().map(object).collect(),
        }
    );
    assert_eq!(document, before, "scope resolution is read-only");
    let explicit = document
        .selection_scope(&selection(&[1], &[1], &[2]), Some(object(4)))
        .unwrap();
    let expected: BTreeSet<PositionKey> = (1..=4)
        .map(|serial| PositionKey {
            instance: object(1),
            atom: atom(serial),
        })
        .chain((1..=3).map(|serial| PositionKey {
            instance: object(4),
            atom: atom(serial),
        }))
        .collect();
    assert_eq!(
        explicit.positions, expected,
        "explicit graph targets affect only their specified placement"
    );
    assert_eq!(explicit.objects, BTreeSet::from([object(1)]));
}

#[test]
fn selected_bonds_move_both_endpoints_once_and_do_not_follow_scene_references() {
    let mut document = drawing();
    let before = geometry(&document);
    let selected = selection(&[2], &[2], &[1, 2]);
    let scope = document
        .selection_scope(&selected, Some(object(4)))
        .unwrap();
    assert_eq!(
        scope.positions,
        (1..=3)
            .map(|serial| PositionKey {
                instance: object(4),
                atom: atom(serial)
            })
            .collect()
    );
    document
        .apply(&[movement(selected, Some(4), 3.0, -2.0)])
        .unwrap();
    for serial in 1..=3 {
        assert_eq!(
            document.position(object(4), atom(serial)),
            Some(point(100.0 + 10.0 * f64::from(serial - 1) + 3.0, -2.0))
        );
    }
    assert_eq!(
        document.position(object(4), atom(4)),
        Some(point(130.0, 0.0))
    );
    for serial in 1..=4 {
        assert_eq!(
            document.position(object(1), atom(serial)),
            Some(point(10.0 * f64::from(serial - 1), 0.0))
        );
    }
    assert_eq!(
        document.object(object(2)),
        Some(&SceneObject::Graphic(Graphic {
            shape: GraphicShape::Line,
            points: [point(43.0, -2.0), point(48.0, -2.0)]
        }))
    );
    assert_eq!(
        document.object(object(3)),
        Some(&SceneObject::Graphic(before.1[1].1.clone()))
    );
    assert_eq!(
        document.references().collect::<Vec<_>>(),
        vec![SceneReference {
            source: object(2),
            target: ReferenceTarget::Bond(bond(2))
        }]
    );
}

#[test]
fn overlapping_parent_child_and_graph_targets_move_every_geometry_once() {
    let mut document = drawing();
    let before = geometry(&document);
    let atoms_before: Vec<_> = document
        .atoms()
        .map(|(id, atom)| (id, atom.clone()))
        .collect();
    let bonds_before: Vec<_> = document
        .bonds()
        .map(|(id, bond)| (id, bond.clone()))
        .collect();
    let selected = selection(&[6, 5, 1, 2, 3], &[1, 2], &[1, 2]);
    document
        .apply(&[movement(selected, Some(1), 7.0, 4.0)])
        .unwrap();
    for serial in 1..=4 {
        assert_eq!(
            document.position(object(1), atom(serial)),
            Some(point(10.0 * f64::from(serial - 1) + 7.0, 4.0))
        );
        assert_eq!(
            document.position(object(4), atom(serial)),
            Some(point(100.0 + 10.0 * f64::from(serial - 1), 0.0))
        );
    }
    assert_eq!(
        document
            .atoms()
            .map(|(id, atom)| (id, atom.clone()))
            .collect::<Vec<_>>(),
        atoms_before
    );
    assert_eq!(
        document
            .bonds()
            .map(|(id, bond)| (id, bond.clone()))
            .collect::<Vec<_>>(),
        bonds_before
    );
    for (id, graphic) in before.1 {
        let expected = Graphic {
            shape: graphic.shape,
            points: graphic
                .points
                .map(|position| point(position.x() + 7.0, position.y() + 4.0)),
        };
        assert_eq!(document.object(id), Some(&SceneObject::Graphic(expected)));
    }
}

#[test]
fn graph_placement_is_required_only_for_graph_targets_and_is_always_validated_when_stated() {
    let document = drawing();
    assert_eq!(
        document.selection_scope(&selection(&[], &[1], &[]), None),
        Err(EditError::InstanceRequired { instances: 2 })
    );
    assert_eq!(
        document.selection_scope(&selection(&[], &[], &[1]), None),
        Err(EditError::InstanceRequired { instances: 2 })
    );
    assert_eq!(
        document
            .selection_scope(&selection(&[2], &[], &[]), None)
            .unwrap()
            .graphics,
        BTreeSet::from([object(2)])
    );
    assert_eq!(
        document.selection_scope(&Selection::default(), Some(object(99))),
        Err(EditError::UnknownObject(object(99)))
    );
    assert!(
        matches!(document.selection_scope(&Selection::default(),Some(object(2))),Err(EditError::ObjectKindMismatch {object:id,..}) if id==object(2))
    );
    let mut single = Document::new(DocumentId::parse("single-scope").unwrap(), "Single").unwrap();
    single
        .apply(&[Command::AddAtom {
            atom: Atom::from(Element::CARBON),
            positions: NewAtomPositions::Single(point(1.0, 2.0)),
            bond_to: None,
        }])
        .unwrap();
    assert_eq!(
        single
            .selection_scope(&selection(&[], &[1], &[]), None)
            .unwrap()
            .positions,
        BTreeSet::from([PositionKey {
            instance: object(1),
            atom: atom(1)
        }])
    );
}

#[test]
fn unknown_selection_references_abort_earlier_commands_and_preserve_all_state() {
    for selected in [
        selection(&[999], &[], &[]),
        selection(&[], &[999], &[]),
        selection(&[], &[], &[999]),
    ] {
        let mut document = drawing();
        let before = document.clone();
        let failure = document
            .apply(&[
                Command::Rename {
                    title: "Must roll back".into(),
                },
                movement(selected.clone(), Some(1), 1.0, 1.0),
            ])
            .unwrap_err();
        assert_eq!(failure.index, Some(1));
        assert!(
            matches!(failure.error, EditError::SelectionUnknown(_)),
            "{selected:?}: {failure}"
        );
        assert_eq!(
            document, before,
            "atomic refusal includes history, selection and identifier counters"
        );
    }
}

#[test]
fn invalid_final_graphic_and_nonfinite_deltas_abort_all_movement() {
    let mut document = drawing();
    document
        .apply(&[Command::AddGraphic {
            graphic: Graphic {
                shape: GraphicShape::Line,
                points: [point(MAX_COORDINATE - 1.0, 0.0), point(MAX_COORDINATE, 0.0)],
            },
            parent: None,
            index: None,
        }])
        .unwrap();
    let before = document.clone();
    let failure = document
        .apply(&[
            Command::Rename {
                title: "Must roll back".into(),
            },
            movement(selection(&[6, 7], &[1], &[1]), Some(1), 1.0, 0.0),
        ])
        .unwrap_err();
    assert_eq!(
        (failure.index, failure.error),
        (Some(1), EditError::CoordinateOutOfRange)
    );
    assert_eq!(
        document, before,
        "a final graphic failure rolls back already translated atoms/graphics"
    );
    for delta in [
        Vector {
            dx: f64::NAN,
            dy: 0.0,
        },
        Vector {
            dx: 0.0,
            dy: f64::INFINITY,
        },
    ] {
        let failure = document
            .apply(&[Command::MoveSelection {
                selection: Selection::default(),
                instance: None,
                delta,
            }])
            .unwrap_err();
        assert_eq!(failure.error, EditError::CoordinateOutOfRange);
        assert_eq!(document, before);
    }
}

#[test]
fn one_union_move_is_one_undo_entry_and_restores_transaction_selections() {
    let mut document = drawing();
    let original_geometry = geometry(&document);
    let before_selection = selection(&[6], &[1], &[1]);
    let after_selection = selection(&[2], &[3], &[]);
    let before_depth = document.undo_depth();
    let revision = document.revision();
    let applied = document
        .apply_selecting(
            &[movement(before_selection.clone(), Some(1), 5.0, 2.0)],
            &TransactionSelection {
                before: Some(before_selection.clone()),
                after: Some(SelectionRequest {
                    objects: vec![ObjectRef::Id(object(2))],
                    atoms: vec![atom(3)],
                    bonds: Vec::new(),
                }),
            },
        )
        .unwrap();
    assert_eq!(applied, Outcome::Changed);
    assert_eq!(document.revision(), revision + 1);
    assert_eq!(document.undo_depth(), before_depth + 1);
    assert_eq!(document.selection(), &after_selection);
    let moved_geometry = geometry(&document);
    document.undo().unwrap();
    assert_eq!(geometry(&document), original_geometry);
    assert_eq!(document.selection(), &before_selection);
    document.redo().unwrap();
    assert_eq!(geometry(&document), moved_geometry);
    assert_eq!(document.selection(), &after_selection);
}

#[test]
fn zero_delta_and_empty_scope_leave_revision_redo_history_and_selection_untouched() {
    let mut document = drawing();
    document
        .apply(&[movement(selection(&[6], &[], &[]), None, 1.0, 1.0)])
        .unwrap();
    document.undo().unwrap();
    let before = document.clone();
    for command in [
        movement(selection(&[6], &[1], &[1]), Some(1), 0.0, 0.0),
        movement(Selection::default(), None, 10.0, 20.0),
    ] {
        assert_eq!(
            document
                .apply_selecting(
                    &[command],
                    &TransactionSelection {
                        before: Some(selection(&[2], &[], &[])),
                        after: Some(SelectionRequest {
                            objects: vec![ObjectRef::Id(object(3))],
                            ..SelectionRequest::default()
                        })
                    }
                )
                .unwrap(),
            Outcome::Unchanged
        );
        assert_eq!(
            document, before,
            "no-op leaves the complete document and redo branch unchanged"
        );
    }
}
