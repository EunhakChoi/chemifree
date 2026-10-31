use chemifree_application::LossDisposition;
use chemifree_application::interchange_codec::{InterchangeCodec, InterchangeFormat};
use chemifree_domain::interchange::{
    Content, Document, ObjectKind, Property, PropertyKind, PropertyValue, XmlName,
};
use chemifree_providers::interchange_codec::LocalInterchangeCodec;

const SOURCE: &str = include_str!("data/cdxml/multi-page.cdxml");

#[test]
fn original_is_reparsed_and_opaque_reference_loss_needs_explicit_consent() {
    let original = SOURCE.replace("<page id=\"1\">", "<page id=\"1\" future=\"4\">");
    let decoded = LocalInterchangeCodec
        .read(InterchangeFormat::Cdxml, original.as_bytes())
        .unwrap();
    assert_eq!(
        decoded.loss_report[0].disposition,
        LossDisposition::Retained
    );
    let unchanged = LocalInterchangeCodec
        .write(
            InterchangeFormat::Cdxml,
            &decoded.document,
            Some(original.as_bytes()),
            false,
        )
        .unwrap();
    assert_eq!(unchanged.bytes, original.as_bytes());

    // A client can submit a freshly reconstructed tree with no baseline. The writer
    // must compare it with the re-parsed original instead of trusting that tree.
    let mut root = decoded.document.root().clone();
    let page = root
        .children
        .iter_mut()
        .find_map(|child| match child {
            Content::Object(object) if object.kind == ObjectKind::Page => Some(object),
            _ => None,
        })
        .unwrap();
    page.id = Some(11);
    let changed = Document::new(root).unwrap();
    let before = changed.clone();
    let error = LocalInterchangeCodec
        .write(
            InterchangeFormat::Cdxml,
            &changed,
            Some(original.as_bytes()),
            false,
        )
        .unwrap_err();
    assert_eq!(error.code, "unsafe_unknown_rewrite");
    assert_eq!(changed, before);
    let accepted = LocalInterchangeCodec
        .write(
            InterchangeFormat::Cdxml,
            &changed,
            Some(original.as_bytes()),
            true,
        )
        .unwrap();
    assert_eq!(
        accepted.loss_report[0].disposition,
        LossDisposition::Dropped
    );
    let text = std::str::from_utf8(&accepted.bytes).unwrap();
    assert!(!text.contains("future="));
    assert!(text.contains("<page id=\"11\""));
    assert_eq!(changed, before);

    // Accepting the loss also acts on an otherwise unchanged tree.
    let stripped = LocalInterchangeCodec
        .write(
            InterchangeFormat::Cdxml,
            &decoded.document,
            Some(original.as_bytes()),
            true,
        )
        .unwrap();
    assert_eq!(
        stripped.loss_report[0].disposition,
        LossDisposition::Dropped
    );
    assert!(
        !std::str::from_utf8(&stripped.bytes)
            .unwrap()
            .contains("future=")
    );
}

#[test]
fn outside_root_content_and_unknown_formats_are_not_silently_discarded() {
    let original = SOURCE.replacen("?>", "?>\n<!-- preserve me -->", 1);
    let decoded = LocalInterchangeCodec
        .read(InterchangeFormat::Cdxml, original.as_bytes())
        .unwrap();
    assert_eq!(
        decoded.loss_report[0].disposition,
        LossDisposition::Retained
    );
    let mut root = decoded.document.root().clone();
    root.id = Some(99);
    let changed = Document::new(root).unwrap();
    assert_eq!(
        LocalInterchangeCodec
            .write(
                InterchangeFormat::Cdxml,
                &changed,
                Some(original.as_bytes()),
                false
            )
            .unwrap_err()
            .code,
        "unsafe_unknown_rewrite"
    );
    let accepted = LocalInterchangeCodec
        .write(
            InterchangeFormat::Cdxml,
            &changed,
            Some(original.as_bytes()),
            true,
        )
        .unwrap();
    assert_eq!(
        accepted.loss_report[0].disposition,
        LossDisposition::Dropped
    );
    assert!(
        !std::str::from_utf8(&accepted.bytes)
            .unwrap()
            .contains("preserve me")
    );
    assert_eq!(
        LocalInterchangeCodec
            .read(InterchangeFormat::Cdx, b"ignored")
            .unwrap_err()
            .code,
        "unsupported_format"
    );
    assert_eq!(
        LocalInterchangeCodec
            .read(InterchangeFormat::Cdxml, &[0xff])
            .unwrap_err()
            .code,
        "invalid_utf8"
    );
}

#[test]
fn opaque_drop_reports_original_and_submitted_sets_separately() {
    let original = SOURCE.replace("<page id=\"1\">", "<page id=\"1\" future=\"4\">");
    let decoded = LocalInterchangeCodec
        .read(InterchangeFormat::Cdxml, original.as_bytes())
        .unwrap();
    let mut submitted = decoded.document.clone();
    assert_eq!(submitted.discard_opaque(), Ok(1));
    submitted
        .edit(|root| {
            for name in ["another", "different"] {
                root.properties.push(Property {
                    kind: PropertyKind::UnknownXml(XmlName {
                        namespace: None,
                        local: name.into(),
                    }),
                    value: PropertyValue::Text("uninterpreted".into()),
                });
            }
        })
        .unwrap();
    let result = LocalInterchangeCodec
        .write(
            InterchangeFormat::Cdxml,
            &submitted,
            Some(original.as_bytes()),
            true,
        )
        .unwrap();
    assert_eq!(result.loss_report[0].disposition, LossDisposition::Dropped);
    assert!(
        result.loss_report[0]
            .reason
            .contains("Original opaque entries: 1; submitted opaque entries: 2;")
    );
    assert!(result.loss_report[0].reason.contains("sets may overlap"));
    let text = std::str::from_utf8(&result.bytes).unwrap();
    assert!(
        !text.contains("future=") && !text.contains("another=") && !text.contains("different=")
    );
}
