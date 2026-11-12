//! Chemifree document file format, version 30 (`*.json`), and the recovery draft format that
//! wraps the same document and history. This is Chemifree's own working format, not an
//! interchange format such as MOL or CDXML.
//!
//! Version 30 adds embedded immutable PNG images; v1–29 forbid images in current/history.
//! Version 29 adds single open cubic spline graphics; v1–28 forbid them in current/history.
//! Version 28 adds bounded polygon vertices; v1–27 forbid polygons in current/history.
//! Version 27 adds axis-aligned ellipse graphics; v1–26 forbid them in current/history.
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
//! so they read with none. Reading never writes; the next write is version 30. Other versions
//! are refused.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};

use records::{
    AtomState, BondState, Keyed, PositionChange, Stored, StoredChange, StoredMolecule, StoredPage,
    StoredPoint, StoredStereoState, UnmarkedBondState,
};
use scene::{ScenePosition, SceneState, StoredReference, StoredReferenceChange, StoredSelection};
use settings::StoredSettings;

mod changes;
mod checks;
mod graph;
mod migrate;
mod objects;
mod read;
mod records;
mod scene;
mod settings;
mod write;

pub(crate) use read::{
    decode, decode_draft, decode_draft_info, decode_revision, earlier_draft_version,
    earlier_version,
};
pub(crate) use settings::{
    StoredCarbonLabels, StoredFont, StoredLabelStyle, StoredLabelWeight, StoredUnit,
};
pub(crate) use write::{encode, encode_draft};

const FORMAT: &str = "chemifree.document";
const FORMAT_VERSION: u32 = 30;
const DRAFT_FORMAT: &str = "chemifree.draft";
const DRAFT_FORMAT_VERSION: u32 = 30;
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
    TwentyNine,
    TwentyEight,
    TwentySeven,
    TwentySix,
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
        29 => Ok(Version::TwentyNine),
        28 => Ok(Version::TwentyEight),
        27 => Ok(Version::TwentySeven),
        26 => Ok(Version::TwentySix),
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
             {VERSION_1}, {VERSION_2}, {VERSION_3}, {VERSION_4}, {VERSION_5}, {VERSION_6}, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, and \
             {current}"
        )),
    }
}

fn parse<T: DeserializeOwned>(bytes: &[u8], kind: &str) -> Result<T, String> {
    serde_json::from_slice(bytes).map_err(|error| format!("invalid {kind} file: {error}"))
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
        Ok(Version::TwentyNine) => Some(29),
        Ok(Version::TwentyEight) => Some(28),
        Ok(Version::TwentySeven) => Some(27),
        Ok(Version::TwentySix) => Some(26),
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
