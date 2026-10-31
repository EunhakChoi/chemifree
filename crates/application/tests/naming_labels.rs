use chemifree_application::naming_labels::{LabelError, decode_labels, verify_catalog_files};

// Invented shape records: labels and ranks have no chemical meaning.
const KIND: &str = include_str!("data/naming-labels-synthetic/kind.jsonl");
const SUBTYPE: &str = include_str!("data/naming-labels-synthetic/subtype.jsonl");

fn files<'a>(kind: &'a str, subtype: &'a str) -> [(&'static str, &'a [u8]); 2] {
    [
        ("normalized/kind-labels.jsonl", kind.as_bytes()),
        ("normalized/subtype-labels.jsonl", subtype.as_bytes()),
    ]
}

#[test]
fn contexts_preserve_order_repeated_spelling_and_exact_integer_keys() {
    let labels = decode_labels(&files(KIND, SUBTYPE)).unwrap();
    assert_eq!(labels.kind.len(), 2);
    assert_eq!(
        labels.kind[0].source_ordinal.to_string(),
        u64::MAX.to_string()
    );
    assert_eq!(labels.kind[0].code, u32::MAX);
    assert_eq!(labels.kind[0].label_bytes, b"X\0");
    assert_eq!(labels.kind[1].label_bytes, b"X\0");
    assert_eq!(
        labels.kind[1].source_ordinal.to_string(),
        (u64::MAX - 1).to_string()
    );
    assert_eq!(labels.kind[1].code, 7);
    assert_eq!(labels.subtype[0].label_bytes, b"A");
    let crlf_kind = KIND.replace('\n', "\r\n");
    let crlf_subtype = SUBTYPE.replace('\n', "\r\n");
    assert!(decode_labels(&files(&crlf_kind, &crlf_subtype)).is_ok());
}

#[test]
fn framing_ascii_witness_integer_form_and_closed_shape_are_checked() {
    let mismatch = SUBTYPE.replace("\"label_bytes_hex\":\"41\"", "\"label_bytes_hex\":\"42\"");
    assert_eq!(
        decode_labels(&files(KIND, &mismatch)).err(),
        Some(LabelError::Record)
    );
    let upper = SUBTYPE.replace("\"41\"", "\"4A\"");
    assert_eq!(
        decode_labels(&files(KIND, &upper)).err(),
        Some(LabelError::Record)
    );
    let decimal = SUBTYPE.replace("\"code_u32\":3", "\"code_u32\":3.00000000000000000001");
    assert_eq!(
        decode_labels(&files(KIND, &decimal)).err(),
        Some(LabelError::Record)
    );
    let extra = SUBTYPE.replace("\"code_u32\":3", "\"code_u32\":3,\"directive\":1");
    assert_eq!(
        decode_labels(&files(KIND, &extra)).err(),
        Some(LabelError::Record)
    );
    let invalid_digest = SUBTYPE.replace(
        "0000000000000000000000000000000000000000000000000000000000000000",
        "A000000000000000000000000000000000000000000000000000000000000000",
    );
    assert_eq!(
        decode_labels(&files(KIND, &invalid_digest)).err(),
        Some(LabelError::Record)
    );
    assert_eq!(
        decode_labels(&files(KIND.trim_end(), SUBTYPE)).err(),
        Some(LabelError::Framing)
    );
    let negative_zero = SUBTYPE.replace(
        "\"source_ordinal_0based\":0",
        "\"source_ordinal_0based\":-0",
    );
    assert_eq!(
        decode_labels(&files(KIND, &negative_zero)).unwrap().subtype[0]
            .source_ordinal
            .to_string(),
        "0"
    );
}

#[test]
fn both_contexts_are_required_and_profile_names_do_not_supply_a_trust_anchor() {
    let data = files(KIND, SUBTYPE);
    assert_eq!(decode_labels(&data[..1]).err(), Some(LabelError::FileSet));
    assert_eq!(
        decode_labels(&[data[0], data[0]]).err(),
        Some(LabelError::FileSet)
    );
    assert_eq!(
        verify_catalog_files(
            "legacy-naming-enum-labels",
            "sha256-set-5eafac8138576244941e5aec63ad04175e7dadd0f1c5f441fbac8ab044278134",
            b"synthetic manifest",
            b"synthetic registry",
            b"synthetic profile",
        ),
        Err(LabelError::CatalogMismatch)
    );
}
