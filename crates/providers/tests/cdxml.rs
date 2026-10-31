use chemifree_domain::interchange::{Content, Object, ObjectKind, PropertyKind, PropertyValue};
use chemifree_providers::cdxml;

const MULTI_PAGE: &str = include_str!("data/cdxml/multi-page.cdxml");

fn find(object: &Object, kind: ObjectKind) -> Vec<&Object> {
    let mut found = Vec::new();
    if object.kind == kind {
        found.push(object);
    }
    for child in &object.children {
        if let Content::Object(child) = child {
            found.extend(find(child, kind.clone()));
        }
    }
    found
}

fn first_mut(object: &mut Object, kind: ObjectKind) -> Option<&mut Object> {
    if object.kind == kind {
        return Some(object);
    }
    for child in &mut object.children {
        if let Content::Object(child) = child
            && let Some(found) = first_mut(child, kind.clone())
        {
            return Some(found);
        }
    }
    None
}

#[test]
fn preserves_pages_molecular_graph_positions_text_and_style() {
    let document = cdxml::read(MULTI_PAGE).unwrap();
    let root = document.document().root();
    assert_eq!(find(root, ObjectKind::Page).len(), 2);
    assert_eq!(find(root, ObjectKind::Fragment).len(), 2);
    let nodes = find(root, ObjectKind::Node);
    assert_eq!(nodes.len(), 3);
    assert_eq!(
        nodes[0]
            .properties
            .iter()
            .find(|p| p.kind == PropertyKind::Position)
            .unwrap()
            .value,
        PropertyValue::Point { x: 1.5, y: -2.0 }
    );
    assert_eq!(
        nodes[1]
            .properties
            .iter()
            .find(|p| p.kind == PropertyKind::Charge)
            .unwrap()
            .value,
        PropertyValue::Integer(-1)
    );
    assert_eq!(
        nodes[1]
            .properties
            .iter()
            .find(|p| p.kind == PropertyKind::Isotope)
            .unwrap()
            .value,
        PropertyValue::Integer(18)
    );
    let bond = find(root, ObjectKind::Bond)[0];
    assert_eq!(
        bond.properties
            .iter()
            .find(|p| p.kind == PropertyKind::BondBegin)
            .unwrap()
            .value,
        PropertyValue::ObjectReference(3)
    );
    assert_eq!(
        bond.properties
            .iter()
            .find(|p| p.kind == PropertyKind::BondEnd)
            .unwrap()
            .value,
        PropertyValue::ObjectReference(4)
    );
    assert_eq!(
        bond.properties
            .iter()
            .find(|p| p.kind == PropertyKind::BondOrder)
            .unwrap()
            .value,
        PropertyValue::Integer(2)
    );
    let run = find(root, ObjectKind::StyleRun)[0];
    assert_eq!(run.children, [Content::Text("α & β".into())]);
    assert_eq!(
        run.properties
            .iter()
            .find(|p| p.kind == PropertyKind::FontIndex)
            .unwrap()
            .value,
        PropertyValue::Integer(3)
    );
    assert_eq!(
        run.properties
            .iter()
            .find(|p| p.kind == PropertyKind::FontSize)
            .unwrap()
            .value,
        PropertyValue::Float(12.0)
    );
    assert_eq!(find(root, ObjectKind::Color).len(), 2);
    let graphic = find(root, ObjectKind::Graphic)[0];
    assert_eq!(graphic.id, Some(10));
    assert_eq!(
        graphic
            .properties
            .iter()
            .find(|p| p.kind == PropertyKind::GraphicType)
            .unwrap()
            .value,
        PropertyValue::Integer(1)
    );
    let reference = find(root, ObjectKind::Represents)[0];
    assert_eq!(
        reference
            .properties
            .iter()
            .find(|p| p.kind == PropertyKind::RepresentObject)
            .unwrap()
            .value,
        PropertyValue::ObjectReference(4)
    );
    assert_eq!(cdxml::write(&document).unwrap(), MULTI_PAGE);
}

#[test]
fn an_edit_without_extensions_writes_semantically_equivalent_unicode_xml() {
    let mut document = cdxml::read(MULTI_PAGE).unwrap();
    document
        .document_mut()
        .edit(|root| {
            let page = root
                .children
                .iter_mut()
                .find_map(|child| {
                    if let Content::Object(page) = child
                        && page.kind == ObjectKind::Page
                    {
                        Some(page)
                    } else {
                        None
                    }
                })
                .unwrap();
            page.id = Some(11);
        })
        .unwrap();
    let written = cdxml::write(&document).unwrap();
    let reloaded = cdxml::read(&written).unwrap();
    assert_eq!(document.document().root(), reloaded.document().root());
    assert!(written.contains("α &amp; β"));
}

#[test]
fn unknown_extensions_are_preserved_but_not_rewritten_after_an_edit() {
    let source = MULTI_PAGE.replace(
        "<page id=\"1\">",
        "<page id=\"1\" future=\"may-reference-9\">",
    );
    let mut document = cdxml::read(&source).unwrap();
    assert_eq!(cdxml::write(&document).unwrap(), source);
    document
        .document_mut()
        .edit(|root| {
            let page = root
                .children
                .iter_mut()
                .find_map(|child| {
                    if let Content::Object(page) = child
                        && page.kind == ObjectKind::Page
                    {
                        Some(page)
                    } else {
                        None
                    }
                })
                .unwrap();
            page.properties
                .push(chemifree_domain::interchange::Property {
                    kind: PropertyKind::BoundingBox,
                    value: PropertyValue::Rectangle {
                        left: 0.0,
                        top: 0.0,
                        right: 10.0,
                        bottom: 10.0,
                    },
                });
        })
        .unwrap();
    assert_eq!(
        cdxml::write(&document).unwrap_err().code,
        "unsafe_unknown_rewrite"
    );
    assert_eq!(document.document_mut().discard_opaque(), Ok(1));
    let rewritten = cdxml::write(&document).unwrap();
    assert!(!rewritten.contains("future="));
    assert!(rewritten.contains("BoundingBox=\"0 0 10 10\""));
}

#[test]
fn rejects_duplicate_or_dangling_ids_and_missing_font_references() {
    let duplicate = MULTI_PAGE.replace("<n id=\"4\"", "<n id=\"3\"");
    let error = cdxml::read(&duplicate).unwrap_err();
    assert_eq!(
        (error.code, error.message.as_str()),
        ("invalid_document", "DuplicateId(3)")
    );
    let dangling = MULTI_PAGE.replace("B=\"3\"", "B=\"99\"");
    let error = cdxml::read(&dangling).unwrap_err();
    assert_eq!(
        (error.code, error.message.as_str()),
        ("invalid_document", "DanglingReference(99)")
    );
    let bad_font = MULTI_PAGE.replace("font=\"3\"", "font=\"99\"");
    let error = cdxml::read(&bad_font).unwrap_err();
    assert_eq!(
        (error.code, error.message.as_str()),
        ("invalid_document", "UnknownFont(99)")
    );
    let bad_graphic_ref = MULTI_PAGE.replace("object=\"4\"", "object=\"99\"");
    let error = cdxml::read(&bad_graphic_ref).unwrap_err();
    assert_eq!(
        (error.code, error.message.as_str()),
        ("invalid_document", "DanglingReference(99)")
    );
}

#[test]
fn malformed_xml_and_entity_declarations_are_rejected() {
    let malformed = cdxml::read("<CDXML><page></CDXML>").unwrap_err();
    assert_eq!(malformed.code, "invalid_xml");
    assert!(malformed.line.is_some());
    let entity =
        "<!DOCTYPE CDXML [<!ENTITY x SYSTEM 'file:///etc/passwd'>]><CDXML><page>&x;</page></CDXML>";
    assert_eq!(cdxml::read(entity).unwrap_err().code, "unsupported_entity");
    let encoding = "<?xml version=\"1.0\" encoding=\"iso-8859-1\"?><CDXML><page/></CDXML>";
    assert_eq!(
        cdxml::read(encoding).unwrap_err().code,
        "unsupported_encoding"
    );
}

#[test]
fn external_doctypes_are_not_resolved_and_undefined_entities_are_rejected() {
    let system =
        "<!DOCTYPE CDXML SYSTEM 'file:///no-such-chemifree-resource.dtd'><CDXML><page/></CDXML>";
    let public = "<!DOCTYPE CDXML PUBLIC '-//example//DTD CDXML//EN' 'file:///no-such-chemifree-resource.dtd'><CDXML><page/></CDXML>";
    for source in [system, public] {
        let document = cdxml::read(source).unwrap();
        assert_eq!(cdxml::write(&document).unwrap(), source);
    }
    let undefined = "<!DOCTYPE CDXML SYSTEM 'file:///no-such-chemifree-resource.dtd'><CDXML><page><t><s>&missing;</s></t></page></CDXML>";
    assert_eq!(cdxml::read(undefined).unwrap_err().code, "invalid_xml");
}

#[test]
fn namespace_prefixes_and_attribute_order_do_not_change_known_semantics() {
    let simple = "<CDXML xmlns=\"urn:example\"><page id=\"1\"><fragment id=\"2\"><n id=\"3\" p=\"2 3\" Element=\"8\"/></fragment></page></CDXML>";
    let prefixed = "<x:CDXML xmlns:x=\"urn:example\"><x:page id=\"1\"><x:fragment id=\"2\"><x:n Element=\"8\" p=\"2 3\" id=\"3\"/></x:fragment></x:page></x:CDXML>";
    let first = cdxml::read(simple).unwrap();
    let second = cdxml::read(prefixed).unwrap();
    let a = find(first.document().root(), ObjectKind::Node)[0];
    let b = find(second.document().root(), ObjectKind::Node)[0];
    assert_eq!(a.id, b.id);
    for kind in [PropertyKind::Position, PropertyKind::AtomicNumber] {
        assert_eq!(
            a.properties.iter().find(|p| p.kind == kind),
            b.properties.iter().find(|p| p.kind == kind)
        );
    }
    let regenerated = cdxml::CdxmlDocument::from_document(first.document().clone());
    let xml = cdxml::write(&regenerated).unwrap();
    assert!(xml.contains("xmlns=\"urn:example\""));
    assert_eq!(
        cdxml::read(&xml).unwrap().document().root(),
        first.document().root()
    );
}

#[test]
fn entity_like_text_in_cdata_is_not_a_dtd_declaration() {
    let source = "<CDXML><page><t><s><![CDATA[<!ENTITY x>]]></s></t></page></CDXML>";
    let document = cdxml::read(source).unwrap();
    let run = find(document.document().root(), ObjectKind::StyleRun)[0];
    assert_eq!(run.children, [Content::Text("<!ENTITY x>".into())]);
}

#[test]
fn refuses_excessive_nesting_before_recursive_conversion() {
    let source = format!("<CDXML><page>{}<t/></page></CDXML>", "<group>".repeat(130))
        + &"</group>".repeat(130);
    let error = cdxml::read(&source).unwrap_err();
    assert_eq!(error.code, "invalid_xml");
    let source = format!(
        "<CDXML><page>{}<t/>{}</page></CDXML>",
        "<group>".repeat(130),
        "</group>".repeat(130)
    );
    let error = cdxml::read(&source).unwrap_err();
    assert_eq!(error.code, "resource_limit");
}

#[test]
fn unknown_subtrees_keep_opaque_ids_without_assuming_their_type() {
    let source =
        "<CDXML><page id=\"1\"><future id=\"uninterpreted\"><n id=\"x\"/></future></page></CDXML>";
    let document = cdxml::read(source).unwrap();
    assert_eq!(cdxml::write(&document).unwrap(), source);
}

#[test]
fn a_bond_cannot_reference_a_node_in_a_different_fragment() {
    let source = "<CDXML><page><fragment id=\"1\"><n id=\"2\"/><b id=\"3\" B=\"2\" E=\"5\"/></fragment><fragment id=\"4\"><n id=\"5\"/></fragment></page></CDXML>";
    let error = cdxml::read(source).unwrap_err();
    assert_eq!(
        (error.code, error.message.as_str()),
        ("invalid_document", "CrossFragmentBond(5)")
    );
}

#[test]
fn refuses_misplaced_known_objects_and_text_without_a_style_run() {
    let misplaced = "<CDXML><page><page/></page></CDXML>";
    assert_eq!(
        cdxml::read(misplaced).unwrap_err().message,
        "InvalidContainment"
    );
    let no_style = "<CDXML><page><t>unstructured</t></page></CDXML>";
    assert_eq!(
        cdxml::read(no_style).unwrap_err().message,
        "MissingTextStyle"
    );
}

#[test]
fn node_types_use_the_published_shared_enum_and_unknown_values_remain_opaque() {
    let source =
        "<CDXML><page><fragment><n id=\"1\" NodeType=\"LinkNode\"/></fragment></page></CDXML>";
    let mut document = cdxml::read(source).unwrap();
    let node = find(document.document().root(), ObjectKind::Node)[0];
    assert_eq!(node.properties[0].kind, PropertyKind::NodeType);
    assert_eq!(node.properties[0].value, PropertyValue::Integer(13));
    document
        .document_mut()
        .edit(|root| {
            first_mut(root, ObjectKind::Node).unwrap().id = Some(2);
        })
        .unwrap();
    assert!(
        cdxml::write(&document)
            .unwrap()
            .contains("NodeType=\"LinkNode\"")
    );

    let future = source.replace("LinkNode", "FutureKind");
    let mut opaque = cdxml::read(&future).unwrap();
    let before = opaque.document().clone();
    assert!(
        opaque
            .document_mut()
            .edit(|root| {
                first_mut(root, ObjectKind::Node).unwrap().id = Some(2);
            })
            .is_err()
    );
    assert_eq!(opaque.document(), &before);
    assert_eq!(cdxml::write(&opaque).unwrap(), future);
}

#[test]
fn refuses_to_misrepresent_cdx_unspecified_order_as_an_xml_single_bond() {
    let mut document = cdxml::read(MULTI_PAGE).unwrap();
    document
        .document_mut()
        .edit(|root| {
            let bond = first_mut(root, ObjectKind::Bond).unwrap();
            bond.properties
                .iter_mut()
                .find(|p| p.kind == PropertyKind::BondOrder)
                .unwrap()
                .value = PropertyValue::Integer(0xffff);
        })
        .unwrap();
    assert_eq!(
        cdxml::write(&document).unwrap_err().code,
        "unsupported_property"
    );
}

#[test]
fn edited_text_rejects_characters_forbidden_in_xml() {
    let mut document = cdxml::read(MULTI_PAGE).unwrap();
    document
        .document_mut()
        .edit(|root| {
            first_mut(root, ObjectKind::StyleRun).unwrap().children =
                vec![Content::Text("unsafe\u{1}character".into())];
        })
        .unwrap();
    assert_eq!(
        cdxml::write(&document).unwrap_err().code,
        "invalid_xml_text"
    );
}

#[test]
fn edited_text_preserves_a_carriage_return_across_xml_normalization() {
    let mut document = cdxml::read(MULTI_PAGE).unwrap();
    document
        .document_mut()
        .edit(|root| {
            first_mut(root, ObjectKind::StyleRun).unwrap().children =
                vec![Content::Text("α\rβ".into())];
        })
        .unwrap();
    let written = cdxml::write(&document).unwrap();
    assert!(written.contains("&#13;"));
    let reloaded = cdxml::read(&written).unwrap();
    assert_eq!(
        find(reloaded.document().root(), ObjectKind::StyleRun)[0].children,
        [Content::Text("α\rβ".into())]
    );
}

#[test]
fn outside_root_processing_instructions_are_not_mistaken_for_xml_declarations() {
    let source = "<?xml-stylesheet href=\"local.css\"?><CDXML><page/></CDXML>";
    let mut document = cdxml::read(source).unwrap();
    assert_eq!(cdxml::write(&document).unwrap(), source);
    document
        .document_mut()
        .edit(|root| root.id = Some(1))
        .unwrap();
    assert_eq!(
        cdxml::write(&document).unwrap_err().code,
        "unsafe_prolog_rewrite"
    );
}

#[test]
fn utf8_bom_is_preserved_and_is_not_an_opaque_instruction() {
    let source =
        "\u{feff}<?xml version=\"1.0\" encoding=\"UTF-8\"?><CDXML><page id=\"1\"/></CDXML>";
    let mut document = cdxml::read(source).unwrap();
    assert!(!document.has_unrewritable_prolog());
    assert_eq!(
        cdxml::write(&document).unwrap().as_bytes(),
        source.as_bytes()
    );
    document
        .document_mut()
        .edit(|root| first_mut(root, ObjectKind::Page).unwrap().id = Some(2))
        .unwrap();
    let rewritten = cdxml::write(&document).unwrap();
    assert!(rewritten.starts_with('\u{feff}'));
    let reloaded = cdxml::read(&rewritten).unwrap();
    assert_eq!(
        find(reloaded.document().root(), ObjectKind::Page)[0].id,
        Some(2)
    );
}
