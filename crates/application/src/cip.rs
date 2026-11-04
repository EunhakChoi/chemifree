//! The CIP labeler port: what `chem.cip` asks of a labeler, the built-in labeler, and the check
//! every answer passes before it can enter a reply.
//!
//! A labeler is domain logic, not infrastructure, so the built-in one ([`BuiltinCip`]) lives in
//! this crate and calls [`chemifree_domain::cip`]. The port exists so that a different labeler
//! (a local-only engine, later) can replace it without touching the operation. Nothing is
//! loaded, discovered or selected per request, and `local_model_loading` stays unimplemented.
//!
//! A replacement labeler is a trusted, locally installed component, not an untrusted input. The
//! application checks an answer only against its own data (the document, the request and the
//! limits it enforces) and trusts what it cannot check, such as the truth of the labeler's rule
//! list and the correctness of its chemistry. [`Checker::check_centers`],
//! [`Checker::check_double_bonds`] and [`Checker::check_candidates`] refuse an answer that does
//! not name each requested unit exactly once, in the order asked, whose state does not fit the
//! unit's kind or its stored record, whose ranking is not a ranking, whose reason does not fit
//! its state, or whose scan charges less than its reports cost, so a replacement cannot widen
//! the wire contract. `chem.cip` itself holds every stage to the node budget it was given.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use chemifree_domain::budget::Interrupt;
use chemifree_domain::chemistry::{self, Finding};
use chemifree_domain::cip::{
    self, BondReport, Candidates, CenterReport, Descriptor, Limits, Reason, State, Stop, Warning,
};
use chemifree_domain::document::{Atom, AtomId, BondId, BondOrder};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::states::{Hydrogens, Radical};
use chemifree_domain::stereo::{CenterState, DoubleBondState, Slot, StereoKey, StereoState};
use chemifree_domain::stereo_drawing::fourth_position;

/// What `chem.cip` needs from a labeler.
pub trait CipLabeler: Send + Sync {
    /// The model identifier a reply names; it tells a client which labeler produced a result.
    fn model_id(&self) -> &'static str;

    /// The sequence rules this labeler applies, in order (`"1a"`, `"1b"`, ...).
    fn rules(&self) -> &'static [&'static str];

    /// The work limits the labeler is run under, in digraph nodes.
    fn limits(&self) -> Limits;

    /// Labels `atoms` as stereocenters, one report per atom in the order given.
    ///
    /// # Errors
    ///
    /// [`Stop::RequestLimit`] when the items together need more than `limits.request_nodes`,
    /// [`Stop::Interrupted`] when `interrupt` stops the work.
    fn label_centers(
        &self,
        graph: &MoleculeGraph,
        atoms: &[AtomId],
        limits: Limits,
        interrupt: &dyn Interrupt,
    ) -> Result<Vec<CenterReport>, Stop>;

    /// Labels `bonds` as double bonds, one report per bond in the order given.
    ///
    /// # Errors
    ///
    /// As [`CipLabeler::label_centers`].
    fn label_double_bonds(
        &self,
        graph: &MoleculeGraph,
        bonds: &[BondId],
        limits: Limits,
        interrupt: &dyn Interrupt,
    ) -> Result<Vec<BondReport>, Stop>;

    /// The units that are, or may be, stereogenic under this labeler's rules, labeled, for
    /// `scope: "stereogenic"`; a unit found not stereogenic is left out. The default is the
    /// built-in scan.
    ///
    /// # Errors
    ///
    /// As [`CipLabeler::label_centers`].
    fn candidates(
        &self,
        graph: &MoleculeGraph,
        limits: Limits,
        interrupt: &dyn Interrupt,
    ) -> Result<Candidates, Stop> {
        cip::candidates(graph, limits, interrupt)
    }
}

/// The labeler built into Chemifree: [`chemifree_domain::cip`], `chemifree-cip-1`.
#[derive(Clone, Copy, Debug, Default)]
pub struct BuiltinCip;

impl CipLabeler for BuiltinCip {
    fn model_id(&self) -> &'static str {
        cip::MODEL_ID
    }

    fn rules(&self) -> &'static [&'static str] {
        &["1a", "1b", "2", "3"]
    }

    fn limits(&self) -> Limits {
        Limits::default()
    }

    fn label_centers(
        &self,
        graph: &MoleculeGraph,
        atoms: &[AtomId],
        limits: Limits,
        interrupt: &dyn Interrupt,
    ) -> Result<Vec<CenterReport>, Stop> {
        cip::label_centers(graph, atoms, limits, interrupt)
    }

    fn label_double_bonds(
        &self,
        graph: &MoleculeGraph,
        bonds: &[BondId],
        limits: Limits,
        interrupt: &dyn Interrupt,
    ) -> Result<Vec<BondReport>, Stop> {
        cip::label_double_bonds(graph, bonds, limits, interrupt)
    }
}

/// What is wrong with a labeler's answer. Each variant is one rule of the check.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Problem {
    /// The answer does not name the requested units exactly once each, in the order asked.
    Units,
    /// A descriptor that does not fit the unit's kind (`R` or `S` for a center, `E` or `Z` for a
    /// double bond).
    Descriptor,
    /// An `ambiguous` state without the one reason it may carry, or an `unsupported` state with
    /// it.
    Reason,
    /// The record reported as stored differs from the document's, or the state does not fit the
    /// record (`assigned` needs a stated configuration, `unknown` a stated `unknown`,
    /// `unspecified` none).
    Stored,
    /// A ranking that is present when the state has none, absent when it has one, or not four
    /// distinct positions of the unit ranked 1 to 4.
    Ranking,
    /// A warning the state and record do not allow, or one repeated.
    Warnings,
    /// A scan that charges less work than its own reports cost.
    Nodes,
}

impl Problem {
    /// The stable code a reply carries.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Units => "units",
            Self::Descriptor => "descriptor",
            Self::Reason => "reason",
            Self::Stored => "stored",
            Self::Ranking => "ranking",
            Self::Warnings => "warnings",
            Self::Nodes => "nodes",
        }
    }
}

/// A labeler's answer the application refuses: which unit, and which rule it broke.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Refusal {
    /// The identifier of the unit (`a3`, `b2`), or empty when the answer as a whole is wrong.
    pub unit: String,
    pub problem: Problem,
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.unit.is_empty() {
            write!(
                formatter,
                "the answer breaks the {} rule",
                self.problem.as_str()
            )
        } else {
            write!(
                formatter,
                "the answer for {} breaks the {} rule",
                self.unit,
                self.problem.as_str()
            )
        }
    }
}

impl std::error::Error for Refusal {}

fn refused(unit: impl ToString, problem: Problem) -> Refusal {
    Refusal {
        unit: unit.to_string(),
        problem,
    }
}

/// Whether the state is one that carries a ranking.
const fn is_ranked(state: State) -> bool {
    matches!(
        state,
        State::Assigned(_) | State::Unknown | State::Unspecified
    )
}

/// The checks that do not depend on the unit's kind. `stored_present` and `stored_kind` say
/// what the document holds for the unit.
fn check_state(
    unit: &str,
    state: State,
    stored: StoredKind,
    in_kind: fn(Descriptor) -> bool,
) -> Result<(), Refusal> {
    match state {
        State::Assigned(descriptor) => {
            if !in_kind(descriptor) {
                return Err(refused(unit, Problem::Descriptor));
            }
            if stored != StoredKind::Stated {
                return Err(refused(unit, Problem::Stored));
            }
        }
        State::Unknown if stored != StoredKind::Unknown => {
            return Err(refused(unit, Problem::Stored));
        }
        State::Unspecified if stored != StoredKind::None => {
            return Err(refused(unit, Problem::Stored));
        }
        State::Ambiguous(reason) if reason != Reason::LigandStereoUnspecified => {
            return Err(refused(unit, Problem::Reason));
        }
        State::Unsupported(Reason::LigandStereoUnspecified) => {
            return Err(refused(unit, Problem::Reason));
        }
        _ => {}
    }
    Ok(())
}

/// What a document holds for a unit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StoredKind {
    None,
    /// A stated configuration (`tetrahedral`, `relative`).
    Stated,
    Unknown,
}

fn center_kind(state: Option<&CenterState>) -> StoredKind {
    match state {
        None => StoredKind::None,
        Some(CenterState::Tetrahedral { .. }) => StoredKind::Stated,
        Some(CenterState::Unknown { .. }) => StoredKind::Unknown,
    }
}

fn bond_kind(state: Option<&DoubleBondState>) -> StoredKind {
    match state {
        None => StoredKind::None,
        Some(DoubleBondState::Relative { .. }) => StoredKind::Stated,
        Some(DoubleBondState::Unknown) => StoredKind::Unknown,
    }
}

fn stored_center(graph: &MoleculeGraph, atom: AtomId) -> Option<CenterState> {
    match graph.stereo().get(StereoKey::Center(atom)) {
        Some(StereoState::Center(state)) => Some(*state),
        _ => None,
    }
}

fn stored_bond(graph: &MoleculeGraph, bond: BondId) -> Option<DoubleBondState> {
    match graph.stereo().get(StereoKey::DoubleBond(bond)) {
        Some(StereoState::DoubleBond(state)) => Some(*state),
        _ => None,
    }
}

/// Whether the warnings are the ones the state and the record allow, each at most once.
/// `stored_not_stereogenic` goes with exactly a `none` state on a stored record.
fn warnings_fit(
    warnings: &[Warning],
    state: State,
    stored: bool,
    slot_unusual: SlotUnusual,
) -> bool {
    let distinct = warnings
        .iter()
        .enumerate()
        .all(|(index, warning)| !warnings[..index].contains(warning));
    let not_stereogenic = warnings.contains(&Warning::StoredNotStereogenic);
    distinct
        && not_stereogenic == (state == State::None && stored)
        && match slot_unusual {
            SlotUnusual::Never => !warnings.contains(&Warning::SlotUnusual),
            SlotUnusual::Required => warnings.contains(&Warning::SlotUnusual),
            SlotUnusual::Either => true,
        }
}

/// Whether a `cip_slot_unusual` warning must, may not or may be on a center, from what the
/// document says: the stored non-atom slot differs from the one the valence model names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SlotUnusual {
    Never,
    Required,
    /// The center is one the built-in rules may stop at before they compare the slots (a state
    /// `unsupported`), so the warning is neither required nor refused.
    Either,
}

/// What a labeler's answer is checked against: the document's stored records, the neighbors of
/// every atom and the ends of every bond, indexed once so that each check is a lookup.
pub struct Checker<'graph> {
    graph: &'graph MoleculeGraph,
    neighbors: BTreeMap<AtomId, BTreeSet<AtomId>>,
    ends: BTreeMap<BondId, (AtomId, AtomId)>,
    atoms: BTreeMap<AtomId, &'graph Atom>,
    orders: BTreeMap<AtomId, Vec<&'graph BondOrder>>,
}

impl<'graph> Checker<'graph> {
    pub fn new(graph: &'graph MoleculeGraph) -> Self {
        let mut neighbors: BTreeMap<AtomId, BTreeSet<AtomId>> =
            graph.atoms().map(|(id, _)| (id, BTreeSet::new())).collect();
        let mut ends = BTreeMap::new();
        let mut orders: BTreeMap<AtomId, Vec<&BondOrder>> = BTreeMap::new();
        for (id, bond) in graph.bonds() {
            ends.insert(id, (bond.begin, bond.end));
            orders.entry(bond.begin).or_default().push(&bond.order);
            orders.entry(bond.end).or_default().push(&bond.order);
            if let Some(set) = neighbors.get_mut(&bond.begin) {
                set.insert(bond.end);
            }
            if let Some(set) = neighbors.get_mut(&bond.end) {
                set.insert(bond.begin);
            }
        }
        Self {
            graph,
            neighbors,
            ends,
            atoms: graph.atoms().collect(),
            orders,
        }
    }

    pub fn graph(&self) -> &'graph MoleculeGraph {
        self.graph
    }

    pub fn has_atom(&self, atom: AtomId) -> bool {
        self.neighbors.contains_key(&atom)
    }

    pub fn has_bond(&self, bond: BondId) -> bool {
        self.ends.contains_key(&bond)
    }

    /// The atoms and bonds of the document.
    pub fn size(&self) -> usize {
        self.neighbors.len() + self.ends.len()
    }

    /// Whether the stored non-atom slot of the center `atom` differs from the one the valence
    /// model names (`Some(true)`), agrees with it or is absent (`Some(false)`), or the center
    /// is not one the model reads (`None`: the same structure tests as the built-in rules).
    fn stored_slot_differs(&self, atom: AtomId, stored: Option<&CenterState>) -> Option<bool> {
        let Some(stored) = stored else {
            return Some(false);
        };
        let record = self.atoms.get(&atom)?;
        if record.kind.element().is_none()
            || !record.attachment_points.is_empty()
            || record.radical != Radical::None
            || record.hydrogens == Hydrogens::Unknown
            || !(3..=4).contains(&self.neighbors.get(&atom)?.len())
        {
            return None;
        }
        let mut values = Vec::new();
        for order in self.orders.get(&atom)? {
            match order {
                BondOrder::Single | BondOrder::Double | BondOrder::Triple => {
                    values.push(chemistry::bond_order_value(order));
                }
                _ => return None,
            }
        }
        if chemistry::assess(atom, record, &values).finding != Finding::Valid {
            return None;
        }
        let derived = fourth_position(atom, record, &values).ok()?;
        let stored_fourth = stored
            .slots()
            .iter()
            .copied()
            .find(|slot| !matches!(slot, Slot::Atom(_)));
        Some(stored_fourth != derived)
    }

    /// Whether `higher` is one of the positions of `end` other than the double bond to `other`:
    /// an atom bonded to `end`, or (an end has two positions besides the bond) a non-atom slot
    /// where fewer than two atoms are bonded.
    fn is_position_of_end(&self, end: AtomId, other: AtomId, higher: Slot) -> bool {
        let Some(bonded) = self.neighbors.get(&end) else {
            return false;
        };
        match higher {
            Slot::Atom(atom) => atom != other && bonded.contains(&atom),
            Slot::ImplicitHydrogen | Slot::LonePair => bonded.len().saturating_sub(1) < 2,
        }
    }

    /// Checks a labeler's answer for `requested` centers against the document it was asked about.
    ///
    /// # Errors
    ///
    /// The first [`Refusal`] found, in the order of `reports`.
    pub fn check_centers(
        &self,
        requested: &[AtomId],
        reports: &[CenterReport],
    ) -> Result<(), Refusal> {
        if reports.len() != requested.len()
            || reports
                .iter()
                .zip(requested)
                .any(|(report, &asked)| report.atom != asked)
        {
            return Err(refused("", Problem::Units));
        }
        for report in reports {
            let unit = report.atom.to_string();
            let Some(bonded) = self.neighbors.get(&report.atom) else {
                return Err(refused(&unit, Problem::Units));
            };
            let stored = stored_center(self.graph, report.atom);
            if report.stored != stored {
                return Err(refused(&unit, Problem::Stored));
            }
            check_state(
                &unit,
                report.state,
                center_kind(stored.as_ref()),
                |descriptor| matches!(descriptor, Descriptor::R | Descriptor::S),
            )?;
            let ranking_fits = match (&report.priority, report.decided_by) {
                (Some(priority), Some(_)) => {
                    // The four positions of the center: its bonded atoms and, with three of
                    // them, one non-atom slot; each exactly once, ranked 1 to 4 in order.
                    let slots: BTreeSet<Slot> = priority.iter().map(|&(slot, _)| slot).collect();
                    let atoms: BTreeSet<AtomId> = priority
                        .iter()
                        .filter_map(|&(slot, _)| match slot {
                            Slot::Atom(atom) => Some(atom),
                            Slot::ImplicitHydrogen | Slot::LonePair => None,
                        })
                        .collect();
                    is_ranked(report.state)
                        && priority.len() == 4
                        && slots.len() == 4
                        && priority
                            .iter()
                            .zip(1_u8..)
                            .all(|(&(_, rank), expected)| rank == expected)
                        && (3..=4).contains(&bonded.len())
                        && atoms == *bonded
                        && stored.as_ref().is_none_or(|state| {
                            slots.iter().copied().collect::<Vec<_>>() == state.slots()
                        })
                }
                (None, None) => !is_ranked(report.state),
                _ => false,
            };
            if !ranking_fits {
                return Err(refused(&unit, Problem::Ranking));
            }
            let slot_unusual = match self.stored_slot_differs(report.atom, stored.as_ref()) {
                Some(true) if matches!(report.state, State::Unsupported(_)) => SlotUnusual::Either,
                Some(true) => SlotUnusual::Required,
                Some(false) | None => SlotUnusual::Never,
            };
            if !warnings_fit(
                &report.warnings,
                report.state,
                stored.is_some(),
                slot_unusual,
            ) {
                return Err(refused(&unit, Problem::Warnings));
            }
        }
        Ok(())
    }

    /// Checks a labeler's answer for `requested` double bonds, as
    /// [`Checker::check_centers`] does for centers.
    ///
    /// # Errors
    ///
    /// The first [`Refusal`] found, in the order of `reports`.
    pub fn check_double_bonds(
        &self,
        requested: &[BondId],
        reports: &[BondReport],
    ) -> Result<(), Refusal> {
        if reports.len() != requested.len()
            || reports
                .iter()
                .zip(requested)
                .any(|(report, &asked)| report.bond != asked)
        {
            return Err(refused("", Problem::Units));
        }
        for report in reports {
            let unit = report.bond.to_string();
            let Some(&(begin, end)) = self.ends.get(&report.bond) else {
                return Err(refused(&unit, Problem::Units));
            };
            let stored = stored_bond(self.graph, report.bond);
            if report.stored != stored {
                return Err(refused(&unit, Problem::Stored));
            }
            check_state(
                &unit,
                report.state,
                bond_kind(stored.as_ref()),
                |descriptor| matches!(descriptor, Descriptor::E | Descriptor::Z),
            )?;
            let ends_fit = match &report.ends {
                Some(ends) => {
                    is_ranked(report.state)
                        && ends[0].atom == begin
                        && ends[1].atom == end
                        && self.is_position_of_end(begin, end, ends[0].higher)
                        && self.is_position_of_end(end, begin, ends[1].higher)
                }
                None => !is_ranked(report.state),
            };
            if !ends_fit {
                return Err(refused(&unit, Problem::Ranking));
            }
            if !warnings_fit(
                &report.warnings,
                report.state,
                stored.is_some(),
                SlotUnusual::Never,
            ) {
                return Err(refused(&unit, Problem::Warnings));
            }
        }
        Ok(())
    }

    /// Checks a labeler's scan: each list ascending without repeats, and every report an answer
    /// for its own unit as for a request.
    ///
    /// # Errors
    ///
    /// The first [`Refusal`] found.
    pub fn check_candidates(&self, found: &Candidates) -> Result<(), Refusal> {
        let atoms: Vec<AtomId> = found.centers.iter().map(|report| report.atom).collect();
        let bonds: Vec<BondId> = found
            .double_bonds
            .iter()
            .map(|report| report.bond)
            .collect();
        if atoms.windows(2).any(|pair| pair[0] >= pair[1])
            || bonds.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(refused("", Problem::Units));
        }
        self.check_centers(&atoms, &found.centers)?;
        self.check_double_bonds(&bonds, &found.double_bonds)?;
        // The charge is what the request budget loses to the scan, so it cannot be less than
        // what the reports it returns cost.
        let cost = found
            .centers
            .iter()
            .map(|report| report.nodes)
            .chain(found.double_bonds.iter().map(|report| report.nodes))
            .fold(0_usize, usize::saturating_add);
        if cost > found.nodes {
            return Err(refused("", Problem::Nodes));
        }
        Ok(())
    }
}
