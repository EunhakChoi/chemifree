# P82 S5: MCP analysis and export tools

P82 stays `in_progress`. Planner decisions taken (the user asked to follow the planner's recommendations): S5 comes before S3 and S4; no MCP-only summary/detail split for large results; `chem.properties.selection` is exposed as the operation's input; one export tool for all formats; no generic operation tool.

## Scope

- Nine read-only tools in `apps/mcp/src/analysis.rs`, merged into the server's tool router: `chemifree_chem_validate`, `chemifree_chem_aromaticity`, `chemifree_chem_identity`, `chemifree_chem_substructure`, `chemifree_chem_stereo`, `chemifree_chem_cip`, `chemifree_chem_inchi`, `chemifree_chem_properties`, `chemifree_document_export`. Arguments are hand-written copies of the operation parameter structs, typed at the JSON level; the server checks values; optional arguments are omitted, never null. They use the read path (no uncertain-edit tracking).
- `NOT_EXPOSED`: 15 executable operations with a reason each (selection scope, arrangement and transform plans, four clipboard operations, five template operations, import, aromatic conversion, layout).
- The live test harness builds the product application with its codecs (`chemifree_host::local_application`), so export runs as shipped.
- No server or application code changes.

## Acceptance mapping (this slice)

| P82 acceptance | Evidence |
|---|---|
| Analysis and export added per engine | `apps/mcp/tests/analysis_tools.rs` `every_executable_operation_is_a_tool_or_named_with_its_reason`: the server's executable operations equal the tools (by name rule, read from the router) plus `NOT_EXPOSED`; `chem.inchi` has a tool and is not executable without a provider |
| Same policy as HTTP/CLI | `each_analysis_and_export_gives_the_reply_of_the_http_api`: each of the nine tools gives the reply of `POST /api/v1/operations` with the same request id (exports of a stereo document through the `accepted_losses` round trip); `a_failed_analysis_or_export_is_a_tool_error_with_the_http_reply` (unknown document, `chem.inchi` unsupported, bad identity mode, MOL with both `document_id` and `records`, `time_limit_ms: 0`) |
| Large results | the server's per-operation bounds and the full reply, as HTTP and the CLI; the adapter's 8 MiB bound is transport safety only (CONTRACT, Results) |
| Real MCP client | `scripts/mcp-client-check.mjs`: 18 tools with hints; validate, formula, SMILES and SVG export equal the HTTP replies for the same request id; `chem.inchi` is a tool error with the server's `unsupported` reply; the document's revision and unsaved state are unchanged |

## Checks run

- `cargo test -p chemifree-mcp`: 81 passed (44 unit, 4 analysis tools, 8 edit tools, 22 live server, 3 stdio).
- `npm run test:mcp -- --profile debug`: passed.
- Mutations caught (Rust): an absent optional sent as null; `chem.inchi` not read-only; `layout.structure` missing from `NOT_EXPOSED`.
- Mutations caught (client check): a missing tool name; `chem.inchi` not an error; broken export parity.

## Limits

- Argument values are not typed beyond JSON types (enums and formats are the server's to check) until S4 generates schemas.
- An unknown identity mode is reported at path `parameters` (serde's report), not `parameters.mode`, as over HTTP.
- No MCP resources, no rich-tree CDXML/CDX export, no edit operations of `NOT_EXPOSED`.
- An export's text appears twice in an MCP message (`structuredContent` and the JSON text in `content`).
