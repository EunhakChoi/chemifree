//! Transaction requests and the wire command list.

use std::collections::BTreeMap;
use std::fmt;

use chemifree_domain::document::{RingPattern, RingSize};
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use super::chemistry::{BondOrderDto, BondQueryDto, BondToDto, HydrogensDto, QueryDto, RadicalDto};
use super::request::{PointDto, SelectionDto};
use super::scene::{
    ArrangementModeDto, CaptionAlignmentDto, CaptionRunDto, DeleteModeDto, FragmentDisplayRunDto,
    GraphicShapeDto, ReferenceTargetDto, SelectionTransformDto,
};
use super::settings::{
    CarbonLabelsDto, DisplayUnitDto, LabelFontDto, LabelStyleDto, LabelWeightDto,
};
use super::stereo::{AtomConfigurationDto, BondConfigurationDto, MarkDto, StereoGroupKindDto};

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

/// One command in a transaction, tagged by `op`. Identifiers use the `a1`/`b1`/`s1` form.
/// Where a new bond names an atom (`bond_to.atom`, `begin`, `end`), `"$N"` also refers to
/// the atom added by the `add_atom` command at index `N` earlier in the same transaction.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandDto {
    /// Independent regular carbon ring, optionally a six-carbon Kekulé recipe.
    InsertRing {
        instance: String,
        center: PointDto,
        #[serde(default, deserialize_with = "ring_size")]
        size: RingSize,
        #[serde(default, deserialize_with = "ring_pattern")]
        pattern: RingPattern,
    },
    /// A fresh regular carbon ring connected to an existing atom by one single bond.
    AttachRing {
        instance: String,
        atom: String,
        direction: RingDirectionDto,
        #[serde(default, deserialize_with = "ring_size")]
        size: RingSize,
        #[serde(default, deserialize_with = "ring_pattern")]
        pattern: RingPattern,
    },
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
    /// Moves the union of existing selected objects, atoms and bond endpoints once.
    /// The movement targets are independent of transaction selection_before/after.
    MoveSelection {
        selection: SelectionDto,
        delta: PointDto,
        #[serde(default)]
        instance: Option<String>,
    },
    /// Arranges existing outermost scene objects as rigid units; no partial atom/bond targets.
    ArrangeObjects {
        objects: Vec<String>,
        mode: ArrangementModeDto,
    },
    /// Transforms resolved geometry only; stored stereo and history selection are independent.
    TransformSelection {
        selection: SelectionDto,
        pivot: PointDto,
        transform: SelectionTransformDto,
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
    /// Explicit members retain their chemistry; text is presentation only.
    ContractFragment {
        instance: String,
        anchor: String,
        members: Vec<String>,
        text: String,
    },
    SetFragmentDisplayText {
        instance: String,
        anchor: String,
        text: String,
    },
    SetFragmentDisplayRuns {
        instance: String,
        anchor: String,
        runs: Vec<FragmentDisplayRunDto>,
    },
    ExpandFragment {
        instance: String,
        anchor: String,
    },
    CollapseEthyl {
        instance: String,
        anchor: String,
        terminal: String,
    },
    UpdateAbbreviationLabel {
        instance: String,
        anchor: String,
        #[serde(deserialize_with = "nullable")]
        text: Option<String>,
    },
    ExpandEthyl {
        instance: String,
        anchor: String,
    },
    AddCaption {
        anchor: PointDto,
        #[serde(default)]
        alignment: CaptionAlignmentDto,
        runs: Vec<CaptionRunDto>,
        #[serde(default)]
        parent: Option<String>,
        #[serde(default)]
        index: Option<usize>,
    },
    UpdateCaption {
        object: String,
        anchor: PointDto,
        #[serde(default)]
        alignment: CaptionAlignmentDto,
        runs: Vec<CaptionRunDto>,
    },
    AddImage {
        mime_type: String,
        data_base64: String,
        origin: PointDto,
        size_pt: PointDto,
        parent: Option<String>,
        index: Option<usize>,
    },
    UpdateImageGeometry {
        object: String,
        origin: PointDto,
        size_pt: PointDto,
    },
    AddGraphic {
        shape: GraphicShapeDto,
        points: [PointDto; 2],
        #[serde(default)]
        parent: Option<String>,
        #[serde(default)]
        index: Option<usize>,
    },
    AddSpline {
        points: [PointDto; 4],
        parent: Option<String>,
        index: Option<usize>,
    },
    UpdateSplinePoints {
        object: String,
        points: [PointDto; 4],
    },
    AddPolygon {
        vertices: Vec<PointDto>,
        parent: Option<String>,
        index: Option<usize>,
    },
    UpdatePolygonVertices {
        object: String,
        vertices: Vec<PointDto>,
    },
    UpdateGraphicPoints {
        object: String,
        points: [PointDto; 2],
    },
    SetGraphicLineWidth {
        object: String,
        #[serde(deserialize_with = "nullable")]
        line_width_pt: Option<f64>,
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
        label_weight: Option<LabelWeightDto>,
        #[serde(default)]
        label_style: Option<LabelStyleDto>,
        #[serde(default)]
        label_underline: Option<bool>,
        #[serde(default)]
        label_outline: Option<bool>,
        #[serde(default)]
        label_shadow: Option<bool>,
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

// Missing size keeps the original six-ring recipe; explicit null is malformed.
fn ring_size<'de, D: Deserializer<'de>>(deserializer: D) -> Result<RingSize, D::Error> {
    match u8::deserialize(deserializer)? {
        5 => Ok(RingSize::Five),
        6 => Ok(RingSize::Six),
        _ => Err(de::Error::custom("ring size must be 5 or 6")),
    }
}

// Omission preserves all-single; explicit null and unknown patterns are malformed.
fn ring_pattern<'de, D: Deserializer<'de>>(deserializer: D) -> Result<RingPattern, D::Error> {
    match String::deserialize(deserializer)?.as_str() {
        "single" => Ok(RingPattern::Single),
        "kekule" => Ok(RingPattern::Kekule),
        _ => Err(de::Error::custom("ring pattern must be single or kekule")),
    }
}

/// Finite nonzero attachment direction, normalized by the Domain; magnitude is ignored.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RingDirectionDto {
    pub dx: f64,
    pub dy: f64,
}
