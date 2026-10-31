//! IEEE binary storage and explicit finite conversion to and from exact rationals.

use crate::{
    Exact, NumericError, Sign,
    quantize::{self, Format, Magnitude, TieRule},
};

/// IEEE storage classification, independent of its sign bit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FloatClass {
    Zero,
    Subnormal,
    Normal,
    Infinite,
    QuietNaN,
    SignalingNaN,
}

#[derive(Clone, Copy)]
struct IeeeFormat {
    fraction_bits: u32,
    exponent_bits: u32,
    bias: i32,
}

const F32: IeeeFormat = IeeeFormat {
    fraction_bits: 23,
    exponent_bits: 8,
    bias: 127,
};
const F64: IeeeFormat = IeeeFormat {
    fraction_bits: 52,
    exponent_bits: 11,
    bias: 1023,
};

impl IeeeFormat {
    fn fraction_mask(self) -> u64 {
        (1_u64 << self.fraction_bits) - 1
    }
    fn max_encoded_exponent(self) -> u64 {
        (1_u64 << self.exponent_bits) - 1
    }
    fn sign_bit(self) -> u32 {
        self.fraction_bits + self.exponent_bits
    }
    fn rounding(self) -> Format {
        Format {
            precision: self.fraction_bits + 1,
            min_exponent: 1 - self.bias,
            max_exponent: self.bias,
            subnormals: true,
            ties: TieRule::ToEven,
        }
    }
}

fn class(bits: u64, format: IeeeFormat) -> FloatClass {
    let fraction = bits & format.fraction_mask();
    let exponent = (bits >> format.fraction_bits) & format.max_encoded_exponent();
    if exponent == 0 {
        if fraction == 0 {
            FloatClass::Zero
        } else {
            FloatClass::Subnormal
        }
    } else if exponent == format.max_encoded_exponent() {
        if fraction == 0 {
            FloatClass::Infinite
        } else if fraction & (1_u64 << (format.fraction_bits - 1)) != 0 {
            FloatClass::QuietNaN
        } else {
            FloatClass::SignalingNaN
        }
    } else {
        FloatClass::Normal
    }
}

fn exact(bits: u64, format: IeeeFormat) -> Result<Exact, NumericError> {
    let sign = if bits & (1_u64 << format.sign_bit()) != 0 {
        Sign::Negative
    } else {
        Sign::Positive
    };
    let fraction = bits & format.fraction_mask();
    let exponent = (bits >> format.fraction_bits) & format.max_encoded_exponent();
    if exponent == format.max_encoded_exponent() {
        return Err(NumericError::NonFinite);
    }
    let (significand, power) = if exponent == 0 {
        (fraction, 1 - format.bias - format.fraction_bits as i32)
    } else {
        (
            (1_u64 << format.fraction_bits) | fraction,
            exponent as i32 - format.bias - format.fraction_bits as i32,
        )
    };
    Exact::from_binary_scale(sign, significand, power)
}

fn from_exact(value: &Exact, format: IeeeFormat) -> Result<u64, NumericError> {
    let sign = u64::from(value.is_negative()) << format.sign_bit();
    match quantize::quantize(value, format.rounding())? {
        Magnitude::Zero => Ok(sign),
        Magnitude::Subnormal(fraction) => Ok(sign | fraction),
        Magnitude::Normal {
            exponent,
            significand,
        } => {
            let encoded_exponent = (exponent + format.bias) as u64;
            let fraction = significand - (1_u64 << format.fraction_bits);
            Ok(sign | (encoded_exponent << format.fraction_bits) | fraction)
        }
    }
}

/// Raw IEEE binary32 bits, including zeros, infinities and NaNs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Binary32(u32);

impl Binary32 {
    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }
    #[must_use]
    pub const fn is_negative(self) -> bool {
        self.0 >> 31 != 0
    }
    #[must_use]
    pub fn class(self) -> FloatClass {
        class(u64::from(self.0), F32)
    }

    /// Decodes a finite stored value exactly, retaining the sign of zero.
    ///
    /// # Errors
    /// Rejects infinities and NaNs.
    pub fn exact(self) -> Result<Exact, NumericError> {
        exact(u64::from(self.0), F32)
    }

    /// Rounds an exact value to binary32 using nearest, ties to even.
    ///
    /// # Errors
    /// Rejects overflow and resource limits; overflow is not silently converted to infinity.
    pub fn from_exact(value: &Exact) -> Result<Self, NumericError> {
        u32::try_from(from_exact(value, F32)?)
            .map(Self)
            .map_err(|_| NumericError::ResourceLimit)
    }

    /// Narrows a finite binary64 value with one binary32 rounding.
    ///
    /// # Errors
    /// Rejects nonfinite input and overflow.
    pub fn from_binary64(value: Binary64) -> Result<Self, NumericError> {
        Self::from_exact(&value.exact()?)
    }
}

/// Raw IEEE binary64 bits, including zeros, infinities and NaNs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Binary64(u64);

impl Binary64 {
    #[must_use]
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }
    #[must_use]
    pub const fn bits(self) -> u64 {
        self.0
    }
    #[must_use]
    pub const fn is_negative(self) -> bool {
        self.0 >> 63 != 0
    }
    #[must_use]
    pub fn class(self) -> FloatClass {
        class(self.0, F64)
    }

    /// Decodes a finite stored value exactly, retaining the sign of zero.
    ///
    /// # Errors
    /// Rejects infinities and NaNs.
    pub fn exact(self) -> Result<Exact, NumericError> {
        exact(self.0, F64)
    }

    /// Rounds an exact value to binary64 using nearest, ties to even.
    ///
    /// # Errors
    /// Rejects overflow and resource limits; overflow is not silently converted to infinity.
    pub fn from_exact(value: &Exact) -> Result<Self, NumericError> {
        from_exact(value, F64).map(Self)
    }

    /// Widens a finite binary32 value exactly, including signed zero.
    ///
    /// # Errors
    /// Rejects nonfinite input.
    pub fn from_binary32(value: Binary32) -> Result<Self, NumericError> {
        Self::from_exact(&value.exact()?)
    }
}
