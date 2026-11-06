//! v9 Et displays and history/drafts; v8 caption documents migrate without data loss or writes.

use std::fs;

use chemifree_application::{Application, DocumentRepository, Draft, StorageError};
use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondOrder, BondTo, Command, Document, DocumentId, NewAtomPositions,
    ObjectId, Point,
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
                alignment: Default::default(),
                anchor: Point::new(10.0, 50.0).unwrap(),
                runs: vec![CaptionRun {
                    shadow: false,
                    outline: false,
                    underline: false,
                    color: None,
                    size_pt: None,
                    font: None,
                    text: "Et 원문 e\u{301}".into(),
                    script: CaptionScript::Baseline,
                    weight: Default::default(),
                    style: Default::default(),
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
    strip_caption_styles(&mut value);
    strip_label_weights(&mut value);
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
    assert_eq!(value["format_version"], 19);
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
    // A recovered complete draft needs no rewrite until there is another edit.
    app.autosave("ethyl-native", Some("unchanged-v8-ethyl"))
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), original);
    let command: chemifree_application::CommandDto = serde_json::from_value(json!({
        "op":"collapse_ethyl", "instance":"s1", "anchor":"a2", "terminal":"a3"
    }))
    .unwrap();
    let edited = app
        .apply_transaction(
            "ethyl-native",
            Some("collapse-after-recovery"),
            recovered.document.revision,
            vec![command],
        )
        .unwrap();
    app.autosave("ethyl-native", Some("rewrite-v9-ethyl"))
        .unwrap();
    let rewritten: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(rewritten["format_version"], 19);
    assert_eq!(rewritten["document"]["revision"], edited.document.revision);
    assert_eq!(
        fs::read(repo.migrated_draft_path(doc.id(), 8)).unwrap(),
        original
    );
    drop(app);
    let mut restored = repo
        .claim_draft(doc.id())
        .unwrap()
        .unwrap()
        .unwrap()
        .document;
    assert_eq!(restored.abbreviations().count(), 1);
    restored.undo().unwrap();
    assert_eq!(restored.abbreviations().count(), 0);
    assert_eq!(restored.title(), "Recovered Et caption");
    restored.undo().unwrap();
    assert_eq!(restored.title(), "Et");
    restored.redo().unwrap();
    restored.redo().unwrap();
    assert_eq!(restored.abbreviations().count(), 1);
}

#[test]
fn caption_persistence_reserves_current_and_both_history_display_records() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = drawing();
    let mut commands = Vec::new();
    for _ in 1..100 {
        let first = commands.len();
        for offset in 0..3 {
            commands.push(Command::AddAtom {
                atom: Atom::from(
                    Element::from_symbol(if offset == 0 { "O" } else { "C" }).unwrap(),
                ),
                positions: NewAtomPositions::Single(Point::new(offset as f64 * 30.0, 0.0).unwrap()),
                bond_to: (offset > 0).then(|| BondTo {
                    atom: AtomRef::Created(first + offset - 1),
                    order: BondOrder::Single,
                }),
            });
        }
    }
    doc.apply(&commands).unwrap();
    let without_history = |document: &Document| {
        let mut parts = document.to_parts();
        parts.undo.clear();
        parts.redo.clear();
        Document::restore(parts).unwrap()
    };
    let before_weight = without_history(&doc).persistence_weight();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/ethyl-native.json");
    let before: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let before_revision = doc.revision();
    let collapses: Vec<_> = (0..100)
        .map(|fragment| Command::CollapseEthyl {
            instance: ObjectId::from_serial(1).unwrap(),
            anchor: AtomId::from_serial(fragment * 3 + 2).unwrap(),
            terminal: AtomId::from_serial(fragment * 3 + 3).unwrap(),
        })
        .collect();
    doc.apply(&collapses).unwrap();
    repo.save(&doc, Some(before_revision)).unwrap();
    let stored: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let current_growth = serde_json::to_vec(&stored["document"]).unwrap().len()
        - serde_json::to_vec(&before["document"]).unwrap().len();
    assert!(
        without_history(&doc).persistence_weight() - before_weight >= current_growth,
        "current display records must fit their additional persistence reservation"
    );
    // Retain just the display edit, so unrelated graph/caption reservations cannot hide
    // missing history accounting. Undo moves that same record into the other stack.
    let mut parts = doc.to_parts();
    let latest = parts.undo.pop().unwrap();
    parts.undo = vec![latest];
    doc = Document::restore(parts).unwrap();
    let mut saved_revision = doc.revision();
    for undo in [false, true] {
        if undo {
            doc.undo().unwrap();
        }
        repo.save(&doc, Some(saved_revision)).unwrap();
        saved_revision = doc.revision();
        let stored: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let history_bytes = serde_json::to_vec(&stored["history"]).unwrap().len();
        assert!(
            doc.persistence_weight() - without_history(&doc).persistence_weight() >= history_bytes,
            "display history must fit its reservation in undo and redo (undo={undo})"
        );
        assert_eq!(repo.load(doc.id()).unwrap(), Some(doc.clone()));
    }
}

fn strip_caption_styles(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if object.get("kind").and_then(Value::as_str) == Some("caption") {
                object.remove("alignment");
            }
            if object.contains_key("script") && object.contains_key("text") {
                object.remove("outline");
                object.remove("shadow");
                object.remove("underline");
                object.remove("color");
                object.remove("size_pt");
                object.remove("font");
                object.remove("weight");
                object.remove("style");
            }
            for child in object.values_mut() {
                strip_caption_styles(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                strip_caption_styles(child);
            }
        }
        _ => {}
    }
}

fn strip_label_weights(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            object.remove("label_weight");
            object.remove("label_style");
            for child in object.values_mut() {
                strip_label_weights(child);
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                strip_label_weights(child);
            }
        }
        _ => {}
    }
}
