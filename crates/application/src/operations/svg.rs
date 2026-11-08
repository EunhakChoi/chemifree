//! The bounded SVG drawing export (P38a) as `document.export` with `format: "svg"`. It reads one
//! document and changes neither the document nor its history. SVG is export only: it cannot be
//! imported.
//!
//! The export draws atoms, bonds, scene graphics and PNG images. Captions and other scene content
//! are reported as losses.

use serde::Serialize;

use super::interchange::export_consent_refusal;
use super::{
    DocumentResult, Known, OperationResponse, OperationResult, Outcome, converted, failure,
};
use crate::svg::{self, ADVANCE_TABLE_ID, SVG_MODEL, ViewBox};
use crate::{Application, CapabilityCatalog, DocumentRepository};

/// The `document.export` parameters for `format: "svg"`.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SvgExportParameters {
    format: String,
    document_id: String,
    /// The `loss_digest` of the refusal that showed what the drawing does not draw.
    #[serde(default)]
    accepted_losses: Option<String>,
}

/// A bounded SVG drawing of the document's atoms, bonds, scene graphics and PNG images, with the
/// working copy it was written from. Captions and other scene content are reported as losses.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SvgExportResult {
    pub content: String,
    /// Always `image/svg+xml`.
    pub media_type: &'static str,
    /// The drawing's `viewBox`, in pt.
    pub view_box: ViewBox,
    /// The label advance table that the drawing's label widths were measured with.
    pub font_table: &'static str,
    /// The working copy the drawing was written from.
    pub snapshot: DocumentResult,
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    /// Writes the drawing from the document view taken under the session lock, so the lock is
    /// not held while the drawing is written. The losses the drawing leaves out are checked
    /// for consent before the content is returned.
    pub(super) fn svg_export(&self, known: &Known, p: SvgExportParameters) -> OperationResponse {
        debug_assert_eq!(p.format, "svg", "the dispatcher routes only SVG here");
        let snapshot = match self.documents.get(&p.document_id) {
            Ok(response) => response,
            Err(error) => {
                return known.reply(
                    Outcome::Error,
                    error.current_revision,
                    None,
                    vec![failure(&error)],
                );
            }
        };
        let drawing = svg::render_drawing(&snapshot.document);
        let losses = svg::svg_losses(&snapshot.document);
        if let Some(refusal) = export_consent_refusal(
            &losses,
            drawing.content.as_bytes(),
            p.accepted_losses.as_deref(),
        ) {
            return known.invalid(refusal);
        }
        let revision = snapshot.document.revision;
        let result = SvgExportResult {
            content: drawing.content,
            media_type: "image/svg+xml",
            view_box: drawing.view_box,
            font_table: ADVANCE_TABLE_ID,
            snapshot: snapshot.into(),
        };
        let mut response = converted(
            known,
            Some(revision),
            OperationResult::SvgExport(Box::new(result)),
            losses,
        );
        response.model_id = Some(SVG_MODEL.into());
        response
    }
}
