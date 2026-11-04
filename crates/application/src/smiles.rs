//! Application-owned boundary for SMILES text: the port a reader and writer implement, and the
//! error vocabulary they share with every entry adapter.
//!
//! A reader turns one SMILES string into atoms and bonds in the drawing's own vocabulary; a
//! writer turns a chemical graph into one string, in recorded order or in Chemifree's canonical
//! form. Neither places atoms: SMILES has no coordinates.

use chemifree_domain::document::{Atom, AtomId, BondOrder};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::stereo::Turn;
use chemifree_domain::stereo_group::GroupKind;
use serde::{Deserialize, Serialize};

use crate::Loss;

/// The reader, and the recorded-order writer.
pub const SMILES_MODEL: &str = "chemifree-smiles-1";
/// The canonical writer. Its output is bound to [`chemifree_domain::identity::CHEMICAL_MODEL_ID`]:
/// two graphs have the same canonical text exactly when they have the same chemical identity.
/// The text is Chemifree's own and is not the canonical form of any other toolkit.
pub const SMILES_CANONICAL_MODEL: &str = "chemifree-smiles-canonical-1";

/// A part of the input text: `offset` is the first UTF-8 byte and `length` the number of bytes.
/// A zero length marks the end of the input, or a position between two characters.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Span {
    pub offset: usize,
    pub length: usize,
}

impl Span {
    pub const fn new(offset: usize, length: usize) -> Self {
        Self { offset, length }
    }

    /// The last byte of a non-empty span, or `None` for an empty one.
    pub const fn last(self) -> Option<usize> {
        if self.length == 0 {
            None
        } else {
            Some(self.offset + self.length - 1)
        }
    }

    /// `5..=9` for bytes 5 through 9 (the last byte, not an exclusive end), or `5` for an
    /// empty span at byte 5.
    pub fn describe(self) -> String {
        match self.last() {
            Some(last) => format!("{}..={last}", self.offset),
            None => self.offset.to_string(),
        }
    }
}

/// Why a code exists, which decides how a caller should treat it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    /// The text is not SMILES.
    Syntax,
    /// The text may be valid SMILES, but the drawing or this implementation cannot hold it.
    Unsupported,
    /// The text states something that contradicts itself or the chemistry it describes, such as a
    /// stereo group member that is no stereocenter.
    Chemistry,
    /// A bound of the reader or writer.
    Limit,
    /// The drawing cannot be written as SMILES that this reader accepts.
    NotWritable,
}

impl ErrorClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::Unsupported => "unsupported",
            Self::Chemistry => "chemistry",
            Self::Limit => "limit",
            Self::NotWritable => "not_writable",
        }
    }

    /// The closed diagnostic family (`malformed_input`, `unsupported_feature`,
    /// `invalid_chemistry`, `resource_limit`) a code of this class belongs to when a family is
    /// asked for; `None` for a writer refusal, which is not an input diagnostic.
    pub const fn family(self) -> Option<&'static str> {
        match self {
            Self::Syntax => Some("malformed_input"),
            Self::Unsupported => Some("unsupported_feature"),
            Self::Chemistry => Some("invalid_chemistry"),
            Self::Limit => Some("resource_limit"),
            Self::NotWritable => None,
        }
    }
}

macro_rules! codes {
    ($($variant:ident = $code:literal, $class:ident, $message:literal;)+) => {
        /// Every refusal of the SMILES reader and writer. The table is the only place a code,
        /// its class and its message are written down; `docs/CONTRACT.md` repeats it and a test
        /// compares the two.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum SmilesCode {
            $($variant,)+
        }

        impl SmilesCode {
            /// All codes, in table order.
            pub const ALL: &'static [SmilesCode] = &[$(Self::$variant,)+];

            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $code,)+
                }
            }

            pub const fn class(self) -> ErrorClass {
                match self {
                    $(Self::$variant => ErrorClass::$class,)+
                }
            }

            pub const fn message(self) -> &'static str {
                match self {
                    $(Self::$variant => $message,)+
                }
            }
        }
    };
}

codes! {
    EmptyInput = "empty_input", Syntax, "The SMILES text is empty.";
    LeadingWhitespace = "leading_whitespace", Syntax, "A SMILES string cannot start with whitespace; whitespace ends it.";
    UnexpectedCharacter = "unexpected_character", Syntax, "This character cannot appear here.";
    BracketRequired = "bracket_required", Syntax, "Only B, C, N, O, P, S, F, Cl, Br and I (and aromatic b, c, n, o, p, s) can be written without brackets.";
    UnclosedBracket = "unclosed_bracket", Syntax, "A bracket atom is not closed by ].";
    MissingElementSymbol = "missing_element_symbol", Syntax, "A bracket atom needs an element symbol.";
    UnknownElement = "unknown_element", Syntax, "This is not an element symbol.";
    InvalidBracketContent = "invalid_bracket_content", Syntax, "A bracket atom is isotope, symbol, hydrogen count, charge and atom class, in that order.";
    HydrogenCountOnHydrogen = "hydrogen_count_on_hydrogen", Syntax, "A hydrogen atom cannot have a hydrogen count; write [H][H] for hydrogen gas.";
    InvalidHydrogenCount = "invalid_hydrogen_count", Syntax, "A hydrogen count is H or H and one digit.";
    InvalidRingLabel = "invalid_ring_label", Syntax, "A ring label is one digit, or % and two digits.";
    RingDigitAtStart = "ring_digit_at_start", Syntax, "A ring label must follow the atom it belongs to; there is no atom yet.";
    RingDigitAfterBranchOpen = "ring_digit_after_branch_open", Syntax, "A ring label cannot follow ( ; it belongs right after an atom.";
    RingDigitAfterBranchClose = "ring_digit_after_branch_close", Syntax, "A ring label cannot follow ) ; it belongs right after an atom.";
    RingDigitAfterDot = "ring_digit_after_dot", Syntax, "A ring label cannot follow a dot.";
    RingDigitAfterDetachedBond = "ring_digit_after_detached_bond", Syntax, "A ring bond symbol must directly follow the atom it belongs to.";
    RingBondMismatch = "ring_bond_mismatch", Syntax, "The two ends of a ring closure name different bond symbols.";
    RingSelfBond = "ring_self_bond", Syntax, "An atom cannot be bonded to itself by a ring closure.";
    DuplicateBond = "duplicate_bond", Syntax, "Two atoms cannot be joined by more than one bond.";
    UnclosedRing = "unclosed_ring", Syntax, "A ring label is opened but never closed.";
    BranchWithoutAtom = "branch_without_atom", Syntax, "A branch needs an atom before its opening parenthesis.";
    EmptyBranch = "empty_branch", Syntax, "A branch cannot be empty.";
    UnmatchedCloseBranch = "unmatched_close_branch", Syntax, "This ) has no matching (.";
    UnclosedBranch = "unclosed_branch", Syntax, "This ( is never closed.";
    DanglingBond = "dangling_bond", Syntax, "A bond symbol must be followed by an atom.";
    DanglingDot = "dangling_dot", Syntax, "A dot must be followed by an atom.";
    BondWithoutAtom = "bond_without_atom", Syntax, "A bond symbol needs an atom before it.";
    DotWithoutAtom = "dot_without_atom", Syntax, "A dot needs an atom before it.";
    RepeatedBondSymbol = "repeated_bond_symbol", Syntax, "Two bond symbols cannot follow each other.";
    StereoClassUnsupported = "stereo_class_unsupported", Unsupported, "Only the tetrahedral marks @ and @@ are read; @TH, @AL, @SP, @TB and @OH classes are not supported.";
    StereoCenterUnsupportedHere = "stereo_center_unsupported_here", Unsupported, "The drawing cannot hold a tetrahedral configuration on this atom.";
    StereoNeighborCount = "stereo_neighbor_count", Unsupported, "A tetrahedral mark needs four neighbours counting one bracket hydrogen: three or four bonds.";
    DirectionalBondUnsupported = "directional_bond_unsupported", Unsupported, "The directional bonds / and \\ (double bond configuration) are not read yet.";
    CxsmilesMalformed = "cxsmiles_malformed", Syntax, "The CXSMILES block is not well formed: a bar, comma separated fields, a closing bar.";
    CxsmilesAtomOutOfRange = "cxsmiles_atom_out_of_range", Syntax, "A CXSMILES atom index is beyond the atoms of the SMILES text.";
    CxsmilesFieldUnsupported = "cxsmiles_field_unsupported", Unsupported, "Only the stereo group fields a:, o<n>: and &<n>: of a CXSMILES block are read.";
    CxsmilesGroupMemberNotStereocenter = "cxsmiles_group_member_not_stereocenter", Chemistry, "A CXSMILES stereo group lists an atom that carries no @ or @@ mark in the text.";
    CxsmilesMemberRepeated = "cxsmiles_member_repeated", Chemistry, "A CXSMILES atom index appears twice among the stereo groups; a center is in one group.";
    WildcardUnsupported = "wildcard_unsupported", Unsupported, "The wildcard atom * is not supported.";
    QuadrupleBondUnsupported = "quadruple_bond_unsupported", Unsupported, "The quadruple bond $ is not supported.";
    RingLabelExtendedUnsupported = "ring_label_extended_unsupported", Unsupported, "Ring labels written %(nnn) are not supported; use 0 to 99.";
    ChargeOutOfRange = "charge_out_of_range", Unsupported, "The charge is outside what the drawing can record.";
    IsotopeZero = "isotope_zero", Unsupported, "The drawing records isotope mass numbers 1 to 999 only; isotope 0 is not recorded.";
    IsotopeOutOfRange = "isotope_out_of_range", Unsupported, "The drawing records isotope mass numbers 1 to 999 only.";
    AromaticBondOutsideRing = "aromatic_bond_outside_ring", Unsupported, "An aromatic bond must lie in a ring; this one is a bridge between rings or a chain bond.";
    AromaticAtomOutsideRing = "aromatic_atom_outside_ring", Unsupported, "A lower-case aromatic atom must be joined to its ring by aromatic bonds.";
    AromaticSystemUnsupported = "aromatic_system_unsupported", Unsupported, "The aromatic ring system is outside the aromaticity model.";
    InputTooLong = "input_too_long", Limit, "The SMILES text is longer than the reader accepts.";
    CommandLimit = "command_limit", Limit, "The structure has more atoms and bonds than one import can add.";
    BranchDepthLimit = "branch_depth_limit", Limit, "Branches are nested more deeply than the reader accepts.";
    RingLabelLimit = "ring_label_limit", Limit, "More rings are open at once than there are ring labels.";
    OutputTooLong = "output_too_long", Limit, "The SMILES text would be longer than the reader accepts.";
    CanonicalLimit = "canonical_limit", Limit, "The canonical search exceeded its size or work limit.";
    EmptyGraph = "empty_graph", NotWritable, "A drawing with no atoms has no SMILES.";
    QueryAtomNotWritable = "query_atom_not_writable", NotWritable, "A query atom has no SMILES form here.";
    BondEndpointsInvalid = "bond_endpoints_invalid", NotWritable, "A bond must join two different atoms of the drawing.";
    BondKindNotWritable = "bond_kind_not_writable", NotWritable, "Only single, double, triple and aromatic bonds can be written.";
    AromaticNotWritable = "aromatic_not_writable", NotWritable, "The aromatic bonds are not an aromatic ring system under the aromaticity model.";
    AromaticElementNotWritable = "aromatic_element_not_writable", NotWritable, "This element has no lower-case aromatic symbol.";
    ChargeNotWritable = "charge_not_writable", NotWritable, "The charge is outside what the reader accepts.";
    IsotopeNotWritable = "isotope_not_writable", NotWritable, "The isotope mass number is outside 1 to 999.";
    HydrogensNotWritable = "hydrogens_not_writable", NotWritable, "The hydrogen count of this atom cannot be determined or written.";
    RadicalNotWritable = "radical_not_writable", NotWritable, "SMILES cannot state this radical so that it reads back the same.";
    AttachmentPointsNotWritable = "attachment_points_not_writable", NotWritable, "Attachment points have no SMILES form.";
    CanonicalUnsupported = "canonical_unsupported", NotWritable, "The graph has no canonical form under the chemical graph model.";
    ReadbackMismatch = "readback_mismatch", NotWritable, "The written text does not read back as the same atoms and bonds.";
}

/// A refusal with the part of the text it concerns. `reason` is the specific cause when the
/// code names a family, such as the aromaticity model's reason for `aromatic_system_unsupported`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SmilesError {
    pub code: SmilesCode,
    pub span: Option<Span>,
    pub reason: Option<String>,
}

impl SmilesError {
    pub fn at(code: SmilesCode, span: Span) -> Self {
        Self {
            code,
            span: Some(span),
            reason: None,
        }
    }

    pub fn new(code: SmilesCode) -> Self {
        Self {
            code,
            span: None,
            reason: None,
        }
    }

    #[must_use]
    pub fn because(self, reason: impl Into<String>) -> Self {
        Self {
            reason: Some(reason.into()),
            ..self
        }
    }
}

impl std::fmt::Display for SmilesError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code.message())?;
        if let Some(reason) = &self.reason {
            write!(formatter, " ({reason})")?;
        }
        Ok(())
    }
}

impl std::error::Error for SmilesError {}

#[derive(Clone, Debug, PartialEq)]
pub struct SmilesAtom {
    pub atom: Atom,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SmilesBond {
    /// Indices into [`SmilesRead::atoms`].
    pub begin: usize,
    pub end: usize,
    pub order: BondOrder,
}

/// One string read: the graph it states, what the reader could not keep, and what it ignored.
#[derive(Clone, Debug, PartialEq)]
pub struct SmilesRead {
    pub atoms: Vec<SmilesAtom>,
    pub bonds: Vec<SmilesBond>,
    /// Non-whitespace text after the terminating whitespace, which the reader does not read.
    pub ignored_text: Option<Span>,
    pub losses: Vec<Loss>,
    /// The configurations and enhanced groups the text states; empty for text without them.
    pub stereo: SmilesStereo,
}

/// A neighbour position of a tetrahedral center in the order the text lists them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmilesSlot {
    /// An index into [`SmilesRead::atoms`].
    Atom(usize),
    /// The hydrogen of a bracket atom, which is a property of the center, not an atom.
    ImplicitHydrogen,
}

/// One `@` or `@@` mark, resolved: the neighbours in OpenSMILES order and the direction the
/// last three run when looking from the first toward the center (`@` is anticlockwise).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SmilesCenter {
    /// Index into [`SmilesRead::atoms`].
    pub atom: usize,
    pub slots: [SmilesSlot; 4],
    pub turn: Turn,
    pub span: Span,
}

/// One enhanced stereo group of a CXSMILES block. Members are atom indexes in the text's atom
/// order, every one of them a [`SmilesCenter`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SmilesGroup {
    pub kind: GroupKind,
    /// The digits the text gave after `o` or `&`; `None` for an absolute group.
    pub number: Option<i32>,
    pub atoms: Vec<usize>,
    /// The field that opened the group.
    pub span: Span,
}

/// What a SMILES text states about stereo, beyond its graph.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SmilesStereo {
    pub centers: Vec<SmilesCenter>,
    pub groups: Vec<SmilesGroup>,
}

/// Which string a writer produces.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SmilesStyle {
    /// The drawing's own atoms and bonds, as recorded: Kekulé bonds stay Kekulé, aromatic
    /// bonds stay aromatic. Atoms are written by a depth-first walk that starts at the first
    /// atom and visits neighbours in document order; `atom_order` lists the result.
    Recorded,
    /// One text for every drawing of the same chemical graph under
    /// [`chemifree_domain::identity::CHEMICAL_MODEL_ID`].
    Canonical,
}

impl SmilesStyle {
    pub const fn model_id(self) -> &'static str {
        match self {
            Self::Recorded => SMILES_MODEL,
            Self::Canonical => SMILES_CANONICAL_MODEL,
        }
    }
}

/// One string written.
#[derive(Clone, Debug, PartialEq)]
pub struct SmilesWrite {
    pub text: String,
    /// The drawing's atom for each written atom, in the order of the text. Hydrogen vertices
    /// folded into a neighbour's count (canonical only) are not listed.
    pub atom_order: Vec<AtomId>,
    /// The hydrogen vertices folded into a neighbour's count, as `(hydrogen, neighbour)`.
    pub folded_hydrogens: Vec<(AtomId, AtomId)>,
    /// What the text does not keep: representation changes and assumptions the text makes.
    pub losses: Vec<Loss>,
}

/// The bounds a codec enforces, which the catalog quotes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SmilesLimits {
    pub max_input_bytes: usize,
    pub max_branch_depth: usize,
}

/// Implemented outside the application. Implementations never panic on any input, and a
/// writer returns only text that the same codec's reader accepts.
pub trait SmilesCodec: Send + Sync {
    fn limits(&self) -> SmilesLimits;

    /// Reads one SMILES string. Non-whitespace text after the first whitespace is reported in
    /// [`SmilesRead::ignored_text`], not read.
    ///
    /// # Errors
    ///
    /// A [`SmilesError`] with the part of `input` it concerns.
    fn read(&self, input: &str) -> Result<SmilesRead, SmilesError>;

    /// Writes `graph` as one SMILES string.
    ///
    /// # Errors
    ///
    /// A [`SmilesError`] of class `not_writable` or `limit` when no string that this codec's
    /// reader accepts states the graph.
    fn write(&self, graph: &MoleculeGraph, style: SmilesStyle) -> Result<SmilesWrite, SmilesError>;
}
