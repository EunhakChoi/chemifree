//! Stored atom, bond, stereo-state and group records as domain values and back, with the
//! identifier and value parsers the other readers share.

use std::collections::BTreeSet;

use chemifree_domain::document::{
    Atom, AtomId, Bond, BondId, BondOrder, DocumentId, GroupId, MOLECULE_ID, ObjectId, PAGE_ID,
    Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::states::{
    AtomKind, AttachmentPoints, BondQuery, Hydrogens, QueryAtom, QueryBondOrder, Radical,
};
use chemifree_domain::stereo::{
    BondEnd, CenterState, DoubleBondState, Relation, Slot, StereoKey, StereoMark, StereoState, Turn,
};
use chemifree_domain::stereo_group::{GroupKind, StereoGroup};

use super::records::{
    AtomRecord, AtomState, BondRecord, BondState, StoredBondEnd, StoredBondQuery, StoredCenter,
    StoredDoubleBond, StoredHydrogens, StoredMark, StoredOrder, StoredPoint, StoredQuery,
    StoredQueryOrder, StoredRadical, StoredRelation, StoredStereoState, StoredTurn,
    UnmarkedBondState,
};
use super::scene::{StoredGroup, StoredGroupKind};
use super::v2;

impl AtomRecord for v2::AtomState {
    /// An element atom with inferred hydrogens, no radical, and no attachment points.
    fn into_atom(self) -> Result<Atom, String> {
        Ok(Atom {
            charge: self.charge,
            isotope: self.isotope,
            ..Atom::from(element(&self.element)?)
        })
    }
}

impl BondRecord for v2::BondState {
    fn into_bond(self) -> Result<Bond, String> {
        Ok(Bond::new(
            atom_id(&self.begin)?,
            atom_id(&self.end)?,
            match self.order {
                v2::Order::Single => BondOrder::Single,
                v2::Order::Double => BondOrder::Double,
                v2::Order::Triple => BondOrder::Triple,
            },
        ))
    }
}

impl AtomRecord for AtomState {
    fn into_atom(self) -> Result<Atom, String> {
        let kind = match (self.element, self.query) {
            (Some(symbol), None) => AtomKind::Element(element(&symbol)?),
            (None, Some(query)) => AtomKind::Query(query_atom(query)?),
            _ => return Err("an atom has exactly one of element and query".to_owned()),
        };
        Ok(Atom {
            kind,
            charge: self.charge,
            isotope: self.isotope,
            hydrogens: match self.hydrogens {
                StoredHydrogens::Inferred {} => Hydrogens::Inferred,
                StoredHydrogens::Unknown {} => Hydrogens::Unknown,
                StoredHydrogens::Specified { count } => Hydrogens::Specified(count),
            },
            radical: match self.radical {
                StoredRadical::None => Radical::None,
                StoredRadical::Singlet => Radical::Singlet,
                StoredRadical::Doublet => Radical::Doublet,
                StoredRadical::Triplet => Radical::Triplet,
                StoredRadical::Unknown => Radical::Unknown,
            },
            // The same rule as an edit's; the rest is checked by `Document::restore`.
            attachment_points: AttachmentPoints::new(&self.attachment_points)
                .map_err(|error| error.to_string())?,
        })
    }
}

impl BondRecord for UnmarkedBondState {
    fn into_bond(self) -> Result<Bond, String> {
        Ok(Bond::new(
            atom_id(&self.begin)?,
            atom_id(&self.end)?,
            read_order(self.order, self.query)?,
        ))
    }
}

impl BondRecord for BondState {
    fn into_bond(self) -> Result<Bond, String> {
        Ok(Bond {
            mark: self.mark.map(read_mark),
            ..Bond::new(
                atom_id(&self.begin)?,
                atom_id(&self.end)?,
                read_order(self.order, self.query)?,
            )
        })
    }
}

fn read_order(order: StoredOrder, query: Option<StoredBondQuery>) -> Result<BondOrder, String> {
    Ok(match (order, query) {
        (StoredOrder::Query, Some(query)) => BondOrder::Query(match query {
            StoredBondQuery::Any {} => BondQuery::Any,
            StoredBondQuery::List { orders } => BondQuery::OneOf(
                orders
                    .into_iter()
                    .map(|order| match order {
                        StoredQueryOrder::Single => QueryBondOrder::Single,
                        StoredQueryOrder::Double => QueryBondOrder::Double,
                        StoredQueryOrder::Triple => QueryBondOrder::Triple,
                        StoredQueryOrder::Aromatic => QueryBondOrder::Aromatic,
                    })
                    .collect(),
            ),
        }),
        (StoredOrder::Query, None) => return Err("a query bond needs its query".to_owned()),
        (_, Some(_)) => return Err("only a query bond has a query".to_owned()),
        (StoredOrder::Single, None) => BondOrder::Single,
        (StoredOrder::Double, None) => BondOrder::Double,
        (StoredOrder::Triple, None) => BondOrder::Triple,
        (StoredOrder::Aromatic, None) => BondOrder::Aromatic,
        (StoredOrder::Dative, None) => BondOrder::Dative,
        (StoredOrder::Unknown, None) => BondOrder::Unknown,
    })
}

fn read_mark(mark: StoredMark) -> StereoMark {
    let end = |end| match end {
        StoredBondEnd::Begin => BondEnd::Begin,
        StoredBondEnd::End => BondEnd::End,
    };
    match mark {
        StoredMark::Wedge { narrow } => StereoMark::Wedge {
            narrow: end(narrow),
        },
        StoredMark::Hash { narrow } => StereoMark::Hash {
            narrow: end(narrow),
        },
        StoredMark::Wavy {} => StereoMark::Wavy,
        StoredMark::Crossed {} => StereoMark::Crossed,
    }
}

fn stored_mark(mark: StereoMark) -> StoredMark {
    let end = |end| match end {
        BondEnd::Begin => StoredBondEnd::Begin,
        BondEnd::End => StoredBondEnd::End,
    };
    match mark {
        StereoMark::Wedge { narrow } => StoredMark::Wedge {
            narrow: end(narrow),
        },
        StereoMark::Hash { narrow } => StoredMark::Hash {
            narrow: end(narrow),
        },
        StereoMark::Wavy => StoredMark::Wavy {},
        StereoMark::Crossed => StoredMark::Crossed {},
    }
}

/// A record id is an atom's (a center) or a bond's (a double bond).
pub(super) fn stereo_key(id: &str) -> Result<StereoKey, String> {
    AtomId::parse(id)
        .map(StereoKey::Center)
        .or_else(|| BondId::parse(id).map(StereoKey::DoubleBond))
        .ok_or_else(|| format!("invalid stereo record identifier {id:?}"))
}

pub(super) fn stereo_key_id(key: StereoKey) -> String {
    match key {
        StereoKey::Center(atom) => atom.to_string(),
        StereoKey::DoubleBond(bond) => bond.to_string(),
    }
}

const IMPLICIT_HYDROGEN: &str = "implicit_hydrogen";
const LONE_PAIR: &str = "lone_pair";

fn read_slot(value: &str) -> Result<Slot, String> {
    match value {
        IMPLICIT_HYDROGEN => Ok(Slot::ImplicitHydrogen),
        LONE_PAIR => Ok(Slot::LonePair),
        id => atom_id(id).map(Slot::Atom),
    }
}

fn stored_slot(slot: &Slot) -> String {
    match slot {
        Slot::Atom(atom) => atom.to_string(),
        Slot::ImplicitHydrogen => IMPLICIT_HYDROGEN.to_owned(),
        Slot::LonePair => LONE_PAIR.to_owned(),
    }
}

fn read_slots(slots: [String; 4]) -> Result<[Slot; 4], String> {
    let [first, second, third, fourth] = slots;
    Ok([
        read_slot(&first)?,
        read_slot(&second)?,
        read_slot(&third)?,
        read_slot(&fourth)?,
    ])
}

/// A stored configuration as the domain's; slots must already be in canonical order, which
/// `Document::restore` checks along with everything else about the record.
pub(super) fn read_stereo_state(state: StoredStereoState) -> Result<StereoState, String> {
    Ok(match state {
        StoredStereoState::Center { state } => StereoState::Center(match state {
            StoredCenter::Tetrahedral { neighbors, turn } => CenterState::Tetrahedral {
                slots: read_slots(neighbors)?,
                turn: match turn {
                    StoredTurn::Anticlockwise => Turn::Anticlockwise,
                    StoredTurn::Clockwise => Turn::Clockwise,
                },
            },
            StoredCenter::Unknown { neighbors } => CenterState::Unknown {
                slots: read_slots(neighbors)?,
            },
        }),
        StoredStereoState::DoubleBond { state } => StereoState::DoubleBond(match state {
            StoredDoubleBond::Relative {
                begin_ref,
                end_ref,
                relation,
            } => DoubleBondState::Relative {
                begin_ref: read_slot(&begin_ref)?,
                end_ref: read_slot(&end_ref)?,
                relation: match relation {
                    StoredRelation::SameSide => Relation::SameSide,
                    StoredRelation::OppositeSides => Relation::OppositeSides,
                },
            },
            StoredDoubleBond::Unknown {} => DoubleBondState::Unknown,
        }),
    })
}

pub(super) fn stored_stereo_state(state: &StereoState) -> StoredStereoState {
    match state {
        StereoState::Center(CenterState::Tetrahedral { slots, turn }) => {
            StoredStereoState::Center {
                state: StoredCenter::Tetrahedral {
                    neighbors: slots.each_ref().map(stored_slot),
                    turn: match turn {
                        Turn::Anticlockwise => StoredTurn::Anticlockwise,
                        Turn::Clockwise => StoredTurn::Clockwise,
                    },
                },
            }
        }
        StereoState::Center(CenterState::Unknown { slots }) => StoredStereoState::Center {
            state: StoredCenter::Unknown {
                neighbors: slots.each_ref().map(stored_slot),
            },
        },
        StereoState::DoubleBond(DoubleBondState::Relative {
            begin_ref,
            end_ref,
            relation,
        }) => StoredStereoState::DoubleBond {
            state: StoredDoubleBond::Relative {
                begin_ref: stored_slot(begin_ref),
                end_ref: stored_slot(end_ref),
                relation: match relation {
                    Relation::SameSide => StoredRelation::SameSide,
                    Relation::OppositeSides => StoredRelation::OppositeSides,
                },
            },
        },
        StereoState::DoubleBond(DoubleBondState::Unknown) => StoredStereoState::DoubleBond {
            state: StoredDoubleBond::Unknown {},
        },
    }
}

fn query_atom(query: StoredQuery) -> Result<QueryAtom, String> {
    Ok(match query {
        StoredQuery::Any {} => QueryAtom::Any,
        StoredQuery::AnyHeavy {} => QueryAtom::AnyHeavy,
        StoredQuery::Heteroatom {} => QueryAtom::Heteroatom,
        StoredQuery::Halogen {} => QueryAtom::Halogen,
        StoredQuery::List { elements, negated } => QueryAtom::List {
            elements: elements
                .iter()
                .map(|symbol| element(symbol))
                .collect::<Result<_, _>>()?,
            negated,
        },
    })
}

pub(super) fn atom_state(atom: &Atom) -> AtomState {
    let (element, query) = match &atom.kind {
        AtomKind::Element(element) => (Some(element.symbol().to_owned()), None),
        AtomKind::Query(query) => (None, Some(stored_query(query))),
    };
    AtomState {
        element,
        query,
        charge: atom.charge,
        isotope: atom.isotope,
        hydrogens: match atom.hydrogens {
            Hydrogens::Inferred => StoredHydrogens::Inferred {},
            Hydrogens::Unknown => StoredHydrogens::Unknown {},
            Hydrogens::Specified(count) => StoredHydrogens::Specified { count },
        },
        radical: match atom.radical {
            Radical::None => StoredRadical::None,
            Radical::Singlet => StoredRadical::Singlet,
            Radical::Doublet => StoredRadical::Doublet,
            Radical::Triplet => StoredRadical::Triplet,
            Radical::Unknown => StoredRadical::Unknown,
        },
        attachment_points: atom.attachment_points.values().collect(),
    }
}

fn stored_query(query: &QueryAtom) -> StoredQuery {
    match query {
        QueryAtom::Any => StoredQuery::Any {},
        QueryAtom::AnyHeavy => StoredQuery::AnyHeavy {},
        QueryAtom::Heteroatom => StoredQuery::Heteroatom {},
        QueryAtom::Halogen => StoredQuery::Halogen {},
        QueryAtom::List { elements, negated } => StoredQuery::List {
            elements: elements
                .iter()
                .map(|element| element.symbol().to_owned())
                .collect(),
            negated: *negated,
        },
    }
}

pub(super) fn bond_state(bond: &Bond) -> BondState {
    let (order, query) = match &bond.order {
        BondOrder::Single => (StoredOrder::Single, None),
        BondOrder::Double => (StoredOrder::Double, None),
        BondOrder::Triple => (StoredOrder::Triple, None),
        BondOrder::Aromatic => (StoredOrder::Aromatic, None),
        BondOrder::Dative => (StoredOrder::Dative, None),
        BondOrder::Unknown => (StoredOrder::Unknown, None),
        BondOrder::Query(BondQuery::Any) => (StoredOrder::Query, Some(StoredBondQuery::Any {})),
        BondOrder::Query(BondQuery::OneOf(orders)) => (
            StoredOrder::Query,
            Some(StoredBondQuery::List {
                orders: orders
                    .iter()
                    .map(|order| match order {
                        QueryBondOrder::Single => StoredQueryOrder::Single,
                        QueryBondOrder::Double => StoredQueryOrder::Double,
                        QueryBondOrder::Triple => StoredQueryOrder::Triple,
                        QueryBondOrder::Aromatic => StoredQueryOrder::Aromatic,
                    })
                    .collect(),
            }),
        ),
    };
    BondState {
        begin: bond.begin.to_string(),
        end: bond.end.to_string(),
        order,
        query,
        mark: bond.mark.map(stored_mark),
    }
}

pub(super) fn stored_point(point: &Point) -> StoredPoint {
    StoredPoint {
        x: point.x(),
        y: point.y(),
    }
}

/// Checks that an instance is on the document's page and draws its molecule.
pub(super) fn parents(page: &str, molecule: &str) -> Result<(), String> {
    if page == PAGE_ID && molecule == MOLECULE_ID {
        Ok(())
    } else {
        Err(format!(
            "a scene instance names page {page:?} and molecule {molecule:?}; a document holds \
             page {PAGE_ID:?} and molecule {MOLECULE_ID:?}"
        ))
    }
}

fn element(symbol: &str) -> Result<Element, String> {
    Element::from_symbol(symbol).ok_or_else(|| format!("unknown element {symbol:?}"))
}

pub(super) fn point(x: f64, y: f64) -> Result<Point, String> {
    Point::new(x, y).map_err(|error| error.to_string())
}

pub(super) fn document_id(value: &str) -> Result<DocumentId, String> {
    DocumentId::parse(value).ok_or_else(|| "invalid document identifier".to_owned())
}

pub(super) fn atom_id(value: &str) -> Result<AtomId, String> {
    AtomId::parse(value).ok_or_else(|| format!("invalid atom identifier {value:?}"))
}

pub(super) fn bond_id(value: &str) -> Result<BondId, String> {
    BondId::parse(value).ok_or_else(|| format!("invalid bond identifier {value:?}"))
}

pub(super) fn group_id(value: &str) -> Result<GroupId, String> {
    GroupId::parse(value).ok_or_else(|| format!("invalid stereo group identifier {value:?}"))
}

pub(super) fn stored_group(group: &StereoGroup) -> StoredGroup {
    StoredGroup {
        kind: match group.kind() {
            GroupKind::Absolute => StoredGroupKind::Absolute,
            GroupKind::And => StoredGroupKind::And,
            GroupKind::Or => StoredGroupKind::Or,
        },
        number: group.stored_number(),
        members: group.members().iter().map(ToString::to_string).collect(),
    }
}

/// A group as stated. A member listed twice is refused rather than merged; whether the group
/// is valid (centers, numbers) is `Document::restore`'s check.
pub(super) fn read_group(stored: StoredGroup) -> Result<StereoGroup, String> {
    let mut members = BTreeSet::new();
    for member in &stored.members {
        if !members.insert(atom_id(member)?) {
            return Err(format!("a stereo group lists atom {member:?} twice"));
        }
    }
    let kind = match stored.kind {
        StoredGroupKind::Absolute => GroupKind::Absolute,
        StoredGroupKind::And => GroupKind::And,
        StoredGroupKind::Or => GroupKind::Or,
    };
    Ok(StereoGroup::new(kind, stored.number, members))
}

pub(super) fn instance_id(value: &str) -> Result<ObjectId, String> {
    ObjectId::parse(value).ok_or_else(|| format!("invalid scene object identifier {value:?}"))
}
