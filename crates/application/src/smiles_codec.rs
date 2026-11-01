//! Application-owned port for the SMILES reader and writers.
//!
//! The codec turns text into atoms and bonds and a graph into text. It knows nothing of
//! documents, placements or consent: those belong to the application operations around it.

use std::fmt;

use chemifree_domain::document::{Atom, AtomId, BondId, BondOrder};
use chemifree_domain::graph::MoleculeGraph;

/// The identity of the reader and the recorded-order writer.
pub const SMILES_MODEL_ID: &str = "chemifree-smiles-1";
/// The identity of the canonical writer. Its output is Chemifree's own, bound to
/// [`CANONICAL_GRAPH_MODEL`], and is not compatible with any toolkit's canonical SMILES.
pub const SMILES_CANONICAL_ID: &str = "chemifree-smiles-canonical-1";
/// The chemical graph identity model the canonical writer is bound to.
pub const CANONICAL_GRAPH_MODEL: &str = chemifree_domain::identity::CHEMICAL_MODEL_ID;

/// Sizes the codec enforces; the catalog wording quotes them from here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SmilesLimits {
    /// The most bytes of SMILES text, whitespace-terminated, either way.
    pub max_input_bytes: usize,
    /// The most atoms plus bonds one string may create (the document transaction limit).
    pub max_commands: usize,
    /// The deepest parenthesis nesting.
    pub max_branch_depth: usize,
    /// The ring-closure labels a string may have open at once.
    pub max_open_rings: usize,
    /// The most atoms the canonical writer numbers.
    pub canonical_max_atoms: usize,
}

/// A byte range of the input: `start` is the first byte and `end` is one past the last, so
/// `start == end` names a position between bytes (the end of the text, or an empty input).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorClass {
    /// The text is not SMILES.
    Syntax,
    /// The text is SMILES the drawing or the models cannot hold.
    Unsupported,
    /// A size limit.
    Limit,
    /// A graph that cannot be written as SMILES that reads back the same.
    NotWritable,
}

impl ErrorClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::Unsupported => "unsupported",
            Self::Limit => "limit",
            Self::NotWritable => "not_writable",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SmilesError {
    pub code: &'static str,
    pub class: ErrorClass,
    pub message: String,
    pub span: Option<SourceSpan>,
    /// The model's own reason, where the code summarizes several.
    pub detail: Option<&'static str>,
}

impl fmt::Display for SmilesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}
impl std::error::Error for SmilesError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RadicalReason {
    /// The element or charge is outside the shared valence model.
    OutsideModel,
    /// Singlet and triplet both fit the stated hydrogens and bonds.
    Ambiguous,
    /// No radical state fits the stated hydrogens and bonds.
    NoState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReadNote {
    /// Non-whitespace text after the first whitespace, which ended the SMILES.
    IgnoredText(SourceSpan),
    /// A bracket atom whose radical state could not be determined and was recorded unknown.
    RadicalUndetermined {
        atom: usize,
        span: SourceSpan,
        reason: RadicalReason,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct SmilesAtom {
    pub atom: Atom,
    pub span: SourceSpan,
    /// The non-vertex hydrogens the shared valence model resolves for the atom with its
    /// bonds: the stated count for a bracket atom, the inferred count for a bare one.
    /// `None` where the model cannot determine them.
    pub hydrogens: Option<u16>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SmilesBond {
    /// Indices into [`SmilesRead::atoms`], in text order.
    pub begin: usize,
    pub end: usize,
    /// Single, double, triple or aromatic.
    pub order: BondOrder,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SmilesRead {
    pub atoms: Vec<SmilesAtom>,
    pub bonds: Vec<SmilesBond>,
    pub notes: Vec<ReadNote>,
    /// Dot-separated parts, whether or not ring closures later joined them.
    pub parts: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmilesStyle {
    /// The drawing's own atom order, with its recorded states where SMILES can say them.
    Recorded,
    /// Chemifree's canonical form of the chemical graph.
    Canonical,
}

impl SmilesStyle {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Recorded => "recorded",
            Self::Canonical => "canonical",
        }
    }
}

/// A representation change a writer made, each naming the atoms or bonds it concerns.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WriteChange {
    /// Attachment points are dropped; SMILES has none.
    AttachmentPoints(Vec<AtomId>),
    /// Atoms drawn with inferred hydrogens are written with the count the valence model
    /// determines, inside brackets.
    InferredHydrogensStated(Vec<AtomId>),
    /// Atoms with stated hydrogens are written bare, so the count is inferred on reading.
    StatedHydrogensInferred(Vec<AtomId>),
    /// Atoms outside the valence model: no hydrogens are determined, so none are written.
    HydrogensAssumed(Vec<AtomId>),
    /// Atoms outside the valence model: reading gives them an undetermined radical state.
    RadicalAssumed(Vec<AtomId>),
    /// Kekulé single and double bonds written as aromatic.
    KekuleAsAromatic(Vec<BondId>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SmilesWrite {
    pub text: String,
    /// The atoms in the order they appear in the text; the n-th atom of a re-read is the
    /// n-th entry.
    pub atom_order: Vec<AtomId>,
    /// Explicit hydrogen atoms folded into their heavy atom (canonical only). They are not
    /// in `atom_order`.
    pub folded_hydrogens: Vec<AtomId>,
    pub changes: Vec<WriteChange>,
}

/// Implemented outside the application. Every text a writer returns has been read back by
/// the same reader and found to give the graph written.
pub trait SmilesCodec: Send + Sync {
    fn limits(&self) -> SmilesLimits;
    fn read(&self, text: &str) -> Result<SmilesRead, SmilesError>;
    fn write(&self, graph: &MoleculeGraph, style: SmilesStyle) -> Result<SmilesWrite, SmilesError>;
}
