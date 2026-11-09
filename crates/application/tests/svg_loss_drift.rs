//! The SVG mutation table (P38a2). From one base drawing, each row applies one mutation through a
//! transaction and reads the export back. A drawn row changes the SVG content. A not-drawn row
//! leaves the content byte-identical. Every row must have exactly the loss features it names, and
//! the features that need consent must be exactly the ones the refusal lists. A failure names the
//! row whose content, losses or consent differ from the table.
//!
//! `svg_losses` is crate-private, so the losses are read through `document.export`. A refusal lists
//! the features that need consent together with the digest that accepts them, and the same export
//! accepted with that digest returns the content and the loss report, which lists every feature.
//! Each row runs in an application of its own, so no row sees another row's document or requests.

use std::path::Path;

use chemifree_application::Application;
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::png_image::EmbeddedPngDecoder;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, OpenSmilesCodec, V2000Codec};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

/// The 1 x 1 PNG that the image tests embed.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DQAAAEgQGALFXOsAAAAABJRU5ErkJggg==";

/// The features of a drawing that states nothing beyond its atoms and bonds.
const PLAIN: &[&str] = &["chemical_graph"];
/// No feature.
const NONE: &[&str] = &[];

/// One row of the table.
struct Row {
    /// The mutation's name, which is also its request ID.
    name: &'static str,
    /// The transaction parameters that apply the mutation: its `commands`, and for the selection
    /// row a `selection_after`.
    transaction: Value,
    /// Whether the mutation changes the SVG content.
    drawn: bool,
    /// The loss features after the mutation, `chemical_graph` included.
    features: &'static [&'static str],
    /// The features among `features` whose losses need consent.
    consent: &'static [&'static str],
}

/// A row whose mutation changes the content and adds no loss.
fn drawn_row(name: &'static str, commands: Value) -> Row {
    Row {
        name,
        transaction: json!({"commands": commands}),
        drawn: true,
        features: PLAIN,
        consent: NONE,
    }
}

/// A row whose mutation keeps the content byte-identical and adds the named losses.
fn dropped_row(
    name: &'static str,
    commands: Value,
    features: &'static [&'static str],
    consent: &'static [&'static str],
) -> Row {
    Row {
        name,
        transaction: json!({"commands": commands}),
        drawn: false,
        features,
        consent,
    }
}

/// The base drawing, with atoms a1 to a7 and bonds b1 to b6 numbered in creation order: a1-a2 (b1),
/// a2-a3 (b2), a2-a4 (b3), a4-a5 (b4), a5-a6 (b5), and a3=a7 (b6). The oxygen a4 carries the ethyl
/// a5-a6, and a2 has three heavy neighbors, so a tetrahedral statement on it is valid. The base
/// states no feature.
fn base_transaction() -> Value {
    json!({"commands": [
        {"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}},
        {"op": "add_atom", "element": "C", "position": {"x": 26, "y": -15},
            "bond_to": {"atom": "a1", "order": "single"}},
        {"op": "add_atom", "element": "C", "position": {"x": 52, "y": 0},
            "bond_to": {"atom": "a2", "order": "single"}},
        {"op": "add_atom", "element": "O", "position": {"x": 26, "y": 15},
            "bond_to": {"atom": "a2", "order": "single"}},
        {"op": "add_atom", "element": "C", "position": {"x": 26, "y": 45},
            "bond_to": {"atom": "a4", "order": "single"}},
        {"op": "add_atom", "element": "C", "position": {"x": 52, "y": 60},
            "bond_to": {"atom": "a5", "order": "single"}},
        {"op": "add_atom", "element": "C", "position": {"x": 78, "y": -15},
            "bond_to": {"atom": "a3", "order": "double"}}
    ]})
}

/// The rows, in the order the report lists them.
///
/// The radical and attachment-point rows on the oxygen check that a state the label does not show
/// leaves the content alone. The `radical-on-carbon` and `attachment-point-on-carbon` rows use a
/// bonded bare carbon: a recorded radical or attachment point makes the renderer label that carbon,
/// so their content changes. The `bold-on-bare-carbons` row changes the oxygen to a carbon, so no
/// atom is labelled and bold reaches no drawn text.
///
/// No row sets a crossed mark on a single bond. `set_bond_mark` and `update_bond` both validate the
/// bond and refuse a mark that does not fit its order (`stereo_mark_bond_order`), so no transaction
/// command can state one. Import routes were not checked.
fn table() -> Vec<Row> {
    vec![
        // Drawn: the content changes and no loss appears.
        drawn_row(
            "charge",
            json!([{"op": "update_atom", "atom": "a4", "charge": 1}]),
        ),
        drawn_row(
            "h-count",
            json!([{"op": "update_atom", "atom": "a1",
                "hydrogens": {"mode": "specified", "count": 3}}]),
        ),
        drawn_row(
            "isotope",
            json!([{"op": "update_atom", "atom": "a4", "isotope": 18}]),
        ),
        drawn_row(
            "bond-order",
            json!([{"op": "update_bond", "bond": "b2", "order": "double"}]),
        ),
        drawn_row(
            "wedge",
            json!([{"op": "set_bond_mark", "bond": "b2",
                "mark": {"kind": "wedge", "narrow": "begin"}}]),
        ),
        drawn_row(
            "hash",
            json!([{"op": "set_bond_mark", "bond": "b2",
                "mark": {"kind": "hash", "narrow": "end"}}]),
        ),
        drawn_row(
            "wavy",
            json!([{"op": "set_bond_mark", "bond": "b2", "mark": {"kind": "wavy"}}]),
        ),
        // A crossed mark on a double bond is drawn with crossing lines, so the content changes and
        // no stereo_mark loss appears.
        drawn_row(
            "crossed-double",
            json!([{"op": "set_bond_mark", "bond": "b6", "mark": {"kind": "crossed"}}]),
        ),
        drawn_row(
            "line-width",
            json!([{"op": "update_settings", "line_width_pt": 2.0}]),
        ),
        drawn_row(
            "label-font",
            json!([{"op": "update_settings", "label_font": "serif"}]),
        ),
        drawn_row(
            "label-size",
            json!([{"op": "update_settings", "label_size_pt": 16.0}]),
        ),
        drawn_row(
            "carbon-labels",
            json!([{"op": "update_settings", "carbon_labels": "all"}]),
        ),
        drawn_row(
            "atom-move",
            json!([{"op": "move_atoms", "atoms": ["a3"], "delta": {"x": 0, "y": 5}}]),
        ),
        // Scene graphics and images are drawn, so each changes the content and adds no loss.
        drawn_row(
            "graphic",
            json!([{"op": "add_graphic", "shape": "line",
                "points": [{"x": 0, "y": 70}, {"x": 20, "y": 70}]}]),
        ),
        drawn_row(
            "image",
            json!([{"op": "add_image", "mime_type": "image/png", "data_base64": PNG,
                "origin": {"x": 0, "y": 80}, "size_pt": {"x": 10, "y": 10}}]),
        ),
        drawn_row(
            "graphic-rectangle",
            json!([{"op": "add_graphic", "shape": "rectangle",
                "points": [{"x": 0, "y": 70}, {"x": 20, "y": 90}]}]),
        ),
        drawn_row(
            "graphic-ellipse",
            json!([{"op": "add_graphic", "shape": "ellipse",
                "points": [{"x": 0, "y": 70}, {"x": 20, "y": 90}]}]),
        ),
        drawn_row(
            "graphic-arrow",
            json!([{"op": "add_graphic", "shape": "arrow",
                "points": [{"x": 0, "y": 70}, {"x": 20, "y": 70}]}]),
        ),
        drawn_row(
            "graphic-polygon",
            json!([{"op": "add_polygon",
                "vertices": [{"x": 0, "y": 70}, {"x": 20, "y": 70}, {"x": 10, "y": 90}]}]),
        ),
        drawn_row(
            "graphic-spline",
            json!([{"op": "add_spline",
                "points": [{"x": 0, "y": 70}, {"x": 10, "y": 60}, {"x": 20, "y": 80},
                    {"x": 30, "y": 70}]}]),
        ),
        // The row shows that the width command is accepted and drawn. It does not compare the width
        // with the default, since a drawn row is compared with the base drawing, which has no graphic.
        drawn_row(
            "graphic-line-width",
            json!([
                {"op": "add_graphic", "shape": "line",
                    "points": [{"x": 0, "y": 70}, {"x": 20, "y": 70}]},
                {"op": "set_graphic_line_width", "object": "s2", "line_width_pt": 4.0}
            ]),
        ),
        // Not drawn: the content is byte-identical and the named losses appear.
        dropped_row(
            "radical",
            json!([{"op": "update_atom", "atom": "a4", "radical": "doublet"}]),
            &["chemical_graph", "radical_mark"],
            &["radical_mark"],
        ),
        dropped_row(
            "attachment-point",
            json!([{"op": "update_atom", "atom": "a4", "attachment_points": [1]}]),
            &["chemical_graph", "attachment_point_mark"],
            &["attachment_point_mark"],
        ),
        // A caption with a character outside the drawn set is not drawn, so the content is
        // byte-identical and only the caption loss appears. The character is 漢 (U+6F22).
        dropped_row(
            "caption",
            json!([{"op": "add_caption", "anchor": {"x": 0, "y": 60},
                "runs": [{"text": "漢", "script": "baseline"}]}]),
            &["chemical_graph", "caption"],
            &["caption"],
        ),
        // A drawable caption with an underline is drawn, so the content changes. The underline is
        // not drawn, so it is the only loss the caption adds, and it needs consent.
        Row {
            name: "caption-underline",
            transaction: json!({"commands": [
                {"op": "add_caption", "anchor": {"x": 0, "y": 60},
                    "runs": [{"text": "H", "script": "baseline", "underline": true}]}
            ]}),
            drawn: true,
            features: &["chemical_graph", "caption_underline"],
            consent: &["caption_underline"],
        },
        // A group needs at least two distinct objects sharing a parent, and the base has only the
        // instance, so two captions are added first. Both use characters outside the drawn set (漢
        // and 字), so neither is drawn. A drawn caption would change the content by itself, which
        // would hide whether the group changes it. The captions are the loss that needs consent;
        // the group adds only scene_group, which needs none. Graphics are not used here, for the
        // same reason: a drawn graphic changes the content by itself.
        dropped_row(
            "group",
            json!([
                {"op": "add_caption", "anchor": {"x": 0, "y": 70},
                    "runs": [{"text": "漢", "script": "baseline"}]},
                {"op": "add_caption", "anchor": {"x": 0, "y": 80},
                    "runs": [{"text": "字", "script": "baseline"}]},
                {"op": "group", "objects": ["s2", "s3"]}
            ]),
            &["chemical_graph", "caption", "scene_group"],
            &["caption"],
        ),
        dropped_row(
            "stereo-configuration",
            json!([{"op": "set_atom_configuration", "atom": "a2",
                "configuration": {"kind": "tetrahedral",
                    "neighbors": ["a1", "a3", "a4", "implicit_hydrogen"],
                    "turn": "clockwise"}}]),
            &["chemical_graph", "stereo_configuration"],
            &["stereo_configuration"],
        ),
        // An enhanced group needs a stored tetrahedral center, so this row also states one.
        dropped_row(
            "stereo-group",
            json!([
                {"op": "set_atom_configuration", "atom": "a2",
                    "configuration": {"kind": "tetrahedral",
                        "neighbors": ["a1", "a3", "a4", "implicit_hydrogen"],
                        "turn": "clockwise"}},
                {"op": "create_stereo_group", "kind": "absolute", "atoms": ["a2"],
                    "number": null}
            ]),
            &["chemical_graph", "stereo_configuration", "stereo_group"],
            &["stereo_configuration", "stereo_group"],
        ),
        dropped_row(
            "bold",
            json!([{"op": "update_settings", "label_weight": "bold"}]),
            &["chemical_graph", "label_typography"],
            &["label_typography"],
        ),
        dropped_row(
            "italic",
            json!([{"op": "update_settings", "label_style": "italic"}]),
            &["chemical_graph", "label_typography"],
            &["label_typography"],
        ),
        dropped_row(
            "underline",
            json!([{"op": "update_settings", "label_underline": true}]),
            &["chemical_graph", "label_typography"],
            &["label_typography"],
        ),
        dropped_row(
            "outline",
            json!([{"op": "update_settings", "label_outline": true}]),
            &["chemical_graph", "label_typography"],
            &["label_typography"],
        ),
        dropped_row(
            "shadow",
            json!([{"op": "update_settings", "label_shadow": true}]),
            &["chemical_graph", "label_typography"],
            &["label_typography"],
        ),
        dropped_row(
            "ethyl-abbreviation",
            json!([{"op": "collapse_ethyl", "instance": "s1", "anchor": "a5", "terminal": "a6"}]),
            &["chemical_graph", "abbreviation_display"],
            &["abbreviation_display"],
        ),
        dropped_row(
            "fragment-display",
            json!([{"op": "contract_fragment", "instance": "s1", "anchor": "a5",
                "members": ["a5", "a6"], "text": "C2H5"}]),
            &["chemical_graph", "abbreviation_display"],
            &["abbreviation_display"],
        ),
        // A transaction needs a command, and an empty list is refused, so the selection rides with
        // a display-unit change: the unit is neither drawn nor a loss.
        Row {
            name: "selection",
            transaction: json!({
                "commands": [{"op": "update_settings", "display_unit": "mm"}],
                "selection_after": {"objects": [], "atoms": ["a1"], "bonds": []}
            }),
            drawn: false,
            features: &["chemical_graph", "selection"],
            consent: NONE,
        },
        dropped_row(
            "title",
            json!([{"op": "rename", "title": "Drift fixture"}]),
            &["chemical_graph", "title"],
            NONE,
        ),
        dropped_row(
            "scene-reference",
            json!([{"op": "add_reference", "source": "s1",
                "target": {"kind": "atom", "id": "a1"}}]),
            &["chemical_graph", "scene_references"],
            NONE,
        ),
        // A query bond is drawn as a dotted line, so the content changes. Its expression is
        // reported without consent, since the canvas shows the same dotted line.
        Row {
            name: "query-bond",
            transaction: json!({"commands": [
                {"op": "update_bond", "bond": "b1", "order": "query",
                    "query": {"kind": "list", "orders": ["single", "double"]}}
            ]}),
            drawn: true,
            features: &["chemical_graph", "query_bond_expression"],
            consent: NONE,
        },
        // Moving a1 by (26, -15) puts it on its neighbor a2 at (26, -15), so the bond b1 has no
        // length and is not drawn. The content changes, and the loss needs no consent.
        Row {
            name: "atom-onto-neighbor",
            transaction: json!({"commands": [
                {"op": "move_atoms", "atoms": ["a1"], "delta": {"x": 26, "y": -15}}
            ]}),
            drawn: true,
            features: &["chemical_graph", "zero_length_bond"],
            consent: NONE,
        },
        // Label-rule rows: a bonded bare carbon with a recorded radical or attachment point shows
        // its label, so the content changes.
        Row {
            name: "radical-on-carbon",
            transaction: json!({"commands": [
                {"op": "update_atom", "atom": "a1", "radical": "doublet"}
            ]}),
            drawn: true,
            features: &["chemical_graph", "radical_mark"],
            consent: &["radical_mark"],
        },
        Row {
            name: "attachment-point-on-carbon",
            transaction: json!({"commands": [
                {"op": "update_atom", "atom": "a1", "attachment_points": [1]}
            ]}),
            drawn: true,
            features: &["chemical_graph", "attachment_point_mark"],
            consent: &["attachment_point_mark"],
        },
        // The oxygen of the base becomes a carbon, so every atom is a bare carbon and no label is
        // drawn. Bold reaches no drawn text, so no label_typography loss appears. The content
        // changes, because the oxygen label is gone.
        drawn_row(
            "bold-on-bare-carbons",
            json!([
                {"op": "update_atom", "atom": "a4", "element": "C"},
                {"op": "update_settings", "label_weight": "bold"}
            ]),
        ),
    ]
}

/// What `document.export` says about a document: its content, the features its loss report lists,
/// and the features whose losses need consent.
struct Drawing {
    content: String,
    /// Every feature of the loss report, consent or not.
    features: Vec<String>,
    /// The features the refusal lists as needing consent; empty when the export was not refused.
    consent: Vec<String>,
}

/// An application call, with the envelope the other application tests use.
fn call(application: &App, request_id: &str, operation: &str, parameters: Value) -> Value {
    let request = json!({
        "contract": 1,
        "request_id": request_id,
        "profile": "preserve-semantics",
        "operation": operation,
        "parameters": parameters,
    });
    serde_json::to_value(application.execute(request.to_string().as_bytes())).unwrap()
}

/// An application with the image decoder and the format codecs that `smiles_operations.rs`
/// attaches. `document.export` is executable only while a format codec is attached.
fn app(directory: &Path) -> App {
    Application::new(LocalCapabilities, FileDocumentRepository::new(directory))
        .with_ctfile_codec(V2000Codec)
        .with_interchange_codec(LocalInterchangeCodec)
        .with_smiles_codec(OpenSmilesCodec)
        .with_image_decoder(EmbeddedPngDecoder)
}

/// Creates an empty document and returns its id.
fn create(application: &App) -> String {
    let created = call(application, "create", "document.create", json!({}));
    created["result"]["document"]["id"]
        .as_str()
        .expect("document.create returns the new document's id")
        .to_owned()
}

/// Applies `transaction` to document `id` at `expected_revision` and returns the revision it
/// leaves, or the refusal reply when the transaction is not accepted.
fn apply(
    application: &App,
    request_id: &str,
    id: &str,
    expected_revision: u64,
    transaction: &Value,
) -> Result<u64, Value> {
    let mut parameters = transaction.clone();
    parameters["document_id"] = json!(id);
    parameters["expected_revision"] = json!(expected_revision);
    let reply = call(application, request_id, "document.transaction", parameters);
    if reply["outcome"] != "ok" {
        return Err(reply);
    }
    Ok(reply["document_revision"]
        .as_u64()
        .expect("an accepted transaction reports its revision"))
}

/// A `document.export` of `id` as SVG, accepting the losses that `accepted_losses` names.
fn export(application: &App, request_id: &str, id: &str, accepted_losses: Option<&str>) -> Value {
    let mut parameters = json!({"format": "svg", "document_id": id});
    if let Some(digest) = accepted_losses {
        parameters["accepted_losses"] = json!(digest);
    }
    call(application, request_id, "document.export", parameters)
}

/// Reads document `id` as a caller does. An export that would drop features needing consent is
/// refused with those features and the digest that accepts them. Accepting that digest returns the
/// content and the whole loss report.
fn read(application: &App, id: &str) -> Drawing {
    let plain = export(application, "export-plain", id, None);
    if plain["outcome"] != "error" {
        return drawing_of(&plain, Vec::new());
    }
    assert_eq!(
        plain["diagnostics"][0]["code"], "loss_consent_required",
        "{plain}"
    );
    let details = &plain["diagnostics"][0]["details"];
    let consent = details["features"]
        .as_array()
        .expect("the refusal lists its features")
        .iter()
        .map(|feature| feature.as_str().expect("a feature is a name").to_owned())
        .collect();
    let digest = details["loss_digest"]
        .as_str()
        .expect("the refusal names the digest that accepts it");
    let accepted = export(application, "export-accepted", id, Some(digest));
    drawing_of(&accepted, consent)
}

/// The content and loss report of a successful export.
fn drawing_of(reply: &Value, consent: Vec<String>) -> Drawing {
    assert_eq!(reply["outcome"], "partial", "{reply}");
    Drawing {
        content: reply["result"]["content"]
            .as_str()
            .expect("an SVG export carries its content")
            .to_owned(),
        features: reply["loss_report"]
            .as_array()
            .expect("an export reports its losses as a list")
            .iter()
            .map(|loss| {
                loss["feature"]
                    .as_str()
                    .expect("a loss names its feature")
                    .to_owned()
            })
            .collect(),
        consent,
    }
}

/// Reads a new base document's drawing, after `row`'s transaction when there is one. A refused
/// transaction is returned as its reply, so the table reports every refused row.
fn read_drawing(row: Option<&Row>) -> Result<Drawing, Value> {
    let directory = tempfile::tempdir().unwrap();
    let application = app(directory.path());
    let id = create(&application);
    let revision = apply(&application, "base", &id, 0, &base_transaction())
        .expect("the base drawing is accepted");
    if let Some(row) = row {
        apply(&application, row.name, &id, revision, &row.transaction)?;
    }
    Ok(read(&application, &id))
}

/// The names in ascending order, so a list is compared as a set and a repeated name still shows.
fn sorted<'a>(names: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut names: Vec<&str> = names.collect();
    names.sort_unstable();
    names
}

/// How a content result reads in a failure message.
fn describe(changed: bool) -> &'static str {
    if changed {
        "changed"
    } else {
        "is byte-identical"
    }
}

#[test]
fn each_mutation_changes_the_drawing_or_adds_exactly_its_losses() {
    let base = read_drawing(None).expect("the base drawing reads");
    assert_eq!(
        base.features,
        ["chemical_graph"],
        "the base draws atoms and bonds and states no other feature"
    );
    assert!(
        base.consent.is_empty(),
        "the base needs no consent, but the export asks for {:?}",
        base.consent
    );

    let mut failures = Vec::new();
    for row in table() {
        let after = match read_drawing(Some(&row)) {
            Ok(after) => after,
            Err(reply) => {
                failures.push(format!(
                    "{}: the transaction was refused: {reply}",
                    row.name
                ));
                continue;
            }
        };
        let changed = after.content != base.content;
        if changed != row.drawn {
            failures.push(format!(
                "{}: the content {}, but the table says it {}",
                row.name,
                describe(changed),
                describe(row.drawn)
            ));
        }
        let features = sorted(after.features.iter().map(String::as_str));
        let expected = sorted(row.features.iter().copied());
        if features != expected {
            failures.push(format!(
                "{}: loss features {features:?}, the table says {expected:?}",
                row.name
            ));
        }
        let consent = sorted(after.consent.iter().map(String::as_str));
        let expected_consent = sorted(row.consent.iter().copied());
        if consent != expected_consent {
            failures.push(format!(
                "{}: consent features {consent:?}, the table says {expected_consent:?}",
                row.name
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} row(s) disagree with the table:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Removing only the underline flag leaves the drawn bytes unchanged: the underline is not drawn,
/// so a caption with the flag exports the same content as the same caption without it.
#[test]
fn the_underline_flag_alone_changes_no_drawn_byte() {
    let base = read_drawing(None).expect("the base drawing reads");
    let underlined_row = Row {
        name: "caption-underlined",
        transaction: json!({"commands": [
            {"op": "add_caption", "anchor": {"x": 0, "y": 60},
                "runs": [{"text": "H", "script": "baseline", "underline": true}]}
        ]}),
        drawn: true,
        features: &["chemical_graph", "caption_underline"],
        consent: &["caption_underline"],
    };
    let plain_row = Row {
        name: "caption-plain",
        transaction: json!({"commands": [
            {"op": "add_caption", "anchor": {"x": 0, "y": 60},
                "runs": [{"text": "H", "script": "baseline", "underline": false}]}
        ]}),
        drawn: true,
        features: PLAIN,
        consent: NONE,
    };
    let underlined =
        read_drawing(Some(&underlined_row)).expect("the underlined caption is accepted");
    let plain = read_drawing(Some(&plain_row)).expect("the plain caption is accepted");
    assert_ne!(
        plain.content, base.content,
        "the plain caption is drawn, so the content changes"
    );
    assert_eq!(
        underlined.content, plain.content,
        "removing only the underline flag leaves the drawn bytes equal"
    );
}

/// The loss report lists features in the order `svg_losses` gives them. This document states
/// sixteen of the table's seventeen features. The seventeenth, `stereo_mark`, sits between
/// `zero_length_bond` and `stereo_configuration` and is absent: no transaction can state a mark
/// that its bond's order does not draw, so no document built from transactions reaches it (see
/// `table`).
#[test]
fn the_loss_report_lists_the_reachable_features_in_the_documented_order() {
    let every = Row {
        name: "every-reachable-feature",
        transaction: json!({
            "commands": [
                {"op": "add_caption", "anchor": {"x": 0, "y": 70},
                    "runs": [{"text": "漢", "script": "baseline"}]},
                {"op": "add_caption", "anchor": {"x": 0, "y": 80},
                    "runs": [{"text": "字", "script": "baseline"}]},
                {"op": "add_caption", "anchor": {"x": 0, "y": 90},
                    "runs": [{"text": "H", "script": "baseline", "underline": true}]},
                {"op": "group", "objects": ["s2", "s3"]},
                {"op": "rename", "title": "Drift fixture"},
                {"op": "update_settings", "label_weight": "bold"},
                {"op": "update_atom", "atom": "a4", "radical": "doublet"},
                {"op": "update_atom", "atom": "a7", "attachment_points": [1]},
                {"op": "add_atom", "query": {"kind": "list", "elements": ["N", "O", "S", "P"]},
                    "position": {"x": 0, "y": 120}},
                {"op": "set_atom_configuration", "atom": "a2",
                    "configuration": {"kind": "tetrahedral",
                        "neighbors": ["a1", "a3", "a4", "implicit_hydrogen"],
                        "turn": "clockwise"}},
                {"op": "create_stereo_group", "kind": "absolute", "atoms": ["a2"],
                    "number": null},
                {"op": "collapse_ethyl", "instance": "s1", "anchor": "a5", "terminal": "a6"},
                {"op": "add_reference", "source": "s1", "target": {"kind": "atom", "id": "a1"}},
                {"op": "update_bond", "bond": "b1", "order": "query",
                    "query": {"kind": "list", "orders": ["single", "double"]}},
                {"op": "move_atoms", "atoms": ["a1"], "delta": {"x": 26, "y": -15}}
            ],
            "selection_after": {"objects": [], "atoms": ["a1"], "bonds": []}
        }),
        drawn: true,
        features: &[
            "chemical_graph",
            "title",
            "selection",
            "scene_references",
            "scene_group",
            "label_typography",
            "radical_mark",
            "attachment_point_mark",
            "query_label_truncated",
            "query_bond_expression",
            "zero_length_bond",
            "stereo_configuration",
            "stereo_group",
            "abbreviation_display",
            "caption",
            "caption_underline",
        ],
        consent: &[
            "abbreviation_display",
            "attachment_point_mark",
            "caption",
            "caption_underline",
            "label_typography",
            "query_label_truncated",
            "radical_mark",
            "stereo_configuration",
            "stereo_group",
        ],
    };
    let after = read_drawing(Some(&every)).expect("the combined transaction is accepted");
    assert_eq!(
        after.features, every.features,
        "the loss report lists the features in the svg_losses order"
    );
    assert_eq!(
        sorted(after.consent.iter().map(String::as_str)),
        sorted(every.consent.iter().copied()),
        "the features that need consent"
    );
}
