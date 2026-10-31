//! Independent formatting witnesses keep display text separate from stored bits.

use chemifree_numerics::{
    Binary32, Binary64, Exact, LengthUnit, LengthValue, NumericError, Sign, format_default_six,
    format_fixed, round_three_half_away_to_binary32,
};
use num_bigint::BigUint;

fn native(raw: Binary64) -> Result<Binary32, NumericError> {
    round_three_half_away_to_binary32(&raw.exact()?)
}

#[test]
fn display_and_native_rounding_match_independent_witnesses() {
    let (mut defaults, mut fixed, mut native_count) = (0, 0, 0);
    for row in include_str!("data/display-vectors.tsv").lines() {
        if row.is_empty() || row.starts_with('#') {
            continue;
        }
        let columns: Vec<_> = row.split('\t').collect();
        assert_eq!(columns.len(), 4, "{row}");
        let raw = Binary64::from_bits(u64::from_str_radix(columns[1], 16).unwrap());
        match columns[0] {
            "default6" => {
                assert_eq!(format_default_six(raw).unwrap(), columns[3], "{row}");
                defaults += 1;
            }
            "fixed" => {
                let places = columns[2].parse::<u8>().unwrap();
                assert_eq!(format_fixed(raw, places).unwrap(), columns[3], "{row}");
                fixed += 1;
            }
            "native3" => {
                let result = native(raw);
                if columns[3] == "overflow" {
                    assert_eq!(result, Err(NumericError::Overflow), "{row}");
                } else {
                    let expected = u32::from_str_radix(columns[3], 16).unwrap();
                    assert_eq!(result.unwrap().bits(), expected, "{row}");
                }
                native_count += 1;
            }
            _ => panic!("unknown fixture profile: {row}"),
        }
    }
    assert_eq!((defaults, fixed, native_count), (224, 2016, 224));
}

#[test]
fn decimal_ties_and_scientific_boundaries_are_explicit() {
    let lower_even = Binary64::from_bits(0x4001_0000_0000_0000); // 2.125
    let upper_even = Binary64::from_bits(0x4003_0000_0000_0000); // 2.375
    assert_eq!(format_fixed(lower_even, 2).unwrap(), "2.12");
    assert_eq!(format_fixed(upper_even, 2).unwrap(), "2.38");
    assert_eq!(
        format_default_six(Binary64::from_bits(0x3ff0_4000_0000_0000)).unwrap(),
        "1.01562"
    );
    assert_eq!(
        format_default_six(Binary64::from_bits(0x3ff0_c000_0000_0000)).unwrap(),
        "1.04688"
    );
    let negative_zero = Binary64::from_bits(1_u64 << 63);
    assert_eq!(format_default_six(negative_zero).unwrap(), "-0");
    assert_eq!(format_fixed(negative_zero, 3).unwrap(), "-0.000");
}

#[test]
fn exact_native_rounding_does_not_require_an_intermediate_binary64_value() {
    let positive = Exact::from_ratio(Sign::Positive, 2001_u16.into(), 2000_u16.into()).unwrap();
    let negative = Exact::from_ratio(Sign::Negative, 2001_u16.into(), 2000_u16.into()).unwrap();
    assert_eq!(
        round_three_half_away_to_binary32(&positive).unwrap().bits(),
        0x3f80_20c5
    );
    assert_eq!(
        round_three_half_away_to_binary32(&negative).unwrap().bits(),
        0xbf80_20c5
    );
    let tiny_negative =
        Exact::from_ratio(Sign::Negative, 1_u8.into(), BigUint::from(10_000_u16)).unwrap();
    assert_eq!(
        round_three_half_away_to_binary32(&tiny_negative)
            .unwrap()
            .bits(),
        0x8000_0000
    );
}

#[test]
fn unit_raw_bits_and_display_are_distinct() {
    let points = LengthValue::new(
        Binary64::from_bits(0x4052_0000_0000_0000),
        LengthUnit::ComputerPoint,
    )
    .unwrap();
    let millimetres = points.convert(LengthUnit::Millimetre).unwrap();
    assert_eq!(millimetres.unit(), LengthUnit::Millimetre);
    assert_eq!(millimetres.raw().bits(), 0x4039_6666_6666_6666);
    assert_eq!(format_fixed(millimetres.raw(), 2).unwrap(), "25.40");
    assert_eq!(millimetres.raw().bits(), 0x4039_6666_6666_6666);
    assert_eq!(points.raw().bits(), 0x4052_0000_0000_0000);
}

#[test]
fn unsupported_precision_and_nonfinite_inputs_are_errors() {
    let one = Binary64::from_bits(0x3ff0_0000_0000_0000);
    let infinity = Binary64::from_bits(0x7ff0_0000_0000_0000);
    let nan = Binary64::from_bits(0x7ff8_0000_0000_0001);
    assert_eq!(format_fixed(one, 9), Err(NumericError::PrecisionOutOfRange));
    assert_eq!(format_default_six(infinity), Err(NumericError::NonFinite));
    assert_eq!(format_fixed(nan, 2), Err(NumericError::NonFinite));
    assert_eq!(native(nan), Err(NumericError::NonFinite));
}
