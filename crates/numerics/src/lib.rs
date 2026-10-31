//! Explicit numeric storage profiles for future independent calculation engines.
//!
//! Stored bits and exact values are separate. Conversion here never substitutes an
//! ordinary host float for a requested profile. Finite arithmetic stores each result
//! in the selected profile; selected ordered, unit and display policies are
//! explicit, separate contracts.

mod arithmetic;
mod display;
mod exact;
mod extended;
mod ieee;
mod ordered;
mod quantize;
mod real48;
mod units;

pub use display::{format_default_six, format_fixed, round_three_half_away_to_binary32};
pub use exact::{Exact, Sign};
pub use extended::{Extended80, ExtendedClass};
pub use ieee::{Binary32, Binary64, FloatClass};
pub use ordered::{RoundedDeviation, TurnDirection, next_heading, round_deviation};
pub use real48::Real48;
pub use units::{
    ConcentrationUnit, ConcentrationValue, LengthUnit, LengthValue, MolarMassGramsPerMole,
};

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
    MissingAuxiliary,
    NonPositiveAuxiliary,
    PrecisionOutOfRange,
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
            Self::MissingAuxiliary => "missing_auxiliary",
            Self::NonPositiveAuxiliary => "nonpositive_auxiliary",
            Self::PrecisionOutOfRange => "precision_out_of_range",
        })
    }
}

impl std::error::Error for NumericError {}
