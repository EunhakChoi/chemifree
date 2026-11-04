//! CIP descriptors (R, S, E and Z) of stored configurations, `chemifree-cip-1` in progress.
//!
//! This is the domain half of the labeler: tetrahedral centers ([`label_centers`]) and double
//! bonds ([`label_double_bonds`]). It reads the stored statement of a unit and the graph, ranks
//! the unit's ligands (or the two substituents at each end of a double bond), and returns a
//! label state. It never reads a mark or a position, stores nothing, and a label is never
//! turned back into a stereo record: nothing here converts a [`Descriptor`] to a
//! [`crate::stereo::CenterState`] or a [`crate::stereo::DoubleBondState`].
//!
//! # Rules applied
//!
//! Rule 1a (atomic number), Rule 1b (a duplicate atom whose corresponding atom is nearer the
//! root ranks higher; duplicates of ordinary ring closures and explicit multiple bonds only),
//! Rule 2 (mass number) and Rule 3 (Z before E), each applied over the whole hierarchical
//! digraph before the next rule is consulted. Rules 4a to 5 are never applied: a tie that they
//! might break is [`Reason::Rule4Or5Needed`], never `none`.
//!
//! # Rule 1b and the distance of a duplicate
//!
//! Rule 1b measures a duplicate to "the corresponding nonduplicated atom node". For a ring
//! closure that is the atom on the path the ring closes on. For the duplicate that a multiple
//! bond adds to the far atom of its parent, the model measures to the parent, one sphere
//! nearer than the duplicate itself; the far end's duplicate lies in the sphere of the atom it
//! stands for. RDKit 2022.03.2 ranks both multiple-bond duplicates in their own sphere (and
//! ranks the second duplicate of a multiple bond that also closes a ring below them), and
//! orders two ligands differently from the model when Rules 1a and 2 leave them to Rule 1b in
//! a small strained ring (two fused four-membered rings with ring double bonds, such as
//! `C12=CC[C@@]1(C)C(C=C)=C2`; 5 of 31 such centers in a generated set). The rule's text does
//! not say which is meant, so a pair of duplicates the two readings order differently is
//! [`Reason::Rule1bDuplicateDistance`], not a guess. The check is on the pair of nodes
//! compared, so it can refuse a center whose final order the other reading would also give
//! (about three in four of the refused centers of the generated sets); it never labels a
//! center the readings could order differently. Two duplicates that are siblings (a ligand is
//! never a duplicate) are exempt: they have no children, so which comes first changes nothing.
//!
//! # Rule 3
//!
//! A node reached from a real parent by a double bond carries the descriptor of that bond:
//! `Z` ranks above `E`, which ranks above a bond that is not stereogenic (and above a node with
//! no double bond). The descriptor of a ligand double bond is intrinsic: it ranks the two
//! substituents at each end of the bond in the whole molecule (the partner excluded as a
//! ligand), with the same rules, and reads the stored relation through them. A bond whose
//! descriptor depends on itself is [`Reason::StereoCycle`]. Two nodes at the same position of
//! the two digraphs of a comparison that are joined to their parents by one and the same double
//! bond are equal under Rule 3 whatever its descriptor, and the descriptor is not asked for: the
//! two ring branches at an end of 1,4-diethylidenecyclohexane meet the other double bond alike,
//! so neither bond is a cycle (both are `none`). Two different bonds whose descriptors need each
//! other (tetra-ethylidenecyclobutane) still are. A ligand double bond that is
//! stereogenic but has no stored record, or is stored as unknown, has no descriptor: when a
//! Rule 3 comparison has to place it against a descriptor it could equal, the result is
//! [`State::Ambiguous`] with [`Reason::LigandStereoUnspecified`], never a guess and never
//! `none`. (RDKit's labeler ranks such a bond below `E`; no IUPAC text this model rests on
//! says so, so it is not adopted.) Two unspecified ligand double bonds that are otherwise
//! equal are ambiguous too, because they may differ.
//!
//! The intrinsic reading is the model's, and RDKit 2022.03.2's labeler reads the descriptor
//! of a ligand double bond in the digraph rooted at the unit being labeled instead; no public
//! IUPAC statement this model rests on says which one the rule means, so where the two can
//! differ the result is [`Reason::Rule3NestedDescriptor`], not a guess. They differ in two
//! ways. The cut: the digraph of a double bond `X=Y` has `X` as root and no real child `Y`
//! (only the duplicate), so a ligand double bond whose end ranking reaches `X` sees `X`
//! without `Y` there and with it in the whole molecule. The ring: a ring closes at a different
//! atom in the digraph than in the whole molecule, so a ranking that goes round a ring can
//! order its neighbors differently. A descriptor read by a comparison is refused when the
//! ranking of its ends closes a ring anywhere in what it reads in turn, or, for a unit that
//! is a double bond, reaches an atom of that bond and reads differently once the bond is left
//! out of the digraphs. A tie at one end of the ligand bond that is the same either way (no
//! ring closes in it and it reaches no atom of the unit) makes the bond not stereogenic in
//! every reading: it is not refused when the tie read no descriptor (the two hydrogens of a
//! `=CH2`), and when it did the descriptors it read are checked in the same way. The check is made on the result of a unit, not inside the
//! descriptors kept for the request, so it does not change them. The refusal is narrow but not
//! exact: some refused bonds agree with RDKit (`tests/cip-fixtures.json` and
//! `scripts/verify-cip-rdkit.py` keep cases of both kinds).
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
//! # Rule 2 and unlabeled atoms
//!
//! A labeled atom ranks by its mass number. An unlabeled atom of an element in the built-in
//! table counts as the table's number (the most abundant natural isotope) only against labels
//! that differ from it: `[11CH3]` ranks below and `[13CH3]` above an unlabeled methyl under
//! every reading of the rule. Whether `[12CH3]` ties with an unlabeled methyl (unlabeled means
//! the most abundant isotope) or ranks above it (unlabeled means the atomic weight, a little
//! over 12) is not decided here, so comparing an unlabeled atom with a label equal to its table
//! number is [`Reason::MassNumber`], never a tie and never an order. The same holds for an
//! unlabeled atom of an element outside the table against any label.
//!
//! # Determinism and limits
//!
//! No decision depends on an atom or bond identifier: neighbors are processed as multisets of
//! ranks, and two nodes that compare equal under every rule have identical subtrees, so their
//! order cannot change a result. A refusal does not stop a comparison (see the engine in
//! `rank.rs`), so the reason an item is refused under and the work counted for it do not
//! depend on which refused atom a lazy expansion would have met first.
//!
//! Work is counted in digraph nodes. Each pair of ligands is compared in its own arena and each
//! tie classification has its own, so the count of an item does not depend on which pair runs
//! first. A comparison whose arena grows past [`Limits::item_nodes`], or that must expand an
//! atom [`MAX_SPHERES`] spheres from the center, is charged `item_nodes + 1` and the item is
//! [`State::Unsupported`] with [`Reason::DigraphLimit`], never `none`; any other comparison is
//! charged the nodes it materialized, refused or not. The cap is checked once per sphere, so a
//! comparison may exceed it by at most one sphere (and its charge is the true count only when
//! it stays below). The request cap is checked after each item.
//!
//! The sphere bound is also what bounds the stack: ordering the children of a node compares
//! subtrees, which orders the children of the nodes in them, so ties nest one level of
//! recursion per sphere. A digraph of a document within [`crate::document::MAX_ATOMS`] can
//! nest about a thousand levels, which overflows a 2 MiB thread stack in a debug build at
//! about 630 (see `tests/cip/main.rs`).
//!
//! Rule 3 ranks the ends of ligand double bonds inside a comparison, which is a nested
//! computation. Those use their own arenas, bounded by [`MAX_NESTED_SPHERES`] spheres and by
//! [`MAX_RULE_3_NESTING`] descriptors waiting on one another; going past either is
//! [`Reason::DigraphLimit`]. The stack of the deepest cases that can be built within a document
//! was measured in a debug build by lowering a thread's stack until the labeler overflowed:
//! a spine of 250 tied levels alone needs about 0.92 MiB, four descriptors waiting on one
//! another in arenas 25 to 32 spheres deep about 0.13 MiB, and the two together (a spine of 215
//! levels above that nest, 245 spheres in all) about 0.89 MiB, so the nested bounds do not add
//! to the center's. The test that builds them runs with 1.5 MiB.
//!
//! A double bond is ranked within [`MAX_NESTED_SPHERES`] spheres of each of its ends whether it
//! is labeled itself or its descriptor is needed by Rule 3, while the ligands of a center are
//! ranked within [`MAX_SPHERES`]. Two substituents that differ only 33 to 256 spheres away are
//! therefore separated as the ligands of a center and refused ([`Reason::DigraphLimit`]) as the
//! two substituents at an end of a double bond, so a label near the bound can depend on the
//! root it is asked from. This is a documented limit: the refusal is `unsupported`, never a
//! wrong label. Each descriptor is computed once per request, and every pair that
//! needs it is charged what it cost again (the descriptor and the descriptors it needs), so
//! the work counted does not depend on which pair or item ran first. The questions of one tie
//! classification (could two positions of an atom in a tied ligand differ?) are charged each
//! descriptor's cost once for the whole classification, and every pair of positions of an atom
//! is asked, so the charge does not depend on the order the neighbors are listed in. The cost
//! of the descriptors an engine needed is counted once, after its comparison. A descriptor whose
//! computation met a cycle (a bond asked for while it was being computed) is the exception: its
//! result depends on which bond of the cycle was asked for first, so it is not kept for the
//! request but reused only while the stack of bonds being computed is the same, and a bond
//! asked for later is computed afresh. A kept descriptor records how many descriptors deep its
//! computation went and is used only where that fits [`MAX_RULE_3_NESTING`] from the position
//! it is asked at (a bond asked for after the descriptors below it, which would otherwise be
//! answered from the memo, is refused as the same bond asked for alone is), and a charge counts
//! a descriptor that does not fit from where it is met as missing, level by level from the
//! item. The label, reason and charge of a bond are therefore the same whatever the order in
//! which the bonds of a request are listed.

mod bond;
mod context;
mod digraph;
mod label;
mod rank;

use std::fmt;

use crate::budget::{Interrupt, Interrupted};
use crate::document::{AtomId, BondId};
use crate::graph::MoleculeGraph;
use crate::stereo::{CenterState, DoubleBondState, Slot};

/// The identifier of the labeler once it is exposed; the first slice exposes nothing.
pub const MODEL_ID: &str = "chemifree-cip-1";

/// The nodes one item may materialize, summed over the ligand pairs it compares. The cap is
/// checked once per sphere, so a comparison may exceed it by at most one sphere.
pub const MAX_ITEM_NODES: usize = 20_000;
/// The nodes one request may materialize, summed over its items.
pub const MAX_REQUEST_NODES: usize = 2_000_000;
/// The farthest sphere (the ligand atoms are sphere 1) whose atoms are expanded; a comparison
/// that needs sphere [`MAX_SPHERES`] + 1 is [`Reason::DigraphLimit`]. Tied branches nest the
/// comparison once per sphere, and a frame of the three functions involved is about 3.3 KB in
/// a debug build, so 256 spheres need about 0.85 MB of a 2 MiB thread stack, and a digraph
/// 900 spheres deep (possible in a 2,000-atom document) would overflow it.
pub const MAX_SPHERES: usize = 256;
/// The sphere bound of the arenas that rank the ends of a ligand double bond for Rule 3 (and of
/// a double bond labeled as a unit, which is the same computation).
pub const MAX_NESTED_SPHERES: usize = 32;
/// The most double-bond descriptors that may be waiting on one another; a request for one more
/// is [`Reason::DigraphLimit`].
pub const MAX_RULE_3_NESTING: usize = 4;

/// A CIP sequence rule, in the order they are applied.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Rule {
    /// Higher atomic number first.
    R1a,
    /// A duplicate atom nearer the root first.
    R1b,
    /// Higher mass number first.
    R2,
    /// `Z` before `E` before a double bond that is not stereogenic.
    R3,
}

impl Rule {
    /// The rule's name in the conventional notation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::R1a => "1a",
            Self::R1b => "1b",
            Self::R2 => "2",
            Self::R3 => "3",
        }
    }
}

/// A descriptor of a stereocenter (`R`, `S`) or a double bond (`E`, `Z`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Descriptor {
    R,
    S,
    E,
    Z,
}

impl Descriptor {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::R => "R",
            Self::S => "S",
            Self::E => "E",
            Self::Z => "Z",
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
    /// The double bond fails the structure test: an end with cumulated bonds (an allene), more
    /// than two other neighbors, positions the valence model does not fit to two, or a stored
    /// reference that names none of an end's positions.
    DoubleBondStructure,
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
    /// Rule 1b compares two duplicates that the two readings of "the corresponding nonduplicated
    /// atom" order differently: the atom a duplicate stands for (which for the duplicate that a
    /// multiple bond adds to its far atom is the parent, a sphere nearer) or the sphere the
    /// duplicate itself lies in (RDKit 2022.03.2). It happens in small strained rings, where a
    /// ring closes a few spheres from the root and Rules 1a and 2 leave the order to Rule 1b.
    Rule1bDuplicateDistance,
    /// Rule 2 compares an unlabeled atom with a labeled one and the answer depends on how an
    /// unlabeled atom is read: the label equals the element's built-in mass number, or the
    /// element has none.
    MassNumber,
    /// A tie that Rules 4 and 5 might break.
    Rule4Or5Needed,
    /// The descriptor of a ligand double bond depends on itself.
    StereoCycle,
    /// Rule 3 needs the descriptor of a ligand double bond that can be read two ways, which
    /// can differ: ranked in the whole molecule, where the labeled unit's own atoms and rings
    /// are part of what the ends of the ligand bond look at, or ranked inside the digraph
    /// rooted at the labeled unit, where they are not. It is refused when the ligand bond's
    /// ranking reaches an atom of the labeled double bond and the same ranking without that
    /// bond reads differently, or when it passes through a ring that closes on itself (the
    /// digraph closes a ring where the unit enters it), unless a tie at one end of the ligand
    /// bond makes it not stereogenic whichever way it is read (a tie that read no descriptor,
    /// or whose descriptors read the same either way). See the module documentation.
    Rule3NestedDescriptor,
    /// Only with [`State::Ambiguous`]: Rule 3 has to place a stereogenic ligand double bond
    /// whose geometry the document does not state (no record, or stored unknown).
    LigandStereoUnspecified,
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
            Self::DoubleBondStructure => "cip_double_bond_structure",
            Self::QueryAtom => "cip_query_atom",
            Self::BondKind => "cip_bond_kind",
            Self::AttachmentPoint => "cip_attachment_point",
            Self::HydrogensUnknown => "cip_hydrogens_unknown",
            Self::AromaticSystem => "cip_aromatic_system",
            Self::MancudeRule1b => "cip_mancude_rule_1b",
            Self::Rule1bDuplicateDistance => "cip_rule_1b_duplicate_distance",
            Self::MassNumber => "cip_mass_number",
            Self::Rule4Or5Needed => "cip_rule_4_or_5_needed",
            Self::StereoCycle => "cip_stereo_cycle",
            Self::Rule3NestedDescriptor => "cip_rule3_nested_descriptor",
            Self::LigandStereoUnspecified => "cip_ligand_stereo_unspecified",
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
    /// The label depends on stereo information the document does not have: Rule 3 needs the
    /// geometry of a ligand double bond that is unspecified or stored unknown. The reason is
    /// [`Reason::LigandStereoUnspecified`].
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

/// One end of a double bond for which a higher substituent was found.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EndReport {
    pub atom: AtomId,
    /// The position that ranks higher at this end.
    pub higher: Slot,
    /// The last rule needed to separate the end's two positions.
    pub decided_by: Rule,
}

/// The result for one double bond.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BondReport {
    pub bond: BondId,
    pub state: State,
    /// The stored record, as stated, if any.
    pub stored: Option<DoubleBondState>,
    /// The higher substituent at the bond's `begin` and `end` atoms; present when both ends have
    /// two different substituents (`Assigned`, `Unknown`, `Unspecified`).
    pub ends: Option<[EndReport; 2]>,
    /// `StoredNotStereogenic` when a record is stored on a bond that is not stereogenic.
    pub warnings: Vec<Warning>,
    /// Digraph nodes this item materialized, with the Rule 3 descriptors it needed.
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
/// comparison, at the start of the sphere, so a cancel stops the request at a sphere boundary
/// and yields no reports.
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
    let oracle = bond::Oracle::new(&context, limits.item_nodes, interrupt);
    let mut used = 0_usize;
    let mut reports = Vec::with_capacity(atoms.len());
    for &atom in atoms {
        let report = label::center(&context, &oracle, atom, limits.item_nodes, interrupt)?;
        charge(&mut used, report.nodes, limits)?;
        reports.push(report);
    }
    Ok(reports)
}

fn charge(used: &mut usize, nodes: usize, limits: Limits) -> Result<(), Stop> {
    *used = used.saturating_add(nodes);
    if *used > limits.request_nodes {
        return Err(Stop::RequestLimit {
            limit: limits.request_nodes,
        });
    }
    Ok(())
}

/// Labels the named bonds as double bonds, one report per name in the order given.
///
/// A stored record is read as stated; a bond without one is judged on its structure alone
/// (`None`, `Unspecified`, `Ambiguous` or `Unsupported`). `Assigned` is `E` or `Z` by the CIP
/// definition: the higher-ranked substituents on the same side is `Z`. Limits and the interrupt
/// are as for [`label_centers`]; the work of an item includes the Rule 3 descriptors it needed.
///
/// # Errors
///
/// [`Stop::RequestLimit`] when the items together exceed [`Limits::request_nodes`] (the sum
/// does not depend on the order of `bonds`), [`Stop::Interrupted`] when `interrupt` stops it.
pub fn label_double_bonds(
    graph: &MoleculeGraph,
    bonds: &[BondId],
    limits: Limits,
    interrupt: &dyn Interrupt,
) -> Result<Vec<BondReport>, Stop> {
    let context = context::Context::new(graph);
    let oracle = bond::Oracle::new(&context, limits.item_nodes, interrupt);
    let mut used = 0_usize;
    let mut reports = Vec::with_capacity(bonds.len());
    for &id in bonds {
        let report = bond::double_bond(&context, &oracle, id, limits.item_nodes)?;
        charge(&mut used, report.nodes, limits)?;
        reports.push(report);
    }
    Ok(reports)
}

/// The units that are, or may be, stereogenic.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Candidates {
    /// Ascending by atom.
    pub centers: Vec<CenterReport>,
    /// Ascending by bond.
    pub double_bonds: Vec<BondReport>,
}

/// Scans the graph for units that are stereogenic under Rules 1a to 3, or may be: an atom that
/// passes the center structure test and a double bond that passes the double-bond structure
/// test, whose label state is not `None`, whether or not a record is stored. A unit the model
/// cannot decide (`Unsupported`, `Ambiguous`) is listed, because it may be stereogenic; one
/// found not stereogenic is not.
///
/// This is not the list of every E/Z unit. A double bond inside a ring system is never listed
/// (the design default is that no ring-size judgement is made), although
/// [`label_double_bonds`] labels a stored ring double bond when it is named; a consumer must
/// not read the scan as the complete list of double bonds that carry or may carry an E or Z.
///
/// The interrupt is asked for every atom and bond, skipped or not, as well as at the spheres
/// of the units that are ranked.
///
/// # Errors
///
/// [`Stop::RequestLimit`] when the units examined together exceed [`Limits::request_nodes`],
/// [`Stop::Interrupted`] when `interrupt` stops it.
pub fn candidates(
    graph: &MoleculeGraph,
    limits: Limits,
    interrupt: &dyn Interrupt,
) -> Result<Candidates, Stop> {
    let context = context::Context::new(graph);
    let oracle = bond::Oracle::new(&context, limits.item_nodes, interrupt);
    let in_ring: std::collections::BTreeSet<BondId> = crate::rings::perceive(graph)
        .into_iter()
        .flat_map(|system| system.bonds)
        .collect();
    let mut used = 0_usize;
    let mut found = Candidates::default();
    for (id, _) in graph.atoms() {
        // Asked for every atom, examined or skipped, so that a scan that ranks nothing (an
        // acyclic chain) can still be stopped.
        interrupt.check().map_err(Stop::Interrupted)?;
        if !label::is_center(&context, id) {
            continue;
        }
        let report = label::center(&context, &oracle, id, limits.item_nodes, interrupt)?;
        charge(&mut used, report.nodes, limits)?;
        if report.state != State::None {
            found.centers.push(report);
        }
    }
    for (id, _) in graph.bonds() {
        interrupt.check().map_err(Stop::Interrupted)?;
        if in_ring.contains(&id) || !bond::is_unit(&context, id) {
            continue;
        }
        let report = bond::double_bond(&context, &oracle, id, limits.item_nodes)?;
        charge(&mut used, report.nodes, limits)?;
        if report.state != State::None {
            found.double_bonds.push(report);
        }
    }
    Ok(found)
}
