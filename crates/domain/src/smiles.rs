//! Bounded basic SMILES syntax. Stereo and query extensions are explicitly refused.

use std::collections::{BTreeMap, BTreeSet};

use crate::aromaticity;
use crate::chemistry::{self, Finding};
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
    /// Byte span of text after the first whitespace, which OpenSMILES treats as the end of
    /// the SMILES. It was not interpreted; callers must report it rather than drop it.
    pub ignored: Option<(usize, usize)>,
}

/// Every syntax-error code the parser can return with the message shown to a user. A test
/// reaches each code, so the table cannot list a code the parser never emits.
pub const SYNTAX_ERRORS: &[(&str, &str)] = &[
    ("empty_input", "Enter a SMILES string."),
    (
        "leading_whitespace",
        "Remove the whitespace before the SMILES; a SMILES cannot begin with whitespace.",
    ),
    (
        "input_limit",
        "The SMILES exceeds a supported size or nesting limit.",
    ),
    (
        "atom_limit",
        "The SMILES exceeds a supported size or nesting limit.",
    ),
    (
        "bond_limit",
        "The SMILES exceeds a supported size or nesting limit.",
    ),
    (
        "branch_limit",
        "The SMILES exceeds a supported size or nesting limit.",
    ),
    (
        "command_limit",
        "This import exceeds the atomic transaction command limit.",
    ),
    ("expected_element", "Expected an atom or element symbol."),
    ("expected_atom", "Expected an atom or element symbol."),
    ("unknown_element", "The element symbol is not recognized."),
    (
        "brackets_required",
        "This element requires brackets in SMILES.",
    ),
    (
        "invalid_bracket_atom",
        "The bracket atom is invalid or incomplete.",
    ),
    (
        "isotope_out_of_range",
        "The isotope mass number is outside the supported range.",
    ),
    (
        "hydrogens_out_of_range",
        "The hydrogen count is outside the supported range.",
    ),
    (
        "hydrogen_count_on_hydrogen",
        "A hydrogen atom cannot have its own hydrogen count.",
    ),
    (
        "charge_out_of_range",
        "The charge is invalid or outside the supported range.",
    ),
    (
        "stereo_unsupported",
        "SMILES stereochemistry is not supported yet.",
    ),
    (
        "query_unsupported",
        "SMILES query extensions are not supported.",
    ),
    (
        "invalid_aromatic_bond",
        "The aromatic atoms and bonds cannot be interpreted by the supported model.",
    ),
    (
        "aromaticity_outside_model",
        "The aromatic atoms and bonds cannot be interpreted by the supported model.",
    ),
    (
        "invalid_ring_number",
        "The ring label or its bond is invalid or unclosed.",
    ),
    (
        "ring_without_atom",
        "The ring label or its bond is invalid or unclosed.",
    ),
    (
        "ring_bond_conflict",
        "The ring label or its bond is invalid or unclosed.",
    ),
    (
        "unclosed_ring",
        "The ring label or its bond is invalid or unclosed.",
    ),
    (
        "ring_after_branch",
        "A ring-closure digit must directly follow its atom, before any branch.",
    ),
    (
        "unexpected_branch_end",
        "A branch is empty, incomplete or misplaced.",
    ),
    (
        "branch_without_atom",
        "A branch is empty, incomplete or misplaced.",
    ),
    (
        "empty_or_incomplete_branch",
        "A branch is empty, incomplete or misplaced.",
    ),
    (
        "unclosed_branch",
        "A branch is empty, incomplete or misplaced.",
    ),
    (
        "unexpected_dot",
        "A disconnected-component separator is misplaced.",
    ),
    (
        "trailing_dot",
        "A disconnected-component separator is misplaced.",
    ),
    (
        "unexpected_bond",
        "A bond is incomplete, duplicated or misplaced.",
    ),
    (
        "trailing_bond",
        "A bond is incomplete, duplicated or misplaced.",
    ),
    (
        "duplicate_or_self_bond",
        "A bond is incomplete, duplicated or misplaced.",
    ),
];

/// The user-facing message for a syntax-error code.
pub fn syntax_message(code: &str) -> &'static str {
    SYNTAX_ERRORS
        .iter()
        .find(|(known, _)| *known == code)
        .map_or(
            "The SMILES could not be parsed under the supported basic profile.",
            |(_, message)| message,
        )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyntaxError {
    pub code: &'static str,
    /// Byte offsets in the UTF-8 input, half-open. End-of-input errors may be empty.
    pub start: usize,
    pub end: usize,
    /// A more specific cause when the code names a broad refusal, such as the aromaticity
    /// model's reason for not accepting a ring system.
    pub reason: Option<&'static str>,
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
    bond_pairs: BTreeSet<(usize, usize)>,
    /// Bonds between aromatic atoms with no written symbol: aromatic when in a ring, else single.
    implicit_aromatic: Vec<usize>,
    /// The previous token closed a branch, where ring-closure digits are not permitted.
    after_branch: bool,
    command_limit: usize,
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
            reason: None,
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
                // OpenSMILES charge ::= '+' DIGIT? DIGIT, so `+0` and `-0` are valid zero charges.
                let digits = self.number();
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
        if a == b || self.bond_pairs.contains(&(a.min(b), a.max(b))) {
            return Err(self.error("duplicate_or_self_bond", start, self.position));
        }
        if self.graph.bonds.len() >= MAX_BONDS {
            return Err(self.error("bond_limit", start, self.position));
        }
        if self.graph.atoms.len() + self.graph.bonds.len() >= self.command_limit {
            return Err(self.error("command_limit", start, self.position));
        }
        let implicit =
            order.is_none() && self.graph.atoms[a].aromatic && self.graph.atoms[b].aromatic;
        let order = order.unwrap_or(if implicit {
            BondOrder::Aromatic
        } else {
            BondOrder::Single
        });
        if implicit {
            self.implicit_aromatic.push(self.graph.bonds.len());
        }
        self.bond_pairs.insert((a.min(b), a.max(b)));
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
        if self.after_branch {
            // OpenSMILES: branched_atom ::= atom ringbond* branch*.
            return Err(self.error("ring_after_branch", start, start + 1));
        }
        if self
            .branches
            .last()
            .is_some_and(|b| b.atom_count == self.graph.atoms.len())
        {
            // A ring-closure digit belongs to an atom, and this branch has none yet.
            return Err(self.error("ring_without_atom", start, start + 1));
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
/// Text after the first whitespace is not interpreted and is reported in `ignored`.
pub fn parse(text: &str) -> Result<SmilesGraph, SyntaxError> {
    parse_with_command_limit(text, usize::MAX)
}

/// As `parse`, but refuse input as soon as atoms plus bonds would exceed `command_limit`
/// (`command_limit` error), so a caller with a smaller transaction bound fails early.
pub fn parse_with_command_limit(
    text: &str,
    command_limit: usize,
) -> Result<SmilesGraph, SyntaxError> {
    let end = text
        .find(|c: char| c.is_ascii_whitespace())
        .unwrap_or(text.len());
    let mut p = Parser {
        text,
        bytes: &text.as_bytes()[..end],
        position: 0,
        graph: SmilesGraph::default(),
        atom_spans: Vec::new(),
        current: None,
        bond: None,
        rings: BTreeMap::new(),
        branches: Vec::new(),
        bond_pairs: BTreeSet::new(),
        implicit_aromatic: Vec::new(),
        after_branch: false,
        command_limit,
    };
    if text.is_empty() {
        return Err(p.error("empty_input", 0, 0));
    }
    if text.len() > MAX_SMILES_BYTES {
        return Err(p.error("input_limit", MAX_SMILES_BYTES, text.len()));
    }
    if end == 0 {
        // OpenSMILES allows no whitespace before the SMILES; the whole input would be ignored.
        let run = text.bytes().take_while(u8::is_ascii_whitespace).count();
        return Err(p.error("leading_whitespace", 0, run));
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
                p.after_branch = true;
                p.position += 1;
            }
            b'.' => {
                if p.current.is_none() || p.bond.is_some() {
                    return Err(p.error("unexpected_dot", start, start + 1));
                }
                p.current = None;
                p.after_branch = false;
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
                if p.graph.atoms.len() + p.graph.bonds.len() >= p.command_limit {
                    return Err(p.error("command_limit", start, start + 1));
                }
                let atom = p.atom()?;
                p.after_branch = false;
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
    if let Some(ring) = p.rings.values().min_by_key(|ring| ring.start) {
        return Err(p.error("unclosed_ring", ring.start, ring.end));
    }
    if let Some((_, start)) = p.bond {
        return Err(p.error("trailing_bond", start, p.position));
    }
    if p.current.is_none() {
        return Err(p.error("trailing_dot", p.position.saturating_sub(1), p.position));
    }
    resolve_implicit_aromatic_bonds(&mut p.graph, &p.implicit_aromatic);
    mark_uncertain_bracket_radicals(&mut p.graph);
    if let Some((reason, atoms)) = aromatic_problem(&p.graph) {
        let start = atoms.iter().map(|&i| p.atom_spans[i].0).min().unwrap_or(0);
        let end = atoms
            .iter()
            .map(|&i| p.atom_spans[i].1)
            .max()
            .unwrap_or(start);
        let mut error = p.error("aromaticity_outside_model", start, end);
        error.reason = Some(reason);
        return Err(error);
    }
    if end < text.len()
        && !text[end..]
            .trim_matches(|c: char| c.is_ascii_whitespace())
            .is_empty()
    {
        p.graph.ignored = Some((end, text.len()));
    }
    Ok(p.graph)
}

/// An unspecified bond between two aromatic atoms is aromatic only when it lies in a ring
/// (OpenSMILES); between separate aromatic systems, as in biphenyl, it is single.
fn resolve_implicit_aromatic_bonds(graph: &mut SmilesGraph, implicit: &[usize]) {
    if implicit.is_empty() {
        return;
    }
    let bridges = bridge_flags(graph);
    for &index in implicit {
        if bridges[index] {
            graph.bonds[index].order = BondOrder::Single;
        }
    }
}

/// Mark every bond that no cycle contains, using an iterative depth-first low-link search.
fn bridge_flags(graph: &SmilesGraph) -> Vec<bool> {
    let n = graph.atoms.len();
    let mut adjacency = vec![Vec::new(); n];
    for (index, bond) in graph.bonds.iter().enumerate() {
        adjacency[bond.begin].push((bond.end, index));
        adjacency[bond.end].push((bond.begin, index));
    }
    let mut bridge = vec![false; graph.bonds.len()];
    let mut discovery = vec![usize::MAX; n];
    let mut low = vec![0; n];
    let mut counter = 0;
    for root in 0..n {
        if discovery[root] != usize::MAX {
            continue;
        }
        discovery[root] = counter;
        low[root] = counter;
        counter += 1;
        // Each frame: vertex, entering bond, next neighbour cursor.
        let mut stack = vec![(root, usize::MAX, 0_usize)];
        while let Some(frame) = stack.last_mut() {
            let (vertex, parent_bond) = (frame.0, frame.1);
            if let Some(&(next, bond)) = adjacency[vertex].get(frame.2) {
                frame.2 += 1;
                if bond == parent_bond {
                    continue;
                }
                if discovery[next] == usize::MAX {
                    discovery[next] = counter;
                    low[next] = counter;
                    counter += 1;
                    stack.push((next, bond, 0));
                } else {
                    low[vertex] = low[vertex].min(discovery[next]);
                }
            } else {
                stack.pop();
                if let Some(parent) = stack.last() {
                    let up = parent.0;
                    low[up] = low[up].min(low[vertex]);
                    if low[vertex] > discovery[up] {
                        bridge[parent_bond] = true;
                    }
                }
            }
        }
    }
    bridge
}

/// SMILES bracket atoms do not receive ordinary implicit H. An open shell may imply radical
/// electrons, but the syntax does not specify their spin multiplicity. The shared
/// `chemifree-valence-2` model is the only judge of a closed shell: an atom it accepts as
/// valid is non-radical; any other bracket atom, including an element or charge the model does
/// not cover, keeps an undetermined (`Unknown`) radical state.
fn mark_uncertain_bracket_radicals(graph: &mut SmilesGraph) {
    let mut orders: Vec<Vec<Option<u16>>> = vec![Vec::new(); graph.atoms.len()];
    for bond in &graph.bonds {
        let order = match bond.order {
            BondOrder::Single => Some(1),
            BondOrder::Double => Some(2),
            BondOrder::Triple => Some(3),
            _ => None,
        };
        orders[bond.begin].push(order);
        orders[bond.end].push(order);
    }
    for (index, item) in graph.atoms.iter_mut().enumerate() {
        // A supported aromatic system is checked as a whole below, including a complete
        // Kekule assignment. Do not infer a radical merely from its nonintegral bonds.
        if item.aromatic || !matches!(item.atom.hydrogens, Hydrogens::Specified(_)) {
            continue;
        }
        let id = AtomId::from_serial((index + 1) as u32).expect("bounded atom index");
        if chemistry::assess(id, &item.atom, &orders[index]).finding != Finding::Valid {
            item.atom.radical = Radical::Unknown;
        }
    }
}

/// Lowercase atoms must survive conversion to the native bond representation without
/// losing aromatic state. The named shared model must prove a complete assignment;
/// structures outside that model are refused rather than assigned guessed radicals.
/// Returns the model's reason and the zero-based atoms of the first failing system.
fn aromatic_problem(graph: &SmilesGraph) -> Option<(&'static str, Vec<usize>)> {
    if !graph.atoms.iter().any(|atom| atom.aromatic) {
        return None;
    }
    let mut with_bond = vec![false; graph.atoms.len()];
    for bond in &graph.bonds {
        if bond.order == BondOrder::Aromatic {
            with_bond[bond.begin] = true;
            with_bond[bond.end] = true;
        }
    }
    if let Some(index) = (0..graph.atoms.len()).find(|&i| graph.atoms[i].aromatic && !with_bond[i])
    {
        return Some(("aromatic_atom_without_aromatic_bond", vec![index]));
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
    let bonds: BTreeMap<_, _> = graph
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
    let molecule = MoleculeGraph::from_parts(atoms, bonds.clone().into_iter().collect());
    let interpretation = aromaticity::perceive(&molecule);
    let indices =
        |ids: &[AtomId]| -> Vec<usize> { ids.iter().map(|id| id.serial() as usize - 1).collect() };
    for system in &interpretation.systems {
        let aromatic = system
            .bonds
            .iter()
            .any(|id| bonds[id].order == BondOrder::Aromatic);
        if aromatic && system.classification != aromaticity::Classification::Aromatic {
            return Some((
                system.reason.unwrap_or("not_aromatic"),
                indices(&system.atoms),
            ));
        }
    }
    let reason = interpretation.kekulize().err()?;
    // Only an aromatic bond outside every ring system reaches here.
    let covered: BTreeSet<_> = interpretation
        .systems
        .iter()
        .flat_map(|system| system.bonds.iter().copied())
        .collect();
    let stray: Vec<usize> = bonds
        .iter()
        .filter(|(id, bond)| bond.order == BondOrder::Aromatic && !covered.contains(id))
        .flat_map(|(_, bond)| indices(&[bond.begin, bond.end]))
        .collect();
    Some((reason, stray))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteError {
    InvalidGraph,
    UnsupportedAtom,
    UnsupportedBond,
    UnsupportedAromaticity,
    /// The text would nest branches deeper than this reader accepts.
    BranchLimit,
    /// A bracket atom needs an explicit hydrogen count that the bounded valence model cannot
    /// determine from the recorded bonds, so writing it could change its meaning.
    HydrogensUndetermined,
    RingLimit,
}

impl WriteError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidGraph => "invalid_graph",
            Self::UnsupportedAtom => "unsupported_atom",
            Self::UnsupportedBond => "unsupported_bond",
            Self::UnsupportedAromaticity => "unsupported_aromaticity",
            Self::BranchLimit => "branch_limit",
            Self::HydrogensUndetermined => "hydrogens_undetermined",
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
    resolve_inferred_hydrogens(input, &mut graph, &indices);
    (graph, indices)
}

/// A bracket atom must state its hydrogens. For a drawn atom with inferred hydrogens that
/// needs brackets (charge, isotope, or an element outside the organic subset), use the count
/// the bounded valence model determines from its bonds; otherwise leave it for the writer
/// to refuse with a specific reason.
fn resolve_inferred_hydrogens(
    input: &MoleculeGraph,
    graph: &mut SmilesGraph,
    indices: &BTreeMap<AtomId, usize>,
) {
    let pending = |item: &SmilesAtom| {
        item.atom.hydrogens == Hydrogens::Inferred
            && item
                .atom
                .kind
                .element()
                .is_some_and(|element| requires_bracket(element.symbol(), item))
    };
    if !graph.atoms.iter().any(pending) {
        return;
    }
    let kekule: BTreeMap<_, _> = if graph.atoms.iter().any(|item| item.aromatic) {
        aromaticity::kekulize(input)
            .map(|pairs| pairs.into_iter().collect())
            .unwrap_or_default()
    } else {
        BTreeMap::new()
    };
    let mut orders: BTreeMap<AtomId, Vec<Option<u16>>> = BTreeMap::new();
    for (id, bond) in input.bonds() {
        let order = match bond.order {
            BondOrder::Single => Some(1),
            BondOrder::Double => Some(2),
            BondOrder::Triple => Some(3),
            BondOrder::Aromatic => kekule.get(&id).map(|order| match order {
                aromaticity::KekuleOrder::Single => 1,
                aromaticity::KekuleOrder::Double => 2,
            }),
            BondOrder::Dative | BondOrder::Query(_) | BondOrder::Unknown => None,
        };
        orders.entry(bond.begin).or_default().push(order);
        orders.entry(bond.end).or_default().push(order);
    }
    for (id, atom) in input.atoms() {
        let index = indices[&id];
        if !pending(&graph.atoms[index]) {
            continue;
        }
        let assessment = chemistry::assess(id, atom, orders.get(&id).map_or(&[], Vec::as_slice));
        if assessment.finding == Finding::Valid
            && let Some(count) = assessment.hydrogen_count.and_then(|n| u8::try_from(n).ok())
        {
            graph.atoms[index].atom.hydrogens = Hydrogens::Specified(count);
        }
    }
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

/// Whether the atom's isotope, charge or element needs a bracket atom in SMILES.
fn requires_bracket(symbol: &str, atom: &SmilesAtom) -> bool {
    let can_plain = if atom.aromatic {
        !matches!(symbol, "As" | "Se")
    } else {
        matches!(
            symbol,
            "B" | "C" | "N" | "O" | "P" | "S" | "F" | "Cl" | "Br" | "I"
        )
    };
    !can_plain || atom.atom.isotope.is_some() || atom.atom.charge != 0
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
    let aromatic = matches!(symbol, "B" | "C" | "N" | "O" | "P" | "S" | "As" | "Se");
    if atom.aromatic && !aromatic {
        return Err(WriteError::UnsupportedAtom);
    }
    let text = if atom.aromatic {
        symbol.to_ascii_lowercase()
    } else {
        symbol.to_owned()
    };
    if !requires_bracket(symbol, atom) && atom.atom.hydrogens == Hydrogens::Inferred {
        return Ok(text);
    }
    let Hydrogens::Specified(count) = atom.atom.hydrogens else {
        // Bracket atoms have zero explicit H by default, so silently changing an inferred
        // state to a bracket atom would change its meaning. `indexed` supplies the count
        // when the valence model can determine it.
        return Err(WriteError::HydrogensUndetermined);
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
    // Report an unsupported aromatic system before any per-atom limitation, so the reason
    // given is the real one. Endpoints are validated above.
    if aromatic_problem(graph).is_some() {
        return Err(WriteError::UnsupportedAromaticity);
    }
    let atom_texts = graph
        .atoms
        .iter()
        .map(atom_text)
        .collect::<Result<Vec<_>, _>>()?;
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
    let mut depth = 0_usize;
    for (root_index, root) in roots.into_iter().enumerate() {
        if root_index > 0 {
            out.push('.');
        }
        let mut stack = vec![Emit::Visit(root)];
        while let Some(event) = stack.pop() {
            match event {
                Emit::Open => {
                    depth += 1;
                    if depth > MAX_BRANCH_DEPTH {
                        // The reader refuses this nesting, so the text would not read back.
                        return Err(WriteError::BranchLimit);
                    }
                    out.push('(');
                }
                Emit::Close => {
                    depth -= 1;
                    out.push(')');
                }
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
