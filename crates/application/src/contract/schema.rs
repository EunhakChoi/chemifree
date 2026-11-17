//! JSON Schema of the transaction command list. It states what `CommandDto` accepts when it is
//! deserialized, which is the contract a tool input has to describe. Built only with the
//! `schema` feature.

use schemars::generate::SchemaSettings;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::Serialize;
use serde_json::{Map, Value};

use super::command::CommandDto;
use super::request::PointDto;

/// The schema of one `CommandDto` for its deserialize contract. The definitions it refers to sit
/// under `$defs` at the root, with the same draft and settings rmcp uses for tool inputs.
///
/// # Panics
///
/// Panics if the generated schema fails to serialize as JSON. A derived schema always serializes.
pub fn command_schema() -> Value {
    let schema = SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<CommandDto>();
    serde_json::to_value(schema).expect("a generated schema serializes to JSON")
}

/// [`command_schema`] as the golden file stores it: the root object with one line per `oneOf`
/// variant and one line per `$defs` entry, each a compact JSON value. Object keys are sorted at
/// every depth by this function, not by serde_json's map type, so the text is the same whichever
/// map implementation the build selects.
///
/// # Panics
///
/// Panics if the schema's root is not a JSON object, which the derived schema never is.
pub fn command_schema_text() -> String {
    let Value::Object(root) = command_schema() else {
        panic!("the command schema is not a JSON object");
    };
    let entries = sorted_entries(&root);
    let mut lines = vec!["{".to_owned()];
    for (index, &(key, value)) in entries.iter().enumerate() {
        let separator = if index + 1 < entries.len() { "," } else { "" };
        let key = compact(key);
        match value {
            Value::Array(items) if !items.is_empty() => {
                lines.push(format!("  {key}: ["));
                lines.extend(indented(list_lines(
                    items.iter().map(compact_sorted).collect(),
                )));
                lines.push(format!("  ]{separator}"));
            }
            Value::Object(members) if !members.is_empty() => {
                lines.push(format!("  {key}: {{"));
                let rendered = sorted_entries(members)
                    .into_iter()
                    .map(|(name, member)| format!("{}: {}", compact(name), compact_sorted(member)))
                    .collect();
                lines.extend(indented(list_lines(rendered)));
                lines.push(format!("  }}{separator}"));
            }
            other => lines.push(format!("  {key}: {}{separator}", compact_sorted(other))),
        }
    }
    lines.push("}".to_owned());
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

fn compact<T: Serialize + ?Sized>(value: &T) -> String {
    serde_json::to_string(value).expect("a JSON value serializes to text")
}

/// `value` as one line of JSON, with the keys of every object inside it in sorted order.
fn compact_sorted(value: &Value) -> String {
    match value {
        Value::Object(members) => {
            let members: Vec<String> = sorted_entries(members)
                .into_iter()
                .map(|(key, member)| format!("{}:{}", compact(key), compact_sorted(member)))
                .collect();
            format!("{{{}}}", members.join(","))
        }
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(compact_sorted).collect();
            format!("[{}]", items.join(","))
        }
        scalar => compact(scalar),
    }
}

/// The entries of a JSON object, ordered by key.
fn sorted_entries(members: &Map<String, Value>) -> Vec<(&String, &Value)> {
    let mut entries: Vec<(&String, &Value)> = members.iter().collect();
    entries.sort_by(|left, right| left.0.cmp(right.0));
    entries
}

/// One entry per line, a comma after every entry but the last.
fn list_lines(entries: Vec<String>) -> Vec<String> {
    let last = entries.len().saturating_sub(1);
    entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            if index < last {
                format!("{entry},")
            } else {
                entry
            }
        })
        .collect()
}

fn indented(lines: Vec<String>) -> impl Iterator<Item = String> {
    lines.into_iter().map(|line| format!("    {line}"))
}

/// A nullable field's schema: the value's schema or `null`. serde accepts `null` for these fields
/// and refuses an absent key. schemars reads a `schema_with` field's optionality from its wrapper
/// type, which is not an `Option`, so the field is required without a `required` attribute; the
/// corpus case with the key missing proves that serde refuses it.
pub(super) fn nullable<T: JsonSchema>(generator: &mut SchemaGenerator) -> Schema {
    <Option<T> as JsonSchema>::json_schema(generator)
}

/// `RingSize` as `CommandDto` reads it: the integer 5 or 6. The field's deserializer refuses every
/// other value, including the float token `5.0`; JSON Schema compares numbers by value and admits
/// it, which the corpus records as an override.
pub(super) fn ring_size_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "integer", "enum": [5, 6] })
}

/// `RingPattern` as `CommandDto` reads it: the string `single` or `kekule`.
pub(super) fn ring_pattern_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "string", "enum": ["single", "kekule"] })
}

/// The `positions` object of `add_atom`. Its field is not nullable: serde refuses `null`, so the
/// schema has no `null` alternative even though the field is optional.
pub(super) fn point_map_schema(generator: &mut SchemaGenerator) -> Schema {
    let point = generator.subschema_for::<PointDto>();
    json_schema!({ "type": "object", "additionalProperties": point })
}
