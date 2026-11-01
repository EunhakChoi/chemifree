//! Signed-value boundaries of the CDX codec, checked against CDXML spellings.
use chemifree_domain::interchange::{Content, Document, Object, PropertyKind, PropertyValue};
use chemifree_providers::{cdx, cdxml};

fn field(bytes: &mut Vec<u8>, tag: u16, value: &[u8]) {
    bytes.extend(tag.to_le_bytes());
    bytes.extend((value.len() as u16).to_le_bytes());
    bytes.extend(value);
}

fn begin(bytes: &mut Vec<u8>, tag: u16, id: u32) {
    bytes.extend(tag.to_le_bytes());
    bytes.extend(id.to_le_bytes());
}

/// A one-node document whose node carries `fields`, framed without the codec.
fn node_with(fields: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut data = b"VjCD0100\x04\x03\x02\x01".to_vec();
    data.extend([0; 16]);
    begin(&mut data, 0x8001, 1);
    begin(&mut data, 0x8003, 2);
    begin(&mut data, 0x8004, 3);
    for (tag, value) in fields {
        field(&mut data, *tag, value);
    }
    data.extend([0; 8]);
    data
}

fn node_integer(document: &Document, kind: PropertyKind) -> Option<i64> {
    fn walk(object: &Object, kind: &PropertyKind) -> Option<i64> {
        for property in &object.properties {
            if &property.kind == kind
                && let PropertyValue::Integer(value) = property.value
            {
                return Some(value);
            }
        }
        object.children.iter().find_map(|child| match child {
            Content::Object(child) => walk(child, kind),
            _ => None,
        })
    }
    walk(document.root(), &kind)
}

fn round_trip(source: &str) -> Result<Document, cdx::CdxError> {
    let xml = cdxml::read(source).expect("CDXML fixture is valid");
    let bytes = cdx::write(xml.document())?;
    let decoded = cdx::read(&bytes).expect("written CDX is readable").document;
    assert_eq!(decoded.root(), xml.document().root(), "{source}");
    Ok(decoded)
}

#[test]
fn coordinate_extremes_round_trip_and_one_step_past_is_refused() {
    // 2147483647 / 65536 and -2147483648 / 65536 are the exact 16.16 limits.
    for p in ["-32768 -32768", "32767.9999847412109375 32767.9999847412109375"] {
        round_trip(&format!(
            "<CDXML><page id=\"1\"><fragment id=\"2\"><n id=\"3\" p=\"{p}\"/></fragment></page></CDXML>"
        ))
        .unwrap_or_else(|error| panic!("{p}: {error:?}"));
    }
    for p in ["32768 0", "0 -32768.0000152587890625"] {
        let xml = cdxml::read(&format!(
            "<CDXML><page id=\"1\"><fragment id=\"2\"><n id=\"3\" p=\"{p}\"/></fragment></page></CDXML>"
        ))
        .unwrap();
        let error = cdx::write(xml.document()).unwrap_err();
        assert_eq!(error.code, "unrepresentable_number", "{p}");
    }
}

#[test]
fn charge_extremes_round_trip_and_one_past_is_refused_by_the_model() {
    for charge in [-128, 127] {
        let document = round_trip(&format!(
            "<CDXML><page id=\"1\"><fragment id=\"2\"><n id=\"3\" Charge=\"{charge}\"/></fragment></page></CDXML>"
        ))
        .unwrap_or_else(|error| panic!("{charge}: {error:?}"));
        assert_eq!(node_integer(&document, PropertyKind::Charge), Some(charge));
    }
    // The common model already bounds charge to the signed-8 range.
    for charge in [-129, 128] {
        let error = cdxml::read(&format!(
            "<CDXML><page id=\"1\"><fragment id=\"2\"><n id=\"3\" Charge=\"{charge}\"/></fragment></page></CDXML>"
        ))
        .unwrap_err();
        assert_eq!(error.code, "invalid_document", "{charge}");
    }
}

#[test]
fn isotope_maximum_round_trips_and_the_model_refuses_one_past() {
    let document = round_trip(
        "<CDXML><page id=\"1\"><fragment id=\"2\"><n id=\"3\" Isotope=\"32767\"/></fragment></page></CDXML>",
    )
    .expect("isotope 32767 is representable");
    assert_eq!(node_integer(&document, PropertyKind::Isotope), Some(32767));
    for isotope in [-1, 32768] {
        let error = cdxml::read(&format!(
            "<CDXML><page id=\"1\"><fragment id=\"2\"><n id=\"3\" Isotope=\"{isotope}\"/></fragment></page></CDXML>"
        ))
        .unwrap_err();
        assert_eq!(error.code, "invalid_document", "{isotope}");
    }
}

#[test]
fn reader_decodes_signed_bytes_and_refuses_malformed_widths() {
    let negative = cdx::read(&node_with(&[(0x0421, vec![0x80])]))
        .unwrap()
        .document;
    assert_eq!(node_integer(&negative, PropertyKind::Charge), Some(-128));
    // 0xFFFE is -2 as a signed 16-bit isotope; the model refuses it, not a wrapped 65534.
    let error = cdx::read(&node_with(&[(0x0420, (-2i16).to_le_bytes().to_vec())]))
        .expect_err("a negative isotope is outside the common model");
    assert_eq!(error.code, "invalid_document");
    let wide = cdx::read(&node_with(&[(0x0421, vec![0, 0, 0, 0x7f])]))
        .unwrap()
        .document;
    assert_eq!(node_integer(&wide, PropertyKind::Charge), Some(127));

    for (tag, value) in [
        (0x0421, vec![1, 2]),
        (0x0421, vec![1, 2, 3]),
        (0x0420, vec![1]),
        (0x0420, vec![1, 2, 3]),
        (0x0402, Vec::new()),
        (0x0200, vec![0; 7]),
    ] {
        let error = cdx::read(&node_with(&[(tag, value.clone())]))
            .expect_err("a malformed field width must be refused");
        assert!(
            error.code.starts_with("invalid_") || error.code == "malformed_property",
            "{tag:#06x} {value:?}: {}",
            error.code
        );
    }
}

#[test]
fn truncating_inside_a_signed_field_is_refused() {
    let bytes = node_with(&[(0x0420, (-2i16).to_le_bytes().to_vec())]);
    let value_start = bytes.len() - 8 - 2;
    for cut in value_start..=value_start + 1 {
        assert!(cdx::read(&bytes[..cut]).is_err(), "cut at {cut}");
    }
}
