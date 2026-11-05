//! Read-only resolution of the exact union targeted by `move_selection`.

use serde::Deserialize;

use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SelectionScopeParameters {
    document_id: String,
    selection: SelectionDto,
    #[serde(default)]
    instance: Option<String>,
}

/// One geometry target, ordered by scene instance then atom identifier.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ScopePosition {
    pub instance: String,
    pub atom: String,
}

/// Unique movement targets in domain identifier order. Objects include selected groups and
/// descendants; positions and graphics are the actual translated geometry targets.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EffectiveSelectionScope {
    pub objects: Vec<String>,
    pub positions: Vec<ScopePosition>,
    pub graphics: Vec<String>,
}

/// The scope and the exact immutable view used to resolve it. No history or selection edit.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SelectionScopeResult {
    pub snapshot: DocumentResult,
    pub effective: EffectiveSelectionScope,
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn resolve_selection_scope(
        &self,
        known: &Known,
        parameters: SelectionScopeParameters,
    ) -> OperationResponse {
        let selection = match crate::contract::exact_selection(&parameters.selection) {
            Ok(selection) => selection,
            Err(message) => {
                let mut details = Map::new();
                details.insert("reason".into(), "selection_unknown".into());
                return known.invalid(diagnostic("invalid_request", message, details));
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
                return known.invalid(diagnostic("invalid_request", message, Map::new()));
            }
        };
        let (snapshot, scope) =
            match self
                .documents
                .selection_scope(&parameters.document_id, &selection, instance)
            {
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
        let effective = EffectiveSelectionScope {
            objects: scope.objects.iter().map(ToString::to_string).collect(),
            positions: scope
                .positions
                .iter()
                .map(|key| ScopePosition {
                    instance: key.instance.to_string(),
                    atom: key.atom.to_string(),
                })
                .collect(),
            graphics: scope.graphics.iter().map(ToString::to_string).collect(),
        };
        known.reply(
            Outcome::Ok,
            Some(revision),
            Some(OperationResult::SelectionScope(Box::new(
                SelectionScopeResult {
                    snapshot: snapshot.into(),
                    effective,
                },
            ))),
            Vec::new(),
        )
    }
}
