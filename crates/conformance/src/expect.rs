//! Expectations: what a step's response (or error envelope) must contain.
//!
//! An expectation is plain JSON with one operator. Objects match as subsets (every expected
//! key must be present and match; other keys are ignored), arrays match with the same length
//! element by element, and `{"$contains": [..]}` matches an array holding a distinct element
//! for each listed pattern, in any order. Numbers compare by exact value: integers without
//! rounding, and a float equals an integer only when it has exactly that value.

use std::fmt;

use serde_json::{Map, Number, Value};

/// The one operator. Any other key starting with `$` is refused.
pub const CONTAINS: &str = "$contains";

/// Floats at or beyond this magnitude are refused in expectations: an integer literal outside
/// the 64-bit range would otherwise be parsed as a rounded float.
const MAX_EXACT_FLOAT: f64 = 9_007_199_254_740_992.0; // 2^53

/// Why an expectation was refused when a case was loaded. `path` is a JSON-pointer-like
/// location inside the expectation (`/document/atoms/0`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExpectationError {
    /// `expect` or `expect_error` is not a JSON object (or is an operator object).
    NotAnObject,
    /// `expect_error` does not name a string `code`.
    MissingCode,
    /// Nothing but `api_version` would be compared.
    NothingCompared,
    EmptyObject {
        path: String,
    },
    UnknownOperator {
        path: String,
        operator: String,
    },
    ContainsWithSiblings {
        path: String,
    },
    ContainsNotAList {
        path: String,
    },
    ContainsEmpty {
        path: String,
    },
    InexactNumber {
        path: String,
    },
}

impl fmt::Display for ExpectationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnObject => write!(formatter, "must be a JSON object"),
            Self::MissingCode => write!(formatter, "must name the error `code` as a string"),
            Self::NothingCompared => write!(
                formatter,
                "must compare at least one value other than api_version"
            ),
            Self::EmptyObject { path } => write!(formatter, "empty object at {path:?}"),
            Self::UnknownOperator { path, operator } => {
                write!(formatter, "unknown operator {operator:?} at {path:?}")
            }
            Self::ContainsWithSiblings { path } => {
                write!(formatter, "{CONTAINS} has sibling keys at {path:?}")
            }
            Self::ContainsNotAList { path } => {
                write!(formatter, "{CONTAINS} is not a list at {path:?}")
            }
            Self::ContainsEmpty { path } => write!(formatter, "{CONTAINS} is empty at {path:?}"),
            Self::InexactNumber { path } => write!(
                formatter,
                "number at {path:?} is a float of magnitude 2^53 or more; use an integer \
                 within the 64-bit range"
            ),
        }
    }
}

/// Validates a success expectation (`expect`).
///
/// # Errors
///
/// Refuses a non-object, an operator at the top, any invalid nested part (see
/// [`ExpectationError`]), and an expectation that compares nothing but `api_version`.
pub fn validate_expect(expected: &Value) -> Result<(), ExpectationError> {
    let Value::Object(fields) = expected else {
        return Err(ExpectationError::NotAnObject);
    };
    if fields.contains_key(CONTAINS) {
        return Err(ExpectationError::NotAnObject);
    }
    if compared_values(expected, "")? == 0 {
        return Err(ExpectationError::NothingCompared);
    }
    Ok(())
}

/// Validates an error expectation (`expect_error`): like [`validate_expect`], and it must name
/// the error `code` as a string.
///
/// # Errors
///
/// As [`validate_expect`], plus [`ExpectationError::MissingCode`].
pub fn validate_expect_error(expected: &Value) -> Result<(), ExpectationError> {
    validate_expect(expected)?;
    match expected.get("code") {
        Some(Value::String(_)) => Ok(()),
        _ => Err(ExpectationError::MissingCode),
    }
}

/// Checks the expectation's shape at every depth and counts the values it compares, leaving
/// out values under `api_version` keys.
fn compared_values(expected: &Value, path: &str) -> Result<usize, ExpectationError> {
    match expected {
        Value::Object(fields) => {
            let patterns = contains_patterns(fields, path)?;
            if let Some(patterns) = patterns {
                let mut count = 0;
                for (index, pattern) in patterns.iter().enumerate() {
                    count += compared_values(pattern, &format!("{path}/{CONTAINS}/{index}"))?;
                }
                return Ok(count);
            }
            let mut count = 0;
            for (key, value) in fields {
                let inner = compared_values(value, &format!("{path}/{key}"))?;
                if key != "api_version" {
                    count += inner;
                }
            }
            Ok(count)
        }
        Value::Array(items) => {
            // The length is compared too, so `[]` is a value.
            let mut count = 1;
            for (index, item) in items.iter().enumerate() {
                count += compared_values(item, &format!("{path}/{index}"))?;
            }
            Ok(count)
        }
        Value::Number(number) => {
            if number
                .as_f64()
                .is_some_and(|value| number.is_f64() && value.abs() >= MAX_EXACT_FLOAT)
            {
                return Err(ExpectationError::InexactNumber {
                    path: path.to_owned(),
                });
            }
            Ok(1)
        }
        Value::Null | Value::Bool(_) | Value::String(_) => Ok(1),
    }
}

/// Reads an expected object's operator, if it has one. Returns the `$contains` patterns, or
/// `None` for a plain subset object; refuses empty objects and malformed operators.
fn contains_patterns<'a>(
    fields: &'a Map<String, Value>,
    path: &str,
) -> Result<Option<&'a [Value]>, ExpectationError> {
    if fields.is_empty() {
        return Err(ExpectationError::EmptyObject {
            path: path.to_owned(),
        });
    }
    if let Some(operator) = fields
        .keys()
        .find(|key| key.starts_with('$') && key.as_str() != CONTAINS)
    {
        return Err(ExpectationError::UnknownOperator {
            path: path.to_owned(),
            operator: operator.clone(),
        });
    }
    let Some(patterns) = fields.get(CONTAINS) else {
        return Ok(None);
    };
    if fields.len() > 1 {
        return Err(ExpectationError::ContainsWithSiblings {
            path: path.to_owned(),
        });
    }
    match patterns {
        Value::Array(patterns) if patterns.is_empty() => Err(ExpectationError::ContainsEmpty {
            path: path.to_owned(),
        }),
        Value::Array(patterns) => Ok(Some(patterns)),
        _ => Err(ExpectationError::ContainsNotAList {
            path: path.to_owned(),
        }),
    }
}

/// Why a response did not match.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mismatch {
    pub path: String,
    pub message: String,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(formatter, "{}", self.message)
        } else {
            write!(formatter, "at {:?}: {}", self.path, self.message)
        }
    }
}

/// Matches `actual` against `expected`. A malformed expectation never matches, so a check
/// skipped at load time cannot turn into a silent pass here.
///
/// # Errors
///
/// Returns the first mismatch found.
pub fn matches(expected: &Value, actual: &Value) -> Result<(), Mismatch> {
    match_at(expected, actual, "")
}

fn match_at(expected: &Value, actual: &Value, path: &str) -> Result<(), Mismatch> {
    let mismatch = |message: String| Mismatch {
        path: path.to_owned(),
        message,
    };
    match expected {
        Value::Object(fields) => {
            let patterns = contains_patterns(fields, path)
                .map_err(|error| mismatch(format!("invalid expectation: {error}")))?;
            if let Some(patterns) = patterns {
                let Value::Array(items) = actual else {
                    return Err(mismatch(format!("expected an array, found {actual}")));
                };
                return contains_distinct(patterns, items, path);
            }
            let Value::Object(actual_fields) = actual else {
                return Err(mismatch(format!("expected an object, found {actual}")));
            };
            for (key, value) in fields {
                let inner = format!("{path}/{key}");
                let Some(found) = actual_fields.get(key) else {
                    return Err(Mismatch {
                        path: inner,
                        message: "missing".to_owned(),
                    });
                };
                match_at(value, found, &inner)?;
            }
            Ok(())
        }
        Value::Array(items) => {
            let Value::Array(actual_items) = actual else {
                return Err(mismatch(format!("expected an array, found {actual}")));
            };
            if items.len() != actual_items.len() {
                return Err(mismatch(format!(
                    "expected {} elements, found {}",
                    items.len(),
                    actual_items.len()
                )));
            }
            for (index, (item, found)) in items.iter().zip(actual_items).enumerate() {
                match_at(item, found, &format!("{path}/{index}"))?;
            }
            Ok(())
        }
        Value::Number(number) => match actual {
            Value::Number(found) if numbers_equal(number, found) => Ok(()),
            _ => Err(mismatch(format!("expected {number}, found {actual}"))),
        },
        Value::Null | Value::Bool(_) | Value::String(_) => {
            if expected == actual {
                Ok(())
            } else {
                Err(mismatch(format!("expected {expected}, found {actual}")))
            }
        }
    }
}

/// Each pattern must match its own element: a maximum bipartite matching between patterns and
/// elements, so an element matched by one pattern is never reused for another.
fn contains_distinct(patterns: &[Value], items: &[Value], path: &str) -> Result<(), Mismatch> {
    let fits: Vec<Vec<bool>> = patterns
        .iter()
        .enumerate()
        .map(|(index, pattern)| {
            let inner = format!("{path}/{CONTAINS}/{index}");
            items
                .iter()
                .map(|item| match_at(pattern, item, &inner).is_ok())
                .collect()
        })
        .collect();
    // owner[j] is the pattern currently holding element j.
    let mut owner: Vec<Option<usize>> = vec![None; items.len()];
    for (index, pattern) in patterns.iter().enumerate() {
        let mut visited = vec![false; items.len()];
        if !assign(index, &fits, &mut owner, &mut visited) {
            return Err(Mismatch {
                path: format!("{path}/{CONTAINS}/{index}"),
                message: format!(
                    "no distinct element matches {pattern} (array of {})",
                    items.len()
                ),
            });
        }
    }
    Ok(())
}

/// One augmenting-path step of Kuhn's algorithm.
fn assign(
    pattern: usize,
    fits: &[Vec<bool>],
    owner: &mut [Option<usize>],
    visited: &mut [bool],
) -> bool {
    for item in 0..owner.len() {
        if !fits[pattern][item] || visited[item] {
            continue;
        }
        visited[item] = true;
        let free = match owner[item] {
            None => true,
            Some(holder) => assign(holder, fits, owner, visited),
        };
        if free {
            owner[item] = Some(pattern);
            return true;
        }
    }
    false
}

fn numbers_equal(expected: &Number, actual: &Number) -> bool {
    match (integer(expected), integer(actual)) {
        (Some(left), Some(right)) => left == right,
        (Some(whole), None) => float_is_integer(actual.as_f64(), whole),
        (None, Some(whole)) => float_is_integer(expected.as_f64(), whole),
        (None, None) => expected.as_f64() == actual.as_f64(),
    }
}

fn integer(number: &Number) -> Option<i128> {
    number
        .as_i64()
        .map(i128::from)
        .or_else(|| number.as_u64().map(i128::from))
}

/// True when `value` is exactly the integer `whole`.
fn float_is_integer(value: Option<f64>, whole: i128) -> bool {
    // 2^126: every integral float below this converts to i128 exactly.
    const LIMIT: f64 = 85_070_591_730_234_615_865_843_651_857_942_052_864.0;
    value.is_some_and(|value| {
        value.is_finite() && value.fract() == 0.0 && value.abs() < LIMIT && value as i128 == whole
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn objects_match_as_subsets_and_report_missing_keys() {
        let actual = json!({"a": 1, "b": {"c": "x", "d": null}});
        assert_eq!(matches(&json!({"b": {"c": "x"}}), &actual), Ok(()));
        assert_eq!(matches(&json!({"b": {"d": null}}), &actual), Ok(()));
        let error = matches(&json!({"b": {"e": 1}}), &actual).unwrap_err();
        assert_eq!(error.path, "/b/e");
    }

    #[test]
    fn arrays_need_the_same_length() {
        let actual = json!([1, 2]);
        assert_eq!(matches(&json!([1, 2]), &actual), Ok(()));
        assert!(
            matches(&json!([1]), &actual).is_err(),
            "shorter must not match"
        );
        assert!(matches(&json!([]), &actual).is_err(), "[] must not match");
        assert!(matches(&json!([1, 2, 3]), &actual).is_err());
    }

    #[test]
    fn contains_uses_distinct_elements() {
        let actual = json!([{"e": "C", "q": 1}, {"e": "C", "q": 0}]);
        // A greedy match would give the first element to the general pattern and fail.
        let expected = json!({"$contains": [{"e": "C"}, {"e": "C", "q": 1}]});
        assert_eq!(matches(&expected, &actual), Ok(()));
        let twice = json!({"$contains": [{"q": 1}, {"q": 1}]});
        assert!(
            matches(&twice, &actual).is_err(),
            "one element must not satisfy two patterns"
        );
    }

    #[test]
    fn integers_compare_exactly_above_2_pow_53() {
        let big = 9_007_199_254_740_993_u64; // 2^53 + 1
        assert!(matches(&json!(big - 1), &json!(big)).is_err());
        assert_eq!(matches(&json!(big), &json!(big)), Ok(()));
        assert!(matches(&json!(u64::MAX - 1), &json!(u64::MAX)).is_err());
        assert!(matches(&json!(-1), &json!(u64::MAX)).is_err());
    }

    #[test]
    fn floats_equal_integers_only_by_exact_value() {
        assert_eq!(matches(&json!(2), &json!(2.0)), Ok(()));
        assert_eq!(matches(&json!(2.0), &json!(2)), Ok(()));
        assert!(matches(&json!(2), &json!(2.5)).is_err());
        assert!(matches(&json!(0.1), &json!(0.1 + 0.2 - 0.2)).is_err());
        let big = 9_007_199_254_740_993_u64;
        // The float nearest 2^53 + 1 is 2^53: it is not that integer.
        assert!(matches(&json!(big), &json!(9_007_199_254_740_992.0)).is_err());
    }

    #[test]
    fn malformed_expectations_never_match() {
        let actual = json!({"a": [1], "b": 2});
        for expected in [
            json!({"a": {}}),
            json!({"a": {"$contains": []}}),
            json!({"a": {"$contains": 1}}),
            json!({"a": {"$contains": [1], "x": 1}}),
            json!({"a": {"$any": [1]}}),
        ] {
            assert!(
                matches(&expected, &actual).is_err(),
                "{expected} must not match"
            );
        }
    }

    #[test]
    fn validation_refuses_empty_objects_at_any_depth() {
        let nested = json!({"a": [{"b": {}}]});
        assert_eq!(
            validate_expect(&nested),
            Err(ExpectationError::EmptyObject {
                path: "/a/0/b".to_owned()
            })
        );
        assert_eq!(
            validate_expect(&json!({"a": {"$contains": [{}]}})),
            Err(ExpectationError::EmptyObject {
                path: "/a/$contains/0".to_owned()
            })
        );
    }

    #[test]
    fn validation_refuses_malformed_contains() {
        assert_eq!(
            validate_expect(&json!({"a": [{"$contains": []}]})),
            Err(ExpectationError::ContainsEmpty {
                path: "/a/0".to_owned()
            })
        );
        assert_eq!(
            validate_expect(&json!({"a": {"$contains": {"x": 1}}})),
            Err(ExpectationError::ContainsNotAList {
                path: "/a".to_owned()
            })
        );
        assert_eq!(
            validate_expect(&json!({"a": {"$contains": [1], "b": 2}})),
            Err(ExpectationError::ContainsWithSiblings {
                path: "/a".to_owned()
            })
        );
    }

    #[test]
    fn validation_refuses_unknown_operators() {
        assert_eq!(
            validate_expect(&json!({"a": {"b": {"$size": 2}}})),
            Err(ExpectationError::UnknownOperator {
                path: "/a/b".to_owned(),
                operator: "$size".to_owned()
            })
        );
    }

    #[test]
    fn validation_needs_a_value_other_than_api_version() {
        assert_eq!(
            validate_expect(&json!({"api_version": "v1"})),
            Err(ExpectationError::NothingCompared)
        );
        assert_eq!(
            validate_expect(&json!({"document": {"api_version": "v1"}})),
            Err(ExpectationError::NothingCompared)
        );
        assert_eq!(validate_expect(&json!({"recovery": null})), Ok(()));
        assert_eq!(validate_expect(&json!({"document": {"atoms": []}})), Ok(()));
    }

    #[test]
    fn validation_needs_an_object() {
        assert_eq!(
            validate_expect(&json!([1])),
            Err(ExpectationError::NotAnObject)
        );
        assert_eq!(
            validate_expect(&json!({"$contains": [1]})),
            Err(ExpectationError::NotAnObject)
        );
    }

    #[test]
    fn error_expectations_name_a_string_code() {
        assert_eq!(
            validate_expect_error(&json!({"command_index": 1})),
            Err(ExpectationError::MissingCode)
        );
        assert_eq!(
            validate_expect_error(&json!({"code": 409})),
            Err(ExpectationError::MissingCode)
        );
        assert_eq!(validate_expect_error(&json!({"code": "x"})), Ok(()));
    }

    #[test]
    fn validation_refuses_floats_that_may_have_been_rounded() {
        assert_eq!(
            validate_expect(&json!({"n": 1.8446744073709552e19})),
            Err(ExpectationError::InexactNumber {
                path: "/n".to_owned()
            })
        );
        assert_eq!(validate_expect(&json!({"n": u64::MAX})), Ok(()));
    }
}
