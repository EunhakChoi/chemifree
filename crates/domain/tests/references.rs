//! References are separate from containment and participate in atomic history.

use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondId, BondOrder, BondTo, Command, Document, DocumentId, EditError,
    NewAtomPositions, ObjectId, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{
    DeleteMode, Graphic, GraphicShape, ReferenceTarget, SceneReference, Selection,
    TransactionSelection,
};

fn object(id: u32) -> ObjectId {
    ObjectId::from_serial(id).unwrap()
}
fn atom(id: u32) -> AtomId {
    AtomId::from_serial(id).unwrap()
}
fn bond(id: u32) -> BondId {
    BondId::from_serial(id).unwrap()
}
fn reference(source: u32, target: ReferenceTarget) -> SceneReference {
    SceneReference {
        source: object(source),
        target,
    }
}
fn graphic(x: f64) -> Command {
    Command::AddGraphic {
        graphic: Graphic {
            shape: GraphicShape::Line,
            points: [
                Point::new(x, 0.0).unwrap(),
                Point::new(x + 5.0, 0.0).unwrap(),
            ],
        },
        parent: None,
        index: None,
    }
}
fn drawing() -> Document {
    let mut document = Document::new(DocumentId::parse("doc-refs").unwrap(), "References").unwrap();
    document
        .apply(&[
            Command::AddAtom {
                atom: Atom::from(Element::CARBON),
                positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
                bond_to: None,
            },
            Command::AddAtom {
                atom: Atom::from(Element::CARBON),
                positions: NewAtomPositions::Single(Point::new(20.0, 0.0).unwrap()),
                bond_to: Some(BondTo {
                    atom: AtomRef::Created(0),
                    order: BondOrder::Single,
                }),
            },
            graphic(0.0),
            graphic(10.0),
        ])
        .unwrap();
    document
}

#[test]
fn referenced_targets_cannot_be_deleted_until_links_are_removed_in_the_same_transaction() {
    let mut document = drawing();
    let to_atom = reference(2, ReferenceTarget::Atom(atom(1)));
    let to_bond = reference(2, ReferenceTarget::Bond(bond(1)));
    let to_object = reference(2, ReferenceTarget::Object(object(3)));
    document
        .apply(&[
            Command::AddReference { reference: to_atom },
            Command::AddReference { reference: to_bond },
            Command::AddReference {
                reference: to_object,
            },
        ])
        .unwrap();
    let original = document.clone();
    let error = document
        .apply(&[Command::DeleteAtoms {
            atoms: vec![atom(1)],
        }])
        .unwrap_err();
    assert!(matches!(error.error, EditError::ReferenceInUse(_)));
    assert_eq!(error.error.reason(), Some("reference_in_use"));
    assert_eq!(document, original);
    let error = document
        .apply(&[Command::DeleteObjects {
            objects: vec![object(3)],
            mode: DeleteMode::Subtree,
        }])
        .unwrap_err();
    assert!(matches!(error.error, EditError::ReferenceInUse(_)));
    assert_eq!(document, original);

    let selected = Selection {
        objects: [object(2)].into(),
        atoms: [atom(1)].into(),
        bonds: [bond(1)].into(),
    };
    document
        .apply_selecting(
            &[
                Command::RemoveReference { reference: to_atom },
                Command::RemoveReference { reference: to_bond },
                Command::DeleteAtoms {
                    atoms: vec![atom(1)],
                },
            ],
            &TransactionSelection {
                before: Some(selected.clone()),
                after: None,
            },
        )
        .unwrap();
    assert_eq!(document.references().collect::<Vec<_>>(), [to_object]);
    assert_eq!(document.atoms().count(), 1);
    assert_eq!(document.bonds().count(), 0);
    document.undo().unwrap();
    assert_eq!(
        document.references().collect::<Vec<_>>(),
        original.references().collect::<Vec<_>>()
    );
    assert_eq!(document.atoms().count(), 2);
    assert_eq!(document.bonds().count(), 1);
    assert_eq!(document.selection(), &selected);
    document.redo().unwrap();
    assert_eq!(document.references().collect::<Vec<_>>(), [to_object]);
    assert_eq!(document.atoms().count(), 1);
}

#[test]
fn deleting_a_subtree_removes_its_owned_links_but_refuses_surviving_owners() {
    let mut document = drawing();
    document
        .apply(&[Command::Group {
            objects: vec![object(2), object(3)],
        }])
        .unwrap();
    let internal = reference(2, ReferenceTarget::Object(object(3)));
    let external = reference(1, ReferenceTarget::Object(object(3)));
    document
        .apply(&[
            Command::AddReference {
                reference: internal,
            },
            Command::AddReference {
                reference: external,
            },
        ])
        .unwrap();
    let before = document.clone();
    assert!(matches!(
        document
            .apply(&[Command::DeleteObjects {
                objects: vec![object(4)],
                mode: DeleteMode::Subtree
            }])
            .unwrap_err()
            .error,
        EditError::ReferenceInUse(_)
    ));
    assert_eq!(document, before);
    document
        .apply(&[
            Command::RemoveReference {
                reference: external,
            },
            Command::DeleteObjects {
                objects: vec![object(4)],
                mode: DeleteMode::Subtree,
            },
        ])
        .unwrap();
    assert_eq!(document.references().count(), 0);
    document.undo().unwrap();
    assert_eq!(
        document.references().collect::<Vec<_>>(),
        before.references().collect::<Vec<_>>()
    );
    assert_eq!(document.object(object(3)), before.object(object(3)));
    assert_eq!(Document::restore(document.to_parts()).unwrap(), document);
}

#[test]
fn restore_rejects_dangling_duplicate_and_impossible_reference_history() {
    let mut document = drawing();
    let link = reference(2, ReferenceTarget::Object(object(3)));
    document
        .apply(&[Command::AddReference { reference: link }])
        .unwrap();
    let mut dangling = document.to_parts();
    dangling
        .references
        .push(reference(2, ReferenceTarget::Object(object(99))));
    assert!(Document::restore(dangling).is_err());
    let mut duplicate = document.to_parts();
    duplicate.references.push(link);
    assert!(Document::restore(duplicate).is_err());
    let mut malformed = document.to_parts();
    malformed.undo.last_mut().unwrap().references[0].1.before = true;
    assert!(Document::restore(malformed).is_err());
}
