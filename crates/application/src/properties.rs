//! Snapshot-based property composition, independent of transports and runtime data loading.

use std::collections::BTreeMap;

use chemifree_domain::document::{AtomId, Document, MAX_ISOTOPE_MASS_NUMBER};
use chemifree_domain::mass::{
    Composition, FormulaError, FormulaScope, MassError, MassTable, atom_groups, composition,
};
use chemifree_domain::scene::Selection;
use chemifree_numerics::{Binary64, format_fixed};
use serde::{Deserialize, Serialize};

use crate::{DataRegistry, ProfileRecords};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PropertyKind {
    Formula,
    Masses,
    Distribution,
}

impl PropertyKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Formula => "formula",
            Self::Masses => "masses",
            Self::Distribution => "distribution",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PropertiesScope {
    Molecule,
    Selection,
    Components,
}

impl PropertiesScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Molecule => "molecule",
            Self::Selection => "selection",
            Self::Components => "components",
        }
    }
    pub const fn domain(self) -> FormulaScope {
        match self {
            Self::Molecule => FormulaScope::Molecule,
            Self::Selection => FormulaScope::Selection,
            Self::Components => FormulaScope::Components,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PropertiesError {
    Formula(FormulaError),
    DataUnavailable,
    DataAmbiguous,
    DataInvalid(&'static str),
    Mass(MassError),
    Display,
}

/// Whether some checked generation can answer at least one request of each kind.
/// Per-element coverage and request-specific chemistry are checked when calculating.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct DataReadiness {
    pub mass: bool,
    pub distribution: bool,
}

pub(crate) fn data_readiness(registry: &DataRegistry) -> DataReadiness {
    let mut readiness = DataReadiness::default();
    for dataset in registry.datasets() {
        let table = match dataset.records() {
            ProfileRecords::MassInput(input) => crate::mass_data::bind(input),
            ProfileRecords::Phase4Nist(input) => crate::mass_data::bind_nist(input),
            _ => continue,
        };
        if let Ok(table) = table {
            for (&element, row) in &table.elements {
                // The drawing model can request natural abundance or a recorded isotope
                // in its 1..=999 range. Check both: an explicit isotope can yield a
                // single-peak distribution even when natural abundances are unavailable.
                let explicit = row
                    .isotopes
                    .iter()
                    .find(|item| (1..=MAX_ISOTOPE_MASS_NUMBER).contains(&item.number))
                    .map(|item| item.number);
                for isotope in [None, explicit] {
                    if readiness.mass && readiness.distribution {
                        break;
                    }
                    let probe = Composition {
                        species: BTreeMap::from([((element, isotope), 1)]),
                        charge: 0,
                    };
                    if probe.masses(&table).is_ok() {
                        readiness.mass = true;
                        readiness.distribution |= probe.distribution(&table).is_ok();
                    }
                }
            }
        }
    }
    readiness
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MassDataView {
    pub registration_id: String,
    pub generation: u64,
    pub dataset_id: String,
    pub dataset_version: String,
    pub profile_id: String,
    pub manifest_sha256: String,
    /// Opaque input provenance; not a separate chemistry policy or unit declaration.
    pub source_schema_id: Option<String>,
    pub calculation_profile: &'static str,
    pub charge_correction_u_per_e: f64,
    pub charge_convention: &'static str,
    pub isotope_selection: &'static str,
    pub abundance_policy: &'static str,
    pub uncertainty: &'static str,
}

/// Select a checked external dataset, never an arbitrary numeric payload from an API request.
/// Without an explicit ID exactly one compatible registration must be available.
pub(crate) fn select_data(
    registry: &DataRegistry,
    registration_id: Option<&str>,
) -> Result<(MassDataView, MassTable), PropertiesError> {
    let mut matches = registry.datasets().iter().filter(|dataset| {
        registration_id.is_none_or(|id| dataset.registration_id() == id)
            && matches!(
                dataset.records(),
                ProfileRecords::MassInput(_) | ProfileRecords::Phase4Nist(_)
            )
    });
    let selected = matches.next().ok_or(PropertiesError::DataUnavailable)?;
    if matches.next().is_some() {
        return Err(PropertiesError::DataAmbiguous);
    }
    let (table, calculation_profile, charge_convention, isotope_selection, abundance_policy) =
        match selected.records() {
            ProfileRecords::MassInput(input) => (
                crate::mass_data::bind(input).map_err(PropertiesError::DataInvalid)?,
                "chemifree-mass-input-1",
                "signed coefficient supplied by the selected input profile",
                "pinned source selection; explicit isotopes override it",
                "profile-defined percentages; absent token is zero",
            ),
            ProfileRecords::Phase4Nist(input) => (
                crate::mass_data::bind_nist(input).map_err(PropertiesError::DataInvalid)?,
                "chemifree-nist-representative-1",
                "subtract CODATA 2022 electron mass per positive elementary charge; binding energy omitted",
                "unique maximum reported fraction when the reported sum is near one; explicit isotopes override it",
                "conditional on reported fractions; omitted unknown rows and pre-normalization sums are reported",
            ),
            _ => return Err(PropertiesError::DataUnavailable),
        };
    Ok((
        MassDataView {
            registration_id: selected.registration_id().into(),
            generation: selected.generation(),
            dataset_id: selected.dataset_id().into(),
            dataset_version: selected.dataset_version().into(),
            profile_id: selected.profile_id().into(),
            manifest_sha256: selected.manifest_sha256().into(),
            source_schema_id: match selected.records() {
                ProfileRecords::MassInput(input) => Some(input.features.schema_id.clone()),
                _ => None,
            },
            calculation_profile,
            charge_correction_u_per_e: table.charge_correction,
            charge_convention,
            isotope_selection,
            abundance_policy,
            uncertainty: "source uncertainties and binding-energy differences are not propagated",
        },
        table,
    ))
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MassView {
    pub average_u: f64,
    pub exact_u: f64,
    pub average_display: String,
    pub exact_display: String,
    /// These elements used a documented natural-weight fallback for exact-profile mass.
    pub exact_natural_weight_fallbacks: Vec<String>,
    pub average_bases: Vec<AverageBasisView>,
    pub average_omitted_abundances: Vec<OmittedAbundance>,
    pub exact_selections: Vec<ExactSelectionView>,
    pub exact_omitted_abundances: Vec<OmittedAbundance>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExactSelectionView {
    pub element: String,
    pub isotope: u16,
    pub basis: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AverageBasisView {
    pub element: String,
    pub basis: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PeakView {
    pub mass_u: f64,
    pub mass_display: String,
    pub probability: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AbundanceSum {
    pub element: String,
    pub input_sum: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OmittedAbundance {
    pub element: String,
    pub isotope: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ComponentView {
    pub atoms: Vec<String>,
    pub formula: String,
    pub formal_charge: i32,
    pub mass: Option<MassView>,
    pub isotope_distribution: Option<Vec<PeakView>>,
    pub abundance_sums: Vec<AbundanceSum>,
    pub omitted_abundances: Vec<OmittedAbundance>,
}

fn display(value: f64, decimals: u8) -> Result<String, PropertiesError> {
    format_fixed(Binary64::from_bits(value.to_bits()), decimals)
        .map_err(|_| PropertiesError::Display)
}

/// The inferred-H map must come from a model applied to this same complete graph snapshot.
/// All component results are assembled locally: any error publishes none of them.
pub(crate) fn calculate(
    document: &Document,
    scope: FormulaScope,
    selection: Option<&Selection>,
    inferred_hydrogens: &BTreeMap<AtomId, u16>,
    kind: PropertyKind,
    table: Option<&MassTable>,
    decimals: u8,
) -> Result<Vec<ComponentView>, PropertiesError> {
    if decimals > 8 {
        return Err(PropertiesError::Display);
    }
    let groups = atom_groups(document, scope, selection).map_err(PropertiesError::Formula)?;
    let mut output = Vec::with_capacity(groups.len());
    for atoms in groups {
        let composition =
            composition(document, &atoms, inferred_hydrogens).map_err(PropertiesError::Formula)?;
        let mut row = ComponentView {
            atoms: atoms.iter().map(ToString::to_string).collect(),
            formula: composition.formula(),
            formal_charge: composition.charge,
            mass: None,
            isotope_distribution: None,
            abundance_sums: Vec::new(),
            omitted_abundances: Vec::new(),
        };
        if kind != PropertyKind::Formula {
            let table = table.ok_or(PropertiesError::DataUnavailable)?;
            let masses = composition.masses(table).map_err(PropertiesError::Mass)?;
            row.mass = Some(MassView {
                average_u: masses.average_u,
                exact_u: masses.exact_u,
                average_display: display(masses.average_u, decimals)?,
                exact_display: display(masses.exact_u, decimals)?,
                exact_natural_weight_fallbacks: masses
                    .natural_weight_fallbacks
                    .iter()
                    .map(|e| e.symbol().to_owned())
                    .collect(),
                average_bases: masses
                    .average_bases
                    .iter()
                    .map(|(element, basis)| AverageBasisView {
                        element: element.symbol().to_owned(),
                        basis: basis.as_str(),
                    })
                    .collect(),
                average_omitted_abundances: masses
                    .average_omitted_abundances
                    .iter()
                    .map(|(element, isotope)| OmittedAbundance {
                        element: element.symbol().to_owned(),
                        isotope: *isotope,
                    })
                    .collect(),
                exact_selections: masses
                    .exact_selections
                    .iter()
                    .map(|(element, isotope, basis)| ExactSelectionView {
                        element: element.symbol().to_owned(),
                        isotope: *isotope,
                        basis: basis.as_str(),
                    })
                    .collect(),
                exact_omitted_abundances: masses
                    .exact_omitted_abundances
                    .iter()
                    .map(|(element, isotope)| OmittedAbundance {
                        element: element.symbol().to_owned(),
                        isotope: *isotope,
                    })
                    .collect(),
            });
            if kind == PropertyKind::Distribution {
                let distribution = composition
                    .distribution(table)
                    .map_err(PropertiesError::Mass)?;
                row.abundance_sums = distribution
                    .abundance_sums
                    .into_iter()
                    .map(|(element, input_sum)| AbundanceSum {
                        element: element.symbol().to_owned(),
                        input_sum,
                    })
                    .collect();
                row.omitted_abundances = distribution
                    .omitted_abundances
                    .into_iter()
                    .map(|(element, isotope)| OmittedAbundance {
                        element: element.symbol().to_owned(),
                        isotope,
                    })
                    .collect();
                row.isotope_distribution = Some(
                    distribution
                        .peaks
                        .into_iter()
                        .map(|peak| {
                            Ok(PeakView {
                                mass_u: peak.mass_u,
                                mass_display: display(peak.mass_u, decimals)?,
                                probability: peak.probability,
                            })
                        })
                        .collect::<Result<_, PropertiesError>>()?,
                );
            }
        }
        output.push(row);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chemifree_domain::document::{Atom, Command, DocumentId, NewAtomPositions, Point};
    use chemifree_domain::element::Element;
    use chemifree_domain::mass::{
        AverageBasis, ElementMass, ExactBasis, Isotope, MissingAbundancePolicy,
    };
    use chemifree_domain::states::Hydrogens;

    #[test]
    fn selected_mass_profile_reports_opaque_schema_and_refuses_unknown_units() {
        let input = crate::mass_data::tests::synthetic_mass_input();
        let dataset = crate::PersonalDataset::fixture(ProfileRecords::MassInput(Box::new(input)));
        let registry = DataRegistry::fixture(vec![dataset]);
        let (source, _) = select_data(&registry, None).unwrap();
        assert_eq!(
            source.source_schema_id.as_deref(),
            Some("independent-test-v3")
        );
        assert_eq!(source.calculation_profile, "chemifree-mass-input-1");

        let mut input = crate::mass_data::tests::synthetic_mass_input();
        input.signed_charge_mass.unit = "kg/C".into();
        let dataset = crate::PersonalDataset::fixture(ProfileRecords::MassInput(Box::new(input)));
        let registry = DataRegistry::fixture(vec![dataset]);
        assert!(matches!(
            select_data(&registry, None),
            Err(PropertiesError::DataInvalid("unsupported_mass_profile"))
        ));
    }

    fn methane() -> Document {
        let mut document = Document::new(DocumentId::parse("methane").unwrap(), "Methane").unwrap();
        document
            .apply(&[Command::AddAtom {
                atom: Atom::from(Element::CARBON),
                positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
                bond_to: None,
            }])
            .unwrap();
        document
    }

    fn table() -> MassTable {
        // Test-only values: published NIST isotope masses and simplified abundances.
        let h = Element::from_symbol("H").unwrap();
        MassTable {
            charge_correction: -0.0005485799090441,
            missing_abundance: MissingAbundancePolicy::Reject,
            elements: BTreeMap::from([
                (
                    Element::CARBON,
                    ElementMass {
                        average: Some(12.011),
                        average_basis: AverageBasis::ProfileNaturalWeight,
                        selected_isotope: 12,
                        selection_basis: ExactBasis::PinnedProfileSelection,
                        isotopes: vec![
                            Isotope {
                                number: 12,
                                mass: 12.0,
                                abundance: Some(0.9893),
                            },
                            Isotope {
                                number: 13,
                                mass: 13.00335483507,
                                abundance: Some(0.0107),
                            },
                        ],
                    },
                ),
                (
                    h,
                    ElementMass {
                        average: Some(1.008),
                        average_basis: AverageBasis::ProfileNaturalWeight,
                        selected_isotope: 1,
                        selection_basis: ExactBasis::PinnedProfileSelection,
                        isotopes: vec![Isotope {
                            number: 1,
                            mass: 1.00782503223,
                            abundance: Some(1.0),
                        }],
                    },
                ),
            ]),
        }
    }

    #[test]
    fn numeric_display_is_a_result_snapshot_not_a_document_edit() {
        let document = methane();
        let before = document.clone();
        let atom = document.atoms().next().unwrap().0;
        let inferred = BTreeMap::from([(atom, 4)]);
        let two = calculate(
            &document,
            FormulaScope::Molecule,
            None,
            &inferred,
            PropertyKind::Masses,
            Some(&table()),
            2,
        )
        .unwrap();
        let five = calculate(
            &document,
            FormulaScope::Molecule,
            None,
            &inferred,
            PropertyKind::Masses,
            Some(&table()),
            5,
        )
        .unwrap();
        assert_eq!(two[0].formula, "CH4");
        assert_eq!(two[0].mass.as_ref().unwrap().exact_display, "16.03");
        assert_eq!(five[0].mass.as_ref().unwrap().exact_display, "16.03130");
        assert_eq!(
            two[0].mass.as_ref().unwrap().exact_u,
            five[0].mass.as_ref().unwrap().exact_u
        );
        assert_eq!(document, before);
    }

    #[test]
    fn unavailable_data_and_undetermined_hydrogen_never_return_a_partial_result() {
        let document = methane();
        let atom = document.atoms().next().unwrap().0;
        assert_eq!(
            document.atoms().next().unwrap().1.hydrogens,
            Hydrogens::Inferred
        );
        assert_eq!(
            calculate(
                &document,
                FormulaScope::Molecule,
                None,
                &BTreeMap::new(),
                PropertyKind::Formula,
                None,
                2
            ),
            Err(PropertiesError::Formula(FormulaError::UnknownHydrogens(
                atom
            )))
        );
        let inferred = BTreeMap::from([(atom, 4)]);
        assert_eq!(
            calculate(
                &document,
                FormulaScope::Molecule,
                None,
                &inferred,
                PropertyKind::Masses,
                None,
                2
            ),
            Err(PropertiesError::DataUnavailable)
        );
    }
}
