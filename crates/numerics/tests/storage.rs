//! Authored mathematical boundary cases; no model coefficients or private inputs.

use chemifree_numerics::{
    Binary32, Binary64, Exact, Extended80, FloatClass, NumericError, Real48, Sign,
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

#[test]
fn exact_rationals_reduce_and_preserve_signed_zero_without_unbounded_inputs() {
    let reduced = ratio(true, 6_u8.into(), 8_u8.into());
    assert!(reduced.is_negative());
    assert_eq!(reduced.numerator(), &BigUint::from(3_u8));
    assert_eq!(reduced.denominator(), &BigUint::from(4_u8));
    let zero = ratio(true, 0_u8.into(), 3_u8.into());
    assert!(zero.is_negative());
    assert_eq!(zero.denominator(), &BigUint::from(1_u8));
    assert_eq!(
        Exact::from_ratio(Sign::Positive, 1_u8.into(), 0_u8.into()),
        Err(NumericError::ZeroDenominator)
    );
    assert_eq!(
        Exact::from_ratio(Sign::Positive, BigUint::from(1_u8) << 65_536, 1_u8.into()),
        Err(NumericError::ResourceLimit)
    );
}

#[test]
fn ieee_raw_bits_classify_zeros_subnormals_infinities_and_nans() {
    for (bits, class, negative) in [
        (0x0000_0000, FloatClass::Zero, false),
        (0x8000_0000, FloatClass::Zero, true),
        (0x0000_0001, FloatClass::Subnormal, false),
        (0x3f80_0000, FloatClass::Normal, false),
        (0xff80_0000, FloatClass::Infinite, true),
        (0x7fc0_0001, FloatClass::QuietNaN, false),
        (0x7f80_0001, FloatClass::SignalingNaN, false),
    ] {
        let value = Binary32::from_bits(bits);
        assert_eq!(
            (value.bits(), value.class(), value.is_negative()),
            (bits, class, negative)
        );
    }
    for (bits, class, negative) in [
        (0x0000_0000_0000_0000, FloatClass::Zero, false),
        (0x8000_0000_0000_0000, FloatClass::Zero, true),
        (0x0000_0000_0000_0001, FloatClass::Subnormal, false),
        (0x3ff0_0000_0000_0000, FloatClass::Normal, false),
        (0xfff0_0000_0000_0000, FloatClass::Infinite, true),
        (0x7ff8_0000_0000_0001, FloatClass::QuietNaN, false),
        (0x7ff0_0000_0000_0001, FloatClass::SignalingNaN, false),
    ] {
        let value = Binary64::from_bits(bits);
        assert_eq!(
            (value.bits(), value.class(), value.is_negative()),
            (bits, class, negative)
        );
    }
    assert_eq!(
        Binary32::from_bits(0x7f80_0000).exact(),
        Err(NumericError::NonFinite)
    );
    assert_eq!(
        Binary64::from_bits(0x7ff8_0000_0000_0001).exact(),
        Err(NumericError::NonFinite)
    );
}

#[test]
fn ieee_decoding_produces_exact_values_without_losing_zero_sign() {
    let value = Binary64::from_bits(0x3ff8_0000_0000_0000).exact().unwrap();
    assert_eq!(value, ratio(false, 3_u8.into(), 2_u8.into()));
    assert_eq!(Binary32::from_bits(1).exact().unwrap(), pow2(-149, false));
    assert_eq!(Binary64::from_bits(1).exact().unwrap(), pow2(-1074, false));
    assert!(
        Binary64::from_bits(1_u64 << 63)
            .exact()
            .unwrap()
            .is_negative()
    );
}

#[test]
fn binary32_rounds_ties_to_even_and_handles_subnormal_boundaries() {
    let denominator = BigUint::from(1_u8) << 24_usize;
    let lower_even = ratio(false, &denominator + 1_u8, denominator.clone());
    let upper_even = ratio(false, &denominator + 3_u8, denominator);
    assert_eq!(
        Binary32::from_exact(&lower_even).unwrap().bits(),
        0x3f80_0000
    );
    assert_eq!(
        Binary32::from_exact(&upper_even).unwrap().bits(),
        0x3f80_0002
    );
    assert_eq!(Binary32::from_exact(&pow2(-150, false)).unwrap().bits(), 0);
    assert_eq!(
        Binary32::from_exact(&pow2(-150, true)).unwrap().bits(),
        0x8000_0000
    );
    let above_half = ratio(false, 3_u8.into(), BigUint::from(1_u8) << 151_usize);
    assert_eq!(Binary32::from_exact(&above_half).unwrap().bits(), 1);
    let max = Binary32::from_bits(0x7f7f_ffff);
    assert_eq!(Binary32::from_exact(&max.exact().unwrap()).unwrap(), max);
    assert_eq!(
        Binary32::from_exact(&pow2(128, false)),
        Err(NumericError::Overflow)
    );
    let threshold = (BigUint::from(1_u8) << 128_usize) - (BigUint::from(1_u8) << 103_usize);
    assert_eq!(
        Binary32::from_exact(&ratio(false, &threshold - 1_u8, 1_u8.into())).unwrap(),
        max
    );
    assert_eq!(
        Binary32::from_exact(&ratio(false, threshold, 1_u8.into())),
        Err(NumericError::Overflow)
    );
    let normal_midpoint = ratio(
        false,
        (BigUint::from(1_u8) << 26_usize) - 4_u8,
        BigUint::from(1_u8) << 152_usize,
    );
    let below_midpoint = ratio(
        false,
        (BigUint::from(1_u8) << 26_usize) - 5_u8,
        BigUint::from(1_u8) << 152_usize,
    );
    assert_eq!(
        Binary32::from_exact(&normal_midpoint).unwrap().bits(),
        0x0080_0000
    );
    assert_eq!(
        Binary32::from_exact(&below_midpoint).unwrap().bits(),
        0x007f_ffff
    );
}

#[test]
fn binary64_rounds_ties_to_even_and_handles_subnormal_boundaries() {
    let denominator = BigUint::from(1_u8) << 53_usize;
    let lower_even = ratio(false, &denominator + 1_u8, denominator.clone());
    let upper_even = ratio(false, &denominator + 3_u8, denominator);
    assert_eq!(
        Binary64::from_exact(&lower_even).unwrap().bits(),
        0x3ff0_0000_0000_0000
    );
    assert_eq!(
        Binary64::from_exact(&upper_even).unwrap().bits(),
        0x3ff0_0000_0000_0002
    );
    assert_eq!(Binary64::from_exact(&pow2(-1075, false)).unwrap().bits(), 0);
    assert_eq!(
        Binary64::from_exact(&pow2(-1075, true)).unwrap().bits(),
        1_u64 << 63
    );
    let above_half = ratio(false, 3_u8.into(), BigUint::from(1_u8) << 1076_usize);
    assert_eq!(Binary64::from_exact(&above_half).unwrap().bits(), 1);
    let max = Binary64::from_bits(0x7fef_ffff_ffff_ffff);
    assert_eq!(Binary64::from_exact(&max.exact().unwrap()).unwrap(), max);
    assert_eq!(
        Binary64::from_exact(&pow2(1024, false)),
        Err(NumericError::Overflow)
    );
    let threshold = (BigUint::from(1_u8) << 1024_usize) - (BigUint::from(1_u8) << 970_usize);
    assert_eq!(
        Binary64::from_exact(&ratio(false, &threshold - 1_u8, 1_u8.into())).unwrap(),
        max
    );
    assert_eq!(
        Binary64::from_exact(&ratio(false, threshold, 1_u8.into())),
        Err(NumericError::Overflow)
    );
    let normal_midpoint = ratio(
        false,
        (BigUint::from(1_u8) << 55_usize) - 4_u8,
        BigUint::from(1_u8) << 1077_usize,
    );
    let below_midpoint = ratio(
        false,
        (BigUint::from(1_u8) << 55_usize) - 5_u8,
        BigUint::from(1_u8) << 1077_usize,
    );
    assert_eq!(
        Binary64::from_exact(&normal_midpoint).unwrap().bits(),
        0x0010_0000_0000_0000
    );
    assert_eq!(
        Binary64::from_exact(&below_midpoint).unwrap().bits(),
        0x000f_ffff_ffff_ffff
    );
}

#[test]
fn ieee_widening_and_narrowing_are_explicit_and_keep_signed_zero() {
    let f32 = Binary32::from_bits(0xbf80_0001);
    let widened = Binary64::from_binary32(f32).unwrap();
    assert_eq!(widened.exact().unwrap(), f32.exact().unwrap());
    assert_eq!(Binary32::from_binary64(widened).unwrap(), f32);
    assert_eq!(
        Binary64::from_binary32(Binary32::from_bits(0x8000_0000))
            .unwrap()
            .bits(),
        1_u64 << 63
    );
    assert_eq!(
        Binary32::from_binary64(Binary64::from_bits(1_u64 << 63))
            .unwrap()
            .bits(),
        0x8000_0000
    );
    assert_eq!(
        Binary32::from_binary64(Binary64::from_bits(0x7ff0_0000_0000_0000)),
        Err(NumericError::NonFinite)
    );
}

#[test]
fn real48_uses_six_byte_format_and_half_away_rounding() {
    let one = ratio(false, 1_u8.into(), 1_u8.into());
    assert_eq!(
        Real48::from_exact(&one).unwrap().bytes(),
        [0x81, 0, 0, 0, 0, 0]
    );
    let denominator = BigUint::from(1_u8) << 40_usize;
    let halfway = ratio(false, &denominator + 1_u8, denominator.clone());
    let negative = ratio(true, &denominator + 1_u8, denominator);
    assert_eq!(
        Real48::from_exact(&halfway).unwrap().bytes(),
        [0x81, 1, 0, 0, 0, 0]
    );
    assert_eq!(
        Real48::from_exact(&negative).unwrap().bytes(),
        [0x81, 1, 0, 0, 0, 0x80]
    );
    let max = Real48::from_bytes([0xff, 0xff, 0xff, 0xff, 0xff, 0x7f]);
    assert_eq!(Real48::from_exact(&max.exact().unwrap()).unwrap(), max);
    let mixed = Real48::from_bytes([0x80, 0x78, 0x56, 0x34, 0x12, 0x7f]);
    assert_eq!(
        mixed.exact().unwrap(),
        ratio(
            false,
            BigUint::from(0xff_1234_5678_u64),
            BigUint::from(1_u8) << 40_usize
        )
    );
    let threshold = (BigUint::from(1_u8) << 127_usize) - (BigUint::from(1_u8) << 86_usize);
    assert_eq!(
        Real48::from_exact(&ratio(false, &threshold - 1_u8, 1_u8.into())).unwrap(),
        max
    );
    assert_eq!(
        Real48::from_exact(&ratio(false, threshold, 1_u8.into())),
        Err(NumericError::Overflow)
    );
}

#[test]
fn real48_rounds_before_flushing_and_does_not_overwrite_on_error() {
    let denominator = BigUint::from(1_u8) << 169_usize;
    let midpoint = ratio(false, (BigUint::from(1_u8) << 41_usize) - 1_u8, denominator);
    let below = ratio(
        false,
        (BigUint::from(1_u8) << 43_usize) - 5_u8,
        BigUint::from(1_u8) << 171_usize,
    );
    assert_eq!(
        Real48::from_exact(&midpoint).unwrap().bytes(),
        [1, 0, 0, 0, 0, 0]
    );
    assert_eq!(Real48::from_exact(&below).unwrap().bytes(), [0; 6]);
    assert_eq!(
        Real48::from_exact(&pow2(-129, true)).unwrap().bytes(),
        [0; 6]
    );
    let raw_zero = Real48::from_bytes([0, 1, 2, 3, 4, 0x80]);
    assert_eq!(raw_zero.bytes(), [0, 1, 2, 3, 4, 0x80]);
    assert!(!raw_zero.exact().unwrap().is_negative());
    assert_eq!(
        Real48::from_exact(&raw_zero.exact().unwrap())
            .unwrap()
            .bytes(),
        [0; 6]
    );
    let mut destination = Real48::from_bytes([0x81, 0, 0, 0, 0, 0]);
    for bits in [
        0x7ff0_0000_0000_0000,
        0xfff0_0000_0000_0000,
        0x7ff8_0000_0000_0001,
        0x7ff0_0000_0000_0001,
    ] {
        assert_eq!(
            destination.store_binary64(Binary64::from_bits(bits)),
            Err(NumericError::NonFinite)
        );
        assert_eq!(destination.bytes(), [0x81, 0, 0, 0, 0, 0]);
    }
    assert_eq!(
        destination.store_binary64(Binary64::from_bits(0x7fef_ffff_ffff_ffff)),
        Err(NumericError::Overflow)
    );
    assert_eq!(destination.bytes(), [0x81, 0, 0, 0, 0, 0]);
    assert_eq!(
        Real48::from_exact(&pow2(127, false)),
        Err(NumericError::Overflow)
    );
}

#[test]
fn independent_fraction_vectors_cover_both_signs_and_tie_rules() {
    let vectors = include_str!("data/storage-vectors.tsv");
    let mut count = 0;
    for (index, line) in vectors.lines().enumerate() {
        if line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 5, "vector line {index}");
        let negative = match fields[1] {
            "0" => false,
            "1" => true,
            _ => panic!("invalid sign on vector line {index}"),
        };
        let value = ratio(
            negative,
            fields[2].parse().unwrap(),
            fields[3].parse().unwrap(),
        );
        let expected = fields[4];
        let actual = match fields[0] {
            "real48" => Real48::from_exact(&value).unwrap().bytes().to_vec(),
            "binary32" => Binary32::from_exact(&value)
                .unwrap()
                .bits()
                .to_le_bytes()
                .to_vec(),
            "binary64" => Binary64::from_exact(&value)
                .unwrap()
                .bits()
                .to_le_bytes()
                .to_vec(),
            "extended80" => Extended80::from_exact(&value).unwrap().bytes().to_vec(),
            _ => panic!("invalid profile on vector line {index}"),
        };
        let encoded: String = actual.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(encoded, expected, "{} vector line {index}", fields[0]);
        count += 1;
    }
    assert_eq!(count, 1178);
}
