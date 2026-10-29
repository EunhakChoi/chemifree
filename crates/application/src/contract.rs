//! v1 document request and response shapes shared by HTTP and the CLI.
//! The browser mirror lives in `apps/web/src/lib/document-contract.ts`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use chemifree_domain::document::{
    Atom, AtomEdit, AtomId, AtomRef, BondId, BondOrder, BondTo, Command, Document, MAX_CHARGE,
    MIN_CHARGE, MOLECULE_ID, NewAtomPositions, ObjectId, ObjectRef, PAGE_ID, Point, SelectionAfter,
    SelectionUpdate, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{DeleteMode, Graphic, Parent, SceneObject, Selection, Shape};
use chemifree_domain::states::{
    AtomKind, AttachmentPoints, BondQuery, Hydrogens, QueryAtom, QueryBondOrder, Radical,
    StateError,
};
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::API_VERSION;
use crate::error::{ApplicationError, ErrorCode};

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
    /// The client's selection after the edit, recorded with it in history. Added within v1.
    #[serde(default)]
    pub selection: Option<SelectionDto>,
    /// The client's selection before the edit, which undo restores; it must name only what
    /// the document holds before the edit. Added within v1.
    #[serde(default)]
    pub selection_before: Option<SelectionDto>,
}

impl TransactionRequest {
    /// The selections the request carries.
    pub fn selections(&self) -> TransactionSelections {
        TransactionSelections {
            before: self.selection_before.clone(),
            after: self.selection.clone(),
        }
    }
}

/// The selections a transaction may carry: the client's selection before the edit (undo
/// restores it) and after it (redo restores it). An omitted one keeps the default (the
/// selection the document holds before; the previous selection wherever it still exists
/// after), which differs from an empty one. Added within v1.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TransactionSelections {
    pub before: Option<SelectionDto>,
    pub after: Option<SelectionDto>,
}

/// Selected scene objects, atoms, and bonds. In a transaction it is the client's selection
/// after the edit and must name only what the edited document holds; in a document view it is
/// what the latest edit, undo, or redo left selected. Repeats count once. Added within v1.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionDto {
    pub objects: Vec<String>,
    pub atoms: Vec<String>,
    pub bonds: Vec<String>,
}

/// What a graphic draws between its two points.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeDto {
    Line,
    Rectangle,
    /// An arrowhead at the second point.
    Arrow,
}

/// What `delete_objects` removes besides the objects it names.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DeleteModeDto {
    /// Everything each object contains too.
    Subtree,
    /// The object alone; a group's children take its place.
    ObjectOnly,
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

/// A query atom: which elements it stands for. Nothing matches it.
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

/// What a query bond stands for. Nothing matches it.
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

/// An atom's hydrogen state. `inferred` (the default) means "to be derived from valence";
/// nothing derives it yet, so it has no count.
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

/// One command in a transaction, tagged by `op`. Identifiers use the `a1`/`b1`/`s1` form.
/// Where a new bond names an atom (`bond_to.atom`, `begin`, `end`), `"$N"` also refers to
/// the atom added by the `add_atom` command at index `N` earlier in the same transaction.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandDto {
    /// Exactly one of `position` (while the document has one scene instance) and `positions`
    /// (a point for each scene instance, keyed by instance identifier).
    ///
    /// The atom is exactly one of `element` and `query`. `hydrogens` (default inferred),
    /// `radical` (default none), and `attachment_points` (default none) were added within v1.
    AddAtom {
        #[serde(default)]
        element: Option<String>,
        #[serde(default)]
        query: Option<QueryDto>,
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
    /// `query` is required with order `query` and refused with any other.
    AddBond {
        begin: String,
        end: String,
        order: BondOrderDto,
        #[serde(default)]
        query: Option<BondQueryDto>,
    },
    /// Omitted fields are unchanged; `"isotope": null` restores natural abundance. At most one
    /// of `element` and `query`. The atom that results must be valid as a whole, so making a
    /// charged atom a query also sends `"charge": 0`.
    UpdateAtom {
        atom: String,
        #[serde(default)]
        element: Option<String>,
        #[serde(default)]
        query: Option<QueryDto>,
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
    /// `query` is required with order `query` and refused with any other.
    UpdateBond {
        bond: String,
        order: BondOrderDto,
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
    /// A line, rectangle, or arrow given by two points, added at the end of `parent` (a group)
    /// or of the page's root objects. This and the commands below were added within v1.
    AddGraphic {
        shape: ShapeDto,
        points: [PointDto; 2],
        #[serde(default)]
        parent: Option<String>,
    },
    /// Replaces objects that share a parent with a new group, where the first of them was.
    Group {
        objects: Vec<String>,
    },
    /// Puts the group's children in its place.
    Ungroup {
        group: String,
    },
    /// `mode` is required: the reply to a missing one is `invalid_command` with reason
    /// `delete_mode_missing`.
    DeleteObjects {
        objects: Vec<String>,
        #[serde(default)]
        mode: Option<DeleteModeDto>,
    },
    /// Moves every instance and graphic in the union of the objects' subtrees once.
    MoveObjects {
        objects: Vec<String>,
        offset: PointDto,
    },
    /// Moves an object to `index` among its parent's children.
    Reorder {
        object: String,
        index: usize,
    },
}

/// The reason code of a `delete_objects` command without `mode`.
pub const DELETE_MODE_MISSING: &str = "delete_mode_missing";

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

/// Why a command could not be converted: a message, and its stable code when it has one.
struct Refusal {
    message: String,
    reason: Option<&'static str>,
}

impl From<String> for Refusal {
    fn from(message: String) -> Self {
        Self {
            message,
            reason: None,
        }
    }
}

impl From<StateError> for Refusal {
    fn from(state: StateError) -> Self {
        Self {
            message: state.to_string(),
            reason: Some(state.code()),
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
            attachment_points: points,
        } => Command::AddAtom {
            atom: Atom {
                hydrogens: hydrogens.map(to_hydrogens).transpose()?.unwrap_or_default(),
                radical: radical.map(Radical::from).unwrap_or_default(),
                attachment_points: points
                    .as_deref()
                    .map(attachment_points)
                    .transpose()?
                    .unwrap_or_default(),
                ..Atom::new(
                    atom_kind(symbol.as_deref(), query.as_ref())?
                        .ok_or(StateError::AtomKindConflict)?,
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
            attachment_points: points,
        } => Command::UpdateAtom {
            atom: atom_id(atom)?,
            edit: AtomEdit {
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
                attachment_points: points.as_deref().map(attachment_points).transpose()?,
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
        CommandDto::AddGraphic {
            shape,
            points: [first, second],
            parent,
        } => Command::AddGraphic {
            graphic: Graphic {
                shape: match shape {
                    ShapeDto::Line => Shape::Line,
                    ShapeDto::Rectangle => Shape::Rectangle,
                    ShapeDto::Arrow => Shape::Arrow,
                },
                points: [point(*first)?, point(*second)?],
            },
            parent: parent.as_deref().map(object_id).transpose()?,
        },
        CommandDto::Group { objects } => Command::Group {
            objects: object_ids(objects)?,
        },
        CommandDto::Ungroup { group } => Command::Ungroup {
            group: object_id(group)?,
        },
        CommandDto::DeleteObjects { objects, mode } => Command::DeleteObjects {
            objects: object_ids(objects)?,
            mode: match mode {
                Some(DeleteModeDto::Subtree) => DeleteMode::Subtree,
                Some(DeleteModeDto::ObjectOnly) => DeleteMode::ObjectOnly,
                None => {
                    return Err(Refusal {
                        message: "delete_objects needs `mode`: \"subtree\" or \"object_only\""
                            .to_owned(),
                        reason: Some(DELETE_MODE_MISSING),
                    });
                }
            },
        },
        CommandDto::MoveObjects { objects, offset } => Command::MoveObjects {
            objects: object_ids(objects)?,
            offset: vector(*offset),
        },
        CommandDto::Reorder { object, index } => Command::Reorder {
            object: object_id(object)?,
            index: *index,
        },
    })
}

/// A transaction's selections as sets of identifiers. A malformed identifier makes the body
/// malformed, and the error names the request field.
pub(crate) fn to_selection_update(
    selections: &TransactionSelections,
) -> Result<SelectionUpdate, ApplicationError> {
    fn malformed(field: &'static str) -> impl Fn(String) -> ApplicationError {
        move |message| {
            ApplicationError::new(ErrorCode::InvalidRequest, format!("{field}: {message}"))
        }
    }
    Ok(SelectionUpdate {
        before: selections
            .before
            .as_ref()
            .map(|selection| selection_before(selection).map_err(malformed("selection_before")))
            .transpose()?,
        after: selections
            .after
            .as_ref()
            .map(|selection| selection_after(selection).map_err(malformed("selection")))
            .transpose()?,
    })
}

/// Nothing is created before the edit, so the selection before it names objects by identifier.
fn selection_before(selection: &SelectionDto) -> Result<Selection, String> {
    Ok(Selection {
        objects: ids(&selection.objects, |id| match command_reference(id) {
            Some(_) => Err(format!(
                "{id:?} names an object the edit creates, which does not exist before it"
            )),
            None => object_id(id),
        })?,
        atoms: ids(&selection.atoms, atom_id)?,
        bonds: ids(&selection.bonds, bond_id)?,
    })
}

/// The selection after the edit may name an object by the index of the command that creates
/// it (`$N`).
fn selection_after(selection: &SelectionDto) -> Result<SelectionAfter, String> {
    Ok(SelectionAfter {
        objects: ids(&selection.objects, object_ref)?,
        atoms: ids(&selection.atoms, atom_id)?,
        bonds: ids(&selection.bonds, bond_id)?,
    })
}

fn ids<T: Ord>(
    values: &[String],
    parse: impl Fn(&str) -> Result<T, String>,
) -> Result<BTreeSet<T>, String> {
    values.iter().map(|value| parse(value)).collect()
}

fn selection_view(selection: &Selection) -> SelectionDto {
    SelectionDto {
        objects: selection.objects.iter().map(ToString::to_string).collect(),
        atoms: selection.atoms.iter().map(ToString::to_string).collect(),
        bonds: selection.bonds.iter().map(ToString::to_string).collect(),
    }
}

/// The kind named by `element` or `query`, at most one of them; `None` when neither is given.
fn atom_kind(symbol: Option<&str>, query: Option<&QueryDto>) -> Result<Option<AtomKind>, Refusal> {
    Ok(match (symbol, query) {
        (Some(_), Some(_)) => return Err(StateError::AtomKindConflict.into()),
        (Some(symbol), None) => Some(AtomKind::Element(element(symbol)?)),
        (None, Some(query)) => Some(AtomKind::Query(query_atom(query)?)),
        (None, None) => None,
    })
}

/// The domain checks the list's size and repeats.
fn query_atom(query: &QueryDto) -> Result<QueryAtom, String> {
    Ok(match query {
        QueryDto::Any {} => QueryAtom::Any,
        QueryDto::AnyHeavy {} => QueryAtom::AnyHeavy,
        QueryDto::Heteroatom {} => QueryAtom::Heteroatom,
        QueryDto::Halogen {} => QueryAtom::Halogen,
        QueryDto::List { elements, negated } => QueryAtom::List {
            elements: elements
                .iter()
                .map(|symbol| element(symbol))
                .collect::<Result<_, _>>()?,
            negated: *negated,
        },
    })
}

/// A bond order with its query, which `query` and only `query` carries. The domain checks the
/// query's list.
fn bond_order(order: BondOrderDto, query: Option<&BondQueryDto>) -> Result<BondOrder, StateError> {
    Ok(match (order, query) {
        (BondOrderDto::Query, Some(BondQueryDto::Any {})) => BondOrder::Query(BondQuery::Any),
        (BondOrderDto::Query, Some(BondQueryDto::List { orders })) => BondOrder::Query(
            BondQuery::OneOf(orders.iter().map(|order| (*order).into()).collect()),
        ),
        (BondOrderDto::Query, None) => return Err(StateError::BondQueryMissing),
        (_, Some(_)) => return Err(StateError::BondQueryUnexpected),
        (BondOrderDto::Single, None) => BondOrder::Single,
        (BondOrderDto::Double, None) => BondOrder::Double,
        (BondOrderDto::Triple, None) => BondOrder::Triple,
        (BondOrderDto::Aromatic, None) => BondOrder::Aromatic,
        (BondOrderDto::Dative, None) => BondOrder::Dative,
        (BondOrderDto::Unknown, None) => BondOrder::Unknown,
    })
}

/// The domain checks the stated count's range; this only narrows the wire integer.
fn to_hydrogens(hydrogens: HydrogensDto) -> Result<Hydrogens, StateError> {
    Ok(match hydrogens {
        HydrogensDto::Inferred {} => Hydrogens::Inferred,
        HydrogensDto::Unknown {} => Hydrogens::Unknown,
        HydrogensDto::Specified { count } => Hydrogens::Specified(
            u8::try_from(count).map_err(|_| StateError::HydrogenCountOutOfRange)?,
        ),
    })
}

fn attachment_points(values: &[u64]) -> Result<AttachmentPoints, StateError> {
    let narrowed = values
        .iter()
        .map(|value| u8::try_from(*value))
        .collect::<Result<Vec<u8>, _>>()
        .map_err(|_| StateError::AttachmentPointsInvalid)?;
    AttachmentPoints::new(&narrowed)
}

/// The domain checks the translated positions.
fn vector(delta: PointDto) -> Vector {
    Vector {
        dx: delta.x,
        dy: delta.y,
    }
}

fn instance_id(value: &str) -> Result<ObjectId, String> {
    ObjectId::parse(value).ok_or_else(|| format!("{value:?} is not a scene instance identifier"))
}

fn object_id(value: &str) -> Result<ObjectId, String> {
    ObjectId::parse(value).ok_or_else(|| format!("{value:?} is not a scene object identifier"))
}

fn object_ids(values: &[String]) -> Result<Vec<ObjectId>, String> {
    values.iter().map(|value| object_id(value)).collect()
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

/// The command index `$N` names, or `None` when `value` does not start with `$`.
fn command_reference(value: &str) -> Option<Result<usize, String>> {
    let index = value.strip_prefix('$')?;
    let malformed = || format!("{value:?} is not a command reference");
    Some(
        if !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()) {
            index.parse().map_err(|_| malformed())
        } else {
            Err(malformed())
        },
    )
}

fn atom_ref(value: &str) -> Result<AtomRef, String> {
    match command_reference(value) {
        Some(index) => index.map(AtomRef::Created),
        None => atom_id(value).map(AtomRef::Id),
    }
}

fn object_ref(value: &str) -> Result<ObjectRef, String> {
    match command_reference(value) {
        Some(index) => index.map(ObjectRef::Created),
        None => object_id(value).map(ObjectRef::Id),
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
    /// What the latest edit, undo, or redo left selected. Added within v1.
    pub selection: SelectionDto,
}

/// Positions are scene points; x grows right and y grows down.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AtomView {
    pub id: String,
    /// The element symbol, or for a query atom its label (see [`query_label`]), which is not
    /// an element symbol: `kind` says which.
    pub element: String,
    pub charge: i8,
    pub isotope: Option<u16>,
    /// The atom's position in the first scene instance, kept for v1 clients.
    pub position: PositionView,
    /// `element` or `query`. This and the fields below were added within v1.
    pub kind: AtomKindView,
    /// The query a query atom stands for; null for an element.
    pub query: Option<QueryDto>,
    /// `{"mode": "inferred"}` has no count: nothing computes it.
    pub hydrogens: HydrogensDto,
    pub radical: RadicalDto,
    /// Ascending, 1-4; empty when there are none.
    pub attachment_points: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AtomKindView {
    Element,
    Query,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MoleculeView {
    pub id: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SceneView {
    pub pages: Vec<PageView>,
    /// The molecule scene instances in identifier (creation) order, wherever each is
    /// contained; the first one's positions are `atoms[].position`.
    pub instances: Vec<InstanceView>,
    /// Every scene object depth first from the page's roots, children in order, with its
    /// parent. Each molecule object is the instance of the same ID in `instances`, which holds
    /// its positions. Added within v1.
    pub objects: Vec<ObjectView>,
}

/// A scene object and where it is contained: `parent` is the page's ID or a group's.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObjectView {
    Molecule {
        id: String,
        parent: String,
        molecule: &'static str,
    },
    Group {
        id: String,
        parent: String,
        /// In order.
        children: Vec<String>,
    },
    Graphic {
        id: String,
        parent: String,
        shape: ShapeDto,
        points: [PositionView; 2],
    },
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
    /// The donor when `order` is `dative`.
    pub begin: String,
    pub end: String,
    pub order: BondOrderDto,
    /// What a bond of order `query` stands for; null otherwise. Added within v1.
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
                        let (kind, element, query) = match &atom.kind {
                            AtomKind::Element(element) => {
                                (AtomKindView::Element, element.symbol().to_owned(), None)
                            }
                            AtomKind::Query(query) => {
                                (AtomKindView::Query, query_label(query), Some(query.into()))
                            }
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
                            attachment_points: atom.attachment_points.values().collect(),
                        }
                    })
                    .collect(),
                bonds: document
                    .bonds()
                    .map(|(id, bond)| BondView {
                        id: id.to_string(),
                        begin: bond.begin.to_string(),
                        end: bond.end.to_string(),
                        order: (&bond.order).into(),
                        query: match &bond.order {
                            BondOrder::Query(query) => Some(query.into()),
                            _ => None,
                        },
                    })
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
                    objects: object_views(document),
                },
                selection: selection_view(document.selection()),
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

fn object_views(document: &Document) -> Vec<ObjectView> {
    document
        .objects_in_tree_order()
        .into_iter()
        .filter_map(|(id, parent)| {
            let parent = match parent {
                Parent::Page => PAGE_ID.to_owned(),
                Parent::Group(group) => group.to_string(),
            };
            Some(match document.object(id)? {
                SceneObject::Molecule => ObjectView::Molecule {
                    id: id.to_string(),
                    parent,
                    molecule: MOLECULE_ID,
                },
                SceneObject::Group(children) => ObjectView::Group {
                    id: id.to_string(),
                    parent,
                    children: children.iter().map(ToString::to_string).collect(),
                },
                SceneObject::Graphic(graphic) => ObjectView::Graphic {
                    id: id.to_string(),
                    parent,
                    shape: match graphic.shape {
                        Shape::Line => ShapeDto::Line,
                        Shape::Rectangle => ShapeDto::Rectangle,
                        Shape::Arrow => ShapeDto::Arrow,
                    },
                    points: graphic.points.map(|point| PositionView {
                        x: point.x(),
                        y: point.y(),
                    }),
                },
            })
        })
        .collect()
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
