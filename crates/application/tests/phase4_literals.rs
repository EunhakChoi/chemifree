use chemifree_application::phase4_literals;
use serde_json::{Value, json};

use phase4_literals::{InputError, Profile};

const METADATA_FILES: [&str; 2] = ["P4-MOLNET-METADATA-001", "P4-MOLNET-METADATA-002"];
const NUMERIC_FILES: [(&str, usize); 6] = [
    ("DATA-LITERALS-MOLNET-LOGS-normalized", 13),
    ("DATA-LITERALS-MOLNET-PKA-PHENOL-normalized", 6),
    ("DATA-LITERALS-MOLNET-PKA-BENZOIC-ACID-normalized", 6),
    ("DATA-LITERALS-MOLNET-PKA-ALIPHATIC-ACID-normalized", 6),
    ("DATA-LITERALS-MOLNET-PKA-ALIPHATIC-AMINE-normalized", 6),
    ("DATA-LITERALS-MOLNET-PKA-ALIPHATIC-ALCOHOL-normalized", 5),
];
const REAL48_FILES: [(&str, usize); 3] = [
    ("DATA-LITERALS-CHEMPROP-BROTO-normalized", 243),
    ("DATA-LITERALS-CHEMPROP-CRIPPEN-VISWANADHAN-normalized", 304),
    ("DATA-LITERALS-CHEMPROP-THERMODYNAMIC-normalized", 87),
];

fn numeric_files() -> Vec<(&'static str, Vec<u8>)> {
    NUMERIC_FILES
        .iter()
        .map(|(id, count)| {
            let rows = (0..*count)
                .map(|index| {
                    json!({
                        "preferred_data_va": "inert-fixture",
                        "source_file_offset": index,
                        "source_section": "fixture",
                        "ieee754_f64_le_hex": "0000000000000000",
                        "value": 0,
                        "semantic_role": "fixture-only"
                    })
                    .to_string()
                })
                .collect::<Vec<_>>()
                .join("\n");
            (*id, format!("{rows}\n").into_bytes())
        })
        .collect()
}

fn real48_files() -> Vec<(&'static str, Vec<u8>)> {
    REAL48_FILES
        .iter()
        .map(|(id, count)| {
            let thermodynamic = *id == REAL48_FILES[2].0;
            let rows = (0..*count)
                .map(|index| {
                    let mut row = json!({
                        "storage_index": index,
                        "key_hex": "e9", "key_latin1": "é",
                        "real48_columns": [{
                            "column_byte_offset": 0,
                            "raw_real48_le_hex": "000000000000",
                            "exact_rational_numerator": "0",
                            "exact_rational_denominator": "1"
                        }]
                    });
                    if thermodynamic {
                        row["secondary_key_latin1"] = "fixture".into();
                    } else {
                        row["aux_word"] = 0.into();
                        row["flag_byte"] = 0.into();
                    }
                    row.to_string()
                })
                .collect::<Vec<_>>()
                .join("\n");
            (*id, format!("{rows}\n").into_bytes())
        })
        .collect()
}

fn refs<'a>(files: &'a [(&'static str, Vec<u8>)]) -> Vec<(&'a str, &'a [u8])> {
    files
        .iter()
        .map(|(id, bytes)| (*id, bytes.as_slice()))
        .collect()
}

fn replace_first_row(files: &mut [(&'static str, Vec<u8>)], mutate: impl FnOnce(&mut Value)) {
    let text = std::str::from_utf8(&files[0].1).unwrap();
    let (first, rest) = text.split_once('\n').unwrap();
    let mut row: Value = serde_json::from_str(first).unwrap();
    mutate(&mut row);
    files[0].1 = format!("{row}\n{rest}").into_bytes();
}

#[test]
fn metadata_is_byte_preserving_and_never_interpreted() {
    let files = [
        (METADATA_FILES[1], b"\xfffixture".as_slice()),
        (METADATA_FILES[0], b"command=do-not-run".as_slice()),
    ];
    let phase4_literals::Records::Metadata(records) =
        phase4_literals::decode_dataset(Profile::Metadata, &files).unwrap()
    else {
        panic!("wrong profile")
    };
    assert_eq!(records[0].id, METADATA_FILES[0]);
    assert_eq!(records[0].bytes, b"command=do-not-run");
    assert_eq!(records[1].bytes, b"\xfffixture");
}

#[test]
fn catalog_verification_refuses_changed_identities_and_metadata_bytes() {
    for (profile, dataset_id, dataset_version) in [
        (
            Profile::Metadata,
            "legacy-molnet-metadata",
            "sha256-set-28cd9dc5852838a9abdb8a385870e7dcfab468f1ab3338fa4f16387dad2bf437",
        ),
        (
            Profile::Numeric,
            "molnet-supporting-numeric-literals",
            "sha256-set-6472f01e148c5db2d03fc2f62d941b2a61c6beec9d5829fcc8943d4633b78ca1",
        ),
        (
            Profile::ChempropTables,
            "chemprop-typed-parameter-tables",
            "sha256-set-8c55c6a8c52c13bbb945a9f60de076d9a8c3c21bd0c0ecedadc3bd19c9cfce2f",
        ),
    ] {
        assert_eq!(
            phase4_literals::verify_catalog_files(
                profile,
                dataset_id,
                dataset_version,
                b"independent manifest",
                b"independent registry",
                b"independent profile"
            ),
            Err(InputError::CatalogMismatch)
        );
        assert_eq!(
            phase4_literals::verify_catalog_files(
                profile,
                "different",
                dataset_version,
                b"",
                b"",
                b""
            ),
            Err(InputError::CatalogMismatch)
        );
    }
}

#[test]
fn numeric_files_are_selected_by_id_and_keep_bits_and_parsed_value_separate() {
    let mut files = numeric_files();
    replace_first_row(&mut files, |row| {
        row["ieee754_f64_le_hex"] = "0000000000000080".into();
        row["value"] = 0.into();
    });
    files.reverse();
    let phase4_literals::Records::Numeric(tables) =
        phase4_literals::decode_dataset(Profile::Numeric, &refs(&files)).unwrap()
    else {
        panic!("wrong profile")
    };
    assert_eq!(tables.len(), 6);
    assert_eq!(tables[0].id, NUMERIC_FILES[0].0);
    assert_eq!(tables[0].rows[0].bits, (-0.0_f64).to_bits());
    assert_eq!(tables[0].rows[0].value.as_i64(), Some(0));
    assert_eq!(tables[0].rows[0].semantic_role, "fixture-only");
}

#[test]
fn each_numeric_file_requires_its_declared_number_of_records() {
    for index in 0..NUMERIC_FILES.len() {
        let mut files = numeric_files();
        files[index].1.clear();
        assert_eq!(
            phase4_literals::decode_dataset(Profile::Numeric, &refs(&files)).err(),
            Some(InputError::RecordCount)
        );
    }
}

#[test]
fn duplicate_unknown_or_missing_file_ids_are_rejected() {
    let files = numeric_files();
    let mut selected = refs(&files);
    selected[1].0 = selected[0].0;
    assert_eq!(
        phase4_literals::decode_dataset(Profile::Numeric, &selected).err(),
        Some(InputError::FileSet)
    );
    selected[1].0 = "not-a-catalog-id";
    assert_eq!(
        phase4_literals::decode_dataset(Profile::Numeric, &selected).err(),
        Some(InputError::FileSet)
    );
    selected.pop();
    assert_eq!(
        phase4_literals::decode_dataset(Profile::Numeric, &selected).err(),
        Some(InputError::FileSet)
    );
}

#[test]
fn numeric_witnesses_must_be_finite_and_agree_with_the_value() {
    for (hex, value) in [
        ("000000000000f03f", json!(0)),
        ("000000000000f07f", json!(0)),
        ("00", json!(0)),
    ] {
        let mut files = numeric_files();
        replace_first_row(&mut files, |row| {
            row["ieee754_f64_le_hex"] = hex.into();
            row["value"] = value;
        });
        assert_eq!(
            phase4_literals::decode_dataset(Profile::Numeric, &refs(&files)).err(),
            Some(InputError::Record)
        );
    }
}

#[test]
fn numeric_schema_is_closed_and_rejects_negative_source_offsets() {
    for mutation in 0..3 {
        let mut files = numeric_files();
        replace_first_row(&mut files, |row| match mutation {
            0 => {
                row.as_object_mut().unwrap().remove("semantic_role");
            }
            1 => {
                row["unrecognized"] = true.into();
            }
            _ => {
                row["source_file_offset"] = (-1).into();
            }
        });
        assert_eq!(
            phase4_literals::decode_dataset(Profile::Numeric, &refs(&files)).err(),
            Some(InputError::Record)
        );
    }
}

#[test]
fn phase4_framing_accepts_empty_lines_crlf_and_uppercase_hex() {
    let mut files = numeric_files();
    replace_first_row(&mut files, |row| {
        row["ieee754_f64_le_hex"] = "000000000000F03F".into();
        row["value"] = 1.into();
    });
    files[0].1 = format!(
        "\r\n{}\r\n",
        String::from_utf8(files[0].1.clone())
            .unwrap()
            .replace('\n', "\r\n")
    )
    .into_bytes();
    assert!(phase4_literals::decode_dataset(Profile::Numeric, &refs(&files)).is_ok());
}

#[test]
fn real48_rows_keep_exact_rationals_raw_bytes_and_distinct_table_metadata() {
    let files = real48_files();
    let phase4_literals::Records::ChempropTables(tables) =
        phase4_literals::decode_dataset(Profile::ChempropTables, &refs(&files)).unwrap()
    else {
        panic!("wrong profile")
    };
    assert_eq!(tables.len(), 3);
    assert_eq!(tables[0].rows.len(), 243);
    assert_eq!(tables[2].rows.len(), 87);
    let first = &tables[0].rows[0];
    assert_eq!(first.key.bytes, [0xe9]);
    assert_eq!(first.key.text, "é");
    assert_eq!(first.columns[0].raw, [0; 6]);
    assert_eq!(first.columns[0].numerator.to_string(), "0");
    assert_eq!(first.columns[0].denominator.to_string(), "1");
    assert!(matches!(
        first.context,
        phase4_literals::Real48Context::Typed { .. }
    ));
    assert!(matches!(
        tables[2].rows[0].context,
        phase4_literals::Real48Context::Thermodynamic { .. }
    ));
}

#[test]
fn chemprop_hex_allows_uppercase_without_changing_its_bytes_or_latin1_text() {
    let mut files = real48_files();
    replace_first_row(&mut files, |row| {
        row["key_hex"] = "E9".into();
        row["real48_columns"][0]["raw_real48_le_hex"] = "000A00000000".into();
    });
    let phase4_literals::Records::ChempropTables(tables) =
        phase4_literals::decode_dataset(Profile::ChempropTables, &refs(&files)).unwrap()
    else {
        panic!("wrong profile")
    };
    assert_eq!(tables[0].rows[0].key.bytes, [0xe9]);
    assert_eq!(tables[0].rows[0].key.text, "é");
    assert_eq!(tables[0].rows[0].columns[0].raw, [0, 10, 0, 0, 0, 0]);
}

#[test]
fn real48_records_reject_bad_rationals_raw_width_and_byte_text_pairs() {
    for mutation in 0..4 {
        let mut files = real48_files();
        replace_first_row(&mut files, |row| match mutation {
            0 => row["real48_columns"][0]["exact_rational_denominator"] = "0".into(),
            1 => row["real48_columns"][0]["exact_rational_numerator"] = "not-an-integer".into(),
            2 => row["real48_columns"][0]["raw_real48_le_hex"] = "00".into(),
            _ => row["key_latin1"] = "different".into(),
        });
        assert_eq!(
            phase4_literals::decode_dataset(Profile::ChempropTables, &refs(&files)).err(),
            Some(InputError::Record)
        );
    }
}

#[test]
fn real48_rationals_keep_large_integer_denominators_without_float_coercion() {
    let mut files = real48_files();
    let denominator = format!("1{}", "0".repeat(600));
    replace_first_row(&mut files, |row| {
        row["real48_columns"][0]["exact_rational_denominator"] = denominator.clone().into();
    });
    let phase4_literals::Records::ChempropTables(tables) =
        phase4_literals::decode_dataset(Profile::ChempropTables, &refs(&files)).unwrap()
    else {
        panic!("wrong profile")
    };
    assert_eq!(
        tables[0].rows[0].columns[0].denominator.to_string(),
        denominator
    );

    replace_first_row(&mut files, |row| {
        row["real48_columns"][0]["exact_rational_denominator"] = "1".repeat(4097).into();
    });
    assert_eq!(
        phase4_literals::decode_dataset(Profile::ChempropTables, &refs(&files)).err(),
        Some(InputError::Record)
    );
}

#[test]
fn real48_tables_require_each_catalog_file_and_its_declared_row_count() {
    let mut files = real48_files();
    files[1].1.clear();
    assert_eq!(
        phase4_literals::decode_dataset(Profile::ChempropTables, &refs(&files)).err(),
        Some(InputError::RecordCount)
    );
    files.pop();
    assert_eq!(
        phase4_literals::decode_dataset(Profile::ChempropTables, &refs(&files)).err(),
        Some(InputError::FileSet)
    );
}
