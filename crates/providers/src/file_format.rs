//! Chemifree document file format, version 26 (`*.json`), and the recovery draft format that
//! wraps the same document and history. This is Chemifree's own working format, not an
//! interchange format such as MOL or CDXML.
//!
//! Version 26 adds nullable graphic line_width_pt; v1–25 migrate as inherited in current/history.
//! Version 25 stores fragment display runs; v24 text migrates to one baseline run in current/history.
//! Version 24 adds explicit fragment_displays; versions 1–23 migrate as empty, including history.
//! Version 23 adds cosmetic display_labels; versions 1–22 retain default Et, including history.
//! Version 22 adds label_shadow; versions 1–21 migrate as false, including history.
//! Version 21 adds label_outline; versions 1–20 migrate as false, including history.
//! Version 20 adds label_underline; versions 1–19 migrate as false, including history.
//! Version 19 adds document label_style; versions 1–18 migrate as normal, including history.
//! Version 18 adds document label_weight; versions 1–17 migrate as normal, including history.
//! Version 17 adds independent boolean run shadows; versions 8–16 default to false.
//! Version 16 adds independent boolean hollow glyph outlines; versions 8–15 default to false.
//! Version 15 adds independent boolean run underlines; versions 8–14 default to false.
//! Version 14 adds nullable canonical opaque sRGB run colors; versions 8–13 inherit foreground.
//! Version 13 adds a nullable per-run size override; versions 8–12 inherit the document size.
//! Version 12 adds a nullable per-run font override; versions 8–11 inherit the document font.
//! Version 11 adds explicit whole-caption alignment; versions 8–10 migrate as left.
//! Version 10 adds explicit caption run weight/style; versions 8–9 migrate them as normal.
//! Version 9 adds per-placement Et display metadata to version 8 captions. It stores the chemical graph
//! (`molecule`), the page's ordered scene tree, directed
//! references, positions, stated stereo configurations (`stereo`), enhanced-stereo groups
//! (`stereo_groups`), selection, and document settings. History records their changes together.
//! An atom state records its kind (an element or a query), charge, isotope, hydrogen state,
//! radical, and attachment points; a bond state its
//! order, for a query bond its query, and, for a bond drawn with a stereo mark, the mark (with
//! the narrow end of a wedge, so a bond's `begin` and `end` are never swapped). Files are
//! compact JSON, so the largest document a save allows, full history included, stays within the
//! read limit.
//!
//! Older versions are still read and migrated in memory, history included: version 2 atoms and
//! bonds get the defaults (an element atom, inferred hydrogens, no radical, no attachment
//! points), and version 1 files, which kept one position on each atom, also move those
//! positions into one page `p1` and one instance `s1`. Versions 2 and 3 reconstruct root
//! order and its history from their instances. Versions 1–4 receive the built-in settings.
//! Versions 1–5 have no stereo marks or configurations, and versions 1–6 have no stereo groups,
//! so they read with none. Reading never writes; the next write is version 26. Other versions
//! are refused.

use std::collections::BTreeSet;

use chemifree_application::{Draft, DraftInfo};
use chemifree_domain::document::{
    Atom, AtomId, Bond, BondId, BondOrder, Change, ChangeSet, Document, DocumentId, DocumentParts,
    FragmentDisplay, FragmentDisplayRun, GroupId, MOLECULE_ID, ObjectId, PAGE_ID, Point,
    PositionKey,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{
    Caption, CaptionAlignment, CaptionRun, CaptionScript, CaptionStyle, CaptionWeight, Graphic,
    GraphicShape, ReferenceTarget, SceneObject, SceneReference, Selection,
};
use chemifree_domain::settings::{
    CarbonLabels, DisplayUnit, LabelFont, LabelStyle, LabelWeight, Settings,
};
use chemifree_domain::states::{
    AtomKind, AttachmentPoints, BondQuery, Hydrogens, QueryAtom, QueryBondOrder, Radical,
};
use chemifree_domain::stereo::{
    BondEnd, CenterState, DoubleBondState, Relation, Slot, StereoKey, StereoMark, StereoState, Turn,
};
use chemifree_domain::stereo_group::{GroupKind, StereoGroup};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};

const FORMAT: &str = "chemifree.document";
const FORMAT_VERSION: u32 = 26;
const DRAFT_FORMAT: &str = "chemifree.draft";
const DRAFT_FORMAT_VERSION: u32 = 26;
/// The version before graph and scene were separated; read and migrated, never written.
const VERSION_1: u32 = 1;
/// The version before recorded hydrogen, radical, query, and attachment states; read and
/// migrated, never written.
const VERSION_2: u32 = 2;
const VERSION_3: u32 = 3;
const VERSION_4: u32 = 4;
/// The version before stereo marks and configurations; read and migrated, never written.
const VERSION_5: u32 = 5;
/// The version before stereo groups; read and migrated, never written.
const VERSION_6: u32 = 6;
/// The one instance a version 1 document becomes.
const MIGRATED_INSTANCE: u32 = 1;

/// Current v26 scene/history: groups, styled/aligned captions, and Et display maps.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SceneFile {
    format: String,
    format_version: u32,
    document: SceneDocument,
    history: SceneHistory,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SceneDraftFile {
    format: String,
    format_version: u32,
    base_revision: u64,
    saved_at_ms: u64,
    document: SceneDocument,
    history: SceneHistory,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SceneDocument {
    #[serde(default)]
    fragment_displays: Vec<StoredFragmentDisplay>,
    #[serde(default)]
    abbreviations: Vec<StoredAbbreviation>,
    #[serde(default)]
    display_labels: Vec<StoredDisplayLabel>,
    id: String,
    revision: u64,
    title: String,
    settings: StoredSettings,
    molecule: StoredMolecule<AtomState, BondState>,
    pages: Vec<StoredPage>,
    roots: Vec<String>,
    objects: Vec<Stored<SceneState>>,
    references: Vec<StoredReference>,
    positions: Vec<ScenePosition>,
    /// Stated configurations; the id is an atom's (a center) or a bond's (a double bond).
    stereo: Vec<Stored<StoredStereoState>>,
    /// Enhanced-stereo groups in identifier order; the id is `e<serial>`.
    stereo_groups: Vec<Stored<StoredGroup>>,
    selection: StoredSelection,
    next_atom: u32,
    next_bond: u32,
    next_object: u32,
    next_group: u32,
}

/// An enhanced-stereo group: `number` is the number as stated (`null` when none), members are
/// atom identifiers, written in ascending order and read in any order like every list in the
/// file (a member listed twice is refused).
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredGroup {
    kind: StoredGroupKind,
    #[serde(deserialize_with = "required_option")]
    number: Option<i32>,
    members: Vec<String>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StoredGroupKind {
    Absolute,
    And,
    Or,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum SceneState {
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
    Graphic {
        shape: StoredShape,
        points: [StoredPoint; 2],
        #[serde(default)]
        line_width_pt: Option<f64>,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredCaptionRun {
    #[serde(default)]
    shadow: bool,
    #[serde(default)]
    outline: bool,
    #[serde(default)]
    underline: bool,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    size_pt: Option<f64>,
    #[serde(default)]
    font: Option<StoredFont>,
    text: String,
    script: StoredCaptionScript,
    #[serde(default)]
    style: Option<StoredCaptionStyle>,
    #[serde(default)]
    weight: Option<StoredCaptionWeight>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StoredCaptionAlignment {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StoredCaptionWeight {
    Normal,
    Bold,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StoredCaptionStyle {
    Normal,
    Italic,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StoredCaptionScript {
    Baseline,
    Subscript,
    Superscript,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StoredShape {
    Line,
    Rectangle,
    Arrow,
}

/// A document's settings, every field present. Lengths are in pt.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredSettings {
    bond_length_pt: f64,
    line_width_pt: f64,
    label_font: StoredFont,
    #[serde(default)]
    label_weight: StoredLabelWeight,
    #[serde(default)]
    label_style: StoredLabelStyle,
    #[serde(default)]
    label_underline: bool,
    #[serde(default)]
    label_outline: bool,
    #[serde(default)]
    label_shadow: bool,
    label_size_pt: f64,
    carbon_labels: StoredCarbonLabels,
    display_unit: StoredUnit,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StoredLabelWeight {
    #[default]
    Normal,
    Bold,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StoredLabelStyle {
    #[default]
    Normal,
    Italic,
}

impl From<LabelWeight> for StoredLabelWeight {
    fn from(weight: LabelWeight) -> Self {
        match weight {
            LabelWeight::Normal => Self::Normal,
            LabelWeight::Bold => Self::Bold,
        }
    }
}

impl From<StoredLabelWeight> for LabelWeight {
    fn from(weight: StoredLabelWeight) -> Self {
        match weight {
            StoredLabelWeight::Normal => Self::Normal,
            StoredLabelWeight::Bold => Self::Bold,
        }
    }
}

impl From<LabelStyle> for StoredLabelStyle {
    fn from(style: LabelStyle) -> Self {
        match style {
            LabelStyle::Normal => Self::Normal,
            LabelStyle::Italic => Self::Italic,
        }
    }
}

impl From<StoredLabelStyle> for LabelStyle {
    fn from(style: StoredLabelStyle) -> Self {
        match style {
            StoredLabelStyle::Normal => Self::Normal,
            StoredLabelStyle::Italic => Self::Italic,
        }
    }
}

// The settings enums are shared with the preferences file, which stores the same values.
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum StoredFont {
    SansSerif,
    Serif,
    Monospace,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StoredCarbonLabels {
    Default,
    All,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StoredUnit {
    Pt,
    Mm,
    Cm,
    In,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ScenePosition {
    instance: String,
    atom: String,
    x: f64,
    y: f64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredSelection {
    objects: Vec<String>,
    atoms: Vec<String>,
    bonds: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredReference {
    source: String,
    target: StoredReferenceTarget,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum StoredReferenceTarget {
    Object { id: String },
    Atom { id: String },
    Bond { id: String },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredReferenceChange {
    reference: StoredReference,
    before: bool,
    after: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SceneHistory {
    undo: Vec<SceneChangeSet>,
    redo: Vec<SceneChangeSet>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SceneChangeSet {
    #[serde(default)]
    fragment_displays: Vec<StoredFragmentDisplayChange>,
    #[serde(default)]
    abbreviations: Vec<StoredAbbreviationChange>,
    #[serde(default)]
    display_labels: Vec<StoredDisplayLabelChange>,
    title: Option<StoredChange<String>>,
    #[serde(deserialize_with = "required_option")]
    settings: Option<StoredChange<StoredSettings>>,
    atoms: Vec<Keyed<AtomState>>,
    bonds: Vec<Keyed<BondState>>,
    roots: Option<StoredChange<Vec<String>>>,
    objects: Vec<Keyed<SceneState>>,
    references: Vec<StoredReferenceChange>,
    positions: Vec<PositionChange>,
    stereo: Vec<Keyed<StoredStereoState>>,
    stereo_groups: Vec<Keyed<StoredGroup>>,
    selection: StoredChange<StoredSelection>,
}

/// Graph membership is explicit. Display text has no chemical interpretation.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredFragmentDisplay {
    instance: String,
    anchor: String,
    members: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    runs: Option<Vec<StoredFragmentDisplayRun>>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredFragmentDisplayState {
    members: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    runs: Option<Vec<StoredFragmentDisplayRun>>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredFragmentDisplayRun {
    text: String,
    script: StoredCaptionScript,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredFragmentDisplayChange {
    instance: String,
    anchor: String,
    #[serde(deserialize_with = "required_option")]
    before: Option<StoredFragmentDisplayState>,
    #[serde(deserialize_with = "required_option")]
    after: Option<StoredFragmentDisplayState>,
}

fn stored_fragment_display(display: &FragmentDisplay) -> StoredFragmentDisplayState {
    StoredFragmentDisplayState {
        members: display.members.iter().map(ToString::to_string).collect(),
        text: None,
        runs: Some(
            display
                .runs
                .iter()
                .map(|run| StoredFragmentDisplayRun {
                    text: run.text.clone(),
                    script: match run.script {
                        CaptionScript::Baseline => StoredCaptionScript::Baseline,
                        CaptionScript::Subscript => StoredCaptionScript::Subscript,
                        CaptionScript::Superscript => StoredCaptionScript::Superscript,
                    },
                })
                .collect(),
        ),
    }
}

fn read_fragment_display(display: StoredFragmentDisplayState) -> Result<FragmentDisplay, String> {
    let mut members: Vec<AtomId> = display
        .members
        .iter()
        .map(|id| atom_id(id))
        .collect::<Result<_, _>>()?;
    // Lists may arrive in any order. Sorting preserves duplicates for domain rejection
    // and makes current/history sides compare by the same canonical representation.
    members.sort_unstable();
    match (display.text, display.runs) {
        (Some(text), None) => Ok(FragmentDisplay::plain(members, text)),
        (None, Some(runs)) => Ok(FragmentDisplay {
            members,
            runs: runs
                .into_iter()
                .map(|run| FragmentDisplayRun {
                    text: run.text,
                    script: match run.script {
                        StoredCaptionScript::Baseline => CaptionScript::Baseline,
                        StoredCaptionScript::Subscript => CaptionScript::Subscript,
                        StoredCaptionScript::Superscript => CaptionScript::Superscript,
                    },
                })
                .collect(),
        }),
        _ => Err(
            "Fragment display requires exactly one version-appropriate text representation.".into(),
        ),
    }
}

/// Cosmetic strings are separate from the existing graph membership history.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredDisplayLabel {
    instance: String,
    anchor: String,
    text: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredDisplayLabelChange {
    instance: String,
    anchor: String,
    #[serde(deserialize_with = "required_option")]
    before: Option<String>,
    #[serde(deserialize_with = "required_option")]
    after: Option<String>,
}

/// The only supported abbreviation is Et; IDs continue to name real graph atoms.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredAbbreviation {
    instance: String,
    anchor: String,
    terminal: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredAbbreviationChange {
    instance: String,
    anchor: String,
    #[serde(deserialize_with = "required_option")]
    before: Option<String>,
    #[serde(deserialize_with = "required_option")]
    after: Option<String>,
}

/// Version 6 is the scene format with stereo, before stereo groups. Its exact shape is kept
/// separately so a version 6 file cannot silently contain groups, and a version 7 file cannot
/// omit `stereo_groups` or `next_group`.
mod v6 {
    use serde::Deserialize;

    use super::{
        AtomState, BondState, Keyed, PositionChange, ScenePosition, SceneState, Stored,
        StoredChange, StoredMolecule, StoredPage, StoredReference, StoredReferenceChange,
        StoredSelection, StoredSettings, StoredStereoState, required_option,
    };

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct File {
        #[serde(rename = "format")]
        pub _format: String,
        #[serde(rename = "format_version")]
        pub _format_version: u32,
        pub document: Document,
        pub history: History,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Draft {
        #[serde(rename = "format")]
        pub _format: String,
        #[serde(rename = "format_version")]
        pub _format_version: u32,
        pub base_revision: u64,
        pub saved_at_ms: u64,
        pub document: Document,
        pub history: History,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Document {
        pub id: String,
        pub revision: u64,
        pub title: String,
        pub settings: StoredSettings,
        pub molecule: StoredMolecule<AtomState, BondState>,
        pub pages: Vec<StoredPage>,
        pub roots: Vec<String>,
        pub objects: Vec<Stored<SceneState>>,
        pub references: Vec<StoredReference>,
        pub positions: Vec<ScenePosition>,
        pub stereo: Vec<Stored<StoredStereoState>>,
        pub selection: StoredSelection,
        pub next_atom: u32,
        pub next_bond: u32,
        pub next_object: u32,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct History {
        pub undo: Vec<ChangeSet>,
        pub redo: Vec<ChangeSet>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct ChangeSet {
        pub title: Option<StoredChange<String>>,
        #[serde(deserialize_with = "required_option")]
        pub settings: Option<StoredChange<StoredSettings>>,
        pub atoms: Vec<Keyed<AtomState>>,
        pub bonds: Vec<Keyed<BondState>>,
        pub roots: Option<StoredChange<Vec<String>>>,
        pub objects: Vec<Keyed<SceneState>>,
        pub references: Vec<StoredReferenceChange>,
        pub positions: Vec<PositionChange>,
        pub stereo: Vec<Keyed<StoredStereoState>>,
        pub selection: StoredChange<StoredSelection>,
    }
}

impl From<v6::Document> for SceneDocument {
    fn from(value: v6::Document) -> Self {
        Self {
            fragment_displays: Vec::new(),
            abbreviations: Vec::new(),
            display_labels: Vec::new(),
            id: value.id,
            revision: value.revision,
            title: value.title,
            settings: value.settings,
            molecule: value.molecule,
            pages: value.pages,
            roots: value.roots,
            objects: value.objects,
            references: value.references,
            positions: value.positions,
            stereo: value.stereo,
            stereo_groups: Vec::new(),
            selection: value.selection,
            next_atom: value.next_atom,
            next_bond: value.next_bond,
            next_object: value.next_object,
            next_group: 1,
        }
    }
}

impl From<v6::History> for SceneHistory {
    fn from(value: v6::History) -> Self {
        Self {
            undo: value.undo.into_iter().map(Into::into).collect(),
            redo: value.redo.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<v6::ChangeSet> for SceneChangeSet {
    fn from(value: v6::ChangeSet) -> Self {
        Self {
            fragment_displays: Vec::new(),
            abbreviations: Vec::new(),
            display_labels: Vec::new(),
            title: value.title,
            settings: value.settings,
            atoms: value.atoms,
            bonds: value.bonds,
            roots: value.roots,
            objects: value.objects,
            references: value.references,
            positions: value.positions,
            stereo: value.stereo,
            stereo_groups: Vec::new(),
            selection: value.selection,
        }
    }
}

/// Version 5 is the scene format with document settings, before stereo marks and
/// configurations. Its exact shape is kept separately so a version 5 file cannot silently
/// contain a mark or stereo records, and a version 6 file cannot omit `stereo`.
mod v5 {
    use serde::{Deserialize, Serialize};

    use super::{
        AtomState, Keyed, PositionChange, ScenePosition, SceneState, Stored, StoredChange,
        StoredMolecule, StoredPage, StoredReference, StoredReferenceChange, StoredSelection,
        StoredSettings, UnmarkedBondState, required_option,
    };

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct File {
        #[serde(rename = "format")]
        pub _format: String,
        #[serde(rename = "format_version")]
        pub _format_version: u32,
        pub document: Document,
        pub history: History,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Draft {
        #[serde(rename = "format")]
        pub _format: String,
        #[serde(rename = "format_version")]
        pub _format_version: u32,
        pub base_revision: u64,
        pub saved_at_ms: u64,
        pub document: Document,
        pub history: History,
    }

    #[derive(Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Document {
        pub id: String,
        pub revision: u64,
        pub title: String,
        pub settings: StoredSettings,
        pub molecule: StoredMolecule<AtomState, UnmarkedBondState>,
        pub pages: Vec<StoredPage>,
        pub roots: Vec<String>,
        pub objects: Vec<Stored<SceneState>>,
        pub references: Vec<StoredReference>,
        pub positions: Vec<ScenePosition>,
        pub selection: StoredSelection,
        pub next_atom: u32,
        pub next_bond: u32,
        pub next_object: u32,
    }

    #[derive(Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct History {
        pub undo: Vec<ChangeSet>,
        pub redo: Vec<ChangeSet>,
    }

    #[derive(Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct ChangeSet {
        pub title: Option<StoredChange<String>>,
        #[serde(deserialize_with = "required_option")]
        pub settings: Option<StoredChange<StoredSettings>>,
        pub atoms: Vec<Keyed<AtomState>>,
        pub bonds: Vec<Keyed<UnmarkedBondState>>,
        pub roots: Option<StoredChange<Vec<String>>>,
        pub objects: Vec<Keyed<SceneState>>,
        pub references: Vec<StoredReferenceChange>,
        pub positions: Vec<PositionChange>,
        pub selection: StoredChange<StoredSelection>,
    }
}

/// Version 4 is the independently implemented scene format, before document settings.
/// Its exact shape is kept separately so version 5 cannot silently omit required settings.
mod v4 {
    use serde::Deserialize;

    use super::{
        AtomState, Keyed, PositionChange, ScenePosition, SceneState, Stored, StoredChange,
        StoredMolecule, StoredPage, StoredReference, StoredReferenceChange, StoredSelection,
        UnmarkedBondState,
    };

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct File {
        #[serde(rename = "format")]
        pub _format: String,
        #[serde(rename = "format_version")]
        pub _format_version: u32,
        pub document: Document,
        pub history: History,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Draft {
        #[serde(rename = "format")]
        pub _format: String,
        #[serde(rename = "format_version")]
        pub _format_version: u32,
        pub base_revision: u64,
        pub saved_at_ms: u64,
        pub document: Document,
        pub history: History,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Document {
        pub id: String,
        pub revision: u64,
        pub title: String,
        pub molecule: StoredMolecule<AtomState, UnmarkedBondState>,
        pub pages: Vec<StoredPage>,
        pub roots: Vec<String>,
        pub objects: Vec<Stored<SceneState>>,
        pub references: Vec<StoredReference>,
        pub positions: Vec<ScenePosition>,
        pub selection: StoredSelection,
        pub next_atom: u32,
        pub next_bond: u32,
        pub next_object: u32,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct History {
        pub undo: Vec<ChangeSet>,
        pub redo: Vec<ChangeSet>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct ChangeSet {
        pub title: Option<StoredChange<String>>,
        pub atoms: Vec<Keyed<AtomState>>,
        pub bonds: Vec<Keyed<UnmarkedBondState>>,
        pub roots: Option<StoredChange<Vec<String>>>,
        pub objects: Vec<Keyed<SceneState>>,
        pub references: Vec<StoredReferenceChange>,
        pub positions: Vec<PositionChange>,
        pub selection: StoredChange<StoredSelection>,
    }
}

/// Allows an explicit null while still requiring this field to be present.
fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

impl From<v4::Document> for v5::Document {
    fn from(value: v4::Document) -> Self {
        Self {
            id: value.id,
            revision: value.revision,
            title: value.title,
            settings: stored_settings(&Settings::default()),
            molecule: value.molecule,
            pages: value.pages,
            roots: value.roots,
            objects: value.objects,
            references: value.references,
            positions: value.positions,
            selection: value.selection,
            next_atom: value.next_atom,
            next_bond: value.next_bond,
            next_object: value.next_object,
        }
    }
}

impl From<v4::History> for v5::History {
    fn from(value: v4::History) -> Self {
        Self {
            undo: value.undo.into_iter().map(Into::into).collect(),
            redo: value.redo.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<v4::ChangeSet> for v5::ChangeSet {
    fn from(value: v4::ChangeSet) -> Self {
        Self {
            title: value.title,
            settings: None,
            atoms: value.atoms,
            bonds: value.bonds,
            roots: value.roots,
            objects: value.objects,
            references: value.references,
            positions: value.positions,
            selection: value.selection,
        }
    }
}

/// Versions 1–5 state no stereo: no bond has a mark and no record exists.
impl From<v5::Document> for SceneDocument {
    fn from(value: v5::Document) -> Self {
        Self {
            fragment_displays: Vec::new(),
            abbreviations: Vec::new(),
            display_labels: Vec::new(),
            id: value.id,
            revision: value.revision,
            title: value.title,
            settings: value.settings,
            molecule: StoredMolecule {
                id: value.molecule.id,
                atoms: value.molecule.atoms,
                bonds: value
                    .molecule
                    .bonds
                    .into_iter()
                    .map(|bond| Stored {
                        id: bond.id,
                        state: bond.state.into(),
                    })
                    .collect(),
            },
            pages: value.pages,
            roots: value.roots,
            objects: value.objects,
            references: value.references,
            positions: value.positions,
            stereo: Vec::new(),
            stereo_groups: Vec::new(),
            selection: value.selection,
            next_atom: value.next_atom,
            next_bond: value.next_bond,
            next_object: value.next_object,
            next_group: 1,
        }
    }
}

impl From<v5::History> for SceneHistory {
    fn from(value: v5::History) -> Self {
        Self {
            undo: value.undo.into_iter().map(Into::into).collect(),
            redo: value.redo.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<v5::ChangeSet> for SceneChangeSet {
    fn from(value: v5::ChangeSet) -> Self {
        Self {
            fragment_displays: Vec::new(),
            abbreviations: Vec::new(),
            display_labels: Vec::new(),
            title: value.title,
            settings: value.settings,
            atoms: value.atoms,
            bonds: value
                .bonds
                .into_iter()
                .map(|keyed| Keyed {
                    id: keyed.id,
                    before: keyed.before.map(Into::into),
                    after: keyed.after.map(Into::into),
                })
                .collect(),
            roots: value.roots,
            objects: value.objects,
            references: value.references,
            positions: value.positions,
            stereo: Vec::new(),
            stereo_groups: Vec::new(),
            selection: value.selection,
        }
    }
}

/// A document file whose atom and bond states are `A` and `B`; versions 2 and 3 differ only
/// there.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct File<A, B> {
    format: String,
    format_version: u32,
    document: StoredDocument<A, B>,
    history: StoredHistory<A, B>,
}

/// A recovery draft: the working copy with the stored revision it started from.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DraftFile<A, B> {
    format: String,
    format_version: u32,
    base_revision: u64,
    saved_at_ms: u64,
    document: StoredDocument<A, B>,
    history: StoredHistory<A, B>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredDocument<A, B> {
    id: String,
    revision: u64,
    title: String,
    molecule: StoredMolecule<A, B>,
    pages: Vec<StoredPage>,
    /// In order; the v1 view reports the first one's positions as `atoms[].position`.
    instances: Vec<StoredInstance>,
    next_atom: u32,
    next_bond: u32,
    next_instance: u32,
}

/// The chemical graph: atoms and bonds without positions. Atoms and bonds are
/// `{"id": ..., "state": {...}}`, matching history entries.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredMolecule<A, B> {
    id: String,
    atoms: Vec<Stored<A>>,
    bonds: Vec<Stored<B>>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Stored<T> {
    id: String,
    state: T,
}

/// A version 3 atom: exactly one of `element` and `query`.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AtomState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    element: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    query: Option<StoredQuery>,
    charge: i8,
    isotope: Option<u16>,
    hydrogens: StoredHydrogens,
    radical: StoredRadical,
    attachment_points: Vec<u8>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum StoredQuery {
    Any {},
    AnyHeavy {},
    Heteroatom {},
    Halogen {},
    List {
        elements: Vec<String>,
        negated: bool,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum StoredHydrogens {
    Inferred {},
    Unknown {},
    Specified { count: u8 },
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StoredRadical {
    None,
    Singlet,
    Doublet,
    Triplet,
    Unknown,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredPage {
    id: String,
}

/// A molecule scene instance with one position per atom of its molecule.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredInstance {
    id: String,
    page: String,
    molecule: String,
    positions: Vec<StoredPosition>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredPosition {
    atom: String,
    x: f64,
    y: f64,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredPoint {
    x: f64,
    y: f64,
}

/// A bond of versions 3 to 5: `query` is present exactly when `order` is `query`. These
/// versions have no stereo mark, so this shape refuses one.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UnmarkedBondState {
    begin: String,
    end: String,
    order: StoredOrder,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    query: Option<StoredBondQuery>,
}

/// A version 6 bond: as before, and `mark` when the bond is drawn as a stereo depiction.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BondState {
    begin: String,
    end: String,
    order: StoredOrder,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    query: Option<StoredBondQuery>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mark: Option<StoredMark>,
}

impl From<UnmarkedBondState> for BondState {
    fn from(value: UnmarkedBondState) -> Self {
        Self {
            begin: value.begin,
            end: value.end,
            order: value.order,
            query: value.query,
            mark: None,
        }
    }
}

/// How a bond is drawn. A wedge or hash records which end of the bond is narrow.
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum StoredMark {
    Wedge { narrow: StoredBondEnd },
    Hash { narrow: StoredBondEnd },
    Wavy {},
    Crossed {},
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StoredBondEnd {
    Begin,
    End,
}

/// One stated configuration. Its record's id is an atom's for a center and a bond's for a double
/// bond; `Document::restore` checks that the kind matches.
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum StoredStereoState {
    Center { state: StoredCenter },
    DoubleBond { state: StoredDoubleBond },
}

/// A center's neighbor slots are atom ids, `implicit_hydrogen`, or `lone_pair`, in canonical
/// order (atoms ascending, then hydrogen, then lone pair).
#[derive(Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum StoredCenter {
    Tetrahedral {
        neighbors: [String; 4],
        turn: StoredTurn,
    },
    Unknown {
        neighbors: [String; 4],
    },
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StoredTurn {
    Anticlockwise,
    Clockwise,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum StoredDoubleBond {
    Relative {
        begin_ref: String,
        end_ref: String,
        relation: StoredRelation,
    },
    Unknown {},
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StoredRelation {
    SameSide,
    OppositeSides,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StoredOrder {
    Single,
    Double,
    Triple,
    Aromatic,
    Dative,
    Query,
    Unknown,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum StoredBondQuery {
    Any {},
    List { orders: Vec<StoredQueryOrder> },
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StoredQueryOrder {
    Single,
    Double,
    Triple,
    Aromatic,
}

/// `undo` is oldest first; `redo` is a stack whose last entry is redone next.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredHistory<A, B> {
    undo: Vec<StoredChangeSet<A, B>>,
    redo: Vec<StoredChangeSet<A, B>>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredChangeSet<A, B> {
    title: Option<StoredChange<String>>,
    atoms: Vec<Keyed<A>>,
    bonds: Vec<Keyed<B>>,
    instances: Vec<Keyed<InstanceState>>,
    positions: Vec<PositionChange>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredChange<T> {
    before: T,
    after: T,
}

/// One entity's change; `null` means the entity did not exist on that side.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Keyed<T> {
    id: String,
    before: Option<T>,
    after: Option<T>,
}

/// An instance's parents. Its positions change through `positions` entries.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct InstanceState {
    page: String,
    molecule: String,
}

/// One atom's position in one instance; `null` where it had none.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PositionChange {
    instance: String,
    atom: String,
    before: Option<StoredPoint>,
    after: Option<StoredPoint>,
}

/// A stored atom state of any version, as a domain atom.
trait AtomRecord {
    fn into_atom(self) -> Result<Atom, String>;
}

/// A stored bond state of any version, as a domain bond.
trait BondRecord {
    fn into_bond(self) -> Result<Bond, String>;
}

/// The version 2 atom and bond states, read for migration only: an element atom with its
/// charge and isotope, and a single, double, or triple bond. Everything else takes its
/// default.
mod v2 {
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct AtomState {
        pub element: String,
        pub charge: i8,
        pub isotope: Option<u16>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct BondState {
        pub begin: String,
        pub end: String,
        pub order: Order,
    }

    #[derive(Clone, Copy, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub(super) enum Order {
        Single,
        Double,
        Triple,
    }
}

/// The version 1 shapes, read for migration only. The header fields are checked by
/// [`version`] before these are parsed.
mod v1 {
    use serde::Deserialize;
    use serde::de::IgnoredAny;

    use super::{Keyed, Stored, StoredChange, StoredPoint, v2};

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct File {
        #[serde(rename = "format")]
        pub _format: IgnoredAny,
        #[serde(rename = "format_version")]
        pub _format_version: IgnoredAny,
        pub document: Document,
        pub history: History,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Draft {
        #[serde(rename = "format")]
        pub _format: IgnoredAny,
        #[serde(rename = "format_version")]
        pub _format_version: IgnoredAny,
        pub base_revision: u64,
        pub saved_at_ms: u64,
        pub document: Document,
        pub history: History,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Document {
        pub id: String,
        pub revision: u64,
        pub title: String,
        pub atoms: Vec<Atom>,
        pub bonds: Vec<Stored<v2::BondState>>,
        pub next_atom: u32,
        pub next_bond: u32,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Atom {
        pub id: String,
        pub state: AtomState,
    }

    /// An atom's chemistry and its one position, as version 1 kept them together.
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct AtomState {
        pub element: String,
        pub charge: i8,
        pub isotope: Option<u16>,
        pub position: StoredPoint,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct History {
        pub undo: Vec<ChangeSet>,
        pub redo: Vec<ChangeSet>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct ChangeSet {
        pub title: Option<StoredChange<String>>,
        pub atoms: Vec<Keyed<AtomState>>,
        pub bonds: Vec<Keyed<v2::BondState>>,
    }
}

/// The two fields every version has, read before choosing a reader.
#[derive(Deserialize)]
struct Header {
    format: String,
    format_version: u32,
}

/// Which reader a file's header calls for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Version {
    Current,
    TwentyFive,
    TwentyFour,
    TwentyThree,
    TwentyTwo,
    TwentyOne,
    Twenty,
    Nineteen,
    Eighteen,
    Seventeen,
    Sixteen,
    Fifteen,
    Fourteen,
    Thirteen,
    Twelve,
    Eleven,
    Ten,
    Nine,
    Eight,
    Seven,
    Six,
    Five,
    Four,
    Three,
    Two,
    One,
}

fn version(bytes: &[u8], format: &str, current: u32, kind: &str) -> Result<Version, String> {
    let header: Header = parse(bytes, kind)?;
    if header.format != format {
        return Err(format!(
            "unsupported {kind} format {:?} version {}",
            header.format, header.format_version
        ));
    }
    match header.format_version {
        version if version == current => Ok(Version::Current),
        25 => Ok(Version::TwentyFive),
        24 => Ok(Version::TwentyFour),
        23 => Ok(Version::TwentyThree),
        22 => Ok(Version::TwentyTwo),
        21 => Ok(Version::TwentyOne),
        20 => Ok(Version::Twenty),
        19 => Ok(Version::Nineteen),
        18 => Ok(Version::Eighteen),
        17 => Ok(Version::Seventeen),
        16 => Ok(Version::Sixteen),
        15 => Ok(Version::Fifteen),
        14 => Ok(Version::Fourteen),
        13 => Ok(Version::Thirteen),
        12 => Ok(Version::Twelve),
        11 => Ok(Version::Eleven),
        10 => Ok(Version::Ten),
        9 => Ok(Version::Nine),
        8 => Ok(Version::Eight),
        7 => Ok(Version::Seven),
        VERSION_6 => Ok(Version::Six),
        VERSION_5 => Ok(Version::Five),
        VERSION_4 => Ok(Version::Four),
        VERSION_3 => Ok(Version::Three),
        VERSION_2 => Ok(Version::Two),
        VERSION_1 => Ok(Version::One),
        version => Err(format!(
            "unsupported {kind} format {format:?} version {version}; this build reads versions \
             {VERSION_1}, {VERSION_2}, {VERSION_3}, {VERSION_4}, {VERSION_5}, {VERSION_6}, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, and \
             {current}"
        )),
    }
}

fn parse<T: DeserializeOwned>(bytes: &[u8], kind: &str) -> Result<T, String> {
    serde_json::from_slice(bytes).map_err(|error| format!("invalid {kind} file: {error}"))
}

pub(crate) fn encode(document: &Document) -> Vec<u8> {
    let (document, history) = stored(document);
    to_bytes(&SceneFile {
        format: FORMAT.to_owned(),
        format_version: FORMAT_VERSION,
        document,
        history,
    })
}

pub(crate) fn encode_draft(draft: &Draft) -> Vec<u8> {
    let (document, history) = stored(&draft.document);
    to_bytes(&SceneDraftFile {
        format: DRAFT_FORMAT.to_owned(),
        format_version: DRAFT_FORMAT_VERSION,
        base_revision: draft.base_revision,
        saved_at_ms: draft.saved_at_ms,
        document,
        history,
    })
}

/// Compact JSON and a final newline. Indentation would multiply the size of a full history
/// beyond the read limit.
fn to_bytes(value: &impl Serialize) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(value).expect("document values are serializable");
    bytes.push(b'\n');
    bytes
}

type Current = (SceneDocument, SceneHistory);

fn stored(document: &Document) -> Current {
    let parts = document.to_parts();
    let stored = SceneDocument {
        fragment_displays: parts
            .fragment_displays
            .iter()
            .map(|(key, display)| StoredFragmentDisplay {
                instance: key.instance.to_string(),
                anchor: key.atom.to_string(),
                members: display.members.iter().map(ToString::to_string).collect(),
                text: None,
                runs: stored_fragment_display(display).runs,
            })
            .collect(),
        display_labels: parts
            .display_labels
            .iter()
            .map(|(key, text)| StoredDisplayLabel {
                instance: key.instance.to_string(),
                anchor: key.atom.to_string(),
                text: text.clone(),
            })
            .collect(),
        abbreviations: parts
            .abbreviations
            .iter()
            .map(|(key, terminal)| StoredAbbreviation {
                instance: key.instance.to_string(),
                anchor: key.atom.to_string(),
                terminal: terminal.to_string(),
            })
            .collect(),
        id: parts.id.to_string(),
        revision: parts.revision,
        title: parts.title,
        settings: stored_settings(&parts.settings),
        molecule: StoredMolecule {
            id: MOLECULE_ID.to_owned(),
            atoms: parts
                .atoms
                .iter()
                .map(|(id, atom)| Stored {
                    id: id.to_string(),
                    state: atom_state(atom),
                })
                .collect(),
            bonds: parts
                .bonds
                .iter()
                .map(|(id, bond)| Stored {
                    id: id.to_string(),
                    state: bond_state(bond),
                })
                .collect(),
        },
        pages: vec![StoredPage {
            id: PAGE_ID.to_owned(),
        }],
        roots: parts.roots.iter().map(ToString::to_string).collect(),
        objects: parts
            .objects
            .iter()
            .map(|(id, object)| Stored {
                id: id.to_string(),
                state: stored_scene_state(object),
            })
            .collect(),
        references: parts.references.iter().map(stored_reference).collect(),
        positions: parts
            .positions
            .iter()
            .map(|(key, point)| ScenePosition {
                instance: key.instance.to_string(),
                atom: key.atom.to_string(),
                x: point.x(),
                y: point.y(),
            })
            .collect(),
        stereo: parts
            .stereo
            .iter()
            .map(|(key, state)| Stored {
                id: stereo_key_id(*key),
                state: stored_stereo_state(state),
            })
            .collect(),
        stereo_groups: parts
            .stereo_groups
            .iter()
            .map(|(id, group)| Stored {
                id: id.to_string(),
                state: stored_group(group),
            })
            .collect(),
        selection: stored_selection(&parts.selection),
        next_atom: parts.next_atom,
        next_bond: parts.next_bond,
        next_object: parts.next_object,
        next_group: parts.next_group,
    };
    let history = SceneHistory {
        undo: parts.undo.iter().map(stored_scene_change_set).collect(),
        redo: parts.redo.iter().map(stored_scene_change_set).collect(),
    };
    (stored, history)
}

#[derive(Deserialize)]
struct RevisionOnly {
    revision: u64,
}

/// Reads only the header and revision; `decode` remains the validating reader.
pub(crate) fn decode_revision(bytes: &[u8]) -> Result<u64, String> {
    #[derive(Deserialize)]
    struct Probe {
        document: RevisionOnly,
    }
    version(bytes, FORMAT, FORMAT_VERSION, "document")?;
    let probe: Probe = parse(bytes, "document")?;
    Ok(probe.document.revision)
}

/// The earlier version of a document file this build reads (1 to 25), which a save
/// replaces only after keeping a copy of it; `None` otherwise.
pub(crate) fn earlier_version(bytes: &[u8]) -> Option<u32> {
    earlier(version(bytes, FORMAT, FORMAT_VERSION, "document"))
}

/// The earlier version of a recovery draft this build reads, if any.
pub(crate) fn earlier_draft_version(bytes: &[u8]) -> Option<u32> {
    earlier(version(bytes, DRAFT_FORMAT, DRAFT_FORMAT_VERSION, "draft"))
}

fn earlier(version: Result<Version, String>) -> Option<u32> {
    match version {
        Ok(Version::One) => Some(VERSION_1),
        Ok(Version::Two) => Some(VERSION_2),
        Ok(Version::Three) => Some(VERSION_3),
        Ok(Version::Four) => Some(VERSION_4),
        Ok(Version::Five) => Some(VERSION_5),
        Ok(Version::Six) => Some(VERSION_6),
        Ok(Version::Fourteen) => Some(14),
        Ok(Version::TwentyFive) => Some(25),
        Ok(Version::TwentyFour) => Some(24),
        Ok(Version::TwentyThree) => Some(23),
        Ok(Version::TwentyTwo) => Some(22),
        Ok(Version::TwentyOne) => Some(21),
        Ok(Version::Twenty) => Some(20),
        Ok(Version::Nineteen) => Some(19),
        Ok(Version::Eighteen) => Some(18),
        Ok(Version::Seventeen) => Some(17),
        Ok(Version::Sixteen) => Some(16),
        Ok(Version::Fifteen) => Some(15),
        Ok(Version::Thirteen) => Some(13),
        Ok(Version::Twelve) => Some(12),
        Ok(Version::Eleven) => Some(11),
        Ok(Version::Ten) => Some(10),
        Ok(Version::Nine) => Some(9),
        Ok(Version::Eight) => Some(8),
        Ok(Version::Seven) => Some(7),
        Ok(Version::Current) | Err(_) => None,
    }
}

/// Parses and fully validates a stored document, including its history; a version 1 to 25 file
/// is migrated.
pub(crate) fn decode(bytes: &[u8]) -> Result<Document, String> {
    let format = version(bytes, FORMAT, FORMAT_VERSION, "document")?;
    check_graphic_fields(bytes, format)?;
    check_abbreviation_fields(bytes, format)?;
    check_caption_fields(bytes, format)?;
    check_settings_fields(bytes, format)?;
    let parts = match format {
        Version::Current
        | Version::TwentyFive
        | Version::TwentyFour
        | Version::TwentyThree
        | Version::TwentyTwo
        | Version::TwentyOne
        | Version::Twenty
        | Version::Nineteen
        | Version::Eighteen
        | Version::Seventeen
        | Version::Sixteen
        | Version::Fifteen
        | Version::Fourteen
        | Version::Thirteen
        | Version::Twelve
        | Version::Eleven
        | Version::Ten
        | Version::Nine
        | Version::Eight
        | Version::Seven => {
            let file: SceneFile = parse(bytes, "document")?;
            scene_parts(file.document, file.history)?
        }
        Version::Six => {
            let file: v6::File = parse(bytes, "document")?;
            scene_parts(file.document.into(), file.history.into())?
        }
        Version::Five => {
            let file: v5::File = parse(bytes, "document")?;
            scene_parts(file.document.into(), file.history.into())?
        }
        Version::Four => {
            let file: v4::File = parse(bytes, "document")?;
            let (document, history): (v5::Document, v5::History) =
                (file.document.into(), file.history.into());
            scene_parts(document.into(), history.into())?
        }
        Version::Three => {
            let file: File<AtomState, UnmarkedBondState> = parse(bytes, "document")?;
            parts(file.document, file.history)?
        }
        Version::Two => {
            let file: File<v2::AtomState, v2::BondState> = parse(bytes, "document")?;
            parts(file.document, file.history)?
        }
        Version::One => {
            let file: v1::File = parse(bytes, "document")?;
            migrate(file.document, file.history)?
        }
    };
    refuse_earlier_captions(format, &parts)?;
    Document::restore(parts).map_err(|error| error.to_string())
}

/// Parses and fully validates a recovery draft; a version 1 to 25 draft is migrated.
pub(crate) fn decode_draft(bytes: &[u8]) -> Result<Draft, String> {
    let format = version(bytes, DRAFT_FORMAT, DRAFT_FORMAT_VERSION, "draft")?;
    check_graphic_fields(bytes, format)?;
    check_abbreviation_fields(bytes, format)?;
    check_caption_fields(bytes, format)?;
    check_settings_fields(bytes, format)?;
    let (parts, base_revision, saved_at_ms) = match format {
        Version::Current
        | Version::TwentyFive
        | Version::TwentyFour
        | Version::TwentyThree
        | Version::TwentyTwo
        | Version::TwentyOne
        | Version::Twenty
        | Version::Nineteen
        | Version::Eighteen
        | Version::Seventeen
        | Version::Sixteen
        | Version::Fifteen
        | Version::Fourteen
        | Version::Thirteen
        | Version::Twelve
        | Version::Eleven
        | Version::Ten
        | Version::Nine
        | Version::Eight
        | Version::Seven => {
            let file: SceneDraftFile = parse(bytes, "draft")?;
            let parts = scene_parts(file.document, file.history)?;
            (parts, file.base_revision, file.saved_at_ms)
        }
        Version::Six => {
            let file: v6::Draft = parse(bytes, "draft")?;
            let parts = scene_parts(file.document.into(), file.history.into())?;
            (parts, file.base_revision, file.saved_at_ms)
        }
        Version::Five => {
            let file: v5::Draft = parse(bytes, "draft")?;
            let parts = scene_parts(file.document.into(), file.history.into())?;
            (parts, file.base_revision, file.saved_at_ms)
        }
        Version::Four => {
            let file: v4::Draft = parse(bytes, "draft")?;
            let (document, history): (v5::Document, v5::History) =
                (file.document.into(), file.history.into());
            let parts = scene_parts(document.into(), history.into())?;
            (parts, file.base_revision, file.saved_at_ms)
        }
        Version::Three => {
            let file: DraftFile<AtomState, UnmarkedBondState> = parse(bytes, "draft")?;
            let parts = parts(file.document, file.history)?;
            (parts, file.base_revision, file.saved_at_ms)
        }
        Version::Two => {
            let file: DraftFile<v2::AtomState, v2::BondState> = parse(bytes, "draft")?;
            let parts = parts(file.document, file.history)?;
            (parts, file.base_revision, file.saved_at_ms)
        }
        Version::One => {
            let file: v1::Draft = parse(bytes, "draft")?;
            let parts = migrate(file.document, file.history)?;
            (parts, file.base_revision, file.saved_at_ms)
        }
    };
    refuse_earlier_captions(format, &parts)?;
    Ok(Draft {
        document: Document::restore(parts).map_err(|error| error.to_string())?,
        base_revision,
        saved_at_ms,
    })
}

/// The shared scene DTO default is migration-only, including every history side.
fn check_graphic_fields(bytes: &[u8], format: Version) -> Result<(), String> {
    fn visit(value: &serde_json::Value, current: bool) -> Result<(), String> {
        match value {
            serde_json::Value::Object(object) => {
                if object.get("kind").and_then(serde_json::Value::as_str) == Some("graphic")
                    && object.contains_key("line_width_pt") != current
                {
                    return Err("Graphic line_width_pt is required in v26 and forbidden in v1–25, including history.".into());
                }
                for child in object.values() {
                    visit(child, current)?;
                }
            }
            serde_json::Value::Array(values) => {
                for child in values {
                    visit(child, current)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    visit(
        &parse(bytes, "document or draft")?,
        format == Version::Current,
    )
}

/// The shared DTO defaults only support migration. v9 requires the fields on every
/// current/history state; earlier headers cannot smuggle even empty v9 fields.
fn check_abbreviation_fields(bytes: &[u8], format: Version) -> Result<(), String> {
    let raw: serde_json::Value = parse(bytes, "document or draft")?;
    let current = matches!(
        format,
        Version::Current
            | Version::TwentyFive
            | Version::TwentyFour
            | Version::TwentyThree
            | Version::TwentyTwo
            | Version::TwentyOne
            | Version::Twenty
            | Version::Nineteen
            | Version::Eighteen
            | Version::Seventeen
            | Version::Sixteen
            | Version::Fifteen
            | Version::Fourteen
            | Version::Thirteen
            | Version::Twelve
            | Version::Eleven
            | Version::Ten
            | Version::Nine
    );
    let document = raw.get("document").ok_or("missing document")?;
    let history = raw.get("history").ok_or("missing history")?;
    let mut states = vec![document];
    for stack in ["undo", "redo"] {
        let entries = history
            .get(stack)
            .and_then(serde_json::Value::as_array)
            .ok_or("missing history stack")?;
        states.extend(entries);
    }
    if states.iter().any(|state| {
        state.get("fragment_displays").is_some()
            != matches!(
                format,
                Version::Current | Version::TwentyFive | Version::TwentyFour
            )
    }) {
        return Err(
            "fragment_displays are required in v24+ and forbidden in v1–23, including history."
                .into(),
        );
    }
    let check_display = |display: &serde_json::Value| -> Result<(), String> {
        if display.is_null() {
            return Ok(());
        }
        let rich = matches!(format, Version::Current | Version::TwentyFive);
        if display.get("runs").is_some() != rich
            || display.get("text").is_some() == rich
            || (rich && !display["runs"].is_array())
            || (!rich && !display["text"].is_string())
        {
            return Err("Fragment display requires runs without text in v25; v24 requires text without runs, including history.".into());
        }
        Ok(())
    };
    for (index, state) in states.iter().enumerate() {
        if let Some(displays) = state
            .get("fragment_displays")
            .and_then(serde_json::Value::as_array)
        {
            for display in displays {
                if index == 0 {
                    check_display(display)?;
                } else {
                    if let Some(before) = display.get("before") {
                        check_display(before)?;
                    }
                    if let Some(after) = display.get("after") {
                        check_display(after)?;
                    }
                }
            }
        }
    }
    if states.iter().any(|state| {
        state.get("display_labels").is_some()
            != matches!(
                format,
                Version::Current | Version::TwentyFive | Version::TwentyFour | Version::TwentyThree
            )
    }) {
        return Err(
            "display_labels are required in v23+ and forbidden in v1–22, including history.".into(),
        );
    }
    if states
        .iter()
        .any(|state| state.get("abbreviations").is_some() != current)
    {
        return Err(
            "Abbreviation fields are required in v9 and forbidden in v1–8, including history."
                .into(),
        );
    }
    Ok(())
}

/// Defaults are migration-only: v22 requires shadow, v21+ outline, v20+ underline,
/// v19+ style and v18+ weight
/// in every stored settings state.
fn check_settings_fields(bytes: &[u8], format: Version) -> Result<(), String> {
    let raw: serde_json::Value = parse(bytes, "document or draft")?;
    let current = matches!(
        format,
        Version::Current
            | Version::TwentyFive
            | Version::TwentyFour
            | Version::TwentyThree
            | Version::TwentyTwo
            | Version::TwentyOne
            | Version::Twenty
            | Version::Nineteen
            | Version::Eighteen
    );
    let check = |settings: &serde_json::Value| -> Result<(), String> {
        if settings.get("label_weight").is_some() != current
            || (current && !matches!(settings["label_weight"].as_str(), Some("normal" | "bold")))
        {
            return Err("label_weight is required as normal or bold in v18–26 and forbidden in v1–17, including history.".into());
        }
        let styled = matches!(
            format,
            Version::Current
                | Version::TwentyFive
                | Version::TwentyFour
                | Version::TwentyThree
                | Version::TwentyTwo
                | Version::TwentyOne
                | Version::Twenty
                | Version::Nineteen
        );
        if settings.get("label_style").is_some() != styled
            || (styled && !matches!(settings["label_style"].as_str(), Some("normal" | "italic")))
        {
            return Err("label_style is required as normal or italic in v19–26 and forbidden in v1–18, including history.".into());
        }
        let underlined = matches!(
            format,
            Version::Current
                | Version::TwentyFive
                | Version::TwentyFour
                | Version::TwentyThree
                | Version::TwentyTwo
                | Version::TwentyOne
                | Version::Twenty
        );
        if settings.get("label_underline").is_some() != underlined
            || (underlined && !settings["label_underline"].is_boolean())
        {
            return Err("label_underline is required as a boolean in v20–26 and forbidden in v1–19, including history.".into());
        }
        let outlined = matches!(
            format,
            Version::Current
                | Version::TwentyFive
                | Version::TwentyFour
                | Version::TwentyThree
                | Version::TwentyTwo
                | Version::TwentyOne
        );
        if settings.get("label_outline").is_some() != outlined
            || (outlined && !settings["label_outline"].is_boolean())
        {
            return Err("label_outline is required as a boolean in v21–26 and forbidden in v1–20, including history.".into());
        }
        let shadowed = matches!(
            format,
            Version::Current
                | Version::TwentyFive
                | Version::TwentyFour
                | Version::TwentyThree
                | Version::TwentyTwo
        );
        if settings.get("label_shadow").is_some() != shadowed
            || (shadowed && !settings["label_shadow"].is_boolean())
        {
            return Err("label_shadow is required as a boolean in v22–26 and forbidden in v1–21, including history.".into());
        }
        Ok(())
    };
    if let Some(settings) = raw["document"].get("settings") {
        check(settings)?;
    }
    for stack in ["undo", "redo"] {
        if let Some(entries) = raw["history"][stack].as_array() {
            for entry in entries {
                if let Some(settings) = entry.get("settings").filter(|value| !value.is_null()) {
                    check(&settings["before"])?;
                    check(&settings["after"])?;
                }
            }
        }
    }
    Ok(())
}

/// Required caption fields are versioned for every current and historical state.
fn check_caption_fields(bytes: &[u8], format: Version) -> Result<(), String> {
    fn check(value: &serde_json::Value, version: u32) -> Result<(), String> {
        let styles = version >= 10;
        let alignment = version >= 11;
        let font = version >= 12;
        let size = version >= 13;
        let color = version >= 14;
        let underline = version >= 15;
        let outline = version >= 16;
        let shadow = version >= 17;
        match value {
            serde_json::Value::Object(object) => {
                if object.get("kind").and_then(serde_json::Value::as_str) == Some("caption") {
                    let field = object.get("alignment");
                    if field.is_some() != alignment || field.is_some_and(serde_json::Value::is_null)
                    {
                        return Err("Caption alignment is required in v11–19 and forbidden in v1–10, including history.".into());
                    }
                    if let Some(runs) = object.get("runs").and_then(serde_json::Value::as_array) {
                        for run in runs {
                            if run.get("shadow").is_some() != shadow
                                || (shadow && !run["shadow"].is_boolean())
                            {
                                return Err("Caption shadow is required as a boolean in v17–19 and forbidden in v1–16, including history.".into());
                            }
                            if run.get("outline").is_some() != outline
                                || (outline && !run["outline"].is_boolean())
                            {
                                return Err("Caption outline is required as a boolean in v16–19 and forbidden in v1–15, including history.".into());
                            }
                            if run.get("underline").is_some() != underline
                                || (underline && !run["underline"].is_boolean())
                            {
                                return Err("Caption underline is required as a boolean in v15–19 and forbidden in v1–14, including history.".into());
                            }
                            if run.get("color").is_some() != color {
                                return Err("Caption color is required in v14–19 and forbidden in v1–13, including history.".into());
                            }
                            if run.get("size_pt").is_some() != size {
                                return Err("Caption size_pt is required in v13–19 and forbidden in v1–12, including history.".into());
                            }
                            if run.get("font").is_some() != font {
                                return Err("Caption font is required in v12–19 and forbidden in v1–11, including history.".into());
                            }
                            for field in ["weight", "style"] {
                                let present = run.get(field).is_some();
                                if present != styles || (present && run[field].is_null()) {
                                    return Err("Caption weight/style are required in v10–19 and forbidden in v1–9, including history.".into());
                                }
                            }
                        }
                    }
                }
                for child in object.values() {
                    check(child, version)?;
                }
            }
            serde_json::Value::Array(values) => {
                for child in values {
                    check(child, version)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    check(
        &parse(bytes, "document or draft")?,
        earlier(Ok(format)).unwrap_or(FORMAT_VERSION),
    )
}

/// Caption variants are forbidden in all earlier versions, including history-only states.
fn refuse_earlier_captions(format: Version, parts: &DocumentParts) -> Result<(), String> {
    if matches!(
        format,
        Version::Current
            | Version::TwentyFive
            | Version::TwentyFour
            | Version::TwentyThree
            | Version::TwentyTwo
            | Version::TwentyOne
            | Version::Twenty
            | Version::Nineteen
            | Version::Eighteen
            | Version::Seventeen
            | Version::Sixteen
            | Version::Fifteen
            | Version::Fourteen
            | Version::Thirteen
            | Version::Twelve
            | Version::Eleven
            | Version::Ten
            | Version::Nine
            | Version::Eight
    ) {
        return Ok(());
    }
    let caption = |object: &SceneObject| matches!(object, SceneObject::Caption(_));
    if parts.objects.iter().any(|(_, object)| caption(object))
        || parts.undo.iter().chain(&parts.redo).any(|change| {
            change.objects.iter().any(|(_, state)| {
                state.before.as_ref().is_some_and(caption)
                    || state.after.as_ref().is_some_and(caption)
            })
        })
    {
        return Err("Captions require file format version 8.".into());
    }
    Ok(())
}

/// Reads a draft's header without restoring its document; `decode_draft` validates.
pub(crate) fn decode_draft_info(bytes: &[u8]) -> Result<DraftInfo, String> {
    #[derive(Deserialize)]
    struct Probe {
        base_revision: u64,
        saved_at_ms: u64,
        document: RevisionOnly,
    }
    version(bytes, DRAFT_FORMAT, DRAFT_FORMAT_VERSION, "draft")?;
    let probe: Probe = parse(bytes, "draft")?;
    Ok(DraftInfo {
        revision: probe.document.revision,
        base_revision: probe.base_revision,
        saved_at_ms: probe.saved_at_ms,
    })
}

fn scene_parts(stored: SceneDocument, history: SceneHistory) -> Result<DocumentParts, String> {
    if stored.molecule.id != MOLECULE_ID {
        return Err(format!("unknown molecule {:?}", stored.molecule.id));
    }
    if !matches!(stored.pages.as_slice(), [page] if page.id == PAGE_ID) {
        return Err(format!("a document holds exactly one page, {PAGE_ID:?}"));
    }
    Ok(DocumentParts {
        fragment_displays: stored
            .fragment_displays
            .into_iter()
            .map(|entry| {
                Ok((
                    PositionKey {
                        instance: instance_id(&entry.instance)?,
                        atom: atom_id(&entry.anchor)?,
                    },
                    read_fragment_display(StoredFragmentDisplayState {
                        members: entry.members,
                        text: entry.text,
                        runs: entry.runs,
                    })?,
                ))
            })
            .collect::<Result<_, String>>()?,
        display_labels: stored
            .display_labels
            .into_iter()
            .map(|entry| {
                Ok((
                    PositionKey {
                        instance: instance_id(&entry.instance)?,
                        atom: atom_id(&entry.anchor)?,
                    },
                    entry.text,
                ))
            })
            .collect::<Result<_, String>>()?,
        abbreviations: stored
            .abbreviations
            .into_iter()
            .map(|entry| {
                Ok((
                    PositionKey {
                        instance: instance_id(&entry.instance)?,
                        atom: atom_id(&entry.anchor)?,
                    },
                    atom_id(&entry.terminal)?,
                ))
            })
            .collect::<Result<_, String>>()?,
        id: document_id(&stored.id)?,
        revision: stored.revision,
        title: stored.title,
        settings: to_settings(&stored.settings),
        atoms: stored
            .molecule
            .atoms
            .into_iter()
            .map(|atom| Ok((atom_id(&atom.id)?, atom.state.into_atom()?)))
            .collect::<Result<_, String>>()?,
        bonds: stored
            .molecule
            .bonds
            .into_iter()
            .map(|bond| Ok((bond_id(&bond.id)?, bond.state.into_bond()?)))
            .collect::<Result<_, String>>()?,
        roots: stored
            .roots
            .iter()
            .map(|id| instance_id(id))
            .collect::<Result<_, _>>()?,
        objects: stored
            .objects
            .into_iter()
            .map(|object| Ok((instance_id(&object.id)?, read_scene_state(object.state)?)))
            .collect::<Result<_, String>>()?,
        references: stored
            .references
            .into_iter()
            .map(read_reference)
            .collect::<Result<_, _>>()?,
        positions: stored
            .positions
            .into_iter()
            .map(|position| {
                Ok((
                    PositionKey {
                        instance: instance_id(&position.instance)?,
                        atom: atom_id(&position.atom)?,
                    },
                    point(position.x, position.y)?,
                ))
            })
            .collect::<Result<_, String>>()?,
        stereo: stored
            .stereo
            .into_iter()
            .map(|record| Ok((stereo_key(&record.id)?, read_stereo_state(record.state)?)))
            .collect::<Result<_, String>>()?,
        stereo_groups: stored
            .stereo_groups
            .into_iter()
            .map(|record| Ok((group_id(&record.id)?, read_group(record.state)?)))
            .collect::<Result<_, String>>()?,
        selection: read_selection(stored.selection)?,
        next_atom: stored.next_atom,
        next_bond: stored.next_bond,
        next_object: stored.next_object,
        next_group: stored.next_group,
        undo: history
            .undo
            .into_iter()
            .map(read_scene_change_set)
            .collect::<Result<_, _>>()?,
        redo: history
            .redo
            .into_iter()
            .map(read_scene_change_set)
            .collect::<Result<_, _>>()?,
    })
}

fn stored_scene_state(object: &SceneObject) -> SceneState {
    match object {
        SceneObject::Caption(caption) => SceneState::Caption {
            alignment: Some(match caption.alignment {
                CaptionAlignment::Left => StoredCaptionAlignment::Left,
                CaptionAlignment::Center => StoredCaptionAlignment::Center,
                CaptionAlignment::Right => StoredCaptionAlignment::Right,
            }),
            anchor: stored_point(&caption.anchor),
            runs: caption
                .runs
                .iter()
                .map(|run| StoredCaptionRun {
                    shadow: run.shadow,
                    outline: run.outline,
                    underline: run.underline,
                    color: run.color.clone(),
                    size_pt: run.size_pt,
                    font: run.font.map(Into::into),
                    text: run.text.clone(),
                    style: Some(match run.style {
                        CaptionStyle::Normal => StoredCaptionStyle::Normal,
                        CaptionStyle::Italic => StoredCaptionStyle::Italic,
                    }),
                    weight: Some(match run.weight {
                        CaptionWeight::Normal => StoredCaptionWeight::Normal,
                        CaptionWeight::Bold => StoredCaptionWeight::Bold,
                    }),
                    script: match run.script {
                        CaptionScript::Baseline => StoredCaptionScript::Baseline,
                        CaptionScript::Subscript => StoredCaptionScript::Subscript,
                        CaptionScript::Superscript => StoredCaptionScript::Superscript,
                    },
                })
                .collect(),
        },
        SceneObject::Instance => SceneState::Instance {
            molecule: MOLECULE_ID.to_owned(),
        },
        SceneObject::Group { children } => SceneState::Group {
            children: children.iter().map(ToString::to_string).collect(),
        },
        SceneObject::Graphic(graphic) => SceneState::Graphic {
            line_width_pt: graphic.line_width_pt,
            shape: match graphic.shape {
                GraphicShape::Line => StoredShape::Line,
                GraphicShape::Rectangle => StoredShape::Rectangle,
                GraphicShape::Arrow => StoredShape::Arrow,
            },
            points: graphic.points.each_ref().map(stored_point),
        },
    }
}

fn stored_reference(reference: &SceneReference) -> StoredReference {
    let target = match reference.target {
        ReferenceTarget::Object(id) => StoredReferenceTarget::Object { id: id.to_string() },
        ReferenceTarget::Atom(id) => StoredReferenceTarget::Atom { id: id.to_string() },
        ReferenceTarget::Bond(id) => StoredReferenceTarget::Bond { id: id.to_string() },
    };
    StoredReference {
        source: reference.source.to_string(),
        target,
    }
}

fn read_reference(reference: StoredReference) -> Result<SceneReference, String> {
    let target = match reference.target {
        StoredReferenceTarget::Object { id } => ReferenceTarget::Object(instance_id(&id)?),
        StoredReferenceTarget::Atom { id } => ReferenceTarget::Atom(atom_id(&id)?),
        StoredReferenceTarget::Bond { id } => ReferenceTarget::Bond(bond_id(&id)?),
    };
    Ok(SceneReference {
        source: instance_id(&reference.source)?,
        target,
    })
}

fn read_scene_state(object: SceneState) -> Result<SceneObject, String> {
    Ok(match object {
        SceneState::Caption {
            alignment,
            anchor,
            runs,
        } => {
            let caption = Caption {
                alignment: match alignment.unwrap_or(StoredCaptionAlignment::Left) {
                    StoredCaptionAlignment::Left => CaptionAlignment::Left,
                    StoredCaptionAlignment::Center => CaptionAlignment::Center,
                    StoredCaptionAlignment::Right => CaptionAlignment::Right,
                },
                anchor: point(anchor.x, anchor.y)?,
                runs: runs
                    .into_iter()
                    .map(|run| CaptionRun {
                        shadow: run.shadow,
                        outline: run.outline,
                        underline: run.underline,
                        color: run.color,
                        size_pt: run.size_pt,
                        font: run.font.map(Into::into),
                        text: run.text,
                        style: match run.style.unwrap_or(StoredCaptionStyle::Normal) {
                            StoredCaptionStyle::Normal => CaptionStyle::Normal,
                            StoredCaptionStyle::Italic => CaptionStyle::Italic,
                        },
                        weight: match run.weight.unwrap_or(StoredCaptionWeight::Normal) {
                            StoredCaptionWeight::Normal => CaptionWeight::Normal,
                            StoredCaptionWeight::Bold => CaptionWeight::Bold,
                        },
                        script: match run.script {
                            StoredCaptionScript::Baseline => CaptionScript::Baseline,
                            StoredCaptionScript::Subscript => CaptionScript::Subscript,
                            StoredCaptionScript::Superscript => CaptionScript::Superscript,
                        },
                    })
                    .collect(),
            };
            caption.validate().map_err(|cause| cause.to_string())?;
            SceneObject::Caption(caption)
        }
        SceneState::Instance { molecule } => {
            parents(PAGE_ID, &molecule)?;
            SceneObject::Instance
        }
        SceneState::Group { children } => SceneObject::Group {
            children: children
                .iter()
                .map(|id| instance_id(id))
                .collect::<Result<_, _>>()?,
        },
        SceneState::Graphic {
            shape,
            points,
            line_width_pt,
        } => {
            let [first, second] = points;
            SceneObject::Graphic(Graphic {
                line_width_pt,
                shape: match shape {
                    StoredShape::Line => GraphicShape::Line,
                    StoredShape::Rectangle => GraphicShape::Rectangle,
                    StoredShape::Arrow => GraphicShape::Arrow,
                },
                points: [point(first.x, first.y)?, point(second.x, second.y)?],
            })
        }
    })
}

fn stored_selection(selection: &Selection) -> StoredSelection {
    StoredSelection {
        objects: selection.objects.iter().map(ToString::to_string).collect(),
        atoms: selection.atoms.iter().map(ToString::to_string).collect(),
        bonds: selection.bonds.iter().map(ToString::to_string).collect(),
    }
}

fn read_selection(selection: StoredSelection) -> Result<Selection, String> {
    fn distinct<T: Ord>(
        values: Vec<String>,
        parse: fn(&str) -> Result<T, String>,
    ) -> Result<BTreeSet<T>, String> {
        let mut parsed = BTreeSet::new();
        for value in values {
            if !parsed.insert(parse(&value)?) {
                return Err(format!("selection repeats identifier {value:?}"));
            }
        }
        Ok(parsed)
    }
    Ok(Selection {
        objects: distinct(selection.objects, instance_id)?,
        atoms: distinct(selection.atoms, atom_id)?,
        bonds: distinct(selection.bonds, bond_id)?,
    })
}

fn read_scene_change_set(stored: SceneChangeSet) -> Result<ChangeSet, String> {
    let mut set = to_change_set(StoredChangeSet {
        title: stored.title,
        atoms: stored.atoms,
        bonds: stored.bonds,
        instances: Vec::new(),
        positions: stored.positions,
    })?;
    set.fragment_displays = stored
        .fragment_displays
        .into_iter()
        .map(|entry| {
            Ok((
                PositionKey {
                    instance: instance_id(&entry.instance)?,
                    atom: atom_id(&entry.anchor)?,
                },
                Change {
                    before: entry.before.map(read_fragment_display).transpose()?,
                    after: entry.after.map(read_fragment_display).transpose()?,
                },
            ))
        })
        .collect::<Result<_, String>>()?;
    set.display_labels = stored
        .display_labels
        .into_iter()
        .map(|entry| {
            Ok((
                PositionKey {
                    instance: instance_id(&entry.instance)?,
                    atom: atom_id(&entry.anchor)?,
                },
                Change {
                    before: entry.before,
                    after: entry.after,
                },
            ))
        })
        .collect::<Result<_, String>>()?;
    set.abbreviations = stored
        .abbreviations
        .into_iter()
        .map(|entry| {
            Ok((
                PositionKey {
                    instance: instance_id(&entry.instance)?,
                    atom: atom_id(&entry.anchor)?,
                },
                Change {
                    before: entry.before.map(|id| atom_id(&id)).transpose()?,
                    after: entry.after.map(|id| atom_id(&id)).transpose()?,
                },
            ))
        })
        .collect::<Result<_, String>>()?;
    set.settings = stored.settings.map(|change| Change {
        before: to_settings(&change.before),
        after: to_settings(&change.after),
    });
    set.roots = stored
        .roots
        .map(|change| {
            Ok::<_, String>(Change {
                before: change
                    .before
                    .iter()
                    .map(|id| instance_id(id))
                    .collect::<Result<_, _>>()?,
                after: change
                    .after
                    .iter()
                    .map(|id| instance_id(id))
                    .collect::<Result<_, _>>()?,
            })
        })
        .transpose()?;
    set.objects = stored
        .objects
        .into_iter()
        .map(|object| {
            Ok((
                instance_id(&object.id)?,
                Change {
                    before: object.before.map(read_scene_state).transpose()?,
                    after: object.after.map(read_scene_state).transpose()?,
                },
            ))
        })
        .collect::<Result<_, String>>()?;
    set.references = stored
        .references
        .into_iter()
        .map(|change| {
            Ok((
                read_reference(change.reference)?,
                Change {
                    before: change.before,
                    after: change.after,
                },
            ))
        })
        .collect::<Result<_, String>>()?;
    set.stereo = stored
        .stereo
        .into_iter()
        .map(|keyed| {
            Ok((
                stereo_key(&keyed.id)?,
                Change {
                    before: keyed.before.map(read_stereo_state).transpose()?,
                    after: keyed.after.map(read_stereo_state).transpose()?,
                },
            ))
        })
        .collect::<Result<_, String>>()?;
    set.stereo_groups = stored
        .stereo_groups
        .into_iter()
        .map(|keyed| {
            Ok((
                group_id(&keyed.id)?,
                Change {
                    before: keyed.before.map(read_group).transpose()?,
                    after: keyed.after.map(read_group).transpose()?,
                },
            ))
        })
        .collect::<Result<_, String>>()?;
    set.selection = Change {
        before: read_selection(stored.selection.before)?,
        after: read_selection(stored.selection.after)?,
    };
    Ok(set)
}

/// Versions 2 and 3 kept instances in ID order without an explicit page-root history.
/// Reconstruct each boundary from the instance changes; domain restore still checks that
/// the resulting changes actually replay from the stored content.
fn migrate_history_roots(roots: &[ObjectId], undo: &mut [ChangeSet], redo: &mut [ChangeSet]) {
    let initial: BTreeSet<ObjectId> = roots.iter().copied().collect();
    let mut current = initial.clone();
    for set in undo.iter_mut().rev() {
        let after: Vec<_> = current.iter().copied().collect();
        for (id, change) in &set.objects {
            if change.before.is_some() {
                current.insert(*id);
            } else {
                current.remove(id);
            }
        }
        let before: Vec<_> = current.iter().copied().collect();
        if before != after {
            set.roots = Some(Change { before, after });
        }
    }
    current = initial;
    for set in redo.iter_mut().rev() {
        let before: Vec<_> = current.iter().copied().collect();
        for (id, change) in &set.objects {
            if change.after.is_some() {
                current.insert(*id);
            } else {
                current.remove(id);
            }
        }
        let after: Vec<_> = current.iter().copied().collect();
        if before != after {
            set.roots = Some(Change { before, after });
        }
    }
}

fn parts<A: AtomRecord, B: BondRecord>(
    stored: StoredDocument<A, B>,
    history: StoredHistory<A, B>,
) -> Result<DocumentParts, String> {
    if stored.molecule.id != MOLECULE_ID {
        return Err(format!(
            "molecule {:?} is not this document's molecule {MOLECULE_ID:?}",
            stored.molecule.id
        ));
    }
    if !matches!(stored.pages.as_slice(), [page] if page.id == PAGE_ID) {
        return Err(format!("a document holds exactly one page, {PAGE_ID:?}"));
    }
    let mut instances = Vec::with_capacity(stored.instances.len());
    let mut positions = Vec::new();
    for instance in stored.instances {
        parents(&instance.page, &instance.molecule)?;
        let id = instance_id(&instance.id)?;
        instances.push(id);
        for position in instance.positions {
            let key = PositionKey {
                instance: id,
                atom: atom_id(&position.atom)?,
            };
            positions.push((key, point(position.x, position.y)?));
        }
    }
    // Older formats ordered instances by ID. Preserve that order when constructing page roots.
    let mut roots = instances.clone();
    roots.sort();
    let mut undo: Vec<ChangeSet> = history
        .undo
        .into_iter()
        .map(to_change_set)
        .collect::<Result<_, _>>()?;
    let mut redo: Vec<ChangeSet> = history
        .redo
        .into_iter()
        .map(to_change_set)
        .collect::<Result<_, _>>()?;
    migrate_history_roots(&roots, &mut undo, &mut redo);
    Ok(DocumentParts {
        fragment_displays: Vec::new(),
        abbreviations: Vec::new(),
        display_labels: Vec::new(),
        id: document_id(&stored.id)?,
        revision: stored.revision,
        title: stored.title,
        settings: Settings::default(),
        atoms: stored
            .molecule
            .atoms
            .into_iter()
            .map(|atom| Ok((atom_id(&atom.id)?, atom.state.into_atom()?)))
            .collect::<Result<_, String>>()?,
        bonds: stored
            .molecule
            .bonds
            .into_iter()
            .map(|bond| Ok((bond_id(&bond.id)?, bond.state.into_bond()?)))
            .collect::<Result<_, String>>()?,
        roots,
        objects: instances
            .into_iter()
            .map(|id| (id, SceneObject::Instance))
            .collect(),
        references: Vec::new(),
        positions,
        stereo: Vec::new(),
        stereo_groups: Vec::new(),
        selection: Selection::default(),
        next_atom: stored.next_atom,
        next_bond: stored.next_bond,
        next_object: stored.next_instance,
        next_group: 1,
        undo,
        redo,
    })
}

/// A version 1 document as current parts: the atoms' positions move into instance `s1`, and
/// each history entry's atom changes split into graph changes and `s1` position changes (an
/// entry that only moved atoms becomes position changes only). `Document::restore` then
/// checks the result as it checks any stored document.
fn migrate(stored: v1::Document, history: v1::History) -> Result<DocumentParts, String> {
    let instance = ObjectId::from_serial(MIGRATED_INSTANCE).expect("serial 1 is an identifier");
    let mut atoms = Vec::with_capacity(stored.atoms.len());
    let mut positions = Vec::with_capacity(stored.atoms.len());
    for atom in stored.atoms {
        let id = atom_id(&atom.id)?;
        let (state, position) = split_atom(atom.state)?;
        atoms.push((id, state));
        positions.push((PositionKey { instance, atom: id }, position));
    }
    let change_sets = |sets: Vec<v1::ChangeSet>| -> Result<Vec<ChangeSet>, String> {
        sets.into_iter()
            .map(|set| migrate_change_set(set, instance))
            .collect()
    };
    Ok(DocumentParts {
        fragment_displays: Vec::new(),
        abbreviations: Vec::new(),
        display_labels: Vec::new(),
        id: document_id(&stored.id)?,
        revision: stored.revision,
        title: stored.title,
        settings: Settings::default(),
        atoms,
        bonds: stored
            .bonds
            .into_iter()
            .map(|bond| Ok((bond_id(&bond.id)?, bond.state.into_bond()?)))
            .collect::<Result<_, String>>()?,
        roots: vec![instance],
        objects: vec![(instance, SceneObject::Instance)],
        references: Vec::new(),
        positions,
        stereo: Vec::new(),
        stereo_groups: Vec::new(),
        selection: Selection::default(),
        next_atom: stored.next_atom,
        next_bond: stored.next_bond,
        next_object: MIGRATED_INSTANCE + 1,
        next_group: 1,
        undo: change_sets(history.undo)?,
        redo: change_sets(history.redo)?,
    })
}

fn migrate_change_set(set: v1::ChangeSet, instance: ObjectId) -> Result<ChangeSet, String> {
    let mut atoms = Vec::new();
    let mut positions = Vec::new();
    for keyed in set.atoms {
        let id = atom_id(&keyed.id)?;
        let (before, before_position) = keyed.before.map(split_atom).transpose()?.unzip();
        let (after, after_position) = keyed.after.map(split_atom).transpose()?.unzip();
        if before != after {
            atoms.push((id, Change { before, after }));
        }
        if before_position != after_position {
            let change = Change {
                before: before_position,
                after: after_position,
            };
            positions.push((PositionKey { instance, atom: id }, change));
        }
    }
    Ok(ChangeSet {
        fragment_displays: Vec::new(),
        abbreviations: Vec::new(),
        display_labels: Vec::new(),
        title: set.title.map(|change| Change {
            before: change.before,
            after: change.after,
        }),
        settings: None,
        atoms,
        bonds: set
            .bonds
            .into_iter()
            .map(|keyed| {
                Ok((
                    bond_id(&keyed.id)?,
                    Change {
                        before: keyed.before.map(BondRecord::into_bond).transpose()?,
                        after: keyed.after.map(BondRecord::into_bond).transpose()?,
                    },
                ))
            })
            .collect::<Result<_, String>>()?,
        roots: None,
        objects: Vec::new(),
        references: Vec::new(),
        positions,
        stereo: Vec::new(),
        stereo_groups: Vec::new(),
        selection: Change::default(),
    })
}

/// A version 1 atom state as the graph atom, with the version 2 defaults, and its position.
fn split_atom(state: v1::AtomState) -> Result<(Atom, Point), String> {
    let position = point(state.position.x, state.position.y)?;
    let atom = v2::AtomState {
        element: state.element,
        charge: state.charge,
        isotope: state.isotope,
    }
    .into_atom()?;
    Ok((atom, position))
}

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
fn stereo_key(id: &str) -> Result<StereoKey, String> {
    AtomId::parse(id)
        .map(StereoKey::Center)
        .or_else(|| BondId::parse(id).map(StereoKey::DoubleBond))
        .ok_or_else(|| format!("invalid stereo record identifier {id:?}"))
}

fn stereo_key_id(key: StereoKey) -> String {
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
fn read_stereo_state(state: StoredStereoState) -> Result<StereoState, String> {
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

fn stored_stereo_state(state: &StereoState) -> StoredStereoState {
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

fn atom_state(atom: &Atom) -> AtomState {
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

fn bond_state(bond: &Bond) -> BondState {
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

impl From<LabelFont> for StoredFont {
    fn from(font: LabelFont) -> Self {
        match font {
            LabelFont::SansSerif => Self::SansSerif,
            LabelFont::Serif => Self::Serif,
            LabelFont::Monospace => Self::Monospace,
        }
    }
}

impl From<StoredFont> for LabelFont {
    fn from(font: StoredFont) -> Self {
        match font {
            StoredFont::SansSerif => Self::SansSerif,
            StoredFont::Serif => Self::Serif,
            StoredFont::Monospace => Self::Monospace,
        }
    }
}

impl From<CarbonLabels> for StoredCarbonLabels {
    fn from(labels: CarbonLabels) -> Self {
        match labels {
            CarbonLabels::Default => Self::Default,
            CarbonLabels::All => Self::All,
        }
    }
}

impl From<StoredCarbonLabels> for CarbonLabels {
    fn from(labels: StoredCarbonLabels) -> Self {
        match labels {
            StoredCarbonLabels::Default => Self::Default,
            StoredCarbonLabels::All => Self::All,
        }
    }
}

impl From<DisplayUnit> for StoredUnit {
    fn from(unit: DisplayUnit) -> Self {
        match unit {
            DisplayUnit::Pt => Self::Pt,
            DisplayUnit::Mm => Self::Mm,
            DisplayUnit::Cm => Self::Cm,
            DisplayUnit::In => Self::In,
        }
    }
}

impl From<StoredUnit> for DisplayUnit {
    fn from(unit: StoredUnit) -> Self {
        match unit {
            StoredUnit::Pt => Self::Pt,
            StoredUnit::Mm => Self::Mm,
            StoredUnit::Cm => Self::Cm,
            StoredUnit::In => Self::In,
        }
    }
}

fn stored_settings(settings: &Settings) -> StoredSettings {
    StoredSettings {
        bond_length_pt: settings.bond_length_pt,
        line_width_pt: settings.line_width_pt,
        label_font: settings.label_font.into(),
        label_weight: settings.label_weight.into(),
        label_style: settings.label_style.into(),
        label_underline: settings.label_underline,
        label_outline: settings.label_outline,
        label_shadow: settings.label_shadow,
        label_size_pt: settings.label_size_pt,
        carbon_labels: settings.carbon_labels.into(),
        display_unit: settings.display_unit.into(),
    }
}

/// Stored settings as the domain's; `Document::restore` checks their ranges.
fn to_settings(stored: &StoredSettings) -> Settings {
    Settings {
        bond_length_pt: stored.bond_length_pt,
        line_width_pt: stored.line_width_pt,
        label_font: stored.label_font.into(),
        label_weight: stored.label_weight.into(),
        label_style: stored.label_style.into(),
        label_underline: stored.label_underline,
        label_outline: stored.label_outline,
        label_shadow: stored.label_shadow,
        label_size_pt: stored.label_size_pt,
        carbon_labels: stored.carbon_labels.into(),
        display_unit: stored.display_unit.into(),
    }
}

fn stored_point(point: &Point) -> StoredPoint {
    StoredPoint {
        x: point.x(),
        y: point.y(),
    }
}

fn stored_scene_change_set(set: &ChangeSet) -> SceneChangeSet {
    SceneChangeSet {
        fragment_displays: set
            .fragment_displays
            .iter()
            .map(|(key, change)| StoredFragmentDisplayChange {
                instance: key.instance.to_string(),
                anchor: key.atom.to_string(),
                before: change.before.as_ref().map(stored_fragment_display),
                after: change.after.as_ref().map(stored_fragment_display),
            })
            .collect(),
        display_labels: set
            .display_labels
            .iter()
            .map(|(key, change)| StoredDisplayLabelChange {
                instance: key.instance.to_string(),
                anchor: key.atom.to_string(),
                before: change.before.clone(),
                after: change.after.clone(),
            })
            .collect(),
        abbreviations: set
            .abbreviations
            .iter()
            .map(|(key, change)| StoredAbbreviationChange {
                instance: key.instance.to_string(),
                anchor: key.atom.to_string(),
                before: change.before.map(|atom| atom.to_string()),
                after: change.after.map(|atom| atom.to_string()),
            })
            .collect(),
        title: set.title.as_ref().map(|change| StoredChange {
            before: change.before.clone(),
            after: change.after.clone(),
        }),
        settings: set.settings.as_ref().map(|change| StoredChange {
            before: stored_settings(&change.before),
            after: stored_settings(&change.after),
        }),
        atoms: set
            .atoms
            .iter()
            .map(|(id, change)| Keyed {
                id: id.to_string(),
                before: change.before.as_ref().map(atom_state),
                after: change.after.as_ref().map(atom_state),
            })
            .collect(),
        bonds: set
            .bonds
            .iter()
            .map(|(id, change)| Keyed {
                id: id.to_string(),
                before: change.before.as_ref().map(bond_state),
                after: change.after.as_ref().map(bond_state),
            })
            .collect(),
        roots: set.roots.as_ref().map(|change| StoredChange {
            before: change.before.iter().map(ToString::to_string).collect(),
            after: change.after.iter().map(ToString::to_string).collect(),
        }),
        objects: set
            .objects
            .iter()
            .map(|(id, change)| Keyed {
                id: id.to_string(),
                before: change.before.as_ref().map(stored_scene_state),
                after: change.after.as_ref().map(stored_scene_state),
            })
            .collect(),
        references: set
            .references
            .iter()
            .map(|(reference, change)| StoredReferenceChange {
                reference: stored_reference(reference),
                before: change.before,
                after: change.after,
            })
            .collect(),
        stereo: set
            .stereo
            .iter()
            .map(|(key, change)| Keyed {
                id: stereo_key_id(*key),
                before: change.before.as_ref().map(stored_stereo_state),
                after: change.after.as_ref().map(stored_stereo_state),
            })
            .collect(),
        stereo_groups: set
            .stereo_groups
            .iter()
            .map(|(id, change)| Keyed {
                id: id.to_string(),
                before: change.before.as_ref().map(stored_group),
                after: change.after.as_ref().map(stored_group),
            })
            .collect(),
        selection: StoredChange {
            before: stored_selection(&set.selection.before),
            after: stored_selection(&set.selection.after),
        },
        positions: set
            .positions
            .iter()
            .map(|(key, change)| PositionChange {
                instance: key.instance.to_string(),
                atom: key.atom.to_string(),
                before: change.before.as_ref().map(stored_point),
                after: change.after.as_ref().map(stored_point),
            })
            .collect(),
    }
}

fn to_change_set<A: AtomRecord, B: BondRecord>(
    stored: StoredChangeSet<A, B>,
) -> Result<ChangeSet, String> {
    let to_instance = |state: InstanceState| {
        parents(&state.page, &state.molecule).map(|()| SceneObject::Instance)
    };
    let to_point = |stored: StoredPoint| point(stored.x, stored.y);
    Ok(ChangeSet {
        fragment_displays: Vec::new(),
        abbreviations: Vec::new(),
        display_labels: Vec::new(),
        title: stored.title.map(|change| Change {
            before: change.before,
            after: change.after,
        }),
        settings: None,
        atoms: stored
            .atoms
            .into_iter()
            .map(|keyed| {
                Ok((
                    atom_id(&keyed.id)?,
                    Change {
                        before: keyed.before.map(AtomRecord::into_atom).transpose()?,
                        after: keyed.after.map(AtomRecord::into_atom).transpose()?,
                    },
                ))
            })
            .collect::<Result<_, String>>()?,
        bonds: stored
            .bonds
            .into_iter()
            .map(|keyed| {
                Ok((
                    bond_id(&keyed.id)?,
                    Change {
                        before: keyed.before.map(BondRecord::into_bond).transpose()?,
                        after: keyed.after.map(BondRecord::into_bond).transpose()?,
                    },
                ))
            })
            .collect::<Result<_, String>>()?,
        roots: None,
        objects: stored
            .instances
            .into_iter()
            .map(|keyed| {
                Ok((
                    instance_id(&keyed.id)?,
                    Change {
                        before: keyed.before.map(to_instance).transpose()?,
                        after: keyed.after.map(to_instance).transpose()?,
                    },
                ))
            })
            .collect::<Result<_, String>>()?,
        references: Vec::new(),
        stereo: Vec::new(),
        stereo_groups: Vec::new(),
        selection: Change::default(),
        positions: stored
            .positions
            .into_iter()
            .map(|change| {
                let key = PositionKey {
                    instance: instance_id(&change.instance)?,
                    atom: atom_id(&change.atom)?,
                };
                Ok((
                    key,
                    Change {
                        before: change.before.map(to_point).transpose()?,
                        after: change.after.map(to_point).transpose()?,
                    },
                ))
            })
            .collect::<Result<_, String>>()?,
    })
}

/// Checks that an instance is on the document's page and draws its molecule.
fn parents(page: &str, molecule: &str) -> Result<(), String> {
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

fn point(x: f64, y: f64) -> Result<Point, String> {
    Point::new(x, y).map_err(|error| error.to_string())
}

fn document_id(value: &str) -> Result<DocumentId, String> {
    DocumentId::parse(value).ok_or_else(|| "invalid document identifier".to_owned())
}

fn atom_id(value: &str) -> Result<AtomId, String> {
    AtomId::parse(value).ok_or_else(|| format!("invalid atom identifier {value:?}"))
}

fn bond_id(value: &str) -> Result<BondId, String> {
    BondId::parse(value).ok_or_else(|| format!("invalid bond identifier {value:?}"))
}

fn group_id(value: &str) -> Result<GroupId, String> {
    GroupId::parse(value).ok_or_else(|| format!("invalid stereo group identifier {value:?}"))
}

fn stored_group(group: &StereoGroup) -> StoredGroup {
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
fn read_group(stored: StoredGroup) -> Result<StereoGroup, String> {
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

fn instance_id(value: &str) -> Result<ObjectId, String> {
    ObjectId::parse(value).ok_or_else(|| format!("invalid scene object identifier {value:?}"))
}
