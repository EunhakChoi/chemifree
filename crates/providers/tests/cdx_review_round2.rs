//! Regression checks for the second independent review of the CDX codec.
use chemifree_application::LossDisposition;
use chemifree_application::interchange_codec::{InterchangeCodec, InterchangeFormat};
use chemifree_domain::interchange::{Content, Document};
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
    font_table_with(platform, 3)
}
fn font_table_with(platform: u16, id: u16) -> Vec<u8> {
    let mut fonts = Vec::new();
    for value in [platform, 1, id, 1252, 5] {
        fonts.extend(value.to_le_bytes());
    }
    fonts.extend(b"Arial");
    fonts
}
fn page_with(fields: &[(u16, Vec<u8>)], object: u16, inner: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut data = header();
    for (tag, value) in fields {
        field(&mut data, *tag, value);
    }
    begin(&mut data, 0x8001, 1);
    begin(&mut data, object, 2);
    for (tag, value) in inner {
        field(&mut data, *tag, value);
    }
    data.extend([0; 6]);
    data
}
fn text_file(text: &[u8]) -> Vec<u8> {
    page_with(
        &[(0x0100, font_table(1))],
        0x8006,
        &[(0x0700, text.to_vec())],
    )
}
fn plain(text: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0, 0];
    bytes.extend(text);
    bytes
}
fn runs(offsets: &[u16], text: &[u8]) -> Vec<u8> {
    let mut bytes = (offsets.len() as u16).to_le_bytes().to_vec();
    for offset in offsets {
        for value in [*offset, 3, 1, 240, 2] {
            bytes.extend(u16::to_le_bytes(value));
        }
    }
    bytes.extend(text);
    bytes
}

const FONT: &str = "<fonttable><font id=\"3\" name=\"Arial\" charset=\"iso-8859-1\"/></fonttable>";
fn text_xml(runs: &str) -> String {
    format!("<CDXML>{FONT}<page id=\"1\"><t id=\"2\">{runs}</t></page></CDXML>")
}
const S: &str = "font=\"3\" face=\"1\" size=\"12\" color=\"2\"";

fn export(
    source: &str,
    discard: bool,
) -> chemifree_application::interchange_codec::InterchangeWrite {
    let xml = cdxml::read(source).unwrap();
    LocalInterchangeCodec
        .write(InterchangeFormat::Cdx, xml.document(), None, discard)
        .unwrap_or_else(|error| panic!("{source}: {error:?}"))
}

#[test]
fn comments_inside_between_and_around_runs_are_dropped_with_consent() {
    let clean = export(&text_xml(&format!("<s {S}>ab</s><s {S}>c</s>")), false).bytes;
    for (noisy, notes) in [
        (format!("<s {S}>a<!--c-->b</s><s {S}>c</s>"), 1),
        (format!("<s {S}>a<?pi v?>b</s><s {S}>c</s>"), 1),
        (format!("<s {S}>a<!--1-->b<!--2--></s><s {S}>c</s>"), 2),
        (format!("<!--x--><s {S}>ab</s><?p?><s {S}>c</s><!--y-->"), 3),
        (format!("<s {S}><!--lead-->ab</s><s {S}>c</s>"), 1),
    ] {
        let written = export(&text_xml(&noisy), true);
        assert_eq!(written.bytes, clean, "{noisy}");
        let dropped: Vec<_> = written
            .loss_report
            .iter()
            .filter(|loss| loss.feature == "xml_comment")
            .collect();
        assert_eq!(dropped.len(), 1, "{noisy}");
        assert_eq!(dropped[0].disposition, LossDisposition::Dropped);
        assert!(
            dropped[0].reason.starts_with(&notes.to_string()),
            "{noisy}: {}",
            dropped[0].reason
        );
    }
}

#[test]
fn the_writer_never_emits_a_value_its_reader_refuses() {
    let base = cdxml::read("<CDXML><page id=\"1\"><fragment id=\"2\"><n id=\"3\" Radical=\"Doublet\"/></fragment></page></CDXML>").unwrap();
    for radical in 0..=300 {
        let mut document = base.document().clone();
        let edited = document.edit(|root| set_radical(root, radical));
        if edited.is_err() {
            continue;
        }
        match cdx::write(&document) {
            Ok(written) => assert!(
                cdx::read(&written.bytes).is_ok(),
                "radical {radical} was written but cannot be read back"
            ),
            Err(error) => assert!(radical > 3, "radical {radical}: {error:?}"),
        }
        if radical > 3 && radical <= 255 {
            assert_eq!(
                cdx::write(&document).unwrap_err().code,
                "invalid_property_value"
            );
        }
    }
    // C1 bytes in unstyled text, and controls injected into a tree, are refused as the reader does.
    let c1 = cdxml::read(&text_xml("<s>caf\u{85}</s>")).unwrap();
    assert_eq!(
        cdx::write(c1.document()).unwrap_err().code,
        "unsupported_encoding"
    );
    let mut control = cdxml::read(&text_xml("<s>ok</s>"))
        .unwrap()
        .document()
        .clone();
    control
        .edit(|root| {
            set_first_text(root, "a\u{1}b");
        })
        .unwrap();
    assert_eq!(
        cdx::write(&control).unwrap_err().code,
        "unsupported_text_character"
    );
}

fn set_radical(object: &mut chemifree_domain::interchange::Object, value: i64) {
    for property in &mut object.properties {
        if property.kind == chemifree_domain::interchange::PropertyKind::Radical {
            property.value = chemifree_domain::interchange::PropertyValue::Integer(value);
        }
    }
    for child in &mut object.children {
        if let Content::Object(child) = child {
            set_radical(child, value);
        }
    }
}

fn set_first_text(object: &mut chemifree_domain::interchange::Object, value: &str) -> bool {
    for child in &mut object.children {
        match child {
            Content::Text(text) => {
                *text = value.to_owned();
                return true;
            }
            Content::Object(child) => {
                if set_first_text(child, value) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

#[test]
fn any_font_table_off_platform_one_is_reported() {
    for platforms in [[0, 1], [1, 0]] {
        let mut bytes = header();
        for platform in platforms {
            // Distinct IDs per platform, so two tables do not collide.
            field(&mut bytes, 0x0100, &font_table_with(platform, 3 + platform));
        }
        begin(&mut bytes, 0x8001, 1);
        bytes.extend([0; 4]);
        match cdx::read(&bytes) {
            Ok(read) => assert_eq!(read.font_platform, Some(0), "{platforms:?}"),
            Err(error) => panic!("{platforms:?}: {error:?}"),
        }
    }
}

#[test]
fn unstyled_text_refuses_the_ambiguous_latin1_range_like_styled_text() {
    let error = cdx::read(&text_file(&plain(b"caf\x85"))).unwrap_err();
    assert_eq!(error.code, "unsupported_encoding");
}

#[test]
fn known_enums_outside_their_range_are_refused_with_a_code_and_offset() {
    let node = |tag: u16, value: Vec<u8>| {
        let mut data = header();
        begin(&mut data, 0x8001, 1);
        begin(&mut data, 0x8003, 3);
        begin(&mut data, 0x8004, 2);
        field(&mut data, tag, &value);
        data.extend([0; 8]);
        data
    };
    let graphic = |value: Vec<u8>| page_with(&[], 0x8007, &[(0x0a00, value)]);
    let bond = |value: Vec<u8>| page_with(&[], 0x8005, &[(0x0600, value)]);
    let cases = [
        node(0x0400, 14i16.to_le_bytes().to_vec()),
        node(0x0402, 0i16.to_le_bytes().to_vec()),
        node(0x0402, 119i16.to_le_bytes().to_vec()),
        node(0x0420, (-2i16).to_le_bytes().to_vec()),
        graphic(8i16.to_le_bytes().to_vec()),
        bond(0u16.to_le_bytes().to_vec()),
    ];
    for bytes in cases {
        let error = cdx::read(&bytes).unwrap_err();
        assert_eq!(error.code, "invalid_property_value", "{bytes:02x?}");
        assert!(error.offset.is_some_and(|offset| offset >= 28), "{error:?}");
    }
    assert!(cdx::read(&node(0x0400, 0i16.to_le_bytes().to_vec())).is_ok());
}

#[test]
fn text_without_runs_and_several_unstyled_runs_have_accurate_outcomes() {
    // The common model requires a style run in every text object, so "no run" cannot occur.
    assert_eq!(
        cdxml::read(&text_xml("")).unwrap_err().code,
        "invalid_document"
    );
    let empty = export(&text_xml("<s/>"), false).bytes;
    let read = cdx::read(&empty).unwrap().document;
    assert_eq!(
        read.root(),
        cdxml::read(&text_xml("<s/>")).unwrap().document().root()
    );
    let several = cdxml::read(&text_xml("<s>a</s><s>b</s>")).unwrap();
    let error = cdx::write(several.document()).unwrap_err();
    assert_eq!(error.code, "unrepresentable_style");
    assert!(!error.message.contains("Mixed"), "{}", error.message);
}

fn through_cdxml(document: &Document) -> Document {
    let xml = cdxml::write(&cdxml::CdxmlDocument::from_document(document.clone())).unwrap();
    cdxml::read(&xml).unwrap().document().clone()
}

#[test]
fn empty_runs_read_the_same_from_cdx_and_cdxml() {
    for bytes in [
        text_file(&plain(b"")),
        text_file(&runs(&[0, 4], b"Test")),
        text_file(&runs(&[0], b"")),
    ] {
        let first = cdx::read(&bytes).unwrap().document;
        assert_eq!(through_cdxml(&first).root(), first.root(), "{bytes:02x?}");
    }
}
