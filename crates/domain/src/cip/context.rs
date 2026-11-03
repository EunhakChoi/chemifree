//! The facts of one graph that the digraph reads, looked up once per request.

use std::collections::{BTreeMap, BTreeSet};

use crate::aromaticity::{self, Classification};
use crate::chemistry::{self, Finding};
use crate::document::{Atom, AtomId, Bond, BondId, BondOrder};
use crate::element::Element;
use crate::graph::MoleculeGraph;
use crate::states::{Hydrogens, Radical};

use super::Reason;

/// The mass number of an atom, as Rule 2 reads it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Mass {
    Number(u16),
    /// An unlabeled atom of an element outside [`natural_mass_number`]'s table: it can only be
    /// compared with another such atom.
    NaturalUnlisted,
}

/// The mass number of the most abundant natural isotope, for the elements the valence model
/// lists plus Si and Se. Public IUPAC/NIST values; a Chemifree choice for Rule 2, not a CIP
/// statement about "natural abundance".
const fn natural_mass_number(atomic_number: u8) -> Option<u16> {
    Some(match atomic_number {
        1 => 1,
        3 => 7,
        5 => 11,
        6 => 12,
        7 => 14,
        8 => 16,
        9 => 19,
        11 => 23,
        12 => 24,
        14 => 28,
        15 => 31,
        16 => 32,
        17 => 35,
        19 => 39,
        20 => 40,
        34 => 80,
        35 => 79,
        53 => 127,
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AtomClass {
    /// A ring atom of an all-carbon neutral aromatic system, read as mancude.
    Mancude,
    /// A ring atom of any other aromatic system, or of one the aromaticity model cannot
    /// classify: it is refused as soon as it must be expanded.
    Refused,
}

/// What a bond contributes to the digraph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Step {
    /// A bond inside a mancude system: no duplicate atoms, whatever order is recorded.
    Mancude,
    /// A single, double or triple bond.
    Order(u8),
}

pub(super) struct Context<'g> {
    pub(super) graph: &'g MoleculeGraph,
    atoms: BTreeMap<AtomId, &'g Atom>,
    bonds: BTreeMap<BondId, &'g Bond>,
    incident: BTreeMap<AtomId, Vec<(AtomId, BondId)>>,
    classes: BTreeMap<AtomId, AtomClass>,
    mancude_bonds: BTreeSet<BondId>,
}

impl<'g> Context<'g> {
    pub(super) fn new(graph: &'g MoleculeGraph) -> Self {
        let atoms: BTreeMap<_, _> = graph.atoms().collect();
        let bonds: BTreeMap<_, _> = graph.bonds().collect();
        let mut incident: BTreeMap<AtomId, Vec<(AtomId, BondId)>> =
            atoms.keys().map(|&id| (id, Vec::new())).collect();
        for (&id, bond) in &bonds {
            if let Some(list) = incident.get_mut(&bond.begin) {
                list.push((bond.end, id));
            }
            if let Some(list) = incident.get_mut(&bond.end) {
                list.push((bond.begin, id));
            }
        }
        for list in incident.values_mut() {
            list.sort_unstable();
        }
        let mut classes = BTreeMap::new();
        let mut mancude_bonds = BTreeSet::new();
        let interpretation = aromaticity::perceive(graph);
        for system in &interpretation.systems {
            let supported = system.classification == Classification::Aromatic
                && system.atoms.iter().all(|id| {
                    atoms.get(id).is_some_and(|atom| {
                        atom.kind.element() == Some(Element::CARBON) && atom.charge == 0
                    })
                });
            // A system the aromaticity model cannot classify is refused only when it holds a
            // ring double or aromatic bond: with none, no mancude reading is possible and the
            // ring is read as drawn (a ring ketone, a cyclobutane with an exocyclic alkene).
            let could_be_mancude = system.bonds.iter().any(|id| {
                bonds.get(id).is_some_and(|bond| {
                    matches!(bond.order, BondOrder::Double | BondOrder::Aromatic)
                })
            });
            let unsupported_system = (system.classification == Classification::Unsupported
                && could_be_mancude)
                || (system.classification == Classification::Aromatic && !supported);
            for &id in &system.atoms {
                if unsupported_system {
                    classes.insert(id, AtomClass::Refused);
                } else if supported {
                    classes.entry(id).or_insert(AtomClass::Mancude);
                }
            }
            if supported {
                mancude_bonds.extend(system.bonds.iter().copied());
            }
        }
        Self {
            graph,
            atoms,
            bonds,
            incident,
            classes,
            mancude_bonds,
        }
    }

    pub(super) fn atom(&self, id: AtomId) -> Option<&'g Atom> {
        self.atoms.get(&id).copied()
    }

    /// The bonded neighbors of `id` with the bond to each, ascending by neighbor.
    pub(super) fn neighbors(&self, id: AtomId) -> &[(AtomId, BondId)] {
        self.incident.get(&id).map_or(&[], Vec::as_slice)
    }

    pub(super) fn bond(&self, id: BondId) -> Option<&'g Bond> {
        self.bonds.get(&id).copied()
    }

    pub(super) fn class(&self, id: AtomId) -> Option<AtomClass> {
        self.classes.get(&id).copied()
    }

    /// The bond between two atoms, if they are bonded.
    pub(super) fn bond_between(&self, a: AtomId, b: AtomId) -> Option<BondId> {
        self.neighbors(a)
            .iter()
            .find_map(|&(neighbor, bond)| (neighbor == b).then_some(bond))
    }

    pub(super) fn bond_step(&self, id: BondId) -> Result<Step, Reason> {
        if self.mancude_bonds.contains(&id) {
            return Ok(Step::Mancude);
        }
        match self.bond(id).map(|bond| &bond.order) {
            Some(BondOrder::Single) => Ok(Step::Order(1)),
            Some(BondOrder::Double) => Ok(Step::Order(2)),
            Some(BondOrder::Triple) => Ok(Step::Order(3)),
            Some(BondOrder::Aromatic) => Err(Reason::AromaticSystem),
            _ => Err(Reason::BondKind),
        }
    }

    /// The element of `id`, or the reason it has none.
    pub(super) fn element(&self, id: AtomId) -> Result<Element, Reason> {
        self.atom(id)
            .and_then(|atom| atom.kind.element())
            .ok_or(Reason::QueryAtom)
    }

    pub(super) fn mass(&self, id: AtomId) -> Result<Mass, Reason> {
        let element = self.element(id)?;
        let atom = self.atom(id).ok_or(Reason::QueryAtom)?;
        Ok(match atom.isotope {
            Some(number) => Mass::Number(number),
            None => natural_mass_number(element.atomic_number())
                .map_or(Mass::NaturalUnlisted, Mass::Number),
        })
    }

    /// The hydrogens a ligand atom carries besides its drawn atoms, for the atom to be
    /// expanded: the valence model's count, or the reason it cannot be given.
    pub(super) fn hydrogens(&self, id: AtomId) -> Result<u16, Reason> {
        let atom = self.atom(id).ok_or(Reason::QueryAtom)?;
        if atom.kind.element().is_none() {
            return Err(Reason::QueryAtom);
        }
        if !atom.attachment_points.is_empty() {
            return Err(Reason::AttachmentPoint);
        }
        let class = self.class(id);
        if class == Some(AtomClass::Refused) {
            return Err(Reason::AromaticSystem);
        }
        let mut orders = Vec::new();
        let mut mancude = 0_u16;
        for &(_, bond) in self.neighbors(id) {
            match self.bond_step(bond)? {
                Step::Mancude => mancude += 1,
                Step::Order(order) => orders.push(Some(u16::from(order))),
            }
        }
        if class != Some(AtomClass::Mancude) {
            let assessment = chemistry::assess(id, atom, &orders);
            return match (assessment.finding, assessment.hydrogen_count) {
                (Finding::Valid, Some(count)) => Ok(count),
                _ => Err(Reason::HydrogensUnknown),
            };
        }
        // A mancude carbon: one of its mancude bonds is double in a Kekulé form, so its bonds
        // fill the valence of four with the hydrogens.
        if atom.radical != Radical::None {
            return Err(Reason::HydrogensUnknown);
        }
        let others: u16 = orders.iter().flatten().sum();
        let used = others + mancude + 1;
        match atom.hydrogens {
            Hydrogens::Specified(count) if used + u16::from(count) == 4 => Ok(u16::from(count)),
            Hydrogens::Inferred => 4_u16.checked_sub(used).ok_or(Reason::HydrogensUnknown),
            _ => Err(Reason::HydrogensUnknown),
        }
    }
}
