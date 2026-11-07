//! Cosmetic text is strict v23 state; migration never changes old graph/history or bytes.
use chemifree_application::{DocumentRepository, Draft, DraftRecoveryTarget};
use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondOrder, BondTo, Command, Document, DocumentId, NewAtomPositions,
    ObjectId, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{Caption, CaptionRun, CaptionScript};
use chemifree_providers::FileDocumentRepository;
use serde_json::{Value, json};
use std::fs;

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

fn label(text: Option<&str>) -> Command {
    Command::UpdateAbbreviationLabel {
        instance: ObjectId::from_serial(1).unwrap(),
        anchor: AtomId::from_serial(2).unwrap(),
        text: text.map(str::to_owned),
    }
}
fn legacy(mut raw: Value) -> Value {
    raw["format_version"] = json!(22);
    strip_fragment_displays(&mut raw);
    raw["document"]
        .as_object_mut()
        .unwrap()
        .remove("display_labels");
    for stack in ["undo", "redo"] {
        for state in raw["history"][stack].as_array_mut().unwrap() {
            state.as_object_mut().unwrap().remove("display_labels");
        }
    }
    raw
}
fn strip_fragment_displays(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("fragment_displays");
            for child in object.values_mut() {
                strip_fragment_displays(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                strip_fragment_displays(child);
            }
        }
        _ => {}
    }
}
#[test]
fn current_file_and_draft_replay_custom_unicode_on_both_history_sides() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = drawing();
    doc.apply(&[collapse(), label(Some("Ph 한 e\u{301} 🧪"))])
        .unwrap();
    doc.apply(&[label(None)]).unwrap();
    doc.undo().unwrap();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/ethyl-native.json");
    let raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(raw["format_version"], 25);
    assert_eq!(
        raw["document"]["display_labels"][0]["text"],
        "Ph 한 e\u{301} 🧪"
    );
    assert_eq!(repo.load(doc.id()).unwrap(), Some(doc.clone()));
    let draft = Draft {
        document: doc.clone(),
        base_revision: doc.revision(),
        saved_at_ms: 42,
    };
    repo.write_draft(&draft).unwrap();
    repo.release_draft(doc.id());
    assert_eq!(repo.claim_draft(doc.id()), Ok(Some(Ok(draft))));
    repo.release_draft(doc.id());
    let mut reopened = repo.load(doc.id()).unwrap().unwrap();
    reopened.redo().unwrap();
    assert!(reopened.to_parts().display_labels.is_empty());
    reopened.undo().unwrap();
    assert_eq!(
        reopened.to_parts().display_labels,
        doc.to_parts().display_labels
    );
}
#[test]
fn strict_fields_current_and_history_refuse_without_rewriting_file_or_draft() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = drawing();
    doc.apply(&[collapse(), label(Some("Me"))]).unwrap();
    doc.apply(&[label(None)]).unwrap();
    doc.undo().unwrap();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/ethyl-native.json");
    let template: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for location in ["/document", "/history/undo/0", "/history/redo/0"] {
        for bad in [
            None,
            Some(Value::Null),
            Some(json!({})),
            Some(json!([{"instance":"s1","anchor":"a1","text":"Ph"}])),
            Some(json!([{"instance":"s1","anchor":"a2","text":"bad\ntext"}])),
        ] {
            for draft in [false, true] {
                let mut raw = template.clone();
                let state = raw.pointer_mut(location).unwrap().as_object_mut().unwrap();
                state.remove("display_labels");
                if let Some(value) = &bad {
                    state.insert("display_labels".into(), value.clone());
                }
                let target = if draft {
                    raw["format"] = json!("chemifree.draft");
                    raw["base_revision"] = json!(doc.revision());
                    raw["saved_at_ms"] = json!(42);
                    root.path().join("drafts/ethyl-native.json")
                } else {
                    path.clone()
                };
                let bytes = serde_json::to_vec(&raw).unwrap();
                fs::create_dir_all(target.parent().unwrap()).unwrap();
                fs::write(&target, &bytes).unwrap();
                if draft {
                    assert!(repo.claim_draft(doc.id()).unwrap().unwrap().is_err());
                    repo.release_draft(doc.id());
                } else {
                    assert!(repo.load(doc.id()).is_err());
                }
                assert_eq!(fs::read(target).unwrap(), bytes);
            }
        }
    }
}
#[test]
fn v22_defaults_and_full_history_survive_read_and_new_save_archives_exact_original() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = drawing();
    doc.apply(&[collapse()]).unwrap();
    doc.apply(&[Command::Rename {
        title: "later".into(),
    }])
    .unwrap();
    doc.undo().unwrap();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/ethyl-native.json");
    let raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let old = legacy(raw);
    let bytes = serde_json::to_vec_pretty(&old).unwrap();
    fs::write(&path, &bytes).unwrap();
    let loaded = repo.load(doc.id()).unwrap().unwrap();
    assert_eq!(loaded, doc);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    repo.save(&loaded, Some(loaded.revision())).unwrap();
    let rewritten: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(rewritten["format_version"], 25);
    assert_eq!(legacy(rewritten), old);
    assert_eq!(fs::read(repo.migrated_path(doc.id(), 22)).unwrap(), bytes);
}
#[test]
fn old_versions_refuse_new_fields_even_empty_and_only_in_history() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = drawing();
    doc.apply(&[collapse()]).unwrap();
    doc.undo().unwrap();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/ethyl-native.json");
    let raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for version in 1..=22 {
        for location in ["/document", "/history/undo/0", "/history/redo/0"] {
            let mut old = legacy(raw.clone());
            old["format_version"] = json!(version);
            old.pointer_mut(location)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("display_labels".into(), json!([]));
            let bytes = serde_json::to_vec(&old).unwrap();
            fs::write(&path, &bytes).unwrap();
            assert!(repo.load(doc.id()).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
}

#[test]
fn maximal_escaped_display_text_and_large_history_fit_the_physical_native_limit() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = drawing();
    let mut graph = Vec::new();
    for _ in 1..100 {
        let first = graph.len();
        for offset in 0..3 {
            graph.push(Command::AddAtom {
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
    doc.apply(&graph).unwrap();
    let compact: Vec<_> = (0..100)
        .map(|index| Command::CollapseEthyl {
            instance: ObjectId::from_serial(1).unwrap(),
            anchor: AtomId::from_serial(index * 3 + 2).unwrap(),
            terminal: AtomId::from_serial(index * 3 + 3).unwrap(),
        })
        .collect();
    doc.apply(&compact).unwrap();
    for index in 0..100 {
        let text = if index % 2 == 0 {
            "\"".repeat(128)
        } else {
            "\\".repeat(128)
        };
        let edits: Vec<_> = (0..100)
            .map(|index| Command::UpdateAbbreviationLabel {
                instance: ObjectId::from_serial(1).unwrap(),
                anchor: AtomId::from_serial(index * 3 + 2).unwrap(),
                text: Some(text.clone()),
            })
            .collect();
        doc.apply(&edits).unwrap();
    }
    assert!(
        doc.to_parts().undo.len() < 100,
        "text history uses byte admission rather than unbounded entity counts"
    );
    repo.save(&doc, None).unwrap();
    let bytes = fs::read(root.path().join("documents/ethyl-native.json")).unwrap();
    assert!(bytes.len() <= doc.persistence_weight());
    assert!(doc.persistence_weight() < 64 * 1024 * 1024);
    assert_eq!(repo.load(doc.id()).unwrap(), Some(doc.clone()));
    let draft = Draft {
        document: doc.clone(),
        base_revision: doc.revision(),
        saved_at_ms: 42,
    };
    repo.write_draft(&draft).unwrap();
    repo.release_draft(doc.id());
    let bytes = fs::read(root.path().join("drafts/ethyl-native.json")).unwrap();
    assert!(bytes.len() <= doc.persistence_weight());
    assert!(bytes.len() < 64 * 1024 * 1024);
    assert_eq!(repo.claim_draft(doc.id()), Ok(Some(Ok(draft))));
    repo.release_draft(doc.id());
}

#[test]
fn invalid_text_in_well_shaped_current_undo_and_redo_records_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = drawing();
    doc.apply(&[collapse(), label(Some("Me"))]).unwrap();
    doc.apply(&[label(None)]).unwrap();
    doc.undo().unwrap();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/ethyl-native.json");
    let template: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for location in [
        "/document/display_labels/0/text",
        "/history/undo/1/display_labels/0/after",
        "/history/redo/0/display_labels/0/before",
    ] {
        for invalid in ["", "   ", "bad\ntext", &"한".repeat(43)] {
            let mut raw = template.clone();
            *raw.pointer_mut(location).unwrap() = json!(invalid);
            let bytes = serde_json::to_vec(&raw).unwrap();
            fs::write(&path, &bytes).unwrap();
            assert!(repo.load(doc.id()).is_err(), "{location}: {invalid:?}");
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
}

#[test]
fn v22_draft_retains_default_display_and_history_then_archives_exact_bytes() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = drawing();
    doc.apply(&[collapse()]).unwrap();
    doc.apply(&[Command::Rename {
        title: "later".into(),
    }])
    .unwrap();
    doc.undo().unwrap();
    // Original-target recovery requires an existing saved base at this revision.
    repo.save(&doc, None).unwrap();
    let draft = Draft {
        document: doc.clone(),
        base_revision: doc.revision(),
        saved_at_ms: 42,
    };
    repo.write_draft(&draft).unwrap();
    repo.release_draft(doc.id());
    let path = root.path().join("drafts/ethyl-native.json");
    let raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let bytes = serde_json::to_vec_pretty(&legacy(raw)).unwrap();
    fs::write(&path, &bytes).unwrap();
    assert_eq!(repo.claim_draft(doc.id()), Ok(Some(Ok(draft))));
    assert_eq!(fs::read(&path).unwrap(), bytes);
    repo.prepare_draft_for_recovery(
        doc.id(),
        DraftRecoveryTarget::Original {
            base_revision: doc.revision(),
        },
    )
    .unwrap();
    assert_eq!(
        fs::read(repo.migrated_draft_path(doc.id(), 22)).unwrap(),
        bytes
    );
    let base_revision = doc.revision();
    let graph = doc.to_parts();
    doc.apply(&[label(Some("Ph 한"))]).unwrap();
    let next = Draft {
        document: doc.clone(),
        base_revision,
        saved_at_ms: 43,
    };
    repo.write_draft(&next).unwrap();
    repo.release_draft(doc.id());
    let mut recovered = repo
        .claim_draft(doc.id())
        .unwrap()
        .unwrap()
        .unwrap()
        .document;
    assert_eq!(recovered, doc);
    assert_eq!(recovered.to_parts().atoms, graph.atoms);
    assert_eq!(recovered.to_parts().bonds, graph.bonds);
    assert_eq!(recovered.to_parts().positions, graph.positions);
    recovered.undo().unwrap();
    assert!(recovered.to_parts().display_labels.is_empty());
    recovered.redo().unwrap();
    assert_eq!(
        recovered.to_parts().display_labels,
        doc.to_parts().display_labels
    );
    assert_eq!(
        fs::read(repo.migrated_draft_path(doc.id(), 22)).unwrap(),
        bytes
    );
    repo.release_draft(doc.id());
}

#[test]
fn invalid_display_history_cannot_hide_beyond_document_or_draft_retention() {
    let mut doc = drawing();
    doc.apply(&[collapse()]).unwrap();
    for index in 0..101 {
        doc.apply(&[label(Some(if index % 2 == 0 { "Ph" } else { "Me" }))])
            .unwrap();
    }
    for redo in [false, true] {
        if redo {
            for _ in 0..100 {
                doc.undo().unwrap();
            }
        }
        let root = tempfile::tempdir().unwrap();
        let repo = FileDocumentRepository::new(root.path());
        repo.save(&doc, None).unwrap();
        let document_path = root.path().join("documents/ethyl-native.json");
        let mut raw: Value = serde_json::from_slice(&fs::read(&document_path).unwrap()).unwrap();
        let stack = if redo { "redo" } else { "undo" };
        let states = raw["history"][stack].as_array_mut().unwrap();
        states.insert(0, states[0].clone());
        assert_eq!(states.len(), 101);
        fs::write(&document_path, serde_json::to_vec(&raw).unwrap()).unwrap();
        assert_eq!(
            repo.load(doc.id()).unwrap(),
            Some(doc.clone()),
            "well-formed excess entries still trim"
        );
        for invalid in ["bad\ntext", "", "Et", &"한".repeat(43)] {
            for draft in [false, true] {
                let mut malformed = raw.clone();
                malformed["history"][stack][0]["display_labels"][0]["before"] = json!(invalid);
                let target = if draft {
                    malformed["format"] = json!("chemifree.draft");
                    malformed["base_revision"] = json!(doc.revision());
                    malformed["saved_at_ms"] = json!(42);
                    root.path().join("drafts/ethyl-native.json")
                } else {
                    document_path.clone()
                };
                let bytes = serde_json::to_vec(&malformed).unwrap();
                fs::create_dir_all(target.parent().unwrap()).unwrap();
                fs::write(&target, &bytes).unwrap();
                let error = if draft {
                    let error = repo.claim_draft(doc.id()).unwrap().unwrap().unwrap_err();
                    repo.release_draft(doc.id());
                    error
                } else {
                    repo.load(doc.id()).unwrap_err().to_string()
                };
                assert!(error.contains("display label history"), "{error}");
                assert_eq!(
                    fs::read(&target).unwrap(),
                    bytes,
                    "malformed input stays untouched"
                );
            }
        }
    }
}
