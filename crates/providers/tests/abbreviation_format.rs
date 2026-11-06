//! v9 Et displays and history/drafts; v8 caption documents migrate without data loss or writes.

use std::fs;

use chemifree_application::{Application, DocumentRepository, Draft, StorageError};
use chemifree_domain::document::{
    Atom, AtomRef, BondOrder, BondTo, Command, Document, DocumentId, NewAtomPositions, ObjectId,
    Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{Caption, CaptionRun, CaptionScript};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

fn drawing() -> Document {
    let mut doc = Document::new(DocumentId::parse("ethyl-native").unwrap(), "Et").unwrap();
    doc.apply(&[
        Command::AddAtom {
            atom: Atom::from(Element::from_symbol("O").unwrap()),
            positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
            bond_to: None,
        },
        Command::AddAtom {
            atom: Atom::from(Element::from_symbol("C").unwrap()),
            positions: NewAtomPositions::Single(Point::new(30.0, 0.0).unwrap()),
            bond_to: Some(BondTo {
                atom: AtomRef::Created(0),
                order: BondOrder::Single,
            }),
        },
        Command::AddAtom {
            atom: Atom::from(Element::from_symbol("C").unwrap()),
            positions: NewAtomPositions::Single(Point::new(45.0, 25.0).unwrap()),
            bond_to: Some(BondTo {
                atom: AtomRef::Created(1),
                order: BondOrder::Single,
            }),
        },
        Command::AddCaption {
            caption: Caption {
                anchor: Point::new(10.0, 50.0).unwrap(),
                runs: vec![CaptionRun {
                    text: "Et 원문 e\u{301}".into(),
                    script: CaptionScript::Baseline,
                }],
            },
            parent: None,
            index: None,
        },
    ])
    .unwrap();
    doc
}
fn collapse() -> Command {
    Command::CollapseEthyl {
        instance: ObjectId::from_serial(1).unwrap(),
        anchor: chemifree_domain::document::AtomId::from_serial(2).unwrap(),
        terminal: chemifree_domain::document::AtomId::from_serial(3).unwrap(),
    }
}
fn v8(mut value: Value) -> Value {
    value["format_version"] = json!(8);
    value["document"]
        .as_object_mut()
        .unwrap()
        .remove("abbreviations");
    for stack in ["undo", "redo"] {
        for entry in value["history"][stack].as_array_mut().unwrap() {
            entry.as_object_mut().unwrap().remove("abbreviations");
        }
    }
    value
}

#[test]
fn v9_document_and_draft_round_trip_display_undo_redo_and_unicode_caption() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = drawing();
    doc.apply(&[collapse()]).unwrap();
    doc.apply(&[Command::ExpandEthyl {
        instance: ObjectId::from_serial(1).unwrap(),
        anchor: chemifree_domain::document::AtomId::from_serial(2).unwrap(),
    }])
    .unwrap();
    doc.undo().unwrap();
    repo.save(&doc, None).unwrap();
    assert_eq!(repo.load(doc.id()).unwrap(), Some(doc.clone()));
    let value: Value =
        serde_json::from_slice(&fs::read(root.path().join("documents/ethyl-native.json")).unwrap())
            .unwrap();
    assert_eq!(value["format_version"], 9);
    assert_eq!(
        value["document"]["abbreviations"],
        json!([{"instance":"s1","anchor":"a2","terminal":"a3"}])
    );
    assert_eq!(
        value["history"]["redo"][0]["abbreviations"][0]["before"],
        "a3"
    );
    let draft = Draft {
        document: doc.clone(),
        base_revision: doc.revision(),
        saved_at_ms: 123,
    };
    repo.write_draft(&draft).unwrap();
    repo.release_draft(doc.id());
    assert_eq!(repo.claim_draft(doc.id()), Ok(Some(Ok(draft))));
    repo.release_draft(doc.id());
    let mut reopened = repo.load(doc.id()).unwrap().unwrap();
    reopened.redo().unwrap();
    assert_eq!(reopened.abbreviations().count(), 0);
    reopened.undo().unwrap();
    assert_eq!(
        reopened.to_parts().abbreviations,
        doc.to_parts().abbreviations
    );
}

#[test]
fn v8_read_keeps_exact_bytes_caption_and_history_then_archives_the_original_on_v9_save() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let doc = drawing();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/ethyl-native.json");
    let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let original = serde_json::to_vec_pretty(&v8(value)).unwrap();
    fs::write(&path, &original).unwrap();
    let mut loaded = repo.load(doc.id()).unwrap().unwrap();
    assert_eq!(loaded, doc);
    assert_eq!(fs::read(&path).unwrap(), original);
    loaded.apply(&[collapse()]).unwrap();
    repo.save(&loaded, Some(doc.revision())).unwrap();
    assert_eq!(fs::read(repo.migrated_path(doc.id(), 8)).unwrap(), original);
    assert_eq!(repo.load(doc.id()).unwrap(), Some(loaded));
}

#[test]
fn current_fields_are_required_old_headers_cannot_smuggle_history_and_invalid_members_refuse() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = drawing();
    doc.apply(&[collapse()]).unwrap();
    doc.undo().unwrap();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/ethyl-native.json");
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let mut missing = original.clone();
    missing["history"]["redo"][0]
        .as_object_mut()
        .unwrap()
        .remove("abbreviations");
    fs::write(&path, serde_json::to_vec(&missing).unwrap()).unwrap();
    assert!(
        matches!(repo.load(doc.id()), Err(StorageError::Corrupt(message)) if message.contains("required in v9"))
    );
    let mut old = v8(original.clone());
    old["history"]["redo"][0]["abbreviations"] =
        original["history"]["redo"][0]["abbreviations"].clone();
    fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
    assert!(
        matches!(repo.load(doc.id()), Err(StorageError::Corrupt(message)) if message.contains("forbidden in v1–8"))
    );
    let mut invalid = original;
    invalid["history"]["redo"][0]["abbreviations"][0]["after"] = json!("a1");
    fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
    assert!(
        matches!(repo.load(doc.id()), Err(StorageError::Corrupt(_))),
        "a forged Et cannot hide oxygen even only in redo history"
    );
}

#[test]
fn v8_drafts_migrate_with_caption_and_keep_original_bytes_before_recovery() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = drawing();
    repo.save(&doc, None).unwrap();
    let base_revision = doc.revision();
    doc.apply(&[Command::Rename {
        title: "Recovered Et caption".into(),
    }])
    .unwrap();
    let draft = Draft {
        document: doc.clone(),
        base_revision,
        saved_at_ms: 125,
    };
    repo.write_draft(&draft).unwrap();
    repo.release_draft(doc.id());
    let path = root.path().join("drafts/ethyl-native.json");
    let stored: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let original = serde_json::to_vec_pretty(&v8(stored)).unwrap();
    fs::write(&path, &original).unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    app.document("ethyl-native").unwrap();
    assert_eq!(fs::read(&path).unwrap(), original);
    let recovered = app
        .recover("ethyl-native", Some("recover-v8-ethyl"))
        .unwrap();
    assert_eq!(recovered.document.title, "Recovered Et caption");
    assert_eq!(recovered.document.abbreviations.len(), 0);
    assert_eq!(
        fs::read(repo.migrated_draft_path(doc.id(), 8)).unwrap(),
        original
    );
    app.autosave("ethyl-native", Some("rewrite-v9-ethyl"))
        .unwrap();
    let rewritten: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(rewritten["format_version"], 9);
}
