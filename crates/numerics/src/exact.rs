//! Reduced signed rationals, including a signed zero.

use num_bigint::BigUint;
use num_integer::Integer;

use crate::NumericError;

const MAX_COMPONENT_BITS: u64 = 65_536;

/// A separate sign keeps the two IEEE zero encodings distinguishable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Sign {
    Positive,
    Negative,
}

impl Sign {
    pub(crate) const fn is_negative(self) -> bool {
        matches!(self, Self::Negative)
    }
}

/// An exact signed rational with a nonzero denominator and reduced magnitude.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Exact {
    pub(crate) sign: Sign,
    pub(crate) numerator: BigUint,
    pub(crate) denominator: BigUint,
}

impl Exact {
    /// Constructs a reduced rational; the sign of zero is retained.
    ///
    /// # Errors
    /// Rejects a zero denominator and components larger than 65,536 bits.
    pub fn from_ratio(
        sign: Sign,
        numerator: BigUint,
        denominator: BigUint,
    ) -> Result<Self, NumericError> {
        if denominator == BigUint::from(0_u8) {
            return Err(NumericError::ZeroDenominator);
        }
        if numerator.bits() > MAX_COMPONENT_BITS || denominator.bits() > MAX_COMPONENT_BITS {
            return Err(NumericError::ResourceLimit);
        }
        let divisor = numerator.gcd(&denominator);
        Ok(Self {
            sign,
            numerator: numerator / &divisor,
            denominator: denominator / divisor,
        })
    }

    /// The sign is meaningful even when the numerator is zero.
    #[must_use]
    pub const fn sign(&self) -> Sign {
        self.sign
    }

    #[must_use]
    pub const fn is_negative(&self) -> bool {
        self.sign.is_negative()
    }

    #[must_use]
    pub fn numerator(&self) -> &BigUint {
        &self.numerator
    }

    #[must_use]
    pub fn denominator(&self) -> &BigUint {
        &self.denominator
    }

    pub(crate) fn from_binary_scale(
        sign: Sign,
        significand: u64,
        exponent: i32,
    ) -> Result<Self, NumericError> {
        if exponent >= 0 {
            Self::from_ratio(
                sign,
                BigUint::from(significand) << exponent as usize,
                1_u8.into(),
            )
        } else {
            Self::from_ratio(
                sign,
                significand.into(),
                BigUint::from(1_u8) << (-exponent) as usize,
            )
        }
    }
}
