use chemifree_application::interchange_codec::{InterchangeCodec, InterchangeFormat};
use chemifree_domain::interchange::{Content, Document, ObjectKind};
use chemifree_providers::interchange_codec::LocalInterchangeCodec;

// BracketedGroup is a public CDXML object with an id and RepeatCount, but it
// remains opaque in this profile. A graphic may represent another object's
// property; see the public BracketedGroup and Represents specifications.
const SOURCE: &str = r#"<CDXML><page id="1"><bracketedgroup id="4" RepeatCount="3"/><graphic id="3" BoundingBox="0 0 1 1"><represent object="4" attribute="RepeatCount"/></graphic></page></CDXML>"#;

#[test]
fn a_reference_to_an_existing_opaque_object_can_be_preserved_unchanged() {
    let decoded = LocalInterchangeCodec
        .read(InterchangeFormat::Cdxml, SOURCE.as_bytes())
        .expect("the target exists, although its other properties are opaque");
    let unchanged = LocalInterchangeCodec
        .write(
            InterchangeFormat::Cdxml,
            &decoded.document,
            Some(SOURCE.as_bytes()),
            false,
        )
        .expect("the original retains unknown reference semantics");
    assert_eq!(unchanged.bytes, SOURCE.as_bytes());
}

#[test]
fn an_absent_target_still_fails_reference_validation() {
    let source = SOURCE.replace("object=\"4\"", "object=\"9\"");
    let error = LocalInterchangeCodec
        .read(InterchangeFormat::Cdxml, source.as_bytes())
        .unwrap_err();
    assert!(error.message.contains("DanglingReference(9)"));
}

#[test]
fn an_opaque_target_cannot_duplicate_a_known_object_id() {
    let source = SOURCE.replace("id=\"4\"", "id=\"1\"");
    let error = LocalInterchangeCodec
        .read(InterchangeFormat::Cdxml, source.as_bytes())
        .unwrap_err();
    assert!(error.message.contains("DuplicateId(1)"));
}

#[test]
fn discarding_a_referenced_opaque_object_is_atomic_and_requires_resolving_the_reference() {
    let source = format!("\u{feff}{SOURCE}");
    let decoded = LocalInterchangeCodec
        .read(InterchangeFormat::Cdxml, source.as_bytes())
        .unwrap();
    let before = decoded.document.clone();
    let error = LocalInterchangeCodec
        .write(
            InterchangeFormat::Cdxml,
            &decoded.document,
            Some(source.as_bytes()),
            true,
        )
        .unwrap_err();
    assert!(error.message.contains("DanglingReference(4)"));
    assert_eq!(decoded.document, before);

    let mut root = before.root().clone();
    let page = root
        .children
        .iter_mut()
        .find_map(|child| match child {
            Content::Object(object) if object.kind == ObjectKind::Page => Some(object),
            _ => None,
        })
        .unwrap();
    let graphic = page
        .children
        .iter_mut()
        .find_map(|child| match child {
            Content::Object(object) if object.kind == ObjectKind::Graphic => Some(object),
            _ => None,
        })
        .unwrap();
    graphic.children.clear(); // The caller deliberately removes the known reference.
    let resolved = Document::new(root).unwrap();
    let written = LocalInterchangeCodec
        .write(
            InterchangeFormat::Cdxml,
            &resolved,
            Some(source.as_bytes()),
            true,
        )
        .unwrap();
    let xml = std::str::from_utf8(&written.bytes).unwrap();
    assert!(xml.starts_with('\u{feff}'));
    assert!(!xml.contains("bracketedgroup"));
    assert!(!xml.contains("represent"));
    assert_eq!(written.loss_report.len(), 1);
    assert_eq!(decoded.document, before);
}

#[test]
fn foreign_and_nonnumeric_extension_ids_are_not_guessed() {
    let foreign = SOURCE
        .replace("<CDXML>", "<CDXML xmlns:x=\"urn:extension\">")
        .replace("bracketedgroup", "x:future");
    let error = LocalInterchangeCodec
        .read(InterchangeFormat::Cdxml, foreign.as_bytes())
        .unwrap_err();
    assert_eq!(error.code, "unsupported_opaque_reference");
    assert!(error.message.contains('4'));

    // A numeric foreign identifier also makes the reference ambiguous when an
    // interpreted object uses the same number; it cannot be silently preferred.
    let collision = foreign.replace(
        "<x:future id=\"4\" RepeatCount=\"3\"/>",
        "<fragment id=\"2\"><n id=\"4\" p=\"0 0\"/></fragment><x:future id=\"4\" RepeatCount=\"3\"/>",
    );
    let error = LocalInterchangeCodec
        .read(InterchangeFormat::Cdxml, collision.as_bytes())
        .unwrap_err();
    assert_eq!(error.code, "unsupported_opaque_reference");

    // Descendants of opaque objects need not be CDXML objects even when they
    // happen to use its namespace and an attribute named id.
    let nested = SOURCE.replace(
        "<bracketedgroup id=\"4\" RepeatCount=\"3\"/>",
        "<future id=\"10\"><payload id=\"4\"/></future>",
    );
    let error = LocalInterchangeCodec
        .read(InterchangeFormat::Cdxml, nested.as_bytes())
        .unwrap_err();
    assert_eq!(error.code, "unsupported_opaque_reference");

    // A numeric spelling outside this profile's integer-ID syntax is retained
    // as opaque data and cannot be claimed absent when it may mean the target.
    let decimal = SOURCE.replace("id=\"4\"", "id=\"4.0\"");
    let error = LocalInterchangeCodec
        .read(InterchangeFormat::Cdxml, decimal.as_bytes())
        .unwrap_err();
    assert_eq!(error.code, "unsupported_opaque_reference");

    let unrelated = r#"<CDXML xmlns:x="urn:extension"><page id="1"><x:future id="custom"/><future id="not-a-number"/></page></CDXML>"#;
    let decoded = LocalInterchangeCodec
        .read(InterchangeFormat::Cdxml, unrelated.as_bytes())
        .unwrap();
    let written = LocalInterchangeCodec
        .write(
            InterchangeFormat::Cdxml,
            &decoded.document,
            Some(unrelated.as_bytes()),
            false,
        )
        .unwrap();
    assert_eq!(written.bytes, unrelated.as_bytes());
}
