use chemifree_application::naming_labels::decode_labels;
use chemifree_application::naming_patterns::{decode_query, decode_retained};

const KIND: &str = include_str!("data/naming-labels-synthetic/kind.jsonl");
const SUBTYPE: &str = include_str!("data/naming-labels-synthetic/subtype.jsonl");
const QUERY: &str = include_str!("data/naming-patterns-synthetic/query.jsonl");
const RETAINED: &str = include_str!("data/naming-patterns-synthetic/retained.jsonl");

fn label_files(subtype: &str) -> [(&str, &[u8]); 2] {
    [
        ("normalized/kind-labels.jsonl", KIND.as_bytes()),
        ("normalized/subtype-labels.jsonl", subtype.as_bytes()),
    ]
}

#[test]
fn label_schema_accepts_exact_decimal_integer_fields() {
    let input = SUBTYPE
        .replace("\"code_u32\":3", "\"code_u32\":3e0")
        .replace(
            "\"source_row_file_offset\":0",
            "\"source_row_file_offset\":0.0",
        );
    let labels = decode_labels(&label_files(&input)).unwrap();
    assert_eq!(labels.subtype[0].code, 3);
    assert_eq!(labels.subtype[0].source_row_file_offset.to_string(), "0");
}

#[test]
fn label_provenance_preserves_indices_larger_than_u64() {
    let large = "9".repeat(400);
    let input = SUBTYPE.replace(
        "\"source_ordinal_0based\":0",
        &format!("\"source_ordinal_0based\":{large}"),
    );
    let labels = decode_labels(&label_files(&input)).unwrap();
    assert_eq!(labels.subtype[0].source_ordinal.to_string(), large);
}

#[test]
fn label_provenance_enforces_the_documented_component_limit() {
    let within = "9".repeat(4096);
    let input = SUBTYPE.replace(
        "\"source_ordinal_0based\":0",
        &format!("\"source_ordinal_0based\":{within}"),
    );
    assert_eq!(
        decode_labels(&label_files(&input)).unwrap().subtype[0]
            .source_ordinal
            .to_string(),
        within
    );
    let excessive = "9".repeat(4097);
    let input = SUBTYPE.replace(
        "\"source_ordinal_0based\":0",
        &format!("\"source_ordinal_0based\":{excessive}"),
    );
    assert!(decode_labels(&label_files(&input)).is_err());
}

#[test]
fn labels_reject_nonintegers_duplicate_keys_and_invalid_numbers() {
    for replacement in [
        "\"code_u32\":3.00000000000000000001",
        "\"code_u32\":4294967296.0",
        "\"code_u32\":3,\"\\u0063ode_u32\":3",
        "\"code_u32\":3e+",
    ] {
        let input = SUBTYPE.replace("\"code_u32\":3", replacement);
        assert!(decode_labels(&label_files(&input)).is_err());
    }
}

#[test]
fn query_provenance_keeps_large_offsets_without_float_conversion() {
    let large = "9".repeat(400);
    let input = QUERY.replacen(
        "\"source_file_offset\":4",
        &format!("\"source_file_offset\":{large}"),
        1,
    );
    let rows = decode_query(input.as_bytes()).unwrap();
    assert_eq!(rows[0].source.offset.to_string(), large);
    assert_eq!(rows[0].source.bytes.to_string(), "8");
}

#[test]
fn pattern_schema_accepts_exact_integral_nullable_indices() {
    let query = QUERY.replace("\"source_index_0based\":0", "\"source_index_0based\":0e0");
    let rows = decode_query(query.as_bytes()).unwrap();
    assert_eq!(
        rows[1].annotations[0]
            .source_index
            .as_ref()
            .unwrap()
            .to_string(),
        "0"
    );
    let retained = RETAINED.replacen(
        "\"entry_ordinal_0based\":0",
        "\"entry_ordinal_0based\":0.0",
        1,
    );
    assert!(decode_retained(retained.as_bytes()).is_ok());
    let duplicate = QUERY.replacen(
        "\"entry_ordinal_0based\":1",
        "\"entry_ordinal_0based\":0.0",
        1,
    );
    assert!(decode_query(duplicate.as_bytes()).is_err());
}
