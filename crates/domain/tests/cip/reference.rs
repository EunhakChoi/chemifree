//! Test oracles that share no code with the implementation: a hand 3D oracle for the sign of a
//! label, and a naive, fully materialized digraph ranking with Rule 3.

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::BTreeMap;

use chemifree_domain::stereo::{Relation, Turn};

use crate::support::{Molecule, Neighbor};

/// The descriptor of a center from vectors alone. `ranks[i]` is the CIP rank (0 highest, 3
/// lowest) of the neighbor listed at position `i`; `turn` is the stored turn read in listed
/// order, looking from the first neighbor toward the center. Neighbor 0 sits on +z and the
/// others below it at 0, 120 and 240 degrees (anticlockwise as seen from +z for `@`); the sign
/// of the triple product of the three highest-ranked vectors, which all point away from the
/// lowest, decides: positive is anticlockwise seen with the lowest pointing away, so S.
pub fn geometry_descriptor(turn: Turn, ranks: [usize; 4]) -> char {
    let angles: [f64; 3] = match turn {
        Turn::Anticlockwise => [0.0, 120.0, 240.0],
        Turn::Clockwise => [0.0, 240.0, 120.0],
    };
    let mut vectors = vec![[0.0, 0.0, 1.0]];
    for angle in angles {
        let radians = angle.to_radians();
        vectors.push([radians.cos(), radians.sin(), -1.0 / 3.0]);
    }
    let by_rank = |rank: usize| {
        let position = ranks.iter().position(|&r| r == rank).expect("ranks 0..4");
        vectors[position]
    };
    let (a, b, c) = (by_rank(0), by_rank(1), by_rank(2));
    let cross = [
        b[1] * c[2] - b[2] * c[1],
        b[2] * c[0] - b[0] * c[2],
        b[0] * c[1] - b[1] * c[0],
    ];
    let triple = a[0] * cross[0] + a[1] * cross[1] + a[2] * cross[2];
    if triple > 0.0 { 'S' } else { 'R' }
}

/// An atom of the reference molecule: element number and mass number, hydrogens included as
/// atoms only when the molecule draws them.
#[derive(Clone, Debug)]
pub struct RefAtom {
    pub z: u8,
    pub isotope: Option<u16>,
}

const fn natural_mass(z: u8) -> u16 {
    match z {
        1 => 1,
        6 => 12,
        7 => 14,
        8 => 16,
        9 => 19,
        _ => 35,
    }
}

const fn valence(z: u8) -> usize {
    match z {
        6 => 4,
        7 => 3,
        8 => 2,
        _ => 1,
    }
}

#[derive(Clone, Debug)]
struct Node {
    z: u8,
    mass: u16,
    /// For a duplicate, the sphere of the atom it duplicates.
    duplicate: Option<usize>,
    /// For a real node reached by a double bond: the atoms of that bond (parent, node).
    double: Option<(usize, usize)>,
    kids: Vec<Node>,
}

impl Node {
    fn leaf(z: u8, mass: u16) -> Self {
        Self {
            z,
            mass,
            duplicate: None,
            double: None,
            kids: Vec::new(),
        }
    }
}

/// What Rule 3 reads of a double bond.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Geo {
    Z,
    E,
    Plain,
    /// Stereogenic but not stated; true when its own ends were ambiguous.
    Unspec(bool),
    /// Its descriptor needed itself.
    Cycle,
}

/// What the naive ranking found, with how sure it is.
pub struct Found {
    /// `Some(ranks)` (0 highest) when all four are distinct, `None` for a tie.
    pub ranks: Option<[usize; 4]>,
    /// A double bond whose geometry is not stated, or a descriptor that needs itself, took part
    /// somewhere in the digraphs: the implementation may answer `ambiguous` or refuse, and where
    /// it answers a label it must be this one.
    pub lenient: bool,
    /// A descriptor needed itself somewhere in the naive evaluation. The naive ranking sorts
    /// every sibling pair eagerly, so it can meet such a cycle where the lazy implementation
    /// never looks; its ranks are then not comparable.
    pub cycle: bool,
    /// Some end of a ligand double bond had two substituents that tie: a refusal for Rules 4
    /// and 5 is possible there.
    pub end_tie: bool,
}

struct Stored {
    begin: usize,
    end: usize,
    begin_ref: Neighbor,
    end_ref: Neighbor,
    relation: Option<Relation>,
}

pub struct Reference {
    atoms: Vec<RefAtom>,
    edges: Vec<(usize, usize, u8)>,
    center: usize,
    stored: Vec<Stored>,
    stack: RefCell<Vec<(usize, usize)>>,
    cache: RefCell<BTreeMap<(usize, usize), Geo>>,
    seen_uncertain: Cell<bool>,
    seen_end_tie: Cell<bool>,
    seen_cycle: Cell<bool>,
    ambiguous: Cell<bool>,
    recording: Cell<bool>,
}

impl Reference {
    pub fn new(molecule: &Molecule, center: usize) -> Self {
        use chemifree_domain::document::BondOrder;
        Self {
            atoms: molecule
                .atoms
                .iter()
                .map(|atom| RefAtom {
                    z: atom.kind.element().expect("element atoms").atomic_number(),
                    isotope: atom.isotope,
                })
                .collect(),
            edges: molecule
                .bonds
                .iter()
                .map(|(a, b, order)| {
                    let order = match order {
                        BondOrder::Single => 1,
                        BondOrder::Double => 2,
                        BondOrder::Triple => 3,
                        other => panic!("reference molecules have no {other:?} bonds"),
                    };
                    (*a, *b, order)
                })
                .collect(),
            center,
            stored: molecule
                .doubles
                .iter()
                .map(|double| {
                    let (begin, end, _) = molecule.bonds[double.bond];
                    Stored {
                        begin,
                        end,
                        begin_ref: double.begin,
                        end_ref: double.end,
                        relation: double.relation,
                    }
                })
                .collect(),
            stack: RefCell::new(Vec::new()),
            cache: RefCell::new(BTreeMap::new()),
            seen_uncertain: Cell::new(false),
            seen_end_tie: Cell::new(false),
            seen_cycle: Cell::new(false),
            ambiguous: Cell::new(false),
            recording: Cell::new(false),
        }
    }

    fn neighbors(&self, atom: usize) -> Vec<(usize, u8)> {
        self.edges
            .iter()
            .filter_map(|&(a, b, order)| {
                if a == atom {
                    Some((b, order))
                } else if b == atom {
                    Some((a, order))
                } else {
                    None
                }
            })
            .collect()
    }

    fn hydrogens(&self, atom: usize) -> usize {
        let used: usize = self
            .neighbors(atom)
            .iter()
            .map(|&(_, o)| usize::from(o))
            .sum();
        valence(self.atoms[atom].z).saturating_sub(used)
    }

    fn mass(&self, atom: usize) -> u16 {
        self.atoms[atom]
            .isotope
            .unwrap_or_else(|| natural_mass(self.atoms[atom].z))
    }

    /// The whole digraph below `atom`, reached from `parent` by a bond of `order`, with `path`
    /// holding the atoms from the root (the center) to `parent` (sphere = index).
    fn digraph(&self, atom: usize, parent: usize, order: u8, path: &mut Vec<usize>) -> Node {
        let depth = path.len();
        let mut node = Node::leaf(self.atoms[atom].z, self.mass(atom));
        path.push(atom);
        for (neighbor, bond) in self.neighbors(atom) {
            if neighbor == parent {
                continue;
            }
            let closing = path.iter().position(|&on_path| on_path == neighbor);
            let sphere = closing.unwrap_or(depth + 1);
            let child = match closing {
                Some(at) => Node {
                    duplicate: Some(at),
                    ..Node::leaf(self.atoms[neighbor].z, self.mass(neighbor))
                },
                None => {
                    let mut kid = self.digraph(neighbor, atom, bond, path);
                    if bond == 2 {
                        kid.double = Some((atom, neighbor));
                    }
                    kid
                }
            };
            node.kids.push(child);
            for _ in 1..bond {
                node.kids.push(Node {
                    duplicate: Some(sphere),
                    ..Node::leaf(self.atoms[neighbor].z, self.mass(neighbor))
                });
            }
        }
        for _ in 1..order {
            node.kids.push(Node {
                duplicate: Some(depth - 1),
                ..Node::leaf(self.atoms[parent].z, self.mass(parent))
            });
        }
        for _ in 0..self.hydrogens(atom) {
            node.kids.push(Node::leaf(1, 1));
        }
        path.pop();
        node
    }

    fn ligand(&self, center: usize, neighbor: Neighbor) -> Node {
        match neighbor {
            Neighbor::Atom(atom) => {
                let order = self
                    .neighbors(center)
                    .into_iter()
                    .find_map(|(other, order)| (other == atom).then_some(order))
                    .expect("a ligand is bonded to the center");
                let mut path = vec![center];
                let mut node = self.digraph(atom, center, order, &mut path);
                self.sort(&mut node);
                node
            }
            Neighbor::Hydrogen => Node::leaf(1, 1),
            Neighbor::LonePair => Node::leaf(0, 0),
        }
    }

    /// The ranking of the listed neighbors by the full rules.
    pub fn ranks(&self, listed: [Neighbor; 4]) -> Found {
        self.seen_uncertain.set(false);
        self.seen_end_tie.set(false);
        self.seen_cycle.set(false);
        self.ambiguous.set(false);
        self.cache.borrow_mut().clear();
        let ligands = listed.map(|neighbor| self.ligand(self.center, neighbor));
        self.recording.set(true);
        let mut wins = [0_i32; 4];
        for i in 0..4 {
            for j in 0..4 {
                if i != j && self.compare_full(&ligands[i], &ligands[j]) == Ordering::Greater {
                    wins[i] += 1;
                }
            }
        }
        self.recording.set(false);
        let mut sorted = wins;
        sorted.sort_unstable();
        Found {
            ranks: (sorted == [0, 1, 2, 3]).then(|| wins.map(|w| 3 - usize::try_from(w).unwrap())),
            lenient: self.seen_uncertain.get() || self.ambiguous.get(),
            cycle: self.seen_cycle.get(),
            end_tie: self.seen_end_tie.get(),
        }
    }

    /// The two substituent positions at `end` of the double bond to `partner`: the other
    /// neighbors, the hydrogens the valence gives, and lone pairs for the rest.
    fn positions(&self, end: usize, partner: usize) -> [Neighbor; 2] {
        let mut list: Vec<Neighbor> = self
            .neighbors(end)
            .into_iter()
            .filter(|&(other, _)| other != partner)
            .map(|(other, _)| Neighbor::Atom(other))
            .collect();
        for _ in 0..self.hydrogens(end) {
            list.push(Neighbor::Hydrogen);
        }
        while list.len() < 2 {
            list.push(Neighbor::LonePair);
        }
        [list[0], list[1]]
    }

    /// Ranks the two positions at one end: `Some(index of the higher)`, or `None` for a tie.
    /// Ambiguity found inside is passed on through the second value.
    fn higher_at(&self, end: usize, partner: usize) -> (Option<usize>, bool) {
        let positions = self.positions(end, partner);
        let outer = self.ambiguous.replace(false);
        let first = self.ligand(end, positions[0]);
        let second = self.ligand(end, positions[1]);
        let was = self.recording.replace(true);
        let ordering = self.compare_full(&first, &second);
        self.recording.set(was);
        let inner = self.ambiguous.replace(outer);
        let higher = match ordering {
            Ordering::Greater => Some(0),
            Ordering::Less => Some(1),
            Ordering::Equal => {
                self.seen_end_tie.set(true);
                None
            }
        };
        (higher, inner)
    }

    /// The side (up or down) a position takes when the reference position of an end is up.
    /// Independent of the implementation's flip counting: sides are assigned, then compared.
    fn side(
        positions: [Neighbor; 2],
        reference: Neighbor,
        relation_up: bool,
        index: usize,
    ) -> bool {
        let reference_index = positions
            .iter()
            .position(|&position| position == reference)
            .expect("the stored reference is one of the two positions");
        if index == reference_index {
            relation_up
        } else {
            !relation_up
        }
    }

    /// The descriptor of the double bond between `a` and `b`, read from the stored record
    /// through the ranking of both ends.
    fn geometry(&self, a: usize, b: usize) -> Geo {
        let key = (a.min(b), a.max(b));
        if let Some(&found) = self.cache.borrow().get(&key) {
            return found;
        }
        if self.stack.borrow().contains(&key) {
            self.ambiguous.set(true);
            self.seen_cycle.set(true);
            return Geo::Cycle;
        }
        self.stack.borrow_mut().push(key);
        let (high_a, amb_a) = self.higher_at(a, b);
        let (high_b, amb_b) = self.higher_at(b, a);
        self.stack.borrow_mut().pop();
        let found = match (high_a, high_b) {
            (None, _) | (_, None) => Geo::Plain,
            _ if amb_a || amb_b => Geo::Unspec(true),
            (Some(ha), Some(hb)) => {
                let record = self.stored.iter().find(|stored| {
                    (stored.begin.min(stored.end), stored.begin.max(stored.end)) == key
                });
                match record.and_then(|stored| stored.relation.map(|relation| (stored, relation))) {
                    None => Geo::Unspec(false),
                    Some((stored, relation)) => {
                        let (pa, pb) = (
                            self.positions(stored.begin, stored.end),
                            self.positions(stored.end, stored.begin),
                        );
                        let (higher_begin, higher_end) = if stored.begin == a {
                            (ha, hb)
                        } else {
                            (hb, ha)
                        };
                        let up_begin = Self::side(pa, stored.begin_ref, true, higher_begin);
                        let reference_end_up = relation == Relation::SameSide;
                        let up_end = Self::side(pb, stored.end_ref, reference_end_up, higher_end);
                        if up_begin == up_end { Geo::Z } else { Geo::E }
                    }
                }
            }
        };
        if matches!(found, Geo::Unspec(_)) {
            self.seen_uncertain.set(true);
        }
        self.cache.borrow_mut().insert(key, found);
        found
    }

    /// The label of the double bond between `a` and `b`: `Z`, `E`, `none`, or `None` when the
    /// naive ranking cannot say (unstated geometry inside).
    pub fn bond_label(&self, a: usize, b: usize) -> Option<&'static str> {
        self.cache.borrow_mut().clear();
        self.seen_uncertain.set(false);
        self.seen_cycle.set(false);
        self.ambiguous.set(false);
        let found = self.geometry(a, b);
        if self.seen_cycle.get() {
            return None;
        }
        match found {
            Geo::Z => Some("Z"),
            Geo::E => Some("E"),
            Geo::Plain => Some("none"),
            Geo::Unspec(_) | Geo::Cycle => None,
        }
    }

    /// Whether the double bond between `a` and `b` is stored with a relation (not unknown).
    pub fn is_stored(&self, a: usize, b: usize) -> bool {
        self.stored.iter().any(|stored| {
            (stored.begin.min(stored.end), stored.begin.max(stored.end)) == (a.min(b), a.max(b))
                && stored.relation.is_some()
        })
    }

    /// A canonical text of the plain subtree below `atom` (no duplicates), for the structural
    /// check that two ligands called equivalent really are.
    pub fn canonical(&self, atom: usize, parent: usize, order: u8) -> String {
        let mut kids: Vec<String> = self
            .neighbors(atom)
            .into_iter()
            .filter(|&(neighbor, _)| neighbor != parent)
            .map(|(neighbor, bond)| self.canonical(neighbor, atom, bond))
            .collect();
        kids.sort();
        format!(
            "({}:{}:{}:h{}[{}])",
            self.atoms[atom].z,
            self.mass(atom),
            order,
            self.hydrogens(atom),
            kids.concat()
        )
    }
}

impl Reference {
    /// What Rule 3 reads of a node: the descriptor of the double bond that leads to it.
    fn node_geo(&self, node: Option<&Node>) -> Option<Geo> {
        let (a, b) = node?.double?;
        Some(self.geometry(a, b))
    }

    /// A key compared by one rule; a missing node reads as a phantom.
    fn key_order(&self, rule: usize, a: Option<&Node>, b: Option<&Node>) -> Ordering {
        let z = |n: Option<&Node>| n.map_or(0, |n| n.z);
        let mass = |n: Option<&Node>| n.map_or(0, |n| n.mass);
        match rule {
            0 => z(a).cmp(&z(b)),
            1 => match (a.and_then(|n| n.duplicate), b.and_then(|n| n.duplicate)) {
                (Some(x), Some(y)) => y.cmp(&x),
                _ => Ordering::Equal,
            },
            2 => mass(a).cmp(&mass(b)),
            _ => {
                let (ga, gb) = (self.node_geo(a), self.node_geo(b));
                let key = |g: Option<Geo>| match g {
                    Some(Geo::Z) => 3,
                    Some(Geo::E) => 2,
                    Some(Geo::Unspec(_)) => 1,
                    _ => 0,
                };
                let unspec = |g: Option<Geo>| matches!(g, Some(Geo::Unspec(_)));
                let settled = |g: Option<Geo>, other: Option<Geo>| match g {
                    Some(Geo::Unspec(maybe_plain)) => {
                        !maybe_plain && matches!(other, None | Some(Geo::Plain | Geo::Cycle))
                    }
                    _ => true,
                };
                if self.recording.get()
                    && (unspec(ga) || unspec(gb))
                    && !(settled(ga, gb) && settled(gb, ga))
                {
                    self.ambiguous.set(true);
                }
                key(ga).cmp(&key(gb))
            }
        }
    }

    fn compare_rule(&self, rule: usize, a: &Node, b: &Node) -> Ordering {
        let (sa, sb) = (spheres(a), spheres(b));
        for j in 0..sa.len().max(sb.len()) {
            let no_groups: Vec<Vec<&Node>> = Vec::new();
            let no_nodes: Vec<&Node> = Vec::new();
            let (ga, gb) = (
                sa.get(j).unwrap_or(&no_groups),
                sb.get(j).unwrap_or(&no_groups),
            );
            if rule == 3 {
                // Every descriptor of a sphere is read before the sphere is compared.
                for node in ga.iter().chain(gb).flatten() {
                    self.node_geo(Some(node));
                }
            }
            for g in 0..ga.len().max(gb.len()) {
                let (la, lb) = (
                    ga.get(g).unwrap_or(&no_nodes),
                    gb.get(g).unwrap_or(&no_nodes),
                );
                for p in 0..la.len().max(lb.len()) {
                    let ordering = self.key_order(rule, la.get(p).copied(), lb.get(p).copied());
                    if ordering != Ordering::Equal {
                        return ordering;
                    }
                }
            }
        }
        Ordering::Equal
    }

    fn compare_full(&self, a: &Node, b: &Node) -> Ordering {
        (0..4)
            .map(|rule| self.compare_rule(rule, a, b))
            .find(|ordering| *ordering != Ordering::Equal)
            .unwrap_or(Ordering::Equal)
    }

    /// Sorts every node's children best first by the full comparison, bottom up (bubble sort,
    /// so a comparison that is not a strict weak order cannot panic the sort).
    fn sort(&self, node: &mut Node) {
        for kid in &mut node.kids {
            self.sort(kid);
        }
        let n = node.kids.len();
        for end in (1..n).rev() {
            for i in 0..end {
                if self.compare_full(&node.kids[i], &node.kids[i + 1]) == Ordering::Less {
                    node.kids.swap(i, i + 1);
                }
            }
        }
    }
}

/// Spheres of a (sorted) subtree: sphere 0 is the node; each next sphere has one group of
/// children per node of the previous sphere, in order.
fn spheres(root: &Node) -> Vec<Vec<Vec<&Node>>> {
    let mut all = vec![vec![vec![root]]];
    loop {
        let last = all.last().expect("sphere 0");
        let next: Vec<Vec<&Node>> = last
            .iter()
            .flatten()
            .map(|node| node.kids.iter().collect())
            .collect();
        if next.iter().all(Vec::is_empty) {
            return all;
        }
        all.push(next);
    }
}

impl Reference {
    /// The pairs of listed neighbors the full rules cannot separate.
    pub fn equal_pairs(&self, listed: [Neighbor; 4]) -> Vec<(usize, usize)> {
        let ligands = listed.map(|neighbor| self.ligand(self.center, neighbor));
        let mut pairs = Vec::new();
        for i in 0..4 {
            for j in (i + 1)..4 {
                if self.compare_full(&ligands[i], &ligands[j]) == Ordering::Equal {
                    pairs.push((i, j));
                }
            }
        }
        pairs
    }

    /// The canonical text of the ligand at a listed position, for acyclic molecules.
    pub fn canonical_ligand(&self, neighbor: Neighbor) -> String {
        match neighbor {
            Neighbor::Atom(atom) => {
                let order = self
                    .neighbors(self.center)
                    .into_iter()
                    .find_map(|(other, order)| (other == atom).then_some(order))
                    .expect("bonded");
                self.canonical(atom, self.center, order)
            }
            Neighbor::Hydrogen => "H".to_owned(),
            Neighbor::LonePair => "LP".to_owned(),
        }
    }
}
