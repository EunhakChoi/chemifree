//! Bind an already verified mass-input generation to the domain's calculation vocabulary.
//! Loader success alone still does not establish an independently validated model result.

use std::collections::BTreeMap;

use chemifree_domain::element::Element;
use chemifree_domain::mass::{
    AverageBasis, ElementMass, ExactBasis, Isotope, MassTable, MissingAbundancePolicy,
};

use crate::mass_input::MassInput;
use crate::phase4_nist::NistSnapshot;

/// CODATA 2022 electron mass in u. Ion correction ignores binding-energy differences.
/// https://physics.nist.gov/cgi-bin/cuu/Value?meu
pub(crate) const CODATA_2022_ELECTRON_MASS_U: f64 = 0.000_548_579_909_044_1;

/// This adapter is selected only for the catalog-pinned mass input profile. The public
/// functional contract defines an absent abundance token as zero; the original token flag
/// remains in the retained source data. Percentages are converted to fractions here. The
/// schema ID is opaque provenance; the catalog-bound structural profile determines the
/// meaning of its fields, and units must be recognized before any value is interpreted.
pub(crate) fn bind(input: &MassInput) -> Result<MassTable, &'static str> {
    if !matches!(input.features.mass_unit.as_str(), "u" | "Da")
        || !matches!(
            input.features.natural_atomic_weight_unit.as_str(),
            "u" | "Da"
        )
        || !matches!(input.features.abundance_unit.as_str(), "%" | "percent")
        || !matches!(input.signed_charge_mass.unit.as_str(), "u/e" | "Da/e")
    {
        return Err("unsupported_mass_profile");
    }
    let mut elements = BTreeMap::new();
    for row in &input.elements {
        let number = u8::try_from(row.atomic_number_u32).map_err(|_| "element_identity")?;
        let element = Element::from_atomic_number(number).ok_or("element_identity")?;
        if element.symbol() != row.symbol {
            return Err("element_identity");
        }
        let selected = input
            .selections
            .iter()
            .find(|item| item.atomic_number_u32 == row.atomic_number_u32);
        let selected_isotope = selected.map_or(Ok(0), |item| {
            u16::try_from(item.mass_number_u32).map_err(|_| "isotope_identity")
        })?;
        let isotopes = input
            .isotopes
            .iter()
            .filter(|item| item.atomic_number_u32 == row.atomic_number_u32)
            .map(|item| {
                Ok(Isotope {
                    number: u16::try_from(item.mass_number_u32).map_err(|_| "isotope_identity")?,
                    mass: f64::from_bits(item.mass.binary64_bits),
                    abundance: Some(f64::from_bits(item.abundance_percent.binary64_bits) / 100.0),
                })
            })
            .collect::<Result<Vec<_>, &'static str>>()?;
        elements.insert(
            element,
            ElementMass {
                average: Some(f64::from_bits(row.natural_atomic_weight.binary64_bits)),
                average_basis: AverageBasis::ProfileNaturalWeight,
                selected_isotope,
                selection_basis: if selected_isotope == 0 {
                    ExactBasis::Unavailable
                } else {
                    ExactBasis::PinnedProfileSelection
                },
                isotopes,
            },
        );
    }
    Ok(MassTable {
        elements,
        charge_correction: f64::from_bits(input.signed_charge_mass.binary64_bits),
        missing_abundance: MissingAbundancePolicy::Reject,
    })
}

/// Bind a catalog-verified NIST snapshot to an explicitly conditional representative profile.
/// When reported fractions sum near one, the mean is their normalized isotope-weighted
/// mass. The input's free-form standard-weight kind has no published scalar vocabulary,
/// so no reported standard-weight value is interpreted as a scalar fallback. An isotope
/// whose fraction is missing remains unknown, and conditional results list each omission.
pub(crate) fn bind_nist(input: &NistSnapshot) -> Result<MassTable, &'static str> {
    let mut elements = BTreeMap::new();
    for row in &input.elements {
        let number = row
            .atomic_number
            .to_string()
            .parse::<u8>()
            .map_err(|_| "element_identity")?;
        let element = Element::from_atomic_number(number).ok_or("element_identity")?;
        if element.symbol() != row.symbol_as_served || elements.contains_key(&element) {
            return Err("element_identity");
        }
        elements.insert(
            element,
            ElementMass {
                average: None,
                average_basis: AverageBasis::Unavailable,
                selected_isotope: 0,
                selection_basis: ExactBasis::Unavailable,
                isotopes: Vec::new(),
            },
        );
    }
    for row in &input.isotopes {
        let number = row
            .atomic_number
            .to_string()
            .parse::<u8>()
            .map_err(|_| "element_identity")?;
        let element = Element::from_atomic_number(number).ok_or("element_identity")?;
        if element.symbol() != row.element_symbol {
            return Err("element_identity");
        }
        let number = row
            .mass_number
            .to_string()
            .parse::<u16>()
            .map_err(|_| "isotope_identity")?;
        if !(1..=999).contains(&number) {
            return Err("isotope_identity");
        }
        let mass = row
            .relative_atomic_mass
            .value_decimal
            .parse::<f64>()
            .map_err(|_| "isotope_mass")?;
        if !mass.is_finite() || mass <= 0.0 {
            return Err("isotope_mass");
        }
        let abundance = row
            .isotopic_composition
            .as_ref()
            .map(|value| {
                value
                    .value_decimal
                    .parse::<f64>()
                    .map_err(|_| "isotope_abundance")
            })
            .transpose()?;
        if abundance.is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value)) {
            return Err("isotope_abundance");
        }
        let entry = elements.get_mut(&element).ok_or("element_identity")?;
        if entry
            .isotopes
            .iter()
            .any(|isotope| isotope.number == number)
        {
            return Err("isotope_identity");
        }
        entry.isotopes.push(Isotope {
            number,
            mass,
            abundance,
        });
    }
    for row in elements.values_mut() {
        let mut sum = 0.0;
        let mut weighted = 0.0;
        let mut best: Option<(u16, f64)> = None;
        let mut tied = false;
        for isotope in &row.isotopes {
            if let Some(abundance) = isotope.abundance {
                sum += abundance;
                weighted += isotope.mass * abundance;
                if abundance > 0.0 && best.is_none_or(|(_, previous)| abundance > previous) {
                    best = Some((isotope.number, abundance));
                    tied = false;
                } else if abundance > 0.0 && best.is_some_and(|(_, previous)| abundance == previous)
                {
                    tied = true;
                }
            }
        }
        if (0.999..=1.001).contains(&sum) {
            if let Some((number, _)) = best
                && !tied
            {
                row.selected_isotope = number;
                row.selection_basis = ExactBasis::MaximumReportedFraction;
            }
            row.average = Some(weighted / sum);
            row.average_basis = AverageBasis::ReportedIsotopeComposition;
        }
    }
    Ok(MassTable {
        elements,
        charge_correction: -CODATA_2022_ELECTRON_MASS_U,
        missing_abundance: MissingAbundancePolicy::ConditionOnReported,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use chemifree_domain::mass::Composition;
    use serde_json::{Value, json};

    pub(crate) fn fixture() -> Value {
        // Synthetic carrier using selected public NIST C masses/fractions; not a production dataset.
        let source = json!({"path":"synthetic","bytes":1,"sha256":"fixture"});
        let isotope = |number: u16, mass: &str, fraction: Option<&str>| {
            json!({
                "atomic_number":6,"element_symbol":"C","isotope_symbol_as_printed":null,"mass_number":number,
                "relative_atomic_mass":{"raw":mass,"value_decimal":mass,"uncertainty_digits":"1","absolute_uncertainty_decimal":"0.1","estimated_marker_present":false},
                "isotopic_composition":fraction.map(|value| json!({"raw":value,"value_decimal":value,"uncertainty_digits":null,"absolute_uncertainty_decimal":null,"estimated_marker_present":false})),
                "source_preformatted_line":1
            })
        };
        json!({
            "record_id":"synthetic","source":source,"source_url":"inert:example","version":"fixture",
            "version_basis":source,"source_evaluation_epochs":{"masses":"fixture","isotopic_compositions":"fixture","standard_atomic_weights":"fixture"},
            "distribution_basis":{"classification":"fixture","classification_source":source,"terms":"fixture","terms_source":source,"terms_url":"inert:terms"},
            "citation":"fixture","normalization":{"quantities":"fixture","decimal_policy":"fixture","estimated_marker":"fixture","bracketed_singleton":"fixture","missing_policy":"fixture","symbols":"fixture","changes":"fixture"},
            "element_count":1,"isotope_count":3,
            "elements":[{"atomic_number":6,"symbol_as_served":"C","standard_atomic_weight":{"kind":"range","raw":null,"lower_decimal":"12.0096","upper_decimal":"12.0116","standard_atomic_weight_value":null},"notes_raw":null}],
            "isotopes":[isotope(12,"12.0",Some("0.9893")),isotope(13,"13.00335483507",Some("0.0107")),isotope(14,"14.0032419884",None)],
            "reported_composition_sums_decimal":{},"limits":[]
        })
    }

    pub(crate) fn decode(value: &Value) -> NistSnapshot {
        let bytes = serde_json::to_vec(value).unwrap();
        crate::phase4_nist::decode_dataset(&[(crate::phase4_nist::FILE_ID, &bytes)]).unwrap()
    }

    pub(crate) fn synthetic_mass_input() -> MassInput {
        let bytes: [&[u8]; 7] = [
            include_bytes!("../tests/data/mass-independent-v3/elements.json"),
            include_bytes!("../tests/data/mass-independent-v3/isotopes.json"),
            include_bytes!("../tests/data/mass-independent-v3/selection.json"),
            include_bytes!("../tests/data/mass-independent-v3/aliases.json"),
            include_bytes!("../tests/data/mass-independent-v3/constants.json"),
            include_bytes!("../tests/data/mass-independent-v3/features.json"),
            include_bytes!("../tests/data/mass-independent-v3/sources.json"),
        ];
        let files: Vec<_> = crate::mass_input::NORMALIZED_PATHS
            .into_iter()
            .zip(bytes)
            .collect();
        let mut input = crate::mass_input::decode(&files).unwrap();
        // These arbitrary fixture masses are not scientific data. Use valid element
        // identities only to exercise the calculation adapter's metadata checks.
        input.elements[0].symbol = "Ga".into();
        input.elements[1].symbol = "Ag".into();
        input.features.mass_unit = "u".into();
        input.features.natural_atomic_weight_unit = "u".into();
        input.features.abundance_unit = "%".into();
        input.signed_charge_mass.unit = "u/e".into();
        input
    }

    #[test]
    fn mass_binding_refuses_unknown_units() {
        let input = synthetic_mass_input();
        assert!(bind(&input).is_ok());
        let mut aliases = synthetic_mass_input();
        aliases.features.schema_id = "other-opaque-source-id".into();
        aliases.features.mass_unit = "Da".into();
        aliases.features.natural_atomic_weight_unit = "Da".into();
        aliases.features.abundance_unit = "percent".into();
        aliases.signed_charge_mass.unit = "Da/e".into();
        assert_eq!(bind(&input), bind(&aliases));
        for field in 0..4 {
            let mut input = synthetic_mass_input();
            match field {
                0 => input.features.mass_unit = "kg".into(),
                1 => input.features.natural_atomic_weight_unit = "kg".into(),
                2 => input.features.abundance_unit = "fraction".into(),
                3 => input.signed_charge_mass.unit = "kg/C".into(),
                _ => unreachable!(),
            }
            assert_eq!(
                bind(&input),
                Err("unsupported_mass_profile"),
                "field {field}"
            );
        }
    }

    #[test]
    fn nist_interval_cannot_supply_a_contradictory_scalar_fallback() {
        let mut value = fixture();
        value["isotopes"][0]["isotopic_composition"]["value_decimal"] = json!("0.1");
        value["isotopes"][1]["isotopic_composition"]["value_decimal"] = json!("0.1");
        value["elements"][0]["standard_atomic_weight"]["reported"] = json!({
            "raw":"42", "value_decimal":"42", "uncertainty_digits":"1",
            "absolute_uncertainty_decimal":"0.1", "estimated_marker_present":false
        });
        let table = bind_nist(&decode(&value)).unwrap();
        assert_eq!(table.elements[&Element::CARBON].average, None);
        assert_eq!(
            table.elements[&Element::CARBON].average_basis,
            AverageBasis::Unavailable
        );
    }

    #[test]
    fn public_profile_uses_reported_fractions_and_names_missing_abundances() {
        let table = bind_nist(&decode(&fixture())).unwrap();
        let carbon = Element::CARBON;
        let composition = Composition {
            species: BTreeMap::from([((carbon, None), 1)]),
            charge: 0,
        };
        let masses = composition.masses(&table).unwrap();
        assert_eq!(masses.exact_u, 12.0);
        assert!((masses.average_u - (12.0 * 0.9893 + 13.00335483507 * 0.0107)).abs() < 1e-12);
        assert_eq!(
            masses.average_bases,
            [(carbon, AverageBasis::ReportedIsotopeComposition)]
        );
        assert_eq!(masses.average_omitted_abundances, [(carbon, 14)]);
        assert_eq!(
            masses.exact_selections,
            [(carbon, 12, ExactBasis::MaximumReportedFraction)]
        );
        assert_eq!(masses.exact_omitted_abundances, [(carbon, 14)]);
        let distribution = composition.distribution(&table).unwrap();
        assert_eq!(distribution.omitted_abundances, [(carbon, 14)]);
        assert_eq!(distribution.peaks.len(), 2);
        assert!((distribution.peaks[1].probability - 0.0107).abs() < 1e-12);
        let labeled = Composition {
            species: BTreeMap::from([((carbon, Some(14)), 1)]),
            charge: 0,
        };
        assert!((labeled.masses(&table).unwrap().exact_u - 14.0032419884).abs() < 1e-12);
        assert_eq!(
            labeled.distribution(&table).unwrap().peaks[0].probability,
            1.0
        );
    }

    #[test]
    fn duplicate_isotope_identities_do_not_publish_a_table() {
        let mut value = fixture();
        value["isotopes"][2]["mass_number"] = json!(13);
        assert_eq!(bind_nist(&decode(&value)), Err("isotope_identity"));
    }

    #[test]
    fn incomplete_reported_fractions_do_not_select_an_exact_isotope() {
        let mut value = fixture();
        value["isotopes"][0]["isotopic_composition"]["value_decimal"] = json!("0.1");
        value["isotopes"][1]["isotopic_composition"]["value_decimal"] = json!("0.1");
        let table = bind_nist(&decode(&value)).unwrap();
        let composition = Composition {
            species: BTreeMap::from([((Element::CARBON, None), 1)]),
            charge: 0,
        };
        assert_eq!(
            composition.masses(&table),
            Err(chemifree_domain::mass::MassError::MissingIsotope)
        );
    }

    #[test]
    fn equal_maximum_reported_fractions_do_not_pick_an_arbitrary_isotope() {
        let mut value = fixture();
        value["isotopes"][0]["isotopic_composition"]["value_decimal"] = json!("0.5");
        value["isotopes"][1]["isotopic_composition"]["value_decimal"] = json!("0.5");
        let table = bind_nist(&decode(&value)).unwrap();
        let composition = Composition {
            species: BTreeMap::from([((Element::CARBON, None), 1)]),
            charge: 0,
        };
        assert_eq!(
            composition.masses(&table),
            Err(chemifree_domain::mass::MassError::MissingIsotope)
        );
    }
}
