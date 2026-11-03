//! CIP descriptors (R and S) of stored tetrahedral centers, `chemifree-cip-1` in progress.
//!
//! This is the domain half of the first slice (centers only). It reads the stored statement of
//! a center and the graph, ranks the center's four ligands, and returns a label state. It never
//! reads a mark or a position, stores nothing, and a label is never turned back into a stereo
//! record: nothing here converts a [`Descriptor`] to a [`crate::stereo::CenterState`].
//!
//! # Rules applied
//!
//! Rule 1a (atomic number), Rule 1b (a duplicate atom whose corresponding atom is nearer the
//! root ranks higher; duplicates of ordinary ring closures and explicit multiple bonds only),
//! and Rule 2 (mass number), each applied over the whole hierarchical digraph before the next
//! rule is consulted. Rule 3 (Z before E) is not implemented yet: a tie after Rule 2 whose
//! digraphs contain a double bond that could separate the ligands is
//! [`Reason::Rule3Pending`], a temporary reason the Rule 3 slice removes. Rules 4a to 5 are
//! never applied: a tie that they might break is [`Reason::Rule4Or5Needed`], never `none`.
//!
//! # Digraph
//!
//! The digraph of a ligand is built lazily, one sphere at a time, from the center as root.
//! Multiple bonds and ring closures give duplicate atoms, a missing substituent is a phantom
//! atom (atomic number 0), a lone pair is a phantom, and implicit hydrogens are hydrogen atoms.
//! An all-carbon neutral aromatic ring system (as `chemifree-aromaticity-2` classifies it) is
//! read as mancude: each of its atoms gets one duplicate carbon and the recorded bond orders
//! inside the system are ignored, so a Kekulé and an aromatic spelling give one digraph. Rule
//! 1b is not defined for those duplicates, so a comparison that needs it is refused. Any other
//! aromatic system is refused as soon as a comparison needs to look inside it.
//!
//! # Determinism and limits
//!
//! No decision depends on an atom or bond identifier: neighbors are processed as multisets of
//! ranks, and two nodes that compare equal under every rule have identical subtrees, so their
//! order cannot change a result. Work is counted in digraph nodes. Each pair of ligands is
//! compared in its own arena, so the count of an item does not depend on which pair runs
//! first; the per-item and per-request caps are checked at sphere boundaries. Reaching a cap
//! makes the item [`State::Unsupported`] with [`Reason::DigraphLimit`], never `none`.

mod context;
mod digraph;
mod label;
mod rank;

use std::fmt;

use crate::budget::{Interrupt, Interrupted};
use crate::document::AtomId;
use crate::graph::MoleculeGraph;
use crate::stereo::{CenterState, Slot};

/// The identifier of the labeler once it is exposed; the first slice exposes nothing.
pub const MODEL_ID: &str = "chemifree-cip-1";

/// The nodes one item may materialize, summed over the ligand pairs it compares.
pub const MAX_ITEM_NODES: usize = 20_000;
/// The nodes one request may materialize, summed over its items.
pub const MAX_REQUEST_NODES: usize = 2_000_000;

/// A CIP sequence rule, in the order they are applied.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Rule {
    /// Higher atomic number first.
    R1a,
    /// A duplicate atom nearer the root first.
    R1b,
    /// Higher mass number first.
    R2,
}

impl Rule {
    /// The rule's name in the conventional notation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::R1a => "1a",
            Self::R1b => "1b",
            Self::R2 => "2",
        }
    }
}

/// A descriptor of a stereocenter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Descriptor {
    R,
    S,
}

impl Descriptor {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::R => "R",
            Self::S => "S",
        }
    }
}

/// Why a unit is `unsupported` or `ambiguous`. The variants are ordered by priority: when two
/// comparisons of one item fail for different reasons the smaller one is reported, so the
/// answer does not depend on which comparison ran first.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Reason {
    /// A named atom that cannot be a center: absent, or not 3 or 4 bonded neighbors.
    NotACandidate,
    /// The center fails the structure test: not an element atom, a radical or an aromatic bond
    /// on the center, or a stored statement the valence model cannot place.
    CenterStructure,
    QueryAtom,
    /// A dative, unknown or query bond met while expanding.
    BondKind,
    AttachmentPoint,
    /// The valence model cannot give the hydrogens of an atom that must be expanded: unknown
    /// hydrogens, a radical, an element or charge it does not list, an unusual valence.
    HydrogensUnknown,
    AromaticSystem,
    /// Rule 1b is needed on a digraph with a mancude duplicate.
    MancudeRule1b,
    /// Rule 2 compares a labeled atom with an unlabeled atom of an element without a
    /// built-in mass number.
    MassNumber,
    /// Temporary: Rule 3 could separate tied ligands and is not implemented yet. Removed by the
    /// Rule 3 slice.
    Rule3Pending,
    /// A tie that Rules 4 and 5 might break.
    Rule4Or5Needed,
    /// The pairwise results are not a consistent order (not expected; reported, not guessed).
    RankingInconsistent,
    DigraphLimit,
}

impl Reason {
    /// The stable machine-readable code.
    pub const fn code(self) -> &'static str {
        match self {
            Self::NotACandidate => "cip_not_a_candidate",
            Self::CenterStructure => "cip_center_structure",
            Self::QueryAtom => "cip_query_atom",
            Self::BondKind => "cip_bond_kind",
            Self::AttachmentPoint => "cip_attachment_point",
            Self::HydrogensUnknown => "cip_hydrogens_unknown",
            Self::AromaticSystem => "cip_aromatic_system",
            Self::MancudeRule1b => "cip_mancude_rule_1b",
            Self::MassNumber => "cip_mass_number",
            Self::Rule3Pending => "cip_rule_3_pending",
            Self::Rule4Or5Needed => "cip_rule_4_or_5_needed",
            Self::RankingInconsistent => "cip_ranking_inconsistent",
            Self::DigraphLimit => "cip_digraph_limit",
        }
    }
}

/// What the algorithm found for one unit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    /// Stored configuration plus a complete ranking gives a label.
    Assigned(Descriptor),
    /// The unit is not stereogenic under the implemented rules, and no unimplemented rule
    /// could separate the tied ligands.
    None,
    /// Stereogenic and nothing is stored.
    Unspecified,
    /// Stereogenic and the stored record says "either".
    Unknown,
    /// The label depends on stereo information the document does not have. Only Rule 3 can
    /// produce it, so nothing returns it yet.
    Ambiguous(Reason),
    /// The model cannot decide.
    Unsupported(Reason),
}

/// An advisory finding about a unit that still has a state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Warning {
    /// A configuration is stored on a unit that is not stereogenic.
    StoredNotStereogenic,
    /// The stored non-atom slot differs from the one the valence model names; the stored slot
    /// was used.
    SlotUnusual,
}

impl Warning {
    pub const fn code(self) -> &'static str {
        match self {
            Self::StoredNotStereogenic => "cip_stored_not_stereogenic",
            Self::SlotUnusual => "cip_slot_unusual",
        }
    }
}

/// The result for one atom.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CenterReport {
    pub atom: AtomId,
    pub state: State,
    /// The stored record, as stated, if any.
    pub stored: Option<CenterState>,
    /// The four positions, highest priority first, with their dense rank from 1; positions
    /// that tie share a rank and are listed by slot order (formatting only). Present when the
    /// ranking is complete and distinct (`Assigned`, `Unknown`, `Unspecified`).
    pub priority: Option<Vec<(Slot, u8)>>,
    /// The last rule needed to separate the four positions; present with `priority`.
    pub decided_by: Option<Rule>,
    pub warnings: Vec<Warning>,
    /// Digraph nodes this item materialized.
    pub nodes: usize,
}

/// Work limits, in digraph nodes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    pub item_nodes: usize,
    pub request_nodes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            item_nodes: MAX_ITEM_NODES,
            request_nodes: MAX_REQUEST_NODES,
        }
    }
}

/// Why a request produced no reports at all.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stop {
    /// The items together materialized more nodes than [`Limits::request_nodes`].
    RequestLimit {
        limit: usize,
    },
    Interrupted(Interrupted),
}

impl fmt::Display for Stop {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequestLimit { limit } => {
                write!(
                    formatter,
                    "the request needs more than {limit} digraph nodes"
                )
            }
            Self::Interrupted(Interrupted::Cancelled) => formatter.write_str("cancelled"),
            Self::Interrupted(Interrupted::TimeLimit) => formatter.write_str("time limit"),
        }
    }
}

impl std::error::Error for Stop {}

/// Labels the named atoms as stereocenters, one report per name in the order given.
///
/// A stored record is read as stated; an atom without one is judged on its structure alone
/// (`None`, `Unspecified` or `Unsupported`). The interrupt is asked once per sphere of every
/// comparison, so a cancel stops the request at a sphere boundary and yields no reports.
///
/// # Errors
///
/// [`Stop::RequestLimit`] when the items together exceed [`Limits::request_nodes`] (the sum
/// does not depend on the order of `atoms`), [`Stop::Interrupted`] when `interrupt` stops it.
pub fn label_centers(
    graph: &MoleculeGraph,
    atoms: &[AtomId],
    limits: Limits,
    interrupt: &dyn Interrupt,
) -> Result<Vec<CenterReport>, Stop> {
    let context = context::Context::new(graph);
    let mut used = 0_usize;
    let mut reports = Vec::with_capacity(atoms.len());
    for &atom in atoms {
        let report = label::center(&context, atom, limits.item_nodes, interrupt)?;
        used = used.saturating_add(report.nodes);
        if used > limits.request_nodes {
            return Err(Stop::RequestLimit {
                limit: limits.request_nodes,
            });
        }
        reports.push(report);
    }
    Ok(reports)
}
