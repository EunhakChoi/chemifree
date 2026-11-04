//! Tetrahedral marks (`@`, `@@`) of bracket atoms: the neighbour bookkeeping done while the text
//! is scanned and the post-pass that turns it into configurations.
//!
//! Rules are those of the OpenSMILES specification, section "Tetrahedral Centers" (3.8.2,
//! <https://opensmiles.github.io/opensmiles/opensmiles.html>): the neighbours of a marked atom are
//! taken in the order the text writes them, looking from the first toward the center, and `@`
//! lists the remaining three anticlockwise, `@@` clockwise. The preceding atom comes first; a
//! hydrogen written as the bracket's `H` follows it (and comes first when the atom starts a
//! component); a ring-closure neighbour stands where the ring digit is written at this atom, in
//! the order of the digits, before the branches and the chain that follow.
//!
//! Not read: a mark on an atom with a lone pair in place of the fourth neighbour (the
//! specification text consulted does not define that position), and the `@TH`, `@AL`, `@SP`,
//! `@TB` and `@OH` classes.

use std::collections::HashMap;

use chemifree_application::smiles::{SmilesCenter, SmilesCode, SmilesError, SmilesSlot, Span};
use chemifree_domain::chemistry;
use chemifree_domain::document::{Atom, AtomId, BondOrder};
use chemifree_domain::states::Hydrogens;
use chemifree_domain::stereo::Turn;
use chemifree_domain::stereo_drawing::fourth_position;

/// The neighbour list of every atom, in text order, and the marks the atoms carry.
#[derive(Default)]
pub(super) struct Neighborhoods {
    /// `None` is a ring label opened here whose closing atom has not been read yet.
    links: Vec<Vec<Option<usize>>>,
    preceded: Vec<bool>,
    marks: Vec<Option<(Turn, Span)>>,
}

impl Neighborhoods {
    pub(super) fn add_atom(&mut self, mark: Option<(Turn, Span)>) {
        self.links.push(Vec::new());
        self.preceded.push(false);
        self.marks.push(mark);
    }

    /// `to` was just added and follows `from` in the chain or a branch.
    pub(super) fn chain_bond(&mut self, from: usize, to: usize) {
        self.links[from].push(Some(to));
        self.links[to].push(Some(from));
        self.preceded[to] = true;
    }

    /// A ring label written at `atom` that no atom has closed: returns the slot to fill later.
    pub(super) fn open_ring(&mut self, atom: usize) -> usize {
        self.links[atom].push(None);
        self.links[atom].len() - 1
    }

    /// The ring label of `opener`'s `slot` is closed by `atom`, at the position the label is
    /// written at `atom`.
    pub(super) fn close_ring(&mut self, atom: usize, opener: usize, slot: usize) {
        self.links[atom].push(Some(opener));
        self.links[opener][slot] = Some(atom);
    }

    /// The configuration each marked atom states. `atoms` are the finished atoms (radicals
    /// decided) and `bonds` every bond as `(begin, end, order)`.
    pub(super) fn centers(
        &self,
        atoms: &[&Atom],
        bonds: &[(usize, usize, &BondOrder)],
    ) -> Result<Vec<SmilesCenter>, SmilesError> {
        let mut centers = Vec::new();
        // Each bond by its two atoms, once, so a marked atom finds a neighbour's bond directly.
        let by_pair: HashMap<(usize, usize), &BondOrder> = bonds
            .iter()
            .map(|&(begin, end, order)| ((begin.min(end), begin.max(end)), order))
            .collect();
        let order_between =
            |one: usize, other: usize| by_pair.get(&(one.min(other), one.max(other))).copied();
        for (index, mark) in self.marks.iter().enumerate() {
            let Some((turn, span)) = *mark else { continue };
            let refuse = |code| SmilesError::at(code, span);
            let atom = atoms[index];
            // The scan refuses an unclosed ring label, so no slot is still open here.
            let neighbors: Vec<usize> = self.links[index].iter().copied().flatten().collect();
            let hydrogens = match atom.hydrogens {
                Hydrogens::Specified(count) => count,
                _ => 0,
            };
            if neighbors
                .iter()
                .any(|&other| order_between(index, other) == Some(&BondOrder::Aromatic))
            {
                return Err(refuse(SmilesCode::StereoCenterUnsupportedHere)
                    .because("an aromatic atom is not a tetrahedral center"));
            }
            match (neighbors.len(), hydrogens) {
                (4, 0) | (3, 1) => {}
                (3, 0) => {
                    return Err(refuse(SmilesCode::StereoCenterUnsupportedHere).because(
                        "three bonds and no hydrogen leave a lone pair, which is not read",
                    ));
                }
                (bonds, hydrogens) => {
                    return Err(refuse(SmilesCode::StereoNeighborCount)
                        .because(format!("{bonds} bonds and {hydrogens} hydrogens")));
                }
            }
            let orders: Vec<Option<u16>> = neighbors
                .iter()
                .map(|&other| order_between(index, other).and_then(chemistry::bond_order_value))
                .collect();
            // The id only labels the valence assessment; an index too large for one is a bound.
            let id = u32::try_from(index + 1)
                .ok()
                .and_then(AtomId::from_serial)
                .ok_or_else(|| refuse(SmilesCode::CommandLimit))?;
            // With the counts above settled, the valence model either names the one fourth
            // position (the hydrogen) or finds no tetrahedral neighbour set at all.
            if fourth_position(id, atom, &orders).is_err() {
                return Err(refuse(SmilesCode::StereoCenterUnsupportedHere)
                    .because("the valence model gives this atom no tetrahedral neighbour set"));
            }
            let mut slots: Vec<SmilesSlot> = neighbors.into_iter().map(SmilesSlot::Atom).collect();
            if hydrogens == 1 {
                slots.insert(
                    usize::from(self.preceded[index]),
                    SmilesSlot::ImplicitHydrogen,
                );
            }
            let slots: [SmilesSlot; 4] = slots
                .try_into()
                .map_err(|_| refuse(SmilesCode::StereoNeighborCount))?;
            centers.push(SmilesCenter {
                atom: index,
                slots,
                turn,
                span,
            });
        }
        Ok(centers)
    }
}
