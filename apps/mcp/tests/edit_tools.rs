//! The edit tools of the MCP adapter against a live API server: create, transaction, undo, redo, save and
//! close, with their request-id replays, their revision conflicts and refusals, and the texts of their
//! failures.
//!
//! The tools are called directly on a `ChemifreeServer`. The document state is read back through the
//! operation that the snapshot tool runs, and the HTTP helper makes the edits of the web editor, so that a
//! conflict between the two clients is a real one.

mod common;

use chemifree_mcp::client::ApiClient;
use chemifree_mcp::discovery::{Discovery, ServerUrl};
use chemifree_mcp::tools::{ChemifreeServer, CloseArgs, CreateArgs, RevisionArgs, TransactionArgs};
use common::{Http, TestServer, closed_port, create_document};
use hyper::{Method, StatusCode};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{Value, json};

/// A server whose tools answer through a client of the server at `url`.
fn tools_for(url: &str) -> ChemifreeServer {
    ChemifreeServer::new(client_for(url))
}

/// A client of the server at `url`, which has the form `http://127.0.0.1:<port>`.
fn client_for(url: &str) -> ApiClient {
    let url: ServerUrl = url.parse().expect("the test address is a loopback address");
    ApiClient::new(Discovery::new(Some(url), None))
}

/// The text of the first content block of a tool result: the summary line of an operation, or the text of a
/// failure.
fn text_of(result: &CallToolResult) -> &str {
    result
        .content
        .first()
        .and_then(ContentBlock::as_text)
        .map(|text| text.text.as_str())
        .expect("the result starts with a text line")
}

/// The reply of an operation result, which is its structured content.
fn reply_of(result: &CallToolResult) -> &Value {
    result
        .structured_content
        .as_ref()
        .expect("an operation result carries its reply")
}

/// Asserts that a tool result is a success, not a tool error.
fn assert_success(result: &CallToolResult) {
    assert_eq!(result.is_error, Some(false), "{result:?}");
}

/// Asserts that a tool result is a tool error.
fn assert_failure(result: &CallToolResult) {
    assert_eq!(result.is_error, Some(true), "{result:?}");
}

/// The element of each atom in a snapshot, in the order that the document lists them.
fn elements_of(snapshot: &Value) -> Vec<String> {
    snapshot["result"]["document"]["atoms"]
        .as_array()
        .expect("the snapshot lists its atoms")
        .iter()
        .map(|atom| {
            atom["element"]
                .as_str()
                .expect("an atom names its element")
                .to_owned()
        })
        .collect()
}

/// The snapshot of a document, read through the operation that the snapshot tool runs.
async fn snapshot(client: &ApiClient, document_id: &str) -> Value {
    let reply = client
        .run_operation(
            "document.snapshot",
            json!({"document_id": document_id}),
            None,
        )
        .await
        .expect("the snapshot operation answers");
    assert_eq!(reply["outcome"], "ok", "{reply}");
    reply
}

/// The command that adds one carbon atom at the origin.
fn add_carbon() -> Value {
    json!({"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}})
}

/// One `add_atom` transaction sent the way the web editor sends it. Returns the status and the reply.
async fn http_transaction(
    http: &Http,
    document_id: &str,
    request_id: &str,
    expected_revision: u64,
) -> (StatusCode, Value) {
    let body = json!({
        "request_id": request_id,
        "expected_revision": expected_revision,
        "commands": [add_carbon()]
    });
    http.request(
        Method::POST,
        &format!("/api/v1/documents/{document_id}/transactions"),
        Some(&body),
    )
    .await
}

fn create(title: Option<&str>) -> Parameters<CreateArgs> {
    Parameters(CreateArgs {
        title: title.map(str::to_owned),
        request_id: None,
    })
}

fn transaction(
    document_id: &str,
    expected_revision: u64,
    commands: Vec<Value>,
    request_id: Option<&str>,
) -> Parameters<TransactionArgs> {
    Parameters(TransactionArgs {
        document_id: document_id.to_owned(),
        expected_revision,
        commands,
        request_id: request_id.map(str::to_owned),
    })
}

fn revision(
    document_id: &str,
    expected_revision: u64,
    request_id: Option<&str>,
) -> Parameters<RevisionArgs> {
    Parameters(RevisionArgs {
        document_id: document_id.to_owned(),
        expected_revision,
        request_id: request_id.map(str::to_owned),
    })
}

fn close(document_id: &str, request_id: Option<&str>) -> Parameters<CloseArgs> {
    Parameters(CloseArgs {
        document_id: document_id.to_owned(),
        request_id: request_id.map(str::to_owned),
    })
}

#[tokio::test]
async fn a_document_is_created_edited_saved_closed_and_reopened_through_the_tools() {
    let server = TestServer::start().await;
    let tools = tools_for(&server.url);
    let client = client_for(&server.url);

    let created = tools
        .chemifree_document_create(create(Some("MCP edit")))
        .await;
    assert_success(&created);
    let document_id = reply_of(&created)["result"]["document"]["id"]
        .as_str()
        .expect("the created document has an id")
        .to_owned();

    let added = tools
        .chemifree_document_transaction(transaction(&document_id, 0, vec![add_carbon()], None))
        .await;
    assert_success(&added);
    assert_eq!(
        text_of(&added),
        "document.transaction: outcome ok, revision 1"
    );
    assert_eq!(
        reply_of(&added)["result"]["session"]["unsaved_changes"],
        true,
        "{added:?}"
    );

    let saved = tools
        .chemifree_document_save(revision(&document_id, 1, None))
        .await;
    assert_success(&saved);
    assert_eq!(text_of(&saved), "document.save: outcome ok, revision 1");
    assert_eq!(
        reply_of(&saved)["result"]["session"]["unsaved_changes"],
        false,
        "{saved:?}"
    );

    let closed = tools
        .chemifree_document_close(close(&document_id, None))
        .await;
    assert_success(&closed);
    assert_eq!(text_of(&closed), "document.close: outcome ok, revision n/a");

    let reopened = snapshot(&client, &document_id).await;
    assert_eq!(reopened["document_revision"], 1, "{reopened}");
    assert_eq!(
        elements_of(&reopened),
        ["C"],
        "the saved atom is there once the session has reopened the file"
    );
}

#[tokio::test]
async fn a_repeated_request_id_returns_the_first_reply_without_applying_it_again() {
    let server = TestServer::start().await;
    let tools = tools_for(&server.url);
    let client = client_for(&server.url);
    let document_id = create_document(&server.http(), "mcp-replay-doc").await;

    let first = tools
        .chemifree_document_transaction(transaction(
            &document_id,
            0,
            vec![add_carbon()],
            Some("mcp-replay-1"),
        ))
        .await;
    let repeated = tools
        .chemifree_document_transaction(transaction(
            &document_id,
            0,
            vec![add_carbon()],
            Some("mcp-replay-1"),
        ))
        .await;
    assert_success(&first);
    assert_eq!(
        text_of(&first),
        "document.transaction: outcome ok, revision 1"
    );
    assert_eq!(
        text_of(&repeated),
        text_of(&first),
        "a replay gives the first summary line"
    );
    assert_eq!(
        repeated.structured_content, first.structured_content,
        "a replay gives the first reply"
    );
    assert_eq!(
        elements_of(&snapshot(&client, &document_id).await),
        ["C"],
        "the repeat added no atom"
    );

    let reused = tools
        .chemifree_document_transaction(transaction(
            &document_id,
            0,
            vec![json!({"op": "add_atom", "element": "N", "position": {"x": 1, "y": 0}})],
            Some("mcp-replay-1"),
        ))
        .await;
    assert_failure(&reused);
    assert_eq!(reply_of(&reused)["outcome"], "error", "{reused:?}");
    assert_eq!(
        reply_of(&reused)["diagnostics"][0]["code"],
        "request_id_reused",
        "the same id with other arguments is refused: {reused:?}"
    );

    let saved = tools
        .chemifree_document_save(revision(&document_id, 1, Some("mcp-save-1")))
        .await;
    let saved_again = tools
        .chemifree_document_save(revision(&document_id, 1, Some("mcp-save-1")))
        .await;
    assert_success(&saved);
    assert_eq!(
        saved_again.structured_content, saved.structured_content,
        "a replayed save gives the first reply"
    );

    let undone = tools
        .chemifree_history_undo(revision(&document_id, 1, Some("mcp-undo-1")))
        .await;
    let undone_again = tools
        .chemifree_history_undo(revision(&document_id, 1, Some("mcp-undo-1")))
        .await;
    assert_success(&undone);
    assert_eq!(text_of(&undone), "history.undo: outcome ok, revision 2");
    assert_eq!(
        undone_again.structured_content, undone.structured_content,
        "a replayed undo gives the first reply"
    );
    assert!(
        elements_of(&snapshot(&client, &document_id).await).is_empty(),
        "the undo ran once, so the atom is gone"
    );
}

#[tokio::test]
async fn a_stale_revision_is_refused_whichever_client_made_the_last_edit() {
    let server = TestServer::start().await;
    let tools = tools_for(&server.url);
    let http = server.http();
    let document_id = create_document(&http, "mcp-conflict-doc").await;

    let first = tools
        .chemifree_document_transaction(transaction(&document_id, 0, vec![add_carbon()], None))
        .await;
    assert_eq!(
        text_of(&first),
        "document.transaction: outcome ok, revision 1"
    );

    // The web editor still holds revision 0, so its edit is refused, and the refusal names revision 1.
    let (status, refused) = http_transaction(&http, &document_id, "http-stale-1", 0).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "revision_conflict", "{refused}");
    assert_eq!(refused["current_revision"], 1, "{refused}");

    // The web editor edits at revision 1, which moves the document to revision 2.
    let (status, edited) = http_transaction(&http, &document_id, "http-current-1", 1).await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(edited["document"]["revision"], 2, "{edited}");

    // The MCP client still holds revision 1, so its edit is refused, and the reply names revision 2.
    let stale = tools
        .chemifree_document_transaction(transaction(&document_id, 1, vec![add_carbon()], None))
        .await;
    assert_failure(&stale);
    assert_eq!(reply_of(&stale)["outcome"], "error", "{stale:?}");
    assert_eq!(
        reply_of(&stale)["diagnostics"][0]["code"],
        "revision_conflict",
        "{stale:?}"
    );
    assert_eq!(
        reply_of(&stale)["diagnostics"][0]["details"]["current_revision"],
        2,
        "{stale:?}"
    );
}

#[tokio::test]
async fn undo_and_redo_move_the_revision_and_the_atoms_together() {
    let server = TestServer::start().await;
    let tools = tools_for(&server.url);
    let client = client_for(&server.url);
    let document_id = create_document(&server.http(), "mcp-history-doc").await;

    let added = tools
        .chemifree_document_transaction(transaction(&document_id, 0, vec![add_carbon()], None))
        .await;
    assert_success(&added);

    let undone = tools
        .chemifree_history_undo(revision(&document_id, 1, None))
        .await;
    assert_success(&undone);
    assert_eq!(text_of(&undone), "history.undo: outcome ok, revision 2");
    let after_undo = snapshot(&client, &document_id).await;
    assert!(
        elements_of(&after_undo).is_empty(),
        "the undo removed the atom: {after_undo}"
    );

    let redone = tools
        .chemifree_history_redo(revision(&document_id, 2, None))
        .await;
    assert_success(&redone);
    assert_eq!(text_of(&redone), "history.redo: outcome ok, revision 3");
    let after_redo = snapshot(&client, &document_id).await;
    assert_eq!(
        elements_of(&after_redo),
        ["C"],
        "the redo restored the atom: {after_redo}"
    );
}

#[tokio::test]
async fn a_refused_transaction_changes_nothing_and_names_the_refusal() {
    let server = TestServer::start().await;
    let tools = tools_for(&server.url);
    let client = client_for(&server.url);
    let document_id = create_document(&server.http(), "mcp-validation-doc").await;

    let unknown_op = tools
        .chemifree_document_transaction(transaction(
            &document_id,
            0,
            vec![json!({"op": "add_atm"})],
            None,
        ))
        .await;
    assert_failure(&unknown_op);
    assert_eq!(reply_of(&unknown_op)["outcome"], "error", "{unknown_op:?}");
    assert_eq!(
        reply_of(&unknown_op)["diagnostics"][0]["code"],
        "invalid_request",
        "{unknown_op:?}"
    );

    let missing_atom = tools
        .chemifree_document_transaction(transaction(
            &document_id,
            0,
            vec![json!({"op": "add_bond", "begin": "a999", "end": "a998", "order": "single"})],
            None,
        ))
        .await;
    assert_failure(&missing_atom);
    assert_eq!(
        reply_of(&missing_atom)["diagnostics"][0]["code"],
        "invalid_command",
        "{missing_atom:?}"
    );
    assert_eq!(
        reply_of(&missing_atom)["diagnostics"][0]["details"]["command_index"],
        0,
        "the refusal names the first command: {missing_atom:?}"
    );

    let empty = tools
        .chemifree_document_transaction(transaction(&document_id, 0, Vec::new(), None))
        .await;
    assert_failure(&empty);
    assert_eq!(
        reply_of(&empty)["diagnostics"][0]["code"],
        "invalid_command",
        "{empty:?}"
    );
    assert!(
        reply_of(&empty)["diagnostics"][0]["details"]["command_index"].is_null(),
        "an empty list names no command: {empty:?}"
    );

    let after = snapshot(&client, &document_id).await;
    assert_eq!(after["document_revision"], 0, "{after}");
    assert!(elements_of(&after).is_empty(), "{after}");
}

#[tokio::test]
async fn closing_with_unsaved_changes_is_refused_and_keeps_the_session() {
    let server = TestServer::start().await;
    let tools = tools_for(&server.url);
    let client = client_for(&server.url);
    let document_id = create_document(&server.http(), "mcp-close-doc").await;
    let added = tools
        .chemifree_document_transaction(transaction(&document_id, 0, vec![add_carbon()], None))
        .await;
    assert_success(&added);

    let closed = tools
        .chemifree_document_close(close(&document_id, None))
        .await;
    assert_failure(&closed);
    assert_eq!(
        reply_of(&closed)["diagnostics"][0]["code"],
        "unsaved_changes",
        "{closed:?}"
    );

    let kept = snapshot(&client, &document_id).await;
    assert_eq!(kept["document_revision"], 1, "{kept}");
    assert_eq!(kept["result"]["session"]["unsaved_changes"], true, "{kept}");
}

#[tokio::test]
async fn an_oversized_transaction_is_refused_with_advice_to_split_it() {
    let server = TestServer::start().await;
    let tools = tools_for(&server.url);
    let client = client_for(&server.url);
    let document_id = create_document(&server.http(), "mcp-large-doc").await;

    // One command with 300 KB of padding makes the body larger than the 256 KiB limit, so the server refuses
    // it before the operation runs.
    let padded = json!({
        "op": "add_atom",
        "element": "C",
        "position": {"x": 0, "y": 0},
        "padding": "x".repeat(300_000)
    });
    let refused = tools
        .chemifree_document_transaction(transaction(&document_id, 0, vec![padded], None))
        .await;

    assert_failure(&refused);
    let text = text_of(&refused);
    assert!(
        text.contains("256 KiB") && text.contains("Split the transaction into smaller ones"),
        "the text names the limit and the advice, got {text}"
    );
    assert!(
        !text.contains("may have taken effect"),
        "a refused body was not applied, got {text}"
    );
    let after = snapshot(&client, &document_id).await;
    assert_eq!(after["document_revision"], 0, "{after}");
}

#[tokio::test]
async fn an_edit_to_an_address_with_no_server_names_no_request_id() {
    let url = format!("http://127.0.0.1:{}", closed_port());
    let tools = tools_for(&url);
    let expected = format!(
        "No running Chemifree API server answered at {url}. Start chemifree-api for that data directory, or check --server-url."
    );

    let closed = tools.chemifree_document_close(close("missing", None)).await;
    assert_failure(&closed);
    assert_eq!(text_of(&closed), expected);

    let edited = tools
        .chemifree_document_transaction(transaction("missing", 0, vec![add_carbon()], None))
        .await;
    assert_failure(&edited);
    assert_eq!(text_of(&edited), expected);
}
