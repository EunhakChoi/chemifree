//! Decoders for individually specified declarative data profiles.

use std::collections::HashSet;

use serde::Deserialize;

use crate::json;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataProfile {
    RingRank,
    ValenceReference,
    ElementEndpoint,
    FixedContours,
    LargeRingSeeds,
}

/// A payload-free reason a profile stream cannot be decoded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileDecodeError {
    InvalidFraming,
    InvalidRecordCount,
    InvalidRecord,
    InvalidRankSet,
    InvalidIndexSet,
    InvalidEndpoint,
    InvalidContour,
    InvalidF64Bits,
}

impl std::fmt::Display for ProfileDecodeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ProfileDecodeError {}

/// An ordered rank operand, preserved as supplied without inferring a priority.
#[derive(PartialEq)]
pub struct RankOperand {
    pub atomic_number: u32,
    pub rank: u32,
}

/// One indexed scalar operand; index zero remains a sentinel.
#[derive(PartialEq)]
pub struct ValenceOperand {
    pub atomic_number: u32,
    pub value: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindingMode {
    SubtractTurns,
    AddTurns,
}

// These data-bearing types intentionally have no Debug implementation: diagnostics must not
// accidentally dump a separately supplied payload.
#[derive(PartialEq)]
pub enum ProfileRecords {
    RingRank(Vec<RankOperand>),
    ValenceReference(Vec<ValenceOperand>),
    ElementEndpoint(u32),
    Contours(Vec<Contour>),
    Phase6(crate::phase6_profiles::DatasetRecords),
    /// Verified NMR input operands; no prediction engine is implied.
    Nmr(Box<crate::nmr_profile::NmrData>),
}

#[derive(PartialEq)]
pub struct Contour {
    pub operand_id: String,
    pub ring_size: u32,
    pub calibration_reference_bits: u64,
    pub constructor_heading_bits: u64,
    pub winding_mode: WindingMode,
    pub joint_policy_flag: bool,
    pub turn_degrees_bits: Vec<u64>,
    pub observed_heading_bits: Vec<u64>,
    pub edge_length_bits: Vec<u64>,
}

/// Validates an entire stream before returning typed operands. Errors contain no payload.
///
/// # Errors
/// Returns a stable reason when framing, shape or cross-record checks fail.
pub fn decode_profile(
    profile: DataProfile,
    bytes: &[u8],
) -> Result<ProfileRecords, ProfileDecodeError> {
    let text = std::str::from_utf8(bytes).map_err(|_| ProfileDecodeError::InvalidFraming)?;
    if text.starts_with('\u{feff}') || !text.ends_with('\n') {
        return Err(ProfileDecodeError::InvalidFraming);
    }
    let expected = match profile {
        DataProfile::RingRank => 22,
        DataProfile::ValenceReference => 110,
        DataProfile::ElementEndpoint => 1,
        DataProfile::FixedContours => 158,
        DataProfile::LargeRingSeeds => 2,
    };
    let lines: Vec<_> = text[..text.len() - 1].split('\n').collect();
    if lines.len() != expected || lines.iter().any(|line| line.trim().is_empty()) {
        return Err(ProfileDecodeError::InvalidRecordCount);
    }
    let rows = lines
        .iter()
        .map(|line| {
            json::parse_integer_zero_strict(line.as_bytes())
                .map_err(|_| ProfileDecodeError::InvalidRecord)
        })
        .collect::<Result<Vec<_>, _>>()?;
    match profile {
        DataProfile::RingRank => {
            let mut atomic_numbers = HashSet::new();
            let mut ranks = HashSet::new();
            let mut result = Vec::with_capacity(expected);
            for value in rows {
                let row: RankRow =
                    serde_json::from_value(value).map_err(|_| ProfileDecodeError::InvalidRecord)?;
                if !(1..=109).contains(&row.atomic_number_u32)
                    || !(1..=22).contains(&row.source_rank_u32)
                    || !atomic_numbers.insert(row.atomic_number_u32)
                    || !ranks.insert(row.source_rank_u32)
                {
                    return Err(ProfileDecodeError::InvalidRankSet);
                }
                result.push(RankOperand {
                    atomic_number: row.atomic_number_u32,
                    rank: row.source_rank_u32,
                });
            }
            Ok(ProfileRecords::RingRank(result))
        }
        DataProfile::ValenceReference => {
            let mut indices = HashSet::new();
            let mut result = Vec::with_capacity(expected);
            for value in rows {
                let row: ValenceRow =
                    serde_json::from_value(value).map_err(|_| ProfileDecodeError::InvalidRecord)?;
                if row.atomic_number_u32 > 109 || !indices.insert(row.atomic_number_u32) {
                    return Err(ProfileDecodeError::InvalidIndexSet);
                }
                result.push(ValenceOperand {
                    atomic_number: row.atomic_number_u32,
                    value: row.valence_reference_u32,
                });
            }
            Ok(ProfileRecords::ValenceReference(result))
        }
        DataProfile::ElementEndpoint => {
            let row: RankRow = serde_json::from_value(
                rows.into_iter()
                    .next()
                    .ok_or(ProfileDecodeError::InvalidRecordCount)?,
            )
            .map_err(|_| ProfileDecodeError::InvalidRecord)?;
            if row.atomic_number_u32 != 1 {
                return Err(ProfileDecodeError::InvalidEndpoint);
            }
            Ok(ProfileRecords::ElementEndpoint(row.source_rank_u32))
        }
        DataProfile::FixedContours | DataProfile::LargeRingSeeds => {
            let mut ids = HashSet::new();
            let mut result = Vec::with_capacity(expected);
            for value in rows {
                let row: ContourWire =
                    serde_json::from_value(value).map_err(|_| ProfileDecodeError::InvalidRecord)?;
                if row.operand_id.is_empty()
                    || row.operand_id.chars().count() > 80
                    || !ids.insert(row.operand_id.clone())
                    || !(3..=4096).contains(&row.ring_size)
                    || !matches!(row.winding_mode, 1 | 2)
                {
                    return Err(ProfileDecodeError::InvalidContour);
                }
                let count = row.ring_size as usize;
                if row.turn_degrees_f64le.len() != count
                    || row.observed_heading_radians_f64le.len() != count
                    || row.edge_length_multipliers_f64le.len() != count
                {
                    return Err(ProfileDecodeError::InvalidContour);
                }
                let turns = decode_bits_array(&row.turn_degrees_f64le)?;
                let headings = decode_bits_array(&row.observed_heading_radians_f64le)?;
                let lengths = decode_bits_array(&row.edge_length_multipliers_f64le)?;
                if lengths.iter().any(|bits| f64::from_bits(*bits) <= 0.0) {
                    return Err(ProfileDecodeError::InvalidContour);
                }
                result.push(Contour {
                    operand_id: row.operand_id,
                    ring_size: row.ring_size,
                    calibration_reference_bits: decode_bits(
                        &row.calibration_reference_radians_f64le,
                    )?,
                    constructor_heading_bits: decode_bits(&row.constructor_heading_radians_f64le)?,
                    winding_mode: if row.winding_mode == 1 {
                        WindingMode::SubtractTurns
                    } else {
                        WindingMode::AddTurns
                    },
                    joint_policy_flag: row.joint_policy_flag,
                    turn_degrees_bits: turns,
                    observed_heading_bits: headings,
                    edge_length_bits: lengths,
                });
            }
            Ok(ProfileRecords::Contours(result))
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RankRow {
    atomic_number_u32: u32,
    source_rank_u32: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ValenceRow {
    atomic_number_u32: u32,
    valence_reference_u32: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContourWire {
    operand_id: String,
    ring_size: u32,
    calibration_reference_radians_f64le: String,
    constructor_heading_radians_f64le: String,
    winding_mode: u32,
    joint_policy_flag: bool,
    turn_degrees_f64le: Vec<String>,
    observed_heading_radians_f64le: Vec<String>,
    edge_length_multipliers_f64le: Vec<String>,
}

fn decode_bits_array(values: &[String]) -> Result<Vec<u64>, ProfileDecodeError> {
    values.iter().map(|value| decode_bits(value)).collect()
}

fn decode_bits(text: &str) -> Result<u64, ProfileDecodeError> {
    let bytes = text.as_bytes();
    if bytes.len() != 16
        || !bytes
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
    {
        return Err(ProfileDecodeError::InvalidF64Bits);
    }
    let mut raw = [0; 8];
    for (output, pair) in raw.iter_mut().zip(bytes.chunks_exact(2)) {
        let digit = |byte: u8| {
            if byte.is_ascii_digit() {
                byte - b'0'
            } else {
                byte - b'a' + 10
            }
        };
        *output = digit(pair[0]) * 16 + digit(pair[1]);
    }
    let bits = u64::from_le_bytes(raw);
    if !f64::from_bits(bits).is_finite() {
        return Err(ProfileDecodeError::InvalidF64Bits);
    }
    Ok(bits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rank_rows_require_complete_unique_rank_set_and_preserve_order() {
        let text: String = (1..=22)
            .rev()
            .map(|rank| format!("{{\"atomic_number_u32\":{rank},\"source_rank_u32\":{rank}}}\n"))
            .collect();
        let ProfileRecords::RingRank(rows) =
            decode_profile(DataProfile::RingRank, text.as_bytes()).unwrap()
        else {
            panic!("wrong type")
        };
        assert_eq!((rows[0].atomic_number, rows[0].rank), (22, 22));
        assert_eq!((rows[21].atomic_number, rows[21].rank), (1, 1));
        assert!(
            decode_profile(
                DataProfile::RingRank,
                text.replace("\"source_rank_u32\":22", "\"source_rank_u32\":21")
                    .as_bytes()
            )
            .is_err()
        );
        assert!(
            decode_profile(
                DataProfile::RingRank,
                text.replace("\"atomic_number_u32\":22", "\"atomic_number_u32\":22.0")
                    .as_bytes()
            )
            .is_err()
        );
        assert!(decode_profile(DataProfile::RingRank, text.trim_end().as_bytes()).is_err());
    }

    #[test]
    fn valence_and_endpoint_keep_unsigned_operands_without_inference() {
        let text: String = (0..110)
            .map(|i| {
                format!("{{\"atomic_number_u32\":{i},\"valence_reference_u32\":4294967295}}\n")
            })
            .collect();
        let ProfileRecords::ValenceReference(rows) =
            decode_profile(DataProfile::ValenceReference, text.as_bytes()).unwrap()
        else {
            panic!("wrong type")
        };
        assert_eq!((rows[0].atomic_number, rows[0].value), (0, u32::MAX));
        assert!(matches!(
            decode_profile(
                DataProfile::ValenceReference,
                text.replace("\"atomic_number_u32\":0", "\"atomic_number_u32\":-0")
                    .as_bytes(),
            ),
            Ok(ProfileRecords::ValenceReference(_))
        ));
        assert!(
            decode_profile(
                DataProfile::ValenceReference,
                text.replace("\"atomic_number_u32\":109", "\"atomic_number_u32\":108")
                    .as_bytes()
            )
            .is_err()
        );
        assert!(matches!(
            decode_profile(
                DataProfile::ElementEndpoint,
                b"{\"atomic_number_u32\":1,\"source_rank_u32\":4294967295}\n"
            ),
            Ok(ProfileRecords::ElementEndpoint(u32::MAX))
        ));
        for bad in ["1.0", "2", "true", "-1"] {
            let text = format!("{{\"atomic_number_u32\":{bad},\"source_rank_u32\":0}}\n");
            assert!(decode_profile(DataProfile::ElementEndpoint, text.as_bytes()).is_err());
        }
    }

    #[test]
    fn contour_bits_and_signed_zero_survive_and_corruption_is_refused() {
        let row = |id: String| json!({"operand_id": id, "ring_size": 3, "calibration_reference_radians_f64le": "0000000000000080", "constructor_heading_radians_f64le": "0000000000000000", "winding_mode": 1, "joint_policy_flag": false, "turn_degrees_f64le": ["0000000000000000", "0000000000000080", "000000000000f03f"], "observed_heading_radians_f64le": ["0000000000000000", "0000000000000000", "0000000000000000"], "edge_length_multipliers_f64le": ["000000000000f03f", "000000000000f03f", "000000000000f03f"]});
        let a = row("synthetic-a".to_owned());
        let b = row("synthetic-b".to_owned());
        let stream = |x: &serde_json::Value, y: &serde_json::Value| format!("{x}\n{y}\n");
        let ProfileRecords::Contours(rows) =
            decode_profile(DataProfile::LargeRingSeeds, stream(&a, &b).as_bytes()).unwrap()
        else {
            panic!("wrong type")
        };
        assert_eq!(rows[0].calibration_reference_bits, (-0.0_f64).to_bits());
        assert_eq!(rows[0].turn_degrees_bits[1], (-0.0_f64).to_bits());
        assert_eq!(rows[0].winding_mode, WindingMode::SubtractTurns);
        let fixed: String = (0..158)
            .map(|index| format!("{}\n", row(format!("synthetic-{index}"))))
            .collect();
        let ProfileRecords::Contours(fixed_rows) =
            decode_profile(DataProfile::FixedContours, fixed.as_bytes()).unwrap()
        else {
            panic!("wrong type")
        };
        assert_eq!(fixed_rows.len(), 158);
        assert!(decode_profile(DataProfile::LargeRingSeeds, fixed.as_bytes()).is_err());
        assert!(decode_profile(DataProfile::LargeRingSeeds, stream(&a, &a).as_bytes()).is_err());
        for (field, value) in [
            (
                "edge_length_multipliers_f64le",
                json!(["0000000000000000", "000000000000f03f", "000000000000f03f"]),
            ),
            (
                "turn_degrees_f64le",
                json!(["000000000000f07f", "0000000000000000", "0000000000000000"]),
            ),
            (
                "observed_heading_radians_f64le",
                json!(["0000000000000000"]),
            ),
        ] {
            let mut invalid = b.clone();
            invalid[field] = value;
            assert!(
                decode_profile(DataProfile::LargeRingSeeds, stream(&a, &invalid).as_bytes())
                    .is_err()
            );
        }
    }
}
