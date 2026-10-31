//! Application-port adapter for the CDXML interchange codec.

use chemifree_application::interchange_codec::{
    InterchangeCodec, InterchangeError, InterchangeFormat, InterchangeRead, InterchangeWrite,
};
use chemifree_application::{Loss, LossDisposition, SemanticImpact};
use chemifree_domain::interchange::Document;

use crate::cdxml::{self, CdxmlDocument, CdxmlError};

#[derive(Clone, Copy, Default)]
pub struct LocalInterchangeCodec;

fn fail(code: &'static str, message: impl Into<String>) -> InterchangeError {
    InterchangeError {
        code,
        message: message.into(),
        line: None,
        column: None,
    }
}

impl From<CdxmlError> for InterchangeError {
    fn from(error: CdxmlError) -> Self {
        Self {
            code: error.code,
            message: error.message,
            line: error.line,
            column: error.column,
        }
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

fn opaque_count(document: &Document) -> Result<usize, InterchangeError> {
    let mut candidate = document.clone();
    candidate
        .discard_opaque()
        .map_err(|error| fail("invalid_document", error.to_string()))
}

impl InterchangeCodec for LocalInterchangeCodec {
    fn read(
        &self,
        format: InterchangeFormat,
        bytes: &[u8],
    ) -> Result<InterchangeRead, InterchangeError> {
        let file = parse(format, bytes)?;
        let count = opaque_count(file.document())?;
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
        if original.is_some() && !discard_opaque && file.document().root() == document.root() {
            let bytes = cdxml::write(&file)?.into_bytes();
            let count = opaque_count(document)?;
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
            return Ok(InterchangeWrite { bytes, loss_report });
        }
        let source_count = opaque_count(file.document())?;
        let submitted_count = opaque_count(document)?;
        let prolog = file.has_unrewritable_prolog();
        if (source_count > 0 || submitted_count > 0 || prolog) && !discard_opaque {
            return Err(fail(
                "unsafe_unknown_rewrite",
                "An edited document contains or originally contained opaque content. Explicitly discard it before rewriting.",
            ));
        }
        let mut submitted = document.clone();
        let mut loss_report = Vec::new();
        if discard_opaque && (source_count > 0 || submitted_count > 0 || prolog) {
            submitted
                .discard_opaque()
                .map_err(|error| fail("invalid_document", error.to_string()))?;
            file.discard_uninterpreted()
                .map_err(|error| fail("invalid_document", error.to_string()))?;
            loss_report.push(opaque_loss(
                LossDisposition::Dropped,
                original.map(|_| source_count),
                Some(submitted_count),
                prolog,
            ));
        }
        file.document_mut()
            .edit(|root| *root = submitted.root().clone())
            .map_err(|error| fail("invalid_document", error.to_string()))?;
        let bytes = cdxml::write(&file)?.into_bytes();
        if bytes.len() > 1024 * 1024 {
            return Err(fail("resource_limit", "CDXML output exceeds 1 MiB."));
        }
        Ok(InterchangeWrite { bytes, loss_report })
    }
}
