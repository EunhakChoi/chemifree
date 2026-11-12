use super::*;
use crate::properties;
use crate::{Draft, DraftRecoveryTarget, DraftStatus, StorageError, TrashEntry};
use chemifree_domain::document::{Document, DocumentId};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Mutex;

struct TestCapabilities;
impl CapabilityCatalog for TestCapabilities {
    fn capabilities(&self) -> Vec<chemifree_domain::Capability> {
        Vec::new()
    }
}
#[derive(Default)]
struct TestRepository(Mutex<BTreeMap<DocumentId, Document>>);
impl DocumentRepository for TestRepository {
    fn allocate_id(&self) -> Result<DocumentId, StorageError> {
        Ok(DocumentId::parse("fixture-document").unwrap())
    }
    fn list_ids(&self) -> Result<Vec<DocumentId>, StorageError> {
        Ok(self.0.lock().unwrap().keys().cloned().collect())
    }
    fn load(&self, id: &DocumentId) -> Result<Option<Document>, StorageError> {
        Ok(self.0.lock().unwrap().get(id).cloned())
    }
    fn save(&self, document: &Document, _base: Option<u64>) -> Result<(), StorageError> {
        self.0
            .lock()
            .unwrap()
            .insert(document.id().clone(), document.clone());
        Ok(())
    }
    fn draft_status(&self, _id: &DocumentId) -> Result<DraftStatus, StorageError> {
        Ok(DraftStatus::None)
    }
    fn write_draft(&self, _draft: &Draft) -> Result<(), StorageError> {
        unreachable!()
    }
    fn claim_draft(&self, _id: &DocumentId) -> Result<Option<Result<Draft, String>>, StorageError> {
        unreachable!()
    }
    fn prepare_draft_for_recovery(
        &self,
        _id: &DocumentId,
        _target: DraftRecoveryTarget,
    ) -> Result<(), StorageError> {
        unreachable!()
    }
    fn release_draft(&self, _id: &DocumentId) {
        unreachable!()
    }
    fn remove_draft(&self, _id: &DocumentId) -> Result<(), StorageError> {
        unreachable!()
    }
    fn saved_at_ms(&self, _id: &DocumentId) -> Result<Option<u64>, StorageError> {
        Ok(None)
    }
    fn trash(&self, _id: &DocumentId, _base: u64) -> Result<(), StorageError> {
        unreachable!()
    }
    fn restore(&self, _id: &DocumentId) -> Result<(), StorageError> {
        unreachable!()
    }
    fn list_trash(&self) -> Result<Vec<TrashEntry>, StorageError> {
        unreachable!()
    }
    fn load_trashed(&self, _id: &DocumentId) -> Result<Option<Document>, StorageError> {
        unreachable!()
    }
}

#[test]
fn catalog_distinguishes_formula_from_data_backed_properties() {
    let states = |app: &Application<TestCapabilities, TestRepository>| {
        let catalog = app.operation_catalog();
        let entry = catalog
            .operations
            .iter()
            .find(|entry| entry.id == "chem.properties")
            .unwrap();
        let execution = |id| {
            entry
                .sub_operations
                .iter()
                .find(|part| part.id == id)
                .unwrap()
                .execution
        };
        (
            entry.execution,
            execution("formula"),
            execution("mass"),
            execution("isotope_distribution"),
        )
    };
    let empty = Application::new(TestCapabilities, TestRepository::default());
    assert_eq!(
        states(&empty),
        ("executable", "executable", "requires_data", "requires_data")
    );

    let snapshot = crate::mass_data::tests::decode(&crate::mass_data::tests::fixture());
    let dataset =
        crate::PersonalDataset::fixture(crate::ProfileRecords::Phase4Nist(Box::new(snapshot)));
    let ready = Application::new(TestCapabilities, TestRepository::default())
        .with_data_registry(crate::DataRegistry::fixture(vec![dataset]));
    assert_eq!(
        states(&ready),
        ("executable", "executable", "executable", "executable")
    );

    let mut input = crate::mass_data::tests::synthetic_mass_input();
    input.features.mass_unit = "unknown-unit".into();
    let dataset =
        crate::PersonalDataset::fixture(crate::ProfileRecords::MassInput(Box::new(input)));
    let incompatible = Application::new(TestCapabilities, TestRepository::default())
        .with_data_registry(crate::DataRegistry::fixture(vec![dataset]));
    assert_eq!(
        states(&incompatible),
        ("executable", "executable", "requires_data", "requires_data")
    );

    let mut sparse = crate::mass_data::tests::fixture();
    sparse["isotopes"] = json!([]);
    sparse["isotope_count"] = json!(0);
    let dataset = crate::PersonalDataset::fixture(crate::ProfileRecords::Phase4Nist(Box::new(
        crate::mass_data::tests::decode(&sparse),
    )));
    let unusable = Application::new(TestCapabilities, TestRepository::default())
        .with_data_registry(crate::DataRegistry::fixture(vec![dataset]));
    assert_eq!(
        states(&unusable),
        ("executable", "executable", "requires_data", "requires_data")
    );

    let mut natural_only = crate::mass_data::tests::synthetic_mass_input();
    natural_only.isotopes.clear();
    natural_only.selections.clear();
    let dataset =
        crate::PersonalDataset::fixture(crate::ProfileRecords::MassInput(Box::new(natural_only)));
    let natural_only = Application::new(TestCapabilities, TestRepository::default())
        .with_data_registry(crate::DataRegistry::fixture(vec![dataset]));
    assert_eq!(
        states(&natural_only),
        ("executable", "executable", "executable", "requires_data")
    );
    // P14 permits an exactly positive decimal whose binary64 witness underflows to 0.
    // Such a table cannot supply a valid numeric mass merely because a row exists.
    let mut underflow = crate::mass_data::tests::synthetic_mass_input();
    underflow.isotopes.clear();
    underflow.selections.clear();
    for element in &mut underflow.elements {
        element.natural_atomic_weight.binary64_bits = 0.0_f64.to_bits();
    }
    let dataset =
        crate::PersonalDataset::fixture(crate::ProfileRecords::MassInput(Box::new(underflow)));
    let underflow = Application::new(TestCapabilities, TestRepository::default())
        .with_data_registry(crate::DataRegistry::fixture(vec![dataset]));
    assert_eq!(
        states(&underflow),
        ("executable", "executable", "requires_data", "requires_data")
    );

    let out_of_range = |abundance: f64| {
        let mut input = crate::mass_data::tests::synthetic_mass_input();
        input.elements.truncate(1);
        input.isotopes.truncate(1);
        input.isotopes[0].mass_number_u32 = 1000;
        input.isotopes[0].mass.binary64_bits = 1000.0_f64.to_bits();
        input.isotopes[0].abundance_percent.binary64_bits = abundance.to_bits();
        input.selections[0].mass_number_u32 = 1000;
        crate::PersonalDataset::fixture(crate::ProfileRecords::MassInput(Box::new(input)))
    };
    let no_natural_distribution = Application::new(TestCapabilities, TestRepository::default())
        .with_data_registry(crate::DataRegistry::fixture(vec![out_of_range(0.0)]));
    assert_eq!(
        states(&no_natural_distribution),
        ("executable", "executable", "executable", "requires_data")
    );
    let natural_distribution = Application::new(TestCapabilities, TestRepository::default())
        .with_data_registry(crate::DataRegistry::fixture(vec![out_of_range(100.0)]));
    assert_eq!(
        states(&natural_distribution),
        ("executable", "executable", "executable", "executable")
    );

    let mut zero = crate::mass_data::tests::fixture();
    zero["isotopes"][0]["isotopic_composition"]["value_decimal"] = json!("0");
    zero["isotopes"][1]["isotopic_composition"]["value_decimal"] = json!("0");
    let dataset = crate::PersonalDataset::fixture(crate::ProfileRecords::Phase4Nist(Box::new(
        crate::mass_data::tests::decode(&zero),
    )));
    let labeled = Application::new(TestCapabilities, TestRepository::default())
        .with_data_registry(crate::DataRegistry::fixture(vec![dataset]));
    assert_eq!(
        states(&labeled),
        ("executable", "executable", "executable", "executable")
    );
    let id = labeled
        .create_document(None, Some("Labeled isotope"))
        .unwrap()
        .document
        .id;
    let call = |request_id: &str, operation: &str, parameters: Value| -> Value {
        let request = json!({"contract":1,"request_id":request_id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
        serde_json::to_value(labeled.execute(request.to_string().as_bytes())).unwrap()
    };
    assert_eq!(
        call(
            "draw-labeled",
            "document.transaction",
            json!({
                "document_id":id,"expected_revision":0,"commands":[{"op":"add_atom","element":"C","hydrogens":{"mode":"specified","count":0},"position":{"x":0,"y":0}}]
            })
        )["outcome"],
        "ok"
    );
    assert_eq!(
        call(
            "set-isotope",
            "document.transaction",
            json!({"document_id":id,"expected_revision":1,"commands":[{"op":"update_atom","atom":"a1","isotope":12}]})
        )["outcome"],
        "ok"
    );
    let fixed = call(
        "fixed-isotope",
        "chem.properties",
        json!({"document_id":id,"scope":"molecule","property":"distribution"}),
    );
    assert_eq!(fixed["outcome"], "ok", "{fixed}");
    assert_eq!(
        fixed["result"]["components"][0]["isotope_distribution"],
        json!([{"mass_u":12.0,"mass_display":"12.00","probability":1.0}])
    );
    assert_eq!(
        call(
            "change-isotope",
            "document.transaction",
            json!({
                "document_id":id,"expected_revision":2,"commands":[{"op":"update_atom","atom":"a1","isotope":null}]
            })
        )["outcome"],
        "ok"
    );
    let natural = call(
        "natural-isotope",
        "chem.properties",
        json!({"document_id":id,"scope":"molecule","property":"distribution"}),
    );
    assert_eq!(natural["outcome"], "unsupported");
    assert!(natural["result"].is_null());

    let valid = crate::PersonalDataset::named_fixture(
        "valid",
        crate::ProfileRecords::Phase4Nist(Box::new(crate::mass_data::tests::decode(
            &crate::mass_data::tests::fixture(),
        ))),
    );
    let mut bad = crate::mass_data::tests::synthetic_mass_input();
    bad.features.mass_unit = "unknown-unit".into();
    let invalid = crate::PersonalDataset::named_fixture(
        "invalid",
        crate::ProfileRecords::MassInput(Box::new(bad)),
    );
    let multiple = crate::DataRegistry::fixture(vec![valid, invalid]);
    assert!(matches!(
        properties::select_data(&multiple, None),
        Err(properties::PropertiesError::DataAmbiguous)
    ));
    assert!(properties::select_data(&multiple, Some("valid")).is_ok());
    assert!(matches!(
        properties::select_data(&multiple, Some("invalid")),
        Err(properties::PropertiesError::DataInvalid(
            "unsupported_mass_profile"
        ))
    ));
    let mixed =
        Application::new(TestCapabilities, TestRepository::default()).with_data_registry(multiple);
    assert_eq!(
        states(&mixed),
        ("executable", "executable", "executable", "executable")
    );
    let make_valid = |id| {
        crate::PersonalDataset::named_fixture(
            id,
            crate::ProfileRecords::Phase4Nist(Box::new(crate::mass_data::tests::decode(
                &crate::mass_data::tests::fixture(),
            ))),
        )
    };
    let both_valid = crate::DataRegistry::fixture(vec![make_valid("first"), make_valid("second")]);
    assert!(matches!(
        properties::select_data(&both_valid, None),
        Err(properties::PropertiesError::DataAmbiguous)
    ));
    assert!(properties::select_data(&both_valid, Some("first")).is_ok());
    assert!(properties::select_data(&both_valid, Some("second")).is_ok());
}

#[test]
fn synthetic_external_profile_supplies_mass_with_honest_selection_and_charge_metadata() {
    let snapshot = crate::mass_data::tests::decode(&crate::mass_data::tests::fixture());
    let dataset =
        crate::PersonalDataset::fixture(crate::ProfileRecords::Phase4Nist(Box::new(snapshot)));
    let app = Application::new(TestCapabilities, TestRepository::default())
        .with_data_registry(crate::DataRegistry::fixture(vec![dataset]));
    let id = app
        .create_document(None, Some("Fixture"))
        .unwrap()
        .document
        .id;
    let call = |request_id: &str, operation: &str, parameters: Value| -> Value {
        let body = json!({"contract":1,"request_id":request_id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
        serde_json::to_value(app.execute(body.to_string().as_bytes())).unwrap()
    };
    let added = call(
        "add",
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":[
            {"op":"add_atom","element":"C","hydrogens":{"mode":"specified","count":0},"position":{"x":0,"y":0}}
        ]}),
    );
    assert_eq!(added["outcome"], "ok");
    let before = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    let preferences: crate::PreferencesChange = serde_json::from_value(json!({
        "ui":{"theme":"system"}, "new_documents":{},
        "computation":{"mass_precision":{"decimals":5}}
    }))
    .unwrap();
    app.set_preferences(None, 0, preferences).unwrap();
    let natural = call(
        "natural",
        "chem.properties",
        json!({"document_id":id,"scope":"molecule","property":"distribution"}),
    );
    assert_eq!(natural["outcome"], "partial", "{natural}");
    assert_eq!(natural["result"]["display_decimals"], 5);
    assert_eq!(
        natural["result"]["components"][0]["mass"]["average_display"],
        "12.01074"
    );
    assert_eq!(
        natural["result"]["data"]["dataset_id"],
        "synthetic-public-values"
    );
    assert_eq!(
        natural["result"]["data"]["source_schema_id"],
        serde_json::Value::Null
    );
    assert_eq!(
        natural["result"]["data"]["calculation_profile"],
        "chemifree-nist-representative-1"
    );
    assert_eq!(natural["result"]["components"][0]["mass"]["exact_u"], 12.0);
    assert_eq!(
        natural["result"]["components"][0]["mass"]["exact_selections"][0]["basis"],
        "maximum_reported_fraction"
    );
    assert_eq!(
        natural["result"]["components"][0]["omitted_abundances"][0]["isotope"],
        14
    );
    assert_eq!(
        natural["loss_report"][0]["feature"],
        "unreported_isotope_abundances"
    );
    assert_eq!(
        serde_json::to_value(app.document(&id).unwrap()).unwrap(),
        before
    );
    app.reset_preferences(None, 1).unwrap();
    let after_reset = call(
        "natural-after-reset",
        "chem.properties",
        json!({"document_id":id,"scope":"molecule","property":"distribution"}),
    );
    assert_eq!(after_reset["outcome"], "partial", "{after_reset}");
    assert_eq!(after_reset["result"]["display_decimals"], 2);
    assert_eq!(
        after_reset["result"]["components"][0]["mass"]["average_display"],
        "12.01"
    );
    assert_eq!(
        natural["result"]["display_decimals"], 5,
        "a previous mass result is unchanged"
    );

    let updated = call(
        "isotope",
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[{"op":"update_atom","atom":"a1","isotope":13,"charge":1}]}),
    );
    assert_eq!(updated["outcome"], "ok");
    let labeled = call(
        "labeled",
        "chem.properties",
        json!({"document_id":id,"scope":"molecule","property":"masses","data_registration_id":"synthetic-test"}),
    );
    assert_eq!(labeled["outcome"], "ok", "{labeled}");
    let exact = labeled["result"]["components"][0]["mass"]["exact_u"]
        .as_f64()
        .unwrap();
    assert!(
        (exact - (13.00335483507 - crate::mass_data::CODATA_2022_ELECTRON_MASS_U)).abs() < 1e-12
    );
    assert_eq!(
        labeled["result"]["components"][0]["mass"]["exact_selections"][0]["basis"],
        "explicit_isotope"
    );
}
