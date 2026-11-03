//! The hierarchical digraph of one pair of ligands, built lazily.

use crate::document::AtomId;
use crate::stereo::Slot;

use super::Reason;
use super::context::{AtomClass, Context, Mass, Step};

pub(super) type NodeIx = usize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    /// An atom reached for the first time on its path.
    Real(AtomId),
    /// A duplicate of `of`: the far end of a multiple bond or the atom a ring closes on.
    /// `of_depth` is the sphere of the corresponding real atom (the root is sphere 0).
    Duplicate { of: AtomId, of_depth: u16 },
    /// The duplicate carbon of a mancude ring atom.
    Mancude,
    /// A hydrogen that is not an atom of the graph.
    Hydrogen,
    /// A lone pair, atomic number 0.
    Phantom,
}

pub(super) struct Node {
    pub(super) kind: Kind,
    pub(super) z: u8,
    pub(super) mass: Mass,
    pub(super) depth: u16,
    parent: Option<NodeIx>,
    /// The order of the bond to the parent (1 for a mancude bond).
    pub(super) bond_in: u8,
    children: Option<Vec<NodeIx>>,
}

/// The nodes of the digraphs of two ligands of one center, which share nothing with any other
/// pair so the work of a pair depends only on the pair.
pub(super) struct Arena<'c, 'g> {
    context: &'c Context<'g>,
    center: AtomId,
    nodes: Vec<Node>,
}

impl<'c, 'g> Arena<'c, 'g> {
    pub(super) fn new(context: &'c Context<'g>, center: AtomId) -> Self {
        Self {
            context,
            center,
            nodes: Vec::new(),
        }
    }

    pub(super) fn len(&self) -> usize {
        self.nodes.len()
    }

    pub(super) fn node(&self, index: NodeIx) -> &Node {
        &self.nodes[index]
    }

    pub(super) fn context(&self) -> &'c Context<'g> {
        self.context
    }

    /// The parent node of `index`; `None` for a ligand, whose parent is the center.
    pub(super) fn parent(&self, index: NodeIx) -> Option<NodeIx> {
        self.nodes[index].parent
    }

    /// The atom the parent of `index` stands for: the center for a ligand.
    pub(super) fn parent_atom(&self, index: NodeIx) -> AtomId {
        match self.nodes[index]
            .parent
            .map(|parent| self.nodes[parent].kind)
        {
            Some(Kind::Real(atom)) => atom,
            _ => self.center,
        }
    }

    /// A ligand of the center, as a root of this arena.
    pub(super) fn root(&mut self, slot: Slot) -> Result<NodeIx, Reason> {
        let (kind, z, mass, bond_in) = match slot {
            Slot::Atom(atom) => {
                let bond = self
                    .context
                    .bond_between(self.center, atom)
                    .ok_or(Reason::CenterStructure)?;
                let order = match self.context.bond_step(bond)? {
                    Step::Order(order) => order,
                    Step::Mancude => 1,
                };
                (
                    Kind::Real(atom),
                    self.context.element(atom)?.atomic_number(),
                    self.context.mass(atom)?,
                    order,
                )
            }
            Slot::ImplicitHydrogen => (Kind::Hydrogen, 1, Mass::Number(1), 1),
            Slot::LonePair => (Kind::Phantom, 0, Mass::Number(0), 1),
        };
        Ok(self.push(kind, z, mass, 1, None, bond_in))
    }

    fn push(
        &mut self,
        kind: Kind,
        z: u8,
        mass: Mass,
        depth: u16,
        parent: Option<NodeIx>,
        bond_in: u8,
    ) -> NodeIx {
        self.nodes.push(Node {
            kind,
            z,
            mass,
            depth,
            parent,
            bond_in,
            children: None,
        });
        self.nodes.len() - 1
    }

    /// The sphere of `atom` on the path from the root to `index`, if it is on it. The center is
    /// sphere 0.
    fn path_depth(&self, index: NodeIx, atom: AtomId) -> Option<u16> {
        let mut cursor = Some(index);
        while let Some(current) = cursor {
            let node = &self.nodes[current];
            if node.kind == Kind::Real(atom) {
                return Some(node.depth);
            }
            cursor = node.parent;
        }
        (atom == self.center).then_some(0)
    }

    /// The children of a real node, built on first use; any other node has none.
    ///
    /// For each neighbor but the parent: a real child, or a duplicate when the neighbor is on
    /// the path (a ring closure), plus one duplicate for each bond beyond the first; a
    /// duplicate of the parent for each bond to it beyond the first; one duplicate carbon for a
    /// mancude atom; and a hydrogen for each hydrogen the valence model gives the atom.
    ///
    /// # Errors
    ///
    /// The reason the atom cannot be expanded: query atom, attachment point, a bond kind or
    /// hydrogen count the model cannot read, an aromatic system it refuses.
    pub(super) fn children(&mut self, index: NodeIx) -> Result<Vec<NodeIx>, Reason> {
        if let Some(children) = &self.nodes[index].children {
            return Ok(children.clone());
        }
        let Kind::Real(atom) = self.nodes[index].kind else {
            self.nodes[index].children = Some(Vec::new());
            return Ok(Vec::new());
        };
        let context = self.context;
        let hydrogens = context.hydrogens(atom)?;
        let depth = self.nodes[index].depth;
        let bond_in = self.nodes[index].bond_in;
        let parent_atom = self.parent_atom(index);
        let mut found = Vec::new();
        for &(neighbor, bond) in context.neighbors(atom) {
            if neighbor == parent_atom {
                continue;
            }
            let step = context.bond_step(bond)?;
            let (z, mass) = (
                context.element(neighbor)?.atomic_number(),
                context.mass(neighbor)?,
            );
            let on_path = self.path_depth(index, neighbor);
            let order = match step {
                Step::Mancude => 1,
                Step::Order(order) => order,
            };
            let corresponding = on_path.unwrap_or(depth + 1);
            let first = match on_path {
                Some(of_depth) => Kind::Duplicate {
                    of: neighbor,
                    of_depth,
                },
                None => Kind::Real(neighbor),
            };
            found.push(self.push(first, z, mass, depth + 1, Some(index), order));
            for _ in 1..order {
                let duplicate = Kind::Duplicate {
                    of: neighbor,
                    of_depth: corresponding,
                };
                found.push(self.push(duplicate, z, mass, depth + 1, Some(index), 1));
            }
        }
        if bond_in > 1 {
            let (z, mass) = (
                context.element(parent_atom)?.atomic_number(),
                context.mass(parent_atom)?,
            );
            for _ in 1..bond_in {
                let duplicate = Kind::Duplicate {
                    of: parent_atom,
                    of_depth: depth - 1,
                };
                found.push(self.push(duplicate, z, mass, depth + 1, Some(index), 1));
            }
        }
        if context.class(atom) == Some(AtomClass::Mancude) {
            found.push(self.push(
                Kind::Mancude,
                6,
                Mass::Number(12),
                depth + 1,
                Some(index),
                1,
            ));
        }
        for _ in 0..hydrogens {
            found.push(self.push(
                Kind::Hydrogen,
                1,
                Mass::Number(1),
                depth + 1,
                Some(index),
                1,
            ));
        }
        self.nodes[index].children = Some(found.clone());
        Ok(found)
    }
}
