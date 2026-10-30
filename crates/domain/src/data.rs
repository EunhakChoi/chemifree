//! Vocabulary and rules of the external data registry: separately supplied datasets that a
//! manifest lists, the features they are bound to, and the reasons a dataset or binding is
//! refused.
//!
//! Data is only ever data. A dataset is a set of files in a declarative format Chemifree
//! defines; nothing in it names code to run, and content that looks executable is refused.
//! Verified data computes nothing: a feature also needs an engine, and none is implemented.

use crate::operation::OperationId;

/// A feature that would use separately supplied data, and the operation it belongs to. The set
/// is closed: a manifest binding any other feature is refused.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Feature {
    /// Numeric property prediction (for example pKa or logP).
    NumericPrediction,
    /// NMR shift prediction.
    NmrPrediction,
    /// Parsing a chemical name into a structure.
    NameToStructure,
    /// Generating a chemical name for a structure.
    StructureToName,
    /// Ring templates for structure layout.
    RingTemplates,
    /// A test fixture binding with no associated scientific operation.
    FixtureTable,
    /// A second test fixture binding used to exercise multi-dataset validation.
    FixtureComposite,
}

impl Feature {
    pub const ALL: [Self; 5] = [
        Self::NumericPrediction,
        Self::NmrPrediction,
        Self::NameToStructure,
        Self::StructureToName,
        Self::RingTemplates,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NumericPrediction => "prediction.numeric",
            Self::NmrPrediction => "prediction.nmr",
            Self::NameToStructure => "naming.name_to_structure",
            Self::StructureToName => "naming.structure_to_name",
            Self::RingTemplates => "layout.ring_templates",
            Self::FixtureTable => "fixture.table",
            Self::FixtureComposite => "fixture.composite",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .chain([Self::FixtureTable, Self::FixtureComposite])
            .find(|feature| feature.as_str() == name)
    }

    /// The operation that would use this feature's data.
    pub const fn operation(self) -> Option<OperationId> {
        match self {
            Self::NumericPrediction | Self::NmrPrediction => Some(OperationId::PredictionEvaluate),
            Self::NameToStructure => Some(OperationId::ChemNameToStructure),
            Self::StructureToName => Some(OperationId::ChemStructureToName),
            Self::RingTemplates => Some(OperationId::LayoutStructure),
            Self::FixtureTable | Self::FixtureComposite => None,
        }
    }

    /// The features whose data `operation` would use; empty for an operation that needs none.
    pub fn of_operation(operation: OperationId) -> Vec<Self> {
        Self::ALL
            .into_iter()
            .filter(|feature| feature.operation() == Some(operation))
            .collect()
    }
}

/// Who may receive a dataset. Public data may be distributed with Chemifree; personal data is
/// supplied by its user at run time and never distributed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Distribution {
    Public,
    Personal,
}

impl Distribution {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Personal => "personal",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [Self::Public, Self::Personal]
            .into_iter()
            .find(|distribution| distribution.as_str() == name)
    }
}

/// A declarative file format a dataset file may be in. Each has one supported version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataFormat {
    /// UTF-8 JSON Lines: one JSON object per line, each with exactly the declared columns.
    TableJsonl,
    /// One UTF-8 JSON object with exactly the declared fields.
    ParametersJson,
}

impl DataFormat {
    pub const ALL: [Self; 2] = [Self::TableJsonl, Self::ParametersJson];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TableJsonl => "table-jsonl",
            Self::ParametersJson => "parameters-json",
        }
    }

    /// The format version this build reads.
    pub const fn version(self) -> u64 {
        1
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|format| format.as_str() == name)
    }
}

/// The type of a declared column or field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueType {
    /// A JSON integer that fits in 64 signed bits.
    Integer,
    /// Any JSON number.
    Number,
    String,
    Boolean,
}

impl ValueType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Integer => "integer",
            Self::Number => "number",
            Self::String => "string",
            Self::Boolean => "boolean",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [Self::Integer, Self::Number, Self::String, Self::Boolean]
            .into_iter()
            .find(|kind| kind.as_str() == name)
    }
}

/// Why the manifest, a dataset or one of its files, or a binding was refused. The codes are
/// stable: HTTP, the CLI, and the operation contract report them as they are.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    // The whole manifest: nothing is active.
    /// The configured data directory does not exist or is not a directory.
    DataRootInvalid,
    ManifestMissing,
    ManifestTooLarge,
    /// Not UTF-8 JSON without repeated keys, a missing, mistyped, or unknown field, or a
    /// malformed identifier, version, hash, or note.
    ManifestInvalid,
    ManifestVersionUnsupported,
    /// A field that would name a module, library, entry point, command, or script.
    ExecutableReference,
    /// A dataset ID, a feature, or a dataset within one binding listed twice.
    DuplicateId,
    /// More datasets, files, or bytes than the loader reads.
    LimitExceeded,

    // A dataset, through one of its files: bindings that need it stay inactive.
    /// A personal dataset where only public data is accepted.
    DistributionNotAllowed,
    /// A claimed personal-use right is not bound to a trusted review and file identity.
    RightsUnverified,
    /// Absolute, with a drive or `..` segment, or otherwise not a plain relative path.
    PathInvalid,
    /// It resolves, through a symbolic link, outside the data directory.
    PathEscape,
    FileMissing,
    NotAFile,
    /// Declared larger than the loader reads.
    FileTooLarge,
    ReadFailed,
    /// An executable or script, by its signature or its name.
    ExecutableContent,
    SizeMismatch,
    HashMismatch,
    /// A format, or a version of it, that this build does not read.
    FormatUnsupported,
    /// The declared columns or fields are unusable for the format.
    SchemaInvalid,
    /// The content does not decode as its declared format and schema: truncated, corrupt, or
    /// different from what was declared.
    ContentInvalid,

    // A binding: it stays inactive.
    UnknownFeature,
    UnknownDataset,
    /// The binding pins another version than the manifest lists.
    VersionMismatch,
    /// A dataset it needs was refused.
    DatasetRefused,
    /// Generic self-declared tables do not establish a scientific feature's profile.
    ProfileUnsupported,
}

impl Refusal {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DataRootInvalid => "data_root_invalid",
            Self::ManifestMissing => "manifest_missing",
            Self::ManifestTooLarge => "manifest_too_large",
            Self::ManifestInvalid => "manifest_invalid",
            Self::ManifestVersionUnsupported => "manifest_version_unsupported",
            Self::ExecutableReference => "executable_reference",
            Self::DuplicateId => "duplicate_id",
            Self::LimitExceeded => "limit_exceeded",
            Self::DistributionNotAllowed => "distribution_not_allowed",
            Self::RightsUnverified => "rights_unverified",
            Self::PathInvalid => "path_invalid",
            Self::PathEscape => "path_escape",
            Self::FileMissing => "file_missing",
            Self::NotAFile => "not_a_file",
            Self::FileTooLarge => "file_too_large",
            Self::ReadFailed => "read_failed",
            Self::ExecutableContent => "executable_content",
            Self::SizeMismatch => "size_mismatch",
            Self::HashMismatch => "hash_mismatch",
            Self::FormatUnsupported => "format_unsupported",
            Self::SchemaInvalid => "schema_invalid",
            Self::ContentInvalid => "content_invalid",
            Self::UnknownFeature => "unknown_feature",
            Self::UnknownDataset => "unknown_dataset",
            Self::VersionMismatch => "version_mismatch",
            Self::DatasetRefused => "dataset_refused",
            Self::ProfileUnsupported => "profile_unsupported",
        }
    }
}

/// Whether `id` can name a dataset: 1–64 characters of lowercase ASCII letters, digits, `.`,
/// `_`, and `-`, starting with a letter or digit.
pub fn is_dataset_id(id: &str) -> bool {
    id.len() <= 64
        && id
            .bytes()
            .next()
            .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        })
}

/// Checks that `path` is a plain relative path inside a data directory: `/`-separated segments
/// that are not empty, `.`, or `..`, without a backslash, a drive or stream colon, a control
/// character, or more than 512 bytes. Returns why it is not.
pub fn check_relative_path(path: &str) -> Result<(), &'static str> {
    if path.is_empty() || path.len() > 512 {
        return Err("must have 1 to 512 bytes");
    }
    if path.starts_with('/') {
        return Err("is absolute");
    }
    if path.contains('\\') || path.contains(':') {
        return Err("contains a backslash or colon");
    }
    if path.chars().any(char::is_control) {
        return Err("contains a control character");
    }
    if path
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err("has an empty, '.', or '..' segment");
    }
    Ok(())
}

/// File name endings of executables, libraries, and scripts. A dataset file named like one is
/// refused whatever it holds.
const EXECUTABLE_EXTENSIONS: [&str; 18] = [
    "exe", "dll", "sys", "com", "scr", "msi", "so", "dylib", "jar", "class", "wasm", "bat", "cmd",
    "ps1", "sh", "py", "js", "vbs",
];

/// Whether `path` names an executable, library, or script by its extension.
pub fn has_executable_extension(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').is_some_and(|(_, extension)| {
        EXECUTABLE_EXTENSIONS
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known))
    })
}

/// The kind of executable `bytes` starts like, if any: a PE (Windows), ELF, or Mach-O image, a
/// WebAssembly module, or a script with an interpreter line.
pub fn executable_signature(bytes: &[u8]) -> Option<&'static str> {
    const SIGNATURES: [(&[u8], &str); 9] = [
        (b"MZ", "a PE executable"),
        (b"\x7fELF", "an ELF executable"),
        (b"\xfe\xed\xfa\xce", "a Mach-O executable"),
        (b"\xfe\xed\xfa\xcf", "a Mach-O executable"),
        (b"\xce\xfa\xed\xfe", "a Mach-O executable"),
        (b"\xcf\xfa\xed\xfe", "a Mach-O executable"),
        (
            b"\xca\xfe\xba\xbe",
            "a universal Mach-O executable or Java class",
        ),
        (b"\0asm", "a WebAssembly module"),
        (b"#!", "a script"),
    ];
    SIGNATURES
        .iter()
        .find(|(signature, _)| bytes.starts_with(signature))
        .map(|&(_, kind)| kind)
}

/// Manifest keys that would name code to load or run. A manifest has none of them anywhere, so
/// one is refused as an attempt to reference code rather than as an unknown field.
const CODE_KEYS: [&str; 14] = [
    "module",
    "library",
    "dll",
    "entry_point",
    "entrypoint",
    "function",
    "symbol",
    "command",
    "script",
    "executable",
    "exec",
    "program",
    "interpreter",
    "plugin",
];

/// Whether a manifest key would name code to load or run, ignoring case and `-` against `_`.
pub fn is_code_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    CODE_KEYS.contains(&normalized.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_round_trip_and_belong_to_data_operations() {
        for feature in Feature::ALL {
            assert_eq!(Feature::parse(feature.as_str()), Some(feature));
        }
        assert_eq!(
            Feature::of_operation(OperationId::PredictionEvaluate),
            [Feature::NumericPrediction, Feature::NmrPrediction]
        );
        assert_eq!(
            Feature::of_operation(OperationId::LayoutStructure),
            [Feature::RingTemplates]
        );
        assert!(Feature::of_operation(OperationId::DocumentTransaction).is_empty());
        assert_eq!(
            Feature::parse("layout.Ring_Templates"),
            None,
            "names are exact"
        );
    }

    #[test]
    fn relative_paths_stay_inside_the_data_directory() {
        for valid in [
            "a.jsonl",
            "tables/rings.jsonl",
            "a/b/c.json",
            "..a/b..",
            "x y.json",
        ] {
            assert_eq!(check_relative_path(valid), Ok(()), "{valid}");
        }
        for (invalid, why) in [
            ("", "must have 1 to 512 bytes"),
            ("/etc/passwd", "is absolute"),
            ("C:/data.json", "contains a backslash or colon"),
            ("tables\\rings.jsonl", "contains a backslash or colon"),
            ("../outside.json", "has an empty, '.', or '..' segment"),
            (
                "tables/../../outside.json",
                "has an empty, '.', or '..' segment",
            ),
            ("./a.json", "has an empty, '.', or '..' segment"),
            ("a//b.json", "has an empty, '.', or '..' segment"),
            ("tables/", "has an empty, '.', or '..' segment"),
            ("a\nb.json", "contains a control character"),
        ] {
            assert_eq!(check_relative_path(invalid), Err(why), "{invalid:?}");
        }
        assert!(check_relative_path(&"a".repeat(513)).is_err());
    }

    #[test]
    fn executables_are_recognized_by_signature_and_name() {
        assert_eq!(executable_signature(b"MZ\x90\0"), Some("a PE executable"));
        assert_eq!(
            executable_signature(b"\x7fELF\x02"),
            Some("an ELF executable")
        );
        assert_eq!(
            executable_signature(b"\xcf\xfa\xed\xfe"),
            Some("a Mach-O executable")
        );
        assert_eq!(executable_signature(b"#!/bin/sh\n"), Some("a script"));
        assert_eq!(executable_signature(b"{\"key\": 1}\n"), None);
        assert_eq!(executable_signature(b""), None);
        assert!(has_executable_extension("lib/engine.DLL"));
        assert!(has_executable_extension("run.sh"));
        assert!(!has_executable_extension("tables/rings.jsonl"));
        assert!(
            !has_executable_extension("sh/rings.json"),
            "only the file name counts"
        );
    }

    #[test]
    fn dataset_ids_and_code_keys() {
        assert!(is_dataset_id("synthetic-table.v1_a"));
        for invalid in ["", "-a", "A", "a b", "a/b", &"a".repeat(65)] {
            assert!(!is_dataset_id(invalid), "{invalid:?}");
        }
        for key in ["module", "Entry-Point", "ENTRYPOINT", "script"] {
            assert!(is_code_key(key), "{key}");
        }
        assert!(!is_code_key("modules_note"));
        assert!(!is_code_key("rights"));
    }
}
