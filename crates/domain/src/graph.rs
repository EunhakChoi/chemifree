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

    /// A graph assembled from parts, for a reader that has no document. The caller supplies
    /// distinct IDs and bonds between listed atoms, as a document guarantees.
    pub fn from_parts(atoms: Vec<(AtomId, Atom)>, bonds: Vec<(BondId, Bond)>) -> Self {
        Self { atoms, bonds }
    }

    /// The atoms for editing the copy, such as setting a state found after reading: a graph
    /// snapshot never feeds back into a document.
    pub fn atoms_mut(&mut self) -> impl Iterator<Item = (AtomId, &mut Atom)> {
        self.atoms.iter_mut().map(|(id, atom)| (*id, atom))
    }

    /// The bonds for editing the copy; see [`MoleculeGraph::atoms_mut`].
    pub fn bonds_mut(&mut self) -> impl Iterator<Item = (BondId, &mut Bond)> {
        self.bonds.iter_mut().map(|(id, bond)| (*id, bond))
    }

    pub fn atoms(&self) -> impl ExactSizeIterator<Item = (AtomId, &Atom)> {
        self.atoms.iter().map(|(id, atom)| (*id, atom))
    }

    pub fn bonds(&self) -> impl ExactSizeIterator<Item = (BondId, &Bond)> {
        self.bonds.iter().map(|(id, bond)| (*id, bond))
    }
}
