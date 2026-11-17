//! The read-only MCP tools, and how their replies become MCP results.
//!
//! The tools read the running server: its operation catalog, its document list, and one document
//! snapshot with its revision, which includes unsaved edits. A reply goes out as `structured_content`,
//! unchanged, with one text line that names the operation's outcome and revision. An operation reply
//! whose outcome is not `ok`, a protocol error, and any failure of the call are tool errors
//! (`isError: true`). Their diagnostics pass through unchanged, and nothing about the chemistry is
//! interpreted here.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::client::{ApiClient, ClientError, DocumentList};

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

/// The MCP server: three read-only tools over one [`ApiClient`].
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

    /// Runs one operation and maps its reply, or its failure, to a tool result.
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
}

#[tool_handler(
    router = self.tool_router,
    name = "chemifree",
    instructions = "Read-only tools for a running local Chemifree server: list documents, read a document snapshot and its revision, and inspect the operation catalog."
)]
impl ServerHandler for ChemifreeServer {}

/// The tool result for an operation reply: the reply unchanged, one summary line, and `isError` when
/// the outcome is not `ok` or the reply is a protocol error.
fn operation_result(operation: &str, reply: Value) -> CallToolResult {
    let line = operation_summary(operation, &reply);
    let is_error = operation_is_error(&reply);
    tool_result(reply, line, is_error)
}

/// The tool result for a document list: the reply unchanged, and its document count.
fn documents_result(list: DocumentList) -> CallToolResult {
    let line = format!("{} documents", list.count);
    tool_result(list.reply, line, false)
}

/// The tool error for a failed call, with the failure's message as its only content.
fn error_result(error: &ClientError) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(error.to_string())])
}

fn tool_result(reply: Value, line: String, is_error: bool) -> CallToolResult {
    let mut result = if is_error {
        CallToolResult::structured_error(reply)
    } else {
        CallToolResult::structured(reply)
    };
    // The constructors also put the JSON text in `content`. The summary line replaces it, because the
    // reply itself is already in `structured_content`.
    result.content = vec![ContentBlock::text(line)];
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

/// Whether an operation reply is a tool error: a protocol error, or any outcome other than `ok`. A reply
/// without an outcome is a tool error too.
#[must_use]
pub fn operation_is_error(reply: &Value) -> bool {
    is_protocol_error(reply) || reply.get("outcome").and_then(Value::as_str) != Some("ok")
}

fn is_protocol_error(reply: &Value) -> bool {
    reply.get("envelope").and_then(Value::as_str) == Some("protocol_error")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(result: &CallToolResult) -> &str {
        result
            .content
            .first()
            .and_then(ContentBlock::as_text)
            .map(|text| text.text.as_str())
            .expect("the result has one text line")
    }

    #[test]
    fn only_an_ok_outcome_is_not_a_tool_error() {
        for (outcome, is_error) in [
            ("ok", false),
            ("partial", true),
            ("error", true),
            ("unsupported", true),
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
    fn an_operation_result_keeps_the_reply_and_flags_a_non_ok_outcome() {
        let reply = json!({
            "envelope": "operation_result",
            "outcome": "partial",
            "document_revision": 1,
            "diagnostics": [{"code": "example"}]
        });
        let result = operation_result("document.snapshot", reply.clone());
        assert_eq!(result.is_error, Some(true));
        assert_eq!(result.structured_content, Some(reply));
        assert_eq!(
            text_of(&result),
            "document.snapshot: outcome partial, revision 1"
        );
    }

    #[test]
    fn a_failed_call_is_a_tool_error_with_its_message() {
        let result = error_result(&ClientError::ReplyTooLarge);
        assert_eq!(result.is_error, Some(true));
        assert_eq!(result.structured_content, None);
        assert_eq!(text_of(&result), ClientError::ReplyTooLarge.to_string());
    }
}
