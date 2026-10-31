//! Explicit numeric storage profiles for future independent calculation engines.
//!
//! Stored bits and exact values are separate. Conversion here never substitutes an
//! ordinary host float for a requested profile. Arithmetic, unit and display policies
//! are separate contracts and are not supplied by these storage types yet.

mod exact;
mod ieee;
mod quantize;
mod real48;

pub use exact::{Exact, Sign};
pub use ieee::{Binary32, Binary64, FloatClass};
pub use real48::Real48;

/// A finite numeric conversion can fail without producing a substitute value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumericError {
    ZeroDenominator,
    ResourceLimit,
    NonFinite,
    Overflow,
}

impl std::fmt::Display for NumericError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ZeroDenominator => "zero_denominator",
            Self::ResourceLimit => "numeric_resource_limit",
            Self::NonFinite => "nonfinite_value",
            Self::Overflow => "numeric_overflow",
        })
    }
}

impl std::error::Error for NumericError {}
