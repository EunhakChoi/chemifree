//! Lexical AqSolDB CSV input. It does not define a solubility model or measurement units.

use crate::phase6_profiles;

/// The public catalog's explicit file identity.
pub const FILE_ID: &str = "PRIVATE-AQSOLDB-V1.0-ORIGINAL";
const HEADER: [&str; 26] = [
    "ID",
    "Name",
    "InChI",
    "InChIKey",
    "SMILES",
    "Solubility",
    "SD",
    "Ocurrences",
    "Group",
    "MolWt",
    "MolLogP",
    "MolMR",
    "HeavyAtomCount",
    "NumHAcceptors",
    "NumHDonors",
    "NumHeteroatoms",
    "NumRotatableBonds",
    "NumValenceElectrons",
    "NumAromaticRings",
    "NumSaturatedRings",
    "NumAliphaticRings",
    "RingCount",
    "TPSA",
    "LabuteASA",
    "BalabanJ",
    "BertzCT",
];
const RECORD_COUNT: usize = 9982;

/// Payload-free rejection of a catalog-bound CSV input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AqSolError {
    CatalogMismatch,
    FileSet,
    Framing,
    Header,
    RecordWidth,
    RecordCount,
}
impl std::fmt::Display for AqSolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::CatalogMismatch => "catalog_identity_mismatch",
            Self::FileSet => "invalid_file_set",
            Self::Framing => "invalid_csv_framing",
            Self::Header => "invalid_csv_header",
            Self::RecordWidth => "invalid_record_width",
            Self::RecordCount => "invalid_record_count",
        })
    }
}
impl std::error::Error for AqSolError {}

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
) -> Result<(), AqSolError> {
    let expected = phase6_profiles::CatalogIdentity {
        dataset_id: "independent-AQSOLDB-V1.0-ORIGINAL",
        dataset_version: "sha256-set-cbc0305b03c070fc61f2b3500e4212acc613fb48f46e630cbdfe6a815985c24f",
        registry_sha256: "8ffd2e9b4d01ce989c63cb552714e1ff01acb8f766dbbc14bc411f08c4484d25",
        profile_sha256: "5add6a3e7a6250dc5c2f280d9a193dc8aef3ba008e47d10e35e7b263c217c2a2",
        manifest_file_sha256: "5ed9a6ea614f9ea0d6e152de21061c30de153e5ef692c72add4ca1337d018483",
    };
    if dataset_id != expected.dataset_id || dataset_version != expected.dataset_version {
        return Err(AqSolError::CatalogMismatch);
    }
    expected
        .check_file_bytes(manifest, registry, catalog)
        .map_err(|_| AqSolError::CatalogMismatch)
}

/// Checks the catalog identity, role and content classification.
#[must_use]
pub fn accepts_file(id: &str, role: &str, class: &str) -> bool {
    id == FILE_ID && role == "auxiliary_data" && class == "model_parameters"
}

/// Named catalog columns. Values remain strings until a separate scientific convention is bound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum Column {
    Id,
    Name,
    Inchi,
    InchiKey,
    Smiles,
    Solubility,
    Sd,
    Ocurrences,
    Group,
    MolWt,
    MolLogP,
    MolMr,
    HeavyAtomCount,
    NumHAcceptors,
    NumHDonors,
    NumHeteroatoms,
    NumRotatableBonds,
    NumValenceElectrons,
    NumAromaticRings,
    NumSaturatedRings,
    NumAliphaticRings,
    RingCount,
    Tpsa,
    LabuteAsa,
    BalabanJ,
    BertzCt,
}

/// One lexical record, including any extra source fields as opaque metadata.
#[derive(PartialEq)]
pub struct AqSolRow {
    values: [String; 26],
    pub extra: Vec<String>,
}
impl AqSolRow {
    #[must_use]
    pub fn get(&self, column: Column) -> &str {
        &self.values[column as usize]
    }
}

/// An extra source column remains opaque and retains its original zero-based position.
#[derive(PartialEq)]
pub struct ExtraColumn {
    pub position: usize,
    pub name: String,
}

/// Ordered records and any extra source columns.
#[derive(PartialEq)]
pub struct AqSolData {
    pub extra_columns: Vec<ExtraColumn>,
    pub rows: Vec<AqSolRow>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum State {
    Start,
    Unquoted,
    Quoted,
    AfterQuote,
}

fn finish_field(field: &mut Vec<u8>, row: &mut Vec<String>) -> Result<(), AqSolError> {
    let bytes = std::mem::take(field);
    row.push(String::from_utf8(bytes).map_err(|_| AqSolError::Framing)?);
    Ok(())
}

fn finish_record(
    field: &mut Vec<u8>,
    row: &mut Vec<String>,
    records: &mut Vec<Vec<String>>,
) -> Result<(), AqSolError> {
    if records.len() == RECORD_COUNT + 1 {
        return Err(AqSolError::RecordCount);
    }
    finish_field(field, row)?;
    records.push(std::mem::take(row));
    Ok(())
}

// RFC 4180 quoting is strict. LF is also accepted as a record separator; bare CR is not.
fn parse_csv(bytes: &[u8]) -> Result<Vec<Vec<String>>, AqSolError> {
    std::str::from_utf8(bytes).map_err(|_| AqSolError::Framing)?;
    let mut state = State::Start;
    let mut field = Vec::new();
    let mut row = Vec::new();
    let mut records = Vec::new();
    let mut record_open = false;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if state == State::Quoted {
            if byte == b'"' {
                state = State::AfterQuote;
            } else {
                field.push(byte);
            }
        } else if state == State::AfterQuote && byte == b'"' {
            field.push(byte);
            state = State::Quoted;
        } else {
            match byte {
                b'"' if state == State::Start => {
                    state = State::Quoted;
                    record_open = true;
                }
                b'"' => return Err(AqSolError::Framing),
                b',' => {
                    finish_field(&mut field, &mut row)?;
                    state = State::Start;
                    record_open = true;
                }
                b'\r' | b'\n' => {
                    if byte == b'\r' {
                        if bytes.get(index + 1) != Some(&b'\n') {
                            return Err(AqSolError::Framing);
                        }
                        index += 1;
                    }
                    finish_record(&mut field, &mut row, &mut records)?;
                    state = State::Start;
                    record_open = false;
                }
                _ if state == State::AfterQuote => return Err(AqSolError::Framing),
                _ => {
                    field.push(byte);
                    state = State::Unquoted;
                    record_open = true;
                }
            }
        }
        index += 1;
    }
    if state == State::Quoted {
        return Err(AqSolError::Framing);
    }
    if record_open {
        finish_record(&mut field, &mut row, &mut records)?;
    }
    Ok(records)
}

/// Decode a single catalog-selected CSV as lexical values and opaque extra columns.
///
/// # Errors
/// Rejects an invalid file identity, framing, header, row width or count.
pub fn decode_dataset(files: &[(&str, &[u8])]) -> Result<AqSolData, AqSolError> {
    let [(id, bytes)] = files else {
        return Err(AqSolError::FileSet);
    };
    if *id != FILE_ID {
        return Err(AqSolError::FileSet);
    }
    if bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES
        || bytes.len() as u64 > crate::MAX_DATA_TOTAL_BYTES
    {
        return Err(AqSolError::Framing);
    }
    let mut records = parse_csv(bytes)?.into_iter();
    let header = records.next().ok_or(AqSolError::Header)?;
    let mut catalog_positions = Vec::with_capacity(HEADER.len());
    let mut extra_columns = Vec::new();
    for (position, name) in header.into_iter().enumerate() {
        if HEADER.get(catalog_positions.len()) == Some(&name.as_str()) {
            catalog_positions.push(position);
        } else if HEADER.contains(&name.as_str()) {
            return Err(AqSolError::Header);
        } else {
            extra_columns.push(ExtraColumn { position, name });
        }
    }
    if catalog_positions.len() != HEADER.len() {
        return Err(AqSolError::Header);
    }
    let mut rows = Vec::with_capacity(RECORD_COUNT);
    for record in records {
        if record.len() != HEADER.len() + extra_columns.len() {
            return Err(AqSolError::RecordWidth);
        }
        let mut values = Vec::with_capacity(HEADER.len());
        let mut extra = Vec::with_capacity(extra_columns.len());
        for (position, value) in record.into_iter().enumerate() {
            if catalog_positions.get(values.len()) == Some(&position) {
                values.push(value);
            } else {
                extra.push(value);
            }
        }
        let values = values.try_into().map_err(|_| AqSolError::RecordWidth)?;
        rows.push(AqSolRow { values, extra });
    }
    if rows.len() != RECORD_COUNT {
        return Err(AqSolError::RecordCount);
    }
    Ok(AqSolData {
        extra_columns,
        rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(extra: bool, rows: usize) -> Vec<u8> {
        let mut header = HEADER.join(",");
        let mut cells = vec![""; 26];
        cells[0] = "0007";
        cells[1] = "\"A, B\"";
        cells[5] = "-0.000";
        let mut row = cells.join(",");
        if extra {
            header.push_str(",extra");
            row.push_str(",\"opaque, metadata\"");
        }
        format!("{header}\r\n{}", vec![row; rows].join("\r\n")).into_bytes()
    }

    #[test]
    fn named_columns_keep_lexical_values_and_unrecognized_columns() {
        let bytes = fixture(true, RECORD_COUNT);
        let data = decode_dataset(&[(FILE_ID, &bytes)]).unwrap();
        assert_eq!(data.rows.len(), RECORD_COUNT);
        assert_eq!(data.extra_columns.len(), 1);
        assert_eq!(data.extra_columns[0].position, 26);
        assert_eq!(data.extra_columns[0].name, "extra");
        assert_eq!(data.rows[0].get(Column::Id), "0007");
        assert_eq!(data.rows[0].get(Column::Name), "A, B");
        assert_eq!(data.rows[0].get(Column::Solubility), "-0.000");
        assert_eq!(data.rows[0].extra, ["opaque, metadata"]);
        assert_eq!(data.rows[1].get(Column::Id), "0007");
    }

    #[test]
    fn extra_source_columns_between_catalog_columns_are_retained() {
        let mut header = HEADER.to_vec();
        header.insert(3, "extra");
        let mut fields = vec![""; HEADER.len()];
        fields[0] = "0007";
        fields[1] = "name";
        fields[5] = "-0.000";
        fields.insert(3, "opaque");
        let data = format!(
            "{}\r\n{}",
            header.join(","),
            vec![fields.join(","); RECORD_COUNT].join("\r\n")
        );
        let loaded = decode_dataset(&[(FILE_ID, data.as_bytes())]).unwrap();
        assert_eq!(loaded.extra_columns.len(), 1);
        assert_eq!(loaded.extra_columns[0].position, 3);
        assert_eq!(loaded.extra_columns[0].name, "extra");
        assert_eq!(loaded.rows[0].extra, ["opaque"]);
        assert_eq!(loaded.rows[0].get(Column::Id), "0007");
        assert_eq!(loaded.rows[0].get(Column::Solubility), "-0.000");
    }

    #[test]
    fn every_named_column_refers_to_its_catalog_position() {
        let row = AqSolRow {
            values: HEADER.map(str::to_owned),
            extra: Vec::new(),
        };
        let expected = [
            (Column::Id, "ID"),
            (Column::Name, "Name"),
            (Column::Inchi, "InChI"),
            (Column::InchiKey, "InChIKey"),
            (Column::Smiles, "SMILES"),
            (Column::Solubility, "Solubility"),
            (Column::Sd, "SD"),
            (Column::Ocurrences, "Ocurrences"),
            (Column::Group, "Group"),
            (Column::MolWt, "MolWt"),
            (Column::MolLogP, "MolLogP"),
            (Column::MolMr, "MolMR"),
            (Column::HeavyAtomCount, "HeavyAtomCount"),
            (Column::NumHAcceptors, "NumHAcceptors"),
            (Column::NumHDonors, "NumHDonors"),
            (Column::NumHeteroatoms, "NumHeteroatoms"),
            (Column::NumRotatableBonds, "NumRotatableBonds"),
            (Column::NumValenceElectrons, "NumValenceElectrons"),
            (Column::NumAromaticRings, "NumAromaticRings"),
            (Column::NumSaturatedRings, "NumSaturatedRings"),
            (Column::NumAliphaticRings, "NumAliphaticRings"),
            (Column::RingCount, "RingCount"),
            (Column::Tpsa, "TPSA"),
            (Column::LabuteAsa, "LabuteASA"),
            (Column::BalabanJ, "BalabanJ"),
            (Column::BertzCt, "BertzCT"),
        ];
        for (column, header) in expected {
            assert_eq!(row.get(column), header);
        }
    }

    #[test]
    fn quoted_commas_quotes_and_line_breaks_are_decoded_without_trimming() {
        let rows =
            parse_csv(b"one,\"two,three\",\"a\"\"b\",\"x\r\ny\"\r\n  z  ,,last,\r\n").unwrap();
        assert_eq!(rows[0], ["one", "two,three", "a\"b", "x\r\ny"]);
        assert_eq!(rows[1], ["  z  ", "", "last", ""]);
        let lf = parse_csv("é,1\nA,2".as_bytes()).unwrap();
        assert_eq!(lf[0], ["é", "1"]);
        assert_eq!(lf[1], ["A", "2"]);
    }

    #[test]
    fn independent_csv_writer_corpus_matches_its_expected_lexical_records() {
        let input = include_bytes!("../tests/data/aqsoldb-independent-v1/records.csv");
        let expected: Vec<Vec<String>> = serde_json::from_slice(include_bytes!(
            "../tests/data/aqsoldb-independent-v1/expected.json"
        ))
        .unwrap();
        assert_eq!(expected.len(), 128);
        assert_eq!(parse_csv(input).unwrap(), expected);
    }

    #[test]
    fn malformed_quotes_bare_cr_and_invalid_utf8_are_rejected() {
        for bad in [
            &b"a,\"unterminated"[..],
            &b"a,b\"c"[..],
            &b"a,\"b\" x"[..],
            &b"a,b\rc,d"[..],
            &b"a,\xff"[..],
        ] {
            assert_eq!(parse_csv(bad).err(), Some(AqSolError::Framing));
        }
    }

    #[test]
    fn identity_header_width_and_record_count_are_checked() {
        let bytes = fixture(false, RECORD_COUNT);
        assert_eq!(decode_dataset(&[]).err(), Some(AqSolError::FileSet));
        assert_eq!(
            decode_dataset(&[("other", &bytes)]).err(),
            Some(AqSolError::FileSet)
        );
        let bad_header =
            String::from_utf8(bytes.clone())
                .unwrap()
                .replacen("Ocurrences", "Occurrences", 1);
        assert_eq!(
            decode_dataset(&[(FILE_ID, bad_header.as_bytes())]).err(),
            Some(AqSolError::Header)
        );
        let mut duplicate = HEADER.to_vec();
        duplicate.insert(3, "Name");
        assert_eq!(
            decode_dataset(&[(FILE_ID, duplicate.join(",").as_bytes())]).err(),
            Some(AqSolError::Header)
        );
        let mut reordered = HEADER.to_vec();
        reordered.swap(1, 2);
        assert_eq!(
            decode_dataset(&[(FILE_ID, reordered.join(",").as_bytes())]).err(),
            Some(AqSolError::Header)
        );
        let too_few = fixture(false, RECORD_COUNT - 1);
        assert_eq!(
            decode_dataset(&[(FILE_ID, &too_few)]).err(),
            Some(AqSolError::RecordCount)
        );
        let too_many = fixture(false, RECORD_COUNT + 1);
        assert_eq!(
            decode_dataset(&[(FILE_ID, &too_many)]).err(),
            Some(AqSolError::RecordCount)
        );
        let mut wrong_width = fixture(false, RECORD_COUNT);
        wrong_width.push(b',');
        assert_eq!(
            decode_dataset(&[(FILE_ID, &wrong_width)]).err(),
            Some(AqSolError::RecordWidth)
        );
    }

    #[test]
    fn catalog_identity_and_file_role_are_explicit() {
        assert!(accepts_file(FILE_ID, "auxiliary_data", "model_parameters"));
        assert!(!accepts_file(FILE_ID, "weights", "model_parameters"));
        assert!(!accepts_file(FILE_ID, "auxiliary_data", "declarative_data"));
        assert_eq!(
            verify_catalog_files(
                "independent-AQSOLDB-V1.0-ORIGINAL",
                "wrong",
                b"m",
                b"r",
                b"c"
            ),
            Err(AqSolError::CatalogMismatch)
        );
    }
}
