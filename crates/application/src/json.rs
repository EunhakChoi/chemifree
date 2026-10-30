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

/// Strict parsing for contracts that accept the JSON integer token `-0` as integer zero.
///
/// Only a bare integer token is normalized; strings, fractional and exponent forms are
/// unchanged. Contracts requiring the sign of a floating-point zero must keep their own
/// raw-bit witness.
///
/// # Errors
/// Returns the parser's error for invalid JSON or duplicate keys.
pub fn parse_integer_zero_strict(bytes: &[u8]) -> Result<Value, serde_json::Error> {
    parse_strict(&normalize_negative_integer_zero(bytes))
}

fn normalize_negative_integer_zero(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(bytes.len());
    let mut inside = false;
    let mut escaped = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if inside {
            output.push(b);
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                inside = false;
            }
            i += 1;
            continue;
        }
        if b == b'"' {
            inside = true;
        }
        let before = i == 0
            || matches!(
                bytes[i - 1],
                b' ' | b'\n' | b'\r' | b'\t' | b':' | b'[' | b','
            );
        let after = i + 2 == bytes.len()
            || bytes
                .get(i + 2)
                .is_some_and(|b| matches!(b, b' ' | b'\n' | b'\r' | b'\t' | b',' | b']' | b'}'));
        if b == b'-' && bytes.get(i + 1) == Some(&b'0') && before && after {
            output.push(b'0');
            i += 2;
        } else {
            output.push(b);
            i += 1;
        }
    }
    output
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

    #[test]
    fn normalizes_only_integer_negative_zero_tokens() {
        let value = parse_integer_zero_strict(
            br#"{"integer":-0,"fraction":-0.0,"exponent":-0e0,"text":"-0"}"#,
        )
        .unwrap();
        assert_eq!(value["integer"].as_u64(), Some(0));
        assert_eq!(value["fraction"].as_u64(), None);
        assert_eq!(value["exponent"].as_u64(), None);
        assert_eq!(value["text"].as_str(), Some("-0"));
        assert!(parse_integer_zero_strict(br#"{"x":-0,"x":0}"#).is_err());
    }
}
