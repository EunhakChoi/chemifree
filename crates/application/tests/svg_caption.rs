//! The caption layout and markup of the bounded SVG export (P38b2): the run rules, the reserved
//! box, the shadow copy, the tspan markup, the drawn character set and the paint order.
//!
//! Every expected value is written out here from the rules in `svg/caption.rs` and the advance
//! table in `svg/font.rs`; none is computed by the code under test. The sans-serif advances used
//! are `H` 0.74 em (row `CDHRU`), `i` 0.27 em (row `i`), `2` 0.65 em (row `234`) and `O` 0.8 em
//! (row `GOQ`). The overhang is 0.04 em, the ascent 1.0 em, the descent 0.4 em and the padding 2 pt.
//! A document's own instance is drawn first, as an empty `<g>` group when it holds no atom, so a
//! caption-only drawing begins with that group.

use std::path::Path;

use chemifree_application::svg::{
    DrawingLayout, LabelBox, ObjectLayout, ViewBox, layout_drawing, render_drawing,
};
use chemifree_application::{Application, DocumentView};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

/// The slack allowed when a computed number is compared with a literal one. It is far below the
/// hundredth that the drawing rounds to.
const EPSILON: f64 = 1e-9;
/// The hundredth to which `view_box_of` moves each view box edge outward.
const HUNDREDTH: f64 = 0.01;
const XML_DECLARATION: &str = r#"<?xml version="1.0" encoding="UTF-8"?>"#;

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

/// The caption entries of `layout`, in paint order.
fn caption_objects(layout: &DrawingLayout) -> Vec<&ObjectLayout> {
    layout
        .objects
        .iter()
        .filter(|object| matches!(object, ObjectLayout::Caption(_)))
        .collect()
}

/// The only caption entry of `layout`. The test panics unless the paint list holds exactly one.
fn sole_caption(layout: &DrawingLayout) -> &ObjectLayout {
    let captions = caption_objects(layout);
    let [caption] = captions.as_slice() else {
        panic!(
            "expected one caption in the paint list, found {}",
            captions.len()
        );
    };
    *caption
}

/// One paint-list entry as `kind id`, so that an order test reads as a list of literal names.
fn paint_entry(object: &ObjectLayout) -> String {
    match object {
        ObjectLayout::Instance { id } => format!("instance {id}"),
        ObjectLayout::Graphic { id, .. } => format!("graphic {id}"),
        ObjectLayout::Image { id, .. } => format!("image {id}"),
        ObjectLayout::Caption(caption) => format!("caption {}", caption.id),
    }
}

/// The lines of `content` between its root element and its closing tag. The declaration, the root
/// and the closing tag are checked on the way, so a test compares only the body.
fn body_of(content: &str) -> Vec<&str> {
    let lines: Vec<&str> = content.lines().collect();
    let [declaration, root, body @ .., closing] = lines.as_slice() else {
        panic!("the drawing needs a declaration, a root element and a closing tag: {content}");
    };
    assert_eq!(
        *declaration, XML_DECLARATION,
        "the drawing starts with its declaration"
    );
    assert!(
        root.starts_with("<svg "),
        "the root element follows the declaration: {root}"
    );
    assert_eq!(*closing, "</svg>", "the closing tag ends the drawing");
    body.to_vec()
}

/// Asserts that `actual` is `expected` to within `EPSILON`. `what` names the value in a failure.
fn assert_near(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= EPSILON,
        "{what}: expected {expected}, got {actual}"
    );
}

/// Asserts a reserved box against the literal `[x, y, width, height]` in pt.
fn assert_bounds(bounds: LabelBox, expected: [f64; 4], what: &str) {
    let [x, y, width, height] = expected;
    assert_near(bounds.x, x, &format!("{what}: left edge"));
    assert_near(bounds.y, y, &format!("{what}: top edge"));
    assert_near(bounds.width, width, &format!("{what}: width"));
    assert_near(bounds.height, height, &format!("{what}: height"));
}

/// Asserts that `view_box` contains `bounds`: no edge of the box lies outside the view box, which
/// `EPSILON` allows for.
fn assert_view_box_contains(view_box: ViewBox, bounds: LabelBox) {
    let right = view_box.x + view_box.width;
    let bottom = view_box.y + view_box.height;
    let box_right = bounds.x + bounds.width;
    let box_bottom = bounds.y + bounds.height;
    assert!(
        view_box.x <= bounds.x + EPSILON,
        "the view box's left edge {} must not lie inside the box's left edge {}",
        view_box.x,
        bounds.x
    );
    assert!(
        view_box.y <= bounds.y + EPSILON,
        "the view box's top edge {} must not lie inside the box's top edge {}",
        view_box.y,
        bounds.y
    );
    assert!(
        right >= box_right - EPSILON,
        "the view box's right edge {right} must not lie inside the box's right edge {box_right}"
    );
    assert!(
        bottom >= box_bottom - EPSILON,
        "the view box's bottom edge {bottom} must not lie inside the box's bottom edge {box_bottom}"
    );
}

/// Asserts that `actual`, a low edge of a view box, is `edge` moved down to the hundredth: at most
/// `edge`, and less than a hundredth below it.
fn assert_low_edge_snapped(actual: f64, edge: f64, what: &str) {
    assert!(
        (edge - HUNDREDTH - EPSILON..=edge + EPSILON).contains(&actual),
        "{what} must be {edge} moved down to the hundredth, got {actual}"
    );
}

/// Asserts that `actual`, a high edge of a view box, is `edge` moved up to the hundredth.
fn assert_high_edge_snapped(actual: f64, edge: f64, what: &str) {
    assert!(
        (edge - EPSILON..=edge + HUNDREDTH + EPSILON).contains(&actual),
        "{what} must be {edge} moved up to the hundredth, got {actual}"
    );
}

#[test]
fn a_run_takes_its_size_and_shift_from_its_script() {
    let view = build(json!([
        {"op":"update_settings","label_font":"sans-serif","label_size_pt":12},
        {"op":"add_caption","anchor":{"x":100,"y":50},"runs":[
            {"text":"H","script":"baseline"},
            {"text":"2","script":"subscript"},
            {"text":"+","script":"superscript","size_pt":24}
        ]}
    ]));
    let layout = layout_drawing(&view);
    let ObjectLayout::Caption(caption) = sole_caption(&layout) else {
        panic!("the paint list entry is not a caption");
    };
    let [baseline, subscript, superscript] = caption.runs.as_slice() else {
        panic!("the caption has three runs, found {}", caption.runs.len());
    };
    // A baseline run keeps the 12 pt label size and sits on the baseline. A subscript is two thirds
    // of that size and sits a third of it below the baseline, a shift of +4. A superscript at 24 pt
    // is two thirds of 24 pt and sits 5/12 of 24 pt above the baseline, a shift of -10.
    assert_near(baseline.size, 12.0, "a baseline run's size");
    assert_near(baseline.shift, 0.0, "a baseline run's shift");
    assert_near(subscript.size, 8.0, "a subscript's size at 12 pt");
    assert_near(subscript.shift, 4.0, "a subscript's shift at 12 pt");
    assert_near(superscript.size, 16.0, "a superscript's size at 24 pt");
    assert_near(superscript.shift, -10.0, "a superscript's shift at 24 pt");
}

#[test]
fn a_run_without_a_font_takes_the_label_font_and_a_named_font_overrides_it() {
    let view = build(json!([
        {"op":"update_settings","label_font":"serif"},
        {"op":"add_caption","anchor":{"x":100,"y":50},"runs":[
            {"text":"H","script":"baseline","font":null},
            {"text":"2","script":"baseline","font":"monospace"},
            {"text":"i","script":"baseline"}
        ]}
    ]));
    let layout = layout_drawing(&view);
    let ObjectLayout::Caption(caption) = sole_caption(&layout) else {
        panic!("the paint list entry is not a caption");
    };
    let families: Vec<&str> = caption.runs.iter().map(|run| run.family).collect();
    assert_eq!(
        families,
        ["serif", "monospace", "serif"],
        "a null font and an omitted font take the serif label font, and a named font wins"
    );
}

#[test]
fn a_run_without_a_colour_is_black_and_a_stored_colour_is_kept_verbatim() {
    let view = build(json!([
        {"op":"update_settings","label_font":"sans-serif","label_size_pt":12},
        {"op":"add_caption","anchor":{"x":100,"y":50},"runs":[
            {"text":"H","script":"baseline","color":null},
            {"text":"i","script":"baseline","color":"#1A2B3C"}
        ]}
    ]));
    let layout = layout_drawing(&view);
    let ObjectLayout::Caption(caption) = sole_caption(&layout) else {
        panic!("the paint list entry is not a caption");
    };
    let colours: Vec<&str> = caption.runs.iter().map(|run| run.color.as_str()).collect();
    assert_eq!(
        colours,
        ["#000000", "#1A2B3C"],
        "a null colour is black, and a stored colour is kept"
    );
    let content = render_drawing(&view).content;
    assert_eq!(
        body_of(&content),
        [
            "<g>",
            "</g>",
            "<g>",
            r##"<text x="100" y="50" text-anchor="start" xml:space="preserve"><tspan font-family="sans-serif" font-size="12" font-weight="normal" font-style="normal" fill="#000000">H</tspan><tspan font-family="sans-serif" font-size="12" font-weight="normal" font-style="normal" fill="#1A2B3C">i</tspan></text>"##,
            "</g>",
        ]
    );
}

#[test]
fn alignment_sets_the_anchor_and_shifts_the_box_by_none_half_or_all_of_the_advance() {
    // "Hi" at 12 pt is 12.12 pt wide: H 0.74 em and i 0.27 em, each at 12 pt. The box reaches
    // 0.48 pt (0.04 em of 12 pt) past each end, then 2 pt of padding, so it is 17.08 pt wide. It
    // spans the 12 pt ascent and the 4.8 pt descent (0.4 em), then the padding, so it is 20.8 pt
    // high and starts 14 pt above the baseline at y = 50. Its left edge is the anchor less 2.48 pt,
    // less the alignment offset: 0 for left, 6.06 for centre (half of 12.12) and 12.12 for right.
    let cases = [
        (
            "left",
            "start",
            97.52,
            r##"<text x="100" y="50" text-anchor="start" xml:space="preserve"><tspan font-family="sans-serif" font-size="12" font-weight="normal" font-style="normal" fill="#000000">Hi</tspan></text>"##,
        ),
        (
            "center",
            "middle",
            91.46,
            r##"<text x="100" y="50" text-anchor="middle" xml:space="preserve"><tspan font-family="sans-serif" font-size="12" font-weight="normal" font-style="normal" fill="#000000">Hi</tspan></text>"##,
        ),
        (
            "right",
            "end",
            85.40,
            r##"<text x="100" y="50" text-anchor="end" xml:space="preserve"><tspan font-family="sans-serif" font-size="12" font-weight="normal" font-style="normal" fill="#000000">Hi</tspan></text>"##,
        ),
    ];
    for (alignment, anchor, left, text_element) in cases {
        let view = build(json!([
            {"op":"update_settings","label_font":"sans-serif","label_size_pt":12},
            {"op":"add_caption","anchor":{"x":100,"y":50},"alignment":alignment,"runs":[
                {"text":"Hi","script":"baseline"}
            ]}
        ]));
        let layout = layout_drawing(&view);
        let ObjectLayout::Caption(caption) = sole_caption(&layout) else {
            panic!("the paint list entry is not a caption");
        };
        assert_eq!(
            caption.anchor, anchor,
            "a {alignment} caption's text-anchor"
        );
        assert_bounds(caption.bounds, [left, 36.0, 17.08, 20.8], alignment);
        let content = render_drawing(&view).content;
        assert_eq!(
            body_of(&content),
            ["<g>", "</g>", "<g>", text_element, "</g>"],
            "a {alignment} caption's markup"
        );
    }
}

#[test]
fn an_outline_strokes_at_a_sixteenth_of_its_glyph_size() {
    let view = build(json!([
        {"op":"update_settings","label_font":"sans-serif","label_size_pt":12},
        {"op":"add_caption","anchor":{"x":100,"y":50},"runs":[
            {"text":"H","script":"baseline","outline":true},
            {"text":"2","script":"subscript","outline":true}
        ]}
    ]));
    let layout = layout_drawing(&view);
    let ObjectLayout::Caption(caption) = sole_caption(&layout) else {
        panic!("the paint list entry is not a caption");
    };
    // A 12 pt run strokes at 12/16 = 0.75 pt. A subscript is 8 pt, so it strokes at 8/16 = 0.5 pt.
    let outlines: Vec<Option<f64>> = caption.runs.iter().map(|run| run.outline).collect();
    assert_eq!(
        outlines,
        [Some(0.75), Some(0.5)],
        "an outline is a sixteenth of the glyph size"
    );
    let content = render_drawing(&view).content;
    assert_eq!(
        body_of(&content),
        [
            "<g>",
            "</g>",
            "<g>",
            r##"<text x="100" y="50" text-anchor="start" xml:space="preserve"><tspan font-family="sans-serif" font-size="12" font-weight="normal" font-style="normal" fill="none" stroke="#000000" stroke-width="0.75" stroke-linejoin="round">H</tspan><tspan dy="4" font-family="sans-serif" font-size="8" font-weight="normal" font-style="normal" fill="none" stroke="#000000" stroke-width="0.5" stroke-linejoin="round">2</tspan></text>"##,
            "</g>",
        ]
    );
}

#[test]
fn the_box_grows_by_half_the_widest_outline_not_by_their_sum() {
    // "H" at 12 pt has an outline of 0.75 pt, so half of it is 0.375. "2" at 8 pt has an outline of
    // 0.5 pt, so half of it is 0.25. The advance is 8.88 + 5.2 = 14.08 pt. The left edge reaches the
    // first run's overhang of 0.48 pt, and the right edge the last run's of 0.32 pt (0.04 em of
    // 8 pt). Vertically the runs span from 38, the 12 pt run's ascent, to 57.2, the subscript's
    // descent. The box then grows by the larger 0.375 on each side, not by 0.625, and gets 2 pt of
    // padding.
    let view = build(json!([
        {"op":"update_settings","label_font":"sans-serif","label_size_pt":12},
        {"op":"add_caption","anchor":{"x":100,"y":50},"runs":[
            {"text":"H","script":"baseline","outline":true},
            {"text":"2","script":"subscript","outline":true}
        ]}
    ]));
    let layout = layout_drawing(&view);
    let ObjectLayout::Caption(caption) = sole_caption(&layout) else {
        panic!("the paint list entry is not a caption");
    };
    assert_bounds(
        caption.bounds,
        [97.145, 35.625, 19.63, 23.95],
        "two outlined runs",
    );
}

#[test]
fn a_shadowed_run_adds_a_copy_one_point_down_and_right_painted_before_the_caption() {
    let view = build(json!([
        {"op":"update_settings","label_font":"sans-serif","label_size_pt":12},
        {"op":"add_caption","anchor":{"x":100,"y":50},"runs":[
            {"text":"H","script":"baseline"},
            {"text":"i","script":"baseline","shadow":true}
        ]}
    ]));
    let layout = layout_drawing(&view);
    let ObjectLayout::Caption(caption) = sole_caption(&layout) else {
        panic!("the paint list entry is not a caption");
    };
    // The box of "Hi" is 17.08 by 20.8 pt at (97.52, 36), and a shadow adds 1 pt on the right and
    // at the bottom.
    assert_bounds(
        caption.bounds,
        [97.52, 36.0, 18.08, 21.8],
        "a shadowed caption",
    );
    let content = render_drawing(&view).content;
    assert_eq!(
        body_of(&content),
        [
            "<g>",
            "</g>",
            "<g>",
            "<g opacity=\"0.35\">",
            r##"<text x="101" y="51" text-anchor="start" xml:space="preserve"><tspan font-family="sans-serif" font-size="12" font-weight="normal" font-style="normal" fill="#000000" visibility="hidden">H</tspan><tspan font-family="sans-serif" font-size="12" font-weight="normal" font-style="normal" fill="#000000">i</tspan></text>"##,
            "</g>",
            r##"<text x="100" y="50" text-anchor="start" xml:space="preserve"><tspan font-family="sans-serif" font-size="12" font-weight="normal" font-style="normal" fill="#000000">H</tspan><tspan font-family="sans-serif" font-size="12" font-weight="normal" font-style="normal" fill="#000000">i</tspan></text>"##,
            "</g>",
        ]
    );
}

#[test]
fn the_caption_is_one_text_chunk_of_relative_dy_tspans_with_no_x_or_y() {
    let view = build(json!([
        {"op":"update_settings","label_font":"sans-serif","label_size_pt":12},
        {"op":"add_caption","anchor":{"x":100,"y":50},"runs":[
            {"text":"H","script":"baseline"},
            {"text":"2","script":"subscript"},
            {"text":"O","script":"baseline"},
            {"text":"+","script":"superscript","size_pt":24}
        ]}
    ]));
    let content = render_drawing(&view).content;
    // The shifts are 0, 4, 0 and -10 pt, so each tspan's dy is the difference from the one before,
    // and the first, which has no difference, has no dy.
    assert_eq!(
        body_of(&content),
        [
            "<g>",
            "</g>",
            "<g>",
            r##"<text x="100" y="50" text-anchor="start" xml:space="preserve"><tspan font-family="sans-serif" font-size="12" font-weight="normal" font-style="normal" fill="#000000">H</tspan><tspan dy="4" font-family="sans-serif" font-size="8" font-weight="normal" font-style="normal" fill="#000000">2</tspan><tspan dy="-4" font-family="sans-serif" font-size="12" font-weight="normal" font-style="normal" fill="#000000">O</tspan><tspan dy="-10" font-family="sans-serif" font-size="16" font-weight="normal" font-style="normal" fill="#000000">+</tspan></text>"##,
            "</g>",
        ]
    );
    let parsed = roxmltree::Document::parse(&content).expect("the drawing is well-formed XML");
    let positions: Vec<(Option<&str>, Option<&str>, Option<&str>)> = parsed
        .descendants()
        .filter(|node| node.tag_name().name() == "tspan")
        .map(|tspan| {
            (
                tspan.attribute("x"),
                tspan.attribute("y"),
                tspan.attribute("dy"),
            )
        })
        .collect();
    assert_eq!(
        positions,
        [
            (None, None, None),
            (None, None, Some("4")),
            (None, None, Some("-4")),
            (None, None, Some("-10")),
        ],
        "no tspan has an x or y, and each dy is its shift less the shift before it"
    );
}

#[test]
fn spaces_are_kept_verbatim_and_markup_characters_are_escaped() {
    const SPACED: &str = "  a  <&>\"  ";
    let view = build(json!([
        {"op":"update_settings","label_font":"sans-serif","label_size_pt":12},
        {"op":"add_caption","anchor":{"x":100,"y":50},"runs":[
            {"text":SPACED,"script":"baseline"}
        ]}
    ]));
    let content = render_drawing(&view).content;
    assert_eq!(
        body_of(&content),
        [
            "<g>",
            "</g>",
            "<g>",
            r##"<text x="100" y="50" text-anchor="start" xml:space="preserve"><tspan font-family="sans-serif" font-size="12" font-weight="normal" font-style="normal" fill="#000000">  a  &lt;&amp;&gt;&quot;  </tspan></text>"##,
            "</g>",
        ]
    );
    let parsed = roxmltree::Document::parse(&content).expect("the drawing is well-formed XML");
    let tspan = parsed
        .descendants()
        .find(|node| node.tag_name().name() == "tspan")
        .expect("the caption has one tspan");
    let parsed_text: String = tspan
        .children()
        .filter(|node| node.is_text())
        .filter_map(|node| node.text())
        .collect();
    assert_eq!(parsed_text, SPACED, "the parsed text is the stored text");
}

#[test]
fn a_caption_with_a_character_outside_the_drawn_set_is_not_drawn_and_changes_no_bytes() {
    let plain = render_drawing(&build(json!([
        {"op":"add_atom","element":"C","position":{"x":0,"y":0}}
    ])));
    let cases = [
        ("U+00AD SOFT HYPHEN", "\u{00AD}"),
        ("U+03A2, a gap in the Greek capitals", "\u{03A2}"),
        ("U+0301 COMBINING ACUTE ACCENT", "\u{0301}"),
        ("U+6F22 CJK ideograph", "\u{6F22}"),
        ("U+05D0 HEBREW LETTER ALEF", "\u{05D0}"),
    ];
    for (name, character) in cases {
        // The outside character is in the second run, so the whole caption is refused, not only
        // the run that holds it.
        let text = format!("a{character}b");
        let view = build(json!([
            {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
            {"op":"add_caption","anchor":{"x":100,"y":50},"runs":[
                {"text":"H","script":"baseline"},
                {"text":text,"script":"subscript"}
            ]}
        ]));
        assert!(
            caption_objects(&layout_drawing(&view)).is_empty(),
            "{name} is outside the drawn set, so the whole caption has no entry"
        );
        let drawing = render_drawing(&view);
        assert_eq!(drawing.content, plain.content, "{name} changes no bytes");
        assert_eq!(
            drawing.view_box, plain.view_box,
            "{name} changes no view box"
        );
    }
}

#[test]
fn a_caption_of_the_boundary_characters_of_the_drawn_set_is_drawn() {
    // U+007E, U+00A0, U+00FF, U+03C9, U+21CC and U+2212: characters at the edges of the drawn
    // ranges, which a caption may use.
    const BOUNDARY: &str = "~\u{00A0}\u{00FF}\u{03C9}\u{21CC}\u{2212}";
    let view = build(json!([
        {"op":"update_settings","label_font":"sans-serif","label_size_pt":12},
        {"op":"add_caption","anchor":{"x":100,"y":50},"runs":[
            {"text":BOUNDARY,"script":"baseline"}
        ]}
    ]));
    let layout = layout_drawing(&view);
    let ObjectLayout::Caption(caption) = sole_caption(&layout) else {
        panic!("the paint list entry is not a caption");
    };
    assert_eq!(
        caption.runs[0].text, BOUNDARY,
        "the run keeps its text unchanged"
    );
    let content = render_drawing(&view).content;
    let text = format!(
        r##"<text x="100" y="50" text-anchor="start" xml:space="preserve"><tspan font-family="sans-serif" font-size="12" font-weight="normal" font-style="normal" fill="#000000">{BOUNDARY}</tspan></text>"##
    );
    assert_eq!(
        body_of(&content),
        ["<g>", "</g>", "<g>", text.as_str(), "</g>"]
    );
}

#[test]
fn the_view_box_contains_each_caption_box_and_snaps_the_union_outward() {
    // "Hi" at (100, 50) reserves 97.52 to 114.6 across and 36 to 56.8 down, as the alignment test
    // shows. The second caption is right-aligned at (200, 120), with a subscript H and a
    // superscript 2 at 8 pt. Its advance is 5.92 + 5.2 = 11.12 pt, so it reserves 186.56 to 202.32
    // across. Its runs span 107 to 127.2 down, so it reserves 105 to 129.2. The view box is the
    // union of the two boxes, with each edge moved outward to the hundredth.
    let view = build(json!([
        {"op":"update_settings","label_font":"sans-serif","label_size_pt":12},
        {"op":"add_caption","anchor":{"x":100,"y":50},"runs":[{"text":"Hi","script":"baseline"}]},
        {"op":"add_caption","anchor":{"x":200,"y":120},"alignment":"right","runs":[
            {"text":"H","script":"subscript"},
            {"text":"2","script":"superscript"}
        ]}
    ]));
    let layout = layout_drawing(&view);
    let captions = caption_objects(&layout);
    let [first, second] = captions.as_slice() else {
        panic!(
            "expected two captions in the paint list, found {}",
            captions.len()
        );
    };
    let ObjectLayout::Caption(first) = first else {
        panic!("the paint list entry is not a caption");
    };
    let ObjectLayout::Caption(second) = second else {
        panic!("the paint list entry is not a caption");
    };
    assert_bounds(
        first.bounds,
        [97.52, 36.0, 17.08, 20.8],
        "the first caption's box",
    );
    assert_bounds(
        second.bounds,
        [186.56, 105.0, 15.76, 24.2],
        "the second caption's box",
    );
    let view_box = render_drawing(&view).view_box;
    assert_low_edge_snapped(view_box.x, 97.52, "the view box's left edge");
    assert_low_edge_snapped(view_box.y, 36.0, "the view box's top edge");
    assert_high_edge_snapped(
        view_box.x + view_box.width,
        202.32,
        "the view box's right edge",
    );
    assert_high_edge_snapped(
        view_box.y + view_box.height,
        129.2,
        "the view box's bottom edge",
    );
    assert_view_box_contains(view_box, first.bounds);
    assert_view_box_contains(view_box, second.bounds);
}

#[test]
fn a_caption_is_painted_between_the_graphic_before_it_and_the_instance_after_it() {
    let view = build(json!([
        {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
        {"op":"add_graphic","shape":"line","points":[{"x":0,"y":30},{"x":30,"y":30}]},
        {"op":"add_caption","anchor":{"x":100,"y":50},"runs":[{"text":"Hi","script":"baseline"}]},
        {"op":"add_instance","offset":{"x":0,"y":110}}
    ]));
    // Object ids follow creation order, with s1 the document's own instance.
    let order: Vec<String> = layout_drawing(&view)
        .objects
        .iter()
        .map(paint_entry)
        .collect();
    assert_eq!(
        order,
        ["instance s1", "graphic s2", "caption s3", "instance s4"],
        "the paint list follows the scene order"
    );
}
