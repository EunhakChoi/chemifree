use chemifree_application::{MAX_DATA_FILE_BYTES, json, phase6_profiles};

#[path = "../src/prepared_xlogp.rs"]
mod prepared_xlogp;

use prepared_xlogp::{PreparedError, decode_dataset, decode_weights, verify_catalog_files};

// Invented values exercise storage shape only and do not represent chemical parameters.
fn synthetic_files() -> (Vec<u8>, String) {
    let examples = [
        ("0.5", "000000000000e03f", [0, 0, 0, 0, 0, 0, 0xe0, 0x3f]),
        ("-0.0", "0000000000000080", [0, 0, 0, 0, 0, 0, 0, 0x80]),
        ("-2.25", "00000000000002c0", [0, 0, 0, 0, 0, 0, 0x02, 0xc0]),
        (
            "2.2250738585072014e-308",
            "0000000000001000",
            [0, 0, 0, 0, 0, 0, 0x10, 0],
        ),
    ];
    let mut raw = Vec::new();
    let mut normalized = String::new();
    for index in 0..100 {
        let (value, hex, bytes) = examples[index % examples.len()];
        raw.extend_from_slice(&bytes);
        normalized.push_str(&format!(
            "{{\"storage_index_0based\":{index},\"bin_family\":\"fixture\",\"family_index_0based\":{index},\"ieee754_f64_le_hex\":\"{hex}\",\"value\":{value}}}\n"
        ));
    }
    (raw, normalized)
}

#[test]
fn paired_raw_and_normalized_rows_preserve_context_and_exact_binary64_bits() {
    let (raw, normalized) = synthetic_files();
    let data = decode_weights(&raw, normalized.as_bytes()).unwrap();
    assert_eq!(data.rows.len(), 100);
    assert_eq!(data.rows[0].bits, 0x3fe0000000000000);
    assert_eq!(data.rows[1].bits, 0x8000000000000000);
    assert_eq!(data.rows[2].bits, 0xc002000000000000);
    assert_eq!(data.rows[3].bits, 0x0010000000000000);
    assert_eq!(data.rows[99].family_index, 99);
    assert_eq!(data.rows[0].bin_family, "fixture");
}

#[test]
fn parsed_value_and_original_bits_remain_separate_witnesses() {
    let (raw, normalized) = synthetic_files();
    let normalized = normalized.replacen("\"value\":-0.0", "\"value\":0.0", 1);
    let data = decode_weights(&raw, normalized.as_bytes()).unwrap();
    assert_eq!(data.rows[1].bits, 0x8000000000000000);
    assert_eq!(data.rows[1].value.as_f64().unwrap().to_bits(), 0);
}

#[test]
fn hashes_are_not_a_substitute_for_matching_raw_and_numeric_witnesses() {
    let (mut raw, normalized) = synthetic_files();
    raw[0] ^= 1;
    assert_eq!(
        decode_weights(&raw, normalized.as_bytes()).err(),
        Some(PreparedError::Inconsistent)
    );
    let (raw, normalized) = synthetic_files();
    let wrong_number = normalized.replacen("\"value\":0.5", "\"value\":1.0", 1);
    assert_eq!(
        decode_weights(&raw, wrong_number.as_bytes()).err(),
        Some(PreparedError::Record)
    );
    let nonfinite = normalized.replacen("000000000000e03f", "000000000000f07f", 1);
    assert_eq!(
        decode_weights(&raw, nonfinite.as_bytes()).err(),
        Some(PreparedError::Record)
    );
}

#[test]
fn record_count_closed_shape_and_storage_indices_are_checked() {
    let (raw, normalized) = synthetic_files();
    let short_raw = &raw[..raw.len() - 1];
    assert_eq!(
        decode_weights(short_raw, normalized.as_bytes()).err(),
        Some(PreparedError::Inconsistent)
    );
    let missing = normalized.lines().take(99).collect::<Vec<_>>().join("\n");
    assert_eq!(
        decode_weights(&raw, missing.as_bytes()).err(),
        Some(PreparedError::Inconsistent)
    );
    let duplicate = normalized.replacen(
        "\"storage_index_0based\":99",
        "\"storage_index_0based\":0",
        1,
    );
    assert_eq!(
        decode_weights(&raw, duplicate.as_bytes()).err(),
        Some(PreparedError::Inconsistent)
    );
    let unknown = normalized.replacen(
        "\"bin_family\":\"fixture\"",
        "\"bin_family\":\"fixture\",\"instruction\":1",
        1,
    );
    assert_eq!(
        decode_weights(&raw, unknown.as_bytes()).err(),
        Some(PreparedError::Record)
    );
    let duplicate_key = normalized.replacen(
        "\"bin_family\":\"fixture\"",
        "\"bin_family\":\"fixture\",\"bin_family\":\"other\"",
        1,
    );
    assert_eq!(
        decode_weights(&raw, duplicate_key.as_bytes()).err(),
        Some(PreparedError::Record)
    );
}

#[test]
fn phase4_allows_crlf_blank_physical_lines_and_uppercase_hex() {
    let (raw, normalized) = synthetic_files();
    let input = format!(
        "\r\n{}\r\n",
        normalized.replace('\n', "\r\n").replace("e03f", "E03F")
    );
    assert!(decode_weights(&raw, input.as_bytes()).is_ok());
    assert!(decode_weights(&raw, normalized.trim_end_matches('\n').as_bytes()).is_ok());
    let whitespace_only = format!(" \n{normalized}");
    assert_eq!(
        decode_weights(&raw, whitespace_only.as_bytes()).err(),
        Some(PreparedError::Record)
    );
    let bom = format!("\u{feff}{normalized}");
    assert_eq!(
        decode_weights(&raw, bom.as_bytes()).err(),
        Some(PreparedError::Framing)
    );
}

#[test]
fn explicit_file_ids_and_catalog_bytes_bind_the_complete_dataset() {
    let (raw, normalized) = synthetic_files();
    let files: [(&str, &[u8]); 2] = [
        ("DATA-RANGE-MOLNET-XLOGP-100-raw_data", &raw),
        (
            "DATA-RANGE-MOLNET-XLOGP-100-normalized",
            normalized.as_bytes(),
        ),
    ];
    assert!(decode_dataset(&files).is_ok());
    assert_eq!(
        decode_dataset(&files[..1]).err(),
        Some(PreparedError::FileSet)
    );
    assert_eq!(
        decode_dataset(&[files[0], files[0]]).err(),
        Some(PreparedError::FileSet)
    );
    assert_eq!(
        verify_catalog_files(
            "molnet-xlogp-100-f64-data",
            "sha256-set-164fb893fc80d7856819a151e9ef53c4ec44cdc18c368a2b2320de2851f42565",
            b"synthetic manifest",
            b"synthetic registry",
            b"synthetic format catalog",
        ),
        Err(PreparedError::CatalogMismatch)
    );
}
