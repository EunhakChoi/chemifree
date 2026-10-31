//! Independent mathematical cases for explicit extended storage and arithmetic.

use chemifree_numerics::{
    Binary32, Binary64, Exact, Extended80, ExtendedClass, NumericError, Real48, Sign,
};
use num_bigint::BigUint;

fn ratio(negative: bool, numerator: BigUint, denominator: BigUint) -> Exact {
    let sign = if negative {
        Sign::Negative
    } else {
        Sign::Positive
    };
    Exact::from_ratio(sign, numerator, denominator).unwrap()
}

fn pow2(exponent: i32, negative: bool) -> Exact {
    if exponent >= 0 {
        ratio(
            negative,
            BigUint::from(1_u8) << exponent as usize,
            1_u8.into(),
        )
    } else {
        ratio(
            negative,
            1_u8.into(),
            BigUint::from(1_u8) << (-exponent) as usize,
        )
    }
}

fn extended(negative: bool, exponent: u16, significand: u64) -> Extended80 {
    let mut bytes = [0_u8; 10];
    bytes[..8].copy_from_slice(&significand.to_le_bytes());
    let signed_exponent = exponent | (u16::from(negative) << 15);
    bytes[8..].copy_from_slice(&signed_exponent.to_le_bytes());
    Extended80::from_bytes(bytes)
}

#[test]
fn extended_raw_bits_distinguish_all_classes_and_keep_sign() {
    let integer = 1_u64 << 63;
    for (value, class, negative) in [
        (extended(false, 0, 0), ExtendedClass::Zero, false),
        (extended(true, 0, 0), ExtendedClass::Zero, true),
        (extended(false, 0, 1), ExtendedClass::Subnormal, false),
        (
            extended(false, 0, integer),
            ExtendedClass::PseudoDenormal,
            false,
        ),
        (extended(false, 1, integer), ExtendedClass::Normal, false),
        (
            extended(true, 0x7fff, integer),
            ExtendedClass::Infinite,
            true,
        ),
        (
            extended(false, 0x7fff, integer | (1_u64 << 62) | 1),
            ExtendedClass::QuietNaN,
            false,
        ),
        (
            extended(false, 0x7fff, integer | 1),
            ExtendedClass::SignalingNaN,
            false,
        ),
        (extended(false, 1, 1), ExtendedClass::Unsupported, false),
        (
            extended(false, 0x7fff, 1),
            ExtendedClass::Unsupported,
            false,
        ),
    ] {
        assert_eq!((value.class(), value.is_negative()), (class, negative));
        assert_eq!(Extended80::from_bytes(value.bytes()), value);
    }
}

#[test]
fn extended_pseudodenormal_has_the_normal_value_but_retains_its_raw_bytes() {
    let pseudo = extended(true, 0, 1_u64 << 63);
    let normal = extended(true, 1, 1_u64 << 63);
    assert_ne!(pseudo.bytes(), normal.bytes());
    assert_eq!(pseudo.exact().unwrap(), pow2(-16382, true));
    assert_eq!(pseudo.exact().unwrap(), normal.exact().unwrap());
    assert_eq!(
        Extended80::from_exact(&pseudo.exact().unwrap()).unwrap(),
        normal
    );
    assert_eq!(extended(false, 0, 1).exact().unwrap(), pow2(-16445, false));
}

#[test]
fn extended_nonfinite_and_unsupported_inputs_are_explicit_errors() {
    assert_eq!(
        extended(false, 0x7fff, 1_u64 << 63).exact(),
        Err(NumericError::NonFinite)
    );
    assert_eq!(
        extended(false, 0x7fff, (1_u64 << 63) | 1).exact(),
        Err(NumericError::NonFinite)
    );
    assert_eq!(
        extended(false, 1, 1).exact(),
        Err(NumericError::UnsupportedEncoding)
    );
    assert_eq!(
        Extended80::from_binary64(Binary64::from_bits(0x7ff0_0000_0000_0000)),
        Err(NumericError::NonFinite)
    );
    let one = extended(false, 0x3fff, 1_u64 << 63);
    assert_eq!(
        one.checked_add(extended(false, 0x7fff, 1_u64 << 63)),
        Err(NumericError::NonFinite)
    );
    assert_eq!(
        one.checked_add(extended(false, 1, 1)),
        Err(NumericError::UnsupportedEncoding)
    );
}

#[test]
fn extended_rounding_uses_64_bits_ties_even_and_subnormals() {
    let denominator = BigUint::from(1_u8) << 64_usize;
    let lower_even = ratio(false, &denominator + 1_u8, denominator.clone());
    let upper_even = ratio(false, &denominator + 3_u8, denominator);
    assert_eq!(
        Extended80::from_exact(&lower_even).unwrap(),
        extended(false, 0x3fff, 1_u64 << 63)
    );
    assert_eq!(
        Extended80::from_exact(&upper_even).unwrap(),
        extended(false, 0x3fff, (1_u64 << 63) + 2)
    );
    assert_eq!(
        Extended80::from_exact(&pow2(-16446, true)).unwrap(),
        extended(true, 0, 0)
    );
    let above_half = ratio(false, 3_u8.into(), BigUint::from(1_u8) << 16447_usize);
    assert_eq!(
        Extended80::from_exact(&above_half).unwrap(),
        extended(false, 0, 1)
    );
    let normal_midpoint = ratio(
        false,
        (BigUint::from(1_u8) << 66_usize) - 4_u8,
        BigUint::from(1_u8) << 16448_usize,
    );
    assert_eq!(
        Extended80::from_exact(&normal_midpoint).unwrap(),
        extended(false, 1, 1_u64 << 63)
    );
}

#[test]
fn extended_largest_finite_and_rounding_overflow_are_distinct() {
    let max = extended(false, 0x7ffe, u64::MAX);
    assert_eq!(Extended80::from_exact(&max.exact().unwrap()).unwrap(), max);
    let threshold = (BigUint::from(1_u8) << 16384_usize) - (BigUint::from(1_u8) << 16319_usize);
    assert_eq!(
        Extended80::from_exact(&ratio(false, &threshold - 1_u8, 1_u8.into())).unwrap(),
        max
    );
    assert_eq!(
        Extended80::from_exact(&ratio(false, threshold, 1_u8.into())),
        Err(NumericError::Overflow)
    );
}

#[test]
fn explicit_intermediate_stores_change_the_expected_result() {
    let one = ratio(false, 1_u8.into(), 1_u8.into());
    let f32_big = Binary32::from_exact(&pow2(24, false)).unwrap();
    let f32_one = Binary32::from_exact(&one).unwrap();
    assert_eq!(
        f32_big
            .checked_add(f32_one)
            .unwrap()
            .checked_sub(f32_big)
            .unwrap()
            .bits(),
        0
    );
    let regrouped = f32_big
        .checked_add(f32_one.checked_sub(f32_big).unwrap())
        .unwrap();
    assert_eq!(regrouped.bits(), 0x3f80_0000);
    let f64_big = Binary64::from_exact(&pow2(53, false)).unwrap();
    let f64_one = Binary64::from_exact(&one).unwrap();
    assert_eq!(
        f64_big
            .checked_add(f64_one)
            .unwrap()
            .checked_sub(f64_big)
            .unwrap()
            .bits(),
        0
    );
    let regrouped = f64_big
        .checked_add(f64_one.checked_sub(f64_big).unwrap())
        .unwrap();
    assert_eq!(regrouped.bits(), 0x3ff0_0000_0000_0000);
    let extended_big = Extended80::from_binary64(f64_big).unwrap();
    let extended_one = Extended80::from_binary64(f64_one).unwrap();
    assert_eq!(
        extended_big
            .checked_add(extended_one)
            .unwrap()
            .checked_sub(extended_big)
            .unwrap()
            .exact()
            .unwrap(),
        one
    );
    let real_big = Real48::from_exact(&pow2(40, false)).unwrap();
    let real_one = Real48::from_exact(&one).unwrap();
    let result = real_big
        .checked_add(real_one)
        .unwrap()
        .checked_sub(real_big)
        .unwrap();
    assert_eq!(
        result.exact().unwrap(),
        ratio(false, 2_u8.into(), 1_u8.into())
    );
}

#[test]
fn finite_arithmetic_keeps_zero_sign_and_reports_invalid_inputs() {
    let negative_zero = Binary64::from_bits(1_u64 << 63);
    let positive_zero = Binary64::from_bits(0);
    let one = Binary64::from_bits(0x3ff0_0000_0000_0000);
    assert_eq!(
        negative_zero.checked_add(negative_zero).unwrap(),
        negative_zero
    );
    assert_eq!(
        negative_zero.checked_add(positive_zero).unwrap(),
        positive_zero
    );
    assert_eq!(
        negative_zero.checked_sub(negative_zero).unwrap(),
        positive_zero
    );
    assert_eq!(negative_zero.checked_mul(one).unwrap(), negative_zero);
    assert_eq!(
        one.checked_div(positive_zero),
        Err(NumericError::DivisionByZero)
    );
    assert_eq!(
        one.checked_add(Binary64::from_bits(0x7ff8_0000_0000_0001)),
        Err(NumericError::NonFinite)
    );
    assert_eq!(
        Binary32::from_bits(0x7f7f_ffff).checked_add(Binary32::from_bits(0x7f7f_ffff)),
        Err(NumericError::Overflow)
    );
}

#[test]
fn extended_division_and_binary64_conversion_are_explicit() {
    let one = Extended80::from_exact(&ratio(false, 1_u8.into(), 1_u8.into())).unwrap();
    let three = Extended80::from_exact(&ratio(false, 3_u8.into(), 1_u8.into())).unwrap();
    assert_eq!(
        one.checked_div(three).unwrap(),
        extended(false, 0x3ffd, 0xaaaa_aaaa_aaaa_aaab)
    );
    assert_eq!(
        extended(true, 0, 0).to_binary64().unwrap().bits(),
        1_u64 << 63
    );
    assert_eq!(
        Extended80::from_binary64(Binary64::from_bits(1_u64 << 63)).unwrap(),
        extended(true, 0, 0)
    );
    assert_eq!(extended(false, 0, 1).to_binary64().unwrap().bits(), 0);
}

#[test]
fn division_uses_each_format_rounding_and_rejects_zero_divisors() {
    let one = ratio(false, 1_u8.into(), 1_u8.into());
    let three = ratio(false, 3_u8.into(), 1_u8.into());
    let zero = ratio(false, 0_u8.into(), 1_u8.into());
    let one32 = Binary32::from_exact(&one).unwrap();
    let three32 = Binary32::from_exact(&three).unwrap();
    assert_eq!(one32.checked_div(three32).unwrap().bits(), 0x3eaa_aaab);
    assert_eq!(
        one32.checked_div(Binary32::from_exact(&zero).unwrap()),
        Err(NumericError::DivisionByZero)
    );
    let one64 = Binary64::from_exact(&one).unwrap();
    let three64 = Binary64::from_exact(&three).unwrap();
    assert_eq!(
        one64.checked_div(three64).unwrap().bits(),
        0x3fd5_5555_5555_5555
    );
    let one48 = Real48::from_exact(&one).unwrap();
    let three48 = Real48::from_exact(&three).unwrap();
    assert_eq!(
        one48.checked_div(three48).unwrap().bytes(),
        [0x7f, 0xab, 0xaa, 0xaa, 0xaa, 0x2a]
    );
    assert_eq!(
        one48.checked_div(Real48::from_exact(&zero).unwrap()),
        Err(NumericError::DivisionByZero)
    );
    let one80 = Extended80::from_exact(&one).unwrap();
    assert_eq!(
        one80.checked_div(Extended80::from_exact(&zero).unwrap()),
        Err(NumericError::DivisionByZero)
    );
}
