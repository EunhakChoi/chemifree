//! Checked geometry transforms. Stored chemical stereo is never transformed or re-derived.

use super::arrangement::ExactSum;
use super::{Graphic, SelectionScope};
use crate::document::{EditError, ObjectId, Point, PositionKey};

/// Geometry transform about an explicit pivot in page coordinates (x right, y down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SelectionTransform {
    /// Positive degrees rotate clockwise visually. Finite values normalize modulo 360.
    Rotate { degrees: f64 },
    /// Mirror across the horizontal line y=pivot.y: x'=x, y'=2*pivot.y-y.
    ReflectHorizontal,
    /// Mirror across the vertical line x=pivot.x: x'=2*pivot.x-x, y'=y.
    ReflectVertical,
}

/// A checked atom position, including unchanged positions in the resolved scope.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransformedPosition {
    pub key: PositionKey,
    pub before: Point,
    pub after: Point,
}

/// A checked graphic, preserving shape and endpoint/corner identity.
#[derive(Clone, Debug, PartialEq)]
pub struct TransformedGraphic {
    pub object: ObjectId,
    pub before: Graphic,
    pub after: Graphic,
}

/// The exact resolved scope and checked geometry before/after a transform.
#[derive(Clone, Debug, PartialEq)]
pub struct TransformPlan {
    pub scope: SelectionScope,
    pub positions: Vec<TransformedPosition>,
    pub graphics: Vec<TransformedGraphic>,
    /// Advisory only: at least one graph position changes. This is not a stereo comparison
    /// or a transaction loss; stored configurations, marks and groups remain unchanged.
    /// The existing stereo inspector can assess agreement with the resulting drawing.
    pub drawing_stereo_may_change: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum PreparedTransform {
    Identity,
    QuarterTurn(u8),
    Rotation { sine: f64, cosine: f64 },
    Horizontal,
    Vertical,
}

fn affine(terms: &[f64]) -> f64 {
    let mut sum = ExactSum::default();
    for value in terms {
        sum.add(*value);
    }
    sum.nearest()
}

impl PreparedTransform {
    pub(crate) fn new(transform: SelectionTransform) -> Result<Self, EditError> {
        Ok(match transform {
            SelectionTransform::Rotate { degrees } => {
                if !degrees.is_finite() {
                    return Err(EditError::TransformInvalidAngle);
                }
                let angle = degrees.rem_euclid(360.0);
                if angle == 0.0 || angle == 360.0 {
                    Self::Identity
                } else if angle == 90.0 {
                    Self::QuarterTurn(1)
                } else if angle == 180.0 {
                    Self::QuarterTurn(2)
                } else if angle == 270.0 {
                    Self::QuarterTurn(3)
                } else {
                    let (sine, cosine) = angle.to_radians().sin_cos();
                    Self::Rotation { sine, cosine }
                }
            }
            SelectionTransform::ReflectHorizontal => Self::Horizontal,
            SelectionTransform::ReflectVertical => Self::Vertical,
        })
    }

    pub(crate) fn supports_rectangle(self) -> bool {
        !matches!(self, Self::Rotation { .. })
    }

    pub(crate) fn point(self, point: Point, pivot: Point) -> Result<Point, EditError> {
        if matches!(self, Self::Identity) || point == pivot {
            return Ok(point);
        }
        let (x, y) = (point.x(), point.y());
        let (px, py) = (pivot.x(), pivot.y());
        // Quarter turns and axis reflections use exact endpoint sums, rounded once.
        // No sine/cosine approximation or multiply-by-near-zero term is introduced.
        let (x, y) = match self {
            Self::Identity => (x, y),
            Self::QuarterTurn(1) => (affine(&[px, py, -y]), affine(&[py, -px, x])),
            Self::QuarterTurn(2) => (affine(&[px, px, -x]), affine(&[py, py, -y])),
            Self::QuarterTurn(_) => (affine(&[px, -py, y]), affine(&[py, px, -x])),
            Self::Horizontal => (x, affine(&[py, py, -y])),
            Self::Vertical => (affine(&[px, px, -x]), y),
            Self::Rotation { sine, cosine } => {
                let dx = x - px;
                let dy = y - py;
                (
                    dx.mul_add(cosine, -dy * sine) + px,
                    dx.mul_add(sine, dy * cosine) + py,
                )
            }
        };
        Point::new(x, y)
    }
}
