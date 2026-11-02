//! `chem.substructure` through the shared Application: the shared RDKit-checked fixtures, the
//! reply's shape, read-only behavior, refusals with their snapshots, and the limits.

use chemifree_application::{Application, ListQuery};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn application() -> (App, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(data.path()));
    (app, data)
}

fn call_as(app: &App, profile: &str, id: &str, operation: &str, parameters: Value) -> Value {
    let request = json!({
        "contract": 1, "request_id": id, "profile": profile,
        "operation": operation, "parameters": parameters,
    });
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn call(app: &App, id: &str, operation: &str, parameters: Value) -> Value {
    call_as(app, "preserve-semantics", id, operation, parameters)
}

/// A bond `kind` of the fixture grammar as the fields of an `add_bond` command.
fn bond_fields(kind: &str) -> Value {
    let order = |c: char| match c {
        '-' => "single",
        '=' => "double",
        '#' => "triple",
        ':' => "aromatic",
        other => panic!("bond kind {other}"),
    };
    match kind {
        "-" | "=" | "#" | ":" => json!({"order": order(kind.chars().next().unwrap())}),
        ">" => json!({"order": "dative"}),
        "?" => json!({"order": "unknown"}),
        "~" => json!({"order": "query", "query": {"kind": "any"}}),
        list => json!({
            "order": "query",
            "query": {"kind": "list", "orders": list.chars().map(order).collect::<Vec<_>>()},
        }),
    }
}

/// The commands that draw a fixture graph: atoms `C`, `N+`, `13C`, `N@1`, `*`, `A`, `Q`, `X`,
/// `[N,O]` or `![N,O]`, then bonds as `[first, second, kind]`. Atoms are numbered from `a1`.
fn commands(atoms: &[String], bonds: &[(usize, usize, String)]) -> Vec<Value> {
    let mut adds = Vec::new();
    let mut updates = Vec::new();
    for (index, spec) in atoms.iter().enumerate() {
        let mut add = json!({"op": "add_atom", "position": {"x": index * 30, "y": 0}});
        let named = match spec.as_str() {
            "*" => Some("any"),
            "A" => Some("any_heavy"),
            "Q" => Some("heteroatom"),
            "X" => Some("halogen"),
            _ => None,
        };
        if let Some(kind) = named {
            add["query"] = json!({"kind": kind});
        } else if let Some(list) = spec.strip_prefix("![").or_else(|| spec.strip_prefix('[')) {
            let elements: Vec<&str> = list.trim_end_matches(']').split(',').collect();
            add["query"] =
                json!({"kind": "list", "elements": elements, "negated": spec.starts_with('!')});
        } else {
            let (spec, hydrogens) = match spec.split_once('@') {
                Some((spec, count)) => (spec, Some(count.parse::<u8>().unwrap())),
                None => (spec.as_str(), None),
            };
            let digits = spec.bytes().take_while(u8::is_ascii_digit).count();
            let rest = &spec[digits..];
            let sign = rest.find(['+', '-']).unwrap_or(rest.len());
            add["element"] = json!(&rest[..sign]);
            if let Some(count) = hydrogens {
                add["hydrogens"] = json!({"mode": "specified", "count": count});
            }
            let charge: i64 = match &rest[sign..] {
                "" => 0,
                "+" => 1,
                "-" => -1,
                other => other.parse().unwrap(),
            };
            let mut update = json!({"op": "update_atom", "atom": format!("a{}", index + 1)});
            if charge != 0 {
                update["charge"] = json!(charge);
            }
            if digits > 0 {
                update["isotope"] = json!(spec[..digits].parse::<u16>().unwrap());
            }
            if update.as_object().unwrap().len() > 2 {
                updates.push(update);
            }
        }
        adds.push(add);
    }
    let mut all = adds;
    all.extend(updates);
    for (first, second, kind) in bonds {
        let mut bond =
            json!({"op": "add_bond", "begin": format!("${first}"), "end": format!("${second}")});
        for (key, value) in bond_fields(kind).as_object().unwrap() {
            bond[key] = value.clone();
        }
        all.push(bond);
    }
    all
}

fn draw(app: &App, title: &str, atoms: &[&str], bonds: &[(usize, usize, &str)]) -> String {
    let atoms: Vec<String> = atoms.iter().map(|a| (*a).to_owned()).collect();
    let bonds: Vec<_> = bonds
        .iter()
        .map(|&(a, b, k)| (a, b, k.to_owned()))
        .collect();
    draw_owned(app, title, &atoms, &bonds)
}

fn draw_owned(
    app: &App,
    title: &str,
    atoms: &[String],
    bonds: &[(usize, usize, String)],
) -> String {
    let id = app.create_document(None, Some(title)).unwrap().document.id;
    let commands = commands(atoms, bonds);
    assert!(commands.len() <= 500, "a fixture fits one transaction");
    let drawn = call(
        app,
        "draw",
        "document.transaction",
        json!({"document_id": id, "expected_revision": 0, "commands": commands}),
    );
    assert_eq!(drawn["outcome"], "ok", "{drawn}");
    id
}

fn search(app: &App, request_id: &str, query: &str, target: &str) -> Value {
    call(
        app,
        request_id,
        "chem.substructure",
        json!({"query_document_id": query, "target_document_id": target}),
    )
}

/// A reply's mappings as target atom numbers (zero-based) in query-atom order.
fn target_indices(reply: &Value) -> Vec<Vec<u64>> {
    let mut mappings: Vec<Vec<u64>> = reply["result"]["mappings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|mapping| {
            let mut pairs: Vec<(u64, u64)> = mapping["atoms"]
                .as_array()
                .unwrap()
                .iter()
                .map(|pair| {
                    let number =
                        |key: &str| pair[key].as_str().unwrap()[1..].parse::<u64>().unwrap() - 1;
                    (number("query"), number("target"))
                })
                .collect();
            pairs.sort_unstable();
            pairs.into_iter().map(|(_, target)| target).collect()
        })
        .collect();
    mappings.sort();
    mappings
}

fn fixtures() -> Vec<Value> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/query-fixtures.json"
    );
    let document: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(document["schema"], 1);
    document["fixtures"].as_array().unwrap().clone()
}

fn graph_of(spec: &Value) -> (Vec<String>, Vec<(usize, usize, String)>) {
    let atoms = spec["atoms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap().to_owned())
        .collect();
    let bonds = spec["bonds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            (
                usize::try_from(b[0].as_u64().unwrap()).unwrap(),
                usize::try_from(b[1].as_u64().unwrap()).unwrap(),
                b[2].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    (atoms, bonds)
}

#[test]
fn every_fixture_gives_its_declared_outcome() {
    let fixtures = fixtures();
    let (mut answered, mut refused) = (0, 0);
    for fixture in &fixtures {
        let name = fixture["name"].as_str().unwrap();
        let (app, _data) = application();
        let (query_atoms, query_bonds) = graph_of(&fixture["query"]);
        let (target_atoms, target_bonds) = graph_of(&fixture["target"]);
        let query = draw_owned(&app, "query", &query_atoms, &query_bonds);
        let target = draw_owned(&app, "target", &target_atoms, &target_bonds);
        let reply = search(&app, "fixture", &query, &target);
        assert_eq!(reply["model_id"], "chemifree-substructure-1", "{name}");
        match (fixture.get("expect"), fixture.get("refused")) {
            (Some(expect), None) => {
                answered += 1;
                assert_eq!(reply["outcome"], "ok", "{name}: {reply}");
                let mut expected: Vec<Vec<u64>> = expect["mappings"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|m| {
                        m.as_array()
                            .unwrap()
                            .iter()
                            .map(|n| n.as_u64().unwrap())
                            .collect()
                    })
                    .collect();
                expected.sort();
                assert_eq!(target_indices(&reply), expected, "{name}");
            }
            (None, Some(refusal)) => {
                refused += 1;
                assert_eq!(reply["outcome"], "unsupported", "{name}: {reply}");
                assert_eq!(reply["result"], Value::Null, "{name}");
                let diagnostic = &reply["diagnostics"][0];
                assert_eq!(diagnostic["code"], refusal["code"], "{name}");
                assert_eq!(diagnostic["details"]["side"], refusal["side"], "{name}");
                assert_eq!(diagnostic["details"]["reason"], refusal["reason"], "{name}");
            }
            other => panic!("{name} must declare exactly one of expect and refused: {other:?}"),
        }
    }
    assert!(
        answered >= 30 && refused >= 10,
        "{answered} answered, {refused} refused"
    );
}

#[test]
fn a_mapping_names_query_and_target_atoms_and_bonds_unambiguously() {
    let (app, _data) = application();
    // The query's ids (a1, a2, b1) and the target's (a1..a3, b1, b2) overlap on purpose.
    let query = draw(&app, "C-O", &["C", "O"], &[(0, 1, "-")]);
    let target = draw(
        &app,
        "ethanol",
        &["C", "C", "O"],
        &[(0, 1, "-"), (1, 2, "-")],
    );
    let reply = search(&app, "ids", &query, &target);
    assert_eq!(reply["outcome"], "ok", "{reply}");
    let result = &reply["result"];
    assert_eq!(
        result["mappings"],
        json!([{
            "atoms": [{"query": "a1", "target": "a2"}, {"query": "a2", "target": "a3"}],
            "bonds": [{"query": "b1", "target": "b2"}],
        }])
    );
    assert_eq!(result["query"]["document"]["id"], query);
    assert_eq!(result["target"]["document"]["id"], target);
    assert_eq!(result["aromaticity_model"], "chemifree-aromaticity-2");
    assert_eq!(
        result["limits"],
        json!({"query_atoms": 64, "search_steps": 2_000_000, "mappings": 1_000})
    );
    assert_eq!(
        reply["document_revision"],
        result["query"]["document"]["revision"]
    );
}

#[test]
fn every_bond_mapping_joins_the_images_of_its_atoms() {
    let (app, _data) = application();
    let kekule: Vec<_> = (0..6)
        .map(|i| (i, (i + 1) % 6, if i % 2 == 0 { "=" } else { "-" }))
        .collect();
    let aromatic: Vec<_> = (0..6).map(|i| (i, (i + 1) % 6, ":")).collect();
    let query = draw(&app, "Kekule", &["C"; 6], &kekule);
    let target = draw(&app, "aromatic", &["C"; 6], &aromatic);
    let reply = search(&app, "bonds", &query, &target);
    let mappings = reply["result"]["mappings"].as_array().unwrap();
    assert_eq!(mappings.len(), 12);
    let ends = |view: &Value, bond: &str| -> (String, String) {
        let found = view["bonds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["id"] == bond)
            .unwrap();
        (
            found["begin"].as_str().unwrap().to_owned(),
            found["end"].as_str().unwrap().to_owned(),
        )
    };
    for mapping in mappings {
        let image = |atom: &str| -> String {
            mapping["atoms"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["query"] == atom)
                .unwrap()["target"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        assert_eq!(mapping["bonds"].as_array().unwrap().len(), 6);
        for pair in mapping["bonds"].as_array().unwrap() {
            let (qa, qz) = ends(
                &reply["result"]["query"]["document"],
                pair["query"].as_str().unwrap(),
            );
            let (ta, tz) = ends(
                &reply["result"]["target"]["document"],
                pair["target"].as_str().unwrap(),
            );
            let joined = (image(&qa), image(&qz));
            assert!(
                joined == (ta.clone(), tz.clone()) || joined == (tz, ta),
                "{mapping}"
            );
        }
    }
}

#[test]
fn a_search_changes_no_document_and_its_request_id_does_not_freeze_a_read() {
    let (app, _data) = application();
    let query = draw(&app, "C", &["C"], &[]);
    let target = draw(&app, "CC", &["C", "C"], &[(0, 1, "-")]);
    let before = (
        serde_json::to_value(app.document(&query).unwrap()).unwrap(),
        serde_json::to_value(app.document(&target).unwrap()).unwrap(),
    );
    let first = search(&app, "same-id", &query, &target);
    assert_eq!(first["result"]["mappings"].as_array().unwrap().len(), 2);
    assert_eq!(first["result"]["query"]["document"], before.0["document"]);
    assert_eq!(first["result"]["query"]["session"], before.0["session"]);
    assert_eq!(first["result"]["target"]["document"], before.1["document"]);
    assert_eq!(first["result"]["target"]["session"], before.1["session"]);
    assert_eq!(
        (
            serde_json::to_value(app.document(&query).unwrap()).unwrap(),
            serde_json::to_value(app.document(&target).unwrap()).unwrap(),
        ),
        before,
        "neither document, revision, session nor history changed"
    );
    // The target changes; the same request ID inspects the then-current drawing.
    let edited = call(
        &app,
        "edit",
        "document.transaction",
        json!({"document_id": target, "expected_revision": 1,
               "commands": [{"op": "add_atom", "element": "C", "position": {"x": 90, "y": 0}}]}),
    );
    assert_eq!(edited["outcome"], "ok");
    let second = search(&app, "same-id", &query, &target);
    assert_eq!(second["result"]["mappings"].as_array().unwrap().len(), 3);
    assert_eq!(second["result"]["target"]["document"]["revision"], 2);
}

#[test]
fn a_drawing_can_be_searched_in_itself_and_the_snapshots_agree() {
    let (app, _data) = application();
    let propane = draw(
        &app,
        "propane",
        &["C", "C", "C"],
        &[(0, 1, "-"), (1, 2, "-")],
    );
    let reply = search(&app, "self", &propane, &propane);
    assert_eq!(target_indices(&reply), vec![vec![0, 1, 2], vec![2, 1, 0]]);
    assert_eq!(reply["result"]["query"], reply["result"]["target"]);
}

#[test]
fn an_unknown_or_malformed_document_is_a_document_error_not_an_empty_answer() {
    let (app, _data) = application();
    let real = draw(&app, "C", &["C"], &[]);
    let reply = search(&app, "missing", "no-such-document", &real);
    assert_eq!(reply["outcome"], "error");
    assert_eq!(reply["result"], Value::Null);
    assert_eq!(reply["diagnostics"][0]["code"], "document_not_found");
    let reply = search(&app, "missing-target", &real, "no-such-document");
    assert_eq!(reply["diagnostics"][0]["code"], "document_not_found");
    let reply = search(&app, "bad-id", &real, "Not A Valid ID");
    assert_eq!(reply["outcome"], "error");
    assert_eq!(reply["diagnostics"][0]["code"], "document_not_found");
    let unknown_parameter = call(
        &app,
        "extra",
        "chem.substructure",
        json!({"query_document_id": real, "target_document_id": real, "max_mappings": 5}),
    );
    assert_eq!(
        unknown_parameter["diagnostics"][0]["code"],
        "invalid_request"
    );
    assert_eq!(unknown_parameter["diagnostics"][0]["path"], "parameters");
    let missing = call(
        &app,
        "none",
        "chem.substructure",
        json!({"query_document_id": real}),
    );
    assert_eq!(missing["diagnostics"][0]["code"], "invalid_request");
}

#[test]
fn the_legacy_profile_runs_nothing() {
    let (app, _data) = application();
    let real = draw(&app, "C", &["C"], &[]);
    let reply = call_as(
        &app,
        "legacy-observed",
        "legacy",
        "chem.substructure",
        json!({"query_document_id": real, "target_document_id": real}),
    );
    assert_eq!(reply["outcome"], "unsupported");
    assert_eq!(reply["diagnostics"][0]["code"], "profile_unavailable");
}

#[test]
fn a_closed_saved_document_is_searched_from_storage() {
    let (app, _data) = application();
    let target = draw(&app, "CC", &["C", "C"], &[(0, 1, "-")]);
    app.save(&target, None, 1).unwrap();
    app.close(&target, None, false, None).unwrap();
    let query = draw(&app, "C", &["C"], &[]);
    let reply = search(&app, "closed", &query, &target);
    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["result"]["mappings"].as_array().unwrap().len(), 2);
    let open: Vec<_> = app
        .list_documents(&ListQuery::default())
        .unwrap()
        .documents
        .into_iter()
        .filter(|entry| entry.open)
        .collect();
    assert_eq!(open.len(), 1, "searching did not open the closed document");
}

#[test]
fn an_empty_or_oversized_query_is_refused_with_its_own_code_and_both_snapshots() {
    let (app, _data) = application();
    let target = draw(&app, "C", &["C"], &[]);
    let empty = app
        .create_document(None, Some("empty"))
        .unwrap()
        .document
        .id;
    let reply = search(&app, "empty", &empty, &target);
    assert_eq!(reply["outcome"], "error");
    assert_eq!(reply["diagnostics"][0]["code"], "empty_query");
    assert_eq!(reply["result"], Value::Null);
    let big = draw(&app, "big", &vec!["C"; 65], &[]);
    let reply = search(&app, "big", &big, &target);
    let diagnostic = &reply["diagnostics"][0];
    assert_eq!(diagnostic["code"], "query_too_large");
    assert_eq!(diagnostic["details"]["atoms"], 65);
    assert_eq!(diagnostic["details"]["limit"], 64);
    assert_eq!(
        diagnostic["details"]["query_snapshot"]["document"]["id"],
        big
    );
    assert_eq!(
        diagnostic["details"]["target_snapshot"]["document"]["id"],
        target
    );
    let at_limit = draw(&app, "at limit", &vec!["C"; 64], &[]);
    assert_eq!(search(&app, "limit", &at_limit, &target)["outcome"], "ok");
}

#[test]
fn a_refusal_carries_the_exact_snapshots_of_both_documents() {
    let (app, _data) = application();
    let kekule: Vec<_> = (0..6)
        .map(|i| (i, (i + 1) % 6, if i % 2 == 0 { "=" } else { "-" }))
        .collect();
    let query = draw(&app, "benzene", &["C"; 6], &kekule);
    let mut tetralin = kekule.clone();
    tetralin.extend([
        (0, 6, "-"),
        (6, 7, "-"),
        (7, 8, "-"),
        (8, 9, "-"),
        (9, 5, "-"),
    ]);
    let target = draw(&app, "tetralin", &["C"; 10], &tetralin);
    let before = serde_json::to_value(app.document(&target).unwrap()).unwrap();
    let reply = search(&app, "tetralin", &query, &target);
    assert_eq!(reply["outcome"], "unsupported");
    assert_eq!(reply["model_id"], "chemifree-substructure-1");
    let diagnostic = &reply["diagnostics"][0];
    assert_eq!(diagnostic["code"], "match_unsupported");
    let details = &diagnostic["details"];
    assert_eq!(details["side"], "target");
    assert_eq!(details["reason"], "fused_partially_aromatic");
    assert_eq!(details["item"], "bond");
    assert_eq!(details["query_item"], "bond");
    assert_eq!(details["target_snapshot"]["document"], before["document"]);
    assert_eq!(details["target_snapshot"]["session"], before["session"]);
    assert_eq!(details["query_snapshot"]["document"]["id"], query);
    assert_eq!(
        reply["document_revision"],
        details["query_snapshot"]["document"]["revision"]
    );
}

#[test]
fn too_many_mappings_and_an_exhausted_search_are_limits_not_short_lists() {
    let (app, _data) = application();
    let pair = draw(&app, "two carbons", &["C", "C"], &[]);
    let many = draw(&app, "40 carbons", &vec!["C"; 40], &[]);
    let reply = search(&app, "mappings", &pair, &many);
    assert_eq!(reply["outcome"], "error");
    assert_eq!(reply["result"], Value::Null);
    let diagnostic = &reply["diagnostics"][0];
    assert_eq!(diagnostic["code"], "match_limit");
    assert_eq!(diagnostic["details"]["limit_kind"], "mappings");
    assert_eq!(diagnostic["details"]["limit"], 1_000);

    // A 15-ring cannot map into a grid (it has no odd cycle), but proving it needs a long search.
    let ring: Vec<_> = (0..15).map(|i| (i, (i + 1) % 15, "-")).collect();
    let ring15 = draw(&app, "15-ring", &["C"; 15], &ring);
    let mut grid_bonds = Vec::new();
    for row in 0..14 {
        for column in 0..14 {
            let here = row * 14 + column;
            if column + 1 < 14 {
                grid_bonds.push((here, here + 1, "-"));
            }
            if row + 1 < 14 {
                grid_bonds.push((here, here + 14, "-"));
            }
        }
    }
    // 196 atoms and 364 bonds take 560 commands: draw the atoms, then the bonds in a second
    // transaction naming the atoms by ID.
    let grid = app.create_document(None, Some("grid")).unwrap().document.id;
    let atoms: Vec<Value> = (0..196)
        .map(|i| json!({"op": "add_atom", "element": "C", "position": {"x": (i % 14) * 20, "y": (i / 14) * 20}}))
        .collect();
    let first = call(
        &app,
        "grid-atoms",
        "document.transaction",
        json!({"document_id": grid, "expected_revision": 0, "commands": atoms}),
    );
    assert_eq!(first["outcome"], "ok");
    let bonds: Vec<Value> = grid_bonds
        .iter()
        .map(|&(a, b, _)| json!({"op": "add_bond", "begin": format!("a{}", a + 1), "end": format!("a{}", b + 1), "order": "single"}))
        .collect();
    let second = call(
        &app,
        "grid-bonds",
        "document.transaction",
        json!({"document_id": grid, "expected_revision": 1, "commands": bonds}),
    );
    assert_eq!(second["outcome"], "ok", "{second}");
    let reply = search(&app, "steps", &ring15, &grid);
    assert_eq!(reply["outcome"], "error");
    let diagnostic = &reply["diagnostics"][0];
    assert_eq!(diagnostic["code"], "match_limit");
    assert_eq!(diagnostic["details"]["limit_kind"], "search_steps");
    assert_eq!(diagnostic["details"]["limit"], 2_000_000);
}

#[test]
fn the_catalog_and_the_operation_agree_for_every_set_of_connected_codecs() {
    use chemifree_providers::interchange_codec::LocalInterchangeCodec;
    use chemifree_providers::{OpenSmilesCodec, V2000Codec};
    let mut notes = std::collections::BTreeSet::new();
    for mask in 0..8_u8 {
        let data = tempfile::tempdir().unwrap();
        let mut app = Application::new(LocalCapabilities, FileDocumentRepository::new(data.path()));
        if mask & 1 != 0 {
            app = app.with_ctfile_codec(V2000Codec);
        }
        if mask & 2 != 0 {
            app = app.with_interchange_codec(LocalInterchangeCodec);
        }
        if mask & 4 != 0 {
            app = app.with_smiles_codec(OpenSmilesCodec);
        }
        let catalog = serde_json::to_value(app.operation_catalog()).unwrap();
        let operations = catalog["operations"].as_array().unwrap();
        let entry = operations
            .iter()
            .find(|entry| entry["id"] == "chem.substructure")
            .unwrap();
        assert_eq!(entry["execution"], "executable", "codecs {mask:03b}");
        assert_eq!(entry["representation"], "supported", "codecs {mask:03b}");
        notes.insert(entry["note"].as_str().unwrap().to_owned());
        // Query atoms and bonds are recorded by the transaction; the note says what matches them.
        let transaction = operations
            .iter()
            .find(|entry| entry["id"] == "document.transaction")
            .unwrap();
        let recorded = transaction["sub_operations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|part| part["id"] == "query_atom_or_bond")
            .unwrap();
        assert!(
            recorded["note"]
                .as_str()
                .unwrap()
                .contains("chem.substructure matches"),
            "codecs {mask:03b}"
        );
        let carbon = draw(&app, "C", &["C"], &[]);
        let reply = search(&app, "each-set", &carbon, &carbon);
        assert_eq!(reply["outcome"], "ok", "codecs {mask:03b}: {reply}");
    }
    assert_eq!(
        notes.len(),
        1,
        "one wording whatever codecs are connected: {notes:?}"
    );
}

/// A small deterministic generator, so a failing seed can be replayed.
struct Random(u64);

impl Random {
    fn below(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        usize::try_from(self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33).unwrap() % bound
    }
}

#[test]
fn seeded_malformed_requests_always_get_a_reply_and_change_nothing() {
    let (app, _data) = application();
    let ethane = draw(&app, "ethane", &["C", "C"], &[(0, 1, "-")]);
    let before = serde_json::to_value(app.document(&ethane).unwrap()).unwrap();
    let valid = json!({
        "contract": 1, "request_id": "fuzz", "profile": "preserve-semantics", "operation": "chem.substructure",
        "parameters": {"query_document_id": ethane, "target_document_id": ethane},
    })
    .to_string();
    let junk: [Value; 8] = [
        json!(null),
        json!(7),
        json!(-1.5),
        json!("a1"),
        json!([]),
        json!({}),
        json!([1, [2]]),
        json!({"x": true}),
    ];
    for seed in 1..=400_usize {
        let mut random = Random((seed as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let bytes = match random.below(3) {
            // A byte of the valid request flipped or removed.
            0 => {
                let mut bytes = valid.clone().into_bytes();
                let at = random.below(bytes.len());
                if random.below(2) == 0 {
                    bytes[at] = u8::try_from(random.below(256)).unwrap();
                } else {
                    bytes.remove(at);
                }
                bytes
            }
            // A parameter replaced by a value of the wrong kind.
            1 => {
                let mut request: Value = serde_json::from_str(&valid).unwrap();
                let key = ["query_document_id", "target_document_id"][random.below(2)];
                request["parameters"][key] = junk[random.below(junk.len())].clone();
                request.to_string().into_bytes()
            }
            // A parameter dropped or added.
            _ => {
                let mut request: Value = serde_json::from_str(&valid).unwrap();
                if random.below(2) == 0 {
                    request["parameters"]
                        .as_object_mut()
                        .unwrap()
                        .remove("target_document_id");
                } else {
                    request["parameters"]["limit"] = junk[random.below(junk.len())].clone();
                }
                request.to_string().into_bytes()
            }
        };
        let reply =
            serde_json::to_value(app.execute(&bytes)).unwrap_or_else(|_| panic!("seed {seed}"));
        let envelope = reply["envelope"].as_str().unwrap();
        assert!(
            envelope == "operation_result" || envelope == "protocol_error",
            "seed {seed}: {reply}"
        );
        if reply["outcome"] != "ok" {
            assert_eq!(
                reply["result"],
                Value::Null,
                "seed {seed}: a failure carries no result"
            );
            assert!(
                !reply["diagnostics"].as_array().unwrap().is_empty(),
                "seed {seed}"
            );
        }
    }
    assert_eq!(
        serde_json::to_value(app.document(&ethane).unwrap()).unwrap(),
        before
    );
}
