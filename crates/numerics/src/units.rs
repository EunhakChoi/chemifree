//! Explicit units, raw storage and selected independent conversion policies.

use num_bigint::BigUint;

use crate::{Binary64, Exact, NumericError, Real48};

/// The computer point is defined as exactly one seventy-second of an inch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LengthUnit {
    ComputerPoint,
    Millimetre,
    Centimetre,
    Inch,
}

impl LengthUnit {
    // Exact millimetres per unit. This is the computer point, not a traditional
    // typography point, and one international inch is exactly 25.4 mm.
    const fn millimetres(self) -> (u16, u16) {
        match self {
            Self::ComputerPoint => (127, 360),
            Self::Millimetre => (1, 1),
            Self::Centimetre => (10, 1),
            Self::Inch => (127, 5),
        }
    }
}

/// A binary64 length and its explicit unit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LengthValue {
    raw: Binary64,
    unit: LengthUnit,
}

impl LengthValue {
    /// Binds finite raw bits to a unit without changing those bits.
    ///
    /// # Errors
    /// Rejects a nonfinite value or a numeric resource limit.
    pub fn new(raw: Binary64, unit: LengthUnit) -> Result<Self, NumericError> {
        raw.exact()?;
        Ok(Self { raw, unit })
    }

    /// The stored binary64 bits, including the sign of zero.
    #[must_use]
    pub const fn raw(self) -> Binary64 {
        self.raw
    }
    /// The unit associated with the stored value.
    #[must_use]
    pub const fn unit(self) -> LengthUnit {
        self.unit
    }
    /// Converts using exact physical factors and one binary64 result store.
    ///
    /// Conversion to the same unit preserves the original raw bits. This policy
    /// does not apply a display rounding or a legacy chemistry conversion rule.
    ///
    /// # Errors
    /// Rejects overflow or a numeric resource limit and leaves the input intact.
    pub fn convert(self, target: LengthUnit) -> Result<Self, NumericError> {
        if self.unit == target {
            return Ok(self);
        }
        let value = self.raw.exact()?;
        let (source_numerator, source_denominator) = self.unit.millimetres();
        let (target_numerator, target_denominator) = target.millimetres();
        let numerator =
            value.numerator() * BigUint::from(source_numerator) * BigUint::from(target_denominator);
        let denominator = value.denominator()
            * BigUint::from(source_denominator)
            * BigUint::from(target_numerator);
        let converted = Exact::from_ratio(value.sign(), numerator, denominator)?;
        Ok(Self {
            raw: Binary64::from_exact(&converted)?,
            unit: target,
        })
    }
}

/// The unit of a stored concentration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConcentrationUnit {
    MolesPerLitre,
    GramsPerLitre,
}

/// A positive, explicitly supplied molar mass in grams per mole.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MolarMassGramsPerMole(Real48);

impl MolarMassGramsPerMole {
    /// Binds a positive stored molar mass to grams per mole.
    ///
    /// # Errors
    /// Rejects zero, negative values and a numeric resource limit.
    pub fn new(raw: Real48) -> Result<Self, NumericError> {
        let value = raw.exact()?;
        if value.is_negative() || value.numerator() == &BigUint::from(0_u8) {
            return Err(NumericError::NonPositiveAuxiliary);
        }
        Ok(Self(raw))
    }

    /// The stored Real48 bytes supplied by the caller.
    #[must_use]
    pub const fn raw(self) -> Real48 {
        self.0
    }
}

/// A Real48 concentration and its explicit unit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConcentrationValue {
    raw: Real48,
    unit: ConcentrationUnit,
}

impl ConcentrationValue {
    /// Binds Real48 bytes to their stated unit without interpreting their provenance.
    #[must_use]
    pub const fn new(raw: Real48, unit: ConcentrationUnit) -> Self {
        Self { raw, unit }
    }
    /// The stored Real48 bytes, including a noncanonical zero.
    #[must_use]
    pub const fn raw(self) -> Real48 {
        self.raw
    }
    /// The unit associated with the stored value.
    #[must_use]
    pub const fn unit(self) -> ConcentrationUnit {
        self.unit
    }
    /// Applies the independent mass/amount concentration relation.
    ///
    /// A changed unit requires an explicit positive molar mass; multiplication or
    /// division is followed by one Real48 store. Identical units preserve raw bytes
    /// and do not require the auxiliary value. This numeric operation does not
    /// decide whether the concentration is scientifically valid.
    ///
    /// # Errors
    /// Rejects a missing molar mass, overflow or a numeric resource limit.
    pub fn convert(
        self,
        target: ConcentrationUnit,
        molar_mass: Option<MolarMassGramsPerMole>,
    ) -> Result<Self, NumericError> {
        if self.unit == target {
            return Ok(self);
        }
        let factor = molar_mass.ok_or(NumericError::MissingAuxiliary)?.raw();
        let raw = match (self.unit, target) {
            (ConcentrationUnit::MolesPerLitre, ConcentrationUnit::GramsPerLitre) => {
                self.raw.checked_mul(factor)?
            }
            (ConcentrationUnit::GramsPerLitre, ConcentrationUnit::MolesPerLitre) => {
                self.raw.checked_div(factor)?
            }
            (ConcentrationUnit::MolesPerLitre, ConcentrationUnit::MolesPerLitre)
            | (ConcentrationUnit::GramsPerLitre, ConcentrationUnit::GramsPerLitre) => {
                return Ok(self);
            }
        };
        Ok(Self { raw, unit: target })
    }
}
