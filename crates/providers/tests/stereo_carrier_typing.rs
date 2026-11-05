//! Known stereo fields remain editable without weakening opaque-extension preservation.

mod common;

use chemifree_application::interchange_codec::{InterchangeCodec, InterchangeFormat};
use chemifree_domain::interchange::{
    Content, Document, Object, ObjectKind, Property, PropertyKind, PropertyValue, ValidationError,
    XmlName,
};
use chemifree_providers::interchange_codec::LocalInterchangeCodec;

const SOURCE: &str = "<CDXML><page id=\"1\"><fragment id=\"2\"><n id=\"3\" p=\"0 0\" Element=\"6\" EnhancedStereoType=\"Or\" EnhancedStereoGroupNum=\"258\" ShowAtomEnhancedStereo=\"yes\"/><n id=\"4\" p=\"10 0\" Element=\"9\"/><b id=\"5\" B=\"3\" E=\"4\" Order=\"1\" Display=\"WedgeBegin\"/></fragment></page></CDXML>";

fn carriers(object: &Object) -> Vec<Property> {
    let mut result = Vec::new();
    for property in &object.properties {
        if matches!(
            property.kind,
            PropertyKind::BondDisplay
                | PropertyKind::EnhancedStereoType
                | PropertyKind::EnhancedStereoGroupNum
                | PropertyKind::ShowAtomEnhancedStereo
        ) {
            result.push(property.clone());
        }
    }
    for child in &object.children {
        if let Content::Object(child) = child {
            result.extend(carriers(child));
        }
    }
    result
}

fn integer(kind: PropertyKind, value: i64) -> Property {
    Property {
        kind,
        value: PropertyValue::Integer(value),
    }
}

fn change_show(object: &mut Object, value: i64) {
    for property in &mut object.properties {
        if property.kind == PropertyKind::ShowAtomEnhancedStereo {
            property.value = PropertyValue::Integer(value);
        }
    }
    for child in &mut object.children {
        if let Content::Object(child) = child {
            change_show(child, value);
        }
    }
}

#[test]
fn known_stereo_fields_have_the_same_typed_values_in_both_formats() {
    let codec = LocalInterchangeCodec;
    let parsed = codec
        .read(InterchangeFormat::Cdxml, SOURCE.as_bytes())
        .unwrap();
    let expected = vec![
        integer(PropertyKind::EnhancedStereoType, 3),
        integer(PropertyKind::EnhancedStereoGroupNum, 258),
        integer(PropertyKind::ShowAtomEnhancedStereo, 1),
        integer(PropertyKind::BondDisplay, 6),
    ];
    assert_eq!(carriers(parsed.document.root()), expected);
    for format in [InterchangeFormat::Cdxml, InterchangeFormat::Cdx] {
        let mut edited = parsed.document.clone();
        edited.edit(|root| change_show(root, 0)).unwrap();
        let written = codec.write(format, &edited, None, false).unwrap();
        assert!(written.loss_report.is_empty(), "{:?}", written.loss_report);
        let reread = codec.read(format, &written.bytes).unwrap();
        let mut expected = expected.clone();
        expected[2] = integer(PropertyKind::ShowAtomEnhancedStereo, 0);
        assert_eq!(carriers(reread.document.root()), expected, "{format:?}");
    }
}

#[test]
fn inherited_or_unrecognized_fields_stay_opaque_and_preserve_original_bytes() {
    let codec = LocalInterchangeCodec;
    let xml = SOURCE.replace("<CDXML>", "<CDXML ShowAtomEnhancedStereo=\"yes\">");
    let binary = common::file(
        0,
        &[common::prop(0x0445, &[255]), common::obj(0x8001, 1, &[])],
    );
    for (format, bytes) in [
        (InterchangeFormat::Cdxml, xml.as_bytes()),
        (InterchangeFormat::Cdx, binary.as_slice()),
    ] {
        let parsed = codec.read(format, bytes).unwrap();
        assert!(matches!(
            parsed.document.root().properties[0].kind,
            PropertyKind::UnknownXml(_) | PropertyKind::UnknownBinary(0x0445)
        ));
        assert_eq!(
            codec
                .write(format, &parsed.document, Some(bytes), false)
                .unwrap()
                .bytes,
            bytes
        );
        let mut edited = parsed.document.clone();
        edited
            .edit(|root| {
                root.properties.push(Property {
                    kind: PropertyKind::BoundingBox,
                    value: PropertyValue::Rectangle {
                        left: 0.0,
                        top: 0.0,
                        right: 20.0,
                        bottom: 20.0,
                    },
                })
            })
            .unwrap();
        assert_eq!(
            codec
                .write(format, &edited, Some(bytes), false)
                .unwrap_err()
                .code,
            "unsafe_unknown_rewrite"
        );
    }
    for value in ["Future", "yes "] {
        let xml = SOURCE.replace(
            "ShowAtomEnhancedStereo=\"yes\"",
            &format!("ShowAtomEnhancedStereo=\"{value}\""),
        );
        let parsed = codec
            .read(InterchangeFormat::Cdxml, xml.as_bytes())
            .unwrap();
        assert_eq!(carriers(parsed.document.root()).len(), 3);
        assert_eq!(
            codec
                .write(
                    InterchangeFormat::Cdxml,
                    &parsed.document,
                    Some(xml.as_bytes()),
                    false
                )
                .unwrap()
                .bytes,
            xml.as_bytes()
        );
    }
}

#[test]
fn cdx_boolean_normalizes_nonzero_bytes_but_malformed_widths_remain_opaque() {
    let codec = LocalInterchangeCodec;
    for (bytes, expected) in [
        (&[0][..], Some(0)),
        (&[1], Some(1)),
        (&[255], Some(1)),
        (&[], None),
        (&[1, 0], None),
    ] {
        let node = common::obj(
            0x8004,
            3,
            &[
                common::prop(0x0200, &common::point(0.0, 0.0)),
                common::prop(0x0402, &[6, 0]),
                common::prop(0x0445, bytes),
            ],
        );
        let source = common::file(
            0,
            &[common::obj(0x8001, 1, &[common::obj(0x8003, 2, &[node])])],
        );
        let parsed = codec.read(InterchangeFormat::Cdx, &source).unwrap();
        let expected: Vec<Property> = expected
            .into_iter()
            .map(|value| integer(PropertyKind::ShowAtomEnhancedStereo, value))
            .collect();
        assert_eq!(carriers(parsed.document.root()), expected, "{bytes:?}");
        assert_eq!(
            codec
                .write(
                    InterchangeFormat::Cdx,
                    &parsed.document,
                    Some(&source),
                    false
                )
                .unwrap()
                .bytes,
            source,
            "{bytes:?}"
        );
    }
}

#[test]
fn typed_fields_reject_bad_values_owners_and_colliding_aliases() {
    for property in [
        integer(PropertyKind::BondDisplay, 6),
        integer(PropertyKind::EnhancedStereoType, 3),
        integer(
            PropertyKind::EnhancedStereoGroupNum,
            i64::from(i32::MAX) + 1,
        ),
        integer(PropertyKind::ShowAtomEnhancedStereo, 2),
    ] {
        let root = Object {
            kind: ObjectKind::Document,
            id: None,
            properties: vec![property],
            children: vec![Content::Object(Object {
                kind: ObjectKind::Page,
                id: Some(1),
                properties: Vec::new(),
                children: Vec::new(),
            })],
        };
        assert_eq!(
            Document::new(root).unwrap_err(),
            ValidationError::InvalidProperty
        );
    }
    let codec = LocalInterchangeCodec;
    let mut parsed = codec
        .read(InterchangeFormat::Cdxml, SOURCE.as_bytes())
        .unwrap()
        .document;
    assert_eq!(
        parsed.edit(|root| change_show(root, 2)).unwrap_err(),
        ValidationError::InvalidProperty
    );
    let error = parsed
        .edit(|root| {
            fn add_alias(object: &mut Object) {
                if object.kind == ObjectKind::Node {
                    object.properties.push(Property {
                        kind: PropertyKind::UnknownXml(XmlName {
                            namespace: None,
                            local: "ShowAtomEnhancedStereo".into(),
                        }),
                        value: PropertyValue::Text("no".into()),
                    });
                    return;
                }
                for child in &mut object.children {
                    if let Content::Object(child) = child {
                        add_alias(child);
                    }
                }
            }
            add_alias(root);
        })
        .unwrap_err();
    assert_eq!(error, ValidationError::DuplicateProperty);
}

#[test]
fn signed_xml_group_numbers_refuse_binary_truncation() {
    let codec = LocalInterchangeCodec;
    for value in [-1, 65536, i32::MAX] {
        let source = SOURCE.replace("258", &value.to_string());
        let parsed = codec
            .read(InterchangeFormat::Cdxml, source.as_bytes())
            .unwrap();
        assert_eq!(
            codec
                .write(InterchangeFormat::Cdx, &parsed.document, None, false)
                .unwrap_err()
                .code,
            "out_of_range"
        );
    }
}
