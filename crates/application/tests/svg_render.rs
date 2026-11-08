//! The SVG drawing (P38a1): byte-exact goldens, structure checks, determinism, save and reopen
//! identity, and the parity fixture that the web check reads. A run with `CHEMIFREE_BLESS_SVG=1`
//! writes the goldens and the fixture and then fails, so a passing run never changes a file.

use std::fs;
use std::path::{Path, PathBuf};

use chemifree_application::svg::{
    ADVANCE_TABLE_ID, DrawingLayout, LabelBox, SvgDrawing, ViewBox, layout_drawing, render_drawing,
};
use chemifree_application::{Application, DocumentView};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

/// The environment variable that rewrites the goldens and the parity fixture.
const BLESS: &str = "CHEMIFREE_BLESS_SVG";
/// The id the parity fixture pins. A document's id is random, and it is the only field of
/// `DocumentView` that is not deterministic; the drawing does not read it.
const PARITY_ID: &str = "parity-fixture";
/// The slack allowed when a drawn value is compared with a bound computed from the same numbers.
const EPSILON: f64 = 1e-9;
const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";
/// The elements a drawing writes.
const ELEMENTS: &[&str] = &[
    "svg", "g", "line", "polyline", "polygon", "path", "text", "tspan",
];
/// The attributes a drawing writes. `space` is the local name of `xml:space`.
const ATTRIBUTES: &[&str] = &[
    "version",
    "width",
    "height",
    "viewBox",
    "fill",
    "stroke",
    "stroke-width",
    "stroke-linecap",
    "stroke-linejoin",
    "font-family",
    "x1",
    "y1",
    "x2",
    "y2",
    "stroke-dasharray",
    "points",
    "d",
    "x",
    "y",
    "text-anchor",
    "dominant-baseline",
    "dy",
    "font-size",
    "font-weight",
    "space",
];
/// The attributes whose values are numbers (or lists of numbers), which carry at most two decimals.
const NUMERIC: &[&str] = &[
    "width",
    "height",
    "viewBox",
    "x",
    "y",
    "x1",
    "y1",
    "x2",
    "y2",
    "points",
    "d",
    "stroke-width",
    "stroke-dasharray",
    "dy",
    "font-size",
];
/// The generic families a label font may name.
const FAMILIES: &[&str] = &["sans-serif", "serif", "monospace"];

fn app(directory: &Path) -> App {
    Application::new(LocalCapabilities, FileDocumentRepository::new(directory))
}

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

/// Creates a document through the application and returns its id.
fn create(application: &App) -> String {
    let created = call(application, "create", "document.create", json!({}));
    created["result"]["document"]["id"]
        .as_str()
        .expect("document.create returns the new document's id")
        .to_owned()
}

/// Applies `commands` to the new document `id` in one transaction, which must succeed.
fn transact(application: &App, id: &str, commands: Value) {
    let reply = call(
        application,
        "transaction",
        "document.transaction",
        json!({"document_id": id, "expected_revision": 0, "commands": commands}),
    );
    assert_eq!(reply["outcome"], "ok", "{reply}");
}

/// A new document with `commands` applied in one transaction, read back through the application.
fn build(commands: Value) -> DocumentView {
    let directory = tempfile::tempdir().unwrap();
    let application = app(directory.path());
    let id = create(&application);
    transact(&application, &id, commands);
    application.document(&id).unwrap().document
}

/// An empty document: no atoms, and the one scene instance every document holds.
fn empty_document() -> DocumentView {
    let directory = tempfile::tempdir().unwrap();
    let application = app(directory.path());
    let id = create(&application);
    application.document(&id).unwrap().document
}

// Commands name atoms by id and bonds by id, as the existing transaction tests do. Atoms are `a1`,
// `a2`, ... and bonds are `b1`, `b2`, ..., each numbered in the order a command creates it. A
// `bond_to` creates one bond, and so does an `add_bond`.

/// A carbon chain ending in an oxygen: bare vertices, with one labelled atom.
fn chain_commands() -> Value {
    json!([
        {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
        {"op":"add_atom","element":"C","position":{"x":26,"y":-15},"bond_to":{"atom":"a1","order":"single"}},
        {"op":"add_atom","element":"O","position":{"x":52,"y":0},"bond_to":{"atom":"a2","order":"single"}}
    ])
}

/// Every bond order: a double (marked crossed, so drawn as two crossing lines), a triple, a
/// benzene ring of aromatic bonds, a dative bond from a labelled nitrogen, a list-query bond and
/// an unknown bond.
fn bond_order_commands() -> Value {
    json!([
        {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
        {"op":"add_atom","element":"C","position":{"x":26,"y":0},"bond_to":{"atom":"a1","order":"double"}},
        {"op":"add_atom","element":"C","position":{"x":0,"y":40}},
        {"op":"add_atom","element":"C","position":{"x":26,"y":40},"bond_to":{"atom":"a3","order":"triple"}},
        {"op":"add_atom","element":"C","position":{"x":20,"y":130}},
        {"op":"add_atom","element":"C","position":{"x":10,"y":147.32},"bond_to":{"atom":"a5","order":"aromatic"}},
        {"op":"add_atom","element":"C","position":{"x":-10,"y":147.32},"bond_to":{"atom":"a6","order":"aromatic"}},
        {"op":"add_atom","element":"C","position":{"x":-20,"y":130},"bond_to":{"atom":"a7","order":"aromatic"}},
        {"op":"add_atom","element":"C","position":{"x":-10,"y":112.68},"bond_to":{"atom":"a8","order":"aromatic"}},
        {"op":"add_atom","element":"C","position":{"x":10,"y":112.68},"bond_to":{"atom":"a9","order":"aromatic"}},
        {"op":"add_bond","begin":"a10","end":"a5","order":"aromatic"},
        {"op":"add_atom","element":"N","position":{"x":60,"y":0}},
        {"op":"add_atom","element":"C","position":{"x":80,"y":-20}},
        {"op":"add_bond","begin":"a11","end":"a12","order":"dative"},
        {"op":"add_atom","element":"C","position":{"x":-40,"y":0}},
        {"op":"add_atom","element":"C","position":{"x":-66,"y":0}},
        {"op":"add_bond","begin":"a13","end":"a14","order":"query","query":{"kind":"list","orders":["single","double"]}},
        {"op":"add_atom","element":"C","position":{"x":-40,"y":40}},
        {"op":"add_atom","element":"C","position":{"x":-66,"y":40}},
        {"op":"add_bond","begin":"a15","end":"a16","order":"unknown"},
        {"op":"set_bond_mark","bond":"b1","mark":{"kind":"crossed"}}
    ])
}

/// A wedge narrow at its begin, a hash narrow at its end, a wavy bond, and a wedge that ends at a
/// labelled nitrogen and is cut short there.
fn stereo_mark_commands() -> Value {
    json!([
        {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
        {"op":"add_atom","element":"C","position":{"x":26,"y":0},"bond_to":{"atom":"a1","order":"single"}},
        {"op":"add_atom","element":"C","position":{"x":0,"y":40}},
        {"op":"add_atom","element":"C","position":{"x":26,"y":40},"bond_to":{"atom":"a3","order":"single"}},
        {"op":"add_atom","element":"C","position":{"x":0,"y":80}},
        {"op":"add_atom","element":"C","position":{"x":26,"y":80},"bond_to":{"atom":"a5","order":"single"}},
        {"op":"add_atom","element":"C","position":{"x":0,"y":120}},
        {"op":"add_atom","element":"N","position":{"x":26,"y":120},"bond_to":{"atom":"a7","order":"single"}},
        {"op":"set_bond_mark","bond":"b1","mark":{"kind":"wedge","narrow":"begin"}},
        {"op":"set_bond_mark","bond":"b2","mark":{"kind":"hash","narrow":"end"}},
        {"op":"set_bond_mark","bond":"b3","mark":{"kind":"wavy"}},
        {"op":"set_bond_mark","bond":"b4","mark":{"kind":"wedge","narrow":"begin"}}
    ])
}

/// Label content: stated hydrogens on a carbon whose bond leans right (so they are drawn left), a
/// nitrogen with three stated hydrogens and a +1 charge, a 13C isotope, an O with a 2- charge, a
/// carbon with unknown hydrogens, two atom queries (one with four elements, which is truncated,
/// and one negated), and every carbon labelled.
fn label_commands() -> Value {
    json!([
        {"op":"add_atom","element":"N","position":{"x":0,"y":0},"hydrogens":{"mode":"specified","count":3}},
        {"op":"update_atom","atom":"a1","charge":1},
        {"op":"add_atom","element":"C","position":{"x":30,"y":0},"hydrogens":{"mode":"specified","count":3}},
        {"op":"add_atom","element":"C","position":{"x":60,"y":0},"bond_to":{"atom":"a2","order":"single"}},
        {"op":"add_atom","element":"C","position":{"x":0,"y":40}},
        {"op":"update_atom","atom":"a4","isotope":13},
        {"op":"add_atom","element":"O","position":{"x":0,"y":80}},
        {"op":"update_atom","atom":"a5","charge":-2},
        {"op":"add_atom","element":"C","position":{"x":30,"y":80}},
        {"op":"update_atom","atom":"a6","hydrogens":{"mode":"unknown"}},
        {"op":"add_atom","query":{"kind":"list","elements":["N","O","S","P"]},"position":{"x":0,"y":120}},
        {"op":"add_atom","query":{"kind":"list","elements":["N","O"],"negated":true},"position":{"x":30,"y":120}},
        {"op":"update_settings","carbon_labels":"all"}
    ])
}

/// A carbonyl with a 2.4 pt line width, a serif face and an 18 pt label.
fn serif_commands() -> Value {
    json!([
        {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
        {"op":"add_atom","element":"O","position":{"x":26,"y":0},"bond_to":{"atom":"a1","order":"double"}},
        {"op":"update_settings","line_width_pt":2.4,"label_font":"serif","label_size_pt":18}
    ])
}

/// Labelled atoms and an aromatic bond in a monospace face at 6 pt.
fn monospace_commands() -> Value {
    json!([
        {"op":"add_atom","element":"N","position":{"x":0,"y":0}},
        {"op":"add_atom","element":"O","position":{"x":26,"y":0},"bond_to":{"atom":"a1","order":"single"}},
        {"op":"add_atom","element":"C","position":{"x":0,"y":40}},
        {"op":"add_atom","element":"C","position":{"x":26,"y":40},"bond_to":{"atom":"a3","order":"aromatic"}},
        {"op":"update_settings","label_font":"monospace","label_size_pt":6}
    ])
}

/// A two-atom molecule and a second instance of it, offset from the first.
fn two_instance_commands() -> Value {
    json!([
        {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
        {"op":"add_atom","element":"O","position":{"x":26,"y":0},"bond_to":{"atom":"a1","order":"double"}},
        {"op":"add_instance","offset":{"x":80,"y":40}}
    ])
}

/// The two-instance document with its scene objects listed in reverse. The web draws the instances
/// in the order of the objects (`structure-canvas.tsx`), so the second instance is drawn first.
fn reversed_objects_view() -> DocumentView {
    let mut view = build(two_instance_commands());
    view.scene.objects.reverse();
    view
}

/// The golden documents, by the name of their golden file.
fn golden_documents() -> Vec<(&'static str, DocumentView)> {
    vec![
        ("chain-bare-vertices", build(chain_commands())),
        ("bond-orders", build(bond_order_commands())),
        ("stereo-marks", build(stereo_mark_commands())),
        ("labels", build(label_commands())),
        ("settings-wide-serif", build(serif_commands())),
        ("monospace-small", build(monospace_commands())),
        ("two-instances", build(two_instance_commands())),
        ("scene-object-order", reversed_objects_view()),
        ("empty", empty_document()),
    ]
}

fn fixture_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/svg")
}

/// Compares each fixture file with its committed text. A blessing run writes every file and fails.
fn check_fixtures(files: &[(String, String)]) {
    let bless = std::env::var_os(BLESS).is_some();
    let directory = fixture_directory();
    let mut problems = Vec::new();
    for (name, content) in files {
        if bless {
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join(name), content).unwrap();
            problems.push(format!("blessed {name}"));
            continue;
        }
        match fs::read_to_string(directory.join(name)) {
            Ok(committed) if committed == *content => {}
            Ok(_) => problems.push(format!(
                "{name} differs from the drawing; review the change, then bless with {BLESS}=1"
            )),
            Err(error) => problems.push(format!("{name} cannot be read: {error}")),
        }
    }
    if bless {
        problems.push(format!(
            "{BLESS} is set, so the files were written; this run fails by design, rerun without it"
        ));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn goldens_match_the_drawing_byte_for_byte() {
    let files: Vec<(String, String)> = golden_documents()
        .into_iter()
        .map(|(name, view)| (format!("{name}.svg"), render_drawing(&view).content))
        .collect();
    check_fixtures(&files);
}

#[test]
fn the_parity_fixture_is_the_rust_layout_of_the_golden_documents() {
    let cases: Vec<Value> = golden_documents()
        .into_iter()
        .map(|(name, mut view)| {
            view.id = PARITY_ID.to_owned();
            json!({
                "name": name,
                "document": serde_json::to_value(&view).unwrap(),
                "layout": serde_json::to_value(layout_drawing(&view)).unwrap(),
            })
        })
        .collect();
    let fixture = json!({"table": ADVANCE_TABLE_ID, "cases": cases});
    let mut content = serde_json::to_string_pretty(&fixture).unwrap();
    content.push('\n');
    check_fixtures(&[("parity.json".to_owned(), content)]);
}

/// The numbers in an attribute value: the separators are commas and spaces, and a `pt` unit is
/// dropped. Tokens that are not numbers (path commands such as `M`) are skipped.
fn numbers(value: &str) -> Vec<f64> {
    value
        .split(|character: char| character == ',' || character.is_whitespace())
        .filter_map(|token| token.trim_end_matches("pt").parse::<f64>().ok())
        .collect()
}

fn close(left: f64, right: f64) -> bool {
    (left - right).abs() < EPSILON
}

/// Whether a point lies in the view box, with the slack of `EPSILON` at each edge.
fn inside(view_box: ViewBox, x: f64, y: f64) -> bool {
    let horizontal = view_box.x - EPSILON..=view_box.x + view_box.width + EPSILON;
    let vertical = view_box.y - EPSILON..=view_box.y + view_box.height + EPSILON;
    horizontal.contains(&x) && vertical.contains(&y)
}

fn assert_two_decimals(name: &str, element: &str, attribute: &str, value: &str) {
    for token in value.split(|character: char| character == ',' || character.is_whitespace()) {
        let number = token.trim_end_matches("pt");
        if number.parse::<f64>().is_ok() {
            let decimals = number
                .split_once('.')
                .map_or(0, |(_, fraction)| fraction.len());
            assert!(
                decimals <= 2,
                "{name}: <{element} {attribute}> writes {number}, which has more than two decimals"
            );
        }
    }
}

/// Checks one drawing's markup against the structure it documents: well-formed XML, the root and
/// its size, the allowed elements and attributes, two decimals at most, black or transparent paint,
/// every drawn point inside the view box, and one text element per labelled atom in each instance.
fn check_structure(name: &str, drawing: &SvgDrawing, layout: &DrawingLayout) {
    let content = drawing.content.as_str();
    assert!(
        content.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"),
        "{name}: the XML declaration comes first, on its own line"
    );
    assert!(
        content.ends_with("</svg>\n"),
        "{name}: the document ends with </svg> and a newline"
    );
    let document = roxmltree::Document::parse(content)
        .unwrap_or_else(|error| panic!("{name} is not well-formed XML: {error}"));
    let root = document.root_element();
    assert_eq!(root.tag_name().name(), "svg", "{name}: the root element");
    assert_eq!(
        root.tag_name().namespace(),
        Some(SVG_NAMESPACE),
        "{name}: the root element is in the SVG namespace"
    );
    assert_eq!(
        root.attribute("version"),
        Some("1.1"),
        "{name}: the version"
    );

    let view_box = drawing.view_box;
    let declared = numbers(root.attribute("viewBox").unwrap_or_default());
    let expected = [view_box.x, view_box.y, view_box.width, view_box.height];
    assert_eq!(
        declared.len(),
        4,
        "{name}: the view box has four numbers, got {declared:?}"
    );
    for (value, wanted) in declared.iter().zip(expected) {
        assert!(
            close(*value, wanted),
            "{name}: the view box {declared:?} differs from {expected:?}"
        );
    }
    for (attribute, wanted) in [("width", view_box.width), ("height", view_box.height)] {
        let value = root.attribute(attribute).unwrap_or_default();
        assert!(
            value.ends_with("pt"),
            "{name}: {attribute}={value} is not in pt"
        );
        let size = numbers(value);
        assert!(
            size.len() == 1 && close(size[0], wanted),
            "{name}: {attribute}={value} differs from the view box size {wanted}"
        );
    }

    for node in document.descendants().filter(|node| node.is_element()) {
        let element = node.tag_name().name();
        assert!(
            ELEMENTS.contains(&element),
            "{name}: <{element}> is not an element a drawing uses"
        );
        for attribute in node.attributes() {
            let attribute_name = attribute.name();
            let value = attribute.value();
            assert!(
                ATTRIBUTES.contains(&attribute_name),
                "{name}: <{element}> carries {attribute_name}, which a drawing does not write"
            );
            match attribute_name {
                "fill" | "stroke" => assert!(
                    value == "#000000" || value == "none",
                    "{name}: <{element} {attribute_name}={value}> is not black or transparent"
                ),
                "font-family" => assert!(
                    FAMILIES.contains(&value),
                    "{name}: font-family {value} is not a generic family"
                ),
                _ => {}
            }
            if NUMERIC.contains(&attribute_name) {
                assert_two_decimals(name, element, attribute_name, value);
            }
        }
        match element {
            "line" => {
                let at =
                    |attribute: &str| numbers(node.attribute(attribute).unwrap_or_default())[0];
                assert!(
                    inside(view_box, at("x1"), at("y1")) && inside(view_box, at("x2"), at("y2")),
                    "{name}: a line leaves the view box"
                );
            }
            "polyline" | "polygon" | "path" => {
                let attribute = if element == "path" { "d" } else { "points" };
                for pair in numbers(node.attribute(attribute).unwrap_or_default()).chunks_exact(2) {
                    assert!(
                        inside(view_box, pair[0], pair[1]),
                        "{name}: <{element}> point ({}, {}) lies outside the view box",
                        pair[0],
                        pair[1]
                    );
                }
            }
            "text" => {
                let x = numbers(node.attribute("x").unwrap_or_default())[0];
                let y = numbers(node.attribute("y").unwrap_or_default())[0];
                assert!(
                    inside(view_box, x, y),
                    "{name}: a text anchor ({x}, {y}) lies outside the view box"
                );
            }
            _ => {}
        }
    }

    let texts = document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "text")
        .count();
    let labelled: usize = layout
        .instances
        .iter()
        .map(|instance| instance.labels.len())
        .sum();
    assert_eq!(
        texts, labelled,
        "{name}: one text element per labelled atom in each instance"
    );
    for instance in &layout.instances {
        for (id, label) in &instance.labels {
            let at = instance
                .atoms
                .get(id)
                .expect("every labelled atom has a point");
            let bounds = LabelBox {
                x: at.x + label.bounds.x,
                y: at.y + label.bounds.y,
                width: label.bounds.width,
                height: label.bounds.height,
            };
            assert!(
                inside(view_box, bounds.x, bounds.y)
                    && inside(view_box, bounds.x + bounds.width, bounds.y + bounds.height),
                "{name}: the label box of {id} lies outside the view box"
            );
        }
    }
}

#[test]
fn every_golden_is_well_formed_svg_with_the_documented_structure() {
    for (name, view) in golden_documents() {
        let drawing = render_drawing(&view);
        check_structure(name, &drawing, &layout_drawing(&view));
    }
}

#[test]
fn a_drawing_is_the_same_bytes_on_every_call_and_whatever_the_document_identity() {
    for (name, view) in golden_documents() {
        let first = render_drawing(&view).content;
        assert_eq!(
            first,
            render_drawing(&view).content,
            "{name} renders differently on a second call"
        );
        let mut renamed = view.clone();
        renamed.id = "another-id".to_owned();
        renamed.title = "Another title".to_owned();
        assert_eq!(
            first,
            render_drawing(&renamed).content,
            "{name} depends on the document's id or title"
        );
    }
}

#[test]
fn a_saved_and_reopened_document_draws_the_same_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let (id, before) = {
        let application = app(directory.path());
        let id = create(&application);
        transact(&application, &id, bond_order_commands());
        let view = application.document(&id).unwrap().document;
        let saved = call(
            &application,
            "save",
            "document.save",
            json!({"document_id": id, "expected_revision": view.revision}),
        );
        assert_eq!(saved["outcome"], "ok", "{saved}");
        let before = render_drawing(&view).content;
        (id, before)
    };
    let reopened = app(directory.path());
    let after = render_drawing(&reopened.document(&id).unwrap().document).content;
    assert_eq!(
        after, before,
        "a reopened document draws different bytes from the saved one"
    );
}

/// The instance ids of `view`'s layout, in the order they are drawn.
fn drawn_instances(view: &DocumentView) -> Vec<String> {
    layout_drawing(view)
        .instances
        .into_iter()
        .map(|instance| instance.instance)
        .collect()
}

#[test]
fn instances_are_drawn_in_the_order_of_the_scene_objects() {
    // The web sorts the instances by their objects (`structure-canvas.tsx`), so the order of the
    // instance list alone does not decide the drawing.
    let mut view = build(two_instance_commands());
    let listed: Vec<String> = view
        .scene
        .instances
        .iter()
        .map(|instance| instance.id.clone())
        .collect();
    assert_eq!(listed.len(), 2, "two instances are in the scene");
    assert_eq!(
        drawn_instances(&view),
        listed,
        "objects and instances agree, so drawn as listed"
    );
    view.scene.objects.reverse();
    let reversed: Vec<String> = listed.iter().rev().cloned().collect();
    assert_eq!(
        drawn_instances(&view),
        reversed,
        "reversed objects are drawn reversed"
    );
    view.scene.objects.reverse();
    view.scene.instances.reverse();
    assert_eq!(
        drawn_instances(&view),
        listed,
        "a reversed instance list is drawn as the objects list it"
    );
}

#[test]
fn a_document_without_an_instance_draws_nothing() {
    // The web draws the scene's instances and nothing else (`placements`); a server document always
    // has at least one instance, so this state comes only from a hand-made view.
    let mut view = build(chain_commands());
    view.scene.instances.clear();
    assert!(
        layout_drawing(&view).instances.is_empty(),
        "no instance, no layout"
    );
    let drawing = render_drawing(&view);
    assert_eq!(
        drawing.view_box,
        ViewBox {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        }
    );
}

#[test]
fn a_document_without_atoms_has_an_empty_view_box() {
    let drawing = render_drawing(&empty_document());
    assert_eq!(
        drawing.view_box,
        ViewBox {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        }
    );
    assert!(
        drawing
            .content
            .contains("width=\"0pt\" height=\"0pt\" viewBox=\"0 0 0 0\""),
        "the root element writes the empty box, got {}",
        drawing.content
    );
}

/// A caller may change a view's settings after it is built, so a non-finite number can reach the
/// drawing. The drawing stops there rather than writing it as `NaN`.
#[test]
#[should_panic(expected = "a drawing number must be finite")]
fn a_non_finite_number_stops_the_drawing() {
    // The line width is written only with a bond's strokes, so the view needs a bond.
    let mut view = build(json!([
        {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
        {"op":"add_atom","element":"C","position":{"x":26,"y":0},"bond_to":{"atom":"a1","order":"single"}}
    ]));
    view.settings.line_width_pt = f64::NAN;
    render_drawing(&view);
}
