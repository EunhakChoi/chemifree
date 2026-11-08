//! The markup of a drawing. Number formatting, escaping and the text of every element are decided
//! here, and nothing else writes markup. Each element is one line, in document order, with no ids,
//! classes, styles or data attributes, so one layout always writes the same bytes.

use super::font;
use super::geometry::{self, PathStep, Pattern, Point, Stroke};
use super::label::AtomLabel;
use super::{DrawingLayout, GraphicMark, InstanceLayout, ObjectLayout, ViewBox};
use crate::{DocumentView, SceneObjectView};

/// The dash lengths of a dashed and a dotted line, in pt, as the canvas draws them (`DASHES`).
const DASHED: &str = "2.5 2";
const DOTTED: &str = "0.1 2.2";

/// A number as the drawing writes it: rounded to the hundredth, in its shortest form, and `0` for
/// zero whatever its sign.
///
/// # Panics
///
/// Panics if `value` is not finite, in debug and release builds alike, so that a non-finite
/// number stops the drawing instead of being written as `NaN` or `inf`. A view built from a
/// `Document` never holds a non-finite number: every position is a `Point`, which `Point::new`
/// keeps finite and within `MAX_COORDINATE`; every document's settings have passed
/// `Settings::validate`; and the layout checks that a length is not zero before dividing by it.
/// `render_drawing` is public and takes any `DocumentView`, including one whose fields a caller
/// has changed, so such a field still reaches this panic.
pub(super) fn number(value: f64) -> String {
    assert!(
        value.is_finite(),
        "a drawing number must be finite, got {value}"
    );
    let rounded = geometry::round2(value);
    if rounded == 0.0 {
        "0".to_owned()
    } else {
        format!("{rounded}")
    }
}

/// Escapes `text` for element content or an attribute value. A character that XML 1.0 forbids is
/// dropped, because not even a character reference can write it.
pub(super) fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            other if is_xml_char(other) => escaped.push(other),
            _ => {}
        }
    }
    escaped
}

/// Whether XML 1.0 allows `character` in a document, as its `Char` production (section 2.2) does:
/// tab, line feed and carriage return, plus every code point from U+0020 to U+10FFFF except the
/// surrogates, U+FFFE and U+FFFF.
fn is_xml_char(character: char) -> bool {
    matches!(
        u32::from(character),
        0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF
    )
}

/// The points of a polyline or polygon: `x,y` pairs separated by spaces.
fn point_list(points: &[Point]) -> String {
    let pairs: Vec<String> = points
        .iter()
        .map(|point| format!("{},{}", number(point.x), number(point.y)))
        .collect();
    pairs.join(" ")
}

/// The data of a path of steps: `M` to start, `Q` for a curve, and `T` to continue it smoothly.
fn path_data(steps: &[PathStep]) -> String {
    let parts: Vec<String> = steps
        .iter()
        .map(|step| match step {
            PathStep::Move(point) => format!("M {} {}", number(point.x), number(point.y)),
            PathStep::Curve { control, end } => format!(
                "Q {} {} {} {}",
                number(control.x),
                number(control.y),
                number(end.x),
                number(end.y)
            ),
            PathStep::Smooth(point) => format!("T {} {}", number(point.x), number(point.y)),
        })
        .collect();
    parts.join(" ")
}

/// The data of hash stripes: each stripe is a move and a line, and they share one path.
fn stripes_data(segments: &[(Point, Point)]) -> String {
    let parts: Vec<String> = segments
        .iter()
        .map(|(from, to)| {
            format!(
                "M {} {} L {} {}",
                number(from.x),
                number(from.y),
                number(to.x),
                number(to.y)
            )
        })
        .collect();
    parts.join(" ")
}

/// The data of a cubic Bézier: `M` to its start, then `C` to its two controls and its end.
///
/// # Panics
///
/// Panics unless `points` holds exactly four points, as `graphic_marks` stores them.
fn cubic_data(points: &[Point]) -> String {
    let &[start, first, second, end] = points else {
        panic!("a cubic stores four points, found {}", points.len());
    };
    format!(
        "M {} {} C {} {} {} {} {} {}",
        number(start.x),
        number(start.y),
        number(first.x),
        number(first.y),
        number(second.x),
        number(second.y),
        number(end.x),
        number(end.y)
    )
}

/// One stroke as an element: a line with its dash, a polyline, a filled polygon with no outline,
/// the stripes of a hashed wedge, or a wave.
fn stroke_element(stroke: &Stroke) -> String {
    match stroke {
        Stroke::Line {
            x1,
            y1,
            x2,
            y2,
            pattern,
        } => {
            let dash = match pattern {
                Pattern::Solid => String::new(),
                Pattern::Dashed => format!(r#" stroke-dasharray="{DASHED}""#),
                Pattern::Dotted => format!(r#" stroke-dasharray="{DOTTED}""#),
            };
            format!(
                r#"<line x1="{}" y1="{}" x2="{}" y2="{}"{dash}/>"#,
                number(*x1),
                number(*y1),
                number(*x2),
                number(*y2)
            )
        }
        Stroke::Polyline { points } => format!(r#"<polyline points="{}"/>"#, point_list(points)),
        Stroke::Polygon { points } => format!(
            r##"<polygon points="{}" fill="#000000" stroke="none"/>"##,
            point_list(points)
        ),
        Stroke::Stripes { segments } => format!(
            r#"<path d="{}" stroke-linecap="butt"/>"#,
            stripes_data(segments)
        ),
        Stroke::Wave {
            from,
            to,
            half_waves,
            amplitude,
        } => format!(
            r#"<path d="{}"/>"#,
            path_data(&geometry::wave_steps(*from, *to, *half_waves, *amplitude))
        ),
    }
}

/// One atom's label as a text element centred at `at` (`ChunkText`). Each run is a `tspan` whose
/// `dy` is its shift less the shift of the run before it, since SVG shifts are relative; a zero
/// `dy` is left out.
fn text_element(at: Point, label: &AtomLabel) -> String {
    let mut tspans: Vec<String> = Vec::new();
    let mut previous = 0.0;
    for item in &label.runs {
        let shift = geometry::round2(item.shift);
        let step = shift - previous;
        previous = shift;
        let dy = if geometry::round2(step) == 0.0 {
            String::new()
        } else {
            format!(r#" dy="{}""#, number(step))
        };
        tspans.push(format!(
            r#"<tspan{dy} font-size="{}" font-weight="{}">{}</tspan>"#,
            number(item.size),
            item.weight,
            escape(&item.text)
        ));
    }
    format!(
        r##"<text x="{}" y="{}" text-anchor="middle" dominant-baseline="central" xml:space="preserve">{}</text>"##,
        number(at.x + label.x),
        number(at.y),
        tspans.concat()
    )
}

/// One instance as a group: its bonds' strokes in the order of `view.bonds`, in a group with the
/// stroke style, and its labels in the order of `view.atoms`, in a group with the label font. A
/// group is written only when it has something in it.
fn instance_block(view: &DocumentView, instance: &InstanceLayout, family: &str) -> Vec<String> {
    let strokes: Vec<String> = view
        .bonds
        .iter()
        .filter_map(|bond| instance.bonds.get(&bond.id))
        .flatten()
        .map(stroke_element)
        .collect();
    let texts: Vec<String> = view
        .atoms
        .iter()
        .filter_map(|atom| {
            let label = instance.labels.get(&atom.id)?;
            let at = instance.atoms.get(&atom.id)?;
            Some(text_element(*at, label))
        })
        .collect();
    let mut lines = vec!["<g>".to_owned()];
    if !strokes.is_empty() {
        lines.push(format!(
            r##"<g fill="none" stroke="#000000" stroke-width="{}" stroke-linecap="round" stroke-linejoin="round">"##,
            number(view.settings.line_width_pt)
        ));
        lines.extend(strokes);
        lines.push("</g>".to_owned());
    }
    if !texts.is_empty() {
        lines.push(format!(r##"<g fill="#000000" font-family="{family}">"##));
        lines.extend(texts);
        lines.push("</g>".to_owned());
    }
    lines.push("</g>".to_owned());
    lines
}

/// One graphic as a group with the canvas's stroke style (`SceneGraphic`,
/// `structure-canvas.tsx:1153-1209`): one element per mark, in paint order. The group's fill is
/// none, so a mark is an outline unless it sets its own fill.
fn graphic_block(width: f64, marks: &[GraphicMark]) -> Vec<String> {
    let mut lines = vec![format!(
        r##"<g fill="none" stroke="#000000" stroke-width="{}" stroke-linecap="round" stroke-linejoin="round">"##,
        number(width)
    )];
    lines.extend(marks.iter().map(mark_element));
    lines.push("</g>".to_owned());
    lines
}

/// One mark of a graphic as one element. A head sets its own black fill, as the canvas fills it
/// (`structure-canvas.tsx:1204`); every other mark takes its stroke from the group.
fn mark_element(mark: &GraphicMark) -> String {
    match mark {
        GraphicMark::Line { x1, y1, x2, y2 } => format!(
            r#"<line x1="{}" y1="{}" x2="{}" y2="{}"/>"#,
            number(*x1),
            number(*y1),
            number(*x2),
            number(*y2)
        ),
        GraphicMark::Head { points } => format!(
            r##"<polygon points="{}" fill="#000000"/>"##,
            point_list(points)
        ),
        GraphicMark::Rect {
            x,
            y,
            width,
            height,
        } => format!(
            r#"<rect x="{}" y="{}" width="{}" height="{}"/>"#,
            number(*x),
            number(*y),
            number(*width),
            number(*height)
        ),
        GraphicMark::Ellipse { cx, cy, rx, ry } => format!(
            r#"<ellipse cx="{}" cy="{}" rx="{}" ry="{}"/>"#,
            number(*cx),
            number(*cy),
            number(*rx),
            number(*ry)
        ),
        GraphicMark::Polygon { points } => {
            format!(r#"<polygon points="{}"/>"#, point_list(points))
        }
        GraphicMark::Cubic { points } => format!(r#"<path d="{}"/>"#, cubic_data(points)),
    }
}

/// The MIME type and base64 text of the image object `id` of `view`.
///
/// # Panics
///
/// Panics if `view` holds no image object with `id`. The layout of `view` names only its image
/// objects, so a missing one is a bug, not an input error.
fn image_data<'a>(view: &'a DocumentView, id: &str) -> (&'a str, &'a str) {
    view.scene
        .objects
        .iter()
        .find_map(|object| match object {
            SceneObjectView::Image {
                id: object_id,
                mime_type,
                data_base64,
                ..
            } if object_id == id => Some((*mime_type, data_base64.as_str())),
            _ => None,
        })
        .expect("the layout names only image objects of its view")
}

/// One image as one element: its rectangle in pt, which the image fills
/// (`preserveAspectRatio="none"`), and its bytes as a data URL. The reference is `xlink:href`, the
/// form SVG 1.1 defines, so the root declares the `xlink` namespace when the drawing holds an image.
fn image_element(x: f64, y: f64, width: f64, height: f64, mime: &str, base64: &str) -> String {
    format!(
        r#"<image x="{}" y="{}" width="{}" height="{}" preserveAspectRatio="none" xlink:href="{}"/>"#,
        number(x),
        number(y),
        number(width),
        number(height),
        escape(&format!("data:{mime};base64,{base64}"))
    )
}

/// The whole drawing: the XML declaration, the root `svg` element with its size and view box, each
/// object in paint order, and the closing tag. One element per line, with a trailing newline. The
/// root declares the `xlink` namespace only when the drawing holds an image, so a drawing without
/// one keeps its bytes.
pub(super) fn document(view: &DocumentView, layout: &DrawingLayout, view_box: ViewBox) -> String {
    let x = number(view_box.x);
    let y = number(view_box.y);
    let width = number(view_box.width);
    let height = number(view_box.height);
    let family = font::family(view.settings.label_font);
    let has_image = layout
        .objects
        .iter()
        .any(|object| matches!(object, ObjectLayout::Image { .. }));
    let xlink_namespace = if has_image {
        r##" xmlns:xlink="http://www.w3.org/1999/xlink""##
    } else {
        ""
    };
    let mut lines = vec![
        r#"<?xml version="1.0" encoding="UTF-8"?>"#.to_owned(),
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg"{xlink_namespace} version="1.1" width="{width}pt" height="{height}pt" viewBox="{x} {y} {width} {height}">"##
        ),
    ];
    for object in &layout.objects {
        match object {
            ObjectLayout::Instance { id } => {
                let instance = layout
                    .instances
                    .iter()
                    .find(|instance| instance.instance == *id)
                    .expect("an instance object names an instance the layout lists");
                lines.extend(instance_block(view, instance, family));
            }
            ObjectLayout::Graphic {
                width: stroke_width,
                marks,
                ..
            } => lines.extend(graphic_block(*stroke_width, marks)),
            ObjectLayout::Image {
                id,
                x: image_x,
                y: image_y,
                width: image_width,
                height: image_height,
            } => {
                let (mime, base64) = image_data(view, id);
                lines.push(image_element(
                    *image_x,
                    *image_y,
                    *image_width,
                    *image_height,
                    mime,
                    base64,
                ));
            }
        }
    }
    lines.push("</svg>".to_owned());
    let mut content = lines.join("\n");
    content.push('\n');
    content
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svg::ADVANCE_TABLE_ID;
    use crate::svg::geometry::LabelBox;
    use crate::svg::label::LabelRun;
    use crate::{
        CarbonLabelsDto, DisplayUnitDto, LabelFontDto, LabelStyleDto, LabelWeightDto, MoleculeView,
        PositionView, SceneView, SelectionDto, SettingsDto, StereoView,
    };

    fn run(text: &str, size: f64, shift: f64, weight: u16) -> LabelRun {
        LabelRun {
            text: text.to_owned(),
            size,
            shift,
            weight,
        }
    }

    #[test]
    fn number_writes_the_hundredth_in_shortest_form_and_no_zero_sign() {
        assert_eq!(number(2.0), "2");
        assert_eq!(number(2.5), "2.5");
        assert_eq!(
            number(-0.004),
            "0",
            "a value that rounds to zero is written as 0"
        );
        assert_eq!(
            number(-0.025),
            "-0.02",
            "JavaScript's Math.round sends -2.5 to -2"
        );
        assert_eq!(number(0.1 + 0.2), "0.3");
    }

    #[test]
    fn escape_replaces_the_four_markup_characters() {
        assert_eq!(escape("[A&B]<\"x\">"), "[A&amp;B]&lt;&quot;x&quot;&gt;");
    }

    #[test]
    fn escape_drops_characters_xml_1_0_forbids_and_keeps_legal_text() {
        let text = "a\u{1}b\u{FFFE}\u{FFFF}c & <d> \"e\"\t\n\r";
        assert_eq!(
            escape(text),
            "abc &amp; &lt;d&gt; &quot;e&quot;\t\n\r",
            "U+0001, U+FFFE and U+FFFF are dropped; escapes, tab, line feed and carriage return are kept"
        );

        let bounds = LabelBox {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };
        let label = AtomLabel {
            x: 0.0,
            runs: vec![run(text, 12.0, 0.0, 400)],
            chunk_box: bounds,
            bounds,
        };
        let fragment = text_element(Point { x: 0.0, y: 0.0 }, &label);
        roxmltree::Document::parse(&fragment).unwrap_or_else(|error| {
            panic!("the rendered label is not well-formed XML: {error}\n{fragment}")
        });
    }

    #[test]
    fn dashed_and_dotted_lines_carry_the_canvas_dash_lengths() {
        let line = |pattern| Stroke::Line {
            x1: 0.0,
            y1: 0.0,
            x2: 10.0,
            y2: 0.0,
            pattern,
        };
        assert_eq!(
            stroke_element(&line(Pattern::Solid)),
            r#"<line x1="0" y1="0" x2="10" y2="0"/>"#
        );
        assert_eq!(
            stroke_element(&line(Pattern::Dashed)),
            r#"<line x1="0" y1="0" x2="10" y2="0" stroke-dasharray="2.5 2"/>"#
        );
        assert_eq!(
            stroke_element(&line(Pattern::Dotted)),
            r#"<line x1="0" y1="0" x2="10" y2="0" stroke-dasharray="0.1 2.2"/>"#
        );
    }

    #[test]
    fn a_label_writes_each_run_shifted_from_the_one_before() {
        let bounds = LabelBox {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };
        let label = AtomLabel {
            x: 1.5,
            runs: vec![
                run("13", 8.0, -5.0, 400),
                run("C", 12.0, 0.0, 500),
                run("+", 8.0, -5.0, 400),
            ],
            chunk_box: bounds,
            bounds,
        };
        assert_eq!(
            text_element(Point { x: 10.0, y: 20.0 }, &label),
            r#"<text x="11.5" y="20" text-anchor="middle" dominant-baseline="central" xml:space="preserve"><tspan dy="-5" font-size="8" font-weight="400">13</tspan><tspan dy="5" font-size="12" font-weight="500">C</tspan><tspan dy="-5" font-size="8" font-weight="400">+</tspan></text>"#
        );
    }

    /// The base64 text of the image tests. It has `+`, `/` and padding, so an alphabet change or a
    /// dropped `=` changes the output. The writer never decodes it, so it need not be a real PNG.
    const IMAGE_BASE64: &str = "iVBORw0KGgoA+/8=";

    /// The scene object and the layout object of an image `img1`: 30 pt by 40 pt, at (10, 20) pt.
    fn image_pair() -> (SceneObjectView, ObjectLayout) {
        let scene_object = SceneObjectView::Image {
            id: "img1".to_owned(),
            parent: None,
            mime_type: "image/png",
            data_base64: IMAGE_BASE64.to_owned(),
            origin: PositionView { x: 10.0, y: 20.0 },
            size_pt: PositionView { x: 30.0, y: 40.0 },
            pixel_width: 1,
            pixel_height: 1,
        };
        let layout_object = ObjectLayout::Image {
            id: "img1".to_owned(),
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        };
        (scene_object, layout_object)
    }

    /// A view whose scene is `scene_objects` alone, with the default label settings.
    fn view_with(scene_objects: Vec<SceneObjectView>) -> DocumentView {
        DocumentView {
            fragment_displays: Vec::new(),
            abbreviations: Vec::new(),
            id: "d1".to_owned(),
            title: "Untitled".to_owned(),
            revision: 0,
            atoms: Vec::new(),
            bonds: Vec::new(),
            molecule: MoleculeView { id: "m1" },
            scene: SceneView {
                pages: Vec::new(),
                instances: Vec::new(),
                roots: Vec::new(),
                objects: scene_objects,
                references: Vec::new(),
            },
            selection: SelectionDto::default(),
            settings: SettingsDto {
                bond_length_pt: 30.0,
                line_width_pt: 1.2,
                label_font: LabelFontDto::SansSerif,
                label_weight: LabelWeightDto::Normal,
                label_style: LabelStyleDto::Normal,
                label_underline: false,
                label_outline: false,
                label_shadow: false,
                label_size_pt: 12.0,
                carbon_labels: CarbonLabelsDto::Default,
                display_unit: DisplayUnitDto::Pt,
            },
            stereo: StereoView::default(),
        }
    }

    /// The drawing of `objects` over `view_with(scene_objects)`, in a 100 pt square view box. The
    /// writer takes a graphic's marks from its layout object and an image's bytes from the view.
    fn drawing(scene_objects: Vec<SceneObjectView>, objects: Vec<ObjectLayout>) -> String {
        let layout = DrawingLayout {
            table: ADVANCE_TABLE_ID,
            instances: Vec::new(),
            objects,
        };
        let view_box = ViewBox {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        document(&view_with(scene_objects), &layout, view_box)
    }

    /// A straight line from (0, 0) to (10, 0) pt.
    fn straight_line() -> GraphicMark {
        GraphicMark::Line {
            x1: 0.0,
            y1: 0.0,
            x2: 10.0,
            y2: 0.0,
        }
    }

    #[test]
    fn each_graphic_shape_writes_its_documented_element() {
        const GROUP: &str = r##"<g fill="none" stroke="#000000" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">"##;
        let at = |x: f64, y: f64| Point { x, y };
        let shapes = [
            (straight_line(), r#"<line x1="0" y1="0" x2="10" y2="0"/>"#),
            (
                GraphicMark::Rect {
                    x: 1.0,
                    y: 2.0,
                    width: 30.0,
                    height: 40.0,
                },
                r#"<rect x="1" y="2" width="30" height="40"/>"#,
            ),
            (
                GraphicMark::Ellipse {
                    cx: 5.0,
                    cy: 6.0,
                    rx: 7.0,
                    ry: 8.0,
                },
                r#"<ellipse cx="5" cy="6" rx="7" ry="8"/>"#,
            ),
            (
                GraphicMark::Polygon {
                    points: vec![at(0.0, 0.0), at(10.0, 0.0), at(5.0, 8.0)],
                },
                r#"<polygon points="0,0 10,0 5,8"/>"#,
            ),
            (
                GraphicMark::Cubic {
                    points: vec![at(0.0, 0.0), at(10.0, 0.0), at(20.0, 10.0), at(30.0, 10.0)],
                },
                r#"<path d="M 0 0 C 10 0 20 10 30 10"/>"#,
            ),
        ];
        for (mark, element) in shapes {
            assert_eq!(
                graphic_block(1.5, std::slice::from_ref(&mark)),
                [GROUP, element, "</g>"],
                "{mark:?} is one element in a group with the stroke style"
            );
        }

        let polygon = mark_element(&GraphicMark::Polygon {
            points: vec![at(0.0, 0.0), at(10.0, 0.0), at(5.0, 8.0)],
        });
        assert!(
            !polygon.contains(" fill="),
            "a polygon sets no fill of its own, so the group's fill none applies: {polygon}"
        );

        let arrow = [
            straight_line(),
            GraphicMark::Head {
                points: vec![at(10.0, 0.0), at(4.0, 3.0), at(4.0, -3.0)],
            },
        ];
        assert_eq!(
            graphic_block(1.5, &arrow),
            [
                GROUP,
                r#"<line x1="0" y1="0" x2="10" y2="0"/>"#,
                r##"<polygon points="10,0 4,3 4,-3" fill="#000000"/>"##,
                "</g>",
            ],
            "an arrow is its shaft, then its head, which sets its own black fill"
        );
    }

    #[test]
    fn an_image_writes_its_base64_verbatim_with_xlink_href() {
        let (scene_object, layout_object) = image_pair();
        let content = drawing(vec![scene_object], vec![layout_object]);
        assert_eq!(
            content.lines().find(|line| line.starts_with("<image")),
            Some(
                r#"<image x="10" y="20" width="30" height="40" preserveAspectRatio="none" xlink:href="data:image/png;base64,iVBORw0KGgoA+/8="/>"#
            ),
            "the data URI is the stored base64 unchanged, the image fills its rectangle, and the reference is xlink:href"
        );
    }

    #[test]
    fn the_root_declares_xlink_only_when_an_image_is_drawn() {
        const ROOT_WITHOUT_XLINK: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" version="1.1" width="100pt" height="100pt" viewBox="0 0 100 100">"#;
        const ROOT_WITH_XLINK: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" version="1.1" width="100pt" height="100pt" viewBox="0 0 100 100">"#;

        let graphic = ObjectLayout::Graphic {
            id: "g1".to_owned(),
            width: 1.0,
            marks: vec![straight_line()],
        };
        let without_image = drawing(Vec::new(), vec![graphic]);
        assert_eq!(
            without_image.lines().nth(1),
            Some(ROOT_WITHOUT_XLINK),
            "a drawing with no image declares no xlink namespace"
        );

        let (scene_object, layout_object) = image_pair();
        let with_image = drawing(vec![scene_object], vec![layout_object]);
        let parsed = roxmltree::Document::parse(&with_image).unwrap_or_else(|error| {
            panic!("a drawing with an image is not well-formed XML: {error}\n{with_image}")
        });
        let image = parsed
            .descendants()
            .find(|node| node.tag_name().name() == "image")
            .expect("the drawing holds one image element");
        assert_eq!(
            image.attribute(("http://www.w3.org/1999/xlink", "href")),
            Some("data:image/png;base64,iVBORw0KGgoA+/8="),
            "xlink:href is in the XLink namespace, which the root declares"
        );
        assert_eq!(
            with_image.lines().nth(1),
            Some(ROOT_WITH_XLINK),
            "a drawing with an image declares the xlink namespace on its root"
        );
    }

    #[test]
    fn a_graphic_width_uses_the_rounded_effective_width() {
        for (width, written) in [(1.234, "1.23"), (1.236, "1.24"), (2.0, "2")] {
            let graphic = ObjectLayout::Graphic {
                id: "g1".to_owned(),
                width,
                marks: vec![straight_line()],
            };
            let content = drawing(Vec::new(), vec![graphic]);
            let group = content
                .lines()
                .find(|line| line.starts_with("<g "))
                .expect("a graphic is drawn in a group");
            let expected = format!(
                r##"<g fill="none" stroke="#000000" stroke-width="{written}" stroke-linecap="round" stroke-linejoin="round">"##
            );
            assert_eq!(
                group, expected,
                "a stroke width of {width} pt is written as {written}, rounded to the hundredth"
            );
        }
    }
}
