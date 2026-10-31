use chemifree_application::phase6_profiles::{FileKind, Records, decode, decode_u64};

#[test]
fn scalar_u64_uses_the_exact_mathematical_integer() {
    for (token, expected) in [
        (&b"1.0"[..], 1),
        (&b"1e0"[..], 1),
        (&b"-0.0"[..], 0),
        (&b"0e99999999999999999999"[..], 0),
        (&b"18446744073709551615.0"[..], u64::MAX),
    ] {
        assert_eq!(decode_u64(token), Ok(expected));
    }
    for token in [
        &b"1.00000000000000000001"[..],
        &b"1e-9999"[..],
        &b"18446744073709551614.999999999999999999"[..],
        &b"18446744073709551616.0"[..],
        &b"-1.0"[..],
        &b"true"[..],
        &b"\"1\""[..],
    ] {
        assert!(decode_u64(token).is_err());
    }
}

#[test]
fn excessive_numeric_token_components_are_refused_without_approximation() {
    let within = format!("0e{}", "9".repeat(4096));
    assert_eq!(decode_u64(within.as_bytes()), Ok(0));
    let excessive = format!("0e{}", "9".repeat(4097));
    assert!(decode_u64(excessive.as_bytes()).is_err());
}

#[test]
fn symbol_rows_read_integral_decimal_without_binary64_coercion() {
    let Records::Symbols(rows) = decode(
        FileKind::ElementSymbols,
        b"{\"atomic_number_u32\":6.0,\"symbol_hex\":\"43\"}\n",
    )
    .unwrap() else {
        panic!("wrong record type")
    };
    assert_eq!(
        (rows[0].atomic_number, rows[0].symbol_bytes.as_slice()),
        (6, &b"C"[..])
    );
    assert!(
        decode(
            FileKind::ElementSymbols,
            b"{\"atomic_number_u32\":6.00000000000000000001,\"symbol_hex\":\"43\"}\n",
        )
        .is_err()
    );
}

#[test]
fn type_and_rank_rows_use_integral_exponents_and_keep_phase6_framing() {
    let Records::Types(rows) = decode(
        FileKind::TypeElements,
        b"{\"internal_type_u32\":4294967295.0,\"atomic_number_u32\":6e0}\n",
    )
    .unwrap() else {
        panic!("wrong record type")
    };
    assert_eq!(
        (rows[0].internal_type, rows[0].atomic_number),
        (u32::MAX, 6)
    );
    assert!(
        decode(
            FileKind::TypeElements,
            b"{\"internal_type_u32\":4294967296.0,\"atomic_number_u32\":6}\n",
        )
        .is_err()
    );
    let rank = b"{\"atomic_number_u32\":6.0,\"source_rank_u32\":1e0}\n";
    for kind in [FileKind::ElementOrderA, FileKind::ElementOrderB] {
        assert!(decode(kind, rank).is_ok());
        assert!(decode(kind, &rank[..rank.len() - 1]).is_err());
        assert!(
            decode(
                kind,
                b"{\"atomic_number_u32\":6,\"source_rank_u32\":1.000000000000000001}\n"
            )
            .is_err()
        );
        assert!(
            decode(
                kind,
                b"{\"atomic_number_u32\":6,\"atomic_number_u32\":6,\"source_rank_u32\":1}\n"
            )
            .is_err()
        );
    }
}
