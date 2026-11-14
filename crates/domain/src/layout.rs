//! Chain layout for the preview: which atoms and bonds a layout moves, which stay fixed, where the
//! two meet, what the preview refuses, and where the moving atoms are placed.
//!
//! This is the deterministic core of the preview. It reads a document and states the layout set,
//! the fixed set, the boundary and the layout components. [`prepare`] returns a [`Preparation`]
//! only when every refusal below passes, and [`place`] or [`place_with`] lays it out.
//! [`move_commands`] turns a placed layout into the `MoveAtoms` commands that apply it.
//!
//! # Sets
//!
//! The layout set holds the selected atoms and the endpoints of the selected bonds, the seeds.
//! With [`Scope::Components`] it holds every atom of the graph-connected component of a seed
//! instead. The fixed set holds every other atom, and fixed atoms are never moved. A boundary bond
//! joins a layout atom to a fixed atom. A layout component is a connected component of the bonds
//! inside the layout set: one with a single boundary bond is anchored to that fixed atom, and one
//! with none is free.
//!
//! # Refusals
//!
//! [`prepare`] checks in this order and returns the first failure:
//!
//! 1. a selection that names scene objects: [`LayoutError::ObjectSelection`];
//! 2. an atom or bond the document does not have: [`LayoutError::UnknownSelection`];
//! 3. no atom and no bond: [`LayoutError::EmptySelection`];
//! 4. more than [`MAX_LAYOUT_ATOMS`] layout atoms: [`LayoutError::TooLarge`];
//! 5. a layout atom in a ring system: [`LayoutError::RingInLayoutSet`]. A ring system with no
//!    layout atom keeps its positions and is only counted when it lies in a graph-connected
//!    component that holds a layout atom;
//! 6. stated stereo or a bond mark on the layout set or a boundary fixed atom:
//!    [`LayoutError::StereoInLayout`];
//! 7. a layout atom that an abbreviation or fragment display of the instance covers: its key, the
//!    hidden terminal of an abbreviation, or a member of a fragment display:
//!    [`LayoutError::DisplayInLayoutSet`];
//! 8. a layout component with two or more boundary bonds: [`LayoutError::MultipleAttachments`];
//! 9. a layout atom, or an anchor, without a position: [`LayoutError::MissingPosition`];
//! 10. a fixed neighbour of an anchor at the anchor's own place:
//!     [`LayoutError::CoincidentFixedAtoms`].
//!
//! Each refusal has a stable code from [`LayoutError::reason`]. Nothing here changes the document.
//!
//! # Placement
//!
//! Each layout component is laid out as a tree whose bonds are one bond length long, in points
//! with y growing down. Anchored components come first, ordered by the grid cell of their fixed
//! anchor and then by rank (see `placement_order`). Free components follow, and each keeps the
//! centroid of its before positions. The rules are in the private `chain` module.
//!
//! [`place`] refuses a placed position outside the coordinate limits, naming the lowest such layout
//! atom in [`LayoutError::CoordinateOutOfRange`]. [`place_with`] also stops when its interrupt asks
//! it to.
//!
//! An overlap is a pair of atoms, one of them laid out, that are not bonded and lie closer than
//! [`OVERLAP_FACTOR`] bond lengths after placement. Overlaps are reported, not refused: the preview
//! counts every one and lists the first [`MAX_LISTED_OVERLAPS`] in ascending order.
//!
//! Renumbering the atoms leaves each placed position unchanged, except at ties that the rules leave
//! to identifiers: isomorphic siblings, the two ends of a free component with equal codes, and
//! isomorphic components at one anchor. At such a tie the atoms trade places under an automorphism,
//! so the set of positions is the same. One case is not covered: components anchored at two
//! distinct fixed atoms in one grid cell (see `placement_order`) are ordered by identifier when
//! their ranks tie, so renumbering can change the result there.

mod chain;

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::budget::{self, Interrupt, Interrupted, NEVER};
use crate::document::{
    Atom, AtomId, Bond, BondId, BondOrder, Command, Document, MAX_COMMANDS, ObjectId, Point, Vector,
};
use crate::graph::MoleculeGraph;
use crate::rings;
use crate::scene::Selection;
use crate::states::{AtomKind, AttachmentPoints, Hydrogens, QueryAtom, Radical};
use crate::stereo::StereoKey;

use chain::{Coord, Rooted};

/// The identifier of the layout model that the preview reports.
pub const MODEL_ID: &str = "chemifree-chain-layout-1";
/// The most atoms a layout set may hold. A larger set is refused before any placement.
pub const MAX_LAYOUT_ATOMS: usize = 500;
/// Two points at most this far apart, in pt, are one place. The anchor order compares points in
/// cells of this size, so two such points on opposite sides of a cell edge can be ordered apart;
/// see `placement_order`.
pub const COINCIDENT_PT: f64 = 1e-6;
/// The distance, in bond lengths, below which two non-bonded atoms count as overlapping.
pub const OVERLAP_FACTOR: f64 = 0.5;
/// The most overlapping pairs a [`Preview`] lists. Its `overlap_count` counts every pair.
pub const MAX_LISTED_OVERLAPS: usize = 100;

/// Which atoms a layout set takes from the selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scope {
    /// Every atom of each graph-connected component that holds a seed.
    Components,
    /// Exactly the seeds.
    Selection,
}

/// A bond between a layout atom and a fixed atom.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Boundary {
    /// The layout atom, which moves.
    pub atom: AtomId,
    /// The fixed atom at the other end, which stays where it is.
    pub fixed_atom: AtomId,
    pub bond: BondId,
}

/// The boundary bond that anchors a layout component to the fixed atoms.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Anchor {
    /// The fixed atom the component hangs from.
    pub fixed_atom: AtomId,
    pub bond: BondId,
}

/// A connected component of the bonds inside the layout set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Component {
    /// Its layout atoms, ascending.
    pub atoms: Vec<AtomId>,
    /// The bonds between its atoms, ascending. Its boundary bond, if it has one, is in `anchor`.
    pub bonds: Vec<BondId>,
    /// The boundary bond that anchors it, or `None` for a free component.
    pub anchor: Option<Anchor>,
}

/// Neighbours of every atom with the bonds that join them, ascending by neighbour then bond.
type Adjacency = BTreeMap<AtomId, Vec<(AtomId, BondId)>>;

/// What a layout of one selection would move and keep, ready for placement.
///
/// The public fields state the sets and the boundary. The private fields hold what placement
/// reads: the instance's positions, the canonical label of each layout atom, the order and the
/// beginning atom of each bond at a layout atom, and every atom's neighbours. A preparation holds
/// no reference to the document.
// Not `PartialEq`: nothing compares preparations, and a derived impl would compare private maps.
#[derive(Clone, Debug)]
pub struct Preparation {
    /// The scene instance whose positions the layout reads.
    pub instance: ObjectId,
    /// The document's bond length in pt, the length every placed bond is drawn at.
    pub bond_length_pt: f64,
    /// The layout set, ascending.
    pub layout_atoms: BTreeSet<AtomId>,
    /// The bonds with both ends in the layout set, ascending.
    pub layout_bonds: BTreeSet<BondId>,
    /// How many atoms are fixed: every atom outside the layout set.
    pub fixed_atom_count: usize,
    /// How many ring systems contain no layout atom and lie in a graph-connected component that
    /// holds a layout atom. Such systems keep their positions. Ring systems in other components are
    /// not searched for and not counted.
    pub fixed_ring_systems: usize,
    /// Every bond between a layout atom and a fixed atom, ascending by layout atom, then by fixed
    /// atom, then by bond.
    pub boundary: Vec<Boundary>,
    /// The layout components, ascending by their smallest atom.
    pub components: Vec<Component>,
    /// The position of every atom the instance places.
    positions: BTreeMap<AtomId, Point>,
    /// The canonical label of each layout atom, which states every atom state the document
    /// records. See [`atom_label`].
    labels: BTreeMap<AtomId, String>,
    /// The order of each bond that has a layout atom at either end.
    bond_orders: BTreeMap<BondId, BondOrder>,
    /// The atom that each of those bonds begins at. The direction of a dative bond is part of its
    /// code.
    bond_begins: BTreeMap<BondId, AtomId>,
    adjacency: Adjacency,
}

/// Why a layout is refused. A variant that lists atoms or bonds lists them in ascending order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LayoutError {
    /// The selection names scene objects, which a layout does not move.
    ObjectSelection,
    /// The selection names an atom or bond that the document does not have.
    UnknownSelection,
    /// The selection names no atom and no bond.
    EmptySelection,
    /// The layout set has `atoms` atoms, more than `limit`.
    TooLarge { atoms: usize, limit: usize },
    /// These layout atoms lie in ring systems. Ring layout is not implemented.
    RingInLayoutSet { atoms: Vec<AtomId> },
    /// Stated stereo touches the layout set or a boundary fixed atom. `atoms` are the stated
    /// centers there; `bonds` are the marked bonds and the stated double bonds there.
    StereoInLayout {
        atoms: Vec<AtomId>,
        bonds: Vec<BondId>,
    },
    /// These layout atoms are covered by an abbreviation or fragment display of the instance: each
    /// is a display's key, the hidden terminal of an abbreviation, or a member of a fragment
    /// display.
    DisplayInLayoutSet { atoms: Vec<AtomId> },
    /// A layout component has two or more boundary bonds, listed in `bonds`. `atoms` are the
    /// component's atoms.
    MultipleAttachments {
        atoms: Vec<AtomId>,
        bonds: Vec<BondId>,
    },
    /// The fixed neighbour `other` of the anchor `atom` lies at the anchor's position.
    CoincidentFixedAtoms { atom: AtomId, other: AtomId },
    /// A placed position of `atom` would lie outside the coordinate limits. Placement reports
    /// it; preparation never does.
    CoordinateOutOfRange { atom: AtomId },
    /// `atom`, a layout atom or an anchor, has no position in the instance.
    MissingPosition { atom: AtomId },
}

impl LayoutError {
    /// The stable code of this refusal, which is sent as its reason.
    pub const fn reason(&self) -> &'static str {
        match self {
            Self::ObjectSelection => "object_selection",
            Self::UnknownSelection => "selection_unknown",
            Self::EmptySelection => "layout_selection_empty",
            Self::TooLarge { .. } => "too_large",
            Self::RingInLayoutSet { .. } => "ring_in_layout_set",
            Self::StereoInLayout { .. } => "stereo_in_layout",
            Self::DisplayInLayoutSet { .. } => "display_in_layout_set",
            Self::MultipleAttachments { .. } => "multiple_attachments",
            Self::CoincidentFixedAtoms { .. } => "coincident_fixed_atoms",
            Self::CoordinateOutOfRange { .. } => "coordinate_out_of_range",
            Self::MissingPosition { .. } => "missing_position",
        }
    }
}

impl fmt::Display for LayoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ObjectSelection => {
                formatter.write_str("the selection names scene objects, not atoms or bonds")
            }
            Self::UnknownSelection => {
                formatter.write_str("the selection names an atom or bond that is missing")
            }
            Self::EmptySelection => formatter.write_str("the selection names no atom or bond"),
            Self::TooLarge { atoms, limit } => write!(
                formatter,
                "the layout set has {atoms} atoms, more than the limit of {limit}"
            ),
            Self::RingInLayoutSet { atoms } => write!(
                formatter,
                "ring layout is not implemented: {} layout atoms lie in ring systems",
                atoms.len()
            ),
            Self::StereoInLayout { .. } => {
                formatter.write_str("stated stereo or a bond mark touches the layout set")
            }
            Self::DisplayInLayoutSet { .. } => {
                formatter.write_str("an abbreviation or display covers a layout atom")
            }
            Self::MultipleAttachments { atoms, .. } => write!(
                formatter,
                "a layout component of {} atoms has more than one bond to fixed atoms",
                atoms.len()
            ),
            Self::CoincidentFixedAtoms { atom, other } => {
                write!(
                    formatter,
                    "fixed atom {other} lies at the place of anchor {atom}"
                )
            }
            Self::CoordinateOutOfRange { atom } => {
                write!(
                    formatter,
                    "atom {atom} would be placed outside the coordinate limits"
                )
            }
            Self::MissingPosition { atom } => {
                write!(formatter, "atom {atom} has no position in the instance")
            }
        }
    }
}

impl std::error::Error for LayoutError {}

/// A run of atoms placed one after another from the atom that the run branches from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Chain {
    /// The atoms of the run, in placement order.
    pub atoms: Vec<AtomId>,
    /// The atom the run branches from: the fixed anchor for the first chain of an anchored
    /// component, the parent atom for every other chain, and `None` for the first chain of a free
    /// component.
    pub branch_from: Option<AtomId>,
}

/// A layout component after placement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlacedComponent {
    /// Its layout atoms, ascending.
    pub atoms: Vec<AtomId>,
    /// The bonds between its atoms, ascending.
    pub bonds: Vec<BondId>,
    /// The boundary bond that anchors it, or `None` for a free component.
    pub anchor: Option<Anchor>,
    /// The atom that placement started from: the boundary atom of an anchored component, or the
    /// root of a free one.
    pub root: AtomId,
    /// Its chains, in the order their first atoms were placed.
    pub chains: Vec<Chain>,
}

/// Where one layout atom stands before and after placement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The layout atom.
    pub atom: AtomId,
    /// Its position in the instance, in pt with y growing down.
    pub before: Point,
    /// Its placed position, in the same units.
    pub after: Point,
}

/// A placed layout: its components, the placement of every layout atom, and its overlaps.
#[derive(Clone, Debug, PartialEq)]
pub struct Preview {
    /// The components in placement order: anchored components first, by the position of their
    /// anchor, then free components; both by rank.
    pub components: Vec<PlacedComponent>,
    /// One placement for every layout atom, ascending by atom.
    pub positions: Vec<Placement>,
    /// How many pairs of atoms overlap, each pair once. See the module documentation for what
    /// counts as an overlap.
    pub overlap_count: usize,
    /// The first [`MAX_LISTED_OVERLAPS`] overlapping pairs in ascending order, each once, with the
    /// smaller identifier first. The list holds every pair when `overlap_count` is at most that
    /// limit.
    pub overlaps: Vec<(AtomId, AtomId)>,
}

/// Prepares the layout set of `selection` in scene instance `instance` of `document`, or refuses.
///
/// `instance` must name a scene instance of `document`; the application checks that before it
/// calls this. For any other identifier no atom has a position, so the first layout atom is
/// refused with [`LayoutError::MissingPosition`].
///
/// # Errors
///
/// Every refusal of the module documentation, in the order given there.
pub fn prepare(
    document: &Document,
    instance: ObjectId,
    selection: &Selection,
    scope: Scope,
) -> Result<Preparation, LayoutError> {
    if !selection.objects.is_empty() {
        return Err(LayoutError::ObjectSelection);
    }
    let atoms: BTreeMap<AtomId, &Atom> = document.atoms().collect();
    let bonds: BTreeMap<BondId, &Bond> = document.bonds().collect();
    let seeds = selected_atoms(selection, &atoms, &bonds)?;
    let adjacency = adjacency_of(&atoms, &bonds);
    let layout_atoms = match scope {
        Scope::Selection => seeds,
        Scope::Components => connected_to(&seeds, &adjacency),
    };
    if layout_atoms.len() > MAX_LAYOUT_ATOMS {
        return Err(LayoutError::TooLarge {
            atoms: layout_atoms.len(),
            limit: MAX_LAYOUT_ATOMS,
        });
    }
    let fixed_atom_count = atoms
        .keys()
        .filter(|atom| !layout_atoms.contains(*atom))
        .count();
    let layout_bonds: BTreeSet<BondId> = bonds
        .iter()
        .filter(|(_, bond)| layout_atoms.contains(&bond.begin) && layout_atoms.contains(&bond.end))
        .map(|(id, _)| *id)
        .collect();
    let boundary = boundary_bonds(&bonds, &layout_atoms);
    let mut components: Vec<Component> = layout_components(&layout_atoms, &adjacency)
        .into_iter()
        .map(|(component_atoms, component_bonds)| Component {
            atoms: component_atoms.into_iter().collect(),
            bonds: component_bonds.into_iter().collect(),
            anchor: None,
        })
        .collect();
    let fixed_ring_systems = ring_check(&atoms, &bonds, &adjacency, &layout_atoms)?;
    let boundary_fixed: BTreeSet<AtomId> = boundary.iter().map(|entry| entry.fixed_atom).collect();
    stereo_check(document, &bonds, &layout_atoms, &boundary_fixed)?;
    display_check(document, instance, &layout_atoms)?;
    let anchors = anchors_of(&components, &boundary)?;
    for (component, anchor) in components.iter_mut().zip(anchors) {
        component.anchor = anchor;
    }
    let positions: BTreeMap<AtomId, Point> = document.positions(instance).collect();
    let labels: BTreeMap<AtomId, String> = layout_atoms
        .iter()
        .filter_map(|atom| atoms.get(atom).map(|record| (*atom, atom_label(record))))
        .collect();
    let bond_orders: BTreeMap<BondId, BondOrder> = bonds
        .iter()
        .filter(|(_, bond)| layout_atoms.contains(&bond.begin) || layout_atoms.contains(&bond.end))
        .map(|(id, bond)| (*id, bond.order.clone()))
        .collect();
    let bond_begins: BTreeMap<BondId, AtomId> = bonds
        .iter()
        .filter(|(_, bond)| layout_atoms.contains(&bond.begin) || layout_atoms.contains(&bond.end))
        .map(|(id, bond)| (*id, bond.begin))
        .collect();
    let preparation = Preparation {
        instance,
        bond_length_pt: document.settings().bond_length_pt,
        layout_atoms,
        layout_bonds,
        fixed_atom_count,
        fixed_ring_systems,
        boundary,
        components,
        positions,
        labels,
        bond_orders,
        bond_begins,
        adjacency,
    };
    preparation.check_positions()?;
    preparation.check_coincident()?;
    Ok(preparation)
}

impl Preparation {
    /// The position of `atom` in the instance.
    ///
    /// # Errors
    ///
    /// [`LayoutError::MissingPosition`] when the instance has no position for `atom`.
    fn position(&self, atom: AtomId) -> Result<Point, LayoutError> {
        self.positions
            .get(&atom)
            .copied()
            .ok_or(LayoutError::MissingPosition { atom })
    }

    /// Requires a position for every layout atom and every anchor.
    fn check_positions(&self) -> Result<(), LayoutError> {
        for &atom in &self.layout_atoms {
            self.position(atom)?;
        }
        for entry in &self.boundary {
            self.position(entry.fixed_atom)?;
        }
        Ok(())
    }

    /// Refuses an anchor with a fixed neighbour at its own place: the direction from the anchor to
    /// such a neighbour is undefined, so placement cannot start there. A fixed neighbour without a
    /// position is not placed and is not compared.
    ///
    /// # Errors
    ///
    /// [`LayoutError::CoincidentFixedAtoms`], naming the first such pair in identifier order.
    fn check_coincident(&self) -> Result<(), LayoutError> {
        let anchors: BTreeSet<AtomId> =
            self.boundary.iter().map(|entry| entry.fixed_atom).collect();
        for anchor in anchors {
            let origin = self.position(anchor)?;
            for &(neighbour, _) in neighbours(&self.adjacency, anchor) {
                if self.layout_atoms.contains(&neighbour) {
                    continue;
                }
                let coincident = self
                    .positions
                    .get(&neighbour)
                    .is_some_and(|point| distance(origin, *point) <= COINCIDENT_PT);
                if coincident {
                    return Err(LayoutError::CoincidentFixedAtoms {
                        atom: anchor,
                        other: neighbour,
                    });
                }
            }
        }
        Ok(())
    }
}

/// The result of a placement step that can stop early: a refusal, or an interruption.
type Halt = budget::Halt<LayoutError>;

impl From<LayoutError> for Halt {
    fn from(error: LayoutError) -> Self {
        Self::Refused(error)
    }
}

/// Places the layout components of `prep` and reports the preview. The interrupt is asked at the
/// start of each component, before each atom is placed, and before each layout atom of the overlap
/// scan.
///
/// # Errors
///
/// The outer `Err` is the interruption. The inner `Err` is [`LayoutError::CoordinateOutOfRange`]
/// for the lowest layout atom whose placed position lies outside the coordinate limits.
pub fn place_with(
    prep: &Preparation,
    interrupt: &dyn Interrupt,
) -> Result<Result<Preview, LayoutError>, Interrupted> {
    budget::interruptible(place_halting(prep, interrupt))
}

/// Places the layout components of `prep` and reports the preview, without an interrupt.
///
/// # Errors
///
/// [`LayoutError::CoordinateOutOfRange`] for the lowest layout atom whose placed position lies
/// outside the coordinate limits.
pub fn place(prep: &Preparation) -> Result<Preview, LayoutError> {
    budget::uninterrupted(place_halting(prep, &NEVER))
}

// One command per layout atom at most, so a prepared layout never exceeds a transaction's command
// limit and needs no refusal of its own.
const _: () = assert!(MAX_LAYOUT_ATOMS <= MAX_COMMANDS);

/// The edit that applies `preview` in scene instance `instance`: one [`Command::MoveAtoms`] for
/// each layout atom whose placed position differs from its before position, ascending by atom as
/// [`Preview::positions`] is. Each command translates that one atom by `after - before`, computed
/// in binary64, in the named instance. An atom with `after == before` (exact equality on both
/// coordinates, so `-0.0 == 0.0`) gets no command, so a free single atom never gets one. Atoms are
/// not grouped by equal delta. At most [`MAX_LAYOUT_ATOMS`] commands result.
#[must_use]
pub fn move_commands(preview: &Preview, instance: ObjectId) -> Vec<Command> {
    preview
        .positions
        .iter()
        .filter(|placement| placement.after != placement.before)
        .map(|placement| Command::MoveAtoms {
            atoms: vec![placement.atom],
            delta: Vector {
                dx: placement.after.x() - placement.before.x(),
                dy: placement.after.y() - placement.before.y(),
            },
            instance: Some(instance),
        })
        .collect()
}

/// A component with its root and tree chosen, ready to be placed in rank order.
struct Ranked<'a> {
    component: &'a Component,
    /// The smallest identifier among the component's atoms, the last key of its rank.
    smallest: AtomId,
    rooted: Rooted,
}

/// The placement behind [`place_with`] and [`place`]. It stops at the first refusal or
/// interruption.
fn place_halting(prep: &Preparation, interrupt: &dyn Interrupt) -> Result<Preview, Halt> {
    let mut ranked: Vec<Ranked<'_>> = Vec::with_capacity(prep.components.len());
    for component in &prep.components {
        let smallest = component
            .atoms
            .iter()
            .copied()
            .min()
            .expect("a layout component has at least one atom");
        ranked.push(Ranked {
            component,
            smallest,
            rooted: chain::root(prep, component)?,
        });
    }
    ranked.sort_by(placement_order);
    let mut placed: BTreeMap<AtomId, Coord> = BTreeMap::new();
    let mut components = Vec::with_capacity(ranked.len());
    for entry in &ranked {
        interrupt.check()?;
        let laid_out = chain::place(prep, &entry.rooted, &placed, interrupt)?;
        placed.extend(laid_out.positions);
        components.push(PlacedComponent {
            atoms: entry.component.atoms.clone(),
            bonds: entry.component.bonds.clone(),
            anchor: entry.component.anchor,
            root: entry.rooted.root,
            chains: laid_out.chains,
        });
    }
    let mut positions = Vec::with_capacity(prep.layout_atoms.len());
    for &atom in &prep.layout_atoms {
        let before = prep.position(atom)?;
        let here = placed_at(&placed, atom);
        let after =
            Point::new(here.x, here.y).map_err(|_| LayoutError::CoordinateOutOfRange { atom })?;
        positions.push(Placement {
            atom,
            before,
            after,
        });
    }
    let (overlap_count, overlaps) = overlap_pairs(prep, &placed, interrupt)?;
    Ok(Preview {
        components,
        positions,
        overlap_count,
        overlaps,
    })
}

/// The placed position of layout atom `atom`. Every layout atom belongs to a component, and every
/// component is placed before positions are read.
fn placed_at(placed: &BTreeMap<AtomId, Coord>, atom: AtomId) -> Coord {
    *placed
        .get(&atom)
        .expect("every layout atom belongs to a component, and every component is placed")
}

/// The order in which components are placed. Anchored components come first, by the grid cell of
/// their anchor (x, then y), where a cell is [`COINCIDENT_PT`] wide. Components whose anchors share
/// a cell, and the free components, are ordered by rank. Two anchors at most [`COINCIDENT_PT`]
/// apart can fall in different cells when they lie on opposite sides of a cell edge, and then the
/// cells decide instead of the rank.
fn placement_order(left: &Ranked<'_>, right: &Ranked<'_>) -> Ordering {
    match (left.rooted.origin(), right.rooted.origin()) {
        (Some(first), Some(second)) => grid_cell(first)
            .cmp(&grid_cell(second))
            .then_with(|| component_rank(left, right)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => component_rank(left, right),
    }
}

/// The cell of `at` in the grid of [`COINCIDENT_PT`]-wide cells, as its index in x and in y.
fn grid_cell(at: Coord) -> (i64, i64) {
    (grid_index(at.x), grid_index(at.y))
}

/// The index of the cell that holds `value`, in pt. Anchors are document positions, at most
/// `MAX_COORDINATE` in magnitude, so the index stays far inside `i64`. Negative zero has index 0.
fn grid_index(value: f64) -> i64 {
    (value / COINCIDENT_PT).round() as i64
}

/// The rank of a component: taller first, then larger, then by code, then by the code of its
/// boundary bond, then by its smallest identifier.
fn component_rank(left: &Ranked<'_>, right: &Ranked<'_>) -> Ordering {
    right
        .rooted
        .height()
        .cmp(&left.rooted.height())
        .then(right.rooted.size().cmp(&left.rooted.size()))
        .then_with(|| left.rooted.code().cmp(right.rooted.code()))
        .then(
            left.rooted
                .boundary_order()
                .cmp(right.rooted.boundary_order()),
        )
        .then(left.smallest.cmp(&right.smallest))
}

/// The pairs of atoms that overlap in the placed layout. A pair has at least one layout atom, is
/// not bonded, and lies closer than [`OVERLAP_FACTOR`] bond lengths. A layout atom takes its placed
/// position, and any other atom its before position. Each pair is visited once, so the count is the
/// number of pairs. The result is that count and the first [`MAX_LISTED_OVERLAPS`] pairs in
/// ascending order, each with the smaller identifier first.
///
/// # Errors
///
/// `Halt::Interrupted` when `interrupt` stops the scan.
fn overlap_pairs(
    prep: &Preparation,
    placed: &BTreeMap<AtomId, Coord>,
    interrupt: &dyn Interrupt,
) -> Result<(usize, Vec<(AtomId, AtomId)>), Halt> {
    let limit = OVERLAP_FACTOR * prep.bond_length_pt;
    let mut count: usize = 0;
    // The smallest pairs found so far, at most MAX_LISTED_OVERLAPS of them. Pairs do not arrive in
    // ascending order; dropping the largest one past the limit keeps the smallest pairs, whichever
    // order they arrive in.
    let mut listed = BTreeSet::new();
    for &atom in &prep.layout_atoms {
        interrupt.check()?;
        let here = placed_at(placed, atom);
        for (&other, &point) in &prep.positions {
            if other == atom {
                continue;
            }
            let there = if prep.layout_atoms.contains(&other) {
                if other < atom {
                    // The row of `other` lists this pair, so it is not repeated here.
                    continue;
                }
                placed_at(placed, other)
            } else {
                Coord::from(point)
            };
            if here.distance(there) < limit && !bonded(prep, atom, other) {
                count += 1;
                listed.insert((atom.min(other), atom.max(other)));
                if listed.len() > MAX_LISTED_OVERLAPS {
                    listed.pop_last();
                }
            }
        }
    }
    Ok((count, listed.into_iter().collect()))
}

/// Whether a bond joins `first` and `second`.
fn bonded(prep: &Preparation, first: AtomId, second: AtomId) -> bool {
    neighbours(&prep.adjacency, first)
        .iter()
        .any(|&(neighbour, _)| neighbour == second)
}

/// The atoms a selection names: its atoms and the endpoints of its bonds.
///
/// # Errors
///
/// [`LayoutError::UnknownSelection`] for an atom or bond the document does not have, and
/// [`LayoutError::EmptySelection`] when the selection names neither.
fn selected_atoms(
    selection: &Selection,
    atoms: &BTreeMap<AtomId, &Atom>,
    bonds: &BTreeMap<BondId, &Bond>,
) -> Result<BTreeSet<AtomId>, LayoutError> {
    if selection.atoms.iter().any(|atom| !atoms.contains_key(atom)) {
        return Err(LayoutError::UnknownSelection);
    }
    let mut seeds = selection.atoms.clone();
    for id in &selection.bonds {
        let bond = bonds.get(id).ok_or(LayoutError::UnknownSelection)?;
        seeds.extend([bond.begin, bond.end]);
    }
    if seeds.is_empty() {
        return Err(LayoutError::EmptySelection);
    }
    Ok(seeds)
}

/// The neighbours of every atom, ascending by neighbour then bond.
fn adjacency_of(atoms: &BTreeMap<AtomId, &Atom>, bonds: &BTreeMap<BondId, &Bond>) -> Adjacency {
    let mut adjacency: Adjacency = atoms.keys().map(|atom| (*atom, Vec::new())).collect();
    for (id, bond) in bonds {
        adjacency
            .entry(bond.begin)
            .or_default()
            .push((bond.end, *id));
        adjacency
            .entry(bond.end)
            .or_default()
            .push((bond.begin, *id));
    }
    for list in adjacency.values_mut() {
        list.sort_unstable();
    }
    adjacency
}

/// The neighbours of `atom` with the bonds that join them, ascending by neighbour then bond.
fn neighbours(adjacency: &Adjacency, atom: AtomId) -> &[(AtomId, BondId)] {
    adjacency.get(&atom).map_or(&[][..], Vec::as_slice)
}

/// The atoms reachable from `start` through atoms that `within` accepts, with the bonds that join
/// them. `start` is always reached, whatever `within` says.
fn explore(
    start: AtomId,
    within: impl Fn(AtomId) -> bool,
    adjacency: &Adjacency,
) -> (BTreeSet<AtomId>, BTreeSet<BondId>) {
    let mut atoms = BTreeSet::new();
    let mut bonds = BTreeSet::new();
    let mut pending = vec![start];
    while let Some(atom) = pending.pop() {
        if !atoms.insert(atom) {
            continue;
        }
        for &(neighbour, bond) in neighbours(adjacency, atom) {
            if within(neighbour) {
                bonds.insert(bond);
                if !atoms.contains(&neighbour) {
                    pending.push(neighbour);
                }
            }
        }
    }
    (atoms, bonds)
}

/// The atoms of every graph-connected component that holds one of `seeds`.
fn connected_to(seeds: &BTreeSet<AtomId>, adjacency: &Adjacency) -> BTreeSet<AtomId> {
    let mut reached = BTreeSet::new();
    for seed in seeds {
        if !reached.contains(seed) {
            reached.extend(explore(*seed, |_| true, adjacency).0);
        }
    }
    reached
}

/// The connected components of the bonds inside the layout set, each as its atoms and its
/// internal bonds, in order of their smallest atom.
fn layout_components(
    layout_atoms: &BTreeSet<AtomId>,
    adjacency: &Adjacency,
) -> Vec<(BTreeSet<AtomId>, BTreeSet<BondId>)> {
    let mut assigned = BTreeSet::new();
    let mut components = Vec::new();
    for &start in layout_atoms {
        if assigned.contains(&start) {
            continue;
        }
        let (atoms, bonds) = explore(start, |atom| layout_atoms.contains(&atom), adjacency);
        assigned.extend(atoms.iter().copied());
        components.push((atoms, bonds));
    }
    components
}

/// The bonds between the layout set and the atoms outside it, ascending by layout atom, then by
/// fixed atom, then by bond.
fn boundary_bonds(
    bonds: &BTreeMap<BondId, &Bond>,
    layout_atoms: &BTreeSet<AtomId>,
) -> Vec<Boundary> {
    let mut boundary = Vec::new();
    for (&bond, record) in bonds {
        let begin_laid_out = layout_atoms.contains(&record.begin);
        let end_laid_out = layout_atoms.contains(&record.end);
        let (atom, fixed_atom) = match (begin_laid_out, end_laid_out) {
            (true, false) => (record.begin, record.end),
            (false, true) => (record.end, record.begin),
            _ => continue,
        };
        boundary.push(Boundary {
            atom,
            fixed_atom,
            bond,
        });
    }
    boundary.sort_unstable_by_key(|entry| (entry.atom, entry.fixed_atom, entry.bond));
    boundary
}

/// Refuses a layout atom that lies in a ring system, and returns how many ring systems hold no
/// layout atom. Only the graph-connected components that hold a layout atom are searched, so the
/// count covers those components alone. A ring system lies inside one component, so each ring
/// that holds a layout atom is found, and a ring system in any other component is neither refused
/// nor counted.
///
/// # Errors
///
/// [`LayoutError::RingInLayoutSet`] naming every layout atom in a ring system.
fn ring_check(
    atoms: &BTreeMap<AtomId, &Atom>,
    bonds: &BTreeMap<BondId, &Bond>,
    adjacency: &Adjacency,
    layout_atoms: &BTreeSet<AtomId>,
) -> Result<usize, LayoutError> {
    let mut systems = Vec::new();
    let mut searched = BTreeSet::new();
    for &start in layout_atoms {
        if searched.contains(&start) {
            continue;
        }
        let (component, component_bonds) = explore(start, |_| true, adjacency);
        searched.extend(component.iter().copied());
        // Every member is reached through the document's bonds, so each lookup holds.
        let graph = MoleculeGraph::from_parts(
            component
                .iter()
                .map(|atom| (*atom, atoms[atom].clone()))
                .collect(),
            component_bonds
                .iter()
                .map(|bond| (*bond, bonds[bond].clone()))
                .collect(),
        );
        systems.extend(rings::perceive(&graph));
    }
    let ring_atoms: BTreeSet<AtomId> = systems
        .iter()
        .flat_map(|system| system.atoms.iter().copied())
        .collect();
    let in_rings: Vec<AtomId> = layout_atoms.intersection(&ring_atoms).copied().collect();
    if !in_rings.is_empty() {
        return Err(LayoutError::RingInLayoutSet { atoms: in_rings });
    }
    // No system holds a layout atom, because a system that did was refused above.
    Ok(systems.len())
}

/// Refuses stated stereo and bond marks that touch the layout set or a boundary fixed atom.
///
/// A stated double bond whose bond the document does not have counts as touching, which is the
/// conservative reading.
///
/// # Errors
///
/// [`LayoutError::StereoInLayout`] listing the touching centers and bonds.
fn stereo_check(
    document: &Document,
    bonds: &BTreeMap<BondId, &Bond>,
    layout_atoms: &BTreeSet<AtomId>,
    boundary_fixed: &BTreeSet<AtomId>,
) -> Result<(), LayoutError> {
    let touched = |atom: AtomId| layout_atoms.contains(&atom) || boundary_fixed.contains(&atom);
    let mut stated_atoms = BTreeSet::new();
    let mut stereo_bonds = BTreeSet::new();
    for (key, _) in document.stereo().iter() {
        match key {
            StereoKey::Center(atom) => {
                if touched(atom) {
                    stated_atoms.insert(atom);
                }
            }
            StereoKey::DoubleBond(bond) => {
                let touching = bonds
                    .get(&bond)
                    .is_none_or(|record| touched(record.begin) || touched(record.end));
                if touching {
                    stereo_bonds.insert(bond);
                }
            }
        }
    }
    for (id, bond) in bonds {
        if bond.mark.is_some() && (touched(bond.begin) || touched(bond.end)) {
            stereo_bonds.insert(*id);
        }
    }
    if stated_atoms.is_empty() && stereo_bonds.is_empty() {
        return Ok(());
    }
    Err(LayoutError::StereoInLayout {
        atoms: stated_atoms.into_iter().collect(),
        bonds: stereo_bonds.into_iter().collect(),
    })
}

/// Refuses a layout atom that an abbreviation or fragment display covers in this instance. An
/// abbreviation covers its key and its hidden terminal. A fragment display covers its key and every
/// member. A display in another instance covers nothing here.
///
/// # Errors
///
/// [`LayoutError::DisplayInLayoutSet`] listing the covered layout atoms, in ascending order.
fn display_check(
    document: &Document,
    instance: ObjectId,
    layout_atoms: &BTreeSet<AtomId>,
) -> Result<(), LayoutError> {
    let mut covered: BTreeSet<AtomId> = BTreeSet::new();
    for (key, terminal) in document.abbreviations() {
        if key.instance == instance {
            covered.extend([key.atom, terminal]);
        }
    }
    for (key, display) in document.fragment_displays() {
        if key.instance == instance {
            covered.insert(key.atom);
            covered.extend(display.members.iter().copied());
        }
    }
    let atoms: Vec<AtomId> = layout_atoms.intersection(&covered).copied().collect();
    if atoms.is_empty() {
        Ok(())
    } else {
        Err(LayoutError::DisplayInLayoutSet { atoms })
    }
}

/// The anchor of each component: its one boundary bond, or `None` when it has none.
///
/// # Errors
///
/// [`LayoutError::MultipleAttachments`] for the first component with two or more boundary bonds.
fn anchors_of(
    components: &[Component],
    boundary: &[Boundary],
) -> Result<Vec<Option<Anchor>>, LayoutError> {
    components
        .iter()
        .map(|component| {
            let attached: Vec<&Boundary> = boundary
                .iter()
                .filter(|entry| component.atoms.binary_search(&entry.atom).is_ok())
                .collect();
            match attached.as_slice() {
                [] => Ok(None),
                [only] => Ok(Some(Anchor {
                    fixed_atom: only.fixed_atom,
                    bond: only.bond,
                })),
                _ => {
                    let mut bonds: Vec<BondId> = attached.iter().map(|entry| entry.bond).collect();
                    bonds.sort_unstable();
                    Err(LayoutError::MultipleAttachments {
                        atoms: component.atoms.clone(),
                        bonds,
                    })
                }
            }
        })
        .collect()
}

/// The canonical label of an atom, which starts the canonical code of its subtree. It has six
/// fields joined by `|`: the kind, the charge, the isotope, the hydrogens, the radical and the
/// attachment points. Together they state every atom state the document records, so two atoms
/// with one label differ in no recorded way. No field holds `|`, `(`, `)` or `;`, so the fields
/// can be read back.
fn atom_label(atom: &Atom) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}",
        kind_label(&atom.kind),
        atom.charge,
        // Validation keeps an isotope at 1 or more, so 0 can mean natural abundance.
        atom.isotope.unwrap_or(0),
        hydrogens_label(atom.hydrogens),
        radical_label(atom.radical),
        attachments_label(atom.attachment_points),
    )
}

/// The kind of an atom: its element symbol, or a query named after `?`. A list names its elements
/// after `?in=`, or `?notin=` when negated, comma-separated and in the order the list records them.
fn kind_label(kind: &AtomKind) -> String {
    match kind {
        AtomKind::Element(element) => element.symbol().to_owned(),
        AtomKind::Query(QueryAtom::Any) => "?any".to_owned(),
        AtomKind::Query(QueryAtom::AnyHeavy) => "?anyheavy".to_owned(),
        AtomKind::Query(QueryAtom::Heteroatom) => "?heteroatom".to_owned(),
        AtomKind::Query(QueryAtom::Halogen) => "?halogen".to_owned(),
        AtomKind::Query(QueryAtom::List { elements, negated }) => {
            let symbols: Vec<&str> = elements.iter().map(|element| element.symbol()).collect();
            let keyword = if *negated { "?notin=" } else { "?in=" };
            format!("{keyword}{}", symbols.join(","))
        }
    }
}

/// The recorded hydrogens: `inferred`, `unknown`, or the stated count.
fn hydrogens_label(hydrogens: Hydrogens) -> String {
    match hydrogens {
        Hydrogens::Inferred => "inferred".to_owned(),
        Hydrogens::Unknown => "unknown".to_owned(),
        Hydrogens::Specified(count) => count.to_string(),
    }
}

/// The recorded radical.
fn radical_label(radical: Radical) -> &'static str {
    match radical {
        Radical::None => "none",
        Radical::Singlet => "singlet",
        Radical::Doublet => "doublet",
        Radical::Triplet => "triplet",
        Radical::Unknown => "unknown",
    }
}

/// The attachment points in ascending order, comma-separated. Empty when there are none.
fn attachments_label(points: AttachmentPoints) -> String {
    let values: Vec<String> = points.values().map(|value| value.to_string()).collect();
    values.join(",")
}

/// The straight-line distance between two points, in pt.
fn distance(a: Point, b: Point) -> f64 {
    (a.x() - b.x()).hypot(a.y() - b.y())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::budget::CancelFlag;
    use crate::document::{
        AtomRef, BondRef, BondTo, Command, DocumentId, NewAtomPositions, RingPattern, RingSize,
    };
    use crate::element::Element;
    use crate::settings::Settings;
    use crate::stereo::{AtomConfiguration, BondEnd, SlotRef, StereoMark};

    fn atom(serial: u32) -> AtomId {
        AtomId::from_serial(serial).unwrap()
    }

    fn bond(serial: u32) -> BondId {
        BondId::from_serial(serial).unwrap()
    }

    fn instance() -> ObjectId {
        ObjectId::from_serial(1).unwrap()
    }

    fn point(x: f64, y: f64) -> Point {
        Point::new(x, y).unwrap()
    }

    fn carbon() -> Atom {
        Atom::from(Element::CARBON)
    }

    fn element(symbol: &str) -> Element {
        Element::from_symbol(symbol).unwrap()
    }

    fn selection(serials: &[u32]) -> Selection {
        Selection {
            atoms: serials.iter().map(|serial| atom(*serial)).collect(),
            ..Selection::default()
        }
    }

    /// A carbon at (x, y), bonded to the atom added by command `bonded_to` of the same transaction.
    fn add_carbon(x: f64, y: f64, bonded_to: Option<usize>) -> Command {
        Command::AddAtom {
            atom: carbon(),
            positions: NewAtomPositions::Single(point(x, y)),
            bond_to: bonded_to.map(|index| BondTo {
                atom: AtomRef::Created(index),
                order: BondOrder::Single,
            }),
        }
    }

    /// Carbons 1..=count in a row 30 pt apart; bond k joins atoms k and k+1.
    fn row(count: usize) -> Document {
        let mut document = Document::new(DocumentId::parse("row").unwrap(), "Row").unwrap();
        let commands: Vec<Command> = (0..count)
            .map(|index| add_carbon(index as f64 * 30.0, 0.0, index.checked_sub(1)))
            .collect();
        document.apply(&commands).unwrap();
        document
    }

    /// The row of three carbons (bonds 1 and 2), then carbons 4 and 5 bonded to each other
    /// (bond 3) and apart from the row.
    fn row_and_pair() -> Document {
        let mut document = row(3);
        document
            .apply(&[
                add_carbon(0.0, 100.0, None),
                add_carbon(30.0, 100.0, Some(0)),
            ])
            .unwrap();
        document
    }

    /// A regular carbon hexagon (atoms 1 to 6) and one carbon apart from it (atom 7).
    fn ring_and_free_carbon() -> Document {
        let mut document = Document::new(DocumentId::parse("ring").unwrap(), "Ring").unwrap();
        document
            .apply(&[
                Command::InsertRing {
                    instance: instance(),
                    center: point(0.0, 0.0),
                    size: RingSize::Six,
                    pattern: RingPattern::Single,
                },
                add_carbon(300.0, 300.0, None),
            ])
            .unwrap();
        document
    }

    /// An oxygen (atom 1) and an ethyl group (atoms 2 and 3) collapsed to one display keyed at
    /// atom 2.
    fn ethyl() -> Document {
        let mut document = Document::new(DocumentId::parse("ethyl").unwrap(), "Et").unwrap();
        document
            .apply(&[
                Command::AddAtom {
                    atom: Atom::from(Element::from_symbol("O").unwrap()),
                    positions: NewAtomPositions::Single(point(0.0, 0.0)),
                    bond_to: None,
                },
                add_carbon(30.0, 0.0, Some(0)),
                add_carbon(45.0, 25.0, Some(1)),
            ])
            .unwrap();
        document
            .apply(&[Command::CollapseEthyl {
                instance: instance(),
                anchor: atom(2),
                terminal: atom(3),
            }])
            .unwrap();
        document
    }

    /// Carbon 1 and carbon 2 at one place and bonded, then carbon 3 bonded to atom 2 at 60 pt.
    fn coincident_neighbour() -> Document {
        let mut document = Document::new(DocumentId::parse("coincident").unwrap(), "Same").unwrap();
        document
            .apply(&[
                add_carbon(0.0, 0.0, None),
                add_carbon(0.0, 0.0, Some(0)),
                add_carbon(60.0, 0.0, Some(1)),
            ])
            .unwrap();
        document
    }

    #[test]
    fn an_empty_selection_is_refused() {
        let error = prepare(
            &row(3),
            instance(),
            &Selection::default(),
            Scope::Components,
        )
        .unwrap_err();
        assert_eq!(error, LayoutError::EmptySelection);
    }

    #[test]
    fn scene_objects_are_refused_before_any_other_check() {
        let selection = Selection {
            objects: [instance()].into(),
            atoms: [atom(9)].into(),
            ..Selection::default()
        };
        let error = prepare(&row(3), instance(), &selection, Scope::Selection).unwrap_err();
        assert_eq!(error, LayoutError::ObjectSelection);
    }

    #[test]
    fn unknown_atoms_and_bonds_are_refused() {
        let document = row(3);
        assert_eq!(
            prepare(&document, instance(), &selection(&[9]), Scope::Selection).unwrap_err(),
            LayoutError::UnknownSelection
        );
        let unknown_bond = Selection {
            bonds: [bond(9)].into(),
            ..Selection::default()
        };
        assert_eq!(
            prepare(&document, instance(), &unknown_bond, Scope::Selection).unwrap_err(),
            LayoutError::UnknownSelection
        );
    }

    #[test]
    fn the_components_scope_takes_each_connected_component_of_a_seed_whole() {
        let prepared = prepare(
            &row_and_pair(),
            instance(),
            &selection(&[2]),
            Scope::Components,
        )
        .unwrap();
        assert_eq!(
            prepared.layout_atoms,
            BTreeSet::from([atom(1), atom(2), atom(3)])
        );
        assert_eq!(prepared.layout_bonds, BTreeSet::from([bond(1), bond(2)]));
        assert_eq!(prepared.fixed_atom_count, 2);
        assert!(prepared.boundary.is_empty());
        assert_eq!(
            prepared.components,
            vec![Component {
                atoms: vec![atom(1), atom(2), atom(3)],
                bonds: vec![bond(1), bond(2)],
                anchor: None,
            }]
        );
    }

    #[test]
    fn a_selected_chain_is_anchored_by_its_one_boundary_bond() {
        let prepared = prepare(&row(4), instance(), &selection(&[3, 4]), Scope::Selection).unwrap();
        assert_eq!(
            prepared.boundary,
            vec![Boundary {
                atom: atom(3),
                fixed_atom: atom(2),
                bond: bond(2),
            }]
        );
        assert_eq!(
            prepared.components,
            vec![Component {
                atoms: vec![atom(3), atom(4)],
                bonds: vec![bond(3)],
                anchor: Some(Anchor {
                    fixed_atom: atom(2),
                    bond: bond(2),
                }),
            }]
        );
        assert_eq!(prepared.fixed_atom_count, 2);
        assert_eq!(prepared.layout_bonds, BTreeSet::from([bond(3)]));
    }

    #[test]
    fn a_selected_bond_seeds_its_two_atoms() {
        let selection = Selection {
            bonds: [bond(1)].into(),
            ..Selection::default()
        };
        let prepared = prepare(&row(3), instance(), &selection, Scope::Selection).unwrap();
        assert_eq!(prepared.layout_atoms, BTreeSet::from([atom(1), atom(2)]));
        assert_eq!(prepared.layout_bonds, BTreeSet::from([bond(1)]));
        assert_eq!(
            prepared.components,
            vec![Component {
                atoms: vec![atom(1), atom(2)],
                bonds: vec![bond(1)],
                anchor: Some(Anchor {
                    fixed_atom: atom(3),
                    bond: bond(2),
                }),
            }]
        );
    }

    #[test]
    fn a_lone_atom_is_a_free_component_without_bonds() {
        let prepared = prepare(&row(1), instance(), &selection(&[1]), Scope::Selection).unwrap();
        assert_eq!(
            prepared.components,
            vec![Component {
                atoms: vec![atom(1)],
                bonds: Vec::new(),
                anchor: None,
            }]
        );
        assert!(prepared.layout_bonds.is_empty());
        assert_eq!(prepared.fixed_atom_count, 0);
    }

    #[test]
    fn a_component_bonded_to_fixed_atoms_twice_is_refused() {
        let error = prepare(&row(3), instance(), &selection(&[2]), Scope::Selection).unwrap_err();
        assert_eq!(
            error,
            LayoutError::MultipleAttachments {
                atoms: vec![atom(2)],
                bonds: vec![bond(1), bond(2)],
            }
        );
    }

    #[test]
    fn ring_atoms_in_the_layout_set_are_refused_while_a_ring_elsewhere_is_not_counted() {
        let document = ring_and_free_carbon();
        assert_eq!(
            prepare(&document, instance(), &selection(&[1]), Scope::Selection).unwrap_err(),
            LayoutError::RingInLayoutSet {
                atoms: vec![atom(1)],
            }
        );
        assert_eq!(
            prepare(
                &document,
                instance(),
                &selection(&[1, 2, 7]),
                Scope::Selection
            )
            .unwrap_err(),
            LayoutError::RingInLayoutSet {
                atoms: vec![atom(1), atom(2)],
            }
        );
        let prepared = prepare(&document, instance(), &selection(&[7]), Scope::Selection).unwrap();
        assert_eq!(prepared.layout_atoms, BTreeSet::from([atom(7)]));
        // The hexagon lies in a component with no layout atom, so it is not counted.
        assert_eq!(prepared.fixed_ring_systems, 0);
        assert_eq!(prepared.fixed_atom_count, 6);
    }

    #[test]
    fn stated_stereo_on_the_layout_set_or_its_boundary_is_refused() {
        let mut document = row(4);
        document
            .apply(&[Command::SetBondMark {
                bond: BondRef::Id(bond(1)),
                mark: Some(StereoMark::Wedge {
                    narrow: BondEnd::Begin,
                }),
            }])
            .unwrap();
        let expected = LayoutError::StereoInLayout {
            atoms: vec![],
            bonds: vec![bond(1)],
        };
        // Atoms 3 and 4 hang from atom 2, a boundary fixed atom, and bond 1 joins atoms 1 and 2.
        assert_eq!(
            prepare(&document, instance(), &selection(&[3, 4]), Scope::Selection).unwrap_err(),
            expected.clone()
        );
        // Atom 1 is in the layout set and bond 1 has an end in it.
        assert_eq!(
            prepare(&document, instance(), &selection(&[1]), Scope::Selection).unwrap_err(),
            expected
        );
        // Atom 4 is anchored at atom 3, and bond 1 does not reach atoms 3 or 4.
        assert!(prepare(&document, instance(), &selection(&[4]), Scope::Selection).is_ok());
    }

    #[test]
    fn a_stated_center_in_or_next_to_the_layout_set_is_refused() {
        // Carbon 1 is bonded to carbons 2 to 5, and its stated configuration names all four.
        let mut document = Document::new(DocumentId::parse("center").unwrap(), "Center").unwrap();
        document
            .apply(&[
                add_carbon(0.0, 0.0, None),
                add_carbon(30.0, 0.0, Some(0)),
                add_carbon(-30.0, 0.0, Some(0)),
                add_carbon(0.0, 30.0, Some(0)),
                add_carbon(0.0, -30.0, Some(0)),
            ])
            .unwrap();
        let slots = [2, 3, 4, 5].map(|serial| SlotRef::Atom(AtomRef::Id(atom(serial))));
        document
            .apply(&[Command::SetAtomConfiguration {
                atom: AtomRef::Id(atom(1)),
                configuration: Some(AtomConfiguration::Unknown { neighbors: slots }),
            }])
            .unwrap();
        let expected = LayoutError::StereoInLayout {
            atoms: vec![atom(1)],
            bonds: Vec::new(),
        };
        // Atom 1 is in the layout set.
        assert_eq!(
            prepare(&document, instance(), &selection(&[1]), Scope::Selection).unwrap_err(),
            expected.clone()
        );
        // Atom 1 is the boundary fixed atom of the layout atom 2.
        assert_eq!(
            prepare(&document, instance(), &selection(&[2]), Scope::Selection).unwrap_err(),
            expected
        );
    }

    #[test]
    fn an_abbreviation_covering_a_layout_atom_is_refused_and_one_covering_fixed_atoms_is_not() {
        let document = ethyl();
        // Atom 2 is the key and atom 3 the hidden terminal, so both are covered.
        assert_eq!(
            prepare(&document, instance(), &selection(&[2, 3]), Scope::Selection).unwrap_err(),
            LayoutError::DisplayInLayoutSet {
                atoms: vec![atom(2), atom(3)],
            }
        );
        // The hidden terminal alone is covered too.
        assert_eq!(
            prepare(&document, instance(), &selection(&[3]), Scope::Selection).unwrap_err(),
            LayoutError::DisplayInLayoutSet {
                atoms: vec![atom(3)],
            }
        );
        // The display covers atoms 2 and 3 only, so the oxygen is laid out.
        assert!(prepare(&document, instance(), &selection(&[1]), Scope::Selection).is_ok());
    }

    #[test]
    fn a_fixed_neighbour_of_an_anchor_at_its_place_is_refused() {
        assert_eq!(
            prepare(
                &coincident_neighbour(),
                instance(),
                &selection(&[3]),
                Scope::Selection
            )
            .unwrap_err(),
            LayoutError::CoincidentFixedAtoms {
                atom: atom(2),
                other: atom(1),
            }
        );
    }

    #[test]
    fn the_layout_set_is_limited_to_500_atoms() {
        let mut document = Document::new(DocumentId::parse("many").unwrap(), "Many").unwrap();
        let commands: Vec<Command> = (0..=MAX_LAYOUT_ATOMS)
            .map(|index| Command::AddAtom {
                atom: carbon(),
                positions: NewAtomPositions::Single(point(index as f64, 0.0)),
                bond_to: None,
            })
            .collect();
        for chunk in commands.chunks(250) {
            document.apply(chunk).unwrap();
        }
        let ids: Vec<AtomId> = document.atoms().map(|(id, _)| id).collect();
        let at_limit = Selection {
            atoms: ids[..MAX_LAYOUT_ATOMS].iter().copied().collect(),
            ..Selection::default()
        };
        let prepared = prepare(&document, instance(), &at_limit, Scope::Selection).unwrap();
        assert_eq!(prepared.layout_atoms.len(), MAX_LAYOUT_ATOMS);
        assert_eq!(prepared.components.len(), MAX_LAYOUT_ATOMS);
        let over_limit = Selection {
            atoms: ids.iter().copied().collect(),
            ..Selection::default()
        };
        assert_eq!(
            prepare(&document, instance(), &over_limit, Scope::Selection).unwrap_err(),
            LayoutError::TooLarge {
                atoms: MAX_LAYOUT_ATOMS + 1,
                limit: MAX_LAYOUT_ATOMS,
            }
        );
    }

    #[test]
    fn the_bond_length_is_the_documents_own_setting() {
        let settings = Settings {
            bond_length_pt: 42.0,
            ..Settings::default()
        };
        let mut document =
            Document::new_with_settings(DocumentId::parse("wide").unwrap(), "Wide", settings)
                .unwrap();
        document.apply(&[add_carbon(0.0, 0.0, None)]).unwrap();
        let prepared = prepare(&document, instance(), &selection(&[1]), Scope::Selection).unwrap();
        assert_eq!(prepared.bond_length_pt, 42.0);
    }

    #[test]
    fn an_instance_without_positions_refuses_its_first_layout_atom() {
        let missing = ObjectId::from_serial(9).unwrap();
        assert_eq!(
            prepare(&row(2), missing, &selection(&[1]), Scope::Selection).unwrap_err(),
            LayoutError::MissingPosition { atom: atom(1) }
        );
    }

    #[test]
    fn atom_labels_state_the_kind_charge_isotope_hydrogens_radical_and_attachment_points() {
        assert_eq!(atom_label(&carbon()), "C|0|0|inferred|none|");
        assert_eq!(
            atom_label(&Atom {
                charge: -1,
                isotope: Some(13),
                ..carbon()
            }),
            "C|-1|13|inferred|none|"
        );
        assert_eq!(
            atom_label(&Atom {
                hydrogens: Hydrogens::Specified(2),
                radical: Radical::Doublet,
                attachment_points: AttachmentPoints::new(&[1, 3]).unwrap(),
                ..carbon()
            }),
            "C|0|0|2|doublet|1,3"
        );
        assert_eq!(
            atom_label(&Atom::new(AtomKind::Query(QueryAtom::Any))),
            "?any|0|0|inferred|none|"
        );
        assert_eq!(
            atom_label(&Atom::new(AtomKind::Query(QueryAtom::List {
                elements: vec![element("N"), element("O")],
                negated: true,
            }))),
            "?notin=N,O|0|0|inferred|none|"
        );
    }

    #[test]
    fn atom_labels_tell_apart_every_recorded_state_and_hold_six_delimited_fields() {
        let list = |elements: Vec<Element>, negated| {
            Atom::new(AtomKind::Query(QueryAtom::List { elements, negated }))
        };
        let (nitrogen, oxygen) = (element("N"), element("O"));
        let states = [
            carbon(),
            Atom {
                charge: 1,
                ..carbon()
            },
            Atom {
                isotope: Some(1),
                ..carbon()
            },
            Atom {
                hydrogens: Hydrogens::Unknown,
                ..carbon()
            },
            Atom {
                hydrogens: Hydrogens::Specified(0),
                ..carbon()
            },
            Atom {
                hydrogens: Hydrogens::Specified(1),
                ..carbon()
            },
            Atom {
                radical: Radical::Singlet,
                ..carbon()
            },
            Atom {
                radical: Radical::Doublet,
                ..carbon()
            },
            Atom {
                radical: Radical::Triplet,
                ..carbon()
            },
            Atom {
                radical: Radical::Unknown,
                ..carbon()
            },
            Atom {
                attachment_points: AttachmentPoints::new(&[1]).unwrap(),
                ..carbon()
            },
            Atom {
                attachment_points: AttachmentPoints::new(&[2]).unwrap(),
                ..carbon()
            },
            Atom {
                attachment_points: AttachmentPoints::new(&[1, 2]).unwrap(),
                ..carbon()
            },
            Atom::from(nitrogen),
            Atom::new(AtomKind::Query(QueryAtom::Any)),
            Atom::new(AtomKind::Query(QueryAtom::AnyHeavy)),
            Atom::new(AtomKind::Query(QueryAtom::Heteroatom)),
            Atom::new(AtomKind::Query(QueryAtom::Halogen)),
            list(vec![nitrogen], false),
            list(vec![nitrogen], true),
            list(vec![nitrogen, oxygen], false),
            list(vec![oxygen, nitrogen], false),
        ];
        let labels: BTreeSet<String> = states.iter().map(atom_label).collect();
        assert_eq!(labels.len(), states.len(), "labels: {labels:?}");
        for label in &labels {
            assert_eq!(label.split('|').count(), 6, "{label}");
            assert!(!label.contains(['(', ')', ';']), "{label}");
        }
    }

    #[test]
    fn each_refusal_has_a_stable_reason_code() {
        let cases = [
            (LayoutError::ObjectSelection, "object_selection"),
            (LayoutError::UnknownSelection, "selection_unknown"),
            (LayoutError::EmptySelection, "layout_selection_empty"),
            (
                LayoutError::TooLarge {
                    atoms: 501,
                    limit: 500,
                },
                "too_large",
            ),
            (
                LayoutError::RingInLayoutSet { atoms: vec![] },
                "ring_in_layout_set",
            ),
            (
                LayoutError::StereoInLayout {
                    atoms: vec![],
                    bonds: vec![],
                },
                "stereo_in_layout",
            ),
            (
                LayoutError::DisplayInLayoutSet { atoms: vec![] },
                "display_in_layout_set",
            ),
            (
                LayoutError::MultipleAttachments {
                    atoms: vec![],
                    bonds: vec![],
                },
                "multiple_attachments",
            ),
            (
                LayoutError::CoincidentFixedAtoms {
                    atom: atom(1),
                    other: atom(2),
                },
                "coincident_fixed_atoms",
            ),
            (
                LayoutError::CoordinateOutOfRange { atom: atom(1) },
                "coordinate_out_of_range",
            ),
            (
                LayoutError::MissingPosition { atom: atom(1) },
                "missing_position",
            ),
        ];
        for (error, code) in cases {
            assert_eq!(error.reason(), code, "{error}");
        }
    }

    /// Carbons placed at `places`, each bonded to the carbon at the given creation index.
    fn carbons(places: &[(f64, f64, Option<usize>)]) -> Document {
        let mut document = Document::new(DocumentId::parse("carbons").unwrap(), "Carbons").unwrap();
        let commands: Vec<Command> = places
            .iter()
            .map(|&(x, y, bonded_to)| add_carbon(x, y, bonded_to))
            .collect();
        document.apply(&commands).unwrap();
        document
    }

    /// Carbon 1 at (100, 100) with carbons 2, 3 and 4 bonded to it: a free star.
    fn star() -> Document {
        carbons(&[
            (100.0, 100.0, None),
            (70.0, 100.0, Some(0)),
            (100.0, 130.0, Some(0)),
            (130.0, 100.0, Some(0)),
        ])
    }

    /// Carbons 1 to 7 as a tree: 1 bonds 2, 3 and 5; 3 bonds 4; 5 bonds 6, and 6 bonds 7.
    fn branched_tree() -> Document {
        carbons(&[
            (100.0, 100.0, None),
            (70.0, 100.0, Some(0)),
            (100.0, 130.0, Some(0)),
            (100.0, 160.0, Some(2)),
            (130.0, 100.0, Some(0)),
            (160.0, 100.0, Some(4)),
            (190.0, 100.0, Some(5)),
        ])
    }

    /// The tree of `branched_tree` at the same places, created in the order 1, 5, 6, 7, 3, 4, 2 of
    /// that tree's numbering, so that its identifiers differ from those of `branched_tree`.
    fn branched_tree_renumbered() -> Document {
        carbons(&[
            (100.0, 100.0, None),
            (130.0, 100.0, Some(0)),
            (160.0, 100.0, Some(1)),
            (190.0, 100.0, Some(2)),
            (100.0, 130.0, Some(0)),
            (100.0, 160.0, Some(4)),
            (70.0, 100.0, Some(0)),
        ])
    }

    /// The placed position of atom `serial` in `preview`.
    fn placement_of(preview: &Preview, serial: u32) -> Placement {
        *preview
            .positions
            .iter()
            .find(|placement| placement.atom == atom(serial))
            .unwrap_or_else(|| panic!("atom {serial} is not in the preview"))
    }

    /// Asserts that `actual` lies within 1e-6 pt of (`x`, `y`).
    fn assert_near(actual: Point, x: f64, y: f64) {
        assert!(
            (actual.x() - x).abs() < 1e-6 && (actual.y() - y).abs() < 1e-6,
            "expected ({x}, {y}), got ({}, {})",
            actual.x(),
            actual.y()
        );
    }

    /// Places atoms `serials` of `document` as one selection, or returns the refusal.
    fn preview_of(document: &Document, serials: &[u32]) -> Result<Preview, LayoutError> {
        let prepared = prepare(document, instance(), &selection(serials), Scope::Selection)?;
        place(&prepared)
    }

    /// An interrupt that never stops placement and counts how often it is asked.
    struct Counting(Cell<usize>);

    impl Interrupt for Counting {
        fn check(&self) -> Result<(), Interrupted> {
            self.0.set(self.0.get() + 1);
            Ok(())
        }
    }

    #[test]
    fn a_free_star_is_rooted_at_its_smallest_end_and_keeps_its_centroid() {
        let preview = preview_of(&star(), &[1, 2, 3, 4]).unwrap();
        let root3 = 3f64.sqrt();
        let component = &preview.components[0];
        assert_eq!(component.root, atom(2));
        assert_eq!(
            component.chains,
            vec![
                Chain {
                    atoms: vec![atom(2), atom(1), atom(3)],
                    branch_from: None,
                },
                Chain {
                    atoms: vec![atom(4)],
                    branch_from: Some(atom(1)),
                },
            ]
        );
        assert_near(placement_of(&preview, 2).after, 100.0 - 15.0 * root3, 122.5);
        assert_near(placement_of(&preview, 1).after, 100.0, 107.5);
        assert_near(placement_of(&preview, 3).after, 100.0 + 15.0 * root3, 122.5);
        assert_near(placement_of(&preview, 4).after, 100.0, 77.5);
        assert!(
            preview.overlaps.is_empty(),
            "overlaps: {:?}",
            preview.overlaps
        );
    }

    #[test]
    fn a_row_chain_hangs_from_its_anchor_on_the_tie_side() {
        // Atom 2 has one placed neighbour, atom 1, at 180 degrees. The candidates at 300 and 60
        // degrees are equally clear of atom 1, and the tie takes 300 degrees.
        let preview = preview_of(&row(4), &[3, 4]).unwrap();
        let root3 = 3f64.sqrt();
        let component = &preview.components[0];
        assert_eq!(component.root, atom(3));
        assert_eq!(
            component.anchor,
            Some(Anchor {
                fixed_atom: atom(2),
                bond: bond(2),
            })
        );
        assert_eq!(
            component.chains,
            vec![Chain {
                atoms: vec![atom(3), atom(4)],
                branch_from: Some(atom(2)),
            }]
        );
        assert_near(placement_of(&preview, 3).after, 45.0, -15.0 * root3);
        assert_near(placement_of(&preview, 4).after, 30.0, -30.0 * root3);
        assert!(
            preview.overlaps.is_empty(),
            "overlaps: {:?}",
            preview.overlaps
        );
    }

    #[test]
    fn a_clearer_side_of_the_anchor_is_taken_over_the_tie_side() {
        let root3 = 3f64.sqrt();
        let mut document = row(4);
        // An unbonded carbon where the tie would put atom 3 makes the other side clearer.
        document
            .apply(&[add_carbon(45.0, -15.0 * root3, None)])
            .unwrap();
        let preview = preview_of(&document, &[3, 4]).unwrap();
        assert_near(placement_of(&preview, 3).after, 45.0, 15.0 * root3);
        assert_near(placement_of(&preview, 4).after, 30.0, 30.0 * root3);
    }

    #[test]
    fn an_anchor_with_no_other_neighbour_hangs_its_root_at_minus_thirty_degrees() {
        let document = carbons(&[(0.0, 0.0, None), (30.0, 0.0, Some(0))]);
        let preview = preview_of(&document, &[2]).unwrap();
        assert_near(placement_of(&preview, 2).after, 15.0 * 3f64.sqrt(), -15.0);
    }

    #[test]
    fn an_anchor_between_two_fixed_neighbours_takes_the_middle_of_the_widest_gap() {
        // Atom 2 is the anchor. Its fixed neighbours lie at 180 degrees (atom 1) and 90 degrees
        // (atom 3) from it. The widest gap runs the long way round, from 180 to 450 degrees, so the
        // root goes to 315 degrees.
        let document = carbons(&[
            (0.0, 0.0, None),
            (30.0, 0.0, Some(0)),
            (30.0, 30.0, Some(1)),
            (60.0, 0.0, Some(1)),
        ]);
        let preview = preview_of(&document, &[4]).unwrap();
        let root2 = 2f64.sqrt();
        assert_near(
            placement_of(&preview, 4).after,
            30.0 + 15.0 * root2,
            -15.0 * root2,
        );
    }

    #[test]
    fn a_free_single_atom_keeps_its_exact_position() {
        let document = carbons(&[(123.5, -7.25, None)]);
        let preview = preview_of(&document, &[1]).unwrap();
        assert_eq!(
            preview.positions,
            vec![Placement {
                atom: atom(1),
                before: point(123.5, -7.25),
                after: point(123.5, -7.25),
            }]
        );
    }

    #[test]
    fn renumbering_the_atoms_leaves_each_placement_unchanged() {
        let serials = [1, 2, 3, 4, 5, 6, 7];
        let original = preview_of(&branched_tree(), &serials).unwrap();
        let renumbered = preview_of(&branched_tree_renumbered(), &serials).unwrap();
        assert_eq!(original.positions.len(), 7);
        for placement in &original.positions {
            let twin = renumbered
                .positions
                .iter()
                .find(|other| other.before == placement.before)
                .unwrap_or_else(|| panic!("no atom at {:?} after renumbering", placement.before));
            assert_near(twin.after, placement.after.x(), placement.after.y());
        }
    }

    #[test]
    fn a_triple_bond_keeps_the_chain_straight() {
        let mut document = Document::new(DocumentId::parse("triple").unwrap(), "Triple").unwrap();
        document
            .apply(&[
                add_carbon(0.0, 0.0, None),
                Command::AddAtom {
                    atom: carbon(),
                    positions: NewAtomPositions::Single(point(30.0, 0.0)),
                    bond_to: Some(BondTo {
                        atom: AtomRef::Created(0),
                        order: BondOrder::Triple,
                    }),
                },
                add_carbon(60.0, 0.0, Some(1)),
            ])
            .unwrap();
        let preview = preview_of(&document, &[1, 2, 3]).unwrap();
        let root3 = 3f64.sqrt();
        assert_eq!(preview.components[0].root, atom(3));
        assert_near(placement_of(&preview, 3).after, 30.0 - 15.0 * root3, 15.0);
        assert_near(placement_of(&preview, 2).after, 30.0, 0.0);
        assert_near(placement_of(&preview, 1).after, 30.0 + 15.0 * root3, -15.0);
        assert!(
            preview.overlaps.is_empty(),
            "overlaps: {:?}",
            preview.overlaps
        );
    }

    #[test]
    fn anchored_components_come_before_free_ones_whatever_their_identifiers() {
        let document = carbons(&[
            (0.0, 300.0, None),
            (0.0, 0.0, None),
            (30.0, 0.0, Some(1)),
            (60.0, 0.0, Some(2)),
        ]);
        let preview = preview_of(&document, &[1, 3, 4]).unwrap();
        assert_eq!(preview.components.len(), 2);
        assert_eq!(preview.components[0].atoms, vec![atom(3), atom(4)]);
        assert_eq!(preview.components[1].atoms, vec![atom(1)]);
    }

    #[test]
    fn a_pair_under_half_a_bond_apart_overlaps_and_a_pair_at_exactly_half_does_not() {
        // Atom 2 lies 10 pt from atom 1 and exactly 15 pt from atom 3, neither of them bonded to it.
        let document = carbons(&[(0.0, 0.0, None), (10.0, 0.0, None), (25.0, 0.0, None)]);
        let preview = preview_of(&document, &[2]).unwrap();
        assert_eq!(preview.overlaps, vec![(atom(1), atom(2))]);
    }

    #[test]
    fn the_lowest_atom_placed_past_the_coordinate_limit_is_refused() {
        // The first pair's free root keeps the centroid x = 99992.5, which puts atom 1 inside the
        // limit at x = 99979.5 and atom 2 past it at x = 100005.5.
        let document = carbons(&[
            (99990.0, 0.0, None),
            (99995.0, 0.0, Some(0)),
            (99990.0, 50.0, None),
            (99995.0, 50.0, Some(2)),
        ]);
        assert_eq!(
            preview_of(&document, &[1, 2, 3, 4]),
            Err(LayoutError::CoordinateOutOfRange { atom: atom(2) })
        );
    }

    #[test]
    fn a_cancelled_placement_stops_with_cancelled() {
        let prepared = prepare(
            &row(3),
            instance(),
            &selection(&[1, 2, 3]),
            Scope::Selection,
        )
        .unwrap();
        let flag = CancelFlag::new();
        flag.cancel();
        assert_eq!(place_with(&prepared, &flag), Err(Interrupted::Cancelled));
    }

    #[test]
    fn the_interrupt_is_asked_once_per_component_placed_atom_and_overlap_row() {
        let prepared = prepare(
            &star(),
            instance(),
            &selection(&[1, 2, 3, 4]),
            Scope::Selection,
        )
        .unwrap();
        let counter = Counting(Cell::new(0));
        let preview = place_with(&prepared, &counter).unwrap().unwrap();
        assert!(preview.overlaps.is_empty());
        assert_eq!(
            counter.0.get(),
            1 + 4 + 4,
            "one check per component, one per placed atom, and one per layout atom of the overlap scan"
        );
    }
}
