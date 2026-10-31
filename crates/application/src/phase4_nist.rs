//! Typed NIST snapshot input. No formula, mass or isotope calculation is implemented here.

use num_bigint::BigInt;
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;

use crate::{json, phase6_profiles};

/// The public catalog's explicit source file identity.
pub const FILE_ID: &str = "PRIVATE-NIST-COMP-4.1-SNAPSHOT-2026-10-24";

/// Payload-free rejection of a source snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NistError {
    CatalogMismatch,
    FileSet,
    Framing,
    Record,
    RecordCount,
}
impl std::fmt::Display for NistError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::CatalogMismatch => "catalog_identity_mismatch",
            Self::FileSet => "invalid_file_set",
            Self::Framing => "invalid_data_framing",
            Self::Record => "invalid_data_record",
            Self::RecordCount => "invalid_record_count",
        })
    }
}
impl std::error::Error for NistError {}

/// Verify the dataset and exact public metadata identities.
///
/// # Errors
/// Rejects a different dataset, version or metadata byte identity.
pub fn verify_catalog_files(
    dataset_id: &str,
    dataset_version: &str,
    manifest: &[u8],
    registry: &[u8],
    catalog: &[u8],
) -> Result<(), NistError> {
    let expected = phase6_profiles::CatalogIdentity {
        dataset_id: "independent-NIST-COMP-4.1-SNAPSHOT-2026-10-24",
        dataset_version: "sha256-set-145f8ddd2877436128b6062bbc3af6dba3986a3a14d36d88a722ef0bb120d986",
        registry_sha256: "8ffd2e9b4d01ce989c63cb552714e1ff01acb8f766dbbc14bc411f08c4484d25",
        profile_sha256: "5add6a3e7a6250dc5c2f280d9a193dc8aef3ba008e47d10e35e7b263c217c2a2",
        manifest_file_sha256: "c0641a1c68d58bca0990449b67c35fa4f567170134807493ef570730f92fbb4f",
    };
    if dataset_id != expected.dataset_id || dataset_version != expected.dataset_version {
        return Err(NistError::CatalogMismatch);
    }
    expected
        .check_file_bytes(manifest, registry, catalog)
        .map_err(|_| NistError::CatalogMismatch)
}

/// Selects the exact catalog file ID, role and classification.
#[must_use]
pub fn accepts_file(id: &str, role: &str, class: &str) -> bool {
    id == FILE_ID && role == "auxiliary_data" && class == "declarative_data"
}

fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}
fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
/// A field that may be absent, explicitly null, or contain a value.
#[derive(Default, PartialEq)]
pub enum OptionalText {
    #[default]
    Missing,
    Null,
    Value(String),
}

fn present_nullable<'de, D>(deserializer: D) -> Result<OptionalText, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(|value| match value {
        Some(value) => OptionalText::Value(value),
        None => OptionalText::Null,
    })
}

/// A field that may be absent or explicitly null, with no non-null variant.
#[derive(Default, PartialEq)]
pub enum OptionalNull {
    #[default]
    Missing,
    Null,
}

fn present_null<'de, D>(deserializer: D) -> Result<OptionalNull, D::Error>
where
    D: Deserializer<'de>,
{
    <()>::deserialize(deserializer).map(|()| OptionalNull::Null)
}

/// Provenance is retained as inert metadata and never used as a path or URL to access.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceIdentity {
    pub path: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub bytes: BigInt,
    pub sha256: String,
}
/// Reported source epochs, preserved without scientific interpretation.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Epochs {
    pub masses: String,
    pub isotopic_compositions: String,
    pub standard_atomic_weights: String,
}
/// Source terms and provenance remain descriptive metadata.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DistributionBasis {
    pub classification: String,
    pub classification_source: SourceIdentity,
    pub terms: String,
    pub terms_source: SourceIdentity,
    pub terms_url: String,
}
/// Published normalization declarations are not calculation policies.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Normalization {
    pub quantities: String,
    pub decimal_policy: String,
    pub estimated_marker: String,
    pub bracketed_singleton: String,
    pub missing_policy: String,
    pub symbols: String,
    pub changes: String,
}
/// Decimal spellings and stated uncertainty are preserved as source strings.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReportedNumber {
    pub raw: String,
    pub value_decimal: String,
    pub uncertainty_digits: String,
    pub absolute_uncertainty_decimal: String,
    pub estimated_marker_present: bool,
}
/// Optional variants remain separate; null and absent source values are distinct.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AtomicWeight {
    pub kind: String,
    #[serde(default, deserialize_with = "present_nullable")]
    pub raw: OptionalText,
    #[serde(default, deserialize_with = "present")]
    pub lower_decimal: Option<String>,
    #[serde(default, deserialize_with = "present")]
    pub upper_decimal: Option<String>,
    #[serde(default, deserialize_with = "present")]
    pub reported: Option<ReportedNumber>,
    #[serde(default, deserialize_with = "json::deserialize_optional_integer")]
    pub mass_number: Option<BigInt>,
    #[serde(default, deserialize_with = "present_null")]
    pub standard_atomic_weight_value: OptionalNull,
    #[serde(default, deserialize_with = "present")]
    pub default_isotope_selected: Option<bool>,
}
/// Source element entry; spelling and nullable notes remain unchanged.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Element {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub atomic_number: BigInt,
    pub symbol_as_served: String,
    pub standard_atomic_weight: AtomicWeight,
    #[serde(deserialize_with = "required_nullable")]
    pub notes_raw: Option<String>,
}
/// Composition may carry both uncertainty strings or both explicit nulls.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Composition {
    pub raw: String,
    pub value_decimal: String,
    #[serde(deserialize_with = "required_nullable")]
    pub uncertainty_digits: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub absolute_uncertainty_decimal: Option<String>,
    pub estimated_marker_present: bool,
}
/// An isotope source entry, without selecting a default or unit convention.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Isotope {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub atomic_number: BigInt,
    pub element_symbol: String,
    #[serde(deserialize_with = "required_nullable")]
    pub isotope_symbol_as_printed: Option<String>,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub mass_number: BigInt,
    pub relative_atomic_mass: ReportedNumber,
    #[serde(deserialize_with = "required_nullable")]
    pub isotopic_composition: Option<Composition>,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub source_preformatted_line: BigInt,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotWire {
    record_id: String,
    source: SourceIdentity,
    source_url: String,
    version: String,
    version_basis: SourceIdentity,
    source_evaluation_epochs: Epochs,
    distribution_basis: DistributionBasis,
    citation: String,
    normalization: Normalization,
    #[serde(deserialize_with = "json::deserialize_integer")]
    element_count: u64,
    #[serde(deserialize_with = "json::deserialize_integer")]
    isotope_count: u64,
    elements: Vec<Element>,
    isotopes: Vec<Isotope>,
    reported_composition_sums_decimal: Box<RawValue>,
    limits: Vec<String>,
}
/// An explicitly opaque JSON object retained without numeric coercion.
#[derive(PartialEq)]
pub struct OpaqueSums(String);
impl OpaqueSums {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
/// The complete snapshot and its inert lineage, not a mass calculation service.
#[derive(PartialEq)]
pub struct NistSnapshot {
    pub record_id: String,
    pub source: SourceIdentity,
    pub source_url: String,
    pub version: String,
    pub version_basis: SourceIdentity,
    pub source_evaluation_epochs: Epochs,
    pub distribution_basis: DistributionBasis,
    pub citation: String,
    pub normalization: Normalization,
    pub elements: Vec<Element>,
    pub isotopes: Vec<Isotope>,
    pub reported_composition_sums_decimal: OpaqueSums,
    pub limits: Vec<String>,
}

/// Decode one catalog-selected, closed JSON snapshot without enabling a chemistry engine.
///
/// # Errors
/// Rejects invalid file identity, malformed or oversized JSON, inconsistent counts and
/// composition uncertainty variants not present in the public schema.
pub fn decode_dataset(files: &[(&str, &[u8])]) -> Result<NistSnapshot, NistError> {
    let [(id, bytes)] = files else {
        return Err(NistError::FileSet);
    };
    if *id != FILE_ID {
        return Err(NistError::FileSet);
    }
    if bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES
        || bytes.len() as u64 > crate::MAX_DATA_TOTAL_BYTES
    {
        return Err(NistError::Framing);
    }
    json::validate_raw_strict(bytes).map_err(|_| NistError::Record)?;
    let row: SnapshotWire = serde_json::from_slice(bytes).map_err(|_| NistError::Record)?;
    if row.element_count != row.elements.len() as u64
        || row.isotope_count != row.isotopes.len() as u64
    {
        return Err(NistError::RecordCount);
    }
    if !row.reported_composition_sums_decimal.get().starts_with('{')
        || row.isotopes.iter().any(|isotope| {
            isotope
                .isotopic_composition
                .as_ref()
                .is_some_and(|composition| {
                    composition.uncertainty_digits.is_some()
                        != composition.absolute_uncertainty_decimal.is_some()
                })
        })
    {
        return Err(NistError::Record);
    }
    Ok(NistSnapshot {
        record_id: row.record_id,
        source: row.source,
        source_url: row.source_url,
        version: row.version,
        version_basis: row.version_basis,
        source_evaluation_epochs: row.source_evaluation_epochs,
        distribution_basis: row.distribution_basis,
        citation: row.citation,
        normalization: row.normalization,
        elements: row.elements,
        isotopes: row.isotopes,
        reported_composition_sums_decimal: OpaqueSums(
            row.reported_composition_sums_decimal.get().into(),
        ),
        limits: row.limits,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn fixture() -> Value {
        let source = json!({"path":"synthetic","bytes":1,"sha256":"fixture"});
        let reported = json!({"raw":"1.0","value_decimal":"1.0","uncertainty_digits":"1","absolute_uncertainty_decimal":"0.1","estimated_marker_present":false});
        json!({
            "record_id":"synthetic","source":source,"source_url":"inert:example","version":"fixture",
            "version_basis":source,"source_evaluation_epochs":{"masses":"fixture","isotopic_compositions":"fixture","standard_atomic_weights":"fixture"},
            "distribution_basis":{"classification":"fixture","classification_source":source,"terms":"fixture","terms_source":source,"terms_url":"inert:terms"},
            "citation":"fixture","normalization":{"quantities":"fixture","decimal_policy":"fixture","estimated_marker":"fixture","bracketed_singleton":"fixture","missing_policy":"fixture","symbols":"fixture","changes":"fixture"},
            "element_count":1,"isotope_count":1,
            "elements":[{"atomic_number":1,"symbol_as_served":"H","standard_atomic_weight":{"kind":"range","raw":null,"lower_decimal":"1.0","upper_decimal":"1.1","standard_atomic_weight_value":null},"notes_raw":null}],
            "isotopes":[{"atomic_number":1,"element_symbol":"H","isotope_symbol_as_printed":null,"mass_number":1,"relative_atomic_mass":reported,"isotopic_composition":{"raw":"0.5","value_decimal":"0.5","uncertainty_digits":null,"absolute_uncertainty_decimal":null,"estimated_marker_present":false},"source_preformatted_line":1}],
            "reported_composition_sums_decimal":{"opaque_fraction":"marker"},"limits":["synthetic"]
        })
    }
    fn bytes(value: &Value) -> Vec<u8> {
        serde_json::to_string(value)
            .unwrap()
            .replace(
                "\"opaque_fraction\":\"marker\"",
                "\"opaque_fraction\":0.1000000000000000000000001",
            )
            .into_bytes()
    }

    #[test]
    fn source_byte_count_is_inert_schema_integer_metadata() {
        let mut value = fixture();
        value["source"]["bytes"] = json!(-1);
        let encoded = bytes(&value);
        let data = decode_dataset(&[(FILE_ID, &encoded)]).unwrap();
        assert_eq!(data.source.bytes.to_string(), "-1");
    }

    #[test]
    fn unbounded_source_and_entry_integers_are_preserved_without_interpretation() {
        let mut fixture = fixture();
        fixture["elements"][0]["standard_atomic_weight"]["mass_number"] = json!(2);
        let magnitude = "9".repeat(400);
        let negative = format!("-{magnitude}");
        let input = String::from_utf8(bytes(&fixture))
            .unwrap()
            .replace("\"bytes\":1", &format!("\"bytes\":{negative}"))
            .replace(
                "\"atomic_number\":1",
                &format!("\"atomic_number\":{magnitude}"),
            )
            .replace("\"mass_number\":2", &format!("\"mass_number\":{negative}"))
            .replace("\"mass_number\":1", &format!("\"mass_number\":{magnitude}"))
            .replace(
                "\"source_preformatted_line\":1",
                &format!("\"source_preformatted_line\":{negative}"),
            );
        for token in [
            format!("\"bytes\":{negative}"),
            format!("\"atomic_number\":{magnitude}"),
            format!("\"mass_number\":{negative}"),
            format!("\"source_preformatted_line\":{negative}"),
        ] {
            assert!(input.contains(&token));
        }
        let data = decode_dataset(&[(FILE_ID, input.as_bytes())]).unwrap();
        assert_eq!(data.source.bytes.to_string(), negative);
        assert_eq!(data.elements[0].atomic_number.to_string(), magnitude);
        assert_eq!(
            data.elements[0]
                .standard_atomic_weight
                .mass_number
                .as_ref()
                .unwrap()
                .to_string(),
            negative
        );
        assert_eq!(data.isotopes[0].mass_number.to_string(), magnitude);
        assert_eq!(
            data.isotopes[0].source_preformatted_line.to_string(),
            negative
        );
        assert_eq!(
            data.reported_composition_sums_decimal.as_str(),
            r#"{"opaque_fraction":0.1000000000000000000000001}"#
        );
        let overlong = input.replace(
            &format!("\"source_preformatted_line\":{negative}"),
            &format!("\"source_preformatted_line\":-{}", "9".repeat(4097)),
        );
        assert_eq!(
            decode_dataset(&[(FILE_ID, overlong.as_bytes())]).err(),
            Some(NistError::Record)
        );
    }

    #[test]
    fn opaque_sums_preserve_numbers_but_refuse_ambiguous_duplicate_keys() {
        let input = String::from_utf8(bytes(&fixture())).unwrap();
        let large = input.replace(
            "\"opaque_fraction\":0.1000000000000000000000001",
            "\"opaque_fraction\":1e9999",
        );
        let data = decode_dataset(&[(FILE_ID, large.as_bytes())]).unwrap();
        assert_eq!(
            data.reported_composition_sums_decimal.as_str(),
            r#"{"opaque_fraction":1e9999}"#
        );
        let duplicate = input.replace(
            "\"opaque_fraction\":0.1000000000000000000000001",
            "\"opaque_fraction\":0,\"opaque_fraction\":1",
        );
        assert_eq!(
            decode_dataset(&[(FILE_ID, duplicate.as_bytes())]).err(),
            Some(NistError::Record)
        );
    }

    #[test]
    fn schema_integer_values_preserve_opaque_composition_and_optional_fields() {
        let mut value = fixture();
        value["source"]["bytes"] = json!(1.0);
        value["version_basis"]["bytes"] = json!(1.0);
        value["distribution_basis"]["classification_source"]["bytes"] = json!(1.0);
        value["distribution_basis"]["terms_source"]["bytes"] = json!(1.0);
        value["element_count"] = json!(1.0);
        value["isotope_count"] = json!(1.0);
        value["elements"][0]["atomic_number"] = json!(1.0);
        value["elements"][0]["standard_atomic_weight"]["mass_number"] = json!(1.0);
        value["isotopes"][0]["atomic_number"] = json!(1.0);
        value["isotopes"][0]["mass_number"] = json!(1.0);
        value["isotopes"][0]["source_preformatted_line"] = json!(1.0);
        let text = String::from_utf8(bytes(&value)).unwrap().replace(
            "\"opaque_fraction\":0.1000000000000000000000001",
            "\"opaque_fraction\":-0.0",
        );
        let data = decode_dataset(&[(FILE_ID, text.as_bytes())]).unwrap();
        assert_eq!(data.source.bytes.to_string(), "1");
        assert_eq!(data.elements[0].atomic_number.to_string(), "1");
        assert_eq!(
            data.elements[0]
                .standard_atomic_weight
                .mass_number
                .as_ref()
                .unwrap()
                .to_string(),
            "1"
        );
        assert_eq!(data.isotopes[0].mass_number.to_string(), "1");
        assert_eq!(
            data.reported_composition_sums_decimal.as_str(),
            r#"{"opaque_fraction":-0.0}"#
        );
        let exponent = text.replace("\"element_count\":1.0", "\"element_count\":1e0");
        assert!(exponent.contains("\"element_count\":1e0"));
        assert!(decode_dataset(&[(FILE_ID, exponent.as_bytes())]).is_ok());
        let invalid = text.replace(
            "\"element_count\":1.0",
            "\"element_count\":1.00000000000000000001",
        );
        assert_eq!(
            decode_dataset(&[(FILE_ID, invalid.as_bytes())]).err(),
            Some(NistError::Record)
        );
    }

    #[test]
    fn snapshot_preserves_nullable_variants_exact_decimals_and_opaque_lineage() {
        let bytes = bytes(&fixture());
        let snapshot = decode_dataset(&[(FILE_ID, &bytes)]).unwrap();
        assert_eq!(snapshot.elements.len(), 1);
        assert_eq!(snapshot.isotopes.len(), 1);
        assert_eq!(snapshot.source.path, "synthetic");
        assert!(matches!(
            snapshot.elements[0].standard_atomic_weight.raw,
            OptionalText::Null
        ));
        assert!(matches!(
            snapshot.elements[0]
                .standard_atomic_weight
                .standard_atomic_weight_value,
            OptionalNull::Null
        ));
        assert!(
            snapshot.isotopes[0]
                .isotopic_composition
                .as_ref()
                .unwrap()
                .uncertainty_digits
                .is_none()
        );
        assert_eq!(
            snapshot.isotopes[0].relative_atomic_mass.value_decimal,
            "1.0"
        );
        assert_eq!(
            snapshot.reported_composition_sums_decimal.as_str(),
            r#"{"opaque_fraction":0.1000000000000000000000001}"#
        );
    }

    #[test]
    fn missing_and_null_fields_and_composition_variants_are_distinguished() {
        let mut missing = fixture();
        missing["elements"][0]
            .as_object_mut()
            .unwrap()
            .remove("notes_raw");
        assert_eq!(
            decode_dataset(&[(FILE_ID, &bytes(&missing))]).err(),
            Some(NistError::Record)
        );
        let mut optional_null = fixture();
        optional_null["elements"][0]["standard_atomic_weight"]["lower_decimal"] = Value::Null;
        assert_eq!(
            decode_dataset(&[(FILE_ID, &bytes(&optional_null))]).err(),
            Some(NistError::Record)
        );
        let mut null_mass_number = fixture();
        null_mass_number["elements"][0]["standard_atomic_weight"]["mass_number"] = Value::Null;
        assert_eq!(
            decode_dataset(&[(FILE_ID, &bytes(&null_mass_number))]).err(),
            Some(NistError::Record)
        );
        let mut mixed = fixture();
        mixed["isotopes"][0]["isotopic_composition"]["uncertainty_digits"] = json!("1");
        assert_eq!(
            decode_dataset(&[(FILE_ID, &bytes(&mixed))]).err(),
            Some(NistError::Record)
        );
        let mut stated = fixture();
        stated["isotopes"][0]["isotopic_composition"]["uncertainty_digits"] = json!("1");
        stated["isotopes"][0]["isotopic_composition"]["absolute_uncertainty_decimal"] =
            json!("0.1");
        assert!(decode_dataset(&[(FILE_ID, &bytes(&stated))]).is_ok());
        let mut no_raw = fixture();
        no_raw["elements"][0]["standard_atomic_weight"]
            .as_object_mut()
            .unwrap()
            .remove("raw");
        let snapshot = decode_dataset(&[(FILE_ID, &bytes(&no_raw))]).unwrap();
        assert!(matches!(
            snapshot.elements[0].standard_atomic_weight.raw,
            OptionalText::Missing
        ));
        let mut value_raw = fixture();
        value_raw["elements"][0]["standard_atomic_weight"]["raw"] = json!("1.0");
        value_raw["elements"][0]["standard_atomic_weight"]
            .as_object_mut()
            .unwrap()
            .remove("standard_atomic_weight_value");
        let snapshot = decode_dataset(&[(FILE_ID, &bytes(&value_raw))]).unwrap();
        assert!(
            matches!(&snapshot.elements[0].standard_atomic_weight.raw, OptionalText::Value(value) if value == "1.0")
        );
        assert!(matches!(
            snapshot.elements[0]
                .standard_atomic_weight
                .standard_atomic_weight_value,
            OptionalNull::Missing
        ));
    }

    #[test]
    fn counts_closed_shapes_and_opaque_object_type_are_checked() {
        let mut count = fixture();
        count["isotope_count"] = json!(2);
        assert_eq!(
            decode_dataset(&[(FILE_ID, &bytes(&count))]).err(),
            Some(NistError::RecordCount)
        );
        let mut extra = fixture();
        extra["isotopes"][0]["unexpected"] = json!(true);
        assert_eq!(
            decode_dataset(&[(FILE_ID, &bytes(&extra))]).err(),
            Some(NistError::Record)
        );
        let mut not_object = fixture();
        not_object["reported_composition_sums_decimal"] = json!(null);
        assert_eq!(
            decode_dataset(&[(FILE_ID, &bytes(&not_object))]).err(),
            Some(NistError::Record)
        );
        let duplicate = String::from_utf8(bytes(&fixture())).unwrap().replacen(
            "\"record_id\":\"synthetic\"",
            "\"record_id\":\"synthetic\",\"record_id\":\"other\"",
            1,
        );
        assert_eq!(
            decode_dataset(&[(FILE_ID, duplicate.as_bytes())]).err(),
            Some(NistError::Record)
        );
    }

    #[test]
    fn file_set_and_catalog_identity_are_explicit() {
        let bytes = bytes(&fixture());
        assert_eq!(decode_dataset(&[]).err(), Some(NistError::FileSet));
        assert_eq!(
            decode_dataset(&[("other", &bytes)]).err(),
            Some(NistError::FileSet)
        );
        assert!(accepts_file(FILE_ID, "auxiliary_data", "declarative_data"));
        assert!(!accepts_file(FILE_ID, "weights", "declarative_data"));
        assert!(!accepts_file(FILE_ID, "auxiliary_data", "model_parameters"));
        assert_eq!(
            verify_catalog_files(
                "independent-NIST-COMP-4.1-SNAPSHOT-2026-10-24",
                "wrong",
                b"m",
                b"r",
                b"c"
            ),
            Err(NistError::CatalogMismatch)
        );
    }
}
