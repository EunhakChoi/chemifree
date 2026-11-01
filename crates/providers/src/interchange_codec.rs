//! Application-port adapter for the CDXML and CDX interchange codecs.

use chemifree_application::interchange_codec::{
    InterchangeCodec, InterchangeError, InterchangeFormat, InterchangeRead, InterchangeWrite,
};
use chemifree_domain::interchange::Document;

use crate::cdx::{self, CdxError};
use crate::cdxml::{self, CdxmlDocument, CdxmlError};
use crate::opaque_policy::{self, ParsedOriginal, Plan};

#[derive(Clone, Copy, Default)]
pub struct LocalInterchangeCodec;

fn fail(code: &'static str, message: impl Into<String>) -> InterchangeError {
    InterchangeError {
        code,
        message: message.into(),
        line: None,
        column: None,
        offset: None,
    }
}

impl From<CdxmlError> for InterchangeError {
    fn from(error: CdxmlError) -> Self {
        Self {
            code: error.code,
            message: error.message,
            line: error.line,
            column: error.column,
            offset: None,
        }
    }
}

impl From<CdxError> for InterchangeError {
    fn from(error: CdxError) -> Self {
        Self {
            code: error.code,
            message: error.message,
            line: None,
            column: None,
            offset: error.offset,
        }
    }
}

fn parse_cdxml(bytes: &[u8]) -> Result<CdxmlDocument, InterchangeError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| fail("invalid_utf8", "CDXML input must be UTF-8."))?;
    cdxml::read(text).map_err(Into::into)
}

impl LocalInterchangeCodec {
    fn write_cdxml(
        document: &Document,
        original: Option<&[u8]>,
        discard_opaque: bool,
    ) -> Result<InterchangeWrite, InterchangeError> {
        document
            .validate()
            .map_err(|error| fail("invalid_document", error.to_string()))?;
        let mut file = match original {
            Some(bytes) => parse_cdxml(bytes)?,
            None => CdxmlDocument::from_document(document.clone()),
        };
        let prolog = file.has_unrewritable_prolog();
        let parsed = original.map(|_| ParsedOriginal {
            document: file.document(),
            unrewritable_outside: prolog,
        });
        match opaque_policy::plan(parsed, document, discard_opaque)? {
            Plan::KeepOriginal { losses } => Ok(InterchangeWrite {
                bytes: cdxml::write(&file)?.into_bytes(),
                loss_report: losses,
            }),
            Plan::Rewrite { discarded, losses } => {
                match discarded {
                    Some(submitted) => file.replace_after_discard(submitted),
                    None => file
                        .document_mut()
                        .edit(|root| *root = document.root().clone())
                        .map_err(|error| fail("invalid_document", error.to_string()))?,
                }
                let bytes = cdxml::write(&file)?.into_bytes();
                if bytes.len() > cdxml::MAX_XML_BYTES {
                    return Err(fail("resource_limit", "CDXML output exceeds 1 MiB."));
                }
                Ok(InterchangeWrite {
                    bytes,
                    loss_report: losses,
                })
            }
        }
    }

    fn write_cdx(
        document: &Document,
        original: Option<&[u8]>,
        discard_opaque: bool,
    ) -> Result<InterchangeWrite, InterchangeError> {
        document
            .validate()
            .map_err(|error| fail(error.code(), error.message()))?;
        // A fault in the caller's original is not a fault in the submitted document.
        let parsed_original = original
            .map(|bytes| {
                cdx::read(bytes).map_err(|error| InterchangeError {
                    code: "invalid_original",
                    message: format!("The original CDX source is invalid: {}", error.message),
                    line: None,
                    column: None,
                    offset: error.offset,
                })
            })
            .transpose()?;
        let parsed = parsed_original.as_ref().map(|file| ParsedOriginal {
            document: file.document(),
            unrewritable_outside: false,
        });
        match opaque_policy::plan(parsed, document, discard_opaque)? {
            Plan::KeepOriginal { losses } => Ok(InterchangeWrite {
                bytes: original.unwrap_or_default().to_vec(),
                loss_report: losses,
            }),
            Plan::Rewrite {
                discarded,
                mut losses,
            } => {
                let target = discarded.as_ref().unwrap_or(document);
                let written = cdx::write(target, parsed_original.as_ref())?;
                losses.extend(written.losses);
                Ok(InterchangeWrite {
                    bytes: written.bytes,
                    loss_report: losses,
                })
            }
        }
    }
}

impl InterchangeCodec for LocalInterchangeCodec {
    fn binary_input_limit(&self) -> usize {
        cdx::MAX_BYTES
    }

    fn read(
        &self,
        format: InterchangeFormat,
        bytes: &[u8],
    ) -> Result<InterchangeRead, InterchangeError> {
        match format {
            InterchangeFormat::Cdxml => {
                let file = parse_cdxml(bytes)?;
                let count = opaque_policy::opaque_count(file.document().root());
                let prolog = file.has_unrewritable_prolog();
                Ok(InterchangeRead {
                    document: file.document().clone(),
                    loss_report: opaque_policy::retained_losses(count, prolog),
                    opaque_outside_root: prolog,
                })
            }
            InterchangeFormat::Cdx => {
                let file = cdx::read(bytes)?;
                let count = opaque_policy::opaque_count(file.document().root());
                Ok(InterchangeRead {
                    document: file.document().clone(),
                    loss_report: opaque_policy::retained_losses(count, false),
                    opaque_outside_root: false,
                })
            }
        }
    }

    fn write(
        &self,
        format: InterchangeFormat,
        document: &Document,
        original: Option<&[u8]>,
        discard_opaque: bool,
    ) -> Result<InterchangeWrite, InterchangeError> {
        match format {
            InterchangeFormat::Cdxml => Self::write_cdxml(document, original, discard_opaque),
            InterchangeFormat::Cdx => Self::write_cdx(document, original, discard_opaque),
        }
    }
}
