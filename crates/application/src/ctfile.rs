//! Application-owned boundary for the bounded V2000 MOL and SDF codec.
//!
//! A record is independent of the editor: in particular it can carry a third coordinate,
//! header lines and SD fields. Conversion to a drawing is a separate, loss-reporting step.

use serde::{Deserialize, Serialize};

use crate::Loss;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CtFormat {
    Mol,
    Sdf,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CtRecord {
    pub title: String,
    pub program: String,
    pub comment: String,
    pub atoms: Vec<CtAtom>,
    pub bonds: Vec<CtBond>,
    /// Ordered; duplicate names are retained rather than overwritten.
    pub fields: Vec<CtField>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CtAtom {
    pub element: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub charge: i8,
    pub isotope: Option<u16>,
    pub radical: CtRadical,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CtRadical {
    None,
    Singlet,
    Doublet,
    Triplet,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CtBond {
    /// Zero-based indices into `atoms`.
    pub begin: usize,
    pub end: usize,
    /// Single, double, or triple; aromatic and query bonds are outside this profile.
    pub order: u8,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CtField {
    pub name: String,
    /// Line breaks are represented by `\n`; a blank value is the empty string.
    pub value: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CtResult<T> {
    pub value: T,
    pub losses: Vec<Loss>,
}

/// A failed conversion, associated with a one-based record and (where known) input line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CtError {
    pub code: &'static str,
    pub message: String,
    pub record: usize,
    pub line: Option<usize>,
}

/// Implemented outside the application; implementations must stage all output on failure.
pub trait CtCodec: Send + Sync {
    fn read(&self, format: CtFormat, input: &[u8]) -> Result<CtResult<Vec<CtRecord>>, CtError>;
    fn write(&self, format: CtFormat, records: &[CtRecord]) -> Result<CtResult<String>, CtError>;
}
