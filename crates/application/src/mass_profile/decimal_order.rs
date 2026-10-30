//! Exact ordering for authoritative decimal tokens.

use std::cmp::Ordering;

// The static view is based on source decimals. Comparing their binary64 witnesses
// would lose distinctions below the binary64 range. The exponent is kept as a
// signed decimal integer, so even a very large textual exponent is not clamped.
pub(super) struct DecimalKey {
    negative: bool,
    digits: Vec<u8>,
    order: SignedDecimal,
}

struct SignedDecimal {
    negative: bool,
    magnitude: Vec<u8>,
}

impl SignedDecimal {
    fn new(text: &str, adjustment: i64) -> Self {
        let (negative, digits) = match text.as_bytes().first() {
            Some(b'-') => (true, &text[1..]),
            Some(b'+') => (false, &text[1..]),
            _ => (false, text),
        };
        let mut magnitude: Vec<_> = digits
            .trim_start_matches('0')
            .bytes()
            .map(|digit| digit - b'0')
            .collect();
        let adjustment_negative = adjustment < 0;
        let adjusted = adjustment.unsigned_abs().to_string();
        let offset: Vec<_> = adjusted
            .trim_start_matches('0')
            .bytes()
            .map(|b| b - b'0')
            .collect();
        let mut result_negative = negative;
        if negative == adjustment_negative {
            magnitude = add_magnitudes(&magnitude, &offset);
        } else {
            match cmp_magnitudes(&magnitude, &offset) {
                Ordering::Less => {
                    magnitude = subtract_magnitudes(&offset, &magnitude);
                    result_negative = adjustment_negative;
                }
                _ => magnitude = subtract_magnitudes(&magnitude, &offset),
            }
        }
        Self {
            negative: result_negative && !magnitude.is_empty(),
            magnitude,
        }
    }

    fn compare(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (true, true) => cmp_magnitudes(&other.magnitude, &self.magnitude),
            (false, false) => cmp_magnitudes(&self.magnitude, &other.magnitude),
        }
    }
}

fn cmp_magnitudes(left: &[u8], right: &[u8]) -> Ordering {
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

fn add_magnitudes(left: &[u8], right: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(left.len().max(right.len()) + 1);
    let mut a = left.iter().rev();
    let mut b = right.iter().rev();
    let mut carry = 0;
    loop {
        match (a.next(), b.next()) {
            (None, None) if carry == 0 => break,
            (a, b) => {
                let sum = a.copied().unwrap_or(0) + b.copied().unwrap_or(0) + carry;
                result.push(sum % 10);
                carry = sum / 10;
            }
        }
    }
    result.reverse();
    result
}

// The caller ensures the left magnitude is at least as large as the right.
fn subtract_magnitudes(left: &[u8], right: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(left.len());
    let mut b = right.iter().rev();
    let mut borrow = 0_i8;
    for digit in left.iter().rev() {
        let difference = *digit as i8 - b.next().copied().unwrap_or(0) as i8 - borrow;
        result.push(if difference < 0 {
            (difference + 10) as u8
        } else {
            difference as u8
        });
        borrow = i8::from(difference < 0);
    }
    while result.last() == Some(&0) {
        result.pop();
    }
    result.reverse();
    result
}

impl DecimalKey {
    pub(super) fn new(text: &str) -> Self {
        // `decimal` already validated the grammar before any key is constructed.
        let (mantissa, exponent) = text.split_once(['e', 'E']).unwrap_or((text, "0"));
        let negative = mantissa.starts_with('-');
        let fraction_len = mantissa
            .split_once('.')
            .map_or(0, |(_, fraction)| fraction.len());
        let mut digits: Vec<_> = mantissa
            .bytes()
            .filter(u8::is_ascii_digit)
            .map(|b| b - b'0')
            .collect();
        let nonzero = digits
            .iter()
            .position(|digit| *digit != 0)
            .unwrap_or(digits.len());
        digits.drain(..nonzero);
        if digits.is_empty() {
            return Self {
                negative: false,
                digits,
                order: SignedDecimal::new("0", 0),
            };
        }
        let adjustment = digits.len() as i64 - fraction_len as i64;
        while digits.last() == Some(&0) {
            digits.pop();
        }
        Self {
            negative,
            digits,
            order: SignedDecimal::new(exponent, adjustment),
        }
    }

    pub(super) fn compare(&self, other: &Self) -> Ordering {
        if self.digits.is_empty() && other.digits.is_empty() {
            return Ordering::Equal;
        }
        if self.negative != other.negative {
            return if self.negative {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        if self.digits.is_empty() {
            return if other.negative {
                Ordering::Greater
            } else {
                Ordering::Less
            };
        }
        if other.digits.is_empty() {
            return if self.negative {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        let order = self.order.compare(&other.order).then_with(|| {
            (0..self.digits.len().max(other.digits.len()))
                .map(|index| {
                    self.digits
                        .get(index)
                        .copied()
                        .unwrap_or(0)
                        .cmp(&other.digits.get(index).copied().unwrap_or(0))
                })
                .find(|order| *order != Ordering::Equal)
                .unwrap_or(Ordering::Equal)
        });
        if self.negative {
            order.reverse()
        } else {
            order
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DecimalKey;
    use std::cmp::Ordering::{Equal, Greater, Less};

    #[test]
    fn source_decimal_order_is_exact_across_signs_and_large_exponents() {
        let cases = [
            ("12.3400", "1234e-2", Equal),
            ("-12.34", "-12.33", Less),
            ("-0", "0e+99999999999999999999", Equal),
            ("1e-99999999999999999999", "2e-99999999999999999999", Less),
            ("9e-99999999999999999998", "1e-99999999999999999997", Less),
            (
                "-1e-99999999999999999999",
                "-1e-99999999999999999998",
                Greater,
            ),
            ("12345678901234567890e-1", "1234567890123456789", Equal),
        ];
        for (left, right, expected) in cases {
            assert_eq!(
                DecimalKey::new(left).compare(&DecimalKey::new(right)),
                expected
            );
            assert_eq!(
                DecimalKey::new(right).compare(&DecimalKey::new(left)),
                expected.reverse()
            );
        }
    }

    #[test]
    fn source_decimal_order_matches_independent_rational_vectors() {
        #[derive(serde::Deserialize)]
        struct Comparison {
            left: String,
            right: String,
            expected: i8,
        }
        let cases: Vec<Comparison> = serde_json::from_str(include_str!(
            "../../tests/data/mass-synthetic/decimal-comparisons.json"
        ))
        .unwrap();
        assert_eq!(cases.len(), 256);
        for case in cases {
            let expected = match case.expected {
                -1 => Less,
                0 => Equal,
                1 => Greater,
                _ => panic!("invalid independent comparison"),
            };
            assert_eq!(
                DecimalKey::new(&case.left).compare(&DecimalKey::new(&case.right)),
                expected
            );
        }
    }
}
