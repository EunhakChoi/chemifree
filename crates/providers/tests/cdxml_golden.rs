//! P22 CDXML results pinned byte for byte. The expected files were produced by the CDXML
//! codec as it stood before the CDX codec was added; a change here means P22 behavior moved.

use std::fmt::Write;
use std::path::PathBuf;

use chemifree_application::LossConsent;
use chemifree_application::interchange_codec::{InterchangeCodec, InterchangeFormat};
use chemifree_domain::interchange::{Content, Document, ObjectKind};
use chemifree_providers::interchange_codec::LocalInterchangeCodec;

const SOURCE: &str = include_str!("data/cdxml/multi-page.cdxml");

#[derive(Clone, Copy)]
enum Edit {
    None,
    PageId11,
    RootId99,
}

struct Case {
    name: &'static str,
    original: String,
    /// When `None` the submitted tree is written without any original.
    use_original: bool,
    edit: Edit,
    discard: bool,
}

fn cases() -> Vec<Case> {
    let future = SOURCE.replace("<page id=\"1\">", "<page id=\"1\" future=\"4\">");
    let prolog = SOURCE.replacen("?>", "?>\n<!-- preserve me -->", 1);
    let bom = "\u{feff}<?xml version=\"1.0\"?><CDXML><page id=\"1\"/></CDXML>".to_owned();
    let case = |name, original: &String, use_original, edit, discard| Case {
        name,
        original: original.clone(),
        use_original,
        edit,
        discard,
    };
    vec![
        case("unchanged-opaque", &future, true, Edit::None, false),
        case("unchanged-opaque-discard", &future, true, Edit::None, true),
        case("edit-opaque-refused", &future, true, Edit::PageId11, false),
        case("edit-opaque-discard", &future, true, Edit::PageId11, true),
        case("prolog-edit-refused", &prolog, true, Edit::RootId99, false),
        case("prolog-edit-discard", &prolog, true, Edit::RootId99, true),
        case(
            "plain-no-original",
            &SOURCE.to_owned(),
            false,
            Edit::None,
            false,
        ),
        case(
            "plain-unchanged",
            &SOURCE.to_owned(),
            true,
            Edit::None,
            false,
        ),
        case(
            "plain-edit",
            &SOURCE.to_owned(),
            true,
            Edit::PageId11,
            false,
        ),
        case("bom-edit", &bom, true, Edit::PageId11, false),
        case(
            "opaque-no-original-discard",
            &future,
            false,
            Edit::None,
            true,
        ),
        case("opaque-no-original", &future, false, Edit::None, false),
    ]
}

fn render(case: &Case) -> String {
    let codec = LocalInterchangeCodec;
    let read = codec
        .read(InterchangeFormat::Cdxml, case.original.as_bytes())
        .unwrap();
    let mut out = String::new();
    assert!(
        read.loss_report
            .iter()
            .all(|loss| loss.consent == LossConsent::NotRequired),
        "{}: CDXML reads never need consent",
        case.name
    );
    for loss in &read.loss_report {
        writeln!(
            out,
            "read-loss {} {} {:?} {:?} {}",
            loss.feature, loss.source_path, loss.disposition, loss.semantic_impact, loss.reason
        )
        .unwrap();
    }
    writeln!(out, "outside-root {}", read.opaque_outside_root).unwrap();
    let mut root = read.document.root().clone();
    match case.edit {
        Edit::None => (),
        Edit::PageId11 => {
            for child in &mut root.children {
                if let Content::Object(object) = child
                    && object.kind == ObjectKind::Page
                {
                    object.id = Some(11);
                    break;
                }
            }
        }
        Edit::RootId99 => root.id = Some(99),
    }
    let submitted = Document::new(root).unwrap();
    let original = case.use_original.then_some(case.original.as_bytes());
    match codec.write(InterchangeFormat::Cdxml, &submitted, original, case.discard) {
        Ok(written) => {
            assert!(
                written
                    .loss_report
                    .iter()
                    .all(|loss| loss.consent == LossConsent::NotRequired),
                "{}: CDXML writes never need consent (discard_opaque is their consent)",
                case.name
            );
            for loss in &written.loss_report {
                writeln!(
                    out,
                    "write-loss {} {} {:?} {:?} {}",
                    loss.feature,
                    loss.source_path,
                    loss.disposition,
                    loss.semantic_impact,
                    loss.reason
                )
                .unwrap();
            }
            writeln!(out, "bytes:").unwrap();
            out.push_str(&String::from_utf8(written.bytes).unwrap());
        }
        Err(error) => writeln!(out, "error {} {}", error.code, error.message).unwrap(),
    }
    out
}

#[test]
fn cdxml_results_match_the_pre_cdx_recordings() {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/cdxml-golden");
    let bless = std::env::var_os("CHEMIFREE_BLESS_CDXML_GOLDEN").is_some();
    for case in cases() {
        let actual = render(&case);
        let path = directory.join(format!("{}.txt", case.name));
        if bless {
            std::fs::write(&path, &actual).unwrap();
        }
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(actual, expected, "case {}", case.name);
    }
}
