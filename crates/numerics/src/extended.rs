//! Explicit ten-byte extended-precision storage and finite conversions.

use crate::{
    Binary64, Exact, NumericError, Sign,
    quantize::{self, Format, Magnitude, TieRule},
};

const INTEGER_BIT: u64 = 1_u64 << 63;
const FRACTION_MASK: u64 = INTEGER_BIT - 1;
const EXPONENT_MASK: u16 = 0x7fff;
const FORMAT: Format = Format {
    precision: 64,
    min_exponent: -16382,
    max_exponent: 16383,
    subnormals: true,
    ties: TieRule::ToEven,
};

/// Classification preserves pseudo-denormals and unsupported raw encodings distinctly.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtendedClass {
    Zero,
    Subnormal,
    PseudoDenormal,
    Normal,
    Infinite,
    QuietNaN,
    SignalingNaN,
    Unsupported,
}

/// Ten little-endian bytes, including the explicit integer bit and raw sign/exponent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Extended80([u8; 10]);

impl Extended80 {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 10]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn bytes(self) -> [u8; 10] {
        self.0
    }

    #[must_use]
    pub const fn is_negative(self) -> bool {
        self.0[9] & 0x80 != 0
    }

    fn fields(self) -> (u16, u64) {
        let mut significand = [0_u8; 8];
        significand.copy_from_slice(&self.0[..8]);
        let exponent = u16::from_le_bytes([self.0[8], self.0[9]]) & EXPONENT_MASK;
        (exponent, u64::from_le_bytes(significand))
    }

    /// Classifies raw encodings without modifying their bytes or executing arithmetic.
    #[must_use]
    pub fn class(self) -> ExtendedClass {
        let (exponent, significand) = self.fields();
        let integer_bit = significand & INTEGER_BIT != 0;
        let fraction = significand & FRACTION_MASK;
        match exponent {
            0 if integer_bit => ExtendedClass::PseudoDenormal,
            0 if fraction == 0 => ExtendedClass::Zero,
            0 => ExtendedClass::Subnormal,
            EXPONENT_MASK if !integer_bit => ExtendedClass::Unsupported,
            EXPONENT_MASK if fraction == 0 => ExtendedClass::Infinite,
            EXPONENT_MASK if fraction & (1_u64 << 62) != 0 => ExtendedClass::QuietNaN,
            EXPONENT_MASK => ExtendedClass::SignalingNaN,
            _ if integer_bit => ExtendedClass::Normal,
            _ => ExtendedClass::Unsupported,
        }
    }

    /// Decodes finite input exactly; pseudo-denormals have their equivalent normal value.
    /// Raw bytes are retained independently and are not canonicalized by reading.
    ///
    /// # Errors
    /// Rejects infinities, NaNs, unsupported encodings and resource limits.
    pub fn exact(self) -> Result<Exact, NumericError> {
        let sign = if self.is_negative() {
            Sign::Negative
        } else {
            Sign::Positive
        };
        let (exponent, significand) = self.fields();
        match self.class() {
            ExtendedClass::Zero => Exact::from_binary_scale(sign, 0, 0),
            ExtendedClass::Subnormal | ExtendedClass::PseudoDenormal => {
                Exact::from_binary_scale(sign, significand, -16445)
            }
            ExtendedClass::Normal => {
                Exact::from_binary_scale(sign, significand, i32::from(exponent) - 16383 - 63)
            }
            ExtendedClass::Infinite | ExtendedClass::QuietNaN | ExtendedClass::SignalingNaN => {
                Err(NumericError::NonFinite)
            }
            ExtendedClass::Unsupported => Err(NumericError::UnsupportedEncoding),
        }
    }

    /// Rounds an exact value to 64 significant bits, nearest with ties to even.
    /// Pseudo-denormals are emitted in their canonical normal encoding.
    ///
    /// # Errors
    /// Rejects overflow or resource limits; overflow is not replaced by infinity.
    pub fn from_exact(value: &Exact) -> Result<Self, NumericError> {
        let (exponent, significand) = match quantize::quantize(value, FORMAT)? {
            Magnitude::Zero => (0_u16, 0_u64),
            Magnitude::Subnormal(significand) => (0, significand),
            Magnitude::Normal {
                exponent,
                significand,
            } => ((exponent + 16383) as u16, significand),
        };
        let signed_exponent = exponent | (u16::from(value.is_negative()) << 15);
        let mut bytes = [0_u8; 10];
        bytes[..8].copy_from_slice(&significand.to_le_bytes());
        bytes[8..].copy_from_slice(&signed_exponent.to_le_bytes());
        Ok(Self(bytes))
    }

    /// Widens a finite binary64 input exactly, including signed zero.
    ///
    /// # Errors
    /// Rejects nonfinite input or a resource limit.
    pub fn from_binary64(value: Binary64) -> Result<Self, NumericError> {
        Self::from_exact(&value.exact()?)
    }

    /// Stores a finite extended value as binary64 with one ties-to-even rounding.
    ///
    /// # Errors
    /// Rejects nonfinite or unsupported input, overflow and resource limits.
    pub fn to_binary64(self) -> Result<Binary64, NumericError> {
        Binary64::from_exact(&self.exact()?)
    }
}
