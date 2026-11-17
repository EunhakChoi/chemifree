//! `chem.properties` closure checks through `Application::execute`: display rounding at the
//! preference edges, profile selection by registration ID, and radicals against exact mass.
//! Fixtures and the in-memory repository come from `property_tests`. The saved-document byte
//! check is not here: the file store lives in `chemifree-providers`, which this crate's unit
//! tests cannot use (see the report of the test-suite pass that added this file).

use super::property_tests::{TestCapabilities, TestRepository};
use super::*;
use crate::{DataRegistry, PersonalDataset, PreferencesChange, ProfileRecords};
use serde_json::json;

const NIST_CONVENTION: &str =
    "subtract CODATA 2022 electron mass per positive elementary charge; binding energy omitted";
const INPUT_CONVENTION: &str = "signed coefficient supplied by the selected input profile";

fn application(datasets: Vec<PersonalDataset>) -> Application<TestCapabilities, TestRepository> {
    Application::new(TestCapabilities, TestRepository::default())
        .with_data_registry(DataRegistry::fixture(datasets))
}

fn call<C: CapabilityCatalog, R: DocumentRepository>(
    app: &Application<C, R>,
    request_id: &str,
    operation: &str,
    parameters: Value,
) -> Value {
    let request = json!({"contract":1,"request_id":request_id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

/// A new document holding one atom of `element` whose hydrogen count is stated.
fn one_atom<C: CapabilityCatalog, R: DocumentRepository>(
    app: &Application<C, R>,
    element: &str,
    hydrogens: u64,
) -> String {
    let id = app
        .create_document(None, Some("Closure"))
        .unwrap()
        .document
        .id;
    let added = call(
        app,
        &format!("add-{id}"),
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":[{
            "op":"add_atom","element":element,"hydrogens":{"mode":"specified","count":hydrogens},"position":{"x":0,"y":0}
        }]}),
    );
    assert_eq!(added["outcome"], "ok", "{added}");
    id
}

fn masses<C: CapabilityCatalog, R: DocumentRepository>(
    app: &Application<C, R>,
    request_id: &str,
    id: &str,
    data_registration_id: Option<&str>,
) -> Value {
    let mut parameters = json!({"document_id":id,"scope":"molecule","property":"masses"});
    if let Some(registration) = data_registration_id {
        parameters["data_registration_id"] = json!(registration);
    }
    call(app, request_id, "chem.properties", parameters)
}

fn set_decimals<C: CapabilityCatalog, R: DocumentRepository>(
    app: &Application<C, R>,
    revision: u64,
    decimals: u64,
) {
    let change: PreferencesChange = serde_json::from_value(json!({
        "ui":{"theme":"system"}, "new_documents":{},
        "computation":{"mass_precision":{"decimals":decimals}}
    }))
    .unwrap();
    app.set_preferences(None, revision, change).unwrap();
}

/// The two profile registrations used by `profiles_are_told_apart`: the public NIST carbon fixture
/// (registered first) and the mass-input fixture, which covers gallium and silver only.
fn profile_application() -> Application<TestCapabilities, TestRepository> {
    let public = PersonalDataset::named_fixture(
        "public-nist",
        ProfileRecords::Phase4Nist(Box::new(crate::mass_data::tests::decode(
            &crate::mass_data::tests::fixture(),
        ))),
    );
    let observed = PersonalDataset::named_fixture(
        "observed-mass-input",
        ProfileRecords::MassInput(Box::new(crate::mass_data::tests::synthetic_mass_input())),
    );
    application(vec![public, observed])
}

/// The `property_tests` carbon fixture with one hydrogen row added: H-1 at reported fraction 1,
/// synthetic test values that are not a production dataset. The binding reads only the reported
/// rows, so the standard-weight interval is informational here.
fn carbon_and_hydrogen() -> Value {
    let mut value = crate::mass_data::tests::fixture();
    value["element_count"] = json!(2);
    value["isotope_count"] = json!(4);
    value["elements"].as_array_mut().unwrap().push(json!({
        "atomic_number":1,"symbol_as_served":"H",
        "standard_atomic_weight":{"kind":"range","raw":null,"lower_decimal":"1.00784","upper_decimal":"1.00811","standard_atomic_weight_value":null},
        "notes_raw":null
    }));
    value["isotopes"].as_array_mut().unwrap().push(json!({
        "atomic_number":1,"element_symbol":"H","isotope_symbol_as_printed":null,"mass_number":1,
        "relative_atomic_mass":{"raw":"1.00782503223","value_decimal":"1.00782503223","uncertainty_digits":"1","absolute_uncertainty_decimal":"0.1","estimated_marker_present":false},
        "isotopic_composition":{"raw":"1.0","value_decimal":"1.0","uncertainty_digits":null,"absolute_uncertainty_decimal":null,"estimated_marker_present":false},
        "source_preformatted_line":4
    }));
    value
}

/// Expected strings come from exact decimal arithmetic (Python `decimal`, 60 digits), not from
/// the code under test. The fixture (`mass_data::tests::fixture`) has carbon 12 at reported
/// fraction 0.9893 with mass 12.0, carbon 13 at 0.0107 with mass 13.00335483507, and carbon 14
/// unreported. The representative average divides by the reported sum, which is 1, so it is
/// 12 x 0.9893 + 13.00335483507 x 0.0107 = 12.010735896735249... Quantized with ties-to-even (the
/// tie rule of `format_fixed`): 0 places is "12", and 8 places is "12.01073590". The value lies
/// 1.7e-9 above the 8-place boundary 12.010735895, far beyond binary64 error (about 2e-15), so no
/// tie can decide it. The 2- and 5-place values "12.01" and "12.01074" agree with `property_tests`.
#[test]
fn display_rounding_edges() {
    let snapshot = crate::mass_data::tests::decode(&crate::mass_data::tests::fixture());
    let app = application(vec![PersonalDataset::fixture(ProfileRecords::Phase4Nist(
        Box::new(snapshot),
    ))]);
    let id = one_atom(&app, "C", 0);

    set_decimals(&app, 0, 0);
    let zero = masses(&app, "masses-0", &id, None);
    assert_eq!(zero["result"]["display_decimals"], 0, "{zero}");
    assert_eq!(
        zero["result"]["components"][0]["mass"]["average_display"], "12",
        "{zero}"
    );
    assert_eq!(
        zero["result"]["components"][0]["mass"]["exact_display"], "12",
        "{zero}"
    );

    set_decimals(&app, 1, 8);
    let eight = masses(&app, "masses-8", &id, None);
    assert_eq!(eight["result"]["display_decimals"], 8, "{eight}");
    assert_eq!(
        eight["result"]["components"][0]["mass"]["average_display"], "12.01073590",
        "{eight}"
    );
    assert_eq!(
        eight["result"]["components"][0]["mass"]["exact_display"], "12.00000000",
        "{eight}"
    );
}

/// Two registrations are compatible with `mass`: the public NIST carbon fixture (registered
/// first) and the mass-input fixture, which covers only gallium and silver. Each id is requested
/// on the element its profile covers. Gallium's expected values are the fixture's own: pinned
/// isotope 8 at mass 8.25, and natural weight 7.25 for the element row.
#[test]
fn profiles_are_told_apart() {
    // The repository hands out one document ID, so each document gets its own application.
    let carbon_app = profile_application();
    let carbon = one_atom(&carbon_app, "C", 0);
    let gallium_app = profile_application();
    let gallium = one_atom(&gallium_app, "Ga", 0);

    let public_reply = masses(&carbon_app, "public", &carbon, Some("public-nist"));
    assert_eq!(public_reply["outcome"], "partial", "{public_reply}");
    let data = &public_reply["result"]["data"];
    assert_eq!(data["registration_id"], "public-nist", "{public_reply}");
    assert_eq!(
        data["calculation_profile"], "chemifree-nist-representative-1",
        "{public_reply}"
    );
    assert_eq!(data["charge_convention"], NIST_CONVENTION, "{public_reply}");

    let observed_reply = masses(
        &gallium_app,
        "observed",
        &gallium,
        Some("observed-mass-input"),
    );
    assert_eq!(observed_reply["outcome"], "ok", "{observed_reply}");
    let data = &observed_reply["result"]["data"];
    assert_eq!(
        data["registration_id"], "observed-mass-input",
        "{observed_reply}"
    );
    assert_eq!(
        data["calculation_profile"], "chemifree-mass-input-1",
        "{observed_reply}"
    );
    assert_eq!(
        data["charge_convention"], INPUT_CONVENTION,
        "{observed_reply}"
    );
    assert_eq!(
        data["source_schema_id"], "independent-test-v3",
        "{observed_reply}"
    );
    let mass = &observed_reply["result"]["components"][0]["mass"];
    assert_eq!(mass["exact_u"], 8.25, "{observed_reply}");
    assert_eq!(mass["average_display"], "7.25", "{observed_reply}");

    // An explicit ID is honoured: the other registration does not cover gallium.
    let crossed = masses(&gallium_app, "crossed", &gallium, Some("public-nist"));
    assert_eq!(crossed["outcome"], "unsupported", "{crossed}");
    assert_eq!(
        crossed["diagnostics"][0]["code"], "missing_element_data",
        "{crossed}"
    );

    // Two compatible registrations and no ID: the request is ambiguous, not resolved by order.
    let ambiguous = masses(&carbon_app, "ambiguous", &carbon, None);
    assert_eq!(ambiguous["outcome"], "error", "{ambiguous}");
    assert_eq!(
        ambiguous["diagnostics"][0]["code"], "mass_data_ambiguous",
        "{ambiguous}"
    );
}

/// C-13 with three stated hydrogens, once as a doublet and once without a radical. Expected exact
/// mass, from exact decimal arithmetic: 13.00335483507 + 3 x 1.00782503223 = 16.02682993176. A
/// doublet is not an isotope or a hydrogen, so only the composition's counts could change it.
#[test]
fn radical_does_not_change_exact_mass() {
    // Each variant gets a fresh application: the repository hands out one document ID.
    let with_hydrogen_app = || {
        let snapshot = crate::mass_data::tests::decode(&carbon_and_hydrogen());
        application(vec![PersonalDataset::fixture(ProfileRecords::Phase4Nist(
            Box::new(snapshot),
        ))])
    };
    let exact_with = |label: &str, radical: &str| -> f64 {
        let app = with_hydrogen_app();
        let id = one_atom(&app, "C", 3);
        let edited = call(
            &app,
            &format!("edit-{label}"),
            "document.transaction",
            json!({"document_id":id,"expected_revision":1,"commands":[{
                "op":"update_atom","atom":"a1","isotope":13,"radical":radical
            }]}),
        );
        assert_eq!(edited["outcome"], "ok", "{edited}");
        let stored = serde_json::to_value(app.document(&id).unwrap()).unwrap();
        assert_eq!(
            stored["document"]["atoms"][0]["radical"], radical,
            "the edit must set the radical"
        );
        let reply = masses(&app, &format!("masses-{label}"), &id, None);
        assert_eq!(reply["outcome"], "ok", "{reply}");
        reply["result"]["components"][0]["mass"]["exact_u"]
            .as_f64()
            .unwrap()
    };
    let doublet = exact_with("doublet", "doublet");
    let plain = exact_with("plain", "none");
    assert!(
        (plain - 16.02682993176).abs() < 1e-12,
        "C-13 H3 exact mass must be 16.02682993176, got {plain}"
    );
    assert_eq!(
        doublet, plain,
        "a doublet radical must not change the exact mass"
    );
}
