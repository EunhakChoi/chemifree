//! Test oracles that share no code with the implementation: a hand 3D oracle for the sign of a
//! label, and a naive, fully materialized digraph ranking.

use std::cmp::Ordering;

use chemifree_domain::stereo::Turn;

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
    kids: Vec<Node>,
}

impl Node {
    fn leaf(z: u8, mass: u16) -> Self {
        Self {
            z,
            mass,
            duplicate: None,
            kids: Vec::new(),
        }
    }
}

pub struct Reference {
    atoms: Vec<RefAtom>,
    edges: Vec<(usize, usize, u8)>,
    center: usize,
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
                None => self.digraph(neighbor, atom, bond, path),
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

    fn ligand(&self, neighbor: Neighbor) -> Node {
        match neighbor {
            Neighbor::Atom(atom) => {
                let order = self
                    .neighbors(self.center)
                    .into_iter()
                    .find_map(|(other, order)| (other == atom).then_some(order))
                    .expect("a ligand is bonded to the center");
                let mut path = vec![self.center];
                let mut node = self.digraph(atom, self.center, order, &mut path);
                sort(&mut node);
                node
            }
            Neighbor::Hydrogen => Node::leaf(1, 1),
            Neighbor::LonePair => Node::leaf(0, 0),
        }
    }

    /// The ranking of the listed neighbors by the full rules: `Some(ranks)` (0 highest) when
    /// all four are distinct, `None` for a tie.
    pub fn ranks(&self, listed: [Neighbor; 4]) -> Option<[usize; 4]> {
        let ligands = listed.map(|neighbor| self.ligand(neighbor));
        let mut wins = [0_i32; 4];
        for i in 0..4 {
            for j in 0..4 {
                if i != j && compare_full(&ligands[i], &ligands[j]) == Ordering::Greater {
                    wins[i] += 1;
                }
            }
        }
        let mut sorted = wins;
        sorted.sort_unstable();
        (sorted == [0, 1, 2, 3]).then(|| wins.map(|w| 3 - usize::try_from(w).unwrap()))
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

/// A key compared by one rule; a missing node reads as a phantom.
fn key_order(rule: usize, a: Option<&Node>, b: Option<&Node>) -> Ordering {
    let z = |n: Option<&Node>| n.map_or(0, |n| n.z);
    let mass = |n: Option<&Node>| n.map_or(0, |n| n.mass);
    match rule {
        0 => z(a).cmp(&z(b)),
        1 => match (a.and_then(|n| n.duplicate), b.and_then(|n| n.duplicate)) {
            (Some(x), Some(y)) => y.cmp(&x),
            _ => Ordering::Equal,
        },
        _ => mass(a).cmp(&mass(b)),
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

fn compare_rule(rule: usize, a: &Node, b: &Node) -> Ordering {
    let (sa, sb) = (spheres(a), spheres(b));
    for j in 0..sa.len().max(sb.len()) {
        let no_groups: Vec<Vec<&Node>> = Vec::new();
        let no_nodes: Vec<&Node> = Vec::new();
        let (ga, gb) = (
            sa.get(j).unwrap_or(&no_groups),
            sb.get(j).unwrap_or(&no_groups),
        );
        for g in 0..ga.len().max(gb.len()) {
            let (la, lb) = (
                ga.get(g).unwrap_or(&no_nodes),
                gb.get(g).unwrap_or(&no_nodes),
            );
            for p in 0..la.len().max(lb.len()) {
                let ordering = key_order(rule, la.get(p).copied(), lb.get(p).copied());
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
        }
    }
    Ordering::Equal
}

fn compare_full(a: &Node, b: &Node) -> Ordering {
    (0..3)
        .map(|rule| compare_rule(rule, a, b))
        .find(|ordering| *ordering != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}

/// Sorts every node's children best first by the full comparison, bottom up (bubble sort, so a
/// comparison that is not a strict weak order cannot panic the sort).
fn sort(node: &mut Node) {
    for kid in &mut node.kids {
        sort(kid);
    }
    let n = node.kids.len();
    for end in (1..n).rev() {
        for i in 0..end {
            if compare_full(&node.kids[i], &node.kids[i + 1]) == Ordering::Less {
                node.kids.swap(i, i + 1);
            }
        }
    }
}

impl Reference {
    /// The pairs of listed neighbors the full rules cannot separate.
    pub fn equal_pairs(&self, listed: [Neighbor; 4]) -> Vec<(usize, usize)> {
        let ligands = listed.map(|neighbor| self.ligand(neighbor));
        let mut pairs = Vec::new();
        for i in 0..4 {
            for j in (i + 1)..4 {
                if compare_full(&ligands[i], &ligands[j]) == Ordering::Equal {
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
