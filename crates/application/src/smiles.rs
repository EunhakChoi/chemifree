//! Application-owned boundary for the bounded SMILES text codec.
//!
//! The codec only handles notation: it turns text into an abstract [`SmilesMolecule`] and an
//! ordered molecule back into text. Hydrogen, radical and aromaticity interpretation belong
//! to the shared chemistry models and are applied by the application around the codec.

use chemifree_domain::document::MAX_COMMANDS;
use chemifree_domain::element::Element;

/// Longest accepted or produced SMILES text, in bytes.
pub const MAX_SMILES_BYTES: usize = 16 * 1024;
/// Deepest accepted or produced branch nesting.
pub const MAX_BRANCH_DEPTH: usize = 64;
/// Most ring-closure labels (`0`-`9`, `%10`-`%99`) written at once.
pub const MAX_WRITTEN_RING_LABELS: usize = 99;
/// Atoms plus bonds one text may create: the same as one transaction accepts.
pub const MAX_SMILES_ITEMS: usize = MAX_COMMANDS;

/// Name and version of the notation implementation, reported with every result.
pub const IMPLEMENTATION: &str = "chemifree-smiles";
pub const VERSION: &str = "1";
/// Name and version of the canonical form. It is Chemifree's own and is not compatible with
/// the canonical strings of any other toolkit.
pub const CANONICAL_IMPLEMENTATION: &str = "chemifree-smiles-canonical";
pub const CANONICAL_VERSION: &str = "1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmilesBondKind {
    Single,
    Double,
    Triple,
    Aromatic,
}

/// One atom as written. `hydrogens` is `None` for an atom outside brackets, whose count is
/// inferred; a bracket atom always states its count.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SmilesAtom {
    pub element: Element,
    /// Written in lower case.
    pub aromatic: bool,
    pub isotope: Option<u16>,
    pub charge: i8,
    pub hydrogens: Option<u8>,
    /// Byte span in the source text; `0..0` for a molecule built for writing.
    pub span: (usize, usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SmilesBond {
    pub begin: usize,
    pub end: usize,
    pub kind: SmilesBondKind,
}

/// Atoms in output (or source) order and their bonds.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SmilesMolecule {
    pub atoms: Vec<SmilesAtom>,
    pub bonds: Vec<SmilesBond>,
}

/// Machine-readable failure of the notation. Each code has one user message.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SmilesErrorCode {
    EmptyInput,
    LeadingWhitespace,
    InputTooLong,
    UnexpectedCharacter,
    BracketRequired,
    UnknownElement,
    UnterminatedBracket,
    InvalidBracketContent,
    InvalidIsotope,
    InvalidHydrogenCount,
    InvalidCharge,
    ChargeOutOfRange,
    InvalidRingNumber,
    RingClosureWithoutAtom,
    RingClosureAfterBranch,
    RingClosureSelf,
    RingBondMismatch,
    DuplicateBond,
    UnclosedRing,
    DanglingBond,
    BondWithoutSource,
    RepeatedBond,
    EmptyComponent,
    BranchWithoutAtom,
    EmptyBranch,
    UnmatchedBranchClose,
    UnclosedBranch,
    BranchDepthLimit,
    SizeLimit,
    AromaticAtomOutsideRing,
    StereoUnsupported,
    QueryUnsupported,
    ReactionUnsupported,
    AtomMapUnsupported,
    BondKindUnsupported,
}

impl SmilesErrorCode {
    /// Every code, in declaration order.
    pub const ALL: [Self; 35] = [
        Self::EmptyInput,
        Self::LeadingWhitespace,
        Self::InputTooLong,
        Self::UnexpectedCharacter,
        Self::BracketRequired,
        Self::UnknownElement,
        Self::UnterminatedBracket,
        Self::InvalidBracketContent,
        Self::InvalidIsotope,
        Self::InvalidHydrogenCount,
        Self::InvalidCharge,
        Self::ChargeOutOfRange,
        Self::InvalidRingNumber,
        Self::RingClosureWithoutAtom,
        Self::RingClosureAfterBranch,
        Self::RingClosureSelf,
        Self::RingBondMismatch,
        Self::DuplicateBond,
        Self::UnclosedRing,
        Self::DanglingBond,
        Self::BondWithoutSource,
        Self::RepeatedBond,
        Self::EmptyComponent,
        Self::BranchWithoutAtom,
        Self::EmptyBranch,
        Self::UnmatchedBranchClose,
        Self::UnclosedBranch,
        Self::BranchDepthLimit,
        Self::SizeLimit,
        Self::AromaticAtomOutsideRing,
        Self::StereoUnsupported,
        Self::QueryUnsupported,
        Self::ReactionUnsupported,
        Self::AtomMapUnsupported,
        Self::BondKindUnsupported,
    ];

    pub const fn code(self) -> &'static str {
        match self {
            Self::EmptyInput => "empty_input",
            Self::LeadingWhitespace => "leading_whitespace",
            Self::InputTooLong => "input_too_long",
            Self::UnexpectedCharacter => "unexpected_character",
            Self::BracketRequired => "bracket_required",
            Self::UnknownElement => "unknown_element",
            Self::UnterminatedBracket => "unterminated_bracket",
            Self::InvalidBracketContent => "invalid_bracket_content",
            Self::InvalidIsotope => "invalid_isotope",
            Self::InvalidHydrogenCount => "invalid_hydrogen_count",
            Self::InvalidCharge => "invalid_charge",
            Self::ChargeOutOfRange => "charge_out_of_range",
            Self::InvalidRingNumber => "invalid_ring_number",
            Self::RingClosureWithoutAtom => "ring_closure_without_atom",
            Self::RingClosureAfterBranch => "ring_closure_after_branch",
            Self::RingClosureSelf => "ring_closure_self",
            Self::RingBondMismatch => "ring_bond_mismatch",
            Self::DuplicateBond => "duplicate_bond",
            Self::UnclosedRing => "unclosed_ring",
            Self::DanglingBond => "dangling_bond",
            Self::BondWithoutSource => "bond_without_source",
            Self::RepeatedBond => "repeated_bond",
            Self::EmptyComponent => "empty_component",
            Self::BranchWithoutAtom => "branch_without_atom",
            Self::EmptyBranch => "empty_branch",
            Self::UnmatchedBranchClose => "unmatched_branch_close",
            Self::UnclosedBranch => "unclosed_branch",
            Self::BranchDepthLimit => "branch_depth_limit",
            Self::SizeLimit => "size_limit",
            Self::AromaticAtomOutsideRing => "aromatic_atom_outside_ring",
            Self::StereoUnsupported => "stereo_unsupported",
            Self::QueryUnsupported => "query_unsupported",
            Self::ReactionUnsupported => "reaction_unsupported",
            Self::AtomMapUnsupported => "atom_map_unsupported",
            Self::BondKindUnsupported => "bond_kind_unsupported",
        }
    }

    /// The user-facing explanation of the code.
    pub const fn message(self) -> &'static str {
        match self {
            Self::EmptyInput => "The SMILES text is empty.",
            Self::LeadingWhitespace => {
                "The SMILES text starts with whitespace, which ends a SMILES; remove it."
            }
            Self::InputTooLong => "The SMILES text is longer than the accepted size.",
            Self::UnexpectedCharacter => "This character is not valid SMILES here.",
            Self::BracketRequired => {
                "This element is outside the organic subset and must be written in brackets."
            }
            Self::UnknownElement => "This is not a known element symbol.",
            Self::UnterminatedBracket => "A bracket atom is missing its closing bracket.",
            Self::InvalidBracketContent => "This is not valid inside a bracket atom.",
            Self::InvalidIsotope => "An isotope is a mass number from 1 to 999.",
            Self::InvalidHydrogenCount => "A hydrogen count is H followed by at most one digit.",
            Self::InvalidCharge => {
                "A charge is a sign followed by one or two digits, or repeated signs."
            }
            Self::ChargeOutOfRange => "A charge must be between -8 and 8.",
            Self::InvalidRingNumber => "A ring closure is a digit or % followed by two digits.",
            Self::RingClosureWithoutAtom => {
                "A ring closure must directly follow the atom it belongs to."
            }
            Self::RingClosureAfterBranch => {
                "A ring closure cannot follow a closed branch; place it directly after its atom."
            }
            Self::RingClosureSelf => "A ring closure cannot join an atom to itself.",
            Self::RingBondMismatch => "The two ends of a ring closure name different bond kinds.",
            Self::DuplicateBond => "These two atoms are already bonded.",
            Self::UnclosedRing => "A ring closure is opened but never closed.",
            Self::DanglingBond => "A bond symbol must be followed by an atom or a ring closure.",
            Self::BondWithoutSource => "A bond symbol needs an atom before it.",
            Self::RepeatedBond => "Two bond symbols cannot follow each other.",
            Self::EmptyComponent => "A dot must be between two components.",
            Self::BranchWithoutAtom => "A branch must follow an atom.",
            Self::EmptyBranch => "A branch must contain at least one atom.",
            Self::UnmatchedBranchClose => "This closing parenthesis has no opening one.",
            Self::UnclosedBranch => "An opening parenthesis is never closed.",
            Self::BranchDepthLimit => "Branches are nested deeper than the accepted limit.",
            Self::SizeLimit => "The text describes more atoms and bonds than one import accepts.",
            Self::AromaticAtomOutsideRing => {
                "An aromatic (lower case) atom needs at least one aromatic bond."
            }
            Self::StereoUnsupported => {
                "Stereochemistry (@, /, \\) is not supported; remove it to read the connectivity."
            }
            Self::QueryUnsupported => "Wildcard and query atoms are not supported.",
            Self::ReactionUnsupported => "Reaction SMILES is not supported.",
            Self::AtomMapUnsupported => "Atom maps are not supported.",
            Self::BondKindUnsupported => "This bond kind is not supported.",
        }
    }
}

/// A failed read: what, and the byte span `start..end` of the offending text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SmilesError {
    pub code: SmilesErrorCode,
    pub start: usize,
    pub end: usize,
}

/// A successful read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SmilesRead {
    pub molecule: SmilesMolecule,
    /// Byte span of non-whitespace text after the terminating whitespace, which is ignored.
    pub ignored: Option<(usize, usize)>,
}

/// Why a molecule could not be written as a string the reader would accept.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmilesFormatError {
    /// More atoms and bonds than the reader accepts.
    SizeLimit,
    /// The text would be longer than the reader accepts.
    OutputLimit,
    /// Branches would nest deeper than the reader accepts.
    BranchDepthLimit,
    /// More ring closures would be open at once than the notation can number.
    RingLabelLimit,
    /// An atom that needs no brackets is not in the organic subset or is charged.
    NotWritable,
}

/// Written text and the order in which it names the molecule's atoms.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SmilesText {
    pub text: String,
    /// `atom_order[k]` is the index in the written molecule of the `k`-th atom of the text.
    pub atom_order: Vec<usize>,
}

/// Implemented outside the application. Output must be accepted again by the same reader.
pub trait SmilesCodec: Send + Sync {
    fn read(&self, text: &str) -> Result<SmilesRead, SmilesError>;
    fn write(&self, molecule: &SmilesMolecule) -> Result<SmilesText, SmilesFormatError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_error_code_has_a_distinct_name_and_message() {
        let names: HashSet<_> = SmilesErrorCode::ALL.iter().map(|c| c.code()).collect();
        let messages: HashSet<_> = SmilesErrorCode::ALL.iter().map(|c| c.message()).collect();
        assert_eq!(names.len(), SmilesErrorCode::ALL.len());
        assert_eq!(messages.len(), SmilesErrorCode::ALL.len());
        assert!(
            SmilesErrorCode::ALL
                .iter()
                .all(|c| !c.message().is_empty() && c.message().ends_with('.'))
        );
    }

    #[test]
    fn the_code_list_covers_every_variant_in_order() {
        for (index, code) in SmilesErrorCode::ALL.iter().enumerate() {
            assert_eq!(*code as usize, index, "{} is out of order", code.code());
        }
        assert_eq!(
            SmilesErrorCode::BondKindUnsupported as usize + 1,
            SmilesErrorCode::ALL.len(),
            "a variant was added after the last listed code"
        );
    }
}
