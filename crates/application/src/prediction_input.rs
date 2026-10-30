//! Typed declarative prediction operands; these do not implement a prediction engine.

use std::collections::{BTreeMap, HashSet};

use serde::de::DeserializeOwned;

use crate::{json, phase6_profiles, prediction_wire as wire};

pub(crate) const FEATURE: &str = "feature-schema.json";
pub(crate) const FILES: [&str; 11] = [
    "parameters/atomic-polarizability.json",
    "parameters/pi-charges.json",
    "parameters/sigma-charges.json",
    "parameters/residual-electronegativity.json",
    "parameters/xlogp.json",
    "parameters/pka.json",
    "parameters/logs.json",
    "parameters/accessibility.json",
    "parameters/calculator-controls.json",
    FEATURE,
    "source-bounds.json",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PredictionInputError {
    FileSet,
    Shape,
    Inconsistent,
    CatalogMismatch,
}
impl std::fmt::Display for PredictionInputError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::FileSet => "invalid_file_set",
            Self::Shape => "invalid_data_record",
            Self::Inconsistent => "inconsistent_dataset",
            Self::CatalogMismatch => "catalog_identity_mismatch",
        })
    }
}
impl std::error::Error for PredictionInputError {}

/// An immutable, complete collection of typed inputs. It is not an executable model.
#[derive(PartialEq)]
pub struct PredictionInputs {
    pub polarizability: wire::PolarizabilityPayload,
    pub pi: wire::PiParametersPayload,
    pub sigma: wire::SigmaParametersPayload,
    pub residual_en: wire::ResidualEnPayload,
    pub xlogp: wire::XlogpPayload,
    pub pka: wire::PkaPayload,
    pub logs: wire::LogsPayload,
    pub accessibility: wire::AccessibilityPayload,
    pub controls: wire::CalculatorControlsPayload,
    pub features: wire::PredictionFeatureMetadata,
    pub sources: wire::PredictionSourceBounds,
}

/// Checks catalog identities against actual metadata bytes.
///
/// # Errors
/// Rejects differing names, versions, byte digests or metadata limits.
pub fn verify_catalog(
    id: &str,
    version: &str,
    manifest: &[u8],
    registry: &[u8],
    profile: &[u8],
) -> Result<(), PredictionInputError> {
    let identity = phase6_profiles::CatalogIdentity {
        dataset_id: "chemdraw16-personal-local-prediction-parameters-phase6",
        dataset_version: "sha256-set-c8fc1873da505cd087bb21de16637e94e8e60ff10f96e4d433884fa3f9f86895",
        registry_sha256: "8d1d6d59cc544e683ba783b1341c587bc315021e5717a6600ca2965eb6c42568",
        profile_sha256: "bb46b22ff0150edec6eb58cba3d90891764f308b38b9fd77b08a4ca05ae687f9",
        manifest_file_sha256: "79dda2368fc2b618ae243e60c5366e3945f8e1fdd1c2def53f339726ec19f95e",
    };
    if id != identity.dataset_id || version != identity.dataset_version {
        return Err(PredictionInputError::CatalogMismatch);
    }
    identity
        .check_file_bytes(manifest, registry, profile)
        .map_err(|_| PredictionInputError::CatalogMismatch)
}

fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, PredictionInputError> {
    if bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES || bytes.starts_with(b"\xef\xbb\xbf") {
        return Err(PredictionInputError::Shape);
    }
    let value = json::parse_integer_zero_strict(bytes).map_err(|_| PredictionInputError::Shape)?;
    serde_json::from_value(value).map_err(|_| PredictionInputError::Shape)
}

/// Decodes all required files together; malformed members refuse the entire set.
///
/// # Errors
/// Rejects incomplete, repeated, malformed or mutually inconsistent files.
pub fn decode(files: &[(&str, &[u8])]) -> Result<PredictionInputs, PredictionInputError> {
    let mut inputs = BTreeMap::new();
    for (path, bytes) in files {
        if !FILES.contains(path) || inputs.insert(*path, *bytes).is_some() {
            return Err(PredictionInputError::FileSet);
        }
    }
    let find = |path| {
        inputs
            .get(path)
            .copied()
            .ok_or(PredictionInputError::FileSet)
    };
    let decoded = PredictionInputs {
        polarizability: parse(find(FILES[0])?)?,
        pi: parse(find(FILES[1])?)?,
        sigma: parse(find(FILES[2])?)?,
        residual_en: parse(find(FILES[3])?)?,
        xlogp: parse(find(FILES[4])?)?,
        pka: parse(find(FILES[5])?)?,
        logs: parse(find(FILES[6])?)?,
        accessibility: parse(find(FILES[7])?)?,
        controls: parse(find(FILES[8])?)?,
        features: parse(find(FILES[9])?)?,
        sources: parse(find(FILES[10])?)?,
    };
    decoded.validate()?;
    Ok(decoded)
}

fn nonblank(text: &str) -> bool {
    !text.trim().is_empty()
}
fn digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn bounded(offset: u64, bytes: u64, parent: u64) -> bool {
    bytes > 0 && offset.checked_add(bytes).is_some_and(|end| end <= parent)
}
fn function_span(span: &wire::FunctionEnvelope, parent: u64) -> bool {
    nonblank(&span.evidence_file)
        && nonblank(&span.binding_scope)
        && digest(&span.evidence_sha256)
        && span.source_file_offset_start < span.source_file_offset_end_exclusive
        && span.source_file_offset_end_exclusive <= parent
}
fn unique<T: Eq + std::hash::Hash>(mut items: impl Iterator<Item = T>) -> bool {
    let mut seen = HashSet::new();
    items.all(|item| seen.insert(item))
}

fn regression(rows: &[wire::RegressionRow], count: u64, order: &[String]) -> bool {
    if rows.len() as u64 != count || order.len() != rows.len() || !unique(order.iter()) {
        return false;
    }
    let mut indices = HashSet::new();
    rows.iter().all(|row| {
        let Ok(index) = usize::try_from(row.index) else {
            return false;
        };
        indices.insert(index)
            && order.get(index) == Some(&row.feature_id)
            && nonblank(&row.feature_id)
    })
}

impl PredictionInputs {
    fn validate(&self) -> Result<(), PredictionInputError> {
        let features = &self.features;
        let mut pka_family_sizes: Vec<_> = self
            .pka
            .models
            .iter()
            .map(|model| model.rows.len())
            .collect();
        pka_family_sizes.sort_unstable();
        if !nonblank(&features.schema_id)
            || features.version == 0
            || !features.row_order_is_data_identity_not_code_dispatch
            || features.original_atom_type_rules_or_pass_sequence_included
            || features.original_code_or_cfg_included
            || features.peoe_element_tensor_shape[0] == 0
            || features.peoe_element_tensor_shape[1..] != [3, 4]
            || features.xlogp_axes.atom_type.start > features.xlogp_axes.atom_type.end_inclusive
            || features.xlogp_axes.correction.start > features.xlogp_axes.correction.end_inclusive
            || !unique(features.table_ids.iter())
            || features.table_ids.iter().any(|value| !nonblank(value))
            || self.sigma.default_controls != self.controls.peoe_default_numeric_controls
            || !self
                .sigma
                .tables
                .iter()
                .any(|table| matches!(table, wire::SigmaTable::ElementCubic(_)))
            || pka_family_sizes != [5, 6, 6, 6, 6]
            || self.logs.model.rows.len() != 11
            || f64::from_bits(self.logs.model.intercept_f64.bits) != 0.0
            || self.xlogp.table.rows.len() != 100
            || (1..=5).any(|distance| {
                !self
                    .accessibility
                    .distance_denominators
                    .iter()
                    .any(|row| row.graph_distance_u32 == distance)
            })
        {
            return Err(PredictionInputError::Inconsistent);
        }
        let mut table_ids = HashSet::from([
            self.polarizability.table.table_id.as_str(),
            self.residual_en.table.table_id.as_str(),
            self.xlogp.table.table_id.as_str(),
        ]);
        table_ids.extend(self.pi.tables.iter().map(|table| table.table_id.as_str()));
        table_ids.extend(self.sigma.tables.iter().map(|table| match table {
            wire::SigmaTable::Damping(table) => table.table_id.as_str(),
            wire::SigmaTable::Orbital(table) => table.table_id.as_str(),
            wire::SigmaTable::ElementCubic(table) => table.table_id.as_str(),
        }));
        if features
            .table_ids
            .iter()
            .any(|table_id| !table_ids.contains(table_id.as_str()))
        {
            return Err(PredictionInputError::Inconsistent);
        }
        if !unique(self.polarizability.table.rows.iter().map(|row| row.key_u64))
            || !unique(
                self.polarizability
                    .table
                    .rows
                    .iter()
                    .map(|row| row.row_ordinal),
            )
        {
            return Err(PredictionInputError::Inconsistent);
        }
        for table in &self.pi.tables {
            if !unique(
                table
                    .rows
                    .iter()
                    .map(|row| (row.key_u64, &row.parameter_family)),
            ) || !unique(table.rows.iter().map(|row| row.row_ordinal))
            {
                return Err(PredictionInputError::Inconsistent);
            }
        }
        for table in &self.sigma.tables {
            let good = match table {
                wire::SigmaTable::Damping(table) => {
                    unique(table.rows.iter().map(|row| row.key_u64))
                        && unique(table.rows.iter().map(|row| row.row_ordinal))
                }
                wire::SigmaTable::Orbital(table) => {
                    unique(table.rows.iter().map(|row| row.key_u64))
                        && unique(table.rows.iter().map(|row| row.row_ordinal))
                }
                wire::SigmaTable::ElementCubic(table) => {
                    table.shape_per_element == [3, 4]
                        && table.rows.len() as u64 == features.peoe_element_tensor_shape[0]
                        && unique(table.rows.iter().map(|row| row.atomic_number_u32))
                        && unique(table.rows.iter().map(|row| row.row_ordinal))
                }
            };
            if !good {
                return Err(PredictionInputError::Inconsistent);
            }
        }
        if !unique(
            self.residual_en
                .table
                .rows
                .iter()
                .map(|row| (&row.property, row.environment_key_i32)),
        ) || !unique(
            self.residual_en
                .table
                .rows
                .iter()
                .map(|row| row.row_ordinal),
        ) || !unique(
            self.xlogp
                .table
                .rows
                .iter()
                .map(|row| (&row.kind, row.index)),
        ) || !unique(self.xlogp.table.rows.iter().map(|row| row.row_ordinal))
            || self.accessibility.distance_denominators.iter().any(|row| {
                (1..=5).contains(&row.graph_distance_u32)
                    && f64::from_bits(row.denominator_f64.bits) == 0.0
            })
            || !unique(self.pka.models.iter().map(|model| &model.model_id))
            || !unique(
                self.accessibility
                    .radii
                    .iter()
                    .map(|row| row.atomic_number_u32),
            )
            || !unique(
                self.accessibility
                    .distance_denominators
                    .iter()
                    .map(|row| row.graph_distance_u32),
            )
        {
            return Err(PredictionInputError::Inconsistent);
        }
        for model in &self.pka.models {
            let order = features
                .regression_feature_orders
                .get(&model.model_id)
                .ok_or(PredictionInputError::Inconsistent)?;
            if !regression(&model.rows, model.feature_count, order) {
                return Err(PredictionInputError::Inconsistent);
            }
        }
        let log_order = features
            .regression_feature_orders
            .get(&self.logs.model.model_id)
            .ok_or(PredictionInputError::Inconsistent)?;
        if !regression(
            &self.logs.model.rows,
            self.logs.model.feature_count,
            log_order,
        ) {
            return Err(PredictionInputError::Inconsistent);
        }
        self.validate_sources()
    }

    fn validate_sources(&self) -> Result<(), PredictionInputError> {
        let source = &self.sources;
        let size = source.source.bytes;
        let evidence = [
            &source.static_literal_profile,
            &source.parameter_bindings,
            &source.numeric_qa,
        ];
        if !nonblank(&source.record_id)
            || !nonblank(&source.source.file_name)
            || size == 0
            || !digest(&source.source.sha256)
            || !source.inert_lineage_only
            || !source.offsets_are_not_dispatch_entries
            || source.original_code_or_control_flow_included
            || evidence
                .iter()
                .any(|item| !nonblank(&item.path) || !digest(&item.sha256))
            || !function_span(&source.accessibility.constructor, size)
            || source
                .control_parameter_records
                .iter()
                .any(|item| !function_span(item, size))
            || source
                .accessibility
                .literal_radius_cells
                .iter()
                .any(|cell| {
                    !digest(&cell.sha256) || !bounded(cell.source_file_offset, cell.bytes, size)
                })
            || source
                .regression_cell_bounds
                .values()
                .flatten()
                .any(|cell| {
                    !digest(&cell.sha256) || !bounded(cell.source_file_offset, cell.bytes, size)
                })
        {
            return Err(PredictionInputError::Inconsistent);
        }
        for table in source.table_bounds.values() {
            let good = match table {
                wire::TableBounds::Bytes(span) => {
                    nonblank(&span.binding_scope)
                        && digest(&span.sha256)
                        && span.row_width_bytes > 0
                        && span
                            .row_width_bytes
                            .checked_mul(span.row_count)
                            .is_some_and(|size| size <= span.bytes)
                        && bounded(span.source_file_offset_start, span.bytes, size)
                }
                wire::TableBounds::Function(span) => {
                    nonblank(&span.evidence_file)
                        && nonblank(&span.binding_scope)
                        && digest(&span.evidence_sha256)
                        && span.source_file_offset_start < span.source_file_offset_end_exclusive
                        && span.source_file_offset_end_exclusive <= size
                        && span
                            .literal_argument_envelopes
                            .as_ref()
                            .is_none_or(|entries| {
                                entries.iter().all(|row| {
                                    row.source_file_offset_start
                                        < row.source_file_offset_end_exclusive
                                        && span
                                            .row_count
                                            .is_none_or(|count| row.row_ordinal < count)
                                        && row.source_file_offset_start
                                            >= span.source_file_offset_start
                                        && row.source_file_offset_end_exclusive
                                            <= span.source_file_offset_end_exclusive
                                })
                            })
                        && span
                            .zero_shift_default_source_file_offset
                            .is_none_or(|offset| {
                                offset >= span.source_file_offset_start
                                    && offset < span.source_file_offset_end_exclusive
                            })
                }
            };
            if !good {
                return Err(PredictionInputError::Inconsistent);
            }
        }
        Ok(())
    }
}
