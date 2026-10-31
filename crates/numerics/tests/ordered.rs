//! Authored, independent witnesses for ordered intermediate stores.

use chemifree_numerics::{Binary64, NumericError, TurnDirection, next_heading, round_deviation};
use num_bigint::BigUint;

fn bits(hex: &str) -> Binary64 {
    Binary64::from_bits(u64::from_str_radix(hex, 16).unwrap())
}

#[test]
fn authored_heading_pairs_match_independent_extended_results() {
    let mut count = 0;
    for row in include_str!("data/heading-vectors.tsv").lines() {
        if row.starts_with('#') || row.is_empty() {
            continue;
        }
        let columns: Vec<_> = row.split('\t').collect();
        assert_eq!(columns.len(), 7, "{row}");
        let direction = match columns[1] {
            "add" => TurnDirection::Add,
            "subtract" => TurnDirection::Subtract,
            _ => panic!("bad direction: {row}"),
        };
        let first = next_heading(bits(columns[2]), bits(columns[3]), direction).unwrap();
        assert_eq!(first, bits(columns[5]), "{row}");
        let second = next_heading(first, bits(columns[4]), direction).unwrap();
        assert_eq!(second, bits(columns[6]), "{row}");
        count += 1;
    }
    assert_eq!(count, 18);
}

#[test]
fn heading_zero_sign_and_winding_are_explicit() {
    let positive_zero = Binary64::from_bits(0);
    let negative_zero = Binary64::from_bits(1_u64 << 63);
    let ninety = Binary64::from_bits(0x4056_8000_0000_0000);
    assert_eq!(
        next_heading(positive_zero, ninety, TurnDirection::Add)
            .unwrap()
            .bits(),
        0x3ff9_21fb_5444_2d18
    );
    assert_eq!(
        next_heading(positive_zero, ninety, TurnDirection::Subtract)
            .unwrap()
            .bits(),
        0xbff9_21fb_5444_2d18
    );
    assert_eq!(
        next_heading(negative_zero, positive_zero, TurnDirection::Subtract).unwrap(),
        negative_zero
    );
    assert_eq!(
        next_heading(negative_zero, positive_zero, TurnDirection::Add).unwrap(),
        positive_zero
    );
}

#[test]
fn heading_rejects_nonfinite_inputs_and_finite_overflow() {
    let one = Binary64::from_bits(0x3ff0_0000_0000_0000);
    let nan = Binary64::from_bits(0x7ff8_0000_0000_0001);
    let infinity = Binary64::from_bits(0x7ff0_0000_0000_0000);
    let maximum = Binary64::from_bits(0x7fef_ffff_ffff_ffff);
    assert_eq!(
        next_heading(nan, one, TurnDirection::Add),
        Err(NumericError::NonFinite)
    );
    assert_eq!(
        next_heading(one, infinity, TurnDirection::Add),
        Err(NumericError::NonFinite)
    );
    assert_eq!(
        next_heading(maximum, maximum, TurnDirection::Add),
        Err(NumericError::Overflow)
    );
}

#[test]
fn deviation_uses_half_up_and_the_strict_greater_than_ten_flag() {
    let ten_point_four_nine = Binary64::from_bits(0x4024_fae1_47ae_147b);
    let below_half = Binary64::from_bits(0x4024_ffff_ffff_ffff);
    let exact_half = Binary64::from_bits(0x4025_0000_0000_0000);
    let ordinary = round_deviation(ten_point_four_nine).unwrap();
    assert_eq!(ordinary.degrees(), &BigUint::from(10_u8));
    assert!(!ordinary.is_irregular());
    let below = round_deviation(below_half).unwrap();
    assert_eq!(below.degrees(), &BigUint::from(10_u8));
    assert!(!below.is_irregular());
    let half = round_deviation(exact_half).unwrap();
    assert_eq!(half.degrees(), &BigUint::from(11_u8));
    assert!(half.is_irregular());
    let signed_zero = round_deviation(Binary64::from_bits(1_u64 << 63)).unwrap();
    assert_eq!(signed_zero.degrees(), &BigUint::from(0_u8));
    assert!(!signed_zero.is_irregular());
    let huge = round_deviation(Binary64::from_bits(0x7fef_ffff_ffff_ffff)).unwrap();
    assert!(huge.is_irregular());
    assert!(huge.degrees().bits() > 1000);
}

#[test]
fn deviation_rejects_negative_and_nonfinite_values() {
    assert_eq!(
        round_deviation(Binary64::from_bits(0xbff0_0000_0000_0000)),
        Err(NumericError::NegativeInput)
    );
    assert_eq!(
        round_deviation(Binary64::from_bits(0x7ff0_0000_0000_0000)),
        Err(NumericError::NonFinite)
    );
}
