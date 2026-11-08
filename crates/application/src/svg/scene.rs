//! The layout of a drawing's graphics and images, in unrounded pt. Each function mirrors one rule
//! of the web editor in `apps/web/src/editor/` (`structure-canvas.tsx`, `graphic-line-width.ts`,
//! `scene-image.tsx`), named in its doc comment. The writer rounds these numbers and writes markup.

use serde::Serialize;

use super::geometry::{self, Point};
use crate::{GraphicShapeDto, PositionView};

/// One primitive of a graphic, in the order it is painted (`SceneGraphic`,
/// `structure-canvas.tsx:1153-1209`). A head is filled as well as stroked
/// (`structure-canvas.tsx:1204`); every other mark is stroked only.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum GraphicMark {
    /// A straight segment: a line, or an arrow's shaft from its first point to its tip.
    Line { x1: f64, y1: f64, x2: f64, y2: f64 },
    /// An arrow's head: the tip, then the two base corners. An arrow with no shaft length has none.
    Head { points: Vec<Point> },
    /// An axis-aligned rectangle: its top-left corner and its size.
    Rect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    /// An axis-aligned ellipse: its centre and its half extents.
    Ellipse { cx: f64, cy: f64, rx: f64, ry: f64 },
    /// A closed outline through its vertices in stored order. No vertex is added to close it.
    Polygon { points: Vec<Point> },
    /// A cubic Bézier through its start, its two controls and its end, in stored order.
    Cubic { points: Vec<Point> },
}

/// One object of a drawing, in paint order (`structure-canvas.tsx:1056-1088`). A group draws
/// nothing and the export draws no caption, so neither has an entry.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObjectLayout {
    /// An instance: the drawing of the `InstanceLayout` with this id.
    Instance { id: String },
    /// A graphic, with its stroke width in pt and its marks.
    Graphic {
        id: String,
        width: f64,
        marks: Vec<GraphicMark>,
    },
    /// An image's rectangle in pt (`scene-image.tsx:13-17`). Its bytes are not laid out.
    Image {
        id: String,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
}

/// The length of a graphic arrow's head, measured back along the shaft from the tip, in pt. It is
/// not scaled by the line width (`structure-canvas.tsx:1184`).
const GRAPHIC_ARROW_LENGTH: f64 = 6.0;
/// Half the width of a graphic arrow's head, in pt (`structure-canvas.tsx:1184`).
const GRAPHIC_ARROW_HALF_WIDTH: f64 = 3.0;

/// The marks of a graphic of `shape` through `points`, in paint order, as `SceneGraphic` draws
/// them (`structure-canvas.tsx:1153-1209`):
///
/// - A line is one `Line` from its first point to its second (`structure-canvas.tsx:1200-1202`).
/// - An arrow is that `Line` as its shaft, then a `Head` at the second point when the shaft has a
///   length (`structure-canvas.tsx:1180-1184, 1204`).
/// - A rectangle is one `Rect` from the smaller corner and the absolute size of the corners
///   (`structure-canvas.tsx:1185`).
/// - An ellipse is one `Ellipse` centred between its corners, with half their absolute differences
///   as its radii (`structure-canvas.tsx:1187-1188`).
/// - A polygon is one `Polygon` through its vertices in stored order
///   (`structure-canvas.tsx:1171-1177`, `polygon.ts:21-25`).
/// - A spline is one `Cubic` through its four points in stored order, with no evaluation
///   (`structure-canvas.tsx:1161-1169`, `spline.ts:13-17`).
///
/// A point count that `shape` does not admit draws nothing, so the result is empty: two points for
/// a line, arrow, rectangle or ellipse, four for a spline, and at least three for a polygon. The
/// domain rejects such a graphic (`Graphic::validate`, crates/domain/src/scene.rs:92-100), so an
/// empty list is unreachable from a document; it only keeps the renderer total for a hand-made
/// view.
#[must_use]
pub(super) fn graphic_marks(shape: GraphicShapeDto, points: &[PositionView]) -> Vec<GraphicMark> {
    let stored: Vec<Point> = points.iter().map(to_point).collect();
    match (shape, stored.as_slice()) {
        (GraphicShapeDto::Line, &[from, to]) => vec![segment(from, to)],
        (GraphicShapeDto::Arrow, &[from, tip]) => {
            let mut marks = vec![segment(from, tip)];
            marks.extend(arrow_head(from, tip));
            marks
        }
        (GraphicShapeDto::Rectangle, &[first, second]) => vec![GraphicMark::Rect {
            x: first.x.min(second.x),
            y: first.y.min(second.y),
            width: (first.x - second.x).abs(),
            height: (first.y - second.y).abs(),
        }],
        (GraphicShapeDto::Ellipse, &[first, second]) => vec![GraphicMark::Ellipse {
            cx: (first.x + second.x) / 2.0,
            cy: (first.y + second.y) / 2.0,
            rx: (first.x - second.x).abs() / 2.0,
            ry: (first.y - second.y).abs() / 2.0,
        }],
        (GraphicShapeDto::Polygon, [_, _, _, ..]) => vec![GraphicMark::Polygon { points: stored }],
        (GraphicShapeDto::Spline, &[_, _, _, _]) => vec![GraphicMark::Cubic { points: stored }],
        // Every shape is listed, so that a new shape must decide its point count here.
        (
            GraphicShapeDto::Line
            | GraphicShapeDto::Arrow
            | GraphicShapeDto::Rectangle
            | GraphicShapeDto::Ellipse
            | GraphicShapeDto::Polygon
            | GraphicShapeDto::Spline,
            _,
        ) => Vec::new(),
    }
}

/// The marks as the writer writes them: each number of each mark rounded to the hundredth by
/// `geometry::round2`, one number at a time, as `svg/writer.rs` writes it. A rectangle's corner and
/// size are rounded apart, not its far corner, because the browser adds them.
#[must_use]
pub(super) fn rounded(marks: &[GraphicMark]) -> Vec<GraphicMark> {
    marks
        .iter()
        .map(|mark| match mark {
            GraphicMark::Line { x1, y1, x2, y2 } => GraphicMark::Line {
                x1: geometry::round2(*x1),
                y1: geometry::round2(*y1),
                x2: geometry::round2(*x2),
                y2: geometry::round2(*y2),
            },
            GraphicMark::Head { points } => GraphicMark::Head {
                points: rounded_points(points),
            },
            GraphicMark::Rect {
                x,
                y,
                width,
                height,
            } => GraphicMark::Rect {
                x: geometry::round2(*x),
                y: geometry::round2(*y),
                width: geometry::round2(*width),
                height: geometry::round2(*height),
            },
            GraphicMark::Ellipse { cx, cy, rx, ry } => GraphicMark::Ellipse {
                cx: geometry::round2(*cx),
                cy: geometry::round2(*cy),
                rx: geometry::round2(*rx),
                ry: geometry::round2(*ry),
            },
            GraphicMark::Polygon { points } => GraphicMark::Polygon {
                points: rounded_points(points),
            },
            GraphicMark::Cubic { points } => GraphicMark::Cubic {
                points: rounded_points(points),
            },
        })
        .collect()
}

/// Each point with both coordinates rounded by `geometry::rounded`.
fn rounded_points(points: &[Point]) -> Vec<Point> {
    points.iter().copied().map(geometry::rounded).collect()
}

/// The box that the ink of `marks` covers, as `(min x, min y, max x, max y)` in pt, or `None` when
/// there are no marks. A line contributes its two ends, a head or a polygon its vertices, a
/// rectangle its two corners, an ellipse the corners of its bounding box, and a cubic its four
/// points. A cubic's control hull is covered, as the canvas's Fit covers it
/// (`structure-canvas.tsx:418`). The box then grows by `width / 2` on every side, which is the
/// stroke's exact reach for every mark but a cubic; for a cubic it only contains the stroke.
#[must_use]
pub(super) fn ink(marks: &[GraphicMark], width: f64) -> Option<(f64, f64, f64, f64)> {
    let mut edges: Option<(f64, f64, f64, f64)> = None;
    let mut include = |point: Point| {
        edges = Some(match edges {
            None => (point.x, point.y, point.x, point.y),
            Some((left, top, right, bottom)) => (
                left.min(point.x),
                top.min(point.y),
                right.max(point.x),
                bottom.max(point.y),
            ),
        });
    };
    for mark in marks {
        match mark {
            GraphicMark::Line { x1, y1, x2, y2 } => {
                include(Point { x: *x1, y: *y1 });
                include(Point { x: *x2, y: *y2 });
            }
            GraphicMark::Head { points }
            | GraphicMark::Polygon { points }
            | GraphicMark::Cubic { points } => {
                for point in points {
                    include(*point);
                }
            }
            GraphicMark::Rect {
                x,
                y,
                width: rect_width,
                height: rect_height,
            } => {
                include(Point { x: *x, y: *y });
                include(Point {
                    x: *x + *rect_width,
                    y: *y + *rect_height,
                });
            }
            GraphicMark::Ellipse { cx, cy, rx, ry } => {
                include(Point {
                    x: *cx - *rx,
                    y: *cy - *ry,
                });
                include(Point {
                    x: *cx + *rx,
                    y: *cy + *ry,
                });
            }
        }
    }
    let (left, top, right, bottom) = edges?;
    let half = width / 2.0;
    Some((left - half, top - half, right + half, bottom + half))
}

/// The stroke width of a graphic: its own override, or else the document's line width. A missing or
/// null override inherits the document setting (`graphicLineWidth`, `graphic-line-width.ts:5-7`,
/// given `settingsOf(document).line_width_pt` at `structure-canvas.tsx:1079`).
#[must_use]
pub(super) fn effective_width(line_width_pt: Option<f64>, document: f64) -> f64 {
    line_width_pt.unwrap_or(document)
}

/// The head of an arrow whose shaft runs from `from` to `tip`, or `None` when the shaft has no
/// length (`structure-canvas.tsx:1181-1184, 1204`). The length is `geometry::distance`, which the
/// canvas's `Math.hypot` may differ from by one ULP.
fn arrow_head(from: Point, tip: Point) -> Option<GraphicMark> {
    let dx = tip.x - from.x;
    let dy = tip.y - from.y;
    let length = geometry::distance(from, tip);
    if length > 0.0 {
        let ux = dx / length;
        let uy = dy / length;
        Some(GraphicMark::Head {
            points: vec![
                tip,
                Point {
                    x: tip.x - ux * GRAPHIC_ARROW_LENGTH - uy * GRAPHIC_ARROW_HALF_WIDTH,
                    y: tip.y - uy * GRAPHIC_ARROW_LENGTH + ux * GRAPHIC_ARROW_HALF_WIDTH,
                },
                Point {
                    x: tip.x - ux * GRAPHIC_ARROW_LENGTH + uy * GRAPHIC_ARROW_HALF_WIDTH,
                    y: tip.y - uy * GRAPHIC_ARROW_LENGTH - ux * GRAPHIC_ARROW_HALF_WIDTH,
                },
            ],
        })
    } else {
        None
    }
}

/// The straight segment from `from` to `to`.
fn segment(from: Point, to: Point) -> GraphicMark {
    GraphicMark::Line {
        x1: from.x,
        y1: from.y,
        x2: to.x,
        y2: to.y,
    }
}

/// A stored position as a layout point.
fn to_point(position: &PositionView) -> Point {
    Point {
        x: position.x,
        y: position.y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position(x: f64, y: f64) -> PositionView {
        PositionView { x, y }
    }

    fn point(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    #[test]
    fn an_arrow_head_is_six_long_and_three_wide_at_the_tip() {
        // The head starts 6 pt back from the tip, with its base corners 3 pt either side of
        // the shaft. The horizontal shaft checks the perpendicular's y-component and the
        // vertical one its x-component, so a sign error in either fails one of the directions.
        let cases = [
            (
                [position(0.0, 0.0), position(10.0, 0.0)],
                [point(10.0, 0.0), point(4.0, 3.0), point(4.0, -3.0)],
            ),
            (
                // A longer shaft than the horizontal case, so a head that grows with the shaft
                // cannot match both.
                [position(0.0, 0.0), position(0.0, 20.0)],
                [point(0.0, 20.0), point(-3.0, 14.0), point(3.0, 14.0)],
            ),
        ];
        for (stored, head) in cases {
            let shaft = GraphicMark::Line {
                x1: stored[0].x,
                y1: stored[0].y,
                x2: stored[1].x,
                y2: stored[1].y,
            };
            assert_eq!(
                graphic_marks(GraphicShapeDto::Arrow, &stored),
                vec![
                    shaft,
                    GraphicMark::Head {
                        points: head.to_vec(),
                    },
                ],
                "arrow from ({}, {}) to ({}, {})",
                stored[0].x,
                stored[0].y,
                stored[1].x,
                stored[1].y,
            );
        }
    }

    #[test]
    fn a_zero_length_arrow_has_a_shaft_and_no_head() {
        // With both ends equal there is no direction, so an unguarded head would hold NaN.
        // NaN never equals a finite value, so matching the finite expected shaft also shows
        // that every value is finite.
        let stored = [position(5.0, 5.0), position(5.0, 5.0)];
        assert_eq!(
            graphic_marks(GraphicShapeDto::Arrow, &stored),
            vec![GraphicMark::Line {
                x1: 5.0,
                y1: 5.0,
                x2: 5.0,
                y2: 5.0,
            }],
            "a zero-length arrow draws its shaft and no head",
        );
    }

    #[test]
    fn a_rectangle_and_ellipse_come_from_either_corner_order() {
        // Every pair spans x 2..8 and y 3..7. The second pair runs backwards on both axes,
        // and the third on x only.
        let corner_pairs = [
            [position(2.0, 3.0), position(8.0, 7.0)],
            [position(8.0, 7.0), position(2.0, 3.0)],
            [position(8.0, 3.0), position(2.0, 7.0)],
        ];
        for corners in corner_pairs {
            assert_eq!(
                graphic_marks(GraphicShapeDto::Rectangle, &corners),
                vec![GraphicMark::Rect {
                    x: 2.0,
                    y: 3.0,
                    width: 6.0,
                    height: 4.0,
                }],
                "rectangle with corners ({}, {}) and ({}, {})",
                corners[0].x,
                corners[0].y,
                corners[1].x,
                corners[1].y,
            );
            assert_eq!(
                graphic_marks(GraphicShapeDto::Ellipse, &corners),
                vec![GraphicMark::Ellipse {
                    cx: 5.0,
                    cy: 5.0,
                    rx: 3.0,
                    ry: 2.0,
                }],
                "ellipse with corners ({}, {}) and ({}, {})",
                corners[0].x,
                corners[0].y,
                corners[1].x,
                corners[1].y,
            );
        }
    }

    #[test]
    fn a_spline_keeps_its_four_stored_points_in_order() {
        let stored = [
            position(0.0, 0.0),
            position(2.0, 6.0),
            position(5.0, -3.0),
            position(8.0, 1.0),
        ];
        assert_eq!(
            graphic_marks(GraphicShapeDto::Spline, &stored),
            vec![GraphicMark::Cubic {
                points: vec![
                    point(0.0, 0.0),
                    point(2.0, 6.0),
                    point(5.0, -3.0),
                    point(8.0, 1.0),
                ],
            }],
            "a spline is one cubic through its four stored points, in stored order",
        );
    }

    #[test]
    fn a_polygon_keeps_its_vertices_in_order_without_a_closing_vertex() {
        let stored = [position(0.0, 0.0), position(10.0, 0.0), position(5.0, 8.0)];
        assert_eq!(
            graphic_marks(GraphicShapeDto::Polygon, &stored),
            vec![GraphicMark::Polygon {
                points: vec![point(0.0, 0.0), point(10.0, 0.0), point(5.0, 8.0)],
            }],
            "a polygon keeps its three vertices and adds no closing vertex",
        );
    }

    #[test]
    fn a_graphic_with_the_wrong_point_count_draws_nothing() {
        // Each count below is one that its shape does not admit: two points for a line, arrow,
        // rectangle or ellipse, four for a spline and at least three for a polygon. Such a graphic
        // must draw no marks rather than panic, so it has no ink box either.
        let one = [position(0.0, 0.0)];
        let two = [position(0.0, 0.0), position(10.0, 0.0)];
        let three = [position(0.0, 0.0), position(4.0, 0.0), position(4.0, 3.0)];
        let five = [
            position(0.0, 0.0),
            position(4.0, 0.0),
            position(4.0, 3.0),
            position(8.0, 3.0),
            position(8.0, 6.0),
        ];
        let cases: [(&str, GraphicShapeDto, &[PositionView]); 11] = [
            ("line, 1 point", GraphicShapeDto::Line, &one),
            ("line, 3 points", GraphicShapeDto::Line, &three),
            ("arrow, 1 point", GraphicShapeDto::Arrow, &one),
            ("arrow, 3 points", GraphicShapeDto::Arrow, &three),
            ("rectangle, 1 point", GraphicShapeDto::Rectangle, &one),
            ("rectangle, 3 points", GraphicShapeDto::Rectangle, &three),
            ("ellipse, 1 point", GraphicShapeDto::Ellipse, &one),
            ("ellipse, 3 points", GraphicShapeDto::Ellipse, &three),
            ("spline, 3 points", GraphicShapeDto::Spline, &three),
            ("spline, 5 points", GraphicShapeDto::Spline, &five),
            ("polygon, 2 points", GraphicShapeDto::Polygon, &two),
        ];
        for (name, shape, stored) in cases {
            let marks = graphic_marks(shape, stored);
            assert!(marks.is_empty(), "{name} draws nothing, got {marks:?}");
            assert_eq!(ink(&marks, 2.0), None, "{name} has no ink box");
        }
    }

    #[test]
    fn ink_grows_each_mark_by_half_the_line_width() {
        // Boxes are (min x, min y, max x, max y) of each shape's ink, grown by half the width
        // on every side: by 1 at width 2.0 and by 0.25 at width 0.5. The arrow's head reaches
        // y = -3 and y = 3, past the shaft. The spline's controls, not its ends, set the top
        // and the bottom.
        let cases = [
            (
                "a line",
                graphic_marks(
                    GraphicShapeDto::Line,
                    &[position(1.0, 2.0), position(5.0, 6.0)],
                ),
                (0.0, 1.0, 6.0, 7.0),
                (0.75, 1.75, 5.25, 6.25),
            ),
            (
                "an arrow",
                graphic_marks(
                    GraphicShapeDto::Arrow,
                    &[position(0.0, 0.0), position(10.0, 0.0)],
                ),
                (-1.0, -4.0, 11.0, 4.0),
                (-0.25, -3.25, 10.25, 3.25),
            ),
            (
                "an ellipse",
                graphic_marks(
                    GraphicShapeDto::Ellipse,
                    &[position(2.0, 3.0), position(8.0, 7.0)],
                ),
                (1.0, 2.0, 9.0, 8.0),
                (1.75, 2.75, 8.25, 7.25),
            ),
            (
                "a cubic",
                graphic_marks(
                    GraphicShapeDto::Spline,
                    &[
                        position(0.0, 0.0),
                        position(2.0, 6.0),
                        position(5.0, -3.0),
                        position(8.0, 1.0),
                    ],
                ),
                (-1.0, -4.0, 9.0, 7.0),
                (-0.25, -3.25, 8.25, 6.25),
            ),
        ];
        for (name, marks, wide, thin) in cases {
            assert_eq!(ink(&marks, 2.0), Some(wide), "{name} at width 2.0");
            assert_eq!(ink(&marks, 0.5), Some(thin), "{name} at width 0.5");
        }
    }

    #[test]
    fn an_explicit_width_overrides_the_document_width_and_none_inherits_it() {
        assert_eq!(
            effective_width(Some(2.0), 0.5),
            2.0,
            "an explicit width wins over a thinner document width",
        );
        assert_eq!(
            effective_width(Some(0.5), 2.0),
            0.5,
            "an explicit width wins over a thicker document width",
        );
        assert_eq!(
            effective_width(None, 2.0),
            2.0,
            "a missing width inherits the document width",
        );
    }

    #[test]
    fn rounded_marks_round_each_number_to_the_hundredth() {
        // Each number is rounded on its own, as the writer writes it. A rectangle's corner and size
        // are rounded apart: x 0.005 and width 0.005 become 0.01 and 0.01, so the browser draws the
        // far edge at 0.02.
        let marks = [
            GraphicMark::Rect {
                x: 0.004,
                y: 1.006,
                width: 10.006,
                height: 2.994,
            },
            GraphicMark::Rect {
                x: 0.005,
                y: 0.0,
                width: 0.005,
                height: 1.0,
            },
            GraphicMark::Ellipse {
                cx: 0.004,
                cy: 1.006,
                rx: 5.006,
                ry: 2.004,
            },
            GraphicMark::Line {
                x1: 0.004,
                y1: -0.004,
                x2: 1.006,
                y2: 1.994,
            },
            GraphicMark::Head {
                points: vec![point(1.006, 0.004), point(-2.994, 3.006)],
            },
            GraphicMark::Polygon {
                points: vec![point(0.004, 0.006), point(4.994, 0.0), point(2.5, 8.004)],
            },
            GraphicMark::Cubic {
                points: vec![
                    point(0.004, 0.0),
                    point(2.0, 6.006),
                    point(5.0, -3.0),
                    point(8.0, 1.004),
                ],
            },
        ];
        let expected = [
            GraphicMark::Rect {
                x: 0.0,
                y: 1.01,
                width: 10.01,
                height: 2.99,
            },
            GraphicMark::Rect {
                x: 0.01,
                y: 0.0,
                width: 0.01,
                height: 1.0,
            },
            GraphicMark::Ellipse {
                cx: 0.0,
                cy: 1.01,
                rx: 5.01,
                ry: 2.0,
            },
            GraphicMark::Line {
                x1: 0.0,
                y1: 0.0,
                x2: 1.01,
                y2: 1.99,
            },
            GraphicMark::Head {
                points: vec![point(1.01, 0.0), point(-2.99, 3.01)],
            },
            GraphicMark::Polygon {
                points: vec![point(0.0, 0.01), point(4.99, 0.0), point(2.5, 8.0)],
            },
            GraphicMark::Cubic {
                points: vec![
                    point(0.0, 0.0),
                    point(2.0, 6.01),
                    point(5.0, -3.0),
                    point(8.0, 1.0),
                ],
            },
        ];
        assert_eq!(
            rounded(&marks),
            expected,
            "each number rounds to the hundredth"
        );
    }
}
