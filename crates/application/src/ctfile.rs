//! Application-owned boundary for the bounded V2000 / V3000 MOL and SDF codec.
//!
//! A record is independent of the editor: in particular it can carry a third coordinate,
//! header lines and SD fields. Conversion to a drawing is a separate, loss-reporting step.
//!
//! The V3000 additions are the `CtTab` selector on the port, the record fields `ctab`, `chiral`
//! and `collections` (not serialized), and the bond field `stereo`.

use serde::{Deserialize, Serialize};

use crate::Loss;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CtFormat {
    Mol,
    Sdf,
}

/// Which connection-table dialect a record was read from or is to be written as.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CtTab {
    V2000,
    V3000,
}

/// The stereo kind named by a V3000 `COLLECTION` line (`MDLV30/STEABS`, `STERAC<n>`, `STEREL<n>`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CtCollectionKind {
    Absolute,
    And,
    Or,
}

/// A V3000 stereo collection as read, not interpreted. The codec carries the number exactly as
/// written (any `i32`, zero and negative included); it states no meaning for an atom outside
/// every collection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CtCollection {
    pub kind: CtCollectionKind,
    /// `None` for `STEABS`, which has no number.
    pub number: Option<i32>,
    /// Zero-based indices into `atoms`, in file order.
    pub atoms: Vec<usize>,
}

/// The value a coordinate has once a writer has put it in a file: four decimals, read back. The
/// writers and the export's stereo comparison both use this one function, so the comparison sees
/// the coordinates the file holds. `None` for a value that does not format as a finite number.
pub fn written_coordinate(value: f64) -> Option<f64> {
    if !value.is_finite() {
        return None;
    }
    format!("{value:.4}").parse::<f64>().ok()
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CtRecord {
    pub title: String,
    pub program: String,
    pub comment: String,
    pub atoms: Vec<CtAtom>,
    pub bonds: Vec<CtBond>,
    /// Ordered; duplicate names are retained rather than overwritten.
    pub fields: Vec<CtField>,
    /// The dialect the record was read from. Not serialized. The dialect a writer produces is
    /// the caller's `tab` argument; this field is ignored when writing.
    #[serde(skip)]
    pub ctab: CtTab,
    /// The V3000 `COUNTS` chiral flag. A reader gives it no meaning; the V3000 writer states it as
    /// given (the projection sets it for a drawing with an absolute group), and the V2000 writer
    /// states 0. Not serialized.
    #[serde(skip)]
    pub chiral: bool,
    /// V3000 stereo collections. The V3000 writer states them, the V2000 writer refuses a record
    /// that holds any. Not serialized.
    #[serde(skip)]
    pub collections: Vec<CtCollection>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CtAtom {
    pub element: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub charge: i8,
    pub isotope: Option<u16>,
    pub radical: CtRadical,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CtRadical {
    None,
    Singlet,
    Doublet,
    Triplet,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CtBond {
    /// Zero-based indices into `atoms`.
    pub begin: usize,
    pub end: usize,
    /// Single, double, or triple (1 through 3) in V2000; V3000 also carries aromatic (4).
    /// Other query bond types are outside this profile.
    pub order: u8,
    /// How the bond is drawn. A wedge or hash is narrow at `begin`; the projection swaps the
    /// endpoints when the drawing's narrow end is the other one.
    pub stereo: CtBondStereo,
}

/// The bond depiction a connection table carries: V3000 `CFG` 1, 3 and 2, and the V2000 bond
/// stereo column 1, 6 and 4 (3 on a double bond).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CtBondStereo {
    None,
    Wedge,
    Hash,
    /// Unknown or either: a wavy single bond or a crossed double bond.
    Either,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CtField {
    pub name: String,
    /// Line breaks are represented by `\n`; a blank value is the empty string.
    pub value: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CtResult<T> {
    pub value: T,
    pub losses: Vec<Loss>,
}

/// A failed conversion, associated with a one-based record and (where known) input line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CtError {
    pub code: &'static str,
    pub message: String,
    pub record: usize,
    pub line: Option<usize>,
}

/// Implemented outside the application; implementations must stage all output on failure.
pub trait CtCodec: Send + Sync {
    /// Reads V2000 records only; a V3000 connection table is refused as `unsupported_version`.
    fn read(&self, format: CtFormat, input: &[u8]) -> Result<CtResult<Vec<CtRecord>>, CtError>;
    /// Writes V2000.
    fn write(&self, format: CtFormat, records: &[CtRecord]) -> Result<CtResult<String>, CtError>;

    /// Like [`CtCodec::read`], but each record may be V2000 or V3000 (see `CtRecord::ctab`).
    /// The default accepts what `read` accepts.
    fn read_ctab(
        &self,
        format: CtFormat,
        input: &[u8],
    ) -> Result<CtResult<Vec<CtRecord>>, CtError> {
        self.read(format, input)
    }

    /// Like [`CtCodec::write`] with a chosen dialect. The default refuses V3000.
    fn write_ctab(
        &self,
        format: CtFormat,
        tab: CtTab,
        records: &[CtRecord],
    ) -> Result<CtResult<String>, CtError> {
        match tab {
            CtTab::V2000 => self.write(format, records),
            CtTab::V3000 => Err(CtError {
                code: "unsupported_version",
                message: "This codec writes V2000 only.".into(),
                record: 1,
                line: None,
            }),
        }
    }
}

/// The closed diagnostic family of a codec reason code, for the day a reason is carried in
/// `details.reason` under a family `code`: one of `malformed_input`, `unsupported_feature`,
/// `unsupported_format`, `dangling_reference` or `resource_limit`. `None` for a code this
/// table does not list (the V2000-only codes keep their own names on the wire today).
pub fn ct_reason_family(reason: &str) -> Option<&'static str> {
    CT_REASON_FAMILIES
        .iter()
        .find(|(code, _)| *code == reason)
        .map(|(_, family)| *family)
}

/// Every `v3000_*` reason, with its family (a provider test scans the V3000 source for them),
/// plus the shared framing and limit codes the V3000 path commonly produces. Codes of the
/// fixed-width V2000 columns and of the SD data section (`invalid_field`, `truncated_atom`,
/// `unsupported_counts`, ...) are not listed; this table is not a complete list of every code
/// a `read_ctab` can return.
pub const CT_REASON_FAMILIES: &[(&str, &str)] = &[
    ("empty_input", "malformed_input"),
    ("truncated", "malformed_input"),
    ("invalid_utf8", "malformed_input"),
    ("invalid_line", "malformed_input"),
    ("invalid_columns", "malformed_input"),
    ("invalid_header", "malformed_input"),
    ("reserved_title", "malformed_input"),
    ("truncated_counts", "malformed_input"),
    ("invalid_counts", "malformed_input"),
    ("missing_m_end", "malformed_input"),
    ("invalid_m_end", "malformed_input"),
    ("trailing_content", "malformed_input"),
    ("unsupported_version", "unsupported_format"),
    ("resource_limit", "resource_limit"),
    ("v3000_missing_prefix", "malformed_input"),
    ("v3000_unterminated_continuation", "malformed_input"),
    ("v3000_invalid_counts_line", "malformed_input"),
    ("v3000_count_mismatch", "malformed_input"),
    ("v3000_missing_block", "malformed_input"),
    ("v3000_duplicate_block", "malformed_input"),
    ("v3000_block_mismatch", "malformed_input"),
    ("v3000_block_not_closed", "malformed_input"),
    ("v3000_invalid_atom", "malformed_input"),
    ("v3000_invalid_bond", "malformed_input"),
    ("v3000_invalid_number", "malformed_input"),
    ("v3000_invalid_property", "malformed_input"),
    ("v3000_duplicate_property", "malformed_input"),
    ("v3000_invalid_index", "malformed_input"),
    ("v3000_duplicate_index", "malformed_input"),
    ("v3000_duplicate_bond", "malformed_input"),
    ("v3000_invalid_charge", "malformed_input"),
    ("v3000_invalid_radical", "malformed_input"),
    ("v3000_invalid_isotope", "malformed_input"),
    ("v3000_invalid_collection", "malformed_input"),
    ("v3000_empty_collection", "malformed_input"),
    ("v3000_duplicate_collection", "malformed_input"),
    ("v3000_collection_atom_shared", "malformed_input"),
    (
        "v3000_collection_member_not_stereocenter",
        "malformed_input",
    ),
    ("v3000_unknown_atom_index", "dangling_reference"),
    ("v3000_unsupported_block", "unsupported_feature"),
    ("v3000_sgroup_not_supported", "unsupported_feature"),
    ("v3000_collection_not_supported_yet", "unsupported_feature"),
    ("v3000_unsupported_3d", "unsupported_feature"),
    ("v3000_unsupported_reaction", "unsupported_feature"),
    ("v3000_unsupported_rgroup", "unsupported_feature"),
    ("v3000_unsupported_query", "unsupported_feature"),
    ("v3000_unsupported_atom", "unsupported_feature"),
    ("v3000_unsupported_atom_property", "unsupported_feature"),
    ("v3000_unsupported_bond", "unsupported_feature"),
    ("v3000_unsupported_bond_property", "unsupported_feature"),
    ("v3000_multiple_structures", "unsupported_feature"),
    ("unsupported_atom", "unsupported_feature"),
    ("unsupported_bond", "unsupported_feature"),
    ("invalid_coordinate", "malformed_input"),
    ("invalid_bond", "malformed_input"),
    ("duplicate_bond", "malformed_input"),
    ("invalid_charge", "malformed_input"),
    ("invalid_isotope", "malformed_input"),
    ("invalid_count", "malformed_input"),
];
