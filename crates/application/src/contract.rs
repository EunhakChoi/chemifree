//! v1 document request and response shapes shared by HTTP and the CLI.
//! The browser mirror lives in `apps/web/src/lib/document-contract.ts`.

use std::collections::BTreeMap;
use std::fmt;

use chemifree_domain::document::{
    Atom, AtomId, AtomKind, AtomQuery, AtomRef, AtomUpdate, AttachmentPoints, Bond, BondId,
    BondOrder, BondQuery, BondTo, Command, Document, EditError, Hydrogens, InstanceId, MAX_CHARGE,
    MAX_HYDROGEN_COUNT, MIN_CHARGE, MOLECULE_ID, NewAtomPositions, PAGE_ID, Point, QueryBondOrder,
    Radical, Vector,
};
use chemifree_domain::element::Element;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::API_VERSION;
use crate::error::{ApplicationError, CommandReason, ErrorCode};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateDocumentRequest {
    pub request_id: String,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransactionRequest {
    pub request_id: String,
    pub expected_revision: u64,
    pub commands: Vec<CommandDto>,
}

/// Body of undo, redo, and save requests.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionRequest {
    pub request_id: String,
    pub expected_revision: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloseRequest {
    pub request_id: String,
    #[serde(default)]
    pub discard_unsaved: bool,
    /// Required with `discard_unsaved`: the revision whose changes the client agreed to lose.
    #[serde(default)]
    pub expected_revision: Option<u64>,
}

/// Body of autosave, recover, and discard-draft requests.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftRequest {
    pub request_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameRequest {
    pub request_id: String,
    pub expected_revision: u64,
    pub title: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreRequest {
    pub request_id: String,
}

/// How the document list is narrowed and ordered: `?query=…&sort=title|recent`.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    /// Text the title must contain, ignoring case; empty lists everything.
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub sort: ListSort,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ListSort {
    /// By title, then identifier.
    #[default]
    Title,
    /// Most recently saved first, then by title.
    Recent,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveAsRequest {
    pub request_id: String,
    pub expected_revision: u64,
    /// Defaults to the source title followed by " (copy)".
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoverAsCopyRequest {
    pub request_id: String,
    /// Defaults to the draft's title followed by " (recovered)".
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PointDto {
    pub x: f64,
    pub y: f64,
}

/// A bond's kind on the wire. `query` comes with a `query` expression; `dative` points from
/// `begin` (the donor) to `end`. `single`, `double`, and `triple` are the original v1 values.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BondOrderDto {
    Single,
    Double,
    Triple,
    Aromatic,
    Dative,
    Query,
    Unknown,
}

/// A query bond's expression: `{"kind": "any"}`, or `{"kind": "list", "orders": [...]}` with
/// distinct orders among `single`, `double`, `triple`, and `aromatic`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BondQueryDto {
    pub kind: QueryListKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orders: Option<Vec<QueryOrderDto>>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryListKind {
    Any,
    List,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryOrderDto {
    Single,
    Double,
    Triple,
    Aromatic,
}

/// A query atom's expression: `{"kind": "any" | "any_heavy" | "heteroatom" | "halogen"}`, or
/// `{"kind": "list", "elements": [...], "negated": bool}` (`negated` defaults to `false` in
/// commands and is always present in replies).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AtomQueryDto {
    pub kind: AtomQueryKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elements: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negated: Option<bool>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AtomQueryKind {
    Any,
    AnyHeavy,
    Heteroatom,
    Halogen,
    List,
}

/// Hydrogens not drawn as atoms: `{"mode": "inferred"}` (not computed: no count is known),
/// `{"mode": "unknown"}`, or `{"mode": "specified", "count": 0..16}`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HydrogensDto {
    pub mode: HydrogenMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HydrogenMode {
    Inferred,
    Unknown,
    Specified,
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

/// Whether an atom is an element or a query.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AtomKindName {
    Element,
    Query,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BondToDto {
    pub atom: String,
    pub order: BondOrderDto,
    /// With `order: "query"` only.
    #[serde(default)]
    pub query: Option<BondQueryDto>,
}

/// One command in a transaction, tagged by `op`. Identifiers use the `a1`/`b1`/`s1` form.
/// Where a new bond names an atom (`bond_to.atom`, `begin`, `end`), `"$N"` also refers to
/// the atom added by the `add_atom` command at index `N` earlier in the same transaction.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandDto {
    /// Exactly one of `element` and `query`, and exactly one of `position` (while the document
    /// has one scene instance) and `positions` (a point for each scene instance, keyed by
    /// instance identifier). The new atom is neutral and of natural abundance; omitted
    /// `hydrogens`, `radical`, and `attachment_points` are inferred, none, and none.
    AddAtom {
        #[serde(default)]
        element: Option<String>,
        #[serde(default)]
        query: Option<AtomQueryDto>,
        #[serde(default)]
        position: Option<PointDto>,
        #[serde(default, deserialize_with = "distinct_keys")]
        positions: Option<BTreeMap<String, PointDto>>,
        #[serde(default)]
        bond_to: Option<BondToDto>,
        #[serde(default)]
        hydrogens: Option<HydrogensDto>,
        #[serde(default)]
        radical: Option<RadicalDto>,
        #[serde(default)]
        attachment_points: Option<Vec<u64>>,
    },
    AddBond {
        begin: String,
        end: String,
        order: BondOrderDto,
        /// With `order: "query"` only.
        #[serde(default)]
        query: Option<BondQueryDto>,
    },
    /// Omitted fields are unchanged; `"isotope": null` restores natural abundance. `element`
    /// and `query` change the atom's kind; at most one of them may be given.
    UpdateAtom {
        atom: String,
        #[serde(default)]
        element: Option<String>,
        #[serde(default)]
        query: Option<AtomQueryDto>,
        #[serde(default)]
        charge: Option<i64>,
        #[serde(default, deserialize_with = "present")]
        isotope: Option<Option<u64>>,
        #[serde(default)]
        hydrogens: Option<HydrogensDto>,
        #[serde(default)]
        radical: Option<RadicalDto>,
        #[serde(default)]
        attachment_points: Option<Vec<u64>>,
    },
    /// `instance` may be left out while the document has one scene instance.
    MoveAtoms {
        atoms: Vec<String>,
        delta: PointDto,
        #[serde(default)]
        instance: Option<String>,
    },
    UpdateBond {
        bond: String,
        order: BondOrderDto,
        /// With `order: "query"` only.
        #[serde(default)]
        query: Option<BondQueryDto>,
    },
    DeleteAtoms {
        atoms: Vec<String>,
    },
    DeleteBonds {
        bonds: Vec<String>,
    },
    Rename {
        title: String,
    },
    /// Copies the positions of `source` (default: the first instance) moved by `offset`.
    AddInstance {
        #[serde(default)]
        source: Option<String>,
        offset: PointDto,
    },
    MoveInstance {
        instance: String,
        offset: PointDto,
    },
    DeleteInstance {
        instance: String,
    },
}

/// Reads an object into a map, refusing a repeated key instead of keeping the last value.
fn distinct_keys<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<BTreeMap<String, PointDto>>, D::Error> {
    struct Distinct;

    impl<'de> Visitor<'de> for Distinct {
        type Value = BTreeMap<String, PointDto>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("an object of points keyed by scene instance")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut points = BTreeMap::new();
            while let Some((key, point)) = map.next_entry::<String, PointDto>()? {
                if points.contains_key(&key) {
                    return Err(de::Error::custom(format!("duplicate key {key:?}")));
                }
                points.insert(key, point);
            }
            Ok(points)
        }
    }

    deserializer.deserialize_map(Distinct).map(Some)
}

/// Distinguishes an explicit `null` (`Some(None)`) from an absent field (`None`).
fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Option<u64>>, D::Error> {
    Option::<u64>::deserialize(deserializer).map(Some)
}

/// Why a wire command could not become a domain command.
struct Refusal {
    message: String,
    reason: Option<CommandReason>,
}

impl Refusal {
    fn new(reason: CommandReason, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            reason: Some(reason),
        }
    }
}

impl From<String> for Refusal {
    fn from(message: String) -> Self {
        Self {
            message,
            reason: None,
        }
    }
}

impl From<EditError> for Refusal {
    fn from(error: EditError) -> Self {
        Self {
            message: error.to_string(),
            reason: CommandReason::of(&error),
        }
    }
}

/// Converts wire commands to domain commands, reporting the first malformed field.
pub(crate) fn to_commands(commands: &[CommandDto]) -> Result<Vec<Command>, ApplicationError> {
    commands
        .iter()
        .enumerate()
        .map(|(index, command)| {
            to_command(command).map_err(|refusal| ApplicationError {
                command_index: Some(index),
                command_reason: refusal.reason,
                ..ApplicationError::new(
                    ErrorCode::InvalidCommand,
                    format!("command {index}: {}", refusal.message),
                )
            })
        })
        .collect()
}

fn to_command(command: &CommandDto) -> Result<Command, Refusal> {
    Ok(match command {
        CommandDto::AddAtom {
            element: symbol,
            query,
            position,
            positions,
            bond_to,
            hydrogens,
            radical,
            attachment_points,
        } => Command::AddAtom {
            atom: Atom {
                hydrogens: hydrogens.map(to_hydrogens).transpose()?.unwrap_or_default(),
                radical: radical.map(Radical::from).unwrap_or_default(),
                attachment_points: attachment_points
                    .as_deref()
                    .map(AttachmentPoints::try_new)
                    .transpose()?
                    .unwrap_or_default(),
                ..Atom::from(
                    atom_kind(symbol.as_deref(), query.as_ref())?.ok_or_else(|| {
                        Refusal::new(
                            CommandReason::AtomKindMissing,
                            "add_atom needs element or query",
                        )
                    })?,
                )
            },
            positions: match (position, positions) {
                (Some(position), None) => NewAtomPositions::Single(point(*position)?),
                (None, Some(positions)) => NewAtomPositions::PerInstance(
                    positions
                        .iter()
                        .map(|(id, position)| Ok((instance_id(id)?, point(*position)?)))
                        .collect::<Result<_, String>>()?,
                ),
                (Some(_), Some(_)) => {
                    return Err("give either position or positions, not both"
                        .to_owned()
                        .into());
                }
                (None, None) => {
                    return Err("add_atom needs position or positions".to_owned().into());
                }
            },
            bond_to: bond_to
                .as_ref()
                .map(|target| {
                    Ok::<_, Refusal>(BondTo {
                        atom: atom_ref(&target.atom)?,
                        order: bond_order(target.order, target.query.as_ref())?,
                    })
                })
                .transpose()?,
        },
        CommandDto::AddBond {
            begin,
            end,
            order,
            query,
        } => Command::AddBond {
            begin: atom_ref(begin)?,
            end: atom_ref(end)?,
            order: bond_order(*order, query.as_ref())?,
        },
        CommandDto::UpdateAtom {
            atom,
            element: symbol,
            query,
            charge,
            isotope,
            hydrogens,
            radical,
            attachment_points,
        } => Command::UpdateAtom {
            atom: atom_id(atom)?,
            update: AtomUpdate {
                kind: atom_kind(symbol.as_deref(), query.as_ref())?,
                // The domain checks the charge range; this only narrows the wire integer.
                charge: charge
                    .map(|charge| {
                        i8::try_from(charge).map_err(|_| {
                            format!("charge {charge} is outside {MIN_CHARGE}..={MAX_CHARGE}")
                        })
                    })
                    .transpose()?,
                isotope: isotope
                    .map(|isotope| {
                        isotope
                            .map(|mass| {
                                u16::try_from(mass)
                                    .map_err(|_| format!("isotope mass number {mass} is too large"))
                            })
                            .transpose()
                    })
                    .transpose()?,
                hydrogens: hydrogens.map(to_hydrogens).transpose()?,
                radical: radical.map(Radical::from),
                attachment_points: attachment_points
                    .as_deref()
                    .map(AttachmentPoints::try_new)
                    .transpose()?,
            },
        },
        CommandDto::MoveAtoms {
            atoms,
            delta,
            instance,
        } => Command::MoveAtoms {
            atoms: atoms
                .iter()
                .map(|id| atom_id(id))
                .collect::<Result<_, _>>()?,
            delta: vector(*delta),
            instance: instance.as_deref().map(instance_id).transpose()?,
        },
        CommandDto::UpdateBond { bond, order, query } => Command::UpdateBond {
            bond: bond_id(bond)?,
            order: bond_order(*order, query.as_ref())?,
        },
        CommandDto::DeleteAtoms { atoms } => Command::DeleteAtoms {
            atoms: atoms
                .iter()
                .map(|id| atom_id(id))
                .collect::<Result<_, _>>()?,
        },
        CommandDto::DeleteBonds { bonds } => Command::DeleteBonds {
            bonds: bonds
                .iter()
                .map(|id| bond_id(id))
                .collect::<Result<_, _>>()?,
        },
        CommandDto::Rename { title } => Command::Rename {
            title: title.clone(),
        },
        CommandDto::AddInstance { source, offset } => Command::AddInstance {
            source: source.as_deref().map(instance_id).transpose()?,
            offset: vector(*offset),
        },
        CommandDto::MoveInstance { instance, offset } => Command::MoveInstance {
            instance: instance_id(instance)?,
            offset: vector(*offset),
        },
        CommandDto::DeleteInstance { instance } => Command::DeleteInstance {
            instance: instance_id(instance)?,
        },
    })
}

/// The kind named by `element` or `query`; `None` when neither is given.
fn atom_kind(
    symbol: Option<&str>,
    query: Option<&AtomQueryDto>,
) -> Result<Option<AtomKind>, Refusal> {
    match (symbol, query) {
        (Some(_), Some(_)) => Err(Refusal::new(
            CommandReason::ElementAndQuery,
            "give either element or query, not both",
        )),
        (Some(symbol), None) => Ok(Some(AtomKind::Element(element(symbol)?))),
        (None, Some(query)) => Ok(Some(AtomKind::Query(atom_query(query)?))),
        (None, None) => Ok(None),
    }
}

fn atom_query(query: &AtomQueryDto) -> Result<AtomQuery, Refusal> {
    let simple = match query.kind {
        AtomQueryKind::Any => AtomQuery::Any,
        AtomQueryKind::AnyHeavy => AtomQuery::AnyHeavy,
        AtomQueryKind::Heteroatom => AtomQuery::Heteroatom,
        AtomQueryKind::Halogen => AtomQuery::Halogen,
        AtomQueryKind::List => {
            // A list without elements is an empty list, which the domain refuses.
            let elements = query
                .elements
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|symbol| element(symbol))
                .collect::<Result<_, _>>()?;
            return Ok(AtomQuery::List {
                elements,
                negated: query.negated.unwrap_or(false),
            });
        }
    };
    if query.elements.is_some() || query.negated.is_some() {
        return Err(Refusal::new(
            CommandReason::QueryListUnexpected,
            "elements and negated belong to a query of kind list",
        ));
    }
    Ok(simple)
}

fn to_hydrogens(hydrogens: HydrogensDto) -> Result<Hydrogens, Refusal> {
    match (hydrogens.mode, hydrogens.count) {
        (HydrogenMode::Specified, Some(count)) => u8::try_from(count)
            .ok()
            .filter(|count| *count <= MAX_HYDROGEN_COUNT)
            .map(Hydrogens::Specified)
            .ok_or_else(|| EditError::HydrogenCountOutOfRange(count).into()),
        (HydrogenMode::Specified, None) => Err(Refusal::new(
            CommandReason::HydrogenCountMissing,
            "specified hydrogens need a count",
        )),
        (HydrogenMode::Inferred | HydrogenMode::Unknown, Some(_)) => Err(Refusal::new(
            CommandReason::HydrogenCountUnexpected,
            "only specified hydrogens have a count",
        )),
        (HydrogenMode::Inferred, None) => Ok(Hydrogens::Inferred),
        (HydrogenMode::Unknown, None) => Ok(Hydrogens::Unknown),
    }
}

fn bond_order(order: BondOrderDto, query: Option<&BondQueryDto>) -> Result<BondOrder, Refusal> {
    let simple = match order {
        BondOrderDto::Single => BondOrder::Single,
        BondOrderDto::Double => BondOrder::Double,
        BondOrderDto::Triple => BondOrder::Triple,
        BondOrderDto::Aromatic => BondOrder::Aromatic,
        BondOrderDto::Dative => BondOrder::Dative,
        BondOrderDto::Unknown => BondOrder::Unknown,
        BondOrderDto::Query => {
            let query = query.ok_or_else(|| {
                Refusal::new(
                    CommandReason::BondQueryMissing,
                    "a query bond needs a query expression",
                )
            })?;
            return Ok(BondOrder::Query(match query.kind {
                QueryListKind::Any if query.orders.is_some() => {
                    return Err(Refusal::new(
                        CommandReason::QueryListUnexpected,
                        "orders belong to a query of kind list",
                    ));
                }
                QueryListKind::Any => BondQuery::Any,
                // A list without orders is an empty list, which the domain refuses.
                QueryListKind::List => BondQuery::List(
                    query
                        .orders
                        .as_deref()
                        .unwrap_or_default()
                        .iter()
                        .map(|order| QueryBondOrder::from(*order))
                        .collect(),
                ),
            }));
        }
    };
    if query.is_some() {
        return Err(Refusal::new(
            CommandReason::BondQueryUnexpected,
            "a query expression needs order query",
        ));
    }
    Ok(simple)
}

impl From<QueryOrderDto> for QueryBondOrder {
    fn from(order: QueryOrderDto) -> Self {
        match order {
            QueryOrderDto::Single => Self::Single,
            QueryOrderDto::Double => Self::Double,
            QueryOrderDto::Triple => Self::Triple,
            QueryOrderDto::Aromatic => Self::Aromatic,
        }
    }
}

impl From<QueryBondOrder> for QueryOrderDto {
    fn from(order: QueryBondOrder) -> Self {
        match order {
            QueryBondOrder::Single => Self::Single,
            QueryBondOrder::Double => Self::Double,
            QueryBondOrder::Triple => Self::Triple,
            QueryBondOrder::Aromatic => Self::Aromatic,
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
        let (mode, count) = match hydrogens {
            Hydrogens::Inferred => (HydrogenMode::Inferred, None),
            Hydrogens::Unknown => (HydrogenMode::Unknown, None),
            Hydrogens::Specified(count) => (HydrogenMode::Specified, Some(count.into())),
        };
        Self { mode, count }
    }
}

impl From<&AtomQuery> for AtomQueryDto {
    fn from(query: &AtomQuery) -> Self {
        let (kind, list) = match query {
            AtomQuery::Any => (AtomQueryKind::Any, None),
            AtomQuery::AnyHeavy => (AtomQueryKind::AnyHeavy, None),
            AtomQuery::Heteroatom => (AtomQueryKind::Heteroatom, None),
            AtomQuery::Halogen => (AtomQueryKind::Halogen, None),
            AtomQuery::List { elements, negated } => {
                (AtomQueryKind::List, Some((elements, *negated)))
            }
        };
        Self {
            kind,
            elements: list.map(|(elements, _)| {
                elements
                    .iter()
                    .map(|element| element.symbol().to_owned())
                    .collect()
            }),
            negated: list.map(|(_, negated)| negated),
        }
    }
}

/// The wire order and, for a query bond, its expression.
fn order_view(order: &BondOrder) -> (BondOrderDto, Option<BondQueryDto>) {
    let order = match order {
        BondOrder::Single => BondOrderDto::Single,
        BondOrder::Double => BondOrderDto::Double,
        BondOrder::Triple => BondOrderDto::Triple,
        BondOrder::Aromatic => BondOrderDto::Aromatic,
        BondOrder::Dative => BondOrderDto::Dative,
        BondOrder::Unknown => BondOrderDto::Unknown,
        BondOrder::Query(BondQuery::Any) => {
            let query = BondQueryDto {
                kind: QueryListKind::Any,
                orders: None,
            };
            return (BondOrderDto::Query, Some(query));
        }
        BondOrder::Query(BondQuery::List(orders)) => {
            let query = BondQueryDto {
                kind: QueryListKind::List,
                orders: Some(orders.iter().map(|order| (*order).into()).collect()),
            };
            return (BondOrderDto::Query, Some(query));
        }
    };
    (order, None)
}

fn atom_view(id: AtomId, atom: &Atom, position: Point) -> AtomView {
    let (kind, element, query) = match &atom.kind {
        AtomKind::Element(element) => (AtomKindName::Element, element.symbol().to_owned(), None),
        AtomKind::Query(query) => (
            AtomKindName::Query,
            query.label(),
            Some(AtomQueryDto::from(query)),
        ),
    };
    AtomView {
        id: id.to_string(),
        element,
        charge: atom.charge,
        isotope: atom.isotope,
        position: PositionView {
            x: position.x(),
            y: position.y(),
        },
        kind,
        query,
        hydrogens: atom.hydrogens.into(),
        radical: atom.radical.into(),
        attachment_points: atom.attachment_points.iter().collect(),
    }
}

fn bond_view(id: BondId, bond: &Bond) -> BondView {
    let (order, query) = order_view(&bond.order);
    BondView {
        id: id.to_string(),
        begin: bond.begin.to_string(),
        end: bond.end.to_string(),
        order,
        query,
    }
}

/// The domain checks the translated positions.
fn vector(delta: PointDto) -> Vector {
    Vector {
        dx: delta.x,
        dy: delta.y,
    }
}

fn instance_id(value: &str) -> Result<InstanceId, String> {
    InstanceId::parse(value).ok_or_else(|| format!("{value:?} is not a scene instance identifier"))
}

fn element(symbol: &str) -> Result<Element, String> {
    Element::from_symbol(symbol).ok_or_else(|| format!("{symbol:?} is not an element symbol"))
}

fn point(position: PointDto) -> Result<Point, String> {
    Point::new(position.x, position.y).map_err(|error| error.to_string())
}

fn atom_id(value: &str) -> Result<AtomId, String> {
    AtomId::parse(value).ok_or_else(|| format!("{value:?} is not an atom identifier"))
}

fn atom_ref(value: &str) -> Result<AtomRef, String> {
    match value.strip_prefix('$') {
        Some(index) if !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()) => index
            .parse()
            .map(AtomRef::Created)
            .map_err(|_| format!("{value:?} is not a command reference")),
        Some(_) => Err(format!("{value:?} is not a command reference")),
        None => atom_id(value).map(AtomRef::Id),
    }
}

fn bond_id(value: &str) -> Result<BondId, String> {
    BondId::parse(value).ok_or_else(|| format!("{value:?} is not a bond identifier"))
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DocumentResponse {
    pub api_version: &'static str,
    pub document: DocumentView,
    pub session: SessionView,
    /// A recovery draft left by an earlier session of this document, if any.
    pub recovery: Option<RecoveryView>,
}

/// An abandoned recovery draft: unsaved work of a session whose process ended.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RecoveryView {
    pub state: RecoveryState,
    /// The draft's revision, in the numbering of the process that wrote it.
    pub revision: Option<u64>,
    /// The stored revision the draft's session started from.
    pub base_revision: Option<u64>,
    /// Milliseconds since the Unix epoch.
    pub saved_at_ms: Option<u64>,
    /// Why the draft cannot be recovered in place, when it cannot.
    pub message: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryState {
    /// Storage still holds the draft's base: it can be recovered in place.
    Recoverable,
    /// Another writer saved since the draft's base: it can be recovered as a copy.
    Outdated,
    /// The draft cannot be read; it can only be discarded.
    Unreadable,
    /// Another running process owns it.
    InUse,
}

/// `atoms` and `bonds` are the chemical graph, `molecule`; `scene` places it on the page.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DocumentView {
    pub id: String,
    pub title: String,
    pub revision: u64,
    pub atoms: Vec<AtomView>,
    pub bonds: Vec<BondView>,
    pub molecule: MoleculeView,
    pub scene: SceneView,
}

/// Positions are scene points; x grows right and y grows down.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AtomView {
    pub id: String,
    /// The element symbol, or for a query atom (`kind: "query"`) its label (`*`, `A`, `Q`, `X`,
    /// `[N,O]`, `![N,O]`), which is never an element symbol.
    pub element: String,
    pub charge: i8,
    pub isotope: Option<u16>,
    /// The atom's position in the first scene instance, kept for v1 clients.
    pub position: PositionView,
    pub kind: AtomKindName,
    /// The query, for a query atom only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<AtomQueryDto>,
    /// `inferred` means not computed: no hydrogen count is reported for it.
    pub hydrogens: HydrogensDto,
    pub radical: RadicalDto,
    /// In increasing order.
    pub attachment_points: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MoleculeView {
    pub id: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SceneView {
    pub pages: Vec<PageView>,
    /// In order; the first one's positions are `atoms[].position`.
    pub instances: Vec<InstanceView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PageView {
    pub id: &'static str,
}

/// A molecule scene instance: the molecule drawn on a page, one position per atom.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InstanceView {
    pub id: String,
    pub page: &'static str,
    pub molecule: &'static str,
    pub positions: Vec<AtomPositionView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AtomPositionView {
    pub atom: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct PositionView {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BondView {
    pub id: String,
    pub begin: String,
    pub end: String,
    pub order: BondOrderDto,
    /// The expression of a query bond (`order: "query"`) only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<BondQueryDto>,
}

/// State of the in-memory editing session that holds the working copy.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SessionView {
    /// Revision last committed to storage by this session.
    pub saved_revision: u64,
    pub unsaved_changes: bool,
    pub undo_depth: usize,
    pub redo_depth: usize,
    /// The revision this session's recovery draft holds, or `null` when it has none.
    pub draft_revision: Option<u64>,
    /// The service's latest failed attempt to draft unsaved work this draft does not hold,
    /// while it keeps retrying; `null` otherwise.
    pub draft_failure: Option<DraftFailureView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DraftFailureView {
    /// Failed attempts in a row.
    pub attempts: u32,
    /// Milliseconds until the service tries again.
    pub retry_in_ms: u64,
    /// The latest attempt's error code, as in the v1 error envelope.
    pub code: &'static str,
    /// With `storage_failed`, why storage failed, as in the v1 error envelope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    pub message: String,
}

impl DocumentResponse {
    pub(crate) fn new(
        document: &Document,
        saved_revision: u64,
        draft_revision: Option<u64>,
    ) -> Self {
        let first = document.first_instance();
        Self {
            api_version: API_VERSION,
            document: DocumentView {
                id: document.id().to_string(),
                title: document.title().to_owned(),
                revision: document.revision(),
                atoms: document
                    .atoms()
                    .map(|(id, atom)| {
                        let position = document
                            .position(first, id)
                            .expect("every atom has a position in every scene instance");
                        atom_view(id, atom, position)
                    })
                    .collect(),
                bonds: document
                    .bonds()
                    .map(|(id, bond)| bond_view(id, bond))
                    .collect(),
                molecule: MoleculeView { id: MOLECULE_ID },
                scene: SceneView {
                    pages: vec![PageView { id: PAGE_ID }],
                    instances: document
                        .instances()
                        .map(|instance| InstanceView {
                            id: instance.to_string(),
                            page: PAGE_ID,
                            molecule: MOLECULE_ID,
                            positions: document
                                .positions(instance)
                                .map(|(atom, point)| AtomPositionView {
                                    atom: atom.to_string(),
                                    x: point.x(),
                                    y: point.y(),
                                })
                                .collect(),
                        })
                        .collect(),
                },
            },
            session: SessionView {
                saved_revision,
                unsaved_changes: saved_revision != document.revision(),
                undo_depth: document.undo_depth(),
                redo_depth: document.redo_depth(),
                draft_revision,
                draft_failure: None,
            },
            recovery: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DocumentListResponse {
    pub api_version: &'static str,
    pub documents: Vec<DocumentSummary>,
    /// Stored entries that exist but could not be read as documents.
    pub unreadable: Vec<UnreadableDocument>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DocumentSummary {
    pub id: String,
    pub title: String,
    pub revision: u64,
    pub atom_count: usize,
    pub bond_count: usize,
    pub open: bool,
    pub unsaved_changes: bool,
    pub recovery: Option<RecoveryView>,
    /// When storage last committed the document, in milliseconds since the Unix epoch.
    pub saved_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UnreadableDocument {
    pub id: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CloseResponse {
    pub api_version: &'static str,
    pub id: String,
    /// True when the session had unsaved changes and they were discarded.
    pub discarded_changes: bool,
}

/// A document saved as a new, independent document.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CopyResponse {
    /// The copy, now open in its own session.
    #[serde(flatten)]
    pub copy: DocumentResponse,
    pub source: CopySource,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CopySource {
    pub id: String,
    /// True when the source's session was closed: its working copy lives on in the copy, and
    /// the source reopens from its last save. A recovered draft leaves its source's session open
    /// unless, with every session slot in use, that clean session was the only one that could
    /// make room for the copy.
    pub closed: bool,
    /// True when that session had unsaved changes, which are now only in the copy.
    pub discarded_changes: bool,
}

/// A document moved to the trash.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DeleteResponse {
    pub api_version: &'static str,
    pub id: String,
    pub title: String,
}

/// A trashed document moved back to the stored documents.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RestoreResponse {
    pub api_version: &'static str,
    pub id: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TrashListResponse {
    pub api_version: &'static str,
    /// Most recently trashed first.
    pub documents: Vec<TrashedSummary>,
    /// Trashed entries that exist but could not be read as documents.
    pub unreadable: Vec<UnreadableDocument>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TrashedSummary {
    pub id: String,
    pub title: String,
    /// The stored revision it was trashed at.
    pub revision: u64,
    pub atom_count: usize,
    pub bond_count: usize,
    /// Milliseconds since the Unix epoch, when known.
    pub deleted_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DraftDiscardResponse {
    pub api_version: &'static str,
    pub id: String,
}
