//! The offline fixture is identified and hashed separately from production DATA.

use chemifree_conformance::{case::parse_case_bytes, run_case};

const CASE: &[u8] = include_bytes!("../../../conformance/cases/formula-and-mass.json");

#[test]
fn formula_and_masses_run_with_the_pinned_fixture() {
    let case = parse_case_bytes(CASE).unwrap();
    run_case(&case).unwrap();
}

#[test]
fn a_changed_fixture_identity_cannot_pass_conformance() {
    let mut case = parse_case_bytes(CASE).unwrap();
    case.data_fixture.as_mut().unwrap().sha256 = "0".repeat(64);
    assert_eq!(
        run_case(&case),
        Err("synthetic data fixture identity does not match its pinned hash".into())
    );
}
