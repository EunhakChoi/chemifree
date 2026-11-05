//! Rigid scene-object arrangement from geometry-only bounds.

use super::SelectionScope;
use crate::document::{EditError, ObjectId, Point, Vector};

/// Six alignments against the union of the selected units' original bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlignMode {
    Left,
    HorizontalCenter,
    Right,
    Top,
    VerticalCenter,
    Bottom,
}

/// Axis for equal-gap distribution, ordered by leading bound and then object identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DistributionAxis {
    Horizontal,
    Vertical,
}

/// An arrangement translates each outermost scene object rigidly; it never changes its shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectArrangement {
    Align(AlignMode),
    Distribute(DistributionAxis),
}

/// Geometry-only page-coordinate bounds. Atom positions and graphic points count; fonts,
/// labels, stroke width, arrowheads and other rendered extents do not.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Bounds {
    pub(crate) fn from_points(points: impl IntoIterator<Item = Point>) -> Option<Self> {
        let mut points = points.into_iter();
        let first = points.next()?;
        let mut bounds = Self {
            left: first.x(),
            right: first.x(),
            top: first.y(),
            bottom: first.y(),
        };
        for point in points {
            bounds.left = bounds.left.min(point.x());
            bounds.right = bounds.right.max(point.x());
            bounds.top = bounds.top.min(point.y());
            bounds.bottom = bounds.bottom.max(point.y());
        }
        Some(bounds)
    }
    fn leading(self, axis: DistributionAxis) -> f64 {
        match axis {
            DistributionAxis::Horizontal => self.left,
            DistributionAxis::Vertical => self.top,
        }
    }
    fn trailing(self, axis: DistributionAxis) -> f64 {
        match axis {
            DistributionAxis::Horizontal => self.right,
            DistributionAxis::Vertical => self.bottom,
        }
    }
}

/// A translation evaluated from its anchors, avoiding cancellation in a rounded displacement.
/// Each changed coordinate is the nearest binary64 value of `coordinate + target - source`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArrangementTranslation {
    pub axis: DistributionAxis,
    pub source: f64,
    pub target: f64,
}

/// An outermost selected object with its original bounds, nominal displacement and exact scope.
/// `delta` is rounded for reporting; `translation` defines coordinate materialization.
#[derive(Clone, Debug, PartialEq)]
pub struct ArrangementUnit {
    pub object: ObjectId,
    pub bounds: Bounds,
    pub delta: Vector,
    pub translation: Option<ArrangementTranslation>,
    pub scope: SelectionScope,
}

impl ArrangementUnit {
    /// Applies the checked anchor translation with one final coordinate rounding.
    pub(crate) fn translated(&self, point: Point) -> Result<Point, EditError> {
        let Some(translation) = self.translation else {
            return Ok(point);
        };
        let coordinate = match translation.axis {
            DistributionAxis::Horizontal => point.x(),
            DistributionAxis::Vertical => point.y(),
        };
        let mut sum = ExactSum::default();
        sum.add(coordinate);
        sum.add(translation.target);
        sum.add(-translation.source);
        let coordinate = sum.nearest();
        match translation.axis {
            DistributionAxis::Horizontal => Point::new(coordinate, point.y()),
            DistributionAxis::Vertical => Point::new(point.x(), coordinate),
        }
    }

    fn set_translation(&mut self, axis: DistributionAxis, source: f64, target: f64, delta: f64) {
        self.delta = match axis {
            DistributionAxis::Horizontal => Vector { dx: delta, dy: 0.0 },
            DistributionAxis::Vertical => Vector { dx: 0.0, dy: delta },
        };
        self.translation = (delta != 0.0).then_some(ArrangementTranslation {
            axis,
            source,
            target,
        });
    }
}

/// A checked arrangement, including stationary units. Units stay in identifier order for
/// alignment, or in leading-bound/identifier order for distribution. The combined scope
/// contains every selected unit, including stationary endpoints, not just changed geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct ArrangementPlan {
    pub units: Vec<ArrangementUnit>,
    pub scope: SelectionScope,
}

// Outward-rounded intervals enclose evaluation error for the actual arithmetic below.
// They replace a fixed epsilon: a displacement is suppressed only when zero lies in
// that enclosure. All coordinates and intermediate values are finite and bounded.
#[derive(Clone, Copy)]
struct Rounded {
    value: f64,
    low: f64,
    high: f64,
}

impl Rounded {
    fn exact(value: f64) -> Self {
        Self {
            value,
            low: value,
            high: value,
        }
    }
    fn add(self, other: Self) -> Self {
        Self {
            value: self.value + other.value,
            low: (self.low + other.low).next_down(),
            high: (self.high + other.high).next_up(),
        }
    }
    fn subtract(self, other: Self) -> Self {
        Self {
            value: self.value - other.value,
            low: (self.low - other.high).next_down(),
            high: (self.high - other.low).next_up(),
        }
    }
    fn multiply(self, factor: f64) -> Self {
        Self {
            value: self.value * factor,
            low: (self.low * factor).next_down(),
            high: (self.high * factor).next_up(),
        }
    }
    fn divide(self, divisor: f64) -> Self {
        Self {
            value: self.value / divisor,
            low: (self.low / divisor).next_down(),
            high: (self.high / divisor).next_up(),
        }
    }
    fn displacement(self) -> f64 {
        if self.low <= 0.0 && self.high >= 0.0 {
            0.0
        } else {
            self.value
        }
    }
}

// Growing nonoverlapping floating expansions use the error-free TwoSum transform.
// Keeping the residuals makes the sign exact for finite bounded input coordinates:
// an actually negative available gap is never excused by a rounding tolerance.
#[derive(Default)]
struct ExactSum {
    parts: Vec<f64>,
}

impl ExactSum {
    fn add(&mut self, value: f64) {
        let mut running = value;
        let mut parts = Vec::with_capacity(self.parts.len() + 1);
        for part in &self.parts {
            let sum = running + part;
            let rounded_part = sum - running;
            let residual = (running - (sum - rounded_part)) + (part - rounded_part);
            if residual != 0.0 {
                parts.push(residual);
            }
            running = sum;
        }
        if running != 0.0 {
            parts.push(running);
        }
        self.parts = parts;
    }
    fn is_negative(&self) -> bool {
        self.parts.last().is_some_and(|part| *part < 0.0)
    }
    // Collapse the nonoverlapping expansion from largest to smallest. A remaining
    // same-sign tail resolves a halfway case without double rounding (ties to even).
    fn nearest(&self) -> f64 {
        let mut parts = self.parts.iter().rev();
        let Some(&first) = parts.next() else {
            return 0.0;
        };
        let mut high = first;
        while let Some(&part) = parts.next() {
            let sum = high + part;
            let residual = part - (sum - high);
            high = sum;
            if residual != 0.0 {
                if parts
                    .next()
                    .is_some_and(|next| (*next > 0.0) == (residual > 0.0))
                {
                    let doubled = residual * 2.0;
                    let corrected = high + doubled;
                    if corrected - high == doubled {
                        high = corrected;
                    }
                }
                break;
            }
        }
        high
    }
    fn rounded(&self) -> Rounded {
        let mut parts = self.parts.iter();
        let Some(first) = parts.next() else {
            return Rounded::exact(0.0);
        };
        let mut result = Rounded::exact(*first);
        for part in parts {
            result = result.add(Rounded::exact(*part));
        }
        result
    }
}

fn center(low: f64, high: f64) -> Rounded {
    Rounded::exact(low).add(Rounded::exact(high)).divide(2.0)
}

pub(crate) fn plan(
    mut units: Vec<ArrangementUnit>,
    arrangement: ObjectArrangement,
) -> Result<ArrangementPlan, EditError> {
    let required = match arrangement {
        ObjectArrangement::Align(_) => 2,
        ObjectArrangement::Distribute(_) => 3,
    };
    if units.len() < required {
        return Err(EditError::ArrangementTooFewUnits {
            required,
            found: units.len(),
        });
    }
    match arrangement {
        ObjectArrangement::Align(mode) => {
            let mut bounds = units[0].bounds;
            for unit in &units[1..] {
                bounds.left = bounds.left.min(unit.bounds.left);
                bounds.right = bounds.right.max(unit.bounds.right);
                bounds.top = bounds.top.min(unit.bounds.top);
                bounds.bottom = bounds.bottom.max(unit.bounds.bottom);
            }
            for unit in &mut units {
                let (axis, source, target, delta) = match mode {
                    AlignMode::Left => (
                        DistributionAxis::Horizontal,
                        unit.bounds.left,
                        bounds.left,
                        bounds.left - unit.bounds.left,
                    ),
                    AlignMode::HorizontalCenter => {
                        let target = center(bounds.left, bounds.right);
                        let source = center(unit.bounds.left, unit.bounds.right);
                        (
                            DistributionAxis::Horizontal,
                            source.value,
                            target.value,
                            target.subtract(source).displacement(),
                        )
                    }
                    AlignMode::Right => (
                        DistributionAxis::Horizontal,
                        unit.bounds.right,
                        bounds.right,
                        bounds.right - unit.bounds.right,
                    ),
                    AlignMode::Top => (
                        DistributionAxis::Vertical,
                        unit.bounds.top,
                        bounds.top,
                        bounds.top - unit.bounds.top,
                    ),
                    AlignMode::VerticalCenter => {
                        let target = center(bounds.top, bounds.bottom);
                        let source = center(unit.bounds.top, unit.bounds.bottom);
                        (
                            DistributionAxis::Vertical,
                            source.value,
                            target.value,
                            target.subtract(source).displacement(),
                        )
                    }
                    AlignMode::Bottom => (
                        DistributionAxis::Vertical,
                        unit.bounds.bottom,
                        bounds.bottom,
                        bounds.bottom - unit.bounds.bottom,
                    ),
                };
                unit.set_translation(axis, source, target, delta);
            }
        }
        ObjectArrangement::Distribute(axis) => {
            units.sort_by(|first, second| {
                let a = first.bounds.leading(axis);
                let b = second.bounds.leading(axis);
                if a == b {
                    first.object.cmp(&second.object)
                } else {
                    a.total_cmp(&b)
                }
            });
            let first = units
                .first()
                .expect("the checked distribution contains three units")
                .bounds;
            // Summing adjacent gaps avoids cancellation between the outer span and
            // rounded widths. Add endpoints separately so even the gap sign is exact.
            let mut space = ExactSum::default();
            for pair in units.windows(2) {
                space.add(pair[1].bounds.leading(axis));
                space.add(-pair[0].bounds.trailing(axis));
            }
            if space.is_negative() {
                return Err(EditError::ArrangementNegativeGap);
            }
            let gap = space.rounded().divide((units.len() - 1) as f64);
            let last_index = units.len() - 1;
            let mut prefix = ExactSum::default();
            prefix.add(first.leading(axis));
            for index in 1..last_index {
                let previous = units[index - 1].bounds;
                prefix.add(previous.trailing(axis));
                prefix.add(-previous.leading(axis));
                let target = prefix.rounded().add(gap.multiply(index as f64));
                let displacement = target
                    .subtract(Rounded::exact(units[index].bounds.leading(axis)))
                    .displacement();
                let source = units[index].bounds.leading(axis);
                units[index].set_translation(axis, source, target.value, displacement);
            }
        }
    }
    let mut scope = SelectionScope::default();
    for unit in &units {
        scope.objects.extend(&unit.scope.objects);
        scope.positions.extend(&unit.scope.positions);
        scope.graphics.extend(&unit.scope.graphics);
    }
    Ok(ArrangementPlan { units, scope })
}
