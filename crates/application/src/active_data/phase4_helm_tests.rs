use super::prepared_tests::{Fixture, assert_binding_unavailable, fixture};
use super::{ActiveDataError, ProfileRecords, phase4_helm};

const PATH: &str = "independent-synthetic.json";
const ROW: &str = r#"{"monomerType":"Terminal","smiles":"C","name":"synthetic","author":"fixture","molfile":"synthetic","naturalAnalog":"X","rgroups":[{"capGroupSmiles":"C","capGroupSMILES":"N","alternateId":"R1","capGroupName":"cap","label":"R1"}],"createDate":null,"id":0,"polymerType":"synthetic","symbol":"X"}"#;

fn helm_fixture(invalid: bool) -> Fixture {
    let row = if invalid {
        ROW.replace("\"createDate\":null", "\"createDate\":false")
    } else {
        ROW.to_owned()
    };
    let bytes = format!("[{}]", vec![row; 705].join(",")).into_bytes();
    let mut test_fixture = fixture(
        "private-independent-helm-core-fecc972",
        vec![(phase4_helm::FILE_ID, PATH, bytes)],
    );
    test_fixture.set_role(PATH, "document_resource");
    test_fixture
}

#[test]
fn fixture_rights_are_checked_before_helm_payload_is_read() {
    let test_fixture = helm_fixture(false);
    let error = match test_fixture.load_production() {
        Err(error) => error,
        Ok(_) => panic!("fixture authorization unexpectedly succeeded"),
    };
    assert_eq!(error, ActiveDataError::RightsUnverified);
    assert_eq!(test_fixture.reads(), 0);
}

#[test]
fn complete_monomers_are_typed_without_a_sequence_engine() {
    let test_fixture = helm_fixture(false);
    let loaded = test_fixture.load().unwrap();
    let ProfileRecords::Phase4Helm(data) = loaded.records() else {
        panic!("wrong dataset type")
    };
    assert_eq!(data.monomers.len(), 705);
    assert_eq!(data.monomers[0].monomer_type, "Terminal");
    assert_eq!(
        data.monomers[0].rgroups[0].cap_group_smiles.as_deref(),
        Some("C")
    );
    assert_eq!(
        data.monomers[0].rgroups[0]
            .cap_group_smiles_upper
            .as_deref(),
        Some("N")
    );
    assert_binding_unavailable(&test_fixture, &loaded);
}

#[test]
fn missing_corrupt_or_invalid_inputs_do_not_publish_data() {
    let mut missing = helm_fixture(false);
    missing.remove(PATH);
    assert!(matches!(
        missing.load(),
        Err(ActiveDataError::DataUnavailable)
    ));
    let mut corrupt = helm_fixture(false);
    corrupt.corrupt(PATH);
    assert!(matches!(
        corrupt.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));
    let invalid = helm_fixture(true);
    assert!(matches!(
        invalid.load(),
        Err(ActiveDataError::ContentInvalid)
    ));
}

#[test]
fn the_single_catalog_file_is_required_with_its_role_and_no_feature_binding() {
    let mut optional = helm_fixture(false);
    optional.mark_optional(PATH);
    assert!(matches!(
        optional.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut wrong_role = helm_fixture(false);
    wrong_role.set_role(PATH, "auxiliary_data");
    assert!(matches!(
        wrong_role.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut unexpected_feature = helm_fixture(false);
    unexpected_feature.set_feature_hash(Some(&"0".repeat(64)));
    assert!(matches!(
        unexpected_feature.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
}
