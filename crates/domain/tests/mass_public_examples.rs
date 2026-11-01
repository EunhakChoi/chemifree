use chemifree_domain::element::Element;
use chemifree_domain::mass::{
    AverageBasis, Composition, ElementMass, ExactBasis, Isotope, MassError, MassTable,
    MissingAbundancePolicy,
};
use std::collections::BTreeMap;

fn e(symbol: &str) -> Element {
    Element::from_symbol(symbol).unwrap()
}
fn fixture() -> MassTable {
    // Test inputs only: published isotope masses, rounded representative abundance for C,
    // and simplified single-isotope abundance for the other fixture elements. This is not
    // a production element table. NIST source: https://physics.nist.gov/PhysRefData/Compositions/
    let rows = [
        (
            "C",
            12.011,
            12,
            vec![(12, 12.0, 0.9893), (13, 13.00335483507, 0.0107)],
        ),
        ("H", 1.008, 1, vec![(1, 1.00782503223, 1.0)]),
        ("O", 15.999, 16, vec![(16, 15.99491461957, 1.0)]),
        ("Na", 22.98976928, 23, vec![(23, 22.9897692820, 1.0)]),
    ];
    MassTable {
        elements: rows
            .into_iter()
            .map(|(symbol, average, selected_isotope, isotopes)| {
                (
                    e(symbol),
                    ElementMass {
                        average: Some(average),
                        average_basis: AverageBasis::ProfileNaturalWeight,
                        selected_isotope,
                        selection_basis: ExactBasis::PinnedProfileSelection,
                        isotopes: isotopes
                            .into_iter()
                            .map(|(number, mass, abundance)| Isotope {
                                number,
                                mass,
                                abundance: Some(abundance),
                            })
                            .collect(),
                    },
                )
            })
            .collect(),
        charge_correction: -0.0005485799090441,
        missing_abundance: MissingAbundancePolicy::Reject,
    }
}

#[test]
fn sodium_acetate_uses_hill_formula_and_independent_exact_mass() {
    // PubChem sodium acetate C2H3NaO2, monoisotopic mass 82.00307362 Da.
    let c = Composition {
        species: BTreeMap::from([
            ((e("C"), None), 2),
            ((e("H"), None), 3),
            ((e("Na"), None), 1),
            ((e("O"), None), 2),
        ]),
        charge: 0,
    };
    assert_eq!(c.formula(), "C2H3NaO2");
    let mass = c.masses(&fixture()).unwrap();
    assert!((mass.exact_u - 82.00307361783).abs() < 1e-10);
    assert!((mass.average_u - 82.03376928).abs() < 1e-10);
    let ion = Composition { charge: 1, ..c };
    assert_eq!(ion.formula(), "C2H3NaO2+");
    assert!(
        (ion.masses(&fixture()).unwrap().exact_u - (mass.exact_u - 0.0005485799090441)).abs()
            < 1e-10
    );
    let divalent = Composition { charge: -2, ..ion };
    assert_eq!(divalent.formula(), "C2H3NaO2^2-");
}

#[test]
fn carbon_pair_has_binomial_isotope_distribution_and_labeled_carbon_is_fixed() {
    let c2 = Composition {
        species: BTreeMap::from([((e("C"), None), 2)]),
        charge: 0,
    };
    let distribution = c2.distribution(&fixture()).unwrap();
    assert_eq!(distribution.abundance_sums, [(e("C"), 1.0)]);
    let peaks = distribution.peaks;
    assert_eq!(peaks.len(), 3);
    for (actual, (mass, probability)) in peaks.iter().zip([
        (24.0, 0.9893_f64.powi(2)),
        (25.00335483507, 2.0 * 0.9893 * 0.0107),
        (26.00670967014, 0.0107_f64.powi(2)),
    ]) {
        assert!((actual.mass_u - mass).abs() < 1e-10);
        assert!((actual.probability - probability).abs() < 1e-12);
    }
    let labeled = Composition {
        species: BTreeMap::from([((e("C"), Some(13)), 1), ((e("H"), None), 4)]),
        charge: 0,
    };
    assert_eq!(labeled.formula(), "[13C]H4");
    assert!(
        (labeled.masses(&fixture()).unwrap().exact_u - (13.00335483507 + 4.0 * 1.00782503223))
            .abs()
            < 1e-10
    );
}

#[test]
fn missing_abundances_and_combinatorial_explosion_are_not_partial_results() {
    let c = Composition {
        species: BTreeMap::from([((e("C"), None), 1)]),
        charge: 0,
    };
    let mut incomplete = fixture();
    incomplete.elements.get_mut(&e("C")).unwrap().isotopes[1].abundance = None;
    assert_eq!(
        c.distribution(&incomplete),
        Err(MassError::MissingAbundance)
    );
    let huge = Composition {
        species: BTreeMap::from([((e("C"), None), 5000)]),
        charge: 0,
    };
    let mut balanced = fixture();
    for isotope in &mut balanced.elements.get_mut(&e("C")).unwrap().isotopes {
        isotope.abundance = Some(0.5);
    }
    assert_eq!(huge.distribution(&balanced), Err(MassError::ResourceLimit));
    let rare = Composition {
        species: BTreeMap::from([((e("C"), None), 500)]),
        charge: 0,
    };
    assert_eq!(
        rare.distribution(&fixture()),
        Err(MassError::ProbabilityUnderflow)
    );
}

#[test]
fn rounded_abundances_are_normalized_only_with_reported_source_sum() {
    let mut table = fixture();
    table.elements.get_mut(&e("C")).unwrap().isotopes[0].abundance = Some(0.98929);
    let before = table.clone();
    let carbon = Composition {
        species: BTreeMap::from([((e("C"), None), 1)]),
        charge: 0,
    };
    let distribution = carbon.distribution(&table).unwrap();
    assert!((distribution.abundance_sums[0].1 - 0.99999).abs() < 1e-12);
    assert!(
        (distribution
            .peaks
            .iter()
            .map(|p| p.probability)
            .sum::<f64>()
            - 1.0)
            .abs()
            < 1e-12
    );
    assert_eq!(table, before, "calculation never changes the source values");
    let row = table.elements.get_mut(&e("C")).unwrap();
    row.isotopes[1].number = row.isotopes[0].number;
    assert_eq!(carbon.masses(&table), Err(MassError::InvalidData));
    assert_eq!(carbon.distribution(&table), Err(MassError::InvalidData));
}

#[test]
fn no_isotope_rows_use_natural_weight_only_for_unlabeled_exact_mass() {
    let mut table = fixture();
    table.elements.get_mut(&e("C")).unwrap().isotopes.clear();
    let unlabeled = Composition {
        species: BTreeMap::from([((e("C"), None), 2)]),
        charge: 0,
    };
    let masses = unlabeled.masses(&table).unwrap();
    assert_eq!(masses.exact_u, 24.022);
    assert_eq!(masses.average_u, 24.022);
    assert_eq!(masses.natural_weight_fallbacks, [e("C")]);
    assert_eq!(
        unlabeled.distribution(&table),
        Err(MassError::MissingAbundance)
    );
    let labeled = Composition {
        species: BTreeMap::from([((e("C"), Some(13)), 1)]),
        charge: 0,
    };
    assert_eq!(labeled.masses(&table), Err(MassError::MissingIsotope));
}
