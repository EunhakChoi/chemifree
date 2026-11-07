//! Read-only checked geometry plans for `transform_selection`.

use chemifree_domain::scene::GraphicShape;
use serde::{Deserialize, Serialize};
use serde_json::Map;

use super::{
    DocumentResult, EffectiveSelectionScope, Known, OperationResponse, OperationResult, Outcome,
};
use crate::contract::{
    GraphicShapeDto, PointDto, PositionView, SelectionDto, SelectionTransformDto,
};
use crate::{Application, CapabilityCatalog, DocumentRepository};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TransformParameters {
    document_id: String,
    selection: SelectionDto,
    pivot: PointDto,
    transform: SelectionTransformDto,
    #[serde(default)]
    instance: Option<String>,
}

/// An exact resolved position target, including stationary positions.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TransformedPositionView {
    pub instance: String,
    pub atom: String,
    pub before: PositionView,
    pub after: PositionView,
}

/// An exact graphic target, preserving shape and endpoint/corner identity.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TransformedGraphicView {
    pub object: String,
    pub shape: GraphicShapeDto,
    pub before: Vec<PositionView>,
    pub after: Vec<PositionView>,
}

/// A checked geometry transform and the immutable source snapshot. The advisory is not
/// a stereo assessment or transaction loss; it says only that graph positions actually change.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TransformPlanResult {
    pub snapshot: DocumentResult,
    pub pivot: PositionView,
    pub transform: SelectionTransformDto,
    pub scope: EffectiveSelectionScope,
    pub positions: Vec<TransformedPositionView>,
    pub graphics: Vec<TransformedGraphicView>,
    pub drawing_stereo_may_change: bool,
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn resolve_transform_plan(
        &self,
        known: &Known,
        parameters: TransformParameters,
    ) -> OperationResponse {
        let selection = match crate::contract::exact_selection(&parameters.selection) {
            Ok(selection) => selection,
            Err(message) => {
                let mut details = Map::new();
                details.insert("reason".into(), "selection_unknown".into());
                return known.invalid(super::diagnostic("invalid_request", message, details));
            }
        };
        let instance = match parameters
            .instance
            .as_deref()
            .map(crate::contract::instance_id)
            .transpose()
        {
            Ok(instance) => instance,
            Err(message) => {
                return known.invalid(super::diagnostic("invalid_request", message, Map::new()));
            }
        };
        let pivot = match crate::contract::point(parameters.pivot) {
            Ok(pivot) => pivot,
            Err(message) => {
                return known.invalid(super::diagnostic("invalid_request", message, Map::new()));
            }
        };
        let (snapshot, plan) = match self.documents.transform_plan(
            &parameters.document_id,
            &selection,
            instance,
            pivot,
            parameters.transform.into(),
        ) {
            Ok(captured) => captured,
            Err(error) => {
                return known.reply(
                    Outcome::Error,
                    error.current_revision,
                    None,
                    vec![super::failure(&error)],
                );
            }
        };
        let position = |point: chemifree_domain::document::Point| PositionView {
            x: point.x(),
            y: point.y(),
        };
        let revision = snapshot.document.revision;
        let result = TransformPlanResult {
            snapshot: snapshot.into(),
            pivot: position(pivot),
            transform: parameters.transform,
            scope: EffectiveSelectionScope::from(&plan.scope),
            positions: plan
                .positions
                .into_iter()
                .map(|target| TransformedPositionView {
                    instance: target.key.instance.to_string(),
                    atom: target.key.atom.to_string(),
                    before: position(target.before),
                    after: position(target.after),
                })
                .collect(),
            graphics: plan
                .graphics
                .into_iter()
                .map(|target| TransformedGraphicView {
                    object: target.object.to_string(),
                    shape: match target.before.shape {
                        GraphicShape::Line => GraphicShapeDto::Line,
                        GraphicShape::Rectangle => GraphicShapeDto::Rectangle,
                        GraphicShape::Ellipse => GraphicShapeDto::Ellipse,
                        GraphicShape::Arrow => GraphicShapeDto::Arrow,
                        GraphicShape::Polygon => GraphicShapeDto::Polygon,
                        GraphicShape::Spline => GraphicShapeDto::Spline,
                    },
                    before: target.before.points.iter().copied().map(position).collect(),
                    after: target.after.points.iter().copied().map(position).collect(),
                })
                .collect(),
            drawing_stereo_may_change: plan.drawing_stereo_may_change,
        };
        known.reply(
            Outcome::Ok,
            Some(revision),
            Some(OperationResult::TransformPlan(Box::new(result))),
            Vec::new(),
        )
    }
}
