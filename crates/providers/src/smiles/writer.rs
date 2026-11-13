//! The SMILES writers. Both end in the same check: the text is read back by this codec's own
//! reader, and every atom's element, charge, isotope, hydrogens and radical, and every bond,
//! must come back as written. A string that fails the check is never returned.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use chemifree_application::smiles::{
    SmilesCode, SmilesError, SmilesSlot, SmilesStyle, SmilesWrite, Span,
};
use chemifree_application::{Loss, LossConsent, LossDisposition, SemanticImpact};
use chemifree_domain::budget::{Interrupt, Interrupted, NEVER};
use chemifree_domain::chemistry::{self, Finding};
use chemifree_domain::document::{
    Atom, AtomId, BondId, BondOrder, MAX_CHARGE, MAX_COMMANDS, MAX_ISOTOPE_MASS_NUMBER, MIN_CHARGE,
};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::identity::{self, ChemicalForm, IdentityError};
use chemifree_domain::states::{Hydrogens, Radical};
use chemifree_domain::stereo::{CenterState, Slot, StereoKey, StereoState, Turn};
use chemifree_domain::stereo_drawing;
use chemifree_domain::stereo_group::{GroupKind, StereoGroupTable};

use super::MAX_INPUT_BYTES;
use super::analysis::{self, is_writable_order};
use super::double_bond_write::{self, Marking, Placement, Request};
use super::double_bonds::{Linked, Topology};
use super::emit::{self, Item, Link};
use super::reader::{self, Parsed};
use super::tables;
use super::{Fail, Halt, interruptible, uninterrupted};

/// The most hydrogens a bracket atom states: one digit.
const MAX_WRITTEN_HYDROGENS: u16 = 9;

/// One atom to write with what the text must say about it and what the writer assumes.
struct Plan {
    id: AtomId,
    element: &'static str,
    symbol: &'static str,
    aromatic: bool,
    charge: i8,
    isotope: Option<u16>,
    hydrogens: u16,
    /// The radical the reader must resolve from the text.
    radical: Radical,
    /// Inferred hydrogens on an atom outside the valence model are written as none.
    assumed_hydrogens: bool,
    /// The text cannot state a radical for an atom outside the valence model.
    assumed_radical: bool,
    /// The valence model finds the valence unusual and so gives no hydrogen count for inferred
    /// hydrogens; a stated count would be a guess.
    guessed_hydrogens: bool,
    /// The valence model finds the valence unusual, whatever the hydrogens.
    invalid_valence: bool,
}

impl Plan {
    fn organic(&self) -> bool {
        if self.aromatic {
            tables::is_bare_aromatic(self.symbol)
        } else {
            tables::is_bare_organic(self.element)
        }
    }

    /// Whether the bare form can read back as this atom. A bare atom is neutral, natural
    /// isotope, with inferred hydrogens and no radical; the read-back check settles whether
    /// the inferred count is the planned one.
    fn may_be_bare(&self) -> bool {
        self.organic()
            && self.charge == 0
            && self.isotope.is_none()
            && self.radical == Radical::None
            && !(self.aromatic && self.element != "C" && self.hydrogens > 0)
    }
}

fn fail(code: SmilesCode) -> Fail {
    SmilesError::new(code)
}

fn check_ranges(atom: &Atom) -> Result<(), Fail> {
    if !(MIN_CHARGE..=MAX_CHARGE).contains(&atom.charge) {
        return Err(fail(SmilesCode::ChargeNotWritable));
    }
    if atom
        .isotope
        .is_some_and(|mass| !(1..=MAX_ISOTOPE_MASS_NUMBER).contains(&mass))
    {
        return Err(fail(SmilesCode::IsotopeNotWritable));
    }
    Ok(())
}

fn check_hydrogens(element: &str, count: u16) -> Result<(), Fail> {
    if count > MAX_WRITTEN_HYDROGENS || (element == "H" && count > 0) {
        return Err(fail(SmilesCode::HydrogensNotWritable));
    }
    Ok(())
}

fn aggregate(
    feature: &str,
    noun: &str,
    ids: &[String],
    reason: &str,
    disposition: LossDisposition,
    impact: SemanticImpact,
    consent: LossConsent,
) -> Option<Loss> {
    const LISTED: usize = 12;
    if ids.is_empty() {
        return None;
    }
    let mut listed = ids[..ids.len().min(LISTED)].join(", ");
    if ids.len() > LISTED {
        listed.push_str(&format!(", and {} more", ids.len() - LISTED));
    }
    Some(Loss {
        consent,
        feature: feature.into(),
        source_path: format!("{noun}[{listed}]"),
        disposition,
        reason: reason.into(),
        semantic_impact: impact,
    })
}

/// What a successful render produced.
struct Rendered {
    text: String,
    /// The plan index written at each position of the text.
    order: Vec<usize>,
    bracket: Vec<bool>,
    parsed: Parsed,
    neighbors: Vec<Vec<usize>>,
    preceded: Vec<bool>,
    /// For every link whether the text writes it as a ring-closure digit.
    closure: Vec<bool>,
}

fn items(plans: &[Plan], bracket: &[bool], turns: &[Option<Turn>]) -> Vec<Item> {
    plans
        .iter()
        .zip(bracket)
        .zip(turns)
        .map(|((plan, &bracket), &turn)| Item {
            symbol: plan.symbol,
            aromatic: plan.aromatic,
            charge: plan.charge,
            isotope: plan.isotope,
            hydrogens: plan.hydrogens,
            bracket,
            turn,
        })
        .collect()
}

/// Emits the plans, reads the text back, and brackets every bare atom that read back with other
/// hydrogens or another radical, until the text reads back exactly.
fn render(
    plans: &[Plan],
    links: &[Link],
    max_bytes: usize,
    interrupt: &dyn Interrupt,
) -> Result<Rendered, Halt> {
    render_with_turns(plans, links, max_bytes, &vec![None; plans.len()], interrupt)
}

fn render_with_turns(
    plans: &[Plan],
    links: &[Link],
    max_bytes: usize,
    turns: &[Option<Turn>],
    interrupt: &dyn Interrupt,
) -> Result<Rendered, Halt> {
    let mut bracket: Vec<bool> = plans
        .iter()
        .zip(turns)
        .map(|(plan, turn)| !plan.may_be_bare() || turn.is_some())
        .collect();
    // The first pass finds every bare atom that reads back differently; the second, with those
    // bracketed, must read back exactly.
    for repaired in [false, true] {
        // Emission is unchecked: the 500-command and 16 KiB limits bound it.
        interrupt.check()?;
        if let Some(plan) = plans
            .iter()
            .zip(&bracket)
            .find_map(|(plan, &bracketed)| (bracketed && plan.guessed_hydrogens).then_some(plan))
        {
            return Err(fail(SmilesCode::HydrogensNotWritable)
                .because(format!(
                    "{}: the valence model gives no hydrogen count for this atom, so a bracket would state a guess",
                    plan.id
                ))
                .into());
        }
        let emitted = emit::emit(&items(plans, &bracket, turns), links)?;
        if emitted.text.len() > max_bytes {
            return Err(fail(SmilesCode::OutputTooLong).into());
        }
        interrupt.check()?;
        let parsed = reader::parse_with(&emitted.text, interrupt)?.map_err(|error| {
            fail(SmilesCode::ReadbackMismatch)
                .because(format!("the text is not read: {}", error.code.as_str()))
        })?;
        let mismatched = compare(plans, links, &emitted.order, &parsed, &bracket)?;
        if mismatched.is_empty() {
            return Ok(Rendered {
                text: emitted.text,
                order: emitted.order,
                bracket,
                parsed,
                neighbors: emitted.neighbors,
                preceded: emitted.preceded,
                closure: emitted.closure,
            });
        }
        if repaired {
            return Err(fail(SmilesCode::ReadbackMismatch)
                .because("bracketing the atoms that read back differently did not settle the text")
                .into());
        }
        for index in mismatched {
            bracket[index] = true;
        }
    }
    // The second pass always returns above; this keeps the function total.
    Err(fail(SmilesCode::ReadbackMismatch).into())
}

/// The bare atoms that must be bracketed, or the first fault no bracket can repair.
fn compare(
    plans: &[Plan],
    links: &[Link],
    order: &[usize],
    parsed: &Parsed,
    bracket: &[bool],
) -> Result<Vec<usize>, Fail> {
    let mismatch = |why: &str| fail(SmilesCode::ReadbackMismatch).because(why.to_owned());
    if parsed.read.atoms.len() != plans.len() || parsed.read.bonds.len() != links.len() {
        return Err(mismatch("the atom or bond count differs"));
    }
    let mut position = vec![0; plans.len()];
    for (at, &plan) in order.iter().enumerate() {
        position[plan] = at;
    }
    let mut bracketed = Vec::new();
    // Faults no bracket can repair, by the atom's index in the drawing's (or the canonical)
    // numbering. The lowest-numbered atom decides the code, whatever order the text is in.
    let mut faults: Vec<(usize, Fail)> = Vec::new();
    for (at, &index) in order.iter().enumerate() {
        let plan = &plans[index];
        let read = &parsed.read.atoms[at].atom;
        let element = read.kind.element().map(|e| e.symbol());
        if element != Some(plan.element)
            || read.charge != plan.charge
            || read.isotope != plan.isotope
        {
            faults.push((index, mismatch("an element, charge or isotope differs")));
            continue;
        }
        let resolved = parsed.resolved[at];
        if resolved.hydrogens == Some(plan.hydrogens) && resolved.radical == plan.radical {
            continue;
        }
        if !bracket[index] {
            bracketed.push(index);
        } else if resolved.hydrogens != Some(plan.hydrogens) {
            faults.push((
                index,
                fail(SmilesCode::HydrogensNotWritable)
                    .because(format!("{}: the text states other hydrogens", plan.id)),
            ));
        } else {
            let why = if plan.invalid_valence {
                "its valence is outside the valence model, so a reader records the radical as unknown"
            } else {
                "SMILES cannot state this radical so that it reads back the same"
            };
            faults.push((
                index,
                fail(SmilesCode::RadicalNotWritable).because(format!("{}: {why}", plan.id)),
            ));
        }
    }
    if let Some((_, fault)) = faults.into_iter().min_by_key(|(index, _)| *index) {
        return Err(fault);
    }
    // Each pair of atoms holds one bond, so a bond is found by its pair. The counts were
    // compared above, so a pair written twice cannot hide behind the map.
    let written: BTreeMap<(usize, usize), &BondOrder> = links
        .iter()
        .map(|link| {
            let (a, b) = (position[link.begin], position[link.end]);
            ((a.min(b), a.max(b)), &link.order)
        })
        .collect();
    let read: BTreeMap<(usize, usize), &BondOrder> = parsed
        .read
        .bonds
        .iter()
        .map(|bond| {
            (
                (bond.begin.min(bond.end), bond.begin.max(bond.end)),
                &bond.order,
            )
        })
        .collect();
    if written.len() != links.len() || written != read {
        return Err(mismatch("a bond differs"));
    }
    Ok(bracketed)
}

pub(super) fn write(
    graph: &MoleculeGraph,
    style: SmilesStyle,
    groups: &StereoGroupTable,
) -> Result<SmilesWrite, Fail> {
    uninterrupted(write_in(graph, style, groups, &NEVER))
}

/// Writes like [`write`], asking `interrupt` at the writer's checkpoints. A stop is
/// `Err(Interrupted)`, never a refusal or a partial text.
pub(super) fn write_with(
    graph: &MoleculeGraph,
    style: SmilesStyle,
    groups: &StereoGroupTable,
    interrupt: &dyn Interrupt,
) -> Result<Result<SmilesWrite, Fail>, Interrupted> {
    interruptible(write_in(graph, style, groups, interrupt))
}

/// The writer, asking `interrupt` in the searches it runs. A stop is `Halt::Interrupted`, never a
/// refusal.
fn write_in(
    graph: &MoleculeGraph,
    style: SmilesStyle,
    groups: &StereoGroupTable,
    interrupt: &dyn Interrupt,
) -> Result<SmilesWrite, Halt> {
    if graph.atoms().len() == 0 {
        return Err(fail(SmilesCode::EmptyGraph).into());
    }
    // A graph assembled from parts can break what a document guarantees.
    let known: HashSet<AtomId> = graph.atoms().map(|(id, _)| id).collect();
    if graph.bonds().any(|(_, bond)| {
        bond.begin == bond.end || !known.contains(&bond.begin) || !known.contains(&bond.end)
    }) {
        return Err(fail(SmilesCode::BondEndpointsInvalid).into());
    }
    match style {
        SmilesStyle::Recorded => recorded(graph, groups, double_bond_write::place, interrupt),
        SmilesStyle::Canonical => canonical(graph, interrupt),
    }
}

/// The recorded writer, with the placement of directional bonds given: a test replaces it with
/// a wrong one to see the read-back check refuse its text.
fn recorded(
    graph: &MoleculeGraph,
    groups: &StereoGroupTable,
    place: impl Fn(&Marking<'_>) -> Placement,
    interrupt: &dyn Interrupt,
) -> Result<SmilesWrite, Halt> {
    if graph.atoms().len() + graph.bonds().len() > MAX_COMMANDS {
        return Err(fail(SmilesCode::CommandLimit).into());
    }
    for (_, atom) in graph.atoms() {
        if atom.kind.element().is_none() {
            return Err(fail(SmilesCode::QueryAtomNotWritable).into());
        }
        check_ranges(atom)?;
        if !atom.attachment_points.is_empty() {
            return Err(fail(SmilesCode::AttachmentPointsNotWritable).into());
        }
        if atom.hydrogens == Hydrogens::Unknown {
            return Err(fail(SmilesCode::HydrogensNotWritable)
                .because("unknown_hydrogens")
                .into());
        }
    }
    let mut aromatic = BTreeSet::new();
    for (id, bond) in graph.bonds() {
        if !is_writable_order(&bond.order) {
            return Err(fail(SmilesCode::BondKindNotWritable).into());
        }
        if bond.order == BondOrder::Aromatic {
            aromatic.insert(id);
        }
    }
    // The aromaticity model decides before any atom is formatted.
    let interpretation = if aromatic.is_empty() {
        None
    } else {
        Some(
            analysis::interpret_with(graph, &aromatic, interrupt)?
                .map_err(|failure| fail(SmilesCode::AromaticNotWritable).because(failure.reason))?,
        )
    };
    let analysed = analysis::analyse(graph, interpretation.as_ref())
        .map_err(|failure| fail(SmilesCode::AromaticNotWritable).because(failure.reason))?;
    let aromatic_atoms: BTreeSet<AtomId> = graph
        .bonds()
        .filter(|(id, _)| aromatic.contains(id))
        .flat_map(|(_, bond)| [bond.begin, bond.end])
        .collect();
    let mut plans = Vec::with_capacity(graph.atoms().len());
    for ((id, atom), assessment) in graph.atoms().zip(&analysed.assessments) {
        let element = atom
            .kind
            .element()
            .expect("queries were refused above")
            .symbol();
        let is_aromatic = aromatic_atoms.contains(&id);
        let symbol = if is_aromatic {
            tables::aromatic_symbol(element)
                .ok_or_else(|| fail(SmilesCode::AromaticElementNotWritable))?
        } else {
            element
        };
        let invalid_valence = assessment.finding == Finding::Invalid;
        let guessed_hydrogens = invalid_valence && atom.hydrogens == Hydrogens::Inferred;
        let (hydrogens, radical, assumed_hydrogens, assumed_radical) =
            match (assessment.finding, assessment.reason) {
                (Finding::Valid | Finding::Invalid, _) => {
                    // The drawing's own count wins; the model's is for inferred hydrogens.
                    let stated = match atom.hydrogens {
                        Hydrogens::Specified(count) => Some(u16::from(count)),
                        _ => None,
                    };
                    let count = stated.or(assessment.hydrogen_count).ok_or_else(|| {
                        fail(SmilesCode::HydrogensNotWritable)
                            .because(format!("{id}: no hydrogen count is stated or determined"))
                    })?;
                    (count, atom.radical, false, false)
                }
                (Finding::Unsupported, Some("element_or_charge")) => {
                    // A reader records the radical of such an atom as unknown, so an unknown
                    // radical is written as it is and a drawn "none" is an assumption.
                    let assumed_radical = match atom.radical {
                        Radical::None => true,
                        Radical::Unknown => false,
                        _ => return Err(fail(SmilesCode::RadicalNotWritable).into()),
                    };
                    match atom.hydrogens {
                        Hydrogens::Specified(count) => {
                            (u16::from(count), Radical::Unknown, false, assumed_radical)
                        }
                        _ => (0, Radical::Unknown, true, assumed_radical),
                    }
                }
                (Finding::Unsupported, Some("radical_state")) => {
                    match (atom.radical, atom.hydrogens) {
                        (Radical::Unknown, Hydrogens::Specified(count)) => {
                            (u16::from(count), Radical::Unknown, false, false)
                        }
                        _ => return Err(fail(SmilesCode::RadicalNotWritable).into()),
                    }
                }
                (_, reason) => {
                    return Err(fail(SmilesCode::HydrogensNotWritable)
                        .because(reason.unwrap_or("unsupported"))
                        .into());
                }
            };
        check_hydrogens(element, hydrogens)?;
        plans.push(Plan {
            id,
            element,
            symbol,
            aromatic: is_aromatic,
            charge: atom.charge,
            isotope: atom.isotope,
            hydrogens,
            radical,
            assumed_hydrogens,
            assumed_radical,
            guessed_hydrogens,
            invalid_valence,
        });
    }
    let index: HashMap<AtomId, usize> = plans.iter().enumerate().map(|(i, p)| (p.id, i)).collect();
    let links: Vec<Link> = graph
        .bonds()
        .map(|(_, bond)| Link {
            begin: index[&bond.begin],
            end: index[&bond.end],
            order: bond.order.clone(),
            mark: None,
        })
        .collect();
    let initial = render(&plans, &links, MAX_INPUT_BYTES, interrupt)?;
    let (rendered, mut losses) =
        recorded_stereo(graph, groups, &plans, &links, initial, place, interrupt)?;
    let source: HashMap<AtomId, &Atom> = graph.atoms().collect();
    let name = |ids: Vec<AtomId>| ids.iter().map(ToString::to_string).collect::<Vec<_>>();
    losses.extend(aggregate(
        "hydrogen_assumption",
        "atoms",
        &name(
            plans
                .iter()
                .filter(|plan| plan.assumed_hydrogens)
                .map(|plan| plan.id)
                .collect(),
        ),
        "These atoms are outside the valence model and have inferred hydrogens, which no model determines; the text states no hydrogens on them.",
        LossDisposition::Unknown,
        SemanticImpact::Chemistry,
        LossConsent::Required,
    ));
    losses.extend(aggregate(
        "radical_assumption",
        "atoms",
        &name(
            plans
                .iter()
                .filter(|plan| plan.assumed_radical)
                .map(|plan| plan.id)
                .collect(),
        ),
        "These atoms are outside the valence model, so the text cannot state that they have no radical; a reader records the radical as unknown.",
        LossDisposition::Unknown,
        SemanticImpact::Chemistry,
        LossConsent::Required,
    ));
    losses.extend(hydrogen_state_losses(
        &plans,
        &rendered.bracket,
        |plan| source[&plan.id].hydrogens,
        true,
    ));
    Ok(SmilesWrite {
        atom_order: rendered.order.iter().map(|&i| plans[i].id).collect(),
        text: rendered.text,
        folded_hydrogens: Vec::new(),
        losses,
    })
}

/// Writes only configurations whose complete neighbour set is expressible by the reader.
/// The emitted walk, including ring-label reuse and depth repair, decides the permutation of a
/// tetrahedral center and the symbol of a directional bond.
fn recorded_stereo(
    graph: &MoleculeGraph,
    groups: &StereoGroupTable,
    plans: &[Plan],
    links: &[Link],
    initial: Rendered,
    place: impl Fn(&Marking<'_>) -> Placement,
    interrupt: &dyn Interrupt,
) -> Result<(Rendered, Vec<Loss>), Halt> {
    let index: HashMap<AtomId, usize> = plans
        .iter()
        .enumerate()
        .map(|(i, plan)| (plan.id, i))
        .collect();
    let source: HashMap<AtomId, &Atom> = graph.atoms().collect();
    let link_of_bond: HashMap<BondId, usize> = graph
        .bonds()
        .enumerate()
        .map(|(link, (bond, _))| (bond, link))
        .collect();
    let mut turns = vec![None; plans.len()];
    let mut retained = BTreeMap::new();
    let mut losses = Vec::new();
    for (key, state) in graph.stereo().iter() {
        interrupt.check()?;
        if let (StereoKey::DoubleBond(_), StereoState::DoubleBond(_)) = (key, state) {
            continue;
        }
        let supported = match (key, state) {
            (
                StereoKey::Center(atom),
                StereoState::Center(center @ CenterState::Tetrahedral { .. }),
            ) => index.get(&atom).and_then(|&at| {
                if plans[at].aromatic {
                    return None;
                }
                let neighbors = &initial.neighbors[at];
                if !matches!((neighbors.len(), plans[at].hydrogens), (4, 0) | (3, 1)) {
                    return None;
                }
                let orders: Vec<Option<u16>> = neighbors
                    .iter()
                    .map(|&other| {
                        links
                            .iter()
                            .find(|link| {
                                (link.begin == at && link.end == other)
                                    || (link.end == at && link.begin == other)
                            })
                            .and_then(|link| chemistry::bond_order_value(&link.order))
                    })
                    .collect();
                if stereo_drawing::fourth_position(atom, source[&atom], &orders).is_err() {
                    return None;
                }
                let mut slots: Vec<Slot> = neighbors
                    .iter()
                    .map(|&other| Slot::Atom(plans[other].id))
                    .collect();
                if plans[at].hydrogens == 1 {
                    slots.insert(usize::from(initial.preceded[at]), Slot::ImplicitHydrogen);
                }
                let slots: [Slot; 4] = slots.try_into().ok()?;
                let turn = center.turn_for_order(slots).ok().flatten()?;
                Some((at, atom, *center, turn))
            }),
            _ => None,
        };
        if let Some((at, atom, center, turn)) = supported {
            turns[at] = Some(turn);
            retained.insert(atom, center);
        } else {
            let path = match key {
                StereoKey::Center(atom) => format!("atoms[{atom}]"),
                StereoKey::DoubleBond(bond) => format!("bonds[{bond}]"),
            };
            losses.push(stereo_loss("stereo_configuration", path,
                "This configuration is not a supported tetrahedral center; unknown and lone-pair configurations are not written."));
        }
    }
    let (requests, double_bond_losses) = double_bond_requests(graph.stereo().iter(), &link_of_bond);
    losses.extend(double_bond_losses);
    if plans.len() + links.len() + retained.len() > MAX_COMMANDS {
        return Err(fail(SmilesCode::CommandLimit).into());
    }
    let mut rendered = if retained.is_empty() {
        initial
    } else {
        render_with_turns(plans, links, MAX_INPUT_BYTES, &turns, interrupt)?
    };
    interrupt.check()?;
    // The directional bonds are placed on the text as the tetrahedral marks leave it.
    let placement = place_double_bonds(graph, plans, links, &rendered, &requests, &place);
    for (index, reason) in &placement.omitted {
        losses.push(stereo_loss(
            "stereo_configuration",
            format!("bonds[{}]", requests[*index].bond),
            reason,
        ));
    }
    let written: Vec<&Request> = placement
        .retained
        .iter()
        .map(|&index| &requests[index])
        .collect();
    let marked: Vec<String> = graph
        .bonds()
        .filter(|(_, bond)| bond.mark.is_some())
        .map(|(id, _)| id.to_string())
        .collect();
    losses.extend(aggregate("stereo_marks", "bonds", &marked,
        "Drawn wedge, hash, wavy and crossed marks are not written; only supported stored tetrahedral and double-bond configurations are emitted.",
        LossDisposition::Dropped, SemanticImpact::Stereo, LossConsent::Required));
    if placement.marks.iter().any(Option::is_some) {
        let marked_links: Vec<Link> = links
            .iter()
            .zip(&placement.marks)
            .map(|(link, &mark)| Link {
                mark,
                ..link.clone()
            })
            .collect();
        rendered = render_with_turns(plans, &marked_links, MAX_INPUT_BYTES, &turns, interrupt)?;
    }
    let position: HashMap<AtomId, usize> = rendered
        .order
        .iter()
        .enumerate()
        .map(|(at, &i)| (plans[i].id, at))
        .collect();
    let mut fields = Vec::new();
    let mut emitted_groups = Vec::new();
    let mut expected_groups = Vec::new();
    let mut used_numbers: BTreeSet<(GroupKind, i32)> = groups
        .iter()
        .filter_map(|(_, group)| {
            group
                .interpreted_number()
                .filter(|number| *number >= 0)
                .map(|number| (group.kind(), number))
        })
        .collect();
    for (id, group) in groups.iter() {
        if group.members().is_empty()
            || group
                .members()
                .iter()
                .any(|atom| !retained.contains_key(atom))
        {
            losses.push(stereo_loss("stereo_group", format!("groups[{id}]"),
                "The complete group cannot be written: every member must carry a supported tetrahedral mark."));
            continue;
        }
        let mut members: Vec<usize> = group.members().iter().map(|atom| position[atom]).collect();
        members.sort_unstable();
        let number = group.interpreted_number().map(|number| {
            if number >= 0 {
                return number;
            }
            // There are at most MAX_COMMANDS groups, so an unused positive i32 exists.
            let replacement = (1..=i32::MAX)
                .find(|candidate| !used_numbers.contains(&(group.kind(), *candidate)))
                .expect("bounded group count leaves an unused positive number");
            used_numbers.insert((group.kind(), replacement));
            replacement
        });
        let header = match group.kind() {
            GroupKind::Absolute => "a".to_owned(),
            GroupKind::And => format!("&{}", number.unwrap_or(0)),
            GroupKind::Or => format!("o{}", number.unwrap_or(0)),
        };
        fields.push(format!(
            "{header}:{}",
            members
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ));
        expected_groups.push((group.kind(), number, members));
        emitted_groups.push(id.to_string());
        if group.kind() != GroupKind::Absolute && group.stored_number() != number {
            losses.push(Loss {
                consent: LossConsent::NotRequired,
                feature: "stereo_group_number".into(),
                source_path: format!("groups[{id}]"),
                disposition: LossDisposition::Normalized,
                reason: "An unstated number is written as 0; a negative number is replaced by an unused positive number while preserving the group and its members.".into(),
                semantic_impact: SemanticImpact::Stereo,
            });
        }
    }
    losses.extend(aggregate(
        "stereo_group_identity", "groups", &emitted_groups,
        "Group identifiers are not stored in CXSMILES; a reader assigns its own identifiers while preserving the emitted kind, number and members.",
        LossDisposition::Dropped, SemanticImpact::Stereo, LossConsent::NotRequired,
    ));
    if plans.len() + links.len() + retained.len() + written.len() + fields.len() > MAX_COMMANDS {
        return Err(fail(SmilesCode::CommandLimit).into());
    }
    if !fields.is_empty() {
        rendered.text.push_str(&format!(" |{}|", fields.join(",")));
    }
    if rendered.text.len() > MAX_INPUT_BYTES {
        return Err(fail(SmilesCode::OutputTooLong).into());
    }
    interrupt.check()?;
    let parsed = reader::parse_with(&rendered.text, interrupt)?.map_err(|error| {
        fail(SmilesCode::ReadbackMismatch)
            .because(format!("stereo readback: {}", error.code.as_str()))
    })?;
    let mut actual = BTreeMap::new();
    for center in &parsed.read.stereo.centers {
        let slots = center.slots.map(|slot| match slot {
            SmilesSlot::Atom(at) => Slot::Atom(plans[rendered.order[at]].id),
            SmilesSlot::ImplicitHydrogen => Slot::ImplicitHydrogen,
        });
        let state = CenterState::tetrahedral(slots, center.turn)
            .map_err(|_| fail(SmilesCode::ReadbackMismatch))?;
        actual.insert(plans[rendered.order[center.atom]].id, state);
    }
    let actual_groups: Vec<_> = parsed
        .read
        .stereo
        .groups
        .iter()
        .map(|group| (group.kind, group.number, group.atoms.clone()))
        .collect();
    if actual != retained || actual_groups != expected_groups {
        return Err(fail(SmilesCode::ReadbackMismatch)
            .because("a tetrahedral configuration or enhanced group differs")
            .into());
    }
    if !double_bond_write::read_back_matches(
        &parsed.read.stereo.double_bonds,
        &parsed.read.bonds,
        &rendered.order,
        &plans.iter().map(|plan| plan.id).collect::<Vec<_>>(),
        links,
        &written,
    ) {
        return Err(fail(SmilesCode::ReadbackMismatch)
            .because("a double-bond configuration differs from the one written")
            .into());
    }
    rendered.parsed = parsed;
    Ok((rendered, losses))
}

/// The double-bond configurations among `records` as placement requests, and a loss for each
/// configuration of a bond the drawing does not have. The requests are in bond-identifier order:
/// placement is greedy in the order of the requests and the bond identifiers decide which of two
/// conflicting configurations is kept, so the order does not depend on the order of `records`.
fn double_bond_requests<'a>(
    records: impl IntoIterator<Item = (StereoKey, &'a StereoState)>,
    link_of_bond: &HashMap<BondId, usize>,
) -> (Vec<Request>, Vec<Loss>) {
    let mut requests = Vec::new();
    let mut losses = Vec::new();
    for (key, state) in records {
        let (StereoKey::DoubleBond(bond), StereoState::DoubleBond(state)) = (key, state) else {
            continue;
        };
        match link_of_bond.get(&bond) {
            Some(&link) => requests.push(Request {
                bond,
                link,
                state: *state,
            }),
            None => losses.push(stereo_loss(
                "stereo_configuration",
                format!("bonds[{bond}]"),
                "This double-bond configuration belongs to a bond that is not in the drawing.",
            )),
        }
    }
    requests.sort_by_key(|request| request.bond);
    (requests, losses)
}

/// Places the directional bonds of `requests` on the text `rendered`, judging each end and ring
/// as the reader will: the atoms are taken with the hydrogens the text states.
fn place_double_bonds(
    graph: &MoleculeGraph,
    plans: &[Plan],
    links: &[Link],
    rendered: &Rendered,
    requests: &[Request],
    place: &impl Fn(&Marking<'_>) -> Placement,
) -> Placement {
    if requests.is_empty() {
        return Placement {
            marks: vec![None; links.len()],
            retained: Vec::new(),
            omitted: Vec::new(),
        };
    }
    // Looked up by identifier, not by position: `plans` and the graph's atoms are not zipped
    // on the assumption that both list the atoms in the same order.
    let by_id: HashMap<AtomId, &Atom> = graph.atoms().collect();
    let written: Vec<Atom> = plans
        .iter()
        .zip(&rendered.bracket)
        .map(|(plan, &bracketed)| {
            let mut atom = by_id[&plan.id].clone();
            atom.hydrogens = if bracketed {
                let count = u8::try_from(plan.hydrogens)
                    .expect("a written hydrogen count is at most MAX_WRITTEN_HYDROGENS");
                Hydrogens::Specified(count)
            } else {
                Hydrogens::Inferred
            };
            atom
        })
        .collect();
    let atoms: Vec<&Atom> = written.iter().collect();
    let aromatic: Vec<bool> = plans.iter().map(|plan| plan.aromatic).collect();
    let linked: Vec<Linked<'_>> = links
        .iter()
        .map(|link| Linked {
            begin: link.begin,
            end: link.end,
            order: &link.order,
            mark: None,
            span: Span::new(0, 0),
        })
        .collect();
    let topology = Topology::new(&atoms, &aromatic, &linked);
    let mut position = vec![0; plans.len()];
    for (at, &plan) in rendered.order.iter().enumerate() {
        position[plan] = at;
    }
    let ids: Vec<AtomId> = plans.iter().map(|plan| plan.id).collect();
    let bonds: Vec<BondId> = graph.bonds().map(|(id, _)| id).collect();
    place(&Marking {
        requests,
        ids: &ids,
        links,
        bonds: &bonds,
        topology: &topology,
        neighbors: &rendered.neighbors,
        closure: &rendered.closure,
        position: &position,
    })
}

fn stereo_loss(feature: &str, source_path: String, reason: &str) -> Loss {
    Loss {
        consent: LossConsent::Required,
        feature: feature.into(),
        source_path,
        disposition: LossDisposition::Dropped,
        reason: reason.into(),
        semantic_impact: SemanticImpact::Stereo,
    }
}

/// Stated hydrogens written bare read back as inferred; with `report_inferred_as_stated`,
/// inferred hydrogens that the valence model determines and the text states in brackets are
/// reported too. The counts agree; the recorded state differs. Counts the model does not
/// determine are never written (see `Plan::guessed_hydrogens`) or are reported as assumptions.
fn hydrogen_state_losses(
    plans: &[Plan],
    bracket: &[bool],
    recorded: impl Fn(&Plan) -> Hydrogens,
    report_inferred_as_stated: bool,
) -> Vec<Loss> {
    let mut stated_as_inferred = Vec::new();
    let mut inferred_as_stated = Vec::new();
    for (plan, &bracketed) in plans.iter().zip(bracket) {
        match (recorded(plan), bracketed) {
            (Hydrogens::Specified(_), false) => stated_as_inferred.push(plan.id.to_string()),
            (Hydrogens::Inferred, true) if report_inferred_as_stated && !plan.assumed_hydrogens => {
                inferred_as_stated.push(plan.id.to_string());
            }
            _ => {}
        }
    }
    let mut losses = Vec::new();
    losses.extend(aggregate(
        "hydrogen_state",
        "atoms",
        &stated_as_inferred,
        "A stated hydrogen count is written as a bare atom, which a reader records as inferred; the count is the same.",
        LossDisposition::Normalized,
        SemanticImpact::Chemistry,
        LossConsent::NotRequired,
    ));
    losses.extend(aggregate(
        "hydrogen_state",
        "atoms",
        &inferred_as_stated,
        "Hydrogens the valence model determines are written as a stated count in brackets; the count is the same.",
        LossDisposition::Normalized,
        SemanticImpact::Chemistry,
        LossConsent::NotRequired,
    ));
    losses
}

fn canonical(graph: &MoleculeGraph, interrupt: &dyn Interrupt) -> Result<SmilesWrite, Halt> {
    canonical_with(graph, interrupt, identity::chemical_form_with)
}

/// The canonical writer, with the search for a graph's chemical form given: it is run on the
/// drawing and again on the text read back, and `interrupt` is passed to each run.
fn canonical_with(
    graph: &MoleculeGraph,
    interrupt: &dyn Interrupt,
    chemical_form: impl Fn(
        &MoleculeGraph,
        &dyn Interrupt,
    ) -> Result<Result<ChemicalForm, IdentityError>, Interrupted>,
) -> Result<SmilesWrite, Halt> {
    let form = chemical_form(graph, interrupt)?.map_err(|error| match error {
        IdentityError::AtomLimit | IdentityError::SearchLimit => {
            fail(SmilesCode::CanonicalLimit).because(error.code())
        }
        IdentityError::Unsupported(reason) => {
            fail(SmilesCode::CanonicalUnsupported).because(reason)
        }
    })?;
    if form.atoms.len() + form.bonds.len() > MAX_COMMANDS {
        return Err(fail(SmilesCode::CommandLimit).into());
    }
    let mut aromatic_atoms = vec![false; form.atoms.len()];
    let mut links = Vec::with_capacity(form.bonds.len());
    for (begin, end, order) in &form.bonds {
        if !is_writable_order(order) {
            return Err(fail(SmilesCode::BondKindNotWritable).into());
        }
        if *order == BondOrder::Aromatic {
            aromatic_atoms[*begin] = true;
            aromatic_atoms[*end] = true;
        }
        links.push(Link {
            begin: *begin,
            end: *end,
            order: order.clone(),
            mark: None,
        });
    }
    let mut plans = Vec::with_capacity(form.atoms.len());
    for (entry, &is_aromatic) in form.atoms.iter().zip(&aromatic_atoms) {
        let atom = &entry.atom;
        check_ranges(atom)?;
        let element = atom
            .kind
            .element()
            .expect("a canonical form holds elements")
            .symbol();
        let symbol = if is_aromatic {
            tables::aromatic_symbol(element)
                .ok_or_else(|| fail(SmilesCode::AromaticElementNotWritable))?
        } else {
            element
        };
        let Hydrogens::Specified(count) = atom.hydrogens else {
            return Err(fail(SmilesCode::HydrogensNotWritable).into());
        };
        check_hydrogens(element, u16::from(count))?;
        plans.push(Plan {
            id: entry.id,
            element,
            symbol,
            aromatic: is_aromatic,
            charge: atom.charge,
            isotope: atom.isotope,
            hydrogens: u16::from(count),
            radical: atom.radical,
            assumed_hydrogens: false,
            assumed_radical: false,
            guessed_hydrogens: false,
            invalid_valence: false,
        });
    }
    let rendered = render(&plans, &links, MAX_INPUT_BYTES, interrupt)?;
    // The text must name the very graph it was written from.
    // Running out of work on the text is the limit again, whatever the drawing's own search
    // did; only a text that reads as another kind of graph is a mismatch.
    let again = chemical_form(&rendered.parsed.graph, interrupt)?.map_err(|error| match error {
        IdentityError::AtomLimit | IdentityError::SearchLimit => {
            fail(SmilesCode::CanonicalLimit).because(error.code())
        }
        IdentityError::Unsupported(reason) => fail(SmilesCode::ReadbackMismatch).because(reason),
    })?;
    if again.canonical.code != form.canonical.code {
        return Err(fail(SmilesCode::ReadbackMismatch)
            .because("canonical_not_fixpoint")
            .into());
    }
    let source: HashMap<AtomId, &Atom> = graph.atoms().collect();
    let mut losses = Vec::new();
    let folded: Vec<String> = form
        .canonical
        .folded_hydrogens
        .iter()
        .map(|(hydrogen, _)| hydrogen.to_string())
        .collect();
    losses.extend(aggregate(
        "folded_hydrogens",
        "atoms",
        &folded,
        "Explicit hydrogen atoms are written as hydrogen counts of their neighbours.",
        LossDisposition::Normalized,
        SemanticImpact::Chemistry,
        LossConsent::NotRequired,
    ));
    // A canonical text states a count wherever it needs a bracket: that the drawing inferred it
    // is not worth a loss, since nothing about the hydrogens changed. Stated counts that
    // become inferred are reported.
    losses.extend(hydrogen_state_losses(
        &plans,
        &rendered.bracket,
        |plan| source[&plan.id].hydrogens,
        false,
    ));
    let rank: HashMap<AtomId, usize> = plans.iter().enumerate().map(|(i, p)| (p.id, i)).collect();
    let aromatic_pairs: BTreeSet<(usize, usize)> = form
        .bonds
        .iter()
        .filter(|(_, _, order)| *order == BondOrder::Aromatic)
        .map(|(a, b, _)| (*a, *b))
        .collect();
    let rewritten: Vec<BondId> = graph
        .bonds()
        .filter(|(_, bond)| bond.order != BondOrder::Aromatic)
        .filter_map(|(id, bond)| {
            let (a, b) = (*rank.get(&bond.begin)?, *rank.get(&bond.end)?);
            aromatic_pairs.contains(&(a.min(b), a.max(b))).then_some(id)
        })
        .collect();
    losses.extend(aggregate(
        "aromatic_bonds",
        "bonds",
        &rewritten
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        "Kekulé bonds of aromatic ring systems are written as aromatic atoms.",
        LossDisposition::Normalized,
        SemanticImpact::Chemistry,
        LossConsent::NotRequired,
    ));
    Ok(SmilesWrite {
        atom_order: rendered.order.iter().map(|&i| plans[i].id).collect(),
        text: rendered.text,
        folded_hydrogens: form.canonical.folded_hydrogens.clone(),
        losses,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The search for the text's own chemical form can run out of work although the drawing's
    /// did not; that is the search limit, not a text that reads back differently.
    #[test]
    fn a_read_back_search_limit_is_reported_as_the_limit() {
        let graph = reader::parse("CCO").unwrap().graph;
        for (second, expected, reason) in [
            (
                IdentityError::SearchLimit,
                SmilesCode::CanonicalLimit,
                IdentityError::SearchLimit.code(),
            ),
            (
                IdentityError::AtomLimit,
                SmilesCode::CanonicalLimit,
                IdentityError::AtomLimit.code(),
            ),
            // A text that reads as another kind of graph is a real mismatch.
            (
                IdentityError::Unsupported("radical_state"),
                SmilesCode::ReadbackMismatch,
                "radical_state",
            ),
        ] {
            let calls = std::cell::Cell::new(0);
            let error = uninterrupted(canonical_with(&graph, &NEVER, |input, interrupt| {
                calls.set(calls.get() + 1);
                if calls.get() == 1 {
                    identity::chemical_form_with(input, interrupt)
                } else {
                    Ok(Err(second))
                }
            }))
            .expect_err("the second search fails");
            assert_eq!(calls.get(), 2, "{second:?}");
            assert_eq!(
                (error.code, error.reason.as_deref()),
                (expected, Some(reason)),
                "{second:?}"
            );
        }
    }

    /// `text` as a drawing in which the double bonds listed are stored trans. Each entry is
    /// `(index, begin, end)`: the bond's position among the text's bonds, counted from zero, and
    /// the text atoms, counted from zero, that its two ends are configured against.
    fn drawing_with_trans_bonds(text: &str, stored: &[(usize, usize, usize)]) -> MoleculeGraph {
        use chemifree_domain::document::{
            AtomRef, BondRef, Command, Document, DocumentId, NewAtomPositions, Point,
        };
        use chemifree_domain::stereo::{BondConfiguration, Relation, SlotRef};

        let read = reader::parse(text).unwrap().read;
        let id = |index: usize| AtomRef::Id(AtomId::from_serial(index as u32 + 1).unwrap());
        let mut commands: Vec<Command> = read
            .atoms
            .iter()
            .map(|entry| Command::AddAtom {
                atom: entry.atom.clone(),
                positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
                bond_to: None,
            })
            .collect();
        commands.extend(read.bonds.iter().map(|bond| Command::AddBond {
            begin: id(bond.begin),
            end: id(bond.end),
            order: bond.order.clone(),
        }));
        commands.extend(
            stored
                .iter()
                .map(|&(index, begin, end)| Command::SetBondConfiguration {
                    bond: BondRef::Created(read.atoms.len() + index),
                    configuration: Some(BondConfiguration::Relative {
                        begin_neighbor: SlotRef::Atom(id(begin)),
                        end_neighbor: SlotRef::Atom(id(end)),
                        relation: Relation::OppositeSides,
                    }),
                }),
        );
        let mut document = Document::new(DocumentId::parse("readback").unwrap(), "R").unwrap();
        document.apply(&commands).unwrap();
        MoleculeGraph::from_document(&document)
    }

    /// `CC=CC=CC` as a drawing in which only the first double bond (b2) is stored, trans.
    fn one_configured_bond() -> MoleculeGraph {
        drawing_with_trans_bonds("CC=CC=CC", &[(1, 0, 3)])
    }

    fn refused_reason(place: impl Fn(&Marking<'_>) -> Placement) -> (SmilesCode, String) {
        let error = uninterrupted(recorded(
            &one_configured_bond(),
            &StereoGroupTable::default(),
            place,
            &NEVER,
        ))
        .expect_err("the read-back check refuses the text");
        (error.code, error.reason.unwrap_or_default())
    }

    #[test]
    fn the_unaltered_placement_writes_the_stored_configuration() {
        let written = uninterrupted(recorded(
            &one_configured_bond(),
            &StereoGroupTable::default(),
            double_bond_write::place,
            &NEVER,
        ))
        .unwrap();
        assert_eq!(written.text, "C/C=C/C=CC");
    }

    #[test]
    fn a_placement_that_states_the_other_relation_fails_the_read_back() {
        let (code, reason) = refused_reason(|marking| {
            let mut placement = double_bond_write::place(marking);
            for mark in placement.marks.iter_mut().skip(2) {
                *mark = mark.map(|mark| mark.flipped());
            }
            placement
        });
        assert_eq!(code, SmilesCode::ReadbackMismatch);
        assert!(reason.contains("double-bond configuration"), "{reason}");
    }

    #[test]
    fn a_placement_that_states_an_extra_configuration_fails_the_read_back() {
        // Marking C4-C5 as well makes the second double bond (b4) marked at both ends.
        let (code, reason) = refused_reason(|marking| {
            let mut placement = double_bond_write::place(marking);
            placement.marks[4] = Some(super::super::double_bonds::Mark::Slash);
            placement
        });
        assert_eq!(code, SmilesCode::ReadbackMismatch);
        assert!(reason.contains("double-bond configuration"), "{reason}");
    }

    #[test]
    fn a_placement_that_states_nothing_fails_the_read_back() {
        let (code, reason) = refused_reason(|marking| {
            let mut placement = double_bond_write::place(marking);
            placement.marks.fill(None);
            placement
        });
        assert_eq!(code, SmilesCode::ReadbackMismatch);
        assert!(reason.contains("double-bond configuration"), "{reason}");
    }

    #[test]
    fn a_configuration_on_a_bond_that_is_not_double_is_left_out_with_a_loss() {
        // A drawing never stores a configuration on a bond that is not double, but a graph copy
        // can have a bond order changed after it was taken: the stored b2 becomes single here.
        let mut graph = one_configured_bond();
        let configured = BondId::from_serial(2).unwrap();
        for (bond, data) in graph.bonds_mut() {
            if bond == configured {
                data.order = BondOrder::Single;
            }
        }
        let written = uninterrupted(recorded(
            &graph,
            &StereoGroupTable::default(),
            double_bond_write::place,
            &NEVER,
        ))
        .expect("a configuration on a bond that is not double is left out, not refused");
        assert_eq!(written.text, "CCCC=CC");
        let consent: Vec<(&str, &str)> = written
            .losses
            .iter()
            .filter(|loss| loss.consent == LossConsent::Required)
            .map(|loss| (loss.feature.as_str(), loss.source_path.as_str()))
            .collect();
        assert_eq!(consent, [("stereo_configuration", "bonds[b2]")]);
        assert!(
            written.losses.iter().any(|loss| loss
                .reason
                .contains("the bond is not a double bond in the written text")),
            "{:?}",
            written.losses
        );
    }

    #[test]
    fn requests_are_in_bond_identifier_order_whatever_order_the_records_come_in() {
        // Three stored double bonds, b2, b4 and b6, all trans.
        let graph = drawing_with_trans_bonds("CC=CC=CC=CC", &[(1, 0, 3), (3, 2, 5), (5, 4, 7)]);
        let link_of_bond: HashMap<BondId, usize> = graph
            .bonds()
            .enumerate()
            .map(|(link, (bond, _))| (bond, link))
            .collect();
        let records: Vec<(StereoKey, &StereoState)> = graph.stereo().iter().collect();
        assert_eq!(records.len(), 3, "each stored configuration is one record");
        let serials = |requests: &[Request]| -> Vec<u32> {
            requests
                .iter()
                .map(|request| request.bond.serial())
                .collect()
        };
        let (in_order, _) = double_bond_requests(records.iter().copied(), &link_of_bond);
        let (reversed, _) = double_bond_requests(records.iter().rev().copied(), &link_of_bond);
        assert_eq!(serials(&in_order), [2, 4, 6]);
        assert_eq!(
            serials(&reversed),
            [2, 4, 6],
            "the records given in reverse order"
        );
    }

    #[test]
    fn a_mark_on_a_ring_closure_digit_is_refused_by_the_emitter() {
        let read = reader::parse("C1CCCCCCC1").unwrap();
        let plans: Vec<Plan> = (0..8)
            .map(|i| Plan {
                id: AtomId::from_serial(i + 1).unwrap(),
                element: "C",
                symbol: "C",
                aromatic: false,
                charge: 0,
                isotope: None,
                hydrogens: 2,
                radical: Radical::None,
                assumed_hydrogens: false,
                assumed_radical: false,
                guessed_hydrogens: false,
                invalid_valence: false,
            })
            .collect();
        assert_eq!(read.read.atoms.len(), 8);
        let links: Vec<Link> = (0..8)
            .map(|i| Link {
                begin: i,
                end: (i + 1) % 8,
                order: BondOrder::Single,
                // The last link closes the ring: it is written as a digit.
                mark: (i == 7).then_some(super::super::double_bonds::Mark::Slash),
            })
            .collect();
        let error = uninterrupted(render(&plans, &links, MAX_INPUT_BYTES, &NEVER))
            .err()
            .unwrap();
        assert_eq!(error.code, SmilesCode::ReadbackMismatch);
        assert!(
            error
                .reason
                .unwrap_or_default()
                .contains("ring-closure digit"),
            "a mark on a ring-closure digit is refused where it would be written"
        );
    }

    #[test]
    fn text_longer_than_the_limit_is_refused_not_returned() {
        let plans: Vec<Plan> = (0..4)
            .map(|i| Plan {
                id: AtomId::from_serial(i + 1).unwrap(),
                element: "C",
                symbol: "C",
                aromatic: false,
                charge: 0,
                isotope: None,
                hydrogens: [3, 2, 2, 3][usize::try_from(i).unwrap()],
                radical: Radical::None,
                assumed_hydrogens: false,
                assumed_radical: false,
                guessed_hydrogens: false,
                invalid_valence: false,
            })
            .collect();
        let links: Vec<Link> = (0..3)
            .map(|i| Link {
                begin: i,
                end: i + 1,
                order: BondOrder::Single,
                mark: None,
            })
            .collect();
        // Four carbons chained: CCCC. The same plans fit in 4 bytes and not in 3.
        assert_eq!(
            uninterrupted(render(&plans, &links, 4, &NEVER))
                .map(|rendered| rendered.text)
                .ok()
                .as_deref(),
            Some("CCCC")
        );
        assert_eq!(
            uninterrupted(render(&plans, &links, 3, &NEVER))
                .err()
                .map(|e| e.code),
            Some(SmilesCode::OutputTooLong)
        );
    }

    /// Counts the questions asked and never stops.
    struct Counting(std::cell::Cell<usize>);

    impl Interrupt for Counting {
        fn check(&self) -> Result<(), Interrupted> {
            self.0.set(self.0.get() + 1);
            Ok(())
        }
    }

    /// The writer's own aromaticity model asks its questions before it refuses. A ring with an
    /// aromatic bond that the model does not classify is refused before any text is emitted or
    /// read back, so the questions counted here are the writer's own, with no read-back to share
    /// them.
    #[test]
    fn the_writers_own_aromaticity_model_asks_questions() {
        let mut graph = reader::parse("C1CCCCC1").unwrap().graph;
        let aromatic_bond = BondId::from_serial(1).unwrap();
        for (bond, data) in graph.bonds_mut() {
            if bond == aromatic_bond {
                data.order = BondOrder::Aromatic;
            }
        }
        let counting = Counting(std::cell::Cell::new(0));
        let error = uninterrupted(recorded(
            &graph,
            &StereoGroupTable::default(),
            double_bond_write::place,
            &counting,
        ))
        .expect_err("a ring that is not aromatic cannot carry an aromatic bond");
        assert_eq!(error.code, SmilesCode::AromaticNotWritable);
        let asked = counting.0.get();
        assert!(
            asked >= 1,
            "the writer's aromaticity model must ask at least one question before it refuses, asked {asked}"
        );
    }
}
