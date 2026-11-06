//! Graphic forests preserve order, hierarchy and internal reference meaning without cloning graphs.

use std::collections::BTreeSet;

use chemifree_domain::document::{
    Atom, AtomId, Command, Document, DocumentId, EditError, NewAtomPositions, ObjectId, Point,
    Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{
    DeleteMode, Graphic, GraphicShape, ObjectRef, ReferenceTarget, SceneObject, SceneReference,
    Selection, TransactionSelection,
};
use chemifree_domain::scene_clipboard::{
    Fragment, FragmentObject, SceneFragmentError, copy_fragment,
};
use chemifree_domain::settings::Settings;

fn id(serial: u32) -> ObjectId {
    ObjectId::from_serial(serial).unwrap()
}
fn point(x: f64, y: f64) -> Point {
    Point::new(x, y).unwrap()
}
fn document(name: &str) -> Document {
    Document::new(DocumentId::parse(name).unwrap(), "Scene fragments").unwrap()
}
fn select(ids: &[u32]) -> Selection {
    Selection {
        objects: ids.iter().copied().map(id).collect(),
        ..Selection::default()
    }
}
fn graphic(shape: GraphicShape, x: f64) -> Command {
    Command::AddGraphic {
        graphic: Graphic {
            shape,
            points: [point(x, 0.0), point(x + 10.0, 10.0)],
        },
        parent: None,
        index: None,
    }
}
fn reference(source: u32, target: u32) -> SceneReference {
    SceneReference {
        source: id(source),
        target: ReferenceTarget::Object(id(target)),
    }
}
fn forest() -> Document {
    let mut source = document("forest");
    source
        .apply(&[
            graphic(GraphicShape::Line, 0.0),       // s2
            graphic(GraphicShape::Arrow, 20.0),     // s3
            graphic(GraphicShape::Rectangle, 40.0), // s4
            graphic(GraphicShape::Line, 60.0),      // s5
            graphic(GraphicShape::Arrow, 80.0),     // s6 outside
            Command::Group {
                objects: vec![id(3), id(4)],
            }, // s7
            Command::Group {
                objects: vec![id(2), id(7)],
            }, // s8
            Command::AddReference {
                reference: reference(8, 7),
            },
            Command::AddReference {
                reference: reference(7, 8),
            },
            Command::AddReference {
                reference: reference(2, 2),
            },
            Command::AddReference {
                reference: reference(6, 8),
            }, // incoming, not copied
        ])
        .unwrap();
    source
}

#[test]
fn copy_deduplicates_ancestors_and_preserves_paint_order_without_following_incoming_references() {
    let source = forest();
    let before = source.clone();
    let fragment = copy_fragment(&source, &select(&[5, 3, 7, 2, 8])).unwrap();
    assert_eq!(fragment.roots, vec![id(8), id(5)]);
    assert_eq!(
        fragment
            .objects
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        vec![id(8), id(2), id(7), id(3), id(4), id(5)]
    );
    assert_eq!(
        fragment.references.iter().copied().collect::<BTreeSet<_>>(),
        BTreeSet::from([reference(8, 7), reference(7, 8), reference(2, 2)])
    );
    assert_eq!(source, before);
}

#[test]
fn selected_roots_with_different_source_parents_follow_drawing_order_not_identifier_order() {
    let mut source = forest();
    source
        .apply(&[Command::RemoveReference {
            reference: reference(7, 8),
        }])
        .unwrap();
    let fragment = copy_fragment(&source, &select(&[5, 7, 2])).unwrap();
    assert_eq!(fragment.roots, vec![id(2), id(7), id(5)]);
    assert_eq!(
        fragment.ordered_ids().unwrap(),
        vec![id(2), id(7), id(3), id(4), id(5)]
    );
    assert_eq!(
        fragment.references,
        vec![reference(2, 2)],
        "the outside group owner's incoming reference is excluded"
    );
}

#[test]
fn paste_builds_graphics_then_bottom_up_groups_and_selects_only_new_roots() {
    let source = forest();
    let fragment = copy_fragment(&source, &select(&[8, 2, 5])).unwrap();
    let mut destination = document("destination");
    destination
        .apply(&[
            Command::AddAtom {
                atom: Atom::from(Element::from_symbol("C").unwrap()),
                positions: NewAtomPositions::Single(point(100.0, 100.0)),
                bond_to: None,
            },
            Command::AddInstance {
                source: Some(id(1)),
                offset: Vector { dx: 40.0, dy: 80.0 },
            },
            graphic(GraphicShape::Arrow, 200.0), // s3 existing
            Command::AddReference {
                reference: reference(3, 1),
            },
        ])
        .unwrap();
    let before = destination.clone();
    let plan = fragment
        .paste_commands(&destination, Vector { dx: 10.0, dy: 20.0 })
        .unwrap();
    assert_eq!(
        destination, before,
        "planning remains read-only even with multiple chemical placements"
    );
    assert_eq!(
        plan.objects
            .iter()
            .map(|entry| (entry.source, entry.target))
            .collect::<Vec<_>>(),
        vec![
            (id(8), id(9)),
            (id(2), id(4)),
            (id(7), id(8)),
            (id(3), id(5)),
            (id(4), id(6)),
            (id(5), id(7))
        ]
    );
    assert_eq!(
        plan.selection.objects,
        vec![ObjectRef::Id(id(9)), ObjectRef::Id(id(7))]
    );
    assert!(plan.selection.atoms.is_empty() && plan.selection.bonds.is_empty());
    assert_eq!(
        plan.commands.len(),
        fragment.objects.len() + fragment.references.len(),
        "there are no temporary dummy graphics or reorder commands"
    );
    destination
        .apply_selecting(
            &plan.commands,
            &TransactionSelection {
                before: None,
                after: Some(plan.selection),
            },
        )
        .unwrap();
    assert_eq!(destination.revision(), before.revision() + 1);
    assert_eq!(destination.undo_depth(), before.undo_depth() + 1);
    assert_eq!(destination.roots(), &[id(1), id(2), id(3), id(9), id(7)]);
    assert_eq!(
        destination.object(id(9)),
        Some(&SceneObject::Group {
            children: vec![id(4), id(8)]
        })
    );
    assert_eq!(
        destination.object(id(8)),
        Some(&SceneObject::Group {
            children: vec![id(5), id(6)]
        })
    );
    assert_eq!(
        destination.object(id(6)),
        Some(&SceneObject::Graphic(Graphic {
            shape: GraphicShape::Rectangle,
            points: [point(50.0, 20.0), point(60.0, 30.0)]
        }))
    );
    assert_eq!(
        destination.selection().objects,
        BTreeSet::from([id(7), id(9)])
    );
    assert_eq!(
        destination.references().collect::<BTreeSet<_>>(),
        BTreeSet::from([
            reference(3, 1),
            reference(9, 8),
            reference(8, 9),
            reference(4, 4)
        ])
    );
    let original = before.to_parts();
    let pasted = destination.to_parts();
    assert_eq!(
        (
            &pasted.atoms,
            &pasted.bonds,
            &pasted.positions,
            &pasted.stereo,
            &pasted.stereo_groups
        ),
        (
            &original.atoms,
            &original.bonds,
            &original.positions,
            &original.stereo,
            &original.stereo_groups
        )
    );
    destination.undo().unwrap();
    let undone = destination.to_parts();
    assert_eq!(
        (
            undone.objects,
            undone.roots,
            undone.references,
            undone.selection
        ),
        (
            original.objects,
            original.roots,
            original.references,
            original.selection
        )
    );
    destination.redo().unwrap();
    let redone = destination.to_parts();
    assert_eq!(
        (
            redone.objects,
            redone.roots,
            redone.references,
            redone.selection
        ),
        (
            pasted.objects,
            pasted.roots,
            pasted.references,
            pasted.selection
        )
    );
}

#[test]
fn copy_refuses_outgoing_external_and_graph_references_without_expanding_scope() {
    let mut source = forest();
    source
        .apply(&[Command::AddReference {
            reference: reference(5, 6),
        }])
        .unwrap();
    let before = source.clone();
    assert_eq!(
        copy_fragment(&source, &select(&[5])).unwrap_err(),
        SceneFragmentError::ReferenceUnsupported(reference(5, 6))
    );
    assert_eq!(source, before);
    let mut graph = document("graph-reference");
    graph
        .apply(&[
            Command::AddAtom {
                atom: Atom::from(Element::from_symbol("C").unwrap()),
                positions: NewAtomPositions::Single(point(0.0, 0.0)),
                bond_to: None,
            },
            graphic(GraphicShape::Line, 0.0),
            Command::AddReference {
                reference: SceneReference {
                    source: id(2),
                    target: ReferenceTarget::Atom(AtomId::from_serial(1).unwrap()),
                },
            },
        ])
        .unwrap();
    assert_eq!(
        copy_fragment(&graph, &select(&[2])).unwrap_err().as_str(),
        "clipboard_scene_reference_unsupported"
    );
}

#[test]
fn copy_refuses_instance_mixed_unknown_and_empty_selections() {
    let source = forest();
    assert_eq!(
        copy_fragment(&source, &select(&[1])).unwrap_err(),
        SceneFragmentError::InstanceUnsupported(id(1))
    );
    let mut containing = source.clone();
    containing
        .apply(&[Command::Group {
            objects: vec![id(1), id(5)],
        }])
        .unwrap();
    assert_eq!(
        copy_fragment(&containing, &select(&[9])).unwrap_err(),
        SceneFragmentError::InstanceUnsupported(id(1))
    );
    assert_eq!(
        copy_fragment(&source, &select(&[99])).unwrap_err(),
        SceneFragmentError::UnknownSelection(id(99))
    );
    assert_eq!(
        copy_fragment(&source, &Selection::default()).unwrap_err(),
        SceneFragmentError::EmptySelection
    );
    let mixed = Selection {
        atoms: BTreeSet::from([AtomId::from_serial(1).unwrap()]),
        ..select(&[2])
    };
    assert_eq!(
        copy_fragment(&source, &mixed).unwrap_err(),
        SceneFragmentError::GraphSelection
    );
}

#[test]
fn source_groups_that_became_empty_or_single_child_are_refused_honestly() {
    for removed in [vec![id(2)], vec![id(2), id(3)]] {
        let mut source = document("small-group");
        source
            .apply(&[
                graphic(GraphicShape::Line, 0.0),
                graphic(GraphicShape::Arrow, 20.0),
                Command::Group {
                    objects: vec![id(2), id(3)],
                },
                Command::DeleteObjects {
                    objects: removed,
                    mode: DeleteMode::Subtree,
                },
            ])
            .unwrap();
        assert_eq!(
            copy_fragment(&source, &select(&[4])).unwrap_err(),
            SceneFragmentError::GroupTooSmall(id(4))
        );
    }
}

#[test]
fn malformed_trees_duplicates_and_external_references_are_refused_before_planning() {
    let fragment = copy_fragment(&forest(), &select(&[8, 5])).unwrap();
    let mut duplicate = fragment.clone();
    duplicate.objects.push(duplicate.objects[0].clone());
    assert_eq!(duplicate.validate(), Err(SceneFragmentError::InvalidTree));
    let mut duplicate_root = fragment.clone();
    duplicate_root.roots.push(id(8));
    assert_eq!(
        duplicate_root.validate(),
        Err(SceneFragmentError::InvalidTree)
    );
    let mut cyclic = fragment.clone();
    cyclic.objects[0].object = SceneObject::Group {
        children: vec![id(8), id(7)],
    };
    assert_eq!(cyclic.validate(), Err(SceneFragmentError::InvalidTree));
    let mut missing_child = fragment.clone();
    missing_child.objects[0].object = SceneObject::Group {
        children: vec![id(2), id(99)],
    };
    assert_eq!(
        missing_child.validate(),
        Err(SceneFragmentError::InvalidTree)
    );
    let mut duplicate_reference = fragment.clone();
    duplicate_reference
        .references
        .push(duplicate_reference.references[0]);
    assert_eq!(
        duplicate_reference.validate(),
        Err(SceneFragmentError::DuplicateReference)
    );
    let mut external = fragment.clone();
    external.references.push(reference(5, 99));
    assert_eq!(
        external.validate(),
        Err(SceneFragmentError::ReferenceUnsupported(reference(5, 99)))
    );
    let mut loose = fragment.clone();
    loose.objects.push(FragmentObject {
        id: id(99),
        object: SceneObject::Graphic(Graphic {
            shape: GraphicShape::Line,
            points: [point(0.0, 0.0), point(1.0, 1.0)],
        }),
    });
    assert_eq!(loose.validate(), Err(SceneFragmentError::InvalidTree));
}

fn deep_fragment(groups: u32) -> Fragment {
    let mut objects = Vec::new();
    for serial in 1..=groups {
        let leaf = id(100 + serial);
        let next = if serial == groups {
            id(200)
        } else {
            id(serial + 1)
        };
        objects.push(FragmentObject {
            id: id(serial),
            object: SceneObject::Group {
                children: vec![leaf, next],
            },
        });
        objects.push(FragmentObject {
            id: leaf,
            object: SceneObject::Graphic(Graphic {
                shape: GraphicShape::Line,
                points: [point(0.0, 0.0), point(1.0, 1.0)],
            }),
        });
    }
    objects.push(FragmentObject {
        id: id(200),
        object: SceneObject::Graphic(Graphic {
            shape: GraphicShape::Line,
            points: [point(0.0, 0.0), point(1.0, 1.0)],
        }),
    });
    Fragment {
        settings: Settings::default(),
        roots: vec![id(1)],
        objects,
        references: Vec::new(),
    }
}

#[test]
fn forest_depth_limit_matches_the_scene_model_and_object_vector_order_is_not_semantic() {
    let mut fragment = deep_fragment(15);
    fragment.validate().unwrap();
    fragment
        .paste_commands(&document("deep-fits"), Vector { dx: 0.0, dy: 0.0 })
        .unwrap();
    let order = fragment.ordered_ids().unwrap();
    fragment.objects.reverse();
    assert_eq!(fragment.ordered_ids().unwrap(), order);
    assert_eq!(
        deep_fragment(16).validate(),
        Err(SceneFragmentError::DepthLimit)
    );
}

#[test]
fn style_coordinate_and_destination_capacity_failures_leave_everything_unchanged() {
    let fragment = copy_fragment(&forest(), &select(&[5])).unwrap();
    let destination = document("unchanged");
    let before = destination.clone();
    for offset in [
        Vector {
            dx: f64::NAN,
            dy: 0.0,
        },
        Vector {
            dx: 100_000.0,
            dy: 0.0,
        },
    ] {
        assert_eq!(
            fragment.paste_commands(&destination, offset).unwrap_err(),
            SceneFragmentError::Invalid(EditError::CoordinateOutOfRange)
        );
    }
    assert_eq!(destination, before);
    let mut mismatch = fragment.clone();
    mismatch.settings.line_width_pt = 2.0;
    assert_eq!(
        mismatch
            .paste_commands(&destination, Vector { dx: 0.0, dy: 0.0 })
            .unwrap_err(),
        SceneFragmentError::StyleMismatch
    );
    let mut full = document("object-capacity");
    full.apply(
        &(0..500)
            .map(|index| graphic(GraphicShape::Line, f64::from(index)))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    full.apply(
        &(0..499)
            .map(|index| graphic(GraphicShape::Line, f64::from(index)))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let before = full.clone();
    assert_eq!(
        fragment
            .paste_commands(&full, Vector { dx: 0.0, dy: 0.0 })
            .unwrap_err(),
        SceneFragmentError::Invalid(EditError::ObjectLimit)
    );
    assert_eq!(full, before);
}

#[test]
fn actual_objects_plus_references_obey_the_single_transaction_command_limit() {
    let objects: Vec<_> = (1..=250)
        .map(|serial| FragmentObject {
            id: id(serial),
            object: SceneObject::Graphic(Graphic {
                shape: GraphicShape::Line,
                points: [point(0.0, 0.0), point(1.0, 1.0)],
            }),
        })
        .collect();
    let mut fragment = Fragment {
        settings: Settings::default(),
        roots: (1..=250).map(id).collect(),
        references: (1..=250).map(|serial| reference(serial, serial)).collect(),
        objects,
    };
    let destination = document("command-capacity");
    assert_eq!(
        fragment
            .paste_commands(&destination, Vector { dx: 0.0, dy: 0.0 })
            .unwrap()
            .commands
            .len(),
        500
    );
    fragment.references.push(reference(1, 2));
    assert_eq!(fragment.validate(), Err(SceneFragmentError::CommandLimit));
}
