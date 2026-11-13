//! An independently written SMILES reader and writers over the shared valence, aromaticity and
//! chemical identity models.
//!
//! Reading and writing follow the OpenSMILES specification
//! (<https://opensmiles.github.io/opensmiles/opensmiles.html>) for the subset the drawing can
//! hold. The reader reads tetrahedral marks (`@`, `@@`) of bracket atoms, the enhanced stereo
//! group fields of a CXSMILES block (`a:`, `o<n>:`, `&<n>:`) and the directional bonds `/` and
//! `\` as double-bond configurations; other mark classes and every other CX field are refused.
//! The recorded writer preserves supported tetrahedral configurations, stored double-bond
//! configurations (as `/` and `\`, see `double_bond_write`) and enhanced stereo groups,
//! reporting the stereo it cannot place as losses; canonical writing remains
//! stereo-insensitive. Query atoms, reaction SMILES
//! and InChI are not implemented. The canonical form is Chemifree's own and is
//! not the canonical form of any other toolkit.

mod analysis;
mod cx;
mod double_bond_write;
mod double_bonds;
mod emit;
mod reader;
mod stereo;
mod tables;
mod writer;

use chemifree_application::smiles::{
    SmilesCodec, SmilesError, SmilesLimits, SmilesRead, SmilesStyle, SmilesWrite,
};
use chemifree_domain::budget::{Interrupt, Interrupted};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::stereo_group::StereoGroupTable;

/// The longest input the reader accepts, in UTF-8 bytes, and the longest text a writer returns.
pub const MAX_INPUT_BYTES: usize = 16 * 1024;
/// The deepest nesting of branches.
pub const MAX_BRANCH_DEPTH: usize = 64;
/// Ring labels run from 0 to this number (`%99`); the writer uses 1 to this number.
pub const MAX_RING_LABEL: usize = 99;

/// The OpenSMILES-subset reader and its recorded and canonical writers.
#[derive(Clone, Copy, Debug, Default)]
pub struct OpenSmilesCodec;

impl SmilesCodec for OpenSmilesCodec {
    fn limits(&self) -> SmilesLimits {
        SmilesLimits {
            max_input_bytes: MAX_INPUT_BYTES,
            max_branch_depth: MAX_BRANCH_DEPTH,
        }
    }

    fn read(&self, input: &str) -> Result<SmilesRead, SmilesError> {
        reader::parse(input).map(|parsed| parsed.read)
    }

    fn write(&self, graph: &MoleculeGraph, style: SmilesStyle) -> Result<SmilesWrite, SmilesError> {
        writer::write(graph, style, &StereoGroupTable::default())
    }
    fn write_with_groups(
        &self,
        graph: &MoleculeGraph,
        style: SmilesStyle,
        groups: &StereoGroupTable,
    ) -> Result<SmilesWrite, SmilesError> {
        writer::write(graph, style, groups)
    }

    fn write_interruptible(
        &self,
        graph: &MoleculeGraph,
        style: SmilesStyle,
        groups: &StereoGroupTable,
        interrupt: &dyn Interrupt,
    ) -> Result<Result<SmilesWrite, SmilesError>, Interrupted> {
        writer::write_with(graph, style, groups, interrupt)
    }
}

type Fail = SmilesError;

/// Why a write produced no text: the writer refused the drawing, or `interrupt` stopped it.
#[derive(Debug)]
pub(super) enum Halt {
    /// The writer refused the drawing; this does not depend on the machine.
    Refused(Fail),
    /// The write was stopped before it finished; the drawing may well be writable.
    Interrupted(Interrupted),
}

impl From<Fail> for Halt {
    fn from(error: Fail) -> Self {
        Self::Refused(error)
    }
}

impl From<Interrupted> for Halt {
    fn from(interrupted: Interrupted) -> Self {
        Self::Interrupted(interrupted)
    }
}

/// Keeps a refusal inside the result and reports a stop outside it, so a stop is never read as a
/// refusal.
pub(super) fn interruptible<T>(result: Result<T, Halt>) -> Result<Result<T, Fail>, Interrupted> {
    match result {
        Ok(value) => Ok(Ok(value)),
        Err(Halt::Refused(error)) => Ok(Err(error)),
        Err(Halt::Interrupted(interrupted)) => Err(interrupted),
    }
}

/// Maps the result of a write given [`NEVER`](chemifree_domain::budget::NEVER), which cannot
/// stop it. A stop would be a bug, so it panics rather than being read as a refusal.
pub(super) fn uninterrupted<T>(result: Result<T, Halt>) -> Result<T, Fail> {
    match result {
        Ok(value) => Ok(value),
        Err(Halt::Refused(error)) => Err(error),
        Err(Halt::Interrupted(interrupted)) => {
            unreachable!("the never-interrupt cannot interrupt: {interrupted:?}")
        }
    }
}
