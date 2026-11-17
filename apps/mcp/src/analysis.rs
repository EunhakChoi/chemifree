//! The analysis and export tools: the read-only `chem.*` operations and `document.export`. Each tool is one
//! operation of the operation contract. Its arguments become the operation's parameters, and its reply goes out
//! unchanged, as the other tools' replies do (see `tools`). None of them changes a document, and each result
//! describes the document's current snapshot, which includes unsaved edits.
//!
//! [`NOT_EXPOSED`] names each executable operation that no tool offers, with its reason. The coverage test in
//! `tests/analysis_tools.rs` requires the tools and that list together to cover the executable operations of the
//! catalog exactly, so a new operation cannot be left out silently.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::tools::{ChemifreeServer, OperationCall};

/// The executable operations of the catalog that no tool offers, each with its reason. Together with the tools, this
/// list names every operation that the catalog marks executable.
pub const NOT_EXPOSED: [(&str, &str); 15] = [
    (
        "document.selection_scope",
        "The movement scope of an editor selection; the adapter keeps no selection state and exposes no selection tool.",
    ),
    (
        "document.arrangement_plan",
        "A plan to align or distribute selected editor objects; the adapter exposes no selection or arrangement tool.",
    ),
    (
        "document.transform_plan",
        "A rotation or reflection plan for an editor selection; the adapter exposes no selection or transform tool.",
    ),
    (
        "document.clipboard_copy",
        "Copies a graph fragment into the editor clipboard; the adapter has no clipboard and exposes no clipboard tool.",
    ),
    (
        "document.clipboard_paste",
        "Pastes a clipboard graph fragment into a document; the adapter has no clipboard and exposes no clipboard tool.",
    ),
    (
        "document.clipboard_scene_copy",
        "Copies graphic-only scene objects into the editor clipboard; the adapter exposes no clipboard tool.",
    ),
    (
        "document.clipboard_scene_paste",
        "Pastes graphic-only scene objects from the editor clipboard; the adapter exposes no clipboard tool.",
    ),
    (
        "document.template_inspect",
        "Validates a template for the editor's template placement; the adapter exposes no template tool.",
    ),
    (
        "document.template_plan",
        "Previews attaching a template in the editor; the adapter exposes no template tool.",
    ),
    (
        "document.template_attach",
        "Attaches a template through one bond, as an undoable edit; the adapter exposes no template tool.",
    ),
    (
        "document.template_replace_plan",
        "Previews replacing a terminal atom with a template anchor; the adapter exposes no template tool.",
    ),
    (
        "document.template_replace",
        "Replaces a terminal atom with a template anchor, as an edit; the adapter exposes no template tool.",
    ),
    (
        "document.import",
        "Creates a document from file text, with consent for reported losses; the adapter has no import tool.",
    ),
    (
        "chem.aromaticity_convert",
        "Converts aromatic and Kekule bonds as one undoable edit; the adapter's edit tools do not include it.",
    ),
    (
        "layout.structure",
        "Previews or applies 2D coordinates; apply is an undoable edit, and the adapter exposes no layout tool.",
    ),
];

/// The arguments of `chemifree_chem_validate`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChemValidateArgs {
    /// The document's id, as `chemifree_documents_list` lists it.
    pub document_id: String,
    /// An optional request id for this call. A new one is made when it is omitted. An inspection is not replayed:
    /// a repeat with the same id inspects the document as it is then.
    pub request_id: Option<String>,
}

/// The arguments of `chemifree_chem_aromaticity`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChemAromaticityArgs {
    /// The document's id, as `chemifree_documents_list` lists it.
    pub document_id: String,
    /// The time limit of this analysis in milliseconds: an integer, where 0 is always exceeded and returns a
    /// `time_limit` error. Omitted, the server's default of 5000 applies; above the server's maximum of 8000 the
    /// call is refused.
    pub time_limit_ms: Option<u64>,
    /// An optional request id for this call. A new one is made when it is omitted. An inspection is not replayed:
    /// a repeat with the same id inspects the document as it is then.
    pub request_id: Option<String>,
}

/// The arguments of `chemifree_chem_identity`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChemIdentityArgs {
    /// The document's id, as `chemifree_documents_list` lists it.
    pub document_id: String,
    /// The identity model, required: `recorded` compares the labeled graph as recorded, and `chemical` compares the
    /// chemistry under the named aromaticity and valence policies. Any other value is refused.
    pub mode: String,
    /// A second document to compare with `document_id`. Omitted, only the one graph is examined.
    pub other_document_id: Option<String>,
    /// The time limit of this analysis in milliseconds: an integer, where 0 is always exceeded and returns a
    /// `time_limit` error. Omitted, the server's default of 5000 applies; above the server's maximum of 8000 the
    /// call is refused.
    pub time_limit_ms: Option<u64>,
    /// An optional request id for this call. A new one is made when it is omitted. An inspection is not replayed:
    /// a repeat with the same id inspects the document as it is then.
    pub request_id: Option<String>,
}

/// The arguments of `chemifree_chem_substructure`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChemSubstructureArgs {
    /// The query drawing, as a document id.
    pub query_document_id: String,
    /// The drawing that the query is searched in, as a document id.
    pub target_document_id: String,
    /// The time limit of this analysis in milliseconds: an integer, where 0 is always exceeded and returns a
    /// `time_limit` error. Omitted, the server's default of 5000 applies; above the server's maximum of 8000 the
    /// call is refused.
    pub time_limit_ms: Option<u64>,
    /// An optional request id for this call. A new one is made when it is omitted. An inspection is not replayed:
    /// a repeat with the same id inspects the documents as they are then.
    pub request_id: Option<String>,
}

/// The arguments of `chemifree_chem_stereo`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChemStereoArgs {
    /// The document's id, as `chemifree_documents_list` lists it.
    pub document_id: String,
    /// The scene instance to inspect, by its identifier. It may be omitted only when the document has exactly one
    /// scene instance; otherwise the call is refused.
    pub instance: Option<String>,
    /// The time limit of this analysis in milliseconds: an integer, where 0 is always exceeded and returns a
    /// `time_limit` error. Omitted, the server's default of 5000 applies; above the server's maximum of 8000 the
    /// call is refused.
    pub time_limit_ms: Option<u64>,
    /// An optional request id for this call. A new one is made when it is omitted. An inspection is not replayed:
    /// a repeat with the same id inspects the document as it is then.
    pub request_id: Option<String>,
}

/// The arguments of `chemifree_chem_cip`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChemCipArgs {
    /// The document's id, as `chemifree_documents_list` lists it.
    pub document_id: String,
    /// Which items to list: `stored` (the stored configurations), `stereogenic`, or `named` (only the atoms and bonds
    /// in `atoms` and `bonds`). Omitted, it is `stored`, or `named` when `atoms` or `bonds` is given; naming items
    /// without a scope lists only those.
    pub scope: Option<String>,
    /// The atoms to list, by identifier, as the document lists them.
    pub atoms: Option<Vec<String>>,
    /// The double bonds to list, by identifier, as the document lists them.
    pub bonds: Option<Vec<String>>,
    /// The time limit of this analysis in milliseconds: an integer, where 0 is always exceeded and returns a
    /// `time_limit` error. Omitted, the server's default of 5000 applies; above the server's maximum of 8000 the
    /// call is refused.
    pub time_limit_ms: Option<u64>,
    /// An optional request id for this call. A new one is made when it is omitted. An inspection is not replayed:
    /// a repeat with the same id inspects the document as it is then.
    pub request_id: Option<String>,
}

/// The arguments of `chemifree_chem_inchi`. The tool needs an InChI provider that the server was started with;
/// without one the call is unsupported.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChemInchiArgs {
    /// The document's id, as `chemifree_documents_list` lists it.
    pub document_id: String,
    /// The `loss_digest` of an earlier refusal, which accepts the losses that the refusal showed.
    pub accepted_losses: Option<String>,
    /// The time limit of this analysis in milliseconds: an integer, where 0 is always exceeded and returns a
    /// `time_limit` error. Omitted, the server's default of 5000 applies; above the server's maximum of 8000 the
    /// call is refused.
    pub time_limit_ms: Option<u64>,
    /// An optional request id for this call. A new one is made when it is omitted. An inspection is not replayed:
    /// a repeat with the same id inspects the document as it is then.
    pub request_id: Option<String>,
}

/// The arguments of `chemifree_chem_properties`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChemPropertiesArgs {
    /// The document's id, as `chemifree_documents_list` lists it.
    pub document_id: String,
    /// The scope, required: `molecule`, `selection` or `components`.
    pub scope: String,
    /// The property, required: `formula`, `masses` or `distribution`. Masses and distributions need a checked data
    /// registration, given as `data_registration_id`.
    pub property: String,
    /// The items of a `selection` scope, as an object with the optional arrays `objects`, `atoms` and `bonds` of
    /// identifiers. It is sent as given.
    pub selection: Option<Map<String, Value>>,
    /// The data registration that masses and distributions use.
    pub data_registration_id: Option<String>,
    /// The time limit of this analysis in milliseconds: an integer, where 0 is always exceeded and returns a
    /// `time_limit` error. Omitted, the server's default of 5000 applies; above the server's maximum of 8000 the
    /// call is refused.
    pub time_limit_ms: Option<u64>,
    /// An optional request id for this call. A new one is made when it is omitted. An inspection is not replayed:
    /// a repeat with the same id inspects the document as it is then.
    pub request_id: Option<String>,
}

/// The arguments of `chemifree_document_export`. Each format takes its own fields, and the server refuses a field
/// that its format does not take.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentExportArgs {
    /// The format, required: `smiles`, `svg`, `cdxml`, `cdx`, `mol` or `sdf`.
    pub format: String,
    /// The document to export, for `smiles`, `svg`, `cdxml` and `cdx`.
    pub document_id: Option<String>,
    /// The documents to write, for `mol` and `sdf`: each an object with `document_id` and optionally `fields`, as
    /// docs/CONTRACT.md describes them under MOL and SDF. It is sent as given.
    pub records: Option<Vec<Map<String, Value>>>,
    /// The SMILES style, required for `smiles`: `recorded` or `canonical`.
    pub style: Option<String>,
    /// The MOL/SDF dialect for `mol` and `sdf`: `v2000` (the default), `v3000` or `auto`.
    pub ctab: Option<String>,
    /// The `loss_digest` of an earlier refusal, which accepts the losses that the refusal showed.
    pub accepted_losses: Option<String>,
    /// The time limit of a `smiles` export in milliseconds: an integer, where 0 is always exceeded and returns a
    /// `time_limit` error. Omitted, the server's default of 5000 applies; above the server's maximum of 8000 the
    /// call is refused. Other formats refuse this field.
    pub time_limit_ms: Option<u64>,
    /// An optional request id for this call. A new one is made when it is omitted. An export is not replayed: a
    /// repeat with the same id exports the document as it is then.
    pub request_id: Option<String>,
}

/// The parameters of one call: each field that has a value, under its name. A field without a value is left out,
/// so the parameters name only what the caller gave and the server applies its own default for the rest.
#[must_use]
fn parameters(fields: Vec<(&'static str, Option<Value>)>) -> Value {
    let object: Map<String, Value> = fields
        .into_iter()
        .filter_map(|(name, value)| value.map(|value| (name.to_owned(), value)))
        .collect();
    Value::Object(object)
}

/// The call of `chem.validate`.
#[must_use]
fn validate_call(args: ChemValidateArgs) -> OperationCall {
    OperationCall {
        operation: "chem.validate",
        parameters: parameters(vec![("document_id", Some(Value::from(args.document_id)))]),
        request_id: args.request_id,
    }
}

/// The call of `chem.aromaticity`.
#[must_use]
fn aromaticity_call(args: ChemAromaticityArgs) -> OperationCall {
    OperationCall {
        operation: "chem.aromaticity",
        parameters: parameters(vec![
            ("document_id", Some(Value::from(args.document_id))),
            ("time_limit_ms", args.time_limit_ms.map(Value::from)),
        ]),
        request_id: args.request_id,
    }
}

/// The call of `chem.identity`.
#[must_use]
fn identity_call(args: ChemIdentityArgs) -> OperationCall {
    OperationCall {
        operation: "chem.identity",
        parameters: parameters(vec![
            ("document_id", Some(Value::from(args.document_id))),
            ("mode", Some(Value::from(args.mode))),
            ("other_document_id", args.other_document_id.map(Value::from)),
            ("time_limit_ms", args.time_limit_ms.map(Value::from)),
        ]),
        request_id: args.request_id,
    }
}

/// The call of `chem.substructure`.
#[must_use]
fn substructure_call(args: ChemSubstructureArgs) -> OperationCall {
    OperationCall {
        operation: "chem.substructure",
        parameters: parameters(vec![
            (
                "query_document_id",
                Some(Value::from(args.query_document_id)),
            ),
            (
                "target_document_id",
                Some(Value::from(args.target_document_id)),
            ),
            ("time_limit_ms", args.time_limit_ms.map(Value::from)),
        ]),
        request_id: args.request_id,
    }
}

/// The call of `chem.stereo`.
#[must_use]
fn stereo_call(args: ChemStereoArgs) -> OperationCall {
    OperationCall {
        operation: "chem.stereo",
        parameters: parameters(vec![
            ("document_id", Some(Value::from(args.document_id))),
            ("instance", args.instance.map(Value::from)),
            ("time_limit_ms", args.time_limit_ms.map(Value::from)),
        ]),
        request_id: args.request_id,
    }
}

/// The call of `chem.cip`.
#[must_use]
fn cip_call(args: ChemCipArgs) -> OperationCall {
    OperationCall {
        operation: "chem.cip",
        parameters: parameters(vec![
            ("document_id", Some(Value::from(args.document_id))),
            ("scope", args.scope.map(Value::from)),
            ("atoms", args.atoms.map(Value::from)),
            ("bonds", args.bonds.map(Value::from)),
            ("time_limit_ms", args.time_limit_ms.map(Value::from)),
        ]),
        request_id: args.request_id,
    }
}

/// The call of `chem.inchi`.
#[must_use]
fn inchi_call(args: ChemInchiArgs) -> OperationCall {
    OperationCall {
        operation: "chem.inchi",
        parameters: parameters(vec![
            ("document_id", Some(Value::from(args.document_id))),
            ("accepted_losses", args.accepted_losses.map(Value::from)),
            ("time_limit_ms", args.time_limit_ms.map(Value::from)),
        ]),
        request_id: args.request_id,
    }
}

/// The call of `chem.properties`.
#[must_use]
fn properties_call(args: ChemPropertiesArgs) -> OperationCall {
    OperationCall {
        operation: "chem.properties",
        parameters: parameters(vec![
            ("document_id", Some(Value::from(args.document_id))),
            ("scope", Some(Value::from(args.scope))),
            ("property", Some(Value::from(args.property))),
            ("selection", args.selection.map(Value::Object)),
            (
                "data_registration_id",
                args.data_registration_id.map(Value::from),
            ),
            ("time_limit_ms", args.time_limit_ms.map(Value::from)),
        ]),
        request_id: args.request_id,
    }
}

/// The call of `document.export`: the format and the fields that it takes, and no others.
#[must_use]
fn export_call(args: DocumentExportArgs) -> OperationCall {
    let records = args
        .records
        .map(|records| Value::Array(records.into_iter().map(Value::Object).collect()));
    OperationCall {
        operation: "document.export",
        parameters: parameters(vec![
            ("format", Some(Value::from(args.format))),
            ("document_id", args.document_id.map(Value::from)),
            ("records", records),
            ("style", args.style.map(Value::from)),
            ("ctab", args.ctab.map(Value::from)),
            ("accepted_losses", args.accepted_losses.map(Value::from)),
            ("time_limit_ms", args.time_limit_ms.map(Value::from)),
        ]),
        request_id: args.request_id,
    }
}

#[tool_router(router = analysis_router, vis = "pub(crate)")]
impl ChemifreeServer {
    #[tool(
        description = "Check the valences of a document of the running Chemifree server (operation chem.validate): one assessment per atom, under the valence model chemifree-valence-3. Read-only: nothing is changed. The result describes the current snapshot, which includes unsaved edits.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    pub async fn chemifree_chem_validate(
        &self,
        Parameters(args): Parameters<ChemValidateArgs>,
    ) -> CallToolResult {
        let call = validate_call(args);
        self.run(call.operation, call.parameters, call.request_id)
            .await
    }

    #[tool(
        description = "Find the aromatic cyclic systems of a document of the running Chemifree server (operation chem.aromaticity), under the named aromaticity policy. A structure that the policy does not cover is reported as unsupported. Read-only: nothing is changed. The result describes the current snapshot, which includes unsaved edits.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    pub async fn chemifree_chem_aromaticity(
        &self,
        Parameters(args): Parameters<ChemAromaticityArgs>,
    ) -> CallToolResult {
        let call = aromaticity_call(args);
        self.run(call.operation, call.parameters, call.request_id)
            .await
    }

    #[tool(
        description = "Examine the graph identity of a document of the running Chemifree server (operation chem.identity): a canonical code and symmetry groups under the named model and, with other_document_id, whether two graphs are equivalent. Neither model reads stereo. Read-only: nothing is changed. The result describes the current snapshot, which includes unsaved edits.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    pub async fn chemifree_chem_identity(
        &self,
        Parameters(args): Parameters<ChemIdentityArgs>,
    ) -> CallToolResult {
        let call = identity_call(args);
        self.run(call.operation, call.parameters, call.request_id)
            .await
    }

    #[tool(
        description = "Map the drawing of a query document into a target document's drawing (operation chem.substructure): every atom and bond mapping, or a refusal or a limit. Read-only: nothing is changed; both results describe the current snapshots, which include unsaved edits.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    pub async fn chemifree_chem_substructure(
        &self,
        Parameters(args): Parameters<ChemSubstructureArgs>,
    ) -> CallToolResult {
        let call = substructure_call(args);
        self.run(call.operation, call.parameters, call.request_id)
            .await
    }

    #[tool(
        description = "Compare the stored stereo configurations of a document of the running Chemifree server with what each scene instance's drawing shows (operation chem.stereo), per stereocenter and double bond. The comparison is advisory: an item that is not listed is not known to be fine. Read-only: nothing is changed. The result describes the current snapshot, which includes unsaved edits.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    pub async fn chemifree_chem_stereo(
        &self,
        Parameters(args): Parameters<ChemStereoArgs>,
    ) -> CallToolResult {
        let call = stereo_call(args);
        self.run(call.operation, call.parameters, call.request_id)
            .await
    }

    #[tool(
        description = "List the CIP descriptors (R, S, E, Z) of a document of the running Chemifree server (operation chem.cip), with the reason for each item that is not labeled. Read-only: nothing is changed. The result describes the current snapshot, which includes unsaved edits.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    pub async fn chemifree_chem_cip(
        &self,
        Parameters(args): Parameters<ChemCipArgs>,
    ) -> CallToolResult {
        let call = cip_call(args);
        self.run(call.operation, call.parameters, call.request_id)
            .await
    }

    #[tool(
        description = "Generate the standard InChI and InChIKey of a document of the running Chemifree server (operation chem.inchi). It needs an InChI provider that the server was started with; without one the server answers unsupported, which is a tool error. Losses that need consent come back as an error reply with a loss_digest; call again with accepted_losses set to that digest. Read-only: nothing is changed. The result describes the current snapshot, which includes unsaved edits.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    pub async fn chemifree_chem_inchi(
        &self,
        Parameters(args): Parameters<ChemInchiArgs>,
    ) -> CallToolResult {
        let call = inchi_call(args);
        self.run(call.operation, call.parameters, call.request_id)
            .await
    }

    #[tool(
        description = "Compute the formula, a selected mass, or an isotope distribution of a document of the running Chemifree server (operation chem.properties). Masses and distributions need a checked data registration. Read-only: nothing is changed. The result describes the current snapshot, which includes unsaved edits.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    pub async fn chemifree_chem_properties(
        &self,
        Parameters(args): Parameters<ChemPropertiesArgs>,
    ) -> CallToolResult {
        let call = properties_call(args);
        self.run(call.operation, call.parameters, call.request_id)
            .await
    }

    #[tool(
        description = "Export a document of the running Chemifree server as text (operation document.export): SMILES (format smiles, with style recorded or canonical), SVG (svg), CDXML or CDX (cdxml, cdx) for one document, or MOL or SDF (mol, sdf) from records. Read-only: nothing is saved or changed, and the export describes the current snapshot, which includes unsaved edits. Losses that need consent come back as an error reply with a loss_digest and no content; call again with the same arguments and accepted_losses set to that digest.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    pub async fn chemifree_document_export(
        &self,
        Parameters(args): Parameters<DocumentExportArgs>,
    ) -> CallToolResult {
        let call = export_call(args);
        self.run(call.operation, call.parameters, call.request_id)
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde::de::DeserializeOwned;
    use serde_json::json;

    use super::*;

    /// The operations that the tools of this module offer.
    const EXPOSED: [&str; 9] = [
        "chem.validate",
        "chem.aromaticity",
        "chem.identity",
        "chem.substructure",
        "chem.stereo",
        "chem.cip",
        "chem.inchi",
        "chem.properties",
        "document.export",
    ];

    /// Arguments as the MCP client sends them: a JSON object that the tool's argument type reads.
    fn arguments<T: DeserializeOwned>(value: Value) -> T {
        serde_json::from_value(value).expect("the arguments read as the tool's argument type")
    }

    #[test]
    fn each_call_names_its_catalog_operation() {
        let operations = [
            validate_call(arguments(json!({"document_id": "d1"}))).operation,
            aromaticity_call(arguments(json!({"document_id": "d1"}))).operation,
            identity_call(arguments(json!({"document_id": "d1", "mode": "chemical"}))).operation,
            substructure_call(arguments(
                json!({"query_document_id": "q1", "target_document_id": "t1"}),
            ))
            .operation,
            stereo_call(arguments(json!({"document_id": "d1"}))).operation,
            cip_call(arguments(json!({"document_id": "d1"}))).operation,
            inchi_call(arguments(json!({"document_id": "d1"}))).operation,
            properties_call(arguments(
                json!({"document_id": "d1", "scope": "molecule", "property": "formula"}),
            ))
            .operation,
            export_call(arguments(json!({"format": "svg", "document_id": "d1"}))).operation,
        ];
        assert_eq!(
            operations,
            [
                "chem.validate",
                "chem.aromaticity",
                "chem.identity",
                "chem.substructure",
                "chem.stereo",
                "chem.cip",
                "chem.inchi",
                "chem.properties",
                "document.export",
            ]
        );
    }

    #[test]
    fn a_call_with_only_its_required_fields_sends_no_optional_key() {
        // An absent optional field is left out, never sent as null: the server reads a missing field as its default.
        assert_eq!(
            validate_call(arguments(json!({"document_id": "d1"}))).parameters,
            json!({"document_id": "d1"})
        );
        assert_eq!(
            aromaticity_call(arguments(json!({"document_id": "d1"}))).parameters,
            json!({"document_id": "d1"})
        );
        assert_eq!(
            identity_call(arguments(json!({"document_id": "d1", "mode": "recorded"}))).parameters,
            json!({"document_id": "d1", "mode": "recorded"})
        );
        assert_eq!(
            substructure_call(arguments(
                json!({"query_document_id": "q1", "target_document_id": "t1"}),
            ))
            .parameters,
            json!({"query_document_id": "q1", "target_document_id": "t1"})
        );
        assert_eq!(
            stereo_call(arguments(json!({"document_id": "d1"}))).parameters,
            json!({"document_id": "d1"})
        );
        assert_eq!(
            cip_call(arguments(json!({"document_id": "d1"}))).parameters,
            json!({"document_id": "d1"})
        );
        assert_eq!(
            inchi_call(arguments(json!({"document_id": "d1"}))).parameters,
            json!({"document_id": "d1"})
        );
        assert_eq!(
            properties_call(arguments(
                json!({"document_id": "d1", "scope": "molecule", "property": "formula"}),
            ))
            .parameters,
            json!({"document_id": "d1", "scope": "molecule", "property": "formula"})
        );
        assert_eq!(
            export_call(arguments(json!({"format": "svg", "document_id": "d1"}))).parameters,
            json!({"format": "svg", "document_id": "d1"})
        );
    }

    #[test]
    fn a_call_with_every_optional_field_sends_each_under_its_name() {
        assert_eq!(
            aromaticity_call(arguments(
                json!({"document_id": "d1", "time_limit_ms": 250})
            ))
            .parameters,
            json!({"document_id": "d1", "time_limit_ms": 250})
        );
        assert_eq!(
            identity_call(arguments(json!({
                "document_id": "d1",
                "mode": "chemical",
                "other_document_id": "d2",
                "time_limit_ms": 0
            })))
            .parameters,
            json!({"document_id": "d1", "mode": "chemical", "other_document_id": "d2", "time_limit_ms": 0})
        );
        assert_eq!(
            stereo_call(arguments(
                json!({"document_id": "d1", "instance": "i1", "time_limit_ms": 9})
            ))
            .parameters,
            json!({"document_id": "d1", "instance": "i1", "time_limit_ms": 9})
        );
        assert_eq!(
            cip_call(arguments(json!({
                "document_id": "d1",
                "scope": "named",
                "atoms": ["a1", "a2"],
                "bonds": ["b1"],
                "time_limit_ms": 100
            })))
            .parameters,
            json!({
                "document_id": "d1",
                "scope": "named",
                "atoms": ["a1", "a2"],
                "bonds": ["b1"],
                "time_limit_ms": 100
            })
        );
        assert_eq!(
            inchi_call(arguments(
                json!({"document_id": "d1", "accepted_losses": "digest", "time_limit_ms": 100})
            ))
            .parameters,
            json!({"document_id": "d1", "accepted_losses": "digest", "time_limit_ms": 100})
        );
        assert_eq!(
            properties_call(arguments(json!({
                "document_id": "d1",
                "scope": "selection",
                "property": "masses",
                "selection": {"atoms": ["a1"]},
                "data_registration_id": "reg-1",
                "time_limit_ms": 100
            })))
            .parameters,
            json!({
                "document_id": "d1",
                "scope": "selection",
                "property": "masses",
                "selection": {"atoms": ["a1"]},
                "data_registration_id": "reg-1",
                "time_limit_ms": 100
            })
        );
        assert_eq!(
            export_call(arguments(json!({
                "format": "smiles",
                "document_id": "d1",
                "style": "canonical",
                "accepted_losses": "digest",
                "time_limit_ms": 100
            })))
            .parameters,
            json!({
                "format": "smiles",
                "document_id": "d1",
                "style": "canonical",
                "accepted_losses": "digest",
                "time_limit_ms": 100
            })
        );
    }

    #[test]
    fn a_selection_and_records_are_sent_exactly_as_given() {
        // The server checks the members of a selection and the fields of a record, so the tool passes them through,
        // unknown members included.
        let selection = json!({"objects": ["o1"], "atoms": ["a1"], "unknown_member": true});
        assert_eq!(
            properties_call(arguments(json!({
                "document_id": "d1",
                "scope": "selection",
                "property": "formula",
                "selection": selection
            })))
            .parameters["selection"],
            selection
        );
        let records = json!([
            {"document_id": "d1"},
            {"document_id": "d2", "fields": [{"name": "Name", "value": "x"}]}
        ]);
        assert_eq!(
            export_call(arguments(json!({"format": "mol", "records": records}))).parameters["records"],
            records
        );
    }

    #[test]
    fn a_request_id_is_kept_for_the_call_when_given_and_absent_otherwise() {
        assert_eq!(
            validate_call(arguments(json!({"document_id": "d1", "request_id": "r-1"}))).request_id,
            Some("r-1".to_owned())
        );
        assert_eq!(
            validate_call(arguments(json!({"document_id": "d1"}))).request_id,
            None
        );
    }

    #[test]
    fn an_argument_that_the_contract_does_not_name_is_refused() {
        // A field of another operation is not an argument of this one.
        let error = serde_json::from_value::<ChemValidateArgs>(json!({
            "document_id": "d1",
            "time_limit_ms": 5
        }))
        .expect_err("validate takes no time limit");
        assert!(
            error.to_string().contains("unknown field `time_limit_ms`"),
            "{error}"
        );
    }

    #[test]
    fn not_exposed_names_each_operation_once_with_a_reason_and_no_exposed_operation() {
        let names: BTreeSet<&str> = NOT_EXPOSED.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names.len(),
            NOT_EXPOSED.len(),
            "an operation is named twice in NOT_EXPOSED"
        );
        for (name, reason) in NOT_EXPOSED {
            assert!(!reason.is_empty(), "{name} has no reason");
            assert!(
                !EXPOSED.contains(&name),
                "{name} is exposed by a tool, so it is not a non-exposed operation"
            );
        }
    }
}
