//! Reading a stereo configuration off a drawing: which turn a stereocenter has, and which side
//! of a double bond two references are on, as the picture shows them. The functions only return
//! values; nothing is stored, and they know nothing of a document's history or of selection.
//! They read a [`Drawing`]: the graph, the bond marks on it, and the positions of ONE scene
//! instance. A file reader that has no [`crate::document::Document`] yet can build one from
//! its own maps and use the same code.
//!
//! # Frame and sign
//!
//! Document coordinates have y pointing down. The derivation frame is right-handed with x to
//! the right, y up and z toward the viewer, so a neighbor `n` of center `c` becomes
//! `v = (ux, -uy, z)`, where `(ux, uy)` is the unit vector from `c` to `n` in document
//! coordinates (the y sign is negated here and nowhere else) and `z` is `+1` for a wedge whose
//! narrow end is `c`, `-1` for a hash whose narrow end is `c`, and `0` otherwise. With the four
//! slots `s0..s3` in canonical order and `D = det[v1 - v0, v2 - v0, v3 - v0]`, `D < 0` is
//! [`Turn::Anticlockwise`] (OpenSMILES `@`) and `D > 0` is [`Turn::Clockwise`] (`@@`). The
//! public model calls an arrangement positive when `det(p0 - p3, p1 - p3, p2 - p3) > 0`; that
//! determinant is exactly `-D`, so positive is anticlockwise. The public specification states
//! no frame; this one is a Chemifree choice, anchored by the tests (hand-computed examples,
//! the OpenSMILES examples, a seeded property test against the definition, and a public
//! fixture drawing).
//!
//! The lift is a convention only for the size of `D`: scaling every lift by a positive number
//! scales `D` and cannot change its sign. What does matter is the relative weights (wedge `+1`,
//! hash `-1`, plain `0`, a derived hydrogen minus the sum): a picture with two or more lifted
//! bonds whose four vectors do not surround the center is decided by the orientation of the
//! lifted points, which is the public model's definition of parity, and other programs may read
//! such a picture differently. That is a limit, not a guarantee.
//!
//! # Marks that bear on a center
//!
//! A wedge or hash bears on the atom at its narrow end only; at the wide end it says nothing.
//! A wavy bond at a center means unknown for that center. A wavy bond whose two ends can both
//! be centers is read for each end on its own: an end that has its own wedge or hash follows
//! that mark and ignores the wavy bond, an end without one is unknown. A wavy bond whose other
//! end cannot be a center, together with a wedge or hash at the same center, is a conflict.
//! Crossed marks never bear on a center. A wavy single bond beside a double bond (the usual
//! "either E or Z" marker) is NOT read as an unknown double bond: that is a deliberate limit.
//! Only a crossed double bond is.
//!
//! # Determinism and limits
//!
//! Only IEEE-754 basic operations and `sqrt` are used, so a result cannot differ between
//! platforms. The three constants guard degenerate input and do not judge plausibility:
//! [`MIN_BOND_LENGTH`], [`DEGENERACY_LIMIT`] and [`COLLINEAR_SINE`]. A nearly straight pair of
//! plain bonds with one wedge gives `|D| = 4 * epsilon` for a tilt of `epsilon` radians, so
//! a drawing flat to within 0.014 degrees is refused as degenerate and a drawing slightly more
//! tilted than that is answered with confidence. The limit is not tightened: coordinates typed
//! with three decimals on a bond 30 points long deviate by about `3e-5` radians, far below it,
//! and a larger threshold would refuse pictures a user means. A reader that wants to flag weak
//! evidence asks [`Drawing::derive_center_with_strength`] and compares `|D|` with
//! [`WEAK_EVIDENCE`], as the stereo inspection does.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use crate::chemistry::{self, Finding};
use crate::document::{Atom, AtomId, Bond, BondId, BondOrder, Document, ObjectId, Point};
use crate::states::{AtomKind, Hydrogens, Radical};
use crate::stereo::{
    BondEnd, CenterState, DoubleBondState, Graph, Relation, Slot, StereoError, StereoMark, Turn,
};

/// The identifier of the rules in this module, reported with the replies of operations that run
/// them. A change to the frame, the lift, the scope or the fourth-position rule, or to the
/// valence model the fourth-position rule reads, is a new identifier.
pub const MODEL_ID: &str = "chemifree-stereo-1";
/// Two atoms closer than this (in points) are treated as on top of each other.
pub const MIN_BOND_LENGTH: f64 = 1e-3;
/// A center whose `|D|` (dimensionless) is below this is refused as degenerate.
pub const DEGENERACY_LIMIT: f64 = 1e-3;
/// A center whose `|D|` is below this but not below [`DEGENERACY_LIMIT`] is derived, and an
/// inspection reports the evidence as weak. `|D|` of 0.1 is about 1.4 degrees of tilt for a
/// nearly straight pair of plain bonds with one wedge (`|D| = 4 * epsilon`).
pub const WEAK_EVIDENCE: f64 = 0.1;
/// A double-bond reference whose bond makes a sine below this with the double bond is refused as
/// lying on its axis.
pub const COLLINEAR_SINE: f64 = 1e-3;

/// Why a drawing does not determine a configuration although the atom or bond could hold one.
/// A structural impossibility (no stereocenter can be here) is a [`StereoError`] instead.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotDerivable {
    /// No mark bears on the center: a plain drawing states nothing.
    NoMarks,
    /// A wavy bond together with a wedge or hash at the same center.
    ConflictingMarks,
    /// The picture is flat (coplanar or doubled-up bonds), or a double-bond reference lies on
    /// its axis.
    Degenerate,
    /// A bonded atom is on top of its neighbor.
    CoincidentAtoms,
    /// The fourth position of a center with three neighbors cannot be named; see
    /// [`fourth_position`].
    FourthPosition,
    /// A double-bond end has no neighbor to draw a direction from.
    NoReference,
}

impl NotDerivable {
    /// Every reason, in documentation order.
    pub const ALL: [Self; 6] = [
        Self::NoMarks,
        Self::ConflictingMarks,
        Self::Degenerate,
        Self::CoincidentAtoms,
        Self::FourthPosition,
        Self::NoReference,
    ];

    /// The stable machine-readable code.
    pub const fn code(self) -> &'static str {
        match self {
            Self::NoMarks => "stereo_not_derivable_no_marks",
            Self::ConflictingMarks => "stereo_not_derivable_conflicting_marks",
            Self::Degenerate => "stereo_not_derivable_degenerate",
            Self::CoincidentAtoms => "stereo_not_derivable_coincident_atoms",
            Self::FourthPosition => "stereo_not_derivable_fourth_position",
            Self::NoReference => "stereo_not_derivable_no_reference",
        }
    }
}

impl fmt::Display for NotDerivable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NoMarks => "no wedge, hash, or wavy mark bears on this center",
            Self::ConflictingMarks => "a wavy bond and a wedge or hash bear on the same center",
            Self::Degenerate => "the drawing is too flat to determine a configuration",
            Self::CoincidentAtoms => "two bonded atoms are drawn on top of each other",
            Self::FourthPosition => {
                "the fourth position of a center with three neighbors cannot be named from the \
                 atom's hydrogens and valence"
            }
            Self::NoReference => "a double-bond end has no neighbor to read a direction from",
        })
    }
}

/// Why a configuration could not be read off a drawing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeriveError {
    UnknownAtom(AtomId),
    UnknownBond(BondId),
    /// The drawing gives no position for this atom.
    MissingPosition(AtomId),
    /// The model cannot hold a configuration here (a query atom, a wrong neighbor count, a bond
    /// that is not double, a cumulated double bond, an end with too many neighbors).
    Structure(StereoError),
    /// The model can, but the drawing does not determine it.
    NotDerivable(NotDerivable),
}

impl From<StereoError> for DeriveError {
    fn from(error: StereoError) -> Self {
        Self::Structure(error)
    }
}

impl From<NotDerivable> for DeriveError {
    fn from(reason: NotDerivable) -> Self {
        Self::NotDerivable(reason)
    }
}

impl fmt::Display for DeriveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownAtom(atom) => write!(formatter, "atom {atom} does not exist"),
            Self::UnknownBond(bond) => write!(formatter, "bond {bond} does not exist"),
            Self::MissingPosition(atom) => write!(formatter, "atom {atom} has no position"),
            Self::Structure(error) => error.fmt(formatter),
            Self::NotDerivable(reason) => reason.fmt(formatter),
        }
    }
}

impl DeriveError {
    /// The stable code sent as `reason` with the refusal, when the cause has one: the structural
    /// codes of [`StereoError`] and the codes of [`NotDerivable`]. A missing atom, bond or
    /// position has none.
    pub const fn code(&self) -> Option<&'static str> {
        match self {
            Self::Structure(error) => Some(error.code()),
            Self::NotDerivable(reason) => Some(reason.code()),
            Self::UnknownAtom(_) | Self::UnknownBond(_) | Self::MissingPosition(_) => None,
        }
    }
}

impl std::error::Error for DeriveError {}

/// A wedge or hash whose narrow end cannot hold a center record: see
/// [`Drawing::misplaced_marks`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MisplacedMark {
    pub bond: BondId,
    /// The atom at the mark's narrow end, which the mark is about.
    pub narrow: AtomId,
    /// The atom at the wide end.
    pub other: AtomId,
    /// Whether the wide end could hold a center record, so the mark may have been drawn from
    /// the wrong end.
    pub other_could_be_center: bool,
}

/// The graph, its bond marks and the positions of one scene instance, with the neighbor map
/// built once. Deriving one center or double bond afterwards costs only its neighborhood.
pub struct Drawing<'a> {
    graph: Graph<'a>,
    bonds: &'a BTreeMap<BondId, Bond>,
    positions: HashMap<AtomId, Point>,
}

impl<'a> Drawing<'a> {
    /// A drawing of `atoms` and `bonds` (whose marks it reads) at `positions`. The caller
    /// supplies bonds between listed atoms and marks that fit their bond's order
    /// ([`StereoMark::fits`]), as a document guarantees; the maps are otherwise unchecked.
    /// [`Self::derive_center`] reads a mark that does not fit as no mark. Work is linear in the
    /// atoms and bonds, once.
    pub fn new(
        atoms: &'a BTreeMap<AtomId, Atom>,
        bonds: &'a BTreeMap<BondId, Bond>,
        positions: impl IntoIterator<Item = (AtomId, Point)>,
    ) -> Self {
        Self {
            graph: Graph::indexed(atoms, bonds),
            bonds,
            positions: positions.into_iter().collect(),
        }
    }

    /// The drawing of `document` in scene instance `instance`. An unknown instance has no
    /// positions, so every derivation then reports a missing position.
    pub fn of_document(document: &'a Document, instance: ObjectId) -> Self {
        let (atoms, bonds) = document.graph_maps();
        Self::new(atoms, bonds, document.positions(instance))
    }

    /// The drawing of `document` in scene instance `instance`, with every position passed through
    /// `shown` first: the picture as a reader of a file written from the document would see it
    /// (for example after coordinates are rounded to the file's decimals).
    pub fn of_document_shown(
        document: &'a Document,
        instance: ObjectId,
        shown: impl Fn(Point) -> Point,
    ) -> Self {
        let (atoms, bonds) = document.graph_maps();
        Self::new(
            atoms,
            bonds,
            document
                .positions(instance)
                .map(|(atom, point)| (atom, shown(point))),
        )
    }

    fn position(&self, atom: AtomId) -> Result<Point, DeriveError> {
        self.positions
            .get(&atom)
            .copied()
            .ok_or(DeriveError::MissingPosition(atom))
    }

    /// Whether `atom` could hold a center record: a non-query atom with 3 or 4 neighbors.
    fn can_be_center(&self, atom: AtomId) -> bool {
        self.graph.atom(atom).is_some_and(|record| {
            !matches!(record.kind, AtomKind::Query(_))
                && (3..=4).contains(&self.graph.neighbors(atom).len())
        })
    }

    /// The atoms a mark bears on that could hold a center record, in ascending order: the atom
    /// at the narrow end of a wedge or hash, and both ends of a wavy bond, when it is a non-query
    /// atom with 3 or 4 neighbors. An unmarked center, a wide end, and a mark whose narrow end
    /// is a terminal atom are not listed: nothing is stored for them unless asked. A mark the
    /// bond's order cannot carry counts as no mark, as in [`Self::derive_center`]. Work is linear
    /// in the bonds.
    pub fn marked_centers(&self) -> Vec<AtomId> {
        let mut found = BTreeSet::new();
        for bond in self.bonds.values() {
            match bond.mark.filter(|mark| mark.fits(&bond.order)) {
                Some(StereoMark::Wedge { narrow } | StereoMark::Hash { narrow }) => {
                    found.insert(narrow_atom(bond, narrow));
                }
                Some(StereoMark::Wavy) => {
                    found.insert(bond.begin);
                    found.insert(bond.end);
                }
                Some(StereoMark::Crossed) | None => {}
            }
        }
        found
            .into_iter()
            .filter(|&atom| self.can_be_center(atom))
            .collect()
    }

    /// The double bonds that carry a crossed mark, in ascending order. An uncrossed double bond
    /// is not listed: its relation is not stored unless asked.
    pub fn crossed_double_bonds(&self) -> Vec<BondId> {
        self.bonds
            .iter()
            .filter(|(_, bond)| {
                bond.order == BondOrder::Double && bond.mark == Some(StereoMark::Crossed)
            })
            .map(|(&id, _)| id)
            .collect()
    }

    /// The wedges and hashes whose narrow end could not hold a center record (a terminal atom, a
    /// query atom, an atom with five or more neighbors): they bear on nothing, so
    /// [`Self::marked_centers`] leaves them out. Ascending by bond. A mark the bond's order
    /// cannot carry counts as no mark. Work is linear in the bonds.
    pub fn misplaced_marks(&self) -> Vec<MisplacedMark> {
        let mut found = Vec::new();
        for (&id, bond) in self.bonds {
            let Some(StereoMark::Wedge { narrow } | StereoMark::Hash { narrow }) =
                bond.mark.filter(|mark| mark.fits(&bond.order))
            else {
                continue;
            };
            let narrow_atom = narrow_atom(bond, narrow);
            if self.can_be_center(narrow_atom) {
                continue;
            }
            let other = if narrow_atom == bond.begin {
                bond.end
            } else {
                bond.begin
            };
            found.push(MisplacedMark {
                bond: id,
                narrow: narrow_atom,
                other,
                other_could_be_center: self.can_be_center(other),
            });
        }
        found
    }

    /// The wavy bonds that join two atoms which could both hold a center record, ascending. Each
    /// end is read on its own (see the module documentation): an end with its own wedge or hash
    /// follows it, an end without one is unknown. Work is linear in the bonds.
    pub fn wavy_between_centers(&self) -> Vec<BondId> {
        self.bonds
            .iter()
            .filter(|(_, bond)| {
                bond.mark == Some(StereoMark::Wavy)
                    && bond.order == BondOrder::Single
                    && self.can_be_center(bond.begin)
                    && self.can_be_center(bond.end)
            })
            .map(|(&id, _)| id)
            .collect()
    }

    /// Whether any bond of `atom` is aromatic.
    pub fn has_aromatic_bond(&self, atom: AtomId) -> bool {
        self.graph.neighbors(atom).iter().any(|&(_, bond)| {
            self.graph
                .bond(bond)
                .is_some_and(|bond| bond.order == BondOrder::Aromatic)
        })
    }

    /// The ends of double bond `bond` that have two other neighbors drawn on the same side of
    /// the double bond's line, in `begin`, `end` order. [`Self::derive_double_bond`] then follows
    /// the lowest-identifier neighbor, which is a best effort, not a reading of both. An end
    /// whose geometry cannot be read (a neighbor on top of the end or on the axis), a bond that
    /// is not double and an end with other than two neighbors are not listed.
    ///
    /// # Errors
    ///
    /// [`DeriveError::UnknownBond`] and [`DeriveError::MissingPosition`].
    pub fn same_side_ends(&self, bond: BondId) -> Result<Vec<AtomId>, DeriveError> {
        let target = self
            .graph
            .bond(bond)
            .ok_or(DeriveError::UnknownBond(bond))?;
        if target.order != BondOrder::Double {
            return Ok(Vec::new());
        }
        let begin = self.position(target.begin)?;
        let end = self.position(target.end)?;
        let axis = (end.x() - begin.x(), end.y() - begin.y());
        let axis_squared = axis.0 * axis.0 + axis.1 * axis.1;
        let mut found = Vec::new();
        for (atom, partner, from) in [
            (target.begin, target.end, begin),
            (target.end, target.begin, end),
        ] {
            let Ok(others) = self.graph.other_neighbors(atom, partner) else {
                continue;
            };
            let [first, second] = others[..] else {
                continue;
            };
            let sides = [first, second]
                .map(|reference| self.side_of_axis(axis, axis_squared, from, reference));
            match sides {
                [Ok(a), Ok(b)] if a.is_sign_positive() == b.is_sign_positive() => {
                    found.push(atom);
                }
                [Err(error @ DeriveError::MissingPosition(_)), _]
                | [_, Err(error @ DeriveError::MissingPosition(_))] => return Err(error),
                _ => {}
            }
        }
        Ok(found)
    }

    /// The configuration of the stereocenter at `atom` that the drawing shows, in canonical
    /// slot order: [`CenterState::Unknown`] when only a wavy bond bears on it, otherwise
    /// tetrahedral (see the module documentation for the frame).
    ///
    /// A bond mark is read only if the bond's order can carry it ([`StereoMark::fits`]: wedge,
    /// hash and wavy on a single bond). A document guarantees that; a drawing built from
    /// arbitrary maps may not, and a mark that does not fit contributes no lift and counts as
    /// no mark at all (it is neither a wavy bond nor a conflict).
    ///
    /// A wavy bond is checked against the other end with a deliberately structural test: the
    /// other end "can be a center" when it is a non-query atom with three or four neighbors,
    /// whatever its bonds are. An sp2 carbon with three neighbors therefore counts as a possible
    /// center, so a wavy bond to it does not conflict with this end's own wedge or hash: this
    /// end follows its mark and reads the wavy bond as plain. Only a wavy bond to an atom that
    /// could not hold a center record at all (a terminal atom, a query atom, five neighbors) is
    /// a conflict beside a mark of the center's own. No valence or hybridization is consulted.
    ///
    /// # Errors
    ///
    /// [`DeriveError::UnknownAtom`]; [`StereoError::CenterUnsupported`] for a query atom or
    /// fewer than 3 or more than 4 neighbors; [`DeriveError::MissingPosition`]; and the
    /// [`NotDerivable`] reasons `NoMarks`, `ConflictingMarks`, `CoincidentAtoms`,
    /// `FourthPosition` and `Degenerate`, checked in that order. An unknown center reads no
    /// geometry, so it cannot be coincident or degenerate.
    pub fn derive_center(&self, atom: AtomId) -> Result<CenterState, DeriveError> {
        self.derive_center_with_strength(atom)
            .map(|(state, _)| state)
    }

    /// [`Self::derive_center`] and, with a tetrahedral result, the strength of the evidence:
    /// `|D|`, which is at least [`DEGENERACY_LIMIT`] and below [`WEAK_EVIDENCE`] when the
    /// drawing is nearly flat. An unknown (wavy) result reads no geometry and has no strength.
    /// The state, the refusals and their order are exactly those of [`Self::derive_center`].
    ///
    /// # Errors
    ///
    /// As [`Self::derive_center`].
    pub fn derive_center_with_strength(
        &self,
        atom: AtomId,
    ) -> Result<(CenterState, Option<f64>), DeriveError> {
        let record = self
            .graph
            .atom(atom)
            .ok_or(DeriveError::UnknownAtom(atom))?;
        let neighbors = self.graph.neighbors(atom);
        if matches!(record.kind, AtomKind::Query(_)) || !(3..=4).contains(&neighbors.len()) {
            return Err(StereoError::CenterUnsupported.into());
        }

        let mut lifts = Vec::with_capacity(neighbors.len());
        let mut orders = Vec::with_capacity(neighbors.len());
        let mut own_mark = false;
        let mut wavy = false;
        let mut wavy_to_non_center = false;
        for &(neighbor, bond_id) in neighbors.iter() {
            let bond = self
                .graph
                .bond(bond_id)
                .ok_or(DeriveError::UnknownBond(bond_id))?;
            orders.push(chemistry::bond_order_value(&bond.order));
            // A mark the bond's order cannot carry (a document never holds one) is no mark.
            let mark = bond.mark.filter(|mark| mark.fits(&bond.order));
            lifts.push(match mark {
                Some(StereoMark::Wedge { narrow }) if narrow_atom(bond, narrow) == atom => {
                    own_mark = true;
                    1.0
                }
                Some(StereoMark::Hash { narrow }) if narrow_atom(bond, narrow) == atom => {
                    own_mark = true;
                    -1.0
                }
                Some(StereoMark::Wavy) => {
                    wavy = true;
                    wavy_to_non_center |= !self.can_be_center(neighbor);
                    0.0
                }
                _ => 0.0,
            });
        }
        // A wavy bond to another possible center belongs to that center too, so this end
        // follows its own wedge or hash; any other wavy bond beside a mark of its own conflicts.
        if wavy && own_mark && wavy_to_non_center {
            return Err(NotDerivable::ConflictingMarks.into());
        }
        let unknown = wavy && !own_mark;
        if !wavy && !own_mark {
            return Err(NotDerivable::NoMarks.into());
        }

        let mut vectors = Vec::with_capacity(4);
        if !unknown {
            let center = self.position(atom)?;
            for (&(neighbor, _), &lift) in neighbors.iter().zip(&lifts) {
                vectors.push(frame_vector(center, self.position(neighbor)?, lift)?);
            }
        }

        let mut slots: Vec<Slot> = neighbors.iter().map(|&(id, _)| Slot::Atom(id)).collect();
        if let Some(fourth) = fourth_position(atom, record, &orders)? {
            slots.push(fourth);
            if let [a, b, c] = vectors[..] {
                vectors.push([
                    -(a[0] + b[0] + c[0]),
                    -(a[1] + b[1] + c[1]),
                    -(a[2] + b[2] + c[2]),
                ]);
            }
        }
        let slots: [Slot; 4] = slots
            .try_into()
            .map_err(|_| StereoError::CenterUnsupported)?;
        if unknown {
            return Ok((CenterState::unknown(slots)?, None));
        }

        let orientation = orientation(&vectors);
        let turn = if orientation <= -DEGENERACY_LIMIT {
            Turn::Anticlockwise
        } else if orientation >= DEGENERACY_LIMIT {
            Turn::Clockwise
        } else {
            return Err(NotDerivable::Degenerate.into());
        };
        // The slots are already canonical (atoms ascending, then the non-atom slot), so the
        // constructor keeps the turn as it is.
        Ok((
            CenterState::tetrahedral(slots, turn)?,
            Some(orientation.abs()),
        ))
    }

    /// The relation of the two references of double bond `bond` that the drawing shows: both
    /// references on the same side of the double bond's line (cis) or opposite sides (trans).
    /// The references are each end's lowest-identifier other neighbor, the canonical form a
    /// stored record has. A crossed double bond is [`DoubleBondState::Unknown`] without reading
    /// any position, even when an end has no other neighbor (an uncrossed one is `NoReference`);
    /// only the structural checks below come first, so it never returns a state a document
    /// would refuse to store. Handedness never matters here, so there is no mirror law to apply.
    ///
    /// # Errors
    ///
    /// [`DeriveError::UnknownBond`]; [`StereoError::DoubleBondRequired`],
    /// [`StereoError::GeometryUnsupported`] (a cumulated double or triple bond at an end) and
    /// [`StereoError::ReferenceInvalid`] (an end with more than two other neighbors);
    /// [`DeriveError::MissingPosition`]; and `NoReference`, `CoincidentAtoms` and `Degenerate`.
    /// A query atom as an end is not special: its geometry is defined like any other.
    pub fn derive_double_bond(&self, bond: BondId) -> Result<DoubleBondState, DeriveError> {
        let target = self
            .graph
            .bond(bond)
            .ok_or(DeriveError::UnknownBond(bond))?;
        if target.order != BondOrder::Double {
            return Err(StereoError::DoubleBondRequired.into());
        }
        let begin_others = self.graph.other_neighbors(target.begin, target.end)?;
        let end_others = self.graph.other_neighbors(target.end, target.begin)?;
        if target.mark == Some(StereoMark::Crossed) {
            return Ok(DoubleBondState::Unknown);
        }
        let (Some(&begin_ref), Some(&end_ref)) = (begin_others.first(), end_others.first()) else {
            return Err(NotDerivable::NoReference.into());
        };

        let begin = self.position(target.begin)?;
        let end = self.position(target.end)?;
        let axis = (end.x() - begin.x(), end.y() - begin.y());
        let axis_squared = axis.0 * axis.0 + axis.1 * axis.1;
        let begin_side = self.side_of_axis(axis, axis_squared, begin, begin_ref)?;
        let end_side = self.side_of_axis(axis, axis_squared, end, end_ref)?;
        // Neither side is zero: `side_of_axis` refused a reference on the axis.
        let relation = if begin_side.is_sign_positive() == end_side.is_sign_positive() {
            Relation::SameSide
        } else {
            Relation::OppositeSides
        };
        Ok(DoubleBondState::Relative {
            begin_ref: Slot::Atom(begin_ref),
            end_ref: Slot::Atom(end_ref),
            relation,
        })
    }

    /// The signed side (the 2D cross product of the axis and the reference bond) of `reference`
    /// relative to the axis through `from`; refused when the axis or the reference bond is
    /// shorter than [`MIN_BOND_LENGTH`], or the sine of their angle is below [`COLLINEAR_SINE`].
    fn side_of_axis(
        &self,
        axis: (f64, f64),
        axis_squared: f64,
        from: Point,
        reference: AtomId,
    ) -> Result<f64, DeriveError> {
        let to = self.position(reference)?;
        let arm = (to.x() - from.x(), to.y() - from.y());
        let arm_squared = arm.0 * arm.0 + arm.1 * arm.1;
        let minimum = MIN_BOND_LENGTH * MIN_BOND_LENGTH;
        if axis_squared < minimum || arm_squared < minimum {
            return Err(NotDerivable::CoincidentAtoms.into());
        }
        let cross = axis.0 * arm.1 - axis.1 * arm.0;
        // sin(angle) = |cross| / (|axis| |arm|), compared without a square root.
        if cross * cross < COLLINEAR_SINE * COLLINEAR_SINE * axis_squared * arm_squared {
            return Err(NotDerivable::Degenerate.into());
        }
        Ok(cross)
    }
}

/// The atom at the narrow end of a mark on `bond`.
const fn narrow_atom(bond: &Bond, narrow: BondEnd) -> AtomId {
    match narrow {
        BondEnd::Begin => bond.begin,
        BondEnd::End => bond.end,
    }
}

/// The frame vector of a neighbor at `to` seen from `center`: the unit direction with y negated
/// (document y is down) and the lift as z.
fn frame_vector(center: Point, to: Point, lift: f64) -> Result<[f64; 3], DeriveError> {
    let delta = (to.x() - center.x(), to.y() - center.y());
    let squared = delta.0 * delta.0 + delta.1 * delta.1;
    if squared < MIN_BOND_LENGTH * MIN_BOND_LENGTH {
        return Err(NotDerivable::CoincidentAtoms.into());
    }
    let length = squared.sqrt();
    Ok([delta.0 / length, -(delta.1 / length), lift])
}

/// `D = det[v1 - v0, v2 - v0, v3 - v0]`.
fn orientation(vectors: &[[f64; 3]]) -> f64 {
    let [v0, v1, v2, v3] = vectors else {
        return 0.0;
    };
    let edge = |v: &[f64; 3]| [v[0] - v0[0], v[1] - v0[1], v[2] - v0[2]];
    let (a, b, c) = (edge(v1), edge(v2), edge(v3));
    a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
        + a[2] * (b[0] * c[1] - b[1] * c[0])
}

/// The position that completes the neighbors of a center, by the one rule every reader of a
/// drawing shares. `orders` holds the integral valence contribution of each bonded neighbor
/// (`None` where a bond has none).
///
/// With three neighbors the atom passes these gates, in this order, or the result is
/// `FourthPosition`:
///
/// - It is an element atom (a query atom is refused), has no radical, and its hydrogens are
///   not unknown.
/// - Every bond has an integral value (aromatic, dative, query and unknown bonds have none).
/// - [`chemistry::assess`] finds it valid, with the stated or the inferred hydrogen count. A
///   stated count is never taken at face value: the valence model must accept it, so a count
///   it contradicts, an element or charge it does not list, and an atom with attachment points
///   are all refused.
///
/// Then the hydrogen count `n` decides:
///
/// - `n = 1`: [`Slot::ImplicitHydrogen`], whatever the charge (an ammonium or a borohydride is a
///   legitimate center), unless a bond is multiple or the bond order sum plus the hydrogen
///   exceeds the lowest valence the model lists for the element and charge (hypervalent), which
///   are refused.
/// - `n = 0`: [`Slot::LonePair`] for a neutral N or P with a bond order sum of three, or a
///   neutral S with a sum of four (the sulfoxide case). Refused: every other element
///   (carbocations, carbanions, boron, sp2 carbon), every charged atom (charged heteroatoms
///   such as an oxonium), and every other sum (hypervalent nitrogen).
/// - `n >= 2`: refused.
///
/// With four neighbors the result is `Ok(None)`, unless hydrogens are stated as more than none,
/// which would be a fifth position and is refused. The inferred count is not consulted. Any
/// other number of neighbors is refused.
///
/// Naming a fourth position never judges whether the atom is a stereocenter. A change to
/// `chemifree-valence-3` can change what an atom is named here.
///
/// # Errors
///
/// [`NotDerivable::FourthPosition`], as above.
pub fn fourth_position(
    id: AtomId,
    atom: &Atom,
    orders: &[Option<u16>],
) -> Result<Option<Slot>, NotDerivable> {
    let refused = NotDerivable::FourthPosition;
    match orders.len() {
        4 => {
            return match atom.hydrogens {
                Hydrogens::Specified(count) if count > 0 => Err(refused),
                _ => Ok(None),
            };
        }
        3 => {}
        _ => return Err(refused),
    }
    let element = atom.kind.element().ok_or(refused)?;
    if atom.radical != Radical::None || atom.hydrogens == Hydrogens::Unknown {
        return Err(refused);
    }
    let sum = orders.iter().copied().sum::<Option<u16>>().ok_or(refused)?;
    let assessment = chemistry::assess(id, atom, orders);
    // The model's count, which is the stated one when the atom states it, and only when the
    // model accepts the whole atom.
    let (Finding::Valid, Some(count)) = (assessment.finding, assessment.hydrogen_count) else {
        return Err(refused);
    };
    let symbol = element.symbol();
    match count {
        1 => {
            let multiple_bond = orders
                .iter()
                .any(|order| order.is_some_and(|value| value > 1));
            let hypervalent = chemistry::restricted_target(symbol, atom.charge)
                .is_some_and(|target| sum + 1 > target);
            if multiple_bond || hypervalent {
                return Err(refused);
            }
            Ok(Some(Slot::ImplicitHydrogen))
        }
        0 => {
            let lone_pair_sum = match symbol {
                "N" | "P" => 3,
                "S" => 4,
                _ => return Err(refused),
            };
            if atom.charge != 0 || sum != lone_pair_sum {
                return Err(refused);
            }
            Ok(Some(Slot::LonePair))
        }
        _ => Err(refused),
    }
}
