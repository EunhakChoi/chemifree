//! Checked read-only plans for the same rigid arrangement used by `arrange_objects`.

use chemifree_domain::scene::DistributionAxis;
use serde::Deserialize;

use super::*;
use crate::contract::{ArrangementModeDto, PositionView};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ArrangementParameters {
    document_id: String,
    objects: Vec<String>,
    mode: ArrangementModeDto,
}

/// Original geometry-only bounds; rendered labels and stroke extents are excluded.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ArrangementBounds {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

/// Coordinate axis affected by a rigid arrangement translation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArrangementAxis {
    Horizontal,
    Vertical,
}

/// Materializes a coordinate as the exact sum `coordinate + target - source`,
/// rounded once to binary64 with ties to even. The other axis stays unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ArrangementTranslationView {
    pub axis: ArrangementAxis,
    pub source: f64,
    pub target: f64,
}

/// One outermost selected unit, including stationary units, in the domain planner's order.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ArrangementUnitView {
    pub object: String,
    pub bounds: ArrangementBounds,
    /// Nominal displacement for reporting; use `translation` to reconstruct geometry.
    pub delta: PositionView,
    /// None leaves every coordinate untouched, including stationary endpoints.
    pub translation: Option<ArrangementTranslationView>,
    pub scope: EffectiveSelectionScope,
}

/// A checked arrangement and its exact snapshot. Previewing changes no selection or history.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ArrangementPlanResult {
    pub snapshot: DocumentResult,
    pub mode: ArrangementModeDto,
    pub units: Vec<ArrangementUnitView>,
    pub scope: EffectiveSelectionScope,
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn resolve_arrangement_plan(
        &self,
        known: &Known,
        parameters: ArrangementParameters,
    ) -> OperationResponse {
        let objects = match parameters
            .objects
            .iter()
            .map(|object| crate::contract::instance_id(object))
            .collect::<Result<_, _>>()
        {
            Ok(objects) => objects,
            Err(message) => {
                return known.invalid(diagnostic("invalid_request", message, Map::new()));
            }
        };
        let (snapshot, plan) = match self.documents.arrangement_plan(
            &parameters.document_id,
            &objects,
            parameters.mode.into(),
        ) {
            Ok(captured) => captured,
            Err(error) => {
                return known.reply(
                    Outcome::Error,
                    error.current_revision,
                    None,
                    vec![failure(&error)],
                );
            }
        };
        let revision = snapshot.document.revision;
        let scope = EffectiveSelectionScope::from(&plan.scope);
        let units = plan
            .units
            .into_iter()
            .map(|unit| ArrangementUnitView {
                object: unit.object.to_string(),
                bounds: ArrangementBounds {
                    left: unit.bounds.left,
                    top: unit.bounds.top,
                    right: unit.bounds.right,
                    bottom: unit.bounds.bottom,
                },
                delta: PositionView {
                    x: unit.delta.dx,
                    y: unit.delta.dy,
                },
                translation: unit
                    .translation
                    .map(|translation| ArrangementTranslationView {
                        axis: match translation.axis {
                            DistributionAxis::Horizontal => ArrangementAxis::Horizontal,
                            DistributionAxis::Vertical => ArrangementAxis::Vertical,
                        },
                        source: translation.source,
                        target: translation.target,
                    }),
                scope: EffectiveSelectionScope::from(&unit.scope),
            })
            .collect();
        known.reply(
            Outcome::Ok,
            Some(revision),
            Some(OperationResult::ArrangementPlan(Box::new(
                ArrangementPlanResult {
                    snapshot: snapshot.into(),
                    mode: parameters.mode,
                    units,
                    scope,
                },
            ))),
            Vec::new(),
        )
    }
}
