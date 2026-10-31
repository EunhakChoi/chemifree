//! Independently constructed CDX records from the public tag/field specifications.
use chemifree_application::interchange_codec::{InterchangeCodec, InterchangeFormat};
use chemifree_domain::interchange::{Content, ObjectKind, PropertyKind, PropertyValue};
use chemifree_providers::{cdx, cdxml, interchange_codec::LocalInterchangeCodec};

fn begin(bytes: &mut Vec<u8>, tag: u16, id: u32) {
    bytes.extend(tag.to_le_bytes());
    bytes.extend(id.to_le_bytes());
}
fn end(bytes: &mut Vec<u8>) {
    bytes.extend([0, 0]);
}
fn field(bytes: &mut Vec<u8>, tag: u16, value: &[u8]) {
    bytes.extend(tag.to_le_bytes());
    if value.len() >= 65535 {
        bytes.extend(u16::MAX.to_le_bytes());
        bytes.extend((value.len() as u32).to_le_bytes());
    } else {
        bytes.extend((value.len() as u16).to_le_bytes());
    }
    bytes.extend(value);
}
fn header() -> Vec<u8> {
    let mut bytes = b"VjCD0100\x04\x03\x02\x01".to_vec();
    bytes.extend([0; 16]);
    bytes
}
fn structure() -> Vec<u8> {
    let mut data = header();
    begin(&mut data, 0x8001, 1);
    begin(&mut data, 0x8003, 2);
    begin(&mut data, 0x8004, 3);
    let mut xy = (-2i32 * 65536).to_le_bytes().to_vec();
    xy.extend(65536i32.to_le_bytes());
    field(&mut data, 0x0200, &xy);
    field(&mut data, 0x0402, &8i16.to_le_bytes());
    field(&mut data, 0x0421, &(-1i32 << 24).to_le_bytes());
    end(&mut data);
    begin(&mut data, 0x8004, 4);
    end(&mut data);
    begin(&mut data, 0x8005, 5);
    field(&mut data, 0x0604, &3u32.to_le_bytes());
    field(&mut data, 0x0605, &4u32.to_le_bytes());
    field(&mut data, 0x0600, &2u16.to_le_bytes());
    end(&mut data);
    end(&mut data);
    end(&mut data);
    end(&mut data);
    data
}

#[test]
fn independently_framed_binary_matches_cdxml_chemistry_and_coordinates() {
    let input = structure();
    let binary = cdx::read(&input).unwrap().document;
    let xml = cdxml::read("<CDXML><page id=\"1\"><fragment id=\"2\"><n id=\"3\" p=\"1 -2\" Element=\"8\" Charge=\"-1\"/><n id=\"4\"/><b id=\"5\" B=\"3\" E=\"4\" Order=\"2\"/></fragment></page></CDXML>").unwrap();
    assert_eq!(binary.root(), xml.document().root());
    assert_eq!(
        cdx::read(&cdx::write(xml.document()).unwrap())
            .unwrap()
            .document
            .root(),
        xml.document().root()
    );
    let codec = LocalInterchangeCodec;
    assert_eq!(
        codec
            .write(InterchangeFormat::Cdx, &binary, Some(&input), false)
            .unwrap()
            .bytes,
        input
    );
}

#[test]
fn explicit_document_has_distinct_object_and_file_terminators() {
    // The top-level object is explicit here, unlike the public implicit-document example.
    // Both object and file endings are independently framed rather than made by the writer.
    let mut input = header();
    begin(&mut input, 0x8000, 9);
    begin(&mut input, 0x8001, 1);
    end(&mut input); // page
    end(&mut input); // document object
    end(&mut input); // file
    for i in 0..input.len() {
        assert!(cdx::read(&input[..i]).is_err(), "accepted prefix {i}");
    }
    let read = cdx::read(&input).unwrap();
    assert_eq!(read.document.root().id, Some(9));
    assert_eq!(cdx::write(&read.document).unwrap(), input);
    let mut wrong_terminator = input.clone();
    let last = wrong_terminator.len() - 2;
    wrong_terminator[last] = 1;
    assert_eq!(
        cdx::read(&wrong_terminator).unwrap_err().code,
        "invalid_cdx_terminator"
    );
}

#[test]
fn every_truncated_prefix_fails_and_extended_text_is_read_and_written() {
    let input = structure();
    for i in 0..input.len() {
        assert!(
            cdx::read(&input[..i]).is_err(),
            "accepted truncated prefix {i}"
        );
    }
    let mut data = header();
    begin(&mut data, 0x8001, 1);
    begin(&mut data, 0x8006, 2);
    let mut text = vec![0, 0];
    text.extend(vec![b'x'; 70_000]);
    field(&mut data, 0x0700, &text);
    end(&mut data);
    end(&mut data);
    end(&mut data);
    let parsed = cdx::read(&data).unwrap().document;
    let Content::Object(page) = &parsed.root().children[0] else {
        panic!("page")
    };
    let Content::Object(text) = &page.children[0] else {
        panic!("text")
    };
    let Content::Object(run) = &text.children[0] else {
        panic!("style")
    };
    let Content::Text(value) = &run.children[0] else {
        panic!("value")
    };
    assert_eq!(value.len(), 70_000);
    let encoded = cdx::write(&parsed).unwrap();
    assert!(encoded.windows(4).any(|w| w == [0x00, 0x07, 0xff, 0xff]));
    assert_eq!(cdx::read(&encoded).unwrap().document.root(), parsed.root());
}

#[test]
fn unknown_properties_and_objects_are_opaque_and_refuse_unverified_rewrites() {
    let mut bytes = header();
    begin(&mut bytes, 0x8001, 1);
    field(&mut bytes, 0x2222, &[3, 0, 0, 0]);
    begin(&mut bytes, 0x9001, 9);
    field(&mut bytes, 0x1234, &[1, 2, 3]);
    end(&mut bytes);
    end(&mut bytes);
    end(&mut bytes);
    let codec = LocalInterchangeCodec;
    let decoded = codec.read(InterchangeFormat::Cdx, &bytes).unwrap();
    let Content::Object(page) = &decoded.document.root().children[0] else {
        panic!("page")
    };
    assert_eq!(page.properties[0].kind, PropertyKind::UnknownBinary(0x2222));
    assert_eq!(
        page.properties[0].value,
        PropertyValue::Bytes(vec![3, 0, 0, 0])
    );
    assert!(
        matches!(&page.children[0], Content::Object(o) if o.kind == ObjectKind::UnknownBinary(0x9001))
    );
    assert_eq!(
        codec
            .write(
                InterchangeFormat::Cdx,
                &decoded.document,
                Some(&bytes),
                false
            )
            .unwrap()
            .bytes,
        bytes
    );
    assert_eq!(
        codec
            .write(InterchangeFormat::Cdx, &decoded.document, None, false)
            .unwrap_err()
            .code,
        "unsafe_unknown_rewrite"
    );
    let mut untrusted = decoded.document.root().clone();
    let Content::Object(page) = &mut untrusted.children[0] else {
        panic!("page")
    };
    page.id = Some(10);
    let forged = chemifree_domain::interchange::Document::new(untrusted).unwrap();
    assert_eq!(
        codec
            .write(InterchangeFormat::Cdx, &forged, Some(&bytes), false)
            .unwrap_err()
            .code,
        "unsafe_unknown_rewrite"
    );
    let discarded = codec
        .write(InterchangeFormat::Cdx, &decoded.document, None, true)
        .unwrap();
    assert!(!discarded.loss_report.is_empty());
    assert!(cdx::read(&discarded.bytes).is_ok());
}

fn styled_text(charset: u16, second_offset: u16) -> Vec<u8> {
    let mut data = header();
    let mut fonts = Vec::new();
    for value in [1u16, 1, 3, charset, 5] {
        fonts.extend(value.to_le_bytes());
    }
    fonts.extend(b"Arial");
    field(&mut data, 0x0100, &fonts);
    begin(&mut data, 0x8001, 1);
    begin(&mut data, 0x8006, 2);
    let mut text = Vec::new();
    for value in [2u16, 0, 3, 1, 240, 2, second_offset, 3, 2, 240, 2] {
        text.extend(value.to_le_bytes());
    }
    text.extend(b"Test");
    field(&mut data, 0x0700, &text);
    end(&mut data);
    end(&mut data);
    end(&mut data);
    data
}

#[test]
fn an_unused_unsupported_font_charset_is_not_invented_as_a_cdxml_name() {
    let mut bytes = header();
    let mut fonts = Vec::new();
    for value in [1u16, 1, 3, 65001, 5] {
        fonts.extend(value.to_le_bytes());
    }
    fonts.extend(b"Arial");
    field(&mut bytes, 0x0100, &fonts);
    begin(&mut bytes, 0x8001, 1);
    end(&mut bytes);
    end(&mut bytes);
    let error = cdx::read(&bytes).unwrap_err();
    assert_eq!(error.code, "unsupported_encoding");
    assert!(error.message.contains("65001"));
}

#[test]
fn styled_text_and_font_table_match_cdxml_and_unknown_encodings_are_refused() {
    let bytes = styled_text(1252, 2);
    let read = cdx::read(&bytes).unwrap();
    assert!(read.font_platform_unrepresented);
    let binary = read.document;
    let xml = cdxml::read("<CDXML><fonttable><font id=\"3\" name=\"Arial\" charset=\"iso-8859-1\"/></fonttable><page id=\"1\"><t id=\"2\"><s font=\"3\" face=\"1\" size=\"12\" color=\"2\">Te</s><s font=\"3\" face=\"2\" size=\"12\" color=\"2\">st</s></t></page></CDXML>").unwrap();
    assert_eq!(binary.root(), xml.document().root());
    assert_eq!(
        cdx::read(&cdx::write(xml.document()).unwrap())
            .unwrap()
            .document
            .root(),
        xml.document().root()
    );
    assert_eq!(
        cdx::read(&styled_text(65001, 2)).unwrap_err().code,
        "unsupported_encoding"
    );
    assert_eq!(
        cdx::read(&styled_text(1252, 0)).unwrap_err().code,
        "invalid_text"
    );
}

#[test]
fn ambiguous_ids_and_bad_bond_references_do_not_become_partial_trees() {
    let mut data = structure();
    // The known byte pattern is a bond's end-reference property, not a string search of an input file.
    let endpoint = [0x05, 0x06, 4, 0, 4, 0, 0, 0];
    let at = data
        .windows(endpoint.len())
        .position(|window| window == endpoint)
        .unwrap();
    data[at + 4] = 99;
    assert_eq!(cdx::read(&data).unwrap_err().code, "invalid_document");
    let mut data = header();
    begin(&mut data, 0x8001, 1);
    begin(&mut data, 0x8003, 2);
    begin(&mut data, 0x8004, 3);
    end(&mut data);
    begin(&mut data, 0x8004, 3);
    end(&mut data);
    end(&mut data);
    end(&mut data);
    end(&mut data);
    assert_eq!(cdx::read(&data).unwrap_err().code, "invalid_document");
}

#[test]
fn property_count_limit_rejects_before_building_an_oversized_tree() {
    let mut data = header();
    begin(&mut data, 0x8001, 1);
    for tag in 0x2000u16..0x2201 {
        field(&mut data, tag, &[]);
    }
    end(&mut data);
    end(&mut data);
    assert_eq!(cdx::read(&data).unwrap_err().code, "resource_limit");
    let mut root = cdx::read(&structure()).unwrap().document.root().clone();
    let Content::Object(page) = &mut root.children[0] else {
        panic!("page")
    };
    page.properties = (0x2000u16..0x2201)
        .map(|tag| chemifree_domain::interchange::Property {
            kind: PropertyKind::UnknownBinary(tag),
            value: PropertyValue::Bytes(Vec::new()),
        })
        .collect();
    let tree = chemifree_domain::interchange::Document::new(root).unwrap();
    assert_eq!(cdx::write(&tree).unwrap_err().code, "resource_limit");
}

#[test]
fn extended_unknown_property_requires_original_or_explicit_discard() {
    let mut bytes = header();
    begin(&mut bytes, 0x8001, 1);
    field(&mut bytes, 0x2222, &vec![0xa5; 70_000]);
    end(&mut bytes);
    end(&mut bytes);
    let codec = LocalInterchangeCodec;
    let read = codec.read(InterchangeFormat::Cdx, &bytes).unwrap();
    assert_eq!(
        codec
            .write(InterchangeFormat::Cdx, &read.document, Some(&bytes), false)
            .unwrap()
            .bytes,
        bytes
    );
    assert_eq!(
        codec
            .write(InterchangeFormat::Cdx, &read.document, None, false)
            .unwrap_err()
            .code,
        "unsafe_unknown_rewrite"
    );
    let dropped = codec
        .write(InterchangeFormat::Cdx, &read.document, None, true)
        .unwrap();
    assert!(
        dropped
            .loss_report
            .iter()
            .any(|loss| loss.feature == "opaque_extensions")
    );
    assert!(dropped.bytes.len() < bytes.len());
}

#[test]
fn color_table_components_agree_with_cdxml() {
    let mut bytes = header();
    let mut colors = 2u16.to_le_bytes().to_vec();
    for channel in [u16::MAX, u16::MAX, u16::MAX, 0, 0, 0] {
        colors.extend(channel.to_le_bytes());
    }
    field(&mut bytes, 0x0300, &colors);
    begin(&mut bytes, 0x8001, 1);
    end(&mut bytes);
    end(&mut bytes);
    let binary = cdx::read(&bytes).unwrap().document;
    let xml = cdxml::read("<CDXML><colortable><color r=\"1\" g=\"1\" b=\"1\"/><color r=\"0\" g=\"0\" b=\"0\"/></colortable><page id=\"1\"/></CDXML>").unwrap();
    assert_eq!(binary.root(), xml.document().root());
    assert_eq!(
        cdx::read(&cdx::write(xml.document()).unwrap())
            .unwrap()
            .document
            .root(),
        xml.document().root()
    );
}

#[test]
fn writer_refuses_unrepresentable_coordinate_instead_of_silent_rounding() {
    let xml =
        cdxml::read("<CDXML><page><fragment><n p=\"0.1 0\"/></fragment></page></CDXML>").unwrap();
    let error = cdx::write(xml.document()).unwrap_err();
    assert_eq!(error.code, "unrepresentable_number");
}
