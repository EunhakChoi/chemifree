//! Error codes with their user messages in one table, so a code cannot lack a message.

use std::fmt;

use crate::document::{AtomId, BondId};

/// A half-open range of UTF-8 byte offsets into the input. Both ends lie on `char` boundaries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorClass {
    /// The text is not valid SMILES.
    Syntax,
    /// Valid SMILES (or a valid drawing) that this implementation does not support.
    Unsupported,
    /// A bound of the reader or writer was exceeded.
    Limit,
    /// A drawing the writer refuses, because its output could not be read back.
    NotWritable,
}

/// What a write refusal is about.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Subject {
    Atom(AtomId),
    Bond(BondId),
}

macro_rules! error_codes {
    ($($variant:ident, $class:ident, $code:literal, $message:literal;)*) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum ErrorCode {
            $($variant,)*
        }

        impl ErrorCode {
            /// Every code, in declaration order.
            pub const ALL: &'static [ErrorCode] = &[$(Self::$variant,)*];

            /// The stable machine-readable code.
            pub const fn code(self) -> &'static str {
                match self {
                    $(Self::$variant => $code,)*
                }
            }

            /// The user-facing message.
            pub const fn message(self) -> &'static str {
                match self {
                    $(Self::$variant => $message,)*
                }
            }

            pub const fn class(self) -> ErrorClass {
                match self {
                    $(Self::$variant => ErrorClass::$class,)*
                }
            }

            pub fn from_code(code: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|candidate| candidate.code() == code)
            }
        }
    };
}

error_codes! {
    // Input framing (reader).
    EmptyInput, Syntax, "empty_input", "The input is empty; SMILES needs at least one atom.";
    LeadingWhitespace, Syntax, "leading_whitespace", "SMILES must not start with whitespace; whitespace ends the SMILES.";
    InputTooLong, Limit, "input_too_long", "The input is longer than the reader accepts.";
    UnexpectedCharacter, Syntax, "unexpected_character", "This character is not part of SMILES.";
    // Atoms.
    BareAtomNotOrganic, Syntax, "bare_atom_not_organic", "Only B, C, N, O, P, S, F, Cl, Br and I (and aromatic b, c, n, o, p, s) may be written without brackets.";
    UnclosedBracketAtom, Syntax, "unclosed_bracket_atom", "This bracket atom is not closed by ].";
    MissingElement, Syntax, "missing_element", "A bracket atom needs an element symbol after its optional isotope.";
    UnknownElement, Syntax, "unknown_element", "This is not an element symbol.";
    BracketUnexpectedCharacter, Syntax, "bracket_unexpected_character", "This character is not allowed here in a bracket atom (isotope, symbol, hydrogens, charge, then ]).";
    IsotopeTooLong, Syntax, "isotope_too_long", "An isotope has at most three digits.";
    IsotopeZeroUnsupported, Unsupported, "isotope_zero_unsupported", "Isotope 0 is not supported: the drawing records mass numbers 1 to 999 only.";
    ChargeSyntax, Syntax, "charge_syntax", "A charge is + or - with an optional number of at most two digits, or a repeated sign.";
    ChargeOutOfRange, Syntax, "charge_out_of_range", "The charge is outside the range the drawing records.";
    // Bonds, dots and branches.
    DanglingBond, Syntax, "dangling_bond", "This bond symbol is not followed by an atom or ring closure.";
    BondWithoutAtom, Syntax, "bond_without_atom", "A bond symbol needs an atom before it.";
    BondAfterDot, Syntax, "bond_after_dot", "A bond symbol cannot follow a dot.";
    RepeatedBond, Syntax, "repeated_bond", "A bond symbol cannot follow another bond symbol.";
    DotWithoutAtom, Syntax, "dot_without_atom", "A dot must separate two atoms.";
    BranchWithoutAtom, Syntax, "branch_without_atom", "A branch must follow an atom or a closed branch.";
    EmptyBranch, Syntax, "empty_branch", "A branch needs at least one atom.";
    UnmatchedCloseBranch, Syntax, "unmatched_close_branch", "This ) has no matching (.";
    UnclosedBranch, Syntax, "unclosed_branch", "This ( is never closed by ).";
    BranchDepthLimit, Limit, "branch_depth_limit", "Branches are nested deeper than the reader accepts.";
    // Ring closures.
    RingDigitWithoutAtom, Syntax, "ring_digit_without_atom", "A ring closure digit needs an atom before it.";
    RingDigitAfterBranchOpen, Syntax, "ring_digit_after_branch_open", "A ring closure digit cannot directly follow (.";
    RingDigitAfterBranchClose, Syntax, "ring_digit_after_branch_close", "A ring closure digit must follow its atom, not a closed branch.";
    RingLabelSyntax, Syntax, "ring_label_syntax", "A % ring label is followed by exactly two digits.";
    RingClosureSelf, Syntax, "ring_closure_self", "A ring closure cannot bond an atom to itself.";
    RingClosureDuplicateBond, Syntax, "ring_closure_duplicate_bond", "These two atoms are already bonded.";
    RingBondConflict, Syntax, "ring_bond_conflict", "The two ends of a ring closure name different bonds.";
    UnclosedRing, Syntax, "unclosed_ring", "This ring closure is never closed.";
    // Limits.
    CommandLimit, Limit, "command_limit", "The structure has more atoms and bonds than one transaction accepts.";
    // Aromaticity.
    AromaticBondOutsideRing, Syntax, "aromatic_bond_outside_ring", "An aromatic bond must be in a ring.";
    AromaticAtomOutsideRing, Syntax, "aromatic_atom_outside_ring", "A lowercase aromatic atom must be in a ring.";
    AromaticAtomNotInAromaticRing, Syntax, "aromatic_atom_not_in_aromatic_ring", "This lowercase atom has no aromatic bond to another atom.";
    AromaticSystemUnsupported, Unsupported, "aromatic_system_unsupported", "The aromatic system is not one the chemifree-aromaticity-2 model supports.";
    // Unsupported SMILES features.
    ChiralityUnsupported, Unsupported, "chirality_unsupported", "Tetrahedral and other chirality (@, @@) is not supported; stereo is not implemented.";
    DirectionalBondUnsupported, Unsupported, "directional_bond_unsupported", "Directional bonds (/, \\) are not supported; stereo is not implemented.";
    WildcardUnsupported, Unsupported, "wildcard_unsupported", "The wildcard atom * is a query atom; query SMILES is not supported.";
    QuadrupleBondUnsupported, Unsupported, "quadruple_bond_unsupported", "The quadruple bond $ is not supported.";
    AtomClassUnsupported, Unsupported, "atom_class_unsupported", "Atom classes (:n) are not supported.";
    ReactionUnsupported, Unsupported, "reaction_unsupported", "Reaction SMILES (>) is not supported.";
    // Writer refusals.
    EmptyGraph, NotWritable, "empty_graph", "A drawing with no atoms has no SMILES; the reader would refuse an empty string.";
    QueryAtomNotWritable, NotWritable, "query_atom_not_writable", "A query atom cannot be written as SMILES.";
    BondKindNotWritable, NotWritable, "bond_kind_not_writable", "Only single, double, triple and aromatic bonds can be written as SMILES.";
    AromaticBridgeNotWritable, NotWritable, "aromatic_bridge_not_writable", "An aromatic bond that is not in a ring would be refused by the reader.";
    AromaticityUnsupported, NotWritable, "aromaticity_unsupported", "The aromatic bonds are not a system chemifree-aromaticity-2 supports, so the reader would refuse them.";
    HydrogensUnknownNotWritable, NotWritable, "hydrogens_unknown_not_writable", "An atom with stated unknown hydrogens cannot be written.";
    HydrogensUndetermined, NotWritable, "hydrogens_undetermined", "The valence model cannot determine this atom's hydrogen count, which its bracket needs.";
    HydrogenCountNotWritable, NotWritable, "hydrogen_count_not_writable", "A bracket atom's hydrogen count is a single digit, 0 to 9.";
    RadicalNotWritable, NotWritable, "radical_not_writable", "The radical state is not the one the reader would infer from the bracket atom's hydrogens.";
    IsotopeNotWritable, NotWritable, "isotope_not_writable", "The isotope is outside 1 to 999.";
    ChargeNotWritable, NotWritable, "charge_not_writable", "The charge is outside the range the reader accepts.";
    RingLabelLimit, Limit, "ring_label_limit", "More rings are open at once than SMILES has ring labels.";
    OutputTooLong, Limit, "output_too_long", "The text would be longer than the reader accepts.";
    AtomLimit, Limit, "atom_limit", "The canonical form is limited in the number of atoms.";
    SearchLimit, Limit, "search_limit", "The canonical search exceeded its work limit.";
    CanonicalUnsupported, NotWritable, "canonical_unsupported", "The chemical graph identity model cannot determine this structure, so it has no canonical form.";
    WriterInconsistent, NotWritable, "writer_inconsistent", "The written text did not read back as the same structure.";
}

/// A reader or writer refusal. `span` is set for read errors, `subject` for write refusals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SmilesError {
    pub code: ErrorCode,
    pub span: Option<Span>,
    pub subject: Option<Subject>,
    /// The aromaticity, valence or identity model's own reason, when one decided.
    pub reason: Option<&'static str>,
}

impl SmilesError {
    pub(crate) const fn at(code: ErrorCode, start: usize, end: usize) -> Self {
        Self {
            code,
            span: Some(Span::new(start, end)),
            subject: None,
            reason: None,
        }
    }

    pub(crate) const fn refused(code: ErrorCode) -> Self {
        Self {
            code,
            span: None,
            subject: None,
            reason: None,
        }
    }

    pub(crate) const fn about(mut self, subject: Subject) -> Self {
        self.subject = Some(subject);
        self
    }

    pub(crate) const fn because(mut self, reason: &'static str) -> Self {
        self.reason = Some(reason);
        self
    }

    pub const fn class(&self) -> ErrorClass {
        self.code.class()
    }
}

impl fmt::Display for SmilesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code.message())
    }
}

impl std::error::Error for SmilesError {}
