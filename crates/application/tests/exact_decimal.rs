#[path = "../src/exact_decimal.rs"]
mod exact_decimal;

use exact_decimal::DecimalOrder;
use std::cmp::Ordering::{Equal, Greater, Less};

#[test]
fn exact_decimal_order_distinguishes_source_values_hidden_by_binary64() {
    for (left, right, expected) in [
        ("-0", "+0.000e999", Equal),
        ("1.000e2", "100", Equal),
        ("5E+2", "500", Equal),
        ("-3.25", "-3.25001", Greater),
        ("-1e-99999999999999999999", "0", Less),
        ("1e-99999999999999999999", "2e-99999999999999999999", Less),
        ("100.000000000000000000000001", "100", Greater),
        ("99.999999999999999999999999", "100", Less),
        ("9e99999999999999999999", "1e100000000000000000000", Less),
    ] {
        let a = DecimalOrder::parse(left).unwrap();
        let b = DecimalOrder::parse(right).unwrap();
        assert_eq!(a.cmp(&b), expected, "{left}, {right}");
        assert_eq!(b.cmp(&a), expected.reverse(), "{right}, {left}");
    }
}

#[test]
fn malformed_or_unreasonably_long_exponents_are_not_silently_approximated() {
    for value in ["", ".", "1e", "1e+", "1..0", "--1", "1e2e3", "NaN", "1 2"] {
        assert!(DecimalOrder::parse(value).is_none(), "{value}");
    }
    let huge = format!("1e{}", "9".repeat(4097));
    assert!(DecimalOrder::parse(&huge).is_none());
}

#[test]
fn ordering_agrees_with_independent_integer_rational_vectors() {
    #[derive(serde::Deserialize)]
    struct Example {
        left: String,
        right: String,
        ordering: i8,
    }
    let cases: Vec<Example> =
        serde_json::from_str(include_str!("data/decimal-redesign/rational-order.json")).unwrap();
    assert_eq!(cases.len(), 300);
    for case in cases {
        let expected = match case.ordering {
            -1 => Less,
            0 => Equal,
            1 => Greater,
            _ => panic!("invalid fixture ordering"),
        };
        assert_eq!(
            DecimalOrder::parse(&case.left)
                .unwrap()
                .cmp(&DecimalOrder::parse(&case.right).unwrap()),
            expected
        );
    }
}
