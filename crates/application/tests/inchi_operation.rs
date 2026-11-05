//! Application boundary checks use an explicit provider fake, not a native chemistry engine.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chemifree_application::inchi::{
    InchiError, InchiErrorKind, InchiOutput, InchiProvider, InchiProviderIdentity,
};
use chemifree_application::{Application, Loss, LossConsent, LossDisposition, SemanticImpact};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::stereo_group::StereoGroupTable;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, OpenSmilesCodec};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

#[derive(Clone)]
struct FixtureProvider {
    answer: Result<InchiOutput, InchiError>,
    calls: Arc<AtomicUsize>,
}

impl InchiProvider for FixtureProvider {
    fn identity(&self) -> InchiProviderIdentity {
        InchiProviderIdentity {
            name: "application-test-fake".into(),
            version: "fixture-1".into(),
            build: "synthetic-build".into(),
            options: vec!["fixture-options".into()],
        }
    }
    fn generate(
        &self,
        _graph: &MoleculeGraph,
        _groups: &StereoGroupTable,
    ) -> Result<InchiOutput, InchiError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.answer.clone()
    }
}

fn fixture(losses: Vec<Loss>) -> FixtureProvider {
    FixtureProvider {
        answer: Ok(InchiOutput {
            inchi: "InChI=1S/CH4/h1H4".into(),
            inchikey: "VNWKTOKETHGBQD-UHFFFAOYSA-N".into(),
            aux_info: Some("AuxInfo=fixture".into()),
            warnings: Vec::new(),
            native_code: 0,
            native_message: None,
            native_log: None,
            losses,
        }),
        calls: Arc::new(AtomicUsize::new(0)),
    }
}

fn call(app: &App, operation: &str, parameters: Value) -> Value {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let request = json!({"contract":1,"request_id":format!("inchi-test-{}", NEXT.fetch_add(1, Ordering::Relaxed)),"profile":"preserve-semantics", "operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn methane(app: &App) -> String {
    let created = call(app, "document.create", json!({}));
    let id = created["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let edited = call(
        app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,
        "commands":[{"op":"add_atom","element":"C","position":{"x":0,"y":0}}]}),
    );
    assert_eq!(edited["outcome"], "ok", "{edited}");
    id
}

#[test]
fn an_unconfigured_provider_is_unavailable_and_returns_no_identifier() {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let entry = app
        .operation_catalog()
        .operations
        .into_iter()
        .find(|entry| entry.id == "chem.inchi")
        .unwrap();
    assert_eq!(entry.representation, "supported");
    assert_eq!(entry.execution, "not_implemented");
    let reply = call(&app, "chem.inchi", json!({"document_id":"missing"}));
    assert_eq!(reply["outcome"], "unsupported");
    assert_eq!(reply["diagnostics"][0]["code"], "inchi_unavailable");
    assert_eq!(reply["result"], Value::Null);
    let unknown = call(
        &app,
        "chem.inchi",
        json!({"document_id":"missing","options":"nonstandard"}),
    );
    assert_eq!(unknown["outcome"], "error", "{unknown}");
}

#[test]
fn a_configured_provider_names_the_exact_snapshot_build_and_options_without_editing() {
    let directory = tempfile::tempdir().unwrap();
    let provider = fixture(Vec::new());
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    )
    .with_inchi_provider(provider.clone());
    let id = methane(&app);
    let before = call(&app, "document.snapshot", json!({"document_id":id}));
    let reply = call(&app, "chem.inchi", json!({"document_id":id}));
    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["model_id"], "chemifree-standard-inchi-1");
    assert_eq!(reply["document_revision"], 1);
    assert_eq!(reply["result"]["snapshot"], before["result"]);
    assert_eq!(
        reply["result"]["provider"],
        serde_json::to_value(provider.identity()).unwrap()
    );
    assert_eq!(reply["result"]["inchi"], "InChI=1S/CH4/h1H4");
    assert_eq!(reply["result"]["inchikey"], "VNWKTOKETHGBQD-UHFFFAOYSA-N");
    assert_eq!(reply["result"]["standard"], true);
    assert_eq!(
        call(&app, "document.snapshot", json!({"document_id":id}))["result"],
        before["result"]
    );
    assert_eq!(provider.calls.load(Ordering::Relaxed), 1);
    let entry = app
        .operation_catalog()
        .operations
        .into_iter()
        .find(|entry| entry.id == "chem.inchi")
        .unwrap();
    assert_eq!(entry.execution, "executable");
}

#[test]
fn required_losses_hide_identifiers_until_consent_bound_to_the_snapshot_is_repeated() {
    let directory = tempfile::tempdir().unwrap();
    let provider = fixture(vec![Loss {
        feature: "fixture_normalization".into(),
        source_path: "atoms[a1]".into(),
        disposition: LossDisposition::Normalized,
        reason: "Explicit test-only normalization.".into(),
        semantic_impact: SemanticImpact::Chemistry,
        consent: LossConsent::Required,
    }]);
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    )
    .with_inchi_provider(provider);
    let id = methane(&app);
    let refused = call(&app, "chem.inchi", json!({"document_id":id}));
    assert_eq!(
        refused["diagnostics"][0]["code"], "loss_consent_required",
        "{refused}"
    );
    assert_eq!(refused["result"], Value::Null);
    let details = &refused["diagnostics"][0]["details"];
    assert_eq!(details["features"], json!(["fixture_normalization"]));
    let digest = &details["loss_digest"];
    let accepted = call(
        &app,
        "chem.inchi",
        json!({"document_id":id,"accepted_losses":digest}),
    );
    assert_eq!(accepted["outcome"], "partial", "{accepted}");
    assert_eq!(accepted["result"]["inchi"], "InChI=1S/CH4/h1H4");
    let changed = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,
        "commands":[{"op":"rename","title":"Changed snapshot"}]}),
    );
    assert_eq!(changed["outcome"], "ok", "{changed}");
    let stale = call(
        &app,
        "chem.inchi",
        json!({"document_id":id,"accepted_losses":digest}),
    );
    assert_eq!(
        stale["diagnostics"][0]["code"], "loss_consent_changed",
        "{stale}"
    );
    assert_eq!(stale["result"], Value::Null);
}

#[test]
fn native_failures_and_malformed_successes_return_no_identifiers() {
    for (answer, expected_code) in [
        (
            Err(InchiError {
                kind: InchiErrorKind::Limit,
                code: "inchi_limit",
                message: "Fixture resource limit.".into(),
            }),
            "inchi_limit",
        ),
        (
            Ok(InchiOutput {
                inchi: "InChI=1/nonstandard".into(),
                ..fixture(Vec::new()).answer.unwrap()
            }),
            "inchi_provider_invalid",
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let provider = FixtureProvider {
            answer,
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let app = Application::new(
            LocalCapabilities,
            FileDocumentRepository::new(directory.path()),
        )
        .with_inchi_provider(provider);
        let id = methane(&app);
        let reply = call(&app, "chem.inchi", json!({"document_id":id}));
        assert_eq!(reply["outcome"], "error", "{reply}");
        assert_eq!(reply["diagnostics"][0]["code"], expected_code, "{reply}");
        assert_eq!(reply["result"], Value::Null);
        assert_eq!(
            reply["diagnostics"][0]["details"]["snapshot"]["document"]["id"],
            id
        );
    }
}

#[test]
fn and_or_groups_are_refused_before_the_native_provider_is_called() {
    for field in ["o1:1", "&1:1"] {
        let directory = tempfile::tempdir().unwrap();
        let provider = fixture(Vec::new());
        let app = Application::new(
            LocalCapabilities,
            FileDocumentRepository::new(directory.path()),
        )
        .with_smiles_codec(OpenSmilesCodec)
        .with_inchi_provider(provider.clone());
        let created = call(&app, "document.create", json!({}));
        let id = created["result"]["document"]["id"].as_str().unwrap();
        let mut parameters = json!({"document_id":id,"expected_revision":0,"format":"smiles",
            "source":format!("F[C@H](Cl)Br |{field}|"),"preview":true});
        let preview = call(&app, "document.import", parameters.clone());
        parameters.as_object_mut().unwrap().remove("preview");
        parameters["preview_digest"] = preview["result"]["preview_digest"].clone();
        parameters["accept_loss"] = json!(true);
        let committed = call(&app, "document.import", parameters);
        assert_eq!(committed["document_revision"], 1, "{committed}");
        let reply = call(&app, "chem.inchi", json!({"document_id":id}));
        assert_eq!(reply["outcome"], "unsupported", "{reply}");
        assert_eq!(
            reply["diagnostics"][0]["code"],
            "inchi_enhanced_groups_unsupported"
        );
        assert_eq!(reply["diagnostics"][0]["details"]["groups"], json!(["e1"]));
        assert_eq!(reply["result"], Value::Null);
        assert_eq!(provider.calls.load(Ordering::Relaxed), 0);
    }
}

#[test]
fn warning_consent_exposes_diagnostics_and_ignores_only_nonsemantic_log_changes() {
    let directory = tempfile::tempdir().unwrap();
    let mut provider = fixture(vec![Loss {
        feature: "native_warning".into(),
        source_path: "provider".into(),
        disposition: LossDisposition::Unknown,
        reason: "Fixture native warning; raw log is diagnostic-only.".into(),
        semantic_impact: SemanticImpact::Unknown,
        consent: LossConsent::Required,
    }]);
    let output = provider.answer.as_mut().unwrap();
    output.native_code = 1;
    output.native_message = Some("opaque warning message".into());
    output.native_log = Some("Elapsed time: 1".into());
    output.warnings = vec!["opaque warning message".into()];
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    )
    .with_inchi_provider(provider.clone());
    let id = methane(&app);
    let before = call(&app, "document.snapshot", json!({"document_id":id}));
    let refused = call(&app, "chem.inchi", json!({"document_id":id}));
    assert_eq!(
        refused["diagnostics"][0]["code"], "loss_consent_required",
        "{refused}"
    );
    assert_eq!(refused["result"], Value::Null);
    let details = &refused["diagnostics"][0]["details"];
    assert_eq!(details["native_code"], 1);
    assert_eq!(details["native_message"], "opaque warning message");
    assert_eq!(details["warnings"], json!(["opaque warning message"]));
    assert_eq!(details["native_log"], "Elapsed time: 1");
    let digest = details["loss_digest"].clone();

    provider.answer.as_mut().unwrap().native_log = Some("Elapsed time: 2".into());
    let mut app = app.with_inchi_provider(provider.clone());
    let accepted = call(
        &app,
        "chem.inchi",
        json!({"document_id":id,"accepted_losses":digest}),
    );
    assert_eq!(accepted["outcome"], "partial", "{accepted}");
    assert_eq!(accepted["result"]["inchi"], "InChI=1S/CH4/h1H4");
    assert_eq!(accepted["result"]["native_log"], "Elapsed time: 2");
    assert_eq!(
        accepted["result"]["native_message"],
        details["native_message"]
    );
    assert_eq!(accepted["result"]["warnings"], details["warnings"]);

    let mut changed_message = provider.clone();
    changed_message.answer.as_mut().unwrap().native_message =
        Some("different warning message".into());
    let mut changed_warnings = provider.clone();
    changed_warnings.answer.as_mut().unwrap().warnings = vec!["different advisory warning".into()];
    let mut changed_code = provider;
    changed_code.answer.as_mut().unwrap().native_code = 0;
    for (scenario, changed) in [
        ("message", changed_message),
        ("warnings", changed_warnings),
        ("code", changed_code),
    ] {
        app = app.with_inchi_provider(changed);
        let stale = call(
            &app,
            "chem.inchi",
            json!({"document_id":id,"accepted_losses":digest}),
        );
        assert_eq!(
            stale["diagnostics"][0]["code"], "loss_consent_changed",
            "changed {scenario}: {stale}"
        );
        assert_eq!(stale["result"], Value::Null, "changed {scenario}");
    }
    assert_eq!(
        call(&app, "document.snapshot", json!({"document_id":id}))["result"],
        before["result"]
    );
}
