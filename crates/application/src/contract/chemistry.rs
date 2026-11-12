//! Atom and bond state shapes (orders, queries, hydrogens, radicals) and their domain conversions.

use chemifree_domain::document::BondOrder;
use chemifree_domain::states::{BondQuery, Hydrogens, QueryAtom, QueryBondOrder, Radical};
use serde::{Deserialize, Serialize};

/// A bond's kind. `aromatic`, `dative`, `query`, and `unknown` were added within v1; a bond of
/// order `query` carries its expression in `query`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BondOrderDto {
    Single,
    Double,
    Triple,
    Aromatic,
    /// From `begin` (the donor) to `end`.
    Dative,
    Query,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BondToDto {
    pub atom: String,
    pub order: BondOrderDto,
    /// Required with order `query`, refused with any other.
    #[serde(default)]
    pub query: Option<BondQueryDto>,
}

/// A query atom: which elements it stands for. `chem.substructure` matches it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueryDto {
    /// Any atom; labelled `*`.
    Any {},
    /// Any atom but hydrogen; labelled `A`.
    AnyHeavy {},
    /// Any atom but carbon and hydrogen; labelled `Q`.
    Heteroatom {},
    /// Any halogen; labelled `X`.
    Halogen {},
    /// One of 1-10 distinct elements, or with `negated`, any atom but them.
    List {
        elements: Vec<String>,
        #[serde(default)]
        negated: bool,
    },
}

/// What a query bond stands for. `chem.substructure` matches it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BondQueryDto {
    Any {},
    /// One of these distinct kinds.
    List {
        orders: Vec<QueryBondOrderDto>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryBondOrderDto {
    Single,
    Double,
    Triple,
    Aromatic,
}

/// An atom's recorded hydrogen state. `inferred` (the default) carries no count; a separate
/// bounded inspection can derive one without changing this value.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum HydrogensDto {
    Inferred {},
    Unknown {},
    /// A stated count of non-vertex hydrogens, 0-16.
    Specified {
        count: u64,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RadicalDto {
    None,
    Singlet,
    Doublet,
    Triplet,
    Unknown,
}

impl From<&BondOrder> for BondOrderDto {
    fn from(order: &BondOrder) -> Self {
        match order {
            BondOrder::Single => Self::Single,
            BondOrder::Double => Self::Double,
            BondOrder::Triple => Self::Triple,
            BondOrder::Aromatic => Self::Aromatic,
            BondOrder::Dative => Self::Dative,
            BondOrder::Query(_) => Self::Query,
            BondOrder::Unknown => Self::Unknown,
        }
    }
}

impl From<QueryBondOrderDto> for QueryBondOrder {
    fn from(order: QueryBondOrderDto) -> Self {
        match order {
            QueryBondOrderDto::Single => Self::Single,
            QueryBondOrderDto::Double => Self::Double,
            QueryBondOrderDto::Triple => Self::Triple,
            QueryBondOrderDto::Aromatic => Self::Aromatic,
        }
    }
}

impl From<QueryBondOrder> for QueryBondOrderDto {
    fn from(order: QueryBondOrder) -> Self {
        match order {
            QueryBondOrder::Single => Self::Single,
            QueryBondOrder::Double => Self::Double,
            QueryBondOrder::Triple => Self::Triple,
            QueryBondOrder::Aromatic => Self::Aromatic,
        }
    }
}

impl From<&BondQuery> for BondQueryDto {
    fn from(query: &BondQuery) -> Self {
        match query {
            BondQuery::Any => Self::Any {},
            BondQuery::OneOf(orders) => Self::List {
                orders: orders.iter().map(|order| (*order).into()).collect(),
            },
        }
    }
}

impl From<RadicalDto> for Radical {
    fn from(radical: RadicalDto) -> Self {
        match radical {
            RadicalDto::None => Self::None,
            RadicalDto::Singlet => Self::Singlet,
            RadicalDto::Doublet => Self::Doublet,
            RadicalDto::Triplet => Self::Triplet,
            RadicalDto::Unknown => Self::Unknown,
        }
    }
}

impl From<Radical> for RadicalDto {
    fn from(radical: Radical) -> Self {
        match radical {
            Radical::None => Self::None,
            Radical::Singlet => Self::Singlet,
            Radical::Doublet => Self::Doublet,
            Radical::Triplet => Self::Triplet,
            Radical::Unknown => Self::Unknown,
        }
    }
}

impl From<Hydrogens> for HydrogensDto {
    fn from(hydrogens: Hydrogens) -> Self {
        match hydrogens {
            Hydrogens::Inferred => Self::Inferred {},
            Hydrogens::Unknown => Self::Unknown {},
            Hydrogens::Specified(count) => Self::Specified {
                count: count.into(),
            },
        }
    }
}

impl From<&QueryAtom> for QueryDto {
    fn from(query: &QueryAtom) -> Self {
        match query {
            QueryAtom::Any => Self::Any {},
            QueryAtom::AnyHeavy => Self::AnyHeavy {},
            QueryAtom::Heteroatom => Self::Heteroatom {},
            QueryAtom::Halogen => Self::Halogen {},
            QueryAtom::List { elements, negated } => Self::List {
                elements: elements
                    .iter()
                    .map(|element| element.symbol().to_owned())
                    .collect(),
                negated: *negated,
            },
        }
    }
}

/// The label a query atom is shown with, which `AtomView::element` carries for it: `*`, `A`,
/// `Q`, `X`, or the element list in brackets (`[N,O]`, negated `![N,O]`). It is never an
/// element symbol; `kind` says which an atom has.
pub fn query_label(query: &QueryAtom) -> String {
    match query {
        QueryAtom::Any => "*".to_owned(),
        QueryAtom::AnyHeavy => "A".to_owned(),
        QueryAtom::Heteroatom => "Q".to_owned(),
        QueryAtom::Halogen => "X".to_owned(),
        QueryAtom::List { elements, negated } => {
            let symbols: Vec<&str> = elements.iter().map(|element| element.symbol()).collect();
            format!("{}[{}]", if *negated { "!" } else { "" }, symbols.join(","))
        }
    }
}
