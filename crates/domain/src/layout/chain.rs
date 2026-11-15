//! The completion path of the chain layout preview: coordinates for the components of a
//! [`Preparation`], each of which is a tree.
//!
//! A layout component is a tree of units. Each layout atom outside a simple layout ring is an atom
//! unit, and each simple ring is one ring unit, whose geometry the `ring` module gives. This module
//! roots each component, ranks the children of every unit, and places the tree depth first. The
//! layout module orders the components and places them one at a time.
//!
//! # Conventions
//!
//! Coordinates are scene points with y growing down. An angle in degrees names the direction
//! `(cos, sin)`, so increasing angles turn clockwise on screen. Every chain bond placed here is
//! exactly one bond length long, and every ring bond is one to within 1e-9 pt.
//!
//! # Children
//!
//! An atom placed with incoming angle `a`, the direction from its parent to it, and turn sign `s`
//! (+1 or -1) places its `k` children as follows.
//!
//! - One child at a linear atom, one with a triple bond or a carbon with two double bonds: at `a`,
//!   with sign `s`. Two double bonds at another atom, as at the sulfur of O=S=O, do not make it
//!   linear.
//! - One child at any other atom: at `a - 60 s`, with sign `-s`. A run of such atoms zigzags at
//!   120 degrees between bonds.
//! - Two or more children: at `a + o` for the offsets `o = -180 + 360 j / (k + 1)`, `j = 1..=k`.
//!   For two children those are -60 and 60; for three they are -90, 0 and 90. The offsets are
//!   sorted by increasing absolute value, and on a tie the offset on the `-s` side comes first. A
//!   child keeps the sign of its offset, or `s` at offset 0.
//!
//! The children take the slots in rank order, so the first-ranked child takes the first slot. A
//! ring member is not placed this way: the `ring` module places the units that hang from it, and
//! never applies the linear rule to it.
//!
//! # Codes and rank
//!
//! The canonical code of a rooted subtree is `(`, its label, `;`, the code of the bond to its
//! parent, the codes of its children in byte order, and `)`. Equal codes mean the subtrees are
//! isomorphic. A ring unit has the code of the `ring` module, which begins `{` and is built from
//! the walk around the ring. A child of an atom may be a ring unit, so its code may begin `{`.
//!
//! The label is `atom_label` in the layout module: six fields joined by `|`, namely the kind, the
//! charge, the isotope, the hydrogens, the radical and the attachment points. The kind is an
//! element symbol, `?any`, `?anyheavy`, `?heteroatom` or `?halogen`, or `?in=` or `?notin=`
//! followed by the element symbols of a list, comma-separated. The isotope is `0` for natural
//! abundance. The hydrogens are `inferred`, `unknown` or a count, and the attachment points are
//! comma-separated.
//!
//! The bond code is `1`, `2`, `3`, `a` or `u` for a single, double, aromatic or unknown bond. A
//! dative bond is `d>` when it begins at the atom the walk comes from, and `d<` when it ends there.
//! A boundary bond is read from its fixed anchor. A query bond is `q*` for a query of any kind, or
//! `q[`, the kinds of its list in stored order separated by `,`, and `]`, where a kind is `1`, `2`,
//! `3` or `a`. A root has no bond to its parent, so its code is `-`. No label or bond code holds
//! `(`, `;` or `)`, which delimit an atom's code, and no label or bond code holds `{`, `}` or `/`,
//! which delimit a ring's code. So every code reads back unambiguously.
//!
//! Children are ranked by height (the most bonds from the child down to a leaf) descending, then by
//! atom count descending, then by code ascending, then by identifier ascending. The identifier is
//! the last key of every ranking here, so it decides only the ties that the other keys leave. A
//! ring unit's identifier is its entry atom.
//!
//! # Roots and chains
//!
//! An anchored component is rooted at its boundary atom. A free component is rooted at an end of a
//! longest path, with the length counted in unit edges: among those ends, the one with the smallest
//! rooted code, then the smaller identifier. An atom end's code starts `(` and a ring end's code
//! starts `{`, so an atom end always beats a ring end, and a ring is the root only when every end
//! of every longest path is a ring. A ring end's identifier is its chosen entry atom. A free
//! single atom is its own root, and an atom root with more than one unit has exactly one child
//! unit.
//!
//! The first-ranked child continues the chain of its parent, unless it is a ring unit, in which
//! case the chain ends at its parent. Every other child starts a new run. See the next section.
//!
//! # Chains and rings
//!
//! Every layout atom of a component is in exactly one chain or one ring. Ring members belong to no
//! chain, and no chain or ring is empty. The root unit, a chain or a ring, branches from the fixed
//! anchor of an anchored component, or from nothing when the component is free. Every other chain
//! or ring branches from its parent atom. Every exocyclic unit placed from a ring member starts a
//! new chain or ring that branches from that member, the entry member included. Chains and rings
//! are each listed in the order their first atoms are placed.
//!
//! # Placement
//!
//! A free component whose root is an atom of two or more atoms is laid out from the origin, with
//! its root's single child at -30 degrees and sign -1. A free component whose root is a ring is
//! laid out by the `ring` module, with the ring's centre at the origin and its entry at 0 degrees.
//! The layout is then moved so that its centroid is the centroid of its before positions. A free
//! single atom keeps its position exactly. For a free component of two or more atoms, the placed
//! layout depends on the before positions only through their centroid.
//!
//! An anchored component hangs from its fixed anchor `f`. Let `m` be the number of placed neighbours
//! of `f`: its fixed neighbours that have a position, and the layout atoms bonded to `f` that were
//! placed earlier. The
//! root is placed one bond length from `f`, in a direction `u`, with incoming angle `u` and sign
//! `s`.
//!
//! - `m = 0`: `u = -30` and `s = -1`.
//! - `m = 1`, with that neighbour at direction `β`: the candidates are `u = β + 120` with `s = +1`,
//!   and `u = β - 120` with `s = -1`. The candidate whose end point lies farther from every other
//!   positioned atom, except `f`, is taken. Within 1e-9 pt, `β + 120` is taken.
//! - `m >= 2`: `u` is the middle of the widest gap between the directions of the neighbours, and
//!   `s = +1`. Within 1e-9 degrees, the gap that starts at the smaller direction is taken.
//!
//! Anchored components are placed first, ordered by the position of their anchor (x, then y), then
//! by rank: height, atom count, code, the code of the boundary bond, and the smallest identifier of
//! the component. Free components follow in the same rank order, without an anchor position.
//!
//! # Renumbering
//!
//! Renumbering atoms leaves each atom's placed position unchanged, except at a tie that the rank
//! leaves to identifiers: isomorphic siblings, the two ends of a free component with equal codes,
//! isomorphic components at one anchor, and the entries and walks of a ring with equal codes (see
//! the `ring` module). At such a tie the atoms trade places under an automorphism, so the set of
//! positions is the same. One case is not covered: components anchored
//! at two distinct fixed atoms that share one place are ordered by identifier when their ranks tie,
//! so renumbering can change the result there.
//!
//! # Checkpoints
//!
//! The interrupt is asked before each atom is placed, and ring members are asked through the same
//! placement step, so before each of them too. `place_with` also asks it once at the start of each
//! component, and once per layout atom of its overlap scan.

use std::cmp::Ordering;
use std::collections::{BTreeMap, VecDeque};

use crate::budget::Interrupt;
use crate::document::{AtomId, BondId, BondOrder, Point};
use crate::element::Element;
use crate::states::{BondQuery, QueryBondOrder};

use super::ring::{self, RingFrame, RingNode};
use super::{Chain, Component, Halt, LayoutError, Preparation, neighbours};

/// An angle within this many steps of a multiple of 30 degrees is that multiple, and its direction
/// is taken from the exact table.
const STEP_TOLERANCE: f64 = 1e-9;
/// Two gaps in degrees, or two clearances in pt, that differ by at most this are tied.
pub(super) const TIE_TOLERANCE: f64 = 1e-9;
/// The turn between the bonds of a zigzag, in degrees.
const ZIGZAG_DEGREES: f64 = 60.0;
/// The direction of a free root's child, and of an anchored root with no placed neighbour.
const FIRST_DEGREES: f64 = -30.0;
/// The two directions of an anchored root with one placed neighbour lie this far from it.
const SIDE_DEGREES: f64 = 120.0;

/// A position in pt, before the range check builds a [`Point`] from it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Coord {
    pub(super) x: f64,
    pub(super) y: f64,
}

impl Coord {
    pub(super) const ORIGIN: Self = Self { x: 0.0, y: 0.0 };

    /// The straight-line distance to `other`, in pt.
    pub(super) fn distance(self, other: Self) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

impl From<Point> for Coord {
    fn from(point: Point) -> Self {
        Self {
            x: point.x(),
            y: point.y(),
        }
    }
}

/// An atom unit of a rooted tree, with the subtree below it. Once built, its children are in
/// placement rank order.
pub(super) struct Node {
    atom: AtomId,
    /// The most unit edges on a path from this atom down to a leaf unit.
    height: usize,
    /// The number of atoms in this subtree, ring members included.
    size: usize,
    /// The canonical code of this subtree. See the module documentation.
    code: String,
    children: Vec<Unit>,
}

/// A unit of a rooted tree: an atom that lies in no simple layout ring, with the subtree below it,
/// or a simple layout ring, which the `ring` module places as one polygon.
pub(super) enum Unit {
    Atom(Node),
    Ring(RingNode),
}

impl Unit {
    /// The canonical code of this unit. See the module documentation.
    pub(super) fn code(&self) -> &str {
        match self {
            Self::Atom(node) => node.code.as_str(),
            Self::Ring(ring) => ring.code.as_str(),
        }
    }

    /// The most unit edges on a path from this unit down to a leaf unit.
    pub(super) fn height(&self) -> usize {
        match self {
            Self::Atom(node) => node.height,
            Self::Ring(ring) => ring.height,
        }
    }

    /// The number of atoms in this unit's subtree, ring members included.
    pub(super) fn size(&self) -> usize {
        match self {
            Self::Atom(node) => node.size,
            Self::Ring(ring) => ring.size,
        }
    }

    /// The identifier of this unit: its atom, or the entry atom of a ring.
    pub(super) fn id(&self) -> AtomId {
        match self {
            Self::Atom(node) => node.atom,
            Self::Ring(ring) => ring.entry(),
        }
    }
}

/// The fixed anchor of an anchored component, and the position it hangs from.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Anchored {
    fixed_atom: AtomId,
    origin: Coord,
}

/// A component with its root chosen and its tree built, ready to be placed.
pub(super) struct Rooted {
    /// The atom placement starts from: the boundary atom of an anchored component, or the root of a
    /// free one.
    pub(super) root: AtomId,
    anchored: Option<Anchored>,
    /// The code of the boundary bond, `-` for a free component. See the module documentation.
    boundary_order: String,
    tree: Unit,
}

impl Rooted {
    /// The position of the anchor, or `None` for a free component.
    pub(super) fn origin(&self) -> Option<Coord> {
        self.anchored.map(|anchored| anchored.origin)
    }

    /// The height of the tree from its root, in unit edges.
    pub(super) fn height(&self) -> usize {
        self.tree.height()
    }

    /// The number of atoms in the tree, ring members included.
    pub(super) fn size(&self) -> usize {
        self.tree.size()
    }

    /// The canonical code of the tree from its root.
    pub(super) fn code(&self) -> &str {
        self.tree.code()
    }

    /// The code of the boundary bond, `-` for a free component.
    pub(super) fn boundary_order(&self) -> &str {
        &self.boundary_order
    }
}

/// The atoms placed for one component, in placement order, and its chains and rings.
pub(super) struct Placed {
    pub(super) positions: Vec<(AtomId, Coord)>,
    pub(super) chains: Vec<Chain>,
    pub(super) rings: Vec<Chain>,
}

/// Where a child goes: the direction of its bond from its parent, and its turn sign.
#[derive(Clone, Copy, Debug)]
pub(super) struct Slot {
    pub(super) angle: f64,
    pub(super) turn: i8,
}

/// Roots `component` and builds its tree. An anchored component is rooted at its boundary atom. A
/// free component is rooted at the end of a longest path with the smallest code, then the smallest
/// identifier.
///
/// # Errors
///
/// [`LayoutError::MissingPosition`] when the anchor of the component has no position.
pub(super) fn root(prep: &Preparation, component: &Component) -> Result<Rooted, LayoutError> {
    match component.anchor {
        Some(anchor) => {
            let boundary_atom = prep
                .boundary
                .iter()
                .find(|entry| entry.bond == anchor.bond)
                .map(|entry| entry.atom)
                .expect("prepare records the boundary atom of every anchored component");
            let origin = Coord::from(prep.position(anchor.fixed_atom)?);
            Ok(Rooted {
                root: boundary_atom,
                anchored: Some(Anchored {
                    fixed_atom: anchor.fixed_atom,
                    origin,
                }),
                boundary_order: code_from(prep, anchor.bond, anchor.fixed_atom),
                tree: if prep.ring_of.contains_key(&boundary_atom) {
                    build(prep, boundary_atom, Some((anchor.fixed_atom, anchor.bond)))
                } else {
                    build(prep, boundary_atom, None)
                },
            })
        }
        None => {
            let (root, tree) = free_root(prep, component);
            Ok(Rooted {
                root,
                anchored: None,
                boundary_order: "-".to_owned(),
                tree,
            })
        }
    }
}

/// Places `rooted`, given the layout atoms `placed` that come before it. An anchored component
/// hangs from its anchor. A free component is laid out from the origin and then moved onto the
/// centroid of its before positions.
///
/// # Errors
///
/// `Halt::Refused` when a layout atom of the component has no position, and `Halt::Interrupted`
/// when `interrupt` stops the placement.
pub(super) fn place(
    prep: &Preparation,
    rooted: &Rooted,
    placed: &BTreeMap<AtomId, Coord>,
    interrupt: &dyn Interrupt,
) -> Result<Placed, Halt> {
    let mut placer = Placer {
        prep,
        interrupt,
        length: prep.bond_length_pt,
        positions: Vec::new(),
        chains: Vec::new(),
        rings: Vec::new(),
    };
    match rooted.anchored {
        Some(anchored) => placer.place_anchored(rooted, anchored, placed)?,
        None => placer.place_free(rooted)?,
    }
    Ok(Placed {
        positions: placer.positions,
        chains: placer.chains,
        rings: placer.rings,
    })
}

/// The run that an atom joins when it is placed: an index into the chains, or into the rings.
#[derive(Clone, Copy)]
enum Run {
    Chain(usize),
    Ring(usize),
}

/// The state of one component's placement.
struct Placer<'a> {
    prep: &'a Preparation,
    interrupt: &'a dyn Interrupt,
    /// The document's bond length in pt.
    length: f64,
    positions: Vec<(AtomId, Coord)>,
    /// The chains of the component. A chain is never empty.
    chains: Vec<Chain>,
    /// The rings of the component, which hold the ring members. A ring is never empty.
    rings: Vec<Chain>,
}

impl Placer<'_> {
    /// Records `atom` at `at` as the next atom of `run`, after asking the interrupt.
    fn put(&mut self, atom: AtomId, at: Coord, run: Run) -> Result<(), Halt> {
        self.interrupt.check()?;
        self.positions.push((atom, at));
        match run {
            Run::Chain(index) => self.chains[index].atoms.push(atom),
            Run::Ring(index) => self.rings[index].atoms.push(atom),
        }
        Ok(())
    }

    /// Starts a chain that branches from `branch_from`, or from nothing for the chain of a free
    /// root, and returns its index.
    fn start_chain(&mut self, branch_from: Option<AtomId>) -> usize {
        self.chains.push(Chain {
            atoms: Vec::new(),
            branch_from,
        });
        self.chains.len() - 1
    }

    /// Starts a ring that branches from `branch_from`, or from nothing for the ring of a free root,
    /// and returns its index.
    fn start_ring(&mut self, branch_from: Option<AtomId>) -> usize {
        self.rings.push(Chain {
            atoms: Vec::new(),
            branch_from,
        });
        self.rings.len() - 1
    }

    /// Places the children of `node`, which sits at `origin` with incoming angle `incoming` and
    /// turn sign `turn`, depth first in rank order. The first child continues chain `chain`, unless
    /// it is a ring; every other child starts a run of its own.
    fn place_children(
        &mut self,
        node: &Node,
        origin: Coord,
        incoming: f64,
        turn: i8,
        chain: usize,
    ) -> Result<(), Halt> {
        let linear = self.is_linear(node.atom);
        let slots = child_slots(node.children.len(), linear, incoming, turn);
        for (index, (child, slot)) in node.children.iter().zip(slots).enumerate() {
            if index == 0 {
                self.place_continuing(child, origin, slot, chain, node.atom)?;
            } else {
                self.place_branch(child, origin, slot, node.atom)?;
            }
        }
        Ok(())
    }

    /// Places `unit`, the first-ranked child of `parent`, at `origin` in the direction of `slot`.
    /// An atom continues `chain`. A ring leaves `chain` at `parent` and starts a ring of its own.
    fn place_continuing(
        &mut self,
        unit: &Unit,
        origin: Coord,
        slot: Slot,
        chain: usize,
        parent: AtomId,
    ) -> Result<(), Halt> {
        match unit {
            Unit::Atom(node) => {
                let at = along(origin, slot.angle, self.length);
                self.put(node.atom, at, Run::Chain(chain))?;
                self.place_children(node, at, slot.angle, slot.turn, chain)
            }
            Unit::Ring(ring) => {
                let frame = ring::entered_frame(ring, origin, slot, self.length);
                self.place_ring(ring, &frame, Some(parent))
            }
        }
    }

    /// Places `unit`, a child of `parent` that starts a new run, at `origin` in the direction of
    /// `slot`. An atom starts a chain that branches from `parent`, and a ring starts a ring that
    /// does.
    fn place_branch(
        &mut self,
        unit: &Unit,
        origin: Coord,
        slot: Slot,
        parent: AtomId,
    ) -> Result<(), Halt> {
        match unit {
            Unit::Atom(node) => {
                let chain = self.start_chain(Some(parent));
                let at = along(origin, slot.angle, self.length);
                self.put(node.atom, at, Run::Chain(chain))?;
                self.place_children(node, at, slot.angle, slot.turn, chain)
            }
            Unit::Ring(ring) => {
                let frame = ring::entered_frame(ring, origin, slot, self.length);
                self.place_ring(ring, &frame, Some(parent))
            }
        }
    }

    /// Places a ring from its frame as a run that branches from `branch_from`: first its members in
    /// walk order, then the units that hang from each member, member by member in walk order.
    fn place_ring(
        &mut self,
        ring: &RingNode,
        frame: &RingFrame,
        branch_from: Option<AtomId>,
    ) -> Result<(), Halt> {
        let record = self.start_ring(branch_from);
        for (member, &at) in ring.members.iter().zip(&frame.positions) {
            self.put(member.atom, at, Run::Ring(record))?;
        }
        for ((member, &at), slots) in ring.members.iter().zip(&frame.positions).zip(&frame.slots) {
            debug_assert_eq!(
                slots.len(),
                member.children.len(),
                "a ring member takes one slot for each unit that hangs from it"
            );
            for (child, &slot) in member.children.iter().zip(slots) {
                self.place_branch(child, at, slot, member.atom)?;
            }
        }
        Ok(())
    }

    /// Places an anchored component: its root beside the anchor, then the rest of its tree.
    fn place_anchored(
        &mut self,
        rooted: &Rooted,
        anchored: Anchored,
        placed: &BTreeMap<AtomId, Coord>,
    ) -> Result<(), Halt> {
        let slot = self.anchor_slot(anchored, placed);
        self.place_branch(&rooted.tree, anchored.origin, slot, anchored.fixed_atom)
    }

    /// Lays out a free component from the origin and moves it onto the centroid of its before
    /// positions. A single atom keeps its position.
    fn place_free(&mut self, rooted: &Rooted) -> Result<(), Halt> {
        let first = self.positions.len();
        match &rooted.tree {
            Unit::Atom(node) => match node.children.as_slice() {
                [] => {
                    let kept = Coord::from(self.prep.position(rooted.root)?);
                    let chain = self.start_chain(None);
                    self.put(rooted.root, kept, Run::Chain(chain))
                }
                [child] => {
                    let chain = self.start_chain(None);
                    self.put(rooted.root, Coord::ORIGIN, Run::Chain(chain))?;
                    let slot = Slot {
                        angle: FIRST_DEGREES,
                        turn: -1,
                    };
                    self.place_continuing(child, Coord::ORIGIN, slot, chain, rooted.root)?;
                    self.recentre(first)
                }
                _ => unreachable!("an atom root is an end of a longest path, so it has one child"),
            },
            Unit::Ring(ring) => {
                let frame = ring::root_frame(ring, self.length);
                self.place_ring(ring, &frame, None)?;
                self.recentre(first)
            }
        }
    }

    /// Shifts the atoms placed from index `first` so that their centroid is the centroid of their
    /// before positions.
    fn recentre(&mut self, first: usize) -> Result<(), Halt> {
        let mut before = Vec::with_capacity(self.positions.len() - first);
        for (atom, _) in &self.positions[first..] {
            before.push(Coord::from(self.prep.position(*atom)?));
        }
        let target = mean(before.into_iter());
        let current = mean(self.positions[first..].iter().map(|&(_, at)| at));
        for (_, at) in &mut self.positions[first..] {
            at.x += target.x - current.x;
            at.y += target.y - current.y;
        }
        Ok(())
    }

    /// The direction and turn sign of an anchored component's root, from the placed neighbours of
    /// its anchor.
    fn anchor_slot(&self, anchored: Anchored, placed: &BTreeMap<AtomId, Coord>) -> Slot {
        let fixed = anchored.fixed_atom;
        let origin = anchored.origin;
        let mut around: Vec<Coord> = Vec::new();
        for &(other, _) in neighbours(&self.prep.adjacency, fixed) {
            if self.prep.layout_atoms.contains(&other) {
                around.extend(placed.get(&other).copied());
            } else {
                around.extend(self.prep.positions.get(&other).copied().map(Coord::from));
            }
        }
        match around.as_slice() {
            [] => Slot {
                angle: FIRST_DEGREES,
                turn: -1,
            },
            [only] => {
                let beta = angle_of(origin, *only);
                let plus = beta + SIDE_DEGREES;
                let minus = beta - SIDE_DEGREES;
                let clear_plus = self.clearance(along(origin, plus, self.length), fixed, placed);
                let clear_minus = self.clearance(along(origin, minus, self.length), fixed, placed);
                if clear_minus > clear_plus + TIE_TOLERANCE {
                    Slot {
                        angle: minus,
                        turn: -1,
                    }
                } else {
                    Slot {
                        angle: plus,
                        turn: 1,
                    }
                }
            }
            _ => widest_gap(origin, &around),
        }
    }

    /// The distance from `at` to the nearest positioned atom other than `fixed`: a fixed atom, or a
    /// layout atom placed so far. With no such atom the distance is infinite.
    fn clearance(&self, at: Coord, fixed: AtomId, placed: &BTreeMap<AtomId, Coord>) -> f64 {
        let fixed_atoms = self
            .prep
            .positions
            .iter()
            .filter(|(atom, _)| **atom != fixed && !self.prep.layout_atoms.contains(*atom))
            .map(|(_, point)| at.distance(Coord::from(*point)));
        let laid_out = placed
            .iter()
            .filter(|(atom, _)| **atom != fixed)
            .map(|(_, coord)| at.distance(*coord));
        fixed_atoms.chain(laid_out).fold(f64::INFINITY, f64::min)
    }

    /// Whether `atom` is linear: one of its bonds is triple, or, at a carbon, two of them are
    /// double. Two double bonds at another atom, as at the sulfur of O=S=O, leave its angle free.
    fn is_linear(&self, atom: AtomId) -> bool {
        let mut doubles = 0;
        for &(_, bond) in neighbours(&self.prep.adjacency, atom) {
            match order_of(self.prep, bond) {
                BondOrder::Triple => return true,
                BondOrder::Double => doubles += 1,
                _ => {}
            }
        }
        doubles >= 2 && is_carbon(self.prep, atom)
    }
}

/// The unit of `prep`'s layout tree that `atom` belongs to, built from `parent`, which is `None` at
/// a root. A simple ring is built by the `ring` module, which builds the units that hang from its
/// members. An atom's children are the layout atoms bonded to it, except its parent, so a boundary
/// bond is never followed. The layout components are trees of units, which `prepare` guarantees, so
/// the recursion ends.
fn build(prep: &Preparation, atom: AtomId, parent: Option<(AtomId, BondId)>) -> Unit {
    if prep.ring_of.contains_key(&atom) {
        return Unit::Ring(match parent {
            Some(parent) => ring::entered(prep, atom, parent),
            None => ring::rooted(prep, atom),
        });
    }
    let order = match parent {
        Some((from, bond)) => code_from(prep, bond, from),
        None => "-".to_owned(),
    };
    let (children, code) = child_units(prep, atom, &order, |other| {
        parent.is_some_and(|(from, _)| from == other)
    });
    let height = children
        .iter()
        .map(|child| child.height() + 1)
        .max()
        .unwrap_or(0);
    let size = 1 + children.iter().map(Unit::size).sum::<usize>();
    Unit::Atom(Node {
        atom,
        height,
        size,
        code,
        children,
    })
}

/// The child units of `atom`, in placement rank order, and the canonical code of `atom`. A child is
/// a layout atom bonded to `atom` that `exclude` does not reject, built with `atom` as its parent.
/// The code is `(`, the label of `atom`, `;`, `order`, the codes of the children in byte order,
/// and `)`. `order` is the code of the bond to the parent, or `-` at a root. A ring member passes
/// an empty `order`, because its walk states its bonds.
pub(super) fn child_units(
    prep: &Preparation,
    atom: AtomId,
    order: &str,
    exclude: impl Fn(AtomId) -> bool,
) -> (Vec<Unit>, String) {
    let mut children: Vec<Unit> = Vec::new();
    for &(other, bond) in neighbours(&prep.adjacency, atom) {
        if prep.layout_atoms.contains(&other) && !exclude(other) {
            children.push(build(prep, other, Some((atom, bond))));
        }
    }
    let mut codes: Vec<&str> = children.iter().map(Unit::code).collect();
    codes.sort_unstable();
    let label = prep
        .labels
        .get(&atom)
        .expect("prepare labels every layout atom");
    let code = format!("({label};{order}{})", codes.concat());
    children.sort_by(rank);
    (children, code)
}

/// The placement rank of two siblings: taller first, then larger, then by code, then by
/// identifier. A ring unit's identifier is its entry atom.
fn rank(left: &Unit, right: &Unit) -> Ordering {
    right
        .height()
        .cmp(&left.height())
        .then(right.size().cmp(&left.size()))
        .then_with(|| left.code().cmp(right.code()))
        .then(left.id().cmp(&right.id()))
}

/// The root and tree of a free component: the end of a longest path with the smallest code, then
/// the smallest identifier. A ring end's identifier is its chosen entry atom.
fn free_root(prep: &Preparation, component: &Component) -> (AtomId, Unit) {
    let mut best: Option<Unit> = None;
    for atom in diameter_ends(prep, component) {
        let tree = build(prep, atom, None);
        let replaces = best
            .as_ref()
            .is_none_or(|current| (tree.code(), tree.id()) < (current.code(), current.id()));
        if replaces {
            best = Some(tree);
        }
    }
    let tree = best.expect("a component has at least one end of a longest path");
    (tree.id(), tree)
}

/// The ends of the longest paths of `component`, ascending, counted in unit edges. A simple ring is
/// one unit, so it is an end once, under its representative atom (see `unit_of`).
///
/// The unit tree is a tree, so the eccentricity of a unit is the greater of its distances to the
/// two ends `a` and `b` of any one longest path, and the ends are the units whose eccentricity is
/// the diameter. Three breadth-first searches find them: one from the first atom's unit, which
/// reaches `a` at its greatest distance; one from `a`, which gives `b` and the diameter; and one
/// from `b`. Each search visits every unit and unit edge of the component once.
fn diameter_ends(prep: &Preparation, component: &Component) -> Vec<AtomId> {
    let first = *component
        .atoms
        .first()
        .expect("a layout component has at least one atom");
    let (a, _) = farthest(&distances(prep, first));
    let from_a = distances(prep, a);
    let (b, diameter) = farthest(&from_a);
    let from_b = distances(prep, b);
    component
        .atoms
        .iter()
        .copied()
        .filter(|atom| unit_of(prep, *atom) == *atom && from_a[atom].max(from_b[atom]) == diameter)
        .collect()
}

/// The atom at the greatest distance in `depths`, and that distance. Any of the farthest atoms is an
/// end of a longest path, so a tie does not change the ends that `diameter_ends` finds.
fn farthest(depths: &BTreeMap<AtomId, usize>) -> (AtomId, usize) {
    depths
        .iter()
        .map(|(&atom, &depth)| (atom, depth))
        .max_by_key(|&(_, depth)| depth)
        .expect("a search reaches its start atom")
}

/// The atom that stands for the unit of `atom`: the smallest member of its simple ring, or the atom
/// itself.
fn unit_of(prep: &Preparation, atom: AtomId) -> AtomId {
    prep.ring_of
        .get(&atom)
        .map_or(atom, |&index| prep.rings[index][0])
}

/// The number of unit edges from the unit of `start` to the unit of each layout atom of its
/// component, keyed by the representative atom of each unit (see `unit_of`). The search is
/// breadth-first over the units.
fn distances(prep: &Preparation, start: AtomId) -> BTreeMap<AtomId, usize> {
    let start = unit_of(prep, start);
    let mut depths = BTreeMap::from([(start, 0)]);
    let mut queue: VecDeque<(AtomId, usize)> = VecDeque::from([(start, 0)]);
    while let Some((current, depth)) = queue.pop_front() {
        let members = match prep.ring_of.get(&current) {
            Some(&index) => prep.rings[index].clone(),
            None => vec![current],
        };
        for atom in members {
            for &(other, _) in neighbours(&prep.adjacency, atom) {
                let next = unit_of(prep, other);
                if prep.layout_atoms.contains(&other)
                    && next != current
                    && !depths.contains_key(&next)
                {
                    depths.insert(next, depth + 1);
                    queue.push_back((next, depth + 1));
                }
            }
        }
    }
    depths
}

/// The slots of `count` children of an atom with incoming angle `incoming` and turn sign `turn`,
/// in rank order. See the module documentation for the rules.
fn child_slots(count: usize, linear: bool, incoming: f64, turn: i8) -> Vec<Slot> {
    match count {
        0 => Vec::new(),
        1 if linear => vec![Slot {
            angle: incoming,
            turn,
        }],
        1 => vec![Slot {
            angle: incoming - f64::from(turn) * ZIGZAG_DEGREES,
            turn: -turn,
        }],
        _ => {
            let parts = (count + 1) as f64;
            let mut offsets: Vec<f64> = (1..=count)
                .map(|step| -180.0 + 360.0 * step as f64 / parts)
                .collect();
            offsets.sort_by(|left, right| offset_order(*left, *right, turn));
            offsets
                .into_iter()
                .map(|offset| Slot {
                    angle: incoming + offset,
                    turn: if offset.abs() <= TIE_TOLERANCE {
                        turn
                    } else {
                        side(offset)
                    },
                })
                .collect()
        }
    }
}

/// The order of two offsets around one bisector, for a turn sign `turn`: the smaller absolute value
/// first. Offsets whose absolute values differ by at most TIE_TOLERANCE form a symmetric pair, and
/// on that tie the offset on the `-turn` side comes first. The slots of one set of siblings or of
/// one ring member are spaced far more than TIE_TOLERANCE apart in absolute value, so the tolerance
/// joins only such pairs, and the order stays total.
pub(super) fn offset_order(left: f64, right: f64, turn: i8) -> Ordering {
    let (left_abs, right_abs) = (left.abs(), right.abs());
    if (left_abs - right_abs).abs() <= TIE_TOLERANCE {
        side_rank(left, turn)
            .cmp(&side_rank(right, turn))
            .then(left.total_cmp(&right))
    } else {
        left_abs.total_cmp(&right_abs)
    }
}

/// The sign of an offset: -1 or +1, with zero counted as +1.
pub(super) fn side(offset: f64) -> i8 {
    if offset < 0.0 { -1 } else { 1 }
}

/// Zero for an offset on the side opposite the turn, so that it sorts first on a tie, and one for
/// the other side.
fn side_rank(offset: f64, turn: i8) -> u8 {
    if side(offset) == -turn { 0 } else { 1 }
}

/// The direction through the middle of the widest gap between the directions of `around` seen from
/// `origin`. Among the gaps within the tolerance of the widest, the one that starts at the smallest
/// direction is taken. The caller passes at least two points.
fn widest_gap(origin: Coord, around: &[Coord]) -> Slot {
    let mut angles: Vec<f64> = around
        .iter()
        .map(|at| normalized(angle_of(origin, *at)))
        .collect();
    angles.sort_by(f64::total_cmp);
    let mut gaps: Vec<(f64, f64)> = angles
        .windows(2)
        .map(|pair| (pair[1] - pair[0], pair[0]))
        .collect();
    if let (Some(&first), Some(&last)) = (angles.first(), angles.last()) {
        gaps.push((first + 360.0 - last, last));
    }
    let widest = gaps
        .iter()
        .map(|&(gap, _)| gap)
        .fold(f64::NEG_INFINITY, f64::max);
    let (gap, start) = gaps
        .into_iter()
        .find(|&(gap, _)| widest - gap <= TIE_TOLERANCE)
        .expect("the widest gap is one of the gaps");
    Slot {
        angle: start + gap / 2.0,
        turn: 1,
    }
}

/// The direction in `[0, 360)` that names the same direction as `degrees`.
fn normalized(degrees: f64) -> f64 {
    let wrapped = degrees.rem_euclid(360.0);
    if wrapped >= 360.0 { 0.0 } else { wrapped }
}

/// The direction from `origin` to `at`, in degrees.
fn angle_of(origin: Coord, at: Coord) -> f64 {
    (at.y - origin.y).atan2(at.x - origin.x).to_degrees()
}

/// The point `length` pt from `origin` in direction `degrees`.
pub(super) fn along(origin: Coord, degrees: f64, length: f64) -> Coord {
    let direction = unit(degrees);
    Coord {
        x: origin.x + length * direction.x,
        y: origin.y + length * direction.y,
    }
}

/// The unit vector in direction `degrees`. Within the step tolerance of a multiple of 30 degrees the
/// exact table is used; any other direction takes `cos` and `sin`.
pub(super) fn unit(degrees: f64) -> Coord {
    let steps = degrees / 30.0;
    let nearest = steps.round();
    if (steps - nearest).abs() > STEP_TOLERANCE {
        let radians = degrees.to_radians();
        return Coord {
            x: radians.cos(),
            y: radians.sin(),
        };
    }
    let half_root3 = 3.0_f64.sqrt() / 2.0;
    // Entry k is the direction of 30 k degrees, with k taken modulo twelve.
    let table = [
        (1.0, 0.0),
        (half_root3, 0.5),
        (0.5, half_root3),
        (0.0, 1.0),
        (-0.5, half_root3),
        (-half_root3, 0.5),
        (-1.0, 0.0),
        (-half_root3, -0.5),
        (-0.5, -half_root3),
        (0.0, -1.0),
        (0.5, -half_root3),
        (half_root3, -0.5),
    ];
    let (x, y) = table[nearest.rem_euclid(12.0) as usize];
    Coord { x, y }
}

/// The mean of `points`, which must not be empty.
fn mean(points: impl Iterator<Item = Coord>) -> Coord {
    let mut sum = Coord::ORIGIN;
    let mut count = 0.0;
    for point in points {
        sum.x += point.x;
        sum.y += point.y;
        count += 1.0;
    }
    Coord {
        x: sum.x / count,
        y: sum.y / count,
    }
}

/// The code of a bond order, as the canonical code of a subtree states it. `forward` says whether
/// a dative bond begins at the atom the walk comes from: `d>` if it does, `d<` if it ends there.
/// The other kinds ignore it. See the module documentation.
fn bond_code(order: &BondOrder, forward: bool) -> String {
    match order {
        BondOrder::Single => "1".to_owned(),
        BondOrder::Double => "2".to_owned(),
        BondOrder::Triple => "3".to_owned(),
        BondOrder::Aromatic => "a".to_owned(),
        BondOrder::Dative => String::from(if forward { "d>" } else { "d<" }),
        BondOrder::Unknown => "u".to_owned(),
        BondOrder::Query(BondQuery::Any) => "q*".to_owned(),
        BondOrder::Query(BondQuery::OneOf(kinds)) => {
            let codes: Vec<&str> = kinds.iter().map(|kind| query_kind_code(*kind)).collect();
            format!("q[{}]", codes.join(","))
        }
    }
}

/// The code of a bond kind that a query names.
fn query_kind_code(kind: QueryBondOrder) -> &'static str {
    match kind {
        QueryBondOrder::Single => "1",
        QueryBondOrder::Double => "2",
        QueryBondOrder::Triple => "3",
        QueryBondOrder::Aromatic => "a",
    }
}

/// The recorded order of `bond`, which has a layout atom at one end.
fn order_of(prep: &Preparation, bond: BondId) -> &BondOrder {
    prep.bond_orders
        .get(&bond)
        .expect("prepare records the order of every bond at a layout atom")
}

/// Whether the layout atom `atom` is a carbon. The first field of its label is its kind: the element
/// symbol of an element, and a query kind begins with `?`, so the field is exactly `C` for carbon.
fn is_carbon(prep: &Preparation, atom: AtomId) -> bool {
    let label = prep
        .labels
        .get(&atom)
        .expect("prepare labels every layout atom");
    label.split('|').next() == Some(Element::CARBON.symbol())
}

/// The atom that `bond` begins at, for a bond that has a layout atom at one end.
fn begin_of(prep: &Preparation, bond: BondId) -> AtomId {
    *prep
        .bond_begins
        .get(&bond)
        .expect("prepare records the beginning atom of every bond at a layout atom")
}

/// The code of `bond` as a walk that comes to it from `from`, one of its two atoms, states it.
pub(super) fn code_from(prep: &Preparation, bond: BondId, from: AtomId) -> String {
    bond_code(order_of(prep, bond), begin_of(prep, bond) == from)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::document::{Atom, AtomRef, BondTo, Command, Document, DocumentId, NewAtomPositions};
    use crate::layout::{Scope, prepare};
    use crate::scene::Selection;

    /// Asserts that `child_slots` gives `expected` as (angle, turn) pairs, in order.
    fn assert_slots(count: usize, linear: bool, incoming: f64, turn: i8, expected: &[(f64, i8)]) {
        let actual: Vec<(f64, i8)> = child_slots(count, linear, incoming, turn)
            .iter()
            .map(|slot| (slot.angle, slot.turn))
            .collect();
        let agrees = actual.len() == expected.len()
            && actual
                .iter()
                .zip(expected)
                .all(|(got, want)| (got.0 - want.0).abs() < 1e-9 && got.1 == want.1);
        assert!(
            agrees,
            "{count} children, linear {linear}, incoming {incoming}, turn {turn}: expected \
             {expected:?}, got {actual:?}"
        );
    }

    #[test]
    fn a_single_child_at_a_linear_atom_continues_straight() {
        assert_slots(1, true, 30.0, -1, &[(30.0, -1)]);
    }

    #[test]
    fn a_single_child_elsewhere_turns_sixty_degrees_and_flips_the_sign() {
        assert_slots(1, false, 30.0, 1, &[(-30.0, -1)]);
        assert_slots(1, false, 30.0, -1, &[(90.0, 1)]);
    }

    #[test]
    fn two_children_take_the_side_opposite_the_turn_first() {
        assert_slots(2, false, 0.0, 1, &[(-60.0, -1), (60.0, 1)]);
        assert_slots(2, false, 0.0, -1, &[(60.0, 1), (-60.0, -1)]);
    }

    #[test]
    fn three_children_take_the_straight_slot_first() {
        assert_slots(3, false, 0.0, 1, &[(0.0, 1), (-90.0, -1), (90.0, 1)]);
        assert_slots(3, false, 0.0, -1, &[(0.0, -1), (90.0, 1), (-90.0, -1)]);
    }

    #[test]
    fn four_children_take_the_slots_nearest_the_incoming_direction_first() {
        assert_slots(
            4,
            false,
            0.0,
            1,
            &[(-36.0, -1), (36.0, 1), (-108.0, -1), (108.0, 1)],
        );
    }

    #[test]
    fn symmetric_offsets_are_sorted_by_absolute_value_with_the_minus_side_first() {
        // Every count from two to twelve and both signs: for k = 6, 10 and 12 the mirrored offsets
        // differ in f64 by about 1e-14, so the tie must be taken within TIE_TOLERANCE.
        for count in 2..=12_usize {
            for turn in [-1_i8, 1] {
                // The incoming angle is zero, so each slot's angle is its offset.
                let offsets: Vec<f64> = child_slots(count, false, 0.0, turn)
                    .iter()
                    .map(|slot| slot.angle)
                    .collect();
                assert_eq!(
                    offsets.len(),
                    count,
                    "{count} children need {count} slots, got {offsets:?}"
                );
                for pair in offsets.windows(2) {
                    assert!(
                        pair[0].abs() <= pair[1].abs() + TIE_TOLERANCE,
                        "{count} children, turn {turn}: offsets {offsets:?} are not sorted by \
                         absolute value"
                    );
                }
                let mut pairs = 0;
                for (first, &earlier) in offsets.iter().enumerate() {
                    for &later in &offsets[first + 1..] {
                        let tied = (earlier.abs() - later.abs()).abs() <= TIE_TOLERANCE;
                        if tied && side(earlier) != side(later) {
                            pairs += 1;
                            assert!(
                                side(earlier) == -turn,
                                "{count} children, turn {turn}: the tied pair {earlier} and \
                                 {later} in {offsets:?} must put the -turn side first"
                            );
                        }
                    }
                }
                assert_eq!(
                    pairs,
                    count / 2,
                    "{count} children, turn {turn}: expected {} symmetric pairs in {offsets:?}",
                    count / 2
                );
            }
        }
    }

    #[test]
    fn the_widest_gap_is_bisected_and_a_tie_takes_the_smaller_start() {
        // The gaps from 0 to 180 degrees and from 180 to 360 degrees are equally wide.
        let around = [Coord { x: 1.0, y: 0.0 }, Coord { x: -1.0, y: 0.0 }];
        let slot = widest_gap(Coord::ORIGIN, &around);
        assert!(
            (slot.angle - 90.0).abs() < 1e-9,
            "the gap starting at 0 degrees is bisected at 90, got {}",
            slot.angle
        );
        assert_eq!(slot.turn, 1);
    }

    #[test]
    fn a_gap_that_wraps_past_zero_is_bisected() {
        // The gaps are 90, 90 and 180 degrees; the widest runs from 180 to 360 degrees.
        let around = [
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: -1.0, y: 0.0 },
        ];
        let slot = widest_gap(Coord::ORIGIN, &around);
        assert!(
            (slot.angle - 270.0).abs() < 1e-9,
            "the wrapping gap is bisected at 270, got {}",
            slot.angle
        );
    }

    #[test]
    fn unit_is_exact_within_a_billionth_of_a_step_and_trigonometric_beyond_it() {
        let exact = unit(90.0 + 1e-12);
        assert_eq!(
            (exact.x, exact.y),
            (0.0, 1.0),
            "a direction within 1e-9 steps of 90 degrees is the exact table entry"
        );
        let computed = unit(90.0 + 1e-6);
        assert!(
            computed.x != 0.0,
            "a direction 1e-6 degrees from 90 is computed, not taken from the table"
        );
        let diagonal = unit(-30.0);
        let root3 = 3.0_f64.sqrt();
        assert!(
            (diagonal.x - root3 / 2.0).abs() < 1e-12 && (diagonal.y + 0.5).abs() < 1e-12,
            "unit(-30) is (sqrt(3)/2, -1/2), got ({}, {})",
            diagonal.x,
            diagonal.y
        );
    }

    #[test]
    fn every_table_entry_matches_the_trigonometric_direction_of_its_angle() {
        for step in 0..12_u32 {
            let degrees = 30.0 * f64::from(step);
            let entry = unit(degrees);
            let radians = degrees.to_radians();
            assert!(
                (entry.x - radians.cos()).abs() < 1e-12 && (entry.y - radians.sin()).abs() < 1e-12,
                "unit({degrees}) = ({}, {}) differs from ({}, {})",
                entry.x,
                entry.y,
                radians.cos(),
                radians.sin()
            );
        }
    }

    #[test]
    fn bond_codes_tell_apart_every_bond_kind_and_keep_the_stored_order_of_a_list() {
        let kinds = [
            BondOrder::Single,
            BondOrder::Double,
            BondOrder::Triple,
            BondOrder::Aromatic,
            BondOrder::Dative,
            BondOrder::Unknown,
            BondOrder::Query(BondQuery::Any),
            BondOrder::Query(BondQuery::OneOf(vec![QueryBondOrder::Single])),
            BondOrder::Query(BondQuery::OneOf(vec![QueryBondOrder::Double])),
            BondOrder::Query(BondQuery::OneOf(vec![
                QueryBondOrder::Single,
                QueryBondOrder::Double,
            ])),
            BondOrder::Query(BondQuery::OneOf(vec![
                QueryBondOrder::Double,
                QueryBondOrder::Single,
            ])),
        ];
        let mut codes: Vec<String> = kinds.iter().map(|order| bond_code(order, true)).collect();
        // A dative bond that ends at the atom the walk comes from is the one other dative code.
        codes.push(bond_code(&BondOrder::Dative, false));
        let distinct: BTreeSet<String> = codes.iter().cloned().collect();
        assert_eq!(
            distinct.len(),
            codes.len(),
            "two bond kinds share a code: {distinct:?}"
        );
        let reversed = BondOrder::Query(BondQuery::OneOf(vec![
            QueryBondOrder::Double,
            QueryBondOrder::Single,
        ]));
        assert_eq!(bond_code(&reversed, true), "q[2,1]");
        assert_eq!(bond_code(&BondOrder::Dative, true), "d>");
        assert_eq!(bond_code(&BondOrder::Dative, false), "d<");
    }

    /// The next value of a linear congruential generator, below `bound`. `state` is its seed.
    fn lcg_next(state: &mut u64, bound: usize) -> usize {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((*state >> 33) as usize) % bound
    }

    /// A tree of `count` atoms from `seed`. Atom k bonds to a random earlier atom, by a single or a
    /// double bond, and each atom is a carbon, a nitrogen or an oxygen at random, so the codes vary.
    fn random_tree(seed: u64, count: usize) -> Document {
        let mut state = seed;
        let symbols = ["C", "N", "O"];
        let mut commands = Vec::with_capacity(count);
        for index in 0..count {
            let element = Element::from_symbol(symbols[lcg_next(&mut state, symbols.len())])
                .expect("C, N and O are elements");
            let bond_to = if index == 0 {
                None
            } else {
                let parent = lcg_next(&mut state, index);
                let order = if lcg_next(&mut state, 2) == 0 {
                    BondOrder::Single
                } else {
                    BondOrder::Double
                };
                Some(BondTo {
                    atom: AtomRef::Created(parent),
                    order,
                })
            };
            commands.push(Command::AddAtom {
                atom: Atom::from(element),
                positions: NewAtomPositions::Single(
                    Point::new(index as f64 * 30.0, 0.0).expect("a finite position"),
                ),
                bond_to,
            });
        }
        let mut document = Document::new(DocumentId::parse("tree").unwrap(), "Tree").unwrap();
        document.apply(&commands).unwrap();
        document
    }

    /// The eccentricity of every atom of `component`, from a breadth-first search from each atom.
    fn all_pairs_eccentricities(prep: &Preparation, component: &Component) -> Vec<(AtomId, usize)> {
        component
            .atoms
            .iter()
            .map(|&atom| {
                let reach = distances(prep, atom).values().copied().max().unwrap_or(0);
                (atom, reach)
            })
            .collect()
    }

    /// The root under the all-pairs rule: among the atoms whose eccentricity is the diameter, the one
    /// whose tree has the smallest code, then the smallest identifier.
    fn all_pairs_root(prep: &Preparation, component: &Component) -> AtomId {
        let reach = all_pairs_eccentricities(prep, component);
        let diameter = reach.iter().map(|&(_, depth)| depth).max().unwrap_or(0);
        reach
            .iter()
            .filter(|&&(_, depth)| depth == diameter)
            .map(|&(atom, _)| build(prep, atom, None))
            .min_by(|left, right| {
                left.code()
                    .cmp(right.code())
                    .then(left.id().cmp(&right.id()))
            })
            .expect("a component has at least one end of a longest path")
            .id()
    }

    #[test]
    fn the_ends_and_the_root_of_random_trees_follow_the_all_pairs_rule() {
        for seed in 0..300_u64 {
            let count = 1 + (seed as usize) % 30;
            let document = random_tree(seed, count);
            let selection = Selection {
                atoms: document.atoms().map(|(atom, _)| atom).collect(),
                ..Selection::default()
            };
            let prepared = prepare(
                &document,
                document.first_instance(),
                &selection,
                Scope::Selection,
            )
            .expect("a tree of elements with no ring and no stated stereo is a layout set");
            let component = &prepared.components[0];
            let reach = all_pairs_eccentricities(&prepared, component);
            let diameter = reach.iter().map(|&(_, depth)| depth).max().unwrap_or(0);
            let expected_ends: Vec<AtomId> = reach
                .iter()
                .filter(|&&(_, depth)| depth == diameter)
                .map(|&(atom, _)| atom)
                .collect();
            assert_eq!(
                diameter_ends(&prepared, component),
                expected_ends,
                "seed {seed}, {count} atoms: the ends of a longest path"
            );
            assert_eq!(
                free_root(&prepared, component).0,
                all_pairs_root(&prepared, component),
                "seed {seed}, {count} atoms: the root"
            );
        }
    }
}
