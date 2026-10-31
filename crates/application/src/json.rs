//! JSON parsing that refuses duplicate object keys, which plain parsing would resolve
//! silently by keeping the last one: an operation request could then mean one thing to a
//! reader and another to the service, and a conformance case could drop an expectation
//! unnoticed.

use std::fmt;
use std::str::FromStr;

use num_bigint::BigUint;
use serde::Deserializer;
use serde::de::{self, Deserialize, MapAccess, SeqAccess, Visitor};
use serde_json::value::RawValue;
use serde_json::{Map, Number, Value};

/// Integer types admitted by the prepared DATA readers.
pub(crate) trait IntegerToken: FromStr {
    const MAX_DIGITS: usize;
}

impl IntegerToken for u8 {
    const MAX_DIGITS: usize = 3;
}
impl IntegerToken for u32 {
    const MAX_DIGITS: usize = 10;
}
impl IntegerToken for u64 {
    const MAX_DIGITS: usize = 20;
}
impl IntegerToken for i32 {
    const MAX_DIGITS: usize = 10;
}
impl IntegerToken for i64 {
    const MAX_DIGITS: usize = 19;
}
impl IntegerToken for BigUint {
    const MAX_DIGITS: usize = 4096;
}

/// Reads an already validated raw JSON token as an exact integer value.
pub(crate) fn parse_integer_raw<T: IntegerToken>(raw: &RawValue) -> Option<T> {
    let token = if raw.get() == "-0" { "0" } else { raw.get() };
    if token.trim_start_matches('-').len() <= T::MAX_DIGITS
        && let Ok(value) = token.parse()
    {
        return Some(value);
    }
    crate::decimal_input::DecimalInput::from_token(token)
        .and_then(|value| value.integer_token(T::MAX_DIGITS))
        .and_then(|value| value.parse().ok())
}

/// Reads a JSON integer token without changing adjacent raw numeric witnesses.
pub(crate) fn deserialize_integer<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: IntegerToken,
{
    let raw = Box::<RawValue>::deserialize(deserializer)?;
    parse_integer_raw(&raw)
        .ok_or_else(|| de::Error::custom("expected an exact integer value in range"))
}

/// Reads a present integer, preserving the distinction between null and absent.
pub(crate) fn deserialize_optional_integer<'de, D, T>(
    deserializer: D,
) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: IntegerToken,
{
    deserialize_integer(deserializer).map(Some)
}

/// Reads a nullable integer while requiring its field to be present.
pub(crate) fn deserialize_nullable_integer<'de, D, T>(
    deserializer: D,
) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: IntegerToken,
{
    Option::<Integer<T>>::deserialize(deserializer).map(|value| value.map(|value| value.0))
}

struct Integer<T>(T);

impl<'de, T: IntegerToken> Deserialize<'de> for Integer<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_integer(deserializer).map(Self)
    }
}

/// Reads an array of integer tokens without narrowing adjacent raw values.
pub(crate) fn deserialize_integer_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: IntegerToken,
{
    Vec::<Integer<T>>::deserialize(deserializer)
        .map(|values| values.into_iter().map(|value| value.0).collect())
}

/// Parses `bytes` as JSON, refusing duplicate keys at any depth.
///
/// # Errors
///
/// Returns the parser's error, which names the position of invalid JSON or a duplicate key.
pub fn parse_strict(bytes: &[u8]) -> Result<Value, serde_json::Error> {
    serde_json::from_slice::<Strict>(bytes).map(|Strict(value)| value)
}

/// Checks original JSON syntax and duplicate keys without converting its numeric values.
/// The normalized copy is used only for the duplicate-key check; consumers decode the
/// original bytes so that integers, floating witnesses and opaque values retain their input.
pub(crate) fn validate_raw_strict(bytes: &[u8]) -> Result<(), serde_json::Error> {
    let _: &RawValue = serde_json::from_slice(bytes)?;
    parse_strict(&normalize_json_numbers(bytes)).map(|_| ())
}

// The original input has already passed the JSON grammar check, so outside strings every
// number start and its characters can be replaced without interpreting its numeric value.
fn normalize_json_numbers(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(bytes.len());
    let mut inside = false;
    let mut escaped = false;
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        if inside {
            output.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                inside = false;
            }
            index += 1;
        } else if byte == b'"' {
            output.push(byte);
            inside = true;
            index += 1;
        } else if byte == b'-' || byte.is_ascii_digit() {
            output.push(b'0');
            index += 1;
            while bytes.get(index).is_some_and(|next| {
                next.is_ascii_digit() || matches!(*next, b'.' | b'e' | b'E' | b'+' | b'-')
            }) {
                index += 1;
            }
        } else {
            output.push(byte);
            index += 1;
        }
    }
    output
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
    fn raw_strict_validation_preserves_large_numbers_and_checks_original_syntax() {
        let large = "9".repeat(400);
        let valid = format!(
            r#"{{"number":{large},"nested":[-1.2e-300,{{"text":"quote: \\\"-5e+3\\\""}}]}}"#
        );
        assert!(parse_strict(valid.as_bytes()).is_err());
        assert!(validate_raw_strict(valid.as_bytes()).is_ok());
        assert!(validate_raw_strict(br#"{"123":1,"0":2,"text":"-7e+12"}"#).is_ok());

        let duplicate = format!(r#"{{"number":{large},"items":[{{"key":0,"\u006bey":1}}]}}"#);
        assert!(validate_raw_strict(duplicate.as_bytes()).is_err());
        for malformed in [
            &br#"{"n":1e+}"#[..],
            &br#"{"n":+1}"#[..],
            &br#"{"n":0,}"#[..],
            &br#"{"s":"unfinished}"#[..],
        ] {
            assert!(validate_raw_strict(malformed).is_err());
        }
    }

    #[test]
    fn oversized_integer_is_rejected_before_direct_integer_parsing() {
        struct Limited;
        impl FromStr for Limited {
            type Err = ();

            fn from_str(token: &str) -> Result<Self, Self::Err> {
                assert!(
                    token.len() <= 2,
                    "oversized token reached the integer parser"
                );
                Ok(Self)
            }
        }
        impl IntegerToken for Limited {
            const MAX_DIGITS: usize = 2;
        }
        let raw = RawValue::from_string("123".into()).unwrap();
        assert!(parse_integer_raw::<Limited>(&raw).is_none());
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
