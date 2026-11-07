//! Bond and stereo-mark geometry in document points, y growing downward. Each function mirrors one
//! rule of `apps/web/src/editor/geometry.ts`, named in its doc comment, with the same constants and
//! the same order of arithmetic, so that the two agree up to the parity tolerance.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::{BondEndDto, BondOrderDto, BondView, MarkDto};

/// Gap between a label box and a bond's ink, before half the line width is added (`LABEL_GAP`).
const LABEL_GAP: f64 = 1.4;
/// The built-in width of a bond line, the width the parallel spacing is measured against (`LINE_WIDTH`).
const LINE_WIDTH: f64 = 1.2;
/// A triple bond's outer lines, the farthest a bond's ink reaches from its axis (`WIDEST_OFFSET`).
const WIDEST_OFFSET: f64 = 3.4;
/// The offset of a double bond's two lines from its axis (`OFFSETS.double`).
const DOUBLE_OFFSET: f64 = 2.2;
/// How far a dative bond's arrowhead reaches beside the axis (`ARROWHEAD_HALF_WIDTH`).
const ARROWHEAD_HALF_WIDTH: f64 = 2.5;
/// How far an unknown bond's zigzag reaches beside the axis (`ZIGZAG_HALF_WIDTH`).
const ZIGZAG_HALF_WIDTH: f64 = 2.0;
/// How far an aromatic bond's dashed line runs beside its line (`INNER_OFFSET`).
const INNER_OFFSET: f64 = 3.0;
/// How much shorter the dashed line is at a bare vertex (`INNER_INSET`).
const INNER_INSET: f64 = 4.0;
/// The largest ring (in atoms) whose side an aromatic bond's inner line follows (`RING_LIMIT`).
const RING_LIMIT: usize = 8;
/// The longest a dative bond's arrowhead is (`Math.min(4, ...)` in `bondStrokes`).
const DATIVE_HEAD_LENGTH: f64 = 4.0;
/// An unknown bond's zigzag has one tooth per this much length, and at least `ZIGZAG_MIN_TEETH`.
const ZIGZAG_RUN_PER_TOOTH: f64 = 3.0;
const ZIGZAG_MIN_TEETH: f64 = 2.0;
/// Full width of a wedge at its wide end (`MARK_STYLE.wedgeWidth`).
const WEDGE_WIDTH: f64 = 4.0;
/// Distance between the centres of two stripes of a hashed wedge (`MARK_STYLE.hashSpacing`).
const HASH_SPACING: f64 = 2.7;
/// How far a wavy bond's curve bulges from its axis (`MARK_STYLE.waveAmplitude`).
const WAVE_AMPLITUDE: f64 = 1.8;
/// The length along a wavy bond of one arc (`MARK_STYLE.waveArc`).
const WAVE_ARC: f64 = 4.0;
/// The most stripes or arcs one bond gets (`MARK_STYLE.maxParts`).
const MAX_PARTS: f64 = 400.0;
/// A bond's lean must exceed this for its atom to count as leaning right (`leaningRight`).
const LEAN_THRESHOLD: f64 = 0.01;

/// A point in document points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// A rectangle in document points; `x`, `y` is its top-left corner (`Box` in the canvas).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct LabelBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// How a straight line is dashed. The dash lengths belong to the writer, which owns the markup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    Solid,
    Dashed,
    Dotted,
}

/// One stroke of a drawn bond (`Stroke` in the canvas): a line, a polyline (an arrowhead or a
/// zigzag), a filled polygon (a wedge), cross stripes (a hashed wedge), or a wave from `from` to
/// `to`, `half_waves` arcs bulging `amplitude` to alternate sides.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum Stroke {
    Line {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        pattern: Pattern,
    },
    Polyline {
        points: Vec<Point>,
    },
    Polygon {
        points: Vec<Point>,
    },
    Stripes {
        segments: Vec<(Point, Point)>,
    },
    Wave {
        from: Point,
        to: Point,
        #[serde(rename = "halfWaves")]
        half_waves: u32,
        amplitude: f64,
    },
}

/// The two ends of a bond in one drawing: where its atoms are, and the label box each keeps clear.
#[derive(Clone, Copy, Debug)]
pub(super) struct Ends {
    pub(super) begin: Point,
    pub(super) end: Point,
    pub(super) begin_box: Option<LabelBox>,
    pub(super) end_box: Option<LabelBox>,
}

/// The parallel-line spacing and the line width of one drawing.
#[derive(Clone, Copy, Debug)]
pub(super) struct Lines {
    pub(super) spacing: f64,
    pub(super) width: f64,
}

/// One command of a stroked path: a move, a quadratic curve, or a smooth quadratic continuation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum PathStep {
    Move(Point),
    Curve { control: Point, end: Point },
    Smooth(Point),
}

/// JavaScript's `Math.round`: the integer nearest to `value`, a tie going toward +infinity. Unlike
/// `f64::round`, a negative half rounds up (`-2.5` is `-2`), and so does the rest of the canvas.
pub(super) fn js_round(value: f64) -> f64 {
    let floor = value.floor();
    if value - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

/// `round` in geometry.ts: `Math.round(value * 100) / 100`, the value to the nearest hundredth.
pub(super) fn round2(value: f64) -> f64 {
    js_round(value * 100.0) / 100.0
}

/// A point with both coordinates rounded by [`round2`].
pub(super) fn rounded(point: Point) -> Point {
    Point {
        x: round2(point.x),
        y: round2(point.y),
    }
}

/// The distance between two points (`distance`). The canvas uses `Math.hypot`, which can differ
/// from this by one ULP; the parity check allows for it.
pub(super) fn distance(a: Point, b: Point) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

/// `Math.sign`: 1, -1, or 0 for zero (Rust's `signum` gives 1 for zero).
fn js_sign(value: f64) -> f64 {
    if value > 0.0 {
        1.0
    } else if value < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// A label box moved to `at` (`placed`): `bounds` is relative to an atom at `at`.
pub(super) fn placed(bounds: LabelBox, at: Point) -> LabelBox {
    LabelBox {
        x: bounds.x + at.x,
        y: bounds.y + at.y,
        width: bounds.width,
        height: bounds.height,
    }
}

/// The smallest box that encloses every box in `boxes` (`union`). Its width is `right - left`, as
/// in the canvas, so a single box comes back with its width recomputed from its edges.
pub(super) fn union(boxes: &[LabelBox]) -> LabelBox {
    let left = boxes
        .iter()
        .map(|bounds| bounds.x)
        .fold(f64::INFINITY, f64::min);
    let top = boxes
        .iter()
        .map(|bounds| bounds.y)
        .fold(f64::INFINITY, f64::min);
    let right = boxes
        .iter()
        .map(|bounds| bounds.x + bounds.width)
        .fold(f64::NEG_INFINITY, f64::max);
    let bottom = boxes
        .iter()
        .map(|bounds| bounds.y + bounds.height)
        .fold(f64::NEG_INFINITY, f64::max);
    LabelBox {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    }
}

/// `box` grown by `by` on every side (`grown`).
fn grown(bounds: LabelBox, by: f64) -> LabelBox {
    LabelBox {
        x: bounds.x - by,
        y: bounds.y - by,
        width: bounds.width + 2.0 * by,
        height: bounds.height + 2.0 * by,
    }
}

/// How far the parallel spacing is multiplied (`bondSpacing`): thinner lines keep the built-in one.
pub(super) fn bond_spacing(line_width: f64) -> f64 {
    (line_width / LINE_WIDTH).max(1.0)
}

/// How far the ink of any bond reaches from its axis, its caps included (`strokeReach`).
pub(super) fn stroke_reach(line_width: f64, spacing: f64) -> f64 {
    (WIDEST_OFFSET * spacing)
        .max(ARROWHEAD_HALF_WIDTH)
        .max(ZIGZAG_HALF_WIDTH)
        + line_width / 2.0
}

/// The number of bonds at each atom, in one pass over the bonds (`degrees`).
pub(super) fn degrees(bonds: &[BondView]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for bond in bonds {
        *counts.entry(bond.begin.clone()).or_insert(0) += 1;
        *counts.entry(bond.end.clone()).or_insert(0) += 1;
    }
    counts
}

/// The atoms whose bonds lean right, so that their stated hydrogens go on the left (`leaningRight`).
/// A bond with no length, or an atom without a position, contributes nothing.
pub(super) fn leaning_right(
    bonds: &[BondView],
    positions: &BTreeMap<String, Point>,
) -> BTreeSet<String> {
    let mut lean: BTreeMap<&str, f64> = BTreeMap::new();
    for bond in bonds {
        let (Some(&begin), Some(&end)) = (positions.get(&bond.begin), positions.get(&bond.end))
        else {
            continue;
        };
        let length = distance(begin, end);
        if length == 0.0 {
            continue;
        }
        let dx = (end.x - begin.x) / length;
        *lean.entry(bond.begin.as_str()).or_insert(0.0) += dx;
        *lean.entry(bond.end.as_str()).or_insert(0.0) -= dx;
    }
    lean.into_iter()
        .filter(|(_, sum)| *sum > LEAN_THRESHOLD)
        .map(|(id, _)| id.to_owned())
        .collect()
}

/// For each bond in a ring of at most [`RING_LIMIT`] atoms, the atoms of the smallest such ring
/// through it (`bondRings`). Breadth first from begin to end without the bond, in bond-list order.
pub(super) fn bond_rings(bonds: &[BondView]) -> BTreeMap<String, Vec<String>> {
    // Each atom's neighbours as (atom, bond), in bond-list order.
    let mut around: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
    for bond in bonds {
        around
            .entry(bond.begin.as_str())
            .or_default()
            .push((bond.end.as_str(), bond.id.as_str()));
        around
            .entry(bond.end.as_str())
            .or_default()
            .push((bond.begin.as_str(), bond.id.as_str()));
    }
    let mut rings = BTreeMap::new();
    for bond in bonds {
        let mut previous: BTreeMap<&str, Option<&str>> =
            BTreeMap::from([(bond.begin.as_str(), None)]);
        let mut frontier = vec![bond.begin.as_str()];
        let mut steps = 1;
        while steps < RING_LIMIT
            && !frontier.is_empty()
            && !previous.contains_key(bond.end.as_str())
        {
            let mut next = Vec::new();
            for &atom in &frontier {
                for &(other, via) in around.get(atom).into_iter().flatten() {
                    if via == bond.id.as_str() || previous.contains_key(other) {
                        continue;
                    }
                    previous.insert(other, Some(atom));
                    next.push(other);
                }
            }
            frontier = next;
            steps += 1;
        }
        if !previous.contains_key(bond.end.as_str()) {
            continue;
        }
        let mut ring = Vec::new();
        let mut atom = Some(bond.end.as_str());
        while let Some(current) = atom {
            ring.push(current.to_owned());
            atom = previous.get(current).copied().flatten();
        }
        rings.insert(bond.id.clone(), ring);
    }
    rings
}

/// Which side of each bond its inner line goes: +1 or -1 (`bondSides`). A bond in a ring takes the
/// side of the ring's centre; any other takes the side where more of its neighbours lie, and +1
/// without neighbours. A bond whose atoms have no positions, or no length, has no side.
pub(super) fn bond_sides(
    bonds: &[BondView],
    positions: &BTreeMap<String, Point>,
    rings: &BTreeMap<String, Vec<String>>,
) -> BTreeMap<String, f64> {
    let mut around: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for bond in bonds {
        around
            .entry(bond.begin.as_str())
            .or_default()
            .push(bond.end.as_str());
        around
            .entry(bond.end.as_str())
            .or_default()
            .push(bond.begin.as_str());
    }
    let mut sides = BTreeMap::new();
    for bond in bonds {
        let (Some(&begin), Some(&end)) = (positions.get(&bond.begin), positions.get(&bond.end))
        else {
            continue;
        };
        let length = distance(begin, end);
        if length == 0.0 {
            continue;
        }
        let normal = Point {
            x: -(end.y - begin.y) / length,
            y: (end.x - begin.x) / length,
        };
        let toward = |point: Point, from: Point| {
            (point.x - from.x) * normal.x + (point.y - from.y) * normal.y
        };
        if let Some(ring) = rings.get(&bond.id) {
            let members: Vec<Point> = ring
                .iter()
                .filter_map(|id| positions.get(id).copied())
                .collect();
            if !members.is_empty() {
                let count = members.len() as f64;
                let centre = Point {
                    x: members.iter().fold(0.0, |sum, point| sum + point.x) / count,
                    y: members.iter().fold(0.0, |sum, point| sum + point.y) / count,
                };
                let lean = toward(centre, begin);
                if lean != 0.0 {
                    sides.insert(bond.id.clone(), if lean < 0.0 { -1.0 } else { 1.0 });
                    continue;
                }
            }
        }
        let mut lean = 0.0;
        for (from, other, at) in [
            (bond.begin.as_str(), bond.end.as_str(), begin),
            (bond.end.as_str(), bond.begin.as_str(), end),
        ] {
            for &id in around.get(from).into_iter().flatten() {
                let Some(&neighbour) = positions.get(id) else {
                    continue;
                };
                if id == other {
                    continue;
                }
                lean += js_sign(toward(neighbour, at));
            }
        }
        sides.insert(bond.id.clone(), if lean < 0.0 { -1.0 } else { 1.0 });
    }
    sides
}

/// A mark the renderer draws on a bond as one stroke set. `None` from [`drawn_mark`] means the bond
/// draws its plain order instead, because a mark does not fit any order but its own (`markFits`).
/// A crossed mark on a double bond is drawn by [`crossed_strokes`] instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DrawnMark {
    Wedge { narrow_at_begin: bool },
    Hash { narrow_at_begin: bool },
    Wavy,
}

fn drawn_mark(mark: &MarkDto, order: BondOrderDto) -> Option<DrawnMark> {
    if order != BondOrderDto::Single {
        return None;
    }
    match mark {
        MarkDto::Wedge { narrow } => Some(DrawnMark::Wedge {
            narrow_at_begin: *narrow == BondEndDto::Begin,
        }),
        MarkDto::Hash { narrow } => Some(DrawnMark::Hash {
            narrow_at_begin: *narrow == BondEndDto::Begin,
        }),
        MarkDto::Wavy {} => Some(DrawnMark::Wavy),
        MarkDto::Crossed {} => None,
    }
}

/// The strokes of a marked bond, trimmed where it meets a label box (`markStrokes`, without the
/// crossed branch). The taper is measured from the narrow end of the whole bond, so a cut bond
/// keeps its slope, and the stripes keep their places.
fn mark_strokes(mark: DrawnMark, ends: Ends, line_width: f64) -> Vec<Stroke> {
    let Ends {
        begin,
        end,
        begin_box,
        end_box,
    } = ends;
    let length = distance(begin, end);
    if length == 0.0 {
        return Vec::new();
    }
    let gap = LABEL_GAP + line_width / 2.0;
    let ux = (end.x - begin.x) / length;
    let uy = (end.y - begin.y) / length;
    let along = |at: f64| Point {
        x: begin.x + ux * at,
        y: begin.y + uy * at,
    };
    let beside = |point: Point, offset: f64| Point {
        x: round2(point.x - uy * offset),
        y: round2(point.y + ux * offset),
    };
    let Some((a, b)) = clipped(begin, end, begin_box, end_box, gap) else {
        return Vec::new();
    };
    let from = distance(begin, a);
    let to = distance(begin, b);
    let (narrow_at_begin, hashed) = match mark {
        DrawnMark::Wavy => {
            let half_waves = js_round((to - from) / WAVE_ARC).clamp(2.0, MAX_PARTS);
            return vec![Stroke::Wave {
                from: rounded(a),
                to: rounded(b),
                half_waves: half_waves as u32,
                amplitude: WAVE_AMPLITUDE,
            }];
        }
        DrawnMark::Wedge { narrow_at_begin } => (narrow_at_begin, false),
        DrawnMark::Hash { narrow_at_begin } => (narrow_at_begin, true),
    };
    let from_narrow = |at: f64| if narrow_at_begin { at } else { length - at };
    let half = |at: f64| (WEDGE_WIDTH.max(line_width) / 2.0) * (from_narrow(at) / length);
    if !hashed {
        // A cut end still at the narrow point is one corner, not two.
        let corners = |at: f64| -> Vec<Point> {
            let width = half(at);
            if width < 0.005 {
                vec![beside(along(at), 0.0)]
            } else {
                vec![beside(along(at), width), beside(along(at), -width)]
            }
        };
        let mut points = corners(from);
        let mut far = corners(to);
        far.reverse();
        points.extend(far);
        return vec![Stroke::Polygon { points }];
    }
    let pitch = HASH_SPACING.max(length / MAX_PARTS);
    let stripe = |at: f64| -> (Point, Point) {
        let reach = (line_width / 2.0).max(half(at));
        (beside(along(at), reach), beside(along(at), -reach))
    };
    let mut segments = Vec::new();
    let mut k = 0usize;
    while (k as f64 + 0.5) * pitch <= length {
        let place = (k as f64 + 0.5) * pitch;
        let at = if narrow_at_begin {
            place
        } else {
            length - place
        };
        if (from..=to).contains(&at) {
            segments.push(stripe(at));
        }
        k += 1;
    }
    // A short visible piece between two stripe places still shows the mark.
    if segments.is_empty() {
        segments.push(stripe((from + to) / 2.0));
    }
    vec![Stroke::Stripes { segments }]
}

/// The two lines of a crossed double bond (the crossed branch of `markStrokes`): each runs from one
/// side of the axis at the begin to the other side at the end, so the two cross midway. Each line
/// is cut where it meets a label box (`clippedAcross`).
fn crossed_strokes(ends: Ends, lines: Lines) -> Vec<Stroke> {
    let Ends {
        begin,
        end,
        begin_box,
        end_box,
    } = ends;
    let length = distance(begin, end);
    if length == 0.0 {
        return Vec::new();
    }
    let gap = LABEL_GAP + lines.width / 2.0;
    let ux = (end.x - begin.x) / length;
    let uy = (end.y - begin.y) / length;
    let beside = |point: Point, offset: f64| Point {
        x: round2(point.x - uy * offset),
        y: round2(point.y + ux * offset),
    };
    let offset = DOUBLE_OFFSET * lines.spacing;
    [offset, -offset]
        .into_iter()
        .filter_map(|side| {
            let (a, b) = clipped_across(
                beside(begin, side),
                beside(end, -side),
                begin_box,
                end_box,
                gap,
            )?;
            Some(segment_stroke((rounded(a), rounded(b)), Pattern::Solid))
        })
        .collect()
}

/// The part of the segment from `a` to `b` between two grown label boxes: from where it last leaves
/// the box at `a` to where it first enters the box at `b`, wherever along the segment that is
/// (`clippedAcross`). Unlike [`clipped`], a box trims the segment even when the segment starts
/// outside it, and a box the segment does not meet trims nothing.
fn clipped_across(
    a: Point,
    b: Point,
    a_box: Option<LabelBox>,
    b_box: Option<LabelBox>,
    gap: f64,
) -> Option<(Point, Point)> {
    let length = distance(a, b);
    let at = |t: f64| Point {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
    };
    // Where the whole line runs inside a grown box, if that part meets the segment at all.
    let met = |bounds: Option<LabelBox>| {
        bounds
            .and_then(|bounds| inside(a, b, grown(bounds, gap)))
            .filter(|&(enter, leave)| leave >= 0.0 && enter <= 1.0)
    };
    let start = met(a_box).map_or(0.0, |(_, leave)| leave.clamp(0.0, 1.0));
    let end = met(b_box).map_or(1.0, |(enter, _)| enter.clamp(0.0, 1.0));
    (end > start && length > 0.0).then(|| (at(start), at(end)))
}

/// The part of the segment from `a` to `b` outside both grown label boxes, from where it leaves
/// the box at `a` to where it enters the box at `b` (`clipped`). Only a box the segment starts or
/// ends in trims it; one that merely passes a box does not. None when no visible part is left.
fn clipped(
    a: Point,
    b: Point,
    a_box: Option<LabelBox>,
    b_box: Option<LabelBox>,
    gap: f64,
) -> Option<(Point, Point)> {
    let length = distance(a, b);
    let at = |t: f64| Point {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
    };
    let from_a = a_box.and_then(|bounds| inside(a, b, grown(bounds, gap)));
    let into_b = b_box.and_then(|bounds| inside(a, b, grown(bounds, gap)));
    let start = match from_a {
        Some((enter, leave)) if enter <= 0.0 => leave.clamp(0.0, 1.0),
        _ => 0.0,
    };
    let end = match into_b {
        Some((enter, leave)) if leave >= 1.0 => enter.clamp(0.0, 1.0),
        _ => 1.0,
    };
    (end > start && length > 0.0).then(|| (at(start), at(end)))
}

/// Where the line through `a` (at 0) and `b` (at 1) runs inside `bounds`: its entry and exit
/// parameters, or None when it misses the box (`inside`).
fn inside(a: Point, b: Point, bounds: LabelBox) -> Option<(f64, f64)> {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let edges = [
        (-dx, a.x - bounds.x),
        (dx, bounds.x + bounds.width - a.x),
        (-dy, a.y - bounds.y),
        (dy, bounds.y + bounds.height - a.y),
    ];
    let mut enter = f64::NEG_INFINITY;
    let mut leave = f64::INFINITY;
    for (direction, room) in edges {
        if direction == 0.0 {
            if room < 0.0 {
                return None;
            }
            continue;
        }
        let t = room / direction;
        if direction < 0.0 {
            enter = enter.max(t);
        } else {
            leave = leave.min(t);
        }
    }
    (enter <= leave).then_some((enter, leave))
}

/// A line stroke from `a` to `b` in the given pattern.
fn segment_stroke((a, b): (Point, Point), pattern: Pattern) -> Stroke {
    Stroke::Line {
        x1: a.x,
        y1: a.y,
        x2: b.x,
        y2: b.y,
        pattern,
    }
}

/// The strokes that draw `bond` (`bondStrokes`): each kind of bond has its own look, lines offset
/// by `lines.spacing`, trimmed at the label boxes; a bond whose mark is drawn is drawn as the mark.
/// `side` (+1 or -1) is the side of an aromatic bond's inner line.
pub(super) fn bond_strokes(bond: &BondView, ends: Ends, side: f64, lines: Lines) -> Vec<Stroke> {
    let Ends {
        begin,
        end,
        begin_box,
        end_box,
    } = ends;
    let length = distance(begin, end);
    if length == 0.0 {
        return Vec::new();
    }
    if let (Some(MarkDto::Crossed {}), BondOrderDto::Double) = (bond.mark.as_ref(), bond.order) {
        return crossed_strokes(ends, lines);
    }
    if let Some(mark) = bond
        .mark
        .as_ref()
        .and_then(|mark| drawn_mark(mark, bond.order))
    {
        return mark_strokes(mark, ends, lines.width);
    }
    let gap = LABEL_GAP + lines.width / 2.0;
    let ux = (end.x - begin.x) / length;
    let uy = (end.y - begin.y) / length;
    let shifted = |point: Point, offset: f64| Point {
        x: point.x - uy * offset,
        y: point.y + ux * offset,
    };
    let clip = |a: Point, b: Point| clipped(a, b, begin_box, end_box, gap);
    // One line of the bond, offset from its axis; `inset` pulls its ends back at bare vertices.
    let line = |offset: f64, inset: f64| -> Option<(Point, Point)> {
        let pulled = inset.min(length / 4.0);
        let from = if begin_box.is_some() {
            begin
        } else {
            Point {
                x: begin.x + ux * pulled,
                y: begin.y + uy * pulled,
            }
        };
        let to = if end_box.is_some() {
            end
        } else {
            Point {
                x: end.x - ux * pulled,
                y: end.y - uy * pulled,
            }
        };
        let (a, b) = clip(shifted(from, offset), shifted(to, offset))?;
        Some((rounded(a), rounded(b)))
    };
    let drawn = |offset: f64, pattern: Pattern, inset: f64| {
        line(offset, inset).map(|segment| segment_stroke(segment, pattern))
    };
    let parallel = |offsets: &[f64]| -> Vec<Stroke> {
        offsets
            .iter()
            .filter_map(|&offset| drawn(offset * lines.spacing, Pattern::Solid, 0.0))
            .collect()
    };
    match bond.order {
        BondOrderDto::Single => parallel(&[0.0]),
        BondOrderDto::Double => parallel(&[-DOUBLE_OFFSET, DOUBLE_OFFSET]),
        BondOrderDto::Triple => parallel(&[-WIDEST_OFFSET, 0.0, WIDEST_OFFSET]),
        BondOrderDto::Aromatic => [
            drawn(0.0, Pattern::Solid, 0.0),
            drawn(
                side * INNER_OFFSET * lines.spacing,
                Pattern::Dashed,
                INNER_INSET,
            ),
        ]
        .into_iter()
        .flatten()
        .collect(),
        BondOrderDto::Query => drawn(0.0, Pattern::Dotted, 0.0).into_iter().collect(),
        BondOrderDto::Dative => {
            // The bond points away from its donor: a line and an arrowhead at its far end.
            let Some((start, tip)) = line(0.0, 0.0) else {
                return Vec::new();
            };
            let size = DATIVE_HEAD_LENGTH.min(distance(start, tip));
            let back = Point {
                x: tip.x - ux * size,
                y: tip.y - uy * size,
            };
            let head = [
                shifted(back, ARROWHEAD_HALF_WIDTH),
                tip,
                shifted(back, -ARROWHEAD_HALF_WIDTH),
            ];
            vec![
                segment_stroke((start, tip), Pattern::Solid),
                Stroke::Polyline {
                    points: head.map(rounded).to_vec(),
                },
            ]
        }
        BondOrderDto::Unknown => {
            let Some((a, b)) = clip(begin, end) else {
                return Vec::new();
            };
            let run = distance(a, b);
            let teeth = js_round(run / ZIGZAG_RUN_PER_TOOTH).max(ZIGZAG_MIN_TEETH);
            let last = teeth as usize;
            let points = (0..=last)
                .map(|index| {
                    let t = index as f64 / teeth;
                    let offset = if index == 0 || index == last {
                        0.0
                    } else if index % 2 == 0 {
                        -ZIGZAG_HALF_WIDTH
                    } else {
                        ZIGZAG_HALF_WIDTH
                    };
                    rounded(shifted(
                        Point {
                            x: a.x + (b.x - a.x) * t,
                            y: a.y + (b.y - a.y) * t,
                        },
                        offset,
                    ))
                })
                .collect();
            vec![Stroke::Polyline { points }]
        }
    }
}

/// The path of a wave stroke (`wavePath`) as steps: `half_waves` quadratic arcs along the axis from
/// `from` to `to`, each bulging `amplitude` to the side opposite its neighbour's, the slope
/// continuous where they meet and both ends on the axis.
pub(super) fn wave_steps(from: Point, to: Point, half_waves: u32, amplitude: f64) -> Vec<PathStep> {
    let length = distance(from, to);
    if length == 0.0 {
        return vec![PathStep::Move(from)];
    }
    let nx = -(to.y - from.y) / length;
    let ny = (to.x - from.x) / length;
    let halves = f64::from(half_waves);
    let node = |index: u32| {
        rounded(Point {
            x: from.x + ((to.x - from.x) * f64::from(index)) / halves,
            y: from.y + ((to.y - from.y) * f64::from(index)) / halves,
        })
    };
    let first = node(0);
    let next = node(1);
    // A quadratic arc peaks half way to its control point.
    let control = rounded(Point {
        x: (first.x + next.x) / 2.0 + 2.0 * amplitude * nx,
        y: (first.y + next.y) / 2.0 + 2.0 * amplitude * ny,
    });
    let mut steps = vec![
        PathStep::Move(first),
        PathStep::Curve { control, end: next },
    ];
    steps.extend((2..=half_waves).map(|index| PathStep::Smooth(node(index))));
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_round_breaks_ties_toward_positive_infinity() {
        // Math.round(2.5) is 3 and Math.round(-2.5) is -2; f64::round would give -3.
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(-2.6), -3.0);
        // The largest double below one half rounds down, as Math.round does.
        assert_eq!(js_round(0.499_999_999_999_999_94), 0.0);
    }

    #[test]
    fn round2_is_math_round_of_hundredths() {
        // -0.025 * 100 is -2.5, which rounds to -2 in JavaScript.
        assert_eq!(round2(-0.025), -0.02);
        assert_eq!(round2(0.125), 0.13);
        assert_eq!(round2(1.004), 1.0);
    }

    #[test]
    fn bond_spacing_grows_only_past_the_built_in_width() {
        assert_eq!(bond_spacing(1.2), 1.0);
        assert_eq!(bond_spacing(0.6), 1.0);
        assert_eq!(bond_spacing(2.4), 2.0);
    }

    #[test]
    fn union_recomputes_the_width_from_the_edges() {
        let bounds = LabelBox {
            x: 0.1,
            y: 0.2,
            width: 3.0,
            height: 4.0,
        };
        let joined = union(&[bounds]);
        assert_eq!(joined.x, bounds.x);
        assert_eq!(joined.width, (bounds.x + bounds.width) - bounds.x);
        assert_eq!(joined.height, (bounds.y + bounds.height) - bounds.y);
    }

    fn bond(id: &str, begin: &str, end: &str, order: BondOrderDto) -> BondView {
        BondView {
            id: id.to_owned(),
            begin: begin.to_owned(),
            end: end.to_owned(),
            order,
            query: None,
            mark: None,
        }
    }

    #[test]
    fn a_bond_starts_past_a_label_box_by_the_label_gap_and_half_the_line_width() {
        // The box's right edge is at x = 0, so the line starts 1.4 + 1.2 / 2 = 2.0 past it.
        let begin_box = LabelBox {
            x: -5.0,
            y: -5.0,
            width: 5.0,
            height: 10.0,
        };
        let ends = Ends {
            begin: Point { x: 0.0, y: 0.0 },
            end: Point { x: 20.0, y: 0.0 },
            begin_box: Some(begin_box),
            end_box: None,
        };
        let lines = Lines {
            spacing: 1.0,
            width: 1.2,
        };
        let strokes = bond_strokes(
            &bond("b1", "a1", "a2", BondOrderDto::Single),
            ends,
            1.0,
            lines,
        );
        let [Stroke::Line { x1, .. }] = strokes.as_slice() else {
            panic!("a single bond draws one line, got {strokes:?}");
        };
        assert_eq!(*x1, 2.0, "the line starts where the label gap ends");
    }

    #[test]
    fn a_ring_bond_takes_the_side_of_the_ring_centre() {
        // A regular hexagon listed in order around its centre, so each bond has the centre on its
        // left, which is side +1.
        let positions: BTreeMap<String, Point> = (0..6)
            .map(|index| {
                let angle = f64::from(index) * std::f64::consts::FRAC_PI_3;
                (
                    format!("a{index}"),
                    Point {
                        x: angle.cos(),
                        y: angle.sin(),
                    },
                )
            })
            .collect();
        let bonds: Vec<BondView> = (0..6)
            .map(|index| {
                bond(
                    &format!("b{index}"),
                    &format!("a{index}"),
                    &format!("a{}", (index + 1) % 6),
                    BondOrderDto::Aromatic,
                )
            })
            .collect();
        let rings = bond_rings(&bonds);
        let sides = bond_sides(&bonds, &positions, &rings);
        for ring_bond in &bonds {
            assert_eq!(
                sides[&ring_bond.id], 1.0,
                "bond {} lies on a ring whose centre is on its left",
                ring_bond.id
            );
        }
    }

    fn crossed_double_bond(begin_box: Option<LabelBox>) -> (BondView, Ends, Lines) {
        let mut bond = bond("b1", "a1", "a2", BondOrderDto::Double);
        bond.mark = Some(MarkDto::Crossed {});
        let ends = Ends {
            begin: Point { x: 0.0, y: 0.0 },
            end: Point { x: 26.0, y: 0.0 },
            begin_box,
            end_box: None,
        };
        let lines = Lines {
            spacing: 1.0,
            width: 1.2,
        };
        (bond, ends, lines)
    }

    #[test]
    fn a_crossed_double_bond_draws_two_lines_that_cross_midway() {
        // Each line leaves one side of the axis at the begin for the other side at the end, at
        // offsets of 2.2 (the double bond's), so the two lines cross at the middle of the bond.
        let (bond, ends, lines) = crossed_double_bond(None);
        assert_eq!(
            bond_strokes(&bond, ends, 1.0, lines),
            vec![
                Stroke::Line {
                    x1: 0.0,
                    y1: 2.2,
                    x2: 26.0,
                    y2: -2.2,
                    pattern: Pattern::Solid,
                },
                Stroke::Line {
                    x1: 0.0,
                    y1: -2.2,
                    x2: 26.0,
                    y2: 2.2,
                    pattern: Pattern::Solid,
                },
            ]
        );
    }

    #[test]
    fn a_crossed_line_that_starts_beside_a_label_box_is_cut_where_it_leaves_it() {
        // The box grown by the gap of 2.0 spans x from 2 to 7 and y from -3 to 3. Each line starts
        // at x = 0, outside it, crosses it, and is cut at x = 7, where it leaves the box, 7 / 26 of
        // the bond along. There the line is 2.2 - 4.4 * 7 / 26 = 1.0154 from the axis (1.02 rounded).
        let (bond, ends, lines) = crossed_double_bond(Some(LabelBox {
            x: 4.0,
            y: -1.0,
            width: 1.0,
            height: 2.0,
        }));
        assert_eq!(
            bond_strokes(&bond, ends, 1.0, lines),
            vec![
                Stroke::Line {
                    x1: 7.0,
                    y1: 1.02,
                    x2: 26.0,
                    y2: -2.2,
                    pattern: Pattern::Solid,
                },
                Stroke::Line {
                    x1: 7.0,
                    y1: -1.02,
                    x2: 26.0,
                    y2: 2.2,
                    pattern: Pattern::Solid,
                },
            ]
        );
    }

    #[test]
    fn a_crossed_single_bond_is_drawn_as_its_plain_line() {
        // A crossed mark fits a double bond only (markFits), so a single bond keeps its own line.
        let mut single = bond("b1", "a1", "a2", BondOrderDto::Single);
        single.mark = Some(MarkDto::Crossed {});
        let ends = Ends {
            begin: Point { x: 0.0, y: 0.0 },
            end: Point { x: 26.0, y: 0.0 },
            begin_box: None,
            end_box: None,
        };
        let lines = Lines {
            spacing: 1.0,
            width: 1.2,
        };
        assert_eq!(
            bond_strokes(&single, ends, 1.0, lines),
            vec![Stroke::Line {
                x1: 0.0,
                y1: 0.0,
                x2: 26.0,
                y2: 0.0,
                pattern: Pattern::Solid,
            }]
        );
    }
}
