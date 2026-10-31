//! Ordered finite numeric policies for later consumers.

use num_bigint::BigUint;
use num_integer::Integer;

use crate::{Binary64, Extended80, NumericError};

// These are the binary64 operands in the selected contour profile, widened exactly.
const PI: Binary64 = Binary64::from_bits(0x4009_21fb_5444_2d18);
const DEGREES_PER_HALF_TURN: Binary64 = Binary64::from_bits(0x4066_8000_0000_0000);

/// Whether a turn is added to or subtracted from the stored preceding heading.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TurnDirection {
    Add,
    Subtract,
}

/// An integer deviation and the associated strict greater-than-ten flag.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoundedDeviation(BigUint);

impl RoundedDeviation {
    /// The rounded nonnegative integer, without imposing a machine integer limit.
    #[must_use]
    pub fn degrees(&self) -> &BigUint {
        &self.0
    }

    /// The selected irregularity threshold is strictly greater than ten.
    #[must_use]
    pub fn is_irregular(&self) -> bool {
        self.0 > BigUint::from(10_u8)
    }
}

/// Applies one turn using the selected ordered extended-precision expression.
///
/// The inputs are stored binary64 values. Multiplication by stored binary64 pi,
/// division by stored binary64 180 and addition or subtraction each round in the
/// extended profile. Only the completed heading is then stored as binary64. Pass
/// that returned value as the preceding heading for the next turn.
///
/// # Errors
/// Rejects nonfinite inputs, arithmetic overflow or resource limits.
pub fn next_heading(
    previous: Binary64,
    turn_degrees: Binary64,
    direction: TurnDirection,
) -> Result<Binary64, NumericError> {
    let previous = Extended80::from_binary64(previous)?;
    let turn = Extended80::from_binary64(turn_degrees)?;
    let pi = Extended80::from_binary64(PI)?;
    let degrees_per_half_turn = Extended80::from_binary64(DEGREES_PER_HALF_TURN)?;
    let radians = turn.checked_mul(pi)?.checked_div(degrees_per_half_turn)?;
    let heading = match direction {
        TurnDirection::Add => previous.checked_add(radians)?,
        TurnDirection::Subtract => previous.checked_sub(radians)?,
    };
    heading.to_binary64()
}

/// Rounds a nonnegative finite binary64 deviation to an integer, ties upward.
///
/// Negative zero is accepted as zero. The flag is true only above ten degrees
/// after rounding; the integer remains exact even when it exceeds machine widths.
///
/// # Errors
/// Rejects negative nonzero and nonfinite inputs, or a numeric resource limit.
pub fn round_deviation(degrees: Binary64) -> Result<RoundedDeviation, NumericError> {
    let value = degrees.exact()?;
    if value.is_negative() && value.numerator() != &BigUint::from(0_u8) {
        return Err(NumericError::NegativeInput);
    }
    let (whole, remainder) = value.numerator().div_rem(value.denominator());
    let rounded = if &(&remainder << 1_usize) >= value.denominator() {
        whole + 1_u8
    } else {
        whole
    };
    Ok(RoundedDeviation(rounded))
}
