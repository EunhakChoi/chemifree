//! JSON parsing that refuses duplicate object keys, which plain parsing would resolve
//! silently by keeping the last one: an operation request could then mean one thing to a
//! reader and another to the service, and a conformance case could drop an expectation
//! unnoticed.

use std::fmt;

use serde::Deserializer;
use serde::de::{self, Deserialize, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};

/// Parses `bytes` as JSON, refusing duplicate keys at any depth.
///
/// # Errors
///
/// Returns the parser's error, which names the position of invalid JSON or a duplicate key.
pub fn parse_strict(bytes: &[u8]) -> Result<Value, serde_json::Error> {
    serde_json::from_slice::<Strict>(bytes).map(|Strict(value)| value)
}

struct Strict(Value);

impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(StrictVisitor).map(Strict)
    }
}

struct StrictVisitor;

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("non-finite number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Value, E> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::new();
        while let Some(Strict(item)) = seq.next_element()? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut fields = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if fields.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate key {key:?}")));
            }
            let Strict(value) = map.next_value()?;
            fields.insert(key, value);
        }
        Ok(Value::Object(fields))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_like_serde_json() {
        let text = br#"{"a": [1, -2, 2.5, 18446744073709551615, null, true, "s"], "b": {}}"#;
        assert_eq!(
            parse_strict(text).unwrap(),
            json!({"a": [1, -2, 2.5, u64::MAX, null, true, "s"], "b": {}})
        );
    }

    #[test]
    fn refuses_duplicate_keys_at_any_depth() {
        let error = parse_strict(br#"{"a": [{"code": "x", "code": "y"}]}"#).unwrap_err();
        assert!(
            error.to_string().contains("duplicate key \"code\""),
            "unexpected error: {error}"
        );
        assert!(parse_strict(br#"{"a": 1, "a": 1}"#).is_err());
    }
}
