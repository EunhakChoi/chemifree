//! `chem.substructure`: every mapping of a query drawing in a target drawing.
//!
//! Read-only. Both graphs are copied under one lock, so the reply names the exact snapshots
//! the answer is about; the search then runs outside the lock. A refusal or limit carries both
//! snapshots too and never a partial list.

use chemifree_domain::aromaticity;
use chemifree_domain::substructure::{
    self, Item, MAX_MAPPINGS, MAX_QUERY_ATOMS, MAX_SEARCH_STEPS, Mapping, MatchError,
};

use super::*;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SubstructureParameters {
    query_document_id: String,
    target_document_id: String,
}

/// One row of the table of refusals and limits `chem.substructure` reports.
#[derive(Clone, Copy, Debug)]
pub struct MatchCodeRow {
    pub code: &'static str,
    pub outcome: Outcome,
    pub message: &'static str,
}

/// Every diagnostic code that `chem.substructure` adds to the document errors of the v1
/// envelope, in documentation order. docs/CONTRACT.md lists the same rows.
pub const MATCH_CODES: [MatchCodeRow; 5] = [
    MatchCodeRow {
        code: "empty_query",
        outcome: Outcome::Error,
        message: "The query drawing has no atoms.",
    },
    MatchCodeRow {
        code: "query_too_large",
        outcome: Outcome::Error,
        message: "The query drawing has more atoms than one search accepts.",
    },
    MatchCodeRow {
        code: "query_unsatisfiable",
        outcome: Outcome::Error,
        message: "No atom or bond can satisfy this query atom or bond.",
    },
    MatchCodeRow {
        code: "match_unsupported",
        outcome: Outcome::Unsupported,
        message: "The matcher cannot read this atom or bond, and the answer depends on it.",
    },
    MatchCodeRow {
        code: "match_limit",
        outcome: Outcome::Error,
        message: "The search reached a limit before it could give a complete answer.",
    },
];

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SubstructureResult {
    /// The exact snapshots of the two documents the answer is about. They are the same
    /// document, at the same revision, when a drawing is searched in itself.
    pub query: DocumentResult,
    pub target: DocumentResult,
    pub aromaticity_model: &'static str,
    pub limits: SubstructureLimits,
    /// Candidate assignments the search tried.
    pub steps: usize,
    /// Every mapping, ordered by the target atoms assigned in query-atom order. An empty list
    /// is a complete answer: the query does not occur.
    pub mappings: Vec<MappingView>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SubstructureLimits {
    pub query_atoms: usize,
    pub search_steps: usize,
    pub mappings: usize,
}

const LIMITS: SubstructureLimits = SubstructureLimits {
    query_atoms: MAX_QUERY_ATOMS,
    search_steps: MAX_SEARCH_STEPS,
    mappings: MAX_MAPPINGS,
};

/// One mapping: which target atom and bond each query atom and bond lies on.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MappingView {
    pub atoms: Vec<AtomPair>,
    pub bonds: Vec<BondPair>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AtomPair {
    pub query: String,
    pub target: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BondPair {
    pub query: String,
    pub target: String,
}

fn view(mapping: Mapping) -> MappingView {
    MappingView {
        atoms: mapping
            .atoms
            .into_iter()
            .map(|(query, target)| AtomPair {
                query: query.to_string(),
                target: target.to_string(),
            })
            .collect(),
        bonds: mapping
            .bonds
            .into_iter()
            .map(|(query, target)| BondPair {
                query: query.to_string(),
                target: target.to_string(),
            })
            .collect(),
    }
}

fn item(item: Item) -> (&'static str, String) {
    match item {
        Item::Atom(id) => ("atom", id.to_string()),
        Item::Bond(id) => ("bond", id.to_string()),
    }
}

fn snapshot_json(snapshot: &DocumentResponse) -> Value {
    serde_json::to_value(DocumentResult::from(snapshot.clone()))
        .expect("validated document snapshots serialize to JSON")
}

/// The reply for a refusal or limit. Its details always hold both snapshots, so a client can
/// tell whether the drawings it holds are the ones refused.
fn failure_reply(
    known: &Known,
    revision: u64,
    error: MatchError,
    query: &DocumentResponse,
    target: &DocumentResponse,
) -> OperationResponse {
    let row = MATCH_CODES
        .iter()
        .find(|row| row.code == error.code())
        .expect("every refusal has a row in the code table");
    let mut details = Map::new();
    details.insert("query_snapshot".into(), snapshot_json(query));
    details.insert("target_snapshot".into(), snapshot_json(target));
    match error {
        MatchError::EmptyQuery => {}
        MatchError::QueryTooLarge { atoms } => {
            details.insert("atoms".into(), atoms.into());
            details.insert("limit".into(), MAX_QUERY_ATOMS.into());
        }
        MatchError::Unsatisfiable(unsatisfiable) => {
            let (kind, id) = item(unsatisfiable);
            details.insert("item".into(), kind.into());
            details.insert("id".into(), id.into());
        }
        MatchError::Unsupported {
            side,
            reason,
            at,
            query: compared,
        } => {
            details.insert("side".into(), side.as_str().into());
            details.insert("reason".into(), reason.into());
            if let Some((kind, id)) = at.map(item) {
                details.insert("item".into(), kind.into());
                details.insert("id".into(), id.into());
            }
            if let Some((kind, id)) = compared.map(item) {
                details.insert("query_item".into(), kind.into());
                details.insert("query_id".into(), id.into());
            }
        }
        MatchError::StepLimit => {
            details.insert("limit_kind".into(), "search_steps".into());
            details.insert("limit".into(), MAX_SEARCH_STEPS.into());
        }
        MatchError::MappingLimit => {
            details.insert("limit_kind".into(), "mappings".into());
            details.insert("limit".into(), MAX_MAPPINGS.into());
        }
    }
    let mut response = known.reply(
        row.outcome,
        Some(revision),
        None,
        vec![diagnostic(row.code, row.message, details)],
    );
    response.model_id = Some(substructure::MODEL_ID.into());
    response
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn substructure(
        &self,
        known: &Known,
        p: SubstructureParameters,
    ) -> OperationResponse {
        let ((query_snapshot, query), target) = match self
            .documents
            .graph_pair_snapshot(&p.query_document_id, Some(&p.target_document_id))
        {
            Ok(pair) => pair,
            Err(error) => {
                return known.reply(
                    Outcome::Error,
                    error.current_revision,
                    None,
                    vec![failure(&error)],
                );
            }
        };
        let (target_snapshot, target) = target.expect("a target document was named");
        let revision = query_snapshot.document.revision;
        match substructure::find(&query, &target) {
            Ok(matches) => {
                let result = SubstructureResult {
                    query: query_snapshot.into(),
                    target: target_snapshot.into(),
                    aromaticity_model: aromaticity::MODEL_ID,
                    limits: LIMITS,
                    steps: matches.steps,
                    mappings: matches.mappings.into_iter().map(view).collect(),
                };
                let mut response = known.reply(
                    Outcome::Ok,
                    Some(revision),
                    Some(OperationResult::Substructure(Box::new(result))),
                    Vec::new(),
                );
                response.model_id = Some(substructure::MODEL_ID.into());
                response
            }
            Err(error) => failure_reply(known, revision, error, &query_snapshot, &target_snapshot),
        }
    }
}
