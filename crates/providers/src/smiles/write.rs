//! The recorded-order and canonical SMILES writers.
//!
//! Both share one emitter: they differ only in the rank that orders atoms (the drawing's
//! order, or the canonical numbering of `chemifree-chemical-graph-2`) and in how bonds and
//! hydrogens are read (as recorded, or as the chemical model normalizes them). A writer never
//! returns text the reader does not read back as the graph it wrote: the text is read again
//! here, atom by atom and bond by bond.

use std::collections::{BTreeMap, BTreeSet};

use chemifree_application::smiles_codec::{SmilesStyle, SmilesWrite, WriteChange};
use chemifree_domain::chemistry::{self, Finding};
use chemifree_domain::document::{
    Atom, AtomId, Bond, BondId, BondOrder, MAX_CHARGE, MAX_ISOTOPE_MASS_NUMBER, MIN_CHARGE,
};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::identity::{self, IdentityError};
use chemifree_domain::states::{AtomKind, Hydrogens, Radical};

use super::error::{Code, Failure};
use super::read;
use super::resolve;
use super::tables;
use super::{MAX_BRANCH_DEPTH, MAX_COMMANDS, MAX_INPUT_BYTES, MAX_OPEN_RINGS};

/// What is written for one atom.
struct Plan {
    id: AtomId,
    atom: Atom,
    aromatic: bool,
    bracket: bool,
    /// Non-vertex hydrogens, folded explicit hydrogens included.
    hydrogens: u16,
    /// What reading the text gives for the radical.
    radical: Radical,
    /// The source's own valence finding, to name the cause of a mismatch.
    reason: Option<&'static str>,
}

fn spelling(plan: &Plan) -> Result<String, Failure> {
    let element = plan
        .atom
        .kind
        .element()
        .ok_or_else(|| Failure::bare(Code::QueryNotWritable))?;
    let symbol = if plan.aromatic {
        tables::aromatic_form(element)
            .ok_or_else(|| Failure::bare(Code::AromaticElementUnsupported))?
            .lower
    } else {
        element.symbol()
    };
    if !plan.bracket {
        return Ok(symbol.to_owned());
    }
    let mut text = String::from("[");
    if let Some(mass) = plan.atom.isotope {
        text.push_str(&mass.to_string());
    }
    text.push_str(symbol);
    match plan.hydrogens {
        0 => {}
        1 => text.push('H'),
        count => text.push_str(&format!("H{count}")),
    }
    match plan.atom.charge {
        0 => {}
        1 => text.push('+'),
        -1 => text.push('-'),
        charge if charge > 0 => text.push_str(&format!("+{charge}")),
        charge => text.push_str(&format!("-{}", -i32::from(charge))),
    }
    text.push(']');
    Ok(text)
}

fn bond_symbol(order: &BondOrder, both_aromatic: bool) -> &'static str {
    match order {
        BondOrder::Double => "=",
        BondOrder::Triple => "#",
        BondOrder::Single if both_aromatic => "-",
        _ => "",
    }
}

fn check_graph(graph: &MoleculeGraph) -> Result<(), Failure> {
    if graph.atoms().len() == 0 {
        return Err(Failure::bare(Code::EmptyGraph));
    }
    if graph.atoms().len() + graph.bonds().len() > MAX_COMMANDS {
        return Err(Failure::bare(Code::CommandLimit));
    }
    for (_, atom) in graph.atoms() {
        if matches!(atom.kind, AtomKind::Query(_)) {
            return Err(Failure::bare(Code::QueryNotWritable));
        }
        if atom
            .isotope
            .is_some_and(|mass| !(1..=MAX_ISOTOPE_MASS_NUMBER).contains(&mass))
        {
            return Err(Failure::bare(Code::IsotopeNotWritable));
        }
        if !(MIN_CHARGE..=MAX_CHARGE).contains(&atom.charge) {
            return Err(Failure::bare(Code::ChargeNotWritable));
        }
        if atom.hydrogens == Hydrogens::Unknown {
            return Err(Failure::bare(Code::HydrogensUnknown));
        }
    }
    for (_, bond) in graph.bonds() {
        if !matches!(
            bond.order,
            BondOrder::Single | BondOrder::Double | BondOrder::Triple | BondOrder::Aromatic
        ) {
            return Err(Failure::bare(Code::BondNotWritable));
        }
    }
    Ok(())
}

fn without_attachment_points(graph: &MoleculeGraph) -> (MoleculeGraph, Vec<AtomId>) {
    let mut dropped = Vec::new();
    let atoms = graph
        .atoms()
        .map(|(id, atom)| {
            let mut atom = atom.clone();
            if !atom.attachment_points.is_empty() {
                dropped.push(id);
                atom.attachment_points = Default::default();
            }
            (id, atom)
        })
        .collect();
    let bonds = graph.bonds().map(|(id, bond)| (id, bond.clone())).collect();
    (MoleculeGraph::from_parts(atoms, bonds), dropped)
}

pub(super) fn write(graph: &MoleculeGraph, style: SmilesStyle) -> Result<SmilesWrite, Failure> {
    check_graph(graph)?;
    let (clean, dropped) = without_attachment_points(graph);
    let mut changes = Vec::new();
    if !dropped.is_empty() {
        changes.push(WriteChange::AttachmentPoints(dropped));
    }
    // The canonical numbering, or the drawing's own order.
    let (ranked, folded): (Vec<AtomId>, Vec<(AtomId, AtomId)>) = match style {
        SmilesStyle::Recorded => (clean.atoms().map(|(id, _)| id).collect(), Vec::new()),
        SmilesStyle::Canonical => {
            let canonical =
                identity::canonicalize_chemical(&clean).map_err(|error| match error {
                    IdentityError::AtomLimit => Failure::bare(Code::AtomLimit),
                    IdentityError::SearchLimit => Failure::bare(Code::SearchLimit),
                    IdentityError::Unsupported(reason) => {
                        Failure::bare(Code::CanonicalUnsupported).because(reason)
                    }
                })?;
            (canonical.numbering, canonical.folded_hydrogens)
        }
    };
    let resolution = resolve::resolve(&clean).map_err(|failure| {
        let code = match style {
            SmilesStyle::Recorded => Code::AromaticUnsupported,
            SmilesStyle::Canonical => Code::CanonicalUnsupported,
        };
        Failure::bare(code).because(failure.reason)
    })?;
    let atoms: BTreeMap<AtomId, &Atom> = clean.atoms().collect();
    let bonds: BTreeMap<BondId, &Bond> = clean.bonds().collect();
    let folded_ids: BTreeSet<AtomId> = folded.iter().map(|(hydrogen, _)| *hydrogen).collect();
    let mut folded_into: BTreeMap<AtomId, u16> = BTreeMap::new();
    for (_, parent) in &folded {
        *folded_into.entry(*parent).or_default() += 1;
    }
    // The bonds as written: canonical text writes every bond of an aromatic system aromatic.
    let written: BTreeMap<BondId, BondOrder> = bonds
        .iter()
        .filter(|(_, bond)| !folded_ids.contains(&bond.begin) && !folded_ids.contains(&bond.end))
        .map(|(&id, bond)| {
            let order =
                if style == SmilesStyle::Canonical && resolution.aromatic_systems.contains(&id) {
                    BondOrder::Aromatic
                } else {
                    bond.order.clone()
                };
            (id, order)
        })
        .collect();
    let aromatic_atoms: BTreeSet<AtomId> = written
        .iter()
        .filter(|(_, order)| **order == BondOrder::Aromatic)
        .flat_map(|(id, _)| [bonds[id].begin, bonds[id].end])
        .collect();
    let kekule_as_aromatic: Vec<BondId> = written
        .iter()
        .filter(|(id, order)| {
            **order == BondOrder::Aromatic && bonds[id].order != BondOrder::Aromatic
        })
        .map(|(&id, _)| id)
        .collect();
    if !kekule_as_aromatic.is_empty() {
        changes.push(WriteChange::KekuleAsAromatic(kekule_as_aromatic));
    }
    let source = resolution.assess(&clean);
    let view_orders = chemistry::incident_orders(
        ranked.iter().copied(),
        clean.bonds().filter(|(id, _)| written.contains_key(id)),
        |id, bond| {
            chemifree_domain::aromaticity::kekule_order_value(&resolution.assignment, id, bond)
        },
    );
    let plans = plan_atoms(
        style,
        &ranked,
        &atoms,
        &aromatic_atoms,
        &folded_into,
        &source,
        &view_orders,
        &mut changes,
    )?;
    let (text, atom_order) = emit(&plans, &bonds, &written)?;
    verify(&text, &atom_order, &plans, &bonds, &written)?;
    Ok(SmilesWrite {
        text,
        atom_order,
        folded_hydrogens: folded.iter().map(|(hydrogen, _)| *hydrogen).collect(),
        changes,
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "One pass over the atoms reads every table the writers share."
)]
fn plan_atoms(
    style: SmilesStyle,
    ranked: &[AtomId],
    atoms: &BTreeMap<AtomId, &Atom>,
    aromatic_atoms: &BTreeSet<AtomId>,
    folded_into: &BTreeMap<AtomId, u16>,
    source: &BTreeMap<AtomId, chemistry::AtomAssessment>,
    view_orders: &BTreeMap<AtomId, Vec<Option<u16>>>,
    changes: &mut Vec<WriteChange>,
) -> Result<Vec<Plan>, Failure> {
    let mut stated = Vec::new();
    let mut inferred = Vec::new();
    let mut hydrogens_assumed = Vec::new();
    let mut radical_assumed = Vec::new();
    let mut plans = Vec::with_capacity(ranked.len());
    for &id in ranked {
        let atom = atoms[&id];
        let element = atom
            .kind
            .element()
            .ok_or_else(|| Failure::bare(Code::QueryNotWritable))?;
        let assessed = &source[&id];
        let folded = folded_into.get(&id).copied().unwrap_or(0);
        let aromatic = aromatic_atoms.contains(&id);
        let mut assumed = false;
        let hydrogens = match atom.hydrogens {
            Hydrogens::Specified(count) => u16::from(count) + folded,
            Hydrogens::Unknown => return Err(Failure::bare(Code::HydrogensUnknown)),
            Hydrogens::Inferred => match assessed.finding {
                Finding::Unsupported => match assessed.reason {
                    Some("element_or_charge") => {
                        assumed = true;
                        folded
                    }
                    reason => {
                        return Err(Failure::bare(Code::HydrogensNotRepresentable)
                            .because(reason.unwrap_or("valence_unsupported")));
                    }
                },
                _ => assessed
                    .hydrogen_count
                    .map(|count| count + folded)
                    .ok_or_else(|| {
                        Failure::bare(Code::HydrogensNotRepresentable)
                            .because(assessed.reason.unwrap_or("hydrogens_unknown"))
                    })?,
            },
        };
        if hydrogens > 9 {
            return Err(
                Failure::bare(Code::HydrogensNotRepresentable).because("hydrogen_count_above_nine")
            );
        }
        // A bare atom reads back with inferred hydrogens and no radical: it is bare only
        // when that is what the atom is.
        let mut probe = atom.clone();
        probe.hydrogens = Hydrogens::Inferred;
        probe.radical = Radical::None;
        let default = chemistry::assess(id, &probe, &view_orders[&id]);
        let default_matches =
            default.finding != Finding::Unsupported && default.hydrogen_count == Some(hydrogens);
        let symbol_ok = if aromatic {
            tables::is_bare_aromatic(element) && !(element.symbol() == "N" && hydrogens > 0)
        } else {
            tables::is_bare_organic(element)
        };
        let may_be_inferred =
            style == SmilesStyle::Canonical || atom.hydrogens == Hydrogens::Inferred;
        let bracket = !(atom.charge == 0
            && atom.isotope.is_none()
            && atom.radical == Radical::None
            && may_be_inferred
            && symbol_ok
            && default_matches);
        if bracket && atom.hydrogens == Hydrogens::Inferred && !assumed {
            inferred.push(id);
        }
        if !bracket && matches!(atom.hydrogens, Hydrogens::Specified(_)) {
            stated.push(id);
        }
        let mut radical = atom.radical;
        if assumed {
            hydrogens_assumed.push(id);
            if radical == Radical::None {
                // Reading an atom outside the valence model gives an undetermined radical.
                radical = Radical::Unknown;
                radical_assumed.push(id);
            }
        }
        plans.push(Plan {
            id,
            atom: atom.clone(),
            aromatic,
            bracket,
            hydrogens,
            radical,
            reason: assessed.reason,
        });
    }
    for (list, change) in [
        (
            inferred,
            WriteChange::InferredHydrogensStated as fn(Vec<AtomId>) -> WriteChange,
        ),
        (stated, WriteChange::StatedHydrogensInferred),
        (hydrogens_assumed, WriteChange::HydrogensAssumed),
        (radical_assumed, WriteChange::RadicalAssumed),
    ] {
        if !list.is_empty() {
            changes.push(change(list));
        }
    }
    Ok(plans)
}

/// Depth-first text in rank order, with ring-closure labels reused as soon as they close.
fn emit(
    plans: &[Plan],
    bonds: &BTreeMap<BondId, &Bond>,
    written: &BTreeMap<BondId, BondOrder>,
) -> Result<(String, Vec<AtomId>), Failure> {
    let position: BTreeMap<AtomId, usize> = plans
        .iter()
        .enumerate()
        .map(|(index, plan)| (plan.id, index))
        .collect();
    let count = plans.len();
    // Neighbours in rank order, with the bond.
    let mut adjacent: Vec<Vec<(usize, BondId)>> = vec![Vec::new(); count];
    for &id in written.keys() {
        let bond = bonds[&id];
        let (a, b) = (position[&bond.begin], position[&bond.end]);
        adjacent[a].push((b, id));
        adjacent[b].push((a, id));
    }
    for list in &mut adjacent {
        list.sort_unstable();
    }
    let mut visited = vec![false; count];
    let mut used: BTreeSet<BondId> = BTreeSet::new();
    let mut children: Vec<Vec<(usize, BondId)>> = vec![Vec::new(); count];
    let mut closings: Vec<Vec<(usize, BondId)>> = vec![Vec::new(); count];
    let mut openings: Vec<Vec<(usize, BondId)>> = vec![Vec::new(); count];
    let mut roots = Vec::new();
    for root in 0..count {
        if visited[root] {
            continue;
        }
        roots.push(root);
        visited[root] = true;
        let mut stack = vec![(root, 0_usize)];
        while let Some(top) = stack.last_mut() {
            let (u, next) = (top.0, top.1);
            if next >= adjacent[u].len() {
                stack.pop();
                continue;
            }
            top.1 += 1;
            let (v, bond) = adjacent[u][next];
            if !used.insert(bond) {
                continue;
            }
            if visited[v] {
                // An undirected depth-first search meets an unused edge to a visited atom
                // only at an ancestor: this is a ring closure, opened at `v`.
                closings[u].push((v, bond));
                openings[v].push((u, bond));
            } else {
                visited[v] = true;
                children[u].push((v, bond));
                stack.push((v, 0));
            }
        }
    }
    for list in closings.iter_mut().chain(openings.iter_mut()) {
        list.sort_unstable();
    }
    enum Item {
        Visit(usize, Option<BondId>),
        Text(&'static str),
    }
    let mut text = String::new();
    let mut order = Vec::with_capacity(count);
    let mut label_of: BTreeMap<BondId, usize> = BTreeMap::new();
    let mut free: BTreeSet<usize> = (1..=MAX_OPEN_RINGS).collect();
    let mut depth = 0_usize;
    let symbol_of = |bond: BondId| {
        let b = bonds[&bond];
        let both = plans[position[&b.begin]].aromatic && plans[position[&b.end]].aromatic;
        bond_symbol(&written[&bond], both)
    };
    for (index, &root) in roots.iter().enumerate() {
        if index > 0 {
            text.push('.');
        }
        let mut stack = vec![Item::Visit(root, None)];
        while let Some(item) = stack.pop() {
            match item {
                Item::Text(piece) => {
                    if piece == "(" {
                        depth += 1;
                        if depth > MAX_BRANCH_DEPTH {
                            return Err(Failure::bare(Code::BranchDepthLimit));
                        }
                    } else {
                        depth -= 1;
                    }
                    text.push_str(piece);
                }
                Item::Visit(u, via) => {
                    if let Some(bond) = via {
                        text.push_str(symbol_of(bond));
                    }
                    text.push_str(&spelling(&plans[u])?);
                    order.push(plans[u].id);
                    // A label that closes here is free again before this atom opens any.
                    for &(_, bond) in &closings[u] {
                        let label = label_of[&bond];
                        push_label(&mut text, label);
                        free.insert(label);
                    }
                    for &(_, bond) in &openings[u] {
                        let label = free
                            .pop_first()
                            .ok_or_else(|| Failure::bare(Code::RingLabelLimit))?;
                        label_of.insert(bond, label);
                        text.push_str(symbol_of(bond));
                        push_label(&mut text, label);
                    }
                    let list = &children[u];
                    if let Some((&(last, last_bond), branches)) = list.split_last() {
                        stack.push(Item::Visit(last, Some(last_bond)));
                        for &(child, bond) in branches.iter().rev() {
                            stack.push(Item::Text(")"));
                            stack.push(Item::Visit(child, Some(bond)));
                            stack.push(Item::Text("("));
                        }
                    }
                }
            }
            if text.len() > MAX_INPUT_BYTES {
                return Err(Failure::bare(Code::TextTooLong));
            }
        }
    }
    Ok((text, order))
}

fn push_label(text: &mut String, label: usize) {
    if label < 10 {
        text.push_str(&label.to_string());
    } else {
        text.push_str(&format!("%{label}"));
    }
}

/// Reads `text` back and compares it, atom by atom and bond by bond, with what was written.
fn verify(
    text: &str,
    order: &[AtomId],
    plans: &[Plan],
    bonds: &BTreeMap<BondId, &Bond>,
    written: &BTreeMap<BondId, BondOrder>,
) -> Result<(), Failure> {
    let read = read::read(text)
        .map_err(|failure| Failure::bare(Code::RoundTripFailed).because(failure.code.as_str()))?;
    let plan_of: BTreeMap<AtomId, &Plan> = plans.iter().map(|plan| (plan.id, plan)).collect();
    let position: BTreeMap<AtomId, usize> =
        order.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    if read.atoms.len() != order.len() || read.bonds.len() != written.len() {
        return Err(Failure::bare(Code::RoundTripFailed).because("count_mismatch"));
    }
    for (read_atom, id) in read.atoms.iter().zip(order) {
        let plan = plan_of[id];
        let same_kind = read_atom.atom.kind == plan.atom.kind
            && read_atom.atom.charge == plan.atom.charge
            && read_atom.atom.isotope == plan.atom.isotope;
        if !same_kind {
            return Err(Failure::bare(Code::RoundTripFailed).because("atom_mismatch"));
        }
        if read_atom.hydrogens != Some(plan.hydrogens) {
            let mut failure = Failure::bare(Code::HydrogensNotRepresentable);
            if let Some(reason) = plan.reason {
                failure = failure.because(reason);
            }
            return Err(failure);
        }
        if read_atom.atom.radical != plan.radical {
            let mut failure = Failure::bare(Code::RadicalNotRepresentable);
            if let Some(reason) = plan.reason {
                failure = failure.because(reason);
            }
            return Err(failure);
        }
    }
    let expected: BTreeSet<(usize, usize, String)> = written
        .iter()
        .map(|(id, order)| {
            let bond = bonds[id];
            let (a, b) = (position[&bond.begin], position[&bond.end]);
            (a.min(b), a.max(b), format!("{order:?}"))
        })
        .collect();
    let found: BTreeSet<(usize, usize, String)> = read
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
    if expected != found {
        return Err(Failure::bare(Code::RoundTripFailed).because("bond_mismatch"));
    }
    Ok(())
}
