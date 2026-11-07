//! v24 fragment metadata is strict in current/history states and absent in older archives.

use std::fs;

use chemifree_application::{DocumentRepository, Draft};
use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondOrder, Command, Document, DocumentId, MAX_ATOMS, NewAtomPositions,
    ObjectId, Point,
};
use chemifree_domain::element::Element;
use chemifree_providers::{FileDocumentRepository, MAX_DOCUMENT_BYTES};
use serde_json::{Value, json};

fn atom(id: u32) -> AtomId {
    AtomId::from_serial(id).unwrap()
}
fn instance() -> ObjectId {
    ObjectId::from_serial(1).unwrap()
}
fn drawing() -> Document {
    let mut doc = Document::new(DocumentId::parse("fragment-native").unwrap(), "Fragment").unwrap();
    doc.apply(
        &(0..4)
            .map(|i| Command::AddAtom {
                atom: Atom::from(Element::CARBON),
                positions: NewAtomPositions::Single(Point::new(i as f64 * 30.0, 0.0).unwrap()),
                bond_to: None,
            })
            .collect::<Vec<_>>(),
    )
    .unwrap();
    doc.apply(&[
        Command::AddBond {
            begin: AtomRef::Id(atom(1)),
            end: AtomRef::Id(atom(2)),
            order: BondOrder::Single,
        },
        Command::AddBond {
            begin: AtomRef::Id(atom(2)),
            end: AtomRef::Id(atom(3)),
            order: BondOrder::Single,
        },
        Command::AddBond {
            begin: AtomRef::Id(atom(2)),
            end: AtomRef::Id(atom(4)),
            order: BondOrder::Single,
        },
    ])
    .unwrap();
    doc
}
fn contract() -> Command {
    Command::ContractFragment {
        instance: instance(),
        anchor: atom(2),
        members: vec![atom(2), atom(3), atom(4)],
        text: "iPr 한 e\u{301} 🧪".into(),
    }
}
fn label(text: &str) -> Command {
    Command::SetFragmentDisplayText {
        instance: instance(),
        anchor: atom(2),
        text: text.into(),
    }
}
fn v23(mut raw: Value) -> Value {
    raw["format_version"] = json!(23);
    raw["document"]
        .as_object_mut()
        .unwrap()
        .remove("fragment_displays");
    for stack in ["undo", "redo"] {
        for state in raw["history"][stack].as_array_mut().unwrap() {
            state.as_object_mut().unwrap().remove("fragment_displays");
        }
    }
    raw
}

#[test]
fn current_file_and_draft_round_trip_both_history_sides_and_unicode() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = drawing();
    doc.apply(&[contract()]).unwrap();
    doc.apply(&[label("OMe")]).unwrap();
    doc.undo().unwrap();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/fragment-native.json");
    let raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(raw["format_version"], 24);
    assert_eq!(
        raw["document"]["fragment_displays"][0]["members"],
        json!(["a2", "a3", "a4"])
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
    assert_eq!(reopened.fragment_displays().next().unwrap().1.text, "OMe");
    reopened.undo().unwrap();
    assert_eq!(
        reopened.fragment_displays().next().unwrap().1.text,
        "iPr 한 e\u{301} 🧪"
    );
}

#[test]
fn malformed_current_history_and_future_headers_refuse_without_rewriting_bytes() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = drawing();
    doc.apply(&[contract()]).unwrap();
    doc.apply(&[label("x")]).unwrap();
    doc.undo().unwrap();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/fragment-native.json");
    let template: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let mut variants = Vec::new();
    for location in ["/document", "/history/undo/0", "/history/redo/0"] {
        for value in [None, Some(Value::Null), Some(json!({}))] {
            let mut raw = template.clone();
            let object = raw.pointer_mut(location).unwrap().as_object_mut().unwrap();
            object.remove("fragment_displays");
            if let Some(value) = value {
                object.insert("fragment_displays".into(), value);
            }
            variants.push(raw);
        }
    }
    for location in [
        "/document/fragment_displays/0",
        "/history/undo/2/fragment_displays/0/after",
        "/history/redo/0/fragment_displays/0/before",
    ] {
        for (field, value) in [
            ("text", json!("\n")),
            ("text", json!("한".repeat(43))),
            ("members", json!([])),
            ("members", json!(["a2", "a2"])),
            ("members", json!(["a3", "a4"])),
            ("members", json!(["a2", "a99999"])),
            ("unknown", json!(true)),
        ] {
            let mut raw = template.clone();
            raw.pointer_mut(location)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert(field.into(), value);
            variants.push(raw);
        }
    }
    let mut future = template.clone();
    future["format_version"] = json!(25);
    variants.push(future);
    for raw in variants {
        for draft in [false, true] {
            let mut raw = raw.clone();
            let target = if draft {
                raw["format"] = json!("chemifree.draft");
                raw["base_revision"] = json!(doc.revision());
                raw["saved_at_ms"] = json!(42);
                root.path().join("drafts/fragment-native.json")
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
            assert_eq!(fs::read(&target).unwrap(), bytes);
        }
    }
}

#[test]
fn v23_migrates_empty_and_archives_exact_original_but_refuses_even_empty_smuggling() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let doc = drawing();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/fragment-native.json");
    let old = v23(serde_json::from_slice(&fs::read(&path).unwrap()).unwrap());
    let bytes = serde_json::to_vec_pretty(&old).unwrap();
    fs::write(&path, &bytes).unwrap();
    let loaded = repo.load(doc.id()).unwrap().unwrap();
    assert_eq!(loaded, doc);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    for location in ["/document", "/history/undo/0"] {
        let mut raw = old.clone();
        raw.pointer_mut(location)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("fragment_displays".into(), json!([]));
        fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
        assert!(repo.load(doc.id()).is_err());
    }
    fs::write(&path, &bytes).unwrap();
    repo.save(&loaded, Some(loaded.revision())).unwrap();
    assert_eq!(fs::read(repo.migrated_path(doc.id(), 23)).unwrap(), bytes);
    let rewritten: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(rewritten["format_version"], 24);
    assert_eq!(v23(rewritten), old);
}

#[test]
fn maximum_member_lists_and_text_history_fit_native_file_and_draft_reservations() {
    let mut doc = Document::new(
        DocumentId::parse("maximum-fragment").unwrap(),
        "Maximum explicit tree",
    )
    .unwrap();
    for first in (0..MAX_ATOMS).step_by(500) {
        let commands: Vec<_> = (first..(first + 500).min(MAX_ATOMS))
            .map(|index| Command::AddAtom {
                atom: Atom::from(Element::CARBON),
                positions: NewAtomPositions::Single(Point::new(index as f64, 0.0).unwrap()),
                bond_to: None,
            })
            .collect();
        doc.apply(&commands).unwrap();
    }
    for first in (2..=MAX_ATOMS).step_by(500) {
        let commands: Vec<_> = (first..(first + 500).min(MAX_ATOMS + 1))
            .map(|index| Command::AddBond {
                begin: AtomRef::Id(atom(index as u32 - 1)),
                end: AtomRef::Id(atom(index as u32)),
                order: BondOrder::Single,
            })
            .collect();
        doc.apply(&commands).unwrap();
    }
    doc.apply(&[Command::ContractFragment {
        instance: instance(),
        anchor: atom(2),
        members: (2..=MAX_ATOMS as u32).map(atom).collect(),
        text: "\\\"".repeat(64),
    }])
    .unwrap();
    for index in 0..100 {
        doc.apply(&[label(&format!(
            "{}{}",
            "\\\"".repeat(62),
            if index % 2 == 0 { "aa" } else { "bb" }
        ))])
        .unwrap();
    }
    assert_eq!(
        doc.fragment_displays().next().unwrap().1.members.len(),
        MAX_ATOMS - 1
    );
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    repo.save(&doc, None).unwrap();
    let bytes = fs::read(root.path().join("documents/maximum-fragment.json")).unwrap();
    assert!(bytes.len() <= doc.persistence_weight());
    assert!((bytes.len() as u64) < MAX_DOCUMENT_BYTES);
    assert_eq!(repo.load(doc.id()).unwrap(), Some(doc.clone()));
    let draft = Draft {
        document: doc.clone(),
        base_revision: doc.revision(),
        saved_at_ms: 42,
    };
    repo.write_draft(&draft).unwrap();
    repo.release_draft(doc.id());
    let bytes = fs::read(root.path().join("drafts/maximum-fragment.json")).unwrap();
    assert!(bytes.len() <= doc.persistence_weight());
    assert!((bytes.len() as u64) < MAX_DOCUMENT_BYTES);
    assert_eq!(repo.claim_draft(doc.id()), Ok(Some(Ok(draft))));
    repo.release_draft(doc.id());
}
