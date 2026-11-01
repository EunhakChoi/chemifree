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

    /// A copy whose listed atoms carry `hydrogens`; used to re-read a model under an
    /// alternative hydrogen hypothesis without touching the original snapshot.
    pub(crate) fn with_hydrogens(
        &self,
        atoms: &std::collections::BTreeSet<AtomId>,
        hydrogens: crate::states::Hydrogens,
    ) -> Self {
        let mut copy = self.clone();
        for (id, atom) in &mut copy.atoms {
            if atoms.contains(id) {
                atom.hydrogens = hydrogens;
            }
        }
        copy
    }

    pub fn atoms(&self) -> impl ExactSizeIterator<Item = (AtomId, &Atom)> {
        self.atoms.iter().map(|(id, atom)| (*id, atom))
    }

    pub fn bonds(&self) -> impl ExactSizeIterator<Item = (BondId, &Bond)> {
        self.bonds.iter().map(|(id, bond)| (*id, bond))
    }
}
