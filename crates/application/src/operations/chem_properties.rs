//! chem.properties: formula, mass and isotope distribution over one snapshot, and its failure codes.

use chemifree_domain::budget::Interrupt;
use serde_json::Map;

use crate::contract::{SelectionDto, TransactionSelections};
use crate::properties::{self, PropertiesError, PropertiesScope, PropertyKind};
use crate::{Application, CapabilityCatalog, DocumentRepository};

use super::reply::{
    Diagnostic, Loss, LossConsent, LossDisposition, OperationResponse, OperationResult, Outcome,
    PropertiesResult, PropertyValenceWarning, SemanticImpact,
};
use super::{Known, diagnostic, failure, stopped_read_only};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PropertiesParameters {
    document_id: String,
    scope: PropertiesScope,
    property: PropertyKind,
    #[serde(default)]
    selection: Option<SelectionDto>,
    #[serde(default)]
    data_registration_id: Option<String>,
    /// The time limit this request asks for, in milliseconds: any value up to the server's maximum
    /// (so it may be above the server's default); above the maximum it is refused.
    #[serde(default)]
    time_limit_ms: Option<u64>,
}

fn property_failure(error: PropertiesError) -> (bool, Diagnostic) {
    use chemifree_domain::mass::{FormulaError, MassError};
    let mut details = Map::new();
    let (unsupported, code, message) = match error {
        PropertiesError::Formula(FormulaError::NoAtoms) => {
            (false, "empty_scope", "The chosen scope contains no atoms.")
        }
        PropertiesError::Formula(
            FormulaError::MissingSelection | FormulaError::InvalidSelection,
        ) => (
            false,
            "invalid_selection",
            "The selection is missing or contains unknown identifiers.",
        ),
        PropertiesError::Formula(FormulaError::UnknownAtom(atom)) => {
            details.insert("atom".into(), atom.to_string().into());
            (
                false,
                "invalid_selection",
                "The selection names an unknown atom.",
            )
        }
        PropertiesError::Formula(FormulaError::QueryAtom(atom)) => {
            details.insert("atom".into(), atom.to_string().into());
            (
                true,
                "query_atom",
                "A query atom does not determine an element for a formula.",
            )
        }
        PropertiesError::Formula(FormulaError::UnknownHydrogens(atom)) => {
            details.insert("atom".into(), atom.to_string().into());
            (
                true,
                "hydrogens_unavailable",
                "The hydrogen count is not determined by the selected valence model.",
            )
        }
        PropertiesError::Formula(FormulaError::UnknownAttachment(atom)) => {
            details.insert("atom".into(), atom.to_string().into());
            (
                true,
                "attachment_unknown",
                "An attachment point does not determine a complete formula.",
            )
        }
        PropertiesError::Formula(FormulaError::CountOverflow) => (
            false,
            "resource_limit",
            "Formula counts exceed the supported range.",
        ),
        PropertiesError::DataUnavailable => (
            true,
            "mass_data_unavailable",
            "No selected, compatible external mass dataset is available.",
        ),
        PropertiesError::DataAmbiguous => (
            false,
            "mass_data_ambiguous",
            "Select one compatible mass data registration.",
        ),
        PropertiesError::DataInvalid(reason) => {
            details.insert("reason".into(), reason.into());
            (
                true,
                "mass_data_invalid",
                "The selected data cannot supply this mass profile.",
            )
        }
        PropertiesError::Mass(reason) => {
            let (code, message) = match reason {
                MassError::MissingElement => (
                    "missing_element_data",
                    "The selected dataset lacks an element.",
                ),
                MassError::MissingIsotope => (
                    "isotope_selection_unavailable",
                    "The selected isotope or an unambiguous default is unavailable.",
                ),
                MassError::MissingAverage => (
                    "average_mass_unavailable",
                    "A defensible average mass is unavailable for an element.",
                ),
                MassError::MissingAbundance => (
                    "abundance_unavailable",
                    "The selected dataset cannot determine this isotope distribution.",
                ),
                MassError::InvalidData => (
                    "mass_data_invalid",
                    "The selected mass data contain an unsupported value.",
                ),
                MassError::ResourceLimit => (
                    "resource_limit",
                    "The isotope distribution exceeds its work or peak limit.",
                ),
                MassError::ProbabilityUnderflow => (
                    "probability_underflow",
                    "An isotope probability cannot be represented; no partial distribution is returned.",
                ),
            };
            (reason != MassError::ResourceLimit, code, message)
        }
        PropertiesError::Display => (
            false,
            "numeric_precision_invalid",
            "The mass display precision cannot be applied.",
        ),
    };
    (unsupported, diagnostic(code, message, details))
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn chemical_properties(
        &self,
        known: &Known,
        parameters: PropertiesParameters,
        cancel: &dyn Interrupt,
    ) -> OperationResponse {
        let (budget, limit_ms) = match self.start_time_limit_or_refuse(
            known,
            parameters.time_limit_ms,
            cancel,
            properties::MODEL_ID,
        ) {
            Ok(started) => started,
            Err(reply) => return *reply,
        };
        if (parameters.scope == PropertiesScope::Selection) != parameters.selection.is_some() {
            return known.invalid(diagnostic(
                "invalid_selection",
                "Selection scope requires an explicit selection; other scopes do not accept one.",
                Map::new(),
            ));
        }
        if parameters.property == PropertyKind::Formula && parameters.data_registration_id.is_some()
        {
            return known.invalid(diagnostic(
                "invalid_request",
                "Formula-only requests do not select mass data.",
                Map::new(),
            ));
        }
        let selection = match crate::contract::to_selection(&TransactionSelections {
            before: parameters.selection.clone(),
            after: None,
        }) {
            Ok(value) => value.before,
            Err(error) => return known.invalid(failure(&error)),
        };
        let (snapshot, document) = match self.documents.analysis_snapshot(&parameters.document_id) {
            Ok(snapshot) => snapshot,
            Err(error) => return known.invalid(failure(&error)),
        };
        let revision = snapshot.document.revision;
        let precision = if parameters.property == PropertyKind::Formula {
            0
        } else {
            match self.preferences.current() {
                Ok(preferences) => preferences.mass_precision.decimals,
                Err(error) => return known.invalid(failure(&error)),
            }
        };
        let data = if parameters.property == PropertyKind::Formula {
            None
        } else {
            match properties::select_data(
                &self.data_registry,
                parameters.data_registration_id.as_deref(),
            ) {
                Ok(data) => Some(data),
                Err(error) => {
                    let (unsupported, diagnostic) = property_failure(error);
                    return if unsupported {
                        known.unsupported(diagnostic)
                    } else {
                        known.invalid(diagnostic)
                    };
                }
            }
        };
        // Valence inspection and the calculation share one time limit.
        let (assessments, calculated) = match self.compute_read_only(&budget, |interrupt| {
            let assessments = chemifree_domain::chemistry::inspect_valence(&document);
            let inferred = assessments
                .iter()
                .filter(|state| state.finding == chemifree_domain::chemistry::Finding::Valid)
                .filter_map(|state| state.hydrogen_count.map(|count| (state.atom, count)))
                .collect();
            let calculated = properties::calculate_with(
                &document,
                parameters.scope.domain(),
                selection.as_ref(),
                &inferred,
                parameters.property,
                data.as_ref().map(|(_, table)| table),
                precision,
                interrupt,
            )?;
            Ok((assessments, calculated))
        }) {
            Ok(computed) => computed,
            Err(interrupted) => {
                return stopped_read_only(
                    known,
                    revision,
                    interrupted,
                    limit_ms,
                    properties::MODEL_ID,
                );
            }
        };
        let components = match calculated {
            Ok(result) => result,
            Err(error) => {
                let (unsupported, diagnostic) = property_failure(error);
                return if unsupported {
                    known.unsupported(diagnostic)
                } else {
                    known.invalid(diagnostic)
                };
            }
        };
        let mut losses = Vec::new();
        let included: std::collections::BTreeSet<_> = components
            .iter()
            .flat_map(|component| component.atoms.iter().map(String::as_str))
            .collect();
        let valence_warnings = assessments
            .iter()
            .filter(|state| {
                state.finding != chemifree_domain::chemistry::Finding::Valid
                    && included.contains(state.atom.to_string().as_str())
            })
            .map(|state| PropertyValenceWarning {
                atom: state.atom.to_string(),
                status: state.finding.as_str(),
                reason: state.reason,
            })
            .collect();
        for (index, component) in components.iter().enumerate() {
            if component.mass.as_ref().is_some_and(|mass| {
                !mass.average_omitted_abundances.is_empty()
                    || !mass.exact_omitted_abundances.is_empty()
            }) || !component.omitted_abundances.is_empty()
            {
                losses.push(Loss { feature: "unreported_isotope_abundances".into(), source_path: format!("components[{index}]"), disposition: LossDisposition::Unknown, reason: "The calculation is conditional on reported isotope fractions; missing fractions are listed in the result.".into(), semantic_impact: SemanticImpact::Chemistry, consent: LossConsent::NotRequired });
            }
            if component
                .mass
                .as_ref()
                .is_some_and(|mass| !mass.exact_natural_weight_fallbacks.is_empty())
            {
                losses.push(Loss { feature: "exact_natural_weight_fallback".into(), source_path: format!("components[{index}].mass"), disposition: LossDisposition::Normalized, reason: "The profile used a natural weight because no isotope rows were available; affected elements are listed in the result.".into(), semantic_impact: SemanticImpact::Chemistry, consent: LossConsent::NotRequired });
            }
        }
        let result = PropertiesResult {
            snapshot: snapshot.into(),
            scope: parameters.scope.as_str(),
            selection: parameters.selection,
            property: parameters.property.as_str(),
            valence_model: chemifree_domain::chemistry::VALENCE_MODEL,
            arithmetic: if parameters.property == PropertyKind::Formula {
                "integer-composition"
            } else {
                "binary64-sequential-species-order; fixed display ties-to-even"
            },
            mass_unit: data.as_ref().map(|_| "u"),
            display_decimals: precision,
            data: data.map(|(view, _)| view),
            valence_warnings,
            components,
        };
        let mut response = known.reply(
            if losses.is_empty() {
                Outcome::Ok
            } else {
                Outcome::Partial
            },
            Some(revision),
            Some(OperationResult::Properties(Box::new(result))),
            Vec::new(),
        );
        response.model_id = Some(properties::MODEL_ID.into());
        response.loss_report = losses;
        response
    }
}
