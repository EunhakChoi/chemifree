//! Regression checks for the first independent review of the CDX codec.
use chemifree_application::LossDisposition;
use chemifree_application::interchange_codec::{InterchangeCodec, InterchangeFormat};
use chemifree_domain::interchange::{
    Content, Document, Object, ObjectKind, PropertyKind, PropertyValue,
};
use chemifree_providers::{cdx, cdxml, interchange_codec::LocalInterchangeCodec};

fn begin(bytes: &mut Vec<u8>, tag: u16, id: u32) {
    bytes.extend(tag.to_le_bytes());
    bytes.extend(id.to_le_bytes());
}
fn field(bytes: &mut Vec<u8>, tag: u16, value: &[u8]) {
    bytes.extend(tag.to_le_bytes());
    bytes.extend((value.len() as u16).to_le_bytes());
    bytes.extend(value);
}
fn header() -> Vec<u8> {
    let mut bytes = b"VjCD0100\x04\x03\x02\x01".to_vec();
    bytes.extend([0; 16]);
    bytes
}
fn font_table(platform: u16) -> Vec<u8> {
    let mut fonts = Vec::new();
    for value in [platform, 1, 3, 1252, 5] {
        fonts.extend(value.to_le_bytes());
    }
    fonts.extend(b"Arial");
    fonts
}
/// One page and fragment holding a single node with `fields`.
fn node_with(fields: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut data = header();
    begin(&mut data, 0x8001, 1);
    begin(&mut data, 0x8003, 2);
    begin(&mut data, 0x8004, 3);
    for (tag, value) in fields {
        field(&mut data, *tag, value);
    }
    data.extend([0; 8]);
    data
}
/// A text object with the raw text property `text`, preceded by an Arial font table.
fn text_file(platform: u16, text: &[u8]) -> Vec<u8> {
    let mut data = header();
    field(&mut data, 0x0100, &font_table(platform));
    begin(&mut data, 0x8001, 1);
    begin(&mut data, 0x8006, 2);
    field(&mut data, 0x0700, text);
    data.extend([0; 6]);
    data
}
fn two_runs(second_offset: u16) -> Vec<u8> {
    let mut text = Vec::new();
    for value in [2u16, 0, 3, 1, 240, 2, second_offset, 3, 2, 240, 2] {
        text.extend(value.to_le_bytes());
    }
    text.extend(b"Test");
    text
}
fn plain(text: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0, 0];
    bytes.extend(text);
    bytes
}
fn losses(
    codec: &LocalInterchangeCodec,
    document: &Document,
    original: Option<&[u8]>,
) -> Vec<String> {
    codec
        .write(InterchangeFormat::Cdx, document, original, false)
        .unwrap()
        .loss_report
        .into_iter()
        .map(|loss| loss.feature)
        .collect()
}

const STYLED_XML: &str = "<CDXML><fonttable><font id=\"3\" name=\"Arial\" charset=\"iso-8859-1\"/></fonttable><page id=\"1\"><t id=\"2\">\n  <s font=\"3\" face=\"1\" size=\"12\" color=\"2\">x</s>\n  <s font=\"3\" face=\"2\" size=\"12\" color=\"2\">y</s>\n</t></page></CDXML>";

#[test]
fn fractional_four_byte_charge_is_refused_not_truncated() {
    for charge in [[0, 0, 0x80, 0], [1, 0, 0, 0], [0, 0, 0x80, 0xff]] {
        let error = cdx::read(&node_with(&[(0x0421, charge.to_vec())])).unwrap_err();
        assert_eq!(error.code, "unsupported_fractional_charge", "{charge:?}");
    }
}

#[test]
fn font_platform_loss_is_reported_whenever_a_font_table_is_written_from_an_unknown_platform() {
    let codec = LocalInterchangeCodec;
    let xml = cdxml::read(STYLED_XML).unwrap();
    assert!(losses(&codec, xml.document(), None).contains(&"font_platform".to_owned()));

    let edit = |original: &[u8]| {
        let mut document = cdx::read(original).unwrap().document;
        document
            .edit(|root| {
                if let Some(Content::Object(page)) = root
                    .children
                    .iter_mut()
                    .find(|c| matches!(c, Content::Object(o) if o.kind == ObjectKind::Page))
                {
                    page.children.push(Content::Object(Object {
                        kind: ObjectKind::Fragment,
                        id: Some(9),
                        properties: Vec::new(),
                        children: Vec::new(),
                    }));
                }
            })
            .unwrap();
        document
    };
    let windows = text_file(1, &plain(b"hi"));
    let mac = text_file(0, &plain(b"hi"));
    assert!(!losses(&codec, &edit(&windows), Some(&windows)).contains(&"font_platform".to_owned()));
    assert!(losses(&codec, &edit(&mac), Some(&mac)).contains(&"font_platform".to_owned()));
}

#[test]
fn indented_cdxml_text_converts_and_comments_need_the_discard_consent() {
    let codec = LocalInterchangeCodec;
    let xml = cdxml::read(STYLED_XML).unwrap();
    let bytes = cdx::write(xml.document())
        .expect("whitespace between runs carries no meaning")
        .bytes;
    let compact = cdxml::read(&STYLED_XML.replace("\n  ", "").replace('\n', "")).unwrap();
    assert_eq!(
        cdx::read(&bytes).unwrap().document.root(),
        compact.document().root()
    );

    let with_comment = STYLED_XML.replace(
        "<s font=\"3\" face=\"1\"",
        "<!--note--><s font=\"3\" face=\"1\"",
    );
    let xml = cdxml::read(&with_comment).unwrap();
    let refused = codec
        .write(InterchangeFormat::Cdx, xml.document(), None, false)
        .unwrap_err();
    assert_eq!(refused.code, "comment_discard_required");
    let written = codec
        .write(InterchangeFormat::Cdx, xml.document(), None, true)
        .unwrap();
    let dropped: Vec<_> = written
        .loss_report
        .iter()
        .filter(|loss| loss.feature == "xml_comment")
        .collect();
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0].disposition, LossDisposition::Dropped);
    assert!(cdx::read(&written.bytes).is_ok());
}

#[test]
fn a_style_run_at_the_end_of_the_text_survives_the_cdxml_round_trip() {
    let source = text_file(1, &two_runs(4));
    let first = cdx::read(&source).unwrap().document;
    let xml_bytes = cdxml::write(&cdxml::CdxmlDocument::from_document(first.clone()))
        .unwrap()
        .into_bytes();
    let xml = cdxml::read(std::str::from_utf8(&xml_bytes).unwrap()).unwrap();
    let rewritten = cdx::write(xml.document())
        .expect("an empty trailing run is representable")
        .bytes;
    assert_eq!(cdx::read(&rewritten).unwrap().document.root(), first.root());
}

#[test]
fn colors_between_cdx_steps_are_rounded_with_a_precision_loss() {
    let codec = LocalInterchangeCodec;
    let xml = cdxml::read("<CDXML><colortable><color r=\"0.5\" g=\"0\" b=\"1\"/></colortable><page id=\"1\"/></CDXML>").unwrap();
    let written = codec
        .write(InterchangeFormat::Cdx, xml.document(), None, false)
        .unwrap();
    let loss: Vec<_> = written
        .loss_report
        .iter()
        .filter(|loss| loss.feature == "color_precision")
        .collect();
    assert_eq!(loss.len(), 1);
    assert_eq!(loss[0].disposition, LossDisposition::Normalized);
    let decoded = cdx::read(&written.bytes).unwrap().document;
    let table = decoded
        .root()
        .children
        .iter()
        .find_map(|c| match c {
            Content::Object(o) if o.kind == ObjectKind::ColorTable => Some(o),
            _ => None,
        })
        .unwrap();
    let Content::Object(color) = &table.children[0] else {
        panic!("a color")
    };
    assert_eq!(
        color.properties[0].value,
        PropertyValue::Float(32768.0 / 65535.0)
    );
    assert_eq!(color.properties[0].kind, PropertyKind::Red);
}

#[test]
fn an_invalid_header_keeps_its_byte_zero_location() {
    let error = cdx::read(b"not a cdx file at all, not a cdx file at all").unwrap_err();
    assert_eq!(error.code, "invalid_cdx_header");
    let codec = LocalInterchangeCodec;
    let refused = codec
        .read(
            InterchangeFormat::Cdx,
            b"not a cdx file at all, not a cdx file at all",
        )
        .unwrap_err();
    assert_eq!(refused.byte_offset, Some(0));
}

#[test]
fn unknown_radical_values_are_refused_at_read() {
    for radical in 0u8..=3 {
        assert!(cdx::read(&node_with(&[(0x0422, vec![radical])])).is_ok());
    }
    let error = cdx::read(&node_with(&[(0x0422, vec![4])])).unwrap_err();
    assert_eq!(error.code, "invalid_property_value");
}

#[test]
fn text_that_xml_cannot_carry_is_refused_at_read() {
    for text in [plain(b"a\x01b"), plain(b"a\x00b")] {
        let error = cdx::read(&text_file(1, &text)).unwrap_err();
        assert_eq!(error.code, "unsupported_text_character");
    }
    let mut styled = two_runs(2);
    styled[24] = 0x01;
    assert_eq!(
        cdx::read(&text_file(1, &styled)).unwrap_err().code,
        "unsupported_text_character"
    );
    assert!(cdx::read(&text_file(1, &plain(b"tab\tline\nbreak"))).is_ok());
}

#[test]
fn font_names_with_control_characters_are_refused_at_read() {
    let mut bytes = text_file(1, &plain(b"hi"));
    let name = bytes.windows(5).position(|w| w == b"Arial").unwrap();
    bytes[name + 4] = 0x0c;
    assert_eq!(
        cdx::read(&bytes).unwrap_err().code,
        "unsupported_text_character"
    );
}
