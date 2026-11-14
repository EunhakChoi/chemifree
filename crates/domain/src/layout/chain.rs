//! The completion path of the chain layout preview: coordinates for the components of a
//! [`Preparation`], each of which is a tree.
//!
//! The ring refusal of `prepare` leaves every layout component acyclic, so this module has no ring
//! rule. It roots each component, ranks the children of every atom, and places the tree depth
//! first. The layout module orders the components and places them one at a time.
//!
//! # Conventions
//!
//! Coordinates are scene points with y growing down. An angle in degrees names the direction
//! `(cos, sin)`, so increasing angles turn clockwise on screen. Every bond placed here is exactly
//! one bond length long.
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
//! The children take the slots in rank order, so the first-ranked child takes the first slot.
//!
//! # Codes and rank
//!
//! The canonical code of a rooted subtree is `(`, its label, `;`, the code of the bond to its
//! parent, the codes of its children in byte order, and `)`. Equal codes mean the subtrees are
//! isomorphic.
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
//! `(`, `;` or `)`, which delimit the code, so every code reads back unambiguously.
//!
//! Children are ranked by height (the most bonds from the child down to a leaf) descending, then by
//! atom count descending, then by code ascending, then by identifier ascending. The identifier is
//! the last key of every ranking here, so it decides only the ties that the other keys leave.
//!
//! # Roots and chains
//!
//! An anchored component is rooted at its boundary atom. A free component is rooted at an end of a
//! longest path: among those ends, the one with the smallest rooted code, then the smaller
//! identifier. Every end of a longest path in a tree is a leaf, so a free root with two or more
//! atoms has exactly one child. A free single atom is its own root.
//!
//! The first-ranked child continues the chain of its parent. Every other child starts a new chain
//! whose `branch_from` is its parent. Chains are listed in the order their first atoms are placed.
//! The first chain of an anchored component branches from its fixed anchor, and the first chain of
//! a free component has no `branch_from`.
//!
//! # Placement
//!
//! A free component of two or more atoms is laid out from the origin, with its root's single child
//! at -30 degrees and sign -1. The layout is then moved so that its centroid is the centroid of its
//! before positions. A free single atom keeps its position exactly. For a free component of two or
//! more atoms, the placed layout depends on the before positions only through their centroid.
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
//! and isomorphic components at one anchor. At such a tie the atoms trade places under an
//! automorphism, so the set of positions is the same. One case is not covered: components anchored
//! at two distinct fixed atoms that share one place are ordered by identifier when their ranks tie,
//! so renumbering can change the result there.
//!
//! # Checkpoints
//!
//! The interrupt is asked before each atom is placed. `place_with` also asks it once at the start of
//! each component, and once per layout atom of its overlap scan.

use std::cmp::Ordering;
use std::collections::{BTreeMap, VecDeque};

use crate::budget::Interrupt;
use crate::document::{AtomId, BondId, BondOrder, Point};
use crate::element::Element;
use crate::states::{BondQuery, QueryBondOrder};

use super::{Chain, Component, Halt, LayoutError, Preparation, neighbours};

/// An angle within this many steps of a multiple of 30 degrees is that multiple, and its direction
/// is taken from the exact table.
const STEP_TOLERANCE: f64 = 1e-9;
/// Two gaps in degrees, or two clearances in pt, that differ by at most this are tied.
const TIE_TOLERANCE: f64 = 1e-9;
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

/// An atom of a rooted tree, with the subtree below it. Once built, its children are in placement
/// rank order.
struct Node {
    atom: AtomId,
    /// The most bonds on a path from this atom down to a leaf.
    height: usize,
    /// The number of atoms in this subtree.
    size: usize,
    /// The canonical code of this subtree. See the module documentation.
    code: String,
    children: Vec<Node>,
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
    tree: Node,
}

impl Rooted {
    /// The position of the anchor, or `None` for a free component.
    pub(super) fn origin(&self) -> Option<Coord> {
        self.anchored.map(|anchored| anchored.origin)
    }

    /// The height of the tree from its root, in bonds.
    pub(super) fn height(&self) -> usize {
        self.tree.height
    }

    /// The number of atoms in the tree.
    pub(super) fn size(&self) -> usize {
        self.tree.size
    }

    /// The canonical code of the tree from its root.
    pub(super) fn code(&self) -> &str {
        &self.tree.code
    }

    /// The code of the boundary bond, `-` for a free component.
    pub(super) fn boundary_order(&self) -> &str {
        &self.boundary_order
    }
}

/// The atoms placed for one component, in placement order, and its chains.
pub(super) struct Placed {
    pub(super) positions: Vec<(AtomId, Coord)>,
    pub(super) chains: Vec<Chain>,
}

/// Where a child goes: the direction of its bond from its parent, and its turn sign.
#[derive(Clone, Copy, Debug)]
struct Slot {
    angle: f64,
    turn: i8,
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
                tree: build(prep, boundary_atom, None),
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
        chains: vec![Chain {
            atoms: Vec::new(),
            branch_from: rooted.anchored.map(|anchored| anchored.fixed_atom),
        }],
    };
    match rooted.anchored {
        Some(anchored) => placer.place_anchored(rooted, anchored, placed)?,
        None => placer.place_free(rooted)?,
    }
    Ok(Placed {
        positions: placer.positions,
        chains: placer.chains,
    })
}

/// The state of one component's placement.
struct Placer<'a> {
    prep: &'a Preparation,
    interrupt: &'a dyn Interrupt,
    /// The document's bond length in pt.
    length: f64,
    positions: Vec<(AtomId, Coord)>,
    chains: Vec<Chain>,
}

impl Placer<'_> {
    /// Records `atom` at `at` as the next atom of chain `chain`, after asking the interrupt.
    fn put(&mut self, atom: AtomId, at: Coord, chain: usize) -> Result<(), Halt> {
        self.interrupt.check()?;
        self.positions.push((atom, at));
        self.chains[chain].atoms.push(atom);
        Ok(())
    }

    /// Starts a chain that branches from `branch_from`, and returns its index.
    fn start_chain(&mut self, branch_from: AtomId) -> usize {
        self.chains.push(Chain {
            atoms: Vec::new(),
            branch_from: Some(branch_from),
        });
        self.chains.len() - 1
    }

    /// Places the children of `node`, which sits at `origin` with incoming angle `incoming` and turn
    /// sign `turn`, depth first in rank order. The first child continues chain `chain`.
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
            let at = along(origin, slot.angle, self.length);
            let child_chain = if index == 0 {
                chain
            } else {
                self.start_chain(node.atom)
            };
            self.put(child.atom, at, child_chain)?;
            self.place_children(child, at, slot.angle, slot.turn, child_chain)?;
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
        let at = along(anchored.origin, slot.angle, self.length);
        self.put(rooted.root, at, 0)?;
        self.place_children(&rooted.tree, at, slot.angle, slot.turn, 0)
    }

    /// Lays out a free component from the origin and moves it onto the centroid of its before
    /// positions. A single atom keeps its position.
    fn place_free(&mut self, rooted: &Rooted) -> Result<(), Halt> {
        match rooted.tree.children.as_slice() {
            [] => {
                let kept = Coord::from(self.prep.position(rooted.root)?);
                self.put(rooted.root, kept, 0)
            }
            [child] => {
                let first = self.positions.len();
                self.put(rooted.root, Coord::ORIGIN, 0)?;
                let at = along(Coord::ORIGIN, FIRST_DEGREES, self.length);
                self.put(child.atom, at, 0)?;
                self.place_children(child, at, FIRST_DEGREES, -1, 0)?;
                self.recentre(first)
            }
            _ => unreachable!("a free root is an end of a longest path, so it has one child"),
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

/// The tree below `atom`, entered from `parent`, which is `None` at the root. Only layout atoms
/// become children, so a boundary bond is never followed. The layout components are trees, which
/// `prepare` guarantees, so the recursion ends.
fn build(prep: &Preparation, atom: AtomId, parent: Option<(AtomId, BondId)>) -> Node {
    let mut children: Vec<Node> = Vec::new();
    for &(other, bond) in neighbours(&prep.adjacency, atom) {
        let is_child =
            prep.layout_atoms.contains(&other) && parent.is_none_or(|(from, _)| from != other);
        if is_child {
            children.push(build(prep, other, Some((atom, bond))));
        }
    }
    let height = children
        .iter()
        .map(|child| child.height + 1)
        .max()
        .unwrap_or(0);
    let size = 1 + children.iter().map(|child| child.size).sum::<usize>();
    let mut codes: Vec<&str> = children.iter().map(|child| child.code.as_str()).collect();
    codes.sort_unstable();
    let label = prep
        .labels
        .get(&atom)
        .expect("prepare labels every layout atom");
    let order = match parent {
        Some((from, bond)) => code_from(prep, bond, from),
        None => "-".to_owned(),
    };
    let code = format!("({label};{order}{})", codes.concat());
    children.sort_by(rank);
    Node {
        atom,
        height,
        size,
        code,
        children,
    }
}

/// The placement rank of two siblings: taller first, then larger, then by code, then by
/// identifier.
fn rank(left: &Node, right: &Node) -> Ordering {
    right
        .height
        .cmp(&left.height)
        .then(right.size.cmp(&left.size))
        .then_with(|| left.code.cmp(&right.code))
        .then(left.atom.cmp(&right.atom))
}

/// The root and tree of a free component: the end of a longest path with the smallest code, then
/// the smallest identifier.
fn free_root(prep: &Preparation, component: &Component) -> (AtomId, Node) {
    let mut best: Option<Node> = None;
    for atom in diameter_ends(prep, component) {
        let tree = build(prep, atom, None);
        let replaces = best.as_ref().is_none_or(|current| {
            (tree.code.as_str(), tree.atom) < (current.code.as_str(), current.atom)
        });
        if replaces {
            best = Some(tree);
        }
    }
    let tree = best.expect("a component has at least one end of a longest path");
    (tree.atom, tree)
}

/// The ends of the longest paths of `component`, ascending.
///
/// The component is a tree, so the eccentricity of an atom is the greater of its distances to the
/// two ends `a` and `b` of any one longest path, and the ends are the atoms whose eccentricity is
/// the diameter. Three breadth-first searches find them: one from the first atom, which reaches `a`
/// at its greatest distance; one from `a`, which gives `b` and the diameter; and one from `b`. Each
/// search visits every atom and bond of the component once.
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
        .filter(|atom| from_a[atom].max(from_b[atom]) == diameter)
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

/// The number of bonds from `start` to each atom of its component, by breadth-first search over
/// the layout atoms.
fn distances(prep: &Preparation, start: AtomId) -> BTreeMap<AtomId, usize> {
    let mut depths = BTreeMap::from([(start, 0)]);
    let mut queue: VecDeque<(AtomId, usize)> = VecDeque::from([(start, 0)]);
    while let Some((atom, depth)) = queue.pop_front() {
        for &(other, _) in neighbours(&prep.adjacency, atom) {
            if prep.layout_atoms.contains(&other) && !depths.contains_key(&other) {
                depths.insert(other, depth + 1);
                queue.push_back((other, depth + 1));
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
            // Offsets whose absolute values differ by at most TIE_TOLERANCE form a symmetric pair.
            // Distinct absolute values are at least 360 / (count + 1) degrees apart, so the
            // tolerance joins only such pairs, and the order stays total.
            offsets.sort_by(|left, right| {
                let (left_abs, right_abs) = (left.abs(), right.abs());
                if (left_abs - right_abs).abs() <= TIE_TOLERANCE {
                    side_rank(*left, turn)
                        .cmp(&side_rank(*right, turn))
                        .then(left.total_cmp(right))
                } else {
                    left_abs.total_cmp(&right_abs)
                }
            });
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

/// The sign of an offset: -1 or +1, with zero counted as +1.
fn side(offset: f64) -> i8 {
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
fn along(origin: Coord, degrees: f64, length: f64) -> Coord {
    let direction = unit(degrees);
    Coord {
        x: origin.x + length * direction.x,
        y: origin.y + length * direction.y,
    }
}

/// The unit vector in direction `degrees`. Within the step tolerance of a multiple of 30 degrees the
/// exact table is used; any other direction takes `cos` and `sin`.
fn unit(degrees: f64) -> Coord {
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
fn code_from(prep: &Preparation, bond: BondId, from: AtomId) -> String {
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
            .min_by(|left, right| left.code.cmp(&right.code).then(left.atom.cmp(&right.atom)))
            .expect("a component has at least one end of a longest path")
            .atom
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
