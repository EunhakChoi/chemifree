//! chem.aromaticity: read-only cyclic-system interpretation of one snapshot.

use chemifree_domain::budget::Interrupt;

use crate::{Application, CapabilityCatalog, DocumentRepository};

use super::reply::{
    AromaticityResult, AromaticitySystem, OperationResponse, OperationResult, Outcome,
};
use super::{Known, failure, stopped_read_only};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AromaticityParameters {
    document_id: String,
    /// The time limit this request asks for, in milliseconds: any value up to the server's maximum
    /// (so it may be above the server's default); above the maximum it is refused.
    #[serde(default)]
    time_limit_ms: Option<u64>,
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn aromaticity(
        &self,
        known: &Known,
        p: AromaticityParameters,
        cancel: &dyn Interrupt,
    ) -> OperationResponse {
        let (budget, limit_ms) = match self.start_time_limit_or_refuse(
            known,
            p.time_limit_ms,
            cancel,
            chemifree_domain::aromaticity::MODEL_ID,
        ) {
            Ok(started) => started,
            Err(reply) => return *reply,
        };
        let (snapshot, graph) = match self.documents.graph_snapshot(&p.document_id) {
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
        let revision = snapshot.document.revision;
        let interpretation = match self.compute_read_only(&budget, |interrupt| {
            chemifree_domain::aromaticity::perceive_with(&graph, interrupt)
        }) {
            Ok(interpretation) => interpretation,
            Err(interrupted) => {
                return stopped_read_only(
                    known,
                    revision,
                    interrupted,
                    limit_ms,
                    chemifree_domain::aromaticity::MODEL_ID,
                );
            }
        };
        let systems = interpretation
            .systems
            .into_iter()
            .map(|system| {
                let classification = match system.classification {
                    chemifree_domain::aromaticity::Classification::Aromatic => "aromatic",
                    chemifree_domain::aromaticity::Classification::NonAromatic => "non_aromatic",
                    chemifree_domain::aromaticity::Classification::Unsupported => "unsupported",
                };
                AromaticitySystem {
                    atoms: system.atoms.into_iter().map(|id| id.to_string()).collect(),
                    bonds: system.bonds.into_iter().map(|id| id.to_string()).collect(),
                    cycle_rank: system.cycle_rank,
                    classification,
                    electrons: system.electrons,
                    reason: system.reason,
                }
            })
            .collect();
        let mut reply = known.reply(
            Outcome::Ok,
            Some(revision),
            Some(OperationResult::Aromaticity(Box::new(AromaticityResult {
                snapshot: snapshot.into(),
                systems,
            }))),
            Vec::new(),
        );
        reply.model_id = Some(chemifree_domain::aromaticity::MODEL_ID.into());
        reply
    }
}
