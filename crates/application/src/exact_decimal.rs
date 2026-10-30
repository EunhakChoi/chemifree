//! Exact ordering of bounded source decimal tokens, without floating-point conversion.

use std::{cmp::Ordering, str::FromStr};

use num_bigint::BigInt;

// Keep exponent parsing bounded for untrusted payloads. This is a rejection limit,
// never a clamp or an approximation of a value.
const MAX_EXPONENT_DIGITS: usize = 4096;

/// Numeric ordering key. The original source spelling is retained by its owner.
#[derive(Eq, PartialEq)]
pub(crate) struct DecimalOrder {
    sign: i8,
    adjusted_exponent: BigInt,
    significant_digits: Vec<u8>,
}

impl DecimalOrder {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let (negative, rest) = match text.as_bytes().first() {
            Some(b'-') => (true, &text[1..]),
            Some(b'+') => (false, &text[1..]),
            _ => (false, text),
        };
        let (mantissa, exponent) = rest.split_once(['e', 'E']).unwrap_or((rest, "0"));
        let exponent_digits = exponent.trim_start_matches(['+', '-']);
        if exponent_digits.is_empty()
            || exponent_digits.len() > MAX_EXPONENT_DIGITS
            || !exponent_digits.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let exponent = BigInt::from_str(exponent).ok()?;
        let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        if whole.is_empty() && fraction.is_empty()
            || !whole.bytes().all(|b| b.is_ascii_digit())
            || !fraction.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let mut digits: Vec<u8> = whole.bytes().chain(fraction.bytes()).collect();
        let first = digits
            .iter()
            .position(|b| *b != b'0')
            .unwrap_or(digits.len());
        if first == digits.len() {
            return Some(Self {
                sign: 0,
                adjusted_exponent: BigInt::from(0),
                significant_digits: Vec::new(),
            });
        }
        digits.drain(..first);
        let adjusted_exponent =
            exponent + BigInt::from(digits.len()) - BigInt::from(fraction.len());
        while digits.last() == Some(&b'0') {
            digits.pop();
        }
        Some(Self {
            sign: if negative { -1 } else { 1 },
            adjusted_exponent,
            significant_digits: digits,
        })
    }
}

impl Ord for DecimalOrder {
    fn cmp(&self, other: &Self) -> Ordering {
        let sign_order = self.sign.cmp(&other.sign);
        if sign_order != Ordering::Equal || self.sign == 0 {
            return sign_order;
        }
        let magnitude_order = self
            .adjusted_exponent
            .cmp(&other.adjusted_exponent)
            .then_with(|| {
                let length = self
                    .significant_digits
                    .len()
                    .max(other.significant_digits.len());
                (0..length)
                    .map(|index| {
                        self.significant_digits
                            .get(index)
                            .copied()
                            .unwrap_or(b'0')
                            .cmp(&other.significant_digits.get(index).copied().unwrap_or(b'0'))
                    })
                    .find(|ordering| *ordering != Ordering::Equal)
                    .unwrap_or(Ordering::Equal)
            });
        if self.sign < 0 {
            magnitude_order.reverse()
        } else {
            magnitude_order
        }
    }
}

impl PartialOrd for DecimalOrder {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
