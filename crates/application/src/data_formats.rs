//! Typed readers of the declarative data formats (see `chemifree_domain::data::DataFormat`).
//!
//! A reader checks the whole content against its declared schema and either returns typed
//! values or says what is wrong. Errors name the line, column, or field, never a value, so a
//! refusal does not repeat the data it refuses.

use std::collections::HashSet;

use chemifree_domain::data::ValueType;
use serde_json::{Map, Value};

use crate::json;

/// A declared column of a table, or a field of a parameter set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Column {
    pub name: String,
    pub value_type: ValueType,
    /// No two rows have the same value in it. Only integer and string columns can be unique.
    pub unique: bool,
}

/// One typed value.
#[derive(Clone, Debug, PartialEq)]
pub enum DataValue {
    Integer(i64),
    Number(f64),
    String(String),
    Boolean(bool),
}

/// The rows of a `table-jsonl` file, each with one value per column, in column order.
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<DataValue>>,
}

/// The values of a `parameters-json` file, one per field, in field order.
#[derive(Clone, Debug, PartialEq)]
pub struct Parameters {
    pub fields: Vec<Column>,
    pub values: Vec<DataValue>,
}

/// Reads a `table-jsonl` file: UTF-8 text with one JSON object per line, each with exactly the
/// declared columns, and at least one line. The last line may end with a line feed; no line is
/// blank.
///
/// # Errors
///
/// Returns what is wrong, with the line (counted from 1) where it is.
pub fn decode_table(bytes: &[u8], columns: &[Column]) -> Result<Table, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "it is not UTF-8 text".to_owned())?;
    let body = text.strip_suffix('\n').unwrap_or(text);
    if body.is_empty() {
        return Err("it has no rows".into());
    }
    let mut seen: Vec<HashSet<String>> = vec![HashSet::new(); columns.len()];
    let mut rows = Vec::new();
    for (index, line) in body.split('\n').enumerate() {
        let number = index + 1;
        if line.trim().is_empty() {
            return Err(format!("line {number} is blank"));
        }
        let object = parse_object(line.as_bytes()).map_err(|why| format!("line {number} {why}"))?;
        let row = typed_values(&object, columns).map_err(|why| format!("line {number}: {why}"))?;
        for ((column, value), seen) in columns.iter().zip(&row).zip(&mut seen) {
            if column.unique && !seen.insert(unique_key(value)) {
                return Err(format!(
                    "line {number} repeats a value of the unique column `{}`",
                    column.name
                ));
            }
        }
        rows.push(row);
    }
    Ok(Table {
        columns: columns.to_vec(),
        rows,
    })
}

/// Reads a `parameters-json` file: one UTF-8 JSON object with exactly the declared fields.
///
/// # Errors
///
/// Returns what is wrong.
pub fn decode_parameters(bytes: &[u8], fields: &[Column]) -> Result<Parameters, String> {
    let object = parse_object(bytes).map_err(|why| format!("the document {why}"))?;
    let values = typed_values(&object, fields)?;
    Ok(Parameters {
        fields: fields.to_vec(),
        values,
    })
}

fn parse_object(bytes: &[u8]) -> Result<Map<String, Value>, String> {
    match json::parse_strict(bytes) {
        Ok(Value::Object(object)) => Ok(object),
        Ok(_) => Err("is not a JSON object".into()),
        Err(error) => Err(format!("is not JSON without repeated keys ({error})")),
    }
}

/// The values of `object` in the order of `columns`, which must be exactly its keys.
fn typed_values(object: &Map<String, Value>, columns: &[Column]) -> Result<Vec<DataValue>, String> {
    if let Some(extra) = object
        .keys()
        .find(|key| !columns.iter().any(|column| &column.name == *key))
    {
        return Err(format!("`{extra}` is not a declared column"));
    }
    columns
        .iter()
        .map(|column| {
            let value = object
                .get(&column.name)
                .ok_or_else(|| format!("`{}` is missing", column.name))?;
            typed(value, column.value_type).ok_or_else(|| {
                format!(
                    "`{}` is not {}",
                    column.name,
                    match column.value_type {
                        ValueType::Integer => "a 64-bit integer",
                        ValueType::Number => "a number",
                        ValueType::String => "a string",
                        ValueType::Boolean => "a boolean",
                    }
                )
            })
        })
        .collect()
}

fn typed(value: &Value, value_type: ValueType) -> Option<DataValue> {
    match value_type {
        ValueType::Integer => value.as_i64().map(DataValue::Integer),
        // JSON has no NaN or infinity, and the parser refuses numbers that overflow.
        ValueType::Number => value.as_f64().map(DataValue::Number),
        ValueType::String => value
            .as_str()
            .map(|text| DataValue::String(text.to_owned())),
        ValueType::Boolean => value.as_bool().map(DataValue::Boolean),
    }
}

/// How a value of a unique column is compared: integers by value, strings exactly.
fn unique_key(value: &DataValue) -> String {
    match value {
        DataValue::Integer(number) => format!("i{number}"),
        DataValue::String(text) => format!("s{text}"),
        // The schema check allows unique only on integer and string columns.
        DataValue::Number(number) => format!("n{}", number.to_bits()),
        DataValue::Boolean(flag) => format!("b{flag}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, value_type: ValueType, unique: bool) -> Column {
        Column {
            name: name.into(),
            value_type,
            unique,
        }
    }

    fn columns() -> Vec<Column> {
        vec![
            column("key", ValueType::String, true),
            column("size", ValueType::Integer, false),
            column("weight", ValueType::Number, false),
            column("flag", ValueType::Boolean, false),
        ]
    }

    #[test]
    fn a_table_decodes_into_typed_rows_in_column_order() {
        let text = b"{\"key\": \"a\", \"size\": 6, \"weight\": 0.5, \"flag\": true}\n\
                     {\"flag\": false, \"weight\": -2, \"size\": -1, \"key\": \"b\"}\n";
        let table = decode_table(text, &columns()).unwrap();
        assert_eq!(
            table.rows,
            [
                vec![
                    DataValue::String("a".into()),
                    DataValue::Integer(6),
                    DataValue::Number(0.5),
                    DataValue::Boolean(true)
                ],
                vec![
                    DataValue::String("b".into()),
                    DataValue::Integer(-1),
                    DataValue::Number(-2.0),
                    DataValue::Boolean(false)
                ],
            ]
        );
        let unterminated = b"{\"key\": \"a\", \"size\": 6, \"weight\": 0.5, \"flag\": true}";
        assert_eq!(
            decode_table(unterminated, &columns()).unwrap().rows.len(),
            1
        );
    }

    #[test]
    fn a_table_that_differs_from_its_schema_is_refused_with_where() {
        let row = r#"{"key": "a", "size": 6, "weight": 0.5, "flag": true}"#;
        for (text, why) in [
            (String::new(), "it has no rows".to_owned()),
            ("\n".into(), "it has no rows".into()),
            (format!("{row}\n\n{row}\n"), "line 2 is blank".into()),
            (
                format!("{row}\n{}", &row[..20]),
                "line 2 is not JSON".into(),
            ),
            (
                format!("{row}\n[1, 2]\n"),
                "line 2 is not a JSON object".into(),
            ),
            (
                r#"{"key": "a", "key": "b", "size": 6, "weight": 0.5, "flag": true}"#.into(),
                "line 1 is not JSON without repeated keys".into(),
            ),
            (
                r#"{"key": "a", "weight": 0.5, "flag": true}"#.into(),
                "line 1: `size` is missing".into(),
            ),
            (
                r#"{"key": "a", "size": 6, "weight": 0.5, "flag": true, "x": 1}"#.into(),
                "line 1: `x` is not a declared column".into(),
            ),
            (
                r#"{"key": "a", "size": 6.5, "weight": 0.5, "flag": true}"#.into(),
                "line 1: `size` is not a 64-bit integer".into(),
            ),
            (
                r#"{"key": "a", "size": 9223372036854775808, "weight": 0.5, "flag": true}"#.into(),
                "line 1: `size` is not a 64-bit integer".into(),
            ),
            (
                r#"{"key": "a", "size": 6, "weight": "0.5", "flag": true}"#.into(),
                "line 1: `weight` is not a number".into(),
            ),
            (
                r#"{"key": null, "size": 6, "weight": 0.5, "flag": true}"#.into(),
                "line 1: `key` is not a string".into(),
            ),
            (
                r#"{"key": "a", "size": 6, "weight": 0.5, "flag": 1}"#.into(),
                "line 1: `flag` is not a boolean".into(),
            ),
            (
                format!("{row}\n{row}\n"),
                "line 2 repeats a value of the unique column `key`".into(),
            ),
        ] {
            let error = decode_table(text.as_bytes(), &columns()).unwrap_err();
            assert!(error.starts_with(&why), "{text:?}: {error}");
        }
        assert_eq!(
            decode_table(b"\xff\n", &columns()).unwrap_err(),
            "it is not UTF-8 text"
        );
    }

    #[test]
    fn parameters_decode_with_exactly_their_fields() {
        let fields = vec![
            column("scale", ValueType::Number, false),
            column("label", ValueType::String, false),
        ];
        let parameters =
            decode_parameters(br#"{"label": "synthetic", "scale": 1.25}"#, &fields).unwrap();
        assert_eq!(
            parameters.values,
            [
                DataValue::Number(1.25),
                DataValue::String("synthetic".into())
            ]
        );
        for (text, why) in [
            (&br#"{"scale": 1.25}"#[..], "`label` is missing"),
            (
                br#"{"scale": 1, "label": "x", "extra": 2}"#,
                "`extra` is not a declared column",
            ),
            (br#"[1]"#, "the document is not a JSON object"),
            (
                br#"{"scale": 1, "label": "x"} trailing"#,
                "the document is not JSON",
            ),
        ] {
            let error = decode_parameters(text, &fields).unwrap_err();
            assert!(error.starts_with(why), "{error}");
        }
    }
}
