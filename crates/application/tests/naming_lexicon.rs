use chemifree_application::naming_lexicon::{LexiconError, decode_dataset, verify_catalog_files};

const LEXICON: &str = include_str!("data/naming-lexicon-synthetic/lexicon.jsonl");
const PRIMARY: &str = include_str!("data/naming-lexicon-synthetic/primary.jsonl");
const SECONDARY: &str = include_str!("data/naming-lexicon-synthetic/secondary.jsonl");

fn files<'a>(
    lexicon: &'a str,
    primary: &'a str,
    secondary: &'a str,
) -> [(&'static str, &'a [u8]); 3] {
    [
        ("normalized/lexicon-entries.jsonl", lexicon.as_bytes()),
        ("normalized/text-map-primary.jsonl", primary.as_bytes()),
        ("normalized/text-map-secondary.jsonl", secondary.as_bytes()),
    ]
}

#[test]
fn typed_records_preserve_source_order_byte_pairs_and_separate_contexts() {
    let rows = decode_dataset(&files(LEXICON, PRIMARY, SECONDARY)).unwrap();
    assert_eq!(rows.entries.len(), 2);
    assert_eq!(rows.entries[0].token.bytes, [0xe9]);
    assert_eq!(rows.entries[0].token.text, "é");
    assert_eq!(rows.entries[1].token.text, "é");
    assert_eq!(rows.entries[0].signed_scalar, -32769);
    assert_eq!(rows.entries[0].graph.stereo[0].source_code, 128);
    assert_eq!(rows.entries[0].graph.valence_override[0].signed_value, -255);
    assert_eq!(
        rows.entries[0].graph.valence_override[0].explicit_valence(),
        Some(0)
    );
    assert_eq!(rows.primary[0].partition.to_string(), "0");
    assert_eq!(rows.primary[1].partition.to_string(), "1");
    assert!(rows.primary[1].divider_marker);
    assert!(rows.secondary[0].replacement.bytes.is_empty());
}

#[test]
fn every_graph_reference_is_checked_against_its_own_array() {
    let cases = [
        LEXICON.replacen(
            "\"atom_index_0based\":0,\"value_u8\":2",
            "\"atom_index_0based\":2,\"value_u8\":2",
            1,
        ),
        LEXICON.replacen(
            "\"atom_index_0based\":0,\"signed_value\":-1",
            "\"atom_index_0based\":2,\"signed_value\":-1",
            1,
        ),
        LEXICON.replacen(
            "\"atom_index_0based\":1,\"source_code_u8\":128",
            "\"atom_index_0based\":2,\"source_code_u8\":128",
            1,
        ),
        LEXICON.replacen(
            "\"atom_index_0based\":1,\"signed_value\":-255",
            "\"atom_index_0based\":2,\"signed_value\":-255",
            1,
        ),
        LEXICON.replacen("\"end_atom_0based\":1", "\"end_atom_0based\":2", 1),
        LEXICON.replacen("\"bond_index_0based\":0", "\"bond_index_0based\":1", 1),
    ];
    for invalid in cases {
        assert_eq!(
            decode_dataset(&files(&invalid, PRIMARY, SECONDARY)).err(),
            Some(LexiconError::Inconsistent)
        );
    }
}

#[test]
fn record_shape_framing_and_latin1_pairs_are_not_interchangeable() {
    let wrong_token = LEXICON.replacen("\"token_hex\":\"e9\"", "\"token_hex\":\"ea\"", 1);
    assert_eq!(
        decode_dataset(&files(&wrong_token, PRIMARY, SECONDARY)).err(),
        Some(LexiconError::Record)
    );
    let wrong_map = PRIMARY.replacen(
        "\"replacement_hex\":\"78\"",
        "\"replacement_hex\":\"79\"",
        1,
    );
    assert_eq!(
        decode_dataset(&files(LEXICON, &wrong_map, SECONDARY)).err(),
        Some(LexiconError::Record)
    );
    let overflow = LEXICON.replacen(
        "\"source_file_offset\":0",
        "\"source_file_offset\":18446744073709551615",
        1,
    );
    let data = decode_dataset(&files(&overflow, PRIMARY, SECONDARY)).unwrap();
    assert_eq!(
        data.entries[0].source.file_offset.to_string(),
        u64::MAX.to_string()
    );
    let unknown = PRIMARY.replacen("\"partition\":0", "\"partition\":0,\"extra\":0", 1);
    assert_eq!(
        decode_dataset(&files(LEXICON, &unknown, SECONDARY)).err(),
        Some(LexiconError::Record)
    );
    assert_eq!(
        decode_dataset(&files(LEXICON.trim_end(), PRIMARY, SECONDARY)).err(),
        Some(LexiconError::Framing)
    );
    assert_eq!(
        decode_dataset(&files(LEXICON, &format!("\n{PRIMARY}"), SECONDARY)).err(),
        Some(LexiconError::Framing)
    );
}

#[test]
fn optional_raw_bounds_and_complete_file_set_are_checked() {
    let required = files(LEXICON, PRIMARY, SECONDARY);
    assert_eq!(
        decode_dataset(&required[..2]).err(),
        Some(LexiconError::FileSet)
    );
    let mut selected = required.to_vec();
    selected.push(("raw/NameToStructure.dat", b"0123456789"));
    assert!(decode_dataset(&selected).is_ok());
    selected[3].1 = b"012345678";
    assert_eq!(
        decode_dataset(&selected).err(),
        Some(LexiconError::Inconsistent)
    );
    selected[3].0 = "raw/unrecognized.dat";
    assert_eq!(decode_dataset(&selected).err(), Some(LexiconError::FileSet));
}

#[test]
fn catalog_identity_requires_verified_metadata_bytes() {
    assert_eq!(
        verify_catalog_files(
            "legacy-naming-lexicon-and-templates",
            "sha256-set-384790510be0a0b8c53fdf2b4839bde2e18bf5cca7cb375d88c0b2a3015547e7",
            b"manifest",
            b"registry",
            b"profile"
        ),
        Err(LexiconError::CatalogMismatch)
    );
}

#[test]
fn producer_ordinals_are_unique_within_each_independent_table() {
    let lexicon = LEXICON.replacen(
        "\"entry_ordinal_0based\":1",
        "\"entry_ordinal_0based\":0",
        1,
    );
    let primary = PRIMARY.replacen("\"pair_ordinal_0based\":1", "\"pair_ordinal_0based\":0", 1);
    let secondary = format!("{SECONDARY}{SECONDARY}");
    let results = [
        decode_dataset(&files(&lexicon, PRIMARY, SECONDARY)).err(),
        decode_dataset(&files(LEXICON, &primary, SECONDARY)).err(),
        decode_dataset(&files(LEXICON, PRIMARY, &secondary)).err(),
    ];
    assert_eq!(results, [Some(LexiconError::Inconsistent); 3]);
}
