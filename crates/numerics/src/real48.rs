//! Six-byte Real48 storage with the selected nearest, half-away conversion.

use crate::{
    Binary64, Exact, NumericError, Sign,
    quantize::{self, Format, Magnitude, TieRule},
};

const FRACTION_BITS: u32 = 39;
const FORMAT: Format = Format {
    precision: 40,
    min_exponent: -128,
    max_exponent: 126,
    subnormals: false,
    ties: TieRule::AwayFromZero,
};

/// Six stored bytes in on-disk order, retained even for a noncanonical zero.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Real48([u8; 6]);

impl Real48 {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 6]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn bytes(self) -> [u8; 6] {
        self.0
    }

    /// Decodes the finite stored value. An exponent byte of zero means positive zero;
    /// the original bytes remain available separately through [`Self::bytes`].
    ///
    /// # Errors
    /// Rejects a numeric resource limit.
    pub fn exact(self) -> Result<Exact, NumericError> {
        if self.0[0] == 0 {
            return Exact::from_binary_scale(Sign::Positive, 0, 0);
        }
        let mut little = [0_u8; 8];
        little[..5].copy_from_slice(&self.0[1..]);
        let encoded = u64::from_le_bytes(little);
        let sign = if encoded & (1_u64 << FRACTION_BITS) != 0 {
            Sign::Negative
        } else {
            Sign::Positive
        };
        let fraction = encoded & ((1_u64 << FRACTION_BITS) - 1);
        let significand = (1_u64 << FRACTION_BITS) | fraction;
        let exponent = i32::from(self.0[0]) - 129 - FRACTION_BITS as i32;
        Exact::from_binary_scale(sign, significand, exponent)
    }

    /// Rounds to 40 significant bits, with halfway values away from zero.
    /// Rounding precedes flushing an exponent below -128 to positive zero.
    ///
    /// # Errors
    /// Rejects overflow or a numeric resource limit.
    pub fn from_exact(value: &Exact) -> Result<Self, NumericError> {
        match quantize::quantize(value, FORMAT)? {
            Magnitude::Zero => Ok(Self([0; 6])),
            Magnitude::Normal {
                exponent,
                significand,
            } => {
                let fraction = significand - (1_u64 << FRACTION_BITS);
                let sign = u64::from(value.is_negative()) << FRACTION_BITS;
                let tail = (fraction | sign).to_le_bytes();
                let mut bytes = [0_u8; 6];
                bytes[0] = (exponent + 129) as u8;
                bytes[1..].copy_from_slice(&tail[..5]);
                Ok(Self(bytes))
            }
            Magnitude::Subnormal(_) => Err(NumericError::ResourceLimit),
        }
    }

    /// Converts a finite binary64 value without intermediate host float arithmetic.
    ///
    /// # Errors
    /// Rejects nonfinite input, overflow or a numeric resource limit.
    pub fn from_binary64(value: Binary64) -> Result<Self, NumericError> {
        Self::from_exact(&value.exact()?)
    }

    /// Replaces the stored bytes only after the full conversion succeeds.
    ///
    /// # Errors
    /// Rejects nonfinite input, overflow or a numeric resource limit and keeps the old bytes.
    pub fn store_binary64(&mut self, value: Binary64) -> Result<(), NumericError> {
        let converted = Self::from_binary64(value)?;
        *self = converted;
        Ok(())
    }
}
