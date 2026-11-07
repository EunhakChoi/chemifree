//! The markup of a drawing. Number formatting, escaping and the text of every element are decided
//! here, and nothing else writes markup. Each element is one line, in document order, with no ids,
//! classes, styles or data attributes, so one layout always writes the same bytes.

use super::font;
use super::geometry::{self, PathStep, Pattern, Point, Stroke};
use super::label::AtomLabel;
use super::{DrawingLayout, InstanceLayout, ViewBox};
use crate::DocumentView;

/// The dash lengths of a dashed and a dotted line, in pt, as the canvas draws them (`DASHES`).
const DASHED: &str = "2.5 2";
const DOTTED: &str = "0.1 2.2";

/// A number as the drawing writes it: rounded to the hundredth, in its shortest form, and `0` for
/// zero whatever its sign.
pub(super) fn number(value: f64) -> String {
    debug_assert!(
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

/// Escapes `text` for element content or an attribute value.
pub(super) fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            other => escaped.push(other),
        }
    }
    escaped
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
fn instance_block(
    view: &DocumentView,
    instance: &InstanceLayout,
    family: &str,
    line_width: f64,
) -> Vec<String> {
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
            number(line_width)
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

/// The whole drawing: the XML declaration, the root `svg` element with its size and view box, each
/// instance in layout order, and the closing tag. One element per line, with a trailing newline.
pub(super) fn document(view: &DocumentView, layout: &DrawingLayout, view_box: ViewBox) -> String {
    let x = number(view_box.x);
    let y = number(view_box.y);
    let width = number(view_box.width);
    let height = number(view_box.height);
    let family = font::family(view.settings.label_font);
    let mut lines = vec![
        r#"<?xml version="1.0" encoding="UTF-8"?>"#.to_owned(),
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" version="1.1" width="{width}pt" height="{height}pt" viewBox="{x} {y} {width} {height}">"##
        ),
    ];
    for instance in &layout.instances {
        lines.extend(instance_block(
            view,
            instance,
            family,
            view.settings.line_width_pt,
        ));
    }
    lines.push("</svg>".to_owned());
    let mut content = lines.join("\n");
    content.push('\n');
    content
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svg::geometry::LabelBox;
    use crate::svg::label::LabelRun;

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
}
