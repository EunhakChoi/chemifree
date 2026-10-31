use chemifree_application::naming_patterns::{
    PatternError, PatternRows, Profile, decode_dataset, decode_query, decode_retained,
    verify_catalog_files,
};

const QUERY: &str = include_str!("data/naming-patterns-synthetic/query.jsonl");
const RETAINED: &str = include_str!("data/naming-patterns-synthetic/retained.jsonl");

#[test]
fn query_records_preserve_null_empty_and_unresolved_predicate_requirements() {
    let rows = decode_query(QUERY.as_bytes()).unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows[0].smiles.is_none());
    assert_eq!(
        rows[0].source_graph_placeholder.as_deref(),
        Some("fixture-placeholder")
    );
    assert_eq!(rows[0].annotations[0].source_index, None);
    assert_eq!(
        rows[1].annotations[0]
            .source_index
            .as_ref()
            .map(ToString::to_string),
        Some("0".into())
    );
    assert_eq!(rows[0].required_predicate_ids, ["fixture-predicate"]);
    assert!(rows[0].annotations[0].requires_independent_named_predicate);
    assert_eq!(rows[1].smiles.as_ref().unwrap().bytes, b"");
    assert_eq!(rows[1].smiles.as_ref().unwrap().text, "");
}

#[test]
fn retained_records_preserve_flags_and_nullable_name_pairs_without_inference() {
    let rows = decode_retained(RETAINED.as_bytes()).unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows[0].allow_substituents);
    assert!(!rows[0].interpret_stereochemistry);
    assert!(!rows[0].is_full_name);
    assert!(rows[0].substituent_name.is_none());
    assert!(rows[0].annotations[0].value.is_none());
    assert_eq!(rows[1].substituent_name.as_ref().unwrap().text, "");
    assert!(rows[1].has_explicit_substituent_name);
    assert!(rows[1].interpret_stereochemistry);
    assert!(rows[1].is_full_name);
}

#[test]
fn nullable_fields_are_required_and_byte_text_pairs_must_match() {
    let missing = QUERY.replacen("\"smiles_ascii\":null,", "", 1);
    assert_eq!(
        decode_query(missing.as_bytes()).err(),
        Some(PatternError::Record)
    );
    let missing_index = QUERY.replacen("\"source_index_0based\":null,", "", 1);
    assert_eq!(
        decode_query(missing_index.as_bytes()).err(),
        Some(PatternError::Record)
    );
    let half_null = QUERY.replacen("\"smiles_bytes_hex\":null", "\"smiles_bytes_hex\":\"\"", 1);
    assert_eq!(
        decode_query(half_null.as_bytes()).err(),
        Some(PatternError::Record)
    );
    let wrong_raw = QUERY.replacen(
        "\"raw_definition_bytes_hex\":\"51\"",
        "\"raw_definition_bytes_hex\":\"52\"",
        1,
    );
    assert_eq!(
        decode_query(wrong_raw.as_bytes()).err(),
        Some(PatternError::Record)
    );
    let wrong_name = RETAINED.replacen(
        "\"retained_name_bytes_hex\":\"54\"",
        "\"retained_name_bytes_hex\":\"55\"",
        1,
    );
    assert_eq!(
        decode_retained(wrong_name.as_bytes()).err(),
        Some(PatternError::Record)
    );
}

#[test]
fn framing_closed_fields_and_inert_source_bounds_are_checked() {
    assert_eq!(
        decode_query(QUERY.trim_end().as_bytes()).err(),
        Some(PatternError::Framing)
    );
    assert_eq!(
        decode_query(format!("\n{QUERY}").as_bytes()).err(),
        Some(PatternError::Framing)
    );
    assert!(decode_query(QUERY.replace('\n', "\r\n").as_bytes()).is_ok());
    let extra = QUERY.replacen(
        "\"source_bytes\":8",
        "\"source_bytes\":8,\"instruction\":1",
        1,
    );
    assert_eq!(
        decode_query(extra.as_bytes()).err(),
        Some(PatternError::Record)
    );
    let overflow = RETAINED.replacen(
        "\"source_file_offset\":1",
        "\"source_file_offset\":18446744073709551615",
        1,
    );
    let rows = decode_retained(overflow.as_bytes()).unwrap();
    assert_eq!(rows[0].source.offset.to_string(), u64::MAX.to_string());
    assert_eq!(rows[0].source.bytes.to_string(), "7");
    let negative = RETAINED.replacen("\"source_file_offset\":1", "\"source_file_offset\":-1", 1);
    assert_eq!(
        decode_retained(negative.as_bytes()).err(),
        Some(PatternError::Record)
    );
    let digest = QUERY.replacen(&"0".repeat(64), &"F".repeat(64), 1);
    assert_eq!(
        decode_query(digest.as_bytes()).err(),
        Some(PatternError::Record)
    );
}

#[test]
fn catalog_names_alone_cannot_authorize_either_profile() {
    assert!(matches!(
        decode_dataset(
            Profile::Query,
            &[("normalized/query-graph-patterns.jsonl", QUERY.as_bytes())]
        ),
        Ok(PatternRows::Query(_))
    ));
    assert_eq!(
        decode_dataset(
            Profile::Query,
            &[(
                "normalized/retained-name-patterns.jsonl",
                RETAINED.as_bytes()
            )]
        )
        .err(),
        Some(PatternError::FileSet)
    );
    assert_eq!(
        decode_dataset(Profile::Retained, &[]).err(),
        Some(PatternError::FileSet)
    );
    for (profile, id, version) in [
        (
            Profile::Query,
            "legacy-naming-query-graph-patterns",
            "sha256-set-fa570fa71c8e49035db6427ae5f5cdc68937ea4ab5ca2e82d64561c18d2d95b4",
        ),
        (
            Profile::Retained,
            "legacy-naming-retained-graph-patterns",
            "sha256-set-cfb579548ea719a1b218d72f5614e304cf7274f5c0934512c8af669754807cd8",
        ),
    ] {
        assert_eq!(
            verify_catalog_files(
                profile,
                id,
                version,
                b"fixture manifest",
                b"fixture registry",
                b"fixture profile"
            ),
            Err(PatternError::CatalogMismatch)
        );
    }
}

#[test]
fn source_ordinals_are_unique_in_each_pattern_stream() {
    let duplicate_query = QUERY.replacen(
        "\"entry_ordinal_0based\":1",
        "\"entry_ordinal_0based\":0",
        1,
    );
    assert_eq!(
        decode_query(duplicate_query.as_bytes()).err(),
        Some(PatternError::Record)
    );
    let duplicate_retained = RETAINED.replacen(
        "\"entry_ordinal_0based\":1",
        "\"entry_ordinal_0based\":0",
        1,
    );
    assert_eq!(
        decode_retained(duplicate_retained.as_bytes()).err(),
        Some(PatternError::Record)
    );
}

#[test]
fn predicate_declarations_are_preserved_without_inventing_an_id_mapping() {
    let input = QUERY
        .replacen(
            "\"required_independent_predicate_ids\":[\"fixture-predicate\"]",
            "\"required_independent_predicate_ids\":[]",
            1,
        )
        .replacen(
            "\"named_match_or_generation_predicate_id\":\"fixture-predicate\"",
            "\"named_match_or_generation_predicate_id\":\"another\"",
            1,
        );
    let rows = decode_query(input.as_bytes()).unwrap();
    assert!(rows[0].required_predicate_ids.is_empty());
    assert_eq!(
        rows[0].named_match_or_generation_predicate_id.as_deref(),
        Some("another")
    );
    assert_eq!(
        rows[0].annotations[0].property_or_predicate_id,
        "fixture-predicate"
    );
    assert!(rows[0].annotations[0].requires_independent_named_predicate);
}
