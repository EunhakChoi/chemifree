use chemifree_application::nickname_data::{NicknameError, decode_dataset, verify_catalog_files};
use sha2::{Digest, Sha256};

const GRAPHS: &str = include_str!("data/nickname-synthetic/graphs.jsonl");
const ALPHABET: &str = include_str!("data/nickname-synthetic/alphabet.jsonl");

fn files<'a>(graphs: &'a str, alphabet: &'a str) -> [(&'static str, &'a [u8]); 2] {
    [
        ("normalized/nickname-graphs.jsonl", graphs.as_bytes()),
        ("normalized/sequence-alphabets.jsonl", alphabet.as_bytes()),
    ]
}

#[test]
fn ordered_candidates_and_nested_xml_are_preserved_per_resource() {
    let data = decode_dataset(&files(GRAPHS, ALPHABET)).unwrap();
    assert_eq!(data.graphs.len(), 2);
    assert_eq!(data.graphs[0].name, "Q");
    assert_eq!(data.graphs[1].name, "R");
    assert_eq!(data.alphabets[0].nickname_names, ["Q", "R", "Q"]);
    assert_eq!(
        data.alphabets[0].display_value,
        data.alphabets[1].display_value
    );
    let node = &data.graphs[0].xml_tree.children[0].children[0];
    assert_eq!(node.text.as_deref(), Some("first"));
    assert_eq!(node.tail.as_deref(), Some("tail"));
    assert_eq!(node.attributes["id"], "1");
    assert_eq!(data.graphs[1].xml_tree.children[0].attributes["id"], "1");
}

#[test]
fn counts_identities_and_bond_endpoints_are_checked_within_each_document() {
    let count = GRAPHS.replacen("\"nodes\":2", "\"nodes\":3", 1);
    let missing = GRAPHS.replacen("\"E\":\"2\"", "\"E\":\"99\"", 1);
    let duplicate = GRAPHS.replacen("\"id\":\"2\"", "\"id\":\"1\"", 1);
    let missing_id = GRAPHS.replacen("\"id\":\"2\"", "\"other\":\"2\"", 1);
    let outcomes = [count, missing, duplicate, missing_id]
        .map(|input| decode_dataset(&files(&input, ALPHABET)).err());
    assert_eq!(outcomes, [Some(NicknameError::Inconsistent); 4]);
}

#[test]
fn missing_or_ambiguous_nickname_references_are_rejected() {
    let missing = ALPHABET.replacen("\"Q\",\"R\",\"Q\"", "\"Q\",\"missing\",\"Q\"", 1);
    assert_eq!(
        decode_dataset(&files(GRAPHS, &missing)).err(),
        Some(NicknameError::Inconsistent)
    );
    let duplicate = GRAPHS.replacen("\"name\":\"R\"", "\"name\":\"Q\"", 1);
    assert_eq!(
        decode_dataset(&files(&duplicate, ALPHABET)).err(),
        Some(NicknameError::Inconsistent)
    );
}

#[test]
fn closed_schema_requires_nullable_fields_and_digests() {
    let missing_tail = GRAPHS.replacen(",\"tail\":null", "", 1);
    assert_eq!(
        decode_dataset(&files(&missing_tail, ALPHABET)).err(),
        Some(NicknameError::Record)
    );
    let bad_digest = ALPHABET.replacen(&"2".repeat(64), &"F".repeat(64), 1);
    assert_eq!(
        decode_dataset(&files(GRAPHS, &bad_digest)).err(),
        Some(NicknameError::Record)
    );
    let extra = ALPHABET.replacen("\"key\":\"x\"", "\"key\":\"x\",\"unknown\":1", 1);
    assert_eq!(
        decode_dataset(&files(GRAPHS, &extra)).err(),
        Some(NicknameError::Record)
    );
    assert_eq!(
        decode_dataset(&files(GRAPHS.trim_end(), ALPHABET)).err(),
        Some(NicknameError::Framing)
    );
}

#[test]
fn profile_requires_complete_files_and_real_catalog_metadata() {
    assert_eq!(
        decode_dataset(&files(GRAPHS, ALPHABET)[..1]).err(),
        Some(NicknameError::FileSet)
    );
    assert_eq!(
        verify_catalog_files(
            "legacy-sequence-nickname-graphs",
            "sha256-set-71abd8af61c6ec68dfaa465245b140196ab3c10cd938fa640a9318381004598d",
            b"manifest",
            b"registry",
            b"profile"
        ),
        Err(NicknameError::CatalogMismatch)
    );
}

fn graphs_with_raw(raw_q: &[u8], raw_r: &[u8]) -> String {
    let mut rows: Vec<serde_json::Value> = GRAPHS
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for (row, bytes) in rows.iter_mut().zip([raw_q, raw_r]) {
        row["source_sha256"] = format!("{:x}", Sha256::digest(bytes)).into();
        row["source_bytes"] = bytes.len().into();
    }
    rows.into_iter()
        .map(|row| format!("{}\n", serde_json::to_string(&row).unwrap()))
        .collect()
}

#[test]
fn supplied_raw_documents_can_be_verified_as_a_subset() {
    let raw_q = br#"<CDXML><fragment id="10"><n id="1" NodeType="Element">first</n>tail<n id="2"/><b id="3" B="1" E="2" Order="1"/></fragment></CDXML>"#;
    let raw_r = br#"<CDXML><n id="1"/></CDXML>"#;
    let graphs = graphs_with_raw(raw_q, raw_r);
    let mut selected = files(&graphs, ALPHABET).to_vec();
    selected.push(("raw/nicknames/Q.cdxml", raw_q));
    assert!(decode_dataset(&selected).is_ok());
}

#[test]
fn supplied_raw_xml_files_match_the_normalized_trees() {
    let raw_q = br#"<CDXML><fragment id="10"><n id="1" NodeType="Element">first</n>tail<n id="2"/><b id="3" B="1" E="2" Order="1"/></fragment></CDXML>"#;
    let raw_r = br#"<CDXML><n id="1"/></CDXML>"#;
    let graphs = graphs_with_raw(raw_q, raw_r);
    let mut selected = files(&graphs, ALPHABET).to_vec();
    selected.push(("raw/nicknames/Q.cdxml", raw_q));
    selected.push(("raw/nicknames/R.cdxml", raw_r));
    assert!(decode_dataset(&selected).is_ok());

    let mut missing = selected.clone();
    missing.pop();
    assert!(decode_dataset(&missing).is_ok());
    let malformed = br#"<CDXML><n id="1"></CDXML>"#;
    let malformed_graphs = graphs_with_raw(malformed, raw_r);
    selected[0].1 = malformed_graphs.as_bytes();
    selected[2].1 = malformed;
    assert_eq!(decode_dataset(&selected).err(), Some(NicknameError::Record));
    let mismatched = br#"<CDXML><fragment id="10"><n id="1" NodeType="Element">first</n>tail<n id="2"/><b id="3" B="1" E="2" Order="2"/></fragment></CDXML>"#;
    let mismatched_graphs = graphs_with_raw(mismatched, raw_r);
    selected[0].1 = mismatched_graphs.as_bytes();
    selected[2].1 = mismatched;
    assert_eq!(
        decode_dataset(&selected).err(),
        Some(NicknameError::Inconsistent)
    );
}

#[test]
fn identical_normalized_tree_does_not_replace_a_raw_source_digest() {
    let raw_q = br#"<CDXML><fragment id="10"><n id="1" NodeType="Element">first</n>tail<n id="2"/><b id="3" B="1" E="2" Order="1"/></fragment></CDXML>"#;
    let raw_r = br#"<CDXML><n id="1"/></CDXML>"#;
    let graphs = graphs_with_raw(raw_q, raw_r);
    let mut lines: Vec<serde_json::Value> = graphs
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    lines[0]["source_sha256"] = "0".repeat(64).into();
    let bad = lines
        .into_iter()
        .map(|line| format!("{}\n", serde_json::to_string(&line).unwrap()))
        .collect::<String>();
    let mut selected = files(&bad, ALPHABET).to_vec();
    selected.push(("raw/nicknames/Q.cdxml", raw_q));
    selected.push(("raw/nicknames/R.cdxml", raw_r));
    assert_eq!(
        decode_dataset(&selected).err(),
        Some(NicknameError::Inconsistent)
    );
}

#[test]
fn raw_xml_never_resolves_external_entities() {
    let temp = tempfile::tempdir().unwrap();
    let secret = temp.path().join("independent-fixture.txt");
    std::fs::write(&secret, "first").unwrap();
    let raw_q = format!(
        "<!DOCTYPE CDXML [<!ENTITY external SYSTEM \"file://{}\">]><CDXML><fragment id=\"10\"><n id=\"1\" NodeType=\"Element\">&external;</n>tail<n id=\"2\"/><b id=\"3\" B=\"1\" E=\"2\" Order=\"1\"/></fragment></CDXML>",
        secret.display(),
    );
    let raw_r = br#"<CDXML><n id="1"/></CDXML>"#;
    let graphs = graphs_with_raw(raw_q.as_bytes(), raw_r);
    let mut selected = files(&graphs, ALPHABET).to_vec();
    selected.push(("raw/nicknames/Q.cdxml", raw_q.as_bytes()));
    selected.push(("raw/nicknames/R.cdxml", raw_r));
    assert_eq!(
        decode_dataset(&selected).err(),
        Some(NicknameError::UnsupportedEntityDeclaration)
    );
}

#[test]
fn ordinary_external_doctype_is_inert_and_mixed_text_is_preserved() {
    let variants: [&[u8]; 2] = [
        br#"<!DOCTYPE CDXML SYSTEM "https://example.invalid/nothing.dtd"><CDXML><fragment id="10"><n id="1" NodeType="Element">fi<!-- ignored -->rst</n>tail<n id="2"/><b id="3" B="1" E="2" Order="1"/></fragment></CDXML>"#,
        br#"<!DOCTYPE CDXML SYSTEM "https://example.invalid/<!ENTITY"><CDXML><fragment id="10"><n id="1" NodeType="Element">first</n>tail<n id="2"/><b id="3" B="1" E="2" Order="1"/></fragment></CDXML>"#,
    ];
    let raw_r = br#"<CDXML><n id="1"/></CDXML>"#;
    for raw_q in variants {
        let graphs = graphs_with_raw(raw_q, raw_r);
        let mut selected = files(&graphs, ALPHABET).to_vec();
        selected.push(("raw/nicknames/Q.cdxml", raw_q));
        selected.push(("raw/nicknames/R.cdxml", raw_r));
        assert!(decode_dataset(&selected).is_ok());
    }
}

#[test]
fn internal_entity_declarations_are_refused_before_they_can_expand() {
    let variants: [&[u8]; 2] = [
        br#"<!DOCTYPE CDXML [<!ENTITY word "first">]><CDXML><fragment id="10"><n id="1" NodeType="Element">&word;</n>tail<n id="2"/><b id="3" B="1" E="2" Order="1"/></fragment></CDXML>"#,
        br#"<!DOCTYPE CDXML SYSTEM "https://example.invalid/a[b" [<!-- before --> <!ENTITY word "first">]><CDXML><fragment id="10"><n id="1" NodeType="Element">&word;</n>tail<n id="2"/><b id="3" B="1" E="2" Order="1"/></fragment></CDXML>"#,
    ];
    let raw_r = br#"<CDXML><n id="1"/></CDXML>"#;
    for raw_q in variants {
        let graphs = graphs_with_raw(raw_q, raw_r);
        let mut selected = files(&graphs, ALPHABET).to_vec();
        selected.push(("raw/nicknames/Q.cdxml", raw_q));
        assert_eq!(
            decode_dataset(&selected).err(),
            Some(NicknameError::UnsupportedEntityDeclaration)
        );
    }
}

#[test]
fn entity_text_inside_a_doctype_comment_is_not_a_declaration() {
    let raw_q = br#"<!DOCTYPE CDXML [<!-- an example <!ENTITY word "first"> -->]><CDXML><fragment id="10"><n id="1" NodeType="Element">first</n>tail<n id="2"/><b id="3" B="1" E="2" Order="1"/></fragment></CDXML>"#;
    let raw_r = br#"<CDXML><n id="1"/></CDXML>"#;
    let graphs = graphs_with_raw(raw_q, raw_r);
    let mut selected = files(&graphs, ALPHABET).to_vec();
    selected.push(("raw/nicknames/Q.cdxml", raw_q));
    assert!(decode_dataset(&selected).is_ok());
}

#[test]
fn alphabet_keys_and_source_ordinals_are_unambiguous_within_each_alphabet() {
    let duplicate_key = ALPHABET.replacen("\"key\":\"y\"", "\"key\":\"x\"", 1);
    let duplicate_ordinal = ALPHABET.replacen(
        "\"source_ordinal_0based\":1",
        "\"source_ordinal_0based\":0",
        1,
    );
    let results = [duplicate_key, duplicate_ordinal]
        .map(|input| decode_dataset(&files(GRAPHS, &input)).err());
    assert_eq!(results, [Some(NicknameError::Inconsistent); 2]);
    let another_alphabet = ALPHABET.replacen(
        "\"alphabet\":\"fixture\",\"source_ordinal_0based\":1,\"key\":\"y\"",
        "\"alphabet\":\"another\",\"source_ordinal_0based\":0,\"key\":\"x\"",
        1,
    );
    assert!(decode_dataset(&files(GRAPHS, &another_alphabet)).is_ok());
}
