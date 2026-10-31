//! Application-owned port for loss-aware interchange readers and writers.

use std::fmt;

use chemifree_domain::interchange::Document;

use crate::Loss;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterchangeFormat {
    Cdxml,
    Cdx,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InterchangeRead {
    pub document: Document,
    pub loss_report: Vec<Loss>,
    /// Outside-root comments or processing instructions require original bytes to preserve.
    pub opaque_outside_root: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InterchangeWrite {
    pub bytes: Vec<u8>,
    pub loss_report: Vec<Loss>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterchangeError {
    pub code: &'static str,
    pub message: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

impl fmt::Display for InterchangeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}
impl std::error::Error for InterchangeError {}

/// Formats are bounded independently by the provider and by the request transport.
/// An original must be parsed and checked against the submitted tree; it is never a
/// client-authoritative reference baseline. Unknown content may only be removed when
/// `discard_opaque` is true, and every such removal is reported.
pub trait InterchangeCodec: Send + Sync {
    fn read(
        &self,
        format: InterchangeFormat,
        bytes: &[u8],
    ) -> Result<InterchangeRead, InterchangeError>;

    fn write(
        &self,
        format: InterchangeFormat,
        document: &Document,
        original: Option<&[u8]>,
        discard_opaque: bool,
    ) -> Result<InterchangeWrite, InterchangeError>;
}
