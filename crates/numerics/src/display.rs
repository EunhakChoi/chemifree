//! Explicit decimal presentation and finite prediction storage policies.

use std::cmp::Ordering;

use num_bigint::BigUint;
use num_integer::Integer;

use crate::{Binary32, Binary64, Exact, NumericError};

#[derive(Clone, Copy)]
enum TieRule {
    ToEven,
    AwayFromZero,
}

fn power_of_ten(exponent: u32) -> BigUint {
    BigUint::from(10_u8).pow(exponent)
}

// Return the rounded nonnegative magnitude of value * 10^shift. The sign is
// kept on Exact so zero can retain its sign independently of the digits.
fn scaled_integer(value: &Exact, shift: i32, tie: TieRule) -> BigUint {
    let mut numerator = value.numerator().clone();
    let mut denominator = value.denominator().clone();
    if shift >= 0 {
        numerator *= power_of_ten(shift as u32);
    } else {
        denominator *= power_of_ten(shift.unsigned_abs());
    }
    let (whole, remainder) = numerator.div_rem(&denominator);
    let round_up = match (remainder << 1_usize).cmp(&denominator) {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => matches!(tie, TieRule::AwayFromZero) || whole.is_odd(),
    };
    if round_up { whole + 1_u8 } else { whole }
}

// The argument is nonzero and finite binary64, whose component sizes keep the
// decimal exponent within i32. Compare exact integers to avoid a float log10.
fn decimal_exponent(value: &Exact) -> i32 {
    let numerator = value.numerator();
    let denominator = value.denominator();
    let estimate =
        numerator.to_str_radix(10).len() as i32 - denominator.to_str_radix(10).len() as i32;
    let below = if estimate >= 0 {
        numerator < &(denominator * power_of_ten(estimate as u32))
    } else {
        &(numerator * power_of_ten(estimate.unsigned_abs())) < denominator
    };
    if below { estimate - 1 } else { estimate }
}

fn sign(value: &Exact) -> &'static str {
    if value.is_negative() { "-" } else { "" }
}

/// Formats finite binary64 using six significant digits and a C-locale-style
/// default-float boundary, with nearest ties-to-even decimal rounding.
///
/// # Errors
/// Rejects nonfinite input or a numeric resource limit.
pub fn format_default_six(raw: Binary64) -> Result<String, NumericError> {
    let value = raw.exact()?;
    if value.numerator() == &BigUint::from(0_u8) {
        return Ok(format!("{}0", sign(&value)));
    }
    let mut exponent = decimal_exponent(&value);
    let mut rounded = scaled_integer(&value, 5 - exponent, TieRule::ToEven);
    if rounded >= BigUint::from(1_000_000_u32) {
        rounded /= 10_u8;
        exponent += 1;
    }
    // Six-digit rounding of a nonzero mantissa always starts with an ASCII
    // decimal digit. Trailing zeros do not contribute to this profile's output.
    let mut digits = rounded.to_str_radix(10);
    while digits.len() > 1 && digits.ends_with('0') {
        digits.truncate(digits.len() - 1);
    }
    let prefix = sign(&value);
    if !(-4..6).contains(&exponent) {
        let (first, rest) = digits.split_at(1);
        let mantissa = if rest.is_empty() {
            first.to_owned()
        } else {
            format!("{first}.{rest}")
        };
        return Ok(format!("{prefix}{mantissa}e{exponent:+03}"));
    }
    let point = exponent + 1;
    if point <= 0 {
        return Ok(format!(
            "{prefix}0.{}{digits}",
            "0".repeat((-point) as usize)
        ));
    }
    let point = point as usize;
    if point >= digits.len() {
        return Ok(format!(
            "{prefix}{digits}{}",
            "0".repeat(point - digits.len())
        ));
    }
    let (whole, fraction) = digits.split_at(point);
    Ok(format!("{prefix}{whole}.{fraction}"))
}

/// Formats finite binary64 with 0–8 fixed decimal places and trailing zeroes.
/// Decimal ties are rounded to even and signed zero remains signed.
///
/// # Errors
/// Rejects an unsupported number of places, nonfinite input or a resource limit.
pub fn format_fixed(raw: Binary64, places: u8) -> Result<String, NumericError> {
    if places > 8 {
        return Err(NumericError::PrecisionOutOfRange);
    }
    let value = raw.exact()?;
    let digits = scaled_integer(&value, i32::from(places), TieRule::ToEven).to_str_radix(10);
    let prefix = sign(&value);
    if places == 0 {
        return Ok(format!("{prefix}{digits}"));
    }
    let places = usize::from(places);
    if digits.len() <= places {
        return Ok(format!(
            "{prefix}0.{}{digits}",
            "0".repeat(places - digits.len())
        ));
    }
    let (whole, fraction) = digits.split_at(digits.len() - places);
    Ok(format!("{prefix}{whole}.{fraction}"))
}

/// Rounds an exact finite value to three decimal places, ties away from zero,
/// then stores that decimal value once in binary32 with ties-to-even rounding.
///
/// # Errors
/// Rejects binary32 overflow or a numeric resource limit.
pub fn round_three_half_away_to_binary32(value: &Exact) -> Result<Binary32, NumericError> {
    let rounded = scaled_integer(value, 3, TieRule::AwayFromZero);
    let decimal = Exact::from_ratio(value.sign(), rounded, BigUint::from(1000_u16))?;
    Binary32::from_exact(&decimal)
}
