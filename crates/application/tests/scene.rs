//! Scene transactions through the shared application and persistent provider.

use chemifree_application::{
    Application, CommandDto, ErrorCode, SelectionDto, TransactionSelections,
};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

fn commands(value: Value) -> Vec<CommandDto> {
    serde_json::from_value(value).unwrap()
}

fn selected(objects: &[&str]) -> SelectionDto {
    SelectionDto {
        objects: objects.iter().map(|id| (*id).to_owned()).collect(),
        ..SelectionDto::default()
    }
}

#[test]
fn grouping_selection_replay_and_history_survive_reopening() {
    let data = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(data.path()));
    let id = app
        .create_document(None, Some("Scene"))
        .unwrap()
        .document
        .id;
    app.apply_transaction(
        &id,
        None,
        0,
        commands(json!([
            {"op":"add_graphic","shape":"line","points":[{"x":0,"y":0},{"x":5,"y":0}]},
            {"op":"add_graphic","shape":"arrow","points":[{"x":10,"y":0},{"x":15,"y":0}]}
        ])),
    )
    .unwrap();

    let request = commands(json!([{ "op":"group", "objects":["s2","s3"] }]));
    let selection = TransactionSelections {
        before: Some(selected(&["s2", "s3"])),
        after: Some(selected(&["$0"])),
    };
    let grouped = app
        .apply_transaction_selecting(&id, Some("group-1"), 1, request.clone(), selection.clone())
        .unwrap();
    let json = serde_json::to_value(&grouped).unwrap();
    assert_eq!(json["document"]["scene"]["roots"], json!(["s1", "s4"]));
    assert_eq!(
        json["document"]["scene"]["objects"][1]["children"],
        json!(["s2", "s3"])
    );
    assert_eq!(json["document"]["selection"]["objects"], json!(["s4"]));

    let replay = app
        .apply_transaction_selecting(&id, Some("group-1"), 1, request.clone(), selection)
        .unwrap();
    assert_eq!(
        replay, grouped,
        "replay returns the original revision and selection"
    );
    let reused = app
        .apply_transaction_selecting(
            &id,
            Some("group-1"),
            1,
            request,
            TransactionSelections::default(),
        )
        .unwrap_err();
    assert_eq!(
        reused.code,
        ErrorCode::RequestIdReused,
        "the selection participates in the request identity"
    );

    let undone = serde_json::to_value(app.undo(&id, None, 2).unwrap()).unwrap();
    assert_eq!(
        undone["document"]["selection"]["objects"],
        json!(["s2", "s3"])
    );
    let redone = serde_json::to_value(app.redo(&id, None, 3).unwrap()).unwrap();
    assert_eq!(redone["document"]["selection"]["objects"], json!(["s4"]));
    app.save(&id, None, 4).unwrap();
    app.close(&id, None, false, None).unwrap();
    let reopened = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    assert_eq!(reopened["document"], redone["document"]);
}

#[test]
fn scene_refusals_are_atomic_and_report_the_rule() {
    let data = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(data.path()));
    let id = app
        .create_document(None, Some("Scene"))
        .unwrap()
        .document
        .id;
    let initial = app.document(&id).unwrap();
    let error = app
        .apply_transaction(
            &id,
            None,
            0,
            commands(json!([
                {"op":"add_graphic","shape":"line","points":[{"x":0,"y":0},{"x":1,"y":1}]},
                {"op":"move_objects","objects":["s99"],"delta":{"x":2,"y":0}}
            ])),
        )
        .unwrap_err();
    assert_eq!(
        (error.code, error.command_index, error.reason_code()),
        (ErrorCode::InvalidCommand, Some(1), Some("object_unknown"))
    );
    assert_eq!(app.document(&id).unwrap(), initial);

    let error = app
        .apply_transaction(
            &id,
            None,
            0,
            commands(json!([
                {"op":"delete_objects","objects":["s1"]}
            ])),
        )
        .unwrap_err();
    assert_eq!(
        (error.command_index, error.reason_code()),
        (Some(0), Some("delete_mode_missing"))
    );
    assert_eq!(app.document(&id).unwrap(), initial);
}
