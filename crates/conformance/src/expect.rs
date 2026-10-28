//! Expectations: subsets of v1 JSON replies.
//!
//! An expected object matches when every key it names is present in the reply and matches;
//! keys it leaves out are not compared. An expected array matches an array of the same length,
//! element by element. `{"$contains": [..]}` matches an array holding a distinct element for
//! each listed item, in any order; it is the only operator, takes a list, and stands alone in
//! its object. Integers compare exactly; a float equals an integer only when it has exactly
//! that value.

use std::fmt;

use serde_json::{Map, Number, Value};

/// The unordered-contains operator.
pub const CONTAINS: &str = "$contains";

/// Why an expectation is malformed or does not match, at a JSON path such as
/// `$.document.atoms[1].id`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mismatch {
    pub path: String,
    pub message: String,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path, self.message)
    }
}

/// Checks that `expected` uses operators correctly and checks something, so a malformed or
/// vacuous expectation is refused when a case loads instead of failing (or passing) only once
/// it runs.
///
/// # Errors
///
/// Returns the first misuse: an unknown `$` key, `$contains` beside other keys, a `$contains`
/// value that is not a list or is empty, an empty object at any depth, or an expectation
/// whose only values (leaves: strings, numbers, booleans, `null`, or `[]`) are `api_version`.
pub fn validate(expected: &Value) -> Result<(), Mismatch> {
    validate_at(expected, "$")?;
    if checked_values(expected, None) == 0 {
        return Err(Mismatch {
            path: "$".to_owned(),
            message: "an expectation names at least one value besides api_version".to_owned(),
        });
    }
    Ok(())
}

/// The leaves `expected` compares, not counting those under an `api_version` key, which every
/// v1 reply carries. `key` is the object key `expected` sits under, if any.
fn checked_values(expected: &Value, key: Option<&str>) -> usize {
    match expected {
        Value::Object(object) => match object.get(CONTAINS) {
            Some(Value::Array(items)) => items.iter().map(|item| checked_values(item, None)).sum(),
            _ => object
                .iter()
                .map(|(key, value)| checked_values(value, Some(key)))
                .sum(),
        },
        Value::Array(items) if !items.is_empty() => {
            items.iter().map(|item| checked_values(item, None)).sum()
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => {
            usize::from(key != Some("api_version"))
        }
    }
}

fn validate_at(expected: &Value, path: &str) -> Result<(), Mismatch> {
    match expected {
        Value::Object(object) => {
            let vacuous = |message: &str| Mismatch {
                path: path.to_owned(),
                message: message.to_owned(),
            };
            if let Some(items) = operator(object, path)? {
                if items.is_empty() {
                    return Err(vacuous("$contains needs at least one item"));
                }
                for (index, item) in items.iter().enumerate() {
                    validate_at(item, &format!("{path}.{CONTAINS}[{index}]"))?;
                }
                return Ok(());
            }
            if object.is_empty() {
                return Err(vacuous("an empty object checks nothing"));
            }
            for (key, value) in object {
                validate_at(value, &format!("{path}.{key}"))?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                validate_at(item, &format!("{path}[{index}]"))?;
            }
            Ok(())
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
    }
}

/// The items of a `$contains` object, `None` for a plain object, or the misuse.
fn operator<'a>(
    object: &'a Map<String, Value>,
    path: &str,
) -> Result<Option<&'a [Value]>, Mismatch> {
    let misuse = |message: String| Mismatch {
        path: path.to_owned(),
        message,
    };
    if let Some(key) = object
        .keys()
        .find(|key| key.starts_with('$') && key.as_str() != CONTAINS)
    {
        return Err(misuse(format!("unknown operator {key:?}")));
    }
    let Some(items) = object.get(CONTAINS) else {
        return Ok(None);
    };
    if object.len() != 1 {
        return Err(misuse(format!("{CONTAINS} cannot have sibling keys")));
    }
    match items {
        Value::Array(items) => Ok(Some(items)),
        other => Err(misuse(format!(
            "{CONTAINS} needs a list, got {}",
            kind(other)
        ))),
    }
}

/// Matches `actual` against `expected`.
///
/// # Errors
///
/// Returns the first difference, or a malformed operator met while matching.
pub fn matches(expected: &Value, actual: &Value) -> Result<(), Mismatch> {
    matches_at(expected, actual, "$")
}

fn matches_at(expected: &Value, actual: &Value, path: &str) -> Result<(), Mismatch> {
    let differ = |message: String| {
        Err(Mismatch {
            path: path.to_owned(),
            message,
        })
    };
    match (expected, actual) {
        (Value::Object(expected), _) => {
            if let Some(items) = operator(expected, path)? {
                let Value::Array(actual) = actual else {
                    return differ(format!("expected an array, got {}", kind(actual)));
                };
                return contains(items, actual, path);
            }
            let Value::Object(actual) = actual else {
                return differ(format!("expected an object, got {}", kind(actual)));
            };
            for (key, value) in expected {
                let child = format!("{path}.{key}");
                match actual.get(key) {
                    Some(found) => matches_at(value, found, &child)?,
                    None => {
                        return Err(Mismatch {
                            path: child,
                            message: "missing from the reply".to_owned(),
                        });
                    }
                }
            }
            Ok(())
        }
        (Value::Array(expected), Value::Array(actual)) => {
            if expected.len() != actual.len() {
                return differ(format!(
                    "expected {} elements, got {}",
                    expected.len(),
                    actual.len()
                ));
            }
            for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
                matches_at(expected, actual, &format!("{path}[{index}]"))?;
            }
            Ok(())
        }
        (Value::Number(expected), Value::Number(actual)) => {
            if numbers_equal(expected, actual) {
                Ok(())
            } else {
                differ(format!("expected {expected}, got {actual}"))
            }
        }
        (Value::Null, Value::Null) => Ok(()),
        (Value::Bool(expected), Value::Bool(actual)) if expected == actual => Ok(()),
        (Value::String(expected), Value::String(actual)) if expected == actual => Ok(()),
        _ => differ(format!("expected {expected}, got {actual}")),
    }
}

/// Each expected item needs its own matching element. Items can match overlapping sets of
/// elements, so this finds a full assignment (augmenting paths) rather than taking the first
/// element each item matches.
fn contains(items: &[Value], actual: &[Value], path: &str) -> Result<(), Mismatch> {
    let candidates: Vec<Vec<usize>> = items
        .iter()
        .map(|item| {
            (0..actual.len())
                .filter(|&index| matches_at(item, &actual[index], path).is_ok())
                .collect()
        })
        .collect();
    let mut owner: Vec<Option<usize>> = vec![None; actual.len()];
    for (index, item) in items.iter().enumerate() {
        let mut visited = vec![false; actual.len()];
        if !assign(index, &candidates, &mut owner, &mut visited) {
            return Err(Mismatch {
                path: format!("{path}.{CONTAINS}[{index}]"),
                message: format!("no distinct element of the reply matches {item}"),
            });
        }
    }
    Ok(())
}

fn assign(
    item: usize,
    candidates: &[Vec<usize>],
    owner: &mut [Option<usize>],
    visited: &mut [bool],
) -> bool {
    for &element in &candidates[item] {
        if visited[element] {
            continue;
        }
        visited[element] = true;
        let free = match owner[element] {
            None => true,
            Some(other) => assign(other, candidates, owner, visited),
        };
        if free {
            owner[element] = Some(item);
            return true;
        }
    }
    false
}

/// Integers compare exactly (as `i128`, so values above 2^53 never round). A float equals an
/// integer only when it is integral and has exactly that value; two floats compare as `f64`.
fn numbers_equal(expected: &Number, actual: &Number) -> bool {
    match (integer(expected), integer(actual)) {
        (Some(expected), Some(actual)) => expected == actual,
        (Some(integer), None) => actual
            .as_f64()
            .is_some_and(|float| float_equals_integer(float, integer)),
        (None, Some(integer)) => expected
            .as_f64()
            .is_some_and(|float| float_equals_integer(float, integer)),
        (None, None) => expected.as_f64() == actual.as_f64(),
    }
}

fn integer(number: &Number) -> Option<i128> {
    number
        .as_u64()
        .map(i128::from)
        .or_else(|| number.as_i64().map(i128::from))
}

/// 2^127, the first float magnitude outside `i128`.
const I128_LIMIT: f64 = 170_141_183_460_469_231_731_687_303_715_884_105_728.0;

fn float_equals_integer(float: f64, integer: i128) -> bool {
    // Within range an integral float converts to `i128` exactly.
    float.is_finite()
        && float.fract() == 0.0
        && float.abs() < I128_LIMIT
        && float as i128 == integer
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn mismatch(expected: Value, actual: Value) -> Mismatch {
        matches(&expected, &actual).expect_err(&format!("{expected} should not match {actual}"))
    }

    #[test]
    fn an_object_matches_on_the_keys_it_names() {
        let reply = json!({ "document": { "revision": 2, "title": "T" }, "recovery": null });
        assert_eq!(
            matches(&json!({ "document": { "revision": 2 } }), &reply),
            Ok(())
        );
        assert_eq!(matches(&json!({}), &reply), Ok(()));
    }

    #[test]
    fn a_named_key_must_be_present_even_when_null_is_expected() {
        let found = mismatch(json!({ "reason": null }), json!({ "code": "x" }));
        assert_eq!(found.path, "$.reason");
        assert_eq!(found.message, "missing from the reply");
    }

    #[test]
    fn arrays_match_element_by_element_with_the_same_length() {
        let reply = json!([{ "id": "a1", "x": 0 }, { "id": "a2", "x": 1 }]);
        assert_eq!(
            matches(&json!([{ "id": "a1" }, { "id": "a2" }]), &reply),
            Ok(())
        );
        assert_eq!(
            mismatch(json!([{ "id": "a1" }]), reply.clone()).message,
            "expected 1 elements, got 2"
        );
        assert_eq!(
            mismatch(json!([{ "id": "a2" }, { "id": "a1" }]), reply).path,
            "$[0].id"
        );
    }

    #[test]
    fn contains_matches_in_any_order_without_fixing_the_length() {
        let reply = json!({ "list": [{ "id": "a" }, { "id": "b" }, { "id": "c" }] });
        let expected = json!({ "list": { "$contains": [{ "id": "c" }, { "id": "a" }] } });
        assert_eq!(matches(&expected, &reply), Ok(()));
        let missing = json!({ "list": { "$contains": [{ "id": "d" }] } });
        assert_eq!(mismatch(missing, reply).path, "$.list.$contains[0]");
    }

    #[test]
    fn contains_needs_a_distinct_element_per_item() {
        let reply = json!([{ "id": "a" }]);
        let twice = json!({ "$contains": [{ "id": "a" }, { "id": "a" }] });
        assert_eq!(mismatch(twice, reply).path, "$.$contains[1]");
    }

    #[test]
    fn contains_finds_an_assignment_when_the_first_choice_blocks_one() {
        // `{}` matches both elements. A greedy search gives it `a`, the first, and then finds
        // nothing left for `{"id": "a"}`.
        let reply = json!([{ "id": "a" }, { "id": "b" }]);
        let expected = json!({ "$contains": [{}, { "id": "a" }] });
        assert_eq!(matches(&expected, &reply), Ok(()));
    }

    #[test]
    fn contains_with_sibling_keys_is_refused_when_loading_and_when_matching() {
        let expected = json!({ "$contains": [1], "extra": true });
        let refused = validate(&expected).expect_err("siblings must be refused");
        assert_eq!(refused.message, "$contains cannot have sibling keys");
        assert_eq!(mismatch(expected, json!([1])).message, refused.message);
    }

    #[test]
    fn contains_needs_a_list_when_loading_and_when_matching() {
        let expected = json!({ "items": { "$contains": { "id": "a" } } });
        let refused = validate(&expected).expect_err("a non-list must be refused");
        assert_eq!(refused.path, "$.items");
        assert_eq!(refused.message, "$contains needs a list, got an object");
        let found = mismatch(expected, json!({ "items": [{ "id": "a" }] }));
        assert_eq!(found, refused);
    }

    #[test]
    fn operator_misuse_nested_in_a_list_is_refused() {
        let expected = json!([{ "$contains": 1 }]);
        assert_eq!(validate(&expected).expect_err("nested misuse").path, "$[0]");
        let nested = json!({ "$contains": [{ "$contains": "x" }] });
        assert_eq!(
            validate(&nested).expect_err("misuse inside contains").path,
            "$.$contains[0]"
        );
    }

    #[test]
    fn empty_objects_and_contains_lists_are_refused_at_any_depth() {
        let refused = |expected: Value| validate(&expected).expect_err(&expected.to_string());
        assert_eq!(refused(json!({})).message, "an empty object checks nothing");
        assert_eq!(refused(json!({ "a": [{ "b": {} }] })).path, "$.a[0].b");
        assert_eq!(
            refused(json!({ "a": { "$contains": [] } })).message,
            "$contains needs at least one item"
        );
    }

    #[test]
    fn an_expectation_compares_a_value_besides_api_version() {
        let only_version = validate(&json!({ "api_version": "v1" })).expect_err("vacuous");
        assert_eq!(
            only_version.message,
            "an expectation names at least one value besides api_version"
        );
        // null and [] are values: they assert absence and emptiness.
        for expected in [
            json!({ "api_version": "v1", "recovery": null }),
            json!({ "document": { "bonds": [] } }),
            json!({ "capabilities": { "$contains": [{ "id": "x" }] } }),
            json!({ "code": "revision_conflict" }),
        ] {
            assert_eq!(validate(&expected), Ok(()), "{expected}");
        }
    }

    #[test]
    fn unknown_operators_are_refused() {
        let expected = json!({ "$any": [1] });
        assert_eq!(
            validate(&expected).expect_err("unknown operator").message,
            "unknown operator \"$any\""
        );
        assert_eq!(
            mismatch(expected, json!([1])).message,
            "unknown operator \"$any\""
        );
    }

    #[test]
    fn integers_above_two_to_the_53_compare_exactly() {
        let above: Value = serde_json::from_str("9007199254740993").unwrap();
        let below: Value = serde_json::from_str("9007199254740992").unwrap();
        assert_eq!(matches(&above, &above), Ok(()));
        assert!(
            matches(&above, &below).is_err(),
            "2^53 + 1 must differ from 2^53"
        );
        let negative: Value = serde_json::from_str("-9007199254740993").unwrap();
        let negative_below: Value = serde_json::from_str("-9007199254740992").unwrap();
        assert!(matches(&negative, &negative_below).is_err());
        assert_eq!(matches(&json!(u64::MAX), &json!(u64::MAX)), Ok(()));
        assert!(matches(&json!(u64::MAX), &json!(u64::MAX - 1)).is_err());
    }

    #[test]
    fn a_float_equals_an_integer_only_with_exactly_its_value() {
        assert_eq!(matches(&json!(15), &json!(15.0)), Ok(()));
        assert_eq!(matches(&json!(-100000.0), &json!(-100000)), Ok(()));
        assert!(matches(&json!(2.5), &json!(2)).is_err());
        // 2^53 + 1 has no f64; comparing through f64 would call these equal.
        let above: Value = serde_json::from_str("9007199254740993").unwrap();
        assert!(matches(&json!(9_007_199_254_740_992.0), &above).is_err());
        assert!(matches(&above, &json!(9_007_199_254_740_992.0)).is_err());
        assert_eq!(matches(&json!(226.4), &json!(226.4)), Ok(()));
        assert!(matches(&json!(226.4), &json!(226.399_999_999_999_98)).is_err());
    }

    #[test]
    fn values_of_different_types_never_match() {
        assert!(matches(&json!(null), &json!(0)).is_err());
        assert!(matches(&json!("1"), &json!(1)).is_err());
        assert!(matches(&json!(false), &json!(null)).is_err());
        assert!(matches(&json!({}), &json!([])).is_err());
        assert!(matches(&json!([]), &json!({})).is_err());
    }
}
