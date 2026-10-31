//! v1 document request and response shapes shared by HTTP and the CLI.
//! The browser mirror lives in `apps/web/src/lib/document-contract.ts`.

use std::collections::BTreeMap;
use std::fmt;

use chemifree_domain::document::{
    Atom, AtomEdit, AtomId, AtomRef, BondId, BondOrder, BondTo, Command, Document, MAX_CHARGE,
    MIN_CHARGE, MOLECULE_ID, NewAtomPositions, ObjectId, PAGE_ID, Point, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{
    DeleteMode, Graphic, GraphicShape, ObjectRef, ReferenceTarget, SceneError, SceneObject,
    SceneReference, Selection, SelectionRequest, TransactionSelection,
};
use chemifree_domain::settings::{CarbonLabels, DisplayUnit, LabelFont, Settings, SettingsEdit};
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
    #[serde(default)]
    pub selection_before: Option<SelectionDto>,
    #[serde(default)]
    pub selection_after: Option<SelectionDto>,
}

/// A transaction's selection snapshots. Object IDs in `after` may also use `$N` to name an
/// object created by command N in the same transaction.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionDto {
    #[serde(default)]
    pub objects: Vec<String>,
    #[serde(default)]
    pub atoms: Vec<String>,
    #[serde(default)]
    pub bonds: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TransactionSelections {
    pub before: Option<SelectionDto>,
    pub after: Option<SelectionDto>,
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

/// The generic font family of atom labels.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LabelFontDto {
    SansSerif,
    Serif,
    Monospace,
}

/// Which carbons are labelled: `default` leaves a bonded carbon that records nothing bare.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CarbonLabelsDto {
    Default,
    All,
}

/// The unit lengths are shown in; stored lengths are always pt.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayUnitDto {
    Pt,
    Mm,
    Cm,
    In,
}

/// A document's settings, every field present; lengths in pt. Added within v1.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsDto {
    pub bond_length_pt: f64,
    pub line_width_pt: f64,
    pub label_font: LabelFontDto,
    pub label_size_pt: f64,
    pub carbon_labels: CarbonLabelsDto,
    pub display_unit: DisplayUnitDto,
}

/// Settings fields to change, or (in preferences) the fields new documents start from; omitted
/// fields keep their values (in preferences: the built-in defaults).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsEditDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bond_length_pt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_width_pt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_font: Option<LabelFontDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_size_pt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carbon_labels: Option<CarbonLabelsDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_unit: Option<DisplayUnitDto>,
}

/// Where a reset takes a document's settings from.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SettingsSource {
    /// The built-in defaults.
    Defaults,
    /// The settings a new document would start from now: the defaults overlaid with the
    /// preferences' `new_documents`.
    Preferences,
}

/// Body of `settings/reset`. Added within v1.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetSettingsRequest {
    pub request_id: String,
    pub expected_revision: u64,
    pub to: SettingsSource,
    #[serde(default)]
    pub selection_before: Option<SelectionDto>,
    #[serde(default)]
    pub selection_after: Option<SelectionDto>,
}

/// Body of `settings/import`: `source` names the document whose settings are applied. Added
/// within v1.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportSettingsRequest {
    pub request_id: String,
    pub expected_revision: u64,
    pub source: String,
    #[serde(default)]
    pub selection_before: Option<SelectionDto>,
    #[serde(default)]
    pub selection_after: Option<SelectionDto>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphicShapeDto {
    Line,
    Rectangle,
    Arrow,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DeleteModeDto {
    Subtree,
    ObjectOnly,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReferenceTargetDto {
    Object { id: String },
    Atom { id: String },
    Bond { id: String },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceDto {
    pub source: String,
    pub target: ReferenceTargetDto,
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
    AddGraphic {
        shape: GraphicShapeDto,
        points: [PointDto; 2],
        #[serde(default)]
        parent: Option<String>,
        #[serde(default)]
        index: Option<usize>,
    },
    Group {
        objects: Vec<String>,
    },
    Ungroup {
        group: String,
    },
    Reorder {
        object: String,
        index: usize,
    },
    DeleteObjects {
        objects: Vec<String>,
        #[serde(default)]
        mode: Option<DeleteModeDto>,
    },
    MoveObjects {
        objects: Vec<String>,
        delta: PointDto,
    },
    AddReference {
        source: String,
        target: ReferenceTargetDto,
    },
    RemoveReference {
        source: String,
        target: ReferenceTargetDto,
    },
    UpdateSettings {
        #[serde(default)]
        bond_length_pt: Option<f64>,
        #[serde(default)]
        line_width_pt: Option<f64>,
        #[serde(default)]
        label_font: Option<LabelFontDto>,
        #[serde(default)]
        label_size_pt: Option<f64>,
        #[serde(default)]
        carbon_labels: Option<CarbonLabelsDto>,
        #[serde(default)]
        display_unit: Option<DisplayUnitDto>,
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

/// Why a command could not be converted: a message, and the refused combination when there
/// is one.
struct Refusal {
    message: String,
    state: Option<StateError>,
    scene: Option<SceneError>,
}

impl From<String> for Refusal {
    fn from(message: String) -> Self {
        Self {
            message,
            state: None,
            scene: None,
        }
    }
}

impl From<StateError> for Refusal {
    fn from(state: StateError) -> Self {
        Self {
            message: state.to_string(),
            state: Some(state),
            scene: None,
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
                invalid_state: refusal.state,
                invalid_scene: refusal.scene,
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
            points,
            parent,
            index,
        } => Command::AddGraphic {
            graphic: Graphic {
                shape: match shape {
                    GraphicShapeDto::Line => GraphicShape::Line,
                    GraphicShapeDto::Rectangle => GraphicShape::Rectangle,
                    GraphicShapeDto::Arrow => GraphicShape::Arrow,
                },
                points: [point(points[0])?, point(points[1])?],
            },
            parent: parent.as_deref().map(instance_id).transpose()?,
            index: *index,
        },
        CommandDto::Group { objects } => Command::Group {
            objects: objects
                .iter()
                .map(|id| instance_id(id))
                .collect::<Result<_, _>>()?,
        },
        CommandDto::Ungroup { group } => Command::Ungroup {
            group: instance_id(group)?,
        },
        CommandDto::Reorder { object, index } => Command::Reorder {
            object: instance_id(object)?,
            index: *index,
        },
        CommandDto::DeleteObjects { objects, mode } => Command::DeleteObjects {
            objects: objects
                .iter()
                .map(|id| instance_id(id))
                .collect::<Result<_, _>>()?,
            mode: match mode.ok_or_else(|| Refusal {
                message: "delete_objects requires a mode".to_owned(),
                state: None,
                scene: Some(SceneError::DeleteModeMissing),
            })? {
                DeleteModeDto::Subtree => DeleteMode::Subtree,
                DeleteModeDto::ObjectOnly => DeleteMode::ObjectOnly,
            },
        },
        CommandDto::MoveObjects { objects, delta } => Command::MoveObjects {
            objects: objects
                .iter()
                .map(|id| instance_id(id))
                .collect::<Result<_, _>>()?,
            delta: vector(*delta),
        },
        CommandDto::AddReference { source, target } => Command::AddReference {
            reference: reference(source, target)?,
        },
        CommandDto::RemoveReference { source, target } => Command::RemoveReference {
            reference: reference(source, target)?,
        },
        CommandDto::UpdateSettings {
            bond_length_pt,
            line_width_pt,
            label_font,
            label_size_pt,
            carbon_labels,
            display_unit,
        } => Command::UpdateSettings(to_settings_edit(&SettingsEditDto {
            bond_length_pt: *bond_length_pt,
            line_width_pt: *line_width_pt,
            label_font: *label_font,
            label_size_pt: *label_size_pt,
            carbon_labels: *carbon_labels,
            display_unit: *display_unit,
        })),
    })
}

/// The domain checks the values' ranges.
pub(crate) fn to_settings_edit(edit: &SettingsEditDto) -> SettingsEdit {
    SettingsEdit {
        bond_length_pt: edit.bond_length_pt,
        line_width_pt: edit.line_width_pt,
        label_font: edit.label_font.map(|font| match font {
            LabelFontDto::SansSerif => LabelFont::SansSerif,
            LabelFontDto::Serif => LabelFont::Serif,
            LabelFontDto::Monospace => LabelFont::Monospace,
        }),
        label_size_pt: edit.label_size_pt,
        carbon_labels: edit.carbon_labels.map(|labels| match labels {
            CarbonLabelsDto::Default => CarbonLabels::Default,
            CarbonLabelsDto::All => CarbonLabels::All,
        }),
        display_unit: edit.display_unit.map(|unit| match unit {
            DisplayUnitDto::Pt => DisplayUnit::Pt,
            DisplayUnitDto::Mm => DisplayUnit::Mm,
            DisplayUnitDto::Cm => DisplayUnit::Cm,
            DisplayUnitDto::In => DisplayUnit::In,
        }),
    }
}

pub(crate) fn settings_edit_view(edit: &SettingsEdit) -> SettingsEditDto {
    SettingsEditDto {
        bond_length_pt: edit.bond_length_pt,
        line_width_pt: edit.line_width_pt,
        label_font: edit.label_font.map(font_view),
        label_size_pt: edit.label_size_pt,
        carbon_labels: edit.carbon_labels.map(carbon_labels_view),
        display_unit: edit.display_unit.map(unit_view),
    }
}

pub(crate) fn settings_view(settings: &Settings) -> SettingsDto {
    SettingsDto {
        bond_length_pt: settings.bond_length_pt,
        line_width_pt: settings.line_width_pt,
        label_font: font_view(settings.label_font),
        label_size_pt: settings.label_size_pt,
        carbon_labels: carbon_labels_view(settings.carbon_labels),
        display_unit: unit_view(settings.display_unit),
    }
}

fn font_view(font: LabelFont) -> LabelFontDto {
    match font {
        LabelFont::SansSerif => LabelFontDto::SansSerif,
        LabelFont::Serif => LabelFontDto::Serif,
        LabelFont::Monospace => LabelFontDto::Monospace,
    }
}

fn carbon_labels_view(labels: CarbonLabels) -> CarbonLabelsDto {
    match labels {
        CarbonLabels::Default => CarbonLabelsDto::Default,
        CarbonLabels::All => CarbonLabelsDto::All,
    }
}

fn unit_view(unit: DisplayUnit) -> DisplayUnitDto {
    match unit {
        DisplayUnit::Pt => DisplayUnitDto::Pt,
        DisplayUnit::Mm => DisplayUnitDto::Mm,
        DisplayUnit::Cm => DisplayUnitDto::Cm,
        DisplayUnit::In => DisplayUnitDto::In,
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

fn reference(source: &str, target: &ReferenceTargetDto) -> Result<SceneReference, String> {
    let target = match target {
        ReferenceTargetDto::Object { id } => ReferenceTarget::Object(instance_id(id)?),
        ReferenceTargetDto::Atom { id } => ReferenceTarget::Atom(atom_id(id)?),
        ReferenceTargetDto::Bond { id } => ReferenceTarget::Bond(bond_id(id)?),
    };
    Ok(SceneReference {
        source: instance_id(source)?,
        target,
    })
}

fn reference_view(reference: SceneReference) -> ReferenceDto {
    let target = match reference.target {
        ReferenceTarget::Object(id) => ReferenceTargetDto::Object { id: id.to_string() },
        ReferenceTarget::Atom(id) => ReferenceTargetDto::Atom { id: id.to_string() },
        ReferenceTarget::Bond(id) => ReferenceTargetDto::Bond { id: id.to_string() },
    };
    ReferenceDto {
        source: reference.source.to_string(),
        target,
    }
}

pub(crate) fn to_selection(
    selection: &TransactionSelections,
) -> Result<TransactionSelection, ApplicationError> {
    fn before(value: &SelectionDto) -> Result<Selection, String> {
        Ok(Selection {
            objects: value
                .objects
                .iter()
                .map(|id| instance_id(id))
                .collect::<Result<_, _>>()?,
            atoms: value
                .atoms
                .iter()
                .map(|id| atom_id(id))
                .collect::<Result<_, _>>()?,
            bonds: value
                .bonds
                .iter()
                .map(|id| bond_id(id))
                .collect::<Result<_, _>>()?,
        })
    }
    fn after(value: &SelectionDto) -> Result<SelectionRequest, String> {
        Ok(SelectionRequest {
            objects: value
                .objects
                .iter()
                .map(|id| object_ref(id))
                .collect::<Result<_, _>>()?,
            atoms: value
                .atoms
                .iter()
                .map(|id| atom_id(id))
                .collect::<Result<_, _>>()?,
            bonds: value
                .bonds
                .iter()
                .map(|id| bond_id(id))
                .collect::<Result<_, _>>()?,
        })
    }
    let result = || -> Result<TransactionSelection, String> {
        Ok(TransactionSelection {
            before: selection.before.as_ref().map(before).transpose()?,
            after: selection.after.as_ref().map(after).transpose()?,
        })
    };
    result().map_err(|message| ApplicationError {
        invalid_scene: Some(SceneError::SelectionUnknown),
        ..ApplicationError::new(
            ErrorCode::InvalidCommand,
            format!("invalid selection: {message}"),
        )
    })
}

fn object_ref(value: &str) -> Result<ObjectRef, String> {
    match value.strip_prefix('$') {
        Some(index) if !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()) => index
            .parse()
            .map(ObjectRef::Created)
            .map_err(|_| format!("{value:?} is not a command reference")),
        Some(_) => Err(format!("{value:?} is not a command reference")),
        None => instance_id(value).map(ObjectRef::Id),
    }
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
    pub selection: SelectionDto,
    pub settings: SettingsDto,
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
    /// `{"mode": "inferred"}` stores no count; analysis can derive one separately.
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
    /// In order; the first one's positions are `atoms[].position`.
    pub instances: Vec<InstanceView>,
    /// Page roots and all objects in depth-first drawing order.
    pub roots: Vec<String>,
    pub objects: Vec<SceneObjectView>,
    pub references: Vec<ReferenceDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SceneObjectView {
    Instance {
        id: String,
        parent: Option<String>,
        molecule: &'static str,
    },
    Group {
        id: String,
        parent: Option<String>,
        children: Vec<String>,
    },
    Graphic {
        id: String,
        parent: Option<String>,
        shape: GraphicShapeDto,
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
                    roots: document.roots().iter().map(ToString::to_string).collect(),
                    objects: document
                        .scene_objects()
                        .into_iter()
                        .map(|(id, parent, object)| {
                            let id = id.to_string();
                            let parent = parent.map(|parent| parent.to_string());
                            match object {
                                SceneObject::Instance => SceneObjectView::Instance {
                                    id,
                                    parent,
                                    molecule: MOLECULE_ID,
                                },
                                SceneObject::Group { children } => SceneObjectView::Group {
                                    id,
                                    parent,
                                    children: children.iter().map(ToString::to_string).collect(),
                                },
                                SceneObject::Graphic(graphic) => SceneObjectView::Graphic {
                                    id,
                                    parent,
                                    shape: match graphic.shape {
                                        GraphicShape::Line => GraphicShapeDto::Line,
                                        GraphicShape::Rectangle => GraphicShapeDto::Rectangle,
                                        GraphicShape::Arrow => GraphicShapeDto::Arrow,
                                    },
                                    points: graphic.points.map(|point| PositionView {
                                        x: point.x(),
                                        y: point.y(),
                                    }),
                                },
                            }
                        })
                        .collect(),
                    references: document.references().map(reference_view).collect(),
                },
                selection: SelectionDto {
                    objects: document
                        .selection()
                        .objects
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                    atoms: document
                        .selection()
                        .atoms
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                    bonds: document
                        .selection()
                        .bonds
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                },
                settings: settings_view(document.settings()),
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
    /// True when the source was open when the copy was made and is no longer open when the
    /// reply is recorded. A recovered-draft copy only evicts a clean source as a last resort;
    /// another request may also evict it while draft removal is in progress.
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
