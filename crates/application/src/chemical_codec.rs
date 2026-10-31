//! Application-owned boundary for chemical interchange formats.
//!
//! A provider parses all records before a document is created. The graph uses domain atoms and
//! bonds, while coordinates are already in scene points. SDF fields remain separate because
//! the current document storage has no field collection.

use chemifree_domain::document::{Atom, BondOrder, Document, Point};
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::operations::Loss;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChemicalFormat {
    Mol,
    Sdf,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DataField {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChemicalAtom {
    pub atom: Atom,
    pub position: Point,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChemicalBond {
    /// Zero-based indices into the record's atoms.
    pub begin: usize,
    pub end: usize,
    pub order: BondOrder,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChemicalRecord {
    pub title: String,
    pub atoms: Vec<ChemicalAtom>,
    pub bonds: Vec<ChemicalBond>,
    pub fields: Vec<DataField>,
    pub losses: Vec<Loss>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WrittenChemical {
    pub text: String,
    pub losses: Vec<Loss>,
}

/// A recoverable format error. Line and record are one-based when known.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodecError {
    pub code: &'static str,
    pub message: String,
    pub record: Option<usize>,
    pub line: Option<usize>,
    pub field: Option<String>,
}

impl fmt::Display for CodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CodecError {}

pub trait ChemicalCodec: Send + Sync {
    /// Parses and checks the complete input before returning any records.
    fn read(&self, format: ChemicalFormat, text: &str) -> Result<Vec<ChemicalRecord>, CodecError>;

    /// Writes one document. Losses identify every unsupported semantic change.
    fn write(
        &self,
        format: ChemicalFormat,
        document: &Document,
        fields: &[DataField],
    ) -> Result<WrittenChemical, CodecError>;
}
