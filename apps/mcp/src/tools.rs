//! The MCP tools, and how their replies become MCP results.
//!
//! The read tools inspect the running server: its operation catalog, its document list, and one document
//! snapshot with its revision, which includes unsaved edits. The edit tools create, edit, save and close
//! documents through the operation contract, which applies the same rules as the web editor's HTTP calls,
//! so an edit shows in the editor that the user has open. A reply goes out as `structured_content`,
//! unchanged, with one text line that names the operation's outcome and revision. An operation reply is a
//! tool error (`isError: true`) when its outcome is anything but `ok` or `partial`: `error`, `unsupported`,
//! or an outcome this adapter does not know. It is also one when it is a protocol error, or when the call
//! failed. A `partial` outcome is not an error: the operation completed, and its notes or losses say what
//! is missing. Diagnostics pass through unchanged, and nothing about the chemistry is interpreted here.

use hyper::StatusCode;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::client::{ApiClient, ClientError, DocumentList, new_request_id};

/// The text of the tool error for an edit that the server refused for its size. The server refuses a body
/// over its size limit before it runs the operation, so nothing was applied, and no request id is named. The
/// limit is the API's `MAX_BODY_BYTES`.
const TOO_LARGE_TEXT: &str = "The Chemifree API server refused the request because it is larger than 256 KiB (HTTP 413), so the operation was not applied. Split the transaction into smaller ones, each with its own request_id and the revision that the previous one returned.";

/// The arguments of `chemifree_capabilities`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilitiesArgs {
    /// An optional request id for this call. A new one is made when it is omitted.
    pub request_id: Option<String>,
}

/// The arguments of `chemifree_document_snapshot`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentSnapshotArgs {
    /// The document's id, as `chemifree_documents_list` lists it.
    pub document_id: String,
    /// An optional request id for this call. A new one is made when it is omitted.
    pub request_id: Option<String>,
}

/// The arguments of `chemifree_document_create`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateArgs {
    /// The new document's title. A default title is used when it is omitted.
    pub title: Option<String>,
    /// An optional request id: 1 to 64 ASCII letters, digits, `_` or `-`. A new one is made when it is
    /// omitted. Repeating a call with the same id and arguments returns the first reply without applying it
    /// again.
    pub request_id: Option<String>,
}

/// The arguments of `chemifree_document_transaction`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransactionArgs {
    /// The document's id, as `chemifree_documents_list` lists it.
    pub document_id: String,
    /// The document's current revision, as `chemifree_document_snapshot` reports it. A transaction at any
    /// other revision is refused with `revision_conflict`, and the reply names the current revision.
    pub expected_revision: u64,
    // The commands. `commands_schema` gives this field's schema, which describes each command.
    #[schemars(schema_with = "commands_schema")]
    pub commands: Vec<Value>,
    /// An optional request id: 1 to 64 ASCII letters, digits, `_` or `-`. A new one is made when it is
    /// omitted. Repeating a call with the same id and arguments returns the first reply without applying it
    /// again.
    pub request_id: Option<String>,
}

/// The arguments of `chemifree_history_undo`, `chemifree_history_redo` and `chemifree_document_save`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RevisionArgs {
    /// The document's id, as `chemifree_documents_list` lists it.
    pub document_id: String,
    /// The document's current revision, as `chemifree_document_snapshot` reports it. A call at any other
    /// revision is refused with `revision_conflict`, and the reply names the current revision.
    pub expected_revision: u64,
    /// An optional request id: 1 to 64 ASCII letters, digits, `_` or `-`. A new one is made when it is
    /// omitted. Repeating a call with the same id and arguments returns the first reply without applying it
    /// again.
    pub request_id: Option<String>,
}

/// The arguments of `chemifree_document_close`. The tool never discards unsaved edits, so a document with
/// unsaved changes stays open: save it first.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CloseArgs {
    /// The document's id, as `chemifree_documents_list` lists it.
    pub document_id: String,
    /// An optional request id: 1 to 64 ASCII letters, digits, `_` or `-`. A new one is made when it is
    /// omitted. Repeating a call with the same id and arguments returns the first reply without applying it
    /// again.
    pub request_id: Option<String>,
}

/// The schema of `commands`: an array of objects, each tagged by `op`. The server checks each command, so the
/// schema names no operation.
fn commands_schema(_generator: &mut SchemaGenerator) -> Schema {
    schemars::json_schema!({
        "type": "array",
        "description": "Transaction commands, 1–500, applied all or nothing. Each is an object tagged by \"op\"; the op names and what they do are the parts of document.transaction in chemifree_capabilities; fields as in docs/CONTRACT.md \"Edits\". The server validates them: a malformed command is outcome error with diagnostic invalid_request, and a refused one is invalid_command with details.command_index naming it when there is one and details.reason when the refusal gives one.",
        "items": {
            "type": "object",
            "required": ["op"],
            "properties": {"op": {"type": "string"}}
        }
    })
}

/// One operation call, as the edit tools make it: the operation, its parameters, and the request id that a
/// repeat must carry. A call without a request id gets a new one when it is sent.
#[derive(Clone, Debug, PartialEq)]
struct OperationCall {
    /// The operation's id, as `chemifree_capabilities` lists it.
    operation: &'static str,
    /// The parameters, with the fields that the contract names and no others.
    parameters: Value,
    /// The caller's request id, or `None` when the tool makes one.
    request_id: Option<String>,
}

/// The call of `chemifree_document_create`: `document.create`, with `title` only when a title is given.
#[must_use]
fn create_call(args: CreateArgs) -> OperationCall {
    let parameters = match args.title {
        Some(title) => json!({"title": title}),
        None => json!({}),
    };
    OperationCall {
        operation: "document.create",
        parameters,
        request_id: args.request_id,
    }
}

/// The call of `chemifree_document_transaction`: `document.transaction`, with its three fields.
#[must_use]
fn transaction_call(args: TransactionArgs) -> OperationCall {
    OperationCall {
        operation: "document.transaction",
        parameters: json!({
            "document_id": args.document_id,
            "expected_revision": args.expected_revision,
            "commands": args.commands,
        }),
        request_id: args.request_id,
    }
}

/// The call of `chemifree_history_undo`, `chemifree_history_redo` or `chemifree_document_save`: the named
/// operation, with the document and the revision that the caller expects.
#[must_use]
fn revision_call(operation: &'static str, args: RevisionArgs) -> OperationCall {
    OperationCall {
        operation,
        parameters: json!({
            "document_id": args.document_id,
            "expected_revision": args.expected_revision,
        }),
        request_id: args.request_id,
    }
}

/// The call of `chemifree_document_close`: `document.close` with the document id only. The tool never sends
/// `discard_unsaved`, so a document with unsaved changes is refused.
#[must_use]
fn close_call(args: CloseArgs) -> OperationCall {
    OperationCall {
        operation: "document.close",
        parameters: json!({"document_id": args.document_id}),
        request_id: args.request_id,
    }
}

/// The MCP server: the read tools and the edit tools, over one [`ApiClient`].
#[derive(Debug)]
pub struct ChemifreeServer {
    client: ApiClient,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl ChemifreeServer {
    /// A server whose tools answer through `client`.
    #[must_use]
    pub fn new(client: ApiClient) -> Self {
        Self {
            client,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Inspect the operation catalog of the running Chemifree server (operation capabilities.inspect). Read-only: nothing is changed.",
        annotations(read_only_hint = true)
    )]
    async fn chemifree_capabilities(
        &self,
        Parameters(args): Parameters<CapabilitiesArgs>,
    ) -> CallToolResult {
        self.run("capabilities.inspect", json!({}), args.request_id)
            .await
    }

    #[tool(
        description = "List the documents that the running Chemifree server holds (GET /api/v1/documents). Read-only: nothing is changed.",
        annotations(read_only_hint = true)
    )]
    async fn chemifree_documents_list(&self) -> CallToolResult {
        match self.client.list_documents().await {
            Ok(list) => documents_result(list),
            Err(error) => error_result(&error),
        }
    }

    #[tool(
        description = "Read a document snapshot, with its revision, from the running Chemifree server (operation document.snapshot). Unsaved edits are included. Read-only: nothing is changed.",
        annotations(read_only_hint = true)
    )]
    async fn chemifree_document_snapshot(
        &self,
        Parameters(args): Parameters<DocumentSnapshotArgs>,
    ) -> CallToolResult {
        self.run(
            "document.snapshot",
            json!({"document_id": args.document_id}),
            args.request_id,
        )
        .await
    }

    /// Creates a document, with an optional title. Its reply names the document's id.
    #[tool(
        description = "Create a document in the running Chemifree server (operation document.create), with an optional title. Pass a request_id if you may retry; otherwise a retry creates another document. Repeating a call with the same request_id and arguments returns the first reply without applying it again, while the same server process still remembers it (the last 64 edits of an open document; a close forgets them).",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    pub async fn chemifree_document_create(
        &self,
        Parameters(args): Parameters<CreateArgs>,
    ) -> CallToolResult {
        self.run_edit(create_call(args)).await
    }

    /// Applies commands to a document, all or nothing, at the revision that the caller expects. A command may
    /// delete atoms or bonds, so the tool is marked destructive.
    #[tool(
        description = "Apply commands to a document of the running Chemifree server (operation document.transaction), all or nothing. expected_revision must be the document's current revision, as chemifree_document_snapshot reports it. Each command is an object tagged by op: the op names and what they do are the parts of document.transaction in chemifree_capabilities, and the fields are as in docs/CONTRACT.md \"Edits\". Without request_id each call is a new request, and once the document has moved on, expected_revision refuses a repeat with revision_conflict. Repeating a call with the same request_id and arguments returns the first reply without applying it again, while the same server process still remembers it (the last 64 edits of an open document; a close forgets them).",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true
        )
    )]
    pub async fn chemifree_document_transaction(
        &self,
        Parameters(args): Parameters<TransactionArgs>,
    ) -> CallToolResult {
        self.run_edit(transaction_call(args)).await
    }

    /// Undoes the last edit of a document, at the revision that the caller expects. The undo history is shared
    /// with the web editor, so this can undo an edit that the user made.
    #[tool(
        description = "Undo the last edit of a document in the running Chemifree server (operation history.undo). The undo history is shared with the web editor: this undoes the document's last edit, whoever made it. expected_revision must be the document's current revision, as chemifree_document_snapshot reports it. Without request_id each call is a new request, and once the document has moved on, expected_revision refuses a repeat with revision_conflict. Repeating a call with the same request_id and arguments returns the first reply without applying it again, while the same server process still remembers it (the last 64 edits of an open document; a close forgets them).",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true
        )
    )]
    pub async fn chemifree_history_undo(
        &self,
        Parameters(args): Parameters<RevisionArgs>,
    ) -> CallToolResult {
        self.run_edit(revision_call("history.undo", args)).await
    }

    /// Redoes the edit that was last undone in a document, at the revision that the caller expects. The undo
    /// history is shared with the web editor, so this can redo an edit that the user undid.
    #[tool(
        description = "Redo the edit that was last undone in a document of the running Chemifree server (operation history.redo). The undo history is shared with the web editor: this redoes the last undone edit, whoever undid it. expected_revision must be the document's current revision, as chemifree_document_snapshot reports it. Without request_id each call is a new request, and once the document has moved on, expected_revision refuses a repeat with revision_conflict. Repeating a call with the same request_id and arguments returns the first reply without applying it again, while the same server process still remembers it (the last 64 edits of an open document; a close forgets them).",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true
        )
    )]
    pub async fn chemifree_history_redo(
        &self,
        Parameters(args): Parameters<RevisionArgs>,
    ) -> CallToolResult {
        self.run_edit(revision_call("history.redo", args)).await
    }

    /// Saves a document to its file, at the revision that the caller expects.
    #[tool(
        description = "Save a document of the running Chemifree server to its file (operation document.save). expected_revision must be the document's current revision, as chemifree_document_snapshot reports it. The session is shared with the web editor, so a save also commits the user's unsaved edits at that revision. Saving an unchanged document is a successful no-op. Without request_id each call is a new request, and once the document has moved on, expected_revision refuses a repeat with revision_conflict. Repeating a call with the same request_id and arguments returns the first reply without applying it again, while the same server process still remembers it (the last 64 edits of an open document; a close forgets them).",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true
        )
    )]
    pub async fn chemifree_document_save(
        &self,
        Parameters(args): Parameters<RevisionArgs>,
    ) -> CallToolResult {
        self.run_edit(revision_call("document.save", args)).await
    }

    /// Closes a document. One with unsaved changes stays open, because the tool never discards edits. The
    /// close is not idempotent: without a request_id, a repeat can close a session that was reopened after the
    /// first close.
    #[tool(
        description = "Close a document in the running Chemifree server (operation document.close). A document with unsaved changes stays open, because this tool does not discard edits: save it first. Pass a request_id if you may retry; the server replays a close with the same id instead of closing a session opened after it. Repeating a call with the same request_id and arguments returns the first reply without applying it again, while the same server process still remembers it (the last 64 edits of an open document; a close forgets them).",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    pub async fn chemifree_document_close(
        &self,
        Parameters(args): Parameters<CloseArgs>,
    ) -> CallToolResult {
        self.run_edit(close_call(args)).await
    }

    /// Runs one read operation and maps its reply, or its failure, to a tool result.
    async fn run(
        &self,
        operation: &str,
        parameters: Value,
        request_id: Option<String>,
    ) -> CallToolResult {
        match self
            .client
            .run_operation(operation, parameters, request_id)
            .await
        {
            Ok(reply) => operation_result(operation, reply),
            Err(error) => error_result(&error),
        }
    }

    /// Runs one edit. The request id is settled here, before the call, so that a failed call can name it.
    async fn run_edit(&self, call: OperationCall) -> CallToolResult {
        let request_id = match call.request_id {
            Some(id) => id,
            None => match new_request_id() {
                Ok(id) => id,
                Err(error) => return error_result(&error),
            },
        };
        match self
            .client
            .run_edit(call.operation, call.parameters, &request_id)
            .await
        {
            Ok(reply) => operation_result(call.operation, reply),
            Err(error) => edit_error_result(&error, &request_id),
        }
    }
}

#[tool_handler(
    router = self.tool_router,
    name = "chemifree",
    instructions = "Tools for a running local Chemifree server: list documents, read a snapshot and its revision, inspect the operation catalog, and create, edit (transactions, undo, redo), save and close documents. Edits need the current expected_revision; the session is shared with the open web editor."
)]
impl ServerHandler for ChemifreeServer {}

/// The tool result for an operation reply: the reply unchanged, one summary line, and `isError` when the
/// reply is a tool error (see [`operation_is_error`]).
fn operation_result(operation: &str, reply: Value) -> CallToolResult {
    let line = operation_summary(operation, &reply);
    let is_error = operation_is_error(&reply);
    tool_result(reply, line, is_error)
}

/// The tool result for a document list: the reply unchanged, and its document count.
fn documents_result(list: DocumentList) -> CallToolResult {
    let line = match list.count {
        1 => "1 document".to_owned(),
        count => format!("{count} documents"),
    };
    tool_result(list.reply, line, false)
}

/// The tool error for a failed call, with the failure's message as its only content.
fn error_result(error: &ClientError) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(error.to_string())])
}

/// The tool error for a failed edit. A call whose outcome is not known names its request id, so that the caller
/// can repeat it with the same id and learn the outcome. The client refuses such a repeat when the server has
/// restarted since the call, because a restarted server no longer remembers the id. A body refused for its size
/// was not applied, so it gets the advice to split the transaction instead. A reply too large to read has a text
/// of its own.
fn edit_error_result(error: &ClientError, request_id: &str) -> CallToolResult {
    let text = if is_too_large(error) {
        TOO_LARGE_TEXT.to_owned()
    } else if matches!(error, ClientError::ReplyTooLarge) {
        format!(
            "The reply of request {request_id} was larger than the adapter's 8 MiB limit, so its outcome cannot be shown; the operation may have been applied. Read the documents with chemifree_documents_list (a created document appears there; an edited one shows its revision) to learn whether it was applied."
        )
    } else if error.may_have_taken_effect() {
        format!(
            "{error} request_id {request_id}: the operation may have taken effect; call again with the same request_id and arguments to learn its outcome (the adapter refuses that call if the server has restarted since)."
        )
    } else {
        error.to_string()
    };
    CallToolResult::error(vec![ContentBlock::text(text)])
}

/// Whether a failed call was refused with HTTP 413, which the server answers for a body over its size limit.
fn is_too_large(error: &ClientError) -> bool {
    matches!(error, ClientError::Status { status, .. } if *status == StatusCode::PAYLOAD_TOO_LARGE)
}

fn tool_result(reply: Value, line: String, is_error: bool) -> CallToolResult {
    let mut result = if is_error {
        CallToolResult::structured_error(reply)
    } else {
        CallToolResult::structured(reply)
    };
    // The constructors put the reply's JSON text in `content`, which a client that reads only
    // `content` needs (the MCP tools specification asks for it next to `structuredContent`). The
    // summary line goes before it.
    result.content.insert(0, ContentBlock::text(line));
    result
}

/// The summary line of an operation reply: `<operation>: outcome <outcome>, revision <revision>`. The
/// outcome of a protocol error is `protocol_error`, and the revision is `n/a` when the reply has none.
#[must_use]
pub fn operation_summary(operation: &str, reply: &Value) -> String {
    let outcome = if is_protocol_error(reply) {
        "protocol_error"
    } else {
        reply
            .get("outcome")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
    };
    let revision = match reply.get("document_revision").and_then(Value::as_u64) {
        Some(revision) => revision.to_string(),
        None => "n/a".to_owned(),
    };
    format!("{operation}: outcome {outcome}, revision {revision}")
}

/// Whether an operation reply is a tool error. A protocol error is one. So is any outcome other than `ok`
/// and `partial`: `error` and `unsupported`, an outcome this adapter does not know, and a reply with no
/// outcome. `partial` is not an error, because the operation completed and its notes or losses say what is
/// missing.
#[must_use]
pub fn operation_is_error(reply: &Value) -> bool {
    if is_protocol_error(reply) {
        return true;
    }
    !matches!(
        reply.get("outcome").and_then(Value::as_str),
        Some("ok" | "partial")
    )
}

fn is_protocol_error(reply: &Value) -> bool {
    reply.get("envelope").and_then(Value::as_str) == Some("protocol_error")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::discovery::{DiscoveryError, ServerUrl};

    fn text_of(result: &CallToolResult) -> &str {
        result
            .content
            .first()
            .and_then(ContentBlock::as_text)
            .map(|text| text.text.as_str())
            .expect("the result has one text line")
    }

    /// The listed tools, each in the form that `tools/list` sends.
    fn wire_tools() -> Vec<Value> {
        ChemifreeServer::tool_router()
            .list_all()
            .iter()
            .map(|tool| serde_json::to_value(tool).expect("a listed tool serializes"))
            .collect()
    }

    /// The listed tool with this name, in the form that `tools/list` sends.
    fn wire_tool(name: &str) -> Value {
        wire_tools()
            .into_iter()
            .find(|tool| tool["name"] == name)
            .unwrap_or_else(|| panic!("the tool list has {name}"))
    }

    /// The names of the arguments that a listed tool requires.
    fn required_arguments(tool: &Value) -> Vec<&str> {
        tool["inputSchema"]["required"]
            .as_array()
            .map(|names| names.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default()
    }

    #[test]
    fn a_create_call_sends_the_title_only_when_one_is_given() {
        let untitled = create_call(CreateArgs {
            title: None,
            request_id: Some("create-1".to_owned()),
        });
        assert_eq!(untitled.operation, "document.create");
        assert_eq!(
            untitled.parameters,
            json!({}),
            "no title is sent, not a null title"
        );
        assert_eq!(untitled.request_id.as_deref(), Some("create-1"));

        let titled = create_call(CreateArgs {
            title: Some("Benzene".to_owned()),
            request_id: None,
        });
        assert_eq!(titled.parameters, json!({"title": "Benzene"}));
        assert_eq!(titled.request_id, None);
    }

    #[test]
    fn a_transaction_call_sends_exactly_the_contract_fields() {
        let call = transaction_call(TransactionArgs {
            document_id: "d1".to_owned(),
            expected_revision: 3,
            commands: vec![json!({"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}})],
            request_id: Some("tx-1".to_owned()),
        });
        assert_eq!(call.operation, "document.transaction");
        assert_eq!(
            call.parameters,
            json!({
                "document_id": "d1",
                "expected_revision": 3,
                "commands": [{"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}}]
            })
        );
        assert_eq!(call.request_id.as_deref(), Some("tx-1"));
    }

    #[test]
    fn a_revision_call_sends_the_document_and_its_revision_under_its_operation() {
        for operation in ["history.undo", "history.redo", "document.save"] {
            let call = revision_call(
                operation,
                RevisionArgs {
                    document_id: "d1".to_owned(),
                    expected_revision: 2,
                    request_id: None,
                },
            );
            assert_eq!(call.operation, operation);
            assert_eq!(
                call.parameters,
                json!({"document_id": "d1", "expected_revision": 2}),
                "{operation}"
            );
            assert_eq!(call.request_id, None);
        }
    }

    #[test]
    fn a_close_call_sends_only_the_document_id() {
        let call = close_call(CloseArgs {
            document_id: "d1".to_owned(),
            request_id: None,
        });
        assert_eq!(call.operation, "document.close");
        assert_eq!(
            call.parameters,
            json!({"document_id": "d1"}),
            "discard_unsaved and expected_revision are never sent"
        );
    }

    #[test]
    fn close_refuses_an_argument_that_would_discard_unsaved_edits() {
        let error = serde_json::from_value::<CloseArgs>(
            json!({"document_id": "d1", "discard_unsaved": true}),
        )
        .expect_err("the close tool has no discard argument");
        assert!(
            error
                .to_string()
                .contains("unknown field `discard_unsaved`"),
            "{error}"
        );
    }

    #[test]
    fn the_transaction_commands_are_an_array_of_objects_tagged_by_op() {
        let tool = wire_tool("chemifree_document_transaction");
        let commands = &tool["inputSchema"]["properties"]["commands"];
        assert_eq!(commands["type"], "array", "{commands}");
        assert_eq!(commands["items"]["type"], "object", "{commands}");
        assert_eq!(commands["items"]["required"], json!(["op"]), "{commands}");
        assert_eq!(
            commands["items"]["properties"]["op"]["type"], "string",
            "{commands}"
        );
    }

    #[test]
    fn the_edits_that_check_a_revision_require_expected_revision_and_the_others_have_none() {
        for name in [
            "chemifree_document_transaction",
            "chemifree_history_undo",
            "chemifree_history_redo",
            "chemifree_document_save",
        ] {
            let tool = wire_tool(name);
            assert!(
                required_arguments(&tool).contains(&"expected_revision"),
                "{name} requires expected_revision, got {:?}",
                required_arguments(&tool)
            );
        }
        for name in ["chemifree_document_create", "chemifree_document_close"] {
            let tool = wire_tool(name);
            assert!(
                tool["inputSchema"]["properties"]
                    .get("expected_revision")
                    .is_none(),
                "{name} has no expected_revision argument"
            );
        }
    }

    #[test]
    fn each_tool_declares_the_hints_of_its_operation() {
        // A save writes the document's file, a transaction may delete what it names, and an undo or a redo acts
        // on the undo history that the web editor shares, so these four are destructive. A repeated call with
        // the same arguments has no further effect for the four that check a revision: a repeat is refused by
        // expected_revision or replayed by request_id, and a save of an unchanged document is a no-op. A create
        // is not idempotent, and neither is a close: a repeat without a request_id makes another document, or
        // closes a session reopened after the first close.
        let expected = [
            ("chemifree_capabilities", json!({"readOnlyHint": true})),
            ("chemifree_documents_list", json!({"readOnlyHint": true})),
            ("chemifree_document_snapshot", json!({"readOnlyHint": true})),
            (
                "chemifree_document_create",
                json!({"readOnlyHint": false, "destructiveHint": false, "idempotentHint": false}),
            ),
            (
                "chemifree_document_transaction",
                json!({"readOnlyHint": false, "destructiveHint": true, "idempotentHint": true}),
            ),
            (
                "chemifree_history_undo",
                json!({"readOnlyHint": false, "destructiveHint": true, "idempotentHint": true}),
            ),
            (
                "chemifree_history_redo",
                json!({"readOnlyHint": false, "destructiveHint": true, "idempotentHint": true}),
            ),
            (
                "chemifree_document_save",
                json!({"readOnlyHint": false, "destructiveHint": true, "idempotentHint": true}),
            ),
            (
                "chemifree_document_close",
                json!({"readOnlyHint": false, "destructiveHint": false, "idempotentHint": false}),
            ),
        ];
        for (name, hints) in expected {
            assert_eq!(wire_tool(name)["annotations"], hints, "{name}");
        }
    }

    #[test]
    fn a_failed_edit_that_may_have_reached_the_server_names_its_request_id() {
        let url: ServerUrl = "http://127.0.0.1:8080".parse().expect("a loopback address");
        let failures = [
            ClientError::Transport {
                url,
                message: "connection reset".to_owned(),
            },
            ClientError::MalformedReply {
                status: StatusCode::OK,
                message: "the body is not JSON".to_owned(),
            },
            ClientError::Status {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                body: String::new(),
            },
        ];
        for failure in failures {
            let result = edit_error_result(&failure, "req-42");
            assert_eq!(result.is_error, Some(true), "{failure}");
            assert_eq!(
                text_of(&result),
                format!(
                    "{failure} request_id req-42: the operation may have taken effect; call again with the same request_id and arguments to learn its outcome (the adapter refuses that call if the server has restarted since)."
                ),
                "{failure}"
            );
        }
    }

    #[test]
    fn a_too_large_reply_to_an_edit_has_its_own_text() {
        let result = edit_error_result(&ClientError::ReplyTooLarge, "req-42");
        assert_eq!(result.is_error, Some(true));
        assert_eq!(
            text_of(&result),
            "The reply of request req-42 was larger than the adapter's 8 MiB limit, so its outcome cannot be shown; the operation may have been applied. Read the documents with chemifree_documents_list (a created document appears there; an edited one shows its revision) to learn whether it was applied."
        );
    }

    #[test]
    fn a_refused_restart_is_reported_with_its_own_text_and_no_repeat_advice() {
        let error = ClientError::ServerRestarted {
            request_id: "req-42".to_owned(),
        };
        let result = edit_error_result(&error, "req-42");
        assert_eq!(result.is_error, Some(true));
        let text = text_of(&result);
        assert_eq!(text, error.to_string());
        assert!(
            text.contains("chemifree_document_snapshot") && !text.contains("call again"),
            "a refused restart names the reads and advises no repeat, got {text}"
        );
    }

    #[test]
    fn the_size_named_in_the_edit_text_is_the_reply_limit() {
        assert_eq!(
            crate::client::MAX_REPLY_BYTES,
            8 * 1024 * 1024,
            "the text of a too large reply says 8 MiB"
        );
    }

    #[test]
    fn a_failed_edit_that_never_reached_the_server_names_no_request_id() {
        let error = ClientError::Discovery(DiscoveryError::NoServerRecorded {
            data_dir: PathBuf::from("data"),
        });
        let result = edit_error_result(&error, "req-42");
        assert_eq!(result.is_error, Some(true));
        assert_eq!(text_of(&result), error.to_string());
    }

    #[test]
    fn a_body_over_the_size_limit_is_split_advice_and_names_no_request_id() {
        let result = edit_error_result(
            &ClientError::Status {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                body: String::new(),
            },
            "req-42",
        );
        assert_eq!(result.is_error, Some(true));
        let text = text_of(&result);
        assert!(
            text.contains("256 KiB") && text.contains("Split the transaction into smaller ones"),
            "the text names the limit and the advice, got {text}"
        );
        assert!(
            !text.contains("may have taken effect") && !text.contains("req-42"),
            "a body that was refused was not applied, got {text}"
        );
    }

    #[test]
    fn ok_and_partial_are_not_tool_errors_and_the_other_outcomes_are() {
        for (outcome, is_error) in [
            ("ok", false),
            ("partial", false),
            ("error", true),
            ("unsupported", true),
            ("future_outcome", true),
        ] {
            let reply = json!({"envelope": "operation_result", "outcome": outcome});
            assert_eq!(operation_is_error(&reply), is_error, "outcome {outcome:?}");
        }
    }

    #[test]
    fn a_protocol_error_and_a_reply_without_an_outcome_are_tool_errors() {
        assert!(
            operation_is_error(&json!({"envelope": "protocol_error", "outcome": "error"})),
            "a protocol error is a tool error"
        );
        assert!(
            operation_is_error(&json!({"envelope": "operation_result"})),
            "a reply without an outcome is a tool error"
        );
    }

    #[test]
    fn summary_lines_name_the_operation_outcome_and_revision() {
        let ok = json!({"envelope": "operation_result", "outcome": "ok", "document_revision": 3});
        assert_eq!(
            operation_summary("document.snapshot", &ok),
            "document.snapshot: outcome ok, revision 3"
        );
        let unsupported = json!({
            "envelope": "operation_result",
            "outcome": "unsupported",
            "document_revision": null
        });
        assert_eq!(
            operation_summary("chem.aromaticity", &unsupported),
            "chem.aromaticity: outcome unsupported, revision n/a"
        );
        let protocol =
            json!({"envelope": "protocol_error", "outcome": "error", "document_revision": null});
        assert_eq!(
            operation_summary("document.snapshot", &protocol),
            "document.snapshot: outcome protocol_error, revision n/a"
        );
    }

    #[test]
    fn a_document_list_result_counts_its_documents_and_keeps_the_reply() {
        let reply = json!({
            "api_version": "v1",
            "documents": [{"id": "a"}, {"id": "b"}],
            "unreadable": [],
            "truncated": false
        });
        let result = documents_result(DocumentList {
            count: 2,
            reply: reply.clone(),
        });
        assert_eq!(text_of(&result), "2 documents");
        assert_eq!(result.is_error, Some(false));
        assert_eq!(result.structured_content, Some(reply));
    }

    #[test]
    fn an_operation_result_keeps_the_reply_and_flags_an_error_outcome() {
        let reply = json!({
            "envelope": "operation_result",
            "outcome": "error",
            "document_revision": 1,
            "diagnostics": [{"code": "example"}]
        });
        let result = operation_result("document.transaction", reply.clone());
        assert_eq!(result.is_error, Some(true));
        assert_eq!(result.structured_content, Some(reply.clone()));
        assert_eq!(
            text_of(&result),
            "document.transaction: outcome error, revision 1"
        );
        // A client that reads only `content` still gets the reply, diagnostics included.
        let json_text = result
            .content
            .get(1)
            .and_then(ContentBlock::as_text)
            .map(|text| text.text.as_str())
            .expect("the second content block is the reply as JSON text");
        assert_eq!(
            serde_json::from_str::<Value>(json_text).expect("the text is JSON"),
            reply
        );
    }

    #[test]
    fn a_partial_outcome_is_a_normal_result_that_names_its_outcome() {
        let reply = json!({
            "envelope": "operation_result",
            "outcome": "partial",
            "document_revision": 2,
            "diagnostics": [{"code": "example"}]
        });
        let result = operation_result("document.transaction", reply.clone());
        assert_eq!(result.is_error, Some(false));
        assert_eq!(
            text_of(&result),
            "document.transaction: outcome partial, revision 2"
        );
        assert_eq!(result.structured_content, Some(reply));
    }

    #[test]
    fn a_list_of_one_document_says_document() {
        let result = documents_result(DocumentList {
            count: 1,
            reply: json!({"documents": [{"id": "a"}]}),
        });
        assert_eq!(text_of(&result), "1 document");
    }

    #[test]
    fn a_failed_call_is_a_tool_error_with_its_message() {
        let result = error_result(&ClientError::ReplyTooLarge);
        assert_eq!(result.is_error, Some(true));
        assert_eq!(result.structured_content, None);
        assert_eq!(text_of(&result), ClientError::ReplyTooLarge.to_string());
    }
}
