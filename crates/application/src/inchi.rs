//! Optional Standard InChI provider boundary. A provider reports only chemistry it can map
//! without guessing; its registered build and effective options accompany every result.

use std::fmt;

use chemifree_domain::budget::{Interrupt, Interrupted};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::stereo_group::StereoGroupTable;
use serde::Serialize;

use crate::Loss;

/// Identifies this bounded application profile, independently of the external library version.
pub const MODEL_ID: &str = "chemifree-standard-inchi-1";
/// Standard identifiers use the external implementation's semantics, not graph canonical text.
pub const STANDARD_SEMANTICS: &str = "Standard InChI applies its own normalization rules; tautomer, protonation and representation distinctions may merge. This is not a claim that a particular normalization occurred or that the input graph or every stereo statement roundtrips.";

/// The actual provider registration and options used, not an inferred library version.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct InchiProviderIdentity {
    pub name: String,
    pub version: String,
    /// A reproducible build identity, such as the checked library SHA-256.
    pub build: String,
    /// Effective native options. An empty list means the provider uses Standard defaults.
    pub options: Vec<String>,
}

/// One successful provider call. Scientific normalizations and omissions belong in `losses`;
/// callers must not assume the identifier preserves every detail of a drawing.
#[derive(Clone, Debug, PartialEq)]
pub struct InchiOutput {
    pub inchi: String,
    pub inchikey: String,
    pub aux_info: Option<String>,
    pub warnings: Vec<String>,
    pub native_code: i32,
    pub native_message: Option<String>,
    pub native_log: Option<String>,
    pub losses: Vec<Loss>,
}

/// How the operation reports a failure without returning a partial identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum InchiErrorKind {
    Unavailable,
    Unsupported,
    Invalid,
    Limit,
    Failed,
}

/// A stable code and precise provider explanation. Failures never contain generated output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InchiError {
    pub kind: InchiErrorKind,
    pub code: &'static str,
    pub message: String,
}

impl fmt::Display for InchiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for InchiError {}

/// A trusted, optional native adapter. Implementations must be thread safe, enforce their
/// native resource limits, and use no original application DLL as an execution engine.
pub trait InchiProvider: Send + Sync {
    /// The checked registration and exact effective Standard InChI options.
    fn identity(&self) -> InchiProviderIdentity;

    /// Generates Standard InChI and its InChIKey from the captured graph and groups.
    /// Stored configurations are the stereo source; coordinates and drawn marks are not
    /// silently substituted for stored configurations. Unsupported chemistry and enhanced
    /// group semantics must be refused rather than silently converted to an absolute form.
    /// Every normalization or omission is reported in the returned losses.
    ///
    /// `stop` is asked after any serialization the provider performs itself and right before
    /// its uninterruptible native call. It is never asked after that call has begun.
    ///
    /// # Errors
    ///
    /// `Err(Interrupted)` when `stop` ends the call before the native work begins; no identifier
    /// is returned. Otherwise the inner [`InchiError`] identifies unavailable libraries,
    /// unsupported or invalid chemistry, resource limits, or native failures. No identifier is
    /// returned for these cases either.
    fn generate(
        &self,
        graph: &MoleculeGraph,
        groups: &StereoGroupTable,
        stop: &dyn Interrupt,
    ) -> Result<Result<InchiOutput, InchiError>, Interrupted>;
}
