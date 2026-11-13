//! Application boundary checks use an explicit provider fake, not a native chemistry engine.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use chemifree_application::inchi::{
    InchiError, InchiErrorKind, InchiOutput, InchiProvider, InchiProviderIdentity, MODEL_ID,
};
use chemifree_application::{
    Application, CancelFlag, Interrupt, Interrupted, Loss, LossConsent, LossDisposition,
    SemanticImpact,
};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::stereo_group::StereoGroupTable;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, OpenSmilesCodec};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

/// The limit a request that sets none has, in milliseconds.
const DEFAULT_LIMIT_MS: u64 = 5_000;
/// The largest limit a request may set, in milliseconds.
const MAXIMUM_LIMIT_MS: u64 = 8_000;
/// Long enough for the holder of the only slot to reach its checkpoint; short enough to end a
/// failing run.
const WAIT: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct FixtureProvider {
    answer: Result<InchiOutput, InchiError>,
    calls: Arc<AtomicUsize>,
    /// Cancelled when generation starts, as a host cancel that arrives while the provider waits
    /// for its native lock would be.
    cancel_on_entry: Option<Arc<CancelFlag>>,
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
        stop: &dyn Interrupt,
    ) -> Result<Result<InchiOutput, InchiError>, Interrupted> {
        if let Some(cancel) = &self.cancel_on_entry {
            cancel.cancel();
        }
        stop.check()?;
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(self.answer.clone())
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
        cancel_on_entry: None,
    }
}

fn request_bytes(operation: &str, parameters: Value) -> Vec<u8> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let body = json!({"contract":1,"request_id":format!("inchi-test-{}", NEXT.fetch_add(1, Ordering::Relaxed)),"profile":"preserve-semantics", "operation":operation,"parameters":parameters});
    body.to_string().into_bytes()
}

fn call(app: &App, operation: &str, parameters: Value) -> Value {
    serde_json::to_value(app.execute(&request_bytes(operation, parameters))).unwrap()
}

/// Runs the request under a host cancel that is already set, as a host that stops it would.
fn call_cancelled(app: &App, operation: &str, parameters: Value, cancel: &CancelFlag) -> Value {
    serde_json::to_value(app.execute_with(&request_bytes(operation, parameters), cancel)).unwrap()
}

/// The reply without its request identifier, the one field that differs between two requests
/// that are otherwise the same.
fn without_request_id(mut reply: Value) -> Value {
    reply.as_object_mut().unwrap().remove("request_id");
    reply
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
            cancel_on_entry: None,
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

#[test]
fn a_zero_time_limit_stops_before_the_native_provider_is_called() {
    let directory = tempfile::tempdir().unwrap();
    let provider = fixture(Vec::new());
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    )
    .with_inchi_provider(provider.clone());
    let id = methane(&app);
    let reply = call(
        &app,
        "chem.inchi",
        json!({"document_id":id,"time_limit_ms":0}),
    );
    assert_eq!(reply["outcome"], "error", "{reply}");
    assert_eq!(reply["document_revision"], 1, "{reply}");
    assert_eq!(reply["result"], Value::Null, "{reply}");
    assert_eq!(reply["model_id"], MODEL_ID, "{reply}");
    assert_eq!(reply["diagnostics"].as_array().unwrap().len(), 1, "{reply}");
    assert_eq!(reply["diagnostics"][0]["code"], "time_limit", "{reply}");
    assert_eq!(
        reply["diagnostics"][0]["details"],
        json!({"limit_ms": 0}),
        "{reply}"
    );
    assert_eq!(
        provider.calls.load(Ordering::Relaxed),
        0,
        "the native provider ran after the time limit had passed"
    );
}

#[test]
fn a_host_cancel_stops_before_the_native_provider_is_called() {
    let directory = tempfile::tempdir().unwrap();
    let provider = fixture(Vec::new());
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    )
    .with_inchi_provider(provider.clone());
    let id = methane(&app);
    let cancel = CancelFlag::new();
    cancel.cancel();
    for parameters in [
        json!({"document_id":id}),
        json!({"document_id":id,"time_limit_ms":0}),
    ] {
        let label = parameters.to_string();
        let reply = call_cancelled(&app, "chem.inchi", parameters, &cancel);
        assert_eq!(reply["outcome"], "error", "{label}: {reply}");
        assert_eq!(reply["document_revision"], 1, "{label}: {reply}");
        assert_eq!(reply["result"], Value::Null, "{label}: {reply}");
        assert_eq!(reply["model_id"], MODEL_ID, "{label}: {reply}");
        assert_eq!(
            reply["diagnostics"].as_array().unwrap().len(),
            1,
            "{label}: {reply}"
        );
        assert_eq!(
            reply["diagnostics"][0]["code"], "cancelled",
            "{label}: {reply}"
        );
        assert_eq!(
            reply["diagnostics"][0]["details"],
            json!({}),
            "{label}: {reply}"
        );
    }
    assert_eq!(
        provider.calls.load(Ordering::Relaxed),
        0,
        "the native provider ran after a host cancel"
    );
}

#[test]
fn a_time_limit_above_the_maximum_is_refused_before_the_document_and_the_provider() {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    // The maximum itself is accepted, so the request reaches the unavailable check.
    let accepted = call(
        &app,
        "chem.inchi",
        json!({"document_id":"missing","time_limit_ms":MAXIMUM_LIMIT_MS}),
    );
    assert_eq!(
        accepted["diagnostics"][0]["code"], "inchi_unavailable",
        "{accepted}"
    );
    let refused = call(
        &app,
        "chem.inchi",
        json!({"document_id":"missing","time_limit_ms":MAXIMUM_LIMIT_MS + 1}),
    );
    assert_eq!(refused["outcome"], "error", "{refused}");
    assert_eq!(refused["result"], Value::Null, "{refused}");
    assert!(refused["document_revision"].is_null(), "{refused}");
    assert_eq!(refused["model_id"], MODEL_ID, "{refused}");
    assert_eq!(
        refused["diagnostics"].as_array().unwrap().len(),
        1,
        "{refused}"
    );
    assert_eq!(
        refused["diagnostics"][0]["code"], "invalid_request",
        "{refused}"
    );
    assert_eq!(
        refused["diagnostics"][0]["path"], "parameters.time_limit_ms",
        "{refused}"
    );
    assert_eq!(
        refused["diagnostics"][0]["details"]["maximum_ms"], MAXIMUM_LIMIT_MS,
        "{refused}"
    );

    // With a provider registered the limit still comes first: the missing document is not read
    // and the provider is not called.
    let provider = fixture(Vec::new());
    let app = app.with_inchi_provider(provider.clone());
    let refused = call(
        &app,
        "chem.inchi",
        json!({"document_id":"missing","time_limit_ms":MAXIMUM_LIMIT_MS + 1}),
    );
    assert_eq!(
        refused["diagnostics"][0]["code"], "invalid_request",
        "{refused}"
    );
    assert_eq!(
        provider.calls.load(Ordering::Relaxed),
        0,
        "the provider ran for a refused limit"
    );
}

#[test]
fn the_loss_consent_digest_is_the_same_with_and_without_a_time_limit() {
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
    let unlimited = call(&app, "chem.inchi", json!({"document_id":id}));
    assert_eq!(
        unlimited["diagnostics"][0]["code"], "loss_consent_required",
        "{unlimited}"
    );
    let limited = call(
        &app,
        "chem.inchi",
        json!({"document_id":id,"time_limit_ms":DEFAULT_LIMIT_MS}),
    );
    assert_eq!(
        limited["diagnostics"], unlimited["diagnostics"],
        "a time limit changed the loss refusal, its consent digest included: {limited}"
    );
    // Consent given to the request without a limit is accepted by the request with one.
    let digest = &unlimited["diagnostics"][0]["details"]["loss_digest"];
    let accepted = call(
        &app,
        "chem.inchi",
        json!({"document_id":id,"time_limit_ms":DEFAULT_LIMIT_MS,"accepted_losses":digest}),
    );
    assert_eq!(accepted["outcome"], "partial", "{accepted}");
    assert_eq!(
        accepted["result"]["inchi"], "InChI=1S/CH4/h1H4",
        "{accepted}"
    );
}

#[test]
fn a_time_limit_the_computation_meets_leaves_the_reply_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    )
    .with_inchi_provider(fixture(Vec::new()));
    let id = methane(&app);
    let plain = call(&app, "chem.inchi", json!({"document_id":id}));
    assert_eq!(plain["outcome"], "ok", "{plain}");
    for limit in [DEFAULT_LIMIT_MS, MAXIMUM_LIMIT_MS] {
        let limited = call(
            &app,
            "chem.inchi",
            json!({"document_id":id,"time_limit_ms":limit}),
        );
        assert_eq!(
            without_request_id(limited.clone()),
            without_request_id(plain.clone()),
            "time_limit_ms {limit} changed the reply: {limited}"
        );
    }
}

/// A document holding a Kekulé benzene at revision 1. A conversion of it asks its interrupt, so
/// its holder stops at a checkpoint while it holds the only slot.
fn kekule_benzene(app: &App) -> String {
    let created = call(app, "document.create", json!({}));
    let id = created["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut commands: Vec<Value> = (0..6)
        .map(|i| json!({"op":"add_atom", "element":"C", "position":{"x":i * 20,"y":0}}))
        .collect();
    for i in 0..6 {
        let order = if i % 2 == 0 { "double" } else { "single" };
        commands.push(json!({"op":"add_bond", "begin":format!("${i}"), "end":format!("${}", (i + 1) % 6), "order":order}));
    }
    let drawn = call(
        app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":commands}),
    );
    assert_eq!(drawn["outcome"], "ok", "{drawn}");
    id
}

/// Stops the holder of the only slot at its first checkpoint, and holds it there until the
/// release sender is dropped.
struct Latch {
    reached: Sender<()>,
    release: Mutex<Receiver<()>>,
    first: AtomicUsize,
}

impl Interrupt for Latch {
    fn check(&self) -> Result<(), Interrupted> {
        if self.first.fetch_add(1, Ordering::SeqCst) == 0 {
            self.reached.send(()).ok();
            // Nothing is ever sent: the receive returns once the release sender is dropped.
            self.release.lock().unwrap().recv().ok();
        }
        Ok(())
    }
}

/// A latch, the receiver that reports when it is reached, and the sender whose drop releases it.
fn new_latch() -> (Latch, Receiver<()>, Sender<()>) {
    let (reached, reached_rx) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    (
        Latch {
            reached,
            release: Mutex::new(release_rx),
            first: AtomicUsize::new(0),
        },
        reached_rx,
        release,
    )
}

#[test]
fn a_cancel_that_arrives_before_the_native_call_stops_it_and_returns_no_identifier() {
    for time_limit_ms in [None, Some(DEFAULT_LIMIT_MS)] {
        let directory = tempfile::tempdir().unwrap();
        let cancel = Arc::new(CancelFlag::new());
        let provider = FixtureProvider {
            cancel_on_entry: Some(Arc::clone(&cancel)),
            ..fixture(Vec::new())
        };
        let app = Application::new(
            LocalCapabilities,
            FileDocumentRepository::new(directory.path()),
        )
        .with_inchi_provider(provider.clone());
        let id = methane(&app);
        // The flag is set only once the provider is entered, so the stop comes from the
        // provider's own check, not from the request's check before it.
        let mut parameters = json!({"document_id":id});
        if let Some(limit) = time_limit_ms {
            parameters["time_limit_ms"] = json!(limit);
        }
        let label = parameters.to_string();
        let reply = call_cancelled(&app, "chem.inchi", parameters, &cancel);
        assert_eq!(reply["outcome"], "error", "{label}: {reply}");
        assert_eq!(reply["document_revision"], 1, "{label}: {reply}");
        assert_eq!(reply["result"], Value::Null, "{label}: {reply}");
        assert_eq!(reply["model_id"], MODEL_ID, "{label}: {reply}");
        assert_eq!(
            reply["diagnostics"].as_array().unwrap().len(),
            1,
            "{label}: {reply}"
        );
        assert_eq!(
            reply["diagnostics"][0]["code"], "cancelled",
            "{label}: {reply}"
        );
        assert_eq!(
            reply["diagnostics"][0]["details"],
            json!({}),
            "{label}: {reply}"
        );
        assert_eq!(
            provider.calls.load(Ordering::Relaxed),
            0,
            "{label}: the native call ran after the cancel"
        );
    }
}

#[test]
fn chem_inchi_answers_while_the_only_compute_slot_is_held() {
    let directory = tempfile::tempdir().unwrap();
    let provider = fixture(Vec::new());
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    )
    .with_inchi_provider(provider.clone())
    .with_compute_concurrency(1);
    let id = methane(&app);
    let idle = call(&app, "chem.inchi", json!({"document_id":id}));
    assert_eq!(idle["outcome"], "ok", "{idle}");
    let holding = kekule_benzene(&app);
    let (latch, reached, release) = new_latch();
    thread::scope(|scope| {
        // Owned by this closure, so a failed assertion below releases the holder.
        let release = release;
        let holder = scope.spawn(|| {
            app.execute_with(
                &request_bytes(
                    "chem.aromaticity_convert",
                    json!({"document_id":holding,"expected_revision":1,"to":"aromatic","time_limit_ms":MAXIMUM_LIMIT_MS}),
                ),
                &latch,
            )
        });
        reached
            .recv_timeout(WAIT)
            .expect("the holder took the only slot and reached its checkpoint");
        // The holder keeps the only slot until it is released, and chem.inchi takes no slot.
        let held = call(&app, "chem.inchi", json!({"document_id":id}));
        drop(release);
        let holder_reply = serde_json::to_value(holder.join().unwrap()).unwrap();
        assert_eq!(holder_reply["outcome"], "ok", "holder: {holder_reply}");
        assert_eq!(held["outcome"], "ok", "{held}");
        assert_eq!(
            without_request_id(held),
            without_request_id(idle),
            "chem.inchi answered differently while the only slot was held"
        );
    });
    assert_eq!(
        provider.calls.load(Ordering::Relaxed),
        2,
        "the provider was not called for both requests"
    );
}

#[test]
fn chem_inchi_is_refused_after_cancel_computations_before_the_provider_is_called() {
    let directory = tempfile::tempdir().unwrap();
    let provider = fixture(Vec::new());
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    )
    .with_inchi_provider(provider.clone());
    let id = methane(&app);
    // The same request answers ok before the close, so the refusal below is the close's doing.
    let before = call(&app, "chem.inchi", json!({"document_id":id}));
    assert_eq!(before["outcome"], "ok", "{before}");
    app.cancel_computations();
    for parameters in [
        json!({"document_id":id}),
        json!({"document_id":id,"time_limit_ms":DEFAULT_LIMIT_MS}),
    ] {
        let label = parameters.to_string();
        let reply = call(&app, "chem.inchi", parameters);
        assert_eq!(reply["outcome"], "error", "{label}: {reply}");
        assert_eq!(reply["document_revision"], 1, "{label}: {reply}");
        assert_eq!(reply["result"], Value::Null, "{label}: {reply}");
        assert_eq!(reply["model_id"], MODEL_ID, "{label}: {reply}");
        assert_eq!(
            reply["diagnostics"].as_array().unwrap().len(),
            1,
            "{label}: {reply}"
        );
        assert_eq!(
            reply["diagnostics"][0]["code"], "cancelled",
            "{label}: {reply}"
        );
        assert_eq!(
            reply["diagnostics"][0]["details"],
            json!({}),
            "{label}: {reply}"
        );
    }
    assert_eq!(
        provider.calls.load(Ordering::Relaxed),
        1,
        "the provider was called after cancel_computations"
    );
}
