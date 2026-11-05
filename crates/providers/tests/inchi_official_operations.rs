//! Opt-in application→registered provider→official native InChI comparisons.
//!
//! The official executable independently reads coordinate/mark V2000 fixtures; the
//! application receives ordinary editing commands with stored configurations. Its drawing
//! positions are deliberately collinear, so a correct result must come from stored stereo.
//! No fake provider, native output fixture or application DLL participates.

use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use chemifree_application::Application;
use chemifree_providers::{
    FileDocumentRepository, InchiRegistration, LocalCapabilities, RegisteredInchiProvider,
};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

struct Fixture {
    name: &'static str,
    tetrahedral: bool,
    clockwise_or_opposite: bool,
}

fn call(app: &App, operation: &str, parameters: Value) -> Value {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let request = json!({"contract":1,"request_id":format!("official-inchi-{}",NEXT.fetch_add(1,Ordering::Relaxed)),"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn build(app: &App, fixture: &Fixture) -> String {
    let created = call(app, "document.create", json!({"title":fixture.name}));
    let id = created["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let symbols = if fixture.tetrahedral {
        ["C", "F", "Cl", "Br"]
    } else {
        ["C", "C", "F", "Cl"]
    };
    let mut commands = Vec::new();
    for (index, symbol) in symbols.iter().enumerate() {
        commands.push(json!({"op":"add_atom","element":symbol,"position":{"x":index*10,"y":0}}));
    }
    let bonds = if fixture.tetrahedral {
        [(0, 1, "single"), (0, 2, "single"), (0, 3, "single")]
    } else {
        [(0, 1, "double"), (0, 2, "single"), (1, 3, "single")]
    };
    for (begin, end, order) in bonds {
        commands.push(json!({"op":"add_bond","begin":format!("${begin}"),"end":format!("${end}"),"order":order}));
    }
    if fixture.tetrahedral {
        // Both application drawings carry the same wedge. Their opposite stored turns
        // must produce opposite identifiers, without interpreting that wedge or geometry.
        commands.push(
            json!({"op":"set_bond_mark","bond":"$5","mark":{"kind":"wedge","narrow":"begin"}}),
        );
        commands.push(json!({"op":"set_atom_configuration","atom":"$0","configuration":{"kind":"tetrahedral","neighbors":["$1","$2","$3","implicit_hydrogen"],"turn":if fixture.clockwise_or_opposite {"clockwise"} else {"anticlockwise"}}}));
        commands.push(
            json!({"op":"create_stereo_group","kind":"absolute","atoms":["$0"],"number":null}),
        );
    } else {
        commands.push(json!({"op":"set_bond_configuration","bond":"$4","configuration":{"kind":"relative","begin_neighbor":"$2","end_neighbor":"$3","relation":if fixture.clockwise_or_opposite {"opposite_sides"} else {"same_side"}}}));
    }
    let edited = call(
        app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":commands}),
    );
    assert!(
        matches!(edited["outcome"].as_str(), Some("ok" | "partial")),
        "{}: {edited}",
        fixture.name
    );
    id
}

/// Independent reference picture: clockwise [F,Cl,Br,H] is the S wedge fan; the
/// double-bond references F and Cl are above/below for opposite-side parity.
fn molfile(fixture: &Fixture) -> String {
    let (symbols, positions, bonds) = if fixture.tetrahedral {
        (
            ["C", "F", "Cl", "Br"],
            [(0.0, 0.0), (0.0, 1.0), (1.0, -0.5), (-1.0, -0.5)],
            [(0, 1, 1), (0, 2, 1), (0, 3, 1)],
        )
    } else {
        (
            ["C", "C", "F", "Cl"],
            [
                (0.0, 0.0),
                (1.0, 0.0),
                (-0.5, 1.0),
                (
                    1.5,
                    if fixture.clockwise_or_opposite {
                        -1.0
                    } else {
                        1.0
                    },
                ),
            ],
            [(0, 1, 2), (0, 2, 1), (1, 3, 1)],
        )
    };
    let mut text = format!(
        "{}\n  Independent public SDK reference\n\n  4  3  0  0  0  0  0  0  0  0999 V2000\n",
        fixture.name
    );
    for (symbol, (x, y)) in symbols.iter().zip(positions) {
        writeln!(
            text,
            "{x:10.4}{y:10.4}{:10.4} {symbol:<3} 0  0  0  0  0  0  0  0  0  0  0  0",
            0.0
        )
        .unwrap();
    }
    for (index, (begin, end, order)) in bonds.iter().enumerate() {
        let mark = if fixture.tetrahedral && index == 1 {
            if fixture.clockwise_or_opposite { 1 } else { 6 }
        } else {
            0
        };
        writeln!(
            text,
            "{:3}{:3}{order:3}{mark:3}  0  0  0",
            begin + 1,
            end + 1
        )
        .unwrap();
    }
    text.push_str("M  END\n");
    text
}

fn reference(executable: &Path, directory: &Path, fixture: &Fixture) -> (String, String) {
    let input = directory.join(format!("{}.mol", fixture.name));
    let output = directory.join(format!("{}.inchi", fixture.name));
    let log = directory.join(format!("{}.log", fixture.name));
    let problem = directory.join(format!("{}.problem.mol", fixture.name));
    std::fs::write(&input, molfile(fixture)).unwrap();
    let execution = Command::new(executable)
        .args([
            input.as_os_str(),
            output.as_os_str(),
            log.as_os_str(),
            problem.as_os_str(),
        ])
        .args(["-W5", "-Key"])
        .output()
        .unwrap();
    assert!(
        execution.status.success(),
        "{}: official executable failed: {}",
        fixture.name,
        String::from_utf8_lossy(&execution.stderr)
    );
    let text = std::fs::read_to_string(output).unwrap();
    let inchi = text
        .lines()
        .find(|line| line.starts_with("InChI=1S/"))
        .unwrap_or_else(|| panic!("{}: no official Standard InChI: {text}", fixture.name))
        .to_owned();
    let key = text
        .lines()
        .find_map(|line| line.strip_prefix("InChIKey="))
        .unwrap_or_else(|| panic!("{}: no official InChIKey: {text}", fixture.name))
        .to_owned();
    (inchi, key)
}

#[test]
#[ignore = "requires explicit official InChI 1.07.5 library, SHA-256 and executable; run with --ignored --nocapture"]
fn operation_stored_tetrahedral_and_ez_stereo_match_official_identifiers_with_exact_consent() {
    let values: Option<Vec<String>> = [
        "CHEMIFREE_TEST_INCHI_LIBRARY",
        "CHEMIFREE_TEST_INCHI_SHA256",
        "CHEMIFREE_TEST_INCHI_EXECUTABLE",
    ]
    .iter()
    .map(|name| std::env::var(name).ok())
    .collect();
    let Some(values) = values else {
        eprintln!(
            "SKIP official application/provider InChI comparison: all three explicit registration environment variables are required; executed=0"
        );
        return;
    };
    let executable = PathBuf::from(&values[2]);
    assert!(
        executable.is_absolute(),
        "official executable path must be absolute"
    );
    let provider = RegisteredInchiProvider::load(InchiRegistration {
        library_path: PathBuf::from(&values[0]),
        version: "1.07.5".into(),
        sha256: values[1].clone(),
        timeout_seconds: 5,
    })
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    )
    .with_inchi_provider(provider);
    let cases = [
        Fixture {
            name: "tetra-clockwise",
            tetrahedral: true,
            clockwise_or_opposite: true,
        },
        Fixture {
            name: "tetra-anticlockwise",
            tetrahedral: true,
            clockwise_or_opposite: false,
        },
        Fixture {
            name: "alkene-opposite",
            tetrahedral: false,
            clockwise_or_opposite: true,
        },
        Fixture {
            name: "alkene-same-side",
            tetrahedral: false,
            clockwise_or_opposite: false,
        },
    ];
    let mut identifiers = Vec::new();
    for fixture in &cases {
        let id = build(&app, fixture);
        let before = call(&app, "document.snapshot", json!({"document_id":id}))["result"].clone();
        let expected = reference(&executable, directory.path(), fixture);
        let first = call(&app, "chem.inchi", json!({"document_id":id}));
        assert_eq!(
            call(&app, "document.snapshot", json!({"document_id":id}))["result"],
            before,
            "{}: initial generation is read-only",
            fixture.name
        );
        let result = if fixture.tetrahedral {
            assert_eq!(
                first["diagnostics"][0]["code"], "loss_consent_required",
                "{first}"
            );
            assert!(
                first["result"].is_null(),
                "unconsented identifiers must remain withheld: {first}"
            );
            let features = first["diagnostics"][0]["details"]["features"]
                .as_array()
                .unwrap();
            assert!(features.contains(&json!("stereo_marks")), "{first}");
            assert!(features.contains(&json!("stereo_group")), "{first}");
            let wrong = call(
                &app,
                "chem.inchi",
                json!({"document_id":id,"accepted_losses":"0".repeat(64)}),
            );
            assert_eq!(
                wrong["diagnostics"][0]["code"], "loss_consent_changed",
                "{wrong}"
            );
            assert!(wrong["result"].is_null(), "{wrong}");
            let digest = first["diagnostics"][0]["details"]["loss_digest"]
                .as_str()
                .unwrap();
            let accepted = call(
                &app,
                "chem.inchi",
                json!({"document_id":id,"accepted_losses":digest}),
            );
            assert_eq!(accepted["outcome"], "partial", "{accepted}");
            let losses = accepted["loss_report"].as_array().unwrap();
            for feature in ["stereo_marks", "stereo_group"] {
                let loss = losses
                    .iter()
                    .find(|loss| loss["feature"] == feature)
                    .unwrap();
                assert_eq!(loss["disposition"], "dropped", "{loss}");
                assert_eq!(loss["semantic_impact"], "stereo", "{loss}");
            }
            accepted
        } else {
            assert_eq!(first["outcome"], "ok", "{first}");
            assert_eq!(first["loss_report"], json!([]), "{first}");
            first
        };
        assert_eq!(
            result["result"]["inchi"].as_str().unwrap().as_bytes(),
            expected.0.as_bytes(),
            "{}: exact official InChI",
            fixture.name
        );
        assert_eq!(
            result["result"]["inchikey"].as_str().unwrap().as_bytes(),
            expected.1.as_bytes(),
            "{}: exact official InChIKey",
            fixture.name
        );
        assert_eq!(result["result"]["snapshot"], before, "{result}");
        assert_eq!(result["result"]["provider"]["version"], "1.07.5");
        assert_eq!(
            result["result"]["provider"]["build"],
            format!("sha256:{}", values[1])
        );
        assert_eq!(result["result"]["provider"]["options"], json!(["-W5"]));
        assert_eq!(result["result"]["standard"], true);
        assert_eq!(
            result["result"]["native_code"], 0,
            "clean representative inputs must have no native warnings: {result}"
        );
        assert_eq!(
            call(&app, "document.snapshot", json!({"document_id":id}))["result"],
            before,
            "{}: consent and generation leave source unchanged",
            fixture.name
        );
        identifiers.push(expected);
        eprintln!(
            "EXECUTED official application/provider comparison: {}",
            fixture.name
        );
    }
    assert_ne!(
        identifiers[0], identifiers[1],
        "opposite tetrahedral configurations must differ"
    );
    assert_ne!(identifiers[2], identifiers[3], "E and Z must differ");
    eprintln!(
        "Official application/provider InChI comparison executed={} skipped=0",
        cases.len()
    );
}
