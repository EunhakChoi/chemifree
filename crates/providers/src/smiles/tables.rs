//! The symbol and whitespace tables the reader and both writers share. OpenSMILES sections
//! cited: "Organic Subset" (bare symbols), "Atomic Symbol" (bracket symbols, aromatic
//! symbols) and the grammar's `terminator`.

use chemifree_domain::element::Element;

/// The grammar's `terminator`: these end the SMILES (OpenSMILES, Grammar).
const WHITESPACE: [u8; 4] = [b' ', b'\t', b'\n', b'\r'];

pub(super) fn is_whitespace(byte: u8) -> bool {
    WHITESPACE.contains(&byte)
}

/// `aliphatic_organic`: the symbols that may be written without brackets (OpenSMILES,
/// "Organic Subset"). Two-letter symbols come first so a longest match picks them.
pub(super) const BARE_ORGANIC: [&str; 10] = ["Cl", "Br", "B", "C", "N", "O", "S", "P", "F", "I"];

/// One aromatic symbol: its lower-case spelling, its element, and whether it may be written
/// without brackets (`aromatic_organic`) or only inside them (`aromatic_symbols`).
pub(super) struct AromaticSymbol {
    pub lower: &'static str,
    pub element: &'static str,
    pub bare: bool,
}

/// OpenSMILES, Grammar: `aromatic_organic` (b c n o s p) and `aromatic_symbols` (adds se, as).
pub(super) const AROMATIC: [AromaticSymbol; 8] = [
    AromaticSymbol {
        lower: "se",
        element: "Se",
        bare: false,
    },
    AromaticSymbol {
        lower: "as",
        element: "As",
        bare: false,
    },
    AromaticSymbol {
        lower: "b",
        element: "B",
        bare: true,
    },
    AromaticSymbol {
        lower: "c",
        element: "C",
        bare: true,
    },
    AromaticSymbol {
        lower: "n",
        element: "N",
        bare: true,
    },
    AromaticSymbol {
        lower: "o",
        element: "O",
        bare: true,
    },
    AromaticSymbol {
        lower: "s",
        element: "S",
        bare: true,
    },
    AromaticSymbol {
        lower: "p",
        element: "P",
        bare: true,
    },
];

/// The lower-case form of `element`, if SMILES has one.
pub(super) fn aromatic_form(element: Element) -> Option<&'static AromaticSymbol> {
    AROMATIC
        .iter()
        .find(|symbol| symbol.element == element.symbol())
}

pub(super) fn is_bare_organic(element: Element) -> bool {
    BARE_ORGANIC.contains(&element.symbol())
}

/// A bare aromatic atom: written without brackets only for the `aromatic_organic` symbols.
pub(super) fn is_bare_aromatic(element: Element) -> bool {
    aromatic_form(element).is_some_and(|symbol| symbol.bare)
}
