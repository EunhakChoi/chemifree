//! Shared helpers of the SMILES test targets: graphs from reads, seeded permutation and a
//! small deterministic random generator.
#![allow(dead_code, reason = "each test target uses a different subset")]

use chemifree_application::smiles_codec::{SmilesCodec, SmilesError, SmilesRead, SmilesWrite};
use chemifree_domain::document::{Atom, AtomId, Bond, BondId};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_providers::smiles::LocalSmilesCodec;

pub const CODEC: LocalSmilesCodec = LocalSmilesCodec;

pub fn read(text: &str) -> Result<SmilesRead, SmilesError> {
    CODEC.read(text)
}

pub fn atom_id(index: usize) -> AtomId {
    AtomId::from_serial(u32::try_from(index + 1).unwrap()).unwrap()
}

pub fn bond_id(index: usize) -> BondId {
    BondId::from_serial(u32::try_from(index + 1).unwrap()).unwrap()
}

pub fn graph(read: &SmilesRead) -> MoleculeGraph {
    permuted(
        read,
        &(0..read.atoms.len()).collect::<Vec<_>>(),
        &mut Rng(1),
        false,
    )
}

/// The read's graph with atom `order[i]` placed at position `i`, bonds shuffled and, when
/// `flip`, endpoints swapped at random. The graph is the same molecule.
pub fn permuted(read: &SmilesRead, order: &[usize], rng: &mut Rng, flip: bool) -> MoleculeGraph {
    let mut new_index = vec![0; order.len()];
    for (position, &old) in order.iter().enumerate() {
        new_index[old] = position;
    }
    let atoms: Vec<(AtomId, Atom)> = order
        .iter()
        .enumerate()
        .map(|(position, &old)| (atom_id(position), read.atoms[old].atom.clone()))
        .collect();
    let mut bonds: Vec<&chemifree_application::smiles_codec::SmilesBond> =
        read.bonds.iter().collect();
    rng.shuffle(&mut bonds);
    let bonds = bonds
        .into_iter()
        .enumerate()
        .map(|(i, bond)| {
            let (mut a, mut b) = (new_index[bond.begin], new_index[bond.end]);
            if flip && rng.next().is_multiple_of(2) {
                std::mem::swap(&mut a, &mut b);
            }
            (
                bond_id(i),
                Bond {
                    begin: atom_id(a),
                    end: atom_id(b),
                    order: bond.order.clone(),
                },
            )
        })
        .collect();
    MoleculeGraph::from_parts(atoms, bonds)
}

pub fn write(
    graph: &MoleculeGraph,
    style: chemifree_application::smiles_codec::SmilesStyle,
) -> Result<SmilesWrite, SmilesError> {
    CODEC.write(graph, style)
}

/// xorshift64*: deterministic, so a failing seed reproduces.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % bound as u64).unwrap()
    }

    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i + 1);
            items.swap(i, j);
        }
    }
}
