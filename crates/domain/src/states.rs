//! Recorded atom and bond states beyond element, charge, and isotope: the atom's kind (an
//! element or a query), its hydrogen state, radical, and attachment points, and the bond kinds
//! a query bond allows.
//!
//! These record what was drawn or stated. The valence module can derive a separate hydrogen count for supported input; aromaticity
//! is not perceived here, and no query is matched against anything. A
//! state that is representable is not thereby computable.

use std::fmt;

use crate::element::Element;

/// The most non-vertex hydrogens an atom can be stated to carry.
pub const MAX_STATED_HYDROGENS: u8 = 16;
/// The most elements a query atom's element list may hold.
pub const MAX_QUERY_ELEMENTS: usize = 10;
/// Attachment points are numbered 1 through this.
pub const MAX_ATTACHMENT_POINT: u8 = 4;

/// What an atom is: an element, or a query that stands for a set of elements.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AtomKind {
    Element(Element),
    Query(QueryAtom),
}

impl From<Element> for AtomKind {
    fn from(element: Element) -> Self {
        Self::Element(element)
    }
}

impl AtomKind {
    /// The element, or `None` for a query atom.
    pub const fn element(&self) -> Option<Element> {
        match self {
            Self::Element(element) => Some(*element),
            Self::Query(_) => None,
        }
    }
}

/// A query atom: which elements it stands for. Recorded only; nothing matches it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueryAtom {
    /// Any atom (`*`).
    Any,
    /// Any atom except hydrogen (`A`).
    AnyHeavy,
    /// Any atom except carbon and hydrogen (`Q`).
    Heteroatom,
    /// Any halogen (`X`).
    Halogen,
    /// One of the listed elements, or with `negated`, any atom but them. The list keeps the
    /// order it was given in and must hold 1 to [`MAX_QUERY_ELEMENTS`] distinct elements.
    List {
        elements: Vec<Element>,
        negated: bool,
    },
}

/// What is recorded about an atom's hydrogens. Separately drawn hydrogen atoms are ordinary
/// atoms and are not counted here.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Hydrogens {
    /// To be derived from valence; the recorded state itself carries no count.
    #[default]
    Inferred,
    /// Stated to be unknown.
    Unknown,
    /// Stated: this many non-vertex hydrogens, 0 to [`MAX_STATED_HYDROGENS`].
    Specified(u8),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Radical {
    #[default]
    None,
    Singlet,
    Doublet,
    Triplet,
    Unknown,
}

/// An atom's attachment points: distinct values 1 to [`MAX_ATTACHMENT_POINT`], kept in
/// ascending order. Empty by default.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct AttachmentPoints(u8);

impl AttachmentPoints {
    /// The points `values` names, which must be distinct, ascending, and within
    /// 1..=[`MAX_ATTACHMENT_POINT`].
    ///
    /// # Errors
    ///
    /// [`StateError::AttachmentPointsInvalid`] for any other list.
    pub fn new(values: &[u8]) -> Result<Self, StateError> {
        let mut bits = 0_u8;
        let mut previous = 0;
        for &value in values {
            if value <= previous || value > MAX_ATTACHMENT_POINT {
                return Err(StateError::AttachmentPointsInvalid);
            }
            previous = value;
            bits |= 1 << (value - 1);
        }
        Ok(Self(bits))
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The points in ascending order.
    pub fn values(self) -> impl Iterator<Item = u8> {
        (1..=MAX_ATTACHMENT_POINT).filter(move |value| self.0 & (1 << (value - 1)) != 0)
    }
}

/// A bond kind a query bond may stand for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryBondOrder {
    Single,
    Double,
    Triple,
    Aromatic,
}

/// What a query bond stands for. Recorded only; nothing matches it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BondQuery {
    Any,
    /// One of these kinds: a non-empty list without repeats, kept in the order given.
    OneOf(Vec<QueryBondOrder>),
}

/// A combination of recorded states that is refused. Each has a stable code, sent as `reason`
/// with `invalid_command`. The list is closed and documented in docs/CONTRACT.md.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateError {
    /// A stated hydrogen count above [`MAX_STATED_HYDROGENS`].
    HydrogenCountOutOfRange,
    /// A query atom with a non-zero charge.
    QueryAtomCharge,
    /// A query atom with an isotope.
    QueryAtomIsotope,
    /// A query element list that is empty, longer than [`MAX_QUERY_ELEMENTS`], or repeats an
    /// element.
    QueryListInvalid,
    /// A query bond list that is empty or repeats a kind.
    BondQueryInvalid,
    /// A bond of order `query` without its expression.
    BondQueryMissing,
    /// A bond query expression with an order other than `query`.
    BondQueryUnexpected,
    /// Attachment points outside 1..=4, repeated, or not ascending.
    AttachmentPointsInvalid,
    /// An atom named both by element and by query, or by neither.
    AtomKindConflict,
}

impl StateError {
    /// Every code, in documentation order.
    pub const ALL: [Self; 9] = [
        Self::HydrogenCountOutOfRange,
        Self::QueryAtomCharge,
        Self::QueryAtomIsotope,
        Self::QueryListInvalid,
        Self::BondQueryInvalid,
        Self::BondQueryMissing,
        Self::BondQueryUnexpected,
        Self::AttachmentPointsInvalid,
        Self::AtomKindConflict,
    ];

    /// The stable machine-readable code.
    pub const fn code(self) -> &'static str {
        match self {
            Self::HydrogenCountOutOfRange => "hydrogen_count_out_of_range",
            Self::QueryAtomCharge => "query_atom_charge",
            Self::QueryAtomIsotope => "query_atom_isotope",
            Self::QueryListInvalid => "query_list_invalid",
            Self::BondQueryInvalid => "bond_query_invalid",
            Self::BondQueryMissing => "bond_query_missing",
            Self::BondQueryUnexpected => "bond_query_unexpected",
            Self::AttachmentPointsInvalid => "attachment_points_invalid",
            Self::AtomKindConflict => "atom_kind_conflict",
        }
    }
}

impl fmt::Display for StateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::HydrogenCountOutOfRange => {
                "a stated hydrogen count must be 0 to 16"
            }
            Self::QueryAtomCharge => "a query atom carries no charge",
            Self::QueryAtomIsotope => "a query atom carries no isotope",
            Self::QueryListInvalid => {
                "a query element list holds 1 to 10 distinct elements"
            }
            Self::BondQueryInvalid => {
                "a query bond list holds distinct kinds among single, double, triple, and aromatic, at least one"
            }
            Self::BondQueryMissing => "a bond of order query needs its query",
            Self::BondQueryUnexpected => "only a bond of order query has a query",
            Self::AttachmentPointsInvalid => {
                "attachment points are distinct values 1 to 4 in ascending order"
            }
            Self::AtomKindConflict => "name an atom by exactly one of element and query",
        })
    }
}

impl std::error::Error for StateError {}

/// Checks that a query or bond list holds 1..=`max` distinct entries.
fn distinct_within<T: PartialEq>(items: &[T], max: usize) -> bool {
    !items.is_empty()
        && items.len() <= max
        && items
            .iter()
            .enumerate()
            .all(|(index, item)| !items[..index].contains(item))
}

impl QueryAtom {
    pub(crate) fn validate(&self) -> Result<(), StateError> {
        match self {
            Self::List { elements, .. } if !distinct_within(elements, MAX_QUERY_ELEMENTS) => {
                Err(StateError::QueryListInvalid)
            }
            _ => Ok(()),
        }
    }
}

impl BondQuery {
    pub(crate) fn validate(&self) -> Result<(), StateError> {
        match self {
            // Four kinds exist, so a list without repeats holds at most four.
            Self::OneOf(orders) if !distinct_within(orders, 4) => Err(StateError::BondQueryInvalid),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_points_are_distinct_ascending_values_from_one_to_four() {
        let points = AttachmentPoints::new(&[1, 3, 4]).unwrap();
        assert_eq!(points.values().collect::<Vec<_>>(), [1, 3, 4]);
        assert!(AttachmentPoints::new(&[]).unwrap().is_empty());
        for refused in [&[0][..], &[5], &[2, 2], &[3, 1]] {
            assert_eq!(
                AttachmentPoints::new(refused),
                Err(StateError::AttachmentPointsInvalid),
                "{refused:?}"
            );
        }
    }

    #[test]
    fn query_lists_hold_one_to_ten_distinct_entries() {
        let element = |symbol| Element::from_symbol(symbol).unwrap();
        let list = |elements: Vec<Element>| QueryAtom::List {
            elements,
            negated: false,
        };
        assert_eq!(list(vec![element("N")]).validate(), Ok(()));
        assert_eq!(list(vec![]).validate(), Err(StateError::QueryListInvalid));
        assert_eq!(
            list(vec![element("N"), element("O"), element("N")]).validate(),
            Err(StateError::QueryListInvalid)
        );
        let ten: Vec<Element> = (1..=10)
            .map(|number| Element::from_atomic_number(number).unwrap())
            .collect();
        assert_eq!(list(ten.clone()).validate(), Ok(()));
        let eleven = [ten, vec![element("Na")]].concat();
        assert_eq!(list(eleven).validate(), Err(StateError::QueryListInvalid));

        assert_eq!(
            BondQuery::OneOf(vec![]).validate(),
            Err(StateError::BondQueryInvalid)
        );
        assert_eq!(
            BondQuery::OneOf(vec![QueryBondOrder::Single, QueryBondOrder::Single]).validate(),
            Err(StateError::BondQueryInvalid)
        );
        assert_eq!(BondQuery::Any.validate(), Ok(()));
    }

    #[test]
    fn codes_are_distinct() {
        let mut codes: Vec<&str> = StateError::ALL.iter().map(|error| error.code()).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), StateError::ALL.len());
    }
}
