//! The capability and catalog wording for every combination of connected format codecs.

use chemifree_application::Application;
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, OpenSmilesCodec, V2000Codec};
use serde_json::Value;

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn application(directory: &std::path::Path, ctfile: bool, interchange: bool, smiles: bool) -> App {
    let mut app = Application::new(LocalCapabilities, FileDocumentRepository::new(directory));
    if ctfile {
        app = app.with_ctfile_codec(V2000Codec);
    }
    if interchange {
        app = app.with_interchange_codec(LocalInterchangeCodec);
    }
    if smiles {
        app = app.with_smiles_codec(OpenSmilesCodec);
    }
    app
}

/// `(label, description, import note, export note)` of a configuration.
fn wording(ctfile: bool, interchange: bool, smiles: bool) -> [String; 4] {
    let directory = tempfile::tempdir().unwrap();
    let app = application(directory.path(), ctfile, interchange, smiles);
    let value = serde_json::to_value(app.capabilities()).unwrap();
    let conversion = value["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "format_conversion")
        .unwrap()
        .clone();
    let note = |id: &str| -> String {
        value["operation_catalog"]["operations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|operation| operation["id"] == id)
            .unwrap()["note"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let text = |field: &Value| field.as_str().unwrap().to_owned();
    [
        text(&conversion["label"]),
        text(&conversion["description"]),
        note("document.import"),
        note("document.export"),
    ]
}

/// Every combination of connected codecs, as `(MOL/SDF, CDXML and CDX, SMILES)`, with the
/// complete wording: the `format_conversion` label and description, and the notes of
/// `document.import` and `document.export`. One sentence per combination; a format is never
/// called unsupported where it is also listed.
/// Which codecs are connected, and the four texts they give.
type Combination = ((bool, bool, bool), [&'static str; 4]);

const GOLDEN: [Combination; 7] = [
    (
        (true, false, false),
        [
            "MOL/SDF subset",
            "Read and write bounded V2000 MOL/SDF with explicit losses. MOL/SDF stereo, query and advanced fields are refused; other formats are unsupported.",
            "Preview, then import with explicit losses and revision-checked consent: append one selected record of a bounded V2000 MOL/SDF subset.",
            "Write with explicit losses: bounded V2000 MOL/SDF with per-record losses.",
        ],
    ),
    (
        (false, true, false),
        [
            "CDXML and CDX subsets",
            "Read and write rich CDXML and a bounded CDX subset (base64 over JSON, at most 1 MiB of CDX; HTTP and CLI limits are lower) with explicit losses. CDXML and CDX content the editor cannot hold is kept opaque or reported as a loss; other formats are unsupported.",
            "Preview, then import with explicit losses and revision-checked consent: read CDXML or a bounded CDX subset (canonical base64, at most 1 MiB) as a rich tree, or create a drawing from its loss-accounted editor projection.",
            "Write with explicit losses: rich CDXML or a bounded CDX subset with opaque-content safety and accepted_losses consent for rounded or dropped CDX content.",
        ],
    ),
    (
        (true, true, false),
        [
            "MOL/SDF, CDXML and CDX subsets",
            "Read and write bounded V2000 MOL/SDF, rich CDXML and a bounded CDX subset (base64 over JSON, at most 1 MiB of CDX; HTTP and CLI limits are lower) with explicit losses. MOL/SDF stereo, query and advanced fields are refused; CDXML and CDX content the editor cannot hold is kept opaque or reported as a loss; other formats are unsupported.",
            "Preview, then import with explicit losses and revision-checked consent: append one selected record of a bounded V2000 MOL/SDF subset; read CDXML or a bounded CDX subset (canonical base64, at most 1 MiB) as a rich tree, or create a drawing from its loss-accounted editor projection.",
            "Write with explicit losses: bounded V2000 MOL/SDF with per-record losses; rich CDXML or a bounded CDX subset with opaque-content safety and accepted_losses consent for rounded or dropped CDX content.",
        ],
    ),
    (
        (false, false, true),
        [
            "SMILES subset",
            "Read and write SMILES text (a bounded OpenSMILES subset of at most 16 KiB and 64 nested branches, written in recorded or canonical form) with explicit losses. SMILES stereochemistry, query atoms and reaction SMILES are refused; other formats are unsupported.",
            "Preview, then import with explicit losses and revision-checked consent: append one SMILES string (at most 16 KiB).",
            "Write with explicit losses: one SMILES string, recorded or canonical, with accepted_losses consent for the assumptions it must make.",
        ],
    ),
    (
        (true, false, true),
        [
            "MOL/SDF and SMILES subsets",
            "Read and write bounded V2000 MOL/SDF and SMILES text (a bounded OpenSMILES subset of at most 16 KiB and 64 nested branches, written in recorded or canonical form) with explicit losses. MOL/SDF stereo, query and advanced fields are refused; SMILES stereochemistry, query atoms and reaction SMILES are refused; other formats are unsupported.",
            "Preview, then import with explicit losses and revision-checked consent: append one selected record of a bounded V2000 MOL/SDF subset; append one SMILES string (at most 16 KiB).",
            "Write with explicit losses: bounded V2000 MOL/SDF with per-record losses; one SMILES string, recorded or canonical, with accepted_losses consent for the assumptions it must make.",
        ],
    ),
    (
        (false, true, true),
        [
            "CDXML, CDX and SMILES subsets",
            "Read and write rich CDXML, a bounded CDX subset (base64 over JSON, at most 1 MiB of CDX; HTTP and CLI limits are lower) and SMILES text (a bounded OpenSMILES subset of at most 16 KiB and 64 nested branches, written in recorded or canonical form) with explicit losses. CDXML and CDX content the editor cannot hold is kept opaque or reported as a loss; SMILES stereochemistry, query atoms and reaction SMILES are refused; other formats are unsupported.",
            "Preview, then import with explicit losses and revision-checked consent: read CDXML or a bounded CDX subset (canonical base64, at most 1 MiB) as a rich tree, or create a drawing from its loss-accounted editor projection; append one SMILES string (at most 16 KiB).",
            "Write with explicit losses: rich CDXML or a bounded CDX subset with opaque-content safety and accepted_losses consent for rounded or dropped CDX content; one SMILES string, recorded or canonical, with accepted_losses consent for the assumptions it must make.",
        ],
    ),
    (
        (true, true, true),
        [
            "MOL/SDF, CDXML, CDX and SMILES subsets",
            "Read and write bounded V2000 MOL/SDF, rich CDXML, a bounded CDX subset (base64 over JSON, at most 1 MiB of CDX; HTTP and CLI limits are lower) and SMILES text (a bounded OpenSMILES subset of at most 16 KiB and 64 nested branches, written in recorded or canonical form) with explicit losses. MOL/SDF stereo, query and advanced fields are refused; CDXML and CDX content the editor cannot hold is kept opaque or reported as a loss; SMILES stereochemistry, query atoms and reaction SMILES are refused; other formats are unsupported.",
            "Preview, then import with explicit losses and revision-checked consent: append one selected record of a bounded V2000 MOL/SDF subset; read CDXML or a bounded CDX subset (canonical base64, at most 1 MiB) as a rich tree, or create a drawing from its loss-accounted editor projection; append one SMILES string (at most 16 KiB).",
            "Write with explicit losses: bounded V2000 MOL/SDF with per-record losses; rich CDXML or a bounded CDX subset with opaque-content safety and accepted_losses consent for rounded or dropped CDX content; one SMILES string, recorded or canonical, with accepted_losses consent for the assumptions it must make.",
        ],
    ),
];

#[test]
fn each_combination_of_codecs_has_one_pinned_wording() {
    for ((ctfile, interchange, smiles), expected) in GOLDEN {
        let found = wording(ctfile, interchange, smiles);
        for (field, (found, expected)) in ["label", "description", "import", "export"]
            .iter()
            .zip(found.iter().zip(expected))
        {
            assert_eq!(
                found, expected,
                "{field} for MOL/SDF={ctfile} CDX={interchange} SMILES={smiles}"
            );
        }
    }
}

#[test]
fn the_wording_never_calls_a_listed_format_unsupported_and_agrees_in_number() {
    for ((ctfile, interchange, smiles), expected) in GOLDEN {
        let [label, description, import, export] = expected;
        let named = [
            (ctfile, "MOL/SDF"),
            (interchange, "CDXML"),
            (smiles, "SMILES"),
        ];
        let count = named.iter().filter(|(connected, _)| *connected).count();
        // The label is singular exactly when one family of formats is connected.
        let plural = label.ends_with("subsets");
        assert_eq!(plural, count > 1 || interchange, "{label}");
        for (connected, name) in named {
            assert_eq!(
                description.contains(name),
                connected,
                "{name} in {description}"
            );
            assert_eq!(
                import.contains(name) || name == "SMILES" && import.contains("SMILES"),
                connected,
                "{name} in {import}"
            );
        }
        assert_eq!(
            description.matches("unsupported").count(),
            1,
            "{description}"
        );
        assert!(description.ends_with("other formats are unsupported."));
        assert!(!export.contains("unsupported"), "{export}");
    }
}

#[test]
fn without_a_codec_the_static_wording_applies_and_nothing_is_claimed() {
    let directory = tempfile::tempdir().unwrap();
    let app = application(directory.path(), false, false, false);
    let value = serde_json::to_value(app.capabilities()).unwrap();
    let conversion = value["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "format_conversion")
        .unwrap();
    assert_eq!(conversion["status"], "not_implemented");
    for operation in value["operation_catalog"]["operations"].as_array().unwrap() {
        if operation["id"] == "document.import" || operation["id"] == "document.export" {
            assert_eq!(operation["execution"], "not_implemented", "{operation}");
        }
    }
}

#[test]
fn the_quoted_limits_are_the_codecs_limits() {
    use chemifree_application::interchange_codec::InterchangeCodec;
    use chemifree_application::smiles::SmilesCodec;
    assert_eq!(OpenSmilesCodec.limits().max_input_bytes, 16 * 1024);
    assert_eq!(OpenSmilesCodec.limits().max_branch_depth, 64);
    assert_eq!(LocalInterchangeCodec.binary_input_limit(), 1024 * 1024);
    let [_, description, import, _] = wording(true, true, true);
    assert!(
        description.contains("at most 16 KiB and 64 nested branches")
            && description.contains("at most 1 MiB")
    );
    assert!(import.contains("at most 16 KiB") && import.contains("at most 1 MiB"));
}
