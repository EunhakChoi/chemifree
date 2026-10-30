//! The external data registry: verifies the separately supplied data a manifest lists and
//! reports, per dataset and per binding, what was verified and what was refused and why.
//!
//! A [`DataSource`] only reads bytes (a provider binds it to one explicitly configured
//! directory); every check is made here, so any source gets the same ones, in this order:
//!
//! 1. The fixture manifest: its exact identity must be provided by a separately trusted
//!    authority, then its size, strict JSON (no repeated keys), no field naming code, its
//!    version, its fields, identifiers, duplicates, and limits. A refused manifest activates
//!    nothing.
//! 2. Each file of each dataset: its path, format and schema, declared size, then the bytes
//!    read (bounded by the declared size): executable signatures, size, SHA-256, and a full
//!    typed decode. A dataset is verified only when every file is.
//! 3. Each binding: its feature, the datasets and versions it names, and whether all of them
//!    were verified. A binding is active only when all are: there is no partial activation.
//!
//! The result is an immutable [`DataStatus`]; nothing is half-applied, and nothing in it is
//! executed. Verified data computes nothing: no engine uses it yet, so every feature still
//! reports its engine as not implemented. The snapshot retains only whole verified datasets.

use std::collections::{HashMap, HashSet};

use chemifree_domain::data::{self, DataFormat, Distribution, Feature, Refusal, ValueType};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::data_formats::{self, Column, Parameters, Table};
use crate::json;

/// The manifest's file name in the data directory.
pub const MANIFEST_FILE: &str = "manifest.json";
/// The manifest version this build reads.
pub const MANIFEST_VERSION: u64 = 1;
pub const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
pub const MAX_DATA_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// The most bytes all files of a manifest may declare together.
pub const MAX_DATA_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_DATASETS: usize = 256;
/// The most files all datasets of a manifest may list together.
pub const MAX_DATA_FILES: usize = 4096;
const MAX_COLUMNS: usize = 256;
const MAX_TEXT_CHARS: usize = 1000;

/// Why a [`DataSource`] could not read a file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceError {
    /// The data directory itself does not exist or is not a directory.
    RootUnusable,
    Missing,
    NotAFile,
    /// The path resolves outside the data directory.
    Escapes,
    /// It holds more bytes than the limit asked for.
    TooLarge,
    /// Any other failure, described without the file's location.
    Io(String),
}

/// Reads files of one data directory. Implementations read only below that directory and only
/// the paths asked for: no search, no network, nothing executed.
pub trait DataSource {
    /// Exact identity of a public fixture manifest, supplied by a separate trusted authority.
    /// A manifest's own distribution label or rights note is never an authorization.
    fn trusted_public_manifest_sha256(&self) -> Option<&str> {
        None
    }

    /// An identity bound by the trusted host to this explicitly selected root. A source
    /// without one cannot be used with a personal-local review.
    fn root_binding_id(&self) -> Option<&str> {
        None
    }

    /// Opens a stable subdirectory as another source with the same trusted root binding.
    /// Reads through the returned source must remain inside that subdirectory even if a
    /// path or symbolic link is replaced. Providers without this guarantee fail closed.
    ///
    /// # Errors
    /// Returns a payload-free source error when the subdirectory cannot be safely opened.
    fn scope(&self, _relative_dir: &str) -> Result<Box<dyn DataSource + '_>, SourceError> {
        Err(SourceError::Io("scoped reads are unsupported".into()))
    }

    /// Reads the file at `path`, relative to the data directory (it has passed
    /// [`data::check_relative_path`]), if it holds at most `limit` bytes.
    ///
    /// # Errors
    ///
    /// Returns why it could not be read; [`SourceError::TooLarge`] when it holds more than
    /// `limit` bytes.
    fn read(&self, path: &str, limit: u64) -> Result<Vec<u8>, SourceError>;
}

/// Which datasets a load accepts after its public manifest identity has been trusted.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DistributionPolicy {
    /// Personal datasets also need a trusted, identity-bound review; a manifest note alone
    /// cannot authorize them.
    #[default]
    RequireVerifiedRights,
    /// Public datasets only, as for checking data meant to be distributed; personal ones are
    /// refused with `distribution_not_allowed`. The caller must independently establish that
    /// the pinned public manifest is permitted for the intended scope.
    PublicOnly,
}

/// What the registry knows: the manifest's state, each dataset and binding, and per feature
/// whether its data is active. Immutable once loaded.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DataStatus {
    pub manifest: ManifestStatus,
    pub datasets: Vec<DatasetStatus>,
    pub bindings: Vec<BindingStatus>,
    pub features: Vec<FeatureStatus>,
}

/// An immutable generation of verified data. Contents are never included in status output.
pub struct DataSnapshot {
    status: DataStatus,
    files: HashMap<String, HashMap<String, LoadedFile>>,
}

/// A decoded file available only when its entire dataset has verified.
pub enum LoadedFile {
    Table(Table),
    Parameters(Parameters),
}

impl DataSnapshot {
    /// The public, payload-free report for this generation.
    #[must_use]
    pub fn status(&self) -> &DataStatus {
        &self.status
    }

    /// Returns a file only if every file in its dataset passed verification.
    #[must_use]
    pub fn file(&self, dataset: &str, path: &str) -> Option<&LoadedFile> {
        self.files.get(dataset)?.get(path)
    }

    fn refused(refusal: Refusal, message: impl Into<String>) -> Self {
        Self {
            status: DataStatus::refused(refusal, message),
            files: HashMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ManifestStatus {
    pub state: ManifestState,
    /// A refusal code when `state` is `refused`.
    pub reason: Option<&'static str>,
    pub message: Option<String>,
    /// The manifest's version, once it could be read.
    pub version: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestState {
    /// No data directory is configured.
    NotConfigured,
    Refused,
    Verified,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    Verified,
    Refused,
    /// Not checked, because its dataset was refused before its files were read.
    NotChecked,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DatasetStatus {
    pub id: String,
    pub version: String,
    pub distribution: &'static str,
    pub state: CheckState,
    /// The first refused file's code, or the dataset's own.
    pub reason: Option<&'static str>,
    pub message: Option<String>,
    pub files: Vec<FileStatus>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FileStatus {
    /// Relative to the data directory, as the manifest lists it.
    pub path: String,
    pub format: String,
    pub format_version: u64,
    pub bytes: u64,
    pub sha256: String,
    pub state: CheckState,
    pub reason: Option<&'static str>,
    pub message: Option<String>,
    /// Rows of a table or fields of a parameter set, once verified.
    pub records: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BindingStatus {
    pub feature: String,
    /// The operation the feature belongs to; null for a feature this build does not know.
    pub operation: Option<&'static str>,
    pub datasets: Vec<DatasetReference>,
    pub state: BindingState,
    pub reason: Option<&'static str>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetReference {
    pub dataset: String,
    pub version: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingState {
    Active,
    Inactive,
}

/// Per feature: whether its data is active, and the engine, which is never implemented yet.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FeatureStatus {
    pub id: &'static str,
    pub operation: &'static str,
    pub data: FeatureData,
    pub engine: &'static str,
    #[serde(skip)]
    feature: Feature,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureData {
    /// An active binding supplies its data.
    Active,
    /// No active binding supplies its data.
    Unavailable,
}

impl Default for DataStatus {
    fn default() -> Self {
        Self::not_configured()
    }
}

impl DataStatus {
    /// No data directory is configured: nothing is active.
    pub fn not_configured() -> Self {
        Self::without_manifest(ManifestState::NotConfigured, None, None)
    }

    fn refused(refusal: Refusal, message: impl Into<String>) -> Self {
        Self::without_manifest(ManifestState::Refused, Some(refusal), Some(message.into()))
    }

    fn without_manifest(
        state: ManifestState,
        refusal: Option<Refusal>,
        message: Option<String>,
    ) -> Self {
        Self {
            manifest: ManifestStatus {
                state,
                reason: refusal.map(Refusal::as_str),
                message,
                version: None,
            },
            datasets: Vec::new(),
            bindings: Vec::new(),
            features: features(&[]),
        }
    }

    /// Whether some active binding supplies data for `feature`.
    pub fn is_active(&self, feature: Feature) -> bool {
        self.features
            .iter()
            .any(|status| status.feature == feature && status.data == FeatureData::Active)
    }

    /// Whether the manifest, every dataset, and every binding were verified.
    pub fn is_fully_verified(&self) -> bool {
        self.manifest.state == ManifestState::Verified
            && !self.datasets.is_empty()
            && self
                .datasets
                .iter()
                .all(|dataset| dataset.state == CheckState::Verified)
            && self
                .bindings
                .iter()
                .all(|binding| binding.state == BindingState::Active)
    }
}

fn features(active: &[Feature]) -> Vec<FeatureStatus> {
    Feature::ALL
        .into_iter()
        .map(|feature| FeatureStatus {
            id: feature.as_str(),
            operation: feature
                .operation()
                .expect("scientific features name an operation")
                .as_str(),
            data: if active.contains(&feature) {
                FeatureData::Active
            } else {
                FeatureData::Unavailable
            },
            engine: "not_implemented",
            feature,
        })
        .collect()
}

/// Verifies the data `source` holds against its manifest. Never fails: every refusal is in the
/// returned status.
pub fn load_data(source: &dyn DataSource, policy: DistributionPolicy) -> DataStatus {
    load_data_snapshot(source, policy).status
}

/// Verifies and retains a single immutable generation without exposing partially checked data.
#[must_use]
pub fn load_data_snapshot(source: &dyn DataSource, policy: DistributionPolicy) -> DataSnapshot {
    let bytes = match source.read(MANIFEST_FILE, MAX_MANIFEST_BYTES) {
        Ok(bytes) => bytes,
        Err(error) => {
            let (refusal, message) = match error {
                SourceError::RootUnusable => (
                    Refusal::DataRootInvalid,
                    "The data directory does not exist or is not a directory.".to_owned(),
                ),
                SourceError::Missing => (
                    Refusal::ManifestMissing,
                    format!("The data directory has no {MANIFEST_FILE}."),
                ),
                SourceError::TooLarge => (
                    Refusal::ManifestTooLarge,
                    format!("{MANIFEST_FILE} is larger than {MAX_MANIFEST_BYTES} bytes."),
                ),
                SourceError::NotAFile | SourceError::Escapes => (
                    Refusal::ManifestMissing,
                    format!("{MANIFEST_FILE} is not a file in the data directory."),
                ),
                SourceError::Io(why) => (
                    Refusal::ManifestMissing,
                    format!("{MANIFEST_FILE} could not be read: {why}"),
                ),
            };
            return DataSnapshot::refused(refusal, message);
        }
    };
    let actual: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if source.trusted_public_manifest_sha256() != Some(actual.as_str()) {
        return DataSnapshot::refused(
            Refusal::RightsUnverified,
            "A trusted public manifest identity is required.",
        );
    }
    let manifest = match parse_manifest(&bytes) {
        Ok(manifest) => manifest,
        Err((refusal, message)) => return DataSnapshot::refused(refusal, message),
    };

    let mut files = HashMap::new();
    let datasets: Vec<DatasetStatus> = manifest
        .datasets
        .iter()
        .map(|dataset| {
            let (status, loaded) = check_dataset(source, dataset, policy);
            if let Some(loaded) = loaded {
                files.insert(dataset.id.clone(), loaded);
            }
            status
        })
        .collect();
    let bindings: Vec<BindingStatus> = manifest
        .bindings
        .iter()
        .map(|binding| check_binding(binding, &datasets))
        .collect();
    let active: Vec<Feature> = bindings
        .iter()
        .filter(|binding| binding.state == BindingState::Active)
        .filter_map(|binding| Feature::parse(&binding.feature))
        .collect();
    let status = DataStatus {
        manifest: ManifestStatus {
            state: ManifestState::Verified,
            reason: None,
            message: None,
            version: Some(MANIFEST_VERSION),
        },
        datasets,
        bindings,
        features: features(&active),
    };
    DataSnapshot { status, files }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    #[expect(dead_code, reason = "checked before the typed read")]
    manifest_version: u64,
    datasets: Vec<DatasetEntry>,
    bindings: Vec<BindingEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DatasetEntry {
    id: String,
    version: String,
    distribution: String,
    rights: String,
    files: Vec<FileEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileEntry {
    path: String,
    bytes: u64,
    sha256: String,
    format: String,
    format_version: u64,
    #[serde(default)]
    columns: Option<Vec<ColumnEntry>>,
    #[serde(default)]
    fields: Option<Vec<ColumnEntry>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColumnEntry {
    name: String,
    #[serde(rename = "type")]
    value_type: String,
    #[serde(default)]
    unique: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingEntry {
    feature: String,
    datasets: Vec<DatasetReference>,
}

type ManifestRefusal = (Refusal, String);

fn parse_manifest(bytes: &[u8]) -> Result<Manifest, ManifestRefusal> {
    let invalid = |message: String| (Refusal::ManifestInvalid, message);
    let value = json::parse_strict(bytes).map_err(|error| {
        invalid(format!(
            "{MANIFEST_FILE} is not UTF-8 JSON without repeated keys: {error}"
        ))
    })?;
    if let Some(key) = code_key(&value) {
        return Err((
            Refusal::ExecutableReference,
            format!("The manifest field `{key}` would name code to run; data never does."),
        ));
    }
    match value.get("manifest_version").map(Value::as_u64) {
        Some(Some(MANIFEST_VERSION)) => {}
        Some(Some(version)) => {
            return Err((
                Refusal::ManifestVersionUnsupported,
                format!(
                    "Manifest version {version} is not supported; this build reads version \
                     {MANIFEST_VERSION}."
                ),
            ));
        }
        _ => {
            return Err(invalid(
                "manifest_version must be a non-negative integer.".into(),
            ));
        }
    }
    let manifest: Manifest = serde_json::from_value(value)
        .map_err(|error| invalid(format!("The manifest is not valid: {error}")))?;
    check_manifest(&manifest)?;
    Ok(manifest)
}

/// The first object key anywhere in `value` that would name code.
fn code_key(value: &Value) -> Option<&str> {
    match value {
        Value::Object(fields) => fields.iter().find_map(|(key, field)| {
            if data::is_code_key(key) {
                Some(key.as_str())
            } else {
                code_key(field)
            }
        }),
        Value::Array(items) => items.iter().find_map(code_key),
        _ => None,
    }
}

fn check_manifest(manifest: &Manifest) -> Result<(), ManifestRefusal> {
    let invalid = |message: String| Err((Refusal::ManifestInvalid, message));
    let file_count: usize = manifest
        .datasets
        .iter()
        .map(|dataset| dataset.files.len())
        .sum();
    let total_bytes = manifest
        .datasets
        .iter()
        .flat_map(|dataset| &dataset.files)
        .try_fold(0_u64, |total, file| total.checked_add(file.bytes));
    if manifest.datasets.len() > MAX_DATASETS
        || file_count > MAX_DATA_FILES
        || total_bytes.is_none_or(|total| total > MAX_DATA_TOTAL_BYTES)
    {
        return Err((
            Refusal::LimitExceeded,
            format!(
                "A manifest lists at most {MAX_DATASETS} datasets and {MAX_DATA_FILES} files of \
                 {MAX_DATA_TOTAL_BYTES} bytes together."
            ),
        ));
    }
    let mut ids = HashSet::new();
    for dataset in &manifest.datasets {
        if !data::is_dataset_id(&dataset.id) {
            return invalid(format!(
                "Dataset ID `{}` is not 1–64 lowercase letters, digits, `.`, `_`, or `-`.",
                dataset.id
            ));
        }
        if !ids.insert(dataset.id.as_str()) {
            return Err((
                Refusal::DuplicateId,
                format!("Dataset `{}` is listed twice.", dataset.id),
            ));
        }
        if !is_short_text(&dataset.version, 64) {
            return invalid(format!(
                "Dataset `{}` needs a version of 1–64 characters.",
                dataset.id
            ));
        }
        if !is_short_text(&dataset.rights, MAX_TEXT_CHARS) {
            return invalid(format!(
                "Dataset `{}` needs a rights note of 1–{MAX_TEXT_CHARS} characters.",
                dataset.id
            ));
        }
        if Distribution::parse(&dataset.distribution).is_none() {
            return invalid(format!(
                "Dataset `{}` has distribution `{}`; it must be public or personal.",
                dataset.id, dataset.distribution
            ));
        }
        if dataset.files.is_empty() {
            return invalid(format!("Dataset `{}` lists no files.", dataset.id));
        }
        let mut paths = HashSet::new();
        for file in &dataset.files {
            if !is_sha256(&file.sha256) {
                return invalid(format!(
                    "Dataset `{}`: sha256 must be 64 lowercase hexadecimal digits.",
                    dataset.id
                ));
            }
            if !paths.insert(file.path.as_str()) {
                return Err((
                    Refusal::DuplicateId,
                    format!("Dataset `{}` lists a file twice.", dataset.id),
                ));
            }
        }
    }
    let mut features = HashSet::new();
    for binding in &manifest.bindings {
        if !features.insert(binding.feature.as_str()) {
            return Err((
                Refusal::DuplicateId,
                format!("Feature `{}` is bound twice.", binding.feature),
            ));
        }
        if binding.datasets.is_empty() {
            return invalid(format!(
                "The binding of `{}` lists no datasets.",
                binding.feature
            ));
        }
        let mut referenced = HashSet::new();
        for reference in &binding.datasets {
            if !referenced.insert(reference.dataset.as_str()) {
                return Err((
                    Refusal::DuplicateId,
                    format!(
                        "The binding of `{}` lists dataset `{}` twice.",
                        binding.feature, reference.dataset
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn is_short_text(text: &str, max_chars: usize) -> bool {
    !text.trim().is_empty()
        && text.chars().count() <= max_chars
        && !text.chars().any(char::is_control)
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn check_dataset(
    source: &dyn DataSource,
    dataset: &DatasetEntry,
    policy: DistributionPolicy,
) -> (DatasetStatus, Option<HashMap<String, LoadedFile>>) {
    let distribution =
        Distribution::parse(&dataset.distribution).expect("the manifest check parsed it");
    let policy_refusal = match (policy, distribution) {
        (DistributionPolicy::PublicOnly, Distribution::Personal) => Some((
            Refusal::DistributionNotAllowed,
            "Only public datasets are accepted here.".to_owned(),
        )),
        (DistributionPolicy::RequireVerifiedRights, Distribution::Personal) => Some((
            Refusal::RightsUnverified,
            "This generic manifest cannot bind a trusted personal-use review.".to_owned(),
        )),
        (_, Distribution::Public) => None,
    };
    let mut loaded = HashMap::new();
    let files: Vec<FileStatus> = dataset
        .files
        .iter()
        .map(|file| {
            let mut status = FileStatus {
                path: file.path.clone(),
                format: file.format.clone(),
                format_version: file.format_version,
                bytes: file.bytes,
                sha256: file.sha256.clone(),
                state: CheckState::NotChecked,
                reason: None,
                message: None,
                records: None,
            };
            if policy_refusal.is_none() {
                match check_file(source, file) {
                    Ok(content) => {
                        status.state = CheckState::Verified;
                        status.records = Some(match &content {
                            LoadedFile::Table(table) => table.rows.len() as u64,
                            LoadedFile::Parameters(parameters) => parameters.values.len() as u64,
                        });
                        loaded.insert(file.path.clone(), content);
                    }
                    Err((refusal, message)) => {
                        status.state = CheckState::Refused;
                        status.reason = Some(refusal.as_str());
                        status.message = Some(message);
                    }
                }
            }
            status
        })
        .collect();
    let (state, reason, message) = if let Some((refusal, message)) = policy_refusal {
        (CheckState::Refused, Some(refusal.as_str()), Some(message))
    } else if let Some(refused) = files.iter().find(|file| file.state == CheckState::Refused) {
        (
            CheckState::Refused,
            refused.reason,
            refused
                .message
                .as_ref()
                .map(|message| format!("{}: {message}", refused.path)),
        )
    } else {
        (CheckState::Verified, None, None)
    };
    let retained = (state == CheckState::Verified).then_some(loaded);
    (
        DatasetStatus {
            id: dataset.id.clone(),
            version: dataset.version.clone(),
            distribution: distribution.as_str(),
            state,
            reason,
            message,
            files,
        },
        retained,
    )
}

/// The schema a file declares, checked against its format.
fn schema(file: &FileEntry, format: DataFormat) -> Result<Vec<Column>, String> {
    let (declared, absent, name) = match format {
        DataFormat::TableJsonl => (&file.columns, &file.fields, "columns"),
        DataFormat::ParametersJson => (&file.fields, &file.columns, "fields"),
    };
    if absent.is_some() {
        return Err(format!("{} declares only {name}", format.as_str()));
    }
    let Some(declared) = declared.as_ref().filter(|declared| !declared.is_empty()) else {
        return Err(format!(
            "{} needs at least one of its {name}",
            format.as_str()
        ));
    };
    if declared.len() > MAX_COLUMNS {
        return Err(format!("at most {MAX_COLUMNS} {name} are read"));
    }
    let mut names = HashSet::new();
    let mut columns = Vec::with_capacity(declared.len());
    for entry in declared {
        if !is_short_text(&entry.name, 128) || !names.insert(entry.name.as_str()) {
            return Err(format!(
                "each of the {name} needs a distinct name of 1–128 characters"
            ));
        }
        let Some(value_type) = ValueType::parse(&entry.value_type) else {
            return Err(format!(
                "`{}` has type `{}`; types are integer, number, string, and boolean",
                entry.name, entry.value_type
            ));
        };
        if entry.unique
            && (format != DataFormat::TableJsonl
                || !matches!(value_type, ValueType::Integer | ValueType::String))
        {
            return Err(format!(
                "`{}` cannot be unique: only integer and string table columns can",
                entry.name
            ));
        }
        columns.push(Column {
            name: entry.name.clone(),
            value_type,
            unique: entry.unique,
        });
    }
    Ok(columns)
}

/// Checks one file and returns its decoded contents from the same bytes that were hashed.
fn check_file(source: &dyn DataSource, file: &FileEntry) -> Result<LoadedFile, ManifestRefusal> {
    data::check_relative_path(&file.path)
        .map_err(|why| (Refusal::PathInvalid, format!("the path {why}")))?;
    if data::has_executable_extension(&file.path) {
        return Err((
            Refusal::ExecutableContent,
            "the file is named like an executable, library, or script".into(),
        ));
    }
    let format = DataFormat::parse(&file.format)
        .filter(|format| format.version() == file.format_version)
        .ok_or_else(|| {
            (
                Refusal::FormatUnsupported,
                format!(
                    "format `{}` version {} is not read by this build; it reads {}",
                    file.format,
                    file.format_version,
                    DataFormat::ALL
                        .map(|format| format!("{} version {}", format.as_str(), format.version()))
                        .join(" and ")
                ),
            )
        })?;
    let columns = schema(file, format).map_err(|why| (Refusal::SchemaInvalid, why))?;
    if file.bytes > MAX_DATA_FILE_BYTES {
        return Err((
            Refusal::FileTooLarge,
            format!("files of up to {MAX_DATA_FILE_BYTES} bytes are read"),
        ));
    }
    let bytes = source
        .read(&file.path, file.bytes)
        .map_err(|error| match error {
            SourceError::Missing => (Refusal::FileMissing, "the file does not exist".to_owned()),
            SourceError::NotAFile => (Refusal::NotAFile, "it is not a regular file".to_owned()),
            SourceError::Escapes => (
                Refusal::PathEscape,
                "it resolves outside the data directory".to_owned(),
            ),
            SourceError::TooLarge => (
                Refusal::SizeMismatch,
                format!("it holds more than the declared {} bytes", file.bytes),
            ),
            SourceError::RootUnusable => (
                Refusal::ReadFailed,
                "the data directory is no longer usable".to_owned(),
            ),
            SourceError::Io(why) => (Refusal::ReadFailed, format!("it could not be read: {why}")),
        })?;
    if let Some(kind) = data::executable_signature(&bytes) {
        return Err((
            Refusal::ExecutableContent,
            format!("it starts like {kind}; data is never executed"),
        ));
    }
    if bytes.len() as u64 != file.bytes {
        return Err((
            Refusal::SizeMismatch,
            format!(
                "it holds {} bytes, not the declared {}",
                bytes.len(),
                file.bytes
            ),
        ));
    }
    let digest: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if digest != file.sha256 {
        return Err((
            Refusal::HashMismatch,
            "its SHA-256 differs from the declared one".into(),
        ));
    }
    let content = match format {
        DataFormat::TableJsonl => {
            data_formats::decode_table(&bytes, &columns).map(LoadedFile::Table)
        }
        DataFormat::ParametersJson => {
            data_formats::decode_parameters(&bytes, &columns).map(LoadedFile::Parameters)
        }
    }
    .map_err(|why| (Refusal::ContentInvalid, why))?;
    Ok(content)
}

fn check_binding(binding: &BindingEntry, datasets: &[DatasetStatus]) -> BindingStatus {
    let feature = Feature::parse(&binding.feature);
    let by_id: HashMap<&str, &DatasetStatus> = datasets
        .iter()
        .map(|dataset| (dataset.id.as_str(), dataset))
        .collect();
    let mut refusal = if feature.is_none() {
        Some((
            Refusal::UnknownFeature,
            format!(
                "Feature `{}` is not one this build knows: {}.",
                binding.feature,
                Feature::ALL.map(Feature::as_str).join(", ")
            ),
        ))
    } else {
        binding
            .datasets
            .iter()
            .find_map(|reference| match by_id.get(reference.dataset.as_str()) {
                None => Some((
                    Refusal::UnknownDataset,
                    format!("Dataset `{}` is not in the manifest.", reference.dataset),
                )),
                Some(dataset) if dataset.version != reference.version => Some((
                    Refusal::VersionMismatch,
                    format!(
                        "It needs version {} of dataset `{}`; the manifest lists version {}.",
                        reference.version, reference.dataset, dataset.version
                    ),
                )),
                Some(dataset) if dataset.state != CheckState::Verified => Some((
                    Refusal::DatasetRefused,
                    format!("Dataset `{}` was refused.", reference.dataset),
                )),
                Some(_) => None,
            })
    };
    if refusal.is_none() && feature.is_some_and(|feature| feature.operation().is_some()) {
        refusal = Some((
            Refusal::ProfileUnsupported,
            "Generic self-declared tables do not establish this feature's data profile.".into(),
        ));
    }
    let (state, reason, message) = match refusal {
        Some((refusal, message)) => (
            BindingState::Inactive,
            Some(refusal.as_str()),
            Some(message),
        ),
        None => (BindingState::Active, None, None),
    };
    BindingStatus {
        feature: binding.feature.clone(),
        operation: feature
            .and_then(Feature::operation)
            .map(|operation| operation.as_str()),
        datasets: binding.datasets.clone(),
        state,
        reason,
        message,
    }
}
