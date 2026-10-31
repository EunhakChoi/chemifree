//! Bounded basic SMILES syntax. Stereo and query extensions are explicitly refused.

use std::collections::{BTreeMap, BTreeSet};

use crate::aromaticity;
use crate::document::{Atom, AtomId, Bond, BondId, BondOrder};
use crate::element::Element;
use crate::graph::MoleculeGraph;
use crate::identity::{self, IdentityError};
use crate::states::{Hydrogens, Radical};

/// Stable only within the explicitly bounded valence, aromaticity, and identity profiles.
pub const CANONICAL_MODEL_ID: &str = "chemifree-smiles-canonical-1";
pub const MAX_ATOMS: usize = 2000;
pub const MAX_BONDS: usize = 4000;
pub const MAX_BRANCH_DEPTH: usize = 128;
pub const MAX_SMILES_BYTES: usize = 128 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct SmilesAtom {
    pub atom: Atom,
    pub aromatic: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SmilesBond {
    pub begin: usize,
    pub end: usize,
    pub order: BondOrder,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SmilesGraph {
    pub atoms: Vec<SmilesAtom>,
    pub bonds: Vec<SmilesBond>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyntaxError {
    pub code: &'static str,
    /// Byte offsets in the UTF-8 input, half-open. End-of-input errors may be empty.
    pub start: usize,
    pub end: usize,
}

struct Ring {
    atom: usize,
    order: Option<BondOrder>,
    start: usize,
    end: usize,
}
struct Branch {
    parent: usize,
    atom_count: usize,
    start: usize,
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    position: usize,
    graph: SmilesGraph,
    atom_spans: Vec<(usize, usize)>,
    current: Option<usize>,
    bond: Option<(BondOrder, usize)>,
    rings: BTreeMap<u8, Ring>,
    branches: Vec<Branch>,
}

impl Parser<'_> {
    fn error(&self, code: &'static str, start: usize, end: usize) -> SyntaxError {
        // Call sites count grammar bytes, but errors can point into a malformed non-ASCII
        // token. Expand outwards to UTF-8 boundaries so a client can safely slice the input.
        let mut start = start.min(self.text.len());
        let mut end = end.min(self.text.len());
        while !self.text.is_char_boundary(start) {
            start -= 1;
        }
        while !self.text.is_char_boundary(end) {
            end += 1;
        }
        SyntaxError {
            code,
            start,
            end: end.max(start),
        }
    }

    fn number(&mut self) -> Option<u32> {
        let begin = self.position;
        let mut value = 0_u32;
        while let Some(&b) = self.bytes.get(self.position) {
            if !b.is_ascii_digit() {
                break;
            }
            value = value.saturating_mul(10).saturating_add(u32::from(b - b'0'));
            self.position += 1;
        }
        (begin != self.position).then_some(value)
    }

    fn atom(&mut self) -> Result<SmilesAtom, SyntaxError> {
        let start = self.position;
        let bracketed = self.bytes[self.position] == b'[';
        if bracketed {
            self.position += 1;
        }
        let isotope = if bracketed { self.number() } else { None };
        if isotope.is_some_and(|n| n == 0 || n > 999) {
            return Err(self.error("isotope_out_of_range", start, self.position));
        }
        let Some(&first) = self.bytes.get(self.position) else {
            return Err(self.error("expected_element", self.position, self.position));
        };
        if first == b'*' {
            return Err(self.error("query_unsupported", self.position, self.position + 1));
        }
        let aromatic = matches!(first, b'b' | b'c' | b'n' | b'o' | b'p' | b's' | b'a');
        let mut symbol = String::new();
        if aromatic {
            symbol.push(char::from(first.to_ascii_uppercase()));
            self.position += 1;
            if bracketed
                && matches!(first, b's' | b'a')
                && self.bytes.get(self.position) == Some(&b'e')
            {
                symbol.push('e');
                self.position += 1;
            } else if bracketed && first == b'a' && self.bytes.get(self.position) == Some(&b's') {
                symbol.push('s');
                self.position += 1;
            }
            if !matches!(
                symbol.as_str(),
                "B" | "C" | "N" | "O" | "P" | "S" | "Se" | "As"
            ) {
                return Err(self.error("unknown_element", start, self.position));
            }
        } else if first.is_ascii_uppercase() {
            symbol.push(char::from(first));
            self.position += 1;
            if self
                .bytes
                .get(self.position)
                .is_some_and(u8::is_ascii_lowercase)
            {
                let candidate = format!("{}{}", symbol, char::from(self.bytes[self.position]));
                if bracketed || matches!(candidate.as_str(), "Cl" | "Br") {
                    symbol = candidate;
                    self.position += 1;
                }
            }
            if !bracketed
                && !matches!(
                    symbol.as_str(),
                    "B" | "C" | "N" | "O" | "P" | "S" | "F" | "Cl" | "Br" | "I"
                )
            {
                return Err(self.error("brackets_required", start, self.position));
            }
        } else {
            let end = self.text[self.position..]
                .chars()
                .next()
                .map_or(self.position, |c| self.position + c.len_utf8());
            return Err(self.error("expected_element", self.position, end));
        }
        let Some(element) = Element::from_symbol(&symbol) else {
            return Err(self.error("unknown_element", start, self.position));
        };
        let mut atom = Atom::from(element);
        atom.isotope = isotope.map(|n| n as u16);
        if bracketed {
            atom.hydrogens = Hydrogens::Specified(0);
            if self.bytes.get(self.position) == Some(&b'@') {
                return Err(self.error("stereo_unsupported", self.position, self.position + 1));
            }
            if self.bytes.get(self.position) == Some(&b'H') {
                let hydrogen_start = self.position;
                self.position += 1;
                let count = self.number().unwrap_or(1);
                if symbol == "H" {
                    return Err(self.error(
                        "hydrogen_count_on_hydrogen",
                        hydrogen_start,
                        self.position,
                    ));
                }
                if count > 16 {
                    return Err(self.error("hydrogens_out_of_range", start, self.position));
                }
                atom.hydrogens = Hydrogens::Specified(count as u8);
            }
            if let Some(sign @ (b'+' | b'-')) = self.bytes.get(self.position).copied() {
                self.position += 1;
                let digits = self.number();
                if digits == Some(0) {
                    return Err(self.error("invalid_charge", start, self.position));
                }
                let mut magnitude = digits.unwrap_or(1);
                if digits.is_none() {
                    while self.bytes.get(self.position) == Some(&sign) {
                        magnitude += 1;
                        self.position += 1;
                    }
                }
                if magnitude > 8 {
                    return Err(self.error("charge_out_of_range", start, self.position));
                }
                atom.charge = if sign == b'+' {
                    magnitude as i8
                } else {
                    -(magnitude as i8)
                };
            }
            if self.bytes.get(self.position) != Some(&b']') {
                let end = (self.position + 1).min(self.bytes.len());
                let code = if self.bytes.get(self.position) == Some(&b'@') {
                    "stereo_unsupported"
                } else {
                    "invalid_bracket_atom"
                };
                return Err(self.error(code, self.position, end));
            }
            self.position += 1;
        }
        Ok(SmilesAtom { atom, aromatic })
    }

    fn connect(
        &mut self,
        a: usize,
        b: usize,
        order: Option<BondOrder>,
        start: usize,
    ) -> Result<(), SyntaxError> {
        if a == b
            || self.graph.bonds.iter().any(|bond| {
                (bond.begin == a && bond.end == b) || (bond.begin == b && bond.end == a)
            })
        {
            return Err(self.error("duplicate_or_self_bond", start, self.position));
        }
        if self.graph.bonds.len() >= MAX_BONDS {
            return Err(self.error("bond_limit", start, self.position));
        }
        let order = order.unwrap_or_else(|| {
            if self.graph.atoms[a].aromatic && self.graph.atoms[b].aromatic {
                BondOrder::Aromatic
            } else {
                BondOrder::Single
            }
        });
        if order == BondOrder::Aromatic
            && !(self.graph.atoms[a].aromatic && self.graph.atoms[b].aromatic)
        {
            return Err(self.error("invalid_aromatic_bond", start, self.position));
        }
        self.graph.bonds.push(SmilesBond {
            begin: a,
            end: b,
            order,
        });
        Ok(())
    }

    fn ring(&mut self) -> Result<(), SyntaxError> {
        let start = self.position;
        let Some(current) = self.current else {
            return Err(self.error("ring_without_atom", start, start + 1));
        };
        if self
            .branches
            .last()
            .is_some_and(|b| b.atom_count == self.graph.atoms.len())
        {
            return Err(self.error("empty_branch", start, start + 1));
        }
        let label = if self.bytes[start] == b'%' {
            if self
                .bytes
                .get(start + 1)
                .is_none_or(|c| !c.is_ascii_digit())
                || self
                    .bytes
                    .get(start + 2)
                    .is_none_or(|c| !c.is_ascii_digit())
            {
                return Err(self.error(
                    "invalid_ring_number",
                    start,
                    (start + 3).min(self.bytes.len()),
                ));
            }
            self.position += 3;
            (self.bytes[start + 1] - b'0') * 10 + self.bytes[start + 2] - b'0'
        } else {
            self.position += 1;
            self.bytes[start] - b'0'
        };
        let order = self.bond.take().map(|(order, _)| order);
        if let Some(open) = self.rings.remove(&label) {
            if open.order.is_some() && order.is_some() && open.order != order {
                return Err(self.error("ring_bond_conflict", start, self.position));
            }
            self.connect(open.atom, current, order.or(open.order), start)?;
        } else {
            self.rings.insert(
                label,
                Ring {
                    atom: current,
                    order,
                    start,
                    end: self.position,
                },
            );
        }
        Ok(())
    }
}

/// Parse a complete SMILES string. Errors use byte spans; no partial graph is returned.
pub fn parse(text: &str) -> Result<SmilesGraph, SyntaxError> {
    let mut p = Parser {
        text,
        bytes: text.as_bytes(),
        position: 0,
        graph: SmilesGraph::default(),
        atom_spans: Vec::new(),
        current: None,
        bond: None,
        rings: BTreeMap::new(),
        branches: Vec::new(),
    };
    if text.is_empty() {
        return Err(p.error("empty_input", 0, 0));
    }
    if text.len() > MAX_SMILES_BYTES {
        return Err(p.error("input_limit", MAX_SMILES_BYTES, text.len()));
    }
    while p.position < p.bytes.len() {
        let start = p.position;
        match p.bytes[start] {
            b'(' => {
                let Some(parent) = p.current else {
                    return Err(p.error("branch_without_atom", start, start + 1));
                };
                if p.bond.is_some() {
                    return Err(p.error("expected_atom", start, start + 1));
                }
                if p.branches
                    .last()
                    .is_some_and(|branch| branch.atom_count == p.graph.atoms.len())
                {
                    return Err(p.error("branch_without_atom", start, start + 1));
                }
                if p.branches.len() >= MAX_BRANCH_DEPTH {
                    return Err(p.error("branch_limit", start, start + 1));
                }
                p.branches.push(Branch {
                    parent,
                    atom_count: p.graph.atoms.len(),
                    start,
                });
                p.position += 1;
            }
            b')' => {
                let Some(branch) = p.branches.pop() else {
                    return Err(p.error("unexpected_branch_end", start, start + 1));
                };
                if p.graph.atoms.len() == branch.atom_count
                    || p.bond.is_some()
                    || p.current.is_none()
                {
                    return Err(p.error("empty_or_incomplete_branch", branch.start, start + 1));
                }
                p.current = Some(branch.parent);
                p.position += 1;
            }
            b'.' => {
                if p.current.is_none() || p.bond.is_some() {
                    return Err(p.error("unexpected_dot", start, start + 1));
                }
                p.current = None;
                p.position += 1;
            }
            b'-' | b'=' | b'#' | b':' => {
                if p.current.is_none() || p.bond.is_some() {
                    return Err(p.error("unexpected_bond", start, start + 1));
                }
                let order = match p.bytes[start] {
                    b'-' => BondOrder::Single,
                    b'=' => BondOrder::Double,
                    b'#' => BondOrder::Triple,
                    _ => BondOrder::Aromatic,
                };
                p.bond = Some((order, start));
                p.position += 1;
            }
            b'/' | b'\\' | b'@' => return Err(p.error("stereo_unsupported", start, start + 1)),
            b'*' => return Err(p.error("query_unsupported", start, start + 1)),
            b'0'..=b'9' | b'%' => p.ring()?,
            _ => {
                if p.graph.atoms.len() >= MAX_ATOMS {
                    return Err(p.error("atom_limit", start, start + 1));
                }
                let atom = p.atom()?;
                let next = p.graph.atoms.len();
                p.atom_spans.push((start, p.position));
                p.graph.atoms.push(atom);
                if let Some(previous) = p.current {
                    let order = p.bond.take().map(|(o, _)| o);
                    p.connect(previous, next, order, start)?;
                }
                p.current = Some(next);
            }
        }
    }
    if let Some(branch) = p.branches.last() {
        return Err(p.error("unclosed_branch", branch.start, p.position));
    }
    if let Some(ring) = p.rings.values().next() {
        return Err(p.error("unclosed_ring", ring.start, ring.end));
    }
    if let Some((_, start)) = p.bond {
        return Err(p.error("trailing_bond", start, p.position));
    }
    if p.current.is_none() {
        return Err(p.error("trailing_dot", p.position - 1, p.position));
    }
    mark_uncertain_bracket_radicals(&mut p.graph);
    if !aromatic_supported(&p.graph) {
        let first = p
            .graph
            .atoms
            .iter()
            .position(|atom| atom.aromatic)
            .unwrap_or(0);
        let (start, end) = p.atom_spans[first];
        return Err(p.error("aromaticity_outside_model", start, end));
    }
    Ok(p.graph)
}

/// SMILES bracket atoms do not receive ordinary implicit H. An incomplete shell may imply
/// radical electrons, but the syntax does not generally specify their spin multiplicity.
/// Keep that uncertainty until the shared valence/aromaticity models can resolve it. Only
/// conventional closed-shell valence states are marked non-radical here.
fn mark_uncertain_bracket_radicals(graph: &mut SmilesGraph) {
    let mut sums = vec![Some(0_u16); graph.atoms.len()];
    for bond in &graph.bonds {
        let order = match bond.order {
            BondOrder::Single => Some(1),
            BondOrder::Double => Some(2),
            BondOrder::Triple => Some(3),
            _ => None,
        };
        for endpoint in [bond.begin, bond.end] {
            sums[endpoint] = sums[endpoint].zip(order).map(|(sum, order)| sum + order);
        }
    }
    for (index, item) in graph.atoms.iter_mut().enumerate() {
        // A supported aromatic system is checked as a whole below, including a complete
        // Kekule assignment. Do not infer a radical merely from its nonintegral bonds.
        if item.aromatic {
            continue;
        }
        let Hydrogens::Specified(hydrogens) = item.atom.hydrogens else {
            continue;
        };
        let symbol = item
            .atom
            .kind
            .element()
            .expect("parser accepts only elements")
            .symbol();
        let allowed: &[u16] = match (symbol, item.atom.charge) {
            ("H", 0) => &[1],
            ("H", -1 | 1) | ("F" | "Cl" | "Br" | "I", -1) => &[0],
            ("Li" | "Na" | "K" | "Rb" | "Cs", 1) => &[0],
            ("Mg" | "Ca" | "Sr" | "Ba", 2) => &[0],
            ("B", 0) => &[3],
            ("B", -1) => &[4],
            ("B", 1) => &[2],
            ("C", 0) => &[4],
            ("C", -1 | 1) => &[3],
            ("N" | "P", 0) => &[3, 5],
            ("N", -1) => &[2],
            ("N", 1) => &[4],
            ("O", 0) => &[2],
            ("O", -1) => &[1],
            ("O", 1) => &[3],
            ("S", 0) => &[2, 4, 6],
            ("F" | "Cl" | "Br" | "I", 0) => &[1],
            _ => &[],
        };
        let closed = sums[index].is_some_and(|sum| allowed.contains(&(sum + u16::from(hydrogens))));
        if !closed {
            item.atom.radical = Radical::Unknown;
        }
    }
}

/// Lowercase atoms must survive conversion to the native bond representation without
/// losing aromatic state. The named shared model must prove a complete assignment;
/// structures outside that model are refused rather than assigned guessed radicals.
fn aromatic_supported(graph: &SmilesGraph) -> bool {
    if !graph.atoms.iter().any(|atom| atom.aromatic) {
        return true;
    }
    if graph.atoms.iter().enumerate().any(|(index, atom)| {
        atom.aromatic
            && !graph.bonds.iter().any(|bond| {
                bond.order == BondOrder::Aromatic && (bond.begin == index || bond.end == index)
            })
    }) {
        return false;
    }
    let atoms = graph
        .atoms
        .iter()
        .enumerate()
        .map(|(index, atom)| {
            (
                AtomId::from_serial((index + 1) as u32).expect("bounded atom index"),
                atom.atom.clone(),
            )
        })
        .collect();
    let bonds = graph
        .bonds
        .iter()
        .enumerate()
        .map(|(index, bond)| {
            (
                BondId::from_serial((index + 1) as u32).expect("bounded bond index"),
                Bond {
                    begin: AtomId::from_serial((bond.begin + 1) as u32)
                        .expect("validated endpoint"),
                    end: AtomId::from_serial((bond.end + 1) as u32).expect("validated endpoint"),
                    order: bond.order.clone(),
                },
            )
        })
        .collect();
    aromaticity::kekulize(&MoleculeGraph::from_parts(atoms, bonds)).is_ok()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteError {
    InvalidGraph,
    UnsupportedAtom,
    UnsupportedBond,
    UnsupportedAromaticity,
    RingLimit,
}

impl WriteError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidGraph => "invalid_graph",
            Self::UnsupportedAtom => "unsupported_atom",
            Self::UnsupportedBond => "unsupported_bond",
            Self::UnsupportedAromaticity => "unsupported_aromaticity",
            Self::RingLimit => "ring_limit",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CanonicalError {
    Identity(IdentityError),
    Write(WriteError),
}

/// Convert a native graph to the writer's indexed representation without changing its
/// recorded atoms or bonds. Aromatic atoms are those incident to recorded aromatic bonds.
fn indexed(input: &MoleculeGraph) -> (SmilesGraph, BTreeMap<AtomId, usize>) {
    let mut graph = SmilesGraph::default();
    let mut indices = BTreeMap::new();
    for (id, atom) in input.atoms() {
        indices.insert(id, graph.atoms.len());
        graph.atoms.push(SmilesAtom {
            atom: atom.clone(),
            aromatic: false,
        });
    }
    for (_, bond) in input.bonds() {
        let begin = indices[&bond.begin];
        let end = indices[&bond.end];
        if bond.order == BondOrder::Aromatic {
            graph.atoms[begin].aromatic = true;
            graph.atoms[end].aromatic = true;
        }
        graph.bonds.push(SmilesBond {
            begin,
            end,
            order: bond.order.clone(),
        });
    }
    (graph, indices)
}

/// Serialize the recorded graph in document order, without claiming canonical ordering.
pub fn write_recorded(input: &MoleculeGraph) -> Result<String, WriteError> {
    let (graph, _) = indexed(input);
    let order: Vec<_> = (0..graph.atoms.len()).collect();
    write(&graph, &order)
}

/// Serialize a chemical graph using the same bounded normalization as chemical identity.
/// Unproved valence, aromaticity, or exhaustive ordering fails without returning a string.
pub fn write_canonical(input: &MoleculeGraph) -> Result<String, CanonicalError> {
    let (normalized, _) = identity::normalize_chemical(input).map_err(CanonicalError::Identity)?;
    let canonical = identity::canonicalize(&normalized).map_err(CanonicalError::Identity)?;
    let (graph, indices) = indexed(&normalized);
    let order: Vec<_> = canonical.numbering.iter().map(|id| indices[id]).collect();
    write(&graph, &order).map_err(CanonicalError::Write)
}

fn atom_text(atom: &SmilesAtom) -> Result<String, WriteError> {
    let Some(element) = atom.atom.kind.element() else {
        return Err(WriteError::UnsupportedAtom);
    };
    if atom.atom.radical != Radical::None
        || !atom.atom.attachment_points.is_empty()
        || atom.atom.hydrogens == Hydrogens::Unknown
    {
        return Err(WriteError::UnsupportedAtom);
    }
    if !(-8..=8).contains(&atom.atom.charge)
        || atom.atom.isotope.is_some_and(|n| n == 0 || n > 999)
        || matches!(atom.atom.hydrogens, Hydrogens::Specified(n) if n > 16)
    {
        return Err(WriteError::InvalidGraph);
    }
    let symbol = element.symbol();
    if symbol == "H" && matches!(atom.atom.hydrogens, Hydrogens::Specified(n) if n > 0) {
        return Err(WriteError::UnsupportedAtom);
    }
    let plain = matches!(
        symbol,
        "B" | "C" | "N" | "O" | "P" | "S" | "F" | "Cl" | "Br" | "I"
    );
    let aromatic = matches!(symbol, "B" | "C" | "N" | "O" | "P" | "S" | "As" | "Se");
    if atom.aromatic && !aromatic {
        return Err(WriteError::UnsupportedAtom);
    }
    let text = if atom.aromatic {
        symbol.to_ascii_lowercase()
    } else {
        symbol.to_owned()
    };
    let can_plain = if atom.aromatic {
        !matches!(symbol, "As" | "Se")
    } else {
        plain
    };
    if can_plain
        && atom.atom.isotope.is_none()
        && atom.atom.charge == 0
        && atom.atom.hydrogens == Hydrogens::Inferred
    {
        return Ok(text);
    }
    let Hydrogens::Specified(count) = atom.atom.hydrogens else {
        // Bracket atoms have zero explicit H by default, so silently changing an inferred
        // state to a bracket atom would change its meaning.
        return Err(WriteError::UnsupportedAtom);
    };
    let mut out = String::from("[");
    if let Some(isotope) = atom.atom.isotope {
        out.push_str(&isotope.to_string());
    }
    out.push_str(&text);
    if count > 0 {
        out.push('H');
        if count != 1 {
            out.push_str(&count.to_string());
        }
    }
    if atom.atom.charge != 0 {
        out.push(if atom.atom.charge > 0 { '+' } else { '-' });
        let magnitude = atom.atom.charge.unsigned_abs();
        if magnitude != 1 {
            out.push_str(&magnitude.to_string());
        }
    }
    out.push(']');
    Ok(out)
}

fn bond_text(graph: &SmilesGraph, edge: usize) -> Result<&'static str, WriteError> {
    let bond = &graph.bonds[edge];
    let both_aromatic = graph.atoms[bond.begin].aromatic && graph.atoms[bond.end].aromatic;
    match bond.order {
        BondOrder::Single => Ok(if both_aromatic { "-" } else { "" }),
        BondOrder::Double => Ok("="),
        BondOrder::Triple => Ok("#"),
        BondOrder::Aromatic if both_aromatic => Ok(""),
        BondOrder::Aromatic => Err(WriteError::UnsupportedBond),
        BondOrder::Dative | BondOrder::Query(_) | BondOrder::Unknown => {
            Err(WriteError::UnsupportedBond)
        }
    }
}

enum Emit {
    Visit(usize),
    Bond(usize),
    Open,
    Close,
}

/// Write a deterministic traversal using the supplied complete atom ordering. This is generic
/// SMILES; a canonical caller must supply an ordering from its named graph identity model.
/// Every recorded supported atom and edge is emitted; unsupported states are refused.
pub fn write(graph: &SmilesGraph, order: &[usize]) -> Result<String, WriteError> {
    let n = graph.atoms.len();
    if n == 0 || n > MAX_ATOMS || graph.bonds.len() > MAX_BONDS || order.len() != n {
        return Err(WriteError::InvalidGraph);
    }
    let unique: BTreeSet<_> = order.iter().copied().collect();
    if unique.len() != n || unique.iter().copied().ne(0..n) {
        return Err(WriteError::InvalidGraph);
    }
    let atom_texts = graph
        .atoms
        .iter()
        .map(atom_text)
        .collect::<Result<Vec<_>, _>>()?;
    let mut rank = vec![0; n];
    for (i, &v) in order.iter().enumerate() {
        rank[v] = i;
    }
    let mut adjacency = vec![Vec::new(); n];
    let mut pairs = BTreeSet::new();
    for (i, bond) in graph.bonds.iter().enumerate() {
        if bond.begin >= n
            || bond.end >= n
            || bond.begin == bond.end
            || !pairs.insert((bond.begin.min(bond.end), bond.begin.max(bond.end)))
        {
            return Err(WriteError::InvalidGraph);
        }
        bond_text(graph, i)?;
        adjacency[bond.begin].push((bond.end, i));
        adjacency[bond.end].push((bond.begin, i));
    }
    for edges in &mut adjacency {
        edges.sort_by_key(|(v, _)| rank[*v]);
    }
    if !aromatic_supported(graph) {
        return Err(WriteError::UnsupportedAromaticity);
    }
    // Brackets suppress ordinary implicit hydrogen in SMILES. A recorded closed-shell
    // atom whose stated bonds and H do not prove that shell would acquire an unknown
    // radical state on reading; refuse that semantic change rather than emitting it.
    let mut interpreted = graph.clone();
    mark_uncertain_bracket_radicals(&mut interpreted);
    if graph
        .atoms
        .iter()
        .zip(&interpreted.atoms)
        .any(|(source, decoded)| source.atom.radical != decoded.atom.radical)
    {
        return Err(WriteError::UnsupportedAtom);
    }
    let mut visited = vec![false; n];
    let mut seen_edge = vec![false; graph.bonds.len()];
    let mut discovery = vec![0; n];
    let mut children = vec![Vec::<(usize, usize)>::new(); n];
    let mut rings = vec![Vec::<(usize, usize, bool)>::new(); n];
    let mut roots = Vec::new();
    let mut next_discovery = 0;
    let mut next_ring = 1;
    for &root in order {
        if visited[root] {
            continue;
        }
        roots.push(root);
        visited[root] = true;
        discovery[root] = next_discovery;
        next_discovery += 1;
        let mut stack = vec![(root, 0)];
        while let Some((v, cursor)) = stack.last_mut() {
            if *cursor == adjacency[*v].len() {
                stack.pop();
                continue;
            }
            let (other, edge) = adjacency[*v][*cursor];
            *cursor += 1;
            if seen_edge[edge] {
                continue;
            }
            seen_edge[edge] = true;
            if !visited[other] {
                visited[other] = true;
                discovery[other] = next_discovery;
                next_discovery += 1;
                children[*v].push((other, edge));
                stack.push((other, 0));
            } else {
                let first = if discovery[*v] < discovery[other] {
                    *v
                } else {
                    other
                };
                let second = if first == *v { other } else { *v };
                rings[first].push((next_ring, edge, true));
                rings[second].push((next_ring, edge, false));
                next_ring += 1;
            }
        }
    }
    let mut out = String::new();
    let mut free_labels: BTreeSet<u8> = (0..100).collect();
    let mut open_labels = BTreeMap::<usize, u8>::new();
    for (root_index, root) in roots.into_iter().enumerate() {
        if root_index > 0 {
            out.push('.');
        }
        let mut stack = vec![Emit::Visit(root)];
        while let Some(event) = stack.pop() {
            match event {
                Emit::Open => out.push('('),
                Emit::Close => out.push(')'),
                Emit::Bond(edge) => out.push_str(bond_text(graph, edge)?),
                Emit::Visit(v) => {
                    out.push_str(&atom_texts[v]);
                    // A closure can release a label before another ring starts at this atom.
                    // Ring identities describe the traversal; labels are reused only after
                    // the matching second endpoint has actually been emitted.
                    rings[v].sort_by_key(|entry| (entry.2, entry.0));
                    for &(ring, edge, first) in &rings[v] {
                        let label = if first {
                            let label = free_labels
                                .range(1..)
                                .next()
                                .copied()
                                .or_else(|| free_labels.first().copied())
                                .ok_or(WriteError::RingLimit)?;
                            free_labels.remove(&label);
                            if open_labels.insert(ring, label).is_some() {
                                return Err(WriteError::InvalidGraph);
                            }
                            out.push_str(bond_text(graph, edge)?);
                            label
                        } else {
                            let label =
                                open_labels.remove(&ring).ok_or(WriteError::InvalidGraph)?;
                            free_labels.insert(label);
                            label
                        };
                        if label < 10 {
                            out.push_str(&label.to_string());
                        } else {
                            out.push('%');
                            out.push_str(&label.to_string());
                        }
                    }
                    let count = children[v].len();
                    for (index, &(child, edge)) in children[v].iter().enumerate().rev() {
                        if index + 1 < count {
                            stack.push(Emit::Close);
                        }
                        stack.push(Emit::Visit(child));
                        stack.push(Emit::Bond(edge));
                        if index + 1 < count {
                            stack.push(Emit::Open);
                        }
                    }
                }
            }
        }
    }
    if !open_labels.is_empty() {
        return Err(WriteError::InvalidGraph);
    }
    Ok(out)
}
