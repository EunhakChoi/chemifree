//! Exact comparisons for source decimal tokens whose binary64 witnesses may underflow.

use std::{cmp::Ordering, str::FromStr};

use num_bigint::{BigInt, BigUint};

// Bounded parsing is a refusal, never a numeric approximation. The bounds apply to
// text length, not exponent magnitude, so even very small finite source tokens compare.
const MAX_COMPONENT_DIGITS: usize = 4096;

#[derive(Eq, PartialEq)]
pub(crate) struct DecimalInput {
    negative: bool,
    coefficient: BigUint,
    digits: usize,
    power_of_ten: BigInt,
}

impl DecimalInput {
    pub(crate) fn from_token(token: &str) -> Option<Self> {
        let (negative, unsigned) = match token.as_bytes().first() {
            Some(b'-') => (true, &token[1..]),
            Some(b'+') => (false, &token[1..]),
            _ => (false, token),
        };
        let mut parts = unsigned.split(['e', 'E']);
        let mantissa = parts.next()?;
        let exponent_text = parts.next().unwrap_or("0");
        if parts.next().is_some() {
            return None;
        }
        let exponent_digits = exponent_text.trim_start_matches(['+', '-']);
        if exponent_digits.is_empty()
            || exponent_digits.len() > MAX_COMPONENT_DIGITS
            || !exponent_digits.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        let exponent = BigInt::from_str(exponent_text).ok()?;
        let mut halves = mantissa.split('.');
        let whole = halves.next()?;
        let fraction = halves.next().unwrap_or("");
        if halves.next().is_some()
            || whole.len() + fraction.len() == 0
            || whole.len() + fraction.len() > MAX_COMPONENT_DIGITS
            || !whole
                .bytes()
                .chain(fraction.bytes())
                .all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        let digits = format!("{whole}{fraction}");
        let normalized = digits.trim_start_matches('0');
        if normalized.is_empty() {
            return Some(Self {
                negative: false,
                coefficient: BigUint::from(0_u8),
                digits: 1,
                power_of_ten: BigInt::from(0),
            });
        }
        let without_tail = normalized.trim_end_matches('0');
        let removed_tail = normalized.len() - without_tail.len();
        Some(Self {
            negative,
            coefficient: BigUint::parse_bytes(without_tail.as_bytes(), 10)?,
            digits: without_tail.len(),
            power_of_ten: exponent - BigInt::from(fraction.len()) + BigInt::from(removed_tail),
        })
    }

    fn is_zero(&self) -> bool {
        self.coefficient == BigUint::from(0_u8)
    }

    /// Returns an exact integer spelling only when its magnitude can fit the caller's type.
    pub(crate) fn integer_token(&self, max_digits: usize) -> Option<String> {
        if self.is_zero() {
            return Some("0".into());
        }
        let max_power = max_digits.checked_sub(self.digits)?;
        if self.power_of_ten < BigInt::from(0) || self.power_of_ten > BigInt::from(max_power) {
            return None;
        }
        let zeros = self.power_of_ten.to_string().parse::<usize>().ok()?;
        let mut result = String::with_capacity(self.digits + zeros + usize::from(self.negative));
        if self.negative {
            result.push('-');
        }
        result.push_str(&self.coefficient.to_str_radix(10));
        for _ in 0..zeros {
            result.push('0');
        }
        Some(result)
    }

    fn magnitude_cmp(&self, other: &Self) -> Ordering {
        let left_size = &self.power_of_ten + BigInt::from(self.digits);
        let right_size = &other.power_of_ten + BigInt::from(other.digits);
        match left_size.cmp(&right_size) {
            Ordering::Equal => {
                // Equal decimal magnitudes imply a scale difference no larger than the
                // bounded coefficient lengths. No huge power of ten is constructed.
                if self.digits < other.digits {
                    let shift = u32::try_from(other.digits - self.digits).expect("bounded length");
                    (&self.coefficient * BigUint::from(10_u8).pow(shift)).cmp(&other.coefficient)
                } else {
                    let shift = u32::try_from(self.digits - other.digits).expect("bounded length");
                    self.coefficient
                        .cmp(&(&other.coefficient * BigUint::from(10_u8).pow(shift)))
                }
            }
            order => order,
        }
    }
}

impl Ord for DecimalInput {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.is_zero() || other.is_zero() {
            return match (self.is_zero(), other.is_zero()) {
                (true, true) => Ordering::Equal,
                (true, false) if other.negative => Ordering::Greater,
                (true, false) => Ordering::Less,
                (false, true) if self.negative => Ordering::Less,
                (false, true) => Ordering::Greater,
                (false, false) => unreachable!(),
            };
        }
        match (self.negative, other.negative) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => self.magnitude_cmp(other),
            (true, true) => self.magnitude_cmp(other).reverse(),
        }
    }
}

impl PartialOrd for DecimalInput {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_decimal_source_without_binary64_underflow_or_rounding() {
        for (left, right, expected) in [
            ("-1e-99999999999999999999", "0", Ordering::Less),
            ("1e-99999999999999999999", "0", Ordering::Greater),
            ("100.00000000000000000001", "100", Ordering::Greater),
            ("14.12500", "14125e-3", Ordering::Equal),
            ("-0.0", "0e999", Ordering::Equal),
            ("-2.125", "-2.12", Ordering::Less),
            ("9e4", "1e5", Ordering::Less),
            ("123e-2", "1.2301", Ordering::Less),
        ] {
            assert_eq!(
                DecimalInput::from_token(left)
                    .unwrap()
                    .cmp(&DecimalInput::from_token(right).unwrap()),
                expected,
                "{left} compared to {right}"
            );
        }
    }

    #[test]
    fn agrees_with_independent_integer_fraction_vectors() {
        let cases = include_str!("../tests/data/decimal-independent-v3/vectors.tsv");
        for (index, line) in cases.lines().enumerate() {
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields.len(), 3);
            let expected = match fields[2] {
                "-1" => Ordering::Less,
                "0" => Ordering::Equal,
                "1" => Ordering::Greater,
                _ => panic!("bad fixture ordering"),
            };
            let left = DecimalInput::from_token(fields[0]).unwrap();
            let right = DecimalInput::from_token(fields[1]).unwrap();
            assert_eq!(left.cmp(&right), expected, "vector {index}");
        }
    }

    #[test]
    fn rejects_malformed_and_overlong_components_without_approximating() {
        for token in ["", ".", "e2", "1e", "1e--1", "1.2.3", "1e2e3", "NaN"] {
            assert!(DecimalInput::from_token(token).is_none(), "{token}");
        }
        assert!(DecimalInput::from_token(&format!("1e{}", "9".repeat(4097))).is_none());
        assert!(DecimalInput::from_token(&"1".repeat(4097)).is_none());
    }
}
