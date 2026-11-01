//! The symbol and whitespace tables that the SMILES reader and writer share, so that one never
//! accepts or emits what the other does not know.
//!
//! Sections cited are those of the OpenSMILES specification
//! (<https://opensmiles.github.io/opensmiles/opensmiles.html>): "Formal Grammar" for the
//! organic subset and aromatic symbols, "Parsing Termination" for whitespace.

/// Characters that end a SMILES string (Parsing Termination).
pub(super) const fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

/// The aliphatic organic subset that may be written without brackets (Organic Subset).
/// `*` is the wildcard and is refused elsewhere.
pub(super) const BARE_ORGANIC: [&str; 10] = ["B", "C", "N", "O", "P", "S", "F", "Cl", "Br", "I"];

/// Lower-case aromatic symbols and the element each names (Formal Grammar,
/// `aromatic_symbols`). The first six are the aromatic organic subset and may be written
/// without brackets.
pub(super) const AROMATIC: [(&str, &str); 8] = [
    ("b", "B"),
    ("c", "C"),
    ("n", "N"),
    ("o", "O"),
    ("p", "P"),
    ("s", "S"),
    ("se", "Se"),
    ("as", "As"),
];

/// How many of [`AROMATIC`] are in the bare (organic subset) form.
const BARE_AROMATIC: usize = 6;

/// Whether the element may be written as an organic-subset atom.
pub(super) fn is_bare_organic(symbol: &str) -> bool {
    BARE_ORGANIC.contains(&symbol)
}

/// The lower-case aromatic symbol of an element, if it has one.
pub(super) fn aromatic_symbol(element: &str) -> Option<&'static str> {
    AROMATIC
        .iter()
        .find(|(_, name)| *name == element)
        .map(|(lower, _)| *lower)
}

/// The element a lower-case aromatic symbol names.
pub(super) fn aromatic_element(lower: &str) -> Option<&'static str> {
    AROMATIC
        .iter()
        .find(|(symbol, _)| *symbol == lower)
        .map(|(_, element)| *element)
}

/// The longest organic-subset symbol that `text` starts with.
pub(super) fn bare_organic_prefix(text: &str) -> Option<&'static str> {
    BARE_ORGANIC
        .iter()
        .copied()
        .filter(|symbol| text.starts_with(symbol))
        .max_by_key(|symbol| symbol.len())
}

/// The bare aromatic symbol that `text` starts with, as `(lower-case symbol, element)`.
pub(super) fn bare_aromatic_prefix(text: &str) -> Option<(&'static str, &'static str)> {
    AROMATIC[..BARE_AROMATIC]
        .iter()
        .copied()
        .find(|(symbol, _)| text.starts_with(symbol))
}

/// Whether the lower-case aromatic symbol may be written without brackets.
pub(super) fn is_bare_aromatic(lower: &str) -> bool {
    AROMATIC[..BARE_AROMATIC]
        .iter()
        .any(|(symbol, _)| *symbol == lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_tables_agree_with_each_other() {
        for (lower, element) in AROMATIC {
            assert_eq!(aromatic_symbol(element), Some(lower));
            assert_eq!(aromatic_element(lower), Some(element));
        }
        for (lower, element) in &AROMATIC[..BARE_AROMATIC] {
            assert!(is_bare_aromatic(lower));
            assert!(
                is_bare_organic(element),
                "{element} is bare aromatic but not bare"
            );
        }
        assert!(!is_bare_aromatic("se") && !is_bare_aromatic("as"));
        assert_eq!(bare_organic_prefix("Cl"), Some("Cl"));
        assert_eq!(bare_organic_prefix("Ca"), Some("C"));
        assert_eq!(bare_organic_prefix("Brc"), Some("Br"));
        assert_eq!(bare_organic_prefix("Na"), None);
        assert_eq!(bare_aromatic_prefix("cc"), Some(("c", "C")));
        // "se" is only bracketed: bare, the s is read and the e is then unexpected.
        assert_eq!(bare_aromatic_prefix("se"), Some(("s", "S")));
        assert!(BARE_ORGANIC.iter().all(|s| !s.is_empty()));
    }

    #[test]
    fn whitespace_is_the_four_terminators() {
        let found: Vec<u8> = (0..=255).filter(|b| is_whitespace(*b)).collect();
        assert_eq!(found, vec![9, 10, 13, 32]);
    }
}
