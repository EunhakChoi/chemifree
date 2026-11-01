//! CDX writer acceptance tests.

mod common;

use chemifree_application::interchange_codec::{
    InterchangeCodec, InterchangeFormat, InterchangeWrite,
};
use chemifree_application::{Loss, LossDisposition};
use chemifree_domain::interchange::{
    Content, Document, Object, ObjectKind, Property, PropertyKind, PropertyValue,
};
use chemifree_providers::cdx;
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use common::*;

const FONT: &str = r#"<fonttable><font id="3" charset="iso-8859-1" name="Arial"/></fonttable>"#;

fn from_cdxml(text: &str) -> Document {
    LocalInterchangeCodec
        .read(InterchangeFormat::Cdxml, text.as_bytes())
        .unwrap()
        .document
}

fn cdx_of(
    document: &Document,
    original: Option<&[u8]>,
    discard: bool,
) -> Result<InterchangeWrite, String> {
    LocalInterchangeCodec
        .write(InterchangeFormat::Cdx, document, original, discard)
        .map_err(|error| format!("{}: {}", error.code, error.message))
}

fn written(text: &str) -> InterchangeWrite {
    cdx_of(&from_cdxml(text), None, false).unwrap()
}

fn reread(bytes: &[u8]) -> Document {
    LocalInterchangeCodec
        .read(InterchangeFormat::Cdx, bytes)
        .unwrap()
        .document
}

fn features(losses: &[Loss]) -> Vec<(&str, LossDisposition)> {
    losses
        .iter()
        .map(|loss| (loss.feature.as_str(), loss.disposition))
        .collect()
}

#[test]
fn rich_file_is_written_back_to_the_same_bytes() {
    let original = rich_file();
    let file = cdx::read(&original).unwrap();
    let written = cdx::write(file.document(), Some(&file)).unwrap();
    assert_eq!(written.bytes, original);
    assert!(written.losses.is_empty(), "{:?}", written.losses);
}

#[test]
fn a_root_without_an_id_is_still_framed_as_header_plus_document_object() {
    let document = from_cdxml("<CDXML><page id=\"1\"/></CDXML>");
    let bytes = cdx_of(&document, None, false).unwrap().bytes;
    assert_eq!(&bytes[..28], &HEADER);
    assert_eq!(
        &bytes[28..34],
        &[0x00, 0x80, 0, 0, 0, 0],
        "tag 0x8000 and ID 0"
    );
    assert_eq!(
        &bytes[bytes.len() - 4..],
        &[0, 0, 0, 0],
        "document end and file end"
    );
    assert_eq!(reread(&bytes), document);
    let with_id = from_cdxml("<CDXML id=\"9\"><page id=\"1\"/></CDXML>");
    let bytes = cdx_of(&with_id, None, false).unwrap().bytes;
    assert_eq!(&bytes[28..34], &[0x00, 0x80, 9, 0, 0, 0]);
    assert_eq!(reread(&bytes), with_id);
}

#[test]
fn every_supported_property_kind_is_reread_unchanged() {
    let text = format!(
        r##"<CDXML BoundingBox="0 0 100 200"><colortable><color r="1" g="1" b="1"/><color r="0" g="1" b="1"/></colortable>{FONT}
<page id="1" BoundingBox="1 2 3 4"><group id="2" BoundingBox="5 6 7 8"><fragment id="3" BoundingBox="1.5 2.25 -3 4">
<n id="4" p="-10.5 20.25" NodeType="Element" Element="8" Isotope="18" Charge="-1" Radical="Doublet"/>
<n id="5" p="30 40" NodeType="Nickname"/><n id="6" p="1 1" NodeType="Unspecified"/>
<b id="7" B="4" E="5" Order="1 2"/><b id="8" B="5" E="6" Order="dative"/><b id="9" B="4" E="6" Order="1.5"/></fragment></group>
<t id="10" p="3 4" BoundingBox="1 2 3 4"><s font="3" size="9.5" face="5" color="2">Hé</s><s font="3" size="12" face="0" color="3">llo</s></t>
<graphic id="11" GraphicType="Rectangle" BoundingBox="0 0 10 10"><represent object="4" attribute="Charge"/></graphic></page></CDXML>"##
    );
    let document = from_cdxml(&text);
    let output = cdx_of(&document, None, false).unwrap();
    assert!(
        output
            .loss_report
            .iter()
            .all(|loss| loss.feature == "font_platform"),
        "{:?}",
        output.loss_report
    );
    assert_eq!(reread(&output.bytes), document);
}

#[test]
fn bond_order_bitsets_including_unspecified_survive_the_binary_format() {
    for bits in [1_i64, 3, 0x1000, 0x8000, 0xffff] {
        let mut document = from_cdxml(
            "<CDXML><page id=\"1\"><fragment id=\"2\"><n id=\"3\"/><n id=\"4\"/><b id=\"5\" B=\"3\" E=\"4\"/></fragment></page></CDXML>",
        );
        document
            .edit(|root| {
                let Content::Object(page) = &mut root.children[0] else {
                    panic!()
                };
                let Content::Object(fragment) = &mut page.children[0] else {
                    panic!()
                };
                let Content::Object(bond) = &mut fragment.children[2] else {
                    panic!()
                };
                bond.properties.push(Property {
                    kind: PropertyKind::BondOrder,
                    value: PropertyValue::Integer(bits),
                });
            })
            .unwrap();
        let bytes = cdx_of(&document, None, false).unwrap().bytes;
        assert_eq!(reread(&bytes), document, "bits {bits:#x}");
    }
}

#[test]
fn values_the_reader_refuses_are_refused_by_the_writer() {
    let base = "<CDXML><page id=\"1\"><fragment id=\"2\"><n id=\"3\" p=\"1 2\"/></fragment></page></CDXML>";
    let mut document = from_cdxml(base);
    document
        .edit(|root| {
            let Content::Object(page) = &mut root.children[0] else {
                panic!()
            };
            let Content::Object(fragment) = &mut page.children[0] else {
                panic!()
            };
            let Content::Object(node) = &mut fragment.children[0] else {
                panic!()
            };
            node.properties.push(Property {
                kind: PropertyKind::Radical,
                value: PropertyValue::Integer(9),
            });
        })
        .unwrap();
    let error = cdx_of(&document, None, false).unwrap_err();
    assert!(error.starts_with("unsupported_property"), "{error}");
}

#[test]
fn font_platform_loss_is_reported_exactly_when_the_source_platform_is_not_known() {
    let text = format!("<CDXML>{FONT}<page id=\"1\"/></CDXML>");
    // CDXML has no platform.
    assert_eq!(
        features(&written(&text).loss_report),
        [("font_platform", LossDisposition::Normalized)]
    );
    // A plain read of a platform-1 file reports nothing and an unchanged rewrite stays silent.
    let platform_one = file(
        0,
        &[
            prop(0x0100, &font_table(1, &[(3, 1252, "Arial")])),
            obj(0x8001, 1, &[]),
        ],
    );
    let read = LocalInterchangeCodec
        .read(InterchangeFormat::Cdx, &platform_one)
        .unwrap();
    assert!(read.loss_report.is_empty());
    let silent = cdx_of(&read.document, Some(&platform_one), false).unwrap();
    assert!(silent.loss_report.is_empty());
    assert_eq!(silent.bytes, platform_one);
    // An edit that leaves the tables alone keeps the known platform.
    let mut edited = read.document.clone();
    edited
        .edit(|root| {
            let Content::Object(page) = root
                .children
                .iter_mut()
                .find_map(|c| match c {
                    Content::Object(o) if o.kind == ObjectKind::Page => Some(c),
                    _ => None,
                })
                .unwrap()
            else {
                panic!()
            };
            page.id = Some(8);
        })
        .unwrap();
    let rewritten = cdx_of(&edited, Some(&platform_one), false).unwrap();
    assert!(
        rewritten.loss_report.is_empty(),
        "{:?}",
        rewritten.loss_report
    );
    assert_ne!(rewritten.bytes, platform_one);
    // The same document without its original has no known platform.
    assert_eq!(
        features(&cdx_of(&edited, None, false).unwrap().loss_report),
        [("font_platform", LossDisposition::Normalized)]
    );
    // A Macintosh source is rewritten for Windows, and that is reported.
    let platform_zero = file(
        0,
        &[
            prop(0x0100, &font_table(0, &[(3, 1252, "Arial")])),
            obj(0x8001, 1, &[]),
        ],
    );
    let mac = LocalInterchangeCodec
        .read(InterchangeFormat::Cdx, &platform_zero)
        .unwrap();
    assert!(mac.loss_report.is_empty());
    let mut changed = mac.document.clone();
    changed
        .edit(|root| {
            let Content::Object(page) = &mut root.children[1] else {
                panic!()
            };
            page.id = Some(8);
        })
        .unwrap();
    assert_eq!(
        features(
            &cdx_of(&changed, Some(&platform_zero), false)
                .unwrap()
                .loss_report
        ),
        [("font_platform", LossDisposition::Normalized)]
    );
    // Editing a font itself makes the old platform unknown even for a Windows source.
    let mut renamed = read.document.clone();
    renamed
        .edit(|root| {
            let Content::Object(table) = &mut root.children[0] else {
                panic!()
            };
            let Content::Object(font) = &mut table.children[0] else {
                panic!()
            };
            for property in &mut font.properties {
                if property.kind == PropertyKind::FontName {
                    property.value = PropertyValue::Text("Other".into());
                }
            }
        })
        .unwrap();
    assert_eq!(
        features(
            &cdx_of(&renamed, Some(&platform_one), false)
                .unwrap()
                .loss_report
        ),
        [("font_platform", LossDisposition::Normalized)]
    );
}

#[test]
fn several_font_tables_each_need_a_known_platform() {
    let two = file(
        0,
        &[
            prop(0x0100, &font_table(1, &[(1, 1252, "A")])),
            prop(0x0100, &font_table(0, &[(2, 1252, "B")])),
            obj(0x8001, 1, &[]),
        ],
    );
    let read = cdx::read(&two).unwrap();
    assert_eq!(read.font_tables().len(), 2);
    let output = cdx::write(read.document(), Some(&read)).unwrap();
    assert_eq!(
        features(&output.losses),
        [("font_platform", LossDisposition::Normalized)]
    );
    assert!(output.losses[0].reason.starts_with("1 font table"));
    assert_eq!(output.losses[0].source_path, "document/font table[1]");
}

#[test]
fn coordinates_colors_and_sizes_round_to_the_nearest_step_and_report_it() {
    let text = format!(
        r#"<CDXML><colortable><color r="0.1" g="0" b="1"/></colortable>{FONT}<page id="1"><fragment id="2"><n id="3" p="0.1 0.5"/></fragment>
<t id="4" p="2 3"><s font="3" size="10.01" face="0" color="2">x</s></t></page></CDXML>"#
    );
    let output = written(&text);
    let precision: Vec<_> = output
        .loss_report
        .iter()
        .filter(|l| l.feature == "precision")
        .collect();
    assert_eq!(precision.len(), 3, "{:?}", output.loss_report);
    assert!(
        precision
            .iter()
            .all(|l| l.disposition == LossDisposition::Normalized)
    );
    let back = reread(&output.bytes);
    // 0.1 pt -> round(0.1 * 65536) / 65536, within half a step.
    let Content::Object(page) = &back.root().children[2] else {
        panic!()
    };
    let Content::Object(fragment) = &page.children[0] else {
        panic!()
    };
    let Content::Object(node) = &fragment.children[0] else {
        panic!()
    };
    let PropertyValue::Point { x, y } = node.properties[0].value else {
        panic!()
    };
    assert_eq!(x, 6554.0 / 65536.0);
    assert_eq!(y, 0.5);
    assert!((x - 0.1).abs() <= 0.5 / 65536.0);
    let Content::Object(text_object) = &page.children[1] else {
        panic!()
    };
    let Content::Object(run) = &text_object.children[0] else {
        panic!()
    };
    assert_eq!(run.properties[1].value, PropertyValue::Float(10.0));
    let Content::Object(colors) = &back.root().children[0] else {
        panic!()
    };
    let Content::Object(color) = &colors.children[0] else {
        panic!()
    };
    assert_eq!(
        color.properties[0].value,
        PropertyValue::Float(6554.0 / 65535.0)
    );
    // A value that is already a step is not reported.
    let exact = written(&format!(
        "<CDXML>{FONT}<page id=\"1\"><fragment id=\"2\"><n id=\"3\" p=\"0.5 -1.25\"/></fragment></page></CDXML>"
    ));
    assert!(exact.loss_report.iter().all(|l| l.feature != "precision"));
}

#[test]
fn values_outside_the_binary_range_are_refused_not_clamped() {
    for (attribute, text) in [("p", "40000 1"), ("p", "1 -32768.5")] {
        let document = from_cdxml(&format!(
            "<CDXML><page id=\"1\"><fragment id=\"2\"><n id=\"3\" {attribute}=\"{text}\"/></fragment></page></CDXML>"
        ));
        let error = cdx_of(&document, None, false).unwrap_err();
        assert!(error.starts_with("out_of_range"), "{text}: {error}");
    }
    let edge = from_cdxml(
        "<CDXML><page id=\"1\"><fragment id=\"2\"><n id=\"3\" p=\"32767.99998474121 -32768\"/></fragment></page></CDXML>",
    );
    assert!(cdx_of(&edge, None, false).is_ok());
    let big_size = from_cdxml(&format!(
        "<CDXML>{FONT}<page id=\"1\"><t id=\"2\"><s font=\"3\" size=\"3277\" color=\"0\">x</s></t></page></CDXML>"
    ));
    assert!(
        cdx_of(&big_size, None, false)
            .unwrap_err()
            .starts_with("out_of_range")
    );
}

#[test]
fn style_runs_without_font_size_or_color_are_refused_and_a_missing_face_is_reported() {
    let page =
        |run: &str| format!("<CDXML>{FONT}<page id=\"1\"><t id=\"2\">{run}</t></page></CDXML>");
    let document = from_cdxml(&page(
        "<s font=\"3\" size=\"10\" color=\"0\">a</s><s font=\"3\" size=\"10\" face=\"1\" color=\"0\">b</s>",
    ));
    let output = cdx_of(&document, None, false).unwrap();
    assert!(
        features(&output.loss_report).contains(&("style_defaults", LossDisposition::Normalized))
    );
    let back = reread(&output.bytes);
    let Content::Object(page_object) = &back.root().children[1] else {
        panic!()
    };
    let Content::Object(text) = &page_object.children[0] else {
        panic!()
    };
    let Content::Object(run) = &text.children[0] else {
        panic!()
    };
    assert_eq!(
        run.properties[2].value,
        PropertyValue::Integer(0),
        "face follows font and size"
    );
    for incomplete in [
        "<s size=\"10\" color=\"0\">a</s><s>b</s>",
        "<s font=\"3\" color=\"0\">a</s><s>b</s>",
        "<s font=\"3\" size=\"10\">a</s><s>b</s>",
        "<s>a</s><s>b</s>",
    ] {
        let error = cdx_of(&from_cdxml(&page(incomplete)), None, false).unwrap_err();
        assert!(
            error.starts_with("unsupported_style_run"),
            "{incomplete}: {error}"
        );
    }
}

#[test]
fn a_single_bare_run_is_an_unstyled_string_and_survives() {
    let document =
        from_cdxml("<CDXML><page id=\"1\"><t id=\"2\"><s>plain \u{e9}</s></t></page></CDXML>");
    let output = cdx_of(&document, None, false).unwrap();
    assert!(output.loss_report.is_empty());
    assert_eq!(reread(&output.bytes), document);
    let styled_start = output
        .bytes
        .windows(6)
        .position(|w| w == [0x00, 0x07, 9, 0, 0, 0]);
    assert!(styled_start.is_some(), "count 0 then 7 text bytes");
}

#[test]
fn whitespace_between_runs_is_ignored_and_comments_are_dropped_only_with_a_reported_loss() {
    let clean = format!(
        "<CDXML>{FONT}<page id=\"1\"><t id=\"2\">\n  <s font=\"3\" size=\"10\" face=\"0\" color=\"0\">a</s>\n  <s font=\"3\" size=\"10\" face=\"0\" color=\"0\">b</s>\n</t></page></CDXML>"
    );
    let output = cdx_of(&from_cdxml(&clean), None, false).unwrap();
    assert_eq!(
        features(&output.loss_report),
        [("font_platform", LossDisposition::Normalized)]
    );
    let noisy = format!(
        "<CDXML>{FONT}<page id=\"1\"><!--p--><t id=\"2\"><!--a--> <s font=\"3\" size=\"10\" face=\"0\" color=\"0\">a<!--inside-->b<?pi x?>c</s><?q?><s font=\"3\" size=\"10\" face=\"0\" color=\"0\">d</s></t></page></CDXML>"
    );
    let output = cdx_of(&from_cdxml(&noisy), None, false).unwrap();
    let dropped = output
        .loss_report
        .iter()
        .find(|l| l.feature == "xml_content")
        .unwrap();
    assert_eq!(dropped.disposition, LossDisposition::Dropped);
    assert!(dropped.reason.starts_with("5 XML"), "{}", dropped.reason);
    let back = reread(&output.bytes);
    let Content::Object(page) = &back.root().children[1] else {
        panic!()
    };
    let Content::Object(text) = &page.children[0] else {
        panic!()
    };
    let Content::Object(run) = &text.children[0] else {
        panic!()
    };
    assert_eq!(run.children, vec![Content::Text("abc".into())]);
    // Stray visible text outside a run is not silently ignored either.
    let stray = format!(
        "<CDXML>{FONT}<page id=\"1\"><t id=\"2\">oops<s font=\"3\" size=\"10\" face=\"0\" color=\"0\">a</s></t></page></CDXML>"
    );
    let output = cdx_of(&from_cdxml(&stray), None, false).unwrap();
    assert!(features(&output.loss_report).contains(&("xml_content", LossDisposition::Dropped)));
}

#[test]
fn model_content_without_a_cdx_encoding_is_reported_as_dropped() {
    let document = from_cdxml(
        "<CDXML xmlns=\"urn:example:cdxml\"><page id=\"1\"><graphic id=\"2\" GraphicType=\"Line\" BoundingBox=\"0 0 1 1\"><represent object=\"1\" attribute=\"Mystery\"/></graphic></page></CDXML>",
    );
    let output = cdx_of(&document, None, false).unwrap();
    let features = features(&output.loss_report);
    assert!(
        features.contains(&("xml_namespace", LossDisposition::Dropped)),
        "{features:?}"
    );
    assert!(
        features.contains(&("represents", LossDisposition::Dropped)),
        "{features:?}"
    );
    let back = reread(&output.bytes);
    let Content::Object(page) = &back.root().children[0] else {
        panic!()
    };
    let Content::Object(graphic) = &page.children[0] else {
        panic!()
    };
    assert!(graphic.children.is_empty());
    // A Represents object that carries its own ID cannot be represented at all.
    let with_id = from_cdxml(
        "<CDXML><page id=\"1\"><graphic id=\"2\" GraphicType=\"Line\" BoundingBox=\"0 0 1 1\"><represent id=\"5\" object=\"1\" attribute=\"Charge\"/></graphic></page></CDXML>",
    );
    assert!(
        cdx_of(&with_id, None, false)
            .unwrap_err()
            .starts_with("unsupported_represents_id")
    );
}

#[test]
fn text_that_cannot_be_encoded_is_refused_not_approximated() {
    let page = |text: &str| {
        format!(
            "<CDXML>{FONT}<page id=\"1\"><t id=\"2\"><s font=\"3\" size=\"10\" face=\"0\" color=\"0\">{text}</s></t></page></CDXML>"
        )
    };
    for (text, label) in [
        ("\u{3b1}", "Greek"),
        ("&#x85;", "C1 control"),
        ("&#x80;", "C1 control"),
        ("&#xd835;&#xdc00;".trim(), ""),
    ] {
        if text.contains("&#xd8") {
            continue;
        }
        let Ok(document) = LocalInterchangeCodec
            .read(InterchangeFormat::Cdxml, page(text).as_bytes())
            .map(|r| r.document)
        else {
            continue;
        };
        let error = cdx_of(&document, None, false).unwrap_err();
        assert!(error.starts_with("unsupported_text"), "{label}: {error}");
    }
    // Controls XML cannot carry may still arrive through a client-built tree.
    let mut document = from_cdxml(&page("a"));
    document
        .edit(|root| {
            let Content::Object(page) = &mut root.children[1] else {
                panic!()
            };
            let Content::Object(text) = &mut page.children[0] else {
                panic!()
            };
            let Content::Object(run) = &mut text.children[0] else {
                panic!()
            };
            run.children = vec![Content::Text("a\u{1}".into())];
        })
        .unwrap();
    assert!(
        cdx_of(&document, None, false)
            .unwrap_err()
            .starts_with("unsupported_text")
    );
}

#[test]
fn fonts_need_a_supported_charset_the_writer_does_not_assume() {
    let wrong = from_cdxml(
        "<CDXML><fonttable><font id=\"3\" charset=\"utf-8\" name=\"Arial\"/></fonttable><page id=\"1\"/></CDXML>",
    );
    assert!(
        cdx_of(&wrong, None, false)
            .unwrap_err()
            .starts_with("unsupported_charset")
    );
    let missing = from_cdxml(
        "<CDXML><fonttable><font id=\"3\" name=\"Arial\"/></fonttable><page id=\"1\"/></CDXML>",
    );
    assert!(
        cdx_of(&missing, None, false)
            .unwrap_err()
            .starts_with("missing_property")
    );
    let upper = from_cdxml(
        "<CDXML><fonttable><font id=\"3\" charset=\"ISO-8859-1\" name=\"Arial\"/></fonttable><page id=\"1\"/></CDXML>",
    );
    assert!(cdx_of(&upper, None, false).is_ok());
}

#[test]
fn tables_after_pages_are_moved_ahead_and_the_move_is_reported() {
    let document = from_cdxml(&format!("<CDXML><page id=\"1\"/>{FONT}</CDXML>"));
    let output = cdx_of(&document, None, false).unwrap();
    assert!(features(&output.loss_report).contains(&("child_order", LossDisposition::Normalized)));
    let back = reread(&output.bytes);
    assert_eq!(back.root().children.len(), 2);
}

#[test]
fn long_text_uses_the_extended_length_and_reads_back() {
    let long: String = (0..70_000)
        .map(|i| char::from(b'a' + (i % 26) as u8))
        .collect();
    let document = from_cdxml(&format!(
        "<CDXML><page id=\"1\"><t id=\"2\"><s>{long}</s></t></page></CDXML>"
    ));
    let bytes = cdx_of(&document, None, false).unwrap().bytes;
    let marker = bytes
        .windows(4)
        .position(|w| w == [0x00, 0x07, 0xff, 0xff])
        .expect("extended property");
    assert_eq!(&bytes[marker + 4..marker + 8], &(70_002_u32).to_le_bytes());
    assert_eq!(reread(&bytes), document);
}

#[test]
fn output_larger_than_the_codec_limit_is_refused() {
    let mut document = from_cdxml("<CDXML><page id=\"1\"><t id=\"2\"><s>x</s></t></page></CDXML>");
    document
        .edit(|root| {
            let Content::Object(page) = &mut root.children[0] else {
                panic!()
            };
            let Content::Object(text) = &mut page.children[0] else {
                panic!()
            };
            let Content::Object(run) = &mut text.children[0] else {
                panic!()
            };
            run.children = vec![Content::Text("x".repeat(1024 * 1024))];
        })
        .unwrap();
    let error = cdx_of(&document, None, false).unwrap_err();
    assert!(error.starts_with("resource_limit"), "{error}");
}

#[test]
fn unknown_content_follows_the_shared_opaque_policy() {
    let original = rich_file_with_opaque();
    let read = LocalInterchangeCodec
        .read(InterchangeFormat::Cdx, &original)
        .unwrap();
    assert_eq!(
        features(&read.loss_report),
        [("opaque_extensions", LossDisposition::Retained)]
    );
    // Unchanged: the original bytes come back, opaque property included.
    let same = cdx_of(&read.document, Some(&original), false).unwrap();
    assert_eq!(same.bytes, original);
    assert_eq!(
        features(&same.loss_report),
        [("opaque_extensions", LossDisposition::Retained)]
    );
    // Edited: refused without a discard, even with the original in hand.
    let mut edited = read.document.clone();
    edited
        .edit(|root| {
            let Content::Object(page) = root
                .children
                .iter_mut()
                .find_map(|c| match c {
                    Content::Object(o) if o.kind == ObjectKind::Page => Some(c),
                    _ => None,
                })
                .unwrap()
            else {
                panic!()
            };
            page.properties.push(Property {
                kind: PropertyKind::BoundingBox,
                value: PropertyValue::Rectangle {
                    left: 0.0,
                    top: 0.0,
                    right: 1.0,
                    bottom: 1.0,
                },
            });
        })
        .unwrap();
    assert!(
        cdx_of(&edited, Some(&original), false)
            .unwrap_err()
            .starts_with("unsafe_unknown_rewrite")
    );
    assert!(
        cdx_of(&edited, None, false)
            .unwrap_err()
            .starts_with("unsafe_unknown_rewrite")
    );
    let discarded = cdx_of(&edited, Some(&original), true).unwrap();
    assert!(
        features(&discarded.loss_report).contains(&("opaque_extensions", LossDisposition::Dropped))
    );
    let back = LocalInterchangeCodec
        .read(InterchangeFormat::Cdx, &discarded.bytes)
        .unwrap();
    assert!(back.loss_report.is_empty());
    // Discarding also acts on an unchanged tree.
    let stripped = cdx_of(&read.document, Some(&original), true).unwrap();
    assert!(stripped.bytes.len() < original.len());
}

#[test]
fn opaque_objects_that_hold_references_cannot_be_discarded_into_a_dangling_reference() {
    // An unknown object owns ID 20; a bond refers to it, so a node-typed target is required
    // and the reader already refuses it.
    let doc = file(
        0,
        &[obj(
            0x8001,
            1,
            &[obj(
                0x8003,
                2,
                &[
                    obj(0x8004, 3, &[]),
                    obj(0x8030, 20, &[]),
                    obj(
                        0x8005,
                        4,
                        &[prop(0x0604, &u32le(3)), prop(0x0605, &u32le(20))],
                    ),
                ],
            )],
        )],
    );
    let error = cdx::read(&doc).unwrap_err();
    assert_eq!(error.code, "invalid_reference");
}

#[test]
fn document_edits_keep_written_bytes_re_readable_across_many_shapes() {
    // A fixed list of small shapes written, re-read and re-written to the same bytes.
    for text in [
        "<CDXML><page id=\"1\"/></CDXML>",
        "<CDXML><page id=\"1\"/><page id=\"2\"/></CDXML>",
        "<CDXML><page><fragment><n/><n/></fragment></page></CDXML>",
        "<CDXML><page id=\"1\"><group id=\"2\"><group id=\"3\"><fragment id=\"4\"><n id=\"5\"/></fragment></group></group></page></CDXML>",
    ] {
        let document = from_cdxml(text);
        let first = cdx_of(&document, None, false).unwrap().bytes;
        let back = reread(&first);
        assert_eq!(back, document, "{text}");
        assert_eq!(cdx_of(&back, None, false).unwrap().bytes, first, "{text}");
    }
}

#[test]
fn objects_in_a_written_tree_never_exceed_what_the_reader_accepts() {
    // Deepest tree the model allows: depth 128 written and re-read.
    let mut inner = Object {
        kind: ObjectKind::Group,
        id: None,
        properties: vec![],
        children: vec![],
    };
    for _ in 0..126 {
        inner = Object {
            kind: ObjectKind::Group,
            id: None,
            properties: vec![],
            children: vec![Content::Object(inner)],
        };
    }
    let page = Object {
        kind: ObjectKind::Page,
        id: None,
        properties: vec![],
        children: vec![Content::Object(inner)],
    };
    let root = Object {
        kind: ObjectKind::Document,
        id: None,
        properties: vec![],
        children: vec![Content::Object(page)],
    };
    let document = Document::new(root).unwrap();
    let bytes = cdx_of(&document, None, false).unwrap().bytes;
    assert_eq!(reread(&bytes), document);
}
