//! Clipboard v3 styles survive the unchanged native v26 repository and recovery history.

use std::fs;

use chemifree_application::{Application, DocumentRepository};
use chemifree_domain::document::{
    Atom, Command, Document, DocumentId, NewAtomPositions, ObjectId, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{Graphic, GraphicShape};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

#[test]
fn copied_widths_keep_native26_and_recovery_undo_without_rewriting_the_source() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut source = Document::new(DocumentId::parse("styled-source").unwrap(), "Source").unwrap();
    source
        .apply(&[
            Command::AddGraphic {
                graphic: Graphic {
                    line_width_pt: Some(2.75),
                    shape: GraphicShape::Line,
                    points: [
                        Point::new(0.0, 0.0).unwrap(),
                        Point::new(10.0, 10.0).unwrap(),
                    ],
                },
                parent: None,
                index: None,
            },
            Command::AddGraphic {
                graphic: Graphic {
                    line_width_pt: None,
                    shape: GraphicShape::Arrow,
                    points: [
                        Point::new(20.0, 0.0).unwrap(),
                        Point::new(30.0, 10.0).unwrap(),
                    ],
                },
                parent: None,
                index: None,
            },
            Command::Group {
                objects: vec![
                    ObjectId::from_serial(2).unwrap(),
                    ObjectId::from_serial(3).unwrap(),
                ],
            },
        ])
        .unwrap();
    repo.save(&source, None).unwrap();
    let source_path = root.path().join("documents/styled-source.json");
    let source_bytes = fs::read(&source_path).unwrap();
    let mut destination =
        Document::new(DocumentId::parse("styled-target").unwrap(), "Target").unwrap();
    destination
        .apply(&[Command::AddAtom {
            atom: Atom::from(Element::from_symbol("N").unwrap()),
            positions: NewAtomPositions::Single(Point::new(3.0, 4.0).unwrap()),
            bond_to: None,
        }])
        .unwrap();
    repo.save(&destination, None).unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let execute = |id: &str, operation: &str, parameters: Value| -> Value {
        serde_json::to_value(app.execute(json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters}).to_string().as_bytes())).unwrap()
    };
    let copied = execute(
        "copy",
        "document.clipboard_scene_copy",
        json!({"document_id":"styled-source","selection":{"objects":["s4"]}}),
    );
    assert_eq!(copied["outcome"], "ok", "{copied}");
    let payload: Value =
        serde_json::from_str(copied["result"]["payload"].as_str().unwrap()).unwrap();
    assert_eq!(payload["version"], 3);
    assert_eq!(fs::read(&source_path).unwrap(), source_bytes);
    let pasted = execute(
        "paste",
        "document.clipboard_scene_paste",
        json!({"document_id":"styled-target","expected_revision":1,"payload":copied["result"]["payload"],"offset":{"x":10,"y":20}}),
    );
    assert_eq!(pasted["outcome"], "ok", "{pasted}");
    let atom_snapshot = pasted["result"]["snapshot"]["document"]["atoms"].clone();
    app.save("styled-target", None, 2).unwrap();
    let target_path = root.path().join("documents/styled-target.json");
    let native: Value = serde_json::from_slice(&fs::read(&target_path).unwrap()).unwrap();
    assert_eq!(native["format_version"], 27);
    assert_eq!(
        native["document"]["objects"][1]["state"]["line_width_pt"],
        2.75
    );
    assert_eq!(
        native["document"]["objects"][2]["state"]["line_width_pt"],
        Value::Null
    );
    app.apply_transaction(
        "styled-target",
        None,
        2,
        serde_json::from_value(
            json!([{"op":"set_graphic_line_width","object":"s2","line_width_pt":null}]),
        )
        .unwrap(),
    )
    .unwrap();
    app.autosave("styled-target", None).unwrap();
    drop(app);
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    app.document("styled-target").unwrap();
    let recovered = app.recover("styled-target", None).unwrap();
    let current = serde_json::to_value(&recovered).unwrap();
    let line = current["document"]["scene"]["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["id"] == "s2")
        .unwrap();
    assert_eq!(line["line_width_pt"], Value::Null);
    assert_eq!(current["document"]["atoms"], atom_snapshot);
    let undone = serde_json::to_value(
        app.undo("styled-target", None, recovered.document.revision)
            .unwrap(),
    )
    .unwrap();
    let line = undone["document"]["scene"]["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["id"] == "s2")
        .unwrap();
    assert_eq!(line["line_width_pt"], 2.75);
    assert_eq!(fs::read(&source_path).unwrap(), source_bytes);
}
