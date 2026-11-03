//! Test support: documents built by commands, and a small SMILES reader that states the
//! configurations a SMILES string implies. The reader is test code only; the fixtures it reads
//! are also read by RDKit (`scripts/verify-cip-rdkit.py`), which does not use it.

use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondOrder, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::states::Hydrogens;
use chemifree_domain::stereo::{AtomConfiguration, SlotRef, Turn};

/// One neighbor of a stereocenter as the SMILES lists it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Neighbor {
    Atom(usize),
    /// The implicit hydrogen or the lone pair the SMILES leaves unwritten.
    Hydrogen,
    LonePair,
}

#[derive(Clone, Debug)]
pub struct Center {
    pub atom: usize,
    pub neighbors: [Neighbor; 4],
    pub turn: Turn,
}

/// A molecule as plain data, before it is a document.
#[derive(Clone, Debug, Default)]
pub struct Molecule {
    pub atoms: Vec<Atom>,
    pub bonds: Vec<(usize, usize, BondOrder)>,
    pub centers: Vec<Center>,
}

pub struct Built {
    pub document: Document,
    /// The atom id of each atom of the molecule, by its index.
    pub ids: Vec<AtomId>,
}

impl Built {
    pub fn graph(&self) -> MoleculeGraph {
        MoleculeGraph::from_document(&self.document)
    }
}

fn point(index: usize) -> Point {
    Point::new(index as f64, 0.0).unwrap()
}

impl Molecule {
    pub fn mirrored(&self) -> Self {
        let mut mirror = self.clone();
        for center in &mut mirror.centers {
            center.turn = center.turn.flipped();
        }
        mirror
    }

    /// The document with atoms added in `atom_order` and bonds in `bond_order` (both
    /// permutations of the indices), so ids and insertion orders differ from the molecule's.
    pub fn build_ordered(&self, atom_order: &[usize], bond_order: &[usize]) -> Built {
        let mut document =
            Document::new(DocumentId::parse("cip-test").unwrap(), "cip test").unwrap();
        let mut ids = vec![None; self.atoms.len()];
        let mut commands = Vec::new();
        for &index in atom_order {
            commands.push(Command::AddAtom {
                atom: self.atoms[index].clone(),
                positions: NewAtomPositions::Single(point(index)),
                bond_to: None,
            });
        }
        document.apply(&commands).unwrap();
        for (&index, (id, _)) in atom_order.iter().zip(document.atoms()) {
            ids[index] = Some(id);
        }
        let ids: Vec<AtomId> = ids.into_iter().map(Option::unwrap).collect();
        let mut commands = Vec::new();
        for &index in bond_order {
            let (begin, end, order) = &self.bonds[index];
            commands.push(Command::AddBond {
                begin: AtomRef::Id(ids[*begin]),
                end: AtomRef::Id(ids[*end]),
                order: order.clone(),
            });
        }
        for center in &self.centers {
            let neighbors = center.neighbors.map(|neighbor| match neighbor {
                Neighbor::Atom(index) => SlotRef::Atom(AtomRef::Id(ids[index])),
                Neighbor::Hydrogen => SlotRef::ImplicitHydrogen,
                Neighbor::LonePair => SlotRef::LonePair,
            });
            commands.push(Command::SetAtomConfiguration {
                atom: AtomRef::Id(ids[center.atom]),
                configuration: Some(AtomConfiguration::Tetrahedral {
                    neighbors,
                    turn: center.turn,
                }),
            });
        }
        document.apply(&commands).unwrap();
        Built { document, ids }
    }

    pub fn build(&self) -> Built {
        let atoms: Vec<usize> = (0..self.atoms.len()).collect();
        let bonds: Vec<usize> = (0..self.bonds.len()).collect();
        self.build_ordered(&atoms, &bonds)
    }
}

/// A bare element atom with inferred hydrogens.
pub fn element(symbol: &str) -> Atom {
    Atom::from(Element::from_symbol(symbol).unwrap())
}

struct Parser<'a> {
    text: &'a [u8],
    position: usize,
    molecule: Molecule,
    aromatic: Vec<bool>,
    /// For each atom, the neighbors in SMILES order (ring-closure digits at their position).
    neighbors: Vec<Vec<Option<Neighbor>>>,
    ring_open: [Option<(usize, usize)>; 10],
    chirality: Vec<Option<Turn>>,
    preceded: Vec<bool>,
    stated_hydrogens: Vec<Option<u8>>,
}

/// Reads the SMILES subset the fixtures use: organic atoms, bracket atoms with isotope,
/// `@`/`@@`, hydrogen count and charge, `-`, `=`, `#`, branches, ring closures `0` to `9`,
/// lowercase `c` and `n` (aromatic bonds between adjacent aromatic atoms). The neighbor order of
/// a stereocenter follows OpenSMILES: the preceding atom, the implicit hydrogen or lone pair,
/// ring-closure digits in the order written, then branches and the next atom.
pub fn parse(smiles: &str) -> Molecule {
    let mut parser = Parser {
        text: smiles.as_bytes(),
        position: 0,
        molecule: Molecule::default(),
        aromatic: Vec::new(),
        neighbors: Vec::new(),
        ring_open: [None; 10],
        chirality: Vec::new(),
        preceded: Vec::new(),
        stated_hydrogens: Vec::new(),
    };
    parser.run();
    parser.finish()
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.get(self.position).copied()
    }

    fn run(&mut self) {
        let mut previous: Option<usize> = None;
        let mut stack: Vec<Option<usize>> = Vec::new();
        let mut order: Option<BondOrder> = None;
        while let Some(byte) = self.peek() {
            match byte {
                b'(' => {
                    stack.push(previous);
                    self.position += 1;
                }
                b')' => {
                    previous = stack.pop().expect("balanced branches");
                    self.position += 1;
                }
                b'-' => {
                    order = Some(BondOrder::Single);
                    self.position += 1;
                }
                b'=' => {
                    order = Some(BondOrder::Double);
                    self.position += 1;
                }
                b'#' => {
                    order = Some(BondOrder::Triple);
                    self.position += 1;
                }
                b'0'..=b'9' => {
                    let digit = usize::from(byte - b'0');
                    self.position += 1;
                    let current = previous.expect("a ring closure follows an atom");
                    self.ring(current, digit, order.take());
                }
                _ => {
                    let index = self.atom();
                    if let Some(from) = previous {
                        self.preceded[index] = true;
                        self.bond(from, index, order.take());
                    }
                    previous = Some(index);
                }
            }
        }
    }

    fn bond(&mut self, from: usize, to: usize, order: Option<BondOrder>) {
        let order = order.unwrap_or(if self.aromatic[from] && self.aromatic[to] {
            BondOrder::Aromatic
        } else {
            BondOrder::Single
        });
        self.molecule.bonds.push((from, to, order));
        self.neighbors[from].push(Some(Neighbor::Atom(to)));
        self.neighbors[to].push(Some(Neighbor::Atom(from)));
    }

    fn ring(&mut self, atom: usize, digit: usize, order: Option<BondOrder>) {
        match self.ring_open[digit].take() {
            None => {
                self.neighbors[atom].push(None);
                self.ring_open[digit] = Some((atom, self.neighbors[atom].len() - 1));
            }
            Some((other, slot)) => {
                let order = order.unwrap_or(if self.aromatic[atom] && self.aromatic[other] {
                    BondOrder::Aromatic
                } else {
                    BondOrder::Single
                });
                self.molecule.bonds.push((other, atom, order));
                self.neighbors[other][slot] = Some(Neighbor::Atom(atom));
                self.neighbors[atom].push(Some(Neighbor::Atom(other)));
            }
        }
    }

    /// Reads one atom and returns its index. The neighbor list starts with the marker for the
    /// implicit hydrogen or lone pair after the preceding atom, which `bond` adds first.
    fn atom(&mut self) -> usize {
        let index = self.molecule.atoms.len();
        let mut atom;
        let mut chirality = None;
        let mut stated = None;
        let mut aromatic = false;
        if self.peek() == Some(b'[') {
            self.position += 1;
            let mut isotope = 0_u16;
            while let Some(digit) = self.peek().filter(u8::is_ascii_digit) {
                isotope = isotope * 10 + u16::from(digit - b'0');
                self.position += 1;
            }
            let symbol = self.symbol(true);
            atom = element(&symbol);
            if isotope != 0 {
                atom.isotope = Some(isotope);
            }
            while self.peek() == Some(b'@') {
                self.position += 1;
                chirality = Some(if self.peek() == Some(b'@') {
                    self.position += 1;
                    Turn::Clockwise
                } else {
                    Turn::Anticlockwise
                });
            }
            let mut hydrogens = 0_u8;
            if self.peek() == Some(b'H') {
                self.position += 1;
                hydrogens = 1;
                if let Some(digit) = self.peek().filter(u8::is_ascii_digit) {
                    hydrogens = digit - b'0';
                    self.position += 1;
                }
            }
            if let Some(sign @ (b'+' | b'-')) = self.peek() {
                self.position += 1;
                let mut magnitude = 1_i8;
                if let Some(digit) = self.peek().filter(u8::is_ascii_digit) {
                    magnitude = i8::try_from(digit - b'0').unwrap();
                    self.position += 1;
                }
                atom.charge = if sign == b'+' { magnitude } else { -magnitude };
            }
            assert_eq!(self.peek(), Some(b']'), "bracket atom in {:?}", self.text);
            self.position += 1;
            atom.hydrogens = Hydrogens::Specified(hydrogens);
            stated = Some(hydrogens);
        } else {
            let symbol = self.symbol(false);
            aromatic = symbol.chars().all(|c| c.is_ascii_lowercase());
            let mut capitalized = symbol.clone();
            capitalized[..1].make_ascii_uppercase();
            atom = element(&capitalized);
        }
        self.molecule.atoms.push(atom);
        self.aromatic.push(aromatic);
        self.neighbors.push(Vec::new());
        self.chirality.push(chirality);
        self.stated_hydrogens.push(stated);
        self.preceded.push(false);
        index
    }

    /// Outside brackets only `Cl` and `Br` have two letters; inside any capital may be
    /// followed by a lowercase letter.
    fn symbol(&mut self, bracket: bool) -> String {
        let first = self.peek().expect("an atom symbol");
        self.position += 1;
        let mut symbol = String::from(char::from(first));
        if let Some(second) = self.peek()
            && (if bracket {
                first.is_ascii_uppercase() && second.is_ascii_lowercase()
            } else {
                (first == b'C' && second == b'l') || (first == b'B' && second == b'r')
            })
        {
            symbol.push(char::from(second));
            self.position += 1;
        }
        symbol
    }

    fn finish(mut self) -> Molecule {
        assert!(
            self.ring_open.iter().all(Option::is_none),
            "unclosed ring in {:?}",
            self.text
        );
        for index in 0..self.molecule.atoms.len() {
            let Some(turn) = self.chirality[index] else {
                continue;
            };
            let mut list: Vec<Neighbor> = self.neighbors[index]
                .iter()
                .map(|neighbor| neighbor.expect("ring closures are closed"))
                .collect();
            // The implicit hydrogen (or the lone pair of a three-coordinate center) sits right
            // after the preceding atom, or first when there is none.
            let fourth = if self.stated_hydrogens[index] == Some(1) {
                Some(Neighbor::Hydrogen)
            } else if list.len() == 3 {
                Some(Neighbor::LonePair)
            } else {
                None
            };
            if let Some(fourth) = fourth {
                list.insert(usize::from(self.preceded[index]), fourth);
            }
            let neighbors: [Neighbor; 4] = list
                .try_into()
                .unwrap_or_else(|list: Vec<Neighbor>| panic!("{} neighbors", list.len()));
            self.molecule.centers.push(Center {
                atom: index,
                neighbors,
                turn,
            });
        }
        self.molecule
    }
}
