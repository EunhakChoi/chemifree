//! The analysis and export tools of the MCP adapter against a live API server. Each one returns the reply that the
//! HTTP API gives for the same operation, changes nothing in the document, and passes its failures through as tool
//! errors. A test also checks that every executable operation of the catalog is either a tool or named in
//! `NOT_EXPOSED` with its reason.
//!
//! The tools are called directly on a `ChemifreeServer`, with their arguments as JSON, which is how an MCP client
//! sends them. The HTTP helper makes the same operation calls that the CLI and the web editor make.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use chemifree_mcp::analysis::NOT_EXPOSED;
use chemifree_mcp::client::ApiClient;
use chemifree_mcp::discovery::{Discovery, ServerUrl};
use chemifree_mcp::tools::ChemifreeServer;
use common::{Http, TestServer, create_document};
use hyper::{Method, StatusCode};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

/// A server whose tools answer through a client of the server at `url`.
fn tools_for(url: &str) -> ChemifreeServer {
    let url: ServerUrl = url.parse().expect("the test address is a loopback address");
    ChemifreeServer::new(ApiClient::new(Discovery::new(Some(url), None)))
}

/// A tool's arguments, as the JSON object that an MCP client sends.
fn arguments<T: DeserializeOwned>(value: Value) -> Parameters<T> {
    Parameters(
        serde_json::from_value(value).expect("the arguments read as the tool's argument type"),
    )
}

/// The reply of an operation result, which is its structured content.
fn reply_of(result: &CallToolResult) -> &Value {
    result
        .structured_content
        .as_ref()
        .expect("an operation result carries its reply")
}

/// An operation call in the form that the HTTP API takes. The test writes it itself rather than using the
/// adapter's, so that the two sides are compared independently.
fn envelope(request_id: &str, operation: &str, parameters: Value) -> Value {
    json!({
        "contract": 1,
        "request_id": request_id,
        "profile": "preserve-semantics",
        "operation": operation,
        "parameters": parameters,
    })
}

/// One operation call to the HTTP API, with its status and reply.
async fn http_operation(
    http: &Http,
    request_id: &str,
    operation: &str,
    parameters: Value,
) -> (StatusCode, Value) {
    http.request(
        Method::POST,
        "/api/v1/operations",
        Some(&envelope(request_id, operation, parameters)),
    )
    .await
}

/// Asserts that a tool's result is the reply that the HTTP API gives for the same operation, request id and
/// parameters, and that the tool error flag is set exactly when the outcome is neither `ok` nor `partial`. Returns
/// the HTTP reply.
async fn assert_same_as_http(
    http: &Http,
    result: &CallToolResult,
    request_id: &str,
    operation: &str,
    parameters: Value,
) -> Value {
    let (status, http_reply) = http_operation(http, request_id, operation, parameters).await;
    assert!(
        status == StatusCode::OK || status == StatusCode::BAD_REQUEST,
        "{operation}: the HTTP API answers 200 or 400, got {status}: {http_reply}"
    );
    assert_eq!(
        reply_of(result),
        &http_reply,
        "{operation}: the tool's reply is the HTTP reply"
    );
    let outcome = http_reply["outcome"].as_str();
    let is_error = !matches!(outcome, Some("ok" | "partial"));
    assert_eq!(
        result.is_error,
        Some(is_error),
        "{operation}: outcome {outcome:?} is a tool error exactly when it is neither ok nor partial"
    );
    http_reply
}

/// Asserts that a reply completed its operation: an outcome of `ok` or `partial`.
fn assert_completed(reply: &Value, operation: &str) {
    assert!(
        matches!(reply["outcome"].as_str(), Some("ok" | "partial")),
        "{operation} completes, got {reply}"
    );
}

/// The command that adds one carbon atom at the origin.
fn add_carbon() -> Value {
    json!({"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}})
}

/// The commands that draw a benzene ring in Kekulé form and one stereocenter. The center is a carbon with a stored
/// tetrahedral configuration, three neighbours and one implicit hydrogen. `$N` names the command at index N.
fn drawing_commands() -> Vec<Value> {
    let ring = [
        (40, 0),
        (20, 35),
        (-20, 35),
        (-40, 0),
        (-20, -35),
        (20, -35),
    ];
    let mut commands: Vec<Value> = ring
        .iter()
        .map(|&(x, y)| json!({"op": "add_atom", "element": "C", "position": {"x": x, "y": y}}))
        .collect();
    for index in 0..6 {
        let order = if index % 2 == 0 { "double" } else { "single" };
        commands.push(json!({
            "op": "add_bond",
            "begin": format!("${index}"),
            "end": format!("${}", (index + 1) % 6),
            "order": order
        }));
    }
    // Indices 12 to 16: the center, three substituents bonded to it, and its configuration.
    commands.push(json!({"op": "add_atom", "element": "C", "position": {"x": 120, "y": 0}}));
    commands.push(json!({
        "op": "add_atom", "element": "Br", "position": {"x": 150, "y": 0},
        "bond_to": {"atom": "$12", "order": "single"}
    }));
    commands.push(json!({
        "op": "add_atom", "element": "Cl", "position": {"x": 120, "y": 30},
        "bond_to": {"atom": "$12", "order": "single"}
    }));
    commands.push(json!({
        "op": "add_atom", "element": "F", "position": {"x": 90, "y": 0},
        "bond_to": {"atom": "$12", "order": "single"}
    }));
    commands.push(json!({
        "op": "set_atom_configuration",
        "atom": "$12",
        "configuration": {
            "kind": "tetrahedral",
            "neighbors": ["$13", "$14", "$15", "implicit_hydrogen"],
            "turn": "clockwise"
        }
    }));
    commands
}

/// A new document with one transaction of `commands` applied to it, as the web editor applies a drawing. The
/// edit is unsaved. Returns the document's id.
async fn draw(http: &Http, name: &str, commands: Vec<Value>) -> String {
    let document_id = create_document(http, &format!("{name}-create")).await;
    let body = json!({
        "request_id": format!("{name}-draw"),
        "expected_revision": 0,
        "commands": commands,
    });
    let (status, reply) = http
        .request(
            Method::POST,
            &format!("/api/v1/documents/{document_id}/transactions"),
            Some(&body),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "drawing {name} was refused: {reply}"
    );
    assert_eq!(
        reply["document"]["revision"], 1,
        "drawing {name} did not apply: {reply}"
    );
    document_id
}

/// The snapshot of a document, read through the HTTP API.
async fn snapshot_of(http: &Http, document_id: &str, request_id: &str) -> Value {
    let (status, reply) = http_operation(
        http,
        request_id,
        "document.snapshot",
        json!({"document_id": document_id}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["outcome"], "ok", "{reply}");
    reply
}

/// The arguments of a tool call that carry `request_id`. The HTTP call that the test compares with the tool's
/// reply has the same request id, because the reply repeats it.
fn with_request_id(parameters: &Value, request_id: &str) -> Value {
    let mut arguments = parameters.clone();
    arguments["request_id"] = json!(request_id);
    arguments
}

/// Runs an export through the tool and the HTTP API, and completes it as a client does. An export that would drop
/// content is first refused with a `loss_consent_required` diagnostic, which gives the digest of its losses. The
/// export is then sent again with that digest in `accepted_losses`, and that call completes. Returns the completed
/// reply.
async fn export_with_consent(
    tools: &ChemifreeServer,
    http: &Http,
    request_id: &str,
    parameters: Value,
) -> Value {
    let refused = tools
        .chemifree_document_export(arguments(with_request_id(&parameters, request_id)))
        .await;
    let refusal = assert_same_as_http(
        http,
        &refused,
        request_id,
        "document.export",
        parameters.clone(),
    )
    .await;
    assert_eq!(
        refusal["diagnostics"][0]["code"], "loss_consent_required",
        "{request_id}: the export names its losses before it is accepted: {refusal}"
    );
    let digest = refusal["diagnostics"][0]["details"]["loss_digest"]
        .as_str()
        .expect("the refusal gives the digest of its losses");
    let mut accepted = parameters;
    accepted["accepted_losses"] = json!(digest);
    let accepted_id = format!("{request_id}-accepted");
    let result = tools
        .chemifree_document_export(arguments(with_request_id(&accepted, &accepted_id)))
        .await;
    let reply = assert_same_as_http(http, &result, &accepted_id, "document.export", accepted).await;
    assert_completed(&reply, request_id);
    reply
}

#[tokio::test(flavor = "multi_thread")]
async fn each_analysis_and_export_gives_the_reply_of_the_http_api() {
    let server = TestServer::start().await;
    let http = server.http();
    let tools = tools_for(&server.url);
    let document = draw(&http, "parity", drawing_commands()).await;
    let query = draw(&http, "parity-query", vec![add_carbon()]).await;

    let result = tools
        .chemifree_chem_validate(arguments(
            json!({"document_id": document, "request_id": "p-validate"}),
        ))
        .await;
    let reply = assert_same_as_http(
        &http,
        &result,
        "p-validate",
        "chem.validate",
        json!({"document_id": document}),
    )
    .await;
    assert_completed(&reply, "chem.validate");

    let result = tools
        .chemifree_chem_aromaticity(arguments(
            json!({"document_id": document, "request_id": "p-arom"}),
        ))
        .await;
    let reply = assert_same_as_http(
        &http,
        &result,
        "p-arom",
        "chem.aromaticity",
        json!({"document_id": document}),
    )
    .await;
    assert_completed(&reply, "chem.aromaticity");

    let result = tools
        .chemifree_chem_identity(arguments(json!({
            "document_id": document,
            "mode": "chemical",
            "other_document_id": document,
            "request_id": "p-identity"
        })))
        .await;
    let reply = assert_same_as_http(
        &http,
        &result,
        "p-identity",
        "chem.identity",
        json!({"document_id": document, "mode": "chemical", "other_document_id": document}),
    )
    .await;
    assert_completed(&reply, "chem.identity");

    let result = tools
        .chemifree_chem_substructure(arguments(json!({
            "query_document_id": query,
            "target_document_id": document,
            "request_id": "p-substructure"
        })))
        .await;
    let reply = assert_same_as_http(
        &http,
        &result,
        "p-substructure",
        "chem.substructure",
        json!({"query_document_id": query, "target_document_id": document}),
    )
    .await;
    assert_completed(&reply, "chem.substructure");

    let result = tools
        .chemifree_chem_stereo(arguments(
            json!({"document_id": document, "request_id": "p-stereo"}),
        ))
        .await;
    let reply = assert_same_as_http(
        &http,
        &result,
        "p-stereo",
        "chem.stereo",
        json!({"document_id": document}),
    )
    .await;
    assert_completed(&reply, "chem.stereo");

    let result = tools
        .chemifree_chem_cip(arguments(
            json!({"document_id": document, "request_id": "p-cip"}),
        ))
        .await;
    let reply = assert_same_as_http(
        &http,
        &result,
        "p-cip",
        "chem.cip",
        json!({"document_id": document}),
    )
    .await;
    assert_completed(&reply, "chem.cip");

    let result = tools
        .chemifree_chem_inchi(arguments(
            json!({"document_id": document, "request_id": "p-inchi"}),
        ))
        .await;
    let reply = assert_same_as_http(
        &http,
        &result,
        "p-inchi",
        "chem.inchi",
        json!({"document_id": document}),
    )
    .await;
    assert_eq!(
        reply["outcome"], "unsupported",
        "no InChI provider is registered in the test server: {reply}"
    );

    let result = tools
        .chemifree_chem_properties(arguments(json!({
            "document_id": document,
            "scope": "molecule",
            "property": "formula",
            "request_id": "p-properties"
        })))
        .await;
    let reply = assert_same_as_http(
        &http,
        &result,
        "p-properties",
        "chem.properties",
        json!({"document_id": document, "scope": "molecule", "property": "formula"}),
    )
    .await;
    assert_completed(&reply, "chem.properties");

    let result = tools
        .chemifree_document_export(arguments(json!({
            "format": "smiles",
            "document_id": document,
            "style": "recorded",
            "request_id": "p-smiles"
        })))
        .await;
    let reply = assert_same_as_http(
        &http,
        &result,
        "p-smiles",
        "document.export",
        json!({"format": "smiles", "document_id": document, "style": "recorded"}),
    )
    .await;
    assert_completed(&reply, "document.export (smiles)");

    // SVG does not draw the stated stereo configuration, so the export is refused until its losses are accepted.
    export_with_consent(
        &tools,
        &http,
        "p-svg",
        json!({"format": "svg", "document_id": document}),
    )
    .await;

    // CDXML writes the drawing and not the stored stereo configuration, so it is refused until its losses are accepted.
    export_with_consent(
        &tools,
        &http,
        "p-cdxml",
        json!({"format": "cdxml", "document_id": document}),
    )
    .await;

    // A MOL file states stereo only through its marks, so the stored configuration is refused until its losses are accepted.
    export_with_consent(
        &tools,
        &http,
        "p-mol",
        json!({"format": "mol", "records": [{"document_id": document}]}),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn no_analysis_or_export_changes_the_revision_or_the_unsaved_state() {
    let server = TestServer::start().await;
    let http = server.http();
    let tools = tools_for(&server.url);
    let document = draw(&http, "unchanged", drawing_commands()).await;
    let before = snapshot_of(&http, &document, "u-before").await;
    assert_eq!(
        before["result"]["session"]["unsaved_changes"], true,
        "the document has an unsaved edit before the calls, so their effect on it is what is checked: {before}"
    );

    tools
        .chemifree_chem_validate(arguments(json!({"document_id": document})))
        .await;
    tools
        .chemifree_chem_aromaticity(arguments(json!({"document_id": document})))
        .await;
    tools
        .chemifree_chem_identity(arguments(
            json!({"document_id": document, "mode": "recorded"}),
        ))
        .await;
    tools
        .chemifree_chem_stereo(arguments(json!({"document_id": document})))
        .await;
    tools
        .chemifree_chem_cip(arguments(json!({"document_id": document})))
        .await;
    tools
        .chemifree_chem_properties(arguments(json!({
            "document_id": document,
            "scope": "molecule",
            "property": "formula"
        })))
        .await;
    tools
        .chemifree_document_export(arguments(json!({"format": "svg", "document_id": document})))
        .await;
    tools
        .chemifree_document_export(arguments(json!({
            "format": "smiles",
            "document_id": document,
            "style": "canonical"
        })))
        .await;

    let after = snapshot_of(&http, &document, "u-after").await;
    assert_eq!(
        after["document_revision"], before["document_revision"],
        "the revision is unchanged"
    );
    assert_eq!(
        after["result"]["session"]["unsaved_changes"],
        before["result"]["session"]["unsaved_changes"],
        "the unsaved state is unchanged"
    );
    assert_eq!(
        after["result"]["document"], before["result"]["document"],
        "the document is unchanged"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_analysis_or_export_is_a_tool_error_with_the_http_reply() {
    let server = TestServer::start().await;
    let http = server.http();
    let tools = tools_for(&server.url);
    let document = draw(&http, "failures", drawing_commands()).await;

    // An unknown document is an error reply.
    let result = tools
        .chemifree_chem_validate(arguments(json!({
            "document_id": "no-such-document",
            "request_id": "f-unknown"
        })))
        .await;
    let reply = assert_same_as_http(
        &http,
        &result,
        "f-unknown",
        "chem.validate",
        json!({"document_id": "no-such-document"}),
    )
    .await;
    assert_eq!(reply["outcome"], "error", "{reply}");

    // Without a registered InChI provider the operation is unsupported, which is a tool error too.
    let result = tools
        .chemifree_chem_inchi(arguments(
            json!({"document_id": document, "request_id": "f-inchi"}),
        ))
        .await;
    let reply = assert_same_as_http(
        &http,
        &result,
        "f-inchi",
        "chem.inchi",
        json!({"document_id": document}),
    )
    .await;
    assert_eq!(reply["outcome"], "unsupported", "{reply}");

    // An identity mode other than `recorded` or `chemical` is refused by the server, with its parameter path.
    let result = tools
        .chemifree_chem_identity(arguments(json!({
            "document_id": document,
            "mode": "sideways",
            "request_id": "f-mode"
        })))
        .await;
    let reply = assert_same_as_http(
        &http,
        &result,
        "f-mode",
        "chem.identity",
        json!({"document_id": document, "mode": "sideways"}),
    )
    .await;
    assert_eq!(reply["outcome"], "error", "{reply}");
    assert_eq!(
        reply["diagnostics"][0]["code"], "invalid_request",
        "{reply}"
    );
    // An unknown variant is reported against the whole parameter object, not against `mode`.
    assert_eq!(
        reply["diagnostics"][0]["path"], "parameters",
        "the refusal gives the path of the bad parameter: {reply}"
    );

    // A MOL export names its documents in records, so a document_id beside them is refused.
    let result = tools
        .chemifree_document_export(arguments(json!({
            "format": "mol",
            "document_id": document,
            "records": [{"document_id": document}],
            "request_id": "f-mol"
        })))
        .await;
    let reply = assert_same_as_http(
        &http,
        &result,
        "f-mol",
        "document.export",
        json!({"format": "mol", "document_id": document, "records": [{"document_id": document}]}),
    )
    .await;
    assert_eq!(reply["outcome"], "error", "{reply}");

    // A time limit of 0 is always exceeded, and the computation is stopped with `time_limit`.
    let result = tools
        .chemifree_chem_aromaticity(arguments(json!({
            "document_id": document,
            "time_limit_ms": 0,
            "request_id": "f-time"
        })))
        .await;
    let reply = assert_same_as_http(
        &http,
        &result,
        "f-time",
        "chem.aromaticity",
        json!({"document_id": document, "time_limit_ms": 0}),
    )
    .await;
    assert_eq!(reply["outcome"], "error", "{reply}");
    assert_eq!(reply["diagnostics"][0]["code"], "time_limit", "{reply}");
}

/// The tool name that the naming rule gives an operation: `chemifree_` and the operation's id with each `.` as `_`.
/// `capabilities.inspect` is the one exception, and it is `chemifree_capabilities`.
fn tool_name_of(operation: &str) -> String {
    match operation {
        "capabilities.inspect" => "chemifree_capabilities".to_owned(),
        _ => format!("chemifree_{}", operation.replace('.', "_")),
    }
}

/// The listed tool that is not an operation: the document list, which is a REST read.
const NOT_AN_OPERATION: &str = "chemifree_documents_list";

/// The operations that have a tool and are executable only when a provider is registered. The test server
/// registers none, so these must not be executable here.
const PROVIDER_ONLY: [&str; 1] = ["chem.inchi"];

#[tokio::test(flavor = "multi_thread")]
async fn every_executable_operation_is_a_tool_or_named_with_its_reason() {
    let server = TestServer::start().await;
    let http = server.http();
    let (status, catalog_reply) =
        http_operation(&http, "coverage-catalog", "capabilities.inspect", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{catalog_reply}");
    let catalog: BTreeMap<String, String> = catalog_reply["result"]["operations"]
        .as_array()
        .expect("the catalog lists its operations")
        .iter()
        .map(|operation| {
            (
                operation["id"]
                    .as_str()
                    .expect("each operation has an id")
                    .to_owned(),
                operation["execution"]
                    .as_str()
                    .expect("each operation has an execution state")
                    .to_owned(),
            )
        })
        .collect();
    let executable: BTreeSet<String> = catalog
        .iter()
        .filter(|(_, execution)| execution.as_str() == "executable")
        .map(|(operation, _)| operation.clone())
        .collect();

    // Each listed tool, except the document list, is the tool of one catalog operation under the naming rule.
    let mut tool_operations: BTreeSet<String> = BTreeSet::new();
    for tool in tools_for(&server.url).listed_tools() {
        let name = tool.name.as_ref();
        if name == NOT_AN_OPERATION {
            continue;
        }
        let operation = catalog
            .keys()
            .find(|operation| tool_name_of(operation) == name)
            .unwrap_or_else(|| panic!("the tool {name} names no operation of the catalog"));
        tool_operations.insert(operation.clone());
    }
    for operation in PROVIDER_ONLY {
        assert!(
            tool_operations.contains(operation),
            "{operation} has a tool"
        );
        assert_ne!(
            catalog[operation], "executable",
            "{operation} is executable only with a provider, and the test server has none"
        );
    }
    let without_provider_only: BTreeSet<String> = tool_operations
        .iter()
        .filter(|operation| !PROVIDER_ONLY.contains(&operation.as_str()))
        .cloned()
        .collect();

    let named: BTreeSet<String> = NOT_EXPOSED
        .iter()
        .map(|(operation, _)| operation.to_string())
        .collect();
    assert_eq!(
        named.len(),
        NOT_EXPOSED.len(),
        "an operation is named twice in NOT_EXPOSED"
    );
    assert!(
        named.is_disjoint(&tool_operations),
        "an operation is both a tool and named in NOT_EXPOSED: {:?}",
        named.intersection(&tool_operations).collect::<Vec<_>>()
    );
    for operation in &named {
        assert!(
            catalog.contains_key(operation),
            "{operation} is named in NOT_EXPOSED but is not an operation of the catalog"
        );
    }

    let expected: BTreeSet<String> = without_provider_only.union(&named).cloned().collect();
    let missing: Vec<&String> = executable.difference(&expected).collect();
    let extra: Vec<&String> = expected.difference(&executable).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "executable operations without a tool or a NOT_EXPOSED reason: {missing:?}; \
         tools or NOT_EXPOSED entries that the catalog does not mark executable: {extra:?}"
    );
}
