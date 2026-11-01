//! The SMILES codec: a reader, a recorded-order writer and a canonical writer.
//!
//! Stereochemistry, query atoms, reactions, atom classes and InChI are not implemented. The
//! canonical form is Chemifree's own (`chemifree-smiles-canonical-1`, bound to
//! `chemifree-chemical-graph-2`) and is not compatible with any toolkit's canonical SMILES.

mod error;
mod read;
mod resolve;
mod tables;
mod write;

use chemifree_application::smiles_codec::{
    SmilesCodec, SmilesError, SmilesLimits, SmilesRead, SmilesStyle, SmilesWrite,
};
use chemifree_domain::document::MAX_COMMANDS as DOCUMENT_COMMANDS;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::identity;

pub use error::Code;

/// The most bytes of SMILES, up to its whitespace terminator, either way.
pub const MAX_INPUT_BYTES: usize = 4096;
/// The most bytes of text after the terminator that a read accepts (reported as ignored).
pub const MAX_TRAILING_BYTES: usize = 65_536;
/// Atoms plus bonds one string may create: the document transaction limit.
pub const MAX_COMMANDS: usize = DOCUMENT_COMMANDS;
pub const MAX_BRANCH_DEPTH: usize = 100;
/// Ring closures open at once, in the reader and the writers. The reader reads labels `0`
/// to `99`, the writers use `1` to `99`; neither has more than this many open.
pub const MAX_OPEN_RINGS: usize = 99;

/// The independently implemented SMILES codec.
#[derive(Clone, Copy, Debug, Default)]
pub struct LocalSmilesCodec;

impl SmilesCodec for LocalSmilesCodec {
    fn limits(&self) -> SmilesLimits {
        SmilesLimits {
            max_input_bytes: MAX_INPUT_BYTES,
            max_commands: MAX_COMMANDS,
            max_branch_depth: MAX_BRANCH_DEPTH,
            max_open_rings: MAX_OPEN_RINGS,
            canonical_max_atoms: identity::MAX_ATOMS,
        }
    }

    fn read(&self, text: &str) -> Result<SmilesRead, SmilesError> {
        read::read(text).map_err(SmilesError::from)
    }

    fn write(&self, graph: &MoleculeGraph, style: SmilesStyle) -> Result<SmilesWrite, SmilesError> {
        write::write(graph, style).map_err(SmilesError::from)
    }
}
