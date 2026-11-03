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
    /// The farthest sphere whose atoms are expanded.
    spheres: usize,
    /// A bond left out of the digraph: no atom reached through it is a real node of its other
    /// end. The duplicate that a double bond adds on the end it leaves is kept. See
    /// [`Arena::without_bond`].
    cut: Option<[AtomId; 2]>,
    nodes: Vec<Node>,
}

impl<'c, 'g> Arena<'c, 'g> {
    pub(super) fn new(context: &'c Context<'g>, center: AtomId, spheres: usize) -> Self {
        Self {
            context,
            center,
            spheres,
            cut: None,
            nodes: Vec::new(),
        }
    }

    /// This arena with the bond between `ends` left out: expanding either end never makes a real
    /// node of the other (the duplicate a double bond adds is kept). This is the digraph of a
    /// double bond seen from inside the digraph rooted at one of its own ends, where the other
    /// end is not a ligand.
    pub(super) fn without_bond(mut self, ends: Option<[AtomId; 2]>) -> Self {
        self.cut = ends;
        self
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

    /// What a ligand of the center is, before it is a node.
    fn root_facts(&self, slot: Slot) -> Result<(Kind, u8, Mass, u8), Reason> {
        Ok(match slot {
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
            Slot::ImplicitHydrogen => (Kind::Hydrogen, 1, Mass::Table(1), 1),
            Slot::LonePair => (Kind::Phantom, 0, Mass::Number(0), 1),
        })
    }

    /// Two ligands of the center, as roots of this arena. Both are checked before either is
    /// added, so a refusal leaves the arena empty whichever ligand fails.
    ///
    /// # Errors
    ///
    /// The smaller reason of the two ligands that cannot be read.
    pub(super) fn roots(&mut self, first: Slot, second: Slot) -> Result<(NodeIx, NodeIx), Reason> {
        let facts = [self.root_facts(first), self.root_facts(second)];
        if let Some(reason) = facts.iter().filter_map(|f| f.as_ref().err()).min() {
            return Err(*reason);
        }
        let [Ok(a), Ok(b)] = facts else {
            return Err(Reason::CenterStructure);
        };
        let a = self.push(a.0, a.1, a.2, 1, None, a.3);
        let b = self.push(b.0, b.1, b.2, 1, None, b.3);
        Ok((a, b))
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

    /// The atoms of the real nodes whose children were built: the atoms this arena looked at
    /// the neighbors of. What a comparison read of the molecule is within these atoms and
    /// their neighbors.
    pub(super) fn expanded_atoms(&self) -> impl Iterator<Item = AtomId> + '_ {
        self.nodes
            .iter()
            .filter_map(|node| match (node.kind, &node.children) {
                (Kind::Real(atom), Some(children)) if !children.is_empty() => Some(atom),
                _ => None,
            })
    }

    /// Whether a ring closure is among the nodes: a duplicate of an atom on the path that is
    /// neither the far end of a multiple bond nor the parent a double bond leads back to.
    pub(super) fn closes_ring(&self) -> bool {
        self.nodes.iter().any(|node| match node.kind {
            // A duplicate is one sphere below the atom that was expanded, so the atom it stands
            // for is 3 or more spheres above it when it is a ring closure, 2 above for the
            // parent of a double bond, and level with it for the far end of a multiple bond.
            Kind::Duplicate { of_depth, .. } => node.depth >= of_depth.saturating_add(3),
            _ => false,
        })
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
    /// Either every child is added or none is: an atom that cannot be expanded adds nothing,
    /// so the size of the arena does not depend on which of its neighbors was read first. The
    /// reason a refused node is reported under is the smallest of all the problems it has.
    ///
    /// # Errors
    ///
    /// The reason the atom cannot be expanded: query atom, attachment point, a bond kind or
    /// hydrogen count the model cannot read, an aromatic system it refuses. Such a node is
    /// left without children, so asking again returns none; [`Reason::DigraphLimit`] is
    /// returned, and nothing is remembered, for an atom in the last sphere the arena allows.
    pub(super) fn children(&mut self, index: NodeIx) -> Result<Vec<NodeIx>, Reason> {
        if let Some(children) = &self.nodes[index].children {
            return Ok(children.clone());
        }
        if !matches!(self.nodes[index].kind, Kind::Real(_)) {
            self.nodes[index].children = Some(Vec::new());
            return Ok(Vec::new());
        }
        if usize::from(self.nodes[index].depth) >= self.spheres {
            return Err(Reason::DigraphLimit);
        }
        match self.planned_children(index) {
            Ok(planned) => {
                let depth = self.nodes[index].depth;
                let found: Vec<NodeIx> = planned
                    .into_iter()
                    .map(|(kind, z, mass, bond)| {
                        self.push(kind, z, mass, depth + 1, Some(index), bond)
                    })
                    .collect();
                self.nodes[index].children = Some(found.clone());
                Ok(found)
            }
            Err(reason) => {
                self.nodes[index].children = Some(Vec::new());
                Err(reason)
            }
        }
    }

    /// The children of the real node `index` as (kind, atomic number, mass, bond order to the
    /// parent), or the smallest reason it cannot be expanded.
    fn planned_children(&self, index: NodeIx) -> Result<Vec<(Kind, u8, Mass, u8)>, Reason> {
        let Kind::Real(atom) = self.nodes[index].kind else {
            return Ok(Vec::new());
        };
        let context = self.context;
        let depth = self.nodes[index].depth;
        let bond_in = self.nodes[index].bond_in;
        let parent_atom = self.parent_atom(index);
        let mut refusal: Option<Reason> = None;
        let mut refuse = |reason: Reason| refusal = Some(refusal.map_or(reason, |r| r.min(reason)));
        let hydrogens = context.hydrogens(atom).unwrap_or_else(|reason| {
            refuse(reason);
            0
        });
        let mut found = Vec::new();
        for &(neighbor, bond) in context.neighbors(atom) {
            if neighbor == parent_atom {
                continue;
            }
            let facts = (|| -> Result<_, Reason> {
                Ok((
                    context.bond_step(bond)?,
                    context.element(neighbor)?.atomic_number(),
                    context.mass(neighbor)?,
                ))
            })();
            let (step, z, mass) = match facts {
                Ok(facts) => facts,
                Err(reason) => {
                    refuse(reason);
                    continue;
                }
            };
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
            let left_out = self
                .cut
                .is_some_and(|ends| ends.contains(&atom) && ends.contains(&neighbor));
            if !left_out {
                found.push((first, z, mass, order));
            }
            for _ in 1..order {
                let duplicate = Kind::Duplicate {
                    of: neighbor,
                    of_depth: corresponding,
                };
                found.push((duplicate, z, mass, 1));
            }
        }
        if bond_in > 1 {
            match (context.element(parent_atom), context.mass(parent_atom)) {
                (Ok(element), Ok(mass)) => {
                    for _ in 1..bond_in {
                        let duplicate = Kind::Duplicate {
                            of: parent_atom,
                            of_depth: depth - 1,
                        };
                        found.push((duplicate, element.atomic_number(), mass, 1));
                    }
                }
                (Err(reason), _) | (_, Err(reason)) => refuse(reason),
            }
        }
        if context.class(atom) == Some(AtomClass::Mancude) {
            found.push((Kind::Mancude, 6, Mass::Table(12), 1));
        }
        for _ in 0..hydrogens {
            found.push((Kind::Hydrogen, 1, Mass::Table(1), 1));
        }
        refusal.map_or(Ok(found), Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{
        Atom, AtomRef, BondOrder, Command, Document, DocumentId, NewAtomPositions, Point,
    };
    use crate::element::Element;
    use crate::graph::MoleculeGraph;

    /// The ring closure of cyclobutene `Y=X, Y-A, A-B, B-X` entered from a center `K` at `Y`
    /// reaches `Y` again from `X` through the double bond: both duplicates of `Y` stand for
    /// the real `Y` in sphere 1, whatever sphere they are in. No label shows the second
    /// duplicate's depth on its own (a search of 60,000 generated molecules found none, and the
    /// first duplicate decides any comparison that reads either), so the arena is read
    /// directly.
    #[test]
    fn both_duplicates_of_a_ring_closing_double_bond_point_at_the_atom_on_the_path() {
        let mut document = Document::new(DocumentId::parse("digraph-test").unwrap(), "t").unwrap();
        let carbon = || Atom::from(Element::from_symbol("C").unwrap());
        let atoms: Vec<Command> = (0..5)
            .map(|i| Command::AddAtom {
                atom: carbon(),
                positions: NewAtomPositions::Single(Point::new(f64::from(i), 0.0).unwrap()),
                bond_to: None,
            })
            .collect();
        document.apply(&atoms).unwrap();
        let ids: Vec<AtomId> = document.atoms().map(|(id, _)| id).collect();
        let [center, y, a, b, x] = [ids[0], ids[1], ids[2], ids[3], ids[4]];
        let bonds: Vec<Command> = [
            (center, y, BondOrder::Single),
            (y, x, BondOrder::Double),
            (y, a, BondOrder::Single),
            (a, b, BondOrder::Single),
            (b, x, BondOrder::Single),
        ]
        .into_iter()
        .map(|(begin, end, order)| Command::AddBond {
            begin: AtomRef::Id(begin),
            end: AtomRef::Id(end),
            order,
        })
        .collect();
        document.apply(&bonds).unwrap();
        let graph = MoleculeGraph::from_document(&document);
        let context = Context::new(&graph);
        let mut arena = Arena::new(&context, center, crate::cip::MAX_SPHERES);
        let (mut node, _) = arena.roots(Slot::Atom(y), Slot::ImplicitHydrogen).unwrap();
        // Walk the way around the ring: Y, A, B, then X as a real atom (not the child of Y).
        for next in [a, b, x] {
            node = arena
                .children(node)
                .unwrap()
                .into_iter()
                .find(|&child| arena.node(child).kind == Kind::Real(next))
                .expect("the ring is walked in order");
        }
        let closure: Vec<Kind> = arena
            .children(node)
            .unwrap()
            .into_iter()
            .map(|child| arena.node(child).kind)
            .filter(|kind| matches!(kind, Kind::Duplicate { .. }))
            .collect();
        let expected = Kind::Duplicate { of: y, of_depth: 1 };
        assert_eq!(closure, vec![expected, expected]);
    }
}
