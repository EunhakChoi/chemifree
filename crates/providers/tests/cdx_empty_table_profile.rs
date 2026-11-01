use chemifree_application::interchange_codec::{InterchangeCodec, InterchangeFormat};
use chemifree_providers::{cdx, interchange_codec::LocalInterchangeCodec};

// Independently framed CDX property, followed by an empty page and file end.
fn with_table(tag: u16, contents: &[u8]) -> Vec<u8> {
    let mut input = b"VjCD0100\x04\x03\x02\x01".to_vec();
    input.extend([0; 16]);
    input.extend(tag.to_le_bytes());
    input.extend((contents.len() as u16).to_le_bytes());
    input.extend(contents);
    input.extend(0x8001u16.to_le_bytes());
    input.extend(1u32.to_le_bytes());
    input.extend([0, 0, 0, 0]);
    input
}

#[test]
fn zero_font_count_is_a_profile_refusal_not_a_claim_of_invalid_cdx() {
    let input = with_table(0x0100, &[1, 0, 0, 0]); // Windows platform, zero fonts.
    let error = cdx::read(&input).unwrap_err();
    assert_eq!(error.code, "unsupported_empty_table");
    assert!(error.message.contains("profile"));
    assert_eq!(error.offset, Some(34));
    let error = LocalInterchangeCodec
        .read(InterchangeFormat::Cdx, &input)
        .unwrap_err();
    assert_eq!(error.code, "unsupported_empty_table");
}

#[test]
fn zero_color_count_is_a_profile_refusal_not_a_claim_of_invalid_cdx() {
    let input = with_table(0x0300, &[0, 0]);
    let error = cdx::read(&input).unwrap_err();
    assert_eq!(error.code, "unsupported_empty_table");
    assert!(error.message.contains("profile"));
    assert_eq!(error.offset, Some(32));
    let error = LocalInterchangeCodec
        .read(InterchangeFormat::Cdx, &input)
        .unwrap_err();
    assert_eq!(error.code, "unsupported_empty_table");
}

#[test]
fn a_document_without_tables_is_still_supported() {
    let mut input = b"VjCD0100\x04\x03\x02\x01".to_vec();
    input.extend([0; 16]);
    input.extend(0x8001u16.to_le_bytes());
    input.extend(1u32.to_le_bytes());
    input.extend([0, 0, 0, 0]);
    assert!(cdx::read(&input).is_ok());
}

#[test]
fn zero_count_with_trailing_table_bytes_is_reported_as_malformed() {
    for (tag, contents, expected) in [
        (0x0100, &[1, 0, 0, 0, 255][..], "invalid_font_table"),
        (0x0300, &[0, 0, 255][..], "invalid_color_table"),
    ] {
        let error = cdx::read(&with_table(tag, contents)).unwrap_err();
        assert_eq!(error.code, expected);
    }
}
