use chemifree_application::naming_lexicon::{LexiconError, decode_dataset as decode_lexicon};
use chemifree_application::nickname_data::{NicknameError, decode_dataset as decode_nicknames};
use sha2::{Digest, Sha256};

const LEXICON: &str = include_str!("data/naming-lexicon-synthetic/lexicon.jsonl");
const PRIMARY: &str = include_str!("data/naming-lexicon-synthetic/primary.jsonl");
const SECONDARY: &str = include_str!("data/naming-lexicon-synthetic/secondary.jsonl");
const GRAPHS: &str = include_str!("data/nickname-synthetic/graphs.jsonl");
const ALPHABET: &str = include_str!("data/nickname-synthetic/alphabet.jsonl");

fn lexicon_files<'a>(lexicon: &'a str, primary: &'a str) -> [(&'static str, &'a [u8]); 3] {
    [
        ("normalized/lexicon-entries.jsonl", lexicon.as_bytes()),
        ("normalized/text-map-primary.jsonl", primary.as_bytes()),
        ("normalized/text-map-secondary.jsonl", SECONDARY.as_bytes()),
    ]
}

fn nickname_files<'a>(graphs: &'a str, alphabet: &'a str) -> [(&'static str, &'a [u8]); 2] {
    [
        ("normalized/nickname-graphs.jsonl", graphs.as_bytes()),
        ("normalized/sequence-alphabets.jsonl", alphabet.as_bytes()),
    ]
}

#[test]
fn lexicon_decodes_exact_integer_values_in_nested_graph_fields() {
    let input = LEXICON
        .replacen("\"signed_scalar\":-32769", "\"signed_scalar\":-32769.0", 1)
        .replacen("\"kind_code_u8\":7", "\"kind_code_u8\":7e0", 1)
        .replacen("\"subtype_code_u8\":9", "\"subtype_code_u8\":9.0", 1)
        .replacen(
            "\"entry_ordinal_0based\":0",
            "\"entry_ordinal_0based\":0e0",
            1,
        )
        .replacen("\"source_file_offset\":0", "\"source_file_offset\":0.0", 1)
        .replacen("\"source_bytes\":5", "\"source_bytes\":5.0", 1)
        .replacen("\"value_u8\":3", "\"value_u8\":3.0", 1)
        .replacen(
            "\"atomic_number_codes_u8\":[42,77]",
            "\"atomic_number_codes_u8\":[42.0,77e0]",
            1,
        )
        .replacen(
            "\"atom_index_0based\":0,\"value_u8\":2",
            "\"atom_index_0based\":0.0,\"value_u8\":2e0",
            1,
        )
        .replacen(
            "\"atom_index_0based\":0,\"signed_value\":-1",
            "\"atom_index_0based\":0e0,\"signed_value\":-1.0",
            1,
        )
        .replacen(
            "\"atom_index_0based\":1,\"source_code_u8\":128",
            "\"atom_index_0based\":1.0,\"source_code_u8\":128.0",
            1,
        )
        .replacen(
            "\"begin_atom_0based\":0,\"end_atom_0based\":1,\"source_bond_code_u8\":3",
            "\"begin_atom_0based\":0.0,\"end_atom_0based\":1e0,\"source_bond_code_u8\":3.0",
            1,
        )
        .replacen(
            "\"bond_index_0based\":0,\"source_code_u8\":5",
            "\"bond_index_0based\":0e0,\"source_code_u8\":5.0",
            1,
        );
    let primary = PRIMARY
        .replacen(
            "\"pair_ordinal_0based\":0",
            "\"pair_ordinal_0based\":0.0",
            1,
        )
        .replacen("\"source_file_offset\":0", "\"source_file_offset\":0e0", 1)
        .replacen("\"source_bytes\":2", "\"source_bytes\":2.0", 1)
        .replacen("\"partition\":0", "\"partition\":0e0", 1);
    for expected in [
        "\"entry_ordinal_0based\":0e0",
        "\"source_file_offset\":0.0",
        "\"source_bytes\":5.0",
        "\"kind_code_u8\":7e0",
        "\"subtype_code_u8\":9.0",
        "\"signed_scalar\":-32769.0",
        "\"value_u8\":3.0",
        "\"atomic_number_codes_u8\":[42.0,77e0]",
        "\"atom_index_0based\":0.0,\"value_u8\":2e0",
        "\"atom_index_0based\":0e0,\"signed_value\":-1.0",
        "\"atom_index_0based\":1.0,\"source_code_u8\":128.0",
        "\"begin_atom_0based\":0.0,\"end_atom_0based\":1e0,\"source_bond_code_u8\":3.0",
        "\"bond_index_0based\":0e0,\"source_code_u8\":5.0",
    ] {
        assert!(
            input.contains(expected),
            "fixture replacement missing: {expected}"
        );
    }
    for expected in [
        "\"pair_ordinal_0based\":0.0",
        "\"source_file_offset\":0e0",
        "\"source_bytes\":2.0",
        "\"partition\":0e0",
    ] {
        assert!(
            primary.contains(expected),
            "fixture replacement missing: {expected}"
        );
    }
    let data = decode_lexicon(&lexicon_files(&input, &primary)).unwrap();
    assert_eq!(data.entries[0].signed_scalar, -32769);
    assert_eq!(data.entries[0].attributes[0].value, 3);
    assert_eq!(data.entries[0].graph.atomic_number_codes, [42, 77]);
    assert_eq!(data.entries[0].graph.charge[0].signed_value, -1);
    assert_eq!(data.entries[0].graph.stereo[0].source_code, 128);
    assert_eq!(data.entries[0].graph.bonds[0].end_atom, 1);
    let fraction = input.replacen("128.0", "128.00000000000000000001", 1);
    assert!(decode_lexicon(&lexicon_files(&fraction, &primary)).is_err());
    let bad_index = input.replacen(
        "\"atom_index_0based\":1.0,\"source_code_u8\":128.0",
        "\"atom_index_0based\":2.0,\"source_code_u8\":128.0",
        1,
    );
    assert_eq!(
        decode_lexicon(&lexicon_files(&bad_index, &primary)).err(),
        Some(LexiconError::Inconsistent)
    );
    let minimum = input.replacen("-32769.0", "-2147483648.0", 1);
    assert_eq!(
        decode_lexicon(&lexicon_files(&minimum, &primary))
            .unwrap()
            .entries[0]
            .signed_scalar,
        i32::MIN
    );
    let maximum = input.replacen("-32769.0", "2147483647.0", 1);
    assert_eq!(
        decode_lexicon(&lexicon_files(&maximum, &primary))
            .unwrap()
            .entries[0]
            .signed_scalar,
        i32::MAX
    );
    let overflow = input.replacen("-32769.0", "-2147483649.0", 1);
    assert!(decode_lexicon(&lexicon_files(&overflow, &primary)).is_err());
    let overflow = input.replacen("-32769.0", "2147483648.0", 1);
    assert!(decode_lexicon(&lexicon_files(&overflow, &primary)).is_err());
    let max_byte = input.replacen("\"kind_code_u8\":7e0", "\"kind_code_u8\":255.0", 1);
    assert_eq!(
        decode_lexicon(&lexicon_files(&max_byte, &primary))
            .unwrap()
            .entries[0]
            .kind,
        255
    );
    let overflow_byte = input.replacen("\"kind_code_u8\":7e0", "\"kind_code_u8\":256.0", 1);
    assert!(decode_lexicon(&lexicon_files(&overflow_byte, &primary)).is_err());
    let duplicate = input.replacen(
        "\"entry_ordinal_0based\":1",
        "\"entry_ordinal_0based\":0.0",
        1,
    );
    assert_eq!(
        decode_lexicon(&lexicon_files(&duplicate, &primary)).err(),
        Some(LexiconError::Inconsistent)
    );
}

#[test]
fn lexicon_provenance_is_exact_and_checked_when_raw_source_exists() {
    let large = "9".repeat(400);
    let lexicon = LEXICON
        .replacen(
            "\"source_file_offset\":0",
            &format!("\"source_file_offset\":{large}"),
            1,
        )
        .replacen(
            "\"entry_ordinal_0based\":0",
            &format!("\"entry_ordinal_0based\":{large}"),
            1,
        )
        .replacen(
            "\"source_bytes\":5",
            &format!("\"source_bytes\":{large}"),
            1,
        );
    let primary = PRIMARY.replacen("\"partition\":0", &format!("\"partition\":{large}"), 1);
    let data = decode_lexicon(&lexicon_files(&lexicon, &primary)).unwrap();
    assert_eq!(data.entries[0].source.ordinal.to_string(), large);
    assert_eq!(data.entries[0].source.file_offset.to_string(), large);
    assert_eq!(data.entries[0].source.bytes.to_string(), large);
    assert_eq!(data.primary[0].partition.to_string(), large);
    let mut with_raw = lexicon_files(&lexicon, &primary).to_vec();
    with_raw.push(("raw/NameToStructure.dat", b"0123456789"));
    assert_eq!(
        decode_lexicon(&with_raw).err(),
        Some(LexiconError::Inconsistent)
    );
}

#[test]
fn nickname_counts_and_ordinals_accept_exact_decimal_values() {
    let graphs = GRAPHS
        .replacen("\"nodes\":2", "\"nodes\":2.0", 1)
        .replacen("\"bonds\":1", "\"bonds\":1e0", 1)
        .replacen("\"source_bytes\":9", "\"source_bytes\":9.0", 1);
    let alphabet = ALPHABET.replacen(
        "\"source_ordinal_0based\":0",
        "\"source_ordinal_0based\":0.0",
        1,
    );
    assert!(graphs.contains("\"nodes\":2.0"));
    assert!(graphs.contains("\"bonds\":1e0"));
    assert!(graphs.contains("\"source_bytes\":9.0"));
    assert!(alphabet.contains("\"source_ordinal_0based\":0.0"));
    let data = decode_nicknames(&nickname_files(&graphs, &alphabet)).unwrap();
    assert_eq!((data.graphs[0].nodes, data.graphs[0].bonds), (2, 1));
    assert_eq!(data.alphabets[0].source_ordinal_0based.to_string(), "0");
    let fraction = graphs.replacen("\"nodes\":2.0", "\"nodes\":2.000000000000000001", 1);
    assert!(decode_nicknames(&nickname_files(&fraction, &alphabet)).is_err());
    let mismatch = graphs.replacen("\"nodes\":2.0", "\"nodes\":3.0", 1);
    assert_eq!(
        decode_nicknames(&nickname_files(&mismatch, &alphabet)).err(),
        Some(NicknameError::Inconsistent)
    );
    let duplicate = alphabet.replacen(
        "\"source_ordinal_0based\":1",
        "\"source_ordinal_0based\":0e0",
        1,
    );
    assert_eq!(
        decode_nicknames(&nickname_files(&graphs, &duplicate)).err(),
        Some(NicknameError::Inconsistent)
    );
}

#[test]
fn nickname_large_source_metadata_is_retained_without_inferring_raw_bytes() {
    let large = "9".repeat(400);
    let raw = br#"<CDXML><fragment id="10"><n id="1" NodeType="Element">first</n>tail<n id="2"/><b id="3" B="1" E="2" Order="1"/></fragment></CDXML>"#;
    let digest = format!("{:x}", Sha256::digest(raw));
    let graphs = GRAPHS.replacen(&"0".repeat(64), &digest, 1).replacen(
        "\"source_bytes\":9",
        &format!("\"source_bytes\":{large}"),
        1,
    );
    let alphabet = ALPHABET.replacen(
        "\"source_ordinal_0based\":0",
        &format!("\"source_ordinal_0based\":{large}"),
        1,
    );
    let data = decode_nicknames(&nickname_files(&graphs, &alphabet)).unwrap();
    assert_eq!(data.graphs[0].source_bytes.to_string(), large);
    assert_eq!(data.alphabets[0].source_ordinal_0based.to_string(), large);
    let mut with_raw = nickname_files(&graphs, &alphabet).to_vec();
    with_raw.push(("raw/nicknames/Q.cdxml", raw));
    assert_eq!(
        decode_nicknames(&with_raw).err(),
        Some(NicknameError::Inconsistent)
    );
    let actual = graphs.replacen(
        &format!("\"source_bytes\":{large}"),
        &format!("\"source_bytes\":{}", raw.len()),
        1,
    );
    let mut matched = nickname_files(&actual, &alphabet).to_vec();
    matched.push(("raw/nicknames/Q.cdxml", raw));
    assert!(decode_nicknames(&matched).is_ok());
}
