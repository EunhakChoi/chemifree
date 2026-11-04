//! v1 document request and response shapes shared by HTTP and the CLI.
//! The browser mirror lives in `apps/web/src/lib/document-contract.ts`.

use std::collections::BTreeMap;
use std::fmt;

use chemifree_domain::document::{
    Atom, AtomEdit, AtomId, AtomRef, BondId, BondOrder, BondRef, BondTo, Command, Document,
    GroupId, GroupRef, MAX_CHARGE, MIN_CHARGE, MOLECULE_ID, NewAtomPositions, ObjectId, PAGE_ID,
    Point, Vector,
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
use chemifree_domain::stereo::{
    AtomConfiguration, BondConfiguration, BondEnd, CenterState, DoubleBondState, Relation, Slot,
    SlotRef, StereoError, StereoMark, StereoRecord, Turn,
};
use chemifree_domain::stereo_group::GroupKind;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::API_VERSION;
use crate::error::{ApplicationError, ErrorCode};
use crate::operations::Loss;

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
    /// Sets how a bond is drawn, or with `"mark": null` clears it. `mark` is required. `bond`
    /// is a bond ID, or `"$N"` for the bond created by command N: an `add_bond`'s bond, or the
    /// `bond_to` bond of an `add_atom`. Added within v1.
    SetBondMark {
        bond: String,
        #[serde(deserialize_with = "nullable")]
        mark: Option<MarkDto>,
    },
    /// States a stereocenter's neighbor order and turn (or states it unknown), or with
    /// `"configuration": null` clears the statement. `configuration` is required. Neighbors
    /// are atom IDs, `"$N"` for an atom created by command N, `"implicit_hydrogen"` or
    /// `"lone_pair"`, in any order. Added within v1.
    SetAtomConfiguration {
        atom: String,
        #[serde(deserialize_with = "nullable")]
        configuration: Option<AtomConfigurationDto>,
    },
    /// States how one neighbor of each end of a double bond lies across it (or states it
    /// unknown), or with `"configuration": null` clears the statement. `configuration` is
    /// required; `bond` is as in `set_bond_mark`. Added within v1.
    SetBondConfiguration {
        bond: String,
        #[serde(deserialize_with = "nullable")]
        configuration: Option<BondConfigurationDto>,
    },
    /// Stores the configuration the bond marks and one scene instance's positions show. With
    /// neither list, for each center a wedge, hash or wavy mark bears on and each crossed
    /// double bond; with `atoms` and/or `bonds` (IDs or `"$N"`, each non-empty), exactly those,
    /// all of which must derive. `instance` may be left out while the document has one scene
    /// instance. Added within v1.
    SetConfigurationFromDrawing {
        #[serde(default)]
        atoms: Option<Vec<String>>,
        #[serde(default)]
        bonds: Option<Vec<String>>,
        #[serde(default)]
        instance: Option<String>,
    },
    /// Creates an enhanced-stereo group of stored tetrahedral centers that are in no group
    /// (`atoms`: atom IDs or `"$N"`, non-empty). `number` is required, an integer within the
    /// 32-bit signed range or `null`; an `absolute` group takes none. The new group is `"$N"`
    /// for later commands. Added within v1.
    CreateStereoGroup {
        kind: StereoGroupKindDto,
        atoms: Vec<String>,
        #[serde(deserialize_with = "nullable")]
        number: Option<i64>,
    },
    /// Deletes a group (`group`: a group ID or `"$N"`); its members stay configured and become
    /// ungrouped. Added within v1.
    DeleteStereoGroup {
        group: String,
    },
    /// Changes a group's kind. Added within v1.
    SetStereoGroupKind {
        group: String,
        kind: StereoGroupKindDto,
    },
    /// Sets the stored number, or with `"number": null` clears it. `number` is required. A
    /// number another group of the kind has is refused (merge the groups instead). Added
    /// within v1.
    SetStereoGroupNumber {
        group: String,
        #[serde(deserialize_with = "nullable")]
        number: Option<i64>,
    },
    /// Adds stored tetrahedral centers that are in no group; members already in it are left as
    /// they are. Added within v1.
    AddStereoGroupMembers {
        group: String,
        atoms: Vec<String>,
    },
    /// Removes members, which become ungrouped; it never empties the group. Added within v1.
    RemoveStereoGroupMembers {
        group: String,
        atoms: Vec<String>,
    },
    /// Moves every member of `from` into `into` and deletes `from`; `into` keeps its kind and
    /// number. Added within v1.
    MergeStereoGroups {
        into: String,
        from: String,
    },
    /// Moves some, not all, members of an `and` or `or` group into a new group of the same
    /// kind with `number` (required, an integer or `null`). The new group is `"$N"` for later
    /// commands. Added within v1.
    SplitStereoGroup {
        group: String,
        atoms: Vec<String>,
        #[serde(deserialize_with = "nullable")]
        number: Option<i64>,
    },
}

/// What an enhanced-stereo group states. `absolute` has no number.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StereoGroupKindDto {
    Absolute,
    And,
    Or,
}

impl From<StereoGroupKindDto> for GroupKind {
    fn from(kind: StereoGroupKindDto) -> Self {
        match kind {
            StereoGroupKindDto::Absolute => Self::Absolute,
            StereoGroupKindDto::And => Self::And,
            StereoGroupKindDto::Or => Self::Or,
        }
    }
}

impl From<GroupKind> for StereoGroupKindDto {
    fn from(kind: GroupKind) -> Self {
        match kind {
            GroupKind::Absolute => Self::Absolute,
            GroupKind::And => Self::And,
            GroupKind::Or => Self::Or,
        }
    }
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

/// A required field whose value may be `null`: unlike a plain `Option` field, an absent key is
/// an error, so a command cannot clear something by leaving a key out.
fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(deserializer)
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
    /// The code of a stereo group value the wire refuses (`stereo_group_number_range`).
    group: Option<&'static str>,
}

impl From<String> for Refusal {
    fn from(message: String) -> Self {
        Self {
            message,
            state: None,
            scene: None,
            group: None,
        }
    }
}

impl From<StateError> for Refusal {
    fn from(state: StateError) -> Self {
        Self {
            message: state.to_string(),
            state: Some(state),
            scene: None,
            group: None,
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
                invalid_stereo_group: refusal.group,
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
                group: None,
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
        CommandDto::SetBondMark { bond, mark } => Command::SetBondMark {
            bond: bond_ref(bond)?,
            mark: mark.as_ref().map(StereoMark::from),
        },
        CommandDto::SetAtomConfiguration {
            atom,
            configuration,
        } => Command::SetAtomConfiguration {
            atom: atom_ref(atom)?,
            configuration: configuration.as_ref().map(atom_configuration).transpose()?,
        },
        CommandDto::SetBondConfiguration {
            bond,
            configuration,
        } => Command::SetBondConfiguration {
            bond: bond_ref(bond)?,
            configuration: configuration.as_ref().map(bond_configuration).transpose()?,
        },
        CommandDto::SetConfigurationFromDrawing {
            atoms,
            bonds,
            instance,
        } => Command::SetConfigurationFromDrawing {
            atoms: atoms
                .as_ref()
                .map(|atoms| atoms.iter().map(|id| atom_ref(id)).collect())
                .transpose()?,
            bonds: bonds
                .as_ref()
                .map(|bonds| bonds.iter().map(|id| bond_ref(id)).collect())
                .transpose()?,
            instance: instance.as_deref().map(instance_id).transpose()?,
        },
        CommandDto::CreateStereoGroup {
            kind,
            atoms,
            number,
        } => Command::CreateStereoGroup {
            kind: (*kind).into(),
            number: group_number(*number)?,
            atoms: atom_refs(atoms)?,
        },
        CommandDto::DeleteStereoGroup { group } => Command::DeleteStereoGroup {
            group: group_ref(group)?,
        },
        CommandDto::SetStereoGroupKind { group, kind } => Command::SetStereoGroupKind {
            group: group_ref(group)?,
            kind: (*kind).into(),
        },
        CommandDto::SetStereoGroupNumber { group, number } => Command::SetStereoGroupNumber {
            group: group_ref(group)?,
            number: group_number(*number)?,
        },
        CommandDto::AddStereoGroupMembers { group, atoms } => Command::AddStereoGroupMembers {
            group: group_ref(group)?,
            atoms: atom_refs(atoms)?,
        },
        CommandDto::RemoveStereoGroupMembers { group, atoms } => {
            Command::RemoveStereoGroupMembers {
                group: group_ref(group)?,
                atoms: atom_refs(atoms)?,
            }
        }
        CommandDto::MergeStereoGroups { into, from } => Command::MergeStereoGroups {
            into: group_ref(into)?,
            from: group_ref(from)?,
        },
        CommandDto::SplitStereoGroup {
            group,
            atoms,
            number,
        } => Command::SplitStereoGroup {
            group: group_ref(group)?,
            atoms: atom_refs(atoms)?,
            number: group_number(*number)?,
        },
    })
}

/// The stored number of a group: any integer of the 32-bit signed range, `0` and negatives
/// included, or none.
fn group_number(number: Option<i64>) -> Result<Option<i32>, Refusal> {
    number
        .map(|value| {
            i32::try_from(value).map_err(|_| Refusal {
                message: format!(
                    "{value} is outside the stereo group number range {} to {}",
                    i32::MIN,
                    i32::MAX
                ),
                state: None,
                scene: None,
                group: Some(GROUP_NUMBER_RANGE),
            })
        })
        .transpose()
}

/// The `reason` of a stereo group number outside the 32-bit signed range. The domain never
/// sees such a number, so this is the wire's own code, listed beside the domain's.
pub const GROUP_NUMBER_RANGE: &str = "stereo_group_number_range";

fn atom_refs(values: &[String]) -> Result<Vec<AtomRef>, String> {
    values.iter().map(|id| atom_ref(id)).collect()
}

/// A group ID (`e1`), or `$N` for the group created by command N.
fn group_ref(value: &str) -> Result<GroupRef, String> {
    match value.strip_prefix('$') {
        Some(index) if !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()) => index
            .parse()
            .map(GroupRef::Created)
            .map_err(|_| format!("{value:?} is not a command reference")),
        Some(_) => Err(format!("{value:?} is not a command reference")),
        None => GroupId::parse(value)
            .map(GroupRef::Id)
            .ok_or_else(|| format!("{value:?} is not a stereo group identifier")),
    }
}

fn atom_configuration(dto: &AtomConfigurationDto) -> Result<AtomConfiguration, String> {
    Ok(match dto {
        AtomConfigurationDto::Tetrahedral { neighbors, turn } => AtomConfiguration::Tetrahedral {
            neighbors: slot_refs(neighbors)?,
            turn: (*turn).into(),
        },
        AtomConfigurationDto::Unknown { neighbors } => AtomConfiguration::Unknown {
            neighbors: slot_refs(neighbors)?,
        },
    })
}

fn bond_configuration(dto: &BondConfigurationDto) -> Result<BondConfiguration, String> {
    Ok(match dto {
        BondConfigurationDto::Relative {
            begin_neighbor,
            end_neighbor,
            relation,
        } => BondConfiguration::Relative {
            begin_neighbor: slot_ref(begin_neighbor)?,
            end_neighbor: slot_ref(end_neighbor)?,
            relation: (*relation).into(),
        },
        BondConfigurationDto::Unknown {} => BondConfiguration::Unknown,
    })
}

/// A neighbor position as a command names it: `implicit_hydrogen`, `lone_pair`, or an atom
/// (`a3`, or `$N`). An atom identifier can never equal either word.
fn slot_ref(value: &str) -> Result<SlotRef, String> {
    match value {
        IMPLICIT_HYDROGEN => Ok(SlotRef::ImplicitHydrogen),
        LONE_PAIR => Ok(SlotRef::LonePair),
        _ => atom_ref(value).map(SlotRef::Atom),
    }
}

fn slot_refs(values: &[String; 4]) -> Result<[SlotRef; 4], String> {
    let mut slots = [SlotRef::ImplicitHydrogen; 4];
    for (slot, value) in slots.iter_mut().zip(values) {
        *slot = slot_ref(value)?;
    }
    Ok(slots)
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

/// A bond ID, or `$N` for the bond created by command N.
fn bond_ref(value: &str) -> Result<BondRef, String> {
    match value.strip_prefix('$') {
        Some(index) if !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()) => index
            .parse()
            .map(BondRef::Created)
            .map_err(|_| format!("{value:?} is not a command reference")),
        Some(_) => Err(format!("{value:?} is not a command reference")),
        None => bond_id(value).map(BondRef::Id),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DocumentResponse {
    pub api_version: &'static str,
    pub document: DocumentView,
    pub session: SessionView,
    /// A recovery draft left by an earlier session of this document, if any.
    pub recovery: Option<RecoveryView>,
    /// What this request cleared of stereo because the edit changed what it was about, as
    /// loss entries; an empty list for every reply but a transaction that cleared something.
    /// Added within v1; always present. It describes this request only: `get`, undo, redo and
    /// later replies never repeat it, and a replayed `request_id` returns it again.
    pub notes: Vec<Loss>,
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
    /// Stated stereo configurations. Added within v1; always present, with empty lists when no
    /// configuration is stated. No record means "unspecified": it says nothing about whether an
    /// atom or bond is stereogenic.
    pub stereo: StereoView,
}

impl DocumentView {
    /// Whether the drawing holds any stereo mark or stated configuration, of any kind
    /// (including wavy, crossed, and `unknown`). Operations that ignore stereo say so when this
    /// is true.
    pub fn holds_stereo(&self) -> bool {
        !self.stereo.atoms.is_empty()
            || !self.stereo.bonds.is_empty()
            || self.bonds.iter().any(|bond| bond.mark.is_some())
    }
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
    /// `{"mode": "inferred"}` has no stored count; inspection may derive one separately.
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
    /// How the bond is drawn; null for a plain bond. Added within v1. A drawing fact only: it
    /// states no configuration, and `null` means "plain", not "non-stereogenic".
    pub mark: Option<MarkDto>,
}

/// A bond mark, as snapshots show it and `set_bond_mark` states it. `begin` and `end` in a mark
/// are the bond's own `begin` and `end`; the bond is never rewritten to move the narrow end.
/// The public functional model's `wedge_begin` and `hash_end` display values name the same
/// facts as `wedge` and `hash` with `narrow`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MarkDto {
    /// Solid wedge on a single bond.
    Wedge { narrow: BondEndDto },
    /// Hashed wedge on a single bond.
    Hash { narrow: BondEndDto },
    /// Wavy single bond: unknown or either. No narrow end.
    Wavy {},
    /// Crossed double bond: unknown or either. No narrow end.
    Crossed {},
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BondEndDto {
    Begin,
    End,
}

/// The stated stereo configurations of a document: centers by atom id, then double bonds by
/// bond id, each with its slots in canonical order.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct StereoView {
    pub atoms: Vec<AtomStereoView>,
    pub bonds: Vec<BondStereoView>,
    /// Enhanced-stereo groups in identifier order. Added within v1; always present, empty when
    /// the document has none.
    pub groups: Vec<StereoGroupView>,
}

/// One enhanced-stereo group. `stored_number` is the number as it was stated (`null` when none;
/// `0` and negatives are kept), `number` the interpreted one (`null` for `absolute`, otherwise
/// the stored number or `0`), `label` the display text built from kind and interpreted number
/// (`abs`, `and2`, `or0`). `members` are atom IDs, ascending, each a stored tetrahedral center.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StereoGroupView {
    pub id: String,
    pub kind: StereoGroupKindDto,
    pub stored_number: Option<i32>,
    pub number: Option<i32>,
    pub label: String,
    pub members: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AtomStereoView {
    pub atom: String,
    pub configuration: AtomConfigurationDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BondStereoView {
    pub bond: String,
    pub configuration: BondConfigurationDto,
}

/// What is stated about a tetrahedral stereocenter. `neighbors` are four distinct slots in
/// canonical order: atom ids ascending, then `implicit_hydrogen`, then `lone_pair`. `turn` is
/// seen looking from the first slot toward the center: `anticlockwise` is OpenSMILES `@` (the
/// public model's positive `parity`), `clockwise` is `@@` (negative).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AtomConfigurationDto {
    Tetrahedral {
        neighbors: [String; 4],
        turn: TurnDto,
    },
    /// Stated unknown over this neighbor set.
    Unknown { neighbors: [String; 4] },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnDto {
    Anticlockwise,
    Clockwise,
}

/// What is stated about a double bond. The neighbors are in canonical form: the
/// lowest-identifier explicit neighbor of the end, or `implicit_hydrogen` where the end has no
/// other explicit neighbor. `opposite_sides` is the public model's `opposite_side`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BondConfigurationDto {
    Relative {
        begin_neighbor: String,
        end_neighbor: String,
        relation: RelationDto,
    },
    /// Stated unknown or either.
    Unknown {},
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationDto {
    SameSide,
    OppositeSides,
}

impl From<&MarkDto> for StereoMark {
    fn from(mark: &MarkDto) -> Self {
        let end = |narrow| match narrow {
            BondEndDto::Begin => BondEnd::Begin,
            BondEndDto::End => BondEnd::End,
        };
        match mark {
            MarkDto::Wedge { narrow } => Self::Wedge {
                narrow: end(*narrow),
            },
            MarkDto::Hash { narrow } => Self::Hash {
                narrow: end(*narrow),
            },
            MarkDto::Wavy {} => Self::Wavy,
            MarkDto::Crossed {} => Self::Crossed,
        }
    }
}

impl From<TurnDto> for Turn {
    fn from(turn: TurnDto) -> Self {
        match turn {
            TurnDto::Anticlockwise => Self::Anticlockwise,
            TurnDto::Clockwise => Self::Clockwise,
        }
    }
}

impl From<Turn> for TurnDto {
    fn from(turn: Turn) -> Self {
        match turn {
            Turn::Anticlockwise => Self::Anticlockwise,
            Turn::Clockwise => Self::Clockwise,
        }
    }
}

impl From<RelationDto> for Relation {
    fn from(relation: RelationDto) -> Self {
        match relation {
            RelationDto::SameSide => Self::SameSide,
            RelationDto::OppositeSides => Self::OppositeSides,
        }
    }
}

impl From<Relation> for RelationDto {
    fn from(relation: Relation) -> Self {
        match relation {
            Relation::SameSide => Self::SameSide,
            Relation::OppositeSides => Self::OppositeSides,
        }
    }
}

impl From<StereoMark> for MarkDto {
    fn from(mark: StereoMark) -> Self {
        let end = |narrow| match narrow {
            BondEnd::Begin => BondEndDto::Begin,
            BondEnd::End => BondEndDto::End,
        };
        match mark {
            StereoMark::Wedge { narrow } => Self::Wedge {
                narrow: end(narrow),
            },
            StereoMark::Hash { narrow } => Self::Hash {
                narrow: end(narrow),
            },
            StereoMark::Wavy => Self::Wavy {},
            StereoMark::Crossed => Self::Crossed {},
        }
    }
}

/// The wire spellings of the two slots that are not atoms. An atom id can never equal either.
const IMPLICIT_HYDROGEN: &str = "implicit_hydrogen";
const LONE_PAIR: &str = "lone_pair";

/// A slot as the wire spells it: an atom id, `implicit_hydrogen`, or `lone_pair`.
pub(crate) fn slot_name(slot: Slot) -> String {
    match slot {
        Slot::Atom(atom) => atom.to_string(),
        Slot::ImplicitHydrogen => IMPLICIT_HYDROGEN.to_owned(),
        Slot::LonePair => LONE_PAIR.to_owned(),
    }
}

/// A center state as snapshots and commands spell it.
pub fn center_dto(state: &CenterState) -> AtomConfigurationDto {
    match state {
        CenterState::Tetrahedral { slots, turn } => AtomConfigurationDto::Tetrahedral {
            neighbors: slots.map(slot_name),
            turn: (*turn).into(),
        },
        CenterState::Unknown { slots } => AtomConfigurationDto::Unknown {
            neighbors: slots.map(slot_name),
        },
    }
}

/// A double-bond state as snapshots and commands spell it.
pub fn double_bond_dto(state: &DoubleBondState) -> BondConfigurationDto {
    match state {
        DoubleBondState::Relative {
            begin_ref,
            end_ref,
            relation,
        } => BondConfigurationDto::Relative {
            begin_neighbor: slot_name(*begin_ref),
            end_neighbor: slot_name(*end_ref),
            relation: (*relation).into(),
        },
        DoubleBondState::Unknown => BondConfigurationDto::Unknown {},
    }
}

impl StereoView {
    /// The document's records as snapshots show them.
    ///
    /// # Errors
    ///
    /// [`StereoError::RecordInvalid`] when a record's kind does not match its key. A document
    /// never holds one (restore and every edit keep them equal), so the view refuses to be
    /// built rather than leave a stored record out of a snapshot.
    pub fn try_new(document: &Document) -> Result<Self, StereoError> {
        let mut view = Self::default();
        for record in document.stereo().typed()? {
            match record {
                StereoRecord::Center { atom, state } => view.atoms.push(AtomStereoView {
                    atom: atom.to_string(),
                    configuration: center_dto(state),
                }),
                StereoRecord::DoubleBond { bond, state } => view.bonds.push(BondStereoView {
                    bond: bond.to_string(),
                    configuration: double_bond_dto(state),
                }),
            }
        }
        view.groups = document
            .stereo_groups()
            .iter()
            .map(|(id, group)| StereoGroupView {
                id: id.to_string(),
                kind: group.kind().into(),
                stored_number: group.stored_number(),
                number: group.interpreted_number(),
                label: group.label(),
                members: group.members().iter().map(ToString::to_string).collect(),
            })
            .collect();
        Ok(view)
    }
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
                        mark: bond.mark.map(MarkDto::from),
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
                stereo: StereoView::try_new(document)
                    .expect("a document's stereo records are kept with the kind of their key"),
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
            notes: Vec::new(),
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
