//! Every SMILES error code, its class and its message, in one table.

use chemifree_application::smiles_codec::{ErrorClass, SmilesError, SourceSpan};

macro_rules! codes {
    ($($variant:ident = ($code:literal, $class:ident, $message:literal),)*) => {
        /// A stable machine-readable error code.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Code {
            $($variant,)*
        }

        impl Code {
            /// Every code, in documentation order.
            pub const ALL: &'static [Code] = &[$(Code::$variant,)*];

            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Code::$variant => $code,)*
                }
            }

            pub const fn class(self) -> ErrorClass {
                match self {
                    $(Code::$variant => ErrorClass::$class,)*
                }
            }

            /// The message shown to the user for this code.
            pub const fn message(self) -> &'static str {
                match self {
                    $(Code::$variant => $message,)*
                }
            }
        }
    };
}

codes! {
    // Syntax (OpenSMILES, Grammar and the sections on atoms, bonds and ring closures).
    EmptyInput = ("empty_input", Syntax, "The SMILES is empty."),
    LeadingWhitespace = ("leading_whitespace", Syntax, "Whitespace ends a SMILES, so a SMILES cannot start with it."),
    UnexpectedCharacter = ("unexpected_character", Syntax, "This character cannot appear here in a SMILES."),
    UnclosedBracket = ("unclosed_bracket", Syntax, "A bracket atom is not closed with a ]."),
    EmptyBracket = ("empty_bracket", Syntax, "A bracket atom needs an element symbol."),
    UnknownElement = ("unknown_element", Syntax, "This is not an element symbol."),
    BracketUnexpectedCharacter = ("bracket_unexpected_character", Syntax, "This character cannot appear here inside a bracket atom."),
    IsotopeTooLong = ("isotope_too_long", Syntax, "An isotope has at most three digits."),
    ChargeSyntax = ("charge_syntax", Syntax, "A charge is + or - with at most two digits, or ++ or --."),
    UnclosedBranch = ("unclosed_branch", Syntax, "A ( is never closed with a )."),
    UnmatchedCloseParen = ("unmatched_close_paren", Syntax, "This ) has no matching (."),
    EmptyBranch = ("empty_branch", Syntax, "A branch needs at least one atom."),
    BranchWithoutAtom = ("branch_without_atom", Syntax, "A branch must follow an atom."),
    DanglingBond = ("dangling_bond", Syntax, "A bond symbol must be followed by an atom or a ring-closure digit."),
    BondWithoutAtom = ("bond_without_atom", Syntax, "A bond symbol must follow an atom or open a branch."),
    ConsecutiveBonds = ("consecutive_bonds", Syntax, "Two bond symbols in a row."),
    MisplacedDot = ("misplaced_dot", Syntax, "A dot must separate two parts, each starting with an atom."),
    DanglingDot = ("dangling_dot", Syntax, "A dot must be followed by an atom."),
    RingDigitWithoutAtom = ("ring_digit_without_atom", Syntax, "A ring-closure digit must directly follow its atom."),
    RingDigitAfterBranch = ("ring_digit_after_branch", Syntax, "A ring-closure digit cannot follow a closing parenthesis; put it right after its atom."),
    RingLabelSyntax = ("ring_label_syntax", Syntax, "A % ring label needs exactly two digits."),
    UnclosedRing = ("unclosed_ring", Syntax, "A ring-closure label is opened but never closed."),
    RingSelfBond = ("ring_self_bond", Syntax, "A ring closure cannot join an atom to itself."),
    DuplicateBond = ("duplicate_bond", Syntax, "These two atoms are already bonded."),
    RingBondConflict = ("ring_bond_conflict", Syntax, "The two ends of a ring closure name different bonds."),
    // Valid SMILES the drawing or the shared models cannot hold.
    StereoUnsupported = ("stereo_unsupported", Unsupported, "Stereochemistry (@, /, \\) is not supported."),
    WildcardUnsupported = ("wildcard_unsupported", Unsupported, "The * wildcard atom is a query, which is not supported."),
    QuadrupleBondUnsupported = ("quadruple_bond_unsupported", Unsupported, "The quadruple bond $ is not supported."),
    AtomClassUnsupported = ("atom_class_unsupported", Unsupported, "Atom classes (:n) are not supported."),
    ExtendedRingLabelUnsupported = ("extended_ring_label_unsupported", Unsupported, "The %(nnn) ring label is not supported; use one or two digits."),
    ReactionUnsupported = ("reaction_unsupported", Unsupported, "Reaction SMILES (>) is not supported."),
    ChargeOutOfRange = ("charge_out_of_range", Unsupported, "The drawing holds charges from the domain minimum to the domain maximum only."),
    IsotopeZeroUnsupported = ("isotope_zero_unsupported", Unsupported, "Isotope 0 cannot be recorded; the drawing holds mass numbers 1 to 999."),
    AromaticBridgeUnsupported = ("aromatic_bridge_unsupported", Unsupported, "An aromatic bond outside every ring cannot be recorded."),
    AromaticAtomOutsideRing = ("aromatic_atom_outside_ring", Unsupported, "A lower-case aromatic atom needs an aromatic bond in a ring."),
    AromaticSystemUnsupported = ("aromatic_system_unsupported", Unsupported, "The aromatic system is not one the aromaticity model reads."),
    // Limits.
    InputTooLong = ("input_too_long", Limit, "The SMILES is longer than the byte limit."),
    CommandLimit = ("command_limit", Limit, "The SMILES creates more atoms and bonds than one edit may."),
    BranchDepthLimit = ("branch_depth_limit", Limit, "Parentheses are nested deeper than the limit."),
    RingLabelLimit = ("ring_label_limit", Limit, "More ring closures are open at once than there are labels."),
    AtomLimit = ("atom_limit", Limit, "The canonical form numbers a limited number of atoms."),
    SearchLimit = ("search_limit", Limit, "The canonical numbering search exceeded its work limit."),
    // Graphs that cannot be written.
    EmptyGraph = ("empty_graph", NotWritable, "A drawing without atoms has no SMILES."),
    QueryNotWritable = ("query_not_writable", NotWritable, "A query atom has no SMILES."),
    BondNotWritable = ("bond_not_writable", NotWritable, "Only single, double, triple and aromatic bonds have a SMILES symbol."),
    IsotopeNotWritable = ("isotope_not_writable", NotWritable, "SMILES holds isotope mass numbers 1 to 999."),
    ChargeNotWritable = ("charge_not_writable", NotWritable, "The charge is outside the range the reader accepts."),
    HydrogensUnknown = ("hydrogens_unknown", NotWritable, "An atom with unknown hydrogens has no SMILES."),
    HydrogensNotRepresentable = ("hydrogens_not_representable", NotWritable, "The atom's hydrogens cannot be determined or written so that they read back the same."),
    RadicalNotRepresentable = ("radical_not_representable", NotWritable, "The atom's radical state cannot be written so that it reads back the same."),
    AromaticUnsupported = ("aromatic_unsupported", NotWritable, "The aromatic bonds are not in a system the aromaticity model reads."),
    AromaticElementUnsupported = ("aromatic_element_unsupported", NotWritable, "The element has no lower-case aromatic symbol."),
    CanonicalUnsupported = ("canonical_unsupported", NotWritable, "The shared valence or aromaticity model cannot determine this graph, so it has no canonical form."),
    TextTooLong = ("text_too_long", NotWritable, "The SMILES would be longer than the reader accepts."),
    RoundTripFailed = ("round_trip_failed", NotWritable, "The written SMILES did not read back as the same graph."),
}

/// A code with where it applies and, where the code summarizes several, the model's reason.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Failure {
    pub code: Code,
    pub span: Option<(usize, usize)>,
    pub detail: Option<&'static str>,
}

impl Failure {
    pub(super) fn at(code: Code, start: usize, end: usize) -> Self {
        Self {
            code,
            span: Some((start, end)),
            detail: None,
        }
    }

    pub(super) fn bare(code: Code) -> Self {
        Self {
            code,
            span: None,
            detail: None,
        }
    }

    pub(super) fn because(mut self, detail: &'static str) -> Self {
        self.detail = Some(detail);
        self
    }
}

impl From<Failure> for SmilesError {
    fn from(failure: Failure) -> Self {
        SmilesError {
            code: failure.code.as_str(),
            class: failure.code.class(),
            message: failure.code.message().into(),
            span: failure.span.map(|(start, end)| SourceSpan { start, end }),
            detail: failure.detail,
        }
    }
}
