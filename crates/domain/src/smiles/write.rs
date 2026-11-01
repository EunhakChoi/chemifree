//! The SMILES writers.
//!
//! Both writers build the same vertex and edge lists and share one emitter; they differ only in
//! how vertices are numbered. The recorded writer numbers atoms in the recorded order and keeps
//! recorded bond kinds. The canonical writer numbers them by the canonical labelled graph of
//! `chemifree-chemical-graph-2`, folds explicit hydrogen vertices into counts and writes every
//! bond of an aromatic system as aromatic, so its text depends only on the graph's chemical
//! meaning.
//!
//! Every string a writer returns has been read back by [`read`](super::read) before it is
//! returned: the writer refuses, with a specific code, anything the reader would not accept.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::aromaticity::{self, KekuleOrder};
use crate::chemistry::{self, Finding};
use crate::document::{
    Atom, AtomId, Bond, BondId, BondOrder, MAX_CHARGE, MAX_COMMANDS, MAX_ISOTOPE_MASS_NUMBER,
    MIN_CHARGE,
};
use crate::element::Element;
use crate::graph::MoleculeGraph;
use crate::identity::{self, IdentityError};
use crate::states::{AtomKind, Hydrogens, Radical};

use super::error::{ErrorCode, SmilesError, Subject};
use super::graph_build::bridges;
use super::{MAX_BRANCH_DEPTH, MAX_INPUT_BYTES, MAX_OPEN_RINGS, radical, read};

/// A written SMILES string and how it relates to the graph it came from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Written {
    pub text: String,
    /// The atoms in the order the text names them. In the canonical form, atoms folded into a
    /// hydrogen count are not here; see `folded_hydrogens`.
    pub order: Vec<AtomId>,
    /// `(hydrogen vertex, heavy atom)` pairs folded into the heavy atom's count. Canonical only.
    pub folded_hydrogens: Vec<(AtomId, AtomId)>,
    /// Atoms whose hydrogens are recorded as inferred in the drawing but stated in the text
    /// (a bracket needs the count), or stated in the drawing but inferred in the text (a bare
    /// organic atom). The counts are equal; the recorded kind of state is not.
    pub hydrogen_state_changes: Vec<AtomId>,
    /// Bonds drawn as single or double that the text writes as aromatic. Canonical only.
    pub kekule_bonds_aromatized: Vec<BondId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Single,
    Double,
    Triple,
    Aromatic,
}

struct Vertex {
    id: AtomId,
    element: Element,
    charge: i8,
    isotope: Option<u16>,
    radical: Radical,
    hydrogens: u16,
    aromatic: bool,
    bracket: bool,
}

struct Edge {
    id: BondId,
    a: usize,
    b: usize,
    kind: Kind,
}

const ORGANIC: [&str; 10] = ["B", "C", "N", "O", "P", "S", "F", "Cl", "Br", "I"];

fn about_atom(code: ErrorCode, id: AtomId) -> SmilesError {
    SmilesError::refused(code).about(Subject::Atom(id))
}

/// Checks that need no model: kinds, ranges and bond kinds. Returns the edge kinds by bond.
fn check_basics(graph: &MoleculeGraph) -> Result<BTreeMap<BondId, Kind>, SmilesError> {
    if graph.atoms().len() == 0 {
        return Err(SmilesError::refused(ErrorCode::EmptyGraph));
    }
    for (id, atom) in graph.atoms() {
        if atom.kind.element().is_none() {
            return Err(about_atom(ErrorCode::QueryAtomNotWritable, id));
        }
        if !(MIN_CHARGE..=MAX_CHARGE).contains(&atom.charge) {
            return Err(about_atom(ErrorCode::ChargeNotWritable, id));
        }
        if atom
            .isotope
            .is_some_and(|mass| !(1..=MAX_ISOTOPE_MASS_NUMBER).contains(&mass))
        {
            return Err(about_atom(ErrorCode::IsotopeNotWritable, id));
        }
        if atom.hydrogens == Hydrogens::Unknown {
            return Err(about_atom(ErrorCode::HydrogensUnknownNotWritable, id));
        }
    }
    let mut kinds = BTreeMap::new();
    for (id, bond) in graph.bonds() {
        let kind = match bond.order {
            BondOrder::Single => Kind::Single,
            BondOrder::Double => Kind::Double,
            BondOrder::Triple => Kind::Triple,
            BondOrder::Aromatic => Kind::Aromatic,
            BondOrder::Dative | BondOrder::Query(_) | BondOrder::Unknown => {
                return Err(
                    SmilesError::refused(ErrorCode::BondKindNotWritable).about(Subject::Bond(id))
                );
            }
        };
        kinds.insert(id, kind);
    }
    if graph.atoms().len() + graph.bonds().len() > MAX_COMMANDS {
        return Err(SmilesError::refused(ErrorCode::CommandLimit));
    }
    Ok(kinds)
}

/// The Kekulé assignment of the recorded aromatic bonds, or the real reason there is none.
fn kekule_assignment(
    graph: &MoleculeGraph,
    kinds: &BTreeMap<BondId, Kind>,
) -> Result<BTreeMap<BondId, KekuleOrder>, SmilesError> {
    if !kinds.values().any(|&kind| kind == Kind::Aromatic) {
        return Ok(BTreeMap::new());
    }
    let bonds: Vec<(BondId, &Bond)> = graph.bonds().collect();
    let positions: HashMap<AtomId, usize> = graph
        .atoms()
        .enumerate()
        .map(|(i, (id, _))| (id, i))
        .collect();
    let edges: Vec<(usize, usize)> = bonds
        .iter()
        .map(|(_, bond)| (positions[&bond.begin], positions[&bond.end]))
        .collect();
    let bridge = bridges(positions.len(), &edges);
    for (index, (id, _)) in bonds.iter().enumerate() {
        if bridge[index] && kinds[id] == Kind::Aromatic {
            return Err(SmilesError::refused(ErrorCode::AromaticBridgeNotWritable)
                .about(Subject::Bond(*id)));
        }
    }
    aromaticity::perceive(graph)
        .kekulize()
        .map(|assignment| assignment.into_iter().collect())
        .map_err(|reason| SmilesError::refused(ErrorCode::AromaticityUnsupported).because(reason))
}

fn bare_kind(atom: &Atom, element: Element) -> bool {
    ORGANIC.contains(&element.symbol())
        && atom.charge == 0
        && atom.isotope.is_none()
        && atom.radical == Radical::None
}

fn bracket_hydrogens(id: AtomId, atom: &Atom, orders: &[Option<u16>]) -> Result<u16, SmilesError> {
    let count = match atom.hydrogens {
        Hydrogens::Specified(count) => u16::from(count),
        Hydrogens::Inferred => {
            let assessed = chemistry::assess(id, atom, orders);
            match (assessed.finding, assessed.hydrogen_count) {
                (Finding::Valid, Some(count)) => count,
                _ => {
                    return Err(about_atom(ErrorCode::HydrogensUndetermined, id)
                        .because(assessed.reason.unwrap_or("unusual_valence")));
                }
            }
        }
        Hydrogens::Unknown => {
            return Err(about_atom(ErrorCode::HydrogensUnknownNotWritable, id));
        }
    };
    if count > 9 {
        return Err(about_atom(ErrorCode::HydrogenCountNotWritable, id));
    }
    Ok(count)
}

/// The vertex of one atom written in brackets with `hydrogens`, after checking that the reader
/// derives the recorded radical state from them.
fn bracket_vertex(
    id: AtomId,
    atom: &Atom,
    element: Element,
    hydrogens: u16,
    aromatic: bool,
    orders: &[Option<u16>],
) -> Result<Vertex, SmilesError> {
    let stated = u8::try_from(hydrogens).expect("a bracket count is at most nine");
    if radical::infer(atom, stated, orders).0 != atom.radical {
        return Err(about_atom(ErrorCode::RadicalNotWritable, id));
    }
    Ok(Vertex {
        id,
        element,
        charge: atom.charge,
        isotope: atom.isotope,
        radical: atom.radical,
        hydrogens,
        aromatic,
        bracket: true,
    })
}

/// Writes the drawing's atoms in their recorded order, keeping recorded bond kinds and hydrogen
/// states. Inferred hydrogens of an atom that needs brackets are written as the count the
/// valence model determines.
pub fn write_recorded(graph: &MoleculeGraph) -> Result<Written, SmilesError> {
    let kinds = check_basics(graph)?;
    let assignment = kekule_assignment(graph, &kinds)?;
    let orders = chemistry::incident_orders(
        graph.atoms().map(|(id, _)| id),
        graph.bonds(),
        |id, bond| aromaticity::kekule_order_value(&assignment, id, bond),
    );
    let positions: HashMap<AtomId, usize> = graph
        .atoms()
        .enumerate()
        .map(|(i, (id, _))| (id, i))
        .collect();
    let edges: Vec<Edge> = graph
        .bonds()
        .map(|(id, bond)| Edge {
            id,
            a: positions[&bond.begin],
            b: positions[&bond.end],
            kind: kinds[&id],
        })
        .collect();
    let mut aromatic = vec![false; positions.len()];
    for edge in edges.iter().filter(|edge| edge.kind == Kind::Aromatic) {
        aromatic[edge.a] = true;
        aromatic[edge.b] = true;
    }
    let mut changes = Vec::new();
    let mut vertices = Vec::with_capacity(positions.len());
    for (index, (id, atom)) in graph.atoms().enumerate() {
        let AtomKind::Element(element) = atom.kind else {
            return Err(about_atom(ErrorCode::QueryAtomNotWritable, id));
        };
        let bare = atom.hydrogens == Hydrogens::Inferred
            && bare_kind(atom, element)
            && (!aromatic[index] || matches!(element.symbol(), "B" | "C" | "N" | "O" | "P" | "S"));
        if bare {
            vertices.push(Vertex {
                id,
                element,
                charge: 0,
                isotope: None,
                radical: Radical::None,
                hydrogens: 0,
                aromatic: aromatic[index],
                bracket: false,
            });
            continue;
        }
        let count = bracket_hydrogens(id, atom, &orders[&id])?;
        if atom.hydrogens == Hydrogens::Inferred {
            changes.push(id);
        }
        vertices.push(bracket_vertex(
            id,
            atom,
            element,
            count,
            aromatic[index],
            &orders[&id],
        )?);
    }
    let (text, emitted) = emit(&vertices, &edges, Roots::First)?;
    verify(&text, &vertices, &edges, &emitted)?;
    Ok(Written {
        text,
        order: emitted.iter().map(|&v| vertices[v].id).collect(),
        folded_hydrogens: Vec::new(),
        hydrogen_state_changes: changes,
        kekule_bonds_aromatized: Vec::new(),
    })
}

/// Writes the canonical SMILES: Chemifree's own, stable under atom order and Kekulé/aromatic
/// spelling, defined for exactly the structures `chemifree-chemical-graph-2` can identify.
pub fn write_canonical(graph: &MoleculeGraph) -> Result<Written, SmilesError> {
    let kinds = check_basics(graph)?;
    let canonical = identity::canonicalize_chemical(graph).map_err(|error| match error {
        IdentityError::AtomLimit => SmilesError::refused(ErrorCode::AtomLimit),
        IdentityError::SearchLimit => SmilesError::refused(ErrorCode::SearchLimit),
        IdentityError::Unsupported(reason) => {
            SmilesError::refused(ErrorCode::CanonicalUnsupported).because(reason)
        }
    })?;
    let atoms: BTreeMap<AtomId, &Atom> = graph.atoms().collect();
    let bonds: BTreeMap<BondId, &Bond> = graph.bonds().collect();
    let assignment = kekule_assignment(graph, &kinds)?;
    let numbered: HashMap<AtomId, usize> = canonical
        .numbering
        .iter()
        .enumerate()
        .map(|(index, &id)| (id, index))
        .collect();
    let aromatic_bonds: BTreeSet<BondId> = canonical.aromatic_bonds.iter().copied().collect();
    let mut edges = Vec::new();
    let mut aromatized = Vec::new();
    for (&id, bond) in &bonds {
        let (Some(&a), Some(&b)) = (numbered.get(&bond.begin), numbered.get(&bond.end)) else {
            continue;
        };
        let kind = if aromatic_bonds.contains(&id) {
            if bond.order != BondOrder::Aromatic {
                aromatized.push(id);
            }
            Kind::Aromatic
        } else {
            kinds[&id]
        };
        edges.push(Edge { id, a, b, kind });
    }
    let mut aromatic = vec![false; numbered.len()];
    for edge in edges.iter().filter(|edge| edge.kind == Kind::Aromatic) {
        aromatic[edge.a] = true;
        aromatic[edge.b] = true;
    }
    // Heavy-atom bond orders: the folded hydrogens' bonds are not in the text.
    let mut orders: Vec<Vec<Option<u16>>> = vec![Vec::new(); numbered.len()];
    for edge in &edges {
        let value = match edge.kind {
            Kind::Single => 1,
            Kind::Double => 2,
            Kind::Triple => 3,
            Kind::Aromatic => bonds
                .get(&edge.id)
                .and_then(|bond| aromaticity::kekule_order_value(&assignment, edge.id, bond))
                .ok_or_else(|| SmilesError::refused(ErrorCode::WriterInconsistent))?,
        };
        orders[edge.a].push(Some(value));
        orders[edge.b].push(Some(value));
    }
    let mut changes = Vec::new();
    let mut vertices = Vec::with_capacity(numbered.len());
    for (index, &id) in canonical.numbering.iter().enumerate() {
        let atom = atoms[&id];
        let AtomKind::Element(element) = atom.kind else {
            return Err(about_atom(ErrorCode::QueryAtomNotWritable, id));
        };
        let hydrogens = canonical.hydrogens[index];
        // A bare atom must read back as exactly this count. The reader reads an unbracketed
        // aromatic nitrogen as pyridine-type, so a nitrogen with hydrogen needs its bracket.
        let mut probe = (*atom).clone();
        probe.hydrogens = Hydrogens::Inferred;
        probe.radical = Radical::None;
        let default = chemistry::assess(id, &probe, &orders[index]);
        let bare = bare_kind(atom, element)
            && default.finding == Finding::Valid
            && default.hydrogen_count == Some(hydrogens)
            && !(aromatic[index] && element.symbol() == "N" && hydrogens > 0);
        let stated = matches!(atom.hydrogens, Hydrogens::Specified(_));
        if bare {
            if stated {
                changes.push(id);
            }
            vertices.push(Vertex {
                id,
                element,
                charge: 0,
                isotope: None,
                radical: Radical::None,
                hydrogens: 0,
                aromatic: aromatic[index],
                bracket: false,
            });
            continue;
        }
        if !stated {
            changes.push(id);
        }
        if hydrogens > 9 {
            return Err(about_atom(ErrorCode::HydrogenCountNotWritable, id));
        }
        vertices.push(bracket_vertex(
            id,
            atom,
            element,
            hydrogens,
            aromatic[index],
            &orders[index],
        )?);
    }
    let (text, emitted) = emit(&vertices, &edges, Roots::Terminal)?;
    verify(&text, &vertices, &edges, &emitted)?;
    Ok(Written {
        text,
        order: emitted.iter().map(|&v| vertices[v].id).collect(),
        folded_hydrogens: canonical.folded_hydrogens,
        hydrogen_state_changes: changes,
        kekule_bonds_aromatized: aromatized,
    })
}

struct Emitter<'a> {
    vertices: &'a [Vertex],
    edges: &'a [Edge],
    children: Vec<Vec<(usize, usize)>>,
    opens: Vec<Vec<(usize, usize)>>,
    closes: Vec<Vec<(usize, usize)>>,
    order: Vec<usize>,
    out: String,
    label_in_use: [bool; MAX_OPEN_RINGS],
    label_of_edge: Vec<Option<usize>>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Roots {
    /// Each component starts at its first recorded atom.
    First,
    /// Each component starts at its lowest-index vertex of fewest neighbours.
    Terminal,
}

/// Depth-first text of every component. Vertices are visited from the lowest index, neighbours
/// in index order; the last child continues the chain and earlier ones are branches. Only
/// vertex indices and the labelled edges decide the text.
fn emit(
    vertices: &[Vertex],
    edges: &[Edge],
    policy: Roots,
) -> Result<(String, Vec<usize>), SmilesError> {
    let count = vertices.len();
    let mut adjacency: Vec<Vec<(usize, usize)>> = vec![Vec::new(); count];
    for (index, edge) in edges.iter().enumerate() {
        adjacency[edge.a].push((edge.b, index));
        adjacency[edge.b].push((edge.a, index));
    }
    for neighbours in &mut adjacency {
        neighbours.sort_unstable();
    }
    let mut emitter = Emitter {
        vertices,
        edges,
        children: vec![Vec::new(); count],
        opens: vec![Vec::new(); count],
        closes: vec![Vec::new(); count],
        order: Vec::with_capacity(count),
        out: String::new(),
        label_in_use: [false; MAX_OPEN_RINGS],
        label_of_edge: vec![None; edges.len()],
    };
    let mut visited = vec![false; count];
    let mut seen_edge = vec![false; edges.len()];
    let mut roots = Vec::new();
    // Components in order of their lowest vertex.
    let mut claimed = vec![false; count];
    for first in 0..count {
        if claimed[first] {
            continue;
        }
        let mut best = first;
        let mut queue = vec![first];
        claimed[first] = true;
        while let Some(vertex) = queue.pop() {
            if policy == Roots::Terminal
                && (adjacency[vertex].len(), vertex) < (adjacency[best].len(), best)
            {
                best = vertex;
            }
            for &(neighbour, _) in &adjacency[vertex] {
                if !claimed[neighbour] {
                    claimed[neighbour] = true;
                    queue.push(neighbour);
                }
            }
        }
        roots.push(best);
    }
    for &root in &roots {
        emitter.discover(root, None, &adjacency, &mut visited, &mut seen_edge);
    }
    for list in emitter.opens.iter_mut().chain(emitter.closes.iter_mut()) {
        list.sort_unstable();
    }
    for (position, &root) in roots.iter().enumerate() {
        if position > 0 {
            emitter.out.push('.');
        }
        emitter.write(root, 0)?;
    }
    if emitter.out.len() > MAX_INPUT_BYTES {
        return Err(SmilesError::refused(ErrorCode::OutputTooLong));
    }
    Ok((emitter.out, emitter.order))
}

impl Emitter<'_> {
    /// Pass one: the depth-first tree and the ring-closing edges. Undirected depth-first search
    /// meets a non-tree edge first from the descendant, which closes the ring its ancestor opened.
    fn discover(
        &mut self,
        vertex: usize,
        arrival: Option<usize>,
        adjacency: &[Vec<(usize, usize)>],
        visited: &mut [bool],
        seen_edge: &mut [bool],
    ) {
        visited[vertex] = true;
        self.order.push(vertex);
        for &(neighbour, edge) in &adjacency[vertex] {
            if Some(edge) == arrival || seen_edge[edge] {
                continue;
            }
            seen_edge[edge] = true;
            if visited[neighbour] {
                self.opens[neighbour].push((vertex, edge));
                self.closes[vertex].push((neighbour, edge));
            } else {
                self.children[vertex].push((neighbour, edge));
                self.discover(neighbour, Some(edge), adjacency, visited, seen_edge);
            }
        }
    }

    fn bond_symbol(&self, edge: usize) -> &'static str {
        let edge = &self.edges[edge];
        match edge.kind {
            Kind::Aromatic => "",
            Kind::Single => {
                if self.vertices[edge.a].aromatic && self.vertices[edge.b].aromatic {
                    "-"
                } else {
                    ""
                }
            }
            Kind::Double => "=",
            Kind::Triple => "#",
        }
    }

    fn atom_text(&mut self, vertex: usize) {
        let v = &self.vertices[vertex];
        if !v.bracket {
            let symbol = v.element.symbol();
            if v.aromatic {
                self.out.push_str(&symbol.to_ascii_lowercase());
            } else {
                self.out.push_str(symbol);
            }
            return;
        }
        self.out.push('[');
        if let Some(mass) = v.isotope {
            self.out.push_str(&mass.to_string());
        }
        let symbol = v.element.symbol();
        if v.aromatic {
            self.out.push_str(&symbol.to_ascii_lowercase());
        } else {
            self.out.push_str(symbol);
        }
        match v.hydrogens {
            0 => {}
            1 => self.out.push('H'),
            count => {
                self.out.push('H');
                self.out.push_str(&count.to_string());
            }
        }
        match v.charge {
            0 => {}
            1 => self.out.push('+'),
            -1 => self.out.push('-'),
            charge if charge > 0 => self.out.push_str(&format!("+{charge}")),
            charge => self.out.push_str(&format!("-{}", -i32::from(charge))),
        }
        self.out.push(']');
    }

    fn label_text(&mut self, label: usize) {
        if label < 10 {
            self.out.push_str(&label.to_string());
        } else {
            self.out.push_str(&format!("%{label}"));
        }
    }

    fn write(&mut self, vertex: usize, depth: usize) -> Result<(), SmilesError> {
        self.atom_text(vertex);
        // A label that closes here is free again before this atom opens new rings, so the same
        // label can close one ring and open the next at one atom.
        for index in 0..self.closes[vertex].len() {
            let (_, edge) = self.closes[vertex][index];
            let label = self.label_of_edge[edge]
                .take()
                .expect("a ring is opened before it closes");
            self.label_in_use[label] = false;
            self.label_text(label);
        }
        for index in 0..self.opens[vertex].len() {
            let (_, edge) = self.opens[vertex][index];
            // Labels 1 to 99 first, then 0, so ordinary molecules use single digits.
            let label = (1..MAX_OPEN_RINGS)
                .chain(0..1)
                .find(|&label| !self.label_in_use[label])
                .ok_or(SmilesError::refused(ErrorCode::RingLabelLimit))?;
            self.label_in_use[label] = true;
            self.label_of_edge[edge] = Some(label);
            let symbol = self.bond_symbol(edge);
            self.out.push_str(symbol);
            self.label_text(label);
        }
        let children = self.children[vertex].clone();
        for (position, &(child, edge)) in children.iter().enumerate() {
            let branch = position + 1 < children.len();
            if branch {
                if depth >= MAX_BRANCH_DEPTH {
                    return Err(SmilesError::refused(ErrorCode::BranchDepthLimit));
                }
                self.out.push('(');
            }
            let symbol = self.bond_symbol(edge);
            self.out.push_str(symbol);
            self.write(child, depth + usize::from(branch))?;
            if branch {
                self.out.push(')');
            }
        }
        Ok(())
    }
}

/// Reads `text` back and compares it with what was meant: atoms, bracket counts, radical
/// states and the bond set, in the order the text names them.
fn verify(
    text: &str,
    vertices: &[Vertex],
    edges: &[Edge],
    emitted: &[usize],
) -> Result<(), SmilesError> {
    let inconsistent = || SmilesError::refused(ErrorCode::WriterInconsistent);
    let read = read(text).map_err(|error| {
        SmilesError::refused(ErrorCode::WriterInconsistent).because(error.code.code())
    })?;
    if read.atoms.len() != vertices.len()
        || read.bonds.len() != edges.len()
        || read.ignored_text.is_some()
    {
        return Err(inconsistent());
    }
    let mut position = vec![0_usize; vertices.len()];
    for (at, &vertex) in emitted.iter().enumerate() {
        position[vertex] = at;
        let vertex = &vertices[vertex];
        let atom = &read.atoms[at].atom;
        let same = atom.kind == AtomKind::Element(vertex.element)
            && atom.charge == vertex.charge
            && atom.isotope == vertex.isotope
            && atom.radical == vertex.radical
            && read.atoms[at].bracket == vertex.bracket
            && (!vertex.bracket
                || atom.hydrogens
                    == Hydrogens::Specified(
                        u8::try_from(vertex.hydrogens).map_err(|_| inconsistent())?,
                    ));
        if !same {
            return Err(inconsistent());
        }
    }
    let wanted = |kind: Kind| match kind {
        Kind::Single => BondOrder::Single,
        Kind::Double => BondOrder::Double,
        Kind::Triple => BondOrder::Triple,
        Kind::Aromatic => BondOrder::Aromatic,
    };
    let mut expected: Vec<(usize, usize, String)> = edges
        .iter()
        .map(|edge| {
            let (a, b) = (position[edge.a], position[edge.b]);
            (a.min(b), a.max(b), format!("{:?}", wanted(edge.kind)))
        })
        .collect();
    let mut actual: Vec<(usize, usize, String)> = read
        .bonds
        .iter()
        .map(|bond| {
            (
                bond.begin.min(bond.end),
                bond.begin.max(bond.end),
                format!("{:?}", bond.order),
            )
        })
        .collect();
    expected.sort_unstable();
    actual.sort_unstable();
    if expected == actual {
        Ok(())
    } else {
        Err(inconsistent())
    }
}
