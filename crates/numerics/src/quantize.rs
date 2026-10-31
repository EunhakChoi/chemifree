//! Exact rational quantization; no host floating-point operations occur here.

use std::cmp::Ordering;

use num_bigint::BigUint;
use num_integer::Integer;

use crate::{Exact, NumericError};

#[derive(Clone, Copy)]
pub(crate) enum TieRule {
    ToEven,
    AwayFromZero,
}

#[derive(Clone, Copy)]
pub(crate) struct Format {
    pub(crate) precision: u32,
    pub(crate) min_exponent: i32,
    pub(crate) max_exponent: i32,
    pub(crate) subnormals: bool,
    pub(crate) ties: TieRule,
}

pub(crate) enum Magnitude {
    Zero,
    Subnormal(u64),
    Normal { exponent: i32, significand: u64 },
}

fn floor_log2(numerator: &BigUint, denominator: &BigUint) -> i32 {
    // Exact's public constructor bounds each component to 65,536 bits.
    let mut exponent = numerator.bits() as i32 - denominator.bits() as i32;
    let below = if exponent >= 0 {
        numerator < &(denominator << exponent as usize)
    } else {
        &(numerator << (-exponent) as usize) < denominator
    };
    if below {
        exponent -= 1;
    }
    exponent
}

fn round_scaled(
    numerator: &BigUint,
    denominator: &BigUint,
    binary_shift: i32,
    ties: TieRule,
) -> BigUint {
    let (scaled_numerator, scaled_denominator) = if binary_shift >= 0 {
        (numerator << binary_shift as usize, denominator.clone())
    } else {
        (numerator.clone(), denominator << (-binary_shift) as usize)
    };
    let (mut quotient, remainder) = scaled_numerator.div_rem(&scaled_denominator);
    let above_half = (&remainder << 1_usize).cmp(&scaled_denominator);
    let increment = above_half == Ordering::Greater
        || (above_half == Ordering::Equal
            && (matches!(ties, TieRule::AwayFromZero) || quotient.bit(0)));
    if increment {
        quotient += 1_u8;
    }
    quotient
}

/// Round to the requested finite encoding; overflow is an error, not an infinity.
pub(crate) fn quantize(value: &Exact, format: Format) -> Result<Magnitude, NumericError> {
    if value.numerator == BigUint::from(0_u8) {
        return Ok(Magnitude::Zero);
    }
    let mut exponent = floor_log2(&value.numerator, &value.denominator);
    if exponent > format.max_exponent {
        return Err(NumericError::Overflow);
    }
    let fraction_bits = format.precision as i32 - 1;
    if format.subnormals && exponent < format.min_exponent {
        let min_subnormal_exponent = format.min_exponent - fraction_bits;
        if exponent < min_subnormal_exponent - 1 {
            return Ok(Magnitude::Zero);
        }
        let rounded = round_scaled(
            &value.numerator,
            &value.denominator,
            -min_subnormal_exponent,
            format.ties,
        );
        if rounded == BigUint::from(0_u8) {
            return Ok(Magnitude::Zero);
        }
        let hidden_bit = BigUint::from(1_u8) << fraction_bits as usize;
        let significand = u64::try_from(&rounded).map_err(|_| NumericError::ResourceLimit)?;
        return if rounded >= hidden_bit {
            Ok(Magnitude::Normal {
                exponent: format.min_exponent,
                significand,
            })
        } else {
            Ok(Magnitude::Subnormal(significand))
        };
    }
    // Real48 rounds at its source exponent before flushing below its minimum.
    if !format.subnormals && exponent < format.min_exponent - 1 {
        return Ok(Magnitude::Zero);
    }
    let mut rounded = round_scaled(
        &value.numerator,
        &value.denominator,
        fraction_bits - exponent,
        format.ties,
    );
    let carry = BigUint::from(1_u8) << format.precision as usize;
    if rounded >= carry {
        rounded >>= 1_usize;
        exponent += 1;
    }
    if exponent > format.max_exponent {
        return Err(NumericError::Overflow);
    }
    if exponent < format.min_exponent {
        return Ok(Magnitude::Zero);
    }
    Ok(Magnitude::Normal {
        exponent,
        significand: u64::try_from(&rounded).map_err(|_| NumericError::ResourceLimit)?,
    })
}
