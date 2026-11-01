//! SMILES text reading and writing over the recorded chemical graph.
//!
//! This is Chemifree's own bounded reader and writer. Grammar decisions cite the public
//! [OpenSMILES specification](https://opensmiles.github.io/opensmiles/opensmiles.html) by
//! section; the earlier Daylight theory manual is the reference for the organic subset's
//! implicit-hydrogen rule. What is not implemented is refused with a specific code, never
//! guessed: stereochemistry (`@`, `/`, `\`), query atoms (`*`), reactions (`>`), atom classes,
//! quadruple bonds, isotope 0 and extended `%(nnn)` ring labels.
//!
//! The canonical form is Chemifree's own: it is the SMILES spelling of the canonical labelled
//! graph of [`crate::identity::canonicalize_chemical`] (`chemifree-chemical-graph-2`), so it is
//! stable under atom permutation and under Kekulé/aromatic spelling for every system
//! `chemifree-aromaticity-2` normalizes, and it is not compatible with any toolkit's
//! canonical SMILES.

mod error;
mod graph_build;
mod parse;
mod radical;
mod write;

pub use error::{ErrorClass, ErrorCode, SmilesError, Span, Subject};
pub use parse::{RadicalNote, Read, ReadAtom, ReadBond, read};
pub use write::{Written, write_canonical, write_recorded};

use crate::identity;

/// The most bytes of input the reader accepts, including any text after the terminator.
pub const MAX_INPUT_BYTES: usize = 4096;
/// The deepest nesting of parentheses the reader accepts.
pub const MAX_BRANCH_DEPTH: usize = 64;
/// Ring labels are `0`-`9` and `%10`-`%99`; at most this many rings are open at once.
pub const MAX_OPEN_RINGS: usize = 100;
/// OpenSMILES bracket atoms: an isotope has at most three digits.
pub const MAX_ISOTOPE_DIGITS: usize = 3;
/// The canonical implementation, bound to [`CANONICAL_GRAPH_MODEL`].
pub const CANONICAL_MODEL_ID: &str = "chemifree-smiles-canonical-1";
/// The recorded-order writer.
pub const RECORDED_MODEL_ID: &str = "chemifree-smiles-recorded-1";
/// The graph identity model the canonical form is computed from.
pub const CANONICAL_GRAPH_MODEL: &str = identity::CHEMICAL_MODEL_ID;

#[cfg(test)]
mod tests;
