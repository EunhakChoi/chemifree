//! `chem.substructure`: every mapping of a query drawing in a target drawing.
//!
//! Read-only. Both graphs are copied under one lock, so the reply names the exact snapshots
//! the answer is about; the search then runs outside the lock. A refusal or search limit carries
//! both snapshots too and never a partial list. The search runs under the request's time limit
//! and the host cancel; a stop reports `time_limit` or `cancelled` with no snapshots, not a row
//! of the code table.

use chemifree_domain::aromaticity;
use chemifree_domain::substructure::{
    self, Item, MAX_MAPPINGS, MAX_QUERY_ATOMS, MAX_SCANNED_CANDIDATES, MAX_SEARCH_STEPS, Mapping,
    MatchError,
};

use super::*;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SubstructureParameters {
    query_document_id: String,
    target_document_id: String,
    /// The time limit this request asks for, in milliseconds: any value up to the server's maximum
    /// (so it may be above the server's default); above the maximum it is refused.
    #[serde(default)]
    time_limit_ms: Option<u64>,
}

/// One row of the table of refusals and limits `chem.substructure` reports.
#[derive(Clone, Copy, Debug)]
pub struct MatchCodeRow {
    pub code: &'static str,
    pub outcome: Outcome,
    pub message: &'static str,
}

/// Every refusal and search-limit code that `chem.substructure` adds to the document errors of the
/// v1 envelope, in documentation order. The shared `time_limit` and `cancelled` stops are not
/// rows here. docs/CONTRACT.md lists the same rows.
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
    /// What this model does with stereo. Added within v1.
    pub stereo: SubstructureStereo,
}

/// Stereo and `chem.substructure`. `compared` is a property of the model, not of the pair: it
/// is `false` in every reply because this model never reads stereo, whether or not the drawings
/// hold any. The `*_present` fields are facts about the two documents.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SubstructureStereo {
    pub compared: bool,
    pub query_present: bool,
    pub target_present: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SubstructureLimits {
    pub query_atoms: usize,
    pub search_steps: usize,
    pub scanned_candidates: usize,
    pub mappings: usize,
}

const LIMITS: SubstructureLimits = SubstructureLimits {
    query_atoms: MAX_QUERY_ATOMS,
    search_steps: MAX_SEARCH_STEPS,
    scanned_candidates: MAX_SCANNED_CANDIDATES,
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

fn snapshot_json(snapshot: DocumentResponse) -> Value {
    serde_json::to_value(DocumentResult::from(snapshot))
        .expect("validated document snapshots serialize to JSON")
}

/// The reply for a refusal or limit. Its details always hold both snapshots, so a client can
/// tell whether the drawings it holds are the ones refused.
fn failure_reply(
    known: &Known,
    revision: u64,
    error: MatchError,
    query: DocumentResponse,
    target: DocumentResponse,
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
        MatchError::ScanLimit => {
            details.insert("limit_kind".into(), "scanned_candidates".into());
            details.insert("limit".into(), MAX_SCANNED_CANDIDATES.into());
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

/// The reply for a document that could not be read. No snapshot was taken, so it names no
/// `document_revision`: `error.current_revision` would be that of whichever document failed,
/// possibly the target, while every other reply of this operation names the query snapshot's
/// revision.
fn lookup_failure_reply(known: &Known, error: &ApplicationError) -> OperationResponse {
    known.reply(Outcome::Error, None, None, vec![failure(error)])
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn substructure(
        &self,
        known: &Known,
        p: SubstructureParameters,
        cancel: &dyn Interrupt,
    ) -> OperationResponse {
        let (budget, limit_ms) = match self.start_time_limit(p.time_limit_ms, cancel) {
            Ok(started) => started,
            Err(refusal) => {
                let mut reply = known.invalid(refusal);
                reply.model_id = Some(substructure::MODEL_ID.into());
                return reply;
            }
        };
        let ((query_snapshot, query), (target_snapshot, target)) = match self
            .documents
            .graph_pair(&p.query_document_id, &p.target_document_id)
        {
            Ok(pair) => pair,
            Err(error) => return lookup_failure_reply(known, &error),
        };
        let revision = query_snapshot.document.revision;
        // The search takes a compute slot under the request's budget. A stop replies with the
        // time limit or the cancel, and with neither a result nor snapshots.
        let search = match self.compute_read_only(&budget, |interrupt| {
            substructure::find_with(&query, &target, interrupt)
        }) {
            Ok(search) => search,
            Err(interrupted) => {
                return stopped_read_only(
                    known,
                    revision,
                    interrupted,
                    limit_ms,
                    substructure::MODEL_ID,
                );
            }
        };
        match search {
            Ok(matches) => {
                let stereo = SubstructureStereo {
                    compared: false,
                    query_present: query.has_stereo(),
                    target_present: target.has_stereo(),
                };
                let mut losses = Vec::new();
                if stereo.query_present {
                    losses.push(stereo_not_read("query"));
                }
                if stereo.target_present {
                    losses.push(stereo_not_read("target"));
                }
                let result = SubstructureResult {
                    query: query_snapshot.into(),
                    target: target_snapshot.into(),
                    aromaticity_model: aromaticity::MODEL_ID,
                    limits: LIMITS,
                    steps: matches.steps,
                    mappings: matches.mappings.into_iter().map(view).collect(),
                    stereo,
                };
                let mut response = converted(
                    known,
                    Some(revision),
                    OperationResult::Substructure(Box::new(result)),
                    losses,
                );
                response.model_id = Some(substructure::MODEL_ID.into());
                response
            }
            Err(error) => failure_reply(known, revision, error, query_snapshot, target_snapshot),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lookup_failure_names_no_revision_even_when_the_error_carries_one() {
        let known = Known {
            request_id: "lookup".into(),
            operation: OperationId::ChemSubstructure,
            profile: CompatibilityProfile::PreserveSemantics,
        };
        // An error of the second document, with that document's revision.
        let error = ApplicationError {
            current_revision: Some(7),
            ..ApplicationError::new(ErrorCode::DocumentNotFound, "document d2 does not exist")
        };
        let reply = lookup_failure_reply(&known, &error);
        assert_eq!(reply.document_revision, None);
        assert_eq!(reply.outcome, Outcome::Error);
        assert!(reply.result.is_none());
    }
}
