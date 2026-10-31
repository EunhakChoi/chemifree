//! An owned chemical graph snapshot for bounded read-only analysis outside session locks.
//! IDs are document-scoped and remain separate from any chemical canonical numbering.

use crate::document::{Atom, AtomId, Bond, BondId, Document};

#[derive(Clone, Debug, PartialEq)]
pub struct MoleculeGraph {
    atoms: Vec<(AtomId, Atom)>,
    bonds: Vec<(BondId, Bond)>,
}

impl MoleculeGraph {
    /// Copies only graph state, not scene, settings, selection, or document history.
    pub fn from_document(document: &Document) -> Self {
        Self {
            atoms: document
                .atoms()
                .map(|(id, atom)| (id, atom.clone()))
                .collect(),
            bonds: document
                .bonds()
                .map(|(id, bond)| (id, bond.clone()))
                .collect(),
        }
    }

    pub fn atoms(&self) -> impl ExactSizeIterator<Item = (AtomId, &Atom)> {
        self.atoms.iter().map(|(id, atom)| (*id, atom))
    }

    pub fn bonds(&self) -> impl ExactSizeIterator<Item = (BondId, &Bond)> {
        self.bonds.iter().map(|(id, bond)| (*id, bond))
    }
}
