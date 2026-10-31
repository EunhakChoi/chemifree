use chemifree_application::interchange_codec::{InterchangeCodec, InterchangeFormat};
use chemifree_domain::interchange::{Content, ObjectKind, ValidationError};
use chemifree_providers::cdxml;
use chemifree_providers::interchange_codec::LocalInterchangeCodec;

// Public FontTable and ColorTable object specifications require at least one
// corresponding entry. Neither comments nor unrelated extensions provide one.
fn assert_empty_table_rejected(table: &str, diagnostic: &str) {
    let source = format!("<CDXML><{table}/><page id=\"1\"/></CDXML>");
    let error = LocalInterchangeCodec
        .read(InterchangeFormat::Cdxml, source.as_bytes())
        .expect_err("an empty CDXML table has no required entry");
    assert_eq!(error.code, "invalid_document");
    assert!(error.message.contains(diagnostic));
}

#[test]
fn empty_font_table_is_rejected() {
    assert_empty_table_rejected("fonttable", "EmptyFontTable");
}

#[test]
fn empty_color_table_is_rejected() {
    assert_empty_table_rejected("colortable", "EmptyColorTable");
}

#[test]
fn absent_color_table_uses_standard_defaults_and_real_entries_remain_valid() {
    for color in [0, 1, 2, 3] {
        let source =
            format!("<CDXML><page id=\"1\"><t><s color=\"{color}\">x</s></t></page></CDXML>");
        assert!(
            LocalInterchangeCodec
                .read(InterchangeFormat::Cdxml, source.as_bytes())
                .is_ok()
        );
    }
    let source = r#"<CDXML><fonttable><font id="3" name="Serif"/></fonttable><colortable><color r="1" g="1" b="1"/></colortable><page id="1"/></CDXML>"#;
    assert!(
        LocalInterchangeCodec
            .read(InterchangeFormat::Cdxml, source.as_bytes())
            .is_ok()
    );
}

#[test]
fn unknown_or_comment_children_are_not_entries_and_failed_edits_are_atomic() {
    let source = r#"<CDXML><fonttable><!--note--><future/></fonttable><page id="1"/></CDXML>"#;
    let error = LocalInterchangeCodec
        .read(InterchangeFormat::Cdxml, source.as_bytes())
        .unwrap_err();
    assert!(error.message.contains("EmptyFontTable"));

    let source = r#"<CDXML><fonttable><font id="3" name="Serif"/></fonttable><colortable><color r="1" g="1" b="1"/></colortable><page id="1"/></CDXML>"#;
    let mut document = cdxml::read(source).unwrap().document().clone();
    for (table_kind, expected) in [
        (ObjectKind::FontTable, ValidationError::EmptyFontTable),
        (ObjectKind::ColorTable, ValidationError::EmptyColorTable),
    ] {
        let before = document.clone();
        let result = document.edit(|root| {
            let table = root
                .children
                .iter_mut()
                .find_map(|child| match child {
                    Content::Object(object) if object.kind == table_kind => Some(object),
                    _ => None,
                })
                .unwrap();
            table.children.clear();
        });
        assert_eq!(result, Err(expected));
        assert_eq!(document, before);
    }
}
