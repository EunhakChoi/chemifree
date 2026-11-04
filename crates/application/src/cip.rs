//! The CIP labeler port: what `chem.cip` asks of a labeler, the built-in labeler, and the check
//! every answer passes before it can enter a reply.
//!
//! A labeler is domain logic, not infrastructure, so the built-in one ([`BuiltinCip`]) lives in
//! this crate and calls [`chemifree_domain::cip`]. The port exists so that a different labeler
//! (a local-only engine, later) can replace it without touching the operation. Nothing is
//! loaded, discovered or selected per request, and `local_model_loading` stays unimplemented.
//!
//! The application never trusts the answer. [`check_centers`] and [`check_double_bonds`] refuse
//! an answer that does not name each requested unit exactly once, in the order asked, whose
//! state does not fit the unit's kind or its stored record, whose ranking is not a ranking, or
//! whose reason does not fit its state, so a replacement cannot widen the wire contract.

use std::collections::BTreeSet;
use std::fmt;

use chemifree_domain::budget::Interrupt;
use chemifree_domain::cip::{
    self, BondReport, Candidates, CenterReport, Descriptor, Limits, Reason, State, Stop, Warning,
};
use chemifree_domain::document::{AtomId, BondId};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::stereo::{CenterState, DoubleBondState, Slot, StereoKey, StereoState};

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

/// The units the candidate scan of `scope: "stereogenic"` finds. The scan is core logic of the
/// built-in rules, not part of the port: it only decides which units a labeler is asked about.
///
/// # Errors
///
/// As [`CipLabeler::label_centers`].
pub fn scan_candidates(
    graph: &MoleculeGraph,
    limits: Limits,
    interrupt: &dyn Interrupt,
) -> Result<Candidates, Stop> {
    cip::candidates(graph, limits, interrupt)
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
    slot_unusual_allowed: bool,
) -> bool {
    let distinct = warnings
        .iter()
        .enumerate()
        .all(|(index, warning)| !warnings[..index].contains(warning));
    let not_stereogenic = warnings.contains(&Warning::StoredNotStereogenic);
    distinct
        && not_stereogenic == (state == State::None && stored)
        && (slot_unusual_allowed || !warnings.contains(&Warning::SlotUnusual))
}

fn slot_known(slot: Slot, atoms: &BTreeSet<AtomId>) -> bool {
    match slot {
        Slot::Atom(atom) => atoms.contains(&atom),
        Slot::ImplicitHydrogen | Slot::LonePair => true,
    }
}

/// Checks a labeler's answer for `requested` centers against the document it was asked about.
///
/// # Errors
///
/// The first [`Refusal`] found, in the order of `reports`.
pub fn check_centers(
    graph: &MoleculeGraph,
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
    let atoms: BTreeSet<AtomId> = graph.atoms().map(|(id, _)| id).collect();
    for report in reports {
        let unit = report.atom.to_string();
        let stored = stored_center(graph, report.atom);
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
                is_ranked(report.state)
                    && priority.len() == 4
                    && priority
                        .iter()
                        .zip(1_u8..)
                        .all(|(&(slot, rank), expected)| {
                            rank == expected && slot_known(slot, &atoms)
                        })
                    && priority
                        .iter()
                        .map(|&(slot, _)| slot)
                        .collect::<BTreeSet<_>>()
                        .len()
                        == 4
                    && stored.as_ref().is_none_or(|state| {
                        let mut slots: Vec<Slot> = priority.iter().map(|&(slot, _)| slot).collect();
                        slots.sort_unstable();
                        slots == state.slots()
                    })
            }
            (None, None) => !is_ranked(report.state),
            _ => false,
        };
        if !ranking_fits {
            return Err(refused(&unit, Problem::Ranking));
        }
        if !warnings_fit(
            &report.warnings,
            report.state,
            stored.is_some(),
            stored.is_some(),
        ) {
            return Err(refused(&unit, Problem::Warnings));
        }
    }
    Ok(())
}

/// Checks a labeler's answer for `requested` double bonds, as [`check_centers`] does for centers.
///
/// # Errors
///
/// The first [`Refusal`] found, in the order of `reports`.
pub fn check_double_bonds(
    graph: &MoleculeGraph,
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
    let atoms: BTreeSet<AtomId> = graph.atoms().map(|(id, _)| id).collect();
    for report in reports {
        let unit = report.bond.to_string();
        let stored = stored_bond(graph, report.bond);
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
                let own = graph
                    .bonds()
                    .find(|&(id, _)| id == report.bond)
                    .map(|(_, bond)| bond);
                is_ranked(report.state)
                    && own
                        .is_some_and(|bond| ends[0].atom == bond.begin && ends[1].atom == bond.end)
                    && ends.iter().all(|end| slot_known(end.higher, &atoms))
            }
            None => !is_ranked(report.state),
        };
        if !ends_fit {
            return Err(refused(&unit, Problem::Ranking));
        }
        if !warnings_fit(&report.warnings, report.state, stored.is_some(), false) {
            return Err(refused(&unit, Problem::Warnings));
        }
    }
    Ok(())
}
