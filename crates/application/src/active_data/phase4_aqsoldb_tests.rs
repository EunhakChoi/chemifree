use super::prepared_tests::{Fixture, assert_binding_unavailable, fixture};
use super::{ActiveDataError, ProfileRecords, phase4_aqsoldb};

const PATH: &str = "synthetic.csv";
const HEADER: &str = "ID,Name,InChI,InChIKey,SMILES,Solubility,SD,Ocurrences,Group,MolWt,MolLogP,MolMR,HeavyAtomCount,NumHAcceptors,NumHDonors,NumHeteroatoms,NumRotatableBonds,NumValenceElectrons,NumAromaticRings,NumSaturatedRings,NumAliphaticRings,RingCount,TPSA,LabuteASA,BalabanJ,BertzCT";

fn aqsoldb_fixture(invalid: bool) -> Fixture {
    let mut cells = vec![""; 26];
    cells[0] = "0007";
    cells[1] = if invalid {
        "\"unterminated"
    } else {
        "\"A, B\""
    };
    cells[5] = "-0.000";
    let row = cells.join(",");
    let bytes = format!("{HEADER}\r\n{}", vec![row; 9982].join("\r\n")).into_bytes();
    fixture(
        "private-independent-aqsoldb-v1.0-original",
        vec![(phase4_aqsoldb::FILE_ID, PATH, bytes)],
    )
}

#[test]
fn fixture_rights_are_checked_before_csv_payload_is_read() {
    let test_fixture = aqsoldb_fixture(false);
    let error = match test_fixture.load_production() {
        Err(error) => error,
        Ok(_) => panic!("fixture authorization unexpectedly succeeded"),
    };
    assert_eq!(error, ActiveDataError::RightsUnverified);
    assert_eq!(test_fixture.reads(), 0);
}

#[test]
fn complete_lexical_rows_are_typed_without_a_solubility_model() {
    let test_fixture = aqsoldb_fixture(false);
    let loaded = test_fixture.load().unwrap();
    let ProfileRecords::Phase4AqSolDb(data) = loaded.records() else {
        panic!("wrong dataset type")
    };
    assert_eq!(data.rows.len(), 9982);
    assert_eq!(data.rows[0].get(phase4_aqsoldb::Column::Id), "0007");
    assert_eq!(data.rows[0].get(phase4_aqsoldb::Column::Name), "A, B");
    assert_eq!(
        data.rows[0].get(phase4_aqsoldb::Column::Solubility),
        "-0.000"
    );
    assert_binding_unavailable(&test_fixture, &loaded);
}

#[test]
fn extra_columns_are_reported_without_names_or_cell_values() {
    let header = HEADER.replacen("InChI,", "private-column,InChI,", 1);
    let mut cells = vec![""; 26];
    cells[0] = "0007";
    cells.insert(2, "synthetic-secret");
    let row = cells.join(",");
    let bytes = format!("{header}\r\n{}", vec![row; 9982].join("\r\n")).into_bytes();
    let test_fixture = fixture(
        "private-independent-aqsoldb-v1.0-original",
        vec![(phase4_aqsoldb::FILE_ID, PATH, bytes)],
    );
    let dataset = test_fixture.load().unwrap();
    let status = crate::active_registry::RegistryEntryStatus::loaded(&dataset);
    assert_eq!(status.registration_id, dataset.registration_id());
    assert_eq!(status.retained_extra_columns, Some(1));
    let json = serde_json::to_string(&status).unwrap();
    assert!(json.contains("\"retained_extra_columns\":1"));
    assert!(!json.contains("private-column"));
    assert!(!json.contains("synthetic-secret"));

    let no_extra = aqsoldb_fixture(false).load().unwrap();
    let status = crate::active_registry::RegistryEntryStatus::loaded(&no_extra);
    assert_eq!(status.retained_extra_columns, None);
    assert!(
        !serde_json::to_string(&status)
            .unwrap()
            .contains("retained_extra_columns")
    );
}

#[test]
fn missing_corrupt_or_invalid_inputs_do_not_publish_data() {
    let mut missing = aqsoldb_fixture(false);
    missing.remove(PATH);
    assert!(matches!(
        missing.load(),
        Err(ActiveDataError::DataUnavailable)
    ));
    let mut corrupt = aqsoldb_fixture(false);
    corrupt.corrupt(PATH);
    assert!(matches!(
        corrupt.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));
    let invalid = aqsoldb_fixture(true);
    assert!(matches!(
        invalid.load(),
        Err(ActiveDataError::ContentInvalid)
    ));
}

#[test]
fn the_catalog_file_is_required_with_its_role_and_no_feature_binding() {
    let mut optional = aqsoldb_fixture(false);
    optional.mark_optional(PATH);
    assert!(matches!(
        optional.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut wrong_role = aqsoldb_fixture(false);
    wrong_role.set_role(PATH, "weights");
    assert!(matches!(
        wrong_role.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut unexpected_feature = aqsoldb_fixture(false);
    unexpected_feature.set_feature_hash(Some(&"0".repeat(64)));
    assert!(matches!(
        unexpected_feature.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
}
