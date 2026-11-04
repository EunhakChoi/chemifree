//! MOL V3000 `COLLECTION` blocks over the wire: P27 enhanced-stereo groups written on export and
//! read on import, the chiral flag rule, the consent a center in no group needs, the refusals
//! that leave a document unchanged, appending into a document that has groups, and the
//! `mol-v2000` / `mol-v3000` format ids.
//!
//! Fixtures are hand-authored. Expected wire text is written out here, not derived from the code.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, V2000Codec};
use serde_json::{Value, json};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn setup() -> (App, TempDir, String) {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    )
    .with_ctfile_codec(V2000Codec);
    let id = new_document(&app, "T");
    (app, directory, id)
}

fn new_document(app: &App, title: &str) -> String {
    let created = call(app, "create", "document.create", json!({"title": title}));
    created["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Every request gets its own `request_id` (a reused one is refused as a different request).
fn call(app: &App, request: &str, operation: &str, parameters: Value) -> Value {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let request = format!("{request}-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let body = json!({"contract": 1, "request_id": request, "profile": "preserve-semantics",
                      "operation": operation, "parameters": parameters});
    let reply = serde_json::to_value(app.execute(body.to_string().as_bytes())).unwrap();
    let losses = reply["loss_report"].as_array().unwrap();
    match reply["outcome"].as_str().unwrap() {
        "ok" => assert!(
            losses
                .iter()
                .all(|l| matches!(l["disposition"].as_str(), Some("retained" | "normalized"))),
            "an ok reply carries a lossy entry: {reply}"
        ),
        "partial" => assert!(!losses.is_empty(), "a partial reply with no loss: {reply}"),
        _ => {}
    }
    reply
}

fn features(reply: &Value) -> Vec<String> {
    reply["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .map(|loss| loss["feature"].as_str().unwrap().to_owned())
        .collect()
}

/// A V3000 file of `blocks` centers: block `b` is a carbon (atom `4b+1`) with F above, Cl lower
/// right (wedge, narrow at the center) and Br lower left, `lines` as the COLLECTION block, and
/// the chiral flag `chiral`. Every center derives a tetrahedral configuration from its wedge.
fn file(blocks: usize, chiral: u8, lines: &[&str]) -> String {
    let mut atoms = String::new();
    let mut bonds = String::new();
    for b in 0..blocks {
        let (n, x) = (4 * b, 5.0 * b as f64);
        for (k, (element, dx, dy)) in [
            ("C", 0.0, 0.0),
            ("F", 0.0, 1.5),
            ("Cl", 1.3, -0.75),
            ("Br", -1.3, -0.75),
        ]
        .into_iter()
        .enumerate()
        {
            atoms += &format!(
                "M  V30 {} {element} {:.4} {dy:.4} 0.0000 0\n",
                n + k + 1,
                x + dx
            );
        }
        let m = 3 * b;
        bonds += &format!("M  V30 {} 1 {} {}\n", m + 1, n + 1, n + 2);
        bonds += &format!("M  V30 {} 1 {} {} CFG=1\n", m + 2, n + 1, n + 3);
        bonds += &format!("M  V30 {} 1 {} {}\n", m + 3, n + 1, n + 4);
    }
    let collection = if lines.is_empty() {
        String::new()
    } else {
        let body: String = lines.iter().map(|l| format!("M  V30 {l}\n")).collect();
        format!("M  V30 BEGIN COLLECTION\n{body}M  V30 END COLLECTION\n")
    };
    format!(
        "T\n\n\n  0  0  0     0  0            999 V3000\nM  V30 BEGIN CTAB\nM  V30 COUNTS {} {} 0 0 {chiral}\nM  V30 BEGIN ATOM\n{atoms}M  V30 END ATOM\nM  V30 BEGIN BOND\n{bonds}M  V30 END BOND\n{collection}M  V30 END CTAB\nM  END\n",
        4 * blocks,
        3 * blocks
    )
}

fn preview(app: &App, id: &str, revision: u64, source: &str) -> Value {
    call(
        app,
        "preview",
        "document.import",
        json!({"document_id": id, "expected_revision": revision, "format": "mol",
               "source": source, "record": 1, "preview": true}),
    )
}

/// Imports `source` (preview, then commit with the digest and acceptance) and returns the commit.
fn import(app: &App, id: &str, revision: u64, source: &str) -> Value {
    let previewed = preview(app, id, revision, source);
    assert_eq!(previewed["outcome"], "partial", "{previewed}");
    let digest = previewed["result"]["preview_digest"].clone();
    let committed = call(
        app,
        "import",
        "document.import",
        json!({"document_id": id, "expected_revision": revision, "format": "mol",
               "source": source, "record": 1, "preview_digest": digest, "accept_loss": true}),
    );
    assert_eq!(committed["outcome"], "partial", "{committed}");
    committed
}

fn document(app: &App, id: &str) -> Value {
    call(app, "snap", "document.snapshot", json!({"document_id": id}))["result"]["document"].clone()
}

fn export(app: &App, id: &str, extra: Value) -> Value {
    let mut parameters = json!({"format": "mol", "records": [{"document_id": id}]});
    for (key, value) in extra.as_object().unwrap() {
        parameters[key] = value.clone();
    }
    call(app, "export", "document.export", parameters)
}

/// The groups as (kind, stored number, members) in id order.
fn groups(doc: &Value) -> Vec<(String, Value, Vec<String>)> {
    doc["stereo"]["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| {
            (
                g["kind"].as_str().unwrap().to_owned(),
                g["stored_number"].clone(),
                g["members"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|m| m.as_str().unwrap().to_owned())
                    .collect(),
            )
        })
        .collect()
}

/// The groups with every number and identifier removed: the kind and the member set.
fn partition(doc: &Value) -> BTreeSet<(String, Vec<String>)> {
    groups(doc)
        .into_iter()
        .map(|(kind, _, members)| (kind, members))
        .collect()
}

fn stated(kind: &str, number: Value, members: &[&str]) -> (String, Value, Vec<String>) {
    (
        kind.to_owned(),
        number,
        members.iter().map(|m| (*m).to_owned()).collect(),
    )
}

fn transact(app: &App, id: &str, revision: u64, commands: Value) -> Value {
    let reply = call(
        app,
        "edit",
        "document.transaction",
        json!({"document_id": id, "expected_revision": revision, "commands": commands}),
    );
    assert!(
        matches!(reply["outcome"].as_str(), Some("ok" | "partial")),
        "{reply}"
    );
    reply
}

const SIX: [&str; 5] = [
    "MDLV30/STEABS ATOMS=(1 1)",
    "MDLV30/STERAC-128 ATOMS=(2 5 9)",
    "MDLV30/STERAC0 ATOMS=(1 13)",
    "MDLV30/STEREL258 ATOMS=(1 17)",
    "MDLV30/STEREL0 ATOMS=(1 21)",
];

#[test]
fn every_kind_and_awkward_number_round_trips_through_export_and_import() {
    let (app, _directory, id) = setup();
    // Import states numbers -128 and 0 (and), 258 and 0 (or) exactly as written.
    let first = import(&app, &id, 0, &file(6, 1, &SIX));
    assert!(
        !features(&first).contains(&"chiral_flag".to_owned()),
        "{first}"
    );
    let before = document(&app, &id);
    assert_eq!(
        groups(&before),
        [
            stated("absolute", Value::Null, &["a1"]),
            stated("and", json!(-128), &["a5", "a9"]),
            stated("and", json!(0), &["a13"]),
            stated("or", json!(258), &["a17"]),
            stated("or", json!(0), &["a21"]),
        ]
    );
    // Export: every group is written, and no consent is needed (every center is in a group).
    let written = export(&app, &id, json!({"ctab": "v3000"}));
    assert_eq!(written["outcome"], "partial", "{written}");
    let text = written["result"]["content"].as_str().unwrap();
    assert!(text.contains("M  V30 COUNTS 24 18 0 0 1\n"), "{text}");
    assert!(
        text.contains(
            "M  V30 BEGIN COLLECTION\nM  V30 MDLV30/STEABS ATOMS=(1 1)\nM  V30 MDLV30/STERAC-128 ATOMS=(2 5 9)\nM  V30 MDLV30/STERAC0 ATOMS=(1 13)\nM  V30 MDLV30/STEREL0 ATOMS=(1 21)\nM  V30 MDLV30/STEREL258 ATOMS=(1 17)\nM  V30 END COLLECTION\n"
        ),
        "{text}"
    );
    assert!(
        !features(&written).contains(&"stereo_group".to_owned()),
        "{written}"
    );
    let identity = written["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .find(|loss| loss["feature"] == "stereo_group_identity")
        .unwrap_or_else(|| panic!("{written}"));
    assert_eq!(
        identity["source_path"],
        "records[1].groups[e1, e2, e3, e4, e5]"
    );
    assert_eq!(identity["disposition"], "dropped");
    // Import back: the same numbers and members.
    let other = new_document(&app, "Back");
    import(&app, &other, 0, text);
    let after = document(&app, &other);
    // The file lists `or` 0 before `or` 258, so the identifiers differ; kind, number and
    // members do not.
    let sorted = |doc: &Value| {
        let mut all = groups(doc);
        all.sort_by_key(|(kind, number, _)| (kind.clone(), number.as_i64()));
        all
    };
    assert_eq!(sorted(&after), sorted(&before));
    assert_eq!(partition(&after), partition(&before));
}

#[test]
fn an_omitted_number_is_written_as_zero_and_the_flag_follows_the_absolute_group() {
    let (app, _directory, id) = setup();
    import(&app, &id, 0, &file(2, 0, &[]));
    transact(
        &app,
        &id,
        1,
        json!([{"op": "create_stereo_group", "kind": "or", "atoms": ["a1"], "number": null},
               {"op": "create_stereo_group", "kind": "and", "atoms": ["a5"], "number": null}]),
    );
    let written = export(&app, &id, json!({"ctab": "v3000"}));
    let text = written["result"]["content"].as_str().unwrap();
    assert!(text.contains("MDLV30/STERAC0 ATOMS=(1 5)\n"), "{text}");
    assert!(text.contains("MDLV30/STEREL0 ATOMS=(1 1)\n"), "{text}");
    assert!(
        text.contains("COUNTS 8 6 0 0 0\n"),
        "no absolute group, so the flag is 0: {text}"
    );
    let back = new_document(&app, "Back");
    import(&app, &back, 0, text);
    assert_eq!(
        groups(&document(&app, &back)),
        [
            stated("and", json!(0), &["a5"]),
            stated("or", json!(0), &["a1"]),
        ],
        "group ids follow the file order, the stored numbers are the written zeros"
    );
}

#[test]
fn a_center_in_no_group_needs_consent_and_a_drawing_without_stereo_does_not() {
    let (app, _directory, id) = setup();
    import(&app, &id, 0, &file(2, 0, &["MDLV30/STEABS ATOMS=(1 1)"]));
    let refused = export(&app, &id, json!({"ctab": "v3000"}));
    assert_eq!(refused["outcome"], "error", "{refused}");
    let diagnostic = &refused["diagnostics"][0];
    assert_eq!(diagnostic["code"], "loss_consent_required");
    assert_eq!(
        diagnostic["details"]["features"],
        json!(["stereo_ungrouped_centers"])
    );
    let loss = &diagnostic["details"]["losses"][0];
    assert_eq!(loss["source_path"], "records[1].atoms[a5]");
    assert_eq!(loss["disposition"], "unknown");
    assert_eq!(loss["semantic_impact"], "stereo");
    let accepted = export(
        &app,
        &id,
        json!({"ctab": "v3000", "accepted_losses": diagnostic["details"]["loss_digest"]}),
    );
    assert_eq!(accepted["outcome"], "partial", "{accepted}");
    // Grouping the center makes the consent unnecessary.
    transact(
        &app,
        &id,
        1,
        json!([{"op": "create_stereo_group", "kind": "or", "atoms": ["a5"], "number": 1}]),
    );
    let all = export(&app, &id, json!({"ctab": "v3000"}));
    assert_eq!(all["outcome"], "partial", "{all}");
    // A drawing with no stereo has no such loss.
    let (plain, _d, plain_id) = setup();
    transact(
        &plain,
        &plain_id,
        0,
        json!([{"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}}]),
    );
    let reply = export(&plain, &plain_id, json!({"ctab": "v3000"}));
    assert!(
        !features(&reply).contains(&"stereo_ungrouped_centers".to_owned()),
        "{reply}"
    );
}

#[test]
fn a_v2000_export_still_needs_consent_for_the_groups_and_writes_none() {
    let (app, _directory, id) = setup();
    import(&app, &id, 0, &file(1, 0, &["MDLV30/STEREL3 ATOMS=(1 1)"]));
    let refused = export(&app, &id, json!({"ctab": "v2000"}));
    assert_eq!(refused["diagnostics"][0]["code"], "loss_consent_required");
    assert!(
        refused["diagnostics"][0]["details"]["features"]
            .as_array()
            .unwrap()
            .contains(&json!("stereo_group")),
        "{refused}"
    );
    let accepted = export(
        &app,
        &id,
        json!({"ctab": "v2000",
               "accepted_losses": refused["diagnostics"][0]["details"]["loss_digest"]}),
    );
    let text = accepted["result"]["content"].as_str().unwrap();
    assert!(
        !text.contains("COLLECTION") && !text.contains("V3000"),
        "{text}"
    );
}

#[test]
fn split_and_merge_after_import_keep_the_partition_through_a_round_trip() {
    let (app, _directory, id) = setup();
    import(
        &app,
        &id,
        0,
        &file(
            4,
            0,
            &[
                "MDLV30/STEREL1 ATOMS=(2 1 5)",
                "MDLV30/STEREL2 ATOMS=(2 9 13)",
            ],
        ),
    );
    let edited = transact(
        &app,
        &id,
        1,
        json!([{"op": "split_stereo_group", "group": "e1", "atoms": ["a5"], "number": 7},
               {"op": "merge_stereo_groups", "into": "e2", "from": "e1"}]),
    );
    assert_eq!(edited["outcome"], "partial", "{edited}");
    let before = document(&app, &id);
    assert_eq!(
        partition(&before),
        BTreeSet::from([
            ("or".to_owned(), vec!["a5".to_owned()]),
            (
                "or".to_owned(),
                vec!["a1".to_owned(), "a9".to_owned(), "a13".to_owned()]
            ),
        ]),
        "{before}"
    );
    let written = export(&app, &id, json!({"ctab": "v3000"}));
    let text = written["result"]["content"].as_str().unwrap();
    let back = new_document(&app, "Back");
    import(&app, &back, 0, text);
    assert_eq!(partition(&document(&app, &back)), partition(&before));
}

fn refusal_reason(reply: &Value) -> (String, String) {
    assert_eq!(reply["outcome"], "error", "{reply}");
    (
        reply["diagnostics"][0]["code"].as_str().unwrap().to_owned(),
        reply["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .to_owned(),
    )
}

#[test]
fn a_refused_collection_leaves_the_document_and_its_revision_alone() {
    let (app, _directory, id) = setup();
    import(&app, &id, 0, &file(1, 0, &[]));
    let before = document(&app, &id);
    let cases = [
        (
            "a plain atom",
            file(1, 0, &["MDLV30/STEABS ATOMS=(1 2)"]),
            "v3000_collection_member_not_stereocenter",
        ),
        (
            "a center and a plain atom",
            file(1, 0, &["MDLV30/STEREL1 ATOMS=(2 1 4)"]),
            "v3000_collection_member_not_stereocenter",
        ),
        (
            "an empty collection",
            file(1, 0, &["MDLV30/STEABS ATOMS=(0)"]),
            "v3000_empty_collection",
        ),
        (
            "two STEABS",
            file(
                2,
                0,
                &["MDLV30/STEABS ATOMS=(1 1)", "MDLV30/STEABS ATOMS=(1 5)"],
            ),
            "v3000_duplicate_collection",
        ),
        (
            "one type and index twice",
            file(
                2,
                0,
                &["MDLV30/STEREL1 ATOMS=(1 1)", "MDLV30/STEREL1 ATOMS=(1 5)"],
            ),
            "v3000_duplicate_collection",
        ),
        (
            "an atom in two collections",
            file(
                1,
                0,
                &["MDLV30/STEREL1 ATOMS=(1 1)", "MDLV30/STERAC1 ATOMS=(1 1)"],
            ),
            "v3000_collection_atom_shared",
        ),
        (
            "an atom twice in one",
            file(1, 0, &["MDLV30/STEREL1 ATOMS=(2 1 1)"]),
            "v3000_duplicate_index",
        ),
        (
            "a forward reference",
            file(1, 0, &["MDLV30/STEREL1 ATOMS=(1 9)"]),
            "v3000_unknown_atom_index",
        ),
        (
            "an index beyond int32",
            file(1, 0, &["MDLV30/STEREL2147483648 ATOMS=(1 1)"]),
            "v3000_invalid_collection",
        ),
    ];
    for (name, source, code) in cases {
        let refused = preview(&app, &id, 1, &source);
        let (found, message) = refusal_reason(&refused);
        assert_eq!(found, code, "{name}: {message}");
        assert_eq!(document(&app, &id), before, "{name}");
    }
    // A center with no wedge (a wavy bond derives `unknown`) is not a stereocenter either.
    let wavy = file(1, 0, &["MDLV30/STEABS ATOMS=(1 1)"]).replace("1 3 CFG=1", "1 3 CFG=2");
    let (found, _) = refusal_reason(&preview(&app, &id, 1, &wavy));
    assert_eq!(found, "v3000_collection_member_not_stereocenter");
    // A commit that is refused after its preview changes nothing either.
    let source = file(1, 0, &["MDLV30/STEREL1 ATOMS=(1 1)"]);
    let digest = preview(&app, &id, 1, &source)["result"]["preview_digest"].clone();
    let unaccepted = call(
        &app,
        "commit",
        "document.import",
        json!({"document_id": id, "expected_revision": 1, "format": "mol",
               "source": source, "record": 1, "preview_digest": digest, "accept_loss": false}),
    );
    assert_eq!(unaccepted["outcome"], "error", "{unaccepted}");
    assert_eq!(document(&app, &id), before);
}

#[test]
fn the_extreme_numbers_import_as_stated_and_export_as_stated() {
    let (app, _directory, id) = setup();
    import(
        &app,
        &id,
        0,
        &file(
            2,
            0,
            &[
                "MDLV30/STERAC-2147483648 ATOMS=(1 1)",
                "MDLV30/STERAC2147483647 ATOMS=(1 5)",
            ],
        ),
    );
    let doc = document(&app, &id);
    assert_eq!(
        groups(&doc),
        [
            stated("and", json!(i32::MIN), &["a1"]),
            stated("and", json!(i32::MAX), &["a5"]),
        ]
    );
    let written = export(&app, &id, json!({"ctab": "v3000"}));
    let text = written["result"]["content"].as_str().unwrap();
    assert!(
        text.contains("MDLV30/STERAC-2147483648 ATOMS=(1 1)\n"),
        "{text}"
    );
    assert!(
        text.contains("MDLV30/STERAC2147483647 ATOMS=(1 5)\n"),
        "{text}"
    );
}

fn losses_of<'a>(reply: &'a Value, feature: &str) -> Vec<&'a Value> {
    reply["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|loss| loss["feature"] == feature)
        .collect()
}

#[test]
fn appending_renumbers_a_colliding_number_and_joins_the_absolute_group_with_a_loss() {
    let (app, _directory, id) = setup();
    import(
        &app,
        &id,
        0,
        &file(
            2,
            1,
            &["MDLV30/STEABS ATOMS=(1 1)", "MDLV30/STEREL1 ATOMS=(1 5)"],
        ),
    );
    // The same numbers again: the absolute members join, the or number is replaced by a free one.
    let second = import(
        &app,
        &id,
        1,
        &file(
            3,
            1,
            &[
                "MDLV30/STEABS ATOMS=(1 1)",
                "MDLV30/STEREL1 ATOMS=(1 5)",
                "MDLV30/STEREL2 ATOMS=(1 9)",
            ],
        ),
    );
    let merged = losses_of(&second, "stereo_group_merged");
    assert_eq!(merged.len(), 1, "{second}");
    assert_eq!(merged[0]["disposition"], "normalized");
    assert_eq!(merged[0]["source_path"], "records[1].collections[1]");
    let renumbered = losses_of(&second, "stereo_group_number");
    assert_eq!(renumbered.len(), 1, "{second}");
    assert_eq!(renumbered[0]["source_path"], "records[1].collections[2]");
    assert!(
        renumbered[0]["reason"]
            .as_str()
            .unwrap()
            .contains("numbered 3"),
        "the free number is above the file's own 2: {second}"
    );
    // Atoms of the appended record are a9-a20; its centers are a9, a13 and a17.
    assert_eq!(
        groups(&document(&app, &id)),
        [
            stated("absolute", Value::Null, &["a1", "a9"]),
            stated("or", json!(1), &["a5"]),
            stated("or", json!(3), &["a13"]),
            stated("or", json!(2), &["a17"]),
        ]
    );
}

#[test]
fn two_collisions_in_one_file_get_two_different_free_numbers() {
    let (app, _directory, id) = setup();
    import(
        &app,
        &id,
        0,
        &file(
            2,
            0,
            &["MDLV30/STERAC1 ATOMS=(1 1)", "MDLV30/STERAC2 ATOMS=(1 5)"],
        ),
    );
    import(
        &app,
        &id,
        1,
        &file(
            2,
            0,
            &["MDLV30/STERAC1 ATOMS=(1 1)", "MDLV30/STERAC2 ATOMS=(1 5)"],
        ),
    );
    let numbers: Vec<Value> = groups(&document(&app, &id))
        .into_iter()
        .map(|(_, number, _)| number)
        .collect();
    assert_eq!(numbers, [json!(1), json!(2), json!(3), json!(4)]);
}

#[test]
fn a_renumbered_group_gets_the_smallest_unused_positive_number_never_a_wrapped_one() {
    let (app, _directory, id) = setup();
    import(
        &app,
        &id,
        0,
        &file(1, 0, &["MDLV30/STEREL2147483647 ATOMS=(1 1)"]),
    );
    import(
        &app,
        &id,
        1,
        &file(1, 0, &["MDLV30/STEREL2147483647 ATOMS=(1 1)"]),
    );
    // 2, then a gap-filling 1 is not taken from below zero.
    import(
        &app,
        &id,
        2,
        &file(1, 0, &["MDLV30/STEREL2147483647 ATOMS=(1 1)"]),
    );
    let numbers: Vec<Value> = groups(&document(&app, &id))
        .into_iter()
        .map(|(_, number, _)| number)
        .collect();
    assert_eq!(numbers, [json!(i32::MAX), json!(1), json!(2)]);
}

#[test]
fn a_gap_below_a_used_number_is_filled_first() {
    let (app, _directory, id) = setup();
    import(
        &app,
        &id,
        0,
        &file(
            2,
            0,
            &["MDLV30/STEREL5 ATOMS=(1 1)", "MDLV30/STEREL-3 ATOMS=(1 5)"],
        ),
    );
    import(&app, &id, 1, &file(1, 0, &["MDLV30/STEREL5 ATOMS=(1 1)"]));
    let numbers: Vec<Value> = groups(&document(&app, &id))
        .into_iter()
        .map(|(_, number, _)| number)
        .collect();
    assert_eq!(numbers, [json!(5), json!(-3), json!(1)]);
}

/// Sends a request under the given id (the other helpers make their ids unique).
fn call_with_id(app: &App, request: &str, operation: &str, parameters: Value) -> Value {
    let body = json!({"contract": 1, "request_id": request, "profile": "preserve-semantics",
                      "operation": operation, "parameters": parameters});
    serde_json::to_value(app.execute(body.to_string().as_bytes())).unwrap()
}

#[test]
fn a_completed_import_is_not_replayed_for_another_format_id() {
    let (app, _directory, id) = setup();
    let source = file(1, 0, &[]);
    let preview = preview(&app, &id, 0, &source);
    let digest = preview["result"]["preview_digest"].clone();
    let commit = |format: &str| {
        call_with_id(
            &app,
            "same-id",
            "document.import",
            json!({"document_id": id, "expected_revision": 0, "format": format,
                   "source": source, "record": 1, "preview_digest": digest, "accept_loss": true}),
        )
    };
    let first = commit("mol");
    assert_eq!(first["outcome"], "partial", "{first}");
    let replay = commit("mol");
    assert_eq!(replay["outcome"], "partial", "an identical request replays");
    for format in ["mol-v3000", "mol-v2000"] {
        let other = commit(format);
        assert_eq!(
            other["diagnostics"][0]["code"], "request_id_reused",
            "{format}: {other}"
        );
    }
}

#[test]
fn auto_picks_v3000_for_a_drawing_with_groups() {
    let (app, _directory, id) = setup();
    import(&app, &id, 0, &file(1, 0, &["MDLV30/STEREL1 ATOMS=(1 1)"]));
    let written = export(&app, &id, json!({"ctab": "auto"}));
    assert_eq!(written["result"]["ctab"], "v3000", "{written}");
    assert!(
        written["result"]["content"]
            .as_str()
            .unwrap()
            .contains("MDLV30/STEREL1 ATOMS=(1 1)"),
        "{written}"
    );
}

#[test]
fn a_malformed_ctab_is_a_parameter_error_not_a_format_conflict() {
    let (app, _directory, id) = setup();
    for bad in [json!(5), json!("V3000"), json!(null), json!("v4000")] {
        let reply = export(&app, &id, json!({"format": "mol-v3000", "ctab": bad}));
        assert_eq!(
            reply["diagnostics"][0]["code"], "invalid_request",
            "{bad}: {reply}"
        );
    }
    // The plain format reports the same for the same value.
    let plain = export(&app, &id, json!({"ctab": "V3000"}));
    assert_eq!(plain["diagnostics"][0]["code"], "invalid_request");
    // A valid differing value is still the conflict.
    let conflict = export(&app, &id, json!({"format": "mol-v2000", "ctab": "v3000"}));
    assert_eq!(conflict["diagnostics"][0]["code"], "format_ctab_conflict");
}

#[test]
fn group_presence_does_not_change_chemistry_replies() {
    let (app, _directory, plain) = setup();
    import(&app, &plain, 0, &file(2, 0, &[]));
    let grouped = new_document(&app, "G");
    import(
        &app,
        &grouped,
        0,
        &file(
            2,
            1,
            &["MDLV30/STEABS ATOMS=(1 1)", "MDLV30/STEREL1 ATOMS=(1 5)"],
        ),
    );
    for (operation, key) in [
        ("chem.cip", "document_id"),
        ("chem.validate", "document_id"),
    ] {
        let a = call(&app, "a", operation, json!({key: plain}));
        let b = call(&app, "b", operation, json!({key: grouped}));
        let (mut a, mut b) = (a["result"].clone(), b["result"].clone());
        for result in [&mut a, &mut b] {
            result.as_object_mut().unwrap().remove("document");
            result.as_object_mut().unwrap().remove("snapshot");
        }
        assert_eq!(a, b, "{operation}");
    }
    let identity = call(
        &app,
        "identity",
        "chem.identity",
        json!({"document_id": plain, "other_document_id": grouped, "mode": "chemical"}),
    );
    assert_ne!(identity["outcome"], "error", "{identity}");
    let same = call(
        &app,
        "identity",
        "chem.identity",
        json!({"document_id": plain, "other_document_id": plain, "mode": "chemical"}),
    );
    assert_eq!(
        identity["result"]["comparison"]["equivalent"], true,
        "{identity}"
    );
    assert_eq!(
        identity["result"]["comparison"]["identity"], same["result"]["comparison"]["identity"],
        "group presence must not change the identity code"
    );
}

#[test]
fn the_format_ids_mol_v2000_and_mol_v3000_name_the_dialect() {
    let (app, _directory, id) = setup();
    import(&app, &id, 0, &file(1, 0, &["MDLV30/STEABS ATOMS=(1 1)"]));
    // Export: the id sets the dialect.
    let v3000 = export(&app, &id, json!({"format": "mol-v3000"}));
    assert_eq!(v3000["result"]["ctab"], "v3000", "{v3000}");
    let same = export(&app, &id, json!({"format": "mol-v3000", "ctab": "v3000"}));
    assert_eq!(same["result"]["content"], v3000["result"]["content"]);
    let v2000 = export(&app, &id, json!({"format": "mol-v2000"}));
    assert_eq!(
        v2000["diagnostics"][0]["code"], "loss_consent_required",
        "{v2000}"
    );
    // A `ctab` that disagrees, or `auto`, is refused.
    for (format, ctab) in [
        ("mol-v3000", "v2000"),
        ("mol-v2000", "v3000"),
        ("mol-v3000", "auto"),
    ] {
        let refused = export(&app, &id, json!({"format": format, "ctab": ctab}));
        assert_eq!(
            refused["diagnostics"][0]["code"], "format_ctab_conflict",
            "{format} {ctab}: {refused}"
        );
    }
    // Import: the id must match the file's dialect.
    let (other, _d, target) = setup();
    let source = file(1, 0, &[]);
    let wrong = call(
        &other,
        "wrong",
        "document.import",
        json!({"document_id": target, "expected_revision": 0, "format": "mol-v2000",
               "source": source, "record": 1, "preview": true}),
    );
    assert_eq!(
        wrong["diagnostics"][0]["code"], "format_ctab_conflict",
        "{wrong}"
    );
    let right = call(
        &other,
        "right",
        "document.import",
        json!({"document_id": target, "expected_revision": 0, "format": "mol-v3000",
               "source": source, "record": 1, "preview": true}),
    );
    assert_eq!(right["outcome"], "partial", "{right}");
    let v2000_file = "T\n\n\n  1  0  0  0  0  0  0  0  0  0999 V2000\n    0.0000    0.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0\nM  END\n";
    let refused = call(
        &other,
        "v3000-on-v2000",
        "document.import",
        json!({"document_id": target, "expected_revision": 0, "format": "mol-v3000",
               "source": v2000_file, "record": 1, "preview": true}),
    );
    assert_eq!(
        refused["diagnostics"][0]["code"], "format_ctab_conflict",
        "{refused}"
    );
    // `mol` still reads either.
    assert_eq!(
        preview(&other, &target, 0, v2000_file)["outcome"],
        "partial"
    );
}

/// A wedge center (atoms 1-4) plus a chain of `extra` carbons, so the import has many commands.
fn big_file(extra: usize, lines: &[&str]) -> String {
    let base = file(1, 0, lines);
    let mut atoms = String::new();
    let mut bonds = String::new();
    for k in 0..extra {
        atoms += &format!("M  V30 {} C {:.4} 10.0000 0.0000 0\n", 5 + k, k as f64);
        bonds += &format!(
            "M  V30 {} 1 {} {}\n",
            4 + k,
            if k == 0 { 4 } else { 4 + k },
            5 + k
        );
    }
    base.replace(
        "COUNTS 4 3 0 0 0",
        &format!("COUNTS {} {} 0 0 0", 4 + extra, 3 + extra),
    )
    .replace("M  V30 END ATOM\n", &format!("{atoms}M  V30 END ATOM\n"))
    .replace("M  V30 END BOND\n", &format!("{bonds}M  V30 END BOND\n"))
}

#[test]
fn an_unreadable_drawing_is_named_as_the_cause_not_the_member_atoms() {
    let (app, _directory, id) = setup();
    let source = big_file(300, &["MDLV30/STEABS ATOMS=(1 1)"]);
    let refused = preview(&app, &id, 0, &source);
    assert_eq!(
        refused["outcome"],
        "error",
        "{}",
        &refused.to_string()[..300]
    );
    assert_eq!(
        refused["diagnostics"][0]["code"],
        "v3000_collection_drawing_unreadable",
        "{}",
        &refused.to_string()[..400]
    );
    assert_eq!(document(&app, &id)["revision"], 0);
}
