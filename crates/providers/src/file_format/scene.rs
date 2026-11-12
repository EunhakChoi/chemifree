//! The version 30 shapes: document, history, scene objects, references, selection, groups and
//! the fragment-display, display-label and Et maps.

use serde::{Deserialize, Serialize};

use super::records::{
    AtomState, BondState, Keyed, PositionChange, Stored, StoredChange, StoredMolecule, StoredPage,
    StoredPoint, StoredStereoState,
};
use super::required_option;
use super::settings::{StoredFont, StoredSettings};

/// Current v30 scene/history: groups, styled/aligned captions, and Et display maps.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SceneFile {
    pub(super) format: String,
    pub(super) format_version: u32,
    pub(super) document: SceneDocument,
    pub(super) history: SceneHistory,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SceneDraftFile {
    pub(super) format: String,
    pub(super) format_version: u32,
    pub(super) base_revision: u64,
    pub(super) saved_at_ms: u64,
    pub(super) document: SceneDocument,
    pub(super) history: SceneHistory,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SceneDocument {
    #[serde(default)]
    pub(super) fragment_displays: Vec<StoredFragmentDisplay>,
    #[serde(default)]
    pub(super) abbreviations: Vec<StoredAbbreviation>,
    #[serde(default)]
    pub(super) display_labels: Vec<StoredDisplayLabel>,
    pub(super) id: String,
    pub(super) revision: u64,
    pub(super) title: String,
    pub(super) settings: StoredSettings,
    pub(super) molecule: StoredMolecule<AtomState, BondState>,
    pub(super) pages: Vec<StoredPage>,
    pub(super) roots: Vec<String>,
    pub(super) objects: Vec<Stored<SceneState>>,
    pub(super) references: Vec<StoredReference>,
    pub(super) positions: Vec<ScenePosition>,
    /// Stated configurations; the id is an atom's (a center) or a bond's (a double bond).
    pub(super) stereo: Vec<Stored<StoredStereoState>>,
    /// Enhanced-stereo groups in identifier order; the id is `e<serial>`.
    pub(super) stereo_groups: Vec<Stored<StoredGroup>>,
    pub(super) selection: StoredSelection,
    pub(super) next_atom: u32,
    pub(super) next_bond: u32,
    pub(super) next_object: u32,
    pub(super) next_group: u32,
}

/// An enhanced-stereo group: `number` is the number as stated (`null` when none), members are
/// atom identifiers, written in ascending order and read in any order like every list in the
/// file (a member listed twice is refused).
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredGroup {
    pub(super) kind: StoredGroupKind,
    #[serde(deserialize_with = "required_option")]
    pub(super) number: Option<i32>,
    pub(super) members: Vec<String>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StoredGroupKind {
    Absolute,
    And,
    Or,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum SceneState {
    Caption {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alignment: Option<StoredCaptionAlignment>,
        anchor: StoredPoint,
        runs: Vec<StoredCaptionRun>,
    },
    Instance {
        molecule: String,
    },
    Group {
        children: Vec<String>,
    },
    Image {
        mime_type: String,
        data_base64: String,
        origin: StoredPoint,
        size_pt: StoredPoint,
    },
    Graphic {
        shape: StoredShape,
        points: Vec<StoredPoint>,
        #[serde(default)]
        line_width_pt: Option<f64>,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredCaptionRun {
    #[serde(default)]
    pub(super) shadow: bool,
    #[serde(default)]
    pub(super) outline: bool,
    #[serde(default)]
    pub(super) underline: bool,
    #[serde(default)]
    pub(super) color: Option<String>,
    #[serde(default)]
    pub(super) size_pt: Option<f64>,
    #[serde(default)]
    pub(super) font: Option<StoredFont>,
    pub(super) text: String,
    pub(super) script: StoredCaptionScript,
    #[serde(default)]
    pub(super) style: Option<StoredCaptionStyle>,
    #[serde(default)]
    pub(super) weight: Option<StoredCaptionWeight>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StoredCaptionAlignment {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StoredCaptionWeight {
    Normal,
    Bold,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StoredCaptionStyle {
    Normal,
    Italic,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StoredCaptionScript {
    Baseline,
    Subscript,
    Superscript,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StoredShape {
    Line,
    Rectangle,
    Ellipse,
    Arrow,
    Polygon,
    Spline,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ScenePosition {
    pub(super) instance: String,
    pub(super) atom: String,
    pub(super) x: f64,
    pub(super) y: f64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredSelection {
    pub(super) objects: Vec<String>,
    pub(super) atoms: Vec<String>,
    pub(super) bonds: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredReference {
    pub(super) source: String,
    pub(super) target: StoredReferenceTarget,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum StoredReferenceTarget {
    Object { id: String },
    Atom { id: String },
    Bond { id: String },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredReferenceChange {
    pub(super) reference: StoredReference,
    pub(super) before: bool,
    pub(super) after: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SceneHistory {
    pub(super) undo: Vec<SceneChangeSet>,
    pub(super) redo: Vec<SceneChangeSet>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SceneChangeSet {
    #[serde(default)]
    pub(super) fragment_displays: Vec<StoredFragmentDisplayChange>,
    #[serde(default)]
    pub(super) abbreviations: Vec<StoredAbbreviationChange>,
    #[serde(default)]
    pub(super) display_labels: Vec<StoredDisplayLabelChange>,
    pub(super) title: Option<StoredChange<String>>,
    #[serde(deserialize_with = "required_option")]
    pub(super) settings: Option<StoredChange<StoredSettings>>,
    pub(super) atoms: Vec<Keyed<AtomState>>,
    pub(super) bonds: Vec<Keyed<BondState>>,
    pub(super) roots: Option<StoredChange<Vec<String>>>,
    pub(super) objects: Vec<Keyed<SceneState>>,
    pub(super) references: Vec<StoredReferenceChange>,
    pub(super) positions: Vec<PositionChange>,
    pub(super) stereo: Vec<Keyed<StoredStereoState>>,
    pub(super) stereo_groups: Vec<Keyed<StoredGroup>>,
    pub(super) selection: StoredChange<StoredSelection>,
}

/// Graph membership is explicit. Display text has no chemical interpretation.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredFragmentDisplay {
    pub(super) instance: String,
    pub(super) anchor: String,
    pub(super) members: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) runs: Option<Vec<StoredFragmentDisplayRun>>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredFragmentDisplayState {
    pub(super) members: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) runs: Option<Vec<StoredFragmentDisplayRun>>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredFragmentDisplayRun {
    pub(super) text: String,
    pub(super) script: StoredCaptionScript,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredFragmentDisplayChange {
    pub(super) instance: String,
    pub(super) anchor: String,
    #[serde(deserialize_with = "required_option")]
    pub(super) before: Option<StoredFragmentDisplayState>,
    #[serde(deserialize_with = "required_option")]
    pub(super) after: Option<StoredFragmentDisplayState>,
}

/// Cosmetic strings are separate from the existing graph membership history.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredDisplayLabel {
    pub(super) instance: String,
    pub(super) anchor: String,
    pub(super) text: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredDisplayLabelChange {
    pub(super) instance: String,
    pub(super) anchor: String,
    #[serde(deserialize_with = "required_option")]
    pub(super) before: Option<String>,
    #[serde(deserialize_with = "required_option")]
    pub(super) after: Option<String>,
}

/// The only supported abbreviation is Et; IDs continue to name real graph atoms.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredAbbreviation {
    pub(super) instance: String,
    pub(super) anchor: String,
    pub(super) terminal: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredAbbreviationChange {
    pub(super) instance: String,
    pub(super) anchor: String,
    #[serde(deserialize_with = "required_option")]
    pub(super) before: Option<String>,
    #[serde(deserialize_with = "required_option")]
    pub(super) after: Option<String>,
}
