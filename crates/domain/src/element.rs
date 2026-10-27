//! Chemical element identity by atomic number. No valence or mass data is implied.

use std::fmt;

/// IUPAC element symbols ordered by atomic number, 1 (H) through 118 (Og).
const SYMBOLS: [&str; 118] = [
    "H", "He", "Li", "Be", "B", "C", "N", "O", "F", "Ne", "Na", "Mg", "Al", "Si", "P", "S", "Cl",
    "Ar", "K", "Ca", "Sc", "Ti", "V", "Cr", "Mn", "Fe", "Co", "Ni", "Cu", "Zn", "Ga", "Ge", "As",
    "Se", "Br", "Kr", "Rb", "Sr", "Y", "Zr", "Nb", "Mo", "Tc", "Ru", "Rh", "Pd", "Ag", "Cd", "In",
    "Sn", "Sb", "Te", "I", "Xe", "Cs", "Ba", "La", "Ce", "Pr", "Nd", "Pm", "Sm", "Eu", "Gd", "Tb",
    "Dy", "Ho", "Er", "Tm", "Yb", "Lu", "Hf", "Ta", "W", "Re", "Os", "Ir", "Pt", "Au", "Hg", "Tl",
    "Pb", "Bi", "Po", "At", "Rn", "Fr", "Ra", "Ac", "Th", "Pa", "U", "Np", "Pu", "Am", "Cm", "Bk",
    "Cf", "Es", "Fm", "Md", "No", "Lr", "Rf", "Db", "Sg", "Bh", "Hs", "Mt", "Ds", "Rg", "Cn", "Nh",
    "Fl", "Mc", "Lv", "Ts", "Og",
];

/// A chemical element. Only identity is modeled; properties are not computed.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Element(u8);

impl Element {
    pub const CARBON: Self = Self(6);

    /// Accepts an exact, case-sensitive IUPAC symbol such as `C`, `Cl`, or `Og`.
    pub fn from_symbol(symbol: &str) -> Option<Self> {
        SYMBOLS
            .iter()
            .position(|candidate| *candidate == symbol)
            .and_then(|index| u8::try_from(index + 1).ok())
            .map(Self)
    }

    pub fn from_atomic_number(number: u8) -> Option<Self> {
        (1..=118).contains(&number).then_some(Self(number))
    }

    pub const fn atomic_number(self) -> u8 {
        self.0
    }

    pub fn symbol(self) -> &'static str {
        SYMBOLS[usize::from(self.0) - 1]
    }
}

impl fmt::Display for Element {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.symbol())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_round_trip_through_atomic_numbers() {
        for number in 1..=118 {
            let element = Element::from_atomic_number(number).unwrap();
            assert_eq!(Element::from_symbol(element.symbol()), Some(element));
        }
        assert_eq!(Element::from_symbol("C"), Some(Element::CARBON));
        assert_eq!(Element::from_symbol("Cl").unwrap().atomic_number(), 17);
        assert_eq!(Element::from_symbol("Og").unwrap().atomic_number(), 118);
    }

    #[test]
    fn unknown_or_miscased_symbols_are_rejected() {
        for symbol in ["", "c", "CL", "Xx", "Uue", " C"] {
            assert_eq!(Element::from_symbol(symbol), None, "{symbol:?}");
        }
        assert_eq!(Element::from_atomic_number(0), None);
        assert_eq!(Element::from_atomic_number(119), None);
    }
}
