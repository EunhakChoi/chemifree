//! The SMILES writers. Both end in the same check: the text is read back by this codec's own
//! reader, and every atom's element, charge, isotope, hydrogens and radical, and every bond,
//! must come back as written. A string that fails the check is never returned.

use std::collections::{BTreeSet, HashMap};

use chemifree_application::smiles::{SmilesCode, SmilesError, SmilesStyle, SmilesWrite};
use chemifree_application::{Loss, LossConsent, LossDisposition, SemanticImpact};
use chemifree_domain::chemistry::Finding;
use chemifree_domain::document::{
    Atom, AtomId, BondId, BondOrder, MAX_CHARGE, MAX_COMMANDS, MAX_ISOTOPE_MASS_NUMBER, MIN_CHARGE,
};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::identity::{self, IdentityError};
use chemifree_domain::states::{Hydrogens, Radical};

use super::MAX_INPUT_BYTES;
use super::analysis::{self, is_writable_order};
use super::emit::{self, Item, Kind, Link};
use super::reader::{self, Parsed};
use super::tables;

type Fail = SmilesError;

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

fn kind_of(order: &BondOrder) -> Option<Kind> {
    match order {
        BondOrder::Single => Some(Kind::Single),
        BondOrder::Double => Some(Kind::Double),
        BondOrder::Triple => Some(Kind::Triple),
        BondOrder::Aromatic => Some(Kind::Aromatic),
        _ => None,
    }
}

fn order_of(kind: Kind) -> BondOrder {
    match kind {
        Kind::Single => BondOrder::Single,
        Kind::Double => BondOrder::Double,
        Kind::Triple => BondOrder::Triple,
        Kind::Aromatic => BondOrder::Aromatic,
    }
}

/// What a successful render produced.
struct Rendered {
    text: String,
    /// The plan index written at each position of the text.
    order: Vec<usize>,
    bracket: Vec<bool>,
    parsed: Parsed,
}

fn items(plans: &[Plan], bracket: &[bool]) -> Vec<Item> {
    plans
        .iter()
        .zip(bracket)
        .map(|(plan, &bracket)| Item {
            symbol: plan.symbol,
            aromatic: plan.aromatic,
            charge: plan.charge,
            isotope: plan.isotope,
            hydrogens: plan.hydrogens,
            bracket,
        })
        .collect()
}

/// Emits the plans, reads the text back, and brackets every bare atom that read back with other
/// hydrogens or another radical, until the text reads back exactly.
fn render(plans: &[Plan], links: &[Link], max_bytes: usize) -> Result<Rendered, Fail> {
    let mut bracket: Vec<bool> = plans.iter().map(|plan| !plan.may_be_bare()).collect();
    // The first pass finds every bare atom that reads back differently; the second, with those
    // bracketed, must read back exactly.
    for repaired in [false, true] {
        if let Some(plan) = plans
            .iter()
            .zip(&bracket)
            .find_map(|(plan, &bracketed)| (bracketed && plan.guessed_hydrogens).then_some(plan))
        {
            return Err(fail(SmilesCode::HydrogensNotWritable).because(format!(
                "{}: the valence model gives no hydrogen count for this atom, so a bracket would state a guess",
                plan.id
            )));
        }
        let emitted = emit::emit(&items(plans, &bracket), links)?;
        if emitted.text.len() > max_bytes {
            return Err(fail(SmilesCode::OutputTooLong));
        }
        let parsed = reader::parse(&emitted.text).map_err(|error| {
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
            });
        }
        if repaired {
            return Err(fail(SmilesCode::ReadbackMismatch).because(
                "bracketing the atoms that read back differently did not settle the text",
            ));
        }
        for index in mismatched {
            bracket[index] = true;
        }
    }
    // The second pass always returns above; this keeps the function total.
    Err(fail(SmilesCode::ReadbackMismatch))
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
    for (at, &index) in order.iter().enumerate() {
        let plan = &plans[index];
        let read = &parsed.read.atoms[at].atom;
        let element = read.kind.element().map(|e| e.symbol());
        if element != Some(plan.element)
            || read.charge != plan.charge
            || read.isotope != plan.isotope
        {
            return Err(mismatch("an element, charge or isotope differs"));
        }
        let resolved = parsed.resolved[at];
        if resolved.hydrogens == Some(plan.hydrogens) && resolved.radical == plan.radical {
            continue;
        }
        if !bracket[index] {
            bracketed.push(index);
        } else if resolved.hydrogens != Some(plan.hydrogens) {
            return Err(fail(SmilesCode::HydrogensNotWritable));
        } else {
            return Err(fail(SmilesCode::RadicalNotWritable));
        }
    }
    let written: BTreeSet<(usize, usize, u8)> = links
        .iter()
        .map(|link| {
            let (a, b) = (position[link.begin], position[link.end]);
            (a.min(b), a.max(b), kind_code(&order_of(link.kind)))
        })
        .collect();
    let read: BTreeSet<(usize, usize, u8)> = parsed
        .read
        .bonds
        .iter()
        .map(|bond| {
            (
                bond.begin.min(bond.end),
                bond.begin.max(bond.end),
                kind_code(&bond.order),
            )
        })
        .collect();
    if written != read {
        return Err(mismatch("a bond differs"));
    }
    Ok(bracketed)
}

fn kind_code(order: &BondOrder) -> u8 {
    match order {
        BondOrder::Single => 1,
        BondOrder::Double => 2,
        BondOrder::Triple => 3,
        _ => 4,
    }
}

pub(super) fn write(graph: &MoleculeGraph, style: SmilesStyle) -> Result<SmilesWrite, Fail> {
    if graph.atoms().len() == 0 {
        return Err(fail(SmilesCode::EmptyGraph));
    }
    match style {
        SmilesStyle::Recorded => recorded(graph),
        SmilesStyle::Canonical => canonical(graph),
    }
}

fn recorded(graph: &MoleculeGraph) -> Result<SmilesWrite, Fail> {
    if graph.atoms().len() + graph.bonds().len() > MAX_COMMANDS {
        return Err(fail(SmilesCode::CommandLimit));
    }
    for (_, atom) in graph.atoms() {
        if atom.kind.element().is_none() {
            return Err(fail(SmilesCode::QueryAtomNotWritable));
        }
        check_ranges(atom)?;
        if !atom.attachment_points.is_empty() {
            return Err(fail(SmilesCode::AttachmentPointsNotWritable));
        }
        if atom.hydrogens == Hydrogens::Unknown {
            return Err(fail(SmilesCode::HydrogensNotWritable).because("unknown_hydrogens"));
        }
    }
    let mut aromatic = BTreeSet::new();
    for (id, bond) in graph.bonds() {
        if !is_writable_order(&bond.order) {
            return Err(fail(SmilesCode::BondKindNotWritable));
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
            analysis::interpret(graph, &aromatic)
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
        let guessed_hydrogens =
            assessment.finding == Finding::Invalid && atom.hydrogens == Hydrogens::Inferred;
        let (hydrogens, radical, assumed_hydrogens, assumed_radical) =
            match (assessment.finding, assessment.reason) {
                (Finding::Valid | Finding::Invalid, _) => (
                    assessment.hydrogen_count.unwrap_or(0),
                    atom.radical,
                    false,
                    false,
                ),
                (Finding::Unsupported, Some("element_or_charge")) => {
                    // A reader records the radical of such an atom as unknown, so an unknown
                    // radical is written as it is and a drawn "none" is an assumption.
                    let assumed_radical = match atom.radical {
                        Radical::None => true,
                        Radical::Unknown => false,
                        _ => return Err(fail(SmilesCode::RadicalNotWritable)),
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
                        _ => return Err(fail(SmilesCode::RadicalNotWritable)),
                    }
                }
                (_, reason) => {
                    return Err(fail(SmilesCode::HydrogensNotWritable)
                        .because(reason.unwrap_or("unsupported")));
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
        });
    }
    let index: HashMap<AtomId, usize> = plans.iter().enumerate().map(|(i, p)| (p.id, i)).collect();
    let links: Vec<Link> = graph
        .bonds()
        .map(|(_, bond)| Link {
            begin: index[&bond.begin],
            end: index[&bond.end],
            kind: kind_of(&bond.order).expect("bond kinds were checked above"),
        })
        .collect();
    let rendered = render(&plans, &links, MAX_INPUT_BYTES)?;
    let mut losses = Vec::new();
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

fn canonical(graph: &MoleculeGraph) -> Result<SmilesWrite, Fail> {
    let form = identity::chemical_form(graph).map_err(|error| match error {
        IdentityError::AtomLimit | IdentityError::SearchLimit => {
            fail(SmilesCode::CanonicalLimit).because(error.code())
        }
        IdentityError::Unsupported(reason) => {
            fail(SmilesCode::CanonicalUnsupported).because(reason)
        }
    })?;
    if form.atoms.len() + form.bonds.len() > MAX_COMMANDS {
        return Err(fail(SmilesCode::CommandLimit));
    }
    let mut aromatic_atoms = vec![false; form.atoms.len()];
    let mut links = Vec::with_capacity(form.bonds.len());
    for (begin, end, order) in &form.bonds {
        let kind = kind_of(order).ok_or_else(|| fail(SmilesCode::BondKindNotWritable))?;
        if kind == Kind::Aromatic {
            aromatic_atoms[*begin] = true;
            aromatic_atoms[*end] = true;
        }
        links.push(Link {
            begin: *begin,
            end: *end,
            kind,
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
            return Err(fail(SmilesCode::HydrogensNotWritable));
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
        });
    }
    let rendered = render(&plans, &links, MAX_INPUT_BYTES)?;
    // The text must name the very graph it was written from.
    let again = identity::chemical_form(&rendered.parsed.graph)
        .map_err(|error| fail(SmilesCode::ReadbackMismatch).because(error.code()))?;
    if again.canonical.code != form.canonical.code {
        return Err(fail(SmilesCode::ReadbackMismatch).because("canonical_not_fixpoint"));
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
            })
            .collect();
        let links: Vec<Link> = (0..3)
            .map(|i| Link {
                begin: i,
                end: i + 1,
                kind: Kind::Single,
            })
            .collect();
        // Four carbons chained: CCCC. The same plans fit in 4 bytes and not in 3.
        assert_eq!(
            render(&plans, &links, 4)
                .map(|rendered| rendered.text)
                .ok()
                .as_deref(),
            Some("CCCC")
        );
        assert_eq!(
            render(&plans, &links, 3).err().map(|e| e.code),
            Some(SmilesCode::OutputTooLong)
        );
    }
}
