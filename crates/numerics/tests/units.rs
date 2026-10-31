//! Authored unit conversions and exact raw-storage boundaries.

use chemifree_numerics::{
    Binary64, ConcentrationUnit, ConcentrationValue, LengthUnit, LengthValue,
    MolarMassGramsPerMole, NumericError, Real48,
};

fn unit(name: &str) -> LengthUnit {
    match name {
        "point" => LengthUnit::ComputerPoint,
        "millimetre" => LengthUnit::Millimetre,
        "centimetre" => LengthUnit::Centimetre,
        "inch" => LengthUnit::Inch,
        _ => panic!("unknown fixture unit {name}"),
    }
}

#[test]
fn length_conversions_match_independent_fraction_witnesses() {
    let mut count = 0;
    let mut overflows = 0;
    for row in include_str!("data/unit-vectors.tsv").lines() {
        if row.starts_with('#') || row.is_empty() {
            continue;
        }
        let columns: Vec<_> = row.split('\t').collect();
        assert_eq!(columns.len(), 4, "{row}");
        let raw = Binary64::from_bits(u64::from_str_radix(columns[2], 16).unwrap());
        let value = LengthValue::new(raw, unit(columns[0])).unwrap();
        let target = unit(columns[1]);
        let result = value.convert(target);
        if columns[3] == "overflow" {
            assert_eq!(result, Err(NumericError::Overflow), "{row}");
            overflows += 1;
        } else {
            let expected = u64::from_str_radix(columns[3], 16).unwrap();
            let converted = result.unwrap();
            assert_eq!(converted.raw().bits(), expected, "{row}");
            assert_eq!(converted.unit(), target, "{row}");
        }
        assert_eq!(value.raw(), raw, "the input is preserved: {row}");
        count += 1;
    }
    assert_eq!((count, overflows), (384, 24));
}

#[test]
fn length_keeps_units_and_raw_bits_and_rejects_nonfinite_values() {
    let one_inch =
        LengthValue::new(Binary64::from_bits(0x3ff0_0000_0000_0000), LengthUnit::Inch).unwrap();
    let points = one_inch.convert(LengthUnit::ComputerPoint).unwrap();
    assert_eq!(points.raw().bits(), 0x4052_0000_0000_0000);
    assert_eq!(points.unit(), LengthUnit::ComputerPoint);
    assert_eq!(one_inch.unit(), LengthUnit::Inch);
    assert_eq!(
        LengthValue::new(Binary64::from_bits(0x7ff8_0000_0000_0001), LengthUnit::Inch),
        Err(NumericError::NonFinite)
    );
}

#[test]
fn concentration_requires_a_positive_explicit_molar_mass() {
    let one = Real48::from_bytes([0x81, 0, 0, 0, 0, 0]);
    let negative_one = Real48::from_bytes([0x81, 0, 0, 0, 0, 0x80]);
    let zero = Real48::from_bytes([0, 0, 0, 0, 0, 0]);
    let value = ConcentrationValue::new(one, ConcentrationUnit::MolesPerLitre);
    assert_eq!(
        value.convert(ConcentrationUnit::GramsPerLitre, None),
        Err(NumericError::MissingAuxiliary)
    );
    assert_eq!(
        MolarMassGramsPerMole::new(zero),
        Err(NumericError::NonPositiveAuxiliary)
    );
    assert_eq!(
        MolarMassGramsPerMole::new(negative_one),
        Err(NumericError::NonPositiveAuxiliary)
    );
    assert_eq!(
        value
            .convert(ConcentrationUnit::MolesPerLitre, None)
            .unwrap(),
        value
    );
}

#[test]
fn concentration_real48_stores_each_conversion_and_can_lose_a_round_trip_bit() {
    let factor = MolarMassGramsPerMole::new(Real48::from_bytes([0x81, 0, 0, 0, 0, 0x40])).unwrap();
    let original = Real48::from_bytes([0x81, 0x02, 0, 0, 0, 0x40]);
    let value = ConcentrationValue::new(original, ConcentrationUnit::MolesPerLitre);
    let mass = value
        .convert(ConcentrationUnit::GramsPerLitre, Some(factor))
        .unwrap();
    assert_eq!(mass.raw().bytes(), [0x82, 0x02, 0, 0, 0, 0x10]);
    assert_eq!(mass.unit(), ConcentrationUnit::GramsPerLitre);
    let back = mass
        .convert(ConcentrationUnit::MolesPerLitre, Some(factor))
        .unwrap();
    assert_eq!(back.raw().bytes(), [0x81, 0x03, 0, 0, 0, 0x40]);
    assert_ne!(back.raw(), original);
    assert_eq!(value.raw(), original);
}

#[test]
fn concentration_identity_keeps_noncanonical_zero_but_a_new_store_canonicalizes_it() {
    let original = Real48::from_bytes([0, 1, 2, 3, 4, 0xff]);
    let value = ConcentrationValue::new(original, ConcentrationUnit::MolesPerLitre);
    assert_eq!(
        value
            .convert(ConcentrationUnit::MolesPerLitre, None)
            .unwrap()
            .raw(),
        original
    );
    let one = MolarMassGramsPerMole::new(Real48::from_bytes([0x81, 0, 0, 0, 0, 0])).unwrap();
    assert_eq!(
        value
            .convert(ConcentrationUnit::GramsPerLitre, Some(one))
            .unwrap()
            .raw()
            .bytes(),
        [0; 6]
    );
}

#[test]
fn concentration_accepts_signed_numeric_values_and_preserves_input_on_overflow() {
    let negative_one = Real48::from_bytes([0x81, 0, 0, 0, 0, 0x80]);
    let one = MolarMassGramsPerMole::new(Real48::from_bytes([0x81, 0, 0, 0, 0, 0])).unwrap();
    let signed = ConcentrationValue::new(negative_one, ConcentrationUnit::MolesPerLitre);
    assert_eq!(
        signed
            .convert(ConcentrationUnit::GramsPerLitre, Some(one))
            .unwrap()
            .raw(),
        negative_one
    );
    let max = Real48::from_bytes([0xff, 0xff, 0xff, 0xff, 0xff, 0x7f]);
    let two = MolarMassGramsPerMole::new(Real48::from_bytes([0x82, 0, 0, 0, 0, 0])).unwrap();
    let source = ConcentrationValue::new(max, ConcentrationUnit::MolesPerLitre);
    assert_eq!(
        source.convert(ConcentrationUnit::GramsPerLitre, Some(two)),
        Err(NumericError::Overflow)
    );
    assert_eq!(source.raw(), max);
}
