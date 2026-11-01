//! Application-port adapter for the CDXML interchange codec.

use chemifree_application::interchange_codec::{
    InterchangeCodec, InterchangeError, InterchangeFormat, InterchangeRead, InterchangeWrite,
};
use chemifree_application::{Loss, LossDisposition, SemanticImpact};
use chemifree_domain::interchange::{Content, Document, Object, ObjectKind, PropertyKind};

use crate::cdx;
use crate::cdxml::{self, CdxmlDocument, CdxmlError};

#[derive(Clone, Copy, Default)]
pub struct LocalInterchangeCodec;

fn fail(code: &'static str, message: impl Into<String>) -> InterchangeError {
    InterchangeError {
        code,
        message: message.into(),
        line: None,
        column: None,
        byte_offset: None,
    }
}

impl From<CdxmlError> for InterchangeError {
    fn from(error: CdxmlError) -> Self {
        Self {
            code: error.code,
            message: error.message,
            line: error.line,
            column: error.column,
            byte_offset: None,
        }
    }
}

impl From<cdx::CdxError> for InterchangeError {
    fn from(error: cdx::CdxError) -> Self {
        let message = match error.offset {
            Some(offset) => format!("{} (byte offset {offset})", error.message),
            None => error.message,
        };
        Self {
            code: error.code,
            message,
            line: None,
            column: None,
            byte_offset: error.offset,
        }
    }
}

fn cdx_platform_loss() -> Loss {
    Loss {
        feature: "font_platform".into(),
        source_path: "document.fonttable".into(),
        disposition: LossDisposition::Normalized,
        reason: "The CDX font platform is not represented in the common tree; this writer records platform 1 unless an unchanged original retains its bytes.".into(),
        semantic_impact: SemanticImpact::Rendering,
    }
}

fn xml_notes_loss(count: usize) -> Loss {
    Loss {
        feature: "xml_comment".into(),
        source_path: "document".into(),
        disposition: LossDisposition::Dropped,
        reason: format!(
            "{count} XML comments or processing instructions cannot be held in CDX and were dropped; they never carried drawing semantics."
        ),
        semantic_impact: SemanticImpact::None,
    }
}

fn color_precision_loss(channels: usize) -> Loss {
    Loss {
        feature: "color_precision".into(),
        source_path: "document.colortable".into(),
        disposition: LossDisposition::Normalized,
        reason: format!(
            "{channels} color channels were not multiples of 1/65535 and were rounded to the nearest 16-bit CDX step."
        ),
        semantic_impact: SemanticImpact::Rendering,
    }
}

fn parse(format: InterchangeFormat, bytes: &[u8]) -> Result<CdxmlDocument, InterchangeError> {
    if format != InterchangeFormat::Cdxml {
        return Err(fail(
            "unsupported_format",
            "Only CDXML is available in this codec.",
        ));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| fail("invalid_utf8", "CDXML input must be UTF-8."))?;
    cdxml::read(text).map_err(Into::into)
}

fn opaque_loss(
    disposition: LossDisposition,
    original: Option<usize>,
    submitted: Option<usize>,
    prolog: bool,
) -> Loss {
    let count = |value: Option<usize>| {
        value.map_or_else(|| "not supplied".into(), |value| value.to_string())
    };
    Loss {
        feature: "opaque_extensions".into(),
        source_path: "document".into(),
        disposition,
        reason: format!(
            "Original opaque entries: {}; submitted opaque entries: {}; each subtree counts once and the sets may overlap. Outside-root material: {prolog}. Unknown references cannot be verified or safely rewritten.",
            count(original),
            count(submitted)
        ),
        semantic_impact: SemanticImpact::Unknown,
    }
}

fn opaque_count(object: &Object) -> usize {
    let properties = object
        .properties
        .iter()
        .filter(|property| {
            matches!(
                property.kind,
                PropertyKind::UnknownXml(_) | PropertyKind::UnknownBinary(_)
            )
        })
        .count();
    properties
        + object
            .children
            .iter()
            .map(|child| match child {
                Content::Object(child)
                    if matches!(
                        child.kind,
                        ObjectKind::UnknownXml(_) | ObjectKind::UnknownBinary(_)
                    ) =>
                {
                    1
                }
                Content::Object(child) => opaque_count(child),
                _ => 0,
            })
            .sum::<usize>()
}

/// What the opaque-content policy decides for one write, whatever the format.
enum OpaquePlan {
    /// The submitted tree is the original's: the original representation is kept.
    Unchanged(Option<Loss>),
    /// The document is reserialized; `discard` is the loss to report when opaque content
    /// (or unrewritable material outside the root) is explicitly discarded first.
    Rewrite { discard: Option<Loss> },
}

/// Opaque data has unknown references, so it is never silently rewritten or dropped.
/// `baseline` is the root decoded from the supplied original, if any; `prolog` marks
/// original material outside the root that cannot be rewritten.
fn plan_opaque(
    baseline: Option<&Object>,
    prolog: bool,
    document: &Document,
    discard_opaque: bool,
) -> Result<OpaquePlan, InterchangeError> {
    let submitted = opaque_count(document.root());
    if !discard_opaque && baseline.is_some_and(|root| root == document.root()) {
        let retained = (submitted > 0 || prolog)
            .then(|| opaque_loss(LossDisposition::Retained, Some(submitted), None, prolog));
        return Ok(OpaquePlan::Unchanged(retained));
    }
    // Without an original, the document is its own source.
    let source = baseline.map_or(submitted, opaque_count);
    if source == 0 && submitted == 0 && !prolog {
        return Ok(OpaquePlan::Rewrite { discard: None });
    }
    if !discard_opaque {
        return Err(fail(
            "unsafe_unknown_rewrite",
            "An edited document contains or originally contained opaque content whose references are unknown. Explicitly discard it before rewriting.",
        ));
    }
    Ok(OpaquePlan::Rewrite {
        discard: Some(opaque_loss(
            LossDisposition::Dropped,
            baseline.map(|_| source),
            Some(submitted),
            prolog,
        )),
    })
}

fn discarded(document: &Document) -> Result<Document, InterchangeError> {
    let mut clean = document.clone();
    clean.discard_opaque().map_err(|error| {
        fail(
            "invalid_document",
            format!("Discarding opaque content would leave an invalid document: {error}"),
        )
    })?;
    Ok(clean)
}

impl InterchangeCodec for LocalInterchangeCodec {
    fn read(
        &self,
        format: InterchangeFormat,
        bytes: &[u8],
    ) -> Result<InterchangeRead, InterchangeError> {
        if format == InterchangeFormat::Cdx {
            let decoded = cdx::read(bytes)?;
            let document = decoded.document;
            let count = opaque_count(document.root());
            let mut losses = Vec::new();
            if count > 0 {
                losses.push(opaque_loss(
                    LossDisposition::Retained,
                    Some(count),
                    None,
                    false,
                ));
            }
            if decoded.font_platform.is_some() {
                losses.push(cdx_platform_loss());
            }
            return Ok(InterchangeRead {
                document,
                loss_report: losses,
                opaque_outside_root: false,
            });
        }
        let file = parse(format, bytes)?;
        let count = opaque_count(file.document().root());
        let prolog = file.has_unrewritable_prolog();
        let loss_report = if count > 0 || prolog {
            vec![opaque_loss(
                LossDisposition::Retained,
                Some(count),
                None,
                prolog,
            )]
        } else {
            Vec::new()
        };
        Ok(InterchangeRead {
            document: file.document().clone(),
            loss_report,
            opaque_outside_root: prolog,
        })
    }

    fn write(
        &self,
        format: InterchangeFormat,
        document: &Document,
        original: Option<&[u8]>,
        discard_opaque: bool,
    ) -> Result<InterchangeWrite, InterchangeError> {
        if format == InterchangeFormat::Cdx {
            let original_read = original.map(cdx::read).transpose()?;
            let plan = plan_opaque(
                original_read.as_ref().map(|read| read.document.root()),
                false,
                document,
                discard_opaque,
            )?;
            let mut losses = Vec::new();
            let mut clean = match plan {
                OpaquePlan::Unchanged(retained) => {
                    return Ok(InterchangeWrite {
                        bytes: original
                            .expect("an unchanged tree has an original")
                            .to_vec(),
                        loss_report: retained.into_iter().collect(),
                    });
                }
                OpaquePlan::Rewrite {
                    discard: Some(loss),
                } => {
                    losses.push(loss);
                    discarded(document)?
                }
                OpaquePlan::Rewrite { discard: None } => document.clone(),
            };
            if let Some((without_notes, notes)) = cdx::without_xml_notes(&clean)? {
                if !discard_opaque {
                    return Err(fail(
                        "comment_discard_required",
                        "CDX cannot hold XML comments or processing instructions; request discard_opaque to drop them and report the loss.",
                    ));
                }
                clean = without_notes;
                losses.push(xml_notes_loss(notes));
            }
            let written = cdx::write(&clean)?;
            // The writer records font platform 1; only an original known to use it avoids a change.
            let source_platform = original_read.as_ref().and_then(|read| read.font_platform);
            if cdx::has_font_table(&clean) && source_platform != Some(1) {
                losses.push(cdx_platform_loss());
            }
            if written.rounded_color_channels > 0 {
                losses.push(color_precision_loss(written.rounded_color_channels));
            }
            return Ok(InterchangeWrite {
                bytes: written.bytes,
                loss_report: losses,
            });
        }
        if format != InterchangeFormat::Cdxml {
            return Err(fail(
                "unsupported_format",
                "Only CDXML is available in this codec.",
            ));
        }
        document
            .validate()
            .map_err(|error| fail("invalid_document", error.to_string()))?;
        let mut file = match original {
            Some(bytes) => parse(format, bytes)?,
            None => CdxmlDocument::from_document(document.clone()),
        };
        // If the original really decodes to this exact tree, the original bytes retain
        // even extensions whose reference semantics are unknown.
        let plan = plan_opaque(
            original.map(|_| file.document().root()),
            file.has_unrewritable_prolog(),
            document,
            discard_opaque,
        )?;
        let mut loss_report = Vec::new();
        match plan {
            OpaquePlan::Unchanged(retained) => {
                let bytes = cdxml::write(&file)?.into_bytes();
                return Ok(InterchangeWrite {
                    bytes,
                    loss_report: retained.into_iter().collect(),
                });
            }
            OpaquePlan::Rewrite {
                discard: Some(loss),
            } => {
                file.replace_after_discard(discarded(document)?);
                loss_report.push(loss);
            }
            OpaquePlan::Rewrite { discard: None } => {
                file.document_mut()
                    .edit(|root| *root = document.root().clone())
                    .map_err(|error| fail("invalid_document", error.to_string()))?;
            }
        }
        let bytes = cdxml::write(&file)?.into_bytes();
        if bytes.len() > 1024 * 1024 {
            return Err(fail("resource_limit", "CDXML output exceeds 1 MiB."));
        }
        Ok(InterchangeWrite { bytes, loss_report })
    }
}
