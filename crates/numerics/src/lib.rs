//! Explicit numeric storage profiles for future independent calculation engines.
//!
//! Stored bits and exact values are separate. Conversion here never substitutes an
//! ordinary host float for a requested profile. Finite arithmetic stores each result
//! in the selected profile, and selected ordered policies are explicit. Units and
//! display policies are separate contracts and are not supplied yet.

mod arithmetic;
mod exact;
mod extended;
mod ieee;
mod ordered;
mod quantize;
mod real48;

pub use exact::{Exact, Sign};
pub use extended::{Extended80, ExtendedClass};
pub use ieee::{Binary32, Binary64, FloatClass};
pub use ordered::{RoundedDeviation, TurnDirection, next_heading, round_deviation};
pub use real48::Real48;

/// A finite numeric conversion can fail without producing a substitute value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumericError {
    ZeroDenominator,
    ResourceLimit,
    NonFinite,
    Overflow,
    DivisionByZero,
    UnsupportedEncoding,
    NegativeInput,
}

impl std::fmt::Display for NumericError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ZeroDenominator => "zero_denominator",
            Self::ResourceLimit => "numeric_resource_limit",
            Self::NonFinite => "nonfinite_value",
            Self::Overflow => "numeric_overflow",
            Self::DivisionByZero => "division_by_zero",
            Self::UnsupportedEncoding => "unsupported_encoding",
            Self::NegativeInput => "negative_input",
        })
    }
}

impl std::error::Error for NumericError {}
