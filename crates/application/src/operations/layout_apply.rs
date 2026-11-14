//! `layout.structure` in mode `"apply"`: the chain layout committed as one undoable edit.
//!
//! The request is read as for a preview (see `layout`). The session layer
//! (`Documents::apply_layout`) answers a replay, checks the hold and `expected_revision` and copies
//! the document under one lock. Here the layout is prepared from that copy with no lock held, then
//! placed in a compute slot under the request's time limit and cancel, and its `MoveAtoms` commands
//! are handed back to be committed after the revision is checked again. The reply is the ordinary
//! document reply with the layout's diagnostics; it holds none of the preview's result
//! fields. Every reply names the layout model, and a stop names no revision.

use chemifree_domain::document::Document;
use chemifree_domain::layout::{self, MODEL_ID, Preparation};

use super::layout::{
    LayoutParameters, LayoutRequest, placement_diagnostics, refusal_reply, resolve_instance,
};
use super::*;
use crate::contract::TransactionSelections;
use crate::documents::{Compute, LayoutApply, LayoutEdit, LayoutHalt};

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    /// Applies the layout that `p` and `request` ask for to the document at `expected_revision`
    /// as one undoable edit, and returns the complete reply. A refusal of the layout carries the
    /// copied document's revision, as in a preview; a refusal of the session layer and a stop
    /// are the ordinary document error replies.
    pub(super) fn layout_apply(
        &self,
        known: &Known,
        p: LayoutParameters,
        request: LayoutRequest,
        expected_revision: u64,
        budget: &Budget<'_>,
        limit_ms: u64,
    ) -> OperationResponse {
        let compute = Compute {
            interrupt: budget,
            limit_ms,
            requested_ms: p.time_limit_ms,
            gate: &self.compute_gate,
        };
        let LayoutRequest {
            scope,
            requested,
            selection,
        } = request;
        let apply = LayoutApply {
            expected_revision,
            selection: p.selection,
            scope: p.scope,
            instance: p.instance,
            selections: TransactionSelections {
                before: p.selection_before,
                after: p.selection_after,
            },
        };
        let outcome = self.documents.apply_layout(
            &p.document_id,
            Some(known.request_id.as_str()),
            apply,
            compute,
            |document: &Document| {
                let instance = resolve_instance(known, requested, document)?;
                layout::prepare(document, instance, &selection, scope)
                    .map_err(|error| refusal_reply(known, document.revision(), &error))
            },
            |prep: Preparation, interrupt: &dyn Interrupt| {
                let preview = match layout::place_with(&prep, interrupt)? {
                    Ok(preview) => preview,
                    Err(error) => return Ok(Err(refusal_reply(known, expected_revision, &error))),
                };
                let commands = layout::move_commands(&preview, prep.instance);
                let diagnostics = placement_diagnostics(&prep, &preview, commands.is_empty());
                Ok(Ok(LayoutEdit {
                    commands,
                    diagnostics,
                }))
            },
        );
        let mut reply = match outcome {
            Ok(mut response) => {
                // Recorded with the reply, so a replay returns them as the first reply did.
                let diagnostics = std::mem::take(&mut response.diagnostics);
                let mut reply = known.document(Ok(response));
                reply.diagnostics.extend(diagnostics);
                reply
            }
            Err(LayoutHalt::Failed(error)) => known.document(Err(error)),
            Err(LayoutHalt::Refused(reply)) => *reply,
        };
        reply.model_id = Some(MODEL_ID.into());
        reply
    }
}
