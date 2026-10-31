//! Binary32 witnesses for prepared data, with their decimal source lexemes retained.

use serde_json::value::RawValue;

/// A finite F32 encoding and the JSON number that independently names its value.
#[derive(PartialEq)]
pub struct F32Witness {
    bits: u32,
    decimal: String,
}

impl F32Witness {
    /// Raw IEEE 754 binary32 bits in host-independent integer form.
    #[must_use]
    pub const fn bits(&self) -> u32 {
        self.bits
    }

    /// Exact JSON number spelling, retained without narrowing it to another decimal string.
    #[must_use]
    pub fn decimal(&self) -> &str {
        &self.decimal
    }
}

/// Payload-free rejection of a malformed, nonfinite or inconsistent witness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum F32WitnessError {
    Encoding,
    Number,
    NonFinite,
    Mismatch,
}

impl std::fmt::Display for F32WitnessError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Encoding => "invalid_f32_encoding",
            Self::Number => "invalid_number",
            Self::NonFinite => "nonfinite_number",
            Self::Mismatch => "numeric_witness_mismatch",
        })
    }
}
impl std::error::Error for F32WitnessError {}

/// Decode little-endian F32 bytes and check the declared JSON value rounds to those bits.
///
/// The JSON number's original spelling is retained. This validates an explicitly F32-bound
/// data witness only; it does not define model arithmetic or a unit.
///
/// # Errors
/// Rejects wrong-width or malformed hex, non-number JSON values, nonfinite numbers and
/// distinct byte and decimal encodings, including distinct signed zeros.
pub fn decode_f32(hex: &str, value: &RawValue) -> Result<F32Witness, F32WitnessError> {
    if hex.len() != 8 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(F32WitnessError::Encoding);
    }
    let mut bytes = [0_u8; 4];
    for (index, target) in bytes.iter_mut().enumerate() {
        *target = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
            .map_err(|_| F32WitnessError::Encoding)?;
    }
    let bits = u32::from_le_bytes(bytes);
    let encoded = f32::from_bits(bits);
    let decimal = value.get();
    if !matches!(decimal.as_bytes().first(), Some(b'-' | b'0'..=b'9')) {
        return Err(F32WitnessError::Number);
    }
    let parsed: f32 = decimal.parse().map_err(|_| F32WitnessError::Number)?;
    if !encoded.is_finite() || !parsed.is_finite() {
        return Err(F32WitnessError::NonFinite);
    }
    if parsed.to_bits() != bits {
        return Err(F32WitnessError::Mismatch);
    }
    Ok(F32Witness {
        bits,
        decimal: decimal.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json_value(value: &str) -> Box<RawValue> {
        RawValue::from_string(value.to_owned()).unwrap()
    }

    #[test]
    fn finite_values_preserve_bits_and_the_exact_decimal_spelling() {
        for (hex, decimal, bits) in [
            ("0000803f", "1", 0x3f80_0000),
            ("00000080", "-0.0", 0x8000_0000),
            (
                "01000000",
                "1.4012984643248170709237295832899161312802619418765e-45",
                1,
            ),
            ("0000803F", "1.000000059604644775390625", 0x3f80_0000),
            ("0100803f", "1.000000059604644775390626", 0x3f80_0001),
        ] {
            let witness = decode_f32(hex, &json_value(decimal)).unwrap();
            assert_eq!(witness.bits(), bits);
            assert_eq!(witness.decimal(), decimal);
        }
    }

    #[test]
    fn independent_fraction_vectors_cover_signed_midpoints_and_neighbors() {
        let vectors = include_str!("../tests/data/f32-witness-independent-v1/vectors.tsv");
        let mut count = 0;
        for line in vectors.lines() {
            let (decimal, hex) = line.split_once('\t').unwrap();
            let value = json_value(decimal);
            let witness = decode_f32(hex, &value).unwrap();
            assert_eq!(witness.decimal(), decimal);
            count += 1;
        }
        assert_eq!(count, 642);
    }

    #[test]
    fn incorrect_bits_signed_zero_nonfinite_and_wrong_types_are_rejected() {
        assert_eq!(
            decode_f32("0000803f", &json_value("2")).err(),
            Some(F32WitnessError::Mismatch)
        );
        assert_eq!(
            decode_f32("00000080", &json_value("0")).err(),
            Some(F32WitnessError::Mismatch)
        );
        assert_eq!(
            decode_f32("0000807f", &json_value("1")).err(),
            Some(F32WitnessError::NonFinite)
        );
        assert_eq!(
            decode_f32("0000803f", &json_value("1e9999")).err(),
            Some(F32WitnessError::NonFinite)
        );
        assert_eq!(
            decode_f32("0000803f", &json_value("true")).err(),
            Some(F32WitnessError::Number)
        );
        for hex in ["0000803", "0000803g", "0000803f00", "한글한글"] {
            assert_eq!(
                decode_f32(hex, &json_value("1")).err(),
                Some(F32WitnessError::Encoding)
            );
        }
    }
}
