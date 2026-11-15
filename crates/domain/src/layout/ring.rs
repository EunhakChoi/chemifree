//! The ring units of the chain layout preview: a simple ring of 3 to 12 atoms that lies wholly in
//! the layout set, drawn as a regular polygon.
//!
//! `chain` places the units, roots them and ranks them. This module holds what a ring unit adds:
//! its walk and rotation codes, its height and size, and the geometry of a placed ring. Angles are
//! in degrees, with y growing down, as in `chain`.
//!
//! # Geometry
//!
//! A ring of `n` members has side `b`, the bond length, and circumradius `R = b / (2 sin(180 / n))`.
//! Its members lie at `C + R u(θ_k)` from the centre `C`, where `θ_k = β + σ k 360 / n`. Here `β` is
//! the outward bisector at the entry `v_0`, `σ` is ±1 for the walk direction, and `θ_k` is also the
//! outward bisector at `v_k`. Every vertex except the entry of an entered ring is computed directly from `C`; that entry
//! stands one bond length from its parent, and `C` is derived from it.
//!
//! A member with `m` exocyclic units spreads them through the exterior wedge `W = 180 + 360 / n`, at
//! the offsets `-W/2 + W j / (m + 1)` for `j = 1..=m` around its bisector. The offsets are sorted as
//! `chain` sorts child slots. A slot's turn sign is the side of its offset, or the ring's sign at
//! offset 0.
//!
//! A ring entered from a parent through a slot `(α, s)` has `σ = s`. Its entry stands where that
//! slot reaches, at `P_e`. The entry has `t = 1 + m` slots, and the parent takes the first of them,
//! at offset `o_p`. So `β = α + 180 - o_p` and `C = P_e + R u(α - o_p)`. A free root ring is centred
//! on the origin with `β = 0`, `σ = 1` and `s = 1`, so its entry is its rightmost vertex.
//!
//! # Codes
//!
//! A ring unit's code is `{`, its order, `;`, and then, for each member `v_k` in walk order, the
//! code `M_k` of the member and the code `B_k` of the bond from `v_k` to `v_{k+1}`, joined by `/`,
//! followed by `}`. The order is the code of the bond from the parent, read from the parent, or `-`
//! for a free root. `M_k` is `(`, the label of `v_k`, `;`, and the codes of its child units in byte
//! order, then `)`. No label or bond code holds `{`, `}` or `/`, so every code reads back
//! unambiguously.
//!
//! An entered ring has two walks, one from each ring neighbour of the entry. The smaller code wins,
//! and on a tie the walk through the smaller neighbour identifier is taken. A free root ring compares
//! all `2n` walks, one per entry and neighbour, and keeps the smallest code. A tie there goes to the
//! smaller entry identifier, then to the smaller neighbour identifier.
//!
//! The height of a unit is the largest number of unit edges from it down to a leaf, and its size
//! counts its atoms, ring members included. The children of each member are kept in rank order.

use std::collections::BTreeMap;

use crate::document::{AtomId, BondId};

use super::chain::{
    Coord, Slot, TIE_TOLERANCE, Unit, along, child_units, code_from, offset_order, side, unit,
};
use super::{Preparation, neighbours};

/// A member of a placed ring, with the units that hang from it in rank order.
pub(super) struct Member {
    pub(super) atom: AtomId,
    pub(super) children: Vec<Unit>,
}

/// A ring unit, with its members in walk order and the entry first.
pub(super) struct RingNode {
    pub(super) members: Vec<Member>,
    /// The largest number of unit edges from this unit down to a leaf.
    pub(super) height: usize,
    /// The number of atoms in this unit, ring members included.
    pub(super) size: usize,
    /// The canonical code of this unit. See the module documentation.
    pub(super) code: String,
}

impl RingNode {
    /// The entry member, which is also the identifier of the unit.
    pub(super) fn entry(&self) -> AtomId {
        self.members[0].atom
    }
}

/// The placed members of a ring, and the slots of each member's child units.
pub(super) struct RingFrame {
    /// The position of each member, in walk order. The first is the entry.
    pub(super) positions: Vec<Coord>,
    /// The slots of each member's child units, in the rank order of `Member::children`.
    pub(super) slots: Vec<Vec<Slot>>,
}

/// The child units of one ring member, and the code that a walk states for the member.
struct Exits {
    children: Vec<Unit>,
    code: String,
}

/// The placement of a ring polygon: its centre, circumradius, outward bisector at the entry, walk
/// sign and turn sign.
struct Polygon {
    centre: Coord,
    radius: f64,
    bisector: f64,
    sigma: f64,
    turn: i8,
}

/// The ring unit entered at `entry` from `parent`, the atom and bond through which it is reached.
/// Its walk is the one with the smaller code.
pub(super) fn entered(prep: &Preparation, entry: AtomId, parent: (AtomId, BondId)) -> RingNode {
    let (from, bond) = parent;
    let order = code_from(prep, bond, from);
    let index = prep.ring_of[&entry];
    let member_exits = exits(prep, index, Some(from));
    let [first, second] = ring_neighbours(prep, index, entry);
    let first_walk = walk(prep, index, entry, first);
    let second_walk = walk(prep, index, entry, second);
    let code_first = walk_code(prep, &order, &first_walk, &member_exits);
    let code_second = walk_code(prep, &order, &second_walk, &member_exits);
    if code_second < code_first {
        assemble(second_walk, code_second, member_exits)
    } else {
        assemble(first_walk, code_first, member_exits)
    }
}

/// The ring unit of a free root, which is the ring that holds `member`, entered at the rotation with
/// the smallest code.
pub(super) fn rooted(prep: &Preparation, member: AtomId) -> RingNode {
    let index = prep.ring_of[&member];
    let member_exits = exits(prep, index, None);
    let mut best: Option<(String, Vec<(AtomId, BondId)>)> = None;
    for &entry in &prep.rings[index] {
        for next in ring_neighbours(prep, index, entry) {
            let steps = walk(prep, index, entry, next);
            let code = walk_code(prep, "-", &steps, &member_exits);
            let replaces = best.as_ref().is_none_or(|(current, _)| code < *current);
            if replaces {
                best = Some((code, steps));
            }
        }
    }
    let (code, steps) = best.expect("a ring has at least one walk");
    assemble(steps, code, member_exits)
}

/// The frame of a ring entered from `parent_at` through `slot`, whose entry stands at the point that
/// `slot` reaches, `length` pt from `parent_at`. See the module documentation.
pub(super) fn entered_frame(
    ring: &RingNode,
    parent_at: Coord,
    slot: Slot,
    length: f64,
) -> RingFrame {
    let size = ring.members.len();
    let radius = circumradius(length, size);
    let entry_at = along(parent_at, slot.angle, length);
    let offsets = exterior_offsets(size, 1 + ring.members[0].children.len(), slot.turn);
    let parent_offset = offsets[0];
    let bisector = slot.angle + 180.0 - parent_offset;
    let centre = along(entry_at, slot.angle - parent_offset, radius);
    let sigma = f64::from(slot.turn);
    let polygon = Polygon {
        centre,
        radius,
        bisector,
        sigma,
        turn: slot.turn,
    };
    let entry_slots: Vec<Slot> = offsets[1..]
        .iter()
        .map(|&offset| exterior_slot(bisector, offset, slot.turn))
        .collect();
    frame(ring, &polygon, entry_at, entry_slots)
}

/// The frame of a free root ring: centred on the origin, with its entry as the rightmost vertex.
pub(super) fn root_frame(ring: &RingNode, length: f64) -> RingFrame {
    let size = ring.members.len();
    let radius = circumradius(length, size);
    let bisector = 0.0;
    let sigma = 1.0;
    let turn = 1;
    let centre = Coord::ORIGIN;
    let entry_at = along(centre, bisector, radius);
    let entry_slots: Vec<Slot> = exterior_offsets(size, ring.members[0].children.len(), turn)
        .into_iter()
        .map(|offset| exterior_slot(bisector, offset, turn))
        .collect();
    let polygon = Polygon {
        centre,
        radius,
        bisector,
        sigma,
        turn,
    };
    frame(ring, &polygon, entry_at, entry_slots)
}

/// The exocyclic units of every member of ring `index`, keyed by member. A `parent` is the atom the
/// ring is entered from, and it is never a child. A free root has no parent.
fn exits(prep: &Preparation, index: usize, parent: Option<AtomId>) -> BTreeMap<AtomId, Exits> {
    let mut map: BTreeMap<AtomId, Exits> = BTreeMap::new();
    for &atom in &prep.rings[index] {
        // The members of this ring are stated by its walk, and the parent is never a child.
        let (children, code) = child_units(prep, atom, "", |other| {
            prep.ring_of.get(&other) == Some(&index) || Some(other) == parent
        });
        map.insert(atom, Exits { children, code });
    }
    map
}

/// The two neighbours of `atom` that lie in ring `index`, ascending by identifier.
fn ring_neighbours(prep: &Preparation, index: usize, atom: AtomId) -> [(AtomId, BondId); 2] {
    let ring: Vec<(AtomId, BondId)> = neighbours(&prep.adjacency, atom)
        .iter()
        .copied()
        .filter(|(other, _)| prep.ring_of.get(other) == Some(&index))
        .collect();
    match ring.as_slice() {
        [first, second] => [*first, *second],
        _ => unreachable!("a member of a simple ring has exactly two ring neighbours"),
    }
}

/// The walk of ring `index` that leaves `entry` along `next`, the bond to its neighbour `v_1`. Its
/// steps are `(v_k, bond from v_k to v_{k+1})`, one for each member, the last one closing the ring.
fn walk(
    prep: &Preparation,
    index: usize,
    entry: AtomId,
    next: (AtomId, BondId),
) -> Vec<(AtomId, BondId)> {
    let length = prep.rings[index].len();
    let mut steps = vec![(entry, next.1)];
    let mut previous = entry;
    let mut current = next.0;
    while steps.len() < length {
        let [first, second] = ring_neighbours(prep, index, current);
        let onward = if first.0 == previous { second } else { first };
        steps.push((current, onward.1));
        previous = current;
        current = onward.0;
    }
    steps
}

/// The code of a walk: `{`, the order, `;`, then each member's code and the code of its bond to the
/// next member, joined by `/`, then `}`.
fn walk_code(
    prep: &Preparation,
    order: &str,
    walk: &[(AtomId, BondId)],
    exits: &BTreeMap<AtomId, Exits>,
) -> String {
    let parts: Vec<String> = walk
        .iter()
        .flat_map(|&(atom, bond)| [exits[&atom].code.clone(), code_from(prep, bond, atom)])
        .collect();
    format!("{{{order};{}}}", parts.join("/"))
}

/// The ring unit that `walk` states, with `code`, and whose members take their children from `exits`.
fn assemble(
    walk: Vec<(AtomId, BondId)>,
    code: String,
    mut exits: BTreeMap<AtomId, Exits>,
) -> RingNode {
    let mut members: Vec<Member> = Vec::with_capacity(walk.len());
    let mut height: usize = 0;
    let mut size = walk.len();
    for (atom, _) in walk {
        let member_exits = exits.remove(&atom).expect("every ring member has exits");
        for child in &member_exits.children {
            height = height.max(child.height() + 1);
            size += child.size();
        }
        members.push(Member {
            atom,
            children: member_exits.children,
        });
    }
    RingNode {
        members,
        height,
        size,
        code,
    }
}

/// The circumradius of a regular polygon of `size` sides of `length` pt.
fn circumradius(length: f64, size: usize) -> f64 {
    length / (2.0 * unit(180.0 / size as f64).y)
}

/// The offsets, in degrees, of `count` slots in the exterior wedge of a `size`-gon, sorted as the
/// slots of `chain` are sorted for a turn sign of `turn`.
fn exterior_offsets(size: usize, count: usize, turn: i8) -> Vec<f64> {
    let wedge = 180.0 + 360.0 / size as f64;
    let mut offsets: Vec<f64> = (1..=count)
        .map(|step| -wedge / 2.0 + wedge * step as f64 / (count + 1) as f64)
        .collect();
    offsets.sort_by(|left, right| offset_order(*left, *right, turn));
    offsets
}

/// The slot at `offset` degrees from `bisector`. Its turn sign is the side of the offset, or `turn`
/// at offset 0.
fn exterior_slot(bisector: f64, offset: f64, turn: i8) -> Slot {
    Slot {
        angle: bisector + offset,
        turn: if offset.abs() <= TIE_TOLERANCE {
            turn
        } else {
            side(offset)
        },
    }
}

/// The frame of a ring whose polygon is `polygon`, with its entry at `entry_at` and its entry's
/// child slots `entry_slots`. Each other member sits on the polygon at its bisector, and takes the
/// slots of its own child units.
fn frame(ring: &RingNode, polygon: &Polygon, entry_at: Coord, entry_slots: Vec<Slot>) -> RingFrame {
    let size = ring.members.len();
    let mut positions = vec![entry_at];
    let mut slots = vec![entry_slots];
    for (step, member) in ring.members.iter().enumerate().skip(1) {
        let theta = polygon.bisector + polygon.sigma * step as f64 * 360.0 / size as f64;
        positions.push(along(polygon.centre, theta, polygon.radius));
        let offsets = exterior_offsets(size, member.children.len(), polygon.turn);
        slots.push(
            offsets
                .into_iter()
                .map(|offset| exterior_slot(theta, offset, polygon.turn))
                .collect(),
        );
    }
    RingFrame { positions, slots }
}
